import {
  completeForecastAnchorBackfillIngestion,
} from "../../../../packages/database/dist/repositories.js";
import {
  createNormalizedForecastAnchorRecord,
} from "../../../../packages/domain/dist/index.js";

const sourceId = "source-1";
const fingerprint = "a".repeat(64);
const identity = {
  adapterVersion: "open-meteo-previous-runs/v1",
  chunkPlanVersion: "open-meteo-previous-runs/v1",
  intervalEndExclusive: "2026-09-03T00:00:00.000Z",
  intervalStart: "2026-09-01T00:00:00.000Z",
  requestedFromDate: "2026-09-01",
  requestedToDate: "2026-09-02",
  sourceConfigFingerprint: fingerprint,
  sourceId,
};
const metrics = {
  apparentTemperatureC: 9,
  blackGlobeTemperatureC: null,
  cloudCoverPercent: 50,
  pm25MicrogramsPerCubicMeter: null,
  precipitationMm: 0,
  precipitationRateMmPerHour: null,
  pressureHpa: 1012,
  relativeHumidityPercent: 75,
  soilElectricalConductivityMicrosiemensPerCm: null,
  soilMoisturePercent: null,
  solarRadiationWm2: null,
  temperatureC: 10,
  uvIndex: null,
  waterLevelM: null,
  windDirectionDegrees: 0,
  windGustMps: 4,
  windSpeedMps: 2,
  wetBulbGlobeTemperatureC: null,
};
const record = createNormalizedForecastAnchorRecord({
  adapterVersion: identity.adapterVersion,
  contractEpoch: "open-meteo-previous-runs-best-match/2026-09",
  dataset: "previous_runs",
  leadHours: 24,
  metadata: {
    device: null,
    model: "best_match",
    provider: { dataset: "previous_runs" },
    quality: null,
    upstreamTimezone: "UTC",
  },
  metrics,
  receivedAt: "2026-09-01T00:10:00.000Z",
  sourceConfigFingerprint: fingerprint,
  sourceId,
  upstreamModel: "best_match",
  validAt: "2026-09-02T00:00:00.000Z",
});

// execute the actual repository with a typed fake driver
async function run(anchorRowCount, recordSourceId = sourceId) {
  const trace = [];
  const client = {
    query: async (sql, binds = []) => {
      const compact = sql.trim().replace(/\s+/gu, " ");

      // retain transaction commands in exact order
      if (["BEGIN", "COMMIT", "ROLLBACK"].includes(compact)) {
        trace.push(compact.toLowerCase());
        return { rowCount: null, rows: [] };
      }

      // classify each exact transaction statement
      if (compact.includes("FROM ingestion_runs") && compact.includes("mode = 'backfill'")) {
        trace.push(`assert:${binds.length}`);
        return { rowCount: 1, rows: [{ value: 1 }] };
      }
      if (compact.includes("INSERT INTO forecast_anchor_records")) {
        trace.push(`records:${binds.length}`);
        return { rowCount: anchorRowCount, rows: [] };
      }
      if (compact.includes("INSERT INTO backfill_chunk_outcomes")) {
        trace.push(`outcome:${binds.length}`);
        return { rowCount: 1, rows: [] };
      }
      if (compact.startsWith("UPDATE ingestion_runs SET state = 'succeeded'")) {
        trace.push(`finalize:${binds.length}`);
        return { rowCount: 1, rows: [] };
      }
      throw new Error(`unexpected query: ${compact}`);
    },
  };
  const session = { client, sourceId };

  try {
    await completeForecastAnchorBackfillIngestion(session, {
      attempts: 2,
      identity,
      records: [{ ...record, sourceId: recordSourceId }],
      responseMetadata: { populated_anchor_count: 1 },
      runId: "run-1",
      upstreamResponseChecksum: "c".repeat(64),
    });
    return { error: null, trace };
  } catch (error) {
    return { error: error.message, trace };
  }
}

const success = await run(1);
const mismatch = await run(0);
const sourceMismatch = await run(1, "source-2");
process.stdout.write(`success=${success.trace.join("|")}\n`);
process.stdout.write(`mismatch=${mismatch.trace.join("|")}\n`);
process.stdout.write(`mismatch-error=${mismatch.error}\n`);
process.stdout.write(`source-mismatch=${sourceMismatch.trace.join("|")}\n`);
process.stdout.write(`source-mismatch-error=${sourceMismatch.error}\n`);
