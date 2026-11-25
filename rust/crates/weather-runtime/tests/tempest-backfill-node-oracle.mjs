import {
  completeBackfillIngestion,
  failIngestionRun,
} from "../../../../packages/database/dist/repositories.js";
import { normalizeTempestObservationPayload } from "../../../../packages/providers/dist/tempest.js";

const sourceId = "source-tempest";
const fingerprint = "a".repeat(64);
const identity = {
  adapterVersion: "tempest-observations-minute/v1",
  chunkPlanVersion: "tempest-observations-one-day-minute/v1",
  intervalEndExclusive: "2026-08-02T00:00:00.000Z",
  intervalStart: "2026-08-01T00:00:00.000Z",
  requestedFromDate: "2026-08-01",
  requestedToDate: "2026-08-01",
  sourceConfigFingerprint: fingerprint,
  sourceId,
};
const records = normalizeTempestObservationPayload(
  {
    device_id: 470937,
    obs: [[
      1785542460,
      1,
      2,
      3,
      360,
      3,
      1012,
      12.5,
      80,
      100,
      1,
      50,
      0.1,
      0,
      null,
      0,
      2.6,
      1,
      null,
      0.1,
      null,
      null,
    ]],
    status: { status_code: 0 },
    type: "obs_st",
  },
  {
    deviceId: 470937,
    endExclusive: identity.intervalEndExclusive,
    locationId: 203055,
    serial: "ST-00198967",
    sourceId,
    start: identity.intervalStart,
    timezone: "America/Los_Angeles",
  },
  "2026-11-07T00:00:00.000Z",
);

// execute one compiled successful transaction against an exact fake client
async function runSuccess() {
  const trace = [];
  const session = {
    sourceId,
    client: {
      query: async (sql, binds = []) => {
        const compact = sql.trim().replace(/\s+/gu, " ");

        // retain transaction commands in order
        if (["BEGIN", "COMMIT", "ROLLBACK"].includes(compact)) {
          trace.push(compact.toLowerCase());
          return { rowCount: null, rows: [] };
        }

        // classify exact lifecycle statements by their stable SQL shape
        if (compact.includes("FROM ingestion_runs") && compact.includes("mode = 'backfill'")) {
          trace.push(`assert:${binds.length}`);
          return { rowCount: 1, rows: [{ value: 1 }] };
        }
        if (compact.includes("INSERT INTO weather_records")) {
          trace.push(`records:${binds.length}`);
          return { rowCount: 1, rows: [] };
        }
        if (compact.includes("INSERT INTO backfill_chunk_outcomes")) {
          trace.push(`outcome:${binds.length}`);
          return { rowCount: 1, rows: [] };
        }
        if (compact.includes("state = 'succeeded'")) {
          trace.push(`finalize:${binds.length}`);
          return { rowCount: 1, rows: [] };
        }
        throw new Error(`unexpected query: ${compact}`);
      },
    },
  };
  await completeBackfillIngestion(session, {
    attempts: 2,
    identity,
    records,
    responseMetadata: {
      bucket_step_minutes: 1,
      device_id: 470937,
      minute_record_count: 1,
      raw_observation_count: 1,
    },
    runId: "run-tempest",
    upstreamResponseChecksum: "c".repeat(64),
  });
  return trace;
}

// execute one compiled durable provider-failure transaction
async function runFailure() {
  const trace = [];
  const session = {
    sourceId,
    client: {
      query: async (sql, binds = []) => {
        const compact = sql.trim().replace(/\s+/gu, " ");

        // retain transaction commands in order
        if (["BEGIN", "COMMIT", "ROLLBACK"].includes(compact)) {
          trace.push(compact.toLowerCase());
          return { rowCount: null, rows: [] };
        }

        // classify exact failed lifecycle statements by stable SQL shape
        if (compact.includes("FROM ingestion_runs") && compact.includes("mode = 'backfill'")) {
          trace.push(`assert:${binds.length}`);
          return { rowCount: 1, rows: [{ value: 1 }] };
        }
        if (compact.includes("INSERT INTO backfill_chunk_outcomes")) {
          trace.push(`outcome:${binds.length}`);
          return { rowCount: 1, rows: [] };
        }
        if (compact.includes("state = 'failed'")) {
          trace.push(`failed:${binds.length}`);
          return { rowCount: 1, rows: [] };
        }
        throw new Error(`unexpected query: ${compact}`);
      },
    },
  };
  await failIngestionRun(session, {
    attempts: 3,
    backfillIdentity: identity,
    error: {
      classification: "retryable",
      code: "provider_unavailable",
      message: "Tempest unavailable",
    },
    runId: "run-tempest",
  });
  return trace;
}

const success = await runSuccess();
const failure = await runFailure();
process.stdout.write(`normalized=${records.length}:${records[0]?.validAt}\n`);
process.stdout.write(`success=${success.join("|")}\n`);
process.stdout.write(`failure=${failure.join("|")}\n`);
