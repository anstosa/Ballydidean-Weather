import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import test from "node:test";

import { runMigrations } from "../../packages/database/dist/index.js";
import {
  createRuntimeRoles,
  createTestPool,
  startPostgres,
  stopPostgres,
} from "../../packages/database/test/postgres-harness.mjs";

const executeFile = promisify(execFile);
const root = resolve(import.meta.dirname, "../..");
const migrationRoot = join(root, "packages/database/migrations");
const catalogChecksum =
  "8d04170868414cd17e78cde2a4d6dd8144dfd606cba4e171686523f045cb77d2";
const readIndexChecksum =
  "e637542cb2e742fe8f22f60313a11f00adbc1d140a811ee718524849fcece12a";

// apply the checked-in runtime ACL to one disposable database
async function applyAcl(server, database) {
  await executeFile("docker", ["cp", `${root}/deploy/postgres/runtime-acl-v2.sql`,
    `${server.name}:/tmp/runtime-acl-v2.sql`]);
  await executeFile("docker", ["exec", server.name, "psql", "--set=ON_ERROR_STOP=1",
    "--username", server.user, "--dbname", database, "--file",
    "/tmp/runtime-acl-v2.sql"]);
}

// run the deployed effective-privilege verifier against one database
async function verifyAcl(server, database) {
  return await executeFile("bash", ["-c", `
source "$1"
container_name=$2
database_name=$3
# restrict compose forwarding to the disposable postgres container
compose() {
  [[ "$1" == exec && "$2" == -T && "$3" == postgres ]] || return 1
  shift 3
  docker exec -i "$container_name" "$@"
}
verify_runtime_database_acl /unused.env "$database_name"
`, "forecast-product-catalog-acl", `${root}/deploy/scripts/common.sh`, server.name,
  database], { timeout: 30_000 });
}

// copy one exact migration prefix for the retained schema-21 check
async function copyMigrationsThrough(target, maximumName) {
  await mkdir(target);

  // copy only the requested immutable prefix
  for (const name of (await readdir(migrationRoot)).sort()) {
    if (name.endsWith(".sql") && name <= maximumName) {
      await copyFile(join(migrationRoot, name), join(target, name));
    }
  }
}

