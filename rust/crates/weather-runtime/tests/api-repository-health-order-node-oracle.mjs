import { readFile } from "node:fs/promises";

const migrationSource = await readFile(
  new URL("../../../../packages/database/dist/migrate.js", import.meta.url),
  "utf8",
);
const apiSource = await readFile(
  new URL("../../../../apps/api/dist/index.js", import.meta.url),
  "utf8",
);

// require named compiled markers in exact source order
function assertOrder(source, label, markers) {
  let previous = -1;

  // advance monotonically through the selected compiled function
  for (const marker of markers) {
    const current = source.indexOf(marker, previous + 1);

    // reject absent or reordered production behavior
    if (current < 0 || current <= previous) {
      throw new Error(`${label} marker is absent or reordered: ${marker}`);
    }
    previous = current;
  }
}

const readinessStart = migrationSource.indexOf("export async function verifyMigrationReadiness");
const readinessEnd = migrationSource.indexOf("async function configureTimeouts", readinessStart);

// require the compiled migration readiness function
if (readinessStart < 0 || readinessEnd < 0) {
  throw new Error("compiled migration readiness function is unavailable");
}
assertOrder(migrationSource.slice(readinessStart, readinessEnd), "readiness", [
  "loadMigrationFiles(migrationDirectory)",
  "validateMigrationHistory",
]);

const healthStart = apiSource.indexOf("async getHealth()");
const healthEnd = apiSource.indexOf("async listHistory", healthStart);

// require the compiled health store function
if (healthStart < 0 || healthEnd < 0) {
  throw new Error("compiled API health function is unavailable");
}
assertOrder(apiSource.slice(healthStart, healthEnd), "health", [
  'pool.query("SELECT 1")',
  "verifyMigrationReadiness",
  "getLatestWorkerHeartbeat",
]);

console.log("health-order=ping,artifacts,ledger,heartbeat");
