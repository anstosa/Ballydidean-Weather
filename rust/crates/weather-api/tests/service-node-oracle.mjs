import assert from "node:assert/strict";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";

const dataAt = "2026-08-22T05:00:00.000Z";
const siteRows = [
  {
    attributionLabel: "Weather data by Open-Meteo",
    attributionUrl: "https://open-meteo.com/",
    latitude: 47.95,
    longitude: -122.42,
    providerKey: "open-meteo",
    providerName: "Open-Meteo",
    siteName: "Ballydidean",
    siteSlug: "ballydidean",
    sourceId: "11",
    sourceKey: "open-meteo-current",
    sourceKind: "model_current",
    stationKind: "virtual",
    stationLatitude: 47.95,
    stationLongitude: -122.42,
    stationName: "Open-Meteo Virtual",
    stationSlug: "open-meteo-virtual",
    timezone: "America/Los_Angeles",
  },
  {
    attributionLabel: "NOAA Tides & Currents",
    attributionUrl: "https://tidesandcurrents.noaa.gov/",
    latitude: 47.95,
    longitude: -122.42,
    providerKey: "noaa-co-ops",
    providerName: "NOAA CO-OPS",
    siteName: "Ballydidean",
    siteSlug: "ballydidean",
    sourceId: "42",
    sourceKey: "glendale-tide-predictions",
    sourceKind: "tide_prediction",
    stationKind: "virtual",
    stationLatitude: 48.12,
    stationLongitude: -122.58,
    stationName: "Glendale Tide Predictions",
    stationSlug: "glendale-tide-predictions",
    timezone: "America/Los_Angeles",
  },
];
let clockReads = 0;
let siteReads = 0;
let failHistory = false;
const currentReads = [];
const historyReads = [];
const tideReads = [];
const handler = createWeatherApi({
  // capture one selected current read
  async getCurrent(siteSlug, query) {
    currentReads.push({ query, siteSlug });
    return [];
  },
  // capture history reads and expose one ordinary repository failure
  async listHistory(query) {
    historyReads.push(query);

    // fail only the final explicit repository-error case
    if (failHistory) {
      throw new Error("database unavailable");
    }
    return [];
  },
  // count active-site discovery calls
  async listSites() {
    siteReads += 1;
    return siteRows;
  },
  // return one projected tide row after recording its resolved range
  async listTides(siteSlug, from, to) {
    tideReads.push({ from, siteSlug, to });
    return [{
      attributionLabel: "NOAA Tides & Currents",
      attributionUrl: "https://tidesandcurrents.noaa.gov/",
      predictionType: "H",
      providerKey: "noaa-co-ops",
      sourceId: "42",
      sourceKind: "tide_prediction",
      stationName: "Glendale Tide Predictions",
      stationSlug: "glendale-tide-predictions",
      validAt: "2026-08-22T11:27:00.000Z",
      waterLevelM: 3.271,
    }];
  },
}, {
  dataAt,
  // count request clocks after the two startup snapshot reads
  now() {
    clockReads += 1;
    return new Date(dataAt);
  },
});
const startupClockReads = clockReads;

// issue one decoded HTTP fixture through the compiled handler
async function request(method, path) {
  return await handler(new Request(`http://weather.test${path}`, { method }));
}

const sites = await request("GET", "/api/v1/sites");
const sitesBody = await sites.json();
const current = await request(
  "GET",
  "/api/v1/sites/ballydidean/current?source=11",
);
const currentBody = await current.json();
const tides = await request("HEAD", "/api/v1/sites/ballydidean/tides");
const tideBodyLength = (await tides.arrayBuffer()).byteLength;
const history = await request(
  "GET",
  "/api/v1/sites/ballydidean/history?limit=1",
);
const historyBody = await history.json();
const method = await request("POST", "/api/v1/sites/ballydidean/current");
const methodBody = await method.json();
const missingRoute = await request("GET", "/api/v1/missing");
const missingRouteBody = await missingRoute.json();
const missingSite = await request("GET", "/api/v1/sites/elsewhere/current");
const missingSiteBody = await missingSite.json();
const invalidQuery = await request(
  "GET",
  "/api/v1/sites/ballydidean/current?unsupported=true",
);
const invalidQueryBody = await invalidQuery.json();
failHistory = true;
const databaseFailure = await request(
  "GET",
  "/api/v1/sites/ballydidean/history?limit=1",
);
const databaseFailureBody = await databaseFailure.json();