// reconcile catalog grants only for exact schema 22 and preserve schema 21
test("forecast product catalog runtime ACL rejects drift and partial installs", {
  timeout: 240_000,
}, async () => {
  const temporaryRoot = await mkdtemp(join(tmpdir(), "weather-catalog-acl-"));
  const schema21Root = join(temporaryRoot, "migrations");
  const server = await startPostgres(17, "forecast-product-catalog-acl");
  const admin = createTestPool(server);
  let owner;
  let schema21Owner;

  try {
    await createRuntimeRoles(admin);
    await admin.query(`ALTER ROLE weather_owner LOGIN PASSWORD 'owner-test';
      ALTER DATABASE weather_test OWNER TO weather_owner;
      ALTER SCHEMA public OWNER TO weather_owner;`);
    owner = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    assert.equal((await runMigrations(owner, migrationRoot)).applied.length, 23);
    await applyAcl(server, "weather_test");
    await verifyAcl(server, "weather_test");

    // preserve exact schema 22 without requiring the additive read index
    await admin.query(`DROP INDEX weather_records_forecast_product_valid_idx;
      DELETE FROM schema_migrations
      WHERE name = '0023_forecast_product_read_index.sql'`);
    await verifyAcl(server, "weather_test");
    assert.deepEqual(
      (await runMigrations(owner, migrationRoot)).applied,
      ["0023_forecast_product_read_index.sql"],
    );
    await verifyAcl(server, "weather_test");

    // reject index ledger and object drift independently of catalog ACLs
    await admin.query(`UPDATE schema_migrations SET checksum = repeat('f', 64)
      WHERE name = '0023_forecast_product_read_index.sql'`);
    await assert.rejects(verifyAcl(server, "weather_test"),
      /runtime database ACL verification failed/u);
    await admin.query(`UPDATE schema_migrations SET checksum = $1
      WHERE name = '0023_forecast_product_read_index.sql'`, [readIndexChecksum]);
    await admin.query(`ALTER INDEX weather_records_forecast_product_valid_idx
      RENAME TO weather_records_forecast_product_valid_missing`);
    await assert.rejects(verifyAcl(server, "weather_test"),
      /runtime database ACL verification failed/u);
    await admin.query(`ALTER INDEX weather_records_forecast_product_valid_missing
      RENAME TO weather_records_forecast_product_valid_idx`);
    await verifyAcl(server, "weather_test");

    const graph = await admin.query(`SELECT
      (SELECT count(*)::integer FROM information_schema.columns
       WHERE table_schema = 'public' AND table_name = 'forecast_products') AS columns,
      (SELECT count(*)::integer FROM pg_constraint
       WHERE conrelid = 'public.forecast_products'::regclass AND contype = 'p') AS primary_keys,
      (SELECT count(*)::integer FROM pg_constraint
       WHERE conrelid = 'public.forecast_products'::regclass AND contype = 'f'
         AND confdeltype = 'r') AS restrict_foreign_keys,
      (SELECT count(*)::integer FROM pg_trigger
       WHERE tgrelid = 'public.weather_records'::regclass AND NOT tgisinternal
         AND tgenabled = 'O' AND tgname LIKE 'weather_records_forecast_products_%') AS triggers,
      (SELECT count(*)::integer FROM pg_proc procedure
       JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
       WHERE namespace.nspname = 'public'
         AND procedure.proname LIKE 'weather_sync_forecast_products_after_%'
         AND procedure.proowner = 'weather_owner'::regrole AND procedure.prosecdef
         AND procedure.provolatile = 'v'
         AND procedure.proconfig = ARRAY['search_path=pg_catalog, public']) AS functions`);
    assert.deepEqual(graph.rows[0], {
      columns: 2,
      functions: 3,
      primary_keys: 1,
      restrict_foreign_keys: 1,
      triggers: 3,
    });

    await admin.query("GRANT INSERT ON forecast_products TO weather_api");
    await assert.rejects(verifyAcl(server, "weather_test"),
      /runtime database ACL verification failed/u);
    await applyAcl(server, "weather_test");
    await verifyAcl(server, "weather_test");

    await admin.query(`GRANT EXECUTE ON FUNCTION
      weather_sync_forecast_products_after_insert() TO weather_ingest`);
    await assert.rejects(verifyAcl(server, "weather_test"),
      /runtime database ACL verification failed/u);
    await applyAcl(server, "weather_test");
    await verifyAcl(server, "weather_test");

    await admin.query(`ALTER FUNCTION weather_sync_forecast_products_after_delete()
      SET search_path = public`);
    await assert.rejects(verifyAcl(server, "weather_test"),
      /runtime database ACL verification failed/u);
    await admin.query(`ALTER FUNCTION weather_sync_forecast_products_after_delete()
      SET search_path = pg_catalog, public`);
    await verifyAcl(server, "weather_test");

    await admin.query("ALTER TABLE forecast_products RENAME TO forecast_products_missing");
    await assert.rejects(applyAcl(server, "weather_test"),
      /forecast product catalog schema is incomplete/u);
    await admin.query("ALTER TABLE forecast_products_missing RENAME TO forecast_products");
    await applyAcl(server, "weather_test");
    await verifyAcl(server, "weather_test");

    await admin.query(`UPDATE schema_migrations SET checksum = repeat('f', 64)
      WHERE name = '0022_forecast_product_catalog.sql'`);
    await assert.rejects(applyAcl(server, "weather_test"),
      /forecast product catalog schema is incomplete/u);
    await admin.query(`UPDATE schema_migrations SET checksum = $1
      WHERE name = '0022_forecast_product_catalog.sql'`, [catalogChecksum]);
    await applyAcl(server, "weather_test");

    await admin.query("CREATE DATABASE weather_schema21 OWNER weather_owner");
    await copyMigrationsThrough(schema21Root, "0021_adjustment_rolling_registration.sql");
    schema21Owner = createTestPool(server, "weather_schema21", "weather_owner", "owner-test");
    assert.equal((await runMigrations(schema21Owner, schema21Root)).applied.length, 21);
    await applyAcl(server, "weather_schema21");
    await verifyAcl(server, "weather_schema21");
    await schema21Owner.query(`INSERT INTO schema_migrations (name, checksum)
      VALUES ('0022_forecast_product_catalog.sql', $1)`, [catalogChecksum]);
    await assert.rejects(applyAcl(server, "weather_schema21"),
      /forecast product catalog schema is partially absent/u);
  } finally {
    await schema21Owner?.end();
    await owner?.end();
    await admin.end();
    await stopPostgres(server);
    await rm(temporaryRoot, { force: true, recursive: true });
  }
});
