import assert from "node:assert/strict";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  canonicalJsonBytes,
  createForecastAdjustmentTemperatureCanaryAuthorization,
  createForecastAdjustmentTemperatureCanaryRuntimeBundle,
  createForecastAdjustmentRuntimeBundle,
  createForecastAdjustmentRuntimeLoaderForRoot,
  FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1,
  runtimeCalendarFingerprint,
  verifyForecastAdjustmentTemperatureCanaryRuntimeBundle,
  verifyForecastAdjustmentWindCanaryRuntimeBundle,
} from "@weather/forecast-adjustment";

import {
  calendarTrendWindow,
  createDatabaseWeatherReadStore,
  createWeatherApi,
  createWeatherApiServer,
  readApiRelease,
  siteForecastDayWindow,
  siteOvernightForecastWindow,
} from "../dist/index.js";

import { createQualifiedFixture } from "../../../packages/forecast-adjustment/test/evidence-fixtures.mjs";

const inactiveAdjustmentRuntime = {
  activationMode: null,
  activeBundle: null,
  authorizationSha256: null,
  candidateArtifactSha256: null,
  enabledMetrics: [],
  evaluationReportSha256: null,
  expiresAt: null,
  loadedAt: "2026-08-22T05:00:00.000Z",
  qualificationReceiptSha256: null,
  reasonCode: "registry_inactive",
  state: "disabled",
  transferReportSha256: null,
};

const inactiveTemperatureAdjustmentRuntime = {
  actionSha256: null,
  activationMode: null,
  activeBundle: null,
  authorizationSha256: null,
  expiresAt: null,
  fullMemberRootSha256: null,
  loadedAt: "2026-08-22T05:00:00.000Z",
  policyReportSha256: null,
  reasonCode: "registry_inactive",
  source: null,
  state: "disabled",
};

const siteRows = [
  {
    attributionLabel: "Weather data by Open-Meteo",
    attributionUrl: "https://open-meteo.com/",
    latitude: 47.950429954185445,
    longitude: -122.42797012608193,
    providerKey: "open-meteo",
    providerName: "Open-Meteo",
    siteName: "Ballydidean",
    siteSlug: "ballydidean",
    sourceId: "10",
    sourceKey: "open-meteo-current-v1",
    sourceKind: "model_current",
    stationKind: "virtual",
    stationLatitude: 47.950429954185445,
    stationLongitude: -122.42797012608193,
    stationName: "Open-Meteo virtual station",
    stationSlug: "open-meteo-virtual",
    timezone: "America/Los_Angeles",
  },
  {
    attributionLabel: "Weather data by Open-Meteo",
    attributionUrl: "https://open-meteo.com/",
    latitude: 47.950429954185445,
    longitude: -122.42797012608193,
    providerKey: "open-meteo",
    providerName: "Open-Meteo",
    siteName: "Ballydidean",
    siteSlug: "ballydidean",
    sourceId: "11",
    sourceKey: "open-meteo-reanalysis-v1",
    sourceKind: "reanalysis",
    stationKind: "virtual",
    stationLatitude: 47.950429954185445,
    stationLongitude: -122.42797012608193,
    stationName: "Open-Meteo virtual station",
    stationSlug: "open-meteo-virtual",
    timezone: "America/Los_Angeles",
  },
  {
    attributionLabel: "Weather data by Open-Meteo",
    attributionUrl: "https://open-meteo.com/",
    latitude: 47.950429954185445,
    longitude: -122.42797012608193,
    providerKey: "open-meteo",
    providerName: "Open-Meteo",
    siteName: "Ballydidean",
    siteSlug: "ballydidean",
    sourceId: "12",
    sourceKey: "open-meteo-forecast-v4",
    sourceKind: "forecast",
    stationKind: "virtual",
    stationLatitude: 47.950429954185445,
    stationLongitude: -122.42797012608193,
    stationName: "Open-Meteo virtual station",
    stationSlug: "open-meteo-virtual",
    timezone: "America/Los_Angeles",
  },
];

const currentRecord = makeRecord({
  id: "101",
  pressureChange3hHpa: -2.5,
  sourceId: "10",
  sourceKind: "model_current",
  sourceKey: "open-meteo-current-v1",
  validAt: "2026-08-22T04:50:00.000Z",
});
// mirror the first normalized raw v4 fixture record
const forecastRecord = makeRecord({
  apparentTemperatureC: 11.7,
  blackGlobeTemperatureC: null,
  cloudCoverPercent: 72,
  deviceModel: null,
  deviceSerial: null,
  deviceVendor: null,
  id: "201",
  lastReceivedAt: "2026-08-22T05:00:00.000Z",
  pm25MicrogramsPerCubicMeter: 5,
  precipitationMm: 0.1,
  precipitationRateMmPerHour: 0.1,
  pressureChange3hHpa: 3.2,
  pressureHpa: 1018.1,
  productRunAt: "2026-08-22T05:00:00.000Z",
  providerMetadata: {
    dataset: "forecast",
    elevation_m: 32,
    grid_cell: "47.941,-122.438",
  },
  qualityMetadata: null,
  relativeHumidityPercent: 83,
  sourceId: "12",
  sourceKind: "forecast",
  sourceKey: "open-meteo-forecast-v4",
  soilElectricalConductivityMicrosiemensPerCm: null,
  soilMoisturePercent: null,
  solarRadiationWm2: null,
  temperatureC: 12.1,
  upstreamTimezone: "GMT",
  uvIndex: 0.2,
  validAt: "2026-08-22T06:00:00.000Z",
  windDirectionDegrees: 180,
  windGustMps: 3.1,
  windSpeedMps: 1.8,
  wetBulbGlobeTemperatureC: null,
});

// create a deterministic storage record
function makeRecord(overrides = {}) {
  return {
    adapterVersion: "forecast-daily/v4",
    apparentTemperatureC: 15.5,
    blackGlobeTemperatureC: 18.4,
    cloudCoverPercent: 42,
    contractEpoch: FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1.contractEpoch,
    deviceModel: "virtual-grid",
    deviceSerial: null,
    deviceVendor: "Open-Meteo",
    firstReceivedAt: "2026-08-22T04:51:00.000Z",
    id: "100",
    lastReceivedAt: "2026-08-22T04:51:00.000Z",
    pm25MicrogramsPerCubicMeter: 7,
    precipitationMm: 0.2,
    precipitationRateMmPerHour: 0.4,
    pressureHpa: 1014.2,
    productRunAt: null,
    providerKey: "open-meteo",
    providerMetadata: {
      dataset: "era5",
      elevation_m: 17,
      grid_cell: "47.95,-122.43",
      request_id: "private-request-id",
    },
    qualityMetadata: {
      confidence_percent: 93,
      flags: ["interpolated"],
      interpolation: "linear",
      status: "accepted",
    },
    relativeHumidityPercent: 78,
    revisionCount: 0,
    siteSlug: "ballydidean",
    sourceId: "11",
    sourceConfigFingerprint:
      FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1.sourceConfigFingerprint,
    sourceKey: "open-meteo-reanalysis-v1",
    sourceKind: "reanalysis",
    stationSlug: "open-meteo-virtual",
    soilElectricalConductivityMicrosiemensPerCm: 420,
    soilMoisturePercent: 34,
    solarRadiationWm2: 320,
    temperatureC: 16.2,
    upstreamModel: "best_match",
    upstreamTimezone: "America/Los_Angeles",
    uvIndex: 2,
    validAt: "2026-08-21T04:00:00.000Z",
    windDirectionDegrees: 225,
    windGustMps: 7.2,
    windSpeedMps: 4.1,
    wetBulbGlobeTemperatureC: 14.1,
    ...overrides,
  };
}