assert.deepEqual(
  [
    sites.status,
    current.status,
    tides.status,
    history.status,
    method.status,
    missingRoute.status,
    missingSite.status,
    invalidQuery.status,
    databaseFailure.status,
  ],
  [200, 200, 200, 200, 405, 404, 404, 400, 500],
);
assert.equal(sitesBody.data[0].stations[0].sources[0].id, "11");
assert.equal(currentBody.generatedAt, dataAt);
assert.deepEqual(historyBody.page, { limit: 1, nextCursor: null });
assert.equal(tideBodyLength, 0);
assert.deepEqual(currentReads, [{
  query: { asOf: dataAt, sourceId: "11" },
  siteSlug: "ballydidean",
}]);
assert.deepEqual(historyReads.map(({ limit }) => limit), [2, 2]);
assert.deepEqual(tideReads, [{
  from: "2026-08-21T05:00:00.000Z",
  siteSlug: "ballydidean",
  to: "2026-09-01T05:00:00.000Z",
}]);
assert.deepEqual([
  methodBody.error.code,
  missingRouteBody.error.code,
  missingSiteBody.error.code,
  invalidQueryBody.error.code,
  databaseFailureBody.error.code,
], [
  "method_not_allowed",
  "not_found",
  "site_not_found",
  "invalid_query",
  "internal_error",
]);
assert.equal(clockReads - startupClockReads, 8);
assert.equal(siteReads, 7);

const nominalDataAt = "2026-08-20T05:00:00.000Z";
const safetyAt = "2026-08-22T05:00:00.000Z";
let divergentClockReads = 0;
const divergentCurrentReads = [];
const divergentHandler = createWeatherApi({
  // bind the construction-snapshotted nominal data instant
  async getCurrent(siteSlug, query) {
    divergentCurrentReads.push({ query, siteSlug });
    return [];
  },
  // reuse active site metadata
  async listSites() {
    return siteRows;
  },
}, {
  dataAt: nominalDataAt,
  // retain an independent real request safety clock
  now() {
    divergentClockReads += 1;
    return new Date(safetyAt);
  },
});
const divergentStartupReads = divergentClockReads;
const divergent = await divergentHandler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/current?source=11",
));
const divergentBody = await divergent.json();
assert.equal(divergent.status, 200);
assert.equal(divergentClockReads - divergentStartupReads, 1);
assert.equal(divergentBody.generatedAt, nominalDataAt);
assert.deepEqual(divergentCurrentReads, [{
  query: { asOf: nominalDataAt, sourceId: "11" },
  siteSlug: "ballydidean",
}]);

const forecastSource = {
  ...siteRows[0],
  sourceId: "12",
  sourceKey: "open-meteo-forecast-v4",
  sourceKind: "forecast",
  stationName: "Open-Meteo Forecast",
  stationSlug: "open-meteo-forecast",
};
const physicalSource = {
  ...siteRows[0],
  providerKey: "ecowitt-local",
  providerName: "Ecowitt",
  sourceId: "13",
  sourceKey: "ecowitt-rain",
  sourceKind: "physical_sensor",
  stationKind: "physical",
  stationName: "Ballydidean Ecowitt",
  stationSlug: "ballydidean-ecowitt",
};

