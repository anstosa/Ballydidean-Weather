import assert from "node:assert/strict";

import {
  SourceSession,
  completeScheduledIngestion,
} from "../../../../packages/database/dist/index.js";

const sourceId = "source-1";
const runId = "run-1";
const windowStart = "2026-11-23T10:00:00.000Z";
const windowEndExclusive = "2026-11-23T11:00:00.000Z";
const validAt = [
  "2026-11-23T10:00:00.000Z",
  "2026-11-23T10:05:00.000Z",
  "2026-11-23T10:10:00.000Z",
];

// build one canonical compiled-domain record
function weatherRecord(index) {
  return {
    metadata: {
      device: null,
      model: null,
      provider: null,
      quality: null,
      upstreamTimezone: "UTC",
    },
    metrics: {
      apparentTemperatureC: null,
      blackGlobeTemperatureC: null,
      cloudCoverPercent: null,
      pm25MicrogramsPerCubicMeter: null,
      precipitationMm: null,
      precipitationRateMmPerHour: null,
      pressureHpa: null,
      relativeHumidityPercent: null,
      soilElectricalConductivityMicrosiemensPerCm: null,
      soilMoisturePercent: null,
      solarRadiationWm2: null,
      temperatureC: index,
      uvIndex: null,
      waterLevelM: null,
      windDirectionDegrees: null,
      windGustMps: null,
      windSpeedMps: null,
      wetBulbGlobeTemperatureC: null,
    },
    productRunAt: index === 1 ? "2026-11-23T06:00:00.000Z" : null,
    receivedAt: "2026-11-23T10:15:00.000Z",
    sourceId,
    sourceKind: "forecast",
    validAt: validAt[index],
  };
}

// create one no-host client that exposes only archive-relevant behavior
function fixtureClient(mode) {
  const trace = [];
  let releaseCount = 0;

  return {
    trace,
    // emulate repository statements while retaining archive order
    async query(text, values = []) {
      // preserve transaction commands without adding database behavior
      if (text === "BEGIN" || text === "ROLLBACK") {
        return { rowCount: null, rows: [] };
      }

      // expose commit before any cold publication
      if (text === "COMMIT") {
        trace.push("commit");

        // preserve the caller commit failure boundary
        if (mode === "commit-failure") {
          throw new Error("commit failed");
        }

        return { rowCount: null, rows: [] };
      }

      // select exact existing identities from the adapter-produced request JSON
      if (text.includes("jsonb_to_recordset")) {
        const requested = JSON.parse(values[1]);
        trace.push(`select:${requested.length}`);
        assert.equal(values[0], sourceId);

        // make every serving value unchanged for the retry fixture
        if (mode === "unchanged") {
          return {
            rowCount: requested.length,
            rows: requested.map((request) => ({ ...request })),
          };
        }

        return {
          rowCount: 2,
          rows: [
            { ...requested[0] },
            { ...requested[1], contentHash: "0".repeat(64) },
          ],
        };
      }

      // load the same locked source lineage used by the archiver
      if (text.includes('run.adapter_version AS "adapterVersion"')) {
        assert.deepEqual(values, [runId, sourceId]);

        // preserve the source requireRow failure
        if (mode === "missing-context") {
          trace.push("context:none");
          return { rowCount: 0, rows: [] };
        }

        trace.push("context");
        return {
          rowCount: 1,
          rows: [{
            adapterVersion: "adapter/v1",
            logicalReceivedAt: new Date("2026-11-23T10:14:00.000Z"),
            providerKey: "provider-1",
            sourceConfigFingerprint: "fingerprint-1",
            sourceKey: "source-key-1",
          }],
        };
      }

      // preserve exact archive savepoint creation
      if (text === "SAVEPOINT weather_revision_archive") {
        trace.push("savepoint");
        return { rowCount: null, rows: [] };
      }

      // expose archive cleanup rollback failures
      if (text === "ROLLBACK TO SAVEPOINT weather_revision_archive") {
        // retain the ordinary database failure
        if (mode === "rollback-failure") {
          trace.push("rollback-to!");
          throw new Error("savepoint rollback failed");
        }

        trace.push("rollback-to");
        return { rowCount: null, rows: [] };
      }

      // preserve release both on the success and recovery paths
      if (text === "RELEASE SAVEPOINT weather_revision_archive") {
        releaseCount += 1;

        // fail only the first release so recovery can complete
        if (mode === "release-failure" && releaseCount === 1) {
          trace.push("release!");
          throw new Error("savepoint release failed");
        }

        trace.push("release");
        return { rowCount: null, rows: [] };
      }

      // admit all non-archive scheduled transaction statements
      if (text.includes("SELECT 1") && text.includes("mode = 'scheduled'")) {
        return { rowCount: 1, rows: [{ "?column?": 1 }] };
      }
      // accept the serving record write outside this helper's scope
      if (text.includes("INSERT INTO weather_records")) {
        return { rowCount: 3, rows: [] };
      }
      // accept the scheduled checkpoint write outside this helper's scope
      if (text.includes("UPDATE ingestion_checkpoints")) {
        return { rowCount: 1, rows: [] };
      }
      // accept the scheduled finalization write outside this helper's scope
      if (text.includes("UPDATE ingestion_runs") && text.includes("state = 'succeeded'")) {
        return { rowCount: 1, rows: [] };
      }

      throw new Error(`unexpected revision archive SQL: ${text}`);
    },
  };
}

// execute one compiled repository archive mode
async function executeMode(mode) {
  const client = fixtureClient(mode);
  const session = new SourceSession(client, sourceId);
  const input = {
    attempts: 1,
    expectedCheckpointVersion: 7,
    lastValidAt: validAt[2],
    providerCursor: null,
    records: [weatherRecord(0), weatherRecord(1), weatherRecord(2)],
    responseMetadata: null,
    runId,
    upstreamResponseChecksum: null,
    windowEndExclusive,
    windowStart,
  };

  // exercise the source's optional archive boundary
  if (mode !== "disabled") {
    input.revisionArchiver = async (queryable, revisions) => {
      assert.equal(queryable, client);
      assert.deepEqual(
        revisions.map((revision) => revision.record.validAt),
        [validAt[1], validAt[2]],
      );
      assert.equal(revisions[0].logicalReceivedAt, "2026-11-23T10:14:00.000Z");

      // retain one ordinary stage failure inside the savepoint
      if (mode === "stage-failure" || mode === "rollback-failure") {
        client.trace.push("stage!");
        throw new Error("stage failed");
      }

      client.trace.push(`stage:${revisions.map((revision) => revision.record.validAt).join(",")}`);
      return {
        // expose publication order and failure swallowing
        async publish() {
          client.trace.push("publish");

          // preserve successful ingestion after cold publication failure
          if (mode === "publish-failure") {
            throw new Error("publish failed");
          }
        },
      };
    };
  }

  // return ordinary failure text with the archive-only trace
  try {
    await completeScheduledIngestion(session, input);
    return `error=null;trace=${client.trace.join("|")}`;
  } catch (error) {
    return `error=${error.message};trace=${client.trace.join("|")}`;
  }
}

const modes = [
  "disabled",
  "unchanged",
  "success",
  "stage-failure",
  "release-failure",
  "publish-failure",
  "commit-failure",
  "missing-context",
  "rollback-failure",
];

// emit one stable semantic signature per compiled repository path
for (const mode of modes) {
  process.stdout.write(`${mode}=${await executeMode(mode)}\n`);
}
