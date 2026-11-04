import assert from "node:assert/strict";
import { appendFile, cp, mkdtemp, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import { runMigrations } from "../dist/index.js";
import {
  createTestPool,
  prepareRuntimeRoles,
  startPostgres,
  stopPostgres,
} from "./postgres-harness.mjs";

const migrationDirectory = resolve(import.meta.dirname, "../migrations");

// copy one genuine ordered migration prefix
async function copyPrefix(root, count) {
  const names = (await readdir(migrationDirectory)).filter(
    // match the production migration loader
    (name) => /^\d{4}_[a-z0-9_]+\.sql$/u.test(name),
  ).sort();

  // preserve exact migration bytes
  for (const name of names.slice(0, count)) {
    await cp(join(migrationDirectory, name), join(root, name));
  }
  return names;
}

// prove schema 22 and 23 commit or roll back together
test("atomic normal forecast upgrade closes the exact schema 21 to 23 bridge", {
  timeout: 180_000,
}, async () => {
  const sourceRoot = await mkdtemp(join(tmpdir(), "weather-atomic-forecast-source-"));
  const schema22Root = await mkdtemp(join(tmpdir(), "weather-atomic-forecast-22-"));
  const driftRoot = await mkdtemp(join(tmpdir(), "weather-atomic-forecast-drift-"));
  const server = await startPostgres(17, "atomic-normal-forecast-migration");
  const admin = createTestPool(server);
  const schema22Database = `weather_atomic_forecast_22_${process.pid}`;
  let owner;
  let schema22Owner;
  let schema22DatabaseCreated = false;

  try {
    await prepareRuntimeRoles(admin);
    owner = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    const names = await copyPrefix(sourceRoot, 21);
    await copyPrefix(schema22Root, 22);
    await copyPrefix(driftRoot, 23);
    await appendFile(
      join(driftRoot, "0023_forecast_product_read_index.sql"),
      "\n-- deliberate checksum drift\n",
    );
    await runMigrations(owner, sourceRoot);
    let indexMigrationReached = false;
    let indexMigrationExecuted = false;
    // wrap one fault-injecting real connection pool
    const faultPool = {
      // retain the real PostgreSQL session and transaction
      async connect() {
        const client = await owner.connect();
        return {
          // fail only after migration 0022 has executed
          async query(sql, values) {
            // execute migration 0023 before aborting its transaction
            if (
              typeof sql === "string" &&
              sql.includes("CREATE INDEX weather_records_forecast_product_valid_idx")
            ) {
              indexMigrationReached = true;
              await client.query(sql, values);
              indexMigrationExecuted = true;
              return await client.query("SELECT 1 / 0");
            }
            return await client.query(sql, values);
          },
          // return the real session after rollback and unlock
          release() {
            client.release();
          },
        };
      },
    };

    await assert.rejects(
      runMigrations(faultPool, migrationDirectory, { atomicNormal21To23: true }),
      /division by zero/u,
    );
    assert.equal(indexMigrationReached, true);
    assert.equal(indexMigrationExecuted, true);
    assert.deepEqual(
      (await owner.query("SELECT name FROM schema_migrations ORDER BY name")).rows,
      names.slice(0, 21).map(
        // retain the exact source ledger after the injected failure
        (name) => ({ name }),
      ),
    );
    assert.deepEqual((await owner.query(`SELECT
      to_regclass('public.forecast_products') AS catalog,
      to_regclass('public.weather_records_forecast_product_valid_idx') AS read_index`)).rows[0], {
      catalog: null,
      read_index: null,
    });

    const applied = await runMigrations(owner, migrationDirectory, {
      atomicNormal21To23: true,
    });
    assert.deepEqual(applied.applied, names.slice(21, 23));
    assert.deepEqual(applied.current, names.slice(0, 21));
    assert.deepEqual((await owner.query(`SELECT
      to_regclass('public.forecast_products')::text AS catalog,
      to_regclass('public.weather_records_forecast_product_valid_idx')::text AS read_index`)).rows[0], {
      catalog: "forecast_products",
      read_index: "weather_records_forecast_product_valid_idx",
    });
    const retry = await runMigrations(owner, migrationDirectory, {
      atomicNormal21To23: true,
    });
    assert.deepEqual(retry.applied, []);
    assert.deepEqual(retry.current, names.slice(0, 23));

    await assert.rejects(runMigrations(owner, migrationDirectory, {
      atomicMaintenanceV14: true,
      atomicNormal21To23: true,
    }), /mutually exclusive/u);
    await assert.rejects(
      runMigrations(owner, driftRoot, { atomicNormal21To23: true }),
      /migration checksum mismatch/u,
    );

    await admin.query(`CREATE DATABASE ${schema22Database} OWNER weather_owner`);
    schema22DatabaseCreated = true;
    schema22Owner = createTestPool(
      server,
      schema22Database,
      "weather_owner",
      "owner-test",
    );
    await runMigrations(schema22Owner, schema22Root);
    await assert.rejects(
      runMigrations(schema22Owner, migrationDirectory, { atomicNormal21To23: true }),
      /exact 21-to-23 migration bridge/u,
    );
    assert.deepEqual((await schema22Owner.query(`SELECT
      count(*)::integer AS migrations,
      to_regclass('public.forecast_products')::text AS catalog,
      to_regclass('public.weather_records_forecast_product_valid_idx') AS read_index
      FROM schema_migrations`)).rows[0], {
      catalog: "forecast_products",
      migrations: 22,
      read_index: null,
    });
  } finally {
    await schema22Owner?.end();
    // remove only the test-owned secondary database
    if (schema22DatabaseCreated) {
      await admin.query(`DROP DATABASE ${schema22Database}`);
    }
    await owner?.end();
    await admin.end();
    await stopPostgres(server);
    await Promise.all([
      rm(sourceRoot, { force: true, recursive: true }),
      rm(schema22Root, { force: true, recursive: true }),
      rm(driftRoot, { force: true, recursive: true }),
    ]);
  }
});
