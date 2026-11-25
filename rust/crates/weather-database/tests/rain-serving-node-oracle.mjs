import assert from "node:assert/strict";

import {
  readRainAdjustmentRun,
  readRainCollectionStatus,
} from "../../../../packages/database/dist/index.js";

const asOf = "2026-09-08T06:00:00.000Z";

// Collapse irrelevant whitespace while retaining every SQL token.
function normalizeSql(text) {
  return text.trim().split(/\s+/u).join(" ");
}

// Build one fake queryable for normal, missing, or contract-mismatch reads.
function fixturePool({ missingRun = false, mismatchedStatus = false } = {}) {
  const calls = [];
  return {
    calls,
    // Return only typed rows owned by the two serving reads.
    async query(text, values = []) {
      const sql = normalizeSql(text);
      if (sql.includes("FROM rain_adjustment_runs")) {
        calls.push(["run", sql, values.join("|")]);
        // Preserve the repository's explicit missing-sidecar result.
        if (missingRun) {
          return { rows: [] };
        }
        return {
          rows: [{
            decision_at: new Date("2026-09-08T02:00:00.000Z"),
            first_received_at: new Date("2026-09-07T18:03:00.000Z"),
            forecast_claim_id: "claim-1",
            generated_at: new Date("2026-09-08T02:05:00.000Z"),
            hours: [
              {
                applied: false,
                correctedPrecipitationMm: 0,
                modelLeadHours: 7,
                rawPrecipitationMm: 0,
                reasonCode: null,
                validAt: "2026-09-08T01:00:00.000Z",
              },
              {
                applied: true,
                correctedPrecipitationMm: 1.25,
                modelLeadHours: 8,
                rawPrecipitationMm: 1.5,
                reasonCode: "hurdle_active",
                validAt: "2026-09-08T02:00:00.000Z",
              },
            ],
            input_sha256: "b".repeat(64),
            model_sha256: "a".repeat(64),
            run_initialized_at: new Date("2026-09-07T18:00:00.000Z"),
          }],
        };
      }
      if (sql === "SELECT * FROM rain_collection_status_v1") {
        calls.push(["status", sql, values.join("|")]);
        return {
          rows: [{
            claims: 14,
            compressed_bytes: "4096",
            contract_version: mismatchedStatus
              ? "rain-prospective-capture/v0"
              : "rain-prospective-capture/v1",
            failed_requests: 2,
            last_claim_at: new Date("2026-09-08T05:00:00.000Z"),
            last_forecast_receipt_at: new Date("2026-09-08T04:00:00.000Z"),
            last_receipt_at: new Date("2026-09-08T05:05:00.000Z"),
            last_station_receipt_at: null,
            model_enabled: false,
            paused_until: null,
            pending_requests: 1,
            qualification_enabled: false,
            receipts: 12,
            stations_seen: 9,
            timely_forecasts: 3,
            unknown_requests: 1,
            valid_forecasts: 4,
            valid_station_windows: 5,
          }],
        };
      }
      throw new Error(`unexpected SQL ${sql}`);
    },
  };
}

const pool = fixturePool();
const run = await readRainAdjustmentRun(pool, "ballydidean", asOf);
const status = await readRainCollectionStatus(pool);
assert.equal(await readRainAdjustmentRun(pool, "elsewhere", asOf), null);
assert.equal(await readRainAdjustmentRun(pool, "ballydidean", "not-a-date"), null);
assert.equal(pool.calls.length, 2);

const missing = fixturePool({ missingRun: true });
assert.equal(await readRainAdjustmentRun(missing, "ballydidean", asOf), null);
const mismatch = fixturePool({ mismatchedStatus: true });
await assert.rejects(readRainCollectionStatus(mismatch), /contract mismatch/u);

console.log(`run-sql=${pool.calls[0][1]}`);
console.log(`run-binds=${pool.calls[0][2]}`);
console.log(`status-sql=${pool.calls[1][1]}`);
console.log(`calls=${pool.calls.map(([kind]) => kind).join(",")}|${missing.calls.length}`);
console.log(`run=${[
  run.runInitializedAt,
  run.modelSha256,
  run.inputSha256,
  run.forecastClaimId,
  run.firstReceivedAt,
  run.decisionAt,
  run.generatedAt,
  run.hours.map((hour) => [
    hour.validAt,
    hour.modelLeadHours,
    hour.rawPrecipitationMm,
    hour.correctedPrecipitationMm,
    hour.applied,
    hour.reasonCode ?? "null",
  ].join("~")).join(","),
].join("|")}`);
console.log(`status=${[
  status.claims,
  status.receipts,
  status.validForecasts,
  status.timelyForecasts,
  status.validStationWindows,
  status.stationsSeen,
  status.failedRequests,
  status.pendingRequests,
  status.unknownRequests,
  status.lastClaimAt,
  status.lastReceiptAt,
  status.lastForecastReceiptAt,
  status.lastStationReceiptAt ?? "null",
  status.pausedUntil ?? "null",
  status.compressedBytes,
].join("|")}`);
console.log(`epochs=${Date.parse(asOf)}`);
