import assert from "node:assert/strict";
import { cp, mkdtemp, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import { runMigrations, verifyMigrationReadiness } from "../dist/index.js";
import {
  createTestPool,
  prepareRuntimeRoles,
  startPostgres,
  stopPostgres,
} from "./postgres-harness.mjs";

const migrationDirectory = resolve(import.meta.dirname, "../migrations");
const revisedMigrationChecksum =
  "58d50e88fa767212865e222ec02e55283ceac5b00538aa2898b5ebc2873b28f6";
const revisionConstraintNames = [
  "ecmwf_temperature_canary_runs_adjustment_revision_receipt_check",
  "forecast_anchor_records_adjustment_revision_receipt_check",
  "rain_adjustment_runs_adjustment_revision_receipt_check",
  "weather_records_adjustment_revision_receipt_check",
];

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

// prove the reviewed direct bridge is atomic and exact
test("direct atomic migration closes the exact schema 18 to revised 23 bridge", {
  timeout: 240_000,
}, async () => {
  const sourceRoot = await mkdtemp(join(tmpdir(), "weather-direct-atomic-source-"));
  const partialRoot = await mkdtemp(join(tmpdir(), "weather-direct-atomic-partial-"));
  const server = await startPostgres(17, "direct-atomic-18-23-migration");
  const admin = createTestPool(server);
  const canonicalDatabase = `weather_direct_canonical_${process.pid}`;
  let canonicalDatabaseCreated = false;
  let canonicalOwner;
  let owner;

  try {
    await prepareRuntimeRoles(admin);
    owner = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    const names = await copyPrefix(sourceRoot, 18);
    await runMigrations(owner, sourceRoot);
    const siteId = (await owner.query(`INSERT INTO sites
      (slug, display_name, latitude, longitude, timezone)
      VALUES ('direct-atomic-fixture', 'Direct atomic fixture', 47.95, -122.43, 'UTC')
      RETURNING id`)).rows[0].id;
    const canaryId = (await owner.query(`INSERT INTO ecmwf_temperature_canary_runs
      (site_id, run_initialized_at, first_received_at, last_received_at, upstream_model,
       adapter_version, provider_response_sha256, model_cycle, recent_error_state,
       recent_error_state_sha256, state_status, state_reason, content_hash, revision_count)
      VALUES ($1, '2026-10-01T00:00:00Z', '2026-10-01T06:01:00Z',
        '2026-10-01T06:01:00Z', 'ecmwf_ifs', 'open-meteo-ecmwf-single-run/v1',
        $2, '50r1', '{}', $3, 'cold', 'fixture', $4, 0)
      RETURNING id`, [siteId, "a".repeat(64), "b".repeat(64), "c".repeat(64)])).rows[0].id;

    await assert.rejects(runMigrations(owner, migrationDirectory, {
      atomicDirect18To23: true,
      atomicMaintenanceV14: true,
    }), /mutually exclusive/u);

    let revisionMigrationReached = false;
    let finalTailExecuted = false;
    const faultPool = {
      // retain the real PostgreSQL session and transaction
      async connect() {
        const client = await owner.connect();
        return {
          // fail only after the final migration has executed
          async query(sql, values) {
            // observe the transformed migration before the injected failure
            if (
              typeof sql === "string" &&
              sql.includes("CREATE FUNCTION weather_adjustment_revision_receipt_valid_v1")
            ) {
              revisionMigrationReached = true;
            }
            // execute the final index before aborting its transaction
            if (
              typeof sql === "string" &&
              sql.includes("CREATE INDEX weather_records_forecast_product_valid_idx")
            ) {
              await client.query(sql, values);
              finalTailExecuted = true;
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
      runMigrations(faultPool, migrationDirectory, { atomicDirect18To23: true }),
      /division by zero/u,
    );
    assert.equal(revisionMigrationReached, true);
    assert.equal(finalTailExecuted, true);
    assert.deepEqual(
      (await owner.query("SELECT name FROM schema_migrations ORDER BY name")).rows,
      names.slice(0, 18).map(
        // retain the exact source ledger after the injected failure
        (name) => ({ name }),
      ),
    );
    assert.equal((await owner.query(`SELECT count(*)::integer AS columns
      FROM information_schema.columns
      WHERE table_schema = 'public'
        AND table_name = 'weather_records'
        AND column_name = 'adjustment_revision_receipt'`)).rows[0].columns, 0);
    assert.deepEqual((await owner.query(`SELECT
      to_regclass('public.forecast_products') AS catalog,
      to_regclass('public.weather_records_forecast_product_valid_idx') AS read_index`)).rows[0], {
      catalog: null,
      read_index: null,
    });

    const applied = await runMigrations(owner, migrationDirectory, {
      atomicDirect18To23: true,
    });
    assert.deepEqual(applied.applied, names.slice(18, 23));
    assert.deepEqual(applied.current, names.slice(0, 18));
    assert.equal(applied.executionVariant, "atomic-direct-18-to-23-revised-0020");
    assert.equal(applied.historySha256,
      "3fa9ed85218278aa3b20537d9fbe9b88ef1adee7d7b2f80fde832c45d9cb4554");
    assert.equal(applied.revised20Sha256, revisedMigrationChecksum);
    assert.equal((await owner.query(`SELECT checksum
      FROM schema_migrations
      WHERE name = '0020_adjustment_revision_frontier.sql'`)).rows[0].checksum,
    revisedMigrationChecksum);
    assert.deepEqual((await owner.query(`SELECT conname, convalidated
      FROM pg_constraint
      WHERE conname = ANY($1::text[])
      ORDER BY conname`, [revisionConstraintNames])).rows,
    revisionConstraintNames.map(
      // keep all four deferred historical scans explicit
      (conname) => ({ conname, convalidated: false }),
    ));
    await owner.query(`UPDATE ecmwf_temperature_canary_runs
      SET adjustment_revision_receipt = NULL
      WHERE id = $1`, [canaryId]);
    await assert.rejects(owner.query(`UPDATE ecmwf_temperature_canary_runs
      SET adjustment_revision_receipt = '{}'::jsonb
      WHERE id = $1`, [canaryId]), /adjustment_revision_receipt_check/u);
    assert.deepEqual(await verifyMigrationReadiness(owner, migrationDirectory), {
      version: "0023_forecast_product_read_index.sql",
    });

    await owner.query(`UPDATE schema_migrations
      SET checksum = $1
      WHERE name = '0020_adjustment_revision_frontier.sql'`, ["f".repeat(64)]);
    await assert.rejects(
      verifyMigrationReadiness(owner, migrationDirectory),
      /migration checksum mismatch/u,
    );
    await owner.query(`UPDATE schema_migrations
      SET checksum = $1
      WHERE name = '0020_adjustment_revision_frontier.sql'`, [revisedMigrationChecksum]);

    const retry = await runMigrations(owner, migrationDirectory, {
      atomicDirect18To23: true,
    });
    assert.deepEqual(retry.applied, []);
    assert.deepEqual(retry.current, names);

    const migration19Checksum = (await owner.query(`SELECT checksum
      FROM schema_migrations
      WHERE name = '0019_adjustment_maintenance_recurring.sql'`)).rows[0].checksum;
    await owner.query(`UPDATE schema_migrations
      SET checksum = $1
      WHERE name = '0019_adjustment_maintenance_recurring.sql'`, ["0".repeat(64)]);
    await assert.rejects(
      runMigrations(owner, migrationDirectory, { atomicDirect18To23: true }),
      /migration checksum mismatch/u,
    );
    await owner.query(`UPDATE schema_migrations
      SET checksum = $1
      WHERE name = '0019_adjustment_maintenance_recurring.sql'`, [migration19Checksum]);

    await admin.query(`CREATE DATABASE ${canonicalDatabase} OWNER weather_owner`);
    canonicalDatabaseCreated = true;
    canonicalOwner = createTestPool(
      server,
      canonicalDatabase,
      "weather_owner",
      "owner-test",
    );
    // reject every partial source between the two reviewed endpoints
    for (const partialCount of [19, 20, 21, 22]) {
      await copyPrefix(partialRoot, partialCount);
      await runMigrations(canonicalOwner, partialRoot);
      await assert.rejects(
        runMigrations(canonicalOwner, migrationDirectory, { atomicDirect18To23: true }),
        /exact 18-to-23 revised migration bridge/u,
      );
    }
    const canonical = await runMigrations(canonicalOwner, migrationDirectory);
    assert.deepEqual(canonical.applied, names.slice(22, 23));
    assert.deepEqual(canonical.current, names.slice(0, 22));
    assert.deepEqual(await verifyMigrationReadiness(canonicalOwner, migrationDirectory), {
      version: "0023_forecast_product_read_index.sql",
    });
    await assert.rejects(
      runMigrations(canonicalOwner, migrationDirectory, { atomicDirect18To23: true }),
      /exact 18-to-23 revised migration bridge/u,
    );
  } finally {
    await canonicalOwner?.end();
    // remove only the test-owned secondary database
    if (canonicalDatabaseCreated) {
      await admin.query(`DROP DATABASE ${canonicalDatabase}`);
    }
    await owner?.end();
    await admin.end();
    await stopPostgres(server);
    await rm(sourceRoot, { force: true, recursive: true });
    await rm(partialRoot, { force: true, recursive: true });
  }
});
