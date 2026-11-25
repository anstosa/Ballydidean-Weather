import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { runMigrations } from "../../../../packages/database/dist/index.js";

const names = [
  "0001_initial_weather.sql",
  "0002_worker_migration_readiness.sql",
  "0003_ecowitt_measurements.sql",
];
const sourceDirectory = resolve(import.meta.dirname, "../../../../packages/database/migrations");
const directory = await mkdtemp(join(tmpdir(), "weather-rust-migration-plan-"));

// calculate digests at the owning Node boundary
function sha256(sql) {
  return createHash("sha256").update(sql).digest("hex");
}

// provide a database-free observation boundary
function fakePool(appliedRows, scripts) {
  const observed = { inserted: [], scripts: [] };
  const client = {
    // emulate only responses consumed by the compiled migrator
    async query(text, values = []) {
      // return the supported server floor
      if (text === "SELECT current_setting('server_version_num') AS server_version_num") {
        return { rowCount: 1, rows: [{ server_version_num: "150000" }] };
      }

      // return the supplied immutable history
      if (text === "SELECT name, checksum FROM schema_migrations ORDER BY name") {
        return { rowCount: appliedRows.length, rows: appliedRows };
      }

      // observe ledger insertion order
      if (text.startsWith("INSERT INTO schema_migrations")) {
        observed.inserted.push({ checksum: values[1], name: values[0] });
      }

      // observe script order without executing SQL
      if (scripts.includes(text)) {
        observed.scripts.push(text);
      }

      return { rowCount: 0, rows: [] };
    },
    // model the pool-client release boundary
    release() {},
  };

  return {
    observed,
    pool: {
      // return the same isolated fake client
      async connect() {
        return client;
      },
    },
  };
}

try {
  const artifacts = [];

  // copy three actual ordered repository artifacts
  for (const name of names) {
    const sql = await readFile(join(sourceDirectory, name), "utf8");
    await writeFile(join(directory, name), sql);
    artifacts.push({ checksum: sha256(sql), name, sql });
  }

  const pendingFixture = fakePool([artifacts[0]], artifacts.map((artifact) => artifact.sql));
  const pending = await runMigrations(pendingFixture.pool, directory);
  assert.deepEqual(pending.current, [names[0]]);
  assert.deepEqual(pending.applied, names.slice(1));
  assert.deepEqual(pendingFixture.observed.inserted, [
    { checksum: artifacts[1].checksum, name: names[1] },
    { checksum: artifacts[2].checksum, name: names[2] },
  ]);
  assert.deepEqual(
    pendingFixture.observed.scripts,
    artifacts.slice(1).map((artifact) => artifact.sql),
  );

  const currentFixture = fakePool(artifacts, artifacts.map((artifact) => artifact.sql));
  const current = await runMigrations(currentFixture.pool, directory);
  assert.deepEqual(current.current, names);
  assert.deepEqual(current.applied, []);
  assert.deepEqual(currentFixture.observed.scripts, []);

  const driftFixture = fakePool([
    { checksum: "0".repeat(64), name: names[0] },
  ], artifacts.map((artifact) => artifact.sql));
  // require the exact checksum error category
  await assert.rejects(
    runMigrations(driftFixture.pool, directory),
    { message: `migration checksum mismatch: ${names[0]}` },
  );

  const divergentFixture = fakePool([
    { checksum: artifacts[1].checksum, name: names[1] },
  ], artifacts.map((artifact) => artifact.sql));
  // require the exact history error category
  await assert.rejects(
    runMigrations(divergentFixture.pool, directory),
    { message: "migration history diverges from ordered artifacts" },
  );
} finally {
  await rm(directory, { force: true, recursive: true });
}

process.stdout.write("migration planning Node oracle passed\n");
