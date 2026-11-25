import assert from "node:assert/strict";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";

const dataAt = "2026-08-22T05:00:00.000Z";
const source = {
  attributionLabel: "Open-Meteo historical weather data",
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
  stationName: "Open-Meteo virtual station",
  stationSlug: "open-meteo-virtual",
  timezone: "America/Los_Angeles",
};

// build one representative repository history row
function record(id, validAt) {
  return {
    adapterVersion: "open-meteo-reanalysis/v1",
    apparentTemperatureC: 15.5,
    blackGlobeTemperatureC: null,
    cloudCoverPercent: 42,
    contentHash: `hash-${id}`,
    contractEpoch: null,
    deviceModel: null,
    deviceSerial: null,
    deviceVendor: null,
    firstReceivedAt: validAt,
    id,
    lastReceivedAt: "2026-08-22T04:51:00.000Z",
    pm25MicrogramsPerCubicMeter: null,
    precipitationMm: 0.2,
    precipitationRateMmPerHour: null,
    pressureHpa: 1014.2,
    productRunAt: null,
    providerKey: "ignored-row-provider",
    providerMetadata: {
      dataset: "era5",
      elevation_m: 17,
      grid_cell: "47.95,-122.42",
      private_request_id: "hidden",
    },
    qualityMetadata: null,
    relativeHumidityPercent: 78,
    revisionCount: 0,
    siteSlug: "ballydidean",
    sourceConfigFingerprint: null,
    sourceId: "11",
    sourceKey: "ignored-row-source",
    sourceKind: "reanalysis",
    stationSlug: "open-meteo-virtual",
    soilElectricalConductivityMicrosiemensPerCm: null,
    soilMoisturePercent: null,
    solarRadiationWm2: 320,
    temperatureC: 16.2,
    upstreamModel: "era5",
    upstreamTimezone: "UTC",
    uvIndex: null,
    validAt,
    waterLevelM: null,
    windDirectionDegrees: 225,
    windGustMps: null,
    windSpeedMps: 4.1,
    wetBulbGlobeTemperatureC: null,
  };
}

const rows = [
  record("103", "2026-08-21T03:00:00.000Z"),
  record("102", "2026-08-21T02:00:00.000Z"),
  record("101", "2026-08-21T01:00:00.000Z"),
];
const queries = [];
const handler = createWeatherApi({
  // return the ordered page plus one lookahead row
  async listHistory(query) {
    queries.push(query);
    return rows;
  },
  // expose one active historical source
  async listSites() {
    return [source];
  },
}, {
  dataAt,
  now: () => new Date(dataAt),
});

const response = await handler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/history?sourceKind=reanalysis&station=open-meteo-virtual&source=11&from=2026-08-01T00%3A00%3A00Z&to=2026-08-22T00%3A00%3A00Z&limit=2",
));
const body = await response.json();
assert.equal(response.status, 200);
assert.deepEqual(queries[0], {
  from: "2026-08-01T00:00:00.000Z",
  limit: 3,
  siteSlug: "ballydidean",
  sourceId: "11",
  sourceKind: "reanalysis",
  stationSlug: "open-meteo-virtual",
  to: "2026-08-22T00:00:00.000Z",
});
assert.equal(body.data.length, 2);
assert.deepEqual(body.data.map(({ id }) => id), ["103", "102"]);
assert.equal(body.data[0].metadata.device, null);
assert.deepEqual(body.data[0].metadata.provider, {
  dataset: "era5",
  elevationM: 17,
  gridCell: "47.95,-122.42",
  propertySensors: null,
});
assert.equal(body.data[0].productRunAt, null);
assert.equal(body.data[0].provenance.sourceKey, "open-meteo-reanalysis-v1");
assert.equal(body.page.limit, 2);
assert.equal(typeof body.page.nextCursor, "string");
const decoded = JSON.parse(Buffer.from(body.page.nextCursor, "base64url").toString("utf8"));
assert.deepEqual(decoded, {
  id: "102",
  validAt: "2026-08-21T02:00:00.000Z",
});

const continued = await handler(new Request(
  `http://weather.test/api/v1/sites/ballydidean/history?cursor=${body.page.nextCursor}&limit=2`,
));
assert.equal(continued.status, 200);
assert.deepEqual(queries[1].cursor, decoded);
assert.equal(queries[1].limit, 3);

const invalidSelection = await handler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/history?source=not-positive&station=Bad",
));
const invalidSelectionBody = await invalidSelection.json();
assert.equal(invalidSelection.status, 400);
assert.deepEqual(invalidSelectionBody, {
  error: {
    code: "invalid_query",
    message: "source must be a positive integer",
  },
});

process.stdout.write("history query and projection Node oracle passed\n");
