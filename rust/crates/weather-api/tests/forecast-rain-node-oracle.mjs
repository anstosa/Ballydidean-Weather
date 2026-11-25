import assert from "node:assert/strict";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";
import {
  RAIN_HURDLE_WIND_MODEL_SHA256,
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

// build one public forecast row with independently variable source and raw identity
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

// build one complete public-safe retained rain run
function rainRun(hourOverrides = {}, runOverrides = {}) {
  return {
    decisionAt: "2026-08-22T02:00:00.000Z",
    firstReceivedAt: "2026-08-22T00:05:00.000Z",
    forecastClaimId: "private-claim",
    generatedAt: "2026-08-22T02:05:00.000Z",
    hours: [{
      applied: true,
      correctedPrecipitationMm: 0.05,
      modelLeadHours: 12,
      rawPrecipitationMm: 0.1,
      reasonCode: null,
      validAt: "2026-08-22T06:00:00.000Z",
      ...hourOverrides,
    }],
    inputSha256: "a".repeat(64),
    modelSha256: RAIN_HURDLE_WIND_MODEL_SHA256,
    runInitializedAt: "2026-08-21T18:00:00.000Z",
    ...runOverrides,
  };
}

const activeSelection = {
  loadedAt,
  runtime: {
    artifactSha256: RAIN_HURDLE_WIND_MODEL_SHA256,
    comparatorAuthority: {},
    reasonCode: null,
    state: "active",
  },
};

// execute one compiled handler case without file, network, or database access
async function readForecast({
  method = "GET",
  readState = "present",
  row = forecastRow(),
  run = rainRun(),
  safetyAt = dataAt,
  selection = activeSelection,
} = {}) {
  const reads = [];
  const options = {
    dataAt,
    now: () => new Date(safetyAt),
  };
  // omit the startup selection entirely for the unconfigured compatibility case
  if (selection !== null) {
    options.rainAdjustment = selection;
  }
  const handler = createWeatherApi({
    async getForecast() {
      return [row];
    },
    async getRainAdjustmentRun(siteSlug, asOf) {
      reads.push({ asOf, siteSlug });
      // expose only the bounded optional read failure under test
      if (readState === "failure") {
        throw new Error("fixture retained rain read failed");
      }
      return readState === "missing" ? null : run;
    },
    async listSites() {
      return [source];
    },
  }, options);
  const response = await handler(new Request(
    "http://weather.test/api/v1/sites/ballydidean/forecast",
    { method },
  ));
  assert.equal(response.status, 200);
  // preserve transport-level HEAD omission after completing the same service reads
  if (method === "HEAD") {
    assert.equal(await response.text(), "");
    return { body: null, reads };
  }
  const body = await response.json();
  assert.equal(body.generatedAt, dataAt);
  assert.equal(body.data[0].metrics.precipitationMm, row.precipitationMm);
  return { body, reads };
}

const active = await readForecast();
assert.deepEqual(active.reads, [{ asOf: dataAt, siteSlug: "ballydidean" }]);
assert.deepEqual(active.body.data[0].rainAdjustment, {
  bundleSha256: RAIN_HURDLE_WIND_MODEL_SHA256,
  contractVersion: "forecast-rain-adjustment-decision/v1",
  correctedPrecipitationMm: 0.05,
  rawBestMatchPrecipitationMm: 0.1,
  reasonCode: null,
  sourceForecast: {
    decisionAt: "2026-08-22T02:00:00.000Z",
    firstReceivedAt: "2026-08-22T00:05:00.000Z",
    modelLeadHours: 12,
    providerKey: "open-meteo",
    rawPrecipitationMm: 0.1,
    runInitializedAt: "2026-08-21T18:00:00.000Z",
    upstreamModel: "ecmwf_ifs",
    validAt: "2026-08-22T06:00:00.000Z",
  },
  state: "active",
});
assert.deepEqual(active.body.rainAdjustmentRuntime, {
  activeBundle: RAIN_HURDLE_WIND_MODEL_SHA256,
  loadedAt,
  reasonCode: null,
  source: {
    decisionAt: "2026-08-22T02:00:00.000Z",
    firstReceivedAt: "2026-08-22T00:05:00.000Z",
    hourCount: 1,
    runInitializedAt: "2026-08-21T18:00:00.000Z",
  },
  state: "active",
});

// keep valid lifecycle evidence while individual row guards fail raw
const missingRaw = await readForecast({ row: forecastRow({ precipitationMm: null }) });
assert.equal(missingRaw.body.data[0].rainAdjustment.state, "raw_fallback");
assert.equal(
  missingRaw.body.data[0].rainAdjustment.reasonCode,
  "outside_operational_window",
);
assert.equal(missingRaw.body.rainAdjustmentRuntime.state, "active");

const sourceMismatch = await readForecast({
  row: forecastRow({ upstreamModel: "ecmwf_ifs" }),
});
assert.equal(sourceMismatch.body.data[0].rainAdjustment.state, "disabled");
assert.equal(
  sourceMismatch.body.data[0].rainAdjustment.reasonCode,
  "model_unavailable",
);
assert.equal(sourceMismatch.body.rainAdjustmentRuntime.state, "active");

const unapplied = await readForecast({
  run: rainRun({
    applied: false,
    correctedPrecipitationMm: 0.1,
    reasonCode: "raw_below_hurdle",
  }),
});
assert.equal(unapplied.body.data[0].rainAdjustment.state, "raw_fallback");
assert.equal(unapplied.body.data[0].rainAdjustment.reasonCode, "raw_below_hurdle");
assert.equal(unapplied.body.rainAdjustmentRuntime.state, "disabled");

// contain missing, failed, malformed, and expired retained evidence identically
for (const options of [
  { readState: "missing" },
  { readState: "failure" },
  { run: rainRun({}, { generatedAt: "2026-08-22T11:00:00.000Z" }) },
  { safetyAt: "2026-08-22T14:00:00.000Z" },
]) {
  const fallback = await readForecast(options);
  assert.equal(fallback.body.data[0].rainAdjustment.state, "disabled");
  assert.equal(
    fallback.body.data[0].rainAdjustment.reasonCode,
    "model_unavailable",
  );
  assert.equal(fallback.body.rainAdjustmentRuntime.state, "disabled");
  assert.equal(
    fallback.body.rainAdjustmentRuntime.reasonCode,
    "model_unavailable",
  );
}

const policyRaw = await readForecast({
  selection: {
    loadedAt: "2026-08-22T04:58:00.000Z",
    runtime: { artifactSha256: null, reasonCode: "policy_raw", state: "disabled" },
  },
});
assert.deepEqual(policyRaw.reads, []);
assert.equal(policyRaw.body.data[0].rainAdjustment.reasonCode, "policy_raw");
assert.deepEqual(policyRaw.body.rainAdjustmentRuntime, {
  activeBundle: null,
  loadedAt: "2026-08-22T04:58:00.000Z",
  reasonCode: "policy_raw",
  source: null,
  state: "disabled",
});

const unconfigured = await readForecast({ selection: null });
assert.deepEqual(unconfigured.reads, [{ asOf: dataAt, siteSlug: "ballydidean" }]);
assert.equal(unconfigured.body.data[0].rainAdjustment.state, "active");
assert.equal(unconfigured.body.rainAdjustmentRuntime.state, "active");
assert.equal(unconfigured.body.rainAdjustmentRuntime.loadedAt, dataAt);

const head = await readForecast({ method: "HEAD" });
assert.equal(head.body, null);
assert.deepEqual(head.reads, [{ asOf: dataAt, siteSlug: "ballydidean" }]);

process.stdout.write("forecast rain active/raw/missing/error Node oracle passed\n");
