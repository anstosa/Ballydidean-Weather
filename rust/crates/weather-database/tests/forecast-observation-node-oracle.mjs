import assert from "node:assert/strict";

import { listForecastObservationInputs } from "../../../../packages/database/dist/index.js";

const asOf = "2026-08-22T05:00:00.000Z";
const captured = [];
const rows = [{
  apparentTemperatureC: 9,
  cloudCoverPercent: 20,
  id: "100",
  lastReceivedAt: new Date("2026-08-22T04:00:00.000Z"),
  pm25MicrogramsPerCubicMeter: null,
  precipitationRateMmPerHour: 0,
  pressureHpa: 1010,
  projectionFrom: new Date("2026-08-21T07:00:00.000Z"),
  providerKey: "ecowitt-local",
  queryFrom: new Date("2026-08-21T04:00:00.000Z"),
  relativeHumidityPercent: 80,
  siteSlug: "ballydidean",
  solarRadiationWm2: 0,
  sourceId: "1",
  sourceKey: "ecowitt-local-live-v1",
  sourceKind: "physical_sensor",
  stationSlug: "ballydidean-ecowitt",
  temperatureC: 10,
  through: new Date(asOf),
  timezone: "America/Los_Angeles",
  uvIndex: 0,
  validAt: new Date("2026-08-22T04:00:00.000Z"),
  windDirectionDegrees: 90,
  windGustMps: 3,
  windSpeedMps: 2,
}];
const pool = {
  // capture the exact fixed-site statement and nominal instant bind
  async query(text, values) {
    captured.push({ text, values });
    return { rowCount: rows.length, rows };
  },
};

const result = await listForecastObservationInputs(pool, asOf);
assert.equal(result.records.length, 1);
assert.equal(result.siteSlug, "ballydidean");
assert.equal(result.timezone, "America/Los_Angeles");
assert.deepEqual(captured[0].values, [asOf]);

// collapse irrelevant formatting while retaining every SQL token
const sql = captured[0].text.trim().split(/\s+/u).join(" ");
process.stdout.write(`sql=${sql}\n`);
process.stdout.write(`binds=${captured[0].values.join("|")}\n`);
process.stdout.write(`input=${result.siteSlug}|${result.timezone}|${result.records[0].id}\n`);
