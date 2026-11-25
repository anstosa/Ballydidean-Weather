import assert from "node:assert/strict";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";
import { getForecastPressureContext } from "../../../../packages/database/dist/index.js";

const dataAt = "2026-08-22T05:00:00.000Z";
const asOf = "2026-08-21T07:00:00.000Z";
const captured = [];
const pool = {
  // capture the complete-vintage repository query
  async query(text, values) {
    captured.push({ text, values });
    return { rows: [] };
  },
};
assert.deepEqual(await getForecastPressureContext(pool, {
  asOf,
  siteSlug: "ballydidean",
}), []);
assert.deepEqual(captured[0].values, [
  "ballydidean",
  "2026-08-21T04:00:00.000Z",
  "2026-08-21T10:00:00.000Z",
  "2026-08-19T07:00:00.000Z",
  asOf,
]);

const source = {
  attributionLabel: "Weather data by Open-Meteo",
  attributionUrl: "https://open-meteo.com/",
  latitude: 47.95,
  longitude: -122.42,
  providerKey: "open-meteo",
  providerName: "Open-Meteo",
  siteName: "Ballydidean",
  siteSlug: "ballydidean",
  sourceId: "12",
  sourceKey: "open-meteo-forecast-v4",
  sourceKind: "forecast",
  stationKind: "virtual",
  stationLatitude: 47.95,
  stationLongitude: -122.42,
  stationName: "Open-Meteo Virtual",
  stationSlug: "open-meteo-virtual",
  timezone: "America/Los_Angeles",
};

// build one raw retained forecast row without adjustment output
function contextRow(index) {
  return {
    adapterVersion: null,
    apparentTemperatureC: 8 + index,
    blackGlobeTemperatureC: null,
    cloudCoverPercent: 40 + index,
    contentHash: `context-${String(index)}`,
    contractEpoch: null,
    deviceModel: null,
    deviceSerial: null,
    deviceVendor: null,
    firstReceivedAt: "2026-08-21T06:05:00.000Z",
    id: String(300 + index),
    lastReceivedAt: "2026-08-21T06:05:00.000Z",
    pm25MicrogramsPerCubicMeter: null,
    precipitationMm: 0.1 * index,
    precipitationRateMmPerHour: 0.1 * index,
    pressureHpa: 1010 + index,
    productRunAt: "2026-08-21T06:00:00.000Z",
    providerKey: "open-meteo",
    providerMetadata: null,
    qualityMetadata: null,
    relativeHumidityPercent: 80 - index,
    revisionCount: 0,
    siteSlug: "ballydidean",
    sourceConfigFingerprint: null,
    sourceId: "12",
    sourceKey: "open-meteo-forecast-v4",
    sourceKind: "forecast",
    stationSlug: "open-meteo-virtual",
    soilElectricalConductivityMicrosiemensPerCm: null,
    soilMoisturePercent: null,
    solarRadiationWm2: null,
    temperatureC: 10 + index,
    upstreamModel: "best_match",
    upstreamTimezone: "GMT",
    uvIndex: null,
    validAt: new Date(Date.parse("2026-08-21T04:00:00.000Z") + index * 3_600_000),
    waterLevelM: null,
    wetBulbGlobeTemperatureC: null,
    windDirectionDegrees: 180,
    windGustMps: 3 + index,
    windSpeedMps: 2 + index,
  };
}

const contextRows = Array.from({ length: 6 }, (_value, index) => contextRow(index));
const pressureReads = [];
const handler = createWeatherApi({
  // keep the primary product empty to isolate the context response
  async getForecast() {
    return [];
  },
  // return one complete prior pressure vintage
  async getForecastPressureContext(siteSlug, requestedAsOf) {
    pressureReads.push({ asOf: requestedAsOf, siteSlug });
    return contextRows;
  },
  // expose active forecast metadata
  async listSites() {
    return [source];
  },
}, {
  dataAt,
  now: () => new Date(dataAt),
});
const response = await handler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/forecast",
));
const body = await response.json();
assert.equal(response.status, 200);
assert.deepEqual(pressureReads, [{ asOf, siteSlug: "ballydidean" }]);
assert.equal(body.pressureContext.length, 6);
assert.deepEqual(body.pressureContext.map((record) => ({
  pressureHpa: record.metrics.pressureHpa,
  productRunAt: record.productRunAt,
  validAt: record.validAt,
})), contextRows.map((row) => ({
  pressureHpa: row.pressureHpa,
  productRunAt: row.productRunAt,
  validAt: row.validAt.toISOString(),
})));
assert.deepEqual(body.pressureContext[0].metrics, {
  apparentTemperatureC: 8,
  blackGlobeTemperatureC: null,
  cloudCoverPercent: 40,
  pm25MicrogramsPerCubicMeter: null,
  precipitationMm: 0,
  precipitationRateMmPerHour: 0,
  pressureHpa: 1010,
  relativeHumidityPercent: 80,
  soilElectricalConductivityMicrosiemensPerCm: null,
  soilMoisturePercent: null,
  solarRadiationWm2: null,
  temperatureC: 10,
  uvIndex: null,
  wetBulbGlobeTemperatureC: null,
  windDirectionDegrees: 180,
  windGustMps: 3,
  windSpeedMps: 2,
});
assert.equal("adjustment" in body.pressureContext[0], false);
assert.equal("rainAdjustment" in body.pressureContext[0], false);
assert.equal("temperatureAdjustment" in body.pressureContext[0], false);

// collapse irrelevant formatting while retaining every SQL token
const sql = captured[0].text.trim().split(/\s+/u).join(" ");
process.stdout.write(`sql=${sql}\n`);
process.stdout.write(`binds=${captured[0].values.join("|")}\n`);
process.stdout.write("forecast pressure repository and route Node oracle passed\n");