// create an isolated API fixture
function createFixture(overrides = {}, options = {}) {
  const currentQueries = [];
  const dailyPrecipitationQueries = [];
  const forecastQueries = [];
  const pressureContextQueries = [];
  const historyQueries = [];
  const trendQueries = [];
  const historyRows = [
    makeRecord({ id: "103", validAt: "2026-08-21T03:00:00.000Z" }),
    makeRecord({ id: "102", validAt: "2026-08-21T02:00:00.000Z" }),
    makeRecord({ id: "101", validAt: "2026-08-21T01:00:00.000Z" }),
  ];
  const store = {
    // return today's nearest-gauge accumulation
    async getDailyPrecipitation(siteSlug, from, to) {
      dailyPrecipitationQueries.push({ from, siteSlug, to });
      return {
        accumulationMm: 2.54,
        sourceId: "13",
        stationSlug: "tempest-64255",
        validThrough: "2026-08-22T04:59:00.000Z",
      };
    },
    // return current records
    async getCurrent(siteSlug, filters) {
      currentQueries.push({ filters, siteSlug });
      return [currentRecord];
    },
    // return normalized forecast rows
    async getForecast(siteSlug, asOf, hours, productSelection) {
      forecastQueries.push({
        asOf,
        hours,
        // preserve the existing captured shape for daily requests
        ...(productSelection === undefined ? {} : { productSelection }),
        siteSlug,
      });
      return [forecastRecord];
    },
    // return no optional retained pressure vintage by default
    async getForecastPressureContext(siteSlug, asOf) {
      pressureContextQueries.push({ asOf, siteSlug });
      return [];
    },
    // return safe health state
    async getHealth() {
      return {
        database: "ready",
        migration: { status: "current", version: "0001_initial_weather.sql" },
        workerLastLoopAt: "2026-08-22T04:45:00.000Z",
      };
    },
    // capture history filters
    async listHistory(query) {
      historyQueries.push(query);
      return historyRows;
    },
    // return active metadata
    async listSites() {
      return siteRows;
    },
    // return observed and predicted tide levels
    async listTides() {
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
    // return normalized trend buckets
    async listTrends(siteSlug, from, to) {
      trendQueries.push({ from, siteSlug, to });
      return [{
        apparentTemperatureC: 15.5,
        precipitationMm: 0.2,
        pressureHpa: 1014.2,
        relativeHumidityPercent: 78,
        temperatureC: 16.2,
        temperatureMaximumC: 19.4,
        temperatureMinimumC: 11.1,
        validAt: new Date("2026-08-22T04:00:00.000Z"),
        windDirectionDegrees: -135,
        windGustMps: 7.2,
        windSpeedMps: 4.1,
      }];
    },
    ...overrides,
  };
  const handler = createWeatherApi(store, {
    // freeze response time
    now: () => new Date("2026-08-22T05:00:00.000Z"),
    version: "2026.08.22-1",
    ...options,
  });
  return {
    currentQueries,
    dailyPrecipitationQueries,
    forecastQueries,
    handler,
    historyQueries,
    pressureContextQueries,
    trendQueries,
  };
}

// build one qualified active runtime without external services
async function createActiveForecastRuntime() {
  const directory = await mkdtemp(join(tmpdir(), "weather-api-adjustment-"));
  const triple = await createQualifiedFixture(directory);
  const bundle = createForecastAdjustmentRuntimeBundle(triple);
  return { bundle, reasonCode: null, state: "active" };
}

// build one bounded canary status fixture
function createWindCanaryRuntime() {
  // use a verified immutable v1 bundle for request-time status checks
  const bundle = JSON.parse(readFileSync(new URL(
    "../../../config/forecast-adjustments/ballydidean/wind-canary-bundles/sha256-5e8b2e3932111621af6785a1b16dfd22edc0a2d26059c6e396654c70119abbe1.json",
    import.meta.url,
  ), "utf8"));
  return {
    bundle,
    reasonCode: null,
    state: "active",
  };
}

// load the immutable permanent canary without mocking its hash links
function createPermanentWindCanaryRuntime() {
  const bundle = JSON.parse(readFileSync(new URL(
    "../../../config/forecast-adjustments/ballydidean/wind-canary-bundles/sha256-51f8efd63bef678a7f02d11bdab91405ec48f19808f64d3fe8354036c9b302a2.json",
    import.meta.url,
  ), "utf8"));
  return { bundle, reasonCode: null, state: "active" };
}

// build one active delayed temperature runtime
function createTemperatureCanaryRuntime() {
  const cutoff = "2026-07-25T07:00:00.000Z";
  const directCoefficients = Array(35).fill(0);
  directCoefficients[0] = 1;
  const authorization =
    createForecastAdjustmentTemperatureCanaryAuthorization({
      activatedAt: "2026-08-20T00:00:00.000Z",
      authorizationReason: "operator-approved opt-in temperature test canary",
      authorizedAt: "2026-08-20T00:00:00.000Z",
      authorizedBy: "Ansel",
      expiresAt: "2026-08-30T00:00:00.000Z",
    });
  const bundle = createForecastAdjustmentTemperatureCanaryRuntimeBundle({
    authorization,
    evidence: {
      modelSourceSha256: "1".repeat(64),
      researchSummarySha256: "2".repeat(64),
      retentionManifestSha256: "3".repeat(64),
      strengthSourceSha256: "4".repeat(64),
    },
    model: {
      adaptiveCoefficients: Array(49).fill(0),
      cohort: "ecmwf_single_run_hindcast",
      contractVersion: "temperature-shortlead-models-research/v1",
      directCoefficients,
      learnedStrengthContractVersion:
        "temperature-winner-extensions-research/v1",
      month: "2026-08",
      scope: "assumed_delay6_next12",
      strengthBands: {
        "1-6": { alpha: 1, supported: true, trainingCutoffUtc: cutoff },
        "7-12": { alpha: 1, supported: true, trainingCutoffUtc: cutoff },
      },
      supported: true,
      trainingCutoffUtc: cutoff,
    },
    runtimeFingerprint: runtimeCalendarFingerprint(),
    servedForecastIdentity: {
      adapterVersion: "open-meteo-ecmwf-single-run/v1",
      dataset: "single_run",
      maximumReceiptAgeHours: 12,
      providerKey: "open-meteo",
      sourceDelayHours: 6,
      upstreamModel: "ecmwf_ifs",
    },
  });
  return { bundle, reasonCode: null, state: "active" };
}

// load the immutable cross-month temperature runtime
function createPermanentTemperatureCanaryRuntime() {
  const bundle = JSON.parse(readFileSync(new URL(
    "../../../config/forecast-adjustments/ballydidean/temperature-canary-bundles/sha256-4d4e229b42823e53d2db062ec18c625bb2d2378a8a46d641fa95fabb59501b0e.json",
    import.meta.url,
  ), "utf8"));
  return { bundle, reasonCode: null, state: "active" };
}

// build one single-run sidecar with a direct cold-start state
function createTemperatureSidecar() {
  return {
    hours: [{
      modelLeadHours: 12,
      rawRelativeHumidityPercent: 80,
      rawTemperatureC: 11,
      rawWindSpeedMps: 2,
      validAt: "2026-08-22T06:00:00.000Z",
    }],
    run: {
      adapterVersion: "open-meteo-ecmwf-single-run/v1",
      firstReceivedAt: "2026-08-22T00:05:00.000Z",
      id: "temperature-run",
      modelCycle: "50r1",
      providerResponseSha256: "5".repeat(64),
      recentErrorState: {
        b24C: null,
        b72C: null,
        cohort: "ecmwf_single_run_hindcast",
        localDates: 0,
        mad72C: null,
        maximumSourceRunInitializedAt: null,
        maximumSourceValidAt: null,
        n24: 0,
        n72: 0,
        sourceKeys: [],
        supported: false,
        targetRunInitializedAt: "2026-08-21T18:00:00.000Z",
        windowEndValidAt: "2026-08-21T11:00:00.000Z",
      },
      runInitializedAt: "2026-08-21T18:00:00.000Z",
      stateReason: "cold_start",
      stateStatus: "cold",
      upstreamModel: "ecmwf_ifs",
    },
  };
}

// build one row matching the candidate's immutable v4 identity
function matchingForecastRecord(overrides = {}) {
  const identity = FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1;
  return {
    ...forecastRecord,
    adapterVersion: identity.adapterVersion,
    contractEpoch: identity.contractEpoch,
    providerMetadata: {
      ...forecastRecord.providerMetadata,
      dataset: identity.dataset,
    },
    sourceConfigFingerprint: identity.sourceConfigFingerprint,
    sourceKey: identity.sourceKey,
    upstreamModel: identity.upstreamModel,
    ...overrides,
  };
}

// write one canonical registry pointing at the selected runtime bundle
async function writeRuntimeRegistry(root, bundle, overrides = {}) {
  const activeBundle = {
    bundleSha256: bundle.bundleSha256,
    candidateArtifactSha256: bundle.candidate.candidateArtifactSha256,
    evaluationReportSha256:
      bundle.evaluationReport.evaluationReportSha256,
    path: `bundles/sha256-${bundle.bundleSha256}.json`,
    qualificationReceiptSha256:
      bundle.qualificationReceipt.qualificationReceiptSha256,
    ...overrides,
  };
  await writeFile(
    join(root, "ballydidean.json"),
    canonicalJsonBytes({
      activeBundle,
      contractVersion: "forecast-adjustment-registry/v1",
    }),
  );
}

// write one canonical runtime bundle at its content-addressed path
async function writeRuntimeBundle(root, bundle) {
  const directory = join(root, "ballydidean", "bundles");
  await mkdir(directory, { recursive: true });
  await writeFile(
    join(directory, `sha256-${bundle.bundleSha256}.json`),
    canonicalJsonBytes(bundle),
  );
}

test("only exact versioned GET and HEAD routes are public", async () => {
  const { handler } = createFixture();
  const accepted = [
    "/api/v1/sites",
    "/api/v1/sites/ballydidean/current",
    "/api/v1/sites/ballydidean/daily-precipitation",
    "/api/v1/sites/ballydidean/forecast",
    "/api/v1/sites/ballydidean/history",
    "/api/v1/sites/ballydidean/trends",
    "/api/v1/sites/ballydidean/tides",
    "/api/v1/health",
  ];

  // verify every documented route and read method
  for (const path of accepted) {
    // verify both read methods
    for (const method of ["GET", "HEAD"]) {
      const response = await handler(
        new Request(`http://weather.test${path}`, { method }),
      );
      assert.equal(response.status, 200, `${method} ${path}`);

      // require bodyless head responses
      if (method === "HEAD") {
        assert.equal(await response.text(), "");
      }
    }
  }

  const rejected = [
    "/",
    "/sites",
    "/api/sites",
    "/api/v1",
    "/api/v1/sites/",
    "/api/v1/sites/ballydidean",
    "/api/v1/sites/ballydidean/current/extra",
    "/api/v1/sites/Bad/current",
    "/api/v2/sites",
  ];

  // reject every undocumented route
  for (const path of rejected) {
    const response = await handler(new Request(`http://weather.test${path}`));
    assert.equal(response.status, 404, path);
  }
});

test("private observation input bypasses site discovery and exposes only compact fields", async () => {
  let reads = 0;
  const { handler } = createFixture({
    async getForecastObservationInput(asOf) {
      reads += 1;
      assert.equal(asOf, "2026-08-22T05:00:00.000Z");
      return {
        projectionFrom: "2026-08-21T07:00:00.000Z",
        queryFrom: "2026-08-21T04:00:00.000Z",
        records: [{
          apparentTemperatureC: 9,
          cloudCoverPercent: 20,
          id: "100",
          lastReceivedAt: "2026-08-22T04:00:00.000Z",
          pm25MicrogramsPerCubicMeter: null,
          precipitationRateMmPerHour: 0,
          pressureHpa: 1010,
          providerKey: "ecowitt-local",
          relativeHumidityPercent: 80,
          solarRadiationWm2: 0,
          sourceId: "1",
          sourceKey: "ecowitt-88f15505d89f-local-live-v1",
          sourceKind: "physical_sensor",
          stationSlug: "ballydidean-ecowitt",
          temperatureC: 10,
          uvIndex: 0,
          validAt: "2026-08-22T04:00:00.000Z",
          windDirectionDegrees: 90,
          windGustMps: 3,
          windSpeedMps: 2,
        }],
        siteSlug: "ballydidean",
        through: asOf,
        timezone: "America/Los_Angeles",
      };
    },
    async listSites() {
      assert.fail("private input must bypass site discovery");
    },
  });
  const response = await handler(new Request(
    "http://weather.test/internal/v1/forecast-observation-input",
  ));
  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(body.contractVersion, "forecast-observation-input/v1");
  assert.equal(body.records.length, 1);
  assert.deepEqual(Object.keys(body.records[0]).sort(), [
    "id", "metrics", "provenance", "receivedAt", "validAt",
  ]);
  assert.equal(JSON.stringify(body).includes("metadata"), false);
  assert.equal(reads, 1);

  assert.equal((await handler(new Request(
    "http://weather.test/internal/v1/forecast-observation-input?from=x",
  ))).status, 400);
  assert.equal((await handler(new Request(
    "http://weather.test/internal/v1/forecast-observation-input",
    { method: "POST" },
  ))).status, 405);
});

test("private observation input refuses a body above four MiB", async () => {
  const { handler } = createFixture({
    async getForecastObservationInput(asOf) {
      return {
        projectionFrom: "2026-08-21T07:00:00.000Z",
        queryFrom: "2026-08-21T04:00:00.000Z",
        records: [{
          apparentTemperatureC: null, cloudCoverPercent: null, id: "1",
          lastReceivedAt: asOf, pm25MicrogramsPerCubicMeter: null,
          precipitationRateMmPerHour: null, pressureHpa: null,
          providerKey: "x".repeat(4 * 1024 * 1024), relativeHumidityPercent: null,
          solarRadiationWm2: null, sourceId: "1", sourceKey: "source",
          sourceKind: "physical_sensor", stationSlug: "station", temperatureC: null,
          uvIndex: null, validAt: "2026-08-22T04:00:00.000Z",
          windDirectionDegrees: null, windGustMps: null, windSpeedMps: null,
        }],
        siteSlug: "ballydidean", through: asOf,
        timezone: "America/Los_Angeles",
      };
    },
  });
  const response = await handler(new Request(
    "http://weather.test/internal/v1/forecast-observation-input",
  ));
  assert.equal(response.status, 500);
  assert.equal((await response.json()).error.code, "internal_error");
});

test("mutation methods fail on every documented route without store writes", async () => {
  const { handler } = createFixture();
  const paths = [
    "/api/v1/sites",
    "/api/v1/sites/ballydidean/current",
    "/api/v1/sites/ballydidean/forecast",
    "/api/v1/sites/ballydidean/history",
    "/api/v1/sites/ballydidean/trends",
    "/api/v1/sites/ballydidean/tides",
    "/api/v1/health",
  ];

  // verify the complete mutation matrix
  for (const method of ["POST", "PUT", "PATCH", "DELETE", "OPTIONS"]) {
    // verify every documented route
    for (const path of paths) {
      const response = await handler(
        new Request(`http://weather.test${path}`, { method }),
      );
      assert.equal(response.status, 405, `${method} ${path}`);
      assert.equal(response.headers.get("allow"), "GET, HEAD");
    }
  }
});

test("GET sites groups active sources and retains direct attribution", async () => {
  const { handler } = createFixture();
  const response = await handler(
    new Request("http://weather.test/api/v1/sites"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(body.data[0].slug, "ballydidean");
  assert.equal(body.data[0].stations[0].sources.length, 3);
  assert.equal(body.data[0].stations[0].latitude, 47.950429954185445);
  assert.equal(body.data[0].stations[0].longitude, -122.42797012608193);
  assert.equal(
    body.data[0].stations[0].sources[0].attribution.label,
    "Weather data by Open-Meteo",
  );
  assert.equal(
    body.data[0].stations[0].sources[0].attribution.url,
    "https://open-meteo.com/",
  );
});

test("current accepts station and source and returns bounded public metadata", async () => {
  const { currentQueries, handler } = createFixture();
  const response = await handler(
    new Request(
      "http://weather.test/api/v1/sites/ballydidean/current?station=open-meteo-virtual&source=10",
    ),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.deepEqual(currentQueries[0], {
    filters: { sourceId: "10", stationSlug: "open-meteo-virtual" },
    siteSlug: "ballydidean",
  });
  assert.equal(body.data[0].freshness.status, "fresh");
  assert.equal(body.data[0].pressureChange3hHpa, -2.5);
  assert.equal(body.data[0].provenance.label, "model-derived current conditions");
  assert.equal(body.data[0].metrics.pm25MicrogramsPerCubicMeter, 7);
  assert.equal(
    body.data[0].metrics.soilElectricalConductivityMicrosiemensPerCm,
    420,
  );
  assert.deepEqual(body.data[0].metadata, {
    device: { model: "virtual-grid", serial: null, vendor: "Open-Meteo" },
    provider: {
      dataset: "era5",
      elevationM: 17,
      gridCell: "47.95,-122.43",
      propertySensors: null,
    },
    quality: {
      confidencePercent: 93,
      flags: ["interpolated"],
      interpolation: "linear",
      status: "accepted",
    },
    upstream: {
      model: "best_match",
      timezone: "America/Los_Angeles",
    },
  });
  assert.doesNotMatch(JSON.stringify(body), /private-request-id|request_id/u);
});

test("current exposes only bounded EcoWitt property sensor snapshots", async () => {
  const { handler } = createFixture({
    // return one record with provider-private sensor snapshots
    async getCurrent() {
      return [makeRecord({
        providerMetadata: {
          dataset: "get_livedata_info",
          property_sensors: [
            {
              channel: 1,
              key: "soil-1",
              model: "WH52",
              readings: {
                soilMoisturePercent: 42,
                temperatureC: 17.7,
                unsafe: "not-a-number",
              },
            },
            { key: "../invalid", model: "Unknown", readings: {} },
          ],
        },
      })];
    },
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/current"),
  );
  const body = await response.json();

  assert.deepEqual(body.data[0].metadata.provider.propertySensors, [
    {
      channel: 1,
      key: "soil-1",
      model: "WH52",
      readings: {
        soilMoisturePercent: 42,
        temperatureC: 17.7,
      },
    },
  ]);
});

test("history uses frozen filter names, defaults, maximum, and opaque cursor", async () => {
  const { handler, historyQueries } = createFixture();
  const defaultResponse = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/history"),
  );
  const response = await handler(
    new Request(
      "http://weather.test/api/v1/sites/ballydidean/history?sourceKind=reanalysis&station=open-meteo-virtual&source=11&from=2026-08-01T00%3A00%3A00Z&to=2026-08-22T00%3A00%3A00Z&limit=2",
    ),
  );
  const body = await response.json();

  assert.equal(defaultResponse.status, 200);
  assert.equal(historyQueries[0].limit, 101);
  assert.equal(response.status, 200);
  assert.equal(body.data.length, 2);
  assert.equal(typeof body.page.nextCursor, "string");
  assert.equal(historyQueries[1].sourceKind, "reanalysis");
  assert.equal(historyQueries[1].stationSlug, "open-meteo-virtual");
  assert.equal(historyQueries[1].sourceId, "11");
  assert.equal(historyQueries[1].from, "2026-08-01T00:00:00.000Z");
  assert.equal(historyQueries[1].to, "2026-08-22T00:00:00.000Z");
  assert.equal(historyQueries[1].limit, 3);

  const maximum = await handler(
    new Request(
      "http://weather.test/api/v1/sites/ballydidean/history?limit=250",
    ),
  );
  const excessive = await handler(
    new Request(
      "http://weather.test/api/v1/sites/ballydidean/history?limit=251",
    ),
  );
  const oldName = await handler(
    new Request(
      "http://weather.test/api/v1/sites/ballydidean/history?kind=reanalysis",
    ),
  );

  assert.equal(maximum.status, 200);
  assert.equal(historyQueries.at(-1).limit, 251);
  assert.equal(excessive.status, 400);
  assert.equal(oldName.status, 400);
});

// lock the public raw v4 response before adjustment metadata exists
test("I-API-01 forecast preserves raw metrics with inactive adjustment metadata", async () => {
  const { forecastQueries, handler, pressureContextQueries } = createFixture();
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.deepEqual(forecastQueries, [{
    asOf: "2026-08-21T07:00:00.000Z",
    hours: 24,
    siteSlug: "ballydidean",
  }]);
  assert.deepEqual(pressureContextQueries, [{
    asOf: "2026-08-21T07:00:00.000Z",
    siteSlug: "ballydidean",
  }]);
  // retain exact serialized field order and public values
  const rawForecast = {
    freshness: {
      ageSeconds: 0,
      label: "Weather value is current",
      status: "fresh",
    },
    id: "201",
    metadata: {
      device: null,
      provider: {
        dataset: "forecast",
        elevationM: 32,
        gridCell: "47.941,-122.438",
        propertySensors: null,
      },
      quality: null,
      upstream: {
        model: "best_match",
        timezone: "GMT",
      },
    },
    metrics: {
      apparentTemperatureC: 11.7,
      blackGlobeTemperatureC: null,
      cloudCoverPercent: 72,
      pm25MicrogramsPerCubicMeter: 5,
      precipitationMm: 0.1,
      precipitationRateMmPerHour: 0.1,
      pressureHpa: 1018.1,
      relativeHumidityPercent: 83,
      soilElectricalConductivityMicrosiemensPerCm: null,
      soilMoisturePercent: null,
      solarRadiationWm2: null,
      temperatureC: 12.1,
      uvIndex: 0.2,
      windDirectionDegrees: 180,
      windGustMps: 3.1,
      windSpeedMps: 1.8,
      wetBulbGlobeTemperatureC: null,
    },
    productRunAt: "2026-08-22T05:00:00.000Z",
    provenance: {
      attribution: {
        label: "Weather data by Open-Meteo",
        url: "https://open-meteo.com/",
      },
      label: "forecast model product",
      providerKey: "open-meteo",
      sourceId: "12",
      sourceKey: "open-meteo-forecast-v4",
      sourceKind: "forecast",
      stationSlug: "open-meteo-virtual",
    },
    receivedAt: "2026-08-22T05:00:00.000Z",
    revisionCount: 0,
    validAt: "2026-08-22T06:00:00.000Z",
    rainAdjustment: {
      contractVersion: "forecast-rain-adjustment-decision/v1",
      state: "disabled",
      reasonCode: "model_unavailable",
      bundleSha256: null,
      correctedPrecipitationMm: null,
      rawBestMatchPrecipitationMm: 0.1,
      sourceForecast: null,
    },
    adjustment: {
      adjustedMetrics: {},
      appliedMetrics: [],
      contractVersion: "forecast-adjustment-decision/v1",
      reasonCode: "registry_inactive",
      state: "disabled",
    },
    temperatureAdjustment: {
      branch: null,
      bundleSha256: null,
      contractVersion: "forecast-temperature-canary-decision/v1",
      correctedTemperatureC: null,
      rawBestMatchTemperatureC: 12.1,
      reasonCode: "registry_inactive",
      recentErrorStateSha256: null,
      sourceForecast: null,
      state: "disabled",
    },
  };
  assert.equal(JSON.stringify(body.data[0]), JSON.stringify(rawForecast));
  assert.equal("pressureChange3hHpa" in body.data[0], false);
  assert.deepEqual(body.adjustmentRuntime, inactiveAdjustmentRuntime);
  assert.deepEqual(
    body.temperatureAdjustmentRuntime,
    inactiveTemperatureAdjustmentRuntime,
  );
  assert.equal(body.days, 1);
  assert.deepEqual(body.pressureContext, []);

  const extendedResponse = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast?days=10"),
  );
  const extendedBody = await extendedResponse.json();
  assert.equal(extendedResponse.status, 200);
  assert.equal(extendedBody.days, 10);
  assert.deepEqual(forecastQueries.at(-1), {
    asOf: "2026-08-21T07:00:00.000Z",
    hours: 240,
    siteSlug: "ballydidean",
  });

  const overnightResponse = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast?window=overnight"),
  );
  const overnightBody = await overnightResponse.json();
  assert.equal(overnightResponse.status, 200);
  assert.equal(overnightBody.days, 1);
  assert.deepEqual(forecastQueries.at(-1), {
    asOf: "2026-08-21T07:00:00.000Z",
    hours: 31,
    productSelection: "anchor-containing",
    siteSlug: "ballydidean",
  });
});

// explicit rain policy must suppress retained sidecars before repository access
test("rain policy-raw response preserves Best Match and frozen startup status", async () => {
  let rainReads = 0;
  const { handler } = createFixture({
    // fail if disabled policy reaches retained adjusted output
    async getRainAdjustmentRun() {
      rainReads += 1;
      assert.fail("policy-raw forecast read a retained rain run");
    },
  }, {
    rainAdjustment: {
      loadedAt: "2026-08-22T04:58:00.000Z",
      runtime: {
        artifactSha256: null,
        reasonCode: "policy_raw",
        state: "disabled",
      },
    },
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();
  assert.equal(response.status, 200);
  assert.equal(rainReads, 0);
  assert.equal(body.data[0].metrics.precipitationMm, 0.1);
  assert.deepEqual(body.data[0].rainAdjustment, {
    bundleSha256: null,
    contractVersion: "forecast-rain-adjustment-decision/v1",
    correctedPrecipitationMm: null,
    rawBestMatchPrecipitationMm: 0.1,
    reasonCode: "policy_raw",
    sourceForecast: null,
    state: "disabled",
  });
  assert.deepEqual(body.rainAdjustmentRuntime, {
    activeBundle: null,
    loadedAt: "2026-08-22T04:58:00.000Z",
    reasonCode: "policy_raw",
    source: null,
    state: "disabled",
  });
  assert.equal(body.data[0].adjustment.reasonCode, "registry_inactive");
  assert.equal(body.data[0].temperatureAdjustment.reasonCode, "registry_inactive");
});

// expose an unadjusted complete retained pressure vintage separately
test("forecast pressure context remains raw and isolated from forecast data", async () => {
  // build one complete prior-vintage pressure window
  const contextRows = Array.from({ length: 6 }, (_value, index) =>
    matchingForecastRecord({
      id: String(300 + index),
      pressureHpa: 1010 + index,
      productRunAt: "2026-08-21T06:00:00.000Z",
      validAt: new Date(
        Date.parse("2026-08-21T04:00:00.000Z") + index * 3_600_000,
      ).toISOString(),
    })
  );
  const { handler } = createFixture({
    // return one complete prior vintage
    async getForecastPressureContext() {
      return contextRows;
    },
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(body.data.length, 1);
  assert.deepEqual(
    body.pressureContext.map(
      // project the public raw context fields
      (record) => ({
        pressureHpa: record.metrics.pressureHpa,
        productRunAt: record.productRunAt,
        validAt: record.validAt,
      }),
    ),
    contextRows.map(
      // project the matching storage fields
      (record) => ({
        pressureHpa: record.pressureHpa,
        productRunAt: record.productRunAt,
        validAt: record.validAt,
      }),
    ),
  );
  assert.equal("adjustment" in body.pressureContext[0], false);
  assert.equal("temperatureAdjustment" in body.pressureContext[0], false);
  assert.equal("rainAdjustment" in body.pressureContext[0], false);
});

// keep optional context failures from changing forecast availability
test("forecast pressure context read failure falls back to an empty array", async () => {
  const { handler } = createFixture({
    // fail only the optional context read
    async getForecastPressureContext() {
      throw new Error("private pressure context failure");
    },
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(body.data.length, 1);
  assert.deepEqual(body.pressureContext, []);
  assert.doesNotMatch(JSON.stringify(body), /private pressure context failure/u);
});

test("I-API-02 active runtime adjusts only its enabled metric-band pairs", async () => {
  const runtime = await createActiveForecastRuntime();
  const rawRow = matchingForecastRecord();
  const { handler } = createFixture(
    {
      // return one exact candidate-identity row
      async getForecast() {
        return [rawRow];
      },
    },
    {
      forecastAdjustment: {
        loadedAt: "2026-08-22T04:59:00.000Z",
        runtime,
      },
    },
  );
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();
  const decision = body.data[0].adjustment;

  assert.equal(response.status, 200);
  assert.equal(body.data[0].metrics.temperatureC, rawRow.temperatureC);
  assert.equal(body.data[0].metrics.cloudCoverPercent, rawRow.cloudCoverPercent);
  assert.equal(decision.state, "active");
  assert.deepEqual(decision.appliedMetrics, ["temperatureC"]);
  assert.deepEqual(decision.adjustedMetrics, { temperatureC: 14.1 });
  assert.equal("cloudCoverPercent" in decision.adjustedMetrics, false);
  assert.deepEqual(body.adjustmentRuntime, {
    activationMode: "qualified",
    activeBundle: runtime.bundle.bundleSha256,
    authorizationSha256: null,
    candidateArtifactSha256:
      runtime.bundle.candidate.candidateArtifactSha256,
    enabledMetrics: ["temperatureC"],
    evaluationReportSha256:
      runtime.bundle.evaluationReport.evaluationReportSha256,
    expiresAt: null,
    loadedAt: "2026-08-22T04:59:00.000Z",
    qualificationReceiptSha256:
      runtime.bundle.qualificationReceipt.qualificationReceiptSha256,
    reasonCode: null,
    state: "active",
    transferReportSha256: null,
  });
  assert.doesNotMatch(
    JSON.stringify(body.adjustmentRuntime),
    /coefficient|evidence|path|station|training/u,
  );
  const healthResponse = await handler(
    new Request("http://weather.test/api/v1/health"),
  );
  const health = await healthResponse.json();
  assert.deepEqual(health.data.adjustmentRuntime, body.adjustmentRuntime);
});

// prove the opt-in temperature path remains separate from Best Match and wind
test("temperature canary exposes explicit ECMWF provenance without mutating raw", async () => {
  const runtime = createTemperatureCanaryRuntime();
  const sidecar = createTemperatureSidecar();
  const sidecarQueries = [];
  const { handler } = createFixture(
    {
      // return one current ECMWF run
      async getTemperatureCanarySidecar(siteSlug, asOf, from, to) {
        sidecarQueries.push({ asOf, from, siteSlug, to });
        return sidecar;
      },
      // return bounded collector state
      async getTemperatureCanaryStatus() {
        return {
          firstReceivedAt: sidecar.run.firstReceivedAt,
          hourCount: 12,
          latestRunInitializedAt: sidecar.run.runInitializedAt,
          stateReason: sidecar.run.stateReason,
          stateStatus: sidecar.run.stateStatus,
        };
      },
    },
    {
      temperatureAdjustment: {
        loadedAt: "2026-08-22T04:59:00.000Z",
        runtime,
      },
    },
  );
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();
  const record = body.data[0];

  assert.equal(response.status, 200);
  assert.equal(record.metrics.temperatureC, 12.1);
  assert.equal(record.adjustment.state, "disabled");
  assert.equal(record.temperatureAdjustment.state, "active");
  assert.equal(record.temperatureAdjustment.branch, "direct");
  assert.equal(record.temperatureAdjustment.rawBestMatchTemperatureC, 12.1);
  assert.equal(record.temperatureAdjustment.sourceForecast.rawTemperatureC, 11);
  assert.equal(
    record.temperatureAdjustment.sourceForecast.operationalHorizonHours,
    6,
  );
  assert.equal(record.temperatureAdjustment.correctedTemperatureC, 12);
  assert.equal(
    record.temperatureAdjustment.sourceForecast.upstreamModel,
    "ecmwf_ifs",
  );
  assert.deepEqual(sidecarQueries, [{
    asOf: "2026-08-22T05:00:00.000Z",
    from: "2026-08-21T07:00:00.000Z",
    siteSlug: "ballydidean",
    to: "2026-08-22T07:00:00.000Z",
  }]);
  assert.deepEqual(body.temperatureAdjustmentRuntime, {
    actionSha256: null,
    activationMode: "temperature_canary",
    activeBundle: runtime.bundle.bundleSha256,
    authorizationSha256: runtime.bundle.authorization.authorizationSha256,
    expiresAt: runtime.bundle.authorization.expiresAt,
    fullMemberRootSha256: null,
    loadedAt: "2026-08-22T04:59:00.000Z",
    policyReportSha256: null,
    reasonCode: null,
    source: {
      adaptiveReady: false,
      firstReceivedAt: sidecar.run.firstReceivedAt,
      hourCount: 12,
      latestRunInitializedAt: sidecar.run.runInitializedAt,
      stateReason: "cold_start",
      stateStatus: "cold",
    },
    state: "active",
  });
});

// preserve permanent authorization in the public runtime projection
test("permanent temperature runtime remains active with a null expiry", async () => {
  const runtime = createPermanentTemperatureCanaryRuntime();
  const { handler } = createFixture({}, {
    temperatureAdjustment: {
      loadedAt: runtime.bundle.authorization.activatedAt,
      runtime,
    },
    now: () => new Date("2027-10-01T06:00:00.000Z"),
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(body.temperatureAdjustmentRuntime.state, "active");
  assert.equal(body.temperatureAdjustmentRuntime.activeBundle, runtime.bundle.bundleSha256);
  assert.equal(body.temperatureAdjustmentRuntime.expiresAt, null);
  assert.equal(body.temperatureAdjustmentRuntime.reasonCode, null);
});

test("qualified temperature maintenance exposes only root receipt authority", async () => {
  const incumbent = createPermanentTemperatureCanaryRuntime().bundle;
  const authority = {
    actionSha256: "a".repeat(64),
    fullMemberRootSha256: "b".repeat(64),
    policyReportSha256: "c".repeat(64),
  };
  const runtime = {
    bundle: {
      bundleSha256: "d".repeat(64),
      maintenanceAuthority: authority,
      maintenancePackage: true,
      model: incumbent.model,
      servedForecastIdentity: incumbent.servedForecastIdentity,
      trainingForecastIdentity: incumbent.trainingForecastIdentity,
    },
    reasonCode: null,
    state: "active",
  };
  const { handler } = createFixture({}, {
    temperatureAdjustment: {
      loadedAt: "2026-10-08T00:00:00.000Z",
      runtime,
    },
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();
  assert.deepEqual(body.temperatureAdjustmentRuntime, {
    ...authority,
    activationMode: "maintenance_qualified",
    activeBundle: runtime.bundle.bundleSha256,
    authorizationSha256: null,
    expiresAt: null,
    loadedAt: "2026-10-08T00:00:00.000Z",
    reasonCode: null,
    source: null,
    state: "active",
  });
});

// prove optional read faults remain observable without changing serving
test("temperature canary read failures stay raw and emit bounded diagnostics", async () => {
  const diagnostics = [];
  const runtime = createTemperatureCanaryRuntime();
  const sidecarFailure = Object.assign(
    new Error("SELECT raw_temperature_c FROM private_sidecar password=secret"),
    { code: "57P01" },
  );
  const statusFailure = Object.assign(
    new TypeError("postgres://weather:secret@database/weather"),
    { code: "ECONNRESET" },
  );
  const { handler } = createFixture(
    {
      // fail only the optional sidecar read
      async getTemperatureCanarySidecar() {
        throw sidecarFailure;
      },
      // fail only the optional status read
      async getTemperatureCanaryStatus() {
        throw statusFailure;
      },
    },
    {
      // capture redacted fallback diagnostics
      logDiagnostic(diagnostic) {
        diagnostics.push(diagnostic);
      },
      temperatureAdjustment: {
        loadedAt: "2026-08-22T04:59:00.000Z",
        runtime,
      },
    },
  );
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(body.data[0].metrics.temperatureC, forecastRecord.temperatureC);
  assert.equal(body.data[0].metrics.windSpeedMps, forecastRecord.windSpeedMps);
  assert.deepEqual(body.data[0].adjustment, {
    adjustedMetrics: {},
    appliedMetrics: [],
    contractVersion: "forecast-adjustment-decision/v1",
    reasonCode: "registry_inactive",
    state: "disabled",
  });
  assert.equal(body.data[0].temperatureAdjustment.state, "raw_fallback");
  // order concurrently collected read diagnostics
  const orderedDiagnostics = diagnostics.toSorted((left, right) =>
    left.operation.localeCompare(right.operation)
  );
  assert.deepEqual(
    orderedDiagnostics,
    [
      {
        errorCode: "57P01",
        errorName: "Error",
        event: "temperature_canary_fallback",
        method: "GET",
        operation: "sidecar_read",
        status: 200,
      },
      {
        errorCode: "ECONNRESET",
        errorName: "TypeError",
        event: "temperature_canary_fallback",
        method: "GET",
        operation: "status_read",
        status: 200,
      },
    ],
  );
  assert.doesNotMatch(
    JSON.stringify({ body, diagnostics }),
    /SELECT|private_sidecar|password|secret|postgres:\/\/|raw_temperature_c/u,
  );
});

// prove health monitoring fallback preserves head semantics
test("temperature canary health status failures stay healthy for HEAD", async () => {
  const diagnostics = [];
  const runtime = createTemperatureCanaryRuntime();
  const { handler } = createFixture(
    {
      // fail only optional collector monitoring
      async getTemperatureCanaryStatus() {
        throw Object.assign(new Error("connection string secret"), {
          code: "08006",
        });
      },
    },
    {
      // capture one request diagnostic
      logDiagnostic(diagnostic) {
        diagnostics.push(diagnostic);
      },
      temperatureAdjustment: {
        loadedAt: "2026-08-22T04:59:00.000Z",
        runtime,
      },
    },
  );
  const response = await handler(
    new Request("http://weather.test/api/v1/health", { method: "HEAD" }),
  );

  assert.equal(response.status, 200);
  assert.equal(await response.text(), "");
  assert.deepEqual(diagnostics, [
    {
      errorCode: "08006",
      errorName: "Error",
      event: "temperature_canary_fallback",
      method: "HEAD",
      operation: "status_read",
      status: 200,
    },
  ]);
});

// prove fallback diagnostics use the independent readiness status
test("temperature canary health fallback records an actual 503 status", async () => {
  const diagnostics = [];
  const runtime = createTemperatureCanaryRuntime();
  const { handler } = createFixture(
    {
      // fail independent database readiness
      async getHealth() {
        return {
          database: "unavailable",
          migration: { status: "unavailable", version: null },
          workerLastLoopAt: null,
        };
      },
      // fail optional collector monitoring
      async getTemperatureCanaryStatus() {
        throw Object.assign(new Error("database host secret"), {
          code: "08006",
        });
      },
    },
    {
      // capture the final response status
      logDiagnostic(diagnostic) {
        diagnostics.push(diagnostic);
      },
      temperatureAdjustment: {
        loadedAt: "2026-08-22T04:59:00.000Z",
        runtime,
      },
    },
  );
  const response = await handler(
    new Request("http://weather.test/api/v1/health"),
  );
  const body = await response.json();

  assert.equal(response.status, 503);
  assert.equal(body.data.ready, false);
  assert.deepEqual(diagnostics, [
    {
      errorCode: "08006",
      errorName: "Error",
      event: "temperature_canary_fallback",
      method: "GET",
      operation: "status_read",
      status: 503,
    },
  ]);
});

// prove row failures collapse to one request diagnostic
test("temperature canary inference failures emit once across many rows", async () => {
  const diagnostics = [];
  const runtime = createTemperatureCanaryRuntime();
  // build enough rows to prove request-level deduplication
  const rows = Array.from({ length: 32 }, (_value, index) => ({
    ...forecastRecord,
    id: String(10_000 + index),
    validAt: new Date(
      Date.parse(forecastRecord.validAt) + index * 3_600_000,
    ).toISOString(),
  }));
  const { handler } = createFixture(
    {
      // return a multi-row raw forecast
      async getForecast() {
        return rows;
      },
      // return no optional source rows
      async getTemperatureCanarySidecar() {
        return null;
      },
      // return no optional monitoring row
      async getTemperatureCanaryStatus() {
        return null;
      },
    },
    {
      // capture only bounded diagnostics
      logDiagnostic(diagnostic) {
        diagnostics.push(diagnostic);
      },
      temperatureAdjustment: {
        // fail every row projection
        apply() {
          throw Object.assign(new Error("raw=99 coefficient=secret"), {
            code: "MODEL_FAILURE",
          });
        },
        loadedAt: "2026-08-22T04:59:00.000Z",
        runtime,
      },
    },
  );
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();
  // issue a second request to prove deduplication resets
  const secondResponse = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  await secondResponse.json();

  assert.equal(response.status, 200);
  assert.equal(body.data.length, rows.length);
  // compare every public raw temperature
  const returnedTemperatures = body.data.map((record) => record.metrics.temperatureC);
  // retain the source raw temperature order
  const expectedTemperatures = rows.map((row) => row.temperatureC);
  assert.deepEqual(
    returnedTemperatures,
    expectedTemperatures,
  );
  assert.equal(secondResponse.status, 200);
  const expectedDiagnostic = {
    errorCode: "MODEL_FAILURE",
    errorName: "Error",
    event: "temperature_canary_fallback",
    method: "GET",
    operation: "inference",
    status: 200,
  };
  assert.deepEqual(diagnostics, [expectedDiagnostic, expectedDiagnostic]);
  assert.doesNotMatch(
    JSON.stringify({ body, diagnostics }),
    /raw=99|coefficient=secret/u,
  );
});

// prove healthy absence is not diagnosed as failure
test("temperature canary empty reads emit no fallback diagnostics", async () => {
  const diagnostics = [];
  const runtime = createTemperatureCanaryRuntime();
  const { handler } = createFixture(
    {
      // return a healthy empty sidecar
      async getTemperatureCanarySidecar() {
        return null;
      },
      // return healthy empty monitoring
      async getTemperatureCanaryStatus() {
        return null;
      },
    },
    {
      // capture unexpected fallback diagnostics
      logDiagnostic(diagnostic) {
        diagnostics.push(diagnostic);
      },
      temperatureAdjustment: {
        loadedAt: "2026-08-22T04:59:00.000Z",
        runtime,
      },
    },
  );
  const forecastResponse = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const healthResponse = await handler(
    new Request("http://weather.test/api/v1/health"),
  );

  assert.equal(forecastResponse.status, 200);
  assert.equal(healthResponse.status, 200);
  assert.deepEqual(diagnostics, []);
});

// prove hostile error and sink behavior cannot escape fail-raw serving
test("temperature canary hostile errors and diagnostic sinks stay fail-raw", async () => {
  const diagnostics = [];
  const runtime = createTemperatureCanaryRuntime();
  const hostileFailure = new Error("private model values");
  Object.defineProperties(hostileFailure, {
    code: {
      // reject diagnostic property access
      get() {
        throw new Error("private code getter");
      },
    },
    name: {
      // reject diagnostic property access
      get() {
        throw new Error("private name getter");
      },
    },
  });
  const { handler } = createFixture(
    {
      // throw one hostile optional-read failure
      async getTemperatureCanarySidecar() {
        throw hostileFailure;
      },
      // keep monitoring healthy and empty
      async getTemperatureCanaryStatus() {
        return null;
      },
    },
    {
      // throw after observing the bounded event
      logDiagnostic(diagnostic) {
        diagnostics.push(diagnostic);
        throw new Error("diagnostic sink unavailable");
      },
      temperatureAdjustment: {
        loadedAt: "2026-08-22T04:59:00.000Z",
        runtime,
      },
    },
  );
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(body.data[0].metrics.temperatureC, forecastRecord.temperatureC);
  assert.deepEqual(diagnostics, [
    {
      errorCode: null,
      errorName: "UnknownError",
      event: "temperature_canary_fallback",
      method: "GET",
      operation: "sidecar_read",
      status: 200,
    },
  ]);
  assert.doesNotMatch(
    JSON.stringify({ body, diagnostics }),
    /private model|private code|private name|sink unavailable/u,
  );
});

test("wind canary runtime status exposes only bounded activation metadata", async () => {
  const runtime = createWindCanaryRuntime();
  const { handler } = createFixture({}, {
    forecastAdjustment: {
      // keep the row projection independent from status metadata
      apply() {
        return {
          adjustedMetrics: {},
          appliedMetrics: [],
          contractVersion: "forecast-adjustment-decision/v1",
          reasonCode: "metric_not_enabled",
          state: "not_applicable",
        };
      },
      loadedAt: "2026-09-09T05:00:00.000Z",
      runtime,
    },
    now: () => new Date("2026-09-09T06:00:00.000Z"),
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.deepEqual(body.adjustmentRuntime, {
    activationMode: "wind_canary",
    activeBundle: runtime.bundle.bundleSha256,
    authorizationSha256: runtime.bundle.authorization.authorizationSha256,
    candidateArtifactSha256: runtime.bundle.candidate.candidateArtifactSha256,
    enabledMetrics: ["windGustMps", "windSpeedMps"],
    evaluationReportSha256: null,
    expiresAt: runtime.bundle.authorization.expiresAt,
    loadedAt: "2026-09-09T05:00:00.000Z",
    qualificationReceiptSha256: null,
    reasonCode: null,
    state: "active",
    transferReportSha256: runtime.bundle.transferReport.transferReportSha256,
  });
  assert.doesNotMatch(
    JSON.stringify(body.adjustmentRuntime),
    /coefficient|evidence|path|station|training/u,
  );
});

// keep public monitoring independent of already validated model payloads
test("public runtime status does not traverse cached model payloads", async () => {
  const wind = createPermanentWindCanaryRuntime();
  const temperature = createPermanentTemperatureCanaryRuntime();
  verifyForecastAdjustmentWindCanaryRuntimeBundle(wind.bundle);
  verifyForecastAdjustmentTemperatureCanaryRuntimeBundle(temperature.bundle);
  const coefficients = wind.bundle.candidate.coefficients;
  const model = temperature.bundle.model;
  let modelPayloadReads = 0;
  Object.defineProperty(wind.bundle.candidate, "coefficients", {
    enumerable: true,
    // observe payload traversal without changing the loaded artifact
    get() {
      modelPayloadReads += 1;
      return coefficients;
    },
  });
  Object.defineProperty(temperature.bundle, "model", {
    enumerable: true,
    // observe payload traversal without changing the loaded artifact
    get() {
      modelPayloadReads += 1;
      return model;
    },
  });
  const { handler } = createFixture({
    // keep readiness independent of the future authorization clock
    async getHealth() {
      return {
        database: "ready",
        migration: { status: "current", version: "0001_initial_weather.sql" },
        workerLastLoopAt: "2027-10-01T05:59:00.000Z",
      };
    },
  }, {
    forecastAdjustment: { loadedAt: wind.bundle.authorization.activatedAt, runtime: wind },
    temperatureAdjustment: { loadedAt: temperature.bundle.authorization.activatedAt, runtime: temperature },
    // keep both permanent authorizations active
    now: () => new Date("2027-10-01T06:00:00.000Z"),
  });
  const response = await handler(new Request("http://weather.test/api/v1/health"));
  const body = await response.json();
  assert.equal(response.status, 200);
  assert.equal(body.data.adjustmentRuntime.state, "active");
  assert.equal(body.data.temperatureAdjustmentRuntime.state, "active");
  assert.equal(modelPayloadReads, 0);
});

// preserve inclusive activation and exclusive finite deadlines without rehashing
test("cached runtime status preserves finite and permanent authorization clocks", async () => {
  const cases = [
    ["wind", createWindCanaryRuntime()],
    ["wind", createPermanentWindCanaryRuntime()],
    ["temperature", createTemperatureCanaryRuntime()],
    ["temperature", createPermanentTemperatureCanaryRuntime()],
  ];

  // exercise both model families and authorization generations
  for (const [metric, runtime] of cases) {
    const { activatedAt, expiresAt } = runtime.bundle.authorization;
    const boundaries = [
      [new Date(Date.parse(activatedAt) - 1).toISOString(), "disabled"],
      [activatedAt, "active"],
      [expiresAt ?? "2027-10-01T06:00:00.000Z", expiresAt === null ? "active" : "disabled"],
    ];

    // freeze the request clock at each authorization boundary
    for (const [evaluatedAt, expectedState] of boundaries) {
      const { handler } = createFixture({
        // keep database readiness independent of the authorization clock
        async getHealth() {
          return {
            database: "ready",
            migration: { status: "current", version: "0001_initial_weather.sql" },
            workerLastLoopAt: evaluatedAt,
          };
        },
      }, {
        [metric === "wind" ? "forecastAdjustment" : "temperatureAdjustment"]: {
          loadedAt: activatedAt,
          runtime,
        },
        // evaluate the cached authorization without advancing time
        now: () => new Date(evaluatedAt),
      });
      const response = await handler(new Request("http://weather.test/api/v1/health"));
      const body = await response.json();
      const status = metric === "wind"
        ? body.data.adjustmentRuntime
        : body.data.temperatureAdjustmentRuntime;
      assert.equal(response.status, 200);
      assert.equal(status.state, expectedState, `${metric} at ${evaluatedAt}`);
      assert.equal(status.reasonCode, expectedState === "active" ? null : "canary_expired");
    }
  }
});

// keep permanent authorization active without inventing an expiry timestamp
test("permanent wind runtime remains active with a null expiry", async () => {
  const runtime = createPermanentWindCanaryRuntime();
  const { handler } = createFixture({}, {
    forecastAdjustment: {
      loadedAt: runtime.bundle.authorization.activatedAt,
      runtime,
    },
    now: () => new Date("2027-10-01T06:00:00.000Z"),
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(body.adjustmentRuntime.state, "active");
  assert.equal(body.adjustmentRuntime.activeBundle, runtime.bundle.bundleSha256);
  assert.equal(body.adjustmentRuntime.expiresAt, null);
  assert.equal(body.adjustmentRuntime.reasonCode, null);
  assert.deepEqual(body.adjustmentRuntime.enabledMetrics, ["windGustMps", "windSpeedMps"]);
});

test("expired wind canary metadata and rows fail raw on the same clock", async () => {
  const runtime = createWindCanaryRuntime();
  const evaluatedAt = [];
  const { handler } = createFixture({}, {
    forecastAdjustment: {
      // capture the exact per-request application clock
      apply(_runtime, input) {
        evaluatedAt.push(input.evaluatedAt);
        return {
          adjustedMetrics: {},
          appliedMetrics: [],
          contractVersion: "forecast-adjustment-decision/v1",
          reasonCode: "canary_expired",
          state: "disabled",
        };
      },
      loadedAt: "2026-09-09T05:00:00.000Z",
      runtime,
    },
    now: () => new Date(runtime.bundle.authorization.expiresAt),
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();

  assert.deepEqual(evaluatedAt, [runtime.bundle.authorization.expiresAt]);
  assert.deepEqual(body.adjustmentRuntime, {
    activationMode: null,
    activeBundle: null,
    authorizationSha256: null,
    candidateArtifactSha256: null,
    enabledMetrics: [],
    evaluationReportSha256: null,
    expiresAt: null,
    loadedAt: "2026-09-09T05:00:00.000Z",
    qualificationReceiptSha256: null,
    reasonCode: "canary_expired",
    state: "disabled",
    transferReportSha256: null,
  });
  assert.equal(body.data[0].adjustment.reasonCode, "canary_expired");

  const healthResponse = await handler(
    new Request("http://weather.test/api/v1/health"),
  );
  const health = await healthResponse.json();
  assert.deepEqual(health.data.adjustmentRuntime, body.adjustmentRuntime);
});

test("I-API-03 runtime identity and lead mismatches fail raw with stable reasons", async () => {
  const runtime = await createActiveForecastRuntime();
  const cases = [
    ["source", { sourceKey: "open-meteo-forecast-v5" }, "identity_mismatch"],
    ["model", { upstreamModel: "gfs_seamless" }, "identity_mismatch"],
    ["epoch", { contractEpoch: `legacy-v4/${"0".repeat(64)}` }, "identity_mismatch"],
    ["adapter", { adapterVersion: "current/v1" }, "identity_mismatch"],
    [
      "lead",
      { validAt: "2026-08-29T06:00:00.000Z" },
      "unsupported_lead",
    ],
  ];

  // verify every immutable identity guard independently
  for (const [name, overrides, reasonCode] of cases) {
    const row = matchingForecastRecord(overrides);
    const { handler } = createFixture(
      {
        // return the selected mismatch only
        async getForecast() {
          return [row];
        },
      },
      {
        forecastAdjustment: {
          loadedAt: "2026-08-22T04:59:00.000Z",
          runtime,
        },
      },
    );
    const response = await handler(
      new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
    );
    const body = await response.json();

    assert.equal(response.status, 200, name);
    assert.deepEqual(body.data[0].metrics, {
      apparentTemperatureC: row.apparentTemperatureC,
      blackGlobeTemperatureC: row.blackGlobeTemperatureC,
      cloudCoverPercent: row.cloudCoverPercent,
      pm25MicrogramsPerCubicMeter: row.pm25MicrogramsPerCubicMeter,
      precipitationMm: row.precipitationMm,
      precipitationRateMmPerHour: row.precipitationRateMmPerHour,
      pressureHpa: row.pressureHpa,
      relativeHumidityPercent: row.relativeHumidityPercent,
      soilElectricalConductivityMicrosiemensPerCm:
        row.soilElectricalConductivityMicrosiemensPerCm,
      soilMoisturePercent: row.soilMoisturePercent,
      solarRadiationWm2: row.solarRadiationWm2,
      temperatureC: row.temperatureC,
      uvIndex: row.uvIndex,
      windDirectionDegrees: row.windDirectionDegrees,
      windGustMps: row.windGustMps,
      windSpeedMps: row.windSpeedMps,
      wetBulbGlobeTemperatureC: row.wetBulbGlobeTemperatureC,
    }, name);
    assert.equal(body.data[0].adjustment.state, "not_applicable", name);
    assert.equal(body.data[0].adjustment.reasonCode, reasonCode, name);
  }
});

test("I-API-05 adjustment exceptions preserve raw forecast status and metrics", async () => {
  const runtime = await createActiveForecastRuntime();
  const rawRow = matchingForecastRecord();
  const { handler } = createFixture(
    {
      // return unchanged v4 storage data
      async getForecast() {
        return [rawRow];
      },
    },
    {
      forecastAdjustment: {
        // simulate an unexpected model implementation exception
        apply() {
          throw new Error("secret coefficient failure");
        },
        loadedAt: "2026-08-22T04:59:00.000Z",
        runtime,
      },
    },
  );
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(JSON.stringify(body.data[0].metrics), JSON.stringify({
    apparentTemperatureC: rawRow.apparentTemperatureC,
    blackGlobeTemperatureC: rawRow.blackGlobeTemperatureC,
    cloudCoverPercent: rawRow.cloudCoverPercent,
    pm25MicrogramsPerCubicMeter: rawRow.pm25MicrogramsPerCubicMeter,
    precipitationMm: rawRow.precipitationMm,
    precipitationRateMmPerHour: rawRow.precipitationRateMmPerHour,
    pressureHpa: rawRow.pressureHpa,
    relativeHumidityPercent: rawRow.relativeHumidityPercent,
    soilElectricalConductivityMicrosiemensPerCm:
      rawRow.soilElectricalConductivityMicrosiemensPerCm,
    soilMoisturePercent: rawRow.soilMoisturePercent,
    solarRadiationWm2: rawRow.solarRadiationWm2,
    temperatureC: rawRow.temperatureC,
    uvIndex: rawRow.uvIndex,
    windDirectionDegrees: rawRow.windDirectionDegrees,
    windGustMps: rawRow.windGustMps,
    windSpeedMps: rawRow.windSpeedMps,
    wetBulbGlobeTemperatureC: rawRow.wetBulbGlobeTemperatureC,
  }));
  assert.deepEqual(body.data[0].adjustment, {
    adjustedMetrics: {},
    appliedMetrics: [],
    contractVersion: "forecast-adjustment-decision/v1",
    reasonCode: "adjustment_error",
    state: "not_applicable",
  });
  assert.doesNotMatch(JSON.stringify(body), /secret|coefficient failure/u);
});

test("I-API-05 provenance permission failures preserve raw API metrics", async () => {
  const runtime = await createActiveForecastRuntime();
  const rawRow = matchingForecastRecord({
    adapterVersion: null,
    contractEpoch: null,
    sourceConfigFingerprint: null,
  });
  const queries = [];
  const pool = {
    // serve raw reads while denying only optional provenance
    async query(text, values) {
      queries.push({ text, values });

      // deny the optional view
      if (text.includes("forecast_runtime_provenance_v1")) {
        throw Object.assign(new Error("permission denied"), { code: "42501" });
      }

      // return no optional complete pressure vintage
      if (text.includes("HAVING COUNT(*) = 6")) {
        return { rows: [] };
      }

      // serve public discovery
      if (text.includes("FROM sites si")) {
        return { rows: siteRows };
      }

      // serve the authoritative raw forecast
      if (text.includes("JOIN LATERAL")) {
        return { rows: [rawRow] };
      }

      throw new Error("unexpected database query");
    },
  };
  const store = createDatabaseWeatherReadStore(pool, {
    migrationAuthorization: null,
    migrationDirectory: "/unused",
    release: "test/v1",
  });
  const handler = createWeatherApi(store, {
    forecastAdjustment: {
      loadedAt: "2026-08-22T04:59:00.000Z",
      runtime,
    },
    // freeze response time
    now: () => new Date("2026-08-22T05:00:00.000Z"),
    version: "test/v1",
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(queries.length, 5);
  assert.doesNotMatch(queries[1].text, /forecast_runtime_provenance_v1/u);
  assert.match(queries[2].text, /FROM forecast_runtime_provenance_v1/u);
  assert.match(queries[3].text, /HAVING COUNT\(\*\) = 6/u);
  assert.match(queries[4].text, /FROM rain_adjustment_runs/u);
  assert.equal(JSON.stringify(body.data[0].metrics), JSON.stringify({
    apparentTemperatureC: rawRow.apparentTemperatureC,
    blackGlobeTemperatureC: rawRow.blackGlobeTemperatureC,
    cloudCoverPercent: rawRow.cloudCoverPercent,
    pm25MicrogramsPerCubicMeter: rawRow.pm25MicrogramsPerCubicMeter,
    precipitationMm: rawRow.precipitationMm,
    precipitationRateMmPerHour: rawRow.precipitationRateMmPerHour,
    pressureHpa: rawRow.pressureHpa,
    relativeHumidityPercent: rawRow.relativeHumidityPercent,
    soilElectricalConductivityMicrosiemensPerCm:
      rawRow.soilElectricalConductivityMicrosiemensPerCm,
    soilMoisturePercent: rawRow.soilMoisturePercent,
    solarRadiationWm2: rawRow.solarRadiationWm2,
    temperatureC: rawRow.temperatureC,
    uvIndex: rawRow.uvIndex,
    windDirectionDegrees: rawRow.windDirectionDegrees,
    windGustMps: rawRow.windGustMps,
    windSpeedMps: rawRow.windSpeedMps,
    wetBulbGlobeTemperatureC: rawRow.wetBulbGlobeTemperatureC,
  }));
  assert.deepEqual(body.data[0].adjustment, {
    adjustedMetrics: {},
    appliedMetrics: [],
    contractVersion: "forecast-adjustment-decision/v1",
    reasonCode: "identity_mismatch",
    state: "not_applicable",
  });
  assert.doesNotMatch(JSON.stringify(body), /permission denied|42501/u);
});

test("I-API-06 invalid runtime trees stay disabled and preserve raw metrics", async () => {
  const qualifiedDirectory = await mkdtemp(
    join(tmpdir(), "weather-api-qualified-"),
  );
  const triple = await createQualifiedFixture(qualifiedDirectory);
  const bundle = createForecastAdjustmentRuntimeBundle(triple);
  const cases = [];

  const missingBundleRoot = await mkdtemp(
    join(tmpdir(), "weather-api-missing-bundle-"),
  );
  await mkdir(join(missingBundleRoot, "ballydidean", "bundles"), {
    recursive: true,
  });
  await writeRuntimeRegistry(missingBundleRoot, bundle);
  cases.push(["missing bundle", missingBundleRoot, "bundle_missing"]);

  const wrongFilenameRoot = await mkdtemp(
    join(tmpdir(), "weather-api-wrong-filename-"),
  );
  await writeRuntimeBundle(wrongFilenameRoot, bundle);
  await writeRuntimeRegistry(wrongFilenameRoot, bundle, {
    path: "bundles/runtime.json",
  });
  cases.push(["wrong filename", wrongFilenameRoot, "registry_invalid"]);

  const substitutedReportRoot = await mkdtemp(
    join(tmpdir(), "weather-api-substituted-report-"),
  );
  const substitutedReportBundle = structuredClone(bundle);
  substitutedReportBundle.evaluationReport.candidateArtifactSha256 =
    "0".repeat(64);
  await writeRuntimeBundle(substitutedReportRoot, substitutedReportBundle);
  await writeRuntimeRegistry(substitutedReportRoot, bundle);
  cases.push(["substituted report", substitutedReportRoot, "bundle_invalid"]);

  const substitutedReceiptRoot = await mkdtemp(
    join(tmpdir(), "weather-api-substituted-receipt-"),
  );
  const substitutedReceiptBundle = structuredClone(bundle);
  substitutedReceiptBundle.qualificationReceipt.evaluationReportSha256 =
    "f".repeat(64);
  await writeRuntimeBundle(substitutedReceiptRoot, substitutedReceiptBundle);
  await writeRuntimeRegistry(substitutedReceiptRoot, bundle);
  cases.push(["substituted receipt", substitutedReceiptRoot, "bundle_invalid"]);

  const forbiddenFieldRoot = await mkdtemp(
    join(tmpdir(), "weather-api-forbidden-field-"),
  );
  const forbiddenFieldBundle = structuredClone(bundle);
  forbiddenFieldBundle.privateEvidencePath = "/secret/evidence";
  await writeRuntimeBundle(forbiddenFieldRoot, forbiddenFieldBundle);
  await writeRuntimeRegistry(forbiddenFieldRoot, bundle);
  cases.push(["forbidden field", forbiddenFieldRoot, "bundle_invalid"]);

  // exercise every loader failure through the public forecast response
  for (const [name, root, reasonCode] of cases) {
    const runtime = await createForecastAdjustmentRuntimeLoaderForRoot(root).load();
    const rawRow = matchingForecastRecord();
    const { handler } = createFixture(
      {
        // preserve the same v4 result for every invalid runtime
        async getForecast() {
          return [rawRow];
        },
      },
      {
        forecastAdjustment: {
          loadedAt: "2026-08-22T04:59:00.000Z",
          runtime,
        },
      },
    );
    const response = await handler(
      new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
    );
    const body = await response.json();

    assert.equal(response.status, 200, name);
    assert.equal(body.adjustmentRuntime.activeBundle, null, name);
    assert.equal(body.adjustmentRuntime.reasonCode, reasonCode, name);
    assert.equal(body.data[0].metrics.temperatureC, rawRow.temperatureC, name);
    assert.equal(body.data[0].adjustment.reasonCode, reasonCode, name);
    assert.doesNotMatch(JSON.stringify(body), /secret|privateEvidencePath/u);
  }
});

test("I-API-04 archive-only Previous Runs remains absent from live reads", async () => {
  const { handler } = createFixture();
  const sitesResponse = await handler(
    new Request("http://weather.test/api/v1/sites"),
  );
  const sites = await sitesResponse.json();
  const previousRunsResponse = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/previous-runs"),
  );

  assert.equal(previousRunsResponse.status, 404);
  assert.equal(
    JSON.stringify(sites).includes("open-meteo-previous-runs-v1"),
    false,
  );
});

test("daily precipitation uses the site-local day and nearest physical gauge", async () => {
  const { dailyPrecipitationQueries, handler } = createFixture();
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/daily-precipitation"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.deepEqual(dailyPrecipitationQueries, [{
    from: "2026-08-21T07:00:00.000Z",
    siteSlug: "ballydidean",
    to: "2026-08-22T05:00:00.000Z",
  }]);
  assert.deepEqual(body.data, {
    accumulationMm: 2.54,
    source: {
      sourceId: "13",
      stationSlug: "tempest-64255",
    },
    validThrough: "2026-08-22T04:59:00.000Z",
  });
});

test("forecast day windows follow site midnight through DST changes", () => {
  assert.deepEqual(
    siteForecastDayWindow(
      "2026-08-22T05:00:00.000Z",
      "America/Los_Angeles",
    ),
    { asOf: "2026-08-21T07:00:00.000Z", hours: 24 },
  );
  assert.deepEqual(
    siteForecastDayWindow(
      "2026-03-08T20:00:00.000Z",
      "America/Los_Angeles",
    ),
    { asOf: "2026-03-08T08:00:00.000Z", hours: 23 },
  );
  assert.deepEqual(
    siteForecastDayWindow(
      "2026-11-01T20:00:00.000Z",
      "America/Los_Angeles",
    ),
    { asOf: "2026-11-01T07:00:00.000Z", hours: 25 },
  );
  assert.deepEqual(
    siteForecastDayWindow(
      "2026-08-22T05:00:00.000Z",
      "America/Los_Angeles",
      10,
    ),
    { asOf: "2026-08-21T07:00:00.000Z", hours: 240 },
  );
  assert.deepEqual(
    siteForecastDayWindow(
      "2026-03-06T20:00:00.000Z",
      "America/Los_Angeles",
      5,
    ),
    { asOf: "2026-03-06T08:00:00.000Z", hours: 119 },
  );
});

// lock the overnight anchor boundary and daylight-saving capacities
test("overnight forecast windows run from anchor midnight through next 07:00", () => {
  assert.deepEqual(
    siteOvernightForecastWindow(
      "2026-09-12T13:59:59.999Z",
      "America/Los_Angeles",
    ),
    { asOf: "2026-09-11T07:00:00.000Z", hours: 31 },
  );
  assert.deepEqual(
    siteOvernightForecastWindow(
      "2026-09-12T14:00:00.000Z",
      "America/Los_Angeles",
    ),
    { asOf: "2026-09-12T07:00:00.000Z", hours: 31 },
  );
  assert.deepEqual(
    siteOvernightForecastWindow(
      "2026-03-08T15:00:00.000Z",
      "America/Los_Angeles",
    ),
    { asOf: "2026-03-08T08:00:00.000Z", hours: 30 },
  );
  assert.deepEqual(
    siteOvernightForecastWindow(
      "2026-11-01T16:00:00.000Z",
      "America/Los_Angeles",
    ),
    { asOf: "2026-11-01T07:00:00.000Z", hours: 32 },
  );
});


test("tides expose normalized NOAA levels and local event types", async () => {
  const { handler } = createFixture();
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/tides"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.deepEqual(body.data[0], {
    eventType: "high",
    kind: "prediction",
    source: {
      attribution: {
        label: "NOAA Tides & Currents",
        url: "https://tidesandcurrents.noaa.gov/",
      },
      providerKey: "noaa-co-ops",
      stationName: "Glendale Tide Predictions",
      stationSlug: "glendale-tide-predictions",
    },
    validAt: "2026-08-22T11:27:00.000Z",
    waterLevelM: 3.271,
  });
});

test("trends expose daily local-calendar history from 2019", async () => {
  const { handler, trendQueries } = createFixture();
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/trends"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(body.generatedAt, "2026-08-22T05:00:00.000Z");
  assert.deepEqual(trendQueries, [{
    from: "2019-01-01T08:00:00.000Z",
    siteSlug: "ballydidean",
    to: "2026-08-22T05:00:00.000Z",
  }]);
  assert.equal(body.data[0].validAt, "2026-08-22T04:00:00.000Z");
  assert.equal(body.data[0].metrics.temperatureC, 16.2);
  assert.equal(body.data[0].metrics.temperatureMaximumC, 19.4);
  assert.equal(body.data[0].metrics.temperatureMinimumC, 11.1);
  assert.equal(body.data[0].metrics.windDirectionDegrees, -135);

  assert.deepEqual(
    calendarTrendWindow("2026-08-22T05:00:00.000Z", "America/Los_Angeles"),
    { from: "2019-01-01T08:00:00.000Z" },
  );

  const invalid = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/trends?range=90d"),
  );
  assert.equal(invalid.status, 400);
});

test("invalid ranges, cursors, duplicate filters, sites, stations, and sources are bounded", async () => {
  const { handler } = createFixture();
  const cases = [
    ["/api/v1/sites/ballydidean/history?from=2026-08-22T01%3A00%3A00Z&to=2026-08-22T00%3A00%3A00Z", 400],
    ["/api/v1/sites/ballydidean/history?cursor=not-a-cursor", 400],
    ["/api/v1/sites/ballydidean/history?source=10&source=11", 400],
    ["/api/v1/sites/elsewhere/current", 404],
    ["/api/v1/sites/ballydidean/current?station=missing", 404],
    ["/api/v1/sites/ballydidean/current?source=999", 404],
    ["/api/v1/sites/ballydidean/current?unsupported=true", 400],
    ["/api/v1/sites/ballydidean/forecast?days=7", 400],
    ["/api/v1/sites/ballydidean/forecast?window=other", 400],
    ["/api/v1/sites/ballydidean/forecast?window=", 400],
    ["/api/v1/sites/ballydidean/forecast?window=overnight&days=1", 400],
    ["/api/v1/sites/ballydidean/forecast?window=overnight&window=overnight", 400],
  ];

  // verify each structured failure
  for (const [path, status] of cases) {
    const response = await handler(new Request(`http://weather.test${path}`));
    const body = await response.json();
    assert.equal(response.status, status, path);
    assert.equal(typeof body.error.code, "string");
    assert.equal(typeof body.error.message, "string");
  }
});

test("health is allowlisted and reports migration readiness and coarse freshness", async () => {
  const { handler } = createFixture();
  const response = await handler(
    new Request("http://weather.test/api/v1/health"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.deepEqual(body, {
    data: {
      adjustmentRuntime: inactiveAdjustmentRuntime,
      database: "ready",
      live: true,
      migration: { status: "current", version: "0001_initial_weather.sql" },
      ready: true,
      temperatureAdjustmentRuntime: inactiveTemperatureAdjustmentRuntime,
      version: "2026.08.22-1",
      worker: { freshness: "fresh" },
    },
  });
  assert.deepEqual(Object.keys(body.data).sort(), [
    "adjustmentRuntime",
    "database",
    "live",
    "migration",
    "ready",
    "temperatureAdjustmentRuntime",
    "version",
    "worker",
  ]);
});

test("health reports the bounded production release environment value", async () => {
  const version = readApiRelease({ WEATHER_RELEASE: "2026.08.22-9" });
  const { handler } = createFixture({}, { version });
  const response = await handler(
    new Request("http://weather.test/api/v1/health"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(body.data.version, "2026.08.22-9");
  assert.throws(
    () => readApiRelease({ WEATHER_RELEASE: " ".repeat(129) }),
    /WEATHER_RELEASE must be non-empty and bounded/u,
  );
});

test("unexpected failures return safe errors and emit redacted structured diagnostics", async () => {
  const diagnostics = [];
  const failure = new TypeError(
    "postgres://weather:super-secret@database/weather?sslkey=/run/secrets/key",
  );
  failure.code = "28P01";
  const { handler } = createFixture(
    {
      // fail before returning metadata
      async listSites() {
        throw failure;
      },
    },
    {
      // capture the internal boundary
      logDiagnostic(diagnostic) {
        diagnostics.push(diagnostic);
      },
    },
  );
  const response = await handler(
    new Request("http://weather.test/api/v1/sites"),
  );
  const body = await response.json();

  assert.equal(response.status, 500);
  assert.deepEqual(body, {
    error: { code: "internal_error", message: "Unexpected server error" },
  });
  assert.deepEqual(diagnostics, [
    {
      errorCode: "28P01",
      errorName: "TypeError",
      event: "api_request_failed",
      method: "GET",
      status: 500,
    },
  ]);
  assert.doesNotMatch(
    JSON.stringify({ body, diagnostics }),
    /super-secret|sslkey|postgres:\/\//u,
  );
});

test("health failures stay live, fail readiness, and redact raw database errors", async () => {
  const { handler } = createFixture({
    // simulate a safely classified database failure
    async getHealth() {
      return {
        database: "unavailable",
        migration: { status: "unavailable", version: null },
        workerLastLoopAt: null,
      };
    },
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/health"),
  );
  const body = await response.json();

  assert.equal(response.status, 503);
  assert.deepEqual(body.data, {
    adjustmentRuntime: inactiveAdjustmentRuntime,
    database: "unavailable",
    live: true,
    migration: { status: "unavailable", version: null },
    ready: false,
    temperatureAdjustmentRuntime: inactiveTemperatureAdjustmentRuntime,
    version: "2026.08.22-1",
    worker: { freshness: "unknown" },
  });
  assert.doesNotMatch(JSON.stringify(body), /password|postgres|host|stack|error/u);
});

test("future worker heartbeats are stale without changing database readiness", async () => {
  const { handler } = createFixture({
    // simulate clock skew beyond the health allowance
    async getHealth() {
      return {
        database: "ready",
        migration: { status: "current", version: "0001_initial_weather.sql" },
        workerLastLoopAt: "2026-08-22T05:30:01.000Z",
      };
    },
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/health"),
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(body.data.ready, true);
  assert.equal(body.data.worker.freshness, "stale");
});

test("Node server exposes the versioned handler end to end", async (context) => {
  const { handler } = createFixture();
  const server = createWeatherApiServer(handler);
  server.listen(0, "127.0.0.1");
  await once(server, "listening");

  // close the ephemeral server
  context.after(async () => {
    server.close();
    await once(server, "close");
  });

  const address = server.address();
  assert.ok(address && typeof address === "object");
  const response = await fetch(
    `http://127.0.0.1:${String(address.port)}/api/v1/sites`,
  );
  const body = await response.json();

  assert.equal(response.status, 200);
  assert.equal(body.data[0].name, "Ballydidean");
});

test("Node server adapter redacts rejected handler diagnostics", async (context) => {
  const diagnostics = [];
  const failure = new Error("authorization=Bearer super-secret");
  failure.code = "ERR_HANDLER";
  const server = createWeatherApiServer(
    // reject outside the fetch-compatible boundary
    async () => {
      throw failure;
    },
    {
      // capture the server boundary
      logDiagnostic(diagnostic) {
        diagnostics.push(diagnostic);
      },
    },
  );
  server.listen(0, "127.0.0.1");
  await once(server, "listening");

  // close the ephemeral server
  context.after(async () => {
    server.close();
    await once(server, "close");
  });

  const address = server.address();
  assert.ok(address && typeof address === "object");
  const response = await fetch(`http://127.0.0.1:${String(address.port)}/`);
  const body = await response.json();

  assert.equal(response.status, 500);
  assert.deepEqual(body, {
    error: { code: "internal_error", message: "Unexpected server error" },
  });
  assert.deepEqual(diagnostics, [
    {
      errorCode: "ERR_HANDLER",
      errorName: "Error",
      event: "api_request_failed",
      method: "GET",
      status: 500,
    },
  ]);
  assert.doesNotMatch(
    JSON.stringify({ body, diagnostics }),
    /super-secret|authorization|Bearer/u,
  );
});
