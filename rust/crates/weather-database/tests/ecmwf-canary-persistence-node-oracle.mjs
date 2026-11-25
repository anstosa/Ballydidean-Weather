import assert from "node:assert/strict";

import { persistEcmwfTemperatureCanaryRun } from "../../../../packages/database/dist/index.js";

const runInitializedAt = "2026-09-07T12:00:00.000Z";

// Build one exact cold run accepted by the production source validator.
function runInput(receivedAt = "2026-09-07T18:03:00.000Z") {
  return {
    adapterVersion: "open-meteo-ecmwf-single-run/v1",
    hours: Array.from({ length: 18 }, (_, index) => ({
      modelLeadHours: index + 1,
      rawRelativeHumidityPercent: 70 + index,
      rawTemperatureC: 10 + index / 10,
      rawWindSpeedMps: 2 + index / 10,
      validAt: new Date(
        Date.parse(runInitializedAt) + (index + 1) * 3_600_000,
      ).toISOString(),
    })),
    modelCycle: "50r1",
    providerResponseSha256: "a".repeat(64),
    receivedAt,
    recentErrorState: {
      b24C: null,
      b72C: null,
      cohort: "ecmwf_single_run_hindcast",
      localDates: 0,
      mad72C: null,
      maximumSourceRunInitializedAt: null,
      maximumSourceValidAt: null,
      n24: 0,
      n72: 0,
      sourceKeys: [],
      supported: false,
      targetRunInitializedAt: runInitializedAt,
      windowEndValidAt: "2026-09-07T05:00:00.000Z",
    },
    runInitializedAt,
    siteSlug: "ballydidean",
    stateReason: "no_causal_forecast_observation_pairs",
    stateStatus: "cold",
    upstreamModel: "ecmwf_ifs",
  };
}

const sql = new Map();
const invocations = [];
let currentTrace = [];
let stored = null;
let storedHours = [];
let failCommit = false;

// Collapse irrelevant whitespace while retaining every SQL token.
function normalizeSql(text) {
  return text.trim().split(/\s+/u).join(" ");
}

const client = {
  // Emulate only the ordered query results owned by this transaction.
  async query(text, values = []) {
    const normalized = normalizeSql(text);
    if (normalized === "BEGIN" || normalized === "COMMIT" || normalized === "ROLLBACK") {
      currentTrace.push(normalized.toLowerCase());
      if (normalized === "COMMIT" && failCommit) {
        failCommit = false;
        throw new Error("commit failed");
      }
      return { rows: [] };
    }
    if (normalized.startsWith("SELECT id, latitude, longitude FROM sites")) {
      sql.set("site", normalized);
      currentTrace.push("site");
      return { rows: [{ id: "site-1", latitude: 47.95, longitude: -122.43 }] };
    }
    if (normalized.startsWith("INSERT INTO ecmwf_temperature_canary_runs")) {
      sql.set("insert-run", normalized);
      currentTrace.push("insert-run");
      if (stored !== null) {
        return { rows: [] };
      }
      stored = {
        adapterVersion: values[4],
        contentHash: values[11],
        firstReceivedAt: values[2],
        id: "run-1",
        modelCycle: values[6],
        providerResponseSha256: values[5],
        recentErrorState: JSON.parse(values[7]),
        recentErrorStateSha256: values[8],
        runInitializedAt: values[1],
        stateReason: values[10],
        stateStatus: values[9],
        upstreamModel: values[3],
      };
      sql.set("insert-run-binds", values.join("|"));
      return { rows: [stored] };
    }
    if (normalized.startsWith("INSERT INTO ecmwf_temperature_canary_hours")) {
      sql.set("insert-hour", normalized);
      currentTrace.push(`hour-${values[2]}`);
      storedHours.push({
        contentHash: values[6],
        modelLeadHours: values[2],
        rawRelativeHumidityPercent: values[4],
        rawTemperatureC: values[3],
        rawWindSpeedMps: values[5],
        validAt: values[1],
      });
      if (values[2] === 1) {
        sql.set("first-hour-binds", values.join("|"));
      }
      if (values[2] === 18) {
        sql.set("last-hour-binds", values.join("|"));
      }
      return { rows: [] };
    }
    if (normalized.includes("FROM ecmwf_temperature_canary_runs") && normalized.endsWith("FOR UPDATE")) {
      sql.set("existing-run", normalized);
      currentTrace.push("existing-run");
      return { rows: [stored] };
    }
    if (normalized.includes("FROM ecmwf_temperature_canary_hours") && normalized.includes("ORDER BY model_lead_hours ASC")) {
      sql.set("existing-hours", normalized);
      currentTrace.push("existing-hours");
      return { rows: storedHours };
    }
    if (normalized.startsWith("UPDATE ecmwf_temperature_canary_runs")) {
      sql.set("update-receipt", normalized);
      currentTrace.push("update-receipt");
      return { rows: [] };
    }
    throw new Error(`unexpected SQL ${normalized}`);
  },
  // Record release after the repository finally block.
  release() {
    currentTrace.push("release");
  },
};
const pool = {
  // Return the retained fake client for every invocation.
  async connect() {
    currentTrace = [];
    invocations.push(currentTrace);
    return client;
  },
};

const inserted = await persistEcmwfTemperatureCanaryRun(pool, runInput());
const retry = await persistEcmwfTemperatureCanaryRun(
  pool,
  runInput("2026-09-07T18:05:00.000Z"),
);
const changed = runInput("2026-09-07T18:06:00.000Z");
changed.hours[7].rawTemperatureC += 1;
await assert.rejects(
  persistEcmwfTemperatureCanaryRun(pool, changed),
  /revision rejected/u,
);
failCommit = true;
await assert.rejects(
  persistEcmwfTemperatureCanaryRun(
    pool,
    runInput("2026-09-07T18:07:00.000Z"),
  ),
  /commit failed/u,
);
assert.equal(inserted.id, "run-1");
assert.equal(retry.firstReceivedAt, "2026-09-07T18:03:00.000Z");

for (const key of [
  "site",
  "insert-run",
  "insert-hour",
  "existing-run",
  "existing-hours",
  "update-receipt",
]) {
  console.log(`${key}-sql=${sql.get(key)}`);
}
console.log(`insert-run-binds=${sql.get("insert-run-binds")}`);
console.log(`first-hour-binds=${sql.get("first-hour-binds")}`);
console.log(`last-hour-binds=${sql.get("last-hour-binds")}`);
console.log(`recent-state-hash=${stored.recentErrorStateSha256}`);
console.log(`run-content-hash=${stored.contentHash}`);
console.log(`hour-hashes=${storedHours.map((hour) => hour.contentHash).join(",")}`);
console.log(`valid-times=${storedHours.map((hour) => hour.validAt).join(",")}`);
console.log(`recent-state-json=${JSON.stringify(stored.recentErrorState)}`);
console.log(`insert-trace=${invocations[0].join(",")}`);
console.log(`retry-trace=${invocations[1].join(",")}`);
console.log(`revision-trace=${invocations[2].join(",")}`);
console.log(`commit-trace=${invocations[3].join(",")}`);
console.log(`result=${inserted.id}|${inserted.stateStatus}|${inserted.firstReceivedAt}`);
console.log(`epochs=${Date.parse(runInitializedAt)}|${Date.parse("2026-09-07T18:03:00.000Z")}`);