// build one complete raw forecast fixture
function forecastRow(validAt, id) {
  return {
    adapterVersion: "open-meteo-forecast-daily/v4",
    apparentTemperatureC: 11.7,
    blackGlobeTemperatureC: null,
    cloudCoverPercent: 72,
    contentHash: `sha256-${id}`,
    contractEpoch: "v4",
    deviceModel: null,
    deviceSerial: null,
    deviceVendor: null,
    firstReceivedAt: "2026-08-21T07:00:00.000Z",
    id,
    lastReceivedAt: "2026-08-21T07:00:00.000Z",
    pm25MicrogramsPerCubicMeter: 5,
    precipitationMm: 0.1,
    precipitationRateMmPerHour: 0.1,
    pressureHpa: 1018.1,
    productRunAt: "2026-08-21T07:00:00.000Z",
    providerKey: "open-meteo",
    providerMetadata: null,
    qualityMetadata: null,
    relativeHumidityPercent: 83,
    revisionCount: 0,
    siteSlug: "ballydidean",
    sourceConfigFingerprint: "fingerprint",
    sourceId: "12",
    sourceKey: "open-meteo-forecast-v4",
    sourceKind: "forecast",
    stationSlug: "open-meteo-forecast",
    soilElectricalConductivityMicrosiemensPerCm: null,
    soilMoisturePercent: null,
    solarRadiationWm2: null,
    temperatureC: 12.1,
    upstreamModel: "best_match",
    upstreamTimezone: "GMT",
    uvIndex: 0.2,
    validAt,
    waterLevelM: null,
    wetBulbGlobeTemperatureC: null,
    windDirectionDegrees: 180,
    windGustMps: 3.1,
    windSpeedMps: 1.8,
  };
}

