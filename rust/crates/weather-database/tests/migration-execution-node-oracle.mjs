import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { runMigrations } from "../../../../packages/database/dist/index.js";

const lockKey = 8_032_416_683_782_917;
const names = [
  "0001_initial_weather.sql",
  "0002_worker_migration_readiness.sql",
  "0003_ecowitt_measurements.sql",
  "0004_tempest_metadata.sql",
];
const sourceDirectory = resolve(import.meta.dirname, "../../../../packages/database/migrations");
const directory = await mkdtemp(join(tmpdir(), "weather-rust-migration-execution-"));

// calculate digests at the owning node boundary
function sha256(sql) {
  return createHash("sha256").update(sql).digest("hex");
}

// expose compiled migration commands without a database
function fakePool(appliedRows, scriptsBySql, failScript = null) {
  const trace = [];
  let historyRead = false;
  const client = {
    // emulate only responses consumed by runMigrations
    async query(text, values = []) {
      // return the supported server floor
      if (text === "SELECT current_setting('server_version_num') AS server_version_num") {
        return { rowCount: 1, rows: [{ server_version_num: "150000" }] };
      }

      // record advisory lock acquisition
      if (text === "SELECT pg_advisory_lock($1::bigint)") {
        trace.push(`lock:${values[0]}`);
      }

      // return the supplied ordered history
      if (text === "SELECT name, checksum FROM schema_migrations ORDER BY name") {
        historyRead = true;
        trace.push("history");
        return { rowCount: appliedRows.length, rows: appliedRows };
      }

      // record only per-migration transaction starts
      if (historyRead && text === "BEGIN") {
        trace.push("begin");
      }

      const scriptName = scriptsBySql.get(text);
      // record opaque script submission
      if (historyRead && scriptName !== undefined) {
        trace.push(`script:${scriptName}`);

        // inject the selected script failure
        if (scriptName === failScript) {
          throw new Error(`script failed: ${scriptName}`);
        }
      }

      // record one applied ledger insertion
      if (historyRead && text.startsWith("INSERT INTO schema_migrations")) {
        trace.push(`record:${values[0]}:${values[1]}`);
      }

      // record transaction completion after history
      if (historyRead && (text === "COMMIT" || text === "ROLLBACK")) {
        trace.push(text.toLowerCase());
      }

      // record advisory lock release
      if (text === "SELECT pg_advisory_unlock($1::bigint)") {
        trace.push(`unlock:${values[0]}`);
      }

      return { rowCount: 0, rows: [] };
    },
    // record client release
    release() {
      trace.push("release");
    },
  };

  return {
    trace,
    pool: {
      // return the isolated fake client
      async connect() {
        return client;
      },
    },
  };
}

try {
  const artifacts = [];
  const scriptsBySql = new Map();

  // copy four actual ordered repository artifacts
  for (const name of names) {
    const sql = await readFile(join(sourceDirectory, name), "utf8");
    await writeFile(join(directory, name), sql);
    const artifact = { checksum: sha256(sql), name, sql };
    artifacts.push(artifact);
    scriptsBySql.set(sql, name);
  }

  const successFixture = fakePool(artifacts.slice(0, 2), scriptsBySql);
  const success = await runMigrations(successFixture.pool, directory);
  assert.deepEqual(success.current, names.slice(0, 2));
  assert.deepEqual(success.applied, names.slice(2));
  assert.deepEqual(successFixture.trace, [
    `lock:${lockKey}`,
    "history",
    "begin",
    `script:${names[2]}`,
    `record:${names[2]}:${artifacts[2].checksum}`,
    "commit",
    "begin",
    `script:${names[3]}`,
    `record:${names[3]}:${artifacts[3].checksum}`,
    "commit",
    `unlock:${lockKey}`,
    "release",
  ]);

  const failureFixture = fakePool(artifacts.slice(0, 1), scriptsBySql, names[2]);
  await assert.rejects(
    runMigrations(failureFixture.pool, directory),
    { message: `script failed: ${names[2]}` },
  );
  assert.deepEqual(failureFixture.trace, [
    `lock:${lockKey}`,
    "history",
    "begin",
    `script:${names[1]}`,
    `record:${names[1]}:${artifacts[1].checksum}`,
    "commit",
    "begin",
    `script:${names[2]}`,
    "rollback",
    `unlock:${lockKey}`,
    "release",
  ]);

  const driftFixture = fakePool([
    { checksum: "0".repeat(64), name: names[0] },
  ], scriptsBySql);
  await assert.rejects(
    runMigrations(driftFixture.pool, directory),
    { message: `migration checksum mismatch: ${names[0]}` },
  );
  assert.deepEqual(driftFixture.trace, [
    `lock:${lockKey}`,
    "history",
    `unlock:${lockKey}`,
    "release",
  ]);
} finally {
  await rm(directory, { force: true, recursive: true });
}

process.stdout.write("migration execution Node oracle passed\n");
