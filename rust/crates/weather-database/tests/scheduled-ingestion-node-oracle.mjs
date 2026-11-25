import assert from "node:assert/strict";
import { createHash } from "node:crypto";

import {
  SourceSession,
  completeScheduledIngestion,
} from "../../../../packages/database/dist/index.js";
import { weatherRecordContent } from "../../../../packages/domain/dist/index.js";

const windowStart = "2026-11-23T10:00:00.000Z";
const windowEndExclusive = "2026-11-23T11:00:00.000Z";
const lastValidAt = "2026-11-23T10:55:00.000Z";
const checksum = "a".repeat(64);
const record = {
  metadata: {
    device: { model: "GW2000", serial: "station-7", vendor: "Ecowitt" },
    model: null,
    provider: { requestId: "response-7" },
    quality: { accepted: true },
    upstreamTimezone: "Europe/London",
  },
  metrics: {
    apparentTemperatureC: 8.25,
    blackGlobeTemperatureC: null,
    cloudCoverPercent: 75,
    pm25MicrogramsPerCubicMeter: 4.5,
    precipitationMm: 0.2,
    precipitationRateMmPerHour: 1.25,
    pressureHpa: 1008.4,
    relativeHumidityPercent: 86,
    soilElectricalConductivityMicrosiemensPerCm: null,
    soilMoisturePercent: null,
    solarRadiationWm2: 32,
    temperatureC: 9.5,
    uvIndex: 0,
    waterLevelM: null,
    windDirectionDegrees: 245,
    windGustMps: 8.75,
    windSpeedMps: 4.25,
    wetBulbGlobeTemperatureC: null,
  },
  productRunAt: null,
  receivedAt: "2026-11-23T10:56:00.000Z",
  sourceId: "source-1",
  sourceKind: "physical_sensor",
  validAt: lastValidAt,
};

// create one exact no-host client for the scheduled transaction
function fakeClient(checkpointRowCount, failCommit = false) {
  const trace = [];
  const expectedHash = createHash("sha256").update(weatherRecordContent(record)).digest("hex");

  return {
    trace,
    // emulate only statements consumed by completeScheduledIngestion
    async query(text, values = []) {
      // preserve transaction start order
      if (text === "BEGIN") {
        trace.push("begin");
        return { rowCount: null, rows: [] };
      }

      // preserve transaction completion order
      if (text === "COMMIT") {
        trace.push("commit");

        // inject the transaction-finalization failure after recording its order
        if (failCommit) {
          throw new Error("commit failed");
        }

        return { rowCount: null, rows: [] };
      }

      // preserve rollback after any operation or commit failure
      if (text === "ROLLBACK") {
        trace.push("rollback");
        return { rowCount: null, rows: [] };
      }

      // guard the exact running scheduled row
      if (text.includes("SELECT 1") && text.includes("mode = 'scheduled'")) {
        assert.deepEqual(values, ["run-1", "source-1", windowStart, windowEndExclusive]);
        trace.push(`assert:${values.join(":")}`);
        return { rowCount: 1, rows: [{ "?column?": 1 }] };
      }

      // inspect the full normalized row bind group
      if (text.includes("INSERT INTO weather_records")) {
        assert.equal(values.length, 32);
        assert.equal(values[0], "source-1");
        assert.equal(values[4], "run-1");
        assert.equal(values[11], '{"accepted":true}');
        assert.equal(values[12], '{"requestId":"response-7"}');
        assert.equal(values[31], expectedHash);
        trace.push(`records:1:${values.length}`);
        return { rowCount: 1, rows: [] };
      }

      // select changed archive inputs before the serving upsert
      if (text.includes("jsonb_to_recordset")) {
        const requested = JSON.parse(values[1]);
        assert.equal(values[0], "source-1");
        assert.equal(requested.length, 1);
        assert.equal(requested[0].contentHash, expectedHash);
        trace.push("revision-select:1");
        return { rowCount: 0, rows: [] };
      }

      // load archive lineage after the serving upsert
      if (text.includes('run.adapter_version AS "adapterVersion"')) {
        assert.deepEqual(values, ["run-1", "source-1"]);
        trace.push("revision-context");
        return {
          rowCount: 1,
          rows: [{
            adapterVersion: "adapter/v1",
            logicalReceivedAt: new Date("2026-11-23T10:54:00.000Z"),
            providerKey: "provider-1",
            sourceConfigFingerprint: "fingerprint-1",
            sourceKey: "source-key-1",
          }],
        };
      }

      // retain the archive savepoint on the same fake client
      if (text === "SAVEPOINT weather_revision_archive") {
        trace.push("revision-savepoint");
        return { rowCount: null, rows: [] };
      }

      // retain archive cleanup before scheduled work resumes
      if (text === "ROLLBACK TO SAVEPOINT weather_revision_archive") {
        trace.push("revision-rollback-to");
        return { rowCount: null, rows: [] };
      }

      // retain archive success or cleanup release order
      if (text === "RELEASE SAVEPOINT weather_revision_archive") {
        trace.push("revision-release");
        return { rowCount: null, rows: [] };
      }

      // expose the optimistic checkpoint version and durable cursor
      if (text.includes("UPDATE ingestion_checkpoints")) {
        assert.deepEqual(values, [
          "source-1",
          7,
          lastValidAt,
          windowStart,
          windowEndExclusive,
          '{"cursor":"cursor-8"}',
        ]);
        trace.push(`checkpoint:update:${values[1]}:${values[5]}`);
        return { rowCount: checkpointRowCount, rows: [] };
      }

      // expose success evidence and the scheduled finalization guard
      if (text.includes("UPDATE ingestion_runs") && text.includes("state = 'succeeded'")) {
        assert.deepEqual(values, [
          "run-1",
          "source-1",
          2,
          1,
          '{"etag":"response-9"}',
          checksum,
          "scheduled",
        ]);
        trace.push(`finalize:${values.slice(2).join(":")}`);
        return { rowCount: 1, rows: [] };
      }

      throw new Error(`unexpected scheduled ingestion SQL: ${text}`);
    },
  };
}