const forecastReads = [];
const dailyReads = [];
const pressureReads = [];
const trendReads = [];
let rainCollectionReads = 0;
let extendedClockReads = 0;
const extendedHandler = createWeatherApi({
  // capture the current local-day nearest-gauge read
  async getDailyPrecipitation(siteSlug, from, to) {
    dailyReads.push({ from, siteSlug, to });
    return {
      accumulationMm: 2.54,
      sourceId: "13",
      stationSlug: "ballydidean-ecowitt",
      validThrough: new Date("2026-08-22T04:59:00.000Z"),
    };
  },
  // capture the expanded daily forecast read
  async getForecast(siteSlug, asOf, hours, productSelection) {
    forecastReads.push({ asOf, hours, productSelection, siteSlug });
    return [
      forecastRow("2026-08-21T04:00:00.000Z", "forecast-context"),
      forecastRow("2026-08-22T04:00:00.000Z", "forecast-public"),
    ];
  },
  // expose optional pressure failure without failing the forecast
  async getForecastPressureContext(siteSlug, asOf) {
    pressureReads.push({ asOf, siteSlug });
    throw new Error("optional pressure unavailable");
  },
  // expose only sanitized aggregate collection evidence
  async getRainCollectionStatus() {
    rainCollectionReads += 1;
    return {
      claims: 12,
      compressedBytes: 4096,
      contractVersion: "rain-prospective-capture/v1",
      failedRequests: 1,
      lastClaimAt: "2026-09-13T01:00:00.000Z",
      lastForecastReceiptAt: "2026-09-13T01:02:00.000Z",
      lastReceiptAt: "2026-09-13T01:01:00.000Z",
      lastStationReceiptAt: "2026-09-13T01:03:00.000Z",
      modelEnabled: false,
      pausedUntil: "2026-09-14T00:00:00.000Z",
      pendingRequests: 2,
      qualificationEnabled: false,
      receipts: 11,
      stationsSeen: 4,
      timelyForecasts: 2,
      unknownRequests: 1,
      validForecasts: 3,
      validStationWindows: 7,
    };
  },
  // expose one active source catalog
  async listSites() {
    return [forecastSource, physicalSource];
  },
  // capture the fixed calendar trend read
  async listTrends(siteSlug, from, to) {
    trendReads.push({ from, siteSlug, to });
    return [{
      apparentTemperatureC: 10.1,
      pm25MicrogramsPerCubicMeter: 12.3,
      precipitationMm: 1.2,
      pressureHpa: 1012.4,
      relativeHumidityPercent: 82,
      temperatureC: 11.5,
      temperatureMaximumC: 19.4,
      temperatureMinimumC: 8.2,
      validAt: new Date("2026-08-21T07:00:00.000Z"),
      windDirectionDegrees: -135,
      windGustMps: 7.2,
      windSpeedMps: 4.1,
    }];
  },
}, {
  dataAt,
  forecastAdjustment: {
    loadedAt: "2026-08-20T05:00:00.000Z",
    runtime: { bundle: null, reasonCode: "registry_inactive", state: "disabled" },
  },
  temperatureAdjustment: {
    loadedAt: "2026-08-20T06:00:00.000Z",
    runtime: { bundle: null, reasonCode: "registry_inactive", state: "disabled" },
  },
  // retain an independent per-request safety clock
  now() {
    extendedClockReads += 1;
    return new Date(dataAt);
  },
});
const daily = await extendedHandler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/daily-precipitation",
));
const dailyBody = await daily.json();
const rainCollection = await extendedHandler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/rain-collection",
));
const rainCollectionBody = await rainCollection.json();
const rainCollectionHead = await extendedHandler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/rain-collection",
  { method: "HEAD" },
));
const forecast = await extendedHandler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/forecast",
));
const forecastBody = await forecast.json();
const trends = await extendedHandler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/trends?range=7d",
  { method: "HEAD" },
));
assert.equal(daily.status, 200);
assert.equal(rainCollection.status, 200);
assert.equal(rainCollectionHead.status, 200);
assert.equal((await rainCollectionHead.arrayBuffer()).byteLength, 0);
assert.equal(forecast.status, 200);
assert.equal(trends.status, 200);
assert.equal((await trends.arrayBuffer()).byteLength, 0);
assert.equal(extendedClockReads, 5);
assert.equal(rainCollectionReads, 2);
assert.deepEqual(dailyReads, [{
  from: "2026-08-21T07:00:00.000Z",
  siteSlug: "ballydidean",
  to: dataAt,
}]);
assert.deepEqual(dailyBody.data, {
  accumulationMm: 2.54,
  source: { sourceId: "13", stationSlug: "ballydidean-ecowitt" },
  validThrough: "2026-08-22T04:59:00.000Z",
});
assert.equal(dailyBody.generatedAt, dataAt);
assert.deepEqual({
  claims: rainCollectionBody.data.claims,
  compressedBytes: rainCollectionBody.data.compressedBytes,
  expectedStations: rainCollectionBody.data.expectedStations,
  expiresAt: rainCollectionBody.data.expiresAt,
  modelEnabled: rainCollectionBody.data.modelEnabled,
  startsAt: rainCollectionBody.data.startsAt,
  stationAccessAuthorized: rainCollectionBody.data.stationAccessAuthorized,
}, {
  claims: 12,
  compressedBytes: 4096,
  expectedStations: 12,
  expiresAt: "2027-10-08T00:00:00.000Z",
  modelEnabled: false,
  startsAt: "2026-09-13T00:00:00.000Z",
  stationAccessAuthorized: true,
});
assert.equal(rainCollectionBody.generatedAt, dataAt);
assert.equal(rainCollectionBody.version, "development");
assert.deepEqual(forecastReads, [{
  asOf: "2026-08-20T19:00:00.000Z",
  hours: 37,
  productSelection: undefined,
  siteSlug: "ballydidean",
}]);
assert.deepEqual(pressureReads, [{
  asOf: "2026-08-21T07:00:00.000Z",
  siteSlug: "ballydidean",
}]);
assert.deepEqual(trendReads, [{
  from: "2019-01-01T08:00:00.000Z",
  siteSlug: "ballydidean",
  to: dataAt,
}]);
assert.deepEqual(forecastBody.data.map(({ id }) => id), ["forecast-public"]);
assert.deepEqual(
  forecastBody.airQualityContext.map(({ id }) => id),
  ["forecast-context"],
);
assert.deepEqual(forecastBody.pressureContext, []);
assert.equal(forecastBody.adjustmentRuntime.loadedAt, "2026-08-20T05:00:00.000Z");
assert.equal(
  forecastBody.temperatureAdjustmentRuntime.loadedAt,
  "2026-08-20T06:00:00.000Z",
);
assert.equal(forecastBody.rainAdjustmentRuntime.loadedAt, dataAt);
assert.deepEqual(forecastBody.data[0].adjustment, {
  adjustedMetrics: {},
  appliedMetrics: [],
  contractVersion: "forecast-adjustment-decision/v1",
  reasonCode: "registry_inactive",
  state: "disabled",
});

