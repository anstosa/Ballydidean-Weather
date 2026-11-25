import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";

const bundle = JSON.parse(await readFile(new URL(
  "../../../../config/forecast-adjustments/ballydidean/wind-canary-bundles/sha256-5e8b2e3932111621af6785a1b16dfd22edc0a2d26059c6e396654c70119abbe1.json",
  import.meta.url,
), "utf8"));
const dataAt = "2026-09-09T05:00:00.000Z";
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

// build one exact served forecast row
function forecastRow(overrides = {}) {
  return {
    adapterVersion: "open-meteo-forecast-daily/v4",
    apparentTemperatureC: 11.7,
    blackGlobeTemperatureC: null,
    cloudCoverPercent: 72,
    contentHash: "sha256-content",
    contractEpoch:
      "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83",
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
    productRunAt: "2026-09-08T18:00:00.000Z",
    providerKey: "open-meteo",
    providerMetadata: {
      dataset: "forecast",
      elevation_m: 32,
      grid_cell: "47.941,-122.438",
    },
    qualityMetadata: null,
    relativeHumidityPercent: 83,
    revisionCount: 0,
    siteSlug: "ballydidean",
    sourceConfigFingerprint:
      "ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4",
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
    validAt: "2026-09-09T06:00:00.000Z",
    waterLevelM: null,
    wetBulbGlobeTemperatureC: null,
    windDirectionDegrees: 180,
    windGustMps: 3.1,
    windSpeedMps: 1.8,
    ...overrides,
  };
}

// route one row through the compiled API and real wind runtime
async function readForecast(
  row,
  evaluatedAt,
  pressureState = "failure",
  applicationFailure = false,
) {
  const forecastAdjustment = {
    loadedAt: bundle.authorization.activatedAt,
    runtime: { bundle, reasonCode: null, state: "active" },
  };
  // exercise the API's bounded per-row application exception fallback
  if (applicationFailure) {
    forecastAdjustment.apply = () => {
      throw new Error("fixture wind application failed");
    };
  }
  const handler = createWeatherApi({
    // return the selected forecast row
    async getForecast() {
      return [row];
    },
    // preserve optional pressure context independently from active wind serving
    async getForecastPressureContext() {
      if (pressureState === "failure") {
        throw new Error("fixture pressure read failed");
      }
      return pressureState === "present"
        ? [forecastRow({ id: "pressure-context" })]
        : [];
    },
    // return the active source catalog
    async listSites() {
      return [source];
    },
  }, {
    dataAt,
    forecastAdjustment,
    now: () => new Date(evaluatedAt),
  });
  const response = await handler(
    new Request("http://weather.test/api/v1/sites/ballydidean/forecast"),
  );
  const body = await response.json();
  assert.equal(response.status, 200);
  assert.equal(body.data.length, 1);
  assert.equal(body.generatedAt, dataAt);
  assert.equal(body.pressureContext.length, pressureState === "present" ? 1 : 0);
  assert.equal(body.adjustmentRuntime.loadedAt, bundle.authorization.activatedAt);
  // expose candidate evidence only while the request safety clock authorizes it
  if (evaluatedAt === bundle.authorization.expiresAt) {
    assert.equal(body.adjustmentRuntime.state, "disabled");
    assert.equal(body.adjustmentRuntime.reasonCode, "canary_expired");
    assert.equal(body.adjustmentRuntime.activeBundle, null);
  } else {
    assert.equal(body.adjustmentRuntime.state, "active");
    assert.equal(body.adjustmentRuntime.activationMode, "wind_canary");
    assert.equal(body.adjustmentRuntime.activeBundle, bundle.bundleSha256);
    assert.deepEqual(body.adjustmentRuntime.enabledMetrics, [
      "windGustMps",
      "windSpeedMps",
    ]);
  }
  return body.data[0];
}

const active = await readForecast(forecastRow(), dataAt, "present");
assert.equal(active.metrics.windDirectionDegrees, 180);
assert.equal(active.metrics.windGustMps, 3.1);
assert.equal(active.metrics.windSpeedMps, 1.8);
assert.deepEqual(active.adjustment, {
  activationKind: "wind_transfer_canary",
  adjustedMetrics: { windGustMps: 0, windSpeedMps: 0 },
  algorithmContractVersion: "robust-hierarchical-median/v1",
  appliedMetrics: ["windGustMps", "windSpeedMps"],
  authorizationSha256: bundle.authorization.authorizationSha256,
  candidateArtifactSha256: bundle.candidate.candidateArtifactSha256,
  contractVersion: "forecast-adjustment-decision/v1",
  leadBand: "001-024",
  rawForecastProvenance: {
    ...bundle.candidate.servedForecastIdentity,
    referenceAt: "2026-09-08T18:00:00.000Z",
    targetLeadHours: 12,
    validAt: "2026-09-09T06:00:00.000Z",
  },
  reasonCode: null,
  state: "active",
  transferReportSha256: bundle.transferReport.transferReportSha256,
});

const nullGust = await readForecast(forecastRow({ windGustMps: null }), dataAt);
assert.equal(nullGust.metrics.windGustMps, null);
assert.deepEqual(nullGust.adjustment.adjustedMetrics, { windSpeedMps: 0 });
assert.deepEqual(nullGust.adjustment.appliedMetrics, ["windSpeedMps"]);

const sourceMismatch = await readForecast(
  forecastRow({ sourceKey: "open-meteo-forecast-v5" }),
  dataAt,
);
assert.deepEqual(sourceMismatch.adjustment, {
  adjustedMetrics: {},
  appliedMetrics: [],
  contractVersion: "forecast-adjustment-decision/v1",
  reasonCode: "identity_mismatch",
  state: "not_applicable",
});
assert.equal(sourceMismatch.metrics.windGustMps, 3.1);

const expiredMismatch = await readForecast(
  forecastRow({ sourceKey: "open-meteo-forecast-v5" }),
  bundle.authorization.expiresAt,
);
assert.deepEqual(expiredMismatch.adjustment, {
  adjustedMetrics: {},
  appliedMetrics: [],
  contractVersion: "forecast-adjustment-decision/v1",
  reasonCode: "canary_expired",
  state: "disabled",
});

const applicationFailure = await readForecast(
  forecastRow(),
  dataAt,
  "failure",
  true,
);
assert.equal(applicationFailure.metrics.windGustMps, 3.1);
assert.equal(applicationFailure.metrics.windSpeedMps, 1.8);
assert.deepEqual(applicationFailure.adjustment, {
  adjustedMetrics: {},
  appliedMetrics: [],
  contractVersion: "forecast-adjustment-decision/v1",
  reasonCode: "adjustment_error",
  state: "not_applicable",
});

process.stdout.write("forecast wind active/fail-raw Node oracle passed\n");