// execute one compiled TypeScript transaction fixture
async function executeFixture(checkpointRowCount, failCommit = false, archiveMode = "disabled") {
  const client = fakeClient(checkpointRowCount, failCommit);
  const session = new SourceSession(client, "source-1");
  const input = {
    attempts: 2,
    expectedCheckpointVersion: 7,
    lastValidAt,
    providerCursor: { cursor: "cursor-8" },
    records: [record],
    responseMetadata: { etag: "response-9" },
    runId: "run-1",
    upstreamResponseChecksum: checksum,
    windowEndExclusive,
    windowStart,
  };

  // arm the optional archive path only for explicit archive fixtures
  if (archiveMode !== "disabled") {
    input.revisionArchiver = async (queryable, revisions) => {
      assert.equal(queryable, client);
      assert.equal(revisions.length, 1);
      assert.equal(revisions[0].contentHash, expectedHashFor(record));
      assert.equal(revisions[0].record.validAt, lastValidAt);
      assert.equal(revisions[0].logicalReceivedAt, "2026-11-23T10:54:00.000Z");

      // preserve a swallowed stage failure inside the savepoint
      if (archiveMode === "stage-failure") {
        client.trace.push("revision-stage!");
        throw new Error("stage failed");
      }

      client.trace.push("revision-stage:1");
      return {
        // publish only after successful serving commit
        async publish() {
          client.trace.push("revision-publish");
        },
      };
    };
  }

  // return both trace and ordinary guarded-write error
  try {
    await completeScheduledIngestion(session, input);
    return { error: null, trace: client.trace };
  } catch (error) {
    return { error: error.message, trace: client.trace };
  }
}

// compute the exact compiled-domain hash asserted by the archive fixture
function expectedHashFor(value) {
  return createHash("sha256").update(weatherRecordContent(value)).digest("hex");
}

const success = await executeFixture(1);
assert.equal(success.error, null);
assert.deepEqual(success.trace, [
  "begin",
  `assert:run-1:source-1:${windowStart}:${windowEndExclusive}`,
  "records:1:32",
  'checkpoint:update:7:{"cursor":"cursor-8"}',
  `finalize:2:1:{"etag":"response-9"}:${checksum}:scheduled`,
  "commit",
]);