let healthClockReads = 0;
let healthReads = 0;
let healthSiteReads = 0;
const inactiveAdjustment = {
  loadedAt: "2026-08-20T05:00:00.000Z",
  runtime: { bundle: null, reasonCode: "registry_inactive", state: "disabled" },
};
const inactiveTemperature = {
  loadedAt: "2026-08-20T06:00:00.000Z",
  runtime: { bundle: null, reasonCode: "registry_inactive", state: "disabled" },
};
const healthHandler = createWeatherApi({
  // expose one ready repository snapshot
  async getHealth() {
    healthReads += 1;
    return {
      database: "ready",
      migration: { status: "current", version: "2026082201" },
      workerLastLoopAt: "2026-08-22T04:50:00.000Z",
    };
  },
  // prove health never performs active-site discovery
  async listSites() {
    healthSiteReads += 1;
    return siteRows;
  },
}, {
  dataAt,
  forecastAdjustment: inactiveAdjustment,
  temperatureAdjustment: inactiveTemperature,
  version: "release-2026",
  // retain one independent safety read per request
  now() {
    healthClockReads += 1;
    return new Date(dataAt);
  },
});
const health = await healthHandler(new Request("http://weather.test/api/v1/health"));
const healthBody = await health.json();
const invalidHealth = await healthHandler(new Request(
  "http://weather.test/api/v1/health?unexpected=true",
));
const invalidHealthBody = await invalidHealth.json();
assert.equal(health.status, 200);
assert.equal(invalidHealth.status, 400);
assert.equal(invalidHealthBody.error.code, "invalid_query");
assert.equal(healthReads, 1);
assert.equal(healthSiteReads, 0);
assert.equal(healthClockReads, 2);
assert.deepEqual({
  database: healthBody.data.database,
  freshness: healthBody.data.worker.freshness,
  migration: healthBody.data.migration,
  ready: healthBody.data.ready,
  version: healthBody.data.version,
}, {
  database: "ready",
  freshness: "fresh",
  migration: { status: "current", version: "2026082201" },
  ready: true,
  version: "release-2026",
});
assert.equal(healthBody.data.adjustmentRuntime.loadedAt, inactiveAdjustment.loadedAt);
assert.equal(
  healthBody.data.temperatureAdjustmentRuntime.loadedAt,
  inactiveTemperature.loadedAt,
);

let failedHealthClockReads = 0;
const failedHealthHandler = createWeatherApi({
  // expose an ordinary health repository failure
  async getHealth() {
    throw new Error("health unavailable");
  },
  // fail loudly if health incorrectly discovers sites
  async listSites() {
    throw new Error("health must not read sites");
  },
}, {
  dataAt,
  forecastAdjustment: inactiveAdjustment,
  temperatureAdjustment: inactiveTemperature,
  now() {
    failedHealthClockReads += 1;
    return new Date(dataAt);
  },
});
const failedHealth = await failedHealthHandler(new Request(
  "http://weather.test/api/v1/health",
  { method: "HEAD" },
));
assert.equal(failedHealth.status, 503);
assert.equal((await failedHealth.arrayBuffer()).byteLength, 0);
assert.equal(failedHealthClockReads, 1);

process.stdout.write("statuses=200|200|200|200|405|404|404|400|500\n");
process.stdout.write(`clock-reads=${String(clockReads - startupClockReads)}\n`);
process.stdout.write(`site-reads=${String(siteReads)}\n`);
process.stdout.write("current=11|2026-08-22T05:00:00.000Z\n");
process.stdout.write("history-limits=2|2\n");
process.stdout.write("head-bytes=0\n");
process.stdout.write("codes=method_not_allowed|not_found|site_not_found|invalid_query|internal_error\n");
process.stdout.write("divergent=2026-08-20T05:00:00.000Z|2026-08-22T05:00:00.000Z|1\n");
process.stdout.write("extended=daily|rain-collection|forecast|trends|pressure-fail-raw|head\n");
process.stdout.write("health=ready|unavailable|no-sites|head\n");
process.stdout.write("service dispatch Node oracle passed\n");
