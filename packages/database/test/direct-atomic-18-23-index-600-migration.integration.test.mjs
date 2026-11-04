import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { appendFile, cp, mkdtemp, readFile, readdir, rm } from "node:fs/promises";
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
const revised20Checksum =
  "58d50e88fa767212865e222ec02e55283ceac5b00538aa2898b5ebc2873b28f6";
const revised23Checksum =
  "7e48acb14710a625cdcbb796bf1bf49ab4e00bf1aeb9bb763ac86410e08663e0";
const revisedHistoryChecksum =
  "1cfd6251eeb66d443e36d661299c9e49e73465fab55e5dab2cfa4ee86dd36ad2";

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

// hash one canonical migration artifact
async function artifactChecksum(name) {
  return createHash("sha256")
    .update(await readFile(join(migrationDirectory, name)))
    .digest("hex");
}

// wrap one real pool while observing migration bodies
function observingPool(owner, observe) {
  return {
    // retain the real PostgreSQL session and transaction
    async connect() {
      const client = await owner.connect();
      return {
        // observe each exact query before delegation
        async query(sql, values) {
          await observe(client, sql, values);
          return await client.query(sql, values);
        },
        // return the real session after rollback and unlock
        release() {
          client.release();
        },
      };
    },
  };
}

// prove the index-only timeout lane remains atomic and exact
test("direct index-600 migration closes one exact schema 18 to revised 23 bridge", {
  timeout: 240_000,
}, async () => {
  const sourceRoot = await mkdtemp(join(tmpdir(), "weather-direct-index-600-source-"));
  const partialRoot = await mkdtemp(join(tmpdir(), "weather-direct-index-600-partial-"));
  const tamperedRoot = await mkdtemp(join(tmpdir(), "weather-direct-index-600-tampered-"));
  const server = await startPostgres(17, "direct-atomic-18-23-index-600-migration");
  const admin = createTestPool(server);
  const partialDatabase = `weather_direct_index_partial_${process.pid}`;
  let partialDatabaseCreated = false;
  let partialOwner;
  let owner;

  try {
    await prepareRuntimeRoles(admin);
    owner = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    const names = await copyPrefix(sourceRoot, 18);
    await runMigrations(owner, sourceRoot);

    await copyPrefix(tamperedRoot, 23);
    await appendFile(
      join(tamperedRoot, "0023_forecast_product_read_index.sql"),
      "\n-- test-only artifact tamper\n",
    );
    await assert.rejects(
      runMigrations(owner, tamperedRoot, {
        atomicDirect18To23Index600s: true,
      }),
      /exact forecast index artifact/u,
    );

    await assert.rejects(runMigrations(owner, migrationDirectory, {
      atomicDirect18To23: true,
      atomicDirect18To23Index600s: true,
    }), /mutually exclusive/u);

    let finalTailExecuted = false;
    const faultPool = observingPool(owner, async (client, sql, values) => {
      // fail only after the transformed index has executed
      if (
        typeof sql === "string" &&
        sql.includes("CREATE INDEX weather_records_forecast_product_valid_idx")
      ) {
        assert.match(sql, /SET LOCAL lock_timeout = '10s';/u);
        assert.match(sql, /SET LOCAL statement_timeout = '600s';/u);
        assert.doesNotMatch(sql, /SET LOCAL statement_timeout = '120s';/u);
        await client.query(sql, values);
        finalTailExecuted = true;
        await client.query("SELECT 1 / 0");
      }
    });

    await assert.rejects(
      runMigrations(faultPool, migrationDirectory, {
        atomicDirect18To23Index600s: true,
      }),
      /division by zero/u,
    );
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

    const migrationBodies = [];
    let indexLedgerTimeout;
    const applied = await runMigrations(observingPool(owner, async (client, sql, values) => {
      // retain only full migration bodies
      if (
        typeof sql === "string" &&
        sql.includes("SET LOCAL statement_timeout") &&
        sql.includes("CREATE")
      ) {
        migrationBodies.push(sql);
      }
      // observe the restored timeout immediately before the revised ledger insert
      if (
        typeof sql === "string" &&
        sql.startsWith("INSERT INTO schema_migrations") &&
        values?.[0] === "0023_forecast_product_read_index.sql" &&
        values?.[1] === revised23Checksum
      ) {
        indexLedgerTimeout = (await client.query("SHOW statement_timeout")).rows[0]
          .statement_timeout;
      }
    }), migrationDirectory, { atomicDirect18To23Index600s: true });
    assert.deepEqual(applied.applied, names.slice(18, 23));
    assert.deepEqual(applied.current, names.slice(0, 18));
    assert.equal(
      applied.executionVariant,
      "atomic-direct-18-to-23-revised-0020-index-600s",
    );
    assert.equal(applied.historySha256, revisedHistoryChecksum);
    assert.equal(applied.revised20Sha256, revised20Checksum);
    assert.equal(applied.revised23Sha256, revised23Checksum);
    assert.equal(indexLedgerTimeout, "2min");
    assert.equal(
      migrationBodies.filter((sql) => sql.includes("statement_timeout = '600s'")).length,
      1,
    );
    assert.equal(
      migrationBodies.filter((sql) => sql.includes("statement_timeout = '120s'")).length,
      1,
    );
    assert.deepEqual((await owner.query(`SELECT name, checksum
      FROM schema_migrations
      WHERE name IN ('0020_adjustment_revision_frontier.sql',
        '0023_forecast_product_read_index.sql')
      ORDER BY name`)).rows, [
      { checksum: revised20Checksum, name: "0020_adjustment_revision_frontier.sql" },
      { checksum: revised23Checksum, name: "0023_forecast_product_read_index.sql" },
    ]);
    assert.deepEqual(await verifyMigrationReadiness(owner, migrationDirectory), {
      version: "0023_forecast_product_read_index.sql",
    });

    // reject a mixed canonical-20 and revised-23 ledger
    await owner.query(`UPDATE schema_migrations
      SET checksum = $1
      WHERE name = '0020_adjustment_revision_frontier.sql'`, [
      await artifactChecksum("0020_adjustment_revision_frontier.sql"),
    ]);
    await assert.rejects(
      verifyMigrationReadiness(owner, migrationDirectory),
      /alternate history is not an exact reviewed ledger/u,
    );
    await owner.query(`UPDATE schema_migrations
      SET checksum = $1
      WHERE name = '0020_adjustment_revision_frontier.sql'`, [revised20Checksum]);

    // retain readiness for the older reviewed direct ledger
    await owner.query(`UPDATE schema_migrations
      SET checksum = $1
      WHERE name = '0023_forecast_product_read_index.sql'`, [
      await artifactChecksum("0023_forecast_product_read_index.sql"),
    ]);
    assert.deepEqual(await verifyMigrationReadiness(owner, migrationDirectory), {
      version: "0023_forecast_product_read_index.sql",
    });
    await assert.rejects(
      runMigrations(owner, migrationDirectory, {
        atomicDirect18To23Index600s: true,
      }),
      /exact 18-to-23 revised migration bridge/u,
    );
    await owner.query(`UPDATE schema_migrations
      SET checksum = $1
      WHERE name = '0023_forecast_product_read_index.sql'`, [revised23Checksum]);

    const retry = await runMigrations(owner, migrationDirectory, {
      atomicDirect18To23Index600s: true,
    });
    assert.deepEqual(retry.applied, []);
    assert.deepEqual(retry.current, names);

    await admin.query(`CREATE DATABASE ${partialDatabase} OWNER weather_owner`);
    partialDatabaseCreated = true;
    partialOwner = createTestPool(
      server,
      partialDatabase,
      "weather_owner",
      "owner-test",
    );
    await copyPrefix(partialRoot, 18);
    await runMigrations(partialOwner, partialRoot);
    // reject every partial source between the two reviewed endpoints
    for (const partialCount of [19, 20, 21, 22]) {
      const name = names[partialCount - 1];
      const checksum = name === "0020_adjustment_revision_frontier.sql"
        ? revised20Checksum
        : await artifactChecksum(name);
      await partialOwner.query(
        "INSERT INTO schema_migrations (name, checksum) VALUES ($1, $2)",
        [name, checksum],
      );
      await assert.rejects(
        runMigrations(partialOwner, migrationDirectory, {
          atomicDirect18To23Index600s: true,
        }),
        /exact 18-to-23 revised migration bridge/u,
      );
    }
  } finally {
    await partialOwner?.end();
    // remove only the test-owned secondary database
    if (partialDatabaseCreated) {
      await admin.query(`DROP DATABASE ${partialDatabase}`);
    }
    await owner?.end();
    await admin.end();
    await stopPostgres(server);
    await rm(sourceRoot, { force: true, recursive: true });
    await rm(partialRoot, { force: true, recursive: true });
    await rm(tamperedRoot, { force: true, recursive: true });
  }
});