const mismatch = await executeFixture(0);
assert.equal(
  mismatch.error,
  "checkpoint compare-and-set did not affect exactly one row",
);
assert.deepEqual(mismatch.trace, [
  "begin",
  `assert:run-1:source-1:${windowStart}:${windowEndExclusive}`,
  "records:1:32",
  'checkpoint:update:7:{"cursor":"cursor-8"}',
  "rollback",
]);

const commitFailure = await executeFixture(1, true);
assert.equal(commitFailure.error, "commit failed");
assert.deepEqual(commitFailure.trace, [
  "begin",
  `assert:run-1:source-1:${windowStart}:${windowEndExclusive}`,
  "records:1:32",
  'checkpoint:update:7:{"cursor":"cursor-8"}',
  `finalize:2:1:{"etag":"response-9"}:${checksum}:scheduled`,
  "commit",
  "rollback",
]);

process.stdout.write(`success=${success.trace.join("|")}\n`);
process.stdout.write(`checkpoint-mismatch=${mismatch.trace.join("|")}\n`);
process.stdout.write(`checkpoint-error=${mismatch.error}\n`);
process.stdout.write(`commit-failure=${commitFailure.trace.join("|")}\n`);
process.stdout.write(`commit-error=${commitFailure.error}\n`);

const archiveSuccess = await executeFixture(1, false, "armed");
assert.equal(archiveSuccess.error, null);
assert.deepEqual(archiveSuccess.trace, [
  "begin",
  `assert:run-1:source-1:${windowStart}:${windowEndExclusive}`,
  "revision-select:1",
  "records:1:32",
  "revision-context",
  "revision-savepoint",
  "revision-stage:1",
  "revision-release",
  'checkpoint:update:7:{"cursor":"cursor-8"}',
  `finalize:2:1:{"etag":"response-9"}:${checksum}:scheduled`,
  "commit",
  "revision-publish",
]);

const archiveStageFailure = await executeFixture(1, false, "stage-failure");
assert.equal(archiveStageFailure.error, null);
assert.deepEqual(archiveStageFailure.trace, [
  "begin",
  `assert:run-1:source-1:${windowStart}:${windowEndExclusive}`,
  "revision-select:1",
  "records:1:32",
  "revision-context",
  "revision-savepoint",
  "revision-stage!",
  "revision-rollback-to",
  "revision-release",
  'checkpoint:update:7:{"cursor":"cursor-8"}',
  `finalize:2:1:{"etag":"response-9"}:${checksum}:scheduled`,
  "commit",
]);

const archiveCheckpointFailure = await executeFixture(0, false, "armed");
assert.equal(
  archiveCheckpointFailure.error,
  "checkpoint compare-and-set did not affect exactly one row",
);
assert.deepEqual(archiveCheckpointFailure.trace, [
  "begin",
  `assert:run-1:source-1:${windowStart}:${windowEndExclusive}`,
  "revision-select:1",
  "records:1:32",
  "revision-context",
  "revision-savepoint",
  "revision-stage:1",
  "revision-release",
  'checkpoint:update:7:{"cursor":"cursor-8"}',
  "rollback",
]);

const archiveCommitFailure = await executeFixture(1, true, "armed");
assert.equal(archiveCommitFailure.error, "commit failed");
assert.deepEqual(archiveCommitFailure.trace, [
  "begin",
  `assert:run-1:source-1:${windowStart}:${windowEndExclusive}`,
  "revision-select:1",
  "records:1:32",
  "revision-context",
  "revision-savepoint",
  "revision-stage:1",
  "revision-release",
  'checkpoint:update:7:{"cursor":"cursor-8"}',
  `finalize:2:1:{"etag":"response-9"}:${checksum}:scheduled`,
  "commit",
  "rollback",
]);

process.stdout.write(`archive-success=${archiveSuccess.trace.join("|")}\n`);
process.stdout.write(`archive-stage-failure=${archiveStageFailure.trace.join("|")}\n`);
process.stdout.write(`archive-checkpoint-failure=${archiveCheckpointFailure.trace.join("|")}\n`);
process.stdout.write(`archive-checkpoint-error=${archiveCheckpointFailure.error}\n`);
process.stdout.write(`archive-commit-failure=${archiveCommitFailure.trace.join("|")}\n`);
process.stdout.write(`archive-commit-error=${archiveCommitFailure.error}\n`);
