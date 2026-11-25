import assert from "node:assert/strict";

import {
  asProviderFailure,
  normalizeNetatmoPayloads,
  normalizePurpleAirCsv,
  normalizeWeatherUndergroundPayloads,
} from "../../../../packages/providers/dist/index.js";

const start = "2026-08-24T13:00:00.000Z";
const endExclusive = "2026-08-24T14:00:00.000Z";
const receivedAt = "2026-08-24T15:00:00.000Z";
const epoch = Date.parse(start) / 1_000;
const shared = { start, endExclusive, sourceId: "source-fixture", timezone: "UTC", model: null, serial: null };
const wu = { ...shared, adapter: "weather-underground", stationId: "KWAREHOUSE1", publicApiKey: "fixture-key" };
const netatmo = {
  ...shared, adapter: "netatmo", deviceId: "aa:bb:cc:dd:ee:ff",
  outdoorModuleId: "11:22:33:44:55:66", rainModuleId: "22:33:44:55:66:77",
  windModuleId: "33:44:55:66:77:88",
};
const purple = { ...shared, adapter: "purpleair", mapVersion: "1.2.3", sensorIndex: 42 };
const header = "time_stamp,sensor_index,humidity_a,humidity_b,pm2.5_atm_a,pm2.5_atm_b,pressure_a,pressure_b,temperature_a,temperature_b";
const empty = { status: "ok", body: null };

// execute the real original decoder/normalizer without provider I/O
function capture(name, normalize, input, payload) {
  let failed = false;
  try {
    normalize(payload, input, receivedAt);
  } catch (error) {
    failed = true;
    // use the exact archive catch conversion including plain domain errors
    const failure = asProviderFailure(error, 1);
    console.log([
      name, failure.attempts, failure.ingestionError.classification,
      failure.ingestionError.code, failure.ingestionError.message,
    ].join("\t"));
  }
  assert(failed, `fixture unexpectedly succeeded: ${name}`);
}

// retain the original index, identity, object, nullable numeric and metric bounds diagnoses
capture("wu-epoch", normalizeWeatherUndergroundPayloads, wu, [{ observations: [{ epoch: 0 }] }]);
capture("wu-identity", normalizeWeatherUndergroundPayloads, wu, [{ observations: [{ epoch, stationID: "other" }] }]);
capture("wu-object", normalizeWeatherUndergroundPayloads, wu, [{ observations: [{ epoch, stationID: wu.stationId }] }]);
capture("wu-qc", normalizeWeatherUndergroundPayloads, wu, [{ observations: [{ epoch, stationID: wu.stationId, metric: {}, qcStatus: "bad" }] }]);
capture("wu-bounds", normalizeWeatherUndergroundPayloads, wu, [{ observations: [{ epoch, stationID: wu.stationId, metric: { tempAvg: 100 } }] }]);
capture("wu-overflow", normalizeWeatherUndergroundPayloads, wu, [{ observations: [{ epoch, stationID: wu.stationId, metric: { pressureMax: 1e308, pressureMin: 1e308 } }] }]);

// retain raw Netatmo key spelling only when the accepted timestamp has an invalid row value
capture("net-root", normalizeNetatmoPayloads, netatmo, [null, empty, empty, empty]);
capture("net-status", normalizeNetatmoPayloads, netatmo, [{ status: "error" }, empty, empty, empty]);
capture("net-body", normalizeNetatmoPayloads, netatmo, [{ status: "ok", body: [] }, { status: "ok", body: "bad" }, empty, empty]);
capture("net-key", normalizeNetatmoPayloads, netatmo, [{ status: "ok", body: { ["0" + epoch]: false } }, empty, empty, empty]);
capture("net-short", normalizeNetatmoPayloads, netatmo, [{ status: "ok", body: { [epoch]: [12] } }, empty, empty, empty]);
capture("net-cell", normalizeNetatmoPayloads, netatmo, [{ status: "ok", body: { [epoch]: ["bad", 50] } }, empty, empty, empty]);

// distinguish the original PurpleAir schema, integer, identity, timestamp and cell diagnoses
capture("pa-empty", normalizePurpleAirCsv, purple, "");
capture("pa-header", normalizePurpleAirCsv, purple, "bad");
capture("pa-width", normalizePurpleAirCsv, purple, `${header}\n${start},42`);
capture("pa-sensor", normalizePurpleAirCsv, purple, `${header}\n${start},42.5,50,52,10,12,1000,1002,68,68`);
capture("pa-identity", normalizePurpleAirCsv, purple, `${header}\n${start},-1,50,52,10,12,1000,1002,68,68`);
capture("pa-timestamp", normalizePurpleAirCsv, purple, `${header}\nnot-a-date,42,50,52,10,12,1000,1002,68,68`);
capture("pa-cell", normalizePurpleAirCsv, purple, `${header}\n${start},42,50,52,bad,12,1000,1002,68,68`);
capture("pa-overflow", normalizePurpleAirCsv, purple, `${header}\n${start},42,50,52,10,12,1e308,1e308,68,68`);

// verify extreme but JS-safe epochs are discarded before any row payload checks
if (process.argv.includes("--padding")) {
  const huge = Number.MAX_SAFE_INTEGER;
  const wuRows = normalizeWeatherUndergroundPayloads(
    [{ observations: [{ epoch: huge }] }], wu, receivedAt,
  );
  const netRows = normalizeNetatmoPayloads(
    [{ status: "ok", body: { [huge]: false } }, empty, empty, empty],
    netatmo, receivedAt,
  );
  assert.deepEqual([wuRows.length, netRows.length], [0, 0]);
  console.log(`padding\t${wuRows.length}\t${netRows.length}`);
}
