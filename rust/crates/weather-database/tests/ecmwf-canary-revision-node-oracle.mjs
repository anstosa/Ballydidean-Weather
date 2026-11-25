import assert from "node:assert/strict";

import { persistEcmwfTemperatureCanaryRun } from "../../../../packages/database/dist/index.js";

const runInitializedAt = "2026-09-07T18:00:00.000Z";
const from = "2026-09-08T01:00:00.000Z";
const to = "2026-09-08T13:00:00.000Z";
const logicalReceivedAt = "2026-09-08T00:04:00.000Z";

// Build one exact cold source run for the optional archive branch.
function runInput() {
  return {
    adapterVersion: "open-meteo-ecmwf-single-run/v1",
    hours: Array.from({ length: 18 }, (_, index) => ({
      modelLeadHours: index + 1,
      rawRelativeHumidityPercent: 70 + index,
      rawTemperatureC: 10 + index / 10,
      rawWindSpeedMps: 2 + index / 10,
      validAt: new Date(Date.parse(runInitializedAt) + (index + 1) * 3_600_000).toISOString(),
    })),
    modelCycle: "50r1",
    providerResponseSha256: "b".repeat(64),
    receivedAt: "2026-09-08T00:03:00.000Z",
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
      windowEndValidAt: "2026-09-07T11:00:00.000Z",
    },
    runInitializedAt,
    siteSlug: "ballydidean",
    stateReason: "no_causal_forecast_observation_pairs",
    stateStatus: "cold",
    upstreamModel: "ecmwf_ifs",
  };
}

// Collapse irrelevant whitespace while retaining every SQL token.
function normalizeSql(text) {
  return text.trim().split(/\s+/u).join(" ");
}

// Build the complete eligible 12-hour Best Match matrix.
function bestMatchRows(invalidGeometry = false) {
  const rows = Array.from({ length: 12 }, (_, index) => ({
    contentHash: "c".repeat(64),
    id: `best-${index + 1}`,
    productRunAt: "2026-09-07T18:00:00.000Z",
    sourceId: "best-source-1",
    validAt: new Date(Date.parse(from) + index * 3_600_000).toISOString(),
  }));
  // retain no partial geometry when one decoded content hash is invalid
  if (invalidGeometry) {
    rows[5].contentHash = "not-a-sha256";
  }
  return rows;
}

// Execute one isolated optional-archive disposition.
async function execute(failArchive, invalidGeometry = false) {
  const trace = [];
  const captured = new Map();
  let stored = null;
  const hourHashes = [];
  let publicationCount = 0;
  let archivedRevision = null;
  const client = {
    // Return only the exact rows needed by the optional archive path.
    async query(text, values = []) {
      const normalized = normalizeSql(text);
      if (["BEGIN", "COMMIT", "ROLLBACK"].includes(normalized)) {
        trace.push(normalized.toLowerCase());
        return { rows: [] };
      }
      if (normalized.startsWith("SELECT id, latitude, longitude FROM sites")) {
        trace.push("site");
        return { rows: [{ id: "site-1", latitude: 47.95, longitude: -122.43 }] };
      }
      if (normalized.startsWith("INSERT INTO ecmwf_temperature_canary_runs")) {
        trace.push("insert-run");
        stored = {
          adapterVersion: values[4],
          contentHash: values[11],
          firstReceivedAt: values[2],
          id: failArchive ? "run-failure" : "run-positive",
          modelCycle: values[6],
          providerResponseSha256: values[5],
          recentErrorState: JSON.parse(values[7]),
          recentErrorStateSha256: values[8],
          runInitializedAt: values[1],
          stateReason: values[10],
          stateStatus: values[9],
          upstreamModel: values[3],
        };
        return { rows: [stored] };
      }
      if (normalized.startsWith("INSERT INTO ecmwf_temperature_canary_hours")) {
        trace.push(`hour-${values[2]}`);
        hourHashes.push(values[6]);
        return { rows: [] };
      }
      if (normalized === 'SELECT clock_timestamp() AS "logicalReceivedAt"') {
        captured.set("clock-sql", normalized);
        trace.push("clock");
        return { rows: [{ logicalReceivedAt: new Date(logicalReceivedAt) }] };
      }
      if (normalized === "SAVEPOINT ecmwf_temperature_revision_archive") {
        captured.set("savepoint-sql", normalized);
        trace.push("savepoint");
        return { rows: [] };
      }
      if (normalized === "ROLLBACK TO SAVEPOINT ecmwf_temperature_revision_archive") {
        captured.set("rollback-savepoint-sql", normalized);
        trace.push("rollback-savepoint");
        return { rows: [] };
      }
      if (normalized === "RELEASE SAVEPOINT ecmwf_temperature_revision_archive") {
        captured.set("release-savepoint-sql", normalized);
        trace.push("release-savepoint");
        return { rows: [] };
      }
      if (normalized.includes("JOIN LATERAL") && normalized.includes("upstream_model = 'best_match'")) {
        captured.set("best-match-sql", normalized);
        captured.set("best-match-binds", values.join("|"));
        trace.push("best-match");
        return { rows: bestMatchRows(invalidGeometry) };
      }
      throw new Error(`unexpected SQL ${normalized}`);
    },
    // Retain release ordering after commit.
    release() {
      trace.push("release");
    },
  };
  const pool = {
    // Return one retained fake client.
    async connect() {
      return client;
    },
  };
  const result = await persistEcmwfTemperatureCanaryRun(
    pool,
    runInput(),
    async (_queryable, revision) => {
      trace.push("stage");
      archivedRevision = revision;
      if (failArchive) {
        throw new Error("archive unavailable");
      }
      return {
        // Count only publication after the serving transaction commits.
        async publish() {
          trace.push("publish");
          publicationCount += 1;
        },
      };
    },
  );
  return { archivedRevision, captured, hourHashes, publicationCount, result, stored, trace };
}

