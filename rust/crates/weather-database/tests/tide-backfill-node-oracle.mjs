import {
  completeBackfillIngestion,
  failIngestionRun,
} from "../../../../packages/database/dist/repositories.js";
import { normalizeNoaaTidePayload } from "../../../../packages/providers/dist/index.js";

const sourceId = "source-tide";
const fingerprint = "a".repeat(64);
const identity = {
  adapterVersion: "noaa-water-level/v1",
  chunkPlanVersion: "noaa-calendar-range/v1",
  intervalEndExclusive: "2026-09-02T00:00:00.000Z",
  intervalStart: "2026-09-01T00:00:00.000Z",
  requestedFromDate: "2026-09-01",
  requestedToDate: "2026-09-01",
  sourceConfigFingerprint: fingerprint,
  sourceId,
};
const records = normalizeNoaaTidePayload(
  { data: [{ f: "0,0,0,0", q: "v", t: "2026-09-01 00:06", v: "1.234" }] },
  {
    datum: "MLLW",
    endExclusive: identity.intervalEndExclusive,
    product: "water_level",
    sourceId,
    sourceKind: "tide_observation",
    start: identity.intervalStart,
    stationId: "9444900",
    timezone: "America/Los_Angeles",
  },
  "2026-09-02T00:01:00.000Z",
);

// execute the compiled repository with an exact fake PostgreSQL client
async function run(recordSourceId = sourceId) {
  const trace = [];
  const client = {
    query: async (sql, binds = []) => {
      const compact = sql.trim().replace(/\s+/gu, " ");

      // retain transaction commands in order
      if (["BEGIN", "COMMIT", "ROLLBACK"].includes(compact)) {
        trace.push(compact.toLowerCase());
        return { rowCount: null, rows: [] };
      }

      // classify each exact lifecycle statement
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
  };
  const session = { client, sourceId };

  try {
    await completeBackfillIngestion(session, {
      attempts: 2,
      identity,
      records: records.map((record) => ({ ...record, sourceId: recordSourceId })),
      responseMetadata: {
        datum: "MLLW",
        normalized_record_count: 1,
        product: "water_level",
        provider: "noaa-co-ops",
        station_id: "9444900",
      },
      runId: "run-1",
      upstreamResponseChecksum: "c".repeat(64),
    });
    return { error: null, trace };
  } catch (error) {
    return { error: error instanceof Error ? error.message : String(error), trace };
  }
}

const success = await run();
const mismatch = await run("source-other");

// execute the compiled durable failed-chunk transaction
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

        // classify the exact failed lifecycle statements
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
      message: "NOAA unavailable",
    },
    runId: "run-1",
  });
  return trace;
}

const failure = await runFailure();
process.stdout.write(`success=${success.trace.join("|")}\n`);
process.stdout.write(`source-mismatch=${mismatch.trace.join("|")}\n`);
process.stdout.write(`source-mismatch-error=${mismatch.error}\n`);
process.stdout.write(`failure=${failure.join("|")}\n`);
