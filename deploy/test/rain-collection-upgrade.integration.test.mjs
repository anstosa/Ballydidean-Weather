import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { copyFile, mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import test from "node:test";
import { promisify } from "node:util";

import { runMigrations } from "../../packages/database/dist/index.js";
import {
  createTestPool,
  prepareRuntimeRoles,
  startPostgres,
  stopPostgres,
} from "../../packages/database/test/postgres-harness.mjs";

const executeFile = promisify(execFile);
const repoRoot = resolve(import.meta.dirname, "../..");
const migrationDirectory = join(repoRoot, "packages/database/migrations");
const runtimeAclPath = join(repoRoot, "deploy/postgres/runtime-acl-v2.sql");
const commonPath = join(repoRoot, "deploy/scripts/common.sh");
const updatePath = join(repoRoot, "deploy/scripts/update.sh");
const runIntegration = process.env.WEATHER_RUN_DEPLOY_INTEGRATION === "1";

// copy the installed predecessor ledger
async function copyMigrationPrefix(destination) {
  // select the exact prior migration files
  const migrations = (await readdir(migrationDirectory))
    .filter((name) => /^\d{4}_.+\.sql$/u.test(name))
    .sort()
    .slice(0, 13);

  // retain exact predecessor bytes
  for (const migration of migrations) {
    await copyFile(join(migrationDirectory, migration), join(destination, migration));
  }

  return migrations;
}

// apply the production ACL file in disposable PostgreSQL
async function applyRuntimeAcl(server) {
  const target = `/tmp/${basename(runtimeAclPath)}`;
  await executeFile("docker", ["cp", runtimeAclPath, `${server.name}:${target}`], {
    timeout: 30_000,
  });
  await executeFile("docker", [
    "exec", server.name, "psql", "--set=ON_ERROR_STOP=1", "--username",
    server.user, "--dbname", "weather_test", "--file", target,
  ], { timeout: 30_000 });
}

// run the installed runtime grant checker without Compose
async function verifyRuntimeAcl(server) {
  return executeFile("bash", [
    "-c",
    `source "$1"
container_name=$2
# route one container command
compose() {
  [[ "$1" == exec && "$2" == -T && "$3" == postgres ]] || return 1
  shift 3
  docker exec -i "$container_name" "$@"
}
verify_runtime_database_acl /unused.env weather_test`,
    "rain-collection-upgrade", commonPath, server.name,
  ], { timeout: 30_000 });
}

// prove the old schema upgrades without raw disclosure
test(
  "rain collection migrates from 0013 and enforces aggregate-only runtime authority",
  {
    // keep the database exercise opt-in
    skip: runIntegration ? false : "set WEATHER_RUN_DEPLOY_INTEGRATION=1",
    timeout: 180_000,
  },
  async () => {
    const prefixDirectory = await mkdtemp(join(tmpdir(), "weather-rain-collection-upgrade-"));
    const server = await startPostgres(17, "rain-collection-upgrade");
    const adminPool = createTestPool(server);
    const pool = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    const apiPool = createTestPool(server, "weather_test", "weather_api", "api-test");
    const ingestPool = createTestPool(server, "weather_test", "weather_ingest", "ingest-test");
    const exportPool = createTestPool(server, "weather_test", "weather_training_export", "training-export-test");

    try {
      await prepareRuntimeRoles(adminPool);
      const prefix = await copyMigrationPrefix(prefixDirectory);
      assert.equal(prefix.at(-1), "0013_ecmwf_temperature_canary.sql");
      assert.deepEqual((await runMigrations(pool, prefixDirectory)).applied, prefix);
      assert.equal((await pool.query("SELECT to_regclass('rain_capture_claims') AS relation")).rows[0].relation, null);
      await applyRuntimeAcl(server);

      // reject a partially installed collection schema
      await pool.query("CREATE TABLE rain_capture_claims (id bigint PRIMARY KEY)");
      await assert.rejects(applyRuntimeAcl(server), (error) =>
        error?.stderr?.includes("prospective rain capture schema is incomplete"));
      await pool.query("DROP TABLE rain_capture_claims");

      const upgraded = await runMigrations(pool, migrationDirectory);
      assert.deepEqual(upgraded.current, prefix);
      assert.deepEqual(upgraded.applied, [
        "0014_rain_collection.sql",
        "0015_rain_station_access.sql",
        "0016_rain_adjustment.sql",
        "0017_adjustment_evaluation_export.sql",
        "0018_adjustment_maintenance_v2.sql",
        "0019_adjustment_maintenance_recurring.sql",
        "0020_adjustment_revision_frontier.sql",
        "0021_adjustment_rolling_registration.sql",
        "0022_forecast_product_catalog.sql",
      ]);
      await applyRuntimeAcl(server);
      await verifyRuntimeAcl(server);

      const status = await apiPool.query("SELECT contract_version, model_enabled, qualification_enabled, claims FROM rain_collection_status_v1");
      assert.deepEqual(status.rows, [{
        claims: 0,
        contract_version: "rain-prospective-capture/v1",
        model_enabled: false,
        qualification_enabled: false,
      }]);
      await assert.rejects(apiPool.query("SELECT id FROM rain_capture_claims"), { code: "42501" });
      await assert.rejects(apiPool.query("SELECT compressed_body FROM rain_capture_receipts"), { code: "42501" });
      await assert.rejects(exportPool.query("SELECT claims FROM rain_collection_status_v1"), { code: "42501" });
      await assert.rejects(exportPool.query("SELECT id FROM rain_capture_claims"), { code: "42501" });
      await assert.rejects(ingestPool.query("UPDATE rain_capture_claims SET slot_key = slot_key WHERE false"), { code: "42501" });
      assert.deepEqual((await apiPool.query("SELECT count(*)::integer AS runs FROM rain_adjustment_runs")).rows, [{ runs: 0 }]);
      await assert.rejects(exportPool.query("SELECT hours FROM rain_adjustment_runs"), { code: "42501" });
      await assert.rejects(apiPool.query("INSERT INTO rain_adjustment_runs DEFAULT VALUES"), { code: "42501" });
      await assert.rejects(ingestPool.query("UPDATE rain_adjustment_runs SET hours = hours WHERE false"), { code: "42501" });

      // prove the checker rejects drift in every sensitive role
      for (const [drift, undo] of [
        ["REVOKE SELECT ON rain_collection_status_v1 FROM weather_api", null],
        ["GRANT SELECT (station_id) ON rain_capture_claims TO weather_api", "REVOKE SELECT (station_id) ON rain_capture_claims FROM weather_api"],
        ["GRANT UPDATE (slot_key) ON rain_capture_claims TO weather_ingest", "REVOKE UPDATE (slot_key) ON rain_capture_claims FROM weather_ingest"],
        ["GRANT SELECT ON rain_capture_receipts TO weather_training_export", null],
        ["REVOKE SELECT ON rain_adjustment_runs FROM weather_api", null],
        ["GRANT SELECT (hours) ON rain_adjustment_runs TO weather_training_export", "REVOKE SELECT (hours) ON rain_adjustment_runs FROM weather_training_export"],
        ["GRANT weather_owner TO weather_api", "REVOKE weather_owner FROM weather_api"],
      ]) {
        await adminPool.query(drift);
        await assert.rejects(verifyRuntimeAcl(server), (error) =>
          error?.stderr?.includes("runtime database ACL verification failed"));

        // remove explicit column drift before replaying table ACLs
        if (undo !== null) {
          await adminPool.query(undo);
        }
        await applyRuntimeAcl(server);
      }
      await verifyRuntimeAcl(server);

      // replay the exact compatibility-clone reset command
      const update = await readFile(updatePath, "utf8");
      const reset = update.match(/--command "(CREATE OR REPLACE FUNCTION weather_source_is_current[^\n]+)"/u);
      assert.notEqual(reset, null);
      await pool.query(reset[1].replaceAll("\\$", "$"));
      const boundary = await pool.query(`
        SELECT count(*) AS migrations,
          to_regclass('rain_capture_claims') AS claims,
          to_regclass('rain_capture_receipts') AS receipts,
          to_regclass('rain_collection_status_v1') AS status,
          to_regclass('rain_adjustment_runs') AS adjustment_runs,
          to_regprocedure('weather_guard_rain_adjustment_run()') AS adjustment_guard,
          to_regprocedure('weather_guard_rain_capture_claim()') AS claim_guard,
          to_regprocedure('weather_guard_rain_capture_receipt()') AS receipt_guard,
          to_regprocedure('weather_reject_rain_capture_mutation()') AS mutation_guard,
          to_regclass('forecast_products') AS forecast_products,
          to_regprocedure('weather_sync_forecast_products_after_insert()') AS forecast_product_insert,
          to_regprocedure('weather_sync_forecast_products_after_delete()') AS forecast_product_delete,
          to_regprocedure('weather_sync_forecast_products_after_truncate()') AS forecast_product_truncate
        FROM schema_migrations
      `);
      assert.deepEqual(boundary.rows[0], {
        adjustment_guard: null,
        adjustment_runs: null,
        claim_guard: null,
        claims: null,
        forecast_product_delete: null,
        forecast_product_insert: null,
        forecast_product_truncate: null,
        forecast_products: null,
        migrations: "8",
        mutation_guard: null,
        receipt_guard: null,
        receipts: null,
        status: null,
      });
      const replayed = await runMigrations(pool, migrationDirectory);
      assert.deepEqual(replayed.applied.slice(-9), [
        "0014_rain_collection.sql",
        "0015_rain_station_access.sql",
        "0016_rain_adjustment.sql",
        "0017_adjustment_evaluation_export.sql",
        "0018_adjustment_maintenance_v2.sql",
        "0019_adjustment_maintenance_recurring.sql",
        "0020_adjustment_revision_frontier.sql",
        "0021_adjustment_rolling_registration.sql",
        "0022_forecast_product_catalog.sql",
      ]);
      const replayedCatalog = await pool.query(`
        SELECT to_regclass('forecast_products')::text AS catalog,
          (SELECT count(*)::integer
           FROM pg_proc procedure
           JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
           WHERE namespace.nspname = 'public'
             AND procedure.proname LIKE 'weather_sync_forecast_products_after_%') AS functions,
          (SELECT count(*)::integer
           FROM pg_trigger trigger
           WHERE trigger.tgrelid = 'weather_records'::regclass
             AND NOT trigger.tgisinternal
             AND trigger.tgname LIKE 'weather_records_forecast_products_%') AS triggers,
          NOT EXISTS (
            (SELECT DISTINCT source_id, product_run_at
             FROM weather_records
             WHERE source_kind = 'forecast'
             EXCEPT
             SELECT source_id, product_run_at FROM forecast_products)
            UNION ALL
            (SELECT source_id, product_run_at FROM forecast_products
             EXCEPT
             SELECT DISTINCT source_id, product_run_at
             FROM weather_records
             WHERE source_kind = 'forecast')
          ) AS consistent
      `);
      assert.deepEqual(replayedCatalog.rows, [{
        catalog: "forecast_products",
        consistent: true,
        functions: 3,
        triggers: 3,
      }]);
      await applyRuntimeAcl(server);
      await verifyRuntimeAcl(server);
    } finally {
      // clean disposable database resources
      // close every disposable connection
      await Promise.all([apiPool, ingestPool, exportPool, pool, adminPool].map((connection) =>
        connection.end().catch(() => undefined)));
      await stopPostgres(server);
      await rm(prefixDirectory, { force: true, recursive: true });
    }
  },
);