// Execute one identical retry and prove that optional archive work stays skipped.
async function executeRetry(priorRun, priorHourHashes) {
  const trace = [];
  let stageCount = 0;
  const client = {
    // Return the existing immutable run and its exact hour identities.
    async query(text, values = []) {
      const normalized = normalizeSql(text);
      if (["BEGIN", "COMMIT", "ROLLBACK"].includes(normalized)) {
        trace.push(normalized.toLowerCase());
        return { rows: [] };
      }
      if (normalized.startsWith("SELECT id, latitude, longitude FROM sites")) {
        trace.push("site");
        return { rows: [{ id: "site-1", latitude: 47.95, longitude: -122.43 }] };
      }
      if (normalized.startsWith("INSERT INTO ecmwf_temperature_canary_runs")) {
        trace.push("insert-run");
        return { rows: [] };
      }
      if (normalized.includes("FROM ecmwf_temperature_canary_runs") && normalized.endsWith("FOR UPDATE")) {
        trace.push("existing-run");
        return { rows: [priorRun] };
      }
      if (normalized.includes("FROM ecmwf_temperature_canary_hours") && normalized.includes("ORDER BY model_lead_hours ASC")) {
        trace.push("existing-hours");
        return { rows: priorHourHashes.map((contentHash) => ({ contentHash })) };
      }
      if (normalized.startsWith("UPDATE ecmwf_temperature_canary_runs")) {
        trace.push("update-receipt");
        return { rows: [] };
      }
      throw new Error(`unexpected retry SQL ${normalized}`);
    },
    // Retain release ordering after the retry commit.
    release() {
      trace.push("release");
    },
  };
  const result = await persistEcmwfTemperatureCanaryRun(
    { connect: async () => client },
    runInput(),
    async () => {
      stageCount += 1;
      return null;
    },
  );
  return { result, stageCount, trace };
}

const positive = await execute(false);
const failure = await execute(true);
const ineligible = await execute(false, true);
const retry = await executeRetry(positive.stored, positive.hourHashes);
assert.equal(positive.archivedRevision.bestMatchRows.length, 12);
assert.equal(positive.publicationCount, 1);
assert.equal(failure.archivedRevision.bestMatchRows.length, 12);
assert.equal(failure.publicationCount, 0);
assert.equal(ineligible.archivedRevision.bestMatchRows.length, 0);
assert.equal(ineligible.publicationCount, 1);
assert.equal(retry.stageCount, 0);

for (const key of [
  "clock-sql",
  "savepoint-sql",
  "rollback-savepoint-sql",
  "release-savepoint-sql",
  "best-match-sql",
  "best-match-binds",
]) {
  const value = positive.captured.get(key) ?? failure.captured.get(key);
  console.log(`${key}=${value}`);
}
console.log(`positive-trace=${positive.trace.join(",")}`);
console.log(`failure-trace=${failure.trace.join(",")}`);
console.log(`ineligible-trace=${ineligible.trace.join(",")}`);
console.log(`retry-trace=${retry.trace.join(",")}`);
console.log(`revision=${[
  positive.archivedRevision.siteId,
  positive.archivedRevision.siteLatitude,
  positive.archivedRevision.siteLongitude,
  positive.archivedRevision.logicalReceivedAt,
  positive.archivedRevision.bestMatchRows.length,
  positive.archivedRevision.hours.length,
  positive.archivedRevision.runInitializedAt,
  positive.archivedRevision.contentHash,
].join("|")}`);
console.log(`result=${positive.result.id}|${failure.result.id}|${retry.result.id}`);
console.log(`hour-hashes=${positive.hourHashes.join(",")}`);
console.log(`epochs=${Date.parse(runInitializedAt)}|${Date.parse("2026-09-08T00:03:00.000Z")}|${Date.parse(from)}|${Date.parse(to)}`);
