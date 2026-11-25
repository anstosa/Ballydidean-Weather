import assert from "node:assert/strict";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";
import {
  createForecastAdjustmentTemperatureCanaryAuthorization,
  createForecastAdjustmentTemperatureCanaryRuntimeBundle,
  runtimeCalendarFingerprint,
} from "../../../../packages/forecast-adjustment/dist/index.js";

const dataAt = "2026-08-22T05:00:00.000Z";
const loadedAt = "2026-08-22T04:59:00.000Z";
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
  stationLatitude: 47.96,
  stationLongitude: -122.43,
  stationName: "Open-Meteo",
  stationSlug: "open-meteo-virtual",
  timezone: "America/Los_Angeles",
};

// build one verified active temperature runtime without file or host access
function temperatureRuntime() {
  const cutoff = "2026-07-25T07:00:00.000Z";
  const directCoefficients = Array(35).fill(0);
  directCoefficients[0] = 1;
  const authorization = createForecastAdjustmentTemperatureCanaryAuthorization({
    activatedAt: "2026-08-20T00:00:00.000Z",
    authorizationReason: "operator-approved temperature oracle",
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
      learnedStrengthContractVersion: "temperature-winner-extensions-research/v1",
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

// build one public Best Match row whose raw temperature must never change
function forecastRow(overrides = {}) {
  return {
    adapterVersion: "open-meteo-forecast-daily/v4",
    apparentTemperatureC: 11.7,
    blackGlobeTemperatureC: null,
    cloudCoverPercent: 72,
    contentHash: "sha256-content",
    contractEpoch: "v4",
    deviceModel: null,
    deviceSerial: null,
    deviceVendor: null,
    firstReceivedAt: dataAt,
    id: "forecast-record-12",
    lastReceivedAt: dataAt,
    pm25MicrogramsPerCubicMeter: 5,
    precipitationMm: 0.1,
    precipitationRateMmPerHour: 0.1,
    pressureHpa: 1018.1,
    productRunAt: "2026-08-21T18:00:00.000Z",
    providerKey: "open-meteo",
    providerMetadata: { dataset: "forecast" },
    qualityMetadata: null,
    relativeHumidityPercent: 83,
    revisionCount: 0,
    siteSlug: "ballydidean",
    sourceConfigFingerprint: "fingerprint",
    sourceId: "12",
    sourceKey: "open-meteo-forecast-v4",
    sourceKind: "forecast",
    stationSlug: "open-meteo-virtual",
    soilElectricalConductivityMicrosiemensPerCm: null,
    soilMoisturePercent: null,
    solarRadiationWm2: null,
    temperatureC: 12.1,
    upstreamModel: "best_match",
    upstreamTimezone: "GMT",
    uvIndex: 0.2,
    validAt: "2026-08-22T06:00:00.000Z",
    waterLevelM: null,
    wetBulbGlobeTemperatureC: null,
    windDirectionDegrees: 180,
    windGustMps: 3.1,
    windSpeedMps: 1.8,
    ...overrides,
  };
}

// build one single-run sidecar with a matching public hour
function temperatureSidecar() {
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

// execute one compiled route case through its real runtime projection
async function readForecast({
  applyFailure = false,
  row = forecastRow(),
  safetyAt = dataAt,
  sidecarFailure = false,
} = {}) {
  const runtime = temperatureRuntime();
  const sidecar = temperatureSidecar();
  const sidecarReads = [];
  const statusReads = [];
  const temperatureAdjustment = {
    loadedAt,
    runtime,
  };
  // inject only the optional inference exception under test
  if (applyFailure) {
    temperatureAdjustment.apply = () => {
      throw new Error("fixture temperature inference failed");
    };
  }
  const handler = createWeatherApi({
    async getForecast() {
      return [row];
    },
    async getTemperatureCanarySidecar(siteSlug, asOf, from, to) {
      sidecarReads.push({ asOf, from, siteSlug, to });
      if (sidecarFailure) {
        throw new Error("fixture temperature sidecar failed");
      }
      return sidecar;
    },
    async getTemperatureCanaryStatus(siteSlug, asOf) {
      statusReads.push({ asOf, siteSlug });
      if (sidecarFailure) {
        throw new Error("fixture temperature status failed");
      }
      return {
        firstReceivedAt: sidecar.run.firstReceivedAt,
        hourCount: sidecar.hours.length,
        latestRunInitializedAt: sidecar.run.runInitializedAt,
        stateReason: sidecar.run.stateReason,
        stateStatus: sidecar.run.stateStatus,
      };
    },
    async listSites() {
      return [source];
    },
  }, {
    dataAt,
    forecastAdjustment: {
      loadedAt: dataAt,
      runtime: { bundle: null, reasonCode: "registry_inactive", state: "disabled" },
    },
    now: () => new Date(safetyAt),
    temperatureAdjustment,
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();
  assert.equal(response.status, 200);
  assert.equal(body.generatedAt, dataAt);
  assert.equal(body.data[0].metrics.temperatureC, 12.1);
  assert.deepEqual(sidecarReads, [{
    asOf: dataAt,
    from: "2026-08-21T07:00:00.000Z",
    siteSlug: "ballydidean",
    to: "2026-08-22T07:00:00.000Z",
  }]);
  assert.deepEqual(statusReads, [{ asOf: safetyAt, siteSlug: "ballydidean" }]);
  return body;
}

const active = await readForecast();
assert.equal(active.data[0].temperatureAdjustment.state, "active");
assert.equal(active.data[0].temperatureAdjustment.branch, "direct");
assert.equal(active.data[0].temperatureAdjustment.correctedTemperatureC, 12);
assert.equal(
  active.data[0].temperatureAdjustment.recentErrorStateSha256,
  "70239b1e0bcc1171e36838ed4e1c4e508efa0a8c30f70393dfcad4df1a5115bf",
);
assert.equal(
  active.data[0].temperatureAdjustment.sourceForecast.operationalHorizonHours,
  6,
);
assert.equal(active.temperatureAdjustmentRuntime.state, "active");
assert.equal(active.temperatureAdjustmentRuntime.source.stateStatus, "cold");

const stale = await readForecast({ safetyAt: "2026-08-22T13:06:00.000Z" });
assert.equal(stale.data[0].temperatureAdjustment.state, "raw_fallback");
assert.equal(stale.data[0].temperatureAdjustment.reasonCode, "source_stale");
assert.equal(stale.data[0].temperatureAdjustment.correctedTemperatureC, null);
assert.equal(
  stale.data[0].temperatureAdjustment.bundleSha256,
  active.data[0].temperatureAdjustment.bundleSha256,
);
assert.equal(stale.data[0].temperatureAdjustment.recentErrorStateSha256, null);
assert.equal(stale.data[0].temperatureAdjustment.sourceForecast, null);

const missingSource = await readForecast({
  row: forecastRow({ upstreamModel: "ecmwf_ifs" }),
});
assert.equal(missingSource.data[0].temperatureAdjustment.state, "raw_fallback");
assert.equal(
  missingSource.data[0].temperatureAdjustment.reasonCode,
  "missing_source_forecast",
);

const readFailure = await readForecast({ sidecarFailure: true });
assert.equal(readFailure.data[0].temperatureAdjustment.state, "raw_fallback");
assert.equal(
  readFailure.data[0].temperatureAdjustment.reasonCode,
  "missing_source_forecast",
);
assert.equal(readFailure.temperatureAdjustmentRuntime.state, "active");
assert.equal(readFailure.temperatureAdjustmentRuntime.source, null);

const inferenceFailure = await readForecast({ applyFailure: true });
assert.equal(inferenceFailure.data[0].temperatureAdjustment.state, "disabled");
assert.equal(
  inferenceFailure.data[0].temperatureAdjustment.reasonCode,
  "bundle_invalid",
);
assert.equal(inferenceFailure.data[0].temperatureAdjustment.bundleSha256, null);

// execute one health request through request-time lifecycle and optional status projection
async function readHealth({
  runtime = temperatureRuntime(),
  safetyAt = dataAt,
  statusFailure = false,
} = {}) {
  const statusReads = [];
  const sidecar = temperatureSidecar();
  const handler = createWeatherApi({
    async getHealth() {
      return {
        database: "ready",
        migration: { status: "current", version: "2026082201" },
        workerLastLoopAt: "2026-08-22T04:50:00.000Z",
      };
    },
    async getTemperatureCanaryStatus(siteSlug, asOf) {
      statusReads.push({ asOf, siteSlug });
      // expose only the bounded optional monitoring failure under test
      if (statusFailure) {
        throw new Error("fixture temperature status failed");
      }
      return {
        firstReceivedAt: sidecar.run.firstReceivedAt,
        hourCount: sidecar.hours.length,
        latestRunInitializedAt: sidecar.run.runInitializedAt,
        stateReason: sidecar.run.stateReason,
        stateStatus: sidecar.run.stateStatus,
      };
    },
    async listSites() {
      assert.fail("health must not discover sites");
    },
  }, {
    dataAt,
    now: () => new Date(safetyAt),
    temperatureAdjustment: { loadedAt, runtime },
  });
  const response = await handler(new Request("http://weather.test/api/v1/health"));
  const body = await response.json();
  assert.equal(response.status, 200);
  return { body, statusReads };
}

const activeHealth = await readHealth();
assert.deepEqual(activeHealth.statusReads, [{
  asOf: dataAt,
  siteSlug: "ballydidean",
}]);
assert.equal(activeHealth.body.data.temperatureAdjustmentRuntime.state, "active");
assert.equal(
  activeHealth.body.data.temperatureAdjustmentRuntime.source.stateStatus,
  "cold",
);

const failedHealthStatus = await readHealth({ statusFailure: true });
assert.equal(
  failedHealthStatus.body.data.temperatureAdjustmentRuntime.state,
  "active",
);
assert.equal(
  failedHealthStatus.body.data.temperatureAdjustmentRuntime.source,
  null,
);

const activeTemperature = temperatureRuntime();
const expiredHealth = await readHealth({
  runtime: activeTemperature,
  safetyAt: activeTemperature.bundle.authorization.expiresAt,
});
assert.deepEqual(expiredHealth.statusReads, []);
assert.equal(expiredHealth.body.data.temperatureAdjustmentRuntime.state, "disabled");
assert.equal(
  expiredHealth.body.data.temperatureAdjustmentRuntime.reasonCode,
  "canary_expired",
);
assert.equal(expiredHealth.body.data.temperatureAdjustmentRuntime.activeBundle, null);
assert.equal(expiredHealth.body.data.temperatureAdjustmentRuntime.source, null);

const disabledHealth = await readHealth({
  runtime: { bundle: null, reasonCode: "registry_inactive", state: "disabled" },
});
assert.deepEqual(disabledHealth.statusReads, []);
assert.equal(disabledHealth.body.data.temperatureAdjustmentRuntime.state, "disabled");
assert.equal(
  disabledHealth.body.data.temperatureAdjustmentRuntime.reasonCode,
  "registry_inactive",
);

process.stdout.write("forecast temperature and health active/raw/error Node oracle passed\n");
