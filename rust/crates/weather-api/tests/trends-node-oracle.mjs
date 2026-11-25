import assert from "node:assert/strict";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";

const dataAt = "2026-08-22T05:00:00.000Z";
const source = {
  attributionLabel: "Weather data by Open-Meteo",
  attributionUrl: "https://open-meteo.com/",
  latitude: 47.95,
  longitude: -122.42,
  providerKey: "open-meteo",
  providerName: "Open-Meteo",
  siteName: "Ballydidean",
  siteSlug: "ballydidean",
  sourceId: "11",
  sourceKey: "open-meteo-reanalysis-v1",
  sourceKind: "reanalysis",
  stationKind: "virtual",
  stationLatitude: 47.95,
  stationLongitude: -122.42,
  stationName: "Open-Meteo Virtual",
  stationSlug: "open-meteo-virtual",
  timezone: "America/Los_Angeles",
};
const rows = [
  {
    apparentTemperatureC: 10.1,
    pm25MicrogramsPerCubicMeter: null,
    precipitationMm: 1.2,
    pressureHpa: 1012.4,
    relativeHumidityPercent: 82,
    temperatureC: 11.5,
    temperatureMaximumC: 14.5,
    temperatureMinimumC: 8.2,
    validAt: new Date("2019-01-01T08:00:00.000Z"),
    windDirectionDegrees: -135,
    windGustMps: 7.2,
    windSpeedMps: 4.1,
  },
  {
    apparentTemperatureC: Number.POSITIVE_INFINITY,
    pm25MicrogramsPerCubicMeter: 12.3,
    precipitationMm: 0,
    pressureHpa: 1014.2,
    relativeHumidityPercent: 78,
    temperatureC: 16.2,
    temperatureMaximumC: 19.4,
    temperatureMinimumC: 11.1,
    validAt: new Date("2026-08-21T07:00:00.000Z"),
    windDirectionDegrees: -90,
    windGustMps: 6.2,
    windSpeedMps: 3.1,
  },
];
const queries = [];
const handler = createWeatherApi({
  // expose the active site calendar metadata
  async listSites() {
    return [source];
  },
  // capture the fixed calendar query and return ordered buckets
  async listTrends(siteSlug, from, to) {
    queries.push({ from, siteSlug, to });
    return rows;
  },
}, {
  dataAt,
  now: () => new Date(dataAt),
});

// verify the parameter-free calendar route
const response = await handler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/trends",
));
const body = await response.json();
assert.equal(response.status, 200);
assert.deepEqual(queries[0], {
  from: "2019-01-01T08:00:00.000Z",
  siteSlug: "ballydidean",
  to: dataAt,
});
assert.deepEqual(body.data.map(({ validAt }) => validAt), [
  "2019-01-01T08:00:00.000Z",
  "2026-08-21T07:00:00.000Z",
]);
assert.deepEqual(body.data[0].metrics, {
  apparentTemperatureC: 10.1,
  pm25MicrogramsPerCubicMeter: null,
  precipitationMm: 1.2,
  pressureHpa: 1012.4,
  relativeHumidityPercent: 82,
  temperatureC: 11.5,
  temperatureMaximumC: 14.5,
  temperatureMinimumC: 8.2,
  windDirectionDegrees: -135,
  windGustMps: 7.2,
  windSpeedMps: 4.1,
});
assert.equal(body.data[1].metrics.apparentTemperatureC, null);
assert.equal(body.generatedAt, dataAt);

// preserve every accepted compatibility selector without changing the query
for (const range of ["24h", "7d", "30d", ""]) {
  const legacy = await handler(new Request(
    `http://weather.test/api/v1/sites/ballydidean/trends?range=${range}`,
  ));
  assert.equal(legacy.status, 200);
}
assert.deepEqual(queries.slice(1), Array.from({ length: 4 }, () => queries[0]));

// reject invalid, duplicate, station, and source filters
for (const query of [
  "range=90d",
  "range=24h&range=7d",
  "station=open-meteo-virtual",
  "source=11",
]) {
  const invalid = await handler(new Request(
    `http://weather.test/api/v1/sites/ballydidean/trends?${query}`,
  ));
  assert.equal(invalid.status, 400);
}

process.stdout.write("trends calendar query and projection Node oracle passed\n");
