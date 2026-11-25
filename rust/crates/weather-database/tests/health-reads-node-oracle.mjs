import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";

import { createDatabaseWeatherReadStore } from "../../../../apps/api/dist/index.js";

const migrationDirectory = new URL(
  "../../../../packages/database/migrations/",
  import.meta.url,
);
const names = (await readdir(migrationDirectory))
  .filter((name) => name.endsWith(".sql"))
  .sort((left, right) => left.localeCompare(right));
const ledger = [];

// Hash exact local migration bytes like the production readiness loader.
for (const name of names) {
  const sql = await readFile(new URL(name, migrationDirectory));
  ledger.push({ checksum: createHash("sha256").update(sql).digest("hex"), name });
}

// Hash the canonical ordered applied ledger like production authorization checks.
function historySha256(rows) {
  const hash = createHash("sha256");
  // Retain exact row boundaries.
  for (const row of rows) {
    hash.update(`${row.name}:${row.checksum}\n`);
  }
  return hash.digest("hex");
}

// Collapse irrelevant SQL whitespace while retaining every query token.
function normalizeSql(text) {
  return text.trim().split(/\s+/u).join(" ");
}

// Build one fake pool with one isolated health failure point.
function fixturePool(failure = null) {
  const trace = [];
  return {
    trace,
    // Execute only the three production health statements.
    async query(text) {
      const sql = normalizeSql(text);
      // The ping is the only failure that stops subsequent reads.
      if (sql === "SELECT 1") {
        trace.push({ kind: "ping", sql });
        if (failure === "ping") {
          throw new Error("ping unavailable");
        }
        return { rows: [] };
      }
      // Migration failures remain isolated from heartbeat collection.
      if (sql === "SELECT name, checksum FROM schema_migrations ORDER BY name") {
        trace.push({ kind: "migration", sql });
        if (failure === "migration") {
          throw new Error("migration unavailable");
        }
        return { rows: ledger };
      }
      // Heartbeat failures collapse to null after migration classification.
      if (sql.includes("FROM worker_heartbeats")) {
        trace.push({ kind: "heartbeat", sql });
        if (failure === "heartbeat") {
          throw new Error("heartbeat unavailable");
        }
        // Invalid decoded timestamps are contained at the canonicalization boundary.
        if (failure === "heartbeat-invalid") {
          return { rows: [{ lastLoopAt: "invalid" }] };
        }
        return { rows: [{ lastLoopAt: "2026-09-07T18:02:00.000Z" }] };
      }
      throw new Error(`unexpected health SQL: ${sql}`);
    },
  };
}

// Run one production database store over an isolated fake pool.
async function run(failure = null) {
  const pool = fixturePool(failure);
  const store = createDatabaseWeatherReadStore(pool, {
    migrationDirectory: migrationDirectory.pathname,
    release: "development",
  });
  return { health: await store.getHealth(), trace: pool.trace };
}

// Render the exact public-safe store snapshot without JSON serialization.
function renderHealth(health) {
  return [
    health.database,
    health.migration.status,
    health.migration.version ?? "null",
    health.workerLastLoopAt ?? "null",
  ].join("|");
}

const current = await run();
const migrationFailure = await run("migration");
const heartbeatFailure = await run("heartbeat");
const heartbeatInvalid = await run("heartbeat-invalid");
const pingFailure = await run("ping");

console.log(`ping-sql=${current.trace[0].sql}`);
console.log(`migration-sql=${current.trace[1].sql}`);
console.log(`heartbeat-sql=${current.trace[2].sql}`);
console.log(`ledger=${ledger.map((row) => `${row.name}:${row.checksum}`).join(",")}`);
console.log(`history-sha256=${historySha256(ledger)}`);
console.log(`heartbeat-epoch=${Date.parse("2026-09-07T18:02:00.000Z")}`);
console.log(`current=${renderHealth(current.health)}`);
console.log(`migration-failure=${renderHealth(migrationFailure.health)}`);
console.log(`heartbeat-failure=${renderHealth(heartbeatFailure.health)}`);
console.log(`heartbeat-invalid=${renderHealth(heartbeatInvalid.health)}`);
console.log(`ping-failure=${renderHealth(pingFailure.health)}`);
console.log(`current-trace=${current.trace.map((entry) => entry.kind).join(",")}`);
console.log(`migration-failure-trace=${migrationFailure.trace.map((entry) => entry.kind).join(",")}`);
console.log(`heartbeat-failure-trace=${heartbeatFailure.trace.map((entry) => entry.kind).join(",")}`);
console.log(`heartbeat-invalid-trace=${heartbeatInvalid.trace.map((entry) => entry.kind).join(",")}`);
console.log(`ping-failure-trace=${pingFailure.trace.map((entry) => entry.kind).join(",")}`);
