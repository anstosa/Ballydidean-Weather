import assert from "node:assert/strict";
import { once } from "node:events";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { createServer } from "node:http";
import { join, resolve } from "node:path";
import test from "node:test";

import { chromium } from "playwright";
import {
  eveningSunTimes,
  FORECAST_ADJUSTMENT_MODE_STORAGE_KEY,
  parseForecastRecordsResponse,
  UNIT_PREFERENCE_STORAGE_KEY,
} from "../dist/index.js";

const repositoryRoot = resolve(import.meta.dirname, "../../..");
const publicRoot = join(repositoryRoot, "apps/web/public");
const distRoot = join(repositoryRoot, "apps/web/dist");
const fixtureAssetVersion = "browser-test";
const fixtureAdminSession = "fixture-admin-session";
const transparentPng = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
  "base64",
);

const site = {
  latitude: 47.950429954185445,
  longitude: -122.42797012608193,
  name: "Ballydidean",
  slug: "ballydidean",
  stations: [
    {
      kind: "virtual",
      latitude: 47.950429954185445,
      longitude: -122.42797012608193,
      name: "Open-Meteo virtual station",
      slug: "open-meteo-virtual",
      sources: [
        {
          attribution: {
            label: "Weather data by Open-Meteo",
            url: "https://open-meteo.com/",
          },
          id: "10",
          key: "open-meteo-current-v1",
          kind: "model_current",
          providerKey: "open-meteo",
          providerName: "Open-Meteo",
          provenanceLabel: "model-derived current conditions",
        },
        {
          attribution: {
            label: "Weather data by Open-Meteo",
            url: "https://open-meteo.com/",
          },
          id: "11",
          key: "open-meteo-reanalysis-v1",
          kind: "reanalysis",
          providerKey: "open-meteo",
          providerName: "Open-Meteo",
          provenanceLabel: "historical reanalysis",
        },
        {
          attribution: {
            label: "Weather data by Open-Meteo",
            url: "https://open-meteo.com/",
          },
          id: "12",
          key: "open-meteo-forecast-v1",
          kind: "forecast",
          providerKey: "open-meteo",
          providerName: "Open-Meteo",
          provenanceLabel: "hourly forecast",
        },
      ],
    },
    {
      kind: "physical",
      latitude: 47.95293,
      longitude: -122.41414,
      name: "Fiske Rd & Paris Pl",
      slug: "tempest-38270",
      sources: [
        {
          attribution: {
            label: "Weather data by Tempest",
            url: "https://tempestwx.com/",
          },
          id: "13",
          key: "tempest-38270-observations-v1",
          kind: "physical_sensor",
          providerKey: "weatherflow-tempest",
          providerName: "WeatherFlow Tempest",
          provenanceLabel: "nearby physical station",
        },
      ],
    },
  ],
  timezone: "America/Los_Angeles",
};

const current = { ...makeRecord("101", "2026-08-22T04:50:00.000Z", 16.2), pressureChange3hHpa: -1.2 };
const older = makeRecord("100", "2026-08-21T04:50:00.000Z", 15.1);
const physicalCurrent = {
  ...makeRecord("301", "2026-08-22T04:52:00.000Z", 12),
  freshness: {
    ageSeconds: 120,
    label: "Station reading is current",
    status: "fresh",
  },
  metadata: {
    ...makeRecord("301", "2026-08-22T04:52:00.000Z", 12).metadata,
    provider: {
      dataset: "get_livedata_info",
      elevationM: null,
      gridCell: null,
      propertySensors: [
        {
          channel: 1,
          key: "soil-1",
          model: "WH52",
          readings: {
            soilMoisturePercent: 42,
            temperatureC: 17.7,
          },
        },
        {
          channel: null,
          key: "gateway",
          model: "GW3000",
          readings: {
            relativeHumidityPercent: 61,
            temperatureC: 21.5,
          },
        },
        {
          channel: 1,
          key: "temperature-1",
          model: "WN31",
          readings: {
            relativeHumidityPercent: 68,
            temperatureC: 20,
          },
        },
        {
          channel: 2,
          key: "temperature-2",
          model: "WN31",
          readings: {
            relativeHumidityPercent: 73,
            temperatureC: 16.5,
          },
        },
      ],
    },
  },
  metrics: {
    ...makeRecord("301", "2026-08-22T04:52:00.000Z", 12).metrics,
    pm25MicrogramsPerCubicMeter: 10,
    pressureHpa: 1013.5,
    relativeHumidityPercent: 81,
    uvIndex: 1,
    windGustMps: 4.5,
    windSpeedMps: 2.5,
  },
  provenance: {
    ...current.provenance,
    label: "nearby physical station",
    providerKey: "weatherflow-tempest",
    sourceId: "13",
    sourceKey: "tempest-38270-observations-v1",
    sourceKind: "physical_sensor",
    stationSlug: "tempest-38270",
  },
};
const forecast = Array.from({ length: 240 }, (_, index) => makeForecastRecord(index));
const adjustmentHashes = {
  bundle: "a".repeat(64),
  candidate: "b".repeat(64),
  report: "c".repeat(64),
  receipt: "d".repeat(64),
  source: "e".repeat(64),
};

// create one browser-visible aggregate scorecard
function adjustmentScorecardFixture() {
  const support = {
    dateCount: 30,
    validHourCount: 240,
    vintageCount: 12,
    targetRowCount: 260,
    rowCount: 240,
    eventCount: 18,
    wetDateCount: 14,
    wetRowCount: 52,
    effectiveWeightSum: 231.5,
    fallbackCount: 3,
    gapCount: 5,
    excludedCount: 20,
    exclusionReasons: { missing_observation: 20 },
    fallbackReasons: { unsupported_lead: 3 },
  };
  // create one unit-bound aggregate metric
  const metric = (unit) => ({
    unit,
    rawMae: 2,
    adjustedMae: 1.75,
    deltaMae: -0.25,
    rawBias: 0.4,
    adjustedBias: 0.1,
    rawRmse: 2.7,
    adjustedRmse: 2.3,
    rawP95: 5.4,
    adjustedP95: 4.8,
    skillPercent: 12.5,
    skillInterval95: { lower: 4.2, upper: 19.1 },
  });
  // create one closed family card
  const family = (name, overrides = {}) => {
    const unit = name === "temperature"
      ? "celsius"
      : name === "wind"
        ? "meters_per_second"
        : "millimeters_per_hour";
    return {
      family: name,
      servingIdentitySha256: "4".repeat(64),
      evidenceClass: "as_issued",
      evidenceCutoffAt: "2026-09-30T23:59:59.000Z",
      supportState: "sufficient",
      comparisonState: "better",
      qualificationState: "supported",
      servingState: "authorized_active",
      recommendation: "retain",
      support,
      metrics: metric(unit),
      slices: [{ dimension: "horizon", label: "001-024", rowCount: 120, metrics: metric(unit) }],
      rainDiagnostics: null,
      bestMatchDiagnostic: name === "wind" ? null : {
        rowCount: 120,
        dateCount: 24,
        bestMatchRawMae: 2.2,
        sourceRawMae: 2,
        sourceAdjustedMae: 1.75,
        unit,
      },
      ...overrides,
    };
  };
  const reliability = Array.from({ length: 10 },
    // preserve every fixed reliability bin
    (_, index) => ({
      count: index,
      meanProbability: index === 0 ? null : index / 10,
      observedFrequency: index === 0 ? null : Math.min(1, index / 10 + 0.02),
    }),
  );
  const rainDiagnostics = {
    annualBalancedVolumeRatio: 0.98,
    winterBalancedVolumeRatio: 1.04,
    wetRawMae: 1.2,
    wetAdjustedMae: 1.1,
    heavyRawMae: 3.2,
    heavyAdjustedMae: 3,
    probabilityOrderViolationCount: 0,
    thresholds: [0.1, 1, 2.5].map(
      // create one fixed rain threshold
      (thresholdMmPerHour) => ({
        thresholdMmPerHour,
        rawBrier: 0.2,
        adjustedBrier: 0.18,
        hits: 20,
        misses: 4,
        falseAlarms: 3,
        pod: 0.83,
        far: 0.13,
        csi: 0.74,
        reliability,
      }),
    ),
    accumulations: [6, 12, 23].map(
      // create one fixed same-run accumulation window
      (hours) => ({ hours, completeWindows: 14, rawMae: 2.4, adjustedMae: 2.1 }),
    ),
  };
  return {
    contractVersion: "forecast-adjustment-scorecard/v1",
    siteKey: "ballydidean",
    generatedAt: "2026-10-01T00:00:00.000Z",
    validThrough: "2099-10-08T00:00:00.000Z",
    servingChanged: false,
    automaticActivationEligible: false,
    operatorApprovalRequired: true,
    inputs: {
      adjustmentEvidenceManifestSha256: "a".repeat(64),
      adjustmentEvidenceWatermarkSha256: "b".repeat(64),
      forecastTrainingManifestSha256: "c".repeat(64),
      localDateFrom: "2026-09-01",
      localDateTo: "2026-09-30",
      reportSha256s: {
        temperature: "d".repeat(64),
        wind: "e".repeat(64),
        rain: "f".repeat(64),
      },
      sourceRevision: "a".repeat(40),
      targetCutoffAt: "2026-09-30T23:59:59.000Z",
    },
    families: {
      temperature: family("temperature"),
      wind: family("wind", {
        evidenceClass: "prospective_receipt",
        supportState: "insufficient",
        comparisonState: "mixed",
        qualificationState: "pending_support",
        servingState: "admin_disabled",
        recommendation: "review_candidate",
      }),
      rain: family("rain", {
        evidenceClass: "retrospective_counterfactual",
        comparisonState: "worse",
        qualificationState: "rejected",
        servingState: "fail_raw",
        recommendation: "review_disable",
        rainDiagnostics,
      }),
    },
  };
}

// create one sanitized pending automatic-policy projection
function adjustmentScorecardV2Fixture() {
  const legacy = adjustmentScorecardFixture();
  const identities = (value) => ({ rain: value, temperature: value, wind: value });
  return {
    actionLineage: {
      actionSha256: "1".repeat(64),
      attemptSha256: "2".repeat(64),
      predecessorActionSha256: null,
      releaseManifestSha256: "3".repeat(64),
      sourceRevision: "4".repeat(40),
    },
    actionProjectionSha256: "5".repeat(64),
    actionState: "release_pending",
    contractVersion: "forecast-adjustment-scorecard/v2",
    families: legacy.families,
    generatedAt: "2099-10-01T00:00:00.000Z",
    history: [{
      actionLineageSha256: "6".repeat(64),
      actionProjectionSha256: "5".repeat(64),
      actionState: "release_pending",
      attemptSha256: "2".repeat(64),
      occurredAt: "2099-10-01T00:00:00.000Z",
      policyDecision: "qualified",
      releaseManifestSha256: "3".repeat(64),
    }],
    identities: {
      active: identities("7".repeat(64)),
      prior: identities(null),
      raw: identities("8".repeat(64)),
      shadow: identities("9".repeat(64)),
    },
    inputs: {
      ...legacy.inputs,
      frontierSha256: "a".repeat(64),
      inputManifestSha256: "b".repeat(64),
      reportSha256: "c".repeat(64),
    },
    job: {
      attemptState: "completed",
      backlogState: "clear",
      dueState: "not_due",
      operatorState: "enabled",
      successState: "succeeded",
    },
    policyDecision: "qualified",
    progress: {
      confirmationCompletedEpochs: 4,
      confirmationRequiredEpochs: 4,
      rainCaptureExpiresAt: "2099-10-07T00:00:00.000Z",
      rollbackCompletedEpochs: 0,
      rollbackRequiredEpochs: 2,
    },
    siteKey: "ballydidean",
    validThrough: "2099-10-08T00:00:00.000Z",
    warnings: {
      capacity: ["next_capture_reserved"],
      capture: [],
      fallback: [],
      gauge: [],
      source: [],
    },
  };
}
const dailyPrecipitation = {
  accumulationMm: 2.54,
  source: {
    sourceId: "71",
    stationSlug: "tempest-64255",
  },
  validThrough: "2026-08-22T04:59:00.000Z",
};
const propertySensorLayout = [
  {
    displayName: "Orchard soil",
    icon: "temperature",
    latitude: 47.9505,
    longitude: -122.4281,
    sensorKey: "soil-1",
    updatedAt: "2026-08-22T04:59:00.000Z",
  },
  {
    displayName: "Barn temperature",
    icon: "temperature",
    latitude: 47.9505,
    longitude: -122.4281,
    sensorKey: "temperature-2",
    updatedAt: "2026-08-22T04:59:00.000Z",
  },
];
const trends = [];

// build matching month samples across three calendar years
for (const [yearIndex, year] of [2019, 2025, 2026].entries()) {
  // retain one representative daily sample per month
  for (let month = 0; month < 12; month += 1) {
    trends.push({
      metrics: {
        apparentTemperatureC: 8 + month + yearIndex * 0.4,
        precipitationMm: month % 3 === 0 ? 0.2 : 0,
        pressureHpa: 1012 + month * 0.3,
        relativeHumidityPercent: 82 - month,
        temperatureC: 9 + Math.sin((month / 12) * Math.PI * 2) * 7 + yearIndex,
        temperatureMaximumC: 14 + Math.sin((month / 12) * Math.PI * 2) * 7 + yearIndex,
        temperatureMinimumC: 5 + Math.sin((month / 12) * Math.PI * 2) * 7 + yearIndex,
        windDirectionDegrees: (month * 30 + yearIndex * 15) % 360,
        windGustMps: 5 + month * 0.25 + yearIndex * 0.2,
        windSpeedMps: 3 + month * 0.2,
      },
      validAt: new Date(Date.UTC(year, month, 15, 8)).toISOString(),
    });
  }
}
const tides = [
  {
    eventType: "low",
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
    validAt: "2026-08-22T00:30:00.000Z",
    waterLevelM: 0.5,
  },
  {
    eventType: null,
    kind: "observation",
    source: {
      attribution: {
        label: "NOAA Tides & Currents",
        url: "https://tidesandcurrents.noaa.gov/",
      },
      providerKey: "noaa-co-ops",
      stationName: "Port Townsend Tide Gauge",
      stationSlug: "port-townsend-tide-gauge",
    },
    validAt: "2026-08-22T04:48:00.000Z",
    waterLevelM: 2.4,
  },
  {
    eventType: null,
    kind: "observation",
    source: {
      attribution: {
        label: "NOAA Tides & Currents",
        url: "https://tidesandcurrents.noaa.gov/",
      },
      providerKey: "noaa-co-ops",
      stationName: "Port Townsend Tide Gauge",
      stationSlug: "port-townsend-tide-gauge",
    },
    validAt: "2026-08-22T04:54:00.000Z",
    waterLevelM: 2.5,
  },
  {
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
    validAt: "2026-08-22T06:30:00.000Z",
    waterLevelM: 3.2,
  },
  {
    eventType: "low",
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
    validAt: "2026-08-22T12:00:00.000Z",
    waterLevelM: 0.4,
  },
];

// create one public weather record
function makeRecord(id, validAt, temperatureC) {
  return {
    freshness: {
      ageSeconds: id === "101" ? 600 : 87_000,
      label: id === "101" ? "Model value is current" : "Model value may be stale",
      status: id === "101" ? "fresh" : "stale",
    },
    id,
    metadata: {
      device: { model: "virtual-grid", serial: null, vendor: "Open-Meteo" },
      provider: {
        dataset: "best_match",
        elevationM: 17,
        gridCell: null,
        propertySensors: null,
      },
      quality: null,
      upstream: {
        model: "best_match",
        timezone: "America/Los_Angeles",
      },
    },
    metrics: {
      apparentTemperatureC: temperatureC - 0.7,
      blackGlobeTemperatureC: null,
      cloudCoverPercent: 42,
      pm25MicrogramsPerCubicMeter: null,
      precipitationMm: 0.2,
      precipitationRateMmPerHour: null,
      pressureHpa: 1014.2,
      relativeHumidityPercent: 78,
      soilElectricalConductivityMicrosiemensPerCm: null,
      soilMoisturePercent: null,
      solarRadiationWm2: null,
      temperatureC,
      uvIndex: null,
      windDirectionDegrees: 225,
      windGustMps: 7.2,
      windSpeedMps: 4.1,
      wetBulbGlobeTemperatureC: null,
    },
    productRunAt: null,
    provenance: {
      attribution: {
        label: "Weather data by Open-Meteo",
        url: "https://open-meteo.com/",
      },
      label: "model-derived current conditions",
      providerKey: "open-meteo",
      sourceId: "10",
      sourceKey: "open-meteo-current-v1",
      sourceKind: "model_current",
      stationSlug: "open-meteo-virtual",
    },
    receivedAt: validAt,
    revisionCount: 0,
    validAt,
  };
}

// create one normalized forecast record
function makeForecastRecord(index) {
  const validAt = new Date(Date.UTC(2026, 7, 21, 7 + index)).toISOString();
  const hour = index % 24;
  const value = makeRecord(
    String(200 + index),
    validAt,
    15 + Math.sin((hour / 24) * Math.PI * 2) * 5,
  );
  return {
    ...value,
    metadata: {
      ...value.metadata,
      provider: { ...value.metadata.provider, dataset: "forecast" },
    },
    metrics: {
      ...value.metrics,
      pm25MicrogramsPerCubicMeter: 5 + hour % 6,
      precipitationRateMmPerHour: hour === 5 ? 2.54 : 0,
      pressureHpa: 1_014 + Math.sin((hour / 24) * Math.PI * 2) * 3,
      relativeHumidityPercent: 62 + Math.cos((hour / 24) * Math.PI * 2) * 12,
      uvIndex: Math.max(0, 8 - Math.abs(12 - hour)),
      windGustMps: 6 + Math.sin((hour / 24) * Math.PI * 2),
      windSpeedMps: 3 + Math.sin((hour / 24) * Math.PI * 2) * 0.5,
    },
    productRunAt: "2026-08-22T05:00:00.000Z",
    provenance: {
      ...value.provenance,
      label: "hourly forecast",
      sourceId: "12",
      sourceKey: "open-meteo-forecast-v1",
      sourceKind: "forecast",
    },
  };
}

// resolve one exact lead band
function adjustmentLeadBand(targetLeadHours) {
  const minimum = Math.floor((targetLeadHours - 1) / 24) * 24 + 1;
  return `${String(minimum).padStart(3, "0")}-${String(minimum + 23).padStart(3, "0")}`;
}

// create one exact active decision
function activeForecastAdjustment(record, targetLeadHours) {
  return {
    adjustedMetrics: {
      temperatureC: record.metrics.temperatureC + 2,
      windSpeedMps: record.metrics.windSpeedMps + 0.4,
    },
    algorithmContractVersion: "robust-hierarchical-median/v1",
    appliedMetrics: ["temperatureC", "windSpeedMps"],
    candidateArtifactSha256: adjustmentHashes.candidate,
    contractVersion: "forecast-adjustment-decision/v1",
    evaluationReportSha256: adjustmentHashes.report,
    leadBand: adjustmentLeadBand(targetLeadHours),
    qualificationReceiptSha256: adjustmentHashes.receipt,
    rawForecastProvenance: {
      adapterVersion: "open-meteo-forecast-daily/v4",
      cohort: "legacy_v4_retrieval_snapshot",
      contractEpoch:
        "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83",
      dataset: record.metadata.provider.dataset,
      referenceAt: new Date(
        Date.parse(record.validAt) - targetLeadHours * 3_600_000,
      ).toISOString(),
      referenceKind: "retrieval_snapshot",
      sourceConfigFingerprint: adjustmentHashes.source,
      sourceKey: record.provenance.sourceKey,
      targetLeadHours,
      upstreamModel: record.metadata.upstream.model,
      validAt: record.validAt,
    },
    reasonCode: null,
    state: "active",
  };
}

// create one wind-only canary decision
function windCanaryForecastAdjustment(record, targetLeadHours) {
  const qualified = activeForecastAdjustment(record, targetLeadHours);
  return {
    activationKind: "wind_transfer_canary",
    adjustedMetrics: {
      windGustMps: record.metrics.windGustMps + 0.8,
      windSpeedMps: record.metrics.windSpeedMps + 0.4,
    },
    algorithmContractVersion: qualified.algorithmContractVersion,
    appliedMetrics: ["windGustMps", "windSpeedMps"],
    authorizationSha256: adjustmentHashes.receipt,
    candidateArtifactSha256: qualified.candidateArtifactSha256,
    contractVersion: qualified.contractVersion,
    leadBand: qualified.leadBand,
    rawForecastProvenance: qualified.rawForecastProvenance,
    reasonCode: null,
    state: "active",
    transferReportSha256: adjustmentHashes.report,
  };
}

// create one active causal rain decision
function activeRainAdjustment(record) {
  const validAtMs = Date.parse(record.validAt);
  const runInitializedAt = new Date(validAtMs - 12 * 3_600_000).toISOString();
  const firstReceivedAt = new Date(validAtMs - 11 * 3_600_000).toISOString();
  const decisionAt = new Date(validAtMs - 4 * 3_600_000).toISOString();
  return {
    bundleSha256: "4".repeat(64),
    contractVersion: "forecast-rain-adjustment-decision/v1",
    correctedPrecipitationMm: record.metrics.precipitationMm,
    rawBestMatchPrecipitationMm: record.metrics.precipitationMm,
    reasonCode: null,
    sourceForecast: {
      decisionAt,
      firstReceivedAt,
      modelLeadHours: 12,
      providerKey: "open-meteo",
      rawPrecipitationMm: record.metrics.precipitationMm,
      runInitializedAt,
      upstreamModel: "ecmwf_ifs",
      validAt: record.validAt,
    },
    state: "active",
  };
}

// create one active rain runtime
function activeRainRuntime() {
  return {
    activeBundle: "4".repeat(64),
    loadedAt: "2026-08-22T03:00:00.000Z",
    reasonCode: null,
    source: {
      decisionAt: "2026-08-22T02:00:00.000Z",
      firstReceivedAt: "2026-08-22T00:05:00.000Z",
      hourCount: 23,
      runInitializedAt: "2026-08-21T18:00:00.000Z",
    },
    state: "active",
  };
}

// create one exact raw decision
function rawForecastAdjustment(state, reasonCode) {
  return {
    adjustedMetrics: {},
    appliedMetrics: [],
    contractVersion: "forecast-adjustment-decision/v1",
    reasonCode,
    state,
  };
}

// create one bounded runtime summary
function forecastAdjustmentRuntime(mode) {
  // expose one synthetic verified bundle
  if (mode === "active" || mode === "canary") {
    return {
      activationMode: mode === "canary" ? "wind_canary" : "qualified",
      activeBundle: adjustmentHashes.bundle,
      authorizationSha256: mode === "canary" ? adjustmentHashes.receipt : null,
      candidateArtifactSha256: adjustmentHashes.candidate,
      enabledMetrics: mode === "canary"
        ? ["windGustMps", "windSpeedMps"]
        : ["temperatureC", "windSpeedMps"],
      evaluationReportSha256: mode === "canary" ? null : adjustmentHashes.report,
      expiresAt: mode === "canary" ? "2026-09-10T05:00:00.000Z" : null,
      loadedAt: "2026-08-22T05:00:00.000Z",
      qualificationReceiptSha256: mode === "canary" ? null : adjustmentHashes.receipt,
      reasonCode: null,
      state: "active",
      transferReportSha256: mode === "canary" ? adjustmentHashes.report : null,
    };
  }

  return {
    activationMode: null,
    activeBundle: null,
    authorizationSha256: null,
    candidateArtifactSha256: null,
    enabledMetrics: [],
    evaluationReportSha256: null,
    expiresAt: null,
    loadedAt: "2026-08-22T05:00:00.000Z",
    qualificationReceiptSha256: null,
    reasonCode: mode === "fault" ? "bundle_invalid" : "registry_inactive",
    state: "disabled",
    transferReportSha256: null,
  };
}

// create one complete adjusted forecast response
function forecastResponse(
  mode,
  settings = { version: 1, temperature: true, wind: true, rain: true },
  records = forecast,
) {
  return {
    adjustmentSettings: settings,
    adjustmentRuntime: forecastAdjustmentRuntime(mode),
    data: records.map(
      // attach one exact per-row decision
      (record, index) => ({
        ...record,
        adjustment: mode === "active" || mode === "canary"
          ? index < 168
            ? mode === "canary"
              ? windCanaryForecastAdjustment(record, index + 1)
              : activeForecastAdjustment(record, index + 1)
            : rawForecastAdjustment("not_applicable", "unsupported_lead")
          : rawForecastAdjustment(
              "disabled",
              mode === "fault" ? "bundle_invalid" : "registry_inactive",
            ),
      }),
    ),
    site,
  };
}

// locate an optional host Chromium binary
function findChromium() {
  const explicit = process.env.WEATHER_CHROMIUM_EXECUTABLE;

  // prefer an explicit test runner binary
  if (explicit !== undefined && existsSync(explicit)) {
    return explicit;
  }

  const cacheRoot = join(process.env.HOME ?? "", ".cache/ms-playwright");
  const candidates = existsSync(cacheRoot)
    ? readdirSync(cacheRoot)
      .filter((name) => name.startsWith("chromium-"))
      .sort()
      .reverse()
    : [];

  // select the newest cached browser
  for (const candidate of candidates) {
    const path = join(cacheRoot, candidate, "chrome-linux64/chrome");

    // require an executable artifact
    if (existsSync(path)) {
      return path;
    }
  }

  return undefined;
}

// launch the available real browser
async function launchBrowser() {
  const executablePath = findChromium();
  return await chromium.launch({
    ...(executablePath === undefined ? {} : { executablePath }),
    headless: true,
  });
}

const MAP_TILE_PATTERN = /^https:\/\/(?:tile\.openstreetmap\.org|basemap\.nationalmap\.gov)\//u;

// create one browser page with deterministic map tiles
async function createFixturePage(browser, options) {
  const page = await browser.newPage(options);
  // replace external tiles with deterministic images
  await page.route(MAP_TILE_PATTERN, async (route) => {
    await route.fulfill({
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256"><rect width="256" height="256" fill="#d9d8d1"/></svg>',
      contentType: "image/svg+xml",
      status: 200,
    });
  });
  return page;
}

// measure rendered alert colors and their WCAG contrast
async function captureAlertContrast(page, label) {
  return await page.locator(".local-alert").filter({ hasText: label }).evaluate(
    // calculate contrast from final browser colors
    (alert) => {
      // parse one computed rgb color
      const channels = (color) => (color.match(/[\d.]+/gu) ?? [])
        .slice(0, 3)
        .map(Number);
      // linearize one srgb channel
      const linear = (channel) => {
        const normalized = channel / 255;
        return normalized <= 0.04045
          ? normalized / 12.92
          : ((normalized + 0.055) / 1.055) ** 2.4;
      };
      // calculate one relative luminance
      const luminance = (color) => {
        const [red = 0, green = 0, blue = 0] = channels(color).map(linear);
        return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
      };
      // compare two opaque rendered colors
      const contrast = (first, second) => {
        const lighter = Math.max(luminance(first), luminance(second));
        const darker = Math.min(luminance(first), luminance(second));
        return (lighter + 0.05) / (darker + 0.05);
      };
      const heading = alert.querySelector("strong");
      const detail = alert.querySelector("span");

      // require the complete visible alert
      if (!(heading instanceof HTMLElement) || !(detail instanceof HTMLElement)) {
        throw new Error("alert content is incomplete");
      }

      const backgroundColor = getComputedStyle(alert).backgroundColor;
      const detailColor = getComputedStyle(detail).color;
      const headingColor = getComputedStyle(heading).color;
      return {
        backgroundColor,
        detailColor,
        detailContrast: contrast(detailColor, backgroundColor),
        headingColor,
        headingContrast: contrast(headingColor, backgroundColor),
      };
    },
  );
}

// capture rendered adjustment-family icon colors and classes
async function captureFamilyIcons(page, view) {
  const selectors = view === "home"
    ? {
      rain: "[data-condition='rain'] .condition-label > .material-symbols-rounded",
      temperature: "[data-condition='temperature'] .condition-label > .material-symbols-rounded",
      wind: "[data-condition='wind'] .condition-label > .material-symbols-rounded",
    }
    : {
      rain: "[data-forecast-chart='rain-rate'] .forecast-chart-heading .material-symbols-rounded",
      temperature: "[data-forecast-chart='temperature'] .forecast-chart-heading .material-symbols-rounded",
      wind: "[data-forecast-chart='wind'] .forecast-chart-heading .material-symbols-rounded",
    };
  const icons = {};

  // inspect each adjusted family independently
  for (const [family, selector] of Object.entries(selectors)) {
    const icon = page.locator(selector);
    icons[family] = {
      className: await icon.getAttribute("class"),
      color: await icon.evaluate(
        // read the final inherited icon color
        (element) => getComputedStyle(element).color,
      ),
    };
  }

  return icons;
}

// capture icons governed only by the global adjustment preference
async function captureGlobalModeIcons(page, view) {
  const selectors = view === "home"
    ? {
      "air-quality": "[data-condition='air-quality'] .condition-label > .material-symbols-rounded",
      humidity: "[data-condition='humidity'] .condition-label > .material-symbols-rounded",
      pressure: "[data-condition='pressure'] .condition-label > .material-symbols-rounded",
      "uv-index": "[data-condition='uv-index'] .condition-label > .material-symbols-rounded",
    }
    : {
      "air-quality": "[data-forecast-chart='air-quality'] .forecast-chart-heading .material-symbols-rounded",
      humidity: "[data-forecast-chart='humidity'] .forecast-chart-heading .material-symbols-rounded",
      pressure: "[data-forecast-chart='pressure'] .forecast-chart-heading .material-symbols-rounded",
      "uv-index": "[data-forecast-chart='uv-index'] .forecast-chart-heading .material-symbols-rounded",
    };
  const icons = {};

  // inspect every global-mode icon independently
  for (const [family, selector] of Object.entries(selectors)) {
    const icon = page.locator(selector);
    icons[family] = {
      className: await icon.getAttribute("class"),
      color: await icon.evaluate(
        // read the final inherited icon color
        (element) => getComputedStyle(element).color,
      ),
    };
  }

  return icons;
}

// capture four icon-only families without their decorative headings
async function captureGlobalModeData(page, view) {
  const families = ["humidity", "air-quality", "pressure", "uv-index"];
  return Object.fromEntries(await Promise.all(families.map(
    // retain rendered readings or serialized chart series per family
    async (family) => {
      const root = page.locator(view === "home"
        ? `[data-condition='${family}']`
        : `[data-forecast-chart='${family}']`);
      return [family, view === "home"
        ? {
          primary: await root.locator(".condition-primary").textContent(),
          forecast: await root.locator(".condition-forecast-readings").textContent(),
        }
        : {
          series: await root.getAttribute("data-forecast-series"),
          values: await root.locator("[data-forecast-value]").allTextContents(),
        }];
    },
  )));
}

// capture stable document-space section geometry
async function captureSectionGeometry(page, selectors) {
  const geometry = {};

  // capture each requested section once
  for (const selector of selectors) {
    geometry[selector] = await page.locator(selector).evaluate(
      // preserve document coordinates across viewport movement
      (element) => {
        const bounds = element.getBoundingClientRect();
        return {
          height: bounds.height,
          left: bounds.left + window.scrollX,
          top: bounds.top + window.scrollY,
          width: bounds.width,
        };
      },
    );
  }

  return geometry;
}

// require one in-place skeleton without restoring the retired masthead dot
async function assertSkeletonLoadingState(page, selector) {
  await page.locator(selector).first().waitFor({ state: "attached" });
  assert.equal(await page.locator(".refresh-indicator").count(), 0);
  assert.equal(await page.locator(".masthead [role='status']").count(), 0);
  assert.equal(await page.locator(".weather-content").getAttribute("aria-busy"), "true");
  const status = page.locator("main.shell > p.sr-only[role='status']");
  assert.equal(await status.count(), 1);
  assert.match(await status.textContent() ?? "", /Refreshing weather data/u);
}

// require a settled route with no permanent loading placeholder
async function assertSkeletonLoadingSettled(page, expectedStatus = /Weather data is up to date/u) {
  await page.locator(".weather-content[aria-busy='false']").waitFor();
  assert.equal(await page.locator(".refresh-indicator").count(), 0);
  assert.equal(await page.locator(".masthead [role='status']").count(), 0);
  assert.equal(await page.locator(".skeleton-region, .skeleton-history-row, .skeleton-history-card").count(), 0);
  const status = page.locator("main.shell > p.sr-only[role='status']");
  assert.equal(await status.count(), 1);
  assert.match(await status.textContent() ?? "", expectedStatus);
}

// require rendered lines to clear every title footprint
async function assertForecastTitleClearance(page) {
  assert.equal(
    await page.locator(".forecast-chart").evaluateAll(
      // compare actual plot and title geometry
      (charts) => charts.every((chart) => {
        const heading = chart.querySelector(".forecast-chart-heading");
        const svg = chart.querySelector("svg");
        const headingBounds = heading?.getBoundingClientRect();
        const svgBounds = svg?.getBoundingClientRect();

        // require complete rendered geometry
        if (headingBounds === undefined || svgBounds === undefined) {
          return false;
        }

        const top = heading?.classList.contains("forecast-chart-heading-top") === true;
        const ordinates = [...chart.querySelectorAll("polyline")].flatMap(
          // read every rendered SVG ordinate
          (line) => (line.getAttribute("points") ?? "").split(" ").flatMap((point) => {
            const ordinate = Number(point.split(",")[1]);
            return Number.isFinite(ordinate) ? [ordinate] : [];
          }),
        );
        const rendered = ordinates.map(
          // project one SVG point into viewport pixels
          (ordinate) => svgBounds.top + (ordinate / 150) * svgBounds.height,
        );
        return top && rendered.every((ordinate) => ordinate >= headingBounds.bottom + 5);
      }),
    ),
    true,
  );
}

// start a bounded static and fake API server
async function startFixtureServer() {
  const state = {
    adjustmentMode: "inactive",
    adjustmentScorecard: adjustmentScorecardFixture(),
    adjustmentScorecardPublicationState: "legacy_display",
    adjustmentScorecardStatus: 200,
    adjustmentSettings: { version: 1, temperature: true, wind: true, rain: true },
    adjustmentSettingsUpdates: 0,
    adminUpdates: 0,
    currentRecords: [current, physicalCurrent],
    dailyPrecipitation,
    failReads: false,
    forecastObservations: [],
    forecastObservationsGeneratedAt: "2026-08-22T05:00:00.000Z",
    forecastPayload: null,
    forecastRecords: forecast,
    mutations: 0,
    requests: [],
    propertySensorLayout: propertySensorLayout.map(
      // isolate each fixture's mutable editor state
      (entry) => ({ ...entry }),
    ),
    tileDelayMs: 0,
    viewerContext: { data: { homeNetwork: false } },
    viewerContextStatus: 200,
  };
  const server = createServer(async (request, response) => {
    const url = new URL(request.url ?? "/", "http://weather.test");
    state.requests.push(`${request.method ?? "GET"} ${url.pathname}${url.search}`);
    const isAdmin = request.headers.cookie
      ?.split(";")
      .some(
        // match only the fixture administrator cookie
        (cookie) => cookie.trim() === `weather_admin_session=${fixtureAdminSession}`,
      ) ?? false;

    // exchange the fixture login form for a browser cookie
    if (request.method === "POST" && url.pathname === "/admin/login") {
      const chunks = [];

      // collect the native URL-encoded form
      for await (const chunk of request) {
        chunks.push(Buffer.from(chunk));
      }

      const form = new URLSearchParams(Buffer.concat(chunks).toString("utf8"));
      // require one password-only fixture form
      const authenticated = form.size === 1 &&
        form.getAll("password").length === 1 &&
        form.get("password") === "test-admin-password";
      response.statusCode = 303;
      response.setHeader("location", authenticated ? "/admin" : "/admin?error=invalid");

      // issue a cookie only after valid credentials
      if (authenticated) {
        // match the browser's embedded cookie context
        const cookieContext = request.headers.host?.startsWith("localhost:") === true
          ? "SameSite=None; Secure; Partitioned"
          : "SameSite=Lax";
        response.setHeader(
          "set-cookie",
          `weather_admin_session=${fixtureAdminSession}; Path=/; HttpOnly; Max-Age=43200; ${cookieContext}`,
        );
      }

      response.end();
      return;
    }

    // clear the fixture browser session
    if (request.method === "POST" && url.pathname === "/admin/logout") {
      // expire the cookie in its original context
      const cookieContext = request.headers.host?.startsWith("localhost:") === true
        ? "SameSite=None; Secure; Partitioned"
        : "SameSite=Lax";
      response.statusCode = 303;
      response.setHeader("location", "/admin");
      response.setHeader(
        "set-cookie",
        `weather_admin_session=; Path=/; HttpOnly; Max-Age=0; ${cookieContext}`,
      );
      response.end();
      return;
    }

    // serve one authenticated review-only scorecard
    if (url.pathname === "/api/v1/admin/sites/ballydidean/forecast-adjustment-scorecard") {
      // require an authenticated browser session
      if (!isAdmin) {
        sendJson(response, { error: { code: "unauthorized" } }, 401);
        return;
      }

      // retain a queryless read-only resource
      if ((request.method !== "GET" && request.method !== "HEAD") || url.search !== "") {
        sendJson(response, { error: { code: "method_not_allowed" } }, 405);
        return;
      }

      response.setHeader("cache-control", "private, no-store");
      sendJson(
        response,
        state.adjustmentScorecardStatus === 200
          ? {
            data: state.adjustmentScorecard,
            publicationState: state.adjustmentScorecardPublicationState,
          }
          : { error: { code: "unavailable" } },
        state.adjustmentScorecardStatus,
      );
      return;
    }

    // persist one authenticated-editor fixture update
    if (url.pathname === "/api/v1/admin/sites/ballydidean/forecast-adjustment-settings") {
      // require an authenticated browser session
      if (!isAdmin) {
        sendJson(response, { error: { code: "unauthorized" } }, 401);
        return;
      }

      // keep the fixture resource read/write only
      if (request.method !== "GET" && request.method !== "PUT") {
        sendJson(response, { error: { code: "method_not_allowed" } }, 405);
        return;
      }

      // save all three independent controls
      if (request.method === "PUT") {
        const chunks = [];

        // collect one bounded JSON update
        for await (const chunk of request) {
          chunks.push(Buffer.from(chunk));
        }

        state.adjustmentSettings = JSON.parse(Buffer.concat(chunks).toString("utf8"));
        state.adjustmentSettingsUpdates += 1;
      }

      sendJson(response, { data: state.adjustmentSettings });
      return;
    }

    // persist one authenticated-editor fixture update
    if (
      request.method === "PUT" &&
      url.pathname === "/api/v1/admin/sites/ballydidean/property-sensor-layout/soil-1"
    ) {
      // enforce the browser session boundary
      if (!isAdmin) {
        sendJson(response, { error: { code: "unauthorized" } }, 401);
        return;
      }

      const chunks = [];

      // collect the bounded browser fixture body
      for await (const chunk of request) {
        chunks.push(Buffer.from(chunk));
      }

      const body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
      state.propertySensorLayout[0] = {
        ...body,
        sensorKey: "soil-1",
        updatedAt: "2026-08-22T05:01:00.000Z",
      };
      state.adminUpdates += 1;
      sendJson(response, { data: state.propertySensorLayout[0] });
      return;
    }

    // reject every mutation
    if (!["GET", "HEAD"].includes(request.method ?? "GET")) {
      state.mutations += 1;
      sendJson(response, { error: { code: "method_not_allowed" } }, 405);
      return;
    }

    // serve same-origin weather-map fixture tiles
    if (/^\/maps\/xweather\/(?:history|forecast)\/(?:radar|clouds|precipitation|wind)\/\d{14}\/\d+\/\d+x\d+\/-?\d+(?:\.\d+)?,-?\d+(?:\.\d+)?\.png$/u.test(url.pathname)) {
      response.statusCode = 200;
      response.setHeader(
        "cache-control",
        url.pathname.includes("/maps/xweather/history/")
          ? "public, max-age=31536000, immutable"
          : "no-store",
      );
      response.setHeader("content-type", "image/png");
      response.setHeader("content-length", String(transparentPng.byteLength));

      // keep cache progress observable in focused browser coverage
      if (state.tileDelayMs > 0) {
        setTimeout(() => {
          // complete one deliberately delayed tile
          response.end(request.method === "HEAD" ? undefined : transparentPng);
        }, state.tileDelayMs);
        return;
      }

      response.end(request.method === "HEAD" ? undefined : transparentPng);
      return;
    }

    // serve site metadata
    if (url.pathname === "/api/v1/sites") {
      sendJson(response, { data: [site] });
      return;
    }

    // serve one uncached display-only viewer context
    if (url.pathname === "/api/v1/viewer-context") {
      response.setHeader("cache-control", "private, no-store");
      sendJson(response, state.viewerContext, state.viewerContextStatus);
      return;
    }

    // simulate a recoverable API outage
    if (state.failReads && url.pathname.startsWith("/api/v1/sites/")) {
      sendJson(response, { error: { code: "unavailable" } }, 503);
      return;
    }

    // serve filtered current data
    if (url.pathname === "/api/v1/sites/ballydidean/current") {
      sendJson(response, { data: state.currentRecords, site });
      return;
    }

    // serve the shared property sensor layout
    if (url.pathname === "/api/v1/sites/ballydidean/property-sensor-layout") {
      sendJson(response, { data: state.propertySensorLayout });
      return;
    }

    // serve today's nearest-gauge rain accumulation
    if (url.pathname === "/api/v1/sites/ballydidean/daily-precipitation") {
      sendJson(response, {
        data: state.dailyPrecipitation,
        generatedAt: "2026-08-22T05:00:00.000Z",
        site,
      });
      return;
    }

    // serve normalized forecast hours
    if (url.pathname === "/api/v1/sites/ballydidean/forecast") {
      sendJson(
        response,
        state.forecastPayload ?? forecastResponse(
          state.adjustmentMode,
          state.adjustmentSettings,
          state.forecastRecords,
        ),
      );
      return;
    }

    // serve projected hourly recordings for past forecast points
    if (url.pathname === "/api/v1/sites/ballydidean/forecast-observations") {
      sendJson(response, {
        data: state.forecastObservations,
        generatedAt: state.forecastObservationsGeneratedAt,
      });
      return;
    }

    // serve daily calendar-year trend buckets
    if (url.pathname === "/api/v1/sites/ballydidean/trends") {
      sendJson(response, {
        data: trends,
        generatedAt: "2026-08-22T05:00:00.000Z",
        site,
      });
      return;
    }

    // serve observed and predicted tide levels
    if (url.pathname === "/api/v1/sites/ballydidean/tides") {
      sendJson(response, {
        data: tides,
        generatedAt: "2026-08-22T05:00:00.000Z",
        site,
      });
      return;
    }

    // serve stable cursor pages
    if (url.pathname === "/api/v1/sites/ballydidean/history") {
      const secondPage = url.searchParams.has("cursor");
      sendJson(response, {
        data: [secondPage ? older : current],
        page: { limit: 100, nextCursor: secondPage ? null : "older-page" },
        site,
      });
      return;
    }

    // serve one same-origin embedding host
    if (url.pathname === "/embed") {
      response.statusCode = 200;
      response.setHeader("content-type", "text/html; charset=utf-8");
      response.end('<!doctype html><html><body><iframe title="Embedded weather" src="/" style="width: 1000px; height: 700px"></iframe></body></html>');
      return;
    }

    // serve one cross-site administrator host
    if (url.pathname === "/admin-embed") {
      const embeddedOrigin = `http://localhost:${String(request.socket.localPort)}`;
      response.statusCode = 200;
      response.setHeader("content-type", "text/html; charset=utf-8");
      response.end(`<!doctype html><html><body><iframe title="Embedded admin" src="${embeddedOrigin}/admin" style="width: 900px; height: 800px"></iframe></body></html>`);
      return;
    }

    const assets = new Map([
      ["/", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/logs", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/logs/", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/map", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/map/", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/admin", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/admin/", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/forecast", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/forecast/", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/trends", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/trends/", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/settings", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/settings/", [join(publicRoot, "index.html"), "text/html; charset=utf-8"]],
      ["/privacy", [join(publicRoot, "privacy.html"), "text/html; charset=utf-8"]],
      ["/privacy/", [join(publicRoot, "privacy.html"), "text/html; charset=utf-8"]],
      ["/manifest.webmanifest", [join(publicRoot, "manifest.webmanifest"), "application/manifest+json; charset=utf-8"]],
      ["/service-worker.js", [join(publicRoot, "service-worker.js"), "text/javascript; charset=utf-8"]],
      [`/assets/${fixtureAssetVersion}/styles.css`, [join(publicRoot, "styles.css"), "text/css; charset=utf-8"]],
      ["/brand/ballydidean-wide.svg", [join(publicRoot, "brand/ballydidean-wide.svg"), "image/svg+xml"]],
      ["/brand/ballydidean-weather-icon-32.png", [join(publicRoot, "brand/ballydidean-weather-icon-32.png"), "image/png"]],
      ["/brand/ballydidean-weather-icon-180.png", [join(publicRoot, "brand/ballydidean-weather-icon-180.png"), "image/png"]],
      ["/brand/ballydidean-weather-icon-192.png", [join(publicRoot, "brand/ballydidean-weather-icon-192.png"), "image/png"]],
      ["/brand/ballydidean-weather-icon-512.png", [join(publicRoot, "brand/ballydidean-weather-icon-512.png"), "image/png"]],
      ["/brand/ballydidean-weather-icon-maskable-512.png", [join(publicRoot, "brand/ballydidean-weather-icon-maskable-512.png"), "image/png"]],
      ["/weather-icons/01-sunny.svg", [join(publicRoot, "weather-icons/01-sunny.svg"), "image/svg+xml"]],
      ["/weather-icons/02-sunny-wind.svg", [join(publicRoot, "weather-icons/02-sunny-wind.svg"), "image/svg+xml"]],
      ["/weather-icons/03-partly-cloudy.svg", [join(publicRoot, "weather-icons/03-partly-cloudy.svg"), "image/svg+xml"]],
      ["/weather-icons/04-partly-cloudy-wind.svg", [join(publicRoot, "weather-icons/04-partly-cloudy-wind.svg"), "image/svg+xml"]],
      ["/weather-icons/05-cloudy.svg", [join(publicRoot, "weather-icons/05-cloudy.svg"), "image/svg+xml"]],
      ["/weather-icons/06-cloudy-wind.svg", [join(publicRoot, "weather-icons/06-cloudy-wind.svg"), "image/svg+xml"]],
      ["/weather-icons/07-light-rain.svg", [join(publicRoot, "weather-icons/07-light-rain.svg"), "image/svg+xml"]],
      ["/weather-icons/08-light-rain-wind.svg", [join(publicRoot, "weather-icons/08-light-rain-wind.svg"), "image/svg+xml"]],
      ["/weather-icons/09-heavy-rain.svg", [join(publicRoot, "weather-icons/09-heavy-rain.svg"), "image/svg+xml"]],
      ["/weather-icons/10-heavy-rain-wind.svg", [join(publicRoot, "weather-icons/10-heavy-rain-wind.svg"), "image/svg+xml"]],
      ["/weather-icons/12-unavailable.svg", [join(publicRoot, "weather-icons/12-unavailable.svg"), "image/svg+xml"]],
      ["/weather-icons/13-clear-night.svg", [join(publicRoot, "weather-icons/13-clear-night.svg"), "image/svg+xml"]],
      ["/weather-icons/14-clear-night-wind.svg", [join(publicRoot, "weather-icons/14-clear-night-wind.svg"), "image/svg+xml"]],
      ["/weather-icons/15-partly-cloudy-night.svg", [join(publicRoot, "weather-icons/15-partly-cloudy-night.svg"), "image/svg+xml"]],
      ["/weather-icons/16-partly-cloudy-night-wind.svg", [join(publicRoot, "weather-icons/16-partly-cloudy-night-wind.svg"), "image/svg+xml"]],
      ["/fonts/google-sans-flex-latin.woff2", [join(publicRoot, "fonts/google-sans-flex-latin.woff2"), "font/woff2"]],
      ["/fonts/material-symbols-rounded-v4.woff2", [join(publicRoot, "fonts/material-symbols-rounded-v4.woff2"), "font/woff2"]],
      [`/assets/${fixtureAssetVersion}/client.js`, [join(distRoot, "client.js"), "text/javascript; charset=utf-8"]],
      [`/assets/${fixtureAssetVersion}/index.js`, [join(distRoot, "index.js"), "text/javascript; charset=utf-8"]],
      [`/assets/${fixtureAssetVersion}/units.js`, [join(distRoot, "units.js"), "text/javascript; charset=utf-8"]],
      [`/assets/${fixtureAssetVersion}/solar-cloud.js`, [join(distRoot, "solar-cloud.js"), "text/javascript; charset=utf-8"]],
      [`/assets/${fixtureAssetVersion}/local-forecast-bias.js`, [join(distRoot, "local-forecast-bias.js"), "text/javascript; charset=utf-8"]],
    ]);
    const requestedAsset = assets.get(url.pathname);
    const adminRoute = url.pathname === "/admin" || url.pathname === "/admin/";
    const asset = adminRoute && !isAdmin
      ? [join(publicRoot, "admin-login.html"), "text/html; charset=utf-8"]
      : requestedAsset;

    // serve allowlisted browser assets
    if (asset !== undefined) {
      response.statusCode = 200;
      response.setHeader("content-type", asset[1]);

      // model the production administrator cache boundary
      if (isAdmin && asset[1] === "text/html; charset=utf-8") {
        response.setHeader("cache-control", "private, no-store");
        response.setHeader("vary", "Cookie");
      }

      const source = readFileSync(asset[0]);
      response.end(
        url.pathname === "/" ||
          url.pathname === "/logs" ||
          url.pathname === "/logs/" ||
          url.pathname === "/map" ||
          url.pathname === "/map/" ||
          url.pathname === "/admin" ||
          url.pathname === "/admin/" ||
          url.pathname === "/forecast" ||
          url.pathname === "/forecast/" ||
          url.pathname === "/trends" ||
          url.pathname === "/trends/" ||
          url.pathname === "/settings" ||
          url.pathname === "/settings/" ||
          url.pathname === "/privacy" ||
          url.pathname === "/privacy/" ||
          url.pathname === "/service-worker.js"
          ? source.toString("utf8").replaceAll("__WEATHER_ASSET_VERSION__", fixtureAssetVersion)
            .replaceAll(
              "__WEATHER_ADMIN__",
              String(isAdmin),
            )
            .replaceAll(
              "__WEATHER_ADMIN_LOGIN_ERROR__",
              url.searchParams.get("error") === "invalid"
                ? '<p class="admin-login-error" role="alert">The password is incorrect.</p>'
                : "",
            )
            .replaceAll("__WEATHER_ROUTE_PRELOAD__", "")
            .replaceAll("__WEATHER_ANALYTICS__", "false")
          : source,
      );
      return;
    }

    response.statusCode = 404;
    response.end("not found");
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();

  // require the expected TCP listener
  if (address === null || typeof address === "string") {
    throw new Error("fixture server did not bind a TCP port");
  }

  return {
    origin: `http://127.0.0.1:${String(address.port)}`,
    server,
    state,
  };
}

// verify daylight cloud correction and automatic fail-raw behavior in the rendered dashboard
test("WS90 sunlight clouds fade within 24 hours and retire on stale readings", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  const at = "2026-08-21T20:00:00.000Z";
  const regional = {
    ...current, id: "solar-regional", validAt: at, receivedAt: at,
    freshness: { ...current.freshness, status: "fresh", ageSeconds: 0 },
    metrics: { ...current.metrics, cloudCoverPercent: 90, precipitationRateMmPerHour: 0, windSpeedMps: 1 },
  };
  // keep the source identity distinct from the nearby physical fixture
  const ws90 = {
    ...physicalCurrent, id: "solar-ws90", validAt: at, receivedAt: at,
    freshness: { ...physicalCurrent.freshness, status: "fresh", ageSeconds: 0 },
    metrics: { ...physicalCurrent.metrics, cloudCoverPercent: null, solarRadiationWm2: 800, precipitationRateMmPerHour: 0, windSpeedMps: 1 },
    provenance: { ...physicalCurrent.provenance, providerKey: "ecowitt-local", stationSlug: "ballydidean-ecowitt", sourceId: "ws90" },
  };
  fixture.state.adjustmentSettings = { version: 1, temperature: false, wind: false, rain: false };
  fixture.state.forecastRecords = forecast.map(
    // make the regional cloud baseline constant so the fade is observable
    (record) => ({ ...record, metrics: { ...record.metrics, cloudCoverPercent: 90 } }),
  );

  try {
    browser = await launchBrowser();
    // check the same behavior at desktop and narrow mobile widths
    for (const viewport of [{ width: 1440, height: 1000 }, { width: 390, height: 844 }]) {
      fixture.state.currentRecords = [ws90, regional];
      fixture.state.failReads = false;
      const page = await createFixturePage(browser, { viewport, timezoneId: site.timezone });
      const errors = [];
      // collect browser startup and render failures
      page.on("pageerror", (error) => errors.push(error.message));
      await page.clock.install({ time: new Date(at) });
      await page.goto(fixture.origin, { waitUntil: "networkidle" });
      const cloudCard = page.locator("[data-condition='clouds']");
      assert.match(await cloudCard.locator(".condition-primary").textContent() ?? "", /10%/u);
      assert.match(await cloudCard.textContent() ?? "", /Sunlight estimate/u);
      assert.doesNotMatch(await cloudCard.textContent() ?? "", /experimental/iu);
      assert.equal(await cloudCard.locator(".condition-detail").isVisible(), true);
      assert.equal(await cloudCard.locator(".forecast-adjusted-icon").count(), 1);
      assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /01-sunny\.svg$/u);
      const toggle = page.getByRole("switch", { name: "Adjusted", exact: true });
      await toggle.click();
      assert.match(await cloudCard.locator(".condition-primary").textContent() ?? "", /90%/u);
      assert.equal(await cloudCard.locator(".forecast-adjusted-icon").count(), 0);
      assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /05-cloudy\.svg$/u);
      await toggle.click();
      await page.getByRole("link", { name: "Forecast", exact: true }).click();
      await page.locator("[data-forecast-charts]").waitFor();
      const grid = page.locator("[data-forecast-charts]");
      const chart = page.locator("[data-forecast-chart='clouds']");
      const series = JSON.parse(await chart.getAttribute("data-forecast-series"));
      assert.equal(series[0].values[12], 90);
      assert.equal(series[0].values[13], 90);
      assert.equal(Math.round(series[0].values[14]), 13);
      assert.equal(Math.round(series[0].values[19]), 30);
      assert.equal(await chart.locator(".forecast-chart-heading h3 > span:last-child").innerText(), "Clouds");
      assert.doesNotMatch(await page.locator("body").innerText(), /experimental/iu);
      await grid.focus();
      await page.keyboard.press("ArrowRight");
      const selected = await grid.getAttribute("data-forecast-selected-position");
      const initialForecastReads = fixture.state.requests.filter(
        // distinguish current-only polling from a forecast reload
        (request) => request.includes("/forecast?"),
      ).length;
      fixture.state.failReads = true;
      await page.clock.fastForward(360_100);
      await page.waitForFunction(
        // wait for the timer's timestamp guard to remove the solar marker despite request failures
        () => !document.querySelector("[data-forecast-chart='clouds'] .forecast-adjusted-icon"),
      );
      const staleSeries = JSON.parse(await chart.getAttribute("data-forecast-series"));
      assert.equal(staleSeries[0].values.every(
        // old observations cannot continue correcting future cloud hours
        (value) => value === 90,
      ), true);
      assert.equal(await grid.getAttribute("data-forecast-selected-position"), selected);
      assert.equal(await grid.evaluate(
        // retain focus so one keyboard step still advances exactly one forecast hour
        (element) => element === document.activeElement,
      ), true);
      await page.keyboard.press("ArrowRight");
      assert.equal(Number(await grid.getAttribute("data-forecast-selected-position")), Number(selected) + 1);
      assert.equal(fixture.state.requests.filter(
        // current polling must not request forecasts or restart the page loader
        (request) => request.includes("/forecast?"),
      ).length, initialForecastReads);
      assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /05-cloudy\.svg$/u);
      assert.deepEqual(errors, []);
      assert.equal(await page.evaluate(
        // prevent the cloud heading from widening a mobile viewport
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ), true);
      await page.close();
    }
  } finally {
    // release the browser and fixture even on a failed assertion
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// prove browser-visible local biases stay bounded and raw data remains intact
test("local observations bias four forecast families for only the next 24 hours", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  const now = new Date("2026-06-21T20:00:00.000Z");
  const publishedAt = "2026-06-21T19:50:00.000Z";
  // create one current record with an exact production source identity
  const currentRecord = (kind, id, validAt, metrics, overrides = {}) => {
    const base = makeRecord(id, validAt, 14);
    const physical = kind === "physical_sensor";
    return {
      ...base,
      freshness: { ageSeconds: 0, label: "Current", status: "fresh" },
      metadata: {
        ...base.metadata,
        provider: { ...base.metadata.provider, dataset: physical ? "get_livedata_info" : "best_match" },
      },
      metrics: {
        ...base.metrics,
        pm25MicrogramsPerCubicMeter: 20,
        pressureHpa: 1_000,
        relativeHumidityPercent: 50,
        uvIndex: 6,
        ...metrics,
      },
      pressureChange3hHpa: physical ? null : -0.5,
      provenance: {
        ...base.provenance,
        label: physical ? "local physical station" : "model-derived current conditions",
        providerKey: physical ? "ecowitt-local" : "open-meteo",
        sourceId: id,
        sourceKey: id,
        sourceKind: kind,
        stationSlug: physical ? "ballydidean-ecowitt" : "open-meteo-virtual",
      },
      receivedAt: validAt,
      ...overrides,
    };
  };
  const regional = currentRecord("model_current", "regional-current", "2026-06-21T19:45:00.000Z", {});
  const farm = currentRecord("physical_sensor", "farm-current", now.toISOString(), {
    pm25MicrogramsPerCubicMeter: null,
    pressureHpa: 1_025,
    relativeHumidityPercent: 70,
    uvIndex: 3,
  }, { pressureChange3hHpa: 1 });
  // create one independently weighted nearby particulate station
  const nearby = (id, value, stationSlug) => currentRecord(
    "physical_sensor",
    id,
    now.toISOString(),
    { pm25MicrogramsPerCubicMeter: value, uvIndex: null },
    {
      provenance: {
        ...farm.provenance,
        providerKey: "purpleair",
        sourceId: id,
        sourceKey: id,
        stationSlug,
      },
    },
  );
  const currentRecords = [farm, regional, nearby("nearby-a", 8, "station-a"), nearby("nearby-b", 12, "station-b")];
  const forecastRecords = Array.from({ length: 40 }, (_, index) => {
    const validAt = new Date(now.getTime() + (index - 3) * 3_600_000).toISOString();
    const base = makeRecord(`local-forecast-${String(index)}`, validAt, 14 + index / 10);
    return {
      ...base,
      freshness: { ageSeconds: 600, label: "Forecast is current", status: "fresh" },
      metadata: {
        ...base.metadata,
        provider: { ...base.metadata.provider, dataset: "forecast" },
        upstream: { model: "best_match", timezone: site.timezone },
      },
      metrics: {
        ...base.metrics,
        apparentTemperatureC: 12 + index / 10,
        pm25MicrogramsPerCubicMeter: 20,
        pressureHpa: 1_000 + index / 10,
        relativeHumidityPercent: 50,
        uvIndex: 6,
      },
      productRunAt: publishedAt,
      provenance: {
        ...base.provenance,
        label: "hourly forecast",
        providerKey: "open-meteo",
        sourceId: "regional-forecast",
        sourceKey: "regional-forecast",
        sourceKind: "forecast",
        stationSlug: "open-meteo-virtual",
      },
      receivedAt: publishedAt,
    };
  });
  const payload = forecastResponse(
    "active",
    { version: 1, temperature: true, wind: true, rain: true },
    forecastRecords,
  );
  const payloadSnapshot = JSON.parse(JSON.stringify(payload));
  const localFamilies = ["humidity", "air-quality", "uv-index", "pressure"];
  const collisionFamilies = [
    "temperature",
    "wind",
    "rain-rate",
    "clouds",
    ...localFamilies,
    "tide",
  ];
  // read the serialized values that drive every rendered chart line
  const chartSeries = async (page) => Object.fromEntries(await Promise.all(localFamilies.map(
    // preserve family order for direct raw comparisons
    async (family) => [
      family,
      JSON.parse(await page.locator(`[data-forecast-chart='${family}']`).getAttribute("data-forecast-series"))[0].values,
    ],
  )));
  // keep forecast headings clear of their selected-value bubbles
  const assertForecastLayout = async (page, width) => {
    const layout = await page.locator(collisionFamilies.map((family) => `[data-forecast-chart='${family}']`).join(", ")).evaluateAll(
      // compare each chart's actual browser geometry
      (charts) => charts.map((chart) => {
        const heading = chart.querySelector(".forecast-chart-heading h3");
        const value = chart.querySelector(".forecast-chart-value");
        const headingBounds = heading?.getBoundingClientRect();
        const valueBounds = value?.getBoundingClientRect();
        const chartBounds = chart.getBoundingClientRect();
        const children = value === null ? [] : [...value.querySelectorAll("small, strong")].map(
          // retain every visible label and value rectangle
          (child) => child.getBoundingClientRect(),
        );

        // report incomplete chart furniture directly
        if (headingBounds === undefined || valueBounds === undefined || children.length === 0) {
          return { family: chart.getAttribute("data-forecast-chart"), complete: false };
        }
        const overlaps = valueBounds.left < headingBounds.right + 2 && valueBounds.right > headingBounds.left - 2 &&
          valueBounds.top < headingBounds.bottom + 2 && valueBounds.bottom > headingBounds.top - 2;
        const within = (bounds) => bounds.left >= chartBounds.left - 1 && bounds.right <= chartBounds.right + 1 &&
          bounds.top >= chartBounds.top - 1 && bounds.bottom <= chartBounds.bottom + 1;
        return {
          childrenFit: children.every(
            // require both the label and numeric value to survive chart clipping
            (bounds) => within(bounds),
          ),
          clipped: !within(headingBounds) || !within(valueBounds),
          complete: true,
          family: chart.getAttribute("data-forecast-chart"),
          overlaps,
        };
      }),
    );
    assert.equal(
      layout.every(
        // require every recorded geometry guard
        (entry) => entry.complete && entry.childrenFit && !entry.overlaps && !entry.clipped,
      ),
      true,
      `forecast heading or value collision at ${String(width)}px: ${JSON.stringify(layout)}`,
    );
    assert.equal(
      await page.evaluate(
        // reject document-level responsive overflow
        () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
      ),
      true,
      `forecast overflow at ${String(width)}px`,
    );
  };
  // scrub across and away from the title zone without hiding either surface
  const assertCrosshairClear = async (page, width) => {
    const grid = page.locator("[data-forecast-charts]");
    // move a fixed number of hourly steps from one grid edge
    const move = async (key, count) => {
      // retain deterministic keyboard scrubbing
      for (let index = 0; index < count; index += 1) {
        await grid.press(key);
      }
    };
    await grid.press("Home");
    assert.equal(Number(await grid.getAttribute("aria-valuenow")), 0);
    await assertForecastLayout(page, width);
    const nearTitle = await page.locator(collisionFamilies.map(
      // inspect every local family's chosen bubble edge
      (family) => `[data-forecast-chart='${family}'] .forecast-chart-value`,
    ).join(", ")).evaluateAll(
      // retain the edge selected after collision fitting
      (values) => values.map((value) => ({
        bottom: value.classList.contains("forecast-chart-value-bottom"),
        key: value.closest("[data-forecast-chart]")?.getAttribute("data-forecast-chart"),
        top: value.classList.contains("forecast-chart-value-top"),
      })),
    );
    await move("ArrowRight", 13);
    assert.equal(Number(await grid.getAttribute("aria-valuenow")), 13);
    await assertForecastLayout(page, width);
    await grid.press("End");
    const last = Number(await grid.getAttribute("aria-valuenow"));
    assert.equal(last > 26, true);
    await assertForecastLayout(page, width);
    const awayFromTitle = await page.locator(collisionFamilies.map(
      // inspect the same bubbles outside the title zone
      (family) => `[data-forecast-chart='${family}'] .forecast-chart-value`,
    ).join(", ")).evaluateAll(
      // retain each restored preferred edge
      (values) => values.map((value) => ({
        bottom: value.classList.contains("forecast-chart-value-bottom"),
        key: value.closest("[data-forecast-chart]")?.getAttribute("data-forecast-chart"),
        top: value.classList.contains("forecast-chart-value-top"),
      })),
    );
    assert.equal(
      awayFromTitle.some(
        // require one preferred top edge to restore after leaving the title
        (edge) => edge.top && nearTitle.find((candidate) => candidate.key === edge.key)?.bottom === true,
      ),
      true,
      `forecast bubbles did not restore outside the title at ${String(width)}px: ${JSON.stringify({ awayFromTitle, nearTitle })}`,
    );
    await move("ArrowLeft", 13);
    assert.equal(Number(await grid.getAttribute("aria-valuenow")), last - 13);
    await assertForecastLayout(page, width);
    await grid.press("Home");
    await assertForecastLayout(page, width);
  };

  fixture.state.currentRecords = currentRecords;
  fixture.state.forecastPayload = payload;

  try {
    browser = await launchBrowser();

    // verify desktop and both supported phone widths
    for (const viewport of [
      { height: 1000, width: 1440 },
      { height: 900, width: 960 },
      { height: 844, width: 390 },
      { height: 780, width: 320 },
    ]) {
      fixture.state.currentRecords = currentRecords;
      const page = await createFixturePage(browser, { timezoneId: site.timezone, viewport });
      const pageErrors = [];
      // retain asynchronous render failures
      page.on("pageerror", (error) => pageErrors.push(error.message));
      await page.clock.setFixedTime(now);
      await page.goto(`${fixture.origin}/forecast`, { waitUntil: "networkidle" });
      await page.getByRole("button", { name: "5 days", exact: true }).click();
      await page.locator('[data-forecast-charts][data-forecast-days="5"]').waitFor();
      const toggle = page.getByRole("switch", { name: "Adjusted", exact: true });
      assert.equal(await toggle.getAttribute("aria-checked"), "true");
      const adjusted = await chartSeries(page);
      const adjustedTemperature = JSON.parse(
        await page.locator("[data-forecast-chart='temperature']").getAttribute("data-forecast-series"),
      );
      const familyLabels = {
        "air-quality": "Air quality",
        humidity: "Humidity",
        pressure: "Pressure",
        "uv-index": "UV index",
      };
      // keep accepted local estimators free of internal status labels
      for (const [family, label] of Object.entries(familyLabels)) {
        assert.equal(
          await page.locator(`[data-forecast-chart='${family}'] .forecast-chart-heading h3 > span:last-child`).innerText(),
          label,
        );
      }
      assert.doesNotMatch(await page.locator("body").innerText(), /experimental/iu);
      await assertForecastLayout(page, viewport.width);
      await assertCrosshairClear(page, viewport.width);
      const api = await page.evaluate(
        // inspect the same immutable API body consumed by the browser
        async () => await fetch("/api/v1/sites/ballydidean/forecast").then(
          // parse one normalized response
          async (response) => await response.json(),
        ),
      );
      assert.deepEqual(api, payloadSnapshot);
      assert.deepEqual(api.data[3].metrics, forecastRecords[3].metrics);
      assert.equal(JSON.stringify(api).includes("local-forecast-bias"), false);

      await toggle.click();
      await page.waitForFunction(
        // wait for all four local families to return raw
        () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "false" &&
          JSON.parse(document.querySelector("[data-forecast-chart='humidity']")?.getAttribute("data-forecast-series") ?? "[]")[0]?.values?.[3] === 50,
      );
      const raw = await chartSeries(page);
      const rawTemperature = JSON.parse(
        await page.locator("[data-forecast-chart='temperature']").getAttribute("data-forecast-series"),
      );
      assert.doesNotMatch(await page.locator("body").innerText(), /experimental/iu);
      await assertForecastLayout(page, viewport.width);
      await assertCrosshairClear(page, viewport.width);
      // compare all four displayed current-hour biases
      assert.deepEqual(
        Object.fromEntries(localFamilies.map((family) => [family, [raw[family][3], adjusted[family][3]]])),
        {
          "air-quality": [20, 10],
          humidity: [50, 70],
          pressure: [0.2999999999999545, 1.7999999999999545],
          "uv-index": [6, 3],
        },
      );
      // retain past hours and expire exactly at the 24-hour boundary
      for (const family of localFamilies) {
        assert.equal(adjusted[family][2], raw[family][2], `${family} changed a past hour`);
        assert.notEqual(adjusted[family][26], raw[family][26], `${family} missed the final covered hour`);
        assert.equal(adjusted[family][27], raw[family][27], `${family} crossed its 24-hour expiry`);
      }
      assert.deepEqual(adjustedTemperature[0], rawTemperature[0]);
      assert.notEqual(adjustedTemperature[1].values[3], rawTemperature[1].values[3]);
      assert.equal(adjustedTemperature[1].values[3], forecastRecords[3].metrics.temperatureC + 2);

      // retain one selected hour while responsive geometry changes around it
      if (viewport.width === 1440) {
        const grid = page.locator("[data-forecast-charts]");
        await grid.press("Home");
        // select one point that fits at desktop but crosses phone headings
        for (let index = 0; index < 9; index += 1) {
          await grid.press("ArrowRight");
        }
        const selected = await grid.getAttribute("data-forecast-selected-position");
        const edges = async () => await page.locator(collisionFamilies.map(
          // inspect every local family's responsive edge
          (family) => `[data-forecast-chart='${family}'] .forecast-chart-value`,
        ).join(", ")).evaluateAll(
          // retain each edge by stable chart key
          (values) => Object.fromEntries(values.map((value) => [
            value.closest("[data-forecast-chart]")?.getAttribute("data-forecast-chart"),
            value.classList.contains("forecast-chart-value-top") ? "top" : "bottom",
          ])),
        );
        const desktopEdges = await edges();
        assert.equal(Object.values(desktopEdges).includes("top"), true);

        // force collision fitting at both phone breakpoints
        const phoneEdges = [];
        for (const resized of [
          { height: 844, width: 390 },
          { height: 780, width: 320 },
        ]) {
          await page.setViewportSize(resized);
          await page.evaluate(
            // wait for responsive observers and one painted chart frame
            async () => await new Promise((resolveFrame) => requestAnimationFrame(() => resolveFrame())),
          );
          assert.equal(await grid.getAttribute("data-forecast-selected-position"), selected);
          await assertForecastLayout(page, resized.width);
          phoneEdges.push(await edges());
        }
        assert.equal(
          Object.entries(desktopEdges).some(
            // require a desktop top edge to move below a phone heading
            ([family, edge]) => edge === "top" && phoneEdges.every((entry) => entry[family] === "bottom"),
          ),
          true,
        );
        await page.setViewportSize(viewport);
        await page.evaluate(
          // wait for the restored desktop fit
          async () => await new Promise((resolveFrame) => requestAnimationFrame(() => resolveFrame())),
        );
        assert.equal(await grid.getAttribute("data-forecast-selected-position"), selected);
        await assertForecastLayout(page, viewport.width);
        assert.deepEqual(await edges(), desktopEdges);
      }

      await toggle.click();
      await page.waitForFunction(
        // wait for the local humidity estimate to return
        () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "true" &&
          JSON.parse(document.querySelector("[data-forecast-chart='humidity']")?.getAttribute("data-forecast-series") ?? "[]")[0]?.values?.[3] === 70,
      );
      await page.reload({ waitUntil: "networkidle" });
      assert.equal(await toggle.getAttribute("aria-checked"), "true");
      await page.getByRole("button", { name: "5 days", exact: true }).click();
      await page.locator('[data-forecast-charts][data-forecast-days="5"]').waitFor();
      assert.equal((await chartSeries(page)).humidity[3], 70);
      await assertForecastLayout(page, viewport.width);

      // exercise both fail-raw source gates once
      if (viewport.width === 960) {
        fixture.state.currentRecords = currentRecords.map(
          // retire every physical observation past its strict freshness limit
          (record) => record.provenance.sourceKind === "physical_sensor"
            ? {
              ...record,
              freshness: { ageSeconds: 960, label: "Station reading is stale", status: "stale" },
              receivedAt: "2026-06-21T19:44:00.000Z",
              validAt: "2026-06-21T19:44:00.000Z",
            }
            : record,
        );
        await page.reload({ waitUntil: "networkidle" });
        await page.getByRole("button", { name: "5 days", exact: true }).click();
        await page.locator('[data-forecast-charts][data-forecast-days="5"]').waitFor();
        assert.deepEqual(await chartSeries(page), raw);
        fixture.state.currentRecords = currentRecords.filter(
          // remove only the required regional baseline
          (record) => record.provenance.sourceKind !== "model_current",
        );
        await page.reload({ waitUntil: "networkidle" });
        await page.getByRole("button", { name: "5 days", exact: true }).click();
        await page.locator('[data-forecast-charts][data-forecast-days="5"]').waitFor();
        assert.deepEqual(await chartSeries(page), raw);
      }
      assert.deepEqual(fixture.state.forecastPayload, payloadSnapshot);
      assert.deepEqual(pageErrors, []);
      assert.equal((await page.screenshot()).byteLength > 1_000, true);
      await page.close();
    }
  } finally {
    // close disposable browser resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// replace past forecast points with recorded hourly conditions in every display mode
test("forecast history uses recorded hourly conditions before raw regional fallback", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  const now = new Date("2026-08-22T05:37:00.000Z");
  const metricKeys = [
    "apparentTemperatureC",
    "cloudCoverPercent",
    "pm25MicrogramsPerCubicMeter",
    "precipitationRateMmPerHour",
    "relativeHumidityPercent",
    "temperatureC",
    "uvIndex",
    "windGustMps",
    "windSpeedMps",
  ];
  const emptyMetrics = Object.fromEntries(metricKeys.map((key) => [key, null]));
  // create one projected hourly response point
  const observation = (index, metrics, sources, pressureChange3hHpa, pressureSource) => ({
    metrics: { ...emptyMetrics, ...metrics },
    pressureChange3hHpa,
    sources: {
      ...sources,
      ...(pressureSource === null ? {} : { pressureChange3hHpa: pressureSource }),
    },
    validAt: forecast[index].validAt,
  });
  const preferred = observation(
    3,
    {
      apparentTemperatureC: 13,
      cloudCoverPercent: 30,
      pm25MicrogramsPerCubicMeter: 10,
      precipitationRateMmPerHour: 2,
      relativeHumidityPercent: 91,
      temperatureC: 30,
      uvIndex: 4,
      windGustMps: 7,
      windSpeedMps: 3,
    },
    {
      apparentTemperatureC: "local_mean",
      cloudCoverPercent: "local_mean",
      pm25MicrogramsPerCubicMeter: "local_mean",
      precipitationRateMmPerHour: "ballydidean",
      relativeHumidityPercent: "ballydidean",
      temperatureC: "ballydidean",
      uvIndex: "regional",
      windGustMps: "local_mean",
      windSpeedMps: "local_mean",
    },
    6,
    "ballydidean",
  );
  const zero = observation(
    4,
    Object.fromEntries(metricKeys.map((key) => [key, 0])),
    Object.fromEntries(metricKeys.map((key) => [key, "ballydidean"])),
    0,
    "regional",
  );
  const missing = observation(5, {}, {}, null, null);
  const observations = [preferred, zero, missing];
  const payload = forecastResponse(
    "active",
    { version: 1, temperature: true, wind: true, rain: true },
    forecast,
  );
  const payloadSnapshot = structuredClone(payload);
  const chartKeys = [
    "temperature",
    "wind",
    "rain-rate",
    "clouds",
    "humidity",
    "air-quality",
    "uv-index",
    "pressure",
    "tide",
  ];
  // read every serialized line that drives the visible charts
  const chartSeries = async (page) => Object.fromEntries(await Promise.all(chartKeys.map(
    // retain stable chart ordering
    async (key) => [
      key,
      JSON.parse(await page.locator(`[data-forecast-chart='${key}']`).getAttribute("data-forecast-series")),
    ],
  )));
  const observationReads = () => fixture.state.requests.filter(
    // count only projected graph recordings
    (request) => request === "GET /api/v1/sites/ballydidean/forecast-observations",
  ).length;

  fixture.state.adjustmentMode = "active";
  fixture.state.forecastObservations = observations;
  fixture.state.forecastObservationsGeneratedAt = now.toISOString();
  fixture.state.forecastPayload = payload;

  try {
    browser = await launchBrowser();

    // prove the same historical splice on desktop and phone layouts
    for (const viewport of [
      { height: 1_000, width: 1_440 },
      { height: 844, width: 390 },
    ]) {
      fixture.state.forecastObservations = structuredClone(observations);
      fixture.state.forecastObservationsGeneratedAt = now.toISOString();
      const readsBefore = observationReads();
      const page = await createFixturePage(browser, { timezoneId: site.timezone, viewport });
      const pageErrors = [];
      // retain asynchronous parsing and redraw failures
      page.on("pageerror", (error) => pageErrors.push(error.message));
      await page.clock.install({ time: now });
      await page.addInitScript(
        // keep rendered values directly comparable to metric fixtures
        ({ key, value }) => localStorage.setItem(key, JSON.stringify(value)),
        {
          key: UNIT_PREFERENCE_STORAGE_KEY,
          value: {
            precipitation: "millimeters",
            pressure: "hectopascals",
            temperature: "celsius",
            waterLevel: "meters",
            windSpeed: "meters_per_second",
          },
        },
      );
      await page.goto(fixture.origin, { waitUntil: "networkidle" });
      assert.equal(observationReads(), readsBefore);
      const observationResponse = page.waitForResponse(
        // capture the exact projected body consumed by the forecast route
        (response) => new URL(response.url()).pathname === "/api/v1/sites/ballydidean/forecast-observations",
      );
      await page.getByRole("link", { name: "Forecast", exact: true }).click();
      const consumed = await (await observationResponse).json();
      await page.locator('[data-forecast-charts][data-forecast-days="1"]').waitFor();
      await page.waitForFunction(
        // wait for the optional projection redraw after the required forecast renders
        () => JSON.parse(document.querySelector("[data-forecast-chart='temperature']")?.getAttribute("data-forecast-series") ?? "[]")[1]?.values?.[3] === 30,
      );
      assert.equal(observationReads(), readsBefore + 1);
      assert.deepEqual(consumed.data, observations);
      assert.doesNotMatch(await page.locator("body").innerText(), /experimental/iu);

      const adjusted = await chartSeries(page);
      assert.deepEqual(adjusted.temperature.map((series) => series.values[3]), [13, 30]);
      assert.deepEqual(adjusted.wind.map((series) => series.values[3]), [3, 7]);
      assert.equal(adjusted["rain-rate"][0].values[3], 2);
      assert.equal(adjusted.clouds[0].values[3], 30);
      assert.equal(adjusted.humidity[0].values[3], 91);
      assert.equal(adjusted["air-quality"][0].values[3], 10);
      assert.equal(adjusted["uv-index"][0].values[3], 4);
      assert.equal(adjusted.pressure[0].values[3], 6);
      // keep genuine recorded zeros instead of treating them as missing
      for (const key of chartKeys.filter((key) => key !== "tide")) {
        assert.equal(
          adjusted[key].every((series) => series.values[4] === 0),
          true,
          `${key} discarded a recorded zero`,
        );
      }
      // bypass governed adjustments when a past field falls back to regional raw
      assert.equal(adjusted.temperature[1].values[5], forecast[5].metrics.temperatureC);
      assert.equal(adjusted.wind[0].values[5], forecast[5].metrics.windSpeedMps);
      // leave future hours under their normal governed forecast behavior
      assert.equal(adjusted.temperature[1].values[23], forecast[23].metrics.temperatureC + 2);
      assert.equal(adjusted.wind[0].values[23], forecast[23].metrics.windSpeedMps + 0.4);

      const grid = page.locator("[data-forecast-charts]");
      await grid.focus();
      await grid.press("Home");
      // select the mixed-source historical hour
      for (let index = 0; index < 3; index += 1) {
        await grid.press("ArrowRight");
      }
      assert.match(await page.locator("[data-forecast-chart='temperature'] .forecast-chart-value").innerText(), /Feels like\s*13\s*°C[\s\S]*Air Temp\s*30\s*°C/iu);
      assert.match(await page.locator("[data-forecast-chart='wind'] .forecast-chart-value").innerText(), /Wind\s*3\s*m\/s[\s\S]*Gust\s*7\s*m\/s/iu);
      assert.match(await page.locator("[data-forecast-chart='humidity'] .forecast-chart-value").innerText(), /91\s*%/u);
      assert.match(await page.locator("[data-forecast-chart='pressure'] .forecast-chart-value").innerText(), /\+6/u);

      const toggle = page.getByRole("switch", { name: "Adjusted", exact: true });
      await toggle.click();
      await page.waitForFunction(
        // wait for raw future temperature while retaining historical recordings
        (value) => JSON.parse(document.querySelector("[data-forecast-chart='temperature']")?.getAttribute("data-forecast-series") ?? "[]")[1]?.values?.[23] === value,
        forecast[23].metrics.temperatureC,
      );
      const raw = await chartSeries(page);
      const times = JSON.parse(await grid.getAttribute("data-forecast-times"));
      const pastIndices = times.slice(0, 24).flatMap(
        // retain only hours behind the graph's regional now marker
        (validAt, index) => Date.parse(validAt) < Date.parse(current.validAt) ? [index] : [],
      );
      // compare every chart family across the display modes
      for (const key of chartKeys) {
        // compare each rendered line independently
        for (const [seriesIndex, series] of adjusted[key].entries()) {
          // require identity at every completed hour
          for (const index of pastIndices) {
            assert.equal(
              series.values[index],
              raw[key][seriesIndex].values[index],
              `${key} changed recorded hour ${String(index)} with the toggle`,
            );
          }
        }
      }
      assert.notEqual(adjusted.temperature[1].values[23], raw.temperature[1].values[23]);
      assert.notEqual(adjusted.wind[0].values[23], raw.wind[0].values[23]);
      assert.deepEqual(adjusted.tide, raw.tide);
      assert.equal(observationReads(), readsBefore + 1);

      // range changes reuse the same bounded observation response
      await page.getByRole("button", { name: "5 days", exact: true }).click();
      await page.locator('[data-forecast-charts][data-forecast-days="5"]').waitFor();
      await page.getByRole("button", { name: "Today", exact: true }).click();
      await page.locator('[data-forecast-charts][data-forecast-days="1"]').waitFor();
      assert.equal(observationReads(), readsBefore + 1);

      fixture.state.forecastObservations = fixture.state.forecastObservations.map(
        // update one recorded farm temperature for the scheduled refresh
        (point) => point.validAt === preferred.validAt
          ? { ...point, metrics: { ...point.metrics, temperatureC: 31 } }
          : point,
      );
      fixture.state.forecastObservationsGeneratedAt = new Date(now.getTime() + 300_100).toISOString();
      const refreshed = page.waitForResponse(
        // await the five-minute observation refresh
        (response) => new URL(response.url()).pathname === "/api/v1/sites/ballydidean/forecast-observations",
      );
      await page.clock.fastForward(300_100);
      await refreshed;
      await page.waitForFunction(
        // require the refreshed recording in raw mode
        () => JSON.parse(document.querySelector("[data-forecast-chart='temperature']")?.getAttribute("data-forecast-series") ?? "[]")[1]?.values?.[3] === 31,
      );
      assert.equal(observationReads(), readsBefore + 2);
      await toggle.click();
      assert.equal(await toggle.getAttribute("aria-checked"), "true");
      assert.equal((await chartSeries(page)).temperature[1].values[3], 31);
      assert.equal(observationReads(), readsBefore + 2);
      assert.deepEqual(fixture.state.forecastPayload, payloadSnapshot);
      assert.doesNotMatch(await page.locator("body").innerText(), /experimental/iu);
      assert.equal(
        await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth),
        0,
      );
      assert.deepEqual(pageErrors, []);
      assert.equal((await page.screenshot()).byteLength > 1_000, true);
      await page.close();
    }
  } finally {
    // close disposable browser resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

test("manifest and service worker provide an installable application shell", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 780, width: 390 },
    });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    // keep the browser title aligned with the application name
    assert.equal(await page.title(), "Ballydídean Weather");
    assert.equal(
      await page.locator('link[rel="manifest"]').getAttribute("href"),
      "/manifest.webmanifest",
    );
    assert.equal(
      await page.locator('link[rel="icon"][type="image/png"]').getAttribute("href"),
      "/brand/ballydidean-weather-icon-32.png",
    );
    assert.equal(
      await page.locator('link[rel="apple-touch-icon"]').getAttribute("href"),
      "/brand/ballydidean-weather-icon-180.png",
    );
    const manifest = await page.evaluate(
      // read the same-origin installation metadata
      async () => await fetch("/manifest.webmanifest").then(
        // parse one manifest response
        async (response) => await response.json(),
      ),
    );
    assert.equal(manifest.name, "Ballydídean Weather");
    assert.equal(manifest.display, "standalone");
    assert.equal(manifest.start_url, "/");
    assert.deepEqual(
      manifest.icons.map(
        // capture current icon identities
        (icon) => icon.src,
      ),
      [
        "/brand/ballydidean-weather-icon-192.png",
        "/brand/ballydidean-weather-icon-512.png",
        "/brand/ballydidean-weather-icon-maskable-512.png",
      ],
    );
    assert.equal(
      manifest.icons.some(
        // require one adaptive launcher icon
        (icon) => icon.sizes === "512x512" && icon.purpose === "maskable",
      ),
      true,
    );
    // verify generated png dimensions
    assert.equal(readFileSync(join(publicRoot, "brand/ballydidean-weather-icon-32.png")).readUInt32BE(16), 32);
    assert.equal(readFileSync(join(publicRoot, "brand/ballydidean-weather-icon-180.png")).readUInt32BE(16), 180);
    assert.equal(readFileSync(join(publicRoot, "brand/ballydidean-weather-icon-192.png")).readUInt32BE(16), 192);
    assert.equal(readFileSync(join(publicRoot, "brand/ballydidean-weather-icon-512.png")).readUInt32BE(16), 512);
    assert.equal(readFileSync(join(publicRoot, "brand/ballydidean-weather-icon-maskable-512.png")).readUInt32BE(16), 512);
    assert.equal(
      manifest.icons.some(
        // require one compact launcher icon
        (icon) => icon.sizes === "192x192" && icon.purpose === "any",
      ),
      true,
    );
    const registration = await page.evaluate(async () => {
      const ready = await navigator.serviceWorker.ready;
      return {
        active: ready.active?.state ?? null,
        scriptUrl: ready.active?.scriptURL ?? null,
        scope: ready.scope,
        updateViaCache: ready.updateViaCache,
      };
    });
    assert.equal(registration.active, "activated");
    assert.equal(registration.scriptUrl, `${fixture.origin}/service-worker.js?release=browser-test`);
    assert.equal(registration.scope, `${fixture.origin}/`);
    assert.equal(registration.updateViaCache, "none");
    assert.equal(
      await page.evaluate(async () => (await caches.keys()).includes("ballydidean-weather-shell-browser-test")),
      true,
    );
    await page.context().setOffline(true);
    assert.equal(
      await page.evaluate(
        // prove a cached route survives network loss
        async () => await fetch("/forecast").then(
          // read one offline shell response
          (response) => response.status,
        ),
      ),
      200,
    );
    assert.equal(
      await page.evaluate(
        // keep display preferences available offline
        async () => await fetch("/settings").then(
          // read one offline settings shell
          (response) => response.status,
        ),
      ),
      200,
    );
    await page.context().setOffline(false);
  } finally {
    // close only disposable browser resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// keep current conditions in now navigation and the full title on one fixed-height row
test("homepage keeps weather in Now navigation and a one-line title through responsive rerenders", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 1280 },
    });
    const sunset = eveningSunTimes(site, new Date("2026-08-22T20:00:00Z")).sunset;

    // require one deterministic daylight boundary
    if (sunset === null) {
      throw new Error("fixture sunset is unavailable");
    }

    await page.clock.install({ time: new Date(sunset.getTime() - 10 * 60_000) });
    await page.route(/\/api\/v1\/sites\/ballydidean\/current$/u, async (route) => {
      const response = await route.fetch();
      const body = await response.json();
      body.data = body.data.map(
        // expose one known dry sensor reading alongside modeled cloud cover
        (record) => record.provenance.sourceKind === "physical_sensor"
          ? {
              ...record,
              metrics: { ...record.metrics, precipitationRateMmPerHour: 0 },
            }
          : record,
      );
      await route.fulfill({ json: body, response });
    });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    await page.evaluate(
      // settle both bundled display fonts before measuring line boxes
      async () => await document.fonts.ready,
    );

    const baselineHeights = new Map([
      [320, 61.96875],
      [360, 61.96875],
      [412, 61.96875],
      [768, 65.984375],
      [1280, 74.390625],
    ]);
    const baselineTitleSizes = new Map([
      [320, 20],
      [360, 22.5],
      [412, 25.75],
      [768, 38.4],
      [1280, 56],
    ]);
    const captureHeaderAndNow = async () => {
      const icon = page.locator(".section-nav-home img.section-nav-weather-icon");
      await icon.waitFor();
      await page.waitForFunction(
        // require decoded current-condition artwork
        () => {
          const image = document.querySelector(".section-nav-home img.section-nav-weather-icon");
          return image instanceof HTMLImageElement && image.complete && image.naturalWidth > 0;
        },
      );
      return await page.locator("main.shell").evaluate(
        // measure one complete rendered homepage header and now destination
        (shell) => {
          const masthead = shell.querySelector(".home-masthead");
          const actions = masthead?.querySelector(".masthead-actions");
          const title = masthead?.querySelector("h1");
          const home = shell.querySelector(".section-nav-home");
          const icon = home?.querySelector("img.section-nav-weather-icon");

          // require the complete title and current-condition destination
          if (
            !(masthead instanceof HTMLElement) ||
            !(actions instanceof HTMLElement) ||
            !(icon instanceof HTMLImageElement) ||
            !(title instanceof HTMLElement) ||
            !(home instanceof HTMLAnchorElement)
          ) {
            throw new Error("homepage header or now navigation is incomplete");
          }

          const bounds = (element) => {
            const rectangle = element.getBoundingClientRect();
            return {
              bottom: rectangle.bottom,
              height: rectangle.height,
              left: rectangle.left,
              right: rectangle.right,
              top: rectangle.top,
              width: rectangle.width,
            };
          };
          const mastheadBounds = masthead.getBoundingClientRect();
          const titleBounds = title.getBoundingClientRect();
          const wordBounds = [...title.querySelectorAll(".masthead-title-text > span")].map(
            // measure only the two visible title words
            (word) => word.getBoundingClientRect(),
          );
          const uniqueLineTops = new Set(wordBounds.map(
            // merge both words when they share a rendered line
            (rectangle) => Math.round(rectangle.top),
          ));
          const titleStyle = getComputedStyle(title);
          const verticalClipBounds = titleStyle.overflowY === "visible" ? mastheadBounds : titleBounds;

          return {
            actions: bounds(actions),
            alt: icon.alt,
            bodyOverflow: document.body.scrollWidth - document.documentElement.clientWidth,
            dashboardCount: home.querySelectorAll('svg[data-nav-icon="dashboard"]').length,
            fontFamily: titleStyle.fontFamily,
            fontSize: Number.parseFloat(titleStyle.fontSize),
            header: bounds(masthead),
            headerOverflow: masthead.scrollWidth - masthead.clientWidth,
            homeAriaCurrent: home.getAttribute("aria-current"),
            homeAriaLabel: home.getAttribute("aria-label"),
            homeHref: home.getAttribute("href"),
            icon: bounds(icon),
            iconComplete: icon.complete,
            iconFilter: getComputedStyle(icon).filter,
            iconHeightAttribute: icon.getAttribute("height"),
            iconInsideNow: icon.closest("a") === home,
            iconNaturalHeight: icon.naturalHeight,
            iconNaturalWidth: icon.naturalWidth,
            iconWidthAttribute: icon.getAttribute("width"),
            lineCount: uniqueLineTops.size,
            mastheadBrandCount: masthead.querySelectorAll(".masthead-brand").length,
            mastheadImageCount: masthead.querySelectorAll("img").length,
            source: new URL(icon.src).pathname,
            title: bounds(title),
            titleClippedHorizontally: wordBounds.some(
              // keep both visible words inside the title box
              (rectangle) => rectangle.left < titleBounds.left - 1 || rectangle.right > titleBounds.right + 1,
            ),
            titleClippedVertically: wordBounds.some(
              // contain text in the actual vertical clipping ancestor
              (rectangle) => rectangle.top < verticalClipBounds.top - 1 || rectangle.bottom > verticalClipBounds.bottom + 1,
            ),
            titleText: title.textContent?.replace(/\s+/gu, " ").trim(),
            titleIsDirectChild: title.parentElement === masthead,
            wordBounds: wordBounds.map(
              // retain concrete title fragments for failure evidence
              (rectangle) => bounds({ getBoundingClientRect: () => rectangle }),
            ),
            wrappedClass: title.classList.contains("masthead-title-wrapped"),
          };
        },
      );
    };

    const nowLink = page.getByRole("link", { name: "Now", exact: true });
    assert.equal(await nowLink.getAttribute("href"), "/");
    assert.equal(await nowLink.getAttribute("aria-label"), "Now");
    assert.equal(await nowLink.getAttribute("aria-current"), "page");
    assert.equal(await nowLink.locator("img.section-nav-weather-icon").count(), 1);
    assert.equal(await nowLink.locator('svg[data-nav-icon="dashboard"]').count(), 0);
    assert.equal(await nowLink.locator(".material-symbols-rounded").count(), 0);

    // prove the same document survives wide, narrow, and restored widths
    for (const width of [1280, 320, 360, 412, 768, 1280]) {
      await page.setViewportSize({ height: 900, width });
      await page.evaluate(
        // wait for any width-dependent font layout
        async () => await document.fonts.ready,
      );
      await page.waitForFunction(
        // wait for the fitted title and css-sized switch to settle
        () => {
          const masthead = document.querySelector(".home-masthead");
          const heading = masthead?.querySelector("h1");
          const text = heading?.querySelector(".masthead-title-text");
          const toggle = masthead?.querySelector("[data-forecast-adjustment-toggle]");
          const track = toggle?.querySelector(".forecast-adjustment-toggle-track");

          // require the complete responsive header
          if (
            !(masthead instanceof HTMLElement) ||
            !(heading instanceof HTMLElement) ||
            !(text instanceof HTMLElement) ||
            !(toggle instanceof HTMLElement) ||
            !(track instanceof HTMLElement)
          ) {
            return false;
          }

          return text.scrollWidth <= text.clientWidth + 1 &&
            Math.abs(toggle.getBoundingClientRect().height - track.getBoundingClientRect().height) < 0.25;
        },
        undefined,
        { polling: 10 },
      );
      const layout = await captureHeaderAndNow();
      const baselineHeight = baselineHeights.get(width);

      // require a recorded production baseline for every exercised width
      if (baselineHeight === undefined) {
        throw new Error(`missing masthead baseline for ${String(width)}px`);
      }

      assert.equal(Math.abs(layout.header.height - baselineHeight) < 1, true, JSON.stringify({ width, layout }));
      assert.equal(layout.alt, "Current weather: Partly cloudy");
      assert.equal(layout.bodyOverflow, 0);
      assert.equal(layout.dashboardCount, 0);
      assert.match(layout.fontFamily, /Google Sans Flex/u);
      assert.equal(layout.headerOverflow, 0);
      assert.equal(layout.homeAriaCurrent, "page");
      assert.equal(layout.homeAriaLabel, "Now");
      assert.equal(layout.homeHref, "/");
      assert.equal(layout.iconComplete, true);
      assert.equal(layout.iconFilter, "none");
      assert.equal(layout.icon.height, 32);
      assert.equal(layout.icon.width, 32);
      assert.equal(layout.iconHeightAttribute, "32");
      assert.equal(layout.iconInsideNow, true);
      assert.equal(layout.iconNaturalHeight > 0, true);
      assert.equal(layout.iconNaturalWidth > 0, true);
      assert.equal(layout.iconWidthAttribute, "32");
      assert.equal(layout.mastheadBrandCount, 0);
      assert.equal(layout.mastheadImageCount, 0);
      assert.equal(await page.locator(".refresh-indicator").count(), 0);
      assert.equal(await page.locator(".masthead [role='status']").count(), 0);
      assert.equal(layout.source, "/weather-icons/03-partly-cloudy.svg");
      assert.equal(layout.title.bottom <= layout.header.bottom + 1, true);
      assert.equal(layout.title.left >= layout.header.left - 1, true);
      assert.equal(layout.title.right <= layout.actions.left + 1, true);
      assert.equal(layout.title.top >= layout.header.top - 1, true);
      assert.equal(layout.titleClippedHorizontally, false, JSON.stringify({ width, layout }));
      assert.equal(layout.titleClippedVertically, false, JSON.stringify({ width, layout }));
      assert.equal(layout.titleIsDirectChild, true);
      assert.equal(layout.titleText, "Ballydídean Weather");
      const baselineTitleSize = baselineTitleSizes.get(width);

      // keep the largest useful one-line scale after removing the visible switch label
      if (baselineTitleSize === undefined) {
        throw new Error(`missing title baseline for ${String(width)}px`);
      }

      assert.equal(layout.fontSize <= baselineTitleSize + 0.1, true, JSON.stringify({ width, layout }));
      assert.equal(layout.fontSize >= baselineTitleSize * 0.85, true, JSON.stringify({ width, layout }));
      assert.equal(layout.lineCount, 1);
      assert.equal(layout.wrappedClass, false);
    }

    const currentReadsBeforeBoundary = fixture.state.requests.filter(
      // count current-only refreshes at the daylight boundary
      (entry) => entry === "GET /api/v1/sites/ballydidean/current",
    ).length;
    const otherWeatherReadsBeforeBoundary = fixture.state.requests.filter(
      // exclude current polling and optional private layout reads
      (entry) => entry.startsWith("GET /api/v1/sites/ballydidean/") &&
        entry !== "GET /api/v1/sites/ballydidean/current" &&
        !entry.endsWith("/property-sensor-layout"),
    ).length;
    await page.clock.fastForward(10 * 60_000 + 2_000);
    await page.waitForFunction(
      // require the scheduled sunset replacement
      () => document.querySelector(".section-nav-weather-icon")?.getAttribute("src") ===
        "/weather-icons/15-partly-cloudy-night.svg",
    );
    assert.equal((await captureHeaderAndNow()).alt, "Current weather: Partly cloudy night");
    assert.equal(fixture.state.requests.filter(
      // refresh only current sunlight inputs at the boundary
      (entry) => entry === "GET /api/v1/sites/ballydidean/current",
    ).length, currentReadsBeforeBoundary + 1);
    assert.equal(fixture.state.requests.filter(
      // keep forecasts tides and daily products unchanged
      (entry) => entry.startsWith("GET /api/v1/sites/ballydidean/") &&
        entry !== "GET /api/v1/sites/ballydidean/current" &&
        !entry.endsWith("/property-sensor-layout"),
    ).length, otherWeatherReadsBeforeBoundary);

    const currentReadsBeforeVisibility = fixture.state.requests.filter(
      // capture current-only polling before two rapid resumes
      (entry) => entry === "GET /api/v1/sites/ballydidean/current",
    ).length;
    await page.clock.setSystemTime(new Date(sunset.getTime() - 1_000));
    const resumedCurrent = page.waitForResponse(
      // require at least one current-only resume response
      (response) => new URL(response.url()).pathname === "/api/v1/sites/ballydidean/current",
    );
    await page.evaluate(
      // simulate a foreground resume immediately before sunset
      () => document.dispatchEvent(new Event("visibilitychange")),
    );
    await resumedCurrent;
    await page.waitForFunction(
      // restore daylight artwork after the current-only resume
      () => document.querySelector(".section-nav-weather-icon")?.getAttribute("src") ===
        "/weather-icons/03-partly-cloudy.svg",
    );
    await page.clock.setSystemTime(new Date(sunset.getTime() + 1_000));
    await page.evaluate(
      // simulate a foreground resume immediately after sunset
      () => document.dispatchEvent(new Event("visibilitychange")),
    );
    await page.waitForFunction(
      // restore night artwork without a full weather reload
      () => document.querySelector(".section-nav-weather-icon")?.getAttribute("src") ===
        "/weather-icons/15-partly-cloudy-night.svg",
    );
    const currentReadsAfterVisibility = fixture.state.requests.filter(
      // allow overlap deduplication across the rapid visibility events
      (entry) => entry === "GET /api/v1/sites/ballydidean/current",
    ).length;
    assert.equal(currentReadsAfterVisibility > currentReadsBeforeVisibility, true);
    assert.equal(currentReadsAfterVisibility <= currentReadsBeforeVisibility + 2, true);
    assert.equal(fixture.state.requests.filter(
      // keep every non-current weather product stable on resume
      (entry) => entry.startsWith("GET /api/v1/sites/ballydidean/") &&
        entry !== "GET /api/v1/sites/ballydidean/current" &&
        !entry.endsWith("/property-sensor-layout"),
    ).length, otherWeatherReadsBeforeBoundary);

    await page.setViewportSize({ height: 900, width: 320 });
    const originalIcon = await page.locator("img.section-nav-weather-icon").elementHandle();
    const toggle = page.getByRole("switch", { name: "Adjusted", exact: true });
    const previousToggleState = await toggle.getAttribute("aria-checked");

    // require the current image before proving its replacement
    if (originalIcon === null) {
      throw new Error("now weather icon is unavailable before rerender");
    }

    await toggle.click();
    await page.waitForFunction(
      // require the controller to replace the old navigation artwork
      ([image, state]) => !image.isConnected &&
        document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") !== state,
      [originalIcon, previousToggleState],
    );
    const rerenderedLayout = await captureHeaderAndNow();
    assert.equal(Math.abs(rerenderedLayout.header.height - 61.96875) < 1, true);
    // keep raw artwork unavailable when modeled rain is unknown
    assert.equal(rerenderedLayout.alt, "Current weather: Conditions unavailable");
    assert.equal(rerenderedLayout.source, "/weather-icons/12-unavailable.svg");
    assert.equal(rerenderedLayout.lineCount, 1);
    assert.equal(rerenderedLayout.mastheadImageCount, 0);
    assert.equal(rerenderedLayout.titleClippedHorizontally, false);
    assert.equal(rerenderedLayout.titleClippedVertically, false);
    assert.equal(rerenderedLayout.wrappedClass, false);

    await page.getByRole("link", { name: "Forecast", exact: true }).click();
    await page.waitForURL(`${fixture.origin}/forecast`);
    await page.locator(".forecast-panel").waitFor();
    assert.equal(await page.locator(".masthead img").count(), 0);
    assert.equal(await page.locator(".masthead-brand").count(), 0);
    assert.equal(await page.locator(".masthead h1").innerText(), "Ballydídean Weather");
    assert.equal(await nowLink.getAttribute("href"), "/");
    assert.equal(await nowLink.getAttribute("aria-current"), null);
    assert.equal(await nowLink.locator("img.section-nav-weather-icon").getAttribute("alt"), "Current weather: Conditions unavailable");
    assert.equal(await nowLink.locator("img.section-nav-weather-icon").getAttribute("src"), "/weather-icons/12-unavailable.svg");
    assert.equal(await nowLink.locator('svg[data-nav-icon="dashboard"]').count(), 0);
    await nowLink.click();
    await page.waitForURL(`${fixture.origin}/`);
    assert.equal((await captureHeaderAndNow()).lineCount, 1);
  } finally {
    // close only disposable browser resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// keep local and history routes from fetching current data solely for navigation artwork
test("Now weather artwork preserves state across routes without widening fetch contracts", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 768 },
    });
    let raining = false;
    await page.clock.setFixedTime(new Date("2026-08-22T20:00:00Z"));
    await page.route(/\/api\/v1\/sites\/ballydidean\/current$/u, async (route) => {
      const response = await route.fetch();
      const body = await response.json();
      body.data = body.data.map(
        // switch only the preferred sensor rain rate
        (record) => record.provenance.sourceKind === "physical_sensor"
          ? {
              ...record,
              metrics: {
                ...record.metrics,
                precipitationRateMmPerHour: raining ? 3 : 0,
                windSpeedMps: 2.5,
              },
            }
          : record,
      );
      await route.fulfill({ json: body, response });
    });
    const currentReads = () => fixture.state.requests.filter(
      // count only the exact current endpoint
      (entry) => entry === "GET /api/v1/sites/ballydidean/current",
    ).length;
    const historyReads = () => fixture.state.requests.filter(
      // count each history page or filter request
      (entry) => entry.startsWith("GET /api/v1/sites/ballydidean/history"),
    ).length;
    const nowLink = page.getByRole("link", { name: "Now", exact: true });
    const nowIcon = nowLink.locator("img.section-nav-weather-icon");

    await page.goto(`${fixture.origin}/settings`, { waitUntil: "networkidle" });
    await nowIcon.waitFor();
    assert.equal(currentReads(), 0);
    assert.equal(historyReads(), 0);
    assert.equal(await page.locator(".masthead img").count(), 0);
    assert.equal(await nowLink.getAttribute("aria-label"), "Now");
    assert.equal(await nowIcon.getAttribute("alt"), "Current weather: Conditions unavailable");
    assert.equal(await nowIcon.getAttribute("src"), "/weather-icons/12-unavailable.svg");
    assert.equal(await nowIcon.getAttribute("width"), "32");
    assert.equal(await nowIcon.getAttribute("height"), "32");
    assert.equal(await nowLink.locator('svg[data-nav-icon="dashboard"]').count(), 0);

    await page.getByRole("link", { name: "Logs", exact: true }).click();
    await page.waitForURL(`${fixture.origin}/logs`);
    await page.locator("table caption").waitFor();
    assert.equal(currentReads(), 0);
    assert.equal(historyReads(), 1);
    assert.equal(await page.locator(".masthead img").count(), 0);
    assert.equal(await nowIcon.getAttribute("src"), "/weather-icons/12-unavailable.svg");

    await nowLink.click();
    await page.waitForURL(`${fixture.origin}/`);
    await page.waitForFunction(
      // require current data to replace the missing-state artwork
      () => document.querySelector(".section-nav-weather-icon")?.getAttribute("src") ===
        "/weather-icons/03-partly-cloudy.svg",
    );
    assert.equal(currentReads(), 1);
    assert.equal(historyReads(), 1);
    assert.equal(await nowIcon.getAttribute("alt"), "Current weather: Partly cloudy");

    await page.getByRole("link", { name: "Settings", exact: true }).click();
    await page.waitForURL(`${fixture.origin}/settings`);
    assert.equal(currentReads(), 1);
    assert.equal(historyReads(), 1);
    assert.equal(await nowIcon.getAttribute("src"), "/weather-icons/03-partly-cloudy.svg");
    await page.getByRole("link", { name: "Logs", exact: true }).click();
    await page.waitForURL(`${fixture.origin}/logs`);
    await page.locator("table caption").waitFor();
    assert.equal(currentReads(), 1);
    assert.equal(historyReads(), 2);
    assert.equal(await nowIcon.getAttribute("src"), "/weather-icons/03-partly-cloudy.svg");

    raining = true;
    await page.getByRole("link", { name: "Forecast", exact: true }).click();
    await page.waitForURL(`${fixture.origin}/forecast`);
    await page.waitForFunction(
      // require route data to update the retained now illustration
      () => document.querySelector(".section-nav-weather-icon")?.getAttribute("src") ===
        "/weather-icons/09-heavy-rain.svg",
    );
    assert.equal(currentReads(), 2);
    assert.equal(historyReads(), 2);
    assert.equal(await page.locator(".masthead img").count(), 0);
    assert.equal(await nowIcon.getAttribute("alt"), "Current weather: Heavy rain");
    assert.equal(await nowLink.getAttribute("aria-current"), null);
    await nowLink.click();
    await page.waitForURL(`${fixture.origin}/`);
    assert.equal(await nowIcon.getAttribute("src"), "/weather-icons/09-heavy-rain.svg");
    assert.equal(await nowLink.getAttribute("aria-current"), "page");
  } finally {
    // close disposable fixture resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// retain one sanitized current-condition summary without flashing unavailable artwork
test("Now weather artwork restores a safe cache and settles cold skeletons honestly", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const cacheKey = "weather.now-icon.ballydidean.v1";
    const sunset = eveningSunTimes(site, new Date("2026-08-22T20:00:00.000Z")).sunset;

    // require one deterministic near-sunset cache window
    if (sunset === null) {
      throw new Error("fixture sunset is unavailable");
    }

    const now = new Date(sunset.getTime() - 10 * 60_000);
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 320 },
    });
    let rain = 0;
    let responseMode = "usable";
    const currentGates = [];

    // hold exactly one upcoming current read
    const holdNextCurrent = () => {
      let markStarted;
      let release;
      const started = new Promise(
        // expose the request interception boundary
        (resolveStarted) => {
          markStarted = resolveStarted;
        },
      );
      const pending = new Promise(
        // expose the response release boundary
        (resolveRelease) => {
          release = resolveRelease;
        },
      );
      currentGates.push({ markStarted, pending });
      return { release, started };
    };
    // read one complete navigation-artwork state
    const readNowArtwork = async (targetPage = page) => await targetPage.locator(".section-nav-home").evaluate(
      // distinguish loading, weather, and unavailable artwork
      (home) => {
        const image = home.querySelector("img.section-nav-weather-icon");
        const skeleton = home.querySelector(".section-nav-weather-skeleton");
        const skeletonBounds = skeleton?.getBoundingClientRect();
        return {
          alt: image?.getAttribute("alt") ?? null,
          imageCount: home.querySelectorAll("img.section-nav-weather-icon").length,
          skeletonAriaLabel: skeleton?.getAttribute("aria-label") ?? null,
          skeletonClasses: skeleton === null ? [] : [...skeleton.classList],
          skeletonCount: home.querySelectorAll(".section-nav-weather-skeleton").length,
          skeletonHeight: skeletonBounds?.height ?? null,
          skeletonRole: skeleton?.getAttribute("role") ?? null,
          skeletonWidth: skeletonBounds?.width ?? null,
          source: image instanceof HTMLImageElement ? new URL(image.src).pathname : null,
        };
      },
    );
    // decode the bounded cache payload
    const readCache = async (targetPage = page) => await targetPage.evaluate((key) => {
      const value = localStorage.getItem(key);
      return value === null ? null : JSON.parse(value);
    }, cacheKey);
    // count only reads that could reveal current private sensor state
    const currentReads = () => fixture.state.requests.filter(
      // keep the exact current endpoint separate from route-local requests
      (entry) => entry === "GET /api/v1/sites/ballydidean/current",
    ).length;
    // read every distinct artwork state painted in the current document
    const observedArtwork = async (targetPage = page) => await targetPage.evaluate(
      () => window.__weatherNowArtworkStates ?? [],
    );

    await page.clock.setFixedTime(now);
    await page.addInitScript(() => {
      const states = [];
      window.__weatherNowArtworkStates = states;
      // retain only distinct rendered loading or image states
      const record = () => {
        const image = document.querySelector("img.section-nav-weather-icon");
        const skeleton = document.querySelector(".section-nav-weather-skeleton");
        const state = skeleton !== null
          ? "loading"
          : image instanceof HTMLImageElement
            ? new URL(image.src).pathname
            : null;

        // skip missing and duplicate mutation records
        if (state !== null && states.at(-1) !== state) {
          states.push(state);
        }
      };
      const observer = new MutationObserver(
        // sample each complete render replacement
        () => record(),
      );
      observer.observe(document, { childList: true, subtree: true });
      document.addEventListener("DOMContentLoaded", record, { once: true });
    });
    await page.route(/\/api\/v1\/sites\/ballydidean\/current$/u, async (route) => {
      const gate = currentGates.shift();

      // expose a deliberately pending current read
      if (gate !== undefined) {
        gate.markStarted();
        await gate.pending;
      }

      // return one explicit outage without hiding the restored cache
      if (responseMode === "failure") {
        await route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: { code: "unavailable" } }) });
        return;
      }

      const response = await route.fetch();
      const body = await response.json();
      body.data = body.data.map(
        // vary only fields admitted to the icon cache
        (record) => {
          // make one successful response intentionally unusable
          if (responseMode === "unavailable") {
            return {
              ...record,
              metrics: {
                ...record.metrics,
                cloudCoverPercent: record.provenance.sourceKind === "model_current"
                  ? null
                  : record.metrics.cloudCoverPercent,
                precipitationRateMmPerHour: record.provenance.sourceKind === "physical_sensor"
                  ? null
                  : record.metrics.precipitationRateMmPerHour,
              },
            };
          }

          // keep clouds modeled and rain plus wind sensor-derived
          return record.provenance.sourceKind === "physical_sensor"
            ? {
                ...record,
                metrics: {
                  ...record.metrics,
                  precipitationRateMmPerHour: rain,
                  windSpeedMps: 2.5,
                },
              }
            : record;
        },
      );
      await route.fulfill({ json: body, response });
    });

    const coldGate = holdNextCurrent();
    await page.goto(fixture.origin, { waitUntil: "domcontentloaded" });
    await coldGate.started;
    await page.getByRole("img", { name: "Loading current weather", exact: true }).waitFor();
    const cold = await readNowArtwork();
    assert.deepEqual(cold, {
      alt: null,
      imageCount: 0,
      skeletonAriaLabel: "Loading current weather",
      skeletonClasses: ["section-nav-weather-icon", "section-nav-weather-skeleton", "skeleton-line"],
      skeletonCount: 1,
      skeletonHeight: 32,
      skeletonRole: "img",
      skeletonWidth: 32,
      source: null,
    });
    assert.deepEqual(await observedArtwork(), ["loading"]);
    const coldScreenshot = await page.screenshot();
    assert.ok(coldScreenshot.byteLength > 500);
    coldGate.release();
    await page.waitForFunction(
      // require live current data to replace the cold skeleton
      () => document.querySelector("img.section-nav-weather-icon")?.getAttribute("src") ===
        "/weather-icons/03-partly-cloudy.svg",
    );
    assert.deepEqual(await readCache(), {
      cachedAt: now.getTime(),
      cloud: 42,
      forecastAdjustmentMode: "adjusted",
      rain: 0,
      windy: false,
    });

    rain = 3;
    const warmGate = holdNextCurrent();
    await page.reload({ waitUntil: "domcontentloaded" });
    await warmGate.started;
    assert.deepEqual(await readNowArtwork(), {
      alt: "Current weather: Partly cloudy",
      imageCount: 1,
      skeletonAriaLabel: null,
      skeletonClasses: [],
      skeletonCount: 0,
      skeletonHeight: null,
      skeletonRole: null,
      skeletonWidth: null,
      source: "/weather-icons/03-partly-cloudy.svg",
    });
    assert.deepEqual(await observedArtwork(), ["/weather-icons/03-partly-cloudy.svg"]);
    const warmScreenshot = await page.screenshot();
    assert.ok(warmScreenshot.byteLength > 500);
    warmGate.release();
    await page.waitForFunction(
      // let newer live rain replace the warm cached illustration
      () => document.querySelector("img.section-nav-weather-icon")?.getAttribute("src") ===
        "/weather-icons/09-heavy-rain.svg",
    );
    assert.deepEqual(await observedArtwork(), [
      "/weather-icons/03-partly-cloudy.svg",
      "/weather-icons/09-heavy-rain.svg",
    ]);
    assert.deepEqual(await readCache(), {
      cachedAt: now.getTime(),
      cloud: 42,
      forecastAdjustmentMode: "adjusted",
      rain: 3,
      windy: false,
    });

    rain = 0;
    await page.reload({ waitUntil: "networkidle" });
    await page.waitForFunction(
      // refresh the cache back to one sun-bearing summary
      () => document.querySelector("img.section-nav-weather-icon")?.getAttribute("src") ===
        "/weather-icons/03-partly-cloudy.svg",
    );
    const currentReadsBeforeLocalRoutes = currentReads();

    await page.clock.setFixedTime(new Date(sunset.getTime() + 1_000));
    await page.goto(`${fixture.origin}/settings`, { waitUntil: "networkidle" });
    assert.equal(currentReads(), currentReadsBeforeLocalRoutes);
    assert.equal((await readNowArtwork()).source, "/weather-icons/15-partly-cloudy-night.svg");
    assert.deepEqual(await observedArtwork(), ["/weather-icons/15-partly-cloudy-night.svg"]);
    await page.goto(`${fixture.origin}/logs`, { waitUntil: "networkidle" });
    await page.locator("table caption").waitFor({ state: "attached" });
    assert.equal(currentReads(), currentReadsBeforeLocalRoutes);
    assert.equal((await readNowArtwork()).source, "/weather-icons/15-partly-cloudy-night.svg");
    assert.deepEqual(await observedArtwork(), ["/weather-icons/15-partly-cloudy-night.svg"]);

    responseMode = "failure";
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    assert.equal((await readNowArtwork()).source, "/weather-icons/15-partly-cloudy-night.svg");
    assert.deepEqual(await observedArtwork(), ["/weather-icons/15-partly-cloudy-night.svg"]);
    assert.deepEqual(await readCache(), {
      cachedAt: now.getTime(),
      cloud: 42,
      forecastAdjustmentMode: "adjusted",
      rain: 0,
      windy: false,
    });

    responseMode = "unavailable";
    const missingGate = holdNextCurrent();
    await page.reload({ waitUntil: "domcontentloaded" });
    await missingGate.started;
    assert.equal((await readNowArtwork()).source, "/weather-icons/15-partly-cloudy-night.svg");
    missingGate.release();
    await page.waitForFunction(
      // clear stale cache only after a successful unavailable response
      () => document.querySelector("img.section-nav-weather-icon")?.getAttribute("src") ===
        "/weather-icons/12-unavailable.svg",
    );
    assert.equal(await readCache(), null);
    assert.deepEqual(await observedArtwork(), [
      "/weather-icons/15-partly-cloudy-night.svg",
      "loading",
      "/weather-icons/12-unavailable.svg",
    ]);

    const invalidPage = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 320 },
    });
    await invalidPage.clock.setFixedTime(now);
    await invalidPage.addInitScript(({ key }) => {
      localStorage.setItem(key, "{not-json");
    }, { key: cacheKey });
    let releaseInvalidCurrent;
    const invalidCurrentPending = new Promise(
      // hold the invalid-cache fallback in its cold loading state
      (resolveRelease) => {
        releaseInvalidCurrent = resolveRelease;
      },
    );
    await invalidPage.route(/\/api\/v1\/sites\/ballydidean\/current$/u, async (route) => {
      await invalidCurrentPending;
      await route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: { code: "unavailable" } }) });
    });
    await invalidPage.goto(fixture.origin, { waitUntil: "domcontentloaded" });
    await invalidPage.getByRole("img", { name: "Loading current weather", exact: true }).waitFor();
    assert.equal((await readNowArtwork(invalidPage)).imageCount, 0);
    releaseInvalidCurrent();
    await invalidPage.waitForFunction(
      // settle a failed cold read to honest unavailable artwork
      () => document.querySelector("img.section-nav-weather-icon")?.getAttribute("src") ===
        "/weather-icons/12-unavailable.svg",
    );
    await invalidPage.close();

    const expiredPage = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 320 },
    });
    await expiredPage.clock.setFixedTime(now);
    await expiredPage.addInitScript(({ key, value }) => {
      localStorage.setItem(key, JSON.stringify(value));
    }, {
      key: cacheKey,
      value: { cachedAt: now.getTime() - 30 * 60_000 - 1, cloud: 42, rain: 0, windy: false },
    });
    const expiredReadsBefore = currentReads();
    await expiredPage.goto(`${fixture.origin}/settings`, { waitUntil: "networkidle" });
    assert.equal(currentReads(), expiredReadsBefore);
    assert.deepEqual(await readNowArtwork(expiredPage), {
      alt: "Current weather: Conditions unavailable",
      imageCount: 1,
      skeletonAriaLabel: null,
      skeletonClasses: [],
      skeletonCount: 0,
      skeletonHeight: null,
      skeletonRole: null,
      skeletonWidth: null,
      source: "/weather-icons/12-unavailable.svg",
    });
    await expiredPage.close();
  } finally {
    // close disposable fixture resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// match the forecast-range height while preserving the complete switch interaction contract
test("adjustment switch matches the forecast range and moves a solid muted-gold or gray sparkle", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    fixture.state.adjustmentMode = "active";
    const page = await createFixturePage(browser, {
      hasTouch: true,
      timezoneId: "America/Los_Angeles",
      viewport: { height: 844, width: 320 },
    });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    const toggle = page.getByRole("switch", { name: "Adjusted", exact: true });
    // capture the switch after each input-driven rerender
    const captureSwitch = async () => await toggle.evaluate(
      // measure the visible control and its complete artwork contract
      (button) => {
        const track = button.querySelector(".forecast-adjustment-toggle-track");
        const thumb = track?.querySelector(".forecast-adjustment-toggle-thumb");
        const sparkle = thumb?.querySelector("svg.forecast-adjustment-sparkle");
        const ink = sparkle?.querySelector(".forecast-adjustment-sparkle-ink");
        const masthead = button.closest(".masthead");
        const title = masthead?.querySelector("h1");
        const titleText = title?.querySelector(".masthead-title-text");

        // require every switch layer
        if (
          !(track instanceof HTMLElement) ||
          !(thumb instanceof HTMLElement) ||
          !(sparkle instanceof SVGElement) ||
          !(ink instanceof SVGElement) ||
          !(masthead instanceof HTMLElement) ||
          !(title instanceof HTMLElement) ||
          !(titleText instanceof HTMLElement)
        ) {
          throw new Error("adjustment sparkle switch is incomplete");
        }

        // compare the nested artwork bounds
        const bounds = (element) => {
          const rectangle = element.getBoundingClientRect();
          return {
            bottom: rectangle.bottom,
            height: rectangle.height,
            left: rectangle.left,
            right: rectangle.right,
            top: rectangle.top,
            width: rectangle.width,
          };
        };
        const buttonBounds = bounds(button);
        const buttonStyle = getComputedStyle(button);
        const sparkleBounds = bounds(sparkle);
        const sparkleStyle = getComputedStyle(sparkle);
        const thumbBounds = bounds(thumb);
        const trackBounds = bounds(track);
        const titleBounds = bounds(title);
        const titleStyle = getComputedStyle(title);
        const titleWordBounds = [...titleText.querySelectorAll("span")].map(
          // measure each visible title word
          (word) => word.getBoundingClientRect(),
        );

        return {
          ariaChecked: button.getAttribute("aria-checked"),
          ariaLabel: button.getAttribute("aria-label"),
          button: buttonBounds,
          buttonBackground: buttonStyle.backgroundColor,
          buttonBorderWidths: [
            buttonStyle.borderTopWidth,
            buttonStyle.borderRightWidth,
            buttonStyle.borderBottomWidth,
            buttonStyle.borderLeftWidth,
          ],
          buttonBoxShadow: buttonStyle.boxShadow,
          buttonFocused: document.activeElement === button,
          buttonOutlineStyle: buttonStyle.outlineStyle,
          buttonOutlineWidth: buttonStyle.outlineWidth,
          gradientElementCount: sparkle.querySelectorAll("defs, linearGradient, stop").length,
          inkFill: ink.getAttribute("fill"),
          inkRenderedFill: getComputedStyle(ink).fill,
          inkRenderedStroke: getComputedStyle(ink).stroke,
          inkStroke: ink.getAttribute("stroke"),
          masthead: bounds(masthead),
          sparkle: sparkleBounds,
          sparkleCentered: Math.abs(
            sparkleBounds.left + sparkleBounds.width / 2 -
              (thumbBounds.left + thumbBounds.width / 2),
          ) < 1 && Math.abs(
            sparkleBounds.top + sparkleBounds.height / 2 -
              (thumbBounds.top + thumbBounds.height / 2),
          ) < 1,
          sparkleDisplay: sparkleStyle.display,
          sparkleHiddenByParent: sparkle.closest('[aria-hidden="true"]') !== null,
          sparkleOpacity: sparkleStyle.opacity,
          sparkleTitleCount: sparkle.querySelectorAll("title").length,
          sparkleColor: sparkleStyle.color,
          text: button.textContent?.trim(),
          thumb: thumbBounds,
          thumbRightInset: trackBounds.right - thumbBounds.right,
          thumbTopInset: thumbBounds.top - trackBounds.top,
          thumbContained: thumbBounds.left >= trackBounds.left &&
            thumbBounds.left + thumbBounds.width <= trackBounds.left + trackBounds.width &&
            thumbBounds.top >= trackBounds.top &&
            thumbBounds.top + thumbBounds.height <= trackBounds.top + trackBounds.height,
          tone: sparkle.getAttribute("data-sparkle-tone"),
          track: trackBounds,
          trackBackground: getComputedStyle(track).backgroundColor,
          trackBackgroundImage: getComputedStyle(track).backgroundImage,
          trackBorderColor: getComputedStyle(track).borderTopColor,
          trackBorderWidth: Number.parseFloat(getComputedStyle(track).borderTopWidth),
          trackPadding: Number.parseFloat(getComputedStyle(track).paddingTop),
          titleClipped: titleWordBounds.some(
            // keep every word inside the single-line title box
            (word) => word.left < titleBounds.left - 1 || word.right > titleBounds.right + 1,
          ),
          titleFontSize: Number.parseFloat(titleStyle.fontSize),
          titleLines: new Set(titleWordBounds.map(
            // merge words sharing one rendered line
            (word) => Math.round(word.top),
          )).size,
        };
      },
    );

    // enforce the standalone switch geometry
    const assertSwitchGeometry = (snapshot) => {
      const inset = (snapshot.track.height - snapshot.thumb.height) / 2;
      const expectedSparkleSize = Math.min(18, snapshot.thumb.width * 0.7);
      assert.equal(Math.abs(snapshot.track.width - snapshot.track.height * 1.75) < 0.25, true, JSON.stringify(snapshot));
      assert.equal(Math.abs(inset - snapshot.trackPadding - snapshot.trackBorderWidth) < 0.25, true, JSON.stringify(snapshot));
      assert.equal(Math.abs(snapshot.thumb.width - snapshot.thumb.height) < 0.25, true, JSON.stringify(snapshot));
      assert.equal(Math.abs(snapshot.thumbTopInset - inset) < 0.25, true, JSON.stringify(snapshot));
      assert.equal(Math.abs(snapshot.sparkle.width - expectedSparkleSize) < 0.25, true, JSON.stringify(snapshot));
      assert.equal(Math.abs(snapshot.sparkle.height - expectedSparkleSize) < 0.25, true, JSON.stringify(snapshot));
      assert.equal(snapshot.sparkle.width <= 18, true);
      assert.equal(Math.abs(snapshot.button.width - Math.max(44, snapshot.track.width)) < 0.25, true, JSON.stringify(snapshot));
      assert.equal(Math.abs(snapshot.button.height - snapshot.track.height) < 0.25, true, JSON.stringify(snapshot));
      assert.equal(snapshot.buttonBackground, "rgba(0, 0, 0, 0)");
      assert.deepEqual(snapshot.buttonBorderWidths, ["0px", "0px", "0px", "0px"]);
      assert.equal(snapshot.buttonBoxShadow, "none");
      assert.equal(snapshot.sparkleCentered, true);
      assert.notEqual(snapshot.sparkleDisplay, "none");
      assert.equal(snapshot.sparkleHiddenByParent, true);
      assert.equal(snapshot.sparkleOpacity, "1");
      assert.equal(snapshot.sparkleTitleCount, 0);
      assert.equal(snapshot.thumbContained, true);
      assert.equal(snapshot.gradientElementCount, 0);
      assert.equal(snapshot.inkFill, "currentColor");
      assert.equal(snapshot.inkStroke, "currentColor");
      assert.equal(snapshot.titleClipped, false, JSON.stringify(snapshot));
      assert.equal(snapshot.titleLines, 1, JSON.stringify(snapshot));
    };
    const baselineHeaderHeights = new Map([
      [320, 61.96875],
      [412, 61.96875],
      [768, 65.984375],
      [1280, 74.390625],
    ]);

    // compare the responsive title, hit-area, and artwork contracts together
    for (const width of [320, 412, 768, 1280]) {
      await page.setViewportSize({ height: 900, width });
      await page.evaluate(
        // settle both title sizing and bundled fonts
        async () => await document.fonts.ready,
      );
      await page.waitForFunction(
        // await the fitted title and css-sized switch after each resize
        () => {
          const title = document.querySelector(".home-masthead h1");
          const titleText = title?.querySelector(".masthead-title-text");
          const button = document.querySelector("[data-forecast-adjustment-toggle]");
          const track = document.querySelector(".forecast-adjustment-toggle-track");
          return title instanceof HTMLElement && titleText instanceof HTMLElement &&
            button instanceof HTMLElement && track instanceof HTMLElement &&
            titleText.scrollWidth <= titleText.clientWidth + 1 &&
            Math.abs(button.getBoundingClientRect().height - track.getBoundingClientRect().height) < 0.25;
        },
      );
      const responsive = await captureSwitch();
      const baselineHeaderHeight = baselineHeaderHeights.get(width);

      // require one explicit baseline for every viewport
      if (baselineHeaderHeight === undefined) {
        throw new Error(`missing switch baseline for ${String(width)}px`);
      }

      assertSwitchGeometry(responsive);
      assert.equal(responsive.ariaChecked, "true");
      assert.equal(Math.abs(responsive.track.height - 47.6) < 0.25, true, JSON.stringify(responsive));
      assert.equal(Math.abs(responsive.masthead.height - baselineHeaderHeight) < 1, true, JSON.stringify(responsive));
      assert.equal(Math.abs(responsive.thumbRightInset - (responsive.track.height - responsive.thumb.height) / 2) < 0.25, true);
    }

    await page.setViewportSize({ height: 900, width: 320 });
    const scaledTrackHeights = [];
    // prove reduced and enlarged browser text scale the shared control height
    for (const rootSize of [12, 20]) {
      await page.evaluate((size) => {
        document.documentElement.style.fontSize = `${String(size)}px`;
      }, rootSize);
      await page.waitForFunction(
        // await the fitted title and scaled switch geometry
        () => {
          const title = document.querySelector(".home-masthead h1");
          const titleText = title?.querySelector(".masthead-title-text");
          const button = document.querySelector("[data-forecast-adjustment-toggle]");
          const track = document.querySelector(".forecast-adjustment-toggle-track");
          return title instanceof HTMLElement && titleText instanceof HTMLElement &&
            button instanceof HTMLElement && track instanceof HTMLElement &&
            titleText.scrollWidth <= titleText.clientWidth + 1 &&
            Math.abs(button.getBoundingClientRect().height - track.getBoundingClientRect().height) < 0.25;
        },
      );
      const scaled = await captureSwitch();
      assertSwitchGeometry(scaled);
      scaledTrackHeights.push(scaled.track.height);
    }
    assert.equal(Math.abs((scaledTrackHeights[0] ?? 0) - 36.2) < 0.25, true, JSON.stringify(scaledTrackHeights));
    assert.equal(Math.abs((scaledTrackHeights[1] ?? 0) - 59) < 0.25, true, JSON.stringify(scaledTrackHeights));
    await page.evaluate(
      // restore the default browser text scale before interaction checks
      () => document.documentElement.style.removeProperty("font-size"),
    );
    await page.waitForFunction(
      // await the restored switch geometry
      () => {
        const button = document.querySelector("[data-forecast-adjustment-toggle]");
        const track = document.querySelector(".forecast-adjustment-toggle-track");
        return button instanceof HTMLElement && track instanceof HTMLElement &&
          Math.abs(button.getBoundingClientRect().height - track.getBoundingClientRect().height) < 0.25 &&
          Math.abs(track.getBoundingClientRect().height - 47.6) < 0.25;
      },
    );

    const enabled = await captureSwitch();
    assertSwitchGeometry(enabled);
    assert.equal(enabled.ariaChecked, "true");
    assert.equal(enabled.ariaLabel, "Adjusted");
    assert.equal(enabled.text, "");
    assert.equal(enabled.tone, "gold");
    assert.equal(enabled.inkRenderedFill, "rgb(197, 138, 16)");
    assert.equal(enabled.inkRenderedStroke, "rgb(197, 138, 16)");
    assert.equal(enabled.sparkleColor, "rgb(197, 138, 16)");
    assert.match(
      enabled.trackBackgroundImage,
      /^linear-gradient\(135deg, rgb\(189, 145, 48\) 0%, rgb\(217, 181, 85\) 35%, rgb\(230, 205, 137\) 50%, rgb\(212, 172, 69\) 70%, rgb\(189, 145, 48\) 100%\)$/u,
    );
    assert.doesNotMatch(enabled.trackBackgroundImage, /rgb\(255, 240, 168\)/u);

    await toggle.click();
    await page.waitForFunction(
      // await the raw-state redraw
      () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "false",
    );
    await page.waitForTimeout(200);
    const disabled = await captureSwitch();
    assertSwitchGeometry(disabled);
    assert.equal(disabled.ariaChecked, "false");
    assert.equal(disabled.ariaLabel, "Adjusted");
    assert.equal(disabled.text, "");
    assert.equal(disabled.track.width, enabled.track.width);
    assert.equal(disabled.track.height, enabled.track.height);
    assert.equal(disabled.thumb.width, enabled.thumb.width);
    assert.equal(disabled.thumb.height, enabled.thumb.height);
    assert.equal(disabled.sparkle.width, enabled.sparkle.width);
    assert.equal(disabled.sparkle.height, enabled.sparkle.height);
    assert.equal(disabled.tone, "gray");
    assert.equal(disabled.inkRenderedFill, "rgb(119, 116, 124)");
    assert.equal(disabled.inkRenderedStroke, "rgb(119, 116, 124)");
    assert.equal(disabled.sparkleColor, "rgb(119, 116, 124)");
    assert.equal(Math.abs(disabled.thumb.left - disabled.track.left - (disabled.track.height - disabled.thumb.height) / 2) < 0.25, true);
    assert.equal(Math.abs(enabled.thumb.left - disabled.thumb.left - (enabled.track.width - enabled.track.height)) < 0.5, true);
    assert.equal(disabled.trackBackgroundImage, "none");
    assert.notEqual(enabled.trackBackground, disabled.trackBackground);
    assert.equal(enabled.trackBorderColor, disabled.trackBorderColor);

    await toggle.hover();
    const hovered = await captureSwitch();
    assert.equal(hovered.buttonBackground, "rgba(0, 0, 0, 0)");
    assert.deepEqual(hovered.buttonBorderWidths, ["0px", "0px", "0px", "0px"]);
    assert.equal(hovered.buttonBoxShadow, "none");
    await page.evaluate(
      // establish the switch as the origin for keyboard traversal
      () => document.querySelector("[data-forecast-adjustment-toggle]")?.focus(),
    );
    await page.keyboard.press("Tab");
    await page.keyboard.press("Shift+Tab");
    const focused = await captureSwitch();
    assert.equal(focused.buttonFocused, true);
    assert.equal(focused.buttonBackground, "rgba(0, 0, 0, 0)");
    assert.deepEqual(focused.buttonBorderWidths, ["0px", "0px", "0px", "0px"]);
    assert.equal(focused.buttonBoxShadow, "none");
    assert.equal(focused.buttonOutlineStyle, "solid");
    assert.equal(Number.parseFloat(focused.buttonOutlineWidth) >= 3, true);
    await toggle.press("Space");
    await page.waitForFunction(
      // await the keyboard-triggered adjusted redraw
      () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "true",
    );
    await page.waitForTimeout(200);
    const keyboardEnabled = await captureSwitch();
    assertSwitchGeometry(keyboardEnabled);
    assert.equal(keyboardEnabled.buttonFocused, true);
    assert.equal(keyboardEnabled.tone, "gold");
    assert.equal(keyboardEnabled.inkFill, "currentColor");
    assert.equal(keyboardEnabled.inkStroke, "currentColor");
    assert.equal(keyboardEnabled.sparkleColor, "rgb(197, 138, 16)");
    assert.equal(keyboardEnabled.trackBackgroundImage, enabled.trackBackgroundImage);
    assert.equal(Math.abs(keyboardEnabled.thumb.left - disabled.thumb.left - (keyboardEnabled.track.width - keyboardEnabled.track.height)) < 0.5, true);

    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await page.getByRole("switch", { name: "Adjusted", exact: true }).getAttribute("aria-checked"), "true");
  } finally {
    // close disposable fixture resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// keep selected forecast watches readable across desktop and mobile layouts
test("adjustment switch rerenders high-contrast wind and frost watches", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();

    // verify both representative responsive layouts
    for (const viewport of [
      { height: 900, width: 1440 },
      { height: 844, width: 390 },
    ]) {
      const windForecast = {
        ...forecast[0],
        metrics: { ...forecast[0].metrics, windGustMps: 15 },
      };
      fixture.state.adjustmentMode = "canary";
      fixture.state.forecastRecords = [windForecast];
      const windPage = await createFixturePage(browser, {
        timezoneId: "America/Los_Angeles",
        viewport,
      });
      await windPage.goto(fixture.origin, { waitUntil: "networkidle" });
      const windAlert = windPage.locator(".local-alert.danger", { hasText: "High wind" });
      await windAlert.waitFor();
      assert.equal(await windAlert.textContent(), "High windGusts reaching 35.3 mph");
      const windContrast = await captureAlertContrast(windPage, "High wind");
      assert.deepEqual(
        {
          backgroundColor: windContrast.backgroundColor,
          detailColor: windContrast.detailColor,
          headingColor: windContrast.headingColor,
        },
        {
          backgroundColor: "rgb(127, 29, 29)",
          detailColor: "rgb(255, 255, 255)",
          headingColor: "rgb(255, 255, 255)",
        },
      );
      assert.equal(windContrast.headingContrast >= 7, true, JSON.stringify(windContrast));
      assert.equal(windContrast.detailContrast >= 7, true, JSON.stringify(windContrast));
      assert.equal((await windPage.screenshot()).byteLength > 1_000, true);
      await windPage.getByRole("switch", { name: "Adjusted", exact: true }).click();
      await windPage.waitForFunction(
        // await the raw forecast watch redraw
        () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "false" &&
          document.querySelector(".alert-list") === null,
      );
      assert.equal(await windPage.getByText("High wind", { exact: true }).count(), 0);
      await windPage.close();

      const frostForecast = {
        ...forecast[0],
        metrics: { ...forecast[0].metrics, temperatureC: -1, windGustMps: 4 },
      };
      fixture.state.adjustmentMode = "active";
      fixture.state.forecastRecords = [frostForecast];
      const frostPage = await createFixturePage(browser, {
        timezoneId: "America/Los_Angeles",
        viewport,
      });
      await frostPage.goto(fixture.origin, { waitUntil: "networkidle" });
      assert.equal(await frostPage.getByText("Frost possible", { exact: true }).count(), 0);
      await frostPage.getByRole("switch", { name: "Adjusted", exact: true }).click();
      const frostAlert = frostPage.locator(".local-alert.caution", { hasText: "Frost possible" });
      await frostAlert.waitFor();
      assert.equal(await frostAlert.textContent(), "Frost possibleForecast low 30.2°F");
      const frostContrast = await captureAlertContrast(frostPage, "Frost possible");
      assert.deepEqual(
        {
          backgroundColor: frostContrast.backgroundColor,
          detailColor: frostContrast.detailColor,
          headingColor: frostContrast.headingColor,
        },
        {
          backgroundColor: "rgb(74, 53, 0)",
          detailColor: "rgb(255, 255, 255)",
          headingColor: "rgb(255, 255, 255)",
        },
      );
      assert.equal(frostContrast.headingContrast >= 7, true, JSON.stringify(frostContrast));
      assert.equal(frostContrast.detailContrast >= 7, true, JSON.stringify(frostContrast));
      assert.equal((await frostPage.screenshot()).byteLength > 1_000, true);
      await frostPage.close();
    }
  } finally {
    // close disposable fixture resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// expose selected active adjustment families without changing their readings
test("adjusted temperature wind and rain icons turn gold on both forecast-bearing routes", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  const gold = "rgb(197, 138, 16)";
  const normal = "rgb(33, 26, 31)";
  // build one valid independently configurable browser payload
  const activePayload = (settings) => {
    const payload = forecastResponse("active", settings, forecast.slice(0, 24));
    const response = {
      ...payload,
      data: payload.data.map(
        // attach active rain evidence to every displayed member
        (record) => ({ ...record, rainAdjustment: activeRainAdjustment(record) }),
      ),
      rainAdjustmentRuntime: activeRainRuntime(),
    };
    const parsed = parseForecastRecordsResponse(response);
    assert.equal(parsed.rainAdjustmentRuntime.state, "active");
    assert.equal(
      parsed.data.every(
        // retain rain decisions only when independently enabled
        (record) => (record.rainAdjustment?.state === "active") === settings.rain,
      ),
      true,
    );
    return response;
  };

  try {
    browser = await launchBrowser();

    // verify desktop and mobile computed colors
    for (const viewport of [
      { height: 900, width: 960 },
      { height: 844, width: 390 },
    ]) {
      fixture.state.forecastPayload = activePayload({
        version: 1,
        temperature: true,
        wind: true,
        rain: true,
      });
      const page = await createFixturePage(browser, { viewport });
      // align historical fixture forecasts with their original farm day
      await page.clock.setFixedTime(new Date("2026-08-22T05:37:00.000Z"));
      await page.goto(fixture.origin, { waitUntil: "networkidle" });
      const adjustedHome = await captureFamilyIcons(page, "home");
      assert.deepEqual(
        Object.fromEntries(Object.entries(adjustedHome).map(([family, icon]) => [family, icon.color])),
        { rain: gold, temperature: gold, wind: gold },
      );
      assert.equal(
        Object.values(adjustedHome).every(
          // require the explicit adjusted marker class
          (icon) => icon.className === "material-symbols-rounded forecast-adjusted-icon",
        ),
        true,
      );
      assert.deepEqual(
        await page.locator("[data-condition='humidity'] .condition-label > .material-symbols-rounded").evaluate(
          // expose humidity through the global adjustment preference
          (element) => ({ className: element.getAttribute("class"), color: getComputedStyle(element).color }),
        ),
        { className: "material-symbols-rounded forecast-adjusted-icon", color: gold },
      );

      await page.getByRole("link", { name: "Forecast" }).click();
      await page.locator("[data-forecast-charts]").waitFor();
      const adjustedForecast = await captureFamilyIcons(page, "forecast");
      assert.deepEqual(
        Object.fromEntries(Object.entries(adjustedForecast).map(([family, icon]) => [family, icon.color])),
        { rain: gold, temperature: gold, wind: gold },
      );
      await page.getByRole("switch", { name: "Adjusted", exact: true }).click();
      await page.waitForFunction(
        // await raw icon rerendering
        () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "false",
      );
      const regional = await captureFamilyIcons(page, "forecast");
      assert.deepEqual(
        Object.fromEntries(Object.entries(regional).map(([family, icon]) => [family, icon.color])),
        { rain: normal, temperature: normal, wind: normal },
      );
      assert.equal(
        Object.values(regional).every(
          // remove the marker in raw mode
          (icon) => icon.className === "material-symbols-rounded",
        ),
        true,
      );
      await page.getByRole("switch", { name: "Adjusted", exact: true }).click();
      await page.waitForFunction(
        // await adjusted icon rerendering
        () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "true",
      );
      assert.deepEqual(
        Object.fromEntries(Object.entries(await captureFamilyIcons(page, "forecast")).map(([family, icon]) => [family, icon.color])),
        { rain: gold, temperature: gold, wind: gold },
      );

      // disable each family without muting its independent neighbors
      for (const disabled of ["temperature", "wind", "rain"]) {
        fixture.state.forecastPayload = activePayload({
          version: 1,
          temperature: disabled !== "temperature",
          wind: disabled !== "wind",
          rain: disabled !== "rain",
        });
        await page.reload({ waitUntil: "networkidle" });
        const icons = await captureFamilyIcons(page, "forecast");

        // compare every family with its persisted setting
        for (const [family, icon] of Object.entries(icons)) {
          assert.equal(icon.color, family === disabled ? normal : gold, `${disabled} disables ${family}`);
        }
      }

      fixture.state.forecastPayload = forecastResponse(
        "fault",
        { version: 1, temperature: true, wind: true, rain: true },
        forecast.slice(0, 24),
      );
      await page.reload({ waitUntil: "networkidle" });
      assert.deepEqual(
        Object.fromEntries(Object.entries(await captureFamilyIcons(page, "forecast")).map(([family, icon]) => [family, icon.color])),
        { rain: normal, temperature: normal, wind: normal },
      );
      await page.close();
    }
  } finally {
    // close disposable fixture resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// expose four context families whenever the global adjustment preference is on
test("adjustment mode colors humidity air quality pressure and UV icons without changing data", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  const gold = "rgb(197, 138, 16)";
  const normal = "rgb(33, 26, 31)";
  const evidenceCases = [
    {
      label: "missing evidence",
      payload: { data: forecast.slice(0, 24), site },
    },
    {
      label: "faulted evidence",
      payload: forecastResponse(
        "fault",
        { version: 1, temperature: true, wind: true, rain: true },
        forecast.slice(0, 24),
      ),
    },
    {
      label: "disabled families",
      payload: forecastResponse(
        "active",
        { version: 1, temperature: false, wind: false, rain: false },
        forecast.slice(0, 24),
      ),
    },
  ];
  // require one complete global-mode icon state
  const assertGlobalIcons = async (page, view, color, marked, label) => {
    const icons = await captureGlobalModeIcons(page, view);
    assert.deepEqual(
      Object.fromEntries(Object.entries(icons).map(
        // compare each icon's final browser presentation
        ([family, icon]) => [family, {
          color: icon.color,
          marked: icon.className?.split(" ").includes("forecast-adjusted-icon") ?? false,
        }],
      )),
      Object.fromEntries(Object.keys(icons).map(
        // apply the global expectation equally across all four families
        (family) => [family, { color, marked }],
      )),
      label,
    );
  };
  // require tide and sunset to remain outside the global icon mode
  const assertStableIcons = async (page, view, label) => {
    const selectors = view === "home"
      ? [
        "[data-condition='tide'] .condition-label > .material-symbols-rounded",
        "[data-condition='sunset'] .condition-label > .material-symbols-rounded",
      ]
      : ["[data-forecast-chart='tide'] .forecast-chart-heading .material-symbols-rounded"];
    assert.deepEqual(
      await Promise.all(selectors.map(
        // inspect each excluded family
        async (selector) => await page.locator(selector).evaluate(
          // retain its normal class and color
          (element) => ({ className: element.getAttribute("class"), color: getComputedStyle(element).color }),
        ),
      )),
      selectors.map(
        // require unchanged decoration for every excluded family
        () => ({ className: "material-symbols-rounded", color: normal }),
      ),
      label,
    );
  };

  try {
    browser = await launchBrowser();
    fixture.state.currentRecords = [current];

    // verify both responsive layouts against every evidence state
    for (const viewport of [
      { height: 900, width: 960 },
      { height: 844, width: 390 },
    ]) {
      const page = await createFixturePage(browser, { viewport });
      // align historical fixture forecasts with their original farm day
      await page.clock.setFixedTime(new Date("2026-08-22T05:37:00.000Z"));

      // separate global preference from correction evidence
      for (const evidenceCase of evidenceCases) {
        fixture.state.forecastPayload = evidenceCase.payload;
        await page.goto(fixture.origin, { waitUntil: "networkidle" });
        const toggle = page.getByRole("switch", { name: "Adjusted", exact: true });
        assert.equal(await toggle.getAttribute("aria-checked"), "true", evidenceCase.label);
        const adjustedHomeData = await captureGlobalModeData(page, "home");
        await assertGlobalIcons(page, "home", gold, true, `${evidenceCase.label} home on`);
        await assertStableIcons(page, "home", `${evidenceCase.label} home exclusions`);

        await page.getByRole("link", { name: "Forecast", exact: true }).click();
        await page.locator("[data-forecast-charts]").waitFor();
        const adjustedForecastData = await captureGlobalModeData(page, "forecast");
        await assertGlobalIcons(page, "forecast", gold, true, `${evidenceCase.label} forecast on`);
        await assertStableIcons(page, "forecast", `${evidenceCase.label} forecast exclusions`);
        assert.deepEqual(
          Object.fromEntries(Object.entries(await captureFamilyIcons(page, "forecast")).map(
            // keep evidence-governed families normal without active corrections
            ([family, icon]) => [family, icon.color],
          )),
          { rain: normal, temperature: normal, wind: normal },
          evidenceCase.label,
        );
        assert.deepEqual(
          await page.locator("[data-forecast-chart='clouds'] .forecast-chart-heading .material-symbols-rounded").evaluate(
            // keep cloud evidence behavior independent
            (element) => ({ className: element.getAttribute("class"), color: getComputedStyle(element).color }),
          ),
          { className: "material-symbols-rounded", color: normal },
          evidenceCase.label,
        );

        await toggle.click();
        await page.waitForFunction(
          // await the complete raw icon redraw
          () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "false",
        );
        await assertGlobalIcons(page, "forecast", normal, false, `${evidenceCase.label} forecast off`);
        await assertStableIcons(page, "forecast", `${evidenceCase.label} raw forecast exclusions`);
        assert.deepEqual(await captureGlobalModeData(page, "forecast"), adjustedForecastData, evidenceCase.label);
        await toggle.click();
        await page.waitForFunction(
          // await restoration of the global icon treatment
          () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "true",
        );
        await assertGlobalIcons(page, "forecast", gold, true, `${evidenceCase.label} forecast restored`);
        assert.deepEqual(await captureGlobalModeData(page, "forecast"), adjustedForecastData, evidenceCase.label);

        await page.getByRole("link", { name: "Now", exact: true }).click();
        await page.locator(".weather-content[aria-busy='false']").waitFor();
        await assertGlobalIcons(page, "home", gold, true, `${evidenceCase.label} home restored`);
        assert.deepEqual(await captureGlobalModeData(page, "home"), adjustedHomeData, evidenceCase.label);
        await toggle.click();
        await page.waitForFunction(
          // await the homepage raw icon redraw
          () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "false",
        );
        await assertGlobalIcons(page, "home", normal, false, `${evidenceCase.label} home off`);
        await assertStableIcons(page, "home", `${evidenceCase.label} raw home exclusions`);
        assert.deepEqual(await captureGlobalModeData(page, "home"), adjustedHomeData, evidenceCase.label);
        await toggle.click();
        await page.waitForFunction(
          // await the next evidence case in adjusted mode
          () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "true",
        );
        await assertGlobalIcons(page, "home", gold, true, `${evidenceCase.label} home rerestored`);
      }
      await page.close();
    }
  } finally {
    // close disposable fixture resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// average fresh nearby stations only when the farm's adjusted current reading is unavailable
test("adjusted current conditions fall back per metric to a deduplicated local station average", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  const emptyMetrics = Object.fromEntries(Object.keys(physicalCurrent.metrics).map(
    // start every physical fixture without inherited readings
    (metric) => [metric, null],
  ));
  // create one isolated physical-station current reading
  const localRecord = ({
    freshnessStatus = "fresh",
    id,
    metrics,
    providerKey = "weatherflow-tempest",
    sourceKey,
    stationSlug,
    validAt,
  }) => ({
    ...physicalCurrent,
    freshness: {
      ageSeconds: freshnessStatus === "stale" ? 90_000 : freshnessStatus === "delayed" ? 3_600 : 120,
      label: freshnessStatus === "stale"
        ? "Station reading may be stale"
        : freshnessStatus === "delayed"
          ? "Station reading is delayed"
          : "Station reading is current",
      status: freshnessStatus,
    },
    id,
    metrics: { ...emptyMetrics, ...metrics },
    pressureChange3hHpa: null,
    provenance: {
      ...physicalCurrent.provenance,
      providerKey,
      sourceId: id,
      sourceKey,
      sourceKind: "physical_sensor",
      stationSlug,
    },
    receivedAt: validAt,
    validAt,
  });
  const regional = {
    ...current,
    metrics: {
      ...current.metrics,
      apparentTemperatureC: 10,
      cloudCoverPercent: 90,
      pm25MicrogramsPerCubicMeter: 5,
      precipitationRateMmPerHour: 0,
      relativeHumidityPercent: 70,
      temperatureC: 11,
      uvIndex: 1,
      wetBulbGlobeTemperatureC: 9,
      windDirectionDegrees: 225,
      windGustMps: 2,
      windSpeedMps: 1,
    },
  };
  const staleFarm = localRecord({
    freshnessStatus: "stale",
    id: "stale-farm",
    metrics: {
      apparentTemperatureC: 40,
      pm25MicrogramsPerCubicMeter: 80,
      precipitationRateMmPerHour: 20,
      temperatureC: 40,
      windDirectionDegrees: 180,
      windGustMps: 25,
      windSpeedMps: 20,
    },
    providerKey: "ecowitt-local",
    sourceKey: "ecowitt-local-live-v1",
    stationSlug: "ballydidean-ecowitt",
    validAt: "2026-08-21T04:55:00.000Z",
  });
  const olderNeighborA = localRecord({
    id: "neighbor-a-older",
    metrics: {
      apparentTemperatureC: 30,
      pm25MicrogramsPerCubicMeter: 90,
      precipitationRateMmPerHour: 12,
      relativeHumidityPercent: 30,
      temperatureC: 30,
      uvIndex: 9,
      wetBulbGlobeTemperatureC: 10,
      windDirectionDegrees: 270,
      windGustMps: 30,
      windSpeedMps: 20,
    },
    sourceKey: "neighbor-a-observations-v1",
    stationSlug: "neighbor-a",
    validAt: "2026-08-22T04:50:00.000Z",
  });
  const newerNeighborA = localRecord({
    id: "neighbor-a-newer",
    metrics: {
      apparentTemperatureC: null,
      pm25MicrogramsPerCubicMeter: 50,
      precipitationRateMmPerHour: 4,
      relativeHumidityPercent: 40,
      temperatureC: null,
      uvIndex: 10,
      wetBulbGlobeTemperatureC: null,
      windDirectionDegrees: 350,
      windGustMps: 20,
      windSpeedMps: 12,
    },
    providerKey: "ecowitt-cloud",
    sourceKey: "neighbor-a-cloud-v1",
    stationSlug: "neighbor-a",
    validAt: "2026-08-22T04:55:00.000Z",
  });
  const neighborB = localRecord({
    freshnessStatus: "delayed",
    id: "neighbor-b",
    metrics: {
      apparentTemperatureC: 10,
      pm25MicrogramsPerCubicMeter: 0,
      precipitationRateMmPerHour: 0,
      relativeHumidityPercent: 60,
      temperatureC: 10,
      uvIndex: 0,
      wetBulbGlobeTemperatureC: 10,
      windDirectionDegrees: 10,
      windGustMps: 0,
      windSpeedMps: 0,
    },
    sourceKey: "neighbor-b-observations-v1",
    stationSlug: "neighbor-b",
    validAt: "2026-08-22T04:54:00.000Z",
  });
  const staleNeighbor = localRecord({
    freshnessStatus: "stale",
    id: "stale-neighbor",
    metrics: {
      apparentTemperatureC: 50,
      pm25MicrogramsPerCubicMeter: 100,
      precipitationRateMmPerHour: 30,
      temperatureC: 50,
      windDirectionDegrees: 180,
      windGustMps: 30,
      windSpeedMps: 30,
    },
    sourceKey: "stale-neighbor-observations-v1",
    stationSlug: "stale-neighbor",
    validAt: "2026-08-21T04:54:00.000Z",
  });
  const wrongKindNeighbor = {
    ...neighborB,
    id: "neighbor-reanalysis",
    provenance: {
      ...neighborB.provenance,
      sourceId: "neighbor-reanalysis",
      sourceKey: "neighbor-reanalysis-v1",
      sourceKind: "reanalysis",
      stationSlug: "reanalysis-neighbor",
    },
  };
  const forecastNeighbor = {
    ...neighborB,
    id: "neighbor-forecast",
    provenance: {
      ...neighborB.provenance,
      sourceId: "neighbor-forecast",
      sourceKey: "neighbor-forecast-v1",
      sourceKind: "forecast",
      stationSlug: "forecast-neighbor",
    },
  };

  try {
    browser = await launchBrowser();
    fixture.state.adjustmentMode = "active";
    fixture.state.currentRecords = [
      regional,
      olderNeighborA,
      newerNeighborA,
      neighborB,
      staleNeighbor,
      wrongKindNeighbor,
      forecastNeighbor,
    ];
    const pageErrors = [];
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 960 },
    });
    // capture unexpected render errors across every fallback state
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.clock.setFixedTime(new Date("2026-08-22T05:37:00.000Z"));
    await page.goto(fixture.origin, { waitUntil: "networkidle" });

    const temperature = page.locator("[data-condition='temperature']");
    const wind = page.locator("[data-condition='wind']");
    const rain = page.locator("[data-condition='rain']");
    const clouds = page.locator("[data-condition='clouds']");
    assert.match(await temperature.locator(".condition-primary").textContent() ?? "", /68\s*°F/u);
    assert.match(await temperature.locator(".condition-secondary").textContent() ?? "", /Air Temp\s*68\s*°F/u);
    assert.match(await wind.locator(".condition-primary").textContent() ?? "", /13\s*mph N/u);
    assert.match(await wind.locator(".condition-secondary").textContent() ?? "", /Gusts\s*22\s*mph/u);
    assert.match(await rain.locator(".condition-primary").textContent() ?? "", /^0\.08\s*in\/h$/u);
    assert.match(await rain.locator(".condition-secondary").textContent() ?? "", /Accumulation\s*0\.1\s*in/u);
    assert.equal(await page.locator("[data-condition='humidity'] .condition-primary").innerText(), "50%");
    assert.equal(await page.locator("[data-condition='air-quality'] .condition-primary").innerText(), "25");
    assert.equal(await page.locator("[data-condition='uv-index'] .condition-primary").innerText(), "5");
    assert.equal(await clouds.locator(".condition-primary").innerText(), "90%");
    assert.equal(await page.locator(".local-alert", { hasText: "High wind" }).count(), 0);
    assert.equal(await page.locator(".local-alert", { hasText: "Heavy rain" }).count(), 0);
    assert.equal(await page.locator(".local-alert", { hasText: "Air quality" }).count(), 0);
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /07-light-rain\.svg$/u);

    const toggle = page.getByRole("switch", { name: "Adjusted", exact: true });
    await toggle.click();
    await page.waitForFunction(
      // await the regional-only redraw
      () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "false",
    );
    assert.match(await temperature.locator(".condition-primary").textContent() ?? "", /50\s*°F/u);
    assert.match(await wind.locator(".condition-primary").textContent() ?? "", /2\s*mph SW/u);
    assert.match(await rain.locator(".condition-primary").textContent() ?? "", /^0\s*in\/h$/u);
    assert.equal(await page.locator("[data-condition='air-quality'] .condition-primary").innerText(), "5");
    assert.equal(await clouds.locator(".condition-primary").innerText(), "90%");
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /05-cloudy\.svg$/u);
    await toggle.click();
    await page.waitForFunction(
      // await restoration of the station-average reading
      () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "true" &&
        document.querySelector("[data-condition='air-quality'] .condition-primary")?.textContent?.trim() === "25",
    );
    assert.match(await temperature.locator(".condition-primary").textContent() ?? "", /68\s*°F/u);
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /07-light-rain\.svg$/u);

    await page.setViewportSize({ height: 844, width: 390 });
    assert.match(await wind.locator(".condition-primary").textContent() ?? "", /13\s*mph N/u);
    assert.equal(
      await page.evaluate(
        // reject mobile page overflow after the fallback redraw
        () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
      ),
      true,
    );
    await page.getByRole("link", { name: "Forecast", exact: true }).click();
    await page.getByRole("link", { name: "Now", exact: true }).click();
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /07-light-rain\.svg$/u);

    const partialFarm = localRecord({
      id: "partial-farm",
      metrics: {
        apparentTemperatureC: 21,
        pm25MicrogramsPerCubicMeter: null,
        precipitationRateMmPerHour: null,
        relativeHumidityPercent: 55,
        temperatureC: 21,
        uvIndex: null,
        wetBulbGlobeTemperatureC: 12,
        windDirectionDegrees: null,
        windGustMps: 5,
        windSpeedMps: -1,
      },
      providerKey: "ecowitt-local",
      sourceKey: "ecowitt-local-live-v1",
      stationSlug: "ballydidean-ecowitt",
      validAt: "2026-08-22T04:56:00.000Z",
    });
    fixture.state.currentRecords = [regional, partialFarm, newerNeighborA, olderNeighborA, neighborB];
    await page.reload({ waitUntil: "networkidle" });
    assert.match(await temperature.locator(".condition-primary").textContent() ?? "", /70\s*°F/u);
    assert.match(await wind.locator(".condition-primary").textContent() ?? "", /13\s*mph N/u);
    assert.match(await rain.locator(".condition-primary").textContent() ?? "", /^0\.08\s*in\/h$/u);
    assert.equal(await page.locator("[data-condition='humidity'] .condition-primary").innerText(), "55%");
    assert.equal(await page.locator("[data-condition='air-quality'] .condition-primary").innerText(), "25");
    assert.equal(await clouds.locator(".condition-primary").innerText(), "90%");

    const opposedNeighborA = localRecord({
      id: "opposed-a",
      metrics: { windDirectionDegrees: 90, windGustMps: 6, windSpeedMps: 6 },
      sourceKey: "opposed-a-v1",
      stationSlug: "opposed-a",
      validAt: "2026-08-22T04:57:00.000Z",
    });
    const opposedNeighborB = localRecord({
      id: "opposed-b",
      metrics: { windDirectionDegrees: 270, windGustMps: 6, windSpeedMps: 6 },
      sourceKey: "opposed-b-v1",
      stationSlug: "opposed-b",
      validAt: "2026-08-22T04:57:00.000Z",
    });
    fixture.state.currentRecords = [regional, opposedNeighborA, opposedNeighborB];
    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await wind.locator(".condition-primary small").innerText(), "mph");

    const midnightNeighbor = localRecord({
      freshnessStatus: "delayed",
      id: "midnight-neighbor",
      metrics: { apparentTemperatureC: 10, temperatureC: 10 },
      sourceKey: "midnight-neighbor-v1",
      stationSlug: "midnight-neighbor",
      validAt: "2026-08-22T06:55:00.000Z",
    });
    fixture.state.currentRecords = [{
      ...regional,
      receivedAt: "2026-08-22T06:50:00.000Z",
      validAt: "2026-08-22T06:50:00.000Z",
    }, midnightNeighbor];
    fixture.state.forecastRecords = forecast.map(
      // separate the neighboring local days around the midnight outage
      (record) => {
        const currentFarmDay = record.validAt >= "2026-08-22T07:00:00.000Z" &&
          record.validAt < "2026-08-23T07:00:00.000Z";
        const temperatureC = currentFarmDay ? 20 : 0;
        return {
          ...record,
          metrics: {
            ...record.metrics,
            apparentTemperatureC: temperatureC,
            temperatureC,
          },
        };
      },
    );
    await page.clock.setFixedTime(new Date("2026-08-22T07:15:00.000Z"));
    await page.reload({ waitUntil: "networkidle" });
    assert.match(await temperature.locator(".condition-primary").textContent() ?? "", /50\s*°F/u);
    assert.match(
      await temperature.locator(".condition-forecast-readings").textContent() ?? "",
      /Max\s*68°F\s*Min\s*68°F\s*Max\s*72°F\s*Min\s*72°F/u,
    );

    fixture.state.currentRecords = [regional, staleFarm, staleNeighbor, wrongKindNeighbor, forecastNeighbor];
    await page.reload({ waitUntil: "networkidle" });
    assert.match(await temperature.locator(".condition-primary").textContent() ?? "", /50\s*°F/u);
    assert.match(await wind.locator(".condition-primary").textContent() ?? "", /2\s*mph SW/u);
    assert.match(await rain.locator(".condition-primary").textContent() ?? "", /^0\s*in\/h$/u);

    const unavailableMetrics = Object.fromEntries(Object.keys(regional.metrics).map(
      // remove every regional metric for the terminal empty state
      (metric) => [metric, null],
    ));
    fixture.state.currentRecords = [{ ...regional, metrics: unavailableMetrics }, staleFarm, staleNeighbor, wrongKindNeighbor, forecastNeighbor];
    fixture.state.forecastRecords = [];
    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await temperature.locator(".condition-status").innerText(), "Unavailable");
    assert.equal(await wind.locator(".condition-status").innerText(), "Unavailable");
    assert.equal(await rain.locator(".condition-status").innerText(), "Unavailable");
    assert.equal(await page.locator("[data-condition='air-quality'] .condition-status").innerText(), "Unavailable");
    assert.equal(await page.locator("[data-condition='uv-index'] .condition-status").innerText(), "Unavailable");
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /12-unavailable\.svg$/u);
    assert.deepEqual(pageErrors, []);
  } finally {
    // close disposable fixture resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// keep raw dashboard weather strictly model-sourced across rerenders and cache use
test("raw current conditions never borrow local station readings or artwork", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  const quietModel = {
    ...current,
    pressureChange3hHpa: -1,
    metrics: {
      ...current.metrics,
      apparentTemperatureC: 10,
      cloudCoverPercent: 90,
      pm25MicrogramsPerCubicMeter: 5,
      precipitationRateMmPerHour: 0,
      pressureHpa: 1_009,
      temperatureC: 11,
      windGustMps: 2,
      windSpeedMps: 1,
      wetBulbGlobeTemperatureC: 9,
    },
  };
  const hazardousLocal = {
    ...physicalCurrent,
    pressureChange3hHpa: 9,
    metrics: {
      ...physicalCurrent.metrics,
      apparentTemperatureC: 35,
      pm25MicrogramsPerCubicMeter: 50,
      precipitationRateMmPerHour: 8,
      pressureHpa: 1_024,
      temperatureC: 34,
      windGustMps: 20,
      windSpeedMps: 10,
      wetBulbGlobeTemperatureC: 30,
    },
    provenance: {
      ...physicalCurrent.provenance,
      label: "first-party physical station",
      providerKey: "ecowitt-local",
      sourceId: "14",
      sourceKey: "ecowitt-local-live-v1",
      stationSlug: "ballydidean-ecowitt",
    },
  };

  try {
    browser = await launchBrowser();
    fixture.state.currentRecords = [quietModel, hazardousLocal];
    fixture.state.viewerContext = { data: { homeNetwork: true } };
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 960 },
    });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    await page.locator("[data-indoor-house]").waitFor();
    assert.match(await page.locator("[data-condition='temperature'] .condition-primary").textContent() ?? "", /95°F/u);
    assert.match(await page.locator("[data-condition='wind'] .condition-primary").textContent() ?? "", /22mph/u);
    assert.match(await page.locator("[data-condition='rain'] .condition-primary").textContent() ?? "", /0\.31in\/h/u);
    assert.match(await page.locator("[data-condition='pressure'] .condition-primary").textContent() ?? "", /\+9\.0/u);
    assert.equal(await page.locator(".local-alert").count(), 4);
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /10-heavy-rain-wind\.svg$/u);
    assert.equal(await page.locator("[data-admin-soil-map]").count(), 1);

    const toggle = page.getByRole("switch", { name: "Adjusted", exact: true });
    await toggle.click();
    await page.waitForFunction(
      // await the complete model-only redraw
      () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "false" &&
        document.querySelector("[data-indoor-house]") === null,
    );
    assert.match(await page.locator("[data-condition='temperature'] .condition-primary").textContent() ?? "", /50°F/u);
    assert.match(await page.locator("[data-condition='wind'] .condition-primary").textContent() ?? "", /2mph/u);
    assert.match(await page.locator("[data-condition='rain'] .condition-primary").textContent() ?? "", /0in\/h/u);
    assert.match(await page.locator("[data-condition='rain'] .condition-secondary").textContent() ?? "", /Accumulation\s*—/u);
    assert.match(await page.locator("[data-condition='pressure'] .condition-primary").textContent() ?? "", /-1\.0/u);
    assert.equal(await page.locator(".local-alert").count(), 0);
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /05-cloudy\.svg$/u);
    assert.equal(await page.locator("[data-admin-soil-map]").count(), 0);

    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await toggle.getAttribute("aria-checked"), "false");
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /05-cloudy\.svg$/u);
    await page.getByRole("link", { name: "Forecast" }).click();
    await page.getByRole("link", { name: "Now" }).click();
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /05-cloudy\.svg$/u);

    fixture.state.failReads = true;
    await page.reload({ waitUntil: "networkidle" });
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /05-cloudy\.svg$/u);
    assert.doesNotMatch(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /10-heavy-rain-wind/u);
    fixture.state.failReads = false;
    await page.reload({ waitUntil: "networkidle" });
    await toggle.click();
    await page.locator("[data-indoor-house]").waitFor();
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /10-heavy-rain-wind\.svg$/u);

    const hazardousModel = {
      ...quietModel,
      metrics: {
        ...quietModel.metrics,
        apparentTemperatureC: 35,
        cloudCoverPercent: 5,
        pm25MicrogramsPerCubicMeter: 50,
        precipitationRateMmPerHour: 8,
        windGustMps: 20,
        windSpeedMps: 10,
        wetBulbGlobeTemperatureC: 30,
      },
    };
    const quietLocal = {
      ...hazardousLocal,
      metrics: {
        ...hazardousLocal.metrics,
        apparentTemperatureC: 12,
        pm25MicrogramsPerCubicMeter: 4,
        precipitationRateMmPerHour: 0,
        temperatureC: 13,
        windGustMps: 2,
        windSpeedMps: 1,
        wetBulbGlobeTemperatureC: 10,
      },
    };
    fixture.state.currentRecords = [hazardousModel, quietLocal];
    await page.reload({ waitUntil: "networkidle" });
    assert.match(await page.locator("[data-condition='temperature'] .condition-primary").textContent() ?? "", /54°F/u);
    assert.doesNotMatch(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /10-heavy-rain-wind/u);
    await toggle.click();
    assert.match(await page.locator("[data-condition='temperature'] .condition-primary").textContent() ?? "", /95°F/u);
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /10-heavy-rain-wind\.svg$/u);
    assert.equal(await page.locator(".local-alert").count(), 4);

    const emptyMetrics = Object.fromEntries(Object.keys(quietModel.metrics).map(
      // remove every model field without exposing local fallback
      (metric) => [metric, null],
    ));
    fixture.state.currentRecords = [{ ...quietModel, metrics: emptyMetrics }, hazardousLocal];
    await page.reload({ waitUntil: "networkidle" });
    assert.match(await page.locator("[data-condition='temperature'] .condition-primary").textContent() ?? "", /—/u);
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /12-unavailable\.svg$/u);
    fixture.state.currentRecords = [hazardousLocal];
    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await page.getByText("No current weather value is available yet.", { exact: true }).count(), 1);
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /12-unavailable\.svg$/u);
  } finally {
    // close disposable fixture resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// fill bounded regional current gaps from the exact raw forecast hour
test("raw current conditions fill regional rain air quality and UV from the matching forecast hour", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  const regionalCurrent = {
    ...current,
    metrics: {
      ...current.metrics,
      cloudCoverPercent: 5,
      pm25MicrogramsPerCubicMeter: null,
      precipitationRateMmPerHour: null,
      uvIndex: null,
      windSpeedMps: 1,
    },
  };
  const quietLocal = {
    ...physicalCurrent,
    metrics: {
      ...physicalCurrent.metrics,
      pm25MicrogramsPerCubicMeter: 2,
      precipitationRateMmPerHour: 0,
      uvIndex: 1,
      windSpeedMps: 1,
    },
  };
  const matchingForecast = {
    ...makeForecastRecord(22),
    freshness: {
      ageSeconds: 2_220,
      label: "Forecast value is delayed",
      status: "delayed",
    },
    id: "regional-current-hour",
    metrics: {
      ...makeForecastRecord(22).metrics,
      pm25MicrogramsPerCubicMeter: 50,
      precipitationMm: 8,
      precipitationRateMmPerHour: 8,
      uvIndex: 9,
    },
    productRunAt: "2026-08-22T05:20:00.000Z",
    receivedAt: "2026-08-22T05:21:00.000Z",
    validAt: "2026-08-22T05:00:00.000Z",
  };
  const forecastRecords = forecast.map(
    // replace only the wall-clock forecast hour
    (record) => record.validAt === matchingForecast.validAt ? matchingForecast : record,
  );
  const adjustedPayload = forecastResponse(
    "active",
    { version: 1, temperature: true, wind: true, rain: true },
    forecastRecords,
  );

  try {
    browser = await launchBrowser();
    fixture.state.adjustmentMode = "active";
    fixture.state.currentRecords = [regionalCurrent, quietLocal];
    fixture.state.forecastPayload = {
      ...adjustedPayload,
      data: adjustedPayload.data.map(
        // make the governed rain value visibly different from the raw regional rate
        (record) => ({
          ...record,
          rainAdjustment: {
            ...activeRainAdjustment(record),
            correctedPrecipitationMm: record.id === matchingForecast.id
              ? 0
              : record.metrics.precipitationMm,
          },
        }),
      ),
      rainAdjustmentRuntime: activeRainRuntime(),
    };
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 960 },
    });
    await page.clock.setFixedTime(new Date("2026-08-22T05:37:00.000Z"));
    await page.goto(fixture.origin, { waitUntil: "networkidle" });

    assert.match(await page.locator("[data-condition='rain'] .condition-primary").innerText(), /^0\s*in\/h$/u);
    assert.equal(await page.locator("[data-condition='air-quality'] .condition-primary").innerText(), "2");
    assert.equal(await page.locator("[data-condition='uv-index'] .condition-primary").innerText(), "1");
    assert.equal(await page.locator(".local-alert", { hasText: "Air quality" }).count(), 0);
    assert.equal(await page.locator(".local-alert", { hasText: "Heavy rain" }).count(), 0);
    assert.doesNotMatch(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /heavy-rain/u);

    const toggle = page.getByRole("switch", { name: "Adjusted", exact: true });
    await toggle.click();
    await page.waitForFunction(
      // await the regional-only redraw
      () => document.querySelector("[data-forecast-adjustment-toggle]")?.getAttribute("aria-checked") === "false",
    );
    assert.match(await page.locator("[data-condition='rain'] .condition-primary").innerText(), /^0\.31\s*in\/h$/u);
    assert.equal(await page.locator("[data-condition='air-quality'] .condition-primary").innerText(), "50");
    assert.equal(await page.locator("[data-condition='uv-index'] .condition-primary").innerText(), "9");
    assert.match(await page.locator(".local-alert", { hasText: "Heavy rain" }).innerText(), /Current rain 0\.31 in\/h/u);
    assert.match(await page.locator(".local-alert", { hasText: "Air quality" }).innerText(), /PM2\.5 is 50/u);
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /09-heavy-rain\.svg$/u);

    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await toggle.getAttribute("aria-checked"), "false");
    assert.equal(await page.locator("[data-condition='air-quality'] .condition-primary").innerText(), "50");
    await page.getByRole("link", { name: "Forecast", exact: true }).click();
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /09-heavy-rain\.svg$/u);
    await page.getByRole("link", { name: "Now", exact: true }).click();
    await page.waitForFunction(
      // await the route's current-condition refresh rather than its numeric skeleton
      () => document.querySelector("[data-condition='uv-index'] .condition-primary")?.textContent?.trim() === "9",
    );
    assert.equal(await page.locator("[data-condition='uv-index'] .condition-primary").innerText(), "9");

    fixture.state.currentRecords = [{
      ...regionalCurrent,
      metrics: {
        ...regionalCurrent.metrics,
        pm25MicrogramsPerCubicMeter: 6,
        precipitationRateMmPerHour: 1,
        uvIndex: 2,
      },
    }, quietLocal];
    await page.reload({ waitUntil: "networkidle" });
    assert.match(await page.locator("[data-condition='rain'] .condition-primary").innerText(), /^0\.04\s*in\/h$/u);
    assert.equal(await page.locator("[data-condition='air-quality'] .condition-primary").innerText(), "6");
    assert.equal(await page.locator("[data-condition='uv-index'] .condition-primary").innerText(), "2");
    assert.doesNotMatch(await page.locator(".local-alert", { hasText: "Heavy rain" }).innerText(), /Current rain/u);
    assert.equal(await page.locator(".local-alert", { hasText: "Air quality" }).count(), 0);
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /07-light-rain\.svg$/u);
  } finally {
    // close disposable fixture resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// reject every forecast value outside the regional current fallback contract
test("raw current forecast fallback rejects wrong stale missing and invalid hours", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  const regionalCurrent = {
    ...current,
    metrics: {
      ...current.metrics,
      pm25MicrogramsPerCubicMeter: null,
      precipitationRateMmPerHour: null,
      uvIndex: null,
    },
  };
  const hazardousLocal = {
    ...physicalCurrent,
    metrics: {
      ...physicalCurrent.metrics,
      pm25MicrogramsPerCubicMeter: 75,
      precipitationRateMmPerHour: 12,
      uvIndex: 11,
    },
  };
  const exactForecast = {
    ...makeForecastRecord(22),
    freshness: {
      ageSeconds: 600,
      label: "Forecast value is current",
      status: "fresh",
    },
    id: "exact-regional-hour",
    metrics: {
      ...makeForecastRecord(22).metrics,
      pm25MicrogramsPerCubicMeter: 0,
      precipitationRateMmPerHour: 0,
      uvIndex: 0,
    },
    productRunAt: "2026-08-22T05:20:00.000Z",
    receivedAt: "2026-08-22T05:21:00.000Z",
    validAt: "2026-08-22T05:00:00.000Z",
  };
  const unavailableCases = [
    {
      label: "wrong hour",
      records: [{ ...exactForecast, id: "wrong-hour", validAt: "2026-08-22T04:00:00.000Z" }],
    },
    {
      label: "stale hour",
      records: [{ ...exactForecast, productRunAt: "2026-08-22T02:00:00.000Z", receivedAt: "2026-08-22T05:21:00.000Z", id: "stale-hour" }],
    },
    {
      label: "future run",
      records: [{ ...exactForecast, productRunAt: "2026-08-22T06:00:00.000Z", id: "future-run" }],
    },
    {
      label: "wrong source identity",
      records: [
        { ...exactForecast, id: "wrong-provider", provenance: { ...exactForecast.provenance, providerKey: "other-model" } },
        { ...exactForecast, id: "wrong-station", provenance: { ...exactForecast.provenance, stationSlug: "other-grid" } },
        { ...exactForecast, id: "wrong-kind", provenance: { ...exactForecast.provenance, sourceKind: "model_current" } },
      ],
    },
    {
      label: "invalid metrics",
      records: [{
        ...exactForecast,
        id: "negative-values",
        metrics: {
          ...exactForecast.metrics,
          pm25MicrogramsPerCubicMeter: -1,
          precipitationRateMmPerHour: -1,
          uvIndex: -1,
        },
      }],
    },
    { label: "missing hour", records: [] },
  ];

  try {
    browser = await launchBrowser();
    fixture.state.adjustmentMode = "active";
    fixture.state.currentRecords = [regionalCurrent, hazardousLocal];
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 960 },
    });
    await page.clock.setFixedTime(new Date("2026-08-22T05:37:00.000Z"));
    await page.addInitScript(
      // begin with the explicit regional-only preference
      (key) => localStorage.setItem(key, "raw"),
      FORECAST_ADJUSTMENT_MODE_STORAGE_KEY,
    );

    // require every excluded candidate to leave all three cards unavailable
    for (const unavailableCase of unavailableCases) {
      fixture.state.forecastPayload = null;
      fixture.state.forecastRecords = unavailableCase.records;
      await page.goto(fixture.origin, { waitUntil: "networkidle" });
      assert.equal(await page.locator("[data-condition='rain'] .condition-status").innerText(), "Unavailable", unavailableCase.label);
      assert.equal(await page.locator("[data-condition='air-quality'] .condition-status").innerText(), "Unavailable", unavailableCase.label);
      assert.equal(await page.locator("[data-condition='uv-index'] .condition-status").innerText(), "Unavailable", unavailableCase.label);
      assert.equal(await page.locator(".local-alert").count(), 0, unavailableCase.label);
      assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /12-unavailable\.svg$/u, unavailableCase.label);
    }

    fixture.state.forecastRecords = [exactForecast];
    await page.reload({ waitUntil: "networkidle" });
    assert.match(await page.locator("[data-condition='rain'] .condition-primary").innerText(), /^0\s*in\/h$/u);
    assert.equal(await page.locator("[data-condition='rain'] .condition-status").innerText(), "Dry");
    assert.equal(await page.locator("[data-condition='air-quality'] .condition-primary").innerText(), "0");
    assert.notEqual(await page.locator("[data-condition='air-quality'] .condition-status").innerText(), "Unavailable");
    assert.equal(await page.locator("[data-condition='uv-index'] .condition-primary").innerText(), "0");
    assert.equal(await page.locator("[data-condition='uv-index'] .condition-status").innerText(), "Low");

    fixture.state.currentRecords = [hazardousLocal];
    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await page.getByText("No current weather value is available yet.", { exact: true }).count(), 1);
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /12-unavailable\.svg$/u);
  } finally {
    // close disposable fixture resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// keep long-lived raw cards and selected forecast hours current without full reloads
test("raw regional current cards refresh hourly products on the background forecast cadence", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  fixture.state.currentRecords = [current, physicalCurrent];
  fixture.state.forecastRecords = forecast.map(
    // publish one recent regional run with quiet initial current-hour readings
    (record) => ({
      ...record,
      productRunAt: "2026-08-22T05:20:00.000Z",
      receivedAt: "2026-08-22T05:21:00.000Z",
      metrics: { ...record.metrics, pm25MicrogramsPerCubicMeter: 5, precipitationRateMmPerHour: 0, uvIndex: 0 },
    }),
  );
  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, { timezoneId: site.timezone, viewport: { height: 900, width: 960 } });
    await page.clock.install({ time: new Date("2026-08-22T05:37:00.000Z") });
    await page.addInitScript(
      // retain the raw preference across navigation
      (key) => localStorage.setItem(key, "raw"),
      FORECAST_ADJUSTMENT_MODE_STORAGE_KEY,
    );
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    assert.equal(await page.locator("[data-condition='air-quality'] .condition-primary").innerText(), "5");
    const before = [...fixture.state.requests];
    fixture.state.forecastRecords = fixture.state.forecastRecords.map(
      // make the next run visibly distinct without changing current records
      (record) => ({ ...record, metrics: { ...record.metrics, pm25MicrogramsPerCubicMeter: 12, precipitationRateMmPerHour: 2.54, uvIndex: 3 } }),
    );
    await page.clock.fastForward(15 * 60_000);
    await page.waitForFunction(
      // await the background forecast response and complete redraw
      () => document.querySelector("[data-condition='air-quality'] .condition-primary")?.textContent === "12",
    );
    assert.match(await page.locator("[data-condition='rain'] .condition-primary").innerText(), /^0\.1\s*in\/h$/u);
    assert.equal(await page.locator("[data-condition='uv-index'] .condition-primary").innerText(), "3");
    assert.match(await page.locator(".section-nav-weather-icon").getAttribute("src") ?? "", /09-heavy-rain\.svg$/u);
    const background = fixture.state.requests.slice(before.length);
    assert.equal(background.filter((url) => url.includes("/sites/ballydidean/forecast")).length, 1);
    assert.equal(background.some((url) => url.includes("/tides") || url.includes("/daily-precipitation")), false);
    await page.getByRole("link", { name: "Forecast", exact: true }).click();
    const grid = page.locator("[data-forecast-charts]");
    await grid.focus();
    await page.keyboard.press("ArrowRight");
    const selected = await grid.getAttribute("data-forecast-selected-position");
    const forecastReads = fixture.state.requests.filter(
      // count only regional product reads
      (url) => url.includes("/sites/ballydidean/forecast"),
    ).length;
    const nextForecast = page.waitForResponse(
      // await the next regional forecast refresh
      (response) => response.url().includes("/sites/ballydidean/forecast") && response.status() === 200,
    );
    await page.clock.fastForward(15 * 60_000);
    await nextForecast;
    assert.equal(fixture.state.requests.filter(
      // require one bounded refresh per interval
      (url) => url.includes("/sites/ballydidean/forecast"),
    ).length, forecastReads + 1);
    assert.equal(await grid.getAttribute("data-forecast-selected-position"), selected);
    assert.equal(await grid.evaluate(
      // preserve the keyboard interaction through the redraw
      (element) => element === document.activeElement,
    ), true);
  } finally {
    // close disposable fixture resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// verify the privacy policy as a static offline document
test("settings opens a readable no-script privacy policy that remains available offline", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 780, width: 390 },
    });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    await page.getByRole("link", { name: "Settings" }).click();
    await page.waitForURL(`${fixture.origin}/settings`);
    const privacyLink = page.getByRole("link", { name: "Privacy policy" });
    assert.equal(await privacyLink.getAttribute("href"), "/privacy");
    assert.equal(await privacyLink.getAttribute("data-weather-route"), null);
    const privacyDocument = page.waitForResponse(
      // require a full static document request
      (response) => response.url() === `${fixture.origin}/privacy` &&
        response.request().resourceType() === "document",
    );
    await privacyLink.click();
    assert.equal((await privacyDocument).status(), 200);
    await page.waitForURL(`${fixture.origin}/privacy`);
    assert.equal(await page.getByRole("heading", { level: 1, name: "Privacy policy" }).isVisible(), true);
    assert.equal(await page.getByRole("link", { name: "Back to weather" }).getAttribute("href"), "/");
    assert.equal(await page.locator('a[href^="mailto:"]').getAttribute("href"), "mailto:sanctuary@ballydidean.farm");
    assert.equal(await page.locator("script").count(), 0);
    const readability = await page.locator("main").evaluate(
      // measure rendered mobile readability
      (main) => {
        const style = getComputedStyle(main);
        return {
          fontSize: Number.parseFloat(style.fontSize),
          horizontalOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
          lineHeight: Number.parseFloat(style.lineHeight),
          visibleWidth: main.getBoundingClientRect().width,
        };
      },
    );
    assert.equal(readability.horizontalOverflow, 0);
    assert.equal(readability.fontSize >= 16, true);
    assert.equal(readability.lineHeight / readability.fontSize >= 1.4, true);
    assert.equal(readability.visibleWidth <= 390 && readability.visibleWidth >= 280, true);
    assert.equal(
      await page.evaluate(
        // require one active shell worker before disconnecting
        async () => (await navigator.serviceWorker.ready).active?.state === "activated",
      ),
      true,
    );
    await page.context().setOffline(true);

    // require both canonical privacy paths offline
    for (const pathname of ["/privacy", "/privacy/"]) {
      const response = await page.goto(`${fixture.origin}${pathname}`, { waitUntil: "domcontentloaded" });
      assert.equal(response?.status(), 200);
      assert.equal(await page.getByRole("heading", { level: 1, name: "Privacy policy" }).isVisible(), true);
    }

    await page.context().setOffline(false);
    const noScriptPage = await browser.newPage({
      javaScriptEnabled: false,
      viewport: { height: 780, width: 320 },
    });
    await noScriptPage.goto(`${fixture.origin}/privacy/`, { waitUntil: "load" });
    assert.equal(await noScriptPage.getByRole("heading", { level: 1, name: "Privacy policy" }).isVisible(), true);
    assert.equal(await noScriptPage.locator("script").count(), 0);
    assert.equal(
      await noScriptPage.evaluate(
        // prohibit narrow-page overflow without JavaScript
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
      ),
      0,
    );
    await noScriptPage.close();
  } finally {
    // close only disposable browser resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// retain switching and fail-raw behavior without an adjustment infobox
test("forecast switch stays accessibly named Adjusted and fail-raw on desktop and mobile", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();

    // verify every Stage 7 browser state at both responsive widths
    for (const viewport of [
      { height: 900, width: 960 },
      { height: 844, width: 390 },
    ]) {
      const page = await createFixturePage(browser, {
        hasTouch: viewport.width < 600,
        viewport,
      });

      // prove the inactive registry preserves the raw presentation
      fixture.state.adjustmentMode = "inactive";
      await page.goto(`${fixture.origin}/forecast`, { waitUntil: "networkidle" });
      assert.equal(await page.locator(".forecast-chart").count(), 9);
      assert.equal(await page.locator("[data-forecast-adjustment-status]").count(), 0);
      assert.equal(await page.getByText("Locally adjusted", { exact: true }).count(), 0);
      const inactiveToggle = page.getByRole("switch", { name: "Adjusted", exact: true });
      assert.equal(await inactiveToggle.getAttribute("aria-checked"), "true");
      assert.equal(await inactiveToggle.getAttribute("data-forecast-adjustment-fallback"), "true");
      assert.equal(await inactiveToggle.isEnabled(), true);
      assert.equal((await inactiveToggle.textContent() ?? "").trim(), "");
      const inactiveTemperature = await page.locator('[data-forecast-chart="temperature"] [data-forecast-value="0"]').textContent();
      const inactiveWind = await page.locator('[data-forecast-chart="wind"] [data-forecast-value="0"]').textContent();
      await inactiveToggle.click();
      assert.equal(await inactiveToggle.getAttribute("aria-checked"), "false");
      assert.equal((await inactiveToggle.textContent() ?? "").trim(), "");
      await inactiveToggle.click();
      assert.equal(await inactiveToggle.getAttribute("aria-checked"), "true");

      // prove a verified bundle still changes chart values
      fixture.state.adjustmentMode = "active";
      await page.reload({ waitUntil: "networkidle" });
      const adjustmentToggle = page.getByRole("switch", { name: "Adjusted", exact: true });
      assert.equal(await page.locator("[data-forecast-adjustment-status]").count(), 0);
      assert.equal((await adjustmentToggle.textContent() ?? "").trim(), "");
      assert.equal(await adjustmentToggle.getAttribute("aria-checked"), "true");
      assert.equal(await adjustmentToggle.isEnabled(), true);
      assert.equal(
        await adjustmentToggle.evaluate(
          // require the switch inside the top-right masthead actions
          (toggle) => toggle.parentElement?.classList.contains("masthead-actions") === true,
        ),
        true,
      );
      assert.equal(
        (await page.locator('[data-forecast-chart="temperature"] [data-forecast-value="0"]').textContent())?.trim(),
        "53 °F",
      );
      const activeScreen = await page.locator("[data-forecast-charts]").screenshot();
      assert.ok(activeScreen.byteLength > 1_000);

      const forecastScrubber = page.getByRole("slider", { name: "Forecast time scrubber" });
      await forecastScrubber.press("ArrowRight");
      const forecastScrubberBounds = await forecastScrubber.boundingBox();
      assert.notEqual(forecastScrubberBounds, null);
      await forecastScrubber.click({
        position: {
          x: forecastScrubberBounds.width * 0.437,
          y: forecastScrubberBounds.height / 2,
        },
      });
      const selectedForecastPosition = await forecastScrubber.getAttribute("data-forecast-selected-position");
      assert.equal(Number.isInteger(Number(selectedForecastPosition)), false);
      const adjustedTemperature = (await page.locator('[data-forecast-chart="temperature"] [data-forecast-value="0"]').textContent())?.trim();
      const adjustedWind = (await page.locator('[data-forecast-chart="wind"] [data-forecast-value="0"]').textContent())?.trim();
      await adjustmentToggle.click();
      const regionalToggle = page.getByRole("switch", { name: "Adjusted", exact: true });
      assert.equal(await regionalToggle.getAttribute("aria-checked"), "false");
      assert.equal((await regionalToggle.textContent() ?? "").trim(), "");
      assert.equal(await regionalToggle.evaluate(
        // retain keyboard focus through rerenders
        (toggle) => document.activeElement === toggle,
      ), true);
      assert.equal(
        await forecastScrubber.getAttribute("data-forecast-selected-position"),
        selectedForecastPosition,
      );
      assert.equal(
        (await page.locator('[data-forecast-chart="temperature"] [data-forecast-value="0"]').textContent())?.trim(),
        adjustedTemperature,
      );
      assert.notEqual(
        (await page.locator('[data-forecast-chart="wind"] [data-forecast-value="0"]').textContent())?.trim(),
        adjustedWind,
      );
      assert.equal(await page.locator("[data-forecast-adjustment-status]").count(), 0);

      // preserve the preference across reloads and both forecast-bearing routes
      await page.reload({ waitUntil: "networkidle" });
      assert.equal(await adjustmentToggle.getAttribute("aria-checked"), "false");
      assert.equal((await adjustmentToggle.textContent() ?? "").trim(), "");
      await page.goto(fixture.origin, { waitUntil: "networkidle" });
      assert.equal(await adjustmentToggle.getAttribute("aria-checked"), "false");
      assert.equal((await adjustmentToggle.textContent() ?? "").trim(), "");
      const rawHomeTemperature = await page.locator("[data-condition='temperature'] .condition-forecast-readings").textContent();
      await adjustmentToggle.click();
      assert.equal(await adjustmentToggle.getAttribute("aria-checked"), "true");
      assert.equal((await adjustmentToggle.textContent() ?? "").trim(), "");
      assert.notEqual(
        await page.locator("[data-condition='temperature'] .condition-forecast-readings").textContent(),
        rawHomeTemperature,
      );
      await page.getByRole("link", { name: "Forecast" }).click();
      await page.locator("[data-forecast-charts]").waitFor();
      assert.equal(await adjustmentToggle.getAttribute("aria-checked"), "true");

      // prove extended hours remain raw regardless of the switch
      await page.getByRole("button", { name: "10 days" }).click();
      await page.locator('[data-forecast-charts][data-forecast-days="10"]').waitFor();
      await page.locator("[data-forecast-charts]").press("End");
      const extendedTemperature = await page.locator('[data-forecast-chart="temperature"] [data-forecast-value="0"]').textContent();
      await adjustmentToggle.click();
      assert.equal(await adjustmentToggle.getAttribute("aria-checked"), "false");
      assert.equal(
        await page.locator('[data-forecast-chart="temperature"] [data-forecast-value="0"]').textContent(),
        extendedTemperature,
      );
      await adjustmentToggle.click();
      assert.equal(await page.locator("[data-forecast-adjustment-status]").count(), 0);

      // prove a bundle cross-link fault cannot interrupt raw charts
      fixture.state.adjustmentMode = "fault";
      await page.reload({ waitUntil: "networkidle" });
      assert.equal(await adjustmentToggle.getAttribute("aria-checked"), "true");
      assert.equal(await adjustmentToggle.getAttribute("data-forecast-adjustment-available"), "false");
      assert.equal((await adjustmentToggle.textContent() ?? "").trim(), "");
      assert.equal(await page.locator("[data-forecast-adjustment-status]").count(), 0);
      assert.equal(
        await page.locator('[data-forecast-chart="temperature"] [data-forecast-value="0"]').textContent(),
        inactiveTemperature,
      );
      assert.equal(
        await page.locator('[data-forecast-chart="wind"] [data-forecast-value="0"]').textContent(),
        inactiveWind,
      );
      assert.equal(await page.getByText("Locally adjusted", { exact: true }).count(), 0);
      assert.equal(await page.locator(".forecast-chart").count(), 9);
      assert.equal(
        await page.locator("body").evaluate(
          // reject forecast horizontal overflow
          (body) => body.scrollWidth > document.documentElement.clientWidth,
        ),
        false,
      );
      const fallbackScreen = await page.locator("[data-forecast-charts]").screenshot();
      assert.ok(fallbackScreen.byteLength > 500);
      await page.close();
    }
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// keep one responsive forecast range in the masthead
test("forecast masthead keeps its range controls on one responsive row", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    fixture.state.adjustmentMode = "active";
    const loadingPage = await createFixturePage(browser, {
      viewport: { height: 900, width: 390 },
    });
    let releaseForecastRead;
    const forecastReadReleased = new Promise(
      // expose one deterministic header loading state
      (resolveRelease) => {
        releaseForecastRead = resolveRelease;
      },
    );
    await loadingPage.route(
      /\/api\/v1\/sites\/ballydidean\/forecast(?:\?|$)/u,
      // hold only the forecast response behind its masthead control
      async (route) => {
        await forecastReadReleased;
        await route.continue();
      },
    );
    await loadingPage.goto(`${fixture.origin}/forecast`, { waitUntil: "domcontentloaded" });
    const loadingRange = loadingPage.getByRole("group", { name: "Forecast range" });
    await loadingRange.waitFor();
    await assertSkeletonLoadingState(loadingPage, ".forecast-panel.skeleton-region");
    assert.equal(await loadingPage.locator(".forecast-range-selector").count(), 1);
    assert.equal(
      await loadingRange.locator("button").evaluateAll(
        // disable each range during the initial read
        (buttons) => buttons.length === 3 && buttons.every(
          // reject one prematurely enabled range
          (button) => button.disabled,
        ),
      ),
      true,
    );
    releaseForecastRead();
    await loadingPage.locator('[data-forecast-charts][data-forecast-days="1"]').waitFor();
    await assertSkeletonLoadingSettled(loadingPage);
    assert.equal(await loadingRange.locator("button:disabled").count(), 0);
    await loadingPage.close();

    const viewports = [
      { height: 1_050, width: 1_440 },
      { height: 900, width: 960 },
      { height: 900, width: 768 },
      { height: 900, width: 673 },
      { height: 900, width: 672 },
      { height: 844, width: 390 },
      { height: 844, width: 320 },
    ];

    // verify both sides of the exact mobile breakpoint
    for (const viewport of viewports) {
      const page = await createFixturePage(browser, {
        hasTouch: viewport.width <= 672,
        viewport,
      });
      const pageErrors = [];
      // capture unexpected browser failures
      page.on("pageerror", (error) => pageErrors.push(error.message));
      await page.goto(`${fixture.origin}/forecast`, { waitUntil: "networkidle" });
      const range = page.getByRole("group", { name: "Forecast range" });
      const toggle = page.getByRole("switch", { name: "Adjusted", exact: true });
      assert.equal(await page.locator(".forecast-range-selector").count(), 1);
      assert.equal(await page.locator(".forecast-controls").count(), 0);
      assert.deepEqual(await range.locator("button").allTextContents(), ["Today", "5 days", "10 days"]);
      // capture the current rendered masthead after each rerender
      const captureLayout = async () => await page.locator(".masthead").evaluate(
        // measure the rendered masthead without relying on css declarations alone
        (masthead) => {
          const heading = masthead.querySelector("h1");
          const selector = masthead.querySelector(".forecast-range-selector");
          const actions = masthead.querySelector(".masthead-actions");
          const adjustment = masthead.querySelector("[data-forecast-adjustment-toggle]");
          const adjustmentTrack = adjustment?.querySelector(".forecast-adjustment-toggle-track");
          const adjustmentThumb = adjustmentTrack?.querySelector(".forecast-adjustment-toggle-thumb");
          const rangeHandle = selector?.querySelector("button[aria-pressed='true']");

          // require every forecast header control
          if (
            !(heading instanceof HTMLElement) ||
            !(selector instanceof HTMLElement) ||
            !(actions instanceof HTMLElement) ||
            !(adjustment instanceof HTMLElement) ||
            !(adjustmentTrack instanceof HTMLElement) ||
            !(adjustmentThumb instanceof HTMLElement) ||
            !(rangeHandle instanceof HTMLElement)
          ) {
            throw new Error("forecast masthead controls are incomplete");
          }

          const mastheadBounds = masthead.getBoundingClientRect();
          const headingBounds = heading.getBoundingClientRect();
          const selectorBounds = selector.getBoundingClientRect();
          const actionsBounds = actions.getBoundingClientRect();
          const adjustmentBounds = adjustment.getBoundingClientRect();
          const adjustmentTrackBounds = adjustmentTrack.getBoundingClientRect();
          const adjustmentThumbBounds = adjustmentThumb.getBoundingClientRect();
          const rangeHandleBounds = rangeHandle.getBoundingClientRect();
          const buttons = [...selector.querySelectorAll("button")];
          const buttonBounds = buttons.map(
            // measure each range button row
            (button) => button.getBoundingClientRect(),
          );
          const buttonTops = buttonBounds.map(
            // project one range button row
            (bounds) => bounds.top,
          );
          const textLineCounts = buttons.map(
            // count the rendered lines in each label
            (button) => {
              const text = document.createRange();
              text.selectNodeContents(button);
              return text.getClientRects().length;
            },
          );
          return {
            actionCenter: actionsBounds.top + actionsBounds.height / 2,
            actionContained: actionsBounds.right <= mastheadBounds.right + 1 &&
              actionsBounds.top >= mastheadBounds.top - 1 &&
              actionsBounds.bottom <= mastheadBounds.bottom + 1,
            adjustmentHeight: adjustmentBounds.height,
            buttonRowSpread: Math.max(...buttonTops) - Math.min(...buttonTops),
            buttonTextLines: textLineCounts,
            directChild: selector.parentElement === masthead,
            flexWrap: getComputedStyle(selector).flexWrap,
            forecastClass: masthead.classList.contains("forecast-masthead"),
            headingCenter: headingBounds.top + headingBounds.height / 2,
            headingRight: headingBounds.right,
            headerOverflow: masthead.scrollWidth - masthead.clientWidth,
            mobileRangeBelow: selectorBounds.top >= Math.max(headingBounds.bottom, actionsBounds.bottom),
            mobileRangeWidthDifference: Math.abs(selectorBounds.width - mastheadBounds.width),
            nextSiblingIsActions: selector.nextElementSibling === actions,
            pageOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
            rangeCenter: selectorBounds.top + selectorBounds.height / 2,
            rangeContained: selectorBounds.left >= mastheadBounds.left - 1 &&
              selectorBounds.right <= mastheadBounds.right + 1 &&
              selectorBounds.top >= mastheadBounds.top - 1 &&
              selectorBounds.bottom <= mastheadBounds.bottom + 1,
            rangeHeight: selectorBounds.height,
            rangeHandleHeight: rangeHandleBounds.height,
            rangeHandleInset: rangeHandleBounds.top - selectorBounds.top,
            rangeBorderColor: getComputedStyle(selector).borderTopColor,
            rangeLeft: selectorBounds.left,
            rangeRight: selectorBounds.right,
            trackHeight: adjustmentTrackBounds.height,
            thumbHeight: adjustmentThumbBounds.height,
            thumbInset: adjustmentThumbBounds.top - adjustmentTrackBounds.top,
            trackBorderColor: getComputedStyle(adjustmentTrack).borderTopColor,
            toggleCenter: adjustmentBounds.top + adjustmentBounds.height / 2,
            toggleLeft: adjustmentBounds.left,
            topRowSeparated: headingBounds.right <= actionsBounds.left,
          };
        },
      );
      // enforce identical visible control heights
      const assertMatchedControlHeights = (currentLayout) => {
        assert.equal(Math.abs(currentLayout.thumbHeight - currentLayout.rangeHandleHeight) < 0.25, true, JSON.stringify({ viewport, currentLayout }));
        assert.equal(Math.abs(currentLayout.thumbInset - currentLayout.rangeHandleInset) < 0.25, true, JSON.stringify({ viewport, currentLayout }));
        assert.equal(currentLayout.trackBorderColor, currentLayout.rangeBorderColor, JSON.stringify({ viewport, currentLayout }));
        assert.equal(
          Math.abs(currentLayout.adjustmentHeight - currentLayout.rangeHeight) < 0.25,
          true,
          JSON.stringify({ viewport, currentLayout }),
        );
        assert.equal(
          Math.abs(currentLayout.trackHeight - currentLayout.rangeHeight) < 0.25,
          true,
          JSON.stringify({ viewport, currentLayout }),
        );
      };
      const layout = await captureLayout();
      assertMatchedControlHeights(layout);
      assert.equal(layout.directChild, true);
      assert.equal(layout.nextSiblingIsActions, true);
      assert.equal(layout.forecastClass, true);
      assert.equal(layout.flexWrap, "nowrap");
      assert.equal(layout.buttonRowSpread < 1, true);
      assert.deepEqual(layout.buttonTextLines, [1, 1, 1]);
      assert.equal(layout.rangeContained, true);
      assert.equal(layout.actionContained, true);
      assert.equal(layout.headerOverflow, 0);
      assert.equal(layout.pageOverflow, 0);

      // keep desktop controls in one nonoverlapping row
      if (viewport.width > 672) {
        assert.equal(Math.abs(layout.headingCenter - layout.rangeCenter) < 1, true);
        assert.equal(Math.abs(layout.toggleCenter - layout.rangeCenter) < 1, true);
        assert.equal(layout.headingRight <= layout.rangeLeft, true);
        assert.equal(layout.rangeRight <= layout.toggleLeft, true);
        assert.equal(layout.mobileRangeBelow, false);
      } else {
        // stack only the full-width range on mobile
        assert.equal(layout.topRowSeparated, true);
        assert.equal(Math.abs(layout.headingCenter - layout.actionCenter) < 1, true);
        assert.equal(layout.mobileRangeBelow, true);
        assert.equal(layout.mobileRangeWidthDifference < 1, true);
      }

      // retain every range and adjustment interaction after masthead rerenders
      for (const days of [5, 10, 1]) {
        const label = days === 1 ? "Today" : `${String(days)} days`;
        await page.getByRole("button", { name: label, exact: true }).click();
        await page.locator(`[data-forecast-charts][data-forecast-days="${String(days)}"]`).waitFor();
        assert.equal(await page.locator(".forecast-range-selector").count(), 1);
        assert.equal(
          await page.getByRole("button", { name: label, exact: true }).getAttribute("aria-pressed"),
          "true",
        );
        assertMatchedControlHeights(await captureLayout());
      }
      assert.equal(await toggle.getAttribute("aria-checked"), "true");
      await toggle.click();
      assert.equal(await toggle.getAttribute("aria-checked"), "false");
      assertMatchedControlHeights(await captureLayout());
      await toggle.click();
      assert.equal(await toggle.getAttribute("aria-checked"), "true");
      assertMatchedControlHeights(await captureLayout());

      // retain the shared height under browser text scaling
      if (viewport.width === 1_440) {
        // compare reduced and enlarged root text
        for (const rootSize of [12, 20]) {
          await page.evaluate(
            // apply one browser text scale
            (size) => {
              document.documentElement.style.fontSize = `${String(size)}px`;
            },
            rootSize,
          );
          await page.evaluate(
            // settle the bundled fonts at the new root size
            async () => await document.fonts.ready,
          );
          assertMatchedControlHeights(await captureLayout());
        }
        await page.evaluate(
          // restore the default root size for disposal
          () => document.documentElement.style.removeProperty("font-size"),
        );
        assertMatchedControlHeights(await captureLayout());
      }
      assert.deepEqual(pageErrors, []);
      await page.close();
    }
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// preserve explicit raw selection with a stable accessible name
test("wind adjustment keeps an Adjusted accessible name and persists the raw choice", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    fixture.state.adjustmentMode = "canary";
    const page = await createFixturePage(browser, {
      hasTouch: true,
      timezoneId: "America/Los_Angeles",
      viewport: { height: 844, width: 390 },
    });
    await page.goto(`${fixture.origin}/forecast`, { waitUntil: "networkidle" });

    const toggle = page.getByRole("switch", { name: "Adjusted", exact: true });
    const rawTemperature = await page.locator('[data-forecast-chart="temperature"]').textContent();
    const adjustedWind = await page.locator('[data-forecast-chart="wind"]').textContent();

    assert.equal(await toggle.isEnabled(), true);
    assert.equal(await toggle.getAttribute("aria-checked"), "true");
    assert.equal((await toggle.textContent() ?? "").trim(), "");
    assert.equal(await page.locator("[data-forecast-adjustment-status]").count(), 0);
    assert.equal(
      await page.locator("body").evaluate(
        // keep the preference switch inside mobile width
        (body) => body.scrollWidth > document.documentElement.clientWidth,
      ),
      false,
    );
    assert.equal(
      await page.evaluate(
        // prove the default needs no stored preference
        (key) => localStorage.getItem(key),
        FORECAST_ADJUSTMENT_MODE_STORAGE_KEY,
      ),
      null,
    );

    await toggle.click();
    assert.equal(await toggle.getAttribute("aria-checked"), "false");
    assert.equal((await toggle.textContent() ?? "").trim(), "");
    assert.equal(await page.locator("[data-forecast-adjustment-status]").count(), 0);
    assert.equal(
      await page.locator('[data-forecast-chart="temperature"]').textContent(),
      rawTemperature,
    );
    assert.notEqual(
      await page.locator('[data-forecast-chart="wind"]').textContent(),
      adjustedWind,
    );
    assert.equal(
      await page.evaluate(
        // retain the explicit raw selection
        (key) => localStorage.getItem(key),
        FORECAST_ADJUSTMENT_MODE_STORAGE_KEY,
      ),
      "raw",
    );

    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await toggle.getAttribute("aria-checked"), "false");
    assert.equal((await toggle.textContent() ?? "").trim(), "");
    await page.getByRole("link", { name: "Now", exact: true }).click();
    assert.equal(await toggle.getAttribute("aria-checked"), "false");
    assert.equal((await toggle.textContent() ?? "").trim(), "");
    await toggle.click();
    assert.equal(await toggle.getAttribute("aria-checked"), "true");
    assert.equal((await toggle.textContent() ?? "").trim(), "");
    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await toggle.getAttribute("aria-checked"), "true");
    assert.equal((await toggle.textContent() ?? "").trim(), "");
    await page.close();
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// enforce both the server production marker and the exact public origin
test("google tag is production-only and initializes once without blocking navigation", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  try {
    browser = await launchBrowser();
    // keep real analytics traffic out of fixture validation
    for (const { origin, enabled, admin = false, blocked = false, tracked } of [
      { origin: "https://weather.ballydidean.farm", enabled: true, tracked: true },
      { origin: "https://weather.ballydidean.farm", enabled: true, blocked: true, tracked: true },
      { origin: "https://weather.ballydidean.farm", enabled: false, tracked: false },
      { origin: "https://weather.ballydidean.farm", enabled: true, admin: true, tracked: false },
      { origin: fixture.origin, enabled: true, tracked: false },
      { origin: "https://preview.example", enabled: true, tracked: false },
      { origin: "https://weather.ballydidean.farm.preview.example", enabled: true, tracked: false },
    ]) {
      const page = await createFixturePage(browser, { serviceWorkers: "block", viewport: { height: 900, width: 390 } });
      const errors = [];
      const tagRequests = [];
      // record uncaught application failures even when the tag is blocked
      page.on("pageerror", (error) => errors.push(error.message));
      // serve only disposable fixture bytes under the tested origin
      await page.route(`${origin}/**`, async (route) => {
        const url = new URL(route.request().url());
        const response = await route.fetch({ url: `${fixture.origin}${url.pathname}${url.search}` });
        // model the server's HTML marker without changing other response bytes
        if (response.headers()["content-type"]?.includes("text/html")) {
          const body = (await response.text())
            .replace('data-weather-analytics="false"', `data-weather-analytics="${String(enabled)}"`)
            .replace('data-weather-admin="false"', `data-weather-admin="${String(admin)}"`);
          await route.fulfill({ response, body });
          return;
        }
        await route.fulfill({ response });
      });
      // stub the third-party loader so tests never send production events
      await page.route("https://www.googletagmanager.com/**", async (route) => {
        tagRequests.push(route.request().url());
        // simulate an ad blocker without changing the application shell
        if (blocked) {
          await route.abort("blockedbyclient");
          return;
        }
        await route.fulfill({ status: 200, contentType: "text/javascript", body: "" });
      });
      await page.goto(origin, { waitUntil: "networkidle" });
      await page.locator(".current-conditions").waitFor();
      assert.deepEqual(tagRequests, tracked ? ["https://www.googletagmanager.com/gtag/js?id=G-NYT2EZS8BX"] : []);
      assert.equal(await page.locator("#weather-google-tag").count(), tracked ? 1 : 0);
      const commands = await page.evaluate(
        // inspect the queued bootstrap independently of the stubbed loader
        () => (window.dataLayer ?? []).map((command) => Array.from(command)),
      );
      assert.equal(commands.length, tracked ? 2 : 0);
      // require the exact requested destination and disable advertising features
      if (tracked) {
        assert.equal(commands[0][0], "js");
        assert.deepEqual(commands[1], ["config", "G-NYT2EZS8BX", {
          allow_google_signals: false,
          allow_ad_personalization_signals: false,
        }]);
      }
      await page.getByRole("link", { name: "Forecast", exact: true }).click();
      await page.waitForURL(`${origin}/forecast`);
      await page.locator(".forecast-panel").waitFor();
      await page.goBack();
      await page.locator(".current-conditions").waitFor();
      await page.locator(".weather-content[aria-busy='false']").waitFor();
      assert.equal(tagRequests.length, tracked ? 1 : 0);
      assert.equal(await page.evaluate(
        // leave page-view events to enhanced measurement rather than duplicating them
        () => window.dataLayer?.length ?? 0,
      ), tracked ? 2 : 0);
      assert.deepEqual(errors, []);
      // drain every proxy callback before closing its page
      await page.unrouteAll({ behavior: "wait" });
      await page.close();
    }
  } finally {
    // release only the disposable fixture and browser
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// verify public navigation preserves one document and browser history
test("public navigation updates the URL and restores routed content from history", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 1440 },
    });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    await page.locator(".current-conditions").waitFor();
    await page.evaluate(
      // mark the current document against full-page reloads
      () => {
        document.documentElement.dataset.navigationDocument = "preserved";
      },
    );

    await page.getByRole("link", { name: "Forecast" }).click();
    await page.waitForURL(`${fixture.origin}/forecast`);
    await page.locator(".forecast-panel").waitFor();
    assert.equal(
      await page.evaluate(
        // confirm navigation kept the original document
        () => document.documentElement.dataset.navigationDocument,
      ),
      "preserved",
    );
    assert.equal(await page.getByRole("link", { name: "Forecast" }).getAttribute("aria-current"), "page");

    await page.getByRole("link", { name: "Trends" }).click();
    await page.waitForURL(`${fixture.origin}/trends`);
    await page.locator(".trends-panel").waitFor();
    assert.equal(await page.getByRole("link", { name: "Trends" }).getAttribute("aria-current"), "page");

    await page.evaluate(
      // restore the preceding application route
      () => window.history.back(),
    );
    await page.waitForURL(`${fixture.origin}/forecast`);
    await page.locator(".forecast-panel").waitFor();
    assert.equal(await page.getByRole("link", { name: "Forecast" }).getAttribute("aria-current"), "page");

    await page.evaluate(
      // revisit the next application route
      () => window.history.forward(),
    );
    await page.waitForURL(`${fixture.origin}/trends`);
    await page.locator(".trends-panel").waitFor();
    assert.equal(
      await page.evaluate(
        // confirm history traversal kept the original document
        () => document.documentElement.dataset.navigationDocument,
      ),
      "preserved",
    );
  } finally {
    // close only disposable browser resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// verify embedded navigation owns the visible same-origin URL
test("same-origin iframe navigation updates the parent URL", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 1440 },
    });
    await page.goto(`${fixture.origin}/embed`, { waitUntil: "networkidle" });
    const frame = page.frameLocator('iframe[title="Embedded weather"]');
    await frame.locator(".current-conditions").waitFor();
    await frame.locator("html").evaluate(
      // mark the child document against iframe reloads
      (documentElement) => {
        documentElement.dataset.navigationDocument = "preserved";
      },
    );

    await frame.getByRole("link", { name: "Forecast" }).click();
    await page.waitForURL(`${fixture.origin}/forecast`, { timeout: 3_000 });
    await frame.locator(".forecast-panel").waitFor();
    assert.equal(
      await frame.locator("html").evaluate(
        // confirm the embedded document stayed mounted
        (documentElement) => documentElement.dataset.navigationDocument,
      ),
      "preserved",
    );

    await frame.getByRole("link", { name: "Trends" }).click();
    await page.waitForURL(`${fixture.origin}/trends`);
    await frame.locator(".trends-panel").waitFor();

    await page.evaluate(
      // restore the preceding embedded route
      () => window.history.back(),
    );
    await page.waitForURL(`${fixture.origin}/forecast`);
    await frame.locator(".forecast-panel").waitFor();

    await page.evaluate(
      // restore the embedding host route
      () => window.history.back(),
    );
    await page.waitForURL(`${fixture.origin}/embed`);
    await frame.locator(".current-conditions").waitFor();
  } finally {
    // close only disposable browser resources
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// send one bounded JSON response
function sendJson(response, body, status = 200) {
  response.statusCode = status;
  response.setHeader("content-type", "application/json; charset=utf-8");
  response.end(JSON.stringify(body));
}

// verify one rejected site wall clock
async function assertRejectedSiteWallClock(page, fixture, field, value) {
  const apiReadsBefore = fixture.state.requests.filter(
    // count weather data reads
    (entry) => entry.includes("/current") || entry.includes("/history"),
  ).length;
  await page.locator(`input[name='${field}']`).fill(value);
  await page.getByRole("button", { name: "Apply filters" }).click();
  const alert = page.getByRole("alert");
  await alert.waitFor();
  const message = await alert.textContent() ?? "";
  const apiReadsAfter = fixture.state.requests.filter(
    // recount weather data reads
    (entry) => entry.includes("/current") || entry.includes("/history"),
  ).length;

  assert.match(message, /daylight saving time/u);
  assert.match(message, /choose another time/iu);
  assert.equal(message.length <= 160, true);
  assert.equal(apiReadsAfter, apiReadsBefore);
  assert.equal(await page.locator(".table-scroll").getByText("61.2").count(), 1);
  assert.equal(await page.locator(".table-scroll").getByText("61.2").isVisible(), true);
}

// preserve dashboard contracts alongside the clouds tile and final tide pair
test("real browser covers filters, pagination, last-good recovery, attribution, and mutation denial", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "UTC",
      viewport: { height: 900, width: 1440 },
    });
    // align historical fixture forecasts with their original farm day
    await page.clock.setFixedTime(new Date("2026-08-22T05:37:00.000Z"));
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    await assert.doesNotReject(() => page.getByRole("heading", { name: "Ballydídean Weather" }).waitFor());
    assert.equal(
      await page.evaluate(async () => {
        // wait for the bundled Pixel typeface
        await document.fonts.ready;
        return document.fonts.check('16px "Google Sans Flex"');
      }),
      true,
    );
    assert.match(
      await page.locator("body").evaluate(
        // require the Pixel typography across the application
        (body) => getComputedStyle(body).fontFamily,
      ),
      /Google Sans Flex/u,
    );
    assert.match(
      await page.locator("[data-condition='temperature'] .condition-primary").textContent() ?? "",
      /52\s*°F/u,
    );
    assert.doesNotMatch(
      await page.locator("[data-condition='air-quality']").textContent() ?? "",
      /µg\/m³/u,
    );
    assert.deepEqual(
      await page.locator(".condition-card").first().evaluate(
        // require shared Material card shape and elevation
        (card) => {
          const style = getComputedStyle(card);
          return {
            borderRadius: style.borderRadius,
            elevated: style.boxShadow !== "none",
          };
        },
      ),
      { borderRadius: "24px", elevated: true },
    );
    assert.equal(
      await page.evaluate(() => document.fonts.check('20px "Material Symbols Rounded"')),
      true,
    );
    assert.match(
      await page.locator(".section-nav-settings .material-symbols-rounded").evaluate(
        // require the official Material symbol face
        (icon) => getComputedStyle(icon).fontFamily,
      ),
      /Material Symbols Rounded/u,
    );
    assert.equal(await page.locator(".condition-color rect").count(), 10);
    assert.equal(await page.locator(".condition-label .material-symbols-rounded").count(), 10);
    assert.equal(await page.locator(".condition-status-color rect").count(), 10);
    assert.equal(await page.locator(".condition-status-dark").count(), 9);
    assert.equal(await page.locator(".condition-status-light").count(), 1);
    assert.equal(
      await page.locator("[data-condition='humidity'] .condition-status-color rect").getAttribute("fill"),
      "rgb(67, 151, 86)",
    );
    assert.match(await page.locator("[data-condition='humidity']").textContent() ?? "", /Comfortable/u);
    assert.equal(
      await page.locator(".condition-status-dark").first().evaluate(
        // require maximum contrast over mid-tone bands
        (status) => getComputedStyle(status).color,
      ),
      "rgb(0, 0, 0)",
    );
    assert.deepEqual(
      await page.locator(".condition-card").evaluateAll(
        // freeze the requested reading order
        (cards) => cards.map((card) => card.getAttribute("data-condition")),
      ),
      ["temperature", "wind", "rain", "clouds", "humidity", "air-quality", "pressure", "uv-index", "tide", "sunset"],
    );
    assert.deepEqual(
      await page.locator(".current-conditions").evaluate(
        // place wind beside temperature
        (grid) => {
          // read one named card
          const bounds = (name) => grid.querySelector(`[data-condition='${name}']`)?.getBoundingClientRect();
          const temperature = bounds("temperature");
          const wind = bounds("wind");

          // require the complete primary row
          if (temperature === undefined || wind === undefined) {
            throw new Error("condition grid is incomplete");
          }

          return {
            equalPrimaryWidths: Math.abs(wind.width - temperature.width) < 1,
            windBesideTemperature: Math.abs(wind.top - temperature.top) < 1 && wind.left >= temperature.right,
          };
        },
      ),
      { equalPrimaryWidths: true, windBesideTemperature: true },
    );
    assert.deepEqual(
      await page.locator('.section-nav a[aria-current="page"] .section-nav-icon').evaluate(
        // require the active Material indicator pill
        (indicator) => {
          const bounds = indicator.getBoundingClientRect();
          const style = getComputedStyle(indicator);
          return {
            backgroundColor: style.backgroundColor,
            height: bounds.height,
            width: bounds.width,
          };
        },
      ),
      { backgroundColor: "rgb(240, 230, 150)", height: 32, width: 56 },
    );
    assert.equal(await page.locator(".weather-content > .current-conditions").count(), 1);
    assert.equal(await page.locator(".current-panel, #current-heading, .freshness, .provenance").count(), 0);
    assert.equal(await page.getByText("Right now", { exact: true }).count(), 0);
    assert.equal(await page.getByRole("heading", { name: "Forecast timeline" }).count(), 0);
    assert.equal(await page.getByRole("heading", { name: "Yearly trends" }).count(), 0);
    assert.equal(await page.getByRole("heading", { name: "Nearby station map" }).count(), 0);
    assert.deepEqual(
      await page.locator(".weather-content > .panel").evaluateAll(
        // require every section wrapper to remain visually flat
        (panels) => panels.map((panel) => {
          const style = getComputedStyle(panel);
          return {
            backgroundColor: style.backgroundColor,
            borderWidth: style.borderWidth,
            boxShadow: style.boxShadow,
            padding: style.padding,
          };
        }),
      ),
      [],
    );
    assert.equal(await page.locator(".alert-list").count(), 0);
    assert.equal(await page.getByText("Farm sensor map").count(), 0);
    await page.getByRole("link", { name: "Forecast" }).click();
    await page.waitForURL(`${fixture.origin}/forecast`);
    await page.waitForLoadState("networkidle");
    assert.equal(await page.getByRole("heading", { name: "Forecast timeline" }).count(), 0);
    assert.equal(await page.locator(".forecast-panel > .forecast-chart-shell").count(), 1);
    assert.equal(await page.locator(".forecast-model, .forecast-scrub-help").count(), 0);
    assert.equal(await page.locator(".section-nav-forecast").getAttribute("aria-current"), "page");
    assert.equal(await page.locator(".current-conditions, .trends-panel, .station-map-panel, [data-history-filters]").count(), 0);
    await page.getByRole("link", { name: "Map", exact: true }).click();
    await page.waitForURL(`${fixture.origin}/map`);
    await page.waitForLoadState("networkidle");
    assert.equal(await page.getByRole("heading", { name: "Property sensors" }).isVisible(), true);
    const propertyMap = page.locator(".property-map");
    const propertySensorList = page.locator(".property-sensor-list");
    // wait for route data to replace the map skeleton
    await propertySensorList.getByText("Orchard soil", { exact: true }).waitFor();
    assert.equal(await propertySensorList.getByText("Orchard soil", { exact: true }).isVisible(), true);
    const propertySensorListButton = propertySensorList.locator('[data-property-sensor-view="soil-1"]');
    const propertySensorMarker = propertyMap.locator('[data-property-sensor-view="soil-1"]');
    assert.equal(
      await propertySensorListButton.locator(".property-sensor-label span").textContent(),
      "Moisture 42 %",
    );
    assert.equal(await propertySensorMarker.locator("title").textContent(), "Orchard soil · Moisture 42 %");
    assert.equal(
      await propertyMap.locator(".property-sensor-marker-head").evaluateAll(
        // separate markers that share an exact coordinate
        (heads) => new Set(heads.map((head) => head.getAttribute("transform"))).size,
      ),
      2,
    );
    assert.deepEqual(
      await page.locator(".property-map-layout").evaluate(
        // keep the bounded sensor catalog to the map's right
        (layout) => {
          const map = layout.querySelector(".property-map");
          const list = layout.querySelector(".property-sensor-list");

          // require both layout columns
          if (!(map instanceof HTMLElement) || !(list instanceof HTMLElement)) {
            throw new Error("property map layout is incomplete");
          }

          const mapBounds = map.getBoundingClientRect();
          const listBounds = list.getBoundingClientRect();
          const style = getComputedStyle(list);
          return {
            listRightOfMap: listBounds.left >= mapBounds.right,
            overflowY: style.overflowY,
          };
        },
      ),
      { listRightOfMap: true, overflowY: "auto" },
    );
    assert.equal(await page.locator("[data-property-sensor-details]").count(), 0);
    await propertySensorListButton.hover();
    assert.equal(await propertySensorMarker.evaluate((marker) => marker.classList.contains("related-hover")), true);
    assert.equal(
      await propertySensorMarker.locator(".property-sensor-marker-dot").evaluate(
        // expose the corresponding map marker strongly
        (marker) => getComputedStyle(marker).strokeWidth,
      ),
      "5px",
    );
    await propertySensorMarker.click();
    const propertySensorDetails = page.locator('[data-property-sensor-details="soil-1"]');
    assert.equal(await propertySensorListButton.getAttribute("aria-expanded"), "true");
    assert.equal(await propertySensorDetails.isVisible(), true);
    assert.match(await propertySensorDetails.textContent() ?? "", /Temp 63\.9 °F/u);
    assert.match(await propertySensorDetails.textContent() ?? "", /Moisture 42 %/u);
    assert.match(await propertySensorDetails.textContent() ?? "", /EcoWitt WH52 · channel 1 · soil-1/u);
    assert.match(await propertySensorDetails.textContent() ?? "", /Position 47\.950500, -122\.428100/u);
    assert.equal(await propertySensorMarker.getAttribute("aria-expanded"), "true");
    assert.equal(await propertyMap.locator("[data-property-map-layer]").count(), 3);
    assert.equal(await propertyMap.getByRole("button", { name: "Satellite" }).getAttribute("aria-pressed"), "true");
    assert.match(
      await propertyMap.locator(".map-tile-layer image").first().getAttribute("href") ?? "",
      /imagery\.nationalmap\.gov\/arcgis\/rest\/services\/USGSNAIPImagery[\s\S]*size=1280%2C800/u,
    );
    assert.equal(
      await page.locator(".property-map-panel .property-sensor-list-icon .material-symbols-rounded").first().textContent(),
      "device_thermostat",
    );
    await propertyMap.getByRole("button", { name: "Topo" }).click();
    assert.match(
      await propertyMap.locator(".map-tile-layer image").first().getAttribute("href") ?? "",
      /USGSTopo\/MapServer\/tile\/16\//u,
    );
    await propertyMap.getByRole("button", { name: "Satellite" }).click();
    assert.match(
      await propertyMap.locator(".map-tile-layer image").first().getAttribute("href") ?? "",
      /imagery\.nationalmap\.gov\/arcgis\/rest\/services\/USGSNAIPImagery[\s\S]*size=1280%2C800/u,
    );
    await propertyMap.getByRole("button", { name: "Roads" }).click();
    assert.equal(await propertyMap.getByRole("button", { name: "Roads" }).getAttribute("aria-pressed"), "true");
    assert.match(
      await propertyMap.locator(".map-tile-layer image").first().getAttribute("href") ?? "",
      /tile\.openstreetmap\.org\/17\//u,
    );
    const propertySensorMarkerSize = await propertyMap.locator(".property-sensor-marker").first().evaluate(
      // capture one marker's fixed visual footprint
      (marker) => {
        const bounds = marker.getBoundingClientRect();
        return { height: bounds.height, width: bounds.width };
      },
    );
    await propertyMap.getByRole("button", { name: "Zoom in" }).click();
    await propertyMap.getByRole("button", { name: "Zoom in" }).click();
    const zoomedPropertySensorMarkerSize = await propertyMap.locator(".property-sensor-marker").first().evaluate(
      // keep marker graphics fixed-size while their anchors move
      (marker) => {
        const bounds = marker.getBoundingClientRect();
        return { height: bounds.height, width: bounds.width };
      },
    );
    assert.equal(Math.abs(zoomedPropertySensorMarkerSize.height - propertySensorMarkerSize.height) < 0.01, true);
    assert.equal(Math.abs(zoomedPropertySensorMarkerSize.width - propertySensorMarkerSize.width) < 0.01, true);
    const propertyMapTransform = await propertyMap.locator("[data-property-interactive-map]").evaluate(
      // verify zoom remains inside the original fixed property extent
      (svg) => ({
        height: Number(svg.dataset.propertyMapHeight),
        scale: Number(svg.dataset.propertyMapScale),
        translateX: Number(svg.dataset.propertyMapTranslateX),
        translateY: Number(svg.dataset.propertyMapTranslateY),
        width: Number(svg.dataset.propertyMapWidth),
      }),
    );
    assert.equal(propertyMapTransform.scale, 2.25);
    assert.equal(propertyMapTransform.translateX <= 0, true);
    assert.equal(propertyMapTransform.translateY <= 0, true);
    assert.equal(
      propertyMapTransform.translateX >= propertyMapTransform.width - propertyMapTransform.width * propertyMapTransform.scale,
      true,
    );
    assert.equal(
      propertyMapTransform.translateY >= propertyMapTransform.height - propertyMapTransform.height * propertyMapTransform.scale,
      true,
    );
    const propertyMapBounds = await propertyMap.locator("[data-property-interactive-map]").boundingBox();

    // pan the zoomed map and retain the bounded translation
    if (propertyMapBounds !== null) {
      await page.mouse.move(
        propertyMapBounds.x + propertyMapBounds.width / 2,
        propertyMapBounds.y + propertyMapBounds.height / 2,
      );
      await page.mouse.down();
      await page.mouse.move(
        propertyMapBounds.x + propertyMapBounds.width,
        propertyMapBounds.y + propertyMapBounds.height,
      );
      await page.mouse.up();
    }

    const pannedPropertyMapTransform = await propertyMap.locator("[data-property-interactive-map]").evaluate(
      // retain the original bounds after an extreme pan
      (svg) => ({
        height: Number(svg.dataset.propertyMapHeight),
        scale: Number(svg.dataset.propertyMapScale),
        translateX: Number(svg.dataset.propertyMapTranslateX),
        translateY: Number(svg.dataset.propertyMapTranslateY),
        width: Number(svg.dataset.propertyMapWidth),
      }),
    );
    assert.equal(pannedPropertyMapTransform.translateX <= 0, true);
    assert.equal(pannedPropertyMapTransform.translateY <= 0, true);
    assert.equal(
      pannedPropertyMapTransform.translateX >= pannedPropertyMapTransform.width - pannedPropertyMapTransform.width * pannedPropertyMapTransform.scale,
      true,
    );
    assert.equal(
      pannedPropertyMapTransform.translateY >= pannedPropertyMapTransform.height - pannedPropertyMapTransform.height * pannedPropertyMapTransform.scale,
      true,
    );
    await propertyMap.getByRole("button", { name: "Reset map" }).click();
    assert.equal(
      await propertyMap.locator("[data-property-interactive-map]").getAttribute("data-property-map-scale"),
      "1",
    );
    assert.equal(
      await page.locator(".property-map-panel").evaluate(
        // keep the property map before the public station map
        (propertyMap) => propertyMap.nextElementSibling?.classList.contains("station-map-panel") === true,
      ),
      true,
    );
    assert.equal(await page.getByRole("heading", { name: "Nearby station map" }).isVisible(), true);
    assert.equal(await page.locator(".section-nav-map").getAttribute("aria-current"), "page");
    assert.equal(await page.locator(".current-conditions, .forecast-panel, .trends-panel").count(), 0);
    const map = page.locator(".station-map");
    const sharedMapSurface = await map.locator(".station-map-svg").evaluate((svg) => ({
      markerOwner: svg.querySelector(".station-marker")?.ownerSVGElement === svg,
      overlayOwner: svg.querySelector(".station-map-overlay")?.ownerSVGElement === svg,
      tileOwner: svg.querySelector(".map-tile-layer image")?.ownerSVGElement === svg,
    }));
    assert.deepEqual(sharedMapSurface, {
      markerOwner: true,
      overlayOwner: true,
      tileOwner: true,
    });
    assert.equal(
      await map.locator(".station-marker-hit-target").count(),
      await map.locator(".station-marker").count(),
    );
    const mapStyleControls = map.locator(".map-style-controls");
    assert.equal(
      await mapStyleControls.evaluate(
        // keep the illustrated picker inside the map canvas
        (controls) => controls.parentElement?.classList.contains("station-map-canvas"),
      ),
      true,
    );
    assert.equal(await mapStyleControls.locator("img").count(), 3);
    assert.deepEqual(
      await page.locator(".nearby-station-list").evaluate(
        // keep station navigation in the page scroll
        (list) => {
          const style = getComputedStyle(list);
          return {
            maxHeight: style.maxHeight,
            overflowY: style.overflowY,
          };
        },
      ),
      { maxHeight: "none", overflowY: "visible" },
    );
    assert.match(
      await map.getByRole("button", { name: "Roads" }).locator("img").getAttribute("src") ?? "",
      /tile\.openstreetmap\.org\/13\//u,
    );
    assert.match(
      await map.getByRole("button", { name: "Topo" }).locator("img").getAttribute("src") ?? "",
      /USGSTopo\/MapServer\/tile\/13\//u,
    );
    assert.match(
      await map.getByRole("button", { name: "Satellite" }).locator("img").getAttribute("src") ?? "",
      /USGSImageryOnly\/MapServer\/tile\/13\//u,
    );
    const stationListButton = page.locator('.nearby-station-select[data-station-select="tempest-38270"]');
    const stationMarker = map.locator('.station-marker[data-station-select="tempest-38270"]');
    await stationListButton.hover();
    assert.equal(await stationMarker.evaluate((marker) => marker.classList.contains("related-hover")), true);
    assert.equal(
      await stationMarker.locator(".station-marker-dot").evaluate(
        // expose the corresponding public marker strongly
        (marker) => getComputedStyle(marker).strokeWidth,
      ),
      "4px",
    );
    await stationListButton.click();
    const stationCurrent = page.locator('[data-station-current="tempest-38270"]');
    assert.equal(await stationListButton.getAttribute("aria-expanded"), "true");
    assert.equal(await stationCurrent.isVisible(), true);
    assert.match(await stationCurrent.textContent() ?? "", /Temp 53\.6 °F/u);
    assert.match(await stationCurrent.textContent() ?? "", /Wind 5\.6 mph · gust 10\.1 mph/u);
    assert.match(await stationCurrent.textContent() ?? "", /Platform WeatherFlow Tempest/u);
    assert.equal(await page.locator("[data-station-current]").count(), 1);
    await page.reload({ waitUntil: "networkidle" });
    await map.locator('.station-marker[data-station-select="tempest-38270"]').click();
    assert.equal(await page.locator('[data-station-current="tempest-38270"]').isVisible(), true);
    assert.match(
      await map.locator(".map-tile-layer image").first().getAttribute("href") ?? "",
      /tile\.openstreetmap\.org\/13\//u,
    );
    assert.equal(await map.getByRole("button", { name: "Roads" }).getAttribute("aria-pressed"), "true");
    const topoButton = map.getByRole("button", { name: "Topo" });
    const inactiveTopoGeometry = await topoButton.evaluate(
      // capture the inactive tile footprint
      (button) => {
        const bounds = button.getBoundingClientRect();
        return {
          borderWidth: getComputedStyle(button).borderWidth,
          height: bounds.height,
          width: bounds.width,
        };
      },
    );
    await topoButton.click();
    assert.equal(await topoButton.getAttribute("aria-pressed"), "true");
    assert.deepEqual(
      await topoButton.evaluate(
        // keep active highlighting within the same footprint
        (button) => {
          const bounds = button.getBoundingClientRect();
          return {
            borderWidth: getComputedStyle(button).borderWidth,
            height: bounds.height,
            width: bounds.width,
          };
        },
      ),
      inactiveTopoGeometry,
    );
    assert.match(
      await map.locator(".map-tile-layer image").first().getAttribute("href") ?? "",
      /USGSTopo\/MapServer\/tile\/13\//u,
    );
    assert.equal(await map.getByText(/U\.S\. Geological Survey/u).isVisible(), true);
    await map.getByRole("button", { name: "Satellite" }).click();
    assert.equal(await map.getByRole("button", { name: "Satellite" }).getAttribute("aria-pressed"), "true");
    assert.match(
      await map.locator(".map-tile-layer image").first().getAttribute("href") ?? "",
      /USGSImageryOnly\/MapServer\/tile\/13\//u,
    );
    assert.equal(await page.locator("[data-history-filters]").count(), 0);
    assert.equal(await page.locator("table").count(), 0);
    assert.equal(
      fixture.state.requests.some(
        // count only normalized history API reads
        (entry) => entry.includes("/api/v1/sites/ballydidean/history"),
      ),
      false,
    );
    await page.getByRole("link", { name: "Trends" }).click();
    await page.waitForURL(`${fixture.origin}/trends`);
    await page.waitForLoadState("networkidle");
    await page.locator("[data-trend-display-mode=aggregate][data-trend-detail=rolling]").waitFor();
    assert.equal(await page.getByRole("heading", { name: "Yearly trends" }).count(), 0);
    assert.equal(await page.getByText("Calendar comparison", { exact: true }).count(), 0);
    assert.equal(await page.locator(".section-nav-trends").getAttribute("aria-current"), "page");
    assert.equal(await page.locator(".current-conditions, .forecast-panel, .station-map-panel, [data-history-filters]").count(), 0);
    assert.equal(await page.locator(".trend-chart").count(), 1);
    assert.equal(await page.locator(".trend-month-label").count(), 12);
    assert.equal(await page.locator(".trend-historical-quartile-band").count(), 1);
    assert.equal(await page.locator(".trend-historical-range-line").count(), 2);
    assert.equal(await page.locator(".trend-aggregate-median-line").count(), 1);
    assert.equal(await page.locator(".trend-year-line").count(), 1);
    assert.equal(await page.locator(".trend-year-hit-target").count(), 0);
    assert.equal(await page.locator(".trend-year-line-current").getAttribute("stroke"), "var(--brand-orange)");
    assert.equal(await page.locator(".trend-y-axis span").count(), 5);
    assert.equal(await page.locator("[data-trend-range]").count(), 0);
    assert.equal(await page.locator("[data-trend-display-mode=aggregate]").count(), 1);
    assert.equal(await page.locator("[data-trend-detail=rolling]").count(), 1);
    const trendMetricTrigger = page.locator("[data-trend-metric-trigger]");
    assert.equal(
      await trendMetricTrigger.evaluate(
        // preserve visible space between the title and caret
        (trigger) => Number.parseFloat(getComputedStyle(trigger).columnGap) >= 4,
      ),
      true,
    );
    assert.equal(await page.getByRole("heading", { name: "Temperature" }).count(), 1);
    assert.equal(await page.locator("[data-trend-metric-option]").count(), 15);
    assert.equal(await page.locator(".trend-metric-option-group").count(), 2);
    assert.equal(await page.locator(".trend-metric-flyover").isVisible(), false);
    await trendMetricTrigger.click();
    assert.equal(await trendMetricTrigger.getAttribute("aria-expanded"), "true");
    assert.equal(await page.locator(".trend-metric-flyover").isVisible(), true);
    assert.equal(await page.locator('[data-trend-metric-option="temperatureC"]').getAttribute("aria-checked"), "true");
    await page.keyboard.press("Escape");
    assert.equal(await trendMetricTrigger.getAttribute("aria-expanded"), "false");
    assert.equal(await page.locator('[data-trend-chart="temperatureC"]').count(), 1);
    assert.equal(await page.locator(".trend-chart").getAttribute("data-trend-domain"), "visible");
    assert.equal(await page.locator("[data-trend-crosshair-value=median]").count(), 1);
    assert.equal(await page.locator('[data-trend-crosshair-value="2026"]').count(), 1);
    assert.match(await page.locator("[data-trend-crosshair-date]").textContent() ?? "", /^(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) \d{1,2}$/u);
    const farmCharts = [
      ["cumulativePrecipitationMm", "Cumulative rainfall"],
      ["temperatureAnomalyC", "Temperature anomaly"],
      ["temperatureRangeC", "Daily temperature range"],
      ["drySpellDays", "Dry spell length"],
      ["growingDegreeDaysC", "Accumulated growing heat"],
      ["frostDayCount", "Frost days"],
      ["extremeDayCount", "Extreme day counts"],
      ["windDirectionRose", "Wind direction rose"],
    ];

    // select and render every added farm chart without another API read
    for (const [metric, label] of farmCharts) {
      await trendMetricTrigger.click();
      await page.locator(`[data-trend-metric-option="${metric}"]`).click();
      await page.locator(`[data-trend-chart="${metric}"]`).waitFor();
      assert.equal(await page.getByRole("heading", { name: label }).count(), 1);

      // distinguish accumulated heat from a count of calendar days
      if (metric === "growingDegreeDaysC") {
        assert.match(await page.locator(".trend-chart-range").innerText(), / GDD$/u);
        assert.match(await page.locator(".trend-insight-note").innerText(), /Base 50 °F/u);
        // retain the heat-unit label on every vertical tick
        for (const label of await page.locator(".trend-y-axis span").allTextContents()) {
          assert.match(label, /^[\d,]+ GDD$/u);
        }
        assert.match(await page.locator('[data-trend-crosshair-value="2026"]').innerText(), / GDD$/u);
      }

      // verify the polar chart's distinct accessible surface
      if (metric === "windDirectionRose") {
        assert.equal(await page.locator(".trend-wind-rose").count(), 1);
        assert.equal(await page.locator("[data-wind-rose-sector]").count() > 0, true);
        assert.equal(await page.locator('[aria-label="Wind direction percentages"] li').count(), 16);
      } else {
        // retain the shared annual line and scrubber contract
        assert.equal(await page.locator(".trend-year-line-current").count(), 1);
        assert.equal(await page.locator("[data-trend-crosshair-value]").count() >= 2, true);
      }
    }

    await trendMetricTrigger.click();
    await page.locator('[data-trend-metric-option="extremeDayCount"]').click();
    await page.locator('[data-trend-chart="extremeDayCount"]').waitFor();
    assert.equal(await page.locator("[data-trend-detail-toggle]").count(), 0);
    assert.equal(await page.locator("[data-trend-extreme-kind]").inputValue(), "heat");
    assert.equal(await page.locator("[data-trend-extreme-threshold]").inputValue(), "86.0");
    await page.locator("[data-trend-extreme-threshold]").fill("80");
    await page.locator("[data-trend-extreme-threshold]").dispatchEvent("change");
    assert.equal(await page.locator("[data-trend-extreme-threshold]").inputValue(), "80.0");
    await page.locator("[data-trend-extreme-kind]").selectOption("rain");
    assert.equal(await page.locator("[data-trend-extreme-kind]").inputValue(), "rain");
    assert.equal(await page.locator("[data-trend-extreme-threshold]").inputValue(), "1.0");
    await trendMetricTrigger.click();
    await page.locator('[data-trend-metric-option="temperatureC"]').click();
    await page.locator('[data-trend-chart="temperatureC"]').waitFor();
    assert.deepEqual(
      await page.locator(".trend-chart").evaluate(
        // capture the aggregate viewport-fitted chart contract
        (chart) => {
          const band = chart.querySelector(".trend-historical-quartile-band");
          const landscape = chart.querySelector(".trend-chart-landscape");
          const legend = chart.querySelector(".trend-chart-legend");
          const median = chart.querySelector(".trend-aggregate-median-line");
          const detailToggle = chart.querySelector("[data-trend-detail-toggle]");
          const modeToggle = chart.querySelector("[data-trend-mode-toggle]");
          const ranges = [...chart.querySelectorAll(".trend-historical-range-line")];
          const title = chart.querySelector(".trend-chart-title");
          const titleControl = title?.closest(".trend-metric-control");
          const svg = chart.querySelector(".trend-chart-landscape > svg");
          const viewport = chart.querySelector(".trend-chart-viewport");
          const xAxis = chart.querySelector(".trend-chart-axis");
          const crosshair = chart.querySelector(".trend-crosshair-line");
          const crosshairSummary = chart.querySelector(".trend-crosshair-summary");
          const crosshairValueFlags = chart.querySelector(".trend-crosshair-values");
          const crosshairDateFlag = chart.querySelector("[data-trend-crosshair-date]");
          const currentYearLine = chart.querySelector(".trend-year-line-current");
          const todayMarker = chart.querySelector(".trend-today-marker");

          // require the complete aggregate chart surface
          if (
            band === null ||
            crosshair === null ||
            crosshairDateFlag === null ||
            crosshairSummary === null ||
            crosshairValueFlags === null ||
            currentYearLine === null ||
            detailToggle === null ||
            landscape === null ||
            legend === null ||
            median === null ||
            modeToggle === null ||
            ranges.length !== 2 ||
            title === null ||
            titleControl === null ||
            svg === null ||
            todayMarker === null ||
            viewport === null ||
            xAxis === null
          ) {
            throw new Error("aggregate trend chart is incomplete");
          }

          const landscapeBounds = landscape.getBoundingClientRect();
          const detailToggleBounds = detailToggle.getBoundingClientRect();
          const legendBounds = legend.getBoundingClientRect();
          const titleControlBounds = titleControl.getBoundingClientRect();
          const svgBounds = svg.getBoundingClientRect();
          const viewportBounds = viewport.getBoundingClientRect();
          const xAxisBounds = xAxis.getBoundingClientRect();
          const crosshairBounds = crosshair.getBoundingClientRect();
          const crosshairDateBounds = crosshairDateFlag.getBoundingClientRect();
          const currentYearPoint = currentYearLine.points.getItem(currentYearLine.points.numberOfItems - 1);
          const currentYearY = (currentYearPoint.y / 280) * landscape.clientHeight;
          const summaryValuesStyle = getComputedStyle(crosshairValueFlags);
          const summaryOnLeft = crosshairSummary.classList.contains("trend-crosshair-summary-left");
          return {
            bandOpacity: Number.parseFloat(getComputedStyle(band).opacity),
            crosshairFlagsAttached: Math.abs(
              Number.parseFloat(getComputedStyle(crosshairSummary).left) -
              Number.parseFloat(getComputedStyle(crosshair).left),
            ) < 1,
            crosshairFlagsBlack: getComputedStyle(crosshairValueFlags).backgroundColor ===
              getComputedStyle(crosshairDateFlag).backgroundColor,
            crosshairFlagsInsidePlot: crosshairSummary.offsetTop >= landscape.clientHeight * (42 / 280) - 1 &&
              crosshairSummary.offsetTop + crosshairSummary.offsetHeight <=
                landscape.clientHeight * ((280 - 34) / 280) + 1,
            crosshairFlagsMatchCurrentYear: Math.abs(
              crosshairSummary.offsetTop + crosshairSummary.offsetHeight / 2 - currentYearY,
            ) < 2,
            crosshairFlagsSquareAgainstLine: summaryOnLeft
              ? summaryValuesStyle.borderTopRightRadius === "0px" &&
                summaryValuesStyle.borderBottomRightRadius === "0px"
              : summaryValuesStyle.borderTopLeftRadius === "0px" &&
                summaryValuesStyle.borderBottomLeftRadius === "0px",
            crosshairFlagsTopIsDynamic: crosshairSummary.style.top.endsWith("px"),
            crosshairDateAboveLine: crosshairDateBounds.bottom < crosshairBounds.top,
            crosshairDateCentered: Math.abs(
              crosshairDateBounds.left + crosshairDateBounds.width / 2 -
              (crosshairBounds.left + crosshairBounds.width / 2),
            ) < 1,
            currentYearMatchesToday: getComputedStyle(currentYearLine).stroke ===
              getComputedStyle(todayMarker).borderLeftColor,
            currentYearUsesDarkOrange: getComputedStyle(currentYearLine).stroke === "rgb(239, 126, 31)",
            detailIcon: detailToggle.querySelector("[data-trend-toggle-icon]")?.getAttribute("data-trend-toggle-icon"),
            detailTopLeftOfTitle: detailToggleBounds.right < titleControlBounds.left &&
              Math.abs(detailToggleBounds.top - titleControlBounds.top) < 2,
            legendCentered: Math.abs(
              legendBounds.left + legendBounds.width / 2 -
              (landscapeBounds.left + landscapeBounds.width / 2),
            ) < 2,
            legendPlacement: legend.dataset.trendLegendPlacement,
            legendPlacementMatchesEdge: legend.dataset.trendLegendPlacement === "top"
              ? Number.parseFloat(getComputedStyle(legend).top) > landscape.clientHeight * (42 / 280)
              : legendBounds.bottom <= xAxisBounds.top + 1,
            medianWidth: Number.parseFloat(getComputedStyle(median).strokeWidth),
            minimumSize: landscapeBounds.width >= viewportBounds.width - 1 &&
              landscape.clientWidth >= viewport.clientWidth,
            modeLabel: chart.querySelector("[data-trend-mode-toggle]")?.textContent,
            modeIcon: modeToggle.querySelector("[data-trend-toggle-icon]")?.getAttribute("data-trend-toggle-icon"),
            monthLabels: [...chart.querySelectorAll(".trend-month-label")].map(
              // read one month label
              (label) => label.textContent,
            ),
            preserveAspectRatio: svg.getAttribute("preserveAspectRatio"),
            rangeDashes: ranges.map(
              // capture each historical range pattern
              (line) => getComputedStyle(line).strokeDasharray,
            ),
            titleTag: title.tagName,
            titleTopCentered: Math.abs(
              titleControlBounds.left + titleControlBounds.width / 2 -
              (landscapeBounds.left + landscapeBounds.width / 2),
            ) < 2 && title.closest(".trend-chart-landscape") === landscape,
            svgFitsViewport: Math.abs(svgBounds.width - viewportBounds.width) < 1 &&
              Math.abs(svgBounds.height - viewportBounds.height) < 1,
            todayLabel: todayMarker.textContent,
            todayPositionInsideChart: Number.parseFloat(getComputedStyle(todayMarker).left) > 0 &&
              Number.parseFloat(getComputedStyle(todayMarker).left) < landscape.clientWidth,
          };
        },
      ),
      {
        bandOpacity: 1,
        crosshairFlagsAttached: true,
        crosshairFlagsBlack: true,
        crosshairFlagsInsidePlot: true,
        crosshairFlagsMatchCurrentYear: true,
        crosshairFlagsSquareAgainstLine: true,
        crosshairFlagsTopIsDynamic: true,
        crosshairDateAboveLine: true,
        crosshairDateCentered: true,
        currentYearMatchesToday: true,
        currentYearUsesDarkOrange: true,
        detailIcon: "daily",
        detailTopLeftOfTitle: true,
        legendCentered: true,
        legendPlacement: "bottom",
        legendPlacementMatchesEdge: true,
        medianWidth: 2,
        minimumSize: true,
        modeLabel: "Show all",
        modeIcon: "show-all",
        monthLabels: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
        preserveAspectRatio: "none",
        rangeDashes: ["3px, 4px", "3px, 4px"],
        titleTag: "H2",
        titleTopCentered: true,
        svgFitsViewport: true,
        todayLabel: "Today",
        todayPositionInsideChart: true,
      },
    );
    await trendMetricTrigger.click();
    await page.locator('[data-trend-metric-option="precipitationMm"]').click();
    await page.locator('[data-trend-chart="precipitationMm"]').waitFor();
    assert.equal(
      await page.locator(".trend-chart-legend").getAttribute("data-trend-legend-placement"),
      "top",
    );
    await page.locator("[data-trend-metric-trigger]").click();
    await page.locator('[data-trend-metric-option="relativeHumidityPercent"]').click();
    await page.locator('[data-trend-chart="relativeHumidityPercent"]').waitFor();
    assert.equal(
      await page.locator(".trend-chart-legend").getAttribute("data-trend-legend-placement"),
      "bottom",
    );
    await page.locator("[data-trend-metric-trigger]").click();
    await page.locator('[data-trend-metric-option="temperatureC"]').click();
    await page.locator('[data-trend-chart="temperatureC"]').waitFor();
    const trendRequestsBeforeSelection = fixture.state.requests.filter(
      // count the calendar endpoint before local chart changes
      (entry) => entry === "GET /api/v1/sites/ballydidean/trends",
    ).length;
    const trendDateBeforeScrub = await page.locator("[data-trend-crosshair-date]").textContent();
    const trendSurfaceBounds = await page.locator("[data-trend-scrub-surface]").boundingBox();

    // require one measurable annual scrub surface
    if (trendSurfaceBounds === null) {
      throw new Error("trend scrub surface is unavailable");
    }

    await page.mouse.move(
      trendSurfaceBounds.x + trendSurfaceBounds.width * 0.8,
      trendSurfaceBounds.y + trendSurfaceBounds.height * 0.5,
    );
    await page.mouse.down();
    await page.mouse.move(
      trendSurfaceBounds.x + trendSurfaceBounds.width * 0.28,
      trendSurfaceBounds.y + trendSurfaceBounds.height * 0.5,
      { steps: 4 },
    );
    await page.mouse.up();
    assert.notEqual(await page.locator("[data-trend-crosshair-date]").textContent(), trendDateBeforeScrub);
    await page.getByRole("button", { name: "Show all" }).click();
    await page.locator("[data-trend-display-mode=all]").waitFor();
    assert.equal(await page.locator('[data-trend-toggle-icon="aggregate"]').count(), 1);
    assert.equal(await page.locator(".trend-aggregate-median-line").count(), 0);
    assert.equal(await page.locator(".trend-year-line").count(), 3);
    assert.equal(await page.locator(".trend-year-hit-target").count(), 3);
    assert.equal(await page.locator(".trend-year-line-current").count(), 1);
    assert.equal(await page.locator("button.trend-year-legend").count(), 3);
    assert.equal(await page.locator("[data-trend-crosshair-value]").count(), 3);
    await page.locator('button.trend-year-legend[data-trend-year-select="2019"]').click();
    await page.locator('[data-selected-trend-year="2019"]').waitFor();
    assert.deepEqual(
      await page.locator(".trend-chart").evaluate(
        // verify one selected year fully hides every other line
        (chart) => {
          const selected = chart.querySelector(".trend-year-line-selected");
          const current = chart.querySelector(".trend-year-line-current");
          const hidden = [...chart.querySelectorAll(".trend-year-line:not(.trend-year-line-selected):not(.trend-year-line-current)")];

          // require one selected calendar year
          if (selected === null || current === null) {
            throw new Error("selected trend year is unavailable");
          }

          return {
            crosshairValues: chart.querySelectorAll("[data-trend-crosshair-value]").length,
            currentOpacity: Number.parseFloat(getComputedStyle(current).opacity),
            currentStroke: current.getAttribute("stroke"),
            hiddenOpacities: hidden.map(
              // capture one hidden yearly opacity
              (line) => Number.parseFloat(getComputedStyle(line).opacity),
            ),
            selectedOpacity: Number.parseFloat(getComputedStyle(selected).opacity),
            selectedYear: selected.getAttribute("data-trend-year"),
          };
        },
      ),
      {
        crosshairValues: 2,
        currentOpacity: 1,
        currentStroke: "var(--brand-orange)",
        hiddenOpacities: [0],
        selectedOpacity: 1,
        selectedYear: "2019",
      },
    );
    await page.locator('button.trend-year-legend[data-trend-year-select="2019"]').click();
    await page.locator("[data-selected-trend-year]").waitFor({ state: "detached" });
    const initialTrendGeometry = await page.locator(".trend-chart").evaluate(
      // capture the fixed overview footprint
      (chart) => {
        const landscape = chart.querySelector(".trend-chart-landscape");
        const viewport = chart.querySelector(".trend-chart-viewport");

        // require the daily-detail containers
        if (!(landscape instanceof HTMLElement) || !(viewport instanceof HTMLElement)) {
          throw new Error("trend detail geometry is incomplete");
        }

        return {
          height: landscape.clientHeight,
          viewportWidth: viewport.clientWidth,
          width: landscape.clientWidth,
        };
      },
    );
    await page.getByRole("button", { name: "Daily detail" }).click();
    await page.locator("[data-trend-detail=daily]").waitFor();
    assert.equal(await page.locator('[data-trend-toggle-icon="rolling"]').count(), 1);
    const dailyTrendGeometry = await page.locator(".trend-chart").evaluate(
      // verify the one fixed daily-detail canvas
      (chart) => {
        const landscape = chart.querySelector(".trend-chart-landscape");
        const viewport = chart.querySelector(".trend-chart-viewport");

        // require the expanded daily canvas
        if (!(landscape instanceof HTMLElement) || !(viewport instanceof HTMLElement)) {
          throw new Error("daily trend geometry is incomplete");
        }

        return {
          height: landscape.clientHeight,
          scrollable: viewport.scrollWidth > viewport.clientWidth,
          width: landscape.clientWidth,
        };
      },
    );
    assert.equal(dailyTrendGeometry.height, initialTrendGeometry.height);
    assert.equal(dailyTrendGeometry.scrollable, true);
    assert.equal(dailyTrendGeometry.width, 3000);
    assert.deepEqual(
      await page.locator(".trend-chart").evaluate(
        // keep chart chrome fixed while the daily data canvas moves
        (chart) => {
          const legend = chart.querySelector(".trend-chart-legend");
          const svg = chart.querySelector(".trend-chart-landscape > svg");
          const title = chart.querySelector(".trend-metric-control");
          const viewport = chart.querySelector(".trend-chart-viewport");
          const xAxis = chart.querySelector(".trend-month-axis");

          // require both fixed chrome and scrollable data
          if (
            !(legend instanceof HTMLElement) ||
            !(svg instanceof SVGElement) ||
            !(title instanceof HTMLElement) ||
            !(viewport instanceof HTMLElement) ||
            !(xAxis instanceof HTMLElement)
          ) {
            throw new Error("daily trend scroll layers are incomplete");
          }

          viewport.scrollLeft = 0;
          const viewportBounds = viewport.getBoundingClientRect();
          const initial = {
            legend: legend.getBoundingClientRect().left,
            plot: svg.getBoundingClientRect().left,
            title: title.getBoundingClientRect().left,
            xAxis: xAxis.getBoundingClientRect().left,
          };
          viewport.scrollLeft = 500;
          const legendBounds = legend.getBoundingClientRect();
          const titleBounds = title.getBoundingClientRect();
          return {
            legendFixed: Math.abs(legend.getBoundingClientRect().left - initial.legend) < 1,
            legendVisible: legendBounds.left >= viewportBounds.left - 1 &&
              legendBounds.right <= viewportBounds.right + 1,
            plotMoved: svg.getBoundingClientRect().left < initial.plot - 499,
            titleFixed: Math.abs(title.getBoundingClientRect().left - initial.title) < 1,
            titleVisible: titleBounds.left >= viewportBounds.left - 1 &&
              titleBounds.right <= viewportBounds.right + 1,
            xAxisMoved: xAxis.getBoundingClientRect().left < initial.xAxis - 499,
          };
        },
      ),
      {
        legendFixed: true,
        legendVisible: true,
        plotMoved: true,
        titleFixed: true,
        titleVisible: true,
        xAxisMoved: true,
      },
    );
    assert.equal(await page.locator("[data-trend-zoom]").count(), 0);
    await page.locator("[data-trend-scrub-surface]").evaluate(
      // simulate a two-finger gesture that must not resize the fixed canvas
      (surface) => {
        const bounds = surface.getBoundingClientRect();
        const emit = (type, pointerId, clientX) => surface.dispatchEvent(new PointerEvent(type, {
          bubbles: true,
          buttons: type === "pointerup" ? 0 : 1,
          clientX,
          clientY: bounds.top + bounds.height * 0.55,
          pointerId,
          pointerType: "touch",
        }));
        emit("pointerdown", 41, bounds.left + bounds.width * 0.4);
        emit("pointerdown", 42, bounds.left + bounds.width * 0.6);
        emit("pointermove", 42, bounds.left + bounds.width * 0.8);
        emit("pointerup", 42, bounds.left + bounds.width * 0.8);
        emit("pointerup", 41, bounds.left + bounds.width * 0.4);
      },
    );
    assert.equal(await page.locator(".trend-chart-landscape").evaluate((landscape) => landscape.clientWidth), 3000);
    await page.locator(".trend-chart-viewport").evaluate(
      // reset native scroll before wheel panning
      (viewport) => {
        viewport.scrollLeft = 0;
      },
    );
    await page.locator("[data-trend-scrub-surface]").dispatchEvent("wheel", {
      clientX: trendSurfaceBounds.x + trendSurfaceBounds.width / 2,
      clientY: trendSurfaceBounds.y + trendSurfaceBounds.height / 2,
      deltaY: 120,
    });
    assert.equal(await page.locator(".trend-chart-viewport").evaluate((viewport) => viewport.scrollLeft > 0), true);
    await page.getByRole("button", { name: "7-day average" }).click();
    await page.locator("[data-trend-detail=rolling]").waitFor();
    assert.deepEqual(
      await page.locator(".trend-chart-landscape").evaluate(
        // preserve the initial chart floor after collapsing detail
        (landscape) => ({
          height: landscape.clientHeight,
          widthAtLeastInitial: landscape.clientWidth >= landscape.closest(".trend-chart-viewport").clientWidth,
        }),
      ),
      { height: initialTrendGeometry.height, widthAtLeastInitial: true },
    );
    await trendMetricTrigger.click();
    await page.locator('[data-trend-metric-option="windGustMps"]').click();
    await page.locator('[data-trend-chart="windGustMps"]').waitFor();
    assert.equal(await page.locator(".trend-chart").count(), 1);
    assert.equal(await page.locator('[data-trend-chart="temperatureC"]').count(), 0);
    assert.equal(await page.locator("[data-selected-trend-year]").count(), 0);
    assert.equal(await page.getByRole("heading", { name: "Wind gust" }).count(), 1);
    assert.equal(await page.locator('[data-trend-metric-option="windGustMps"]').getAttribute("aria-checked"), "true");
    assert.equal(await page.locator("[data-trend-crosshair-value]").count(), 3);
    await page.getByRole("button", { name: "Aggregate" }).click();
    await page.locator("[data-trend-display-mode=aggregate]").waitFor();
    assert.equal(await page.locator(".trend-year-line").count(), 1);
    assert.equal(await page.locator(".trend-year-line-current").getAttribute("stroke"), "var(--brand-orange)");
    assert.equal(await page.locator(".trend-aggregate-median-line").count(), 1);
    assert.equal(await page.locator("[data-trend-crosshair-value=median]").count(), 1);
    assert.equal(await page.locator("[data-trend-crosshair-value]").count(), 2);
    assert.equal(
      fixture.state.requests.filter(
        // keep every trend control entirely client-side
        (entry) => entry === "GET /api/v1/sites/ballydidean/trends",
      ).length,
      trendRequestsBeforeSelection,
    );
    assert.ok(
      fixture.state.requests.some(
        // require the parameter-free calendar endpoint
        (entry) => entry === "GET /api/v1/sites/ballydidean/trends",
      ),
    );
    await page.getByRole("link", { name: "Settings" }).click();
    await page.waitForURL(`${fixture.origin}/settings`);
    await page.getByRole("link", { name: "Logs", exact: true }).click();
    await page.waitForURL(`${fixture.origin}/logs`);
    await page.waitForLoadState("networkidle");
    await page.locator("table caption").waitFor();
    assert.equal(await page.locator("#current-heading").count(), 0);
    assert.equal(
      await page.locator(".source-kind").getByText("model-derived current conditions").isVisible(),
      true,
    );
    const credits = page.locator(".credits details");
    const projectCredit = page.locator(".project-credit");
    const projectCreditLink = projectCredit.getByRole("link", {
      exact: true,
      name: "Ballydidean Farm Sanctuary",
    });
    assert.equal(await credits.getAttribute("open"), null);
    assert.equal(await projectCredit.count(), 1);
    assert.equal(await projectCredit.isVisible(), true);
    assert.equal(await projectCredit.innerText(), "Built with love by Ballydidean Farm Sanctuary");
    assert.equal(await projectCreditLink.getAttribute("href"), "https://ballydidean.farm");
    assert.equal(await projectCreditLink.getAttribute("rel"), "noreferrer");
    assert.equal(await page.getByRole("link", { name: "Weather data by Open-Meteo" }).isVisible(), false);
    assert.equal(await page.getByRole("link", { name: "CC BY 4.0" }).isVisible(), false);
    await credits.getByText("Data sources & credits", { exact: true }).click();
    assert.equal(await credits.getAttribute("open"), "");
    assert.equal(await projectCredit.isVisible(), true);
    assert.equal(await page.getByRole("link", { name: "Weather data by Open-Meteo" }).isVisible(), true);
    assert.equal(await page.getByRole("link", { name: "CC BY 4.0" }).isVisible(), true);
    assert.equal(await page.locator(".masthead img").count(), 0);
    assert.equal(await page.locator("[data-site-selector]").count(), 0);
    assert.equal(await page.locator("table caption").count(), 1);
    const currentReadCount = fixture.state.requests.filter(
      // count homepage current reads
      (entry) => entry.includes("/current"),
    ).length;

    await page.locator("select[name='stationSlug']").selectOption("open-meteo-virtual");
    await page.locator("select[name='sourceId']").selectOption("11");
    await page.locator("select[name='sourceKind']").selectOption("reanalysis");
    await page.locator("input[name='from']").fill("2026-08-21T21:30");
    await page.locator("input[name='to']").fill("2026-08-21T22:30");
    const filteredHistoryResponse = page.waitForResponse(
      // await the submitted SPA request rather than the document load state
      (response) =>
        new URL(response.url()).pathname ===
        "/api/v1/sites/ballydidean/history",
    );
    await page.getByRole("button", { name: "Apply filters" }).click();
    await filteredHistoryResponse;
    assert.equal(
      fixture.state.requests.filter((entry) => entry.includes("/current")).length,
      currentReadCount,
    );
    assert.ok(
      fixture.state.requests.some(
        // require frozen history filter names
        (entry) =>
          entry.includes("/history?") &&
          entry.includes("sourceKind=reanalysis") &&
          entry.includes("from=2026-08-22T04%3A30%3A00.000Z") &&
          entry.includes("to=2026-08-22T05%3A30%3A00.000Z"),
      ),
    );

    await page.getByRole("button", { name: "Next" }).click();
    await page.getByText("59.2").first().waitFor();
    assert.equal(await page.getByRole("button", { name: "Next" }).isDisabled(), true);
    await page.getByRole("button", { name: "Previous" }).click();
    await page.getByText("61.2").first().waitFor();

    fixture.state.failReads = true;
    await page.getByRole("button", { name: "Apply filters" }).click();
    await page.getByRole("alert").waitFor();
    assert.match(await page.getByRole("alert").textContent() ?? "", /status 503/u);
    assert.equal(await page.getByText("61.2").first().isVisible(), true);
    assert.equal(await credits.getAttribute("open"), null);
    assert.equal(await page.getByRole("link", { name: "CC BY 4.0" }).isVisible(), false);

    fixture.state.failReads = false;
    await page.getByRole("button", { name: "Apply filters" }).click();
    await page.getByText("Weather data is up to date.").waitFor();
    assert.equal(await page.getByRole("alert").count(), 0);

    const mutationStatus = await page.evaluate(async () => {
      const response = await fetch("/api/v1/sites", { method: "POST" });
      return response.status;
    });
    assert.equal(mutationStatus, 405);
    assert.equal(fixture.state.mutations, 1);
    assert.equal(await page.getByText("61.2").first().isVisible(), true);

    await page.locator("body").press("Home");
    await page.keyboard.press("Tab");
    assert.equal(await page.evaluate(() => document.activeElement?.tagName), "A");
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("href")), "/");
    await page.keyboard.press("Tab");
    assert.equal(await page.evaluate(() => document.activeElement?.tagName), "A");
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("href")), "/forecast");
    await page.keyboard.press("Tab");
    assert.equal(await page.evaluate(() => document.activeElement?.tagName), "A");
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("href")), "/trends");
    await page.keyboard.press("Tab");
    assert.equal(await page.evaluate(() => document.activeElement?.tagName), "A");
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("href")), "/map");
    await page.keyboard.press("Tab");
    assert.equal(await page.evaluate(() => document.activeElement?.tagName), "A");
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("href")), "/settings");
    await page.keyboard.press("Tab");
    assert.equal(await page.evaluate(() => document.activeElement?.tagName), "SUMMARY");
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// verify local-network panels never grant administrator authority
test("anonymous home-network viewers see indoor and soil panels only while allowed", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 844, width: 390 },
    });
    fixture.state.viewerContext = { data: { homeNetwork: true } };
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    await page.locator("[data-indoor-house]").waitFor();
    await page.locator("[data-admin-soil-map]").waitFor();
    assert.equal(await page.locator("html").getAttribute("data-weather-admin"), "false");
    assert.equal(await page.locator('[data-soil-moisture-sensor="soil-1"]').count(), 1);
    assert.equal(fixture.state.requests.filter((entry) => entry === "GET /api/v1/sites/ballydidean/property-sensor-layout").length, 1);
    assert.equal(fixture.state.requests.some((entry) => entry.includes("/api/v1/admin/")), false);

    const currentReads = fixture.state.requests.filter(
      // count current-only visibility refreshes independently
      (entry) => entry === "GET /api/v1/sites/ballydidean/current",
    ).length;
    const otherWeatherReads = fixture.state.requests.filter(
      // exclude current polling and optional layout reads
      (entry) => entry.startsWith("GET /api/v1/sites/ballydidean/") &&
        entry !== "GET /api/v1/sites/ballydidean/current" &&
        !entry.endsWith("/property-sensor-layout"),
    ).length;
    fixture.state.viewerContext = { data: { homeNetwork: false } };
    const denied = page.waitForResponse(
      // await the uncached location claim before changing the fixture again
      (response) => new URL(response.url()).pathname === "/api/v1/viewer-context",
    );
    const deniedCurrent = page.waitForResponse(
      // await the independent current-only resume read
      (response) => new URL(response.url()).pathname === "/api/v1/sites/ballydidean/current",
    );
    await page.evaluate(
      // simulate a tab resuming away from home Wi-Fi
      () => document.dispatchEvent(new Event("visibilitychange")),
    );
    await Promise.all([denied, deniedCurrent]);
    await page.locator("[data-indoor-house]").waitFor({ state: "detached" });
    assert.equal(await page.locator("[data-admin-soil-map]").count(), 0);
    assert.equal(fixture.state.requests.filter(
      // refresh current sunlight inputs on foreground resume
      (entry) => entry === "GET /api/v1/sites/ballydidean/current",
    ).length > currentReads, true);
    assert.equal(fixture.state.requests.filter(
      // avoid reloading forecasts tides or daily products
      (entry) => entry.startsWith("GET /api/v1/sites/ballydidean/") &&
        entry !== "GET /api/v1/sites/ballydidean/current" &&
        !entry.endsWith("/property-sensor-layout"),
    ).length, otherWeatherReads);
    assert.equal(fixture.state.requests.filter((entry) => entry === "GET /api/v1/sites/ballydidean/property-sensor-layout").length, 1);

    fixture.state.viewerContext = { data: { homeNetwork: true } };
    const allowed = page.waitForResponse(
      // await the renewed location claim
      (response) => new URL(response.url()).pathname === "/api/v1/viewer-context",
    );
    await page.evaluate(
      // simulate the browser regaining the home connection
      () => window.dispatchEvent(new Event("online")),
    );
    await allowed;
    await page.locator("[data-indoor-house]").waitFor();
    fixture.state.viewerContext = { data: { homeNetwork: "true" } };
    const malformed = page.waitForResponse(
      // await the malformed location claim
      (response) => new URL(response.url()).pathname === "/api/v1/viewer-context",
    );
    await page.evaluate(
      // reject a malformed positive claim during the next check
      () => document.dispatchEvent(new Event("visibilitychange")),
    );
    await malformed;
    await page.locator("[data-indoor-house]").waitFor({ state: "detached" });

    fixture.state.viewerContext = { data: { homeNetwork: true } };
    fixture.state.viewerContextStatus = 503;
    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await page.locator("[data-indoor-house]").count(), 0);
    fixture.state.viewerContextStatus = 200;
    await page.reload({ waitUntil: "networkidle" });
    await page.locator("[data-admin-soil-map]").waitFor();
    await page.getByRole("link", { name: "Forecast" }).click();
    await page.waitForURL(`${fixture.origin}/forecast`);
    assert.equal(await page.locator("[data-indoor-house]").count(), 0);
    fixture.state.viewerContext = { data: { homeNetwork: false } };
    await page.getByRole("link", { name: "Now" }).click();
    await page.waitForURL(fixture.origin + "/");
    assert.equal(await page.locator("[data-indoor-house]").count(), 0);

    await page.goto(`${fixture.origin}/admin`, { waitUntil: "networkidle" });
    assert.equal(await page.getByRole("heading", { name: "Admin sign in" }).isVisible(), true);
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// apply outdoor temperature tones to authorized indoor readings without coupling them to display units
test("indoor temperatures use discrete comfort colors across loading, units, and missing readings", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 320 },
    });
    fixture.state.viewerContext = { data: { homeNetwork: true } };
    let releaseInitialCurrent;
    const initialCurrentReleased = new Promise(
      // expose the neutral indoor loading state before current readings arrive
      (resolveRelease) => {
        releaseInitialCurrent = resolveRelease;
      },
    );
    let holdInitialCurrent = true;
    let indoorTemperatures = new Map([
      ["gateway", 12],
      ["temperature-1", 20],
      ["temperature-2", 24],
    ]);
    await page.route(/\/api\/v1\/sites\/ballydidean\/current$/u, async (route) => {
      // hold only the first current read behind the visible house skeleton
      if (holdInitialCurrent) {
        await initialCurrentReleased;
      }

      const response = await route.fetch();
      const body = await response.json();
      body.data = body.data.map(
        // replace only fixture property-sensor temperatures
        (record) => record.provenance.sourceKind === "physical_sensor"
          ? {
              ...record,
              metadata: {
                ...record.metadata,
                provider: {
                  ...record.metadata.provider,
                  propertySensors: record.metadata.provider.propertySensors.map(
                    // retain every unrelated sensor reading
                    (sensor) => indoorTemperatures.has(sensor.key)
                      ? {
                          ...sensor,
                          readings: {
                            ...sensor.readings,
                            temperatureC: indoorTemperatures.get(sensor.key),
                          },
                        }
                      : sensor,
                  ),
                },
              },
            }
          : record,
      );
      await route.fulfill({ json: body, response });
    });
    const readIndoor = async () => await page.evaluate(
      // capture the current attached house and its painted tones atomically
      () => {
        const house = document.querySelector("[data-indoor-house]");

        // reject missing or detached render snapshots
        if (!(house instanceof HTMLElement) || !house.isConnected) {
          throw new Error("indoor house is unavailable");
        }

        return {
          readings: [...house.querySelectorAll(".indoor-house-temperature")].map(
            // inspect one rendered reading
            (reading) => ({
              color: getComputedStyle(reading).color,
              tone: [...reading.classList].find((className) => className.startsWith("condition-forecast-tone-")),
            }),
          ),
          text: [...house.querySelectorAll(".indoor-house-level")].map(
            // retain the physical floor order
            (level) => level.textContent,
          ),
        };
      },
    );

    await page.goto(fixture.origin, { waitUntil: "domcontentloaded" });
    const house = page.locator("[data-indoor-house]");
    await house.waitFor();
    assert.equal(await house.getAttribute("aria-busy"), "true");
    assert.deepEqual((await readIndoor()).readings, Array.from({ length: 3 }, () => ({
      color: "rgb(0, 0, 0)",
      tone: "condition-forecast-tone-neutral",
    })));
    assert.equal(await house.locator(".skeleton-temperature").count(), 3);

    holdInitialCurrent = false;
    releaseInitialCurrent();
    await page.locator("[data-indoor-house][aria-busy='false']").waitFor();
    assert.equal(await page.locator("html").getAttribute("data-weather-admin"), "false");
    assert.deepEqual(await readIndoor(), {
      readings: [
        { color: "rgb(56, 120, 197)", tone: "condition-forecast-tone-blue" },
        { color: "rgb(67, 151, 86)", tone: "condition-forecast-tone-green" },
        { color: "rgb(239, 126, 31)", tone: "condition-forecast-tone-orange" },
      ],
      text: ["Second floor54°F", "First floor68°F", "Basement75°F"],
    });
    assert.ok((await house.screenshot()).byteLength > 500);

    await page.getByRole("link", { name: "Settings" }).click();
    await page.waitForURL(`${fixture.origin}/settings`);
    assert.equal(await page.locator("[data-indoor-house]").count(), 0);
    const settings = page.getByRole("region", { name: "Measurement units" });
    await settings.locator("select[name='temperature']").selectOption("celsius");
    await settings.getByRole("button", { name: "Save units" }).click();
    await page.getByRole("link", { name: "Now" }).click();
    await page.waitForURL(`${fixture.origin}/`);
    await page.locator("[data-indoor-house][aria-busy='false']").waitFor();
    assert.deepEqual(await readIndoor(), {
      readings: [
        { color: "rgb(56, 120, 197)", tone: "condition-forecast-tone-blue" },
        { color: "rgb(67, 151, 86)", tone: "condition-forecast-tone-green" },
        { color: "rgb(239, 126, 31)", tone: "condition-forecast-tone-orange" },
      ],
      text: ["Second floor12°C", "First floor20°C", "Basement24°C"],
    });
    await page.setViewportSize({ height: 900, width: 1280 });
    assert.ok((await house.screenshot()).byteLength > 500);

    indoorTemperatures = new Map([
      ["gateway", null],
      ["temperature-1", 20],
      ["temperature-2", 28],
    ]);
    await page.reload({ waitUntil: "networkidle" });
    await page.locator("[data-indoor-house][aria-busy='false']").waitFor();
    assert.deepEqual(await readIndoor(), {
      readings: [
        { color: "rgb(0, 0, 0)", tone: "condition-forecast-tone-neutral" },
        { color: "rgb(67, 151, 86)", tone: "condition-forecast-tone-green" },
        { color: "rgb(207, 67, 55)", tone: "condition-forecast-tone-red" },
      ],
      text: ["Second floor—", "First floor20°C", "Basement28°C"],
    });
    assert.equal(fixture.state.requests.some((entry) => entry.includes("/api/v1/admin/")), false);
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// recheck visible home access while refreshing only current sunlight inputs
test("home-network timer revokes access without rereading full weather products", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, { timezoneId: "America/Los_Angeles" });
    await page.clock.install({ time: new Date("2026-09-14T19:00:00Z") });
    fixture.state.viewerContext = { data: { homeNetwork: true } };
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    await page.locator("[data-indoor-house]").waitFor();
    const contextReads = fixture.state.requests.filter(
      // count private-context checks only
      (entry) => entry === "GET /api/v1/viewer-context",
    ).length;
    const currentReads = fixture.state.requests.filter(
      // count the bounded current-only polling stream
      (entry) => entry === "GET /api/v1/sites/ballydidean/current",
    ).length;
    const otherWeatherReads = fixture.state.requests.filter(
      // exclude current polling and optional private positions
      (entry) => entry.startsWith("GET /api/v1/sites/ballydidean/") &&
        entry !== "GET /api/v1/sites/ballydidean/current" &&
        !entry.endsWith("/property-sensor-layout"),
    ).length;
    const layoutReads = fixture.state.requests.filter(
      // track optional private layout reads exactly
      (entry) => entry === "GET /api/v1/sites/ballydidean/property-sensor-layout",
    ).length;
    const adminReads = fixture.state.requests.filter(
      // track every protected api request exactly
      (entry) => entry.includes("/api/v1/admin/"),
    ).length;

    fixture.state.viewerContext = { data: { homeNetwork: false } };
    const expired = page.waitForResponse(
      // wait for the interval check to finish
      (response) => new URL(response.url()).pathname === "/api/v1/viewer-context",
    );
    const refreshedCurrent = page.waitForResponse(
      // await the current-only minute refresh
      (response) => new URL(response.url()).pathname === "/api/v1/sites/ballydidean/current",
    );
    await page.clock.fastForward(60_100);
    await Promise.all([expired, refreshedCurrent]);
    await page.locator("[data-indoor-house]").waitFor({ state: "detached" });
    assert.equal(fixture.state.requests.filter((entry) => entry === "GET /api/v1/viewer-context").length, contextReads + 1);
    assert.equal(fixture.state.requests.filter(
      // run exactly one current refresh for the elapsed minute
      (entry) => entry === "GET /api/v1/sites/ballydidean/current",
    ).length, currentReads + 1);
    assert.equal(fixture.state.requests.filter(
      // preserve full weather-product volume during the timer check
      (entry) => entry.startsWith("GET /api/v1/sites/ballydidean/") &&
        entry !== "GET /api/v1/sites/ballydidean/current" &&
        !entry.endsWith("/property-sensor-layout"),
    ).length, otherWeatherReads);
    assert.equal(fixture.state.requests.filter(
      // avoid repeating the retained private layout
      (entry) => entry === "GET /api/v1/sites/ballydidean/property-sensor-layout",
    ).length, layoutReads);
    assert.equal(fixture.state.requests.filter(
      // never cross into administrator endpoints
      (entry) => entry.includes("/api/v1/admin/"),
    ).length, adminReads);

    await page.getByRole("link", { name: "Forecast" }).click();
    await page.waitForURL(`${fixture.origin}/forecast`);
    await page.locator("[data-forecast-charts]").waitFor();
    const routeContextReads = fixture.state.requests.filter((entry) => entry === "GET /api/v1/viewer-context").length;
    const routeCurrentReads = fixture.state.requests.filter(
      // capture forecast-route current polling independently
      (entry) => entry === "GET /api/v1/sites/ballydidean/current",
    ).length;
    const routeOtherWeatherReads = fixture.state.requests.filter(
      // capture full products after the intentional route load
      (entry) => entry.startsWith("GET /api/v1/sites/ballydidean/") &&
        entry !== "GET /api/v1/sites/ballydidean/current" &&
        !entry.endsWith("/property-sensor-layout"),
    ).length;
    const routeLayoutReads = fixture.state.requests.filter(
      // retain the route's settled private-layout count
      (entry) => entry === "GET /api/v1/sites/ballydidean/property-sensor-layout",
    ).length;
    const routeAdminReads = fixture.state.requests.filter(
      // retain the route's settled protected-api count
      (entry) => entry.includes("/api/v1/admin/"),
    ).length;
    const forecastCurrent = page.waitForResponse(
      // await one forecast-route current-only interval read
      (response) => new URL(response.url()).pathname === "/api/v1/sites/ballydidean/current",
    );
    await page.clock.fastForward(60_100);
    await forecastCurrent;
    assert.equal(fixture.state.requests.filter((entry) => entry === "GET /api/v1/viewer-context").length, routeContextReads);
    assert.equal(fixture.state.requests.filter(
      // retain minute current refreshes on the forecast route
      (entry) => entry === "GET /api/v1/sites/ballydidean/current",
    ).length, routeCurrentReads + 1);
    assert.equal(fixture.state.requests.filter(
      // avoid forecast tide daily layout or admin reloads on the interval
      (entry) => entry.startsWith("GET /api/v1/sites/ballydidean/") &&
        entry !== "GET /api/v1/sites/ballydidean/current" &&
        !entry.endsWith("/property-sensor-layout"),
    ).length, routeOtherWeatherReads);
    assert.equal(fixture.state.requests.filter(
      // keep private layouts off the forecast timer
      (entry) => entry === "GET /api/v1/sites/ballydidean/property-sensor-layout",
    ).length, routeLayoutReads);
    assert.equal(fixture.state.requests.filter(
      // keep protected apis off the forecast timer
      (entry) => entry.includes("/api/v1/admin/"),
    ).length, routeAdminReads);
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// verify the complete administrator session inside a cross-site iframe
test("admin login and logout work inside an iframe", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 1000 },
    });
    await page.goto(`${fixture.origin}/admin-embed`, { waitUntil: "networkidle" });
    const frame = page.frameLocator('iframe[title="Embedded admin"]');
    await frame.getByRole("heading", { name: "Admin sign in" }).waitFor();
    await frame.getByLabel("Password").fill("test-admin-password");
    await frame.getByRole("button", { name: "Sign in" }).click();
    await frame.getByRole("heading", { name: "Property sensors" }).waitFor();
    assert.match(
      await frame.locator("html").evaluate(
        // prove the browser crossed into the distinct embedded site
        () => window.location.origin,
      ),
      /^http:\/\/localhost:/u,
    );
    assert.equal(
      await frame.locator("html").evaluate(
        // keep the authenticated navigation inside the embedded document
        () => window.location.pathname,
      ),
      "/admin",
    );
    assert.equal(page.url(), `${fixture.origin}/admin-embed`);
    assert.equal(
      (await page.context().cookies()).find(
        // locate the opaque embedded administrator session
        (cookie) => cookie.name === "weather_admin_session",
      )?.partitionKey,
      "http://127.0.0.1",
    );
    await frame.getByRole("button", { name: "Log out" }).click();
    await frame.getByRole("heading", { name: "Admin sign in" }).waitFor();
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

test("admin forecast switches persist independently while the public cloud toggle remains", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 850, width: 1100 },
    });
    await page.goto(`${fixture.origin}/admin`, { waitUntil: "networkidle" });
    await page.getByLabel("Password").fill("test-admin-password");
    await page.getByRole("button", { name: "Sign in" }).click();
    await page.getByRole("heading", { name: "Forecast adjustments" }).waitFor();
    const form = page.locator("[data-admin-forecast-adjustments]");
    // wait beyond the heading-only loading skeleton
    await form.waitFor();
    assert.equal(await form.getByRole("checkbox").count(), 3);
    assert.equal(await form.getByRole("checkbox", { name: "Temperature" }).isChecked(), true);
    assert.equal(await form.getByRole("checkbox", { name: "Wind" }).isChecked(), true);
    assert.equal(await form.getByRole("checkbox", { name: "Rain" }).isChecked(), true);
    await form.getByRole("checkbox", { name: "Temperature" }).uncheck();
    await form.getByRole("checkbox", { name: "Wind" }).uncheck();
    await form.getByRole("checkbox", { name: "Rain" }).uncheck();
    await form.getByRole("button", { name: "Save adjustments" }).click();
    await form.getByText("Forecast adjustments saved.").waitFor();
    assert.equal(fixture.state.adjustmentSettingsUpdates, 1);
    assert.deepEqual(fixture.state.adjustmentSettings, {
      version: 1, temperature: false, wind: false, rain: false,
    });

    await page.goto(`${fixture.origin}/forecast`, { waitUntil: "networkidle" });
    assert.equal(await page.locator("[data-forecast-adjustment-toggle]").count(), 1);
    await page.goto(`${fixture.origin}/admin`, { waitUntil: "networkidle" });
    await page.getByRole("heading", { name: "Forecast adjustments" }).waitFor();
    const reopened = page.locator("[data-admin-forecast-adjustments]");
    // require the loaded editor before counting controls
    await reopened.waitFor();
    assert.equal(await reopened.getByRole("checkbox").count(), 3);
    assert.equal(await reopened.getByRole("checkbox", { name: "Temperature" }).isChecked(), false);
    assert.equal(await reopened.getByRole("checkbox", { name: "Wind" }).isChecked(), false);
    assert.equal(await reopened.getByRole("checkbox", { name: "Rain" }).isChecked(), false);
    await reopened.getByRole("checkbox", { name: "Rain" }).check();
    await reopened.getByRole("button", { name: "Save adjustments" }).click();
    await reopened.getByText("Forecast adjustments saved.").waitFor();
    await page.goto(`${fixture.origin}/forecast`, { waitUntil: "networkidle" });
    assert.equal(await page.locator("[data-forecast-adjustment-toggle]").count(), 1);
    assert.deepEqual(fixture.state.adjustmentSettings, {
      version: 1, temperature: false, wind: false, rain: true,
    });
    await page.goto(`${fixture.origin}/admin`, { waitUntil: "networkidle" });
    await page.getByRole("heading", { name: "Forecast adjustments" }).waitFor();
    const finalForm = page.locator("[data-admin-forecast-adjustments]");
    assert.equal(await finalForm.getByRole("checkbox", { name: "Temperature" }).isChecked(), false);
    assert.equal(await finalForm.getByRole("checkbox", { name: "Wind" }).isChecked(), false);
    assert.equal(await finalForm.getByRole("checkbox", { name: "Rain" }).isChecked(), true);

    // distinguish temperature and wind through save and reload
    await finalForm.getByRole("checkbox", { name: "Wind" }).check();
    await finalForm.getByRole("button", { name: "Save adjustments" }).click();
    await finalForm.getByText("Forecast adjustments saved.").waitFor();
    assert.deepEqual(fixture.state.adjustmentSettings, {
      version: 1, temperature: false, wind: true, rain: true,
    });
    await page.reload({ waitUntil: "networkidle" });
    await page.getByRole("heading", { name: "Forecast adjustments" }).waitFor();
    const mixedForm = page.locator("[data-admin-forecast-adjustments]");
    assert.equal(await mixedForm.getByRole("checkbox", { name: "Temperature" }).isChecked(), false);
    assert.equal(await mixedForm.getByRole("checkbox", { name: "Wind" }).isChecked(), true);
    assert.equal(await mixedForm.getByRole("checkbox", { name: "Rain" }).isChecked(), true);
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

test("admin adjustment scorecard renders legacy and pending v2 reports fail-closed", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 390 },
    });
    await page.goto(`${fixture.origin}/admin`, { waitUntil: "networkidle" });
    await page.getByLabel("Password").fill("test-admin-password");
    await page.getByRole("button", { name: "Sign in" }).click();
    const scorecard = page.locator(".adjustment-scorecard[data-scorecard-state='ready']");
    await scorecard.waitFor();
    assert.equal(await scorecard.locator("[data-adjustment-family]").count(), 3);
    assert.deepEqual(
      await scorecard.locator("[data-adjustment-family] h3").allTextContents(),
      ["Temperature", "Wind", "Rain"],
    );
    assert.equal(await scorecard.getByText("Worse", { exact: true }).count(), 1);
    assert.equal(await scorecard.getByText("95% interval 4.2% to 19.1%", { exact: false }).count(), 3);
    assert.equal(await scorecard.locator("button, form, input, select, textarea").count(), 0);
    assert.equal(await scorecard.getByText("Historical display only.", { exact: false }).count(), 1);
    assert.equal(await page.evaluate(
      // reject scorecard-induced horizontal viewport overflow
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ), true);
    await scorecard.locator("[data-adjustment-family='rain'] summary", { hasText: "Rain diagnostics" }).click();
    assert.equal(await scorecard.getByRole("columnheader", { name: "Raw Brier" }).count(), 1);
    assert.equal(fixture.state.requests.filter(
      // count only the dedicated queryless protected read
      (entry) => entry === "GET /api/v1/admin/sites/ballydidean/forecast-adjustment-scorecard",
    ).length, 1);

    fixture.state.adjustmentScorecard = adjustmentScorecardV2Fixture();
    fixture.state.adjustmentScorecardPublicationState = "pending_unapplied";
    await page.reload({ waitUntil: "networkidle" });
    const pendingScorecard = page.locator(
      ".adjustment-scorecard[data-scorecard-publication-state='pending_unapplied']",
    );
    await pendingScorecard.waitFor();
    assert.equal(
      await pendingScorecard.getByText("Pending and unapplied.", { exact: false }).count(),
      1,
    );
    assert.equal(await pendingScorecard.getByText("Automatic action history (1)").count(), 1);
    assert.equal(await pendingScorecard.locator("button, form, input, select, textarea").count(), 0);
    assert.equal(await page.evaluate(
      // reject v2 scorecard-induced horizontal viewport overflow
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ), true);

    fixture.state.adjustmentScorecardStatus = 503;
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.locator(".adjustment-scorecard[data-scorecard-state='unavailable']").waitFor();
    assert.equal(await page.getByText("No validated scorecard is available.", { exact: false }).count(), 1);

    fixture.state.adjustmentScorecardStatus = 403;
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.locator(".adjustment-scorecard[data-scorecard-state='unauthorized']").waitFor();
    assert.equal(await page.getByText("Administrator session is unavailable.", { exact: false }).count(), 1);
    await page.close();
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

test("admin editor signs in, names, and places a reporting EcoWitt sensor", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 900 },
    });
    await page.goto(`${fixture.origin}/admin`, { waitUntil: "networkidle" });
    assert.equal(await page.getByRole("heading", { name: "Admin sign in" }).isVisible(), true);
    assert.equal(await page.locator(".admin-login-icon").textContent(), "settings");
    assert.equal(await page.locator('input[name="username"]').count(), 0);
    assert.equal(await page.locator('input[name="password"]').count(), 1);
    await page.getByLabel("Password").fill("wrong-password");
    await page.getByRole("button", { name: "Sign in" }).click();
    assert.equal(await page.getByRole("alert").textContent(), "The password is incorrect.");
    await page.getByLabel("Password").fill("test-admin-password");
    await page.getByRole("button", { name: "Sign in" }).click();
    await page.getByRole("heading", { name: "Property sensors" }).waitFor();
    await page.evaluate(
      // wait for the administrator page to own its worker
      async () => {
        await navigator.serviceWorker.ready;
      },
    );
    await page.waitForFunction(
      // require navigation interception before testing the cache
      () => navigator.serviceWorker.controller !== null,
    );
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    const freshHomepageSoilMap = page.locator("[data-admin-soil-map]");
    await page.getByRole("heading", { name: "Soil moisture" }).waitFor();
    assert.equal(await freshHomepageSoilMap.locator('[data-soil-moisture-sensor="soil-1"]').count(), 1);
    assert.equal(await freshHomepageSoilMap.getByText("1 reporting", { exact: true }).isVisible(), true);
    assert.equal(await freshHomepageSoilMap.getByText(/needs a position in Admin/u).count(), 0);
    assert.deepEqual(
      await page.evaluate(
        // inspect the shared shell after authenticated navigation
        async () => {
          const cache = await caches.open("ballydidean-weather-shell-browser-test");
          const requests = await cache.keys();
          const homepageRequests = requests.filter(
            // select every cached homepage variant
            (request) => new URL(request.url).pathname === "/",
          );
          return await Promise.all(homepageRequests.map(
            // inspect every stored homepage response
            async (request) => await cache.match(request, { ignoreVary: true }).then(
              // expose only the administrator marker
              async (response) => /data-weather-admin="true"/u.test(await response?.text() ?? ""),
            ),
          ));
        },
      ),
      [false],
    );
    await page.goto(`${fixture.origin}/admin`, { waitUntil: "networkidle" });
    await page.getByRole("heading", { name: "Property sensors" }).waitFor();
    assert.equal(await page.getByRole("heading", { name: "Property sensors" }).isVisible(), true);
    assert.equal(await page.locator('[data-property-sensor-select="soil-1"]').getAttribute("aria-pressed"), "true");
    assert.equal(
      await page.locator('[data-property-sensor-select="soil-1"] .material-symbols-rounded').textContent(),
      "device_thermostat",
    );
    assert.equal(await page.locator('input[name="icon"]').count(), 4);
    assert.equal(await page.getByRole("radio", { name: "Temperature" }).isChecked(), true);
    assert.equal(await page.getByRole("button", { name: "Save sensor" }).locator("svg").count(), 1);
    assert.equal(await page.locator(".property-admin-map [data-property-map-layer]").count(), 3);
    await page.locator(".property-admin-map").getByRole("button", { name: "Roads" }).click();
    assert.equal(
      await page.locator(".property-admin-map").getByRole("button", { name: "Roads" }).getAttribute("aria-pressed"),
      "true",
    );
    const name = page.locator('input[name="displayName"]');
    await name.fill("North orchard soil");
    const markerGeometry = await page.locator(".property-position-marker").evaluate(
      // require the marker tip to terminate at its coordinate anchor
      (marker) => {
        const bounds = marker.getBoundingClientRect();
        const pin = marker.querySelector(".property-position-marker-pin");

        // require the complete marker geometry
        if (!(pin instanceof SVGGraphicsElement)) {
          throw new Error("property marker pin is missing");
        }

        const pinBounds = pin.getBBox();
        return {
          height: bounds.height,
          pinBottom: pinBounds.y + pinBounds.height,
          width: bounds.width,
        };
      },
    );
    assert.equal(Math.abs(markerGeometry.pinBottom) < 0.01, true);
    await page.locator(".property-admin-map").getByRole("button", { name: "Zoom in" }).click();
    const zoomedMarkerGeometry = await page.locator(".property-position-marker").evaluate(
      // keep the coordinate marker fixed-size during map zoom
      (marker) => {
        const bounds = marker.getBoundingClientRect();
        return { height: bounds.height, width: bounds.width };
      },
    );
    assert.equal(Math.abs(zoomedMarkerGeometry.height - markerGeometry.height) < 0.01, true);
    assert.equal(Math.abs(zoomedMarkerGeometry.width - markerGeometry.width) < 0.01, true);
    await page.locator("[data-property-position-map]").click({ position: { x: 380, y: 170 } });
    const latitude = Number(await page.locator('input[name="latitude"]').inputValue());
    const longitude = Number(await page.locator('input[name="longitude"]').inputValue());
    assert.equal(Number.isFinite(latitude), true);
    assert.equal(Number.isFinite(longitude), true);
    await page.locator(".property-icon-options label").filter({ hasText: "Air quality" }).click();
    assert.equal(
      await page.locator("[data-property-position-marker-icon]").textContent(),
      "masks",
    );
    assert.equal(
      await page.locator(".property-admin-actions").evaluate(
        // align the save action against the editor's right edge
        (actions) => {
          const button = actions.querySelector("button");
          const form = actions.closest("form");

          // require the rendered save geometry
          if (!(button instanceof HTMLElement) || !(form instanceof HTMLElement)) {
            return false;
          }

          return Math.abs(button.getBoundingClientRect().right - form.getBoundingClientRect().right) < 1;
        },
      ),
      true,
    );
    await page.getByRole("button", { name: "Save sensor" }).click();
    await page.waitForFunction(
      // wait for the saved name to survive the controller rerender
      () => document.querySelector('input[name="displayName"]')?.value === "North orchard soil",
    );
    assert.equal(fixture.state.adminUpdates, 1);
    assert.equal(fixture.state.propertySensorLayout[0].displayName, "North orchard soil");
    assert.equal(fixture.state.propertySensorLayout[0].icon, "air-quality");
    await page.getByRole("link", { name: "Now" }).click();
    await page.getByRole("heading", { name: "Indoor temperatures" }).waitFor();
    // wait for populated house data
    await page.locator("[data-indoor-house][aria-busy='false']").waitFor();
    assert.deepEqual(
      await page.locator(".indoor-house-level").allTextContents(),
      ["Second floor71°F", "First floor68°F", "Basement62°F"],
    );
    const soilMap = page.locator("[data-admin-soil-map]");
    await page.getByRole("heading", { name: "Soil moisture" }).waitFor();
    assert.equal(await soilMap.locator("[data-soil-moisture-sensor]").count(), 1);
    assert.equal(await soilMap.locator('[data-soil-moisture-sensor="soil-1"] text').textContent(), "42%");
    assert.equal(await soilMap.locator("[data-property-sensor-view]").count(), 0);
    assert.equal(await soilMap.locator(".farm-marker").count(), 0);
    assert.equal(await soilMap.getByText("Barn temperature", { exact: true }).count(), 0);
    assert.equal(
      await soilMap.locator('[data-soil-moisture-sensor="soil-1"]').getAttribute("aria-label"),
      "North orchard soil: 42% soil moisture",
    );
    assert.equal(
      await soilMap.evaluate(
        // keep the soil overview below the indoor house
        (map) => {
          const house = document.querySelector("[data-indoor-house]");
          return house instanceof HTMLElement && map.getBoundingClientRect().top >= house.getBoundingClientRect().bottom;
        },
      ),
      true,
    );
    assert.equal(
      await page.locator(".indoor-house-illustration").evaluate(
        // join the roof base cleanly to the second floor
        (illustration) => {
          const roof = illustration.querySelector(".indoor-house-roof");
          const levels = illustration.querySelector(".indoor-house-levels");

          // require both house sections
          if (!(roof instanceof HTMLElement) || !(levels instanceof HTMLElement)) {
            return false;
          }

          return Math.abs(roof.getBoundingClientRect().bottom - levels.getBoundingClientRect().top) < 1;
        },
      ),
      true,
    );
    await page.waitForFunction(
      // wait for fonts and grid tracks before comparing geometry
      () => {
        const house = document.querySelector("[data-indoor-house]");
        const cards = [...document.querySelectorAll(".condition-card")];

        // require the complete rendered dashboard
        if (!(house instanceof HTMLElement) || cards.length === 0) {
          return false;
        }

        const cardsBottom = Math.max(...cards.map(
          // measure every current-condition card
          (card) => card.getBoundingClientRect().bottom,
        ));
        return house.getBoundingClientRect().top >= cardsBottom;
      },
    );
    await page.setViewportSize({ height: 844, width: 390 });
    await page.evaluate(
      // wait one paint for responsive observers
      async () => await new Promise((resolveFrame) => requestAnimationFrame(() => resolveFrame())),
    );
    assert.equal(
      await page.locator(".indoor-house-illustration").evaluate(
        // preserve the roof join at the phone breakpoint
        (illustration) => {
          const roof = illustration.querySelector(".indoor-house-roof");
          const levels = illustration.querySelector(".indoor-house-levels");

          // require both house sections
          if (!(roof instanceof HTMLElement) || !(levels instanceof HTMLElement)) {
            return false;
          }

          return Math.abs(roof.getBoundingClientRect().bottom - levels.getBoundingClientRect().top) < 1;
        },
      ),
      true,
    );
    assert.equal(
      await page.evaluate(
        // keep the admin-only house inside a phone viewport
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
      true,
    );
    assert.equal(await soilMap.isVisible(), true);
    await page.goto(`${fixture.origin}/map`, { waitUntil: "networkidle" });
    assert.equal(await page.getByText("North orchard soil", { exact: true }).first().isVisible(), true);
    assert.equal(
      await page.locator('.property-sensor-marker .property-sensor-marker-icon').first().textContent(),
      "masks",
    );
    await page.goto(`${fixture.origin}/admin`, { waitUntil: "networkidle" });
    await page.getByRole("button", { name: "Log out" }).click();
    await page.getByRole("heading", { name: "Admin sign in" }).waitFor();
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

test("forecast charts share one touch-controlled crosshair", { timeout: 60_000 }, async () => {
  // prevent the production edge from injecting a hidden radar preload
  assert.doesNotMatch(readFileSync(join(publicRoot, "index.html"), "utf8"), /__WEATHER_ROUTE_PRELOAD__|\/maps\/xweather\//u);
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      hasTouch: true,
      viewport: { height: 844, width: 390 },
    });
    await page.goto(`${fixture.origin}/forecast`, { waitUntil: "domcontentloaded" });
    await assertSkeletonLoadingSettled(page);
    const grid = page.locator("[data-forecast-charts]");
    const line = page.locator(".forecast-shared-crosshair");
    const currentLine = page.locator(".forecast-current-time-line");
    const session = await page.context().newCDPSession(page);

    // dispatch one browser-native touch gesture
    const swipe = async (startX, startY, endX, endY) => {
      await session.send("Input.dispatchTouchEvent", {
        touchPoints: [{ x: startX, y: startY }],
        type: "touchStart",
      });
      await session.send("Input.dispatchTouchEvent", {
        touchPoints: [{ x: endX, y: endY }],
        type: "touchMove",
      });
      await session.send("Input.dispatchTouchEvent", { touchPoints: [], type: "touchEnd" });
      await page.waitForTimeout(40);
    };

    assert.equal(await page.locator(".forecast-chart").count(), 9);
    assert.equal(await page.locator(".forecast-chart-line").count(), 11);
    assert.deepEqual(
      await page.locator(".forecast-chart-line").evaluateAll(
        // match every forecast range
        (chartLines) => [...new Set(chartLines.map((chartLine) => getComputedStyle(chartLine).strokeWidth))].sort(),
      ),
      ["1.5px", "2px"],
    );
    assert.equal(await page.locator(".forecast-chart-guide").count(), 0);
    assert.equal(await page.locator(".forecast-chart linearGradient").count(), 11);
    assert.equal(await page.locator(".forecast-chart-scale").count(), 18);
    assert.equal(await page.locator(".forecast-x-tick").count(), 24);
    assert.equal(await page.locator(".forecast-x-axis time").count(), 5);
    assert.equal(await page.locator("button[data-forecast-days]").count(), 3);
    assert.equal(await page.locator("[data-forecast-weather-map]").count(), 0);
    assert.equal(
      fixture.state.requests.some(
        // do not load hidden forecast map tiles
        (entry) => /^GET \/maps\/xweather\/(?:history|forecast)\//u.test(entry),
      ),
      false,
    );
    assert.equal(await grid.getAttribute("data-forecast-days"), "1");
    assert.equal(await page.locator(".forecast-chart-days").count(), 0);
    assert.equal(await page.getByRole("button", { name: "Today" }).getAttribute("aria-pressed"), "true");
    assert.deepEqual(
      await page.locator(".forecast-range-selector").evaluate(
        // keep the active range distinct inside one grouped surface
        (selector) => {
          const selected = selector.querySelector('[aria-pressed="true"]');
          const unselected = selector.querySelector('[aria-pressed="false"]');
          const selectorStyle = getComputedStyle(selector);
          const selectedStyle = selected === null ? null : getComputedStyle(selected);
          const unselectedStyle = unselected === null ? null : getComputedStyle(unselected);
          return {
            grouped: selectorStyle.backgroundColor !== "rgba(0, 0, 0, 0)",
            selectedBackgroundDistinct:
              selectedStyle?.backgroundColor !== unselectedStyle?.backgroundColor,
            selectedTextDistinct: selectedStyle?.color !== unselectedStyle?.color,
          };
        },
      ),
      {
        grouped: true,
        selectedBackgroundDistinct: true,
        selectedTextDistinct: true,
      },
    );
    assert.match(await page.locator("[data-forecast-crosshair-time]").textContent() ?? "", /\d{1,2}:\d{2}\s[AP]M/u);
    assert.equal(await page.locator(".forecast-chart-heading-top").count(), 9);
    assert.equal(await page.locator(".forecast-chart-heading-bottom").count(), 0);
    assert.equal(await page.locator('.forecast-x-tick[data-forecast-light="day"]').count(), 15);
    assert.equal(await page.locator('.forecast-x-tick[data-forecast-light="night"]').count(), 9);
    assert.equal(await page.locator(".forecast-chart-daylight").count(), 9);
    assert.equal(await page.locator('.forecast-chart-daylight [data-forecast-light="day"]').count(), 135);
    assert.equal(await page.locator('.forecast-chart-daylight [data-forecast-light="night"]').count(), 81);
    assert.deepEqual(
      await page.locator(".forecast-x-axis time").allTextContents(),
      ["12 AM", "6 AM", "12 PM", "6 PM", "11 PM"],
    );
    assert.deepEqual(
      await page.locator(".forecast-chart-shell").evaluate(
        // share one visible daylight scale across charts and axis
        (shell) => {
          const dayTick = shell.querySelector('[data-forecast-light="day"]');
          const nightTick = shell.querySelector('[data-forecast-light="night"]');
          const plotDay = shell.querySelector('.forecast-chart-daylight [data-forecast-light="day"]');
          const plotNight = shell.querySelector('.forecast-chart-daylight [data-forecast-light="night"]');
          return {
            dayBackground: dayTick === null ? "none" : getComputedStyle(dayTick).backgroundColor,
            nightBackground: nightTick === null ? "none" : getComputedStyle(nightTick).backgroundColor,
            plotDayBackground: plotDay === null ? "none" : getComputedStyle(plotDay).backgroundColor,
            plotNightBackground: plotNight === null ? "none" : getComputedStyle(plotNight).backgroundColor,
          };
        },
      ),
      {
        dayBackground: "rgba(240, 218, 89, 0.2)",
        nightBackground: "rgba(84, 68, 96, 0.14)",
        plotDayBackground: "rgba(240, 218, 89, 0.2)",
        plotNightBackground: "rgba(84, 68, 96, 0.14)",
      },
    );
    assert.deepEqual(
      await page.locator(".forecast-chart").evaluateAll(
        // preserve one chart per current-condition stat
        (charts) => charts.map((chart) => chart.getAttribute("data-forecast-chart")),
      ),
      ["temperature", "wind", "rain-rate", "clouds", "humidity", "air-quality", "uv-index", "pressure", "tide"],
    );
    assert.deepEqual(
      await page.locator('[data-forecast-chart="temperature"]').evaluate(
        // expose air temperature separately from the provider's feels-like series
        (chart) => ({
          labels: [...chart.querySelectorAll(".forecast-chart-value small")].map((label) => label.textContent),
          lineClasses: [...chart.querySelectorAll(".forecast-chart-line")].map((line) => line.getAttribute("class")),
          series: JSON.parse(chart.getAttribute("data-forecast-series") ?? "[]"),
        }),
      ),
      {
        labels: ["Feels like", "Air Temp"],
        lineClasses: ["forecast-chart-line forecast-chart-line-0", "forecast-chart-line forecast-chart-line-1"],
        series: [
          { label: "Feels like", values: forecast.slice(0, 24).map((record) => record.metrics.apparentTemperatureC) },
          { label: "Air Temp", values: forecast.slice(0, 24).map((record) => record.metrics.temperatureC) },
        ],
      },
    );
    assert.deepEqual(
      await page.locator('[data-forecast-chart="clouds"]').evaluate(
        // preserve percentage cover after rain with the shared cloud palette
        (chart) => ({
          domain: [Number(chart.getAttribute("data-forecast-min")), Number(chart.getAttribute("data-forecast-max"))],
          format: chart.getAttribute("data-forecast-format"),
          series: JSON.parse(chart.getAttribute("data-forecast-series") ?? "[]"),
          stopColors: [...new Set([...chart.querySelectorAll("stop")].map((stop) => stop.getAttribute("stop-color")))],
        }),
      ),
      {
        domain: [0, 100],
        format: "cloudCover",
        series: [{ label: "Cover", values: Array.from({ length: 24 }, () => 42) }],
        stopColors: ["rgb(105, 133, 155)"],
      },
    );
    const expectedPressureChanges = forecast.slice(0, 24).map(
      // compare each complete same-run endpoint to three hours earlier
      (record, index) => index < 3
        ? null
        : record.metrics.pressureHpa - forecast[index - 3].metrics.pressureHpa,
    );
    assert.deepEqual(
      await page.locator('[data-forecast-chart="pressure"]').evaluate(
        // retain signed unitless rolling changes on a symmetric scale
        (chart) => ({
          domain: [Number(chart.getAttribute("data-forecast-min")), Number(chart.getAttribute("data-forecast-max"))],
          format: chart.getAttribute("data-forecast-format"),
          series: JSON.parse(chart.getAttribute("data-forecast-series") ?? "[]"),
          stopColors: [...new Set([...chart.querySelectorAll("stop")].map((stop) => stop.getAttribute("stop-color")))],
        }),
      ),
      {
        domain: [-6, 6],
        format: "pressureChange",
        series: [{ label: "3h change", values: expectedPressureChanges }],
        stopColors: [
          "rgb(136, 136, 130)",
          "rgb(230, 181, 25)",
          "rgb(200, 183, 68)",
          "rgb(0, 146, 63)",
        ],
      },
    );
    assert.doesNotMatch(
      await page.locator('[data-forecast-chart="pressure"]').textContent() ?? "",
      /(?:hPa|inHg)/u,
    );
    assert.doesNotMatch(
      await page.locator('[data-forecast-chart="air-quality"]').textContent() ?? "",
      /(?:µg\/m³|ug\/m3)/u,
    );
    assert.equal(
      await page.locator(".forecast-chart").evaluateAll(
        // place every title at the plot's top-left edge
        (charts) => charts.every((chart) => {
          const heading = chart.querySelector(".forecast-chart-heading");
          const plot = chart.querySelector(".forecast-chart-plot");
          const headingBounds = heading?.getBoundingClientRect();
          const plotBounds = plot?.getBoundingClientRect();
          const atTop = heading?.classList.contains("forecast-chart-heading-top") === true;
          return headingBounds !== undefined && plotBounds !== undefined && atTop &&
            Math.abs(headingBounds.left - plotBounds.left) < 1 &&
            Math.abs(headingBounds.top - plotBounds.top) < 1 &&
            headingBounds.bottom <= plotBounds.bottom &&
            getComputedStyle(heading).position === "absolute";
        }),
      ),
      true,
    );
    assert.equal(
      await page.locator(".forecast-chart").evaluateAll(
        // reserve a line-free gutter beneath every title
        (charts) => charts.every((chart) => {
          const ordinates = [...chart.querySelectorAll("polyline")].flatMap(
            // read every rendered SVG ordinate
            (line) => (line.getAttribute("points") ?? "").split(" ").flatMap((point) => {
              const ordinate = Number(point.split(",")[1]);
              return Number.isFinite(ordinate) ? [ordinate] : [];
            }),
          );
          return ordinates.every((ordinate) => ordinate >= 52);
        }),
      ),
      true,
    );
    await assertForecastTitleClearance(page);
    await grid.press("End");
    assert.equal(await page.locator("[data-forecast-weather-map]").count(), 0);
    assert.equal(
      fixture.state.requests.some(
        // keep every hidden forecast map layer network-idle
        (entry) => /^GET \/maps\/xweather\/(?:history|forecast)\//u.test(entry),
      ),
      false,
    );
    await page.getByRole("button", { name: "5 days" }).click();
    await page.waitForFunction(
      // wait for the five-day forecast render
      () => document.querySelectorAll(".forecast-x-tick").length === 120,
    );
    assert.equal(await page.getByRole("button", { name: "5 days" }).getAttribute("aria-pressed"), "true");
    assert.equal(await page.locator("[data-forecast-weather-map]").count(), 0);
    assert.equal(JSON.parse(await grid.getAttribute("data-forecast-times") ?? "[]").length, 121);
    assert.deepEqual(
      await page.locator(".forecast-chart-line").evaluateAll(
        // match every forecast range
        (chartLines) => [...new Set(chartLines.map((chartLine) => getComputedStyle(chartLine).strokeWidth))].sort(),
      ),
      ["1.5px", "2px"],
    );
    assert.match(await page.locator("[data-forecast-crosshair-time]").textContent() ?? "", /^(?:Sun|Mon|Tue|Wed|Thu|Fri|Sat),[\s\S]*\d{1,2}:\d{2}\s[AP]M/u);
    assert.equal(await page.locator(".forecast-chart-days").count(), 9);
    assert.equal(await page.locator(".forecast-chart-day-start").count(), 45);
    assert.deepEqual(
      await page.locator(".forecast-chart-days").first().locator("b").allTextContents(),
      ["Fri 21", "Sat 22", "Sun 23", "Mon 24", "Tue 25"],
    );
    assert.equal(
      await page.locator(".forecast-chart-day-start").evaluateAll(
        // require each midnight divider to span its complete chart
        (markers) => markers.every((marker) => {
          const markerBounds = marker.getBoundingClientRect();
          const plotBounds = marker.closest(".forecast-chart-plot")?.getBoundingClientRect();
          return plotBounds !== undefined && Math.abs(markerBounds.top - plotBounds.top) < 1 &&
            Math.abs(markerBounds.bottom - plotBounds.bottom) < 1 &&
            getComputedStyle(marker).borderLeftStyle === "solid";
        }),
      ),
      true,
    );
    assert.equal(
      await page.locator(".forecast-chart-day-start b").first().evaluate(
        // require the reviewed light-gray day panel
        (panel) => getComputedStyle(panel).backgroundColor,
      ),
      "rgb(228, 228, 228)",
    );
    await assertForecastTitleClearance(page);
    await page.getByRole("button", { name: "10 days" }).click();
    await page.waitForFunction(
      // wait for the ten-day forecast render
      () => document.querySelectorAll(".forecast-x-tick").length === 240,
    );
    assert.equal(await page.getByRole("button", { name: "10 days" }).getAttribute("aria-pressed"), "true");
    assert.equal(JSON.parse(await grid.getAttribute("data-forecast-times") ?? "[]").length, 241);
    assert.deepEqual(
      await page.locator(".forecast-chart-line").evaluateAll(
        // match every forecast range
        (chartLines) => [...new Set(chartLines.map((chartLine) => getComputedStyle(chartLine).strokeWidth))].sort(),
      ),
      ["1.5px", "2px"],
    );
    assert.match(await page.locator("[data-forecast-crosshair-time]").textContent() ?? "", /^(?:Sun|Mon|Tue|Wed|Thu|Fri|Sat),[\s\S]*\d{1,2}:\d{2}\s[AP]M/u);
    assert.equal(await page.locator(".forecast-chart-day-start").count(), 90);
    await assertForecastTitleClearance(page);
    assert.equal(
      fixture.state.requests.some(
        // require the five-day API contract
        (entry) => entry.includes("/forecast?days=5"),
      ),
      true,
    );
    assert.equal(
      fixture.state.requests.some(
        // require the ten-day API contract
        (entry) => entry.includes("/forecast?days=10"),
      ),
      true,
    );
    await page.getByRole("button", { name: "Today" }).click();
    await page.waitForFunction(
      // restore the one-day interaction fixture
      () => document.querySelectorAll(".forecast-x-tick").length === 24,
    );
    await assertSkeletonLoadingSettled(page);
    assert.deepEqual(
      await page.locator(".forecast-chart").evaluateAll(
        // retain the requested stable raw-unit chart domains
        (charts) => Object.fromEntries(charts.flatMap((chart) => {
          const key = chart.getAttribute("data-forecast-chart");

          // inspect only explicitly stabilized chart domains
          if (key === "humidity" || key === null) {
            return [];
          }

          return [[key, [Number(chart.getAttribute("data-forecast-min")), Number(chart.getAttribute("data-forecast-max"))]]];
        })),
      ),
      {
        "air-quality": [0, 14.4882614724],
        clouds: [0, 100],
        pressure: [-6, 6],
        "rain-rate": [0, 25.4],
        temperature: [-1.1111111111, 26.6666666667],
        tide: [-0.3048, 3.6576],
        "uv-index": [0, 4],
        wind: [0, 22.3519999995],
      },
    );
    assert.equal(
      await page.locator(".forecast-chart-line").evaluateAll(
        // render every forecast line from its SVG condition-color gradient
        (lines) => lines.every((line) => line.getAttribute("stroke")?.startsWith("url(#forecast-line-") === true),
      ),
      true,
    );
    assert.deepEqual(
      await page.locator(".forecast-chart-shell").evaluate(
        // keep both lines spanning the complete chart stack and axis
        (chartShell) => {
          const sharedLineElement = chartShell.querySelector(".forecast-shared-crosshair");
          const currentLineElement = chartShell.querySelector(".forecast-current-time-line");
          const sharedLine = sharedLineElement?.getBoundingClientRect();
          const currentLine = currentLineElement?.getBoundingClientRect();
          const charts = [...chartShell.querySelectorAll(".forecast-chart")];
          const first = charts[0]?.getBoundingClientRect();
          const last = charts.at(-1)?.getBoundingClientRect();
          const axis = chartShell.querySelector(".forecast-x-axis")?.getBoundingClientRect();
          return {
            currentReachesAxis: currentLine !== undefined && axis !== undefined && currentLine.bottom >= axis.bottom,
            currentReachesBottom: currentLine !== undefined && last !== undefined && currentLine.bottom >= last.bottom,
            currentReachesTop: currentLine !== undefined && first !== undefined && currentLine.top <= first.top,
            currentUnderScrubber: currentLineElement !== null && sharedLineElement !== null &&
              (currentLineElement.compareDocumentPosition(sharedLineElement) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0,
            reachesAxis: sharedLine !== undefined && axis !== undefined && sharedLine.bottom >= axis.bottom,
            reachesBottom: sharedLine !== undefined && last !== undefined && sharedLine.bottom >= last.bottom,
            reachesTop: sharedLine !== undefined && first !== undefined && sharedLine.top <= first.top,
          };
        },
      ),
      {
        currentReachesAxis: true,
        currentReachesBottom: true,
        currentReachesTop: true,
        currentUnderScrubber: true,
        reachesAxis: true,
        reachesBottom: true,
        reachesTop: true,
      },
    );
    await grid.focus();
    assert.equal(
      await grid.evaluate(
        // remove the chart-specific focus effect
        (chartGrid) => getComputedStyle(chartGrid).outlineStyle,
      ),
      "none",
    );
    assert.equal(
      await page.locator(".forecast-chart").evaluateAll(
        // place every value bubble at one reviewed plot edge
        (charts) => charts.every(
          // verify one forecast bubble
          (chart) => {
            const plot = chart.querySelector(".forecast-chart-plot")?.getBoundingClientRect();
            const value = chart.querySelector(".forecast-chart-value");
            const bounds = value?.getBoundingClientRect();
            const lineZ = Number(getComputedStyle(document.querySelector(".forecast-shared-crosshair")).zIndex);
            const valueZ = value === null ? 0 : Number(getComputedStyle(value).zIndex);
            const atTop = value?.classList.contains("forecast-chart-value-top") === true;
            const atBottom = value?.classList.contains("forecast-chart-value-bottom") === true;
            return plot !== undefined && bounds !== undefined &&
              ((atTop && bounds.top - plot.top >= 0 && bounds.top - plot.top < 8) ||
                (atBottom && plot.bottom - bounds.bottom >= 0 && plot.bottom - bounds.bottom < 8)) &&
              atTop !== atBottom && valueZ > lineZ;
          },
        ),
      ),
      true,
    );
    assert.equal(await page.locator(".forecast-chart-value-top").count() > 0, true);
    assert.equal(await page.locator(".forecast-chart-value-bottom").count() > 0, true);

    await grid.scrollIntoViewIfNeeded();
    const bounds = await grid.boundingBox();
    const mastheadBounds = await page.locator(".forecast-masthead").boundingBox();

    // require a measurable touch surface
    if (bounds === null || mastheadBounds === null) {
      throw new Error("forecast touch surface is unavailable");
    }

    const touchY = Math.min(
      bounds.y + bounds.height - 2,
      Math.max(bounds.y + Math.min(100, bounds.height / 4), mastheadBounds.y + mastheadBounds.height + 16),
    );
    const horizontalTarget = bounds.x + bounds.width * 0.75;
    const currentLineBefore = await currentLine.boundingBox();
    await swipe(bounds.x + 16, touchY, horizontalTarget, touchY);
    const horizontalLine = await line.boundingBox();
    const currentLineAfter = await currentLine.boundingBox();

    // require the direct line position
    if (horizontalLine === null || currentLineBefore === null || currentLineAfter === null) {
      throw new Error("forecast crosshair is unavailable");
    }

    assert.equal(Math.abs(horizontalLine.x - horizontalTarget) < 3, true);
    assert.equal(Math.abs(currentLineAfter.x - currentLineBefore.x) < 1, true);
    assert.equal(Number(await grid.getAttribute("aria-valuenow")) >= 17, true);

    const centerX = bounds.x + bounds.width / 2;
    const quarterHourTarget = centerX + bounds.width / (24 * 4);
    await swipe(quarterHourTarget, touchY, quarterHourTarget, touchY);
    const quarterHourClock = await page.locator("[data-forecast-crosshair-time]").textContent() ?? "";
    assert.match(quarterHourClock, /12:1[45] PM/u);
    await swipe(centerX, touchY, centerX, touchY);
    const centerClock = await page.locator("[data-forecast-crosshair-time]").textContent() ?? "";
    assert.match(centerClock, /(?:11:59 AM|12:00 PM)/u);
    assert.notEqual(centerClock, quarterHourClock);
    const centerLine = await line.boundingBox();
    const centerAirQuality = await page.locator('[data-forecast-chart="air-quality"] [data-forecast-value]').textContent();
    assert.equal(
      await page.locator('[data-forecast-chart="uv-index"] .forecast-chart-value').getAttribute("class"),
      "forecast-chart-value forecast-chart-value-bottom",
    );
    const scrollBefore = await page.evaluate(() => window.scrollY);
    await swipe(centerX, touchY, centerX, touchY - 96);
    const scrolledLine = await line.boundingBox();
    const scrolledAirQuality = await page.locator('[data-forecast-chart="air-quality"] [data-forecast-value]').textContent();
    const scrollAfter = await page.evaluate(() => window.scrollY);

    // require normal vertical page scrolling
    if (centerLine === null || scrolledLine === null) {
      throw new Error("forecast scrolling crosshair is unavailable");
    }

    assert.equal(Math.abs(scrolledLine.x - centerLine.x) < 1, true);
    assert.equal(scrolledAirQuality, centerAirQuality);
    assert.equal(scrollAfter > scrollBefore, true);
    assert.equal(await page.locator(".forecast-chart-value").count(), 9);
    await swipe(bounds.x + 8, touchY, bounds.x + 8, touchY);
    assert.deepEqual(
      await page.locator('[data-forecast-chart="uv-index"]').evaluate(
        // move the left-edge pill opposite the title while retaining its line
        (chart) => ({
          headingTop: chart.querySelector(".forecast-chart-heading")?.classList.contains("forecast-chart-heading-top"),
          valueTop: chart.querySelector(".forecast-chart-value")?.classList.contains("forecast-chart-value-top"),
          valueBottom: chart.querySelector(".forecast-chart-value")?.classList.contains("forecast-chart-value-bottom"),
        }),
      ),
      { headingTop: true, valueBottom: true, valueTop: false },
    );
    assert.deepEqual(
      await page.locator(".forecast-chart-shell").evaluate(
        // square the pill corners that meet the left edge of the selector line
        (chartShell) => {
          const timeStyle = getComputedStyle(chartShell.querySelector(".forecast-shared-crosshair"));
          const clockStyle = getComputedStyle(chartShell.querySelector(".forecast-crosshair-label time"));
          const valueStyle = getComputedStyle(chartShell.querySelector(".forecast-chart-value"));
          return {
            clockLeft: [clockStyle.borderTopLeftRadius, clockStyle.borderBottomLeftRadius],
            lineColor: timeStyle.backgroundColor,
            valueLeft: [valueStyle.borderTopLeftRadius, valueStyle.borderBottomLeftRadius],
          };
        },
      ),
      {
        clockLeft: ["0px", "0px"],
        lineColor: "rgb(0, 0, 0)",
        valueLeft: ["0px", "0px"],
      },
    );
    assert.deepEqual(
      await page.locator(".forecast-x-axis").evaluate(
        // keep the shortened stack's axis sticky and clear of mobile navigation
        (axis) => {
          const navigation = document.querySelector(".section-nav")?.getBoundingClientRect();
          const bounds = axis.getBoundingClientRect();
          return {
            nonOverlapping: navigation !== undefined && bounds.bottom <= navigation.top + 1,
            position: getComputedStyle(axis).position,
            visible: bounds.bottom > 0 && bounds.top < window.innerHeight,
          };
        },
      ),
      { nonOverlapping: true, position: "sticky", visible: true },
    );
    await page.evaluate(() => {
      const chartGrid = document.querySelector(".forecast-chart-grid");

      // move well inside the chart stack
      if (chartGrid !== null) {
        window.scrollTo(0, chartGrid.getBoundingClientRect().top + window.scrollY + 360);
      }
    });
    await page.waitForTimeout(50);
    assert.deepEqual(
      await page.locator(".forecast-chart-shell").evaluate(
        // retain the line labels below the pinned masthead
        (chartShell) => {
          const masthead = document.querySelector(".forecast-masthead")?.getBoundingClientRect();
          const now = chartShell.querySelector(".forecast-current-time-label span")?.getBoundingClientRect();
          const clock = chartShell.querySelector(".forecast-crosshair-label time")?.getBoundingClientRect();
          return {
            clockBelowMasthead: masthead !== undefined && clock !== undefined &&
              clock.top >= masthead.bottom - 1 && clock.top < masthead.bottom + 8,
            labelsAligned: now !== undefined && clock !== undefined && Math.abs(now.top - clock.top) < 1,
            nowBelowMasthead: masthead !== undefined && now !== undefined &&
              now.top >= masthead.bottom - 1 && now.top < masthead.bottom + 8,
          };
        },
      ),
      { clockBelowMasthead: true, labelsAligned: true, nowBelowMasthead: true },
    );
    assert.equal(
      await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth),
      0,
    );
    await page.setViewportSize({ height: 844, width: 320 });
    assert.deepEqual(
      await page.locator(".forecast-chart-shell").evaluate(
        // keep the shared time axis inside the narrow chart shell
        (shell) => {
          const axis = shell.querySelector(".forecast-x-axis")?.getBoundingClientRect();
          const finalTick = shell.querySelector(".forecast-x-axis time:last-child")?.getBoundingClientRect();
          return {
            finalTickContained: axis !== undefined && finalTick !== undefined && finalTick.right <= axis.right,
            overflow: shell.scrollWidth - shell.clientWidth,
          };
        },
      ),
      { finalTickContained: true, overflow: 0 },
    );
    assert.equal(
      await page.locator(".forecast-x-axis time").evaluateAll(
        // keep each compact hour label readable on the narrowest viewport
        (labels) => labels.every((label, index) => {
          const next = labels[index + 1];
          return next === undefined || label.getBoundingClientRect().right <= next.getBoundingClientRect().left;
        }),
      ),
      true,
    );
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// retain complete rolling windows at midnight without showing yesterday on the axis
test("forecast pressure starts at midnight using retained prior-day context", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  try {
    browser = await launchBrowser();
    // check the populated line at desktop and narrow phone widths
    for (const width of [1440, 390, 320]) {
      const page = await createFixturePage(browser, { viewport: { height: 900, width } });
      let includeContext = true;
      await page.route(/\/api\/v1\/sites\/ballydidean\/forecast(?:\?|$)/u,
        // return an older complete run separately from today's newest forecast
        async (route) => {
          const response = await route.fetch();
          const body = await response.json();
          const first = body.data[0];
          const base = Date.parse(first.validAt);
          body.pressureContext = includeContext ? [1_000, 1_001, 1_002, 1_004, 1_007, 1_009].map(
            // preserve one consistent prior vintage across the midnight boundary
            (pressureHpa, index) => ({
              ...first,
              id: `pressure-context-${index}`,
              metrics: { ...first.metrics, pressureHpa },
              productRunAt: new Date(base - 3_600_000).toISOString(),
              validAt: new Date(base + (index - 3) * 3_600_000).toISOString(),
            }),
          ) : [];
          await route.fulfill({ response, json: body });
        },
      );
      await page.goto(`${fixture.origin}/forecast`, { waitUntil: "networkidle" });
      const grid = page.locator("[data-forecast-charts]");
      const pressure = page.locator('[data-forecast-chart="pressure"]');
      assert.deepEqual(JSON.parse(await pressure.getAttribute("data-forecast-series"))[0].values.slice(0, 3), [4, 6, 7]);
      assert.equal((await pressure.locator("polyline").first().getAttribute("points")).split(",")[0], "0.00");
      assert.equal(JSON.parse(await grid.getAttribute("data-forecast-times"))[0], forecast[0].validAt);
      assert.equal(await page.locator(".forecast-x-tick").count(), 24);
      await grid.press("Home");
      assert.equal(await pressure.locator("[data-forecast-value]").innerText(), "+4.0");
      await grid.press("ArrowRight");
      assert.equal(await pressure.locator("[data-forecast-value]").innerText(), "+6.0");
      await grid.press("ArrowRight");
      assert.equal(await pressure.locator("[data-forecast-value]").innerText(), "+7.0");
      // carry the same prior-day context through both horizon reload paths
      for (const days of [5, 10]) {
        await page.getByRole("button", { name: `${days} days`, exact: true }).click();
        await page.waitForFunction(
          // wait for the selected forecast range to finish rendering
          (days) => document.querySelectorAll(".forecast-x-tick").length === days * 24,
          days,
        );
        assert.deepEqual(JSON.parse(await pressure.getAttribute("data-forecast-series"))[0].values.slice(0, 3), [4, 6, 7]);
      }
      includeContext = false;
      await page.reload({ waitUntil: "networkidle" });
      assert.deepEqual(JSON.parse(await pressure.getAttribute("data-forecast-series"))[0].values.slice(0, 3), [null, null, null]);
      assert.equal(await page.locator("[data-forecast-chart]").count(), 9);
      await page.close();
    }
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// verify labeled forecast placeholders
test("forecast skeletons expose every chart label on the reserved cards", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, { viewport: { height: 844, width: 390 } });
    let releaseForecastReads;
    const forecastReadsReleased = new Promise(
      // expose one deterministic forecast gate
      (resolveRelease) => {
        releaseForecastReads = resolveRelease;
      },
    );
    await page.route(
      /\/api\/v1\/sites\/ballydidean\/(?:current|forecast|tides)/u,
      // hold the initial forecast behind its reserved layout
      async (route) => {
        await forecastReadsReleased;
        await route.continue();
      },
    );
    await page.goto(`${fixture.origin}/forecast`, { waitUntil: "domcontentloaded" });
    await assertSkeletonLoadingState(page, ".forecast-panel.skeleton-region");
    assert.deepEqual(
      await page.locator(".skeleton-forecast-chart h3").allTextContents(),
      ["device_thermostatTemperature", "airWind", "rainyRain rate", "cloudClouds", "humidity_percentageHumidity", "masksAir quality", "wb_sunnyUV index", "speedPressure", "waterTide"],
    );
    assert.equal(
      await page.locator(".skeleton-forecast-chart").evaluateAll(
        // keep each label visible over one gray reserved card
        (cards) => cards.every((card) => {
          const label = card.querySelector("h3");
          const background = getComputedStyle(card).backgroundColor;
          const color = label === null ? "transparent" : getComputedStyle(label).color;
          return background !== "rgba(0, 0, 0, 0)" && background !== "rgb(255, 255, 255)" && color !== "transparent";
        }),
      ),
      true,
    );
    assert.equal(
      await page.locator(".skeleton-forecast-chart").evaluateAll(
        // animate every reserved forecast chart beneath its stable label
        (charts) => charts.every((chart) => {
          const shimmer = getComputedStyle(chart, "::after");
          return shimmer.animationName === "skeleton-shimmer" &&
            shimmer.backgroundImage.includes("linear-gradient");
        }),
      ),
      true,
    );
    assert.equal(await page.locator(".skeleton-forecast-map").count(), 0);
    assert.equal(
      fixture.state.requests.some(
        // avoid tile work while the hidden map skeleton is loading
        (entry) => /^GET \/maps\/xweather\/(?:history|forecast)\//u.test(entry),
      ),
      false,
    );
    releaseForecastReads();
    await page.locator(".forecast-panel:not(.skeleton-region)").waitFor();
    await assertSkeletonLoadingSettled(page);

    let releaseFiveDayRead;
    const fiveDayReadReleased = new Promise(
      // expose one subsequent range gate
      (resolveRelease) => {
        releaseFiveDayRead = resolveRelease;
      },
    );
    await page.route(
      /\/api\/v1\/sites\/ballydidean\/forecast\?days=5$/u,
      // hold the replacement range behind the same in-place chart skeletons
      async (route) => {
        await fiveDayReadReleased;
        await route.continue();
      },
    );
    await page.getByRole("button", { name: "5 days", exact: true }).click();
    await assertSkeletonLoadingState(page, ".forecast-panel.skeleton-region");
    assert.equal(await page.getByRole("group", { name: "Forecast range" }).locator("button:disabled").count(), 3);
    releaseFiveDayRead();
    await page.locator('[data-forecast-charts][data-forecast-days="5"]').waitFor();
    await assertSkeletonLoadingSettled(page);

    let releaseFailedRangeRead;
    const failedRangeReadReleased = new Promise(
      // expose one recoverable range failure
      (resolveRelease) => {
        releaseFailedRangeRead = resolveRelease;
      },
    );
    await page.route(
      /\/api\/v1\/sites\/ballydidean\/forecast\?days=10$/u,
      // fail only after the replacement skeleton is visible
      async (route) => {
        await failedRangeReadReleased;
        await route.fulfill({
          contentType: "application/json",
          json: { error: { code: "unavailable" } },
          status: 503,
        });
      },
    );
    await page.getByRole("button", { name: "10 days", exact: true }).click();
    await assertSkeletonLoadingState(page, ".forecast-panel.skeleton-region");
    releaseFailedRangeRead();
    await page.getByRole("alert").waitFor();
    await page.locator(".forecast-panel:not(.skeleton-region)").waitFor();
    await assertSkeletonLoadingSettled(page, /Weather refresh failed/u);
    assert.equal(await page.locator("[data-forecast-charts]").count(), 1);
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// reserve visible trend geometry across responsive entry paths
test("trend skeleton shimmers and preserves desktop and mobile chart geometry", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();

    // cover first loads and client-side navigation at both breakpoints
    for (const { width, navigate } of [
      { width: 960, navigate: false },
      { width: 390, navigate: false },
      { width: 960, navigate: true },
      { width: 390, navigate: true },
    ]) {
      const page = await createFixturePage(browser, { viewport: { height: 844, width } });
      let releaseTrendRead;
      const trendReadReleased = new Promise(
        // expose one deterministic trend gate
        (resolveRelease) => {
          releaseTrendRead = resolveRelease;
        },
      );
      await page.route(
        /\/api\/v1\/sites\/ballydidean\/trends/u,
        // hold each trend read behind its reserved chart
        async (route) => {
          await trendReadReleased;
          await route.continue();
        },
      );

      try {
        // enter trends from either a loaded homepage or a fresh document
        if (navigate) {
          await page.goto(fixture.origin, { waitUntil: "networkidle" });
          await page.getByRole("link", { name: "Trends", exact: true }).click();
        } else {
          await page.goto(`${fixture.origin}/trends`, { waitUntil: "domcontentloaded" });
        }

        await assertSkeletonLoadingState(page, ".skeleton-trend-chart");
        await page.evaluate(
          // stabilize font-dependent geometry before comparison
          () => document.fonts.ready,
        );
        const selectors = [".trends-panel", ".trend-chart", ".trend-chart-viewport", ".trend-chart-landscape"];
        const loadingGeometry = await captureSectionGeometry(page, selectors);
        assert.ok(loadingGeometry[".trend-chart"].height >= 300, `trend skeleton collapsed at ${width}px`);
        assert.ok(loadingGeometry[".trend-chart-viewport"].height >= 300);
        assert.equal(await page.getByRole("region", { name: "Trends" }).getAttribute("aria-busy"), "true");
        assert.equal(await page.locator(".skeleton-trend-chart [data-trend-mode-toggle]").count(), 0);
        assert.equal(await page.getByRole("button", { name: "Show all" }).count(), 0);
        assert.equal(
          await page.locator(".skeleton-trend-chart").evaluate(
            // animate the complete reserved trend surface
            (chart) => getComputedStyle(chart, "::after").animationName,
          ),
          "skeleton-shimmer",
        );
        releaseTrendRead();
        await page.locator(".trends-panel:not(.skeleton-region)").waitFor();
        await assertSkeletonLoadingSettled(page);
        assert.deepEqual(await captureSectionGeometry(page, selectors), loadingGeometry);
      } finally {
        releaseTrendRead();
        await page.close();
      }
    }
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// reserve the public map while keeping local-only settings immediately usable
test("map uses an in-place skeleton while settings stays local-only", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const mapPage = await createFixturePage(browser, { viewport: { height: 900, width: 960 } });
    let releaseMapReads;
    const mapReadsReleased = new Promise(
      // expose one deterministic map gate
      (resolveRelease) => {
        releaseMapReads = resolveRelease;
      },
    );
    await mapPage.route(
      /\/api\/v1\/sites\/ballydidean\/(?:current|property-sensor-layout)/u,
      // hold all map data behind its reserved geography
      async (route) => {
        await mapReadsReleased;
        await route.continue();
      },
    );
    await mapPage.goto(`${fixture.origin}/map`, { waitUntil: "domcontentloaded" });
    await assertSkeletonLoadingState(mapPage, ".station-map-panel.skeleton-region");
    assert.equal(await mapPage.locator(".property-map-panel.skeleton-region").count(), 1);
    assert.equal(await mapPage.locator(".skeleton-map").count(), 2);
    assert.equal(await mapPage.locator(".skeleton-station-list li").count(), 11);
    releaseMapReads();
    await mapPage.locator(".station-map-panel:not(.skeleton-region)").waitFor();
    await mapPage.locator(".property-map-panel:not(.skeleton-region)").waitFor();
    await assertSkeletonLoadingSettled(mapPage);
    await mapPage.close();

    const weatherReadsBeforeSettings = fixture.state.requests.filter(
      // count only site-data API reads
      (entry) => entry.startsWith("GET /api/v1/sites/ballydidean/"),
    ).length;
    const settingsPage = await createFixturePage(browser, { viewport: { height: 844, width: 390 } });
    await settingsPage.goto(`${fixture.origin}/settings`, { waitUntil: "networkidle" });
    await settingsPage.locator(".unit-settings-page").waitFor();
    await assertSkeletonLoadingSettled(settingsPage);
    assert.equal(await settingsPage.locator(".skeleton-line, .skeleton-region").count(), 0);
    assert.equal(fixture.state.requests.filter(
      // keep settings independent from weather data
      (entry) => entry.startsWith("GET /api/v1/sites/ballydidean/"),
    ).length, weatherReadsBeforeSettings);
    await settingsPage.close();
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// replace bootstrap and protected missing-data notices with stable skeletons
test("bootstrap and administrator reads use in-place skeletons", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();

    // verify the static startup shell at phone and desktop widths
    for (const width of [320, 1280]) {
      const page = await createFixturePage(browser, { viewport: { height: 900, width } });
      let releaseClient;
      const clientReleased = new Promise(
        // expose the server-rendered bootstrap before the module mounts
        (resolveRelease) => {
          releaseClient = resolveRelease;
        },
      );
      await page.route(
        /\/assets\/browser-test\/client\.js$/u,
        // hold only the interactive module
        async (route) => {
          await clientReleased;
          await route.continue();
        },
      );
      await page.goto(fixture.origin, { waitUntil: "commit" });
      const shell = page.locator("main.shell.skeleton-region");
      await shell.waitFor();
      assert.equal(await shell.getAttribute("aria-busy"), "true");
      assert.equal(await shell.getByRole("heading", { name: "Ballydídean Weather" }).count(), 1);
      assert.equal(await shell.locator(".home-masthead .masthead-title-text").count(), 1);
      assert.equal(await shell.locator(".weather-content.skeleton-fields .skeleton-field").count(), 3);
      assert.equal(await shell.getByRole("status").evaluate((status) => status.matches(".sr-only")), true);
      assert.equal(await page.locator(".refresh-indicator").count(), 0);
      assert.equal(await page.evaluate(
        // reject bootstrap horizontal overflow
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ), true);
      const bootstrapScreen = await page.screenshot();
      assert.ok(bootstrapScreen.byteLength > 500);
      releaseClient();
      await page.locator(".current-conditions:not(.skeleton-region)").waitFor();
      await assertSkeletonLoadingSettled(page);
      await page.close();
    }

    const adminPage = await createFixturePage(browser, {
      timezoneId: "America/Los_Angeles",
      viewport: { height: 900, width: 960 },
    });
    await adminPage.goto(`${fixture.origin}/admin`, { waitUntil: "networkidle" });
    await adminPage.getByLabel("Password").fill("test-admin-password");
    let releaseAdminReads;
    const adminReadsReleased = new Promise(
      // expose both protected editor skeletons after authentication
      (resolveRelease) => {
        releaseAdminReads = resolveRelease;
      },
    );
    await adminPage.route(
      /\/api\/v1\/(?:sites\/ballydidean\/(?:current|property-sensor-layout)|admin\/sites\/ballydidean\/(?:forecast-adjustment-settings|forecast-adjustment-scorecard))$/u,
      // hold the complete administrator read set
      async (route) => {
        await adminReadsReleased;
        await route.continue();
      },
    );
    await adminPage.getByRole("button", { name: "Sign in" }).click();
    await assertSkeletonLoadingState(adminPage, ".forecast-adjustment-admin.skeleton-region");
    assert.equal(await adminPage.locator(".property-admin.skeleton-region").count(), 1);
    assert.equal(await adminPage.locator(".forecast-adjustment-admin .skeleton-field").count(), 3);
    assert.equal(await adminPage.locator(".adjustment-scorecard .adjustment-scorecard-card").count(), 3);
    assert.equal(await adminPage.locator(".property-admin .skeleton-field").count(), 6);
    assert.equal(await adminPage.locator(".property-admin-map.skeleton-map").count(), 1);
    releaseAdminReads();
    await adminPage.locator("[data-admin-forecast-adjustments]").waitFor();
    await adminPage.locator("[data-property-sensor-form]").waitFor();
    await assertSkeletonLoadingSettled(adminPage);
    await adminPage.close();
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// make humidity discomfort depend on paired air temperature at every screen size
test("humidity colors stay comfortable in cool air and warn only in hot humid air", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  try {
    browser = await launchBrowser();
    // cover both compact and wide card layouts
    for (const width of [320, 1440]) {
      const page = await createFixturePage(browser, { viewport: { height: 900, width } });
      await page.clock.setFixedTime(new Date("2026-08-21T20:00:00Z"));
      let temperatureC = 16;
      // retain the numeric humidity while varying only its paired air temperature
      await page.route("**/api/v1/sites/ballydidean/current", async (route) => {
        const response = await route.fetch();
        const body = await response.json();
        body.data = body.data.map(
          // prevent a feels-like value from acting as the humidity heat gate
          (record) => ({ ...record, metrics: { ...record.metrics, relativeHumidityPercent: 85, temperatureC, apparentTemperatureC: 35 } }),
        );
        await route.fulfill({ response, json: body });
      });
      // keep the humidity maximum cold while another forecast hour is hot
      await page.route("**/api/v1/sites/ballydidean/forecast", async (route) => {
        const response = await route.fetch();
        const body = await response.json();
        body.data = [[95, 16], [75, 30], [85, null], [85, 30]].map(
          // align each humidity and temperature pair with its own hourly color stop
          ([relativeHumidityPercent, temperature], index) => ({
            ...body.data[index],
            validAt: `2026-08-21T${String(18 + index)}:00:00.000Z`,
            metrics: { ...body.data[index].metrics, relativeHumidityPercent, temperatureC: temperature },
          }),
        );
        await route.fulfill({ response, json: body });
      });
      // test known comfort, actual hot humidity and missing temperature separately
      for (const [temperature, color, label] of [
        [16, "rgb(67, 151, 86)", "Comfortable"],
        [30, "rgb(207, 67, 55)", "Very humid"],
        [null, "rgb(136, 136, 130)", "Unavailable"],
      ]) {
        temperatureC = temperature;
        await page.goto(fixture.origin, { waitUntil: "networkidle" });
        const card = page.locator("[data-condition='humidity']");
        assert.equal(await card.locator(".condition-color rect").getAttribute("fill"), color);
        assert.equal(await card.locator(".condition-status").innerText(), label);
        assert.equal(await card.locator(".condition-primary").innerText(), "85%");
        assert.equal(await card.locator(".condition-forecast-reading").getAttribute("class"), "condition-forecast-reading condition-forecast-tone-green");
        assert.match(await card.locator(".condition-forecast-reading").innerText(), /^Max\s*95%$/u);
      }
      await page.goto(`${fixture.origin}/forecast`, { waitUntil: "networkidle" });
      assert.deepEqual(await page.locator("#forecast-line-humidity-0 stop").evaluateAll(
        // include the terminal repeated stop in the pairing check
        (stops) => stops.map((stop) => stop.getAttribute("stop-color")),
      ), ["rgb(67, 151, 86)", "rgb(239, 126, 31)", "rgb(136, 136, 130)", "rgb(207, 67, 55)", "rgb(207, 67, 55)"]);
      await page.close();
    }
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// distinguish current pressure movement from the strongest change across today
test("pressure tile colors three-hour speed and shows the whole-day maximum", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  try {
    browser = await launchBrowser();
    // keep long status labels and signed readings readable at every breakpoint
    for (const width of [1440, 1280, 1100, 960, 880, 420, 390, 320]) {
      const page = await createFixturePage(browser, { timezoneId: "Asia/Tokyo", viewport: { height: 900, width } });
      await page.clock.setFixedTime(new Date("2026-09-18T18:00:00Z"));
      let change = -6.1;
      let freshness = "fresh";
      const errors = [];
      // record uncaught rendering failures
      page.on("pageerror", (error) => errors.push(error.message));
      // preserve each record's pressure while controlling its same-source tendency
      await page.route("**/api/v1/sites/ballydidean/current", async (route) => {
        const response = await route.fetch();
        const body = await response.json();
        // make freshness and tendency travel together through the public contract
        body.data = body.data.map((record) => ({
          ...record,
          pressureChange3hHpa: change,
          freshness: { ...record.freshness, status: freshness },
        }));
        await route.fulfill({ response, json: body });
      });
      // keep a stronger morning change ahead of the smaller afternoon window
      await page.route("**/api/v1/sites/ballydidean/forecast", async (route) => {
        const response = await route.fetch();
        const body = await response.json();
        // include an earlier window ending at five am and a future one ending at two pm
        body.data = body.data.slice(0, 8).map((record, index) => ({
          ...record,
          validAt: new Date(Date.parse(index < 4 ? "2026-09-18T09:00:00Z" : "2026-09-18T18:00:00Z") + index % 4 * 3_600_000).toISOString(),
          metrics: { ...record.metrics, pressureHpa: 1014 - index % 4 * (index < 4 ? 4 : 2) },
        }));
        await route.fulfill({ response, json: body });
      });
      await page.goto(fixture.origin, { waitUntil: "networkidle" });
      const card = page.locator("[data-condition='pressure']");
      assert.equal(await card.locator(".condition-primary").innerText(), "-6.1");
      assert.equal(await card.locator(".condition-status").innerText(), "Very rapid fall");
      assert.equal(await card.locator(".condition-secondary").count(), 0);
      assert.equal(await card.locator(".condition-forecast-reading").count(), 2);
      assert.doesNotMatch(await card.innerText(), /Barometer|Later|By|hPa|3h/u);
      assert.match(await card.locator(".condition-forecast").innerText(), /^Max\s*-12\.0\s*5:00\s*AM$/u);
      assert.equal(await card.locator(".condition-primary small, .condition-forecast-reading:first-child small").count(), 0);
      assert.equal(await card.locator(".condition-forecast-reading").first().getAttribute("class"), "condition-forecast-reading condition-forecast-tone-red");
      assert.equal(await card.locator(".condition-color rect").getAttribute("fill"), "rgb(207, 67, 55)");
      assert.equal(await card.evaluate(
        // reject clipped numbers and overlap between current and predicted changes
        (tile) => {
          const label = tile.querySelector(".condition-label");
          const status = tile.querySelector(".condition-status");
          const primary = tile.querySelector(".condition-primary");
          const forecast = tile.querySelector(".condition-forecast");
          const maximumLabel = forecast.querySelector(".condition-forecast-label").getBoundingClientRect();
          const maximumValue = forecast.querySelector("strong").getBoundingClientRect();
          const primaryBounds = primary.getBoundingClientRect();
          const forecastBounds = forecast.getBoundingClientRect();
          const neighboringPrimary = document.querySelector("[data-condition='uv-index'] .condition-primary").getBoundingClientRect();
          return label.getBoundingClientRect().right <= status.getBoundingClientRect().left &&
            Math.abs(primaryBounds.top - neighboringPrimary.top) < 1 &&
            primaryBounds.right <= forecastBounds.left &&
            maximumLabel.right <= maximumValue.left && maximumLabel.bottom > maximumValue.top &&
            maximumValue.bottom <= forecast.querySelector(".condition-forecast-reading:last-child").getBoundingClientRect().top &&
            [tile, label, status, primary, forecast, ...tile.querySelectorAll("strong")].every(
              // retain the full contents of every displayed reading
              (element) => element.scrollWidth <= element.clientWidth + 1 && element.getBoundingClientRect().bottom <= tile.getBoundingClientRect().bottom,
            );
        },
      ), true, `pressure layout overlaps or clips at ${width}px`);
      const height = (await card.boundingBox()).height;
      // verify the same color scale in both directions without shifting card geometry
      for (const [value, label, color] of [
        [0.1, "Steady", "rgb(0, 146, 63)"],
        [0.5, "Slow rise", "rgb(200, 183, 68)"],
        [-2, "Falling", "rgb(230, 181, 25)"],
        [3.5, "Rapid rise", "rgb(239, 126, 31)"],
        [6, "Very rapid rise", "rgb(207, 67, 55)"],
        [12.3, "Very rapid rise", "rgb(207, 67, 55)"],
        [null, "Unavailable", "rgb(136, 136, 130)"],
      ]) {
        change = value;
        await page.reload({ waitUntil: "networkidle" });
        assert.equal(await card.locator(".condition-status").innerText(), label);
        assert.equal(await card.locator(".condition-color rect").getAttribute("fill"), color);
        assert.equal(await card.locator(".condition-status").evaluate(
          // retain every character in long fallback and directional labels
          (element) => element.scrollWidth <= element.clientWidth + 1,
        ), true, `${label} clips at ${width}px`);
        assert.equal(await card.evaluate(
          // keep every wrapped status label above the maximum and its time
          (tile) => tile.querySelector(".condition-status").getBoundingClientRect().bottom <=
            tile.querySelector(".condition-forecast").getBoundingClientRect().top,
        ), true, `${label} overlaps the maximum at ${width}px`);
        assert.equal(await card.locator("strong").evaluateAll(
          // retain unitless readings when an extreme signed rate needs another digit
          (readings) => readings.every((reading) => reading.scrollWidth <= reading.clientWidth + 1),
        ), true, `${value} clips at ${width}px`);
        assert.equal((await card.boundingBox()).height, height, `${label} changes pressure height at ${width}px`);
      }
      change = 6;
      freshness = "delayed";
      await page.reload({ waitUntil: "networkidle" });
      assert.equal(await card.locator(".condition-primary").innerText(), "—");
      assert.equal(await card.locator(".condition-status").innerText(), "Unavailable");
      assert.deepEqual(errors, []);
      await page.close();
    }
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// keep rain and cloud secondary rows aligned without permanent empty space
async function assertRainCloudRowGeometry(page, width) {
  const geometry = await page.locator("[data-condition='rain'], [data-condition='clouds']").evaluateAll(
    // measure the actual divider and value rows in each adjacent card
    (cards) => cards.map(
      // capture one card's secondary row and forecast boundaries
      (card) => {
        const divider = card.querySelector(".condition-secondary-divider").getBoundingClientRect();
        const value = card.querySelector(".condition-secondary > strong").getBoundingClientRect();
        const body = card.querySelector(".condition-body").getBoundingClientRect();
        return {
          card: card.dataset.condition,
          dividerTop: divider.top,
          valueTop: value.top,
          valueGap: value.top - divider.bottom,
          height: card.getBoundingClientRect().height,
          forecastFits: [...card.querySelectorAll(".condition-forecast-reading")].every(
            // keep forecast units inside the card's normal padding
            (reading) => reading.getBoundingClientRect().right <= body.right + 1,
          ),
        };
      },
    ),
  );
  assert.equal(geometry.length, 2);
  assert.ok(Math.abs(geometry[0].dividerTop - geometry[1].dividerTop) < 1, `secondary dividers differ at ${width}px: ${JSON.stringify(geometry)}`);
  assert.ok(Math.abs(geometry[0].valueTop - geometry[1].valueTop) < 1, `secondary values differ at ${width}px: ${JSON.stringify(geometry)}`);
  // reject stretched gaps between each label and its value
  for (const card of geometry) {
    assert.ok(card.valueGap <= 6, `secondary row is stretched at ${width}px: ${JSON.stringify(card)}`);
    assert.ok(card.forecastFits, `forecast clips at ${width}px: ${JSON.stringify(card)}`);
    // keep ordinary phone ranges compact while separate tests cover longer wrapped ranges
    if (width <= 390) {
      assert.ok(card.height <= 155, `unused vertical space at ${width}px: ${JSON.stringify(card)}`);
    }
  }
}

// keep daylight clarity and all-day cloud extrema usable at every breakpoint
test("clouds tile shows the clearest daylight range and includes night in daily extrema", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();

    // verify wide, tablet and narrow paired layouts in a different browser timezone
    for (const width of [1440, 960, 880, 390, 320]) {
      const page = await createFixturePage(browser, {
        timezoneId: "Asia/Tokyo",
        viewport: { height: 900, width },
      });
      const errors = [];
      // retain uncaught rendering failures
      page.on("pageerror", (error) => errors.push(error.message));
      await page.clock.install({ time: new Date("2026-08-22T06:59:50Z") });
      let forecastDay = 0;
      let forecastReads = 0;
      let currentCloudCover = 100;
      // exercise the longest cloud reading and dry rain status
      await page.route("**/api/v1/sites/ballydidean/current", async (route) => {
        const response = await route.fetch();
        const body = await response.json();
        // keep current cloud status independent of daily forecast extrema
        body.data = body.data.map((record) => ({
          ...record,
          metrics: { ...record.metrics, cloudCoverPercent: currentCloudCover, precipitationRateMmPerHour: 0 },
        }));
        await route.fulfill({ response, json: body });
      });
      // control cloud forecasts without changing fixture globals
      await page.route("**/api/v1/sites/ballydidean/forecast", async (route) => {
        const response = await route.fetch();
        const body = await response.json();
        forecastReads += 1;
        // keep the clearest night separate from the earliest tied daylight window
        body.data = body.data.map((record, index) => {
          const hour = index % 24;
          const daylight = hour >= 7 && hour < 19;
          return {
            ...record,
            metrics: {
              ...record.metrics,
              cloudCoverPercent: daylight ? [11, 12, 15].includes(index) ? 10 : 50 : hour === 0 ? 0 : 100,
              solarRadiationWm2: daylight ? 200 : 0,
            },
          };
        }).slice(forecastDay * 24, (forecastDay + 1) * 24);
        await route.fulfill({ response, json: body });
      });
      await page.goto(fixture.origin, { waitUntil: "networkidle" });
      const tile = page.locator("[data-condition='clouds']");
      await assertRainCloudRowGeometry(page, width);
      assert.equal(await tile.locator(".condition-primary").innerText(), "100%");
      assert.match(await tile.locator(".condition-secondary").innerText(), /Clearest\s*11 AM–1PM/u);
      assert.match(await tile.locator(".condition-secondary-comparison").innerText(), /Overnight\s*12–1AM/u);
      assert.equal(await tile.locator(".condition-status").innerText(), "Heavy");
      assert.equal(await page.locator("[data-condition='rain'] .condition-status").innerText(), "Dry");
      assert.equal(await tile.locator(".condition-forecast").isVisible(), true);
      assert.deepEqual(await tile.locator(".condition-forecast-reading").allTextContents(), ["Max 100%", "Min 0%"]);
      assert.match(await tile.getAttribute("class"), /compact-condition clouds-condition/u);
      assert.equal(await tile.locator(".condition-color rect").getAttribute("fill"), "rgb(105, 133, 155)");
      assert.equal(await tile.evaluate(
        // reject overlapping headings or clipped cloud statistics
        (card) => {
          const label = card.querySelector(".condition-label");
          const status = card.querySelector(".condition-status");
          const primary = card.querySelector(".condition-primary");
          const secondary = card.querySelector(".condition-secondary");
          const extrema = card.querySelector(".condition-forecast");
          const daytime = secondary.querySelector(":scope > strong");
          const overall = secondary.querySelector(".condition-secondary-comparison strong");
          const divider = secondary.querySelector(".condition-secondary-divider");
          const overnight = secondary.querySelector(".condition-secondary-comparison > span");
          const tideLabel = document.querySelector("[data-condition='tide'] .condition-forecast-label");
          const dividerLine = getComputedStyle(divider, "::after");
          return primary.getBoundingClientRect().right <= extrema.getBoundingClientRect().left &&
            extrema.getBoundingClientRect().bottom <= secondary.getBoundingClientRect().top &&
            daytime.getBoundingClientRect().right < overall.getBoundingClientRect().left &&
            Math.abs(divider.getBoundingClientRect().width - secondary.getBoundingClientRect().width) < 1 &&
            dividerLine.content !== "none" && Number.parseFloat(dividerLine.width) > 20 &&
            divider.getBoundingClientRect().bottom < overnight.getBoundingClientRect().top &&
            overnight.getBoundingClientRect().bottom <= overall.getBoundingClientRect().top &&
            getComputedStyle(overnight).fontSize === getComputedStyle(tideLabel).fontSize &&
            Number.parseFloat(getComputedStyle(overall).fontSize) < Number.parseFloat(getComputedStyle(daytime).fontSize) &&
            label.getBoundingClientRect().right <= status.getBoundingClientRect().left &&
            [card, label, primary, secondary, extrema].every(
              // require full content width at narrow breakpoints
              (element) => element.scrollWidth <= element.clientWidth,
            );
        },
      ), true, `clouds layout overlaps or clips at ${width}px`);
      assert.deepEqual(await page.locator(".condition-card:nth-last-child(-n+2)").evaluateAll(
        // retain the final pair in document order
        (cards) => cards.map(
          // read each final card identity
          (card) => card.dataset.condition,
        ),
      ), ["tide", "sunset"]);
      assert.equal(await page.locator(".current-conditions").evaluate(
        // keep tide immediately beside sunset with equal widths
        (grid) => {
          const tide = grid.querySelector("[data-condition='tide']").getBoundingClientRect();
          const sunset = grid.querySelector("[data-condition='sunset']").getBoundingClientRect();
          return Math.abs(tide.top - sunset.top) < 1 && Math.abs(tide.width - sunset.width) < 1 && tide.right < sunset.left;
        },
      ), true, `final pair is not adjacent at ${width}px`);

      // fetch the new one-day forecast at farm midnight without a page reload
      forecastDay = 1;
      currentCloudCover = 0;
      const refreshed = page.waitForResponse("**/api/v1/sites/ballydidean/forecast");
      await page.clock.fastForward(10_100);
      await refreshed;
      await page.locator(".weather-content[aria-busy='false']").waitFor();
      assert.equal(await page.locator(".refresh-indicator").count(), 0);
      assert.equal(forecastReads, 2);
      assert.equal(await tile.locator(".condition-status").innerText(), "Clear");
      assert.equal(await tile.locator(".condition-primary").innerText(), "0%");
      assert.deepEqual(await tile.locator(".condition-forecast-reading").allTextContents(), ["Max 100%", "Min 0%"]);
      assert.match(await tile.locator(".condition-secondary").innerText(), /Clearest\s*7 AM–7PM/u);
      assert.match(await tile.locator(".condition-secondary-comparison").innerText(), /Overnight\s*12–1AM/u);
      assert.equal(await tile.locator(".condition-secondary").evaluate(
        // keep a full daylight range inside its reserved row
        (secondary) => secondary.scrollWidth <= secondary.clientWidth && secondary.scrollHeight <= secondary.clientHeight,
      ), true, `daylight cloud range clips at ${width}px`);
      assert.deepEqual(errors, []);
      await page.close();
    }

    const page = await createFixturePage(browser, { viewport: { height: 900, width: 390 } });
    let cloudCover = 42;
    // exercise light clouds and then unavailable model statistics
    await page.route(/\/api\/v1\/sites\/ballydidean\/(?:current|forecast)/u, async (route) => {
      const response = await route.fetch();
      const body = await response.json();
      // update both cloud summaries without affecting other readings
      body.data = body.data.map((record) => ({ ...record, metrics: { ...record.metrics, cloudCoverPercent: cloudCover } }));
      await route.fulfill({ response, json: body });
    });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    const tile = page.locator("[data-condition='clouds']");
    assert.equal(await tile.locator(".condition-status").innerText(), "Light");
    cloudCover = null;
    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await tile.locator(".condition-primary").innerText(), "—");
    assert.equal(await tile.locator(".condition-status").innerText(), "Unavailable");
    assert.deepEqual(await tile.locator(".condition-forecast-reading").allTextContents(), ["Max —", "Min —"]);
    assert.match(await tile.locator(".condition-secondary").innerText(), /Clearest\s*—/u);
    assert.equal(await tile.locator(".condition-secondary-comparison").count(), 0);
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// hide redundant overall windows and reserve extra height only for wrapped content
test("clouds secondary shows overall only when its clearest time range differs", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, { viewport: { height: 900, width: 320 } });
    await page.clock.setFixedTime(new Date("2026-08-21T20:00:00Z"));
    let nightTied = false;
    // choose an identical daytime minimum or an earlier tied night window
    await page.route("**/api/v1/sites/ballydidean/forecast", async (route) => {
      const response = await route.fetch();
      const body = await response.json();
      // keep equal cloud percentages while changing only the earliest overall window
      body.data = body.data.slice(0, 24).map((record, hour) => ({
        ...record,
        metrics: { ...record.metrics, cloudCoverPercent: [11, 12].includes(hour) || (nightTied && hour === 0) ? 10 : 80 },
      }));
      await route.fulfill({ response, json: body });
    });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    const tile = page.locator("[data-condition='clouds']");
    const daytime = tile.locator(".condition-secondary > strong");
    assert.equal(await daytime.innerText(), "11 AM–1PM");
    assert.equal(await tile.locator(".condition-secondary-comparison").count(), 0);
    const initialHeight = (await tile.boundingBox()).height;
    await assertRainCloudRowGeometry(page, 320);
    nightTied = true;
    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await daytime.innerText(), "11 AM–1PM");
    assert.equal(await tile.locator(".condition-secondary-comparison strong").innerText(), "12–1AM");
    await assertRainCloudRowGeometry(page, 320);
    assert.ok((await tile.boundingBox()).height <= initialHeight + 12);
    nightTied = false;
    await page.reload({ waitUntil: "networkidle" });
    assert.equal(await tile.locator(".condition-secondary-comparison").count(), 0);
    assert.equal((await tile.boundingBox()).height, initialHeight);
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// round the daylight-selected range without changing sunset precision
test("clouds range rounds sunrise and sunset to hours on desktop and mobile", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();

    // retain the full daylight range in a foreign browser timezone
    for (const width of [1440, 320]) {
      const page = await createFixturePage(browser, { timezoneId: "Asia/Tokyo", viewport: { height: 900, width } });
      await page.clock.setFixedTime(new Date("2026-09-17T20:00:00Z"));
      // keep night extrema distinct and modeled light unavailable
      await page.route("**/api/v1/sites/ballydidean/forecast", async (route) => {
        const response = await route.fetch();
        const body = await response.json();
        // include partial dawn and dusk bins in the tied daylight minimum
        body.data = body.data.slice(0, 24).map((record, hour) => ({
          ...record,
          validAt: new Date(Date.parse("2026-09-17T07:00:00Z") + hour * 3_600_000).toISOString(),
          metrics: { ...record.metrics, cloudCoverPercent: hour === 0 ? 0 : hour === 23 ? 100 : 10, solarRadiationWm2: null, uvIndex: null },
        }));
        await route.fulfill({ response, json: body });
      });
      await page.goto(fixture.origin, { waitUntil: "networkidle" });
      const tile = page.locator("[data-condition='clouds']");
      assert.equal((await tile.locator(".condition-secondary > strong").innerText()).replace(/\s+/gu, " "), "7 AM–7PM");
      assert.equal(await tile.locator(".condition-secondary-comparison strong").innerText(), "12–1AM");
      assert.equal(await page.locator("[data-condition='sunset'] .condition-primary").innerText(), "7:17PM");
      assert.deepEqual(await tile.locator(".condition-forecast-reading").allTextContents(), ["Max 100%", "Min 0%"]);
      assert.equal(await tile.locator(".condition-secondary").evaluate(
        // retain the rounded range without clipping its reserved row
        (element) => element.scrollWidth <= element.clientWidth && element.scrollHeight <= element.clientHeight,
      ), true, `sunrise–sunset range clips at ${width}px`);
      await page.close();
    }
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// exclude repeated nighttime hours while retaining their full-day extrema
test("clouds range stays unavailable for a night-only daylight-saving forecast", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, { timezoneId: "Asia/Tokyo", viewport: { height: 900, width: 320 } });
    await page.clock.setFixedTime(new Date("2026-11-01T20:00:00Z"));
    // isolate the first one-am bin on the farm's twenty-five-hour day
    await page.route("**/api/v1/sites/ballydidean/forecast", async (route) => {
      const response = await route.fetch();
      const body = await response.json();
      body.data = [{ ...body.data[0], validAt: "2026-11-01T08:00:00Z" }];
      await route.fulfill({ response, json: body });
    });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    const secondary = page.locator("[data-condition='clouds'] .condition-secondary");
    assert.equal(await secondary.locator(":scope > strong").innerText(), "—");
    assert.equal(await secondary.locator(".condition-secondary-comparison strong").innerText(), "1 AM PDT–1 AM PST");
    assert.deepEqual(await page.locator("[data-condition='clouds'] .condition-forecast-reading").allTextContents(), ["Max 42%", "Min 42%"]);
    assert.equal(await secondary.evaluate(
      // preserve the compact row for an unavailable daylight range
      (element) => element.scrollWidth <= element.clientWidth && element.scrollHeight <= element.clientHeight,
    ), true);
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// display farm-local evening times and their daily change in another browser timezone
test("sunset tile shows today's sunset and golden hour on desktop and mobile", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();

    // cover desktop and narrow paired sunset cards
    for (const width of [1280, 960, 881, 880, 390, 360, 320]) {
      const page = await createFixturePage(browser, {
        timezoneId: "Asia/Tokyo",
        viewport: { height: 900, width },
      });
      const errors = [];
      // record uncaught browser errors
      page.on("pageerror", (error) => errors.push(error.message));
      await page.clock.setFixedTime(new Date("2026-09-12T20:00:00Z"));
      await page.goto(fixture.origin, { waitUntil: "networkidle" });
      const tile = page.locator("[data-condition='sunset']");
      await tile.waitFor();
      assert.equal(await tile.locator(".condition-primary").innerText(), "7:28PM");
      assert.equal(await tile.locator(".condition-primary strong").evaluate(
        // use the shared primary size rather than a smaller sunset-only scale
        (reading) => getComputedStyle(reading).fontSize ===
          getComputedStyle(document.querySelector("[data-condition='temperature'] .condition-primary strong")).fontSize,
      ), true, `sunset primary size differs at ${width}px`);
      assert.match(await tile.locator(".condition-secondary").innerText(), /Golden hour\s*6:47PM/u);
      assert.match(await tile.locator(".condition-status").innerText(), /Today/u);
      assert.equal(await tile.locator(".condition-forecast-label").innerText(), "vs yesterday");
      assert.equal(await tile.locator(".condition-forecast strong").innerText(), "-2 mins");
      assert.equal(await tile.evaluate(
        // require the comparison to fit to the right without clipping or overlap
        (card) => {
          const label = card.querySelector(".condition-label");
          const status = card.querySelector(".condition-status");
          const primary = card.querySelector(".condition-primary");
          const secondary = card.querySelector(".condition-secondary");
          const comparison = card.querySelector(".condition-forecast");
          const sunset = card.querySelector(".condition-primary strong");

          // require each part of the visual hierarchy
          if (!label || !status || !primary || !secondary || !comparison || !sunset) {
            return false;
          }

          const contentsFit = [card, label, primary, secondary, comparison].every(
            // check full rendered content widths
            (element) => element.scrollWidth <= element.clientWidth,
          );
          return contentsFit && label.getBoundingClientRect().right <= status.getBoundingClientRect().left &&
            primary.getBoundingClientRect().bottom <= secondary.getBoundingClientRect().top &&
            sunset.getBoundingClientRect().right <= comparison.getBoundingClientRect().left &&
            comparison.getBoundingClientRect().bottom <= secondary.getBoundingClientRect().top;
        },
      ), true, `sunset layout overlaps or clips at ${width}px`);
      const screenshot = await tile.screenshot();
      assert.ok(screenshot.byteLength > 1_000);

      await page.clock.setFixedTime(new Date("2026-09-13T06:59:59Z"));
      await page.reload({ waitUntil: "networkidle" });
      assert.equal(await tile.locator(".condition-primary").innerText(), "7:28PM");
      assert.match(await tile.locator(".condition-secondary").innerText(), /6:47PM/u);

      await page.clock.setFixedTime(new Date("2026-09-13T07:00:00Z"));
      await page.reload({ waitUntil: "networkidle" });
      assert.equal(await tile.locator(".condition-primary").innerText(), "7:26PM");
      assert.match(await tile.locator(".condition-secondary").innerText(), /6:45PM/u);
      assert.equal(await tile.locator(".condition-forecast strong").innerText(), "-2 mins");

      // show later and unchanged sunsets without losing the signed comparison
      for (const [day, expected] of [["2026-01-15", "+2 mins"], ["2026-06-21", "0 mins"]]) {
        await page.clock.setFixedTime(new Date(`${day}T20:00:00Z`));
        await page.reload({ waitUntil: "networkidle" });
        assert.equal(await tile.locator(".condition-forecast strong").innerText(), expected);
      }
      assert.deepEqual(errors, []);
      await page.close();
    }
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// roll an open homepage into the next farm day and refresh its one-day forecast
test("sunset refreshes at farm midnight and when a suspended tab resumes", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, { timezoneId: "Asia/Tokyo" });
    await page.clock.install({ time: new Date("2026-09-13T06:59:50Z") });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    const tile = page.locator("[data-condition='sunset']");
    assert.equal(await tile.locator(".condition-primary").innerText(), "7:28PM");
    const forecastPath = "GET /api/v1/sites/ballydidean/forecast";
    const initialForecastReads = fixture.state.requests.filter(
      // count complete day forecast reads
      (entry) => entry === forecastPath,
    ).length;
    const midnightRefresh = page.waitForResponse("**/api/v1/sites/ballydidean/forecast");
    await page.clock.fastForward(11_000);
    await midnightRefresh;
    await page.locator(".weather-content[aria-busy='false']").waitFor();
    assert.equal(await page.locator(".refresh-indicator").count(), 0);
    assert.equal(await tile.locator(".condition-primary").innerText(), "7:26PM");
    assert.match(await tile.locator(".condition-secondary").innerText(), /6:45PM/u);
    assert.equal(await tile.locator(".condition-forecast strong").innerText(), "-2 mins");
    assert.equal(fixture.state.requests.filter(
      // fetch today's replacement forecast once
      (entry) => entry === forecastPath,
    ).length, initialForecastReads + 1);

    const resumedRefresh = page.waitForResponse("**/api/v1/sites/ballydidean/forecast");
    await page.clock.setSystemTime(new Date("2026-09-15T19:00:00Z"));
    await page.evaluate(
      // simulate resuming after background timers were suspended
      () => document.dispatchEvent(new Event("visibilitychange")),
    );
    await resumedRefresh;
    await page.locator(".weather-content[aria-busy='false']").waitFor();
    assert.equal(await page.locator(".refresh-indicator").count(), 0);
    assert.equal(await tile.locator(".condition-primary").innerText(), "7:22PM");
    assert.match(await tile.locator(".condition-secondary").innerText(), /6:41PM/u);
    assert.equal(await tile.locator(".condition-forecast strong").innerText(), "-2 mins");
    assert.equal(fixture.state.requests.filter(
      // refresh once after resuming on a later farm day
      (entry) => entry === forecastPath,
    ).length, initialForecastReads + 2);
    await page.evaluate(
      // same-day visibility changes must not reread weather
      () => document.dispatchEvent(new Event("visibilitychange")),
    );
    assert.equal(fixture.state.requests.filter(
      // retain the bounded day-refresh count
      (entry) => entry === forecastPath,
    ).length, initialForecastReads + 2);
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// preserve responsive labels and geometry while current weather loads
test("initial skeletons preserve homepage geometry while weather data loads", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();

    // verify both compact and wide responsive layouts
    for (const width of [390, 960]) {
      const page = await createFixturePage(browser, { viewport: { height: 900, width } });
      let releaseWeatherReads;
      const weatherReadsReleased = new Promise(
        // expose one deterministic response gate
        (resolveRelease) => {
          releaseWeatherReads = resolveRelease;
        },
      );
      await page.route(
        /\/api\/v1\/sites\/ballydidean\/(?:current|forecast|trends)/u,
        // hold first-load weather data behind the skeletons
        async (route) => {
          await weatherReadsReleased;
          await route.continue();
        },
      );
      await page.goto(fixture.origin, { waitUntil: "domcontentloaded" });
      await assertSkeletonLoadingState(page, ".current-conditions.skeleton-region");
      assert.equal(await page.locator(".skeleton-card").count(), 10);
      // abbreviate only the compact homepage label
      const temperatureLabel = page.locator('[data-condition="temperature"] .condition-label > span:last-child');
      const expectedTemperatureLabel = width <= 672 ? "TEMP" : "TEMPERATURE";
      assert.equal(await temperatureLabel.innerText(), expectedTemperatureLabel);
      assert.equal(await page.locator('[data-condition="temperature"] .condition-secondary > span').innerText(), "Air Temp");
      assert.equal(
        await page.locator(".skeleton-card").evaluateAll(
          // animate every reserved condition card
          (cards) => cards.every(
            (card) => getComputedStyle(card, "::after").animationName === "skeleton-shimmer",
          ),
        ),
        true,
      );
      assert.equal(await page.locator(".skeleton-forecast-chart").count(), 0);
      assert.equal(await page.locator(".skeleton-trend-chart").count(), 0);
      assert.equal(await page.getByText("No current model value is available yet.").count(), 0);
      const selectors = [
        ".masthead",
        ".current-conditions",
        ".credits",
      ];
      const loadingGeometry = await captureSectionGeometry(page, selectors);
      const loadingCardHeights = await page.locator(".condition-card").evaluateAll(
        // capture every responsive card track
        (cards) => cards.map((card) => card.getBoundingClientRect().height),
      );
      releaseWeatherReads();
      await page.locator(".current-conditions:not(.skeleton-region)").waitFor();
      await page.waitForFunction(
        // require every first-load skeleton to clear
        () => document.querySelector(".skeleton-region") === null,
      );
      await assertSkeletonLoadingSettled(page);
      assert.equal(await temperatureLabel.innerText(), expectedTemperatureLabel);
      assert.equal(await page.locator('[data-condition="temperature"] .condition-secondary > span').innerText(), "Air Temp");
      const loadedGeometry = await captureSectionGeometry(page, selectors);
      const loadedCardHeights = await page.locator(".condition-card").evaluateAll(
        // recapture every responsive card track
        (cards) => cards.map((card) => card.getBoundingClientRect().height),
      );

      assert.deepEqual(loadedCardHeights, loadingCardHeights);
      assert.deepEqual(loadedGeometry, loadingGeometry);
      assert.equal(await page.locator(".skeleton-line").count(), 0);
      assert.equal(await page.locator("body").evaluate(
        // reject responsive horizontal overflow
        (body) => body.scrollWidth > document.documentElement.clientWidth,
      ), false);
      await page.close();
    }
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

test("history skeletons cover initial reads, pagination, and empty results", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const historyRecords = Array.from({ length: 25 },
      // create one representative full history page
      (_, index) => makeRecord(
        String(400 + index),
        new Date(Date.UTC(2026, 7, 22, 4, 50 - index)).toISOString(),
        16.2 - index * 0.1,
      ),
    );

    // verify both compact and wide history layouts
    for (const width of [390, 960]) {
      const page = await createFixturePage(browser, { viewport: { height: 900, width } });
      let releaseHistoryRead;
      const historyReadReleased = new Promise(
        // expose one deterministic history gate
        (resolveRelease) => {
          releaseHistoryRead = resolveRelease;
        },
      );
      await page.route(
        /\/api\/v1\/sites\/ballydidean\/history/u,
        // hold first-load history behind the skeleton rows
        async (route) => {
          await historyReadReleased;
          await route.fulfill({
            contentType: "application/json",
            json: {
              data: historyRecords,
              page: { limit: 25, nextCursor: "older-page" },
              site,
            },
            status: 200,
          });
        },
      );
      await page.goto(`${fixture.origin}/logs`, { waitUntil: "domcontentloaded" });
      await assertSkeletonLoadingState(page, ".skeleton-history-row");
      assert.equal(await page.locator(".skeleton-history-row").count(), 25);
      assert.equal(await page.locator(".skeleton-history-card").count(), 25);
      assert.equal(
        await page.locator(".skeleton-history-row td, .skeleton-history-card").evaluateAll(
          // animate every reserved desktop and mobile history surface
          (skeletons) => skeletons.every(
            (skeleton) => getComputedStyle(skeleton, "::after").animationName === "skeleton-shimmer",
          ),
        ),
        true,
      );
      assert.equal(await page.getByText("No records match these filters.").count(), 0);
      const selectors = [".masthead", ".section-nav", ".weather-content > .panel", ".pagination", ".credits"];
      const loadingGeometry = await captureSectionGeometry(page, selectors);
      releaseHistoryRead();
      await page.locator(".skeleton-history-row").first().waitFor({ state: "detached" });
      await page.locator("tbody tr:not(.skeleton-history-row)").first().waitFor({ state: "attached" });
      await assertSkeletonLoadingSettled(page);
      const loadedGeometry = await captureSectionGeometry(page, selectors);

      // expose compact source provenance as a disclosure
      if (width === 390) {
        assert.deepEqual(
          await page.locator(".history-card details summary").first().evaluate((summary) => ({
            cursor: getComputedStyle(summary).cursor,
            indicator: getComputedStyle(summary, "::after").content,
          })),
          { cursor: "pointer", indicator: '"›"' },
        );
      }

      assert.deepEqual(loadedGeometry, loadingGeometry);
      assert.equal(await page.locator(".skeleton-line").count(), 0);

      // exercise subsequent pagination and an honest empty filter once
      if (width === 390) {
        let releaseNextPageRead;
        const nextPageReadReleased = new Promise(
          // expose one cursor-page gate
          (resolveRelease) => {
            releaseNextPageRead = resolveRelease;
          },
        );
        await page.route(
          (url) => url.pathname === "/api/v1/sites/ballydidean/history" && url.searchParams.has("cursor"),
          // reserve the full result page while the cursor advances
          async (route) => {
            await nextPageReadReleased;
            await route.fulfill({
              contentType: "application/json",
              json: {
                data: [older],
                page: { limit: 25, nextCursor: null },
                site,
              },
              status: 200,
            });
          },
        );
        await page.getByRole("button", { name: "Next", exact: true }).click();
        await assertSkeletonLoadingState(page, ".skeleton-history-row");
        assert.equal(await page.getByRole("button", { name: "Previous", exact: true }).isDisabled(), true);
        assert.equal(await page.getByRole("button", { name: "Next", exact: true }).isDisabled(), true);
        releaseNextPageRead();
        await page.locator(".history-card strong").filter({ hasText: "59.2" }).waitFor();
        await assertSkeletonLoadingSettled(page);

        let releaseEmptyRead;
        const emptyReadReleased = new Promise(
          // expose one filtered empty-state gate
          (resolveRelease) => {
            releaseEmptyRead = resolveRelease;
          },
        );
        await page.route(
          /\/api\/v1\/sites\/ballydidean\/history/u,
          // return an honest empty page after its skeleton
          async (route) => {
            await emptyReadReleased;
            await route.fulfill({
              contentType: "application/json",
              json: {
                data: [],
                page: { limit: 25, nextCursor: null },
                site,
              },
              status: 200,
            });
          },
        );
        await page.locator(".history-filter-disclosure").evaluate(
          // expose the compact filter form for pointer interaction
          (details) => {
            details.open = true;
          },
        );
        await page.locator("select[name='sourceKind']").selectOption("reanalysis");
        await page.getByRole("button", { name: "Apply filters", exact: true }).click();
        await assertSkeletonLoadingState(page, ".skeleton-history-row");
        releaseEmptyRead();
        await page.locator(".history-cards-empty").waitFor();
        await assertSkeletonLoadingSettled(page);
      }
      await page.close();
    }
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

test("real browser rejects Los Angeles DST gaps and overlaps from UTC without losing last-good data", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;
  const pageErrors = [];

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, {
      timezoneId: "UTC",
      viewport: { height: 900, width: 1440 },
    });
    // capture unexpected handler errors
    page.on("pageerror", (error) => {
      pageErrors.push(error.message);
    });
    await page.goto(`${fixture.origin}/logs`, { waitUntil: "networkidle" });
    assert.equal(
      await page.evaluate(
        // read the actual browser timezone
        () => Intl.DateTimeFormat().resolvedOptions().timeZone,
      ),
      "UTC",
    );

    await assertRejectedSiteWallClock(
      page,
      fixture,
      "from",
      "2026-03-08T02:30",
    );
    await assertRejectedSiteWallClock(
      page,
      fixture,
      "to",
      "2026-11-01T01:30",
    );
    assert.deepEqual(pageErrors, []);
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// retain unit preferences and geometry across all condition cards
// preserve unit preferences and comparison styling across the complete dashboard
test("real browser configures and persists every measurement unit preference", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, { viewport: { height: 900, width: 960 } });
    // align historical fixture forecasts with their original farm day
    await page.clock.setFixedTime(new Date("2026-08-22T05:37:00.000Z"));
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    const currentTemperature = page.locator("[data-condition='temperature']");
    assert.match(await currentTemperature.locator(".condition-primary").textContent() ?? "", /52\s*°F/u);
    assert.match(await currentTemperature.textContent() ?? "", /Air Temp\s*54\s*°F/u);
    const currentWind = page.locator("[data-condition='wind']");
    assert.match(await currentWind.textContent() ?? "", /Wind\s*Breezy\s*6\s*mph SW/u);
    assert.match(await currentWind.textContent() ?? "", /Gusts\s*10\s*mph/u);
    assert.equal(
      await page.locator(".condition-card:not(.pressure-condition) .condition-primary strong").evaluateAll(
        // keep sunset and the other measurements on the shared primary scale
        (readings) => new Set(readings.map(
          // read one primary scale
          (reading) => getComputedStyle(reading).fontSize,
        )).size,
      ),
      1,
    );
    const secondarySectionGeometry = await page
      .locator("[data-condition='temperature'], [data-condition='wind'], [data-condition='rain'], [data-condition='tide']")
      .evaluateAll(
        // reserve two forecast rows inside every secondary section
        (cards) => cards.map(
          // measure one paired section
          (card) => {
            const secondary = card.querySelector(".condition-secondary");
            const forecast = card.querySelector(".condition-forecast-readings");
            const row = forecast?.querySelector(".condition-forecast-reading");
            const value = row?.querySelector("strong");

            // require the complete paired layout
            if (secondary === null || forecast === null || row === null || value === null || value === undefined) {
              return null;
            }

            const rootFontSize = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
            const rowGap = Number.parseFloat(getComputedStyle(forecast).rowGap);
            const rowHeight = value.getBoundingClientRect().height;
            return {
              height: secondary.getBoundingClientRect().height,
              requiredHeight: rowHeight * 2 + rowGap + rootFontSize,
            };
          },
        ),
      );
    assert.equal(secondarySectionGeometry.includes(null), false);
    assert.equal(
      new Set(secondarySectionGeometry.map(
        // compare one rendered section height
        (geometry) => geometry?.height,
      )).size,
      1,
      JSON.stringify(secondarySectionGeometry),
    );
    assert.equal(
      secondarySectionGeometry.every(
        // retain one rem of normal breathing room
        (geometry) => geometry !== null && geometry.height >= geometry.requiredHeight,
      ),
      true,
      JSON.stringify(secondarySectionGeometry),
    );
    const pairedDividerTops = await page.locator("[data-condition='temperature'], [data-condition='wind']").evaluateAll(
      // measure paired secondary dividers
      (cards) => cards.map(
        // capture one divider position
        (card) => card.querySelector(".condition-secondary-divider")?.getBoundingClientRect().top ?? null,
      ),
    );
    assert.equal(pairedDividerTops.includes(null), false);
    assert.ok(
      Math.abs((pairedDividerTops[0] ?? 0) - (pairedDividerTops[1] ?? 0)) <= 1,
      JSON.stringify(pairedDividerTops),
    );
    assert.equal(
      await page.locator("[data-condition='temperature'], [data-condition='wind'], [data-condition='rain'], [data-condition='tide']").evaluateAll(
        // keep each secondary label and reading flush left
        (cards) => cards.every(
          // verify one secondary reading
          (card) => {
            const divider = card.querySelector(".condition-secondary-divider");
            const reading = card.querySelector(".condition-secondary strong");

            // require both secondary elements
            if (divider === null || reading === null) {
              return false;
            }

            const body = card.querySelector(".condition-body");

            // require the full card body boundary
            if (body === null) {
              return false;
            }

            return divider.getBoundingClientRect().bottom <= reading.getBoundingClientRect().top &&
              Math.abs(divider.getBoundingClientRect().left - reading.getBoundingClientRect().left) <= 1 &&
              Math.abs(divider.getBoundingClientRect().right - body.getBoundingClientRect().right) <= 1 &&
              getComputedStyle(divider, "::before").content === "none" &&
              Number.parseFloat(getComputedStyle(divider, "::after").width) > 0;
          },
        ),
      ),
      true,
    );
    assert.equal(
      await currentTemperature.evaluate(
        // keep the apparent-temperature forecast below the full divider
        (card) => {
          const divider = card.querySelector(".condition-secondary-divider");
          const apparentMaximum = card.querySelector(".condition-forecast-reading:nth-child(3) strong");

          // require both temperature rows
          if (divider === null || apparentMaximum === null) {
            return false;
          }

          return apparentMaximum.getBoundingClientRect().top >= divider.getBoundingClientRect().bottom;
        },
      ),
      true,
    );
    assert.equal(await page.locator("[data-condition='pressure'] .condition-primary").innerText(), "—");
    const currentTide = page.locator("[data-condition='tide']");
    assert.match(await currentTide.locator(".condition-status").textContent() ?? "", /High/u);
    assert.match(await currentTide.locator(".condition-primary").textContent() ?? "", /8\.2\s*ft/u);
    assert.match(await currentTide.locator(".condition-secondary").textContent() ?? "", /Direction\s*Rising/u);
    assert.equal(await currentTide.locator(".condition-detail").count(), 0);
    const tideForecastAlignment = await currentTide.evaluate(
      // align the next turn with the direction reading
      (card) => {
        const direction = card.querySelector(".condition-secondary strong");
        const forecast = card.querySelector(".condition-forecast-label");

        // require both tide sections
        if (direction === null || forecast === null) {
          return null;
        }

        return {
          directionTop: direction.getBoundingClientRect().top,
          forecastTop: forecast.getBoundingClientRect().top,
        };
      },
    );
    assert.equal(
      tideForecastAlignment !== null &&
        Math.abs(tideForecastAlignment.forecastTop - tideForecastAlignment.directionTop) <= 4,
      true,
      JSON.stringify(tideForecastAlignment),
    );
    assert.doesNotMatch(await currentTide.textContent() ?? "", /Forecast/u);
    assert.match(await currentTide.textContent() ?? "", /Rising/u);
    assert.doesNotMatch(await page.locator(".current-conditions").textContent() ?? "", /Next 24h/u);
    assert.match(await currentTemperature.textContent() ?? "", /Max\s*67°F\s*Min\s*49°F\s*Max\s*68°F\s*Min\s*50°F/u);
    assert.match(await currentWind.textContent() ?? "", /Max\s*8 mph\s*Max\s*16 mph/u);
    const currentRain = page.locator("[data-condition='rain']");
    assert.match(await currentRain.textContent() ?? "", /Rain/u);
    assert.match(await currentRain.textContent() ?? "", /Max 0\.1 in\/h/u);
    assert.match(await currentRain.textContent() ?? "", /Accumulation\s*0\.1\s*in/u);
    assert.match(await currentRain.textContent() ?? "", /Total 0\.19 in/u);
    assert.match(await page.locator("[data-condition='air-quality']").textContent() ?? "", /Max 10/u);
    assert.doesNotMatch(await page.locator("[data-condition='air-quality']").textContent() ?? "", /µg\/m³/u);
    assert.match(await page.locator("[data-condition='uv-index']").textContent() ?? "", /Max 8/u);
    assert.match(await page.locator("[data-condition='pressure']").textContent() ?? "", /Max\s*-2\.3/u);
    assert.match(await page.locator("[data-condition='humidity']").textContent() ?? "", /Max 74%/u);
    assert.match(await currentTide.textContent() ?? "", /Next low\s*5:00 AM/u);
    assert.deepEqual(
      await page.locator(".condition-forecast-reading").evaluateAll(
        // retain the requested threshold colors at consistent emphasis
        (readings) => readings.map((reading) => ({
          color: getComputedStyle(reading).color,
          condition: reading.closest("[data-condition]")?.getAttribute("data-condition"),
          opacity: getComputedStyle(reading).opacity,
        })),
      ),
      [
        { color: "rgb(67, 151, 86)", condition: "temperature", opacity: "0.75" },
        { color: "rgb(56, 120, 197)", condition: "temperature", opacity: "0.75" },
        { color: "rgb(67, 151, 86)", condition: "temperature", opacity: "0.75" },
        { color: "rgb(56, 120, 197)", condition: "temperature", opacity: "0.75" },
        { color: "rgb(67, 151, 86)", condition: "wind", opacity: "0.75" },
        { color: "rgb(230, 181, 25)", condition: "wind", opacity: "0.75" },
        { color: "rgb(56, 120, 197)", condition: "rain", opacity: "0.75" },
        { color: "rgb(0, 0, 0)", condition: "rain", opacity: "0.75" },
        { color: "rgb(0, 0, 0)", condition: "clouds", opacity: "0.75" },
        { color: "rgb(0, 0, 0)", condition: "clouds", opacity: "0.75" },
        { color: "rgb(67, 151, 86)", condition: "humidity", opacity: "0.75" },
        { color: "rgb(230, 181, 25)", condition: "air-quality", opacity: "0.75" },
        { color: "rgb(230, 181, 25)", condition: "pressure", opacity: "0.75" },
        { color: "rgb(0, 0, 0)", condition: "pressure", opacity: "0.75" },
        { color: "rgb(207, 67, 55)", condition: "uv-index", opacity: "0.75" },
        { color: "rgb(0, 0, 0)", condition: "tide", opacity: "0.75" },
        { color: "rgb(0, 0, 0)", condition: "sunset", opacity: "0.75" },
      ],
    );
    assert.equal(await page.locator(".condition-card").count(), 10);
    assert.deepEqual(
      await page.locator(".condition-card").evaluateAll(
        // retain the requested dashboard sequence
        (cards) => cards.map(
          // read one card identity
          (card) => card.getAttribute("data-condition"),
        ),
      ),
      ["temperature", "wind", "rain", "clouds", "humidity", "air-quality", "pressure", "uv-index", "tide", "sunset"],
    );
    assert.equal(
      await page.locator(".condition-forecast-reading").evaluateAll(
        // keep every forecast label beside its value
        (readings) => readings.every((reading) => {
          const label = reading.querySelector(".condition-forecast-label");
          const value = reading.querySelector("strong");

          // require complete forecast geometry
          if (label === null || value === null) {
            return false;
          }

          // keep every maximum and minimum beside its label
          if (!["Max", "Min"].includes(label.textContent ?? "")) {
            return true;
          }

          const labelBounds = label.getBoundingClientRect();
          const valueBounds = value.getBoundingClientRect();
          return labelBounds.right <= valueBounds.left &&
            labelBounds.bottom > valueBounds.top &&
            labelBounds.top < valueBounds.bottom;
        }),
      ),
      true,
    );
    const gustForecastAlignment = await currentWind.evaluate(
      // align the gust forecast at the secondary row start
      (card) => {
        const secondary = card.querySelector(".condition-secondary strong");
        const forecastSecondary = card.querySelector(".condition-forecast-reading:last-child strong");

        // require both gust values
        if (secondary === null || forecastSecondary === null) {
          return null;
        }

        return {
          forecastTop: forecastSecondary.getBoundingClientRect().top,
          secondaryTop: secondary.getBoundingClientRect().top,
        };
      },
    );
    assert.equal(
      gustForecastAlignment !== null &&
        Math.abs(gustForecastAlignment.secondaryTop - gustForecastAlignment.forecastTop) <= 4,
      true,
      JSON.stringify(gustForecastAlignment),
    );
    const rainForecastAlignment = await currentRain.evaluate(
      // align accumulation totals at the secondary row start
      (card) => {
        const secondary = card.querySelector(".condition-secondary strong");
        const forecastSecondary = card.querySelector(".condition-forecast-reading:last-child strong");

        // require both accumulation values
        if (secondary === null || forecastSecondary === null) {
          return null;
        }

        return {
          forecastTop: forecastSecondary.getBoundingClientRect().top,
          secondaryTop: secondary.getBoundingClientRect().top,
        };
      },
    );
    assert.equal(
      rainForecastAlignment !== null &&
        Math.abs(rainForecastAlignment.secondaryTop - rainForecastAlignment.forecastTop) <= 4,
      true,
      JSON.stringify(rainForecastAlignment),
    );
    assert.deepEqual(
      await page.locator(".condition-card").evaluateAll(
        // reject clipped content inside every condition card
        (cards) => cards
          .filter((card) => card.scrollWidth > card.clientWidth)
          .map((card) => card.getAttribute("data-condition")),
      ),
      [],
    );
    assert.equal(await page.locator(".metric").count(), 0);

    await page.getByRole("link", { name: "Settings" }).click();
    await page.waitForURL(`${fixture.origin}/settings`);
    const settings = page.getByRole("region", { name: "Measurement units" });
    await settings.waitFor();
    assert.equal(await page.getByRole("dialog").count(), 0);
    assert.equal(await page.locator(".section-nav-settings").getAttribute("aria-current"), "page");
    await settings.locator("select[name='temperature']").selectOption("celsius");
    await settings.locator("select[name='windSpeed']").selectOption("meters_per_second");
    await settings.locator("select[name='precipitation']").selectOption("millimeters");
    await settings.locator("select[name='pressure']").selectOption("hectopascals");
    await settings.locator("select[name='waterLevel']").selectOption("meters");
    await settings.getByRole("button", { name: "Save units" }).click();
    await page.getByRole("link", { name: "Now" }).click();
    await page.waitForURL(`${fixture.origin}/`);
    await page.locator(".current-conditions:not(.skeleton-region)").waitFor();
    await page.locator(".weather-content[aria-busy='false']").waitFor();

    assert.match(await currentTemperature.locator(".condition-primary").textContent() ?? "", /11\s*°C/u);
    assert.match(await currentTemperature.textContent() ?? "", /Air Temp\s*12\s*°C/u);
    assert.match(await currentWind.textContent() ?? "", /Wind\s*Breezy\s*3\s*m\/s SW/u);
    assert.match(await currentWind.textContent() ?? "", /Gusts\s*5\s*m\/s/u);
    assert.match(await currentWind.textContent() ?? "", /Peak reading 5 m\/s/u);
    assert.equal(await page.locator("[data-condition='pressure'] .condition-primary").innerText(), "—");
    assert.match(await currentTide.locator(".condition-status").textContent() ?? "", /High/u);
    assert.match(await currentTide.locator(".condition-primary").textContent() ?? "", /2\.5\s*m/u);
    assert.match(await currentTide.textContent() ?? "", /Rising/u);
    assert.match(await currentTemperature.textContent() ?? "", /Max\s*19°C\s*Min\s*9°C\s*Max\s*20°C\s*Min\s*10°C/u);
    assert.deepEqual(
      await currentTemperature.locator(".condition-forecast-reading").evaluateAll(
        // keep sub-55f extrema blue when the display switches to celsius
        (readings) => readings.map((reading) => getComputedStyle(reading).color),
      ),
      ["rgb(67, 151, 86)", "rgb(56, 120, 197)", "rgb(67, 151, 86)", "rgb(56, 120, 197)"],
    );
    assert.match(await currentWind.textContent() ?? "", /Max\s*4 m\/s\s*Max\s*7 m\/s/u);
    assert.match(await currentRain.textContent() ?? "", /Max 2\.5 mm\/h/u);
    assert.match(await currentRain.textContent() ?? "", /Accumulation\s*2\.5\s*mm/u);
    assert.match(await currentRain.textContent() ?? "", /Total 4\.8 mm/u);
    assert.match(await page.locator("[data-condition='pressure']").textContent() ?? "", /Max\s*-2\.3/u);
    assert.deepEqual(
      await page.evaluate(
        // read the persisted browser preference record
        (key) => JSON.parse(localStorage.getItem(key) ?? "null"),
        UNIT_PREFERENCE_STORAGE_KEY,
      ),
      {
        precipitation: "millimeters",
        pressure: "hectopascals",
        temperature: "celsius",
        waterLevel: "meters",
        windSpeed: "meters_per_second",
      },
    );

    await page.getByRole("link", { name: "Settings" }).click();
    await page.waitForURL(`${fixture.origin}/settings`);
    await page.getByRole("link", { name: "Logs", exact: true }).click();
    await page.waitForURL(`${fixture.origin}/logs`);
    await page.locator("tbody tr:not(.skeleton-history-row)").first().waitFor();
    await page.locator(".weather-content[aria-busy='false']").waitFor();
    assert.equal(await page.getByRole("columnheader", { name: "Temperature (°C)" }).isVisible(), true);
    assert.equal(await page.getByText("16.2").first().isVisible(), true);
    await page.reload({ waitUntil: "networkidle" });
    await page.locator("tbody tr:not(.skeleton-history-row)").first().waitFor();
    await page.locator(".weather-content[aria-busy='false']").waitFor();
    assert.equal(await page.getByRole("columnheader", { name: "Temperature (°C)" }).isVisible(), true);
    await page.getByRole("link", { name: "Settings" }).click();
    await page.waitForURL(`${fixture.origin}/settings`);
    assert.equal(await page.locator("select[name='temperature']").inputValue(), "celsius");
    assert.equal(await page.locator("select[name='windSpeed']").inputValue(), "meters_per_second");
    assert.equal(await page.locator("select[name='precipitation']").inputValue(), "millimeters");
    assert.equal(await page.locator("select[name='pressure']").inputValue(), "hectopascals");
    assert.equal(await page.locator("select[name='waterLevel']").inputValue(), "meters");
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// align the desktop rail to each route without moving the mobile toolbar
test("desktop navigation aligns with the first visible route content", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  // compare one desktop rail against its direct content surface
  const assertDesktopAlignment = async (page, selector, expectedMargin, label) => {
    const layout = await page.locator("main.shell").evaluate(
      // measure only the route's first visible content element
      (shell, { selector }) => {
        const content = shell.querySelector(".weather-content");
        const first = content?.querySelector(`:scope > ${selector}`);
        const navigation = shell.querySelector(".section-nav");

        // require complete desktop route geometry
        if (!(content instanceof HTMLElement) || !(first instanceof HTMLElement) || !(navigation instanceof HTMLElement)) {
          throw new Error(`navigation alignment is incomplete for ${selector}`);
        }

        const contentBounds = content.getBoundingClientRect();
        const firstBounds = first.getBoundingClientRect();
        const navigationBounds = navigation.getBoundingClientRect();
        return {
          contentOffset: firstBounds.top - contentBounds.top,
          firstMargin: getComputedStyle(first).marginTop,
          navigationMargin: getComputedStyle(navigation).marginTop,
          navigationOffset: navigationBounds.top - contentBounds.top,
          position: getComputedStyle(navigation).position,
          topDifference: navigationBounds.top - firstBounds.top,
        };
      },
      { selector },
    );
    const expectedPixels = Number.parseFloat(expectedMargin);
    assert.ok(Math.abs(layout.topDifference) < 1, `${label}: ${JSON.stringify(layout)}`);
    assert.ok(Math.abs(layout.contentOffset - expectedPixels) < 1, `${label}: content moved ${JSON.stringify(layout)}`);
    assert.ok(Math.abs(layout.navigationOffset - expectedPixels) < 1, `${label}: rail offset ${JSON.stringify(layout)}`);
    assert.equal(layout.firstMargin, expectedMargin, label);
    assert.equal(layout.navigationMargin, expectedMargin, label);
    assert.equal(layout.position, "sticky", label);
  };
  // preserve the sticky viewport inset after the document scrolls
  const assertStickyInset = async (page, label) => {
    await page.evaluate(() => window.scrollTo(0, Math.min(400, document.documentElement.scrollHeight - innerHeight)));
    const layout = await page.locator(".section-nav").evaluate(
      // capture the active sticky position
      (navigation) => {
        const masthead = document.querySelector(".home-masthead, .forecast-masthead");
        const mastheadBottom = masthead === null ? 0 : masthead.getBoundingClientRect().bottom;
        return {
          computedTop: Number.parseFloat(getComputedStyle(navigation).top),
          expectedTop: mastheadBottom + 16,
          mastheadBottom,
          position: getComputedStyle(navigation).position,
          top: navigation.getBoundingClientRect().top,
        };
      },
    );
    assert.equal(layout.position, "sticky", `${label}: ${JSON.stringify(layout)}`);
    assert.ok(Math.abs(layout.computedTop - layout.expectedTop) < 0.25, `${label}: ${JSON.stringify(layout)}`);
    assert.ok(Math.abs(layout.top - layout.expectedTop) < 0.25, `${label}: ${JSON.stringify(layout)}`);
    await page.evaluate(() => window.scrollTo(0, 0));
  };
  // retain the fixed bottom navigation below the desktop breakpoint
  const assertMobileNavigation = async (page, label) => {
    assert.deepEqual(
      await page.locator(".section-nav").evaluate(
        // measure the complete bottom toolbar
        (navigation) => {
          const bounds = navigation.getBoundingClientRect();
          return {
            bottomAligned: Math.abs(bounds.bottom - document.documentElement.clientHeight) < 1,
            marginTop: getComputedStyle(navigation).marginTop,
            position: getComputedStyle(navigation).position,
            widthAligned: Math.abs(bounds.width - document.documentElement.clientWidth) < 1,
          };
        },
      ),
      { bottomAligned: true, marginTop: "0px", position: "fixed", widthAligned: true },
      label,
    );
  };

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, { viewport: { height: 900, width: 960 } });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    await assertDesktopAlignment(page, ".current-conditions", "16px", "home loaded");
    await assertStickyInset(page, "home sticky inset");

    // put one alert before the current cards without changing their spacing
    const ordinaryCurrent = fixture.state.currentRecords;
    fixture.state.currentRecords = ordinaryCurrent.map(
      // force current heat and air-quality alerts from every eligible source
      (record) => ({
        ...record,
        metrics: {
          ...record.metrics,
          apparentTemperatureC: 40,
          pm25MicrogramsPerCubicMeter: 100,
          temperatureC: 40,
          wetBulbGlobeTemperatureC: 35,
        },
      }),
    );
    await page.reload({ waitUntil: "networkidle" });
    await page.locator(".alert-list").waitFor();
    await assertDesktopAlignment(page, ".alert-list", "16px", "home alerts");
    fixture.state.currentRecords = ordinaryCurrent;

    // exercise each loaded public route through browser navigation
    await page.getByRole("link", { name: "Forecast", exact: true }).click();
    await page.locator(".forecast-panel:not(.skeleton-region)").waitFor();
    await assertDesktopAlignment(page, ".forecast-panel", "19.2px", "forecast loaded");
    await assertStickyInset(page, "forecast sticky inset");
    await page.getByRole("link", { name: "Trends", exact: true }).click();
    await page.locator(".trends-panel:not(.skeleton-region)").waitFor();
    await assertDesktopAlignment(page, ".trends-panel", "19.2px", "trends loaded");
    await page.getByRole("link", { name: "Map", exact: true }).click();
    await page.locator(".property-map-panel:not(.skeleton-region)").waitFor();
    await assertDesktopAlignment(page, ".property-map-panel", "19.2px", "map loaded");
    await page.getByRole("link", { name: "Settings", exact: true }).click();
    await page.locator(".settings-page").waitFor();
    await assertDesktopAlignment(page, ".settings-page", "0px", "settings loaded");
    await page.getByRole("link", { name: "Logs", exact: true }).click();
    await page.locator(".weather-content[aria-busy='false']").waitFor();
    await assertDesktopAlignment(page, ".panel", "19.2px", "logs loaded");

    // retain alignment at every representative desktop width and the exact breakpoint
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    for (const width of [1_440, 768, 673]) {
      await page.setViewportSize({ height: 900, width });
      await assertDesktopAlignment(page, ".current-conditions", "16px", `home ${width}px`);
    }
    for (const width of [672, 390, 320]) {
      await page.setViewportSize({ height: 844, width });
      await assertMobileNavigation(page, `mobile ${width}px`);
    }
    await page.close();

    // hold the initial homepage reads behind its aligned skeleton
    const loadingHome = await createFixturePage(browser, { viewport: { height: 900, width: 960 } });
    let releaseHomeReads;
    const homeReadsReleased = new Promise(
      // expose one deterministic homepage loading gate
      (resolveRelease) => {
        releaseHomeReads = resolveRelease;
      },
    );
    await loadingHome.route(
      /\/api\/v1\/sites\/ballydidean\/(?:current|daily-precipitation|forecast|tides)/u,
      // hold all first-load weather reads
      async (route) => {
        await homeReadsReleased;
        await route.continue();
      },
    );
    await loadingHome.goto(fixture.origin, { waitUntil: "domcontentloaded" });
    await assertSkeletonLoadingState(loadingHome, ".current-conditions.skeleton-region");
    await assertDesktopAlignment(loadingHome, ".current-conditions", "16px", "home loading");
    releaseHomeReads();
    await assertSkeletonLoadingSettled(loadingHome);
    await loadingHome.close();

    // hold the forecast behind its route-matched panel skeleton
    const loadingForecast = await createFixturePage(browser, { viewport: { height: 900, width: 960 } });
    let releaseForecastReads;
    const forecastReadsReleased = new Promise(
      // expose one deterministic forecast loading gate
      (resolveRelease) => {
        releaseForecastReads = resolveRelease;
      },
    );
    await loadingForecast.route(
      /\/api\/v1\/sites\/ballydidean\/forecast(?:\?|$)/u,
      // hold the initial forecast response
      async (route) => {
        await forecastReadsReleased;
        await route.continue();
      },
    );
    await loadingForecast.goto(`${fixture.origin}/forecast`, { waitUntil: "domcontentloaded" });
    await assertSkeletonLoadingState(loadingForecast, ".forecast-panel.skeleton-region");
    await assertDesktopAlignment(loadingForecast, ".forecast-panel", "19.2px", "forecast loading");
    releaseForecastReads();
    await assertSkeletonLoadingSettled(loadingForecast);
    await loadingForecast.close();

    // align one actionable error before all fallback content
    fixture.state.failReads = true;
    const errorPage = await createFixturePage(browser, { viewport: { height: 900, width: 960 } });
    await errorPage.goto(fixture.origin, { waitUntil: "networkidle" });
    await errorPage.locator(".notice.error").waitFor();
    await assertDesktopAlignment(errorPage, ".notice.error", "16px", "home error");
    await errorPage.close();
    fixture.state.failReads = false;
  } finally {
    // restore fixture state before disposing browser resources
    fixture.state.failReads = false;
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// pin only the primary weather mastheads above their scrolling content
test("home and forecast mastheads stay usable while pinned", { timeout: 120_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  // settle sticky layout after scrolling or resizing
  const settleLayout = async (page) => await page.evaluate(
    // wait for two rendered frames
    async () => await new Promise((resolveFrame) => requestAnimationFrame(
      () => requestAnimationFrame(resolveFrame),
    )),
  );
  // move enough content behind the masthead to exercise sticky stacking
  const pinMasthead = async (page) => {
    const scroll = await page.evaluate(() => {
      const maximum = document.documentElement.scrollHeight - innerHeight;
      const target = Math.min(maximum, Math.max(360, innerHeight * 0.75));
      window.scrollTo(0, target);
      return { maximum, target };
    });
    assert.ok(scroll.maximum >= 32, `page cannot exercise sticky layout: ${JSON.stringify(scroll)}`);
    assert.ok(scroll.target >= 32, `page did not scroll far enough: ${JSON.stringify(scroll)}`);
    await settleLayout(page);
  };
  // measure a pinned route header and the chrome below it
  const assertPinnedMasthead = async (page, route, width) => {
    const selector = route === "forecast" ? ".forecast-masthead" : ".home-masthead";
    const layout = await page.locator(selector).evaluate(
      // inspect final browser geometry and stacking
      (masthead, { route, width }) => {
        const bounds = masthead.getBoundingClientRect();
        const style = getComputedStyle(masthead);
        const heading = masthead.querySelector("h1")?.getBoundingClientRect();
        const toggle = masthead.querySelector("[data-forecast-adjustment-toggle]")?.getBoundingClientRect();
        const range = masthead.querySelector(".forecast-range-selector")?.getBoundingClientRect();
        const navigation = document.querySelector(".section-nav")?.getBoundingClientRect();
        const weatherContent = document.querySelector(".weather-content");
        const probeX = bounds.left + bounds.width / 2;
        const probeY = Math.max(bounds.top, bounds.bottom - 2);
        const hitStack = document.elementsFromPoint(probeX, probeY);
        const background = style.backgroundColor;
        const backgroundAlpha = background === "transparent"
          ? 0
          : Number(background.match(/rgba\([^,]+,[^,]+,[^,]+,\s*([\d.]+)\)/u)?.[1] ?? 1);
        return {
          background,
          controlsContained: heading !== undefined && toggle !== undefined &&
            heading.top >= bounds.top - 1 && heading.bottom <= bounds.bottom + 1 &&
            toggle.top >= bounds.top - 1 && toggle.bottom <= bounds.bottom + 1,
          headerHitFirst: masthead.contains(hitStack[0] ?? null),
          headingVisible: heading !== undefined && heading.bottom > 0 && heading.top < innerHeight,
          layeredOverContent: weatherContent !== null && hitStack.some(
            // find the obscured route content in the hit-test stack
            (element) => element === weatherContent || weatherContent.contains(element),
          ),
          navigationBelow: navigation !== undefined && navigation.top >= bounds.bottom - 1,
          opaque: backgroundAlpha === 1,
          overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
          pinned: Math.abs(bounds.top - Number.parseFloat(style.top)) < 1,
          position: style.position,
          rangeContained: route !== "forecast" || range !== undefined &&
            range.top >= bounds.top - 1 && range.bottom <= bounds.bottom + 1,
          routeClass: masthead.classList.contains(`${route}-masthead`),
          top: bounds.top,
          topOffset: style.top,
          toggleVisible: toggle !== undefined && toggle.bottom > 0 && toggle.top < innerHeight,
          width,
          zIndex: Number.parseFloat(style.zIndex),
        };
      },
      { route, width },
    );
    assert.equal(layout.position, "sticky", `${route} ${String(width)}px: ${JSON.stringify(layout)}`);
    assert.equal(layout.pinned, true, `${route} ${String(width)}px: ${JSON.stringify(layout)}`);
    assert.equal(layout.routeClass, true, `${route} ${String(width)}px: ${JSON.stringify(layout)}`);
    assert.equal(layout.headingVisible, true, `${route} ${String(width)}px: ${JSON.stringify(layout)}`);
    assert.equal(layout.toggleVisible, true, `${route} ${String(width)}px: ${JSON.stringify(layout)}`);
    assert.equal(layout.controlsContained, true, `${route} ${String(width)}px: ${JSON.stringify(layout)}`);
    assert.equal(layout.rangeContained, true, `${route} ${String(width)}px: ${JSON.stringify(layout)}`);
    assert.equal(layout.opaque, true, `${route} ${String(width)}px: ${JSON.stringify(layout)}`);
    assert.equal(layout.headerHitFirst, true, `${route} ${String(width)}px: ${JSON.stringify(layout)}`);
    assert.equal(layout.layeredOverContent, true, `${route} ${String(width)}px: ${JSON.stringify(layout)}`);
    assert.equal(layout.zIndex >= 1, true, `${route} ${String(width)}px: ${JSON.stringify(layout)}`);
    assert.equal(layout.overflow, 0, `${route} ${String(width)}px: ${JSON.stringify(layout)}`);

    // keep only the desktop rail below the pinned header
    if (width > 672) {
      assert.equal(layout.navigationBelow, true, `${route} ${String(width)}px: ${JSON.stringify(layout)}`);
    }
  };
  // keep forecast clocks below the pinned masthead
  const assertForecastClocks = async (page, width) => {
    const clocks = await page.locator(".forecast-chart-shell").evaluate(
      // compare both sticky clocks against the masthead edge
      (shell) => {
        const masthead = document.querySelector(".forecast-masthead")?.getBoundingClientRect();
        const selected = shell.querySelector(".forecast-crosshair-label time")?.getBoundingClientRect();
        const current = shell.querySelector(".forecast-current-time-label span")?.getBoundingClientRect();
        return {
          currentBelow: masthead !== undefined && current !== undefined && current.top >= masthead.bottom - 1,
          currentVisible: current !== undefined && current.top >= 0 && current.bottom <= innerHeight,
          selectedBelow: masthead !== undefined && selected !== undefined && selected.top >= masthead.bottom - 1,
          selectedVisible: selected !== undefined && selected.top >= 0 && selected.bottom <= innerHeight,
        };
      },
    );
    assert.deepEqual(
      clocks,
      { currentBelow: true, currentVisible: true, selectedBelow: true, selectedVisible: true },
      `forecast clocks ${String(width)}px`,
    );
  };

  try {
    browser = await launchBrowser();
    fixture.state.adjustmentMode = "active";

    // cover one wide desktop and both narrow supported phones
    for (const viewport of [
      { height: 700, width: 1_440 },
      { height: 844, width: 390 },
      { height: 780, width: 320 },
    ]) {
      const page = await createFixturePage(browser, {
        hasTouch: viewport.width <= 672,
        viewport,
      });
      const pageErrors = [];
      // retain all unexpected page failures
      page.on("pageerror", (error) => pageErrors.push(error.message));
      await page.clock.setFixedTime(new Date("2026-08-22T05:37:00.000Z"));
      await page.goto(fixture.origin, { waitUntil: "networkidle" });
      await pinMasthead(page);
      await assertPinnedMasthead(page, "home", viewport.width);
      const homeToggle = page.getByRole("switch", { name: "Adjusted", exact: true });
      await homeToggle.click();
      assert.equal(await homeToggle.getAttribute("aria-checked"), "false");
      await homeToggle.click();
      assert.equal(await homeToggle.getAttribute("aria-checked"), "true");

      // keep the forecast controls actionable while their header is pinned
      await page.getByRole("link", { name: "Forecast", exact: true }).click();
      await page.locator('[data-forecast-charts][data-forecast-days="1"]').waitFor();
      await page.getByRole("button", { name: "5 days", exact: true }).click();
      await page.locator('[data-forecast-charts][data-forecast-days="5"]').waitFor();
      await page.evaluate(() => {
        const grid = document.querySelector(".forecast-chart-grid");

        // move the clocks into their sticky range
        if (grid !== null) {
          window.scrollTo(0, grid.getBoundingClientRect().top + window.scrollY + 360);
        }
      });
      await settleLayout(page);
      await assertPinnedMasthead(page, "forecast", viewport.width);
      await assertForecastClocks(page, viewport.width);
      const forecastToggle = page.getByRole("switch", { name: "Adjusted", exact: true });
      await forecastToggle.click();
      assert.equal(await forecastToggle.getAttribute("aria-checked"), "false");
      await page.getByRole("button", { name: "Today", exact: true }).click();
      await page.locator('[data-forecast-charts][data-forecast-days="1"]').waitFor();
      await page.getByRole("button", { name: "5 days", exact: true }).click();
      await page.locator('[data-forecast-charts][data-forecast-days="5"]').waitFor();
      await forecastToggle.click();
      assert.equal(await forecastToggle.getAttribute("aria-checked"), "true");

      // leave non-weather routes on the ordinary document flow
      await page.getByRole("link", { name: "Trends", exact: true }).click();
      await page.locator(".trends-panel:not(.skeleton-region)").waitFor();
      await page.evaluate(() => window.scrollTo(0, 0));
      await settleLayout(page);
      const ordinaryBefore = await page.locator(".masthead").evaluate(
        // capture the ordinary masthead before document scrolling
        (masthead) => masthead.getBoundingClientRect().top,
      );
      const ordinaryScroll = await page.evaluate(() => {
        const target = Math.min(360, document.documentElement.scrollHeight - innerHeight);
        window.scrollTo(0, target);
        return target;
      });
      assert.ok(ordinaryScroll > 0, `trends route cannot scroll: ${JSON.stringify(viewport)}`);
      await settleLayout(page);
      const ordinaryHeader = await page.locator(".masthead").evaluate(
        // require the ordinary masthead to follow document scrolling
        (masthead) => ({
          position: getComputedStyle(masthead).position,
          top: masthead.getBoundingClientRect().top,
        }),
      );
      assert.notEqual(ordinaryHeader.position, "sticky", JSON.stringify({ viewport, ordinaryHeader }));
      assert.notEqual(ordinaryHeader.position, "fixed", JSON.stringify({ viewport, ordinaryHeader }));
      assert.equal(
        Math.abs(ordinaryBefore - ordinaryHeader.top - ordinaryScroll) < 1,
        true,
        JSON.stringify({ viewport, ordinaryBefore, ordinaryHeader, ordinaryScroll }),
      );

      // restore the home route and its pinned state after navigation
      await page.getByRole("link", { name: "Now", exact: true }).click();
      await page.locator(".home-masthead").waitFor();
      await page.evaluate(() => window.scrollTo(0, 0));
      await settleLayout(page);
      assert.equal(
        await page.locator(".home-masthead").evaluate((masthead) => masthead.getBoundingClientRect().top >= 0),
        true,
      );
      await pinMasthead(page);
      await assertPinnedMasthead(page, "home", viewport.width);

      // recompute sticky geometry across a live breakpoint resize
      if (viewport.width === 1_440) {
        await page.setViewportSize({ height: 844, width: 390 });
        await settleLayout(page);
        await assertPinnedMasthead(page, "home", 390);
        await page.setViewportSize(viewport);
        await settleLayout(page);
        await assertPinnedMasthead(page, "home", viewport.width);
      }
      assert.deepEqual(pageErrors, []);
      await page.close();
    }
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

// preserve tablet geometry with shared primary measurement sizes
test("real browser keeps the tablet masthead and compact navigation in separate rows", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, { viewport: { height: 900, width: 960 } });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    assert.equal(
      await page.locator(".condition-card:not(.pressure-condition) .condition-primary strong").evaluateAll(
        // retain shared tablet scales including the sunset clock
        (readings) => new Set(readings.map(
          // read one tablet primary scale
          (reading) => getComputedStyle(reading).fontSize,
        )).size,
      ),
      1,
    );
    assert.equal(
      await page.locator("[data-condition='temperature'], [data-condition='wind'], [data-condition='rain'], [data-condition='tide']").evaluateAll(
        // retain one static secondary height on tablets
        (cards) => new Set(cards.map(
          // measure one tablet secondary section
          (card) => {
            const secondary = card.querySelector(".condition-secondary");
            return secondary === null ? null : secondary.getBoundingClientRect().height;
          },
        )).size,
      ),
      1,
    );
    assert.equal(
      await page.locator(".current-conditions").evaluate(
        // keep temperature and wind paired on tablets
        (grid) => {
          const temperature = grid.querySelector("[data-condition='temperature']")?.getBoundingClientRect();
          const wind = grid.querySelector("[data-condition='wind']")?.getBoundingClientRect();
          return temperature !== undefined && wind !== undefined &&
            Math.abs(temperature.top - wind.top) < 1 &&
            Math.abs(temperature.width - wind.width) < 1 &&
            wind.left >= temperature.right;
        },
      ),
      true,
    );
    await page.goto(`${fixture.origin}/logs`, { waitUntil: "networkidle" });
    const headerLayout = await page.locator("main.shell").evaluate(
      // measure the exact tablet header geometry
      (shell) => {
        const masthead = shell.querySelector(".masthead");
        const heading = masthead?.querySelector("h1");
        const navigation = shell.querySelector(".section-nav");
        const forecast = navigation?.querySelector('a[href="/forecast"]');
        const home = navigation?.querySelector(".section-nav-home");
        const map = navigation?.querySelector(".section-nav-map");
        const trends = navigation?.querySelector('a[href="/trends"]');
        const settings = navigation?.querySelector(".section-nav-settings");
        const icon = settings?.querySelector(".material-symbols-rounded");
        const headingRange = document.createRange();

        // require the complete header surface
        if (
          masthead === null ||
          heading === null ||
          navigation === null ||
          forecast === null ||
          home === null ||
          map === null ||
          trends === null ||
          settings === null ||
          icon === null
        ) {
          throw new Error("header controls are incomplete");
        }

        headingRange.selectNodeContents(heading);
        const shellBounds = shell.getBoundingClientRect();
        const mastheadBounds = masthead.getBoundingClientRect();
        const navigationBounds = navigation.getBoundingClientRect();
        const forecastIcon = forecast.querySelector(".material-symbols-rounded");
        const forecastIconBounds = forecastIcon?.getBoundingClientRect();
        const homeIcon = home.querySelector("img.section-nav-weather-icon");
        const homeIconBounds = homeIcon?.getBoundingClientRect();
        const mapIcon = map.querySelector(".material-symbols-rounded");
        const trendsIcon = trends.querySelector(".material-symbols-rounded");
        const trendsIconBounds = trendsIcon?.getBoundingClientRect();
        const iconBounds = icon.getBoundingClientRect();
        const activeIndicator = settings.querySelector(".section-nav-icon");
        const activeIndicatorBounds = activeIndicator?.getBoundingClientRect();
        const controls = [home, forecast, trends, map, settings];
        const controlBounds = controls.map(
          // measure every rail destination
          (control) => control.getBoundingClientRect(),
        );
        return {
          activeIndicatorBackground: activeIndicator === null
            ? null
            : getComputedStyle(activeIndicator).backgroundColor,
          activeIndicatorHeight: activeIndicatorBounds?.height,
          activeIndicatorWidth: activeIndicatorBounds?.width,
          bodyWidth: document.body.scrollWidth,
          controlLabels: [
            home.lastElementChild?.textContent,
            forecast.lastElementChild?.textContent,
            trends.lastElementChild?.textContent,
            map.lastElementChild?.textContent,
            settings.lastElementChild?.textContent,
          ],
          decorationContent: getComputedStyle(masthead, "::after").content,
          desktopNavigationLeftAligned: Math.abs(navigationBounds.left - shellBounds.left) < 1,
          forecastIconIsGlyph: forecastIconBounds !== undefined && forecastIconBounds.width <= 24,
          forecastIconText: forecastIcon?.textContent,
          headingLineCount: headingRange.getClientRects().length,
          headingText: heading.textContent,
          homeAriaLabel: home.getAttribute("aria-label"),
          homeDashboardCount: home.querySelectorAll('svg[data-nav-icon="dashboard"]').length,
          homeIconAlt: homeIcon?.getAttribute("alt"),
          homeIconHeight: homeIconBounds?.height,
          homeIconIsImage: homeIconBounds !== undefined && homeIcon.tagName === "IMG",
          homeIconSource: homeIcon instanceof HTMLImageElement ? new URL(homeIcon.src).pathname : null,
          homeIconWidth: homeIconBounds?.width,
          iconFont: getComputedStyle(icon).fontFamily,
          iconHeight: iconBounds.height,
          iconText: icon.textContent,
          iconWidth: iconBounds.width,
          imageCount: masthead.querySelectorAll("img").length,
          linkCount: masthead.querySelectorAll("a").length,
          mapIconText: mapIcon?.textContent,
          navigationBelowMasthead: navigationBounds.top >= mastheadBounds.bottom,
          navigationPosition: getComputedStyle(navigation).position,
          navigationWidth: navigationBounds.width,
          railIsColumn: controlBounds.every(
            // keep every destination below its predecessor
            (bounds, index) => index === 0 || bounds.top >= controlBounds[index - 1].bottom,
          ),
          trendsIconText: trendsIcon?.textContent,
          trendsIconIsGlyph: trendsIconBounds !== undefined && trendsIconBounds.width <= 24,
          viewportWidth: document.documentElement.clientWidth,
        };
      },
    );

    assert.equal(headerLayout.activeIndicatorBackground, "rgb(240, 230, 150)");
    assert.equal(headerLayout.activeIndicatorHeight, 32);
    assert.equal(headerLayout.activeIndicatorWidth, 56);
    assert.deepEqual(headerLayout.controlLabels, ["Now", "Forecast", "Trends", "Map", "Settings"]);
    assert.equal(headerLayout.decorationContent, "none");
    assert.equal(headerLayout.desktopNavigationLeftAligned, true);
    assert.equal(headerLayout.headingLineCount, 1);
    assert.equal(headerLayout.headingText, "Ballydídean Weather");
    assert.equal(headerLayout.forecastIconIsGlyph, true);
    assert.equal(headerLayout.forecastIconText, "partly_cloudy_day");
    assert.equal(headerLayout.homeAriaLabel, "Now");
    assert.equal(headerLayout.homeDashboardCount, 0);
    assert.equal(headerLayout.homeIconAlt, "Current weather: Conditions unavailable");
    assert.equal(headerLayout.homeIconHeight, 32);
    assert.equal(headerLayout.homeIconIsImage, true);
    assert.equal(headerLayout.homeIconSource, "/weather-icons/12-unavailable.svg");
    assert.equal(headerLayout.homeIconWidth, 32);
    assert.equal(headerLayout.imageCount, 0);
    assert.match(headerLayout.iconFont, /Material Symbols Rounded/u);
    assert.equal(headerLayout.iconText, "settings");
    assert.equal(headerLayout.iconWidth <= 24 && headerLayout.iconHeight <= 24, true);
    assert.equal(headerLayout.linkCount, 0);
    assert.equal(headerLayout.mapIconText, "map");
    assert.equal(headerLayout.navigationBelowMasthead, true);
    assert.equal(headerLayout.navigationPosition, "sticky");
    assert.equal(headerLayout.navigationWidth, 92);
    assert.equal(headerLayout.railIsColumn, true);
    assert.equal(headerLayout.trendsIconIsGlyph, true);
    assert.equal(headerLayout.trendsIconText, "trending_up");
    assert.equal(headerLayout.bodyWidth <= headerLayout.viewportWidth, true);
    assert.equal(
      await page.locator(".condition-card").evaluateAll(
        // keep every tablet pill in the upper-right corner
        (cards) => cards.every(
          // verify one card
          (card) => {
            const heading = card.querySelector(".condition-card-heading")?.getBoundingClientRect();
            const status = card.querySelector(".condition-status")?.getBoundingClientRect();
            return heading !== undefined && status !== undefined &&
              Math.abs(status.top - heading.top) < 1 &&
              status.right > heading.left + heading.width / 2;
          },
        ),
      ),
      true,
    );
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});

test("real browser keeps the dashboard within a mobile viewport", { timeout: 60_000 }, async () => {
  const fixture = await startFixtureServer();
  let browser;

  try {
    browser = await launchBrowser();
    const page = await createFixturePage(browser, { viewport: { height: 844, width: 390 } });
    await page.goto(fixture.origin, { waitUntil: "networkidle" });
    assert.equal(
      await page.locator(".condition-card:not(.pressure-condition) .condition-primary strong").evaluateAll(
        // retain shared phone scales including the sunset clock
        (readings) => new Set(readings.map(
          // read one mobile primary scale
          (reading) => getComputedStyle(reading).fontSize,
        )).size,
      ),
      1,
    );
    assert.equal(
      await page.locator("[data-condition='temperature'], [data-condition='wind'], [data-condition='rain'], [data-condition='tide']").evaluateAll(
        // retain one static secondary height on phones
        (cards) => new Set(cards.map(
          // measure one mobile secondary section
          (card) => {
            const secondary = card.querySelector(".condition-secondary");
            return secondary === null ? null : secondary.getBoundingClientRect().height;
          },
        )).size,
      ),
      1,
    );
    assert.equal(
      await page.locator(".current-conditions").evaluate(
        // keep temperature and wind paired on mobile
        (grid) => {
          const temperature = grid.querySelector("[data-condition='temperature']")?.getBoundingClientRect();
          const wind = grid.querySelector("[data-condition='wind']")?.getBoundingClientRect();
          return temperature !== undefined && wind !== undefined &&
            Math.abs(temperature.top - wind.top) < 1 &&
            Math.abs(temperature.width - wind.width) < 1 &&
            wind.left >= temperature.right;
        },
      ),
      true,
    );
    const mobileDividerTops = await page.locator("[data-condition='temperature'], [data-condition='wind']").evaluateAll(
      // measure paired mobile dividers
      (cards) => cards.map(
        // capture one divider position
        (card) => card.querySelector(".condition-secondary-divider")?.getBoundingClientRect().top ?? null,
      ),
    );
    assert.equal(mobileDividerTops.includes(null), false);
    assert.ok(
      Math.abs((mobileDividerTops[0] ?? 0) - (mobileDividerTops[1] ?? 0)) <= 1,
      JSON.stringify(mobileDividerTops),
    );
    assert.deepEqual(
      await page.locator(".condition-card").evaluateAll(
        // keep compact labels clear of status pills
        (cards) => cards.filter(
          // verify one mobile card
          (card) => {
            const label = card.querySelector(".condition-label");
            const status = card.querySelector(".condition-status");

            // require both heading controls
            if (label === null || status === null) {
              return true;
            }

            const labelBounds = label.getBoundingClientRect();
            const statusBounds = status.getBoundingClientRect();
            return label.scrollWidth > label.clientWidth || labelBounds.right > statusBounds.left;
          },
        ).map((card) => card.getAttribute("data-condition")),
      ),
      [],
    );
    const mobileTideDirection = page.locator("[data-condition='tide'] .condition-secondary");
    assert.equal(await mobileTideDirection.isVisible(), true);
    assert.match(await mobileTideDirection.textContent() ?? "", /Direction\s*Rising/u);
    assert.equal(await page.locator("[data-condition='tide'] .condition-detail").count(), 0);
    await page.setViewportSize({ height: 844, width: 320 });
    assert.deepEqual(
      await page.locator(".section-nav").evaluate(
        // keep the complete toolbar on one narrow row
        (navigation) => {
          const controls = [...navigation.children].map((control) => control.getBoundingClientRect());
          const navigationBounds = navigation.getBoundingClientRect();
          return {
            bottomAligned: Math.abs(navigationBounds.bottom - document.documentElement.clientHeight) < 1,
            fullWidth: Math.abs(navigationBounds.width - document.documentElement.clientWidth) < 1,
            oneRow: controls.every((control) => Math.abs(control.top - controls[0].top) < 1),
            position: getComputedStyle(navigation).position,
            withinViewport: navigationBounds.left >= 0 && navigationBounds.right <= document.documentElement.clientWidth,
          };
        },
      ),
      { bottomAligned: true, fullWidth: true, oneRow: true, position: "fixed", withinViewport: true },
    );
    assert.equal(
      await page.locator("body").evaluate(
        // reject narrow toolbar overflow
        (body) => body.scrollWidth > document.documentElement.clientWidth,
      ),
      false,
    );
    await page.setViewportSize({ height: 844, width: 390 });
    await page.goto(`${fixture.origin}/trends`, { waitUntil: "networkidle" });
    assert.deepEqual(
      await page.locator(".trends-panel").evaluate(
        // rotate the chart and its selector together on phones
        (panel) => {
          const chart = panel.querySelector(".trend-chart");
          const crosshair = panel.querySelector(".trend-crosshair-line");
          const crosshairDate = panel.querySelector(".trend-crosshair-date-pill");
          const crosshairSummary = panel.querySelector(".trend-crosshair-summary");
          const crosshairValueFlags = panel.querySelector(".trend-crosshair-values");
          const currentYearLine = panel.querySelector(".trend-year-line-current");
          const detailToggle = panel.querySelector("[data-trend-detail-toggle]");
          const landscape = panel.querySelector(".trend-chart-landscape");
          const legend = panel.querySelector(".trend-chart-legend");
          const masthead = document.querySelector(".masthead");
          const navigation = document.querySelector(".section-nav");
          const title = panel.querySelector(".trend-chart-title");
          const titleControl = title?.closest(".trend-metric-control");
          const svg = panel.querySelector(".trend-chart-landscape > svg");
          const viewport = panel.querySelector(".trend-chart-viewport");

          // require the complete mobile trend surface
          if (
            chart === null ||
            crosshair === null ||
            crosshairDate === null ||
            crosshairSummary === null ||
            crosshairValueFlags === null ||
            currentYearLine === null ||
            detailToggle === null ||
            landscape === null ||
            legend === null ||
            masthead === null ||
            navigation === null ||
            title === null ||
            titleControl === null ||
            svg === null ||
            viewport === null
          ) {
            throw new Error("mobile trend chart is incomplete");
          }

          const chartBounds = chart.getBoundingClientRect();
          const detailToggleStyle = getComputedStyle(detailToggle);
          const landscapeBounds = landscape.getBoundingClientRect();
          const crosshairBounds = crosshair.getBoundingClientRect();
          const crosshairDateBounds = crosshairDate.getBoundingClientRect();
          const crosshairSummaryBounds = crosshairSummary.getBoundingClientRect();
          const navigationBounds = navigation.getBoundingClientRect();
          const viewportBounds = viewport.getBoundingClientRect();
          const landscapeTransform = getComputedStyle(landscape).transform;
          const legendStyle = getComputedStyle(legend);
          const titleControlStyle = getComputedStyle(titleControl);
          const chartStyle = getComputedStyle(chart);
          const currentYearPoint = currentYearLine.points.getItem(currentYearLine.points.numberOfItems - 1);
          const currentYearY = (currentYearPoint.y / 280) * landscape.clientHeight;
          const crosshairValueFlagsStyle = getComputedStyle(crosshairValueFlags);
          const summaryOnLeft = crosshairSummary.classList.contains("trend-crosshair-summary-left");
          return {
            cardRemoved: chartStyle.borderTopStyle === "none" &&
              chartStyle.backgroundColor === "rgba(0, 0, 0, 0)" &&
              chartStyle.boxShadow === "none" &&
              chartStyle.paddingTop === "0px",
            chartFitsAboveNavigation: chartBounds.bottom <= navigationBounds.top + 1,
            crosshairInsideLandscape: crosshairBounds.left >= landscapeBounds.left - 1 &&
              crosshairBounds.right <= landscapeBounds.right + 1 &&
              crosshairBounds.top >= landscapeBounds.top - 1 &&
              crosshairBounds.bottom <= landscapeBounds.bottom + 1,
            crosshairDateCentered: Math.abs(
              Number.parseFloat(getComputedStyle(crosshairDate).left) -
              Number.parseFloat(getComputedStyle(crosshair).left),
            ) < 1,
            crosshairDateInsideLandscape: crosshairDateBounds.left >= landscapeBounds.left - 1 &&
              crosshairDateBounds.right <= landscapeBounds.right + 1 &&
              crosshairDateBounds.top >= landscapeBounds.top - 1 &&
              crosshairDateBounds.bottom <= landscapeBounds.bottom + 1,
            crosshairDateInsideRotatedChart: crosshairDate.closest(".trend-chart-landscape") === landscape,
            crosshairSummaryInsideLandscape: crosshairSummaryBounds.left >= landscapeBounds.left - 1 &&
              crosshairSummaryBounds.right <= landscapeBounds.right + 1 &&
              crosshairSummaryBounds.top >= landscapeBounds.top - 1 &&
              crosshairSummaryBounds.bottom <= landscapeBounds.bottom + 1,
            crosshairSummaryInsidePlot: crosshairSummary.offsetTop >= landscape.clientHeight * (42 / 280) - 1 &&
              crosshairSummary.offsetTop + crosshairSummary.offsetHeight <=
                landscape.clientHeight * ((280 - 34) / 280) + 1,
            crosshairSummaryMatchesCurrentYear: Math.abs(
              crosshairSummary.offsetTop + crosshairSummary.offsetHeight / 2 - currentYearY,
            ) < 2,
            crosshairSummarySquareAgainstLine: summaryOnLeft
              ? crosshairValueFlagsStyle.borderTopRightRadius === "0px" &&
                crosshairValueFlagsStyle.borderBottomRightRadius === "0px"
              : crosshairValueFlagsStyle.borderTopLeftRadius === "0px" &&
                crosshairValueFlagsStyle.borderBottomLeftRadius === "0px",
            detailIcon: detailToggle.querySelector("[data-trend-toggle-icon]")?.getAttribute("data-trend-toggle-icon"),
            detailTopLeftOfTitle: Number.parseFloat(detailToggleStyle.left) <
              Number.parseFloat(titleControlStyle.left) &&
              Math.abs(Number.parseFloat(detailToggleStyle.top) - Number.parseFloat(titleControlStyle.top)) < 2,
            landscapeFitsViewport: landscapeBounds.left >= viewportBounds.left - 1 &&
              landscapeBounds.right <= viewportBounds.right + 1 &&
              landscapeBounds.top >= viewportBounds.top - 1 &&
              landscapeBounds.bottom <= viewportBounds.bottom + 1,
            landscapeRotated: landscapeTransform.startsWith("matrix(0, 1, -1, 0"),
            legendAtChosenEdge: legend.dataset.trendLegendPlacement === "top"
              ? Number.parseFloat(legendStyle.top) > landscape.clientHeight * (42 / 280)
              : Number.parseFloat(legendStyle.bottom) > landscape.clientHeight * (34 / 280),
            legendInsideRotatedChart: legend.closest(".trend-chart-landscape") === landscape,
            mastheadTransform: getComputedStyle(masthead).transform,
            navigationTransform: getComputedStyle(navigation).transform,
            titleInsideRotatedChart: title.closest(".trend-chart-landscape") === landscape,
            titleTag: title.tagName,
            titleTopCenteredInGraph: Math.abs(
              Number.parseFloat(titleControlStyle.left) - landscape.clientWidth / 2,
            ) < 1 && Number.parseFloat(titleControlStyle.top) < landscape.clientHeight / 2,
            svgFitsBothAxes: svg.clientWidth === landscape.clientWidth &&
              svg.clientHeight === landscape.clientHeight,
          };
        },
      ),
      {
        cardRemoved: true,
        chartFitsAboveNavigation: true,
        crosshairDateCentered: true,
        crosshairDateInsideLandscape: true,
        crosshairDateInsideRotatedChart: true,
        crosshairInsideLandscape: true,
        crosshairSummaryInsideLandscape: true,
        crosshairSummaryInsidePlot: true,
        crosshairSummaryMatchesCurrentYear: true,
        crosshairSummarySquareAgainstLine: true,
        detailIcon: "daily",
        detailTopLeftOfTitle: true,
        landscapeFitsViewport: true,
        landscapeRotated: true,
        legendAtChosenEdge: true,
        legendInsideRotatedChart: true,
        mastheadTransform: "none",
        navigationTransform: "none",
        titleInsideRotatedChart: true,
        titleTag: "H2",
        titleTopCenteredInGraph: true,
        svgFitsBothAxes: true,
      },
    );
    assert.equal(await page.getByRole("heading", { name: "Yearly trends" }).count(), 0);
    assert.equal(await page.getByRole("heading", { name: "Temperature" }).count(), 1);
    const mobileTrendMetricTrigger = page.locator("[data-trend-metric-trigger]");
    await mobileTrendMetricTrigger.click();
    assert.equal(await page.locator(".trend-metric-flyover").isVisible(), true);
    assert.equal(
      await page.locator(".trend-metric-flyover").evaluate(
        // keep the flyover inside the rotated chart coordinate system
        (flyover) => flyover.closest(".trend-chart-landscape") !== null,
      ),
      true,
    );
    await page.keyboard.press("Escape");
    const mobileTrendDateBefore = await page.locator("[data-trend-crosshair-date]").textContent();
    const mobileTrendBounds = await page.locator("[data-trend-scrub-surface]").boundingBox();

    // require one rotated annual scrub surface
    if (mobileTrendBounds === null) {
      throw new Error("mobile trend scrub surface is unavailable");
    }

    await page.mouse.move(
      mobileTrendBounds.x + mobileTrendBounds.width * 0.5,
      mobileTrendBounds.y + mobileTrendBounds.height * 0.75,
    );
    await page.mouse.down();
    await page.mouse.move(
      mobileTrendBounds.x + mobileTrendBounds.width * 0.5,
      mobileTrendBounds.y + mobileTrendBounds.height * 0.3,
      { steps: 4 },
    );
    await page.mouse.up();
    assert.notEqual(await page.locator("[data-trend-crosshair-date]").textContent(), mobileTrendDateBefore);
    const initialMobileTrendWidth = await page.locator(".trend-chart-landscape").evaluate((landscape) => landscape.clientWidth);
    await page.getByRole("button", { name: "Daily detail" }).click();
    await page.locator("[data-trend-detail=daily]").waitFor();
    await mobileTrendMetricTrigger.click();
    assert.deepEqual(
      await page.locator(".trend-chart").evaluate(
        // rotate the fixed daily flyover with the scrollable chart
        (chart) => {
          const fixedChrome = chart.querySelector(".trend-chart-fixed-chrome");
          const flyover = chart.querySelector(".trend-metric-flyover");
          const landscape = chart.querySelector(".trend-chart-landscape");

          // require both mobile rotation layers
          if (fixedChrome === null || flyover === null || landscape === null) {
            throw new Error("mobile trend flyover rotation is incomplete");
          }

          return {
            flyoverInsideFixedChrome: flyover.closest(".trend-chart-fixed-chrome") === fixedChrome,
            matchingRotation: getComputedStyle(fixedChrome).transform === getComputedStyle(landscape).transform,
          };
        },
      ),
      { flyoverInsideFixedChrome: true, matchingRotation: true },
    );
    await page.keyboard.press("Escape");
    assert.deepEqual(
      await page.locator(".trend-chart").evaluate(
        // verify the fixed daily canvas scrolls along the rotated annual axis
        (chart) => {
          const landscape = chart.querySelector(".trend-chart-landscape");
          const viewport = chart.querySelector(".trend-chart-viewport");

          // require both rotated detail containers
          if (!(landscape instanceof HTMLElement) || !(viewport instanceof HTMLElement)) {
            throw new Error("mobile trend detail geometry is incomplete");
          }

          return {
            scrollable: viewport.scrollHeight > viewport.clientHeight,
            width: landscape.clientWidth,
          };
        },
      ),
      { scrollable: true, width: 3000 },
    );
    assert.deepEqual(
      await page.locator(".trend-chart").evaluate(
        // keep rotated chart chrome fixed while the daily data canvas moves
        (chart) => {
          const legend = chart.querySelector(".trend-chart-legend");
          const svg = chart.querySelector(".trend-chart-landscape > svg");
          const title = chart.querySelector(".trend-metric-control");
          const viewport = chart.querySelector(".trend-chart-viewport");
          const xAxis = chart.querySelector(".trend-month-axis");

          // require both rotated fixed chrome and scrollable data
          if (
            !(legend instanceof HTMLElement) ||
            !(svg instanceof SVGElement) ||
            !(title instanceof HTMLElement) ||
            !(viewport instanceof HTMLElement) ||
            !(xAxis instanceof HTMLElement)
          ) {
            throw new Error("mobile daily trend scroll layers are incomplete");
          }

          viewport.scrollTop = 0;
          const viewportBounds = viewport.getBoundingClientRect();
          const initial = {
            legend: legend.getBoundingClientRect().top,
            plot: svg.getBoundingClientRect().top,
            title: title.getBoundingClientRect().top,
            xAxis: xAxis.getBoundingClientRect().top,
          };
          viewport.scrollTop = 500;
          const legendBounds = legend.getBoundingClientRect();
          const titleBounds = title.getBoundingClientRect();
          return {
            legendFixed: Math.abs(legend.getBoundingClientRect().top - initial.legend) < 1,
            legendVisible: legendBounds.top >= viewportBounds.top - 1 &&
              legendBounds.bottom <= viewportBounds.bottom + 1,
            plotMoved: svg.getBoundingClientRect().top < initial.plot - 499,
            titleFixed: Math.abs(title.getBoundingClientRect().top - initial.title) < 1,
            titleVisible: titleBounds.top >= viewportBounds.top - 1 &&
              titleBounds.bottom <= viewportBounds.bottom + 1,
            xAxisMoved: xAxis.getBoundingClientRect().top < initial.xAxis - 499,
          };
        },
      ),
      {
        legendFixed: true,
        legendVisible: true,
        plotMoved: true,
        titleFixed: true,
        titleVisible: true,
        xAxisMoved: true,
      },
    );
    await page.locator("[data-trend-scrub-surface]").evaluate(
      // simulate a pinch that must not resize the fixed daily canvas
      (surface) => {
        const bounds = surface.getBoundingClientRect();
        const emit = (type, pointerId, clientY) => surface.dispatchEvent(new PointerEvent(type, {
          bubbles: true,
          buttons: type === "pointerup" ? 0 : 1,
          clientX: bounds.left + bounds.width * 0.5,
          clientY,
          pointerId,
          pointerType: "touch",
        }));
        emit("pointerdown", 51, bounds.top + bounds.height * 0.4);
        emit("pointerdown", 52, bounds.top + bounds.height * 0.6);
        emit("pointermove", 52, bounds.top + bounds.height * 0.8);
        emit("pointerup", 52, bounds.top + bounds.height * 0.8);
        emit("pointerup", 51, bounds.top + bounds.height * 0.4);
      },
    );
    assert.equal(await page.locator(".trend-chart-landscape").evaluate((landscape) => landscape.clientWidth), 3000);
    assert.equal(await page.locator("[data-trend-zoom]").count(), 0);
    await page.locator(".trend-chart-viewport").evaluate(
      // reset the rotated native scroll position
      (viewport) => {
        viewport.scrollTop = 0;
      },
    );
    await page.locator("[data-trend-scrub-surface]").dispatchEvent("wheel", {
      clientX: mobileTrendBounds.x + mobileTrendBounds.width / 2,
      clientY: mobileTrendBounds.y + mobileTrendBounds.height / 2,
      deltaY: 120,
    });
    assert.equal(await page.locator(".trend-chart-viewport").evaluate((viewport) => viewport.scrollTop > 0), true);
    await page.getByRole("button", { name: "7-day average" }).click();
    await page.locator("[data-trend-detail=rolling]").waitFor();
    assert.equal(await page.locator(".trend-chart-landscape").evaluate(
      // preserve the original rotated width floor
      (landscape, initialWidth) => landscape.clientWidth >= initialWidth,
      initialMobileTrendWidth,
    ), true);
    assert.equal(
      await page.locator("body").evaluate(
        // reject rotated-chart horizontal overflow
        (body) => body.scrollWidth > document.documentElement.clientWidth,
      ),
      false,
    );
    await mobileTrendMetricTrigger.click();
    await page.locator('[data-trend-metric-option="windDirectionRose"]').click();
    await page.locator('[data-trend-chart="windDirectionRose"]').waitFor();
    assert.deepEqual(
      await page.locator(".trend-chart").evaluate(
        // keep the polar chart inside the rotated mobile surface
        (chart) => {
          const landscape = chart.querySelector(".trend-chart-landscape");
          const rose = chart.querySelector(".trend-wind-rose");

          // require the complete mobile polar surface
          if (!(landscape instanceof HTMLElement) || !(rose instanceof SVGElement)) {
            throw new Error("mobile wind rose is incomplete");
          }

          const landscapeBounds = landscape.getBoundingClientRect();
          const roseBounds = rose.getBoundingClientRect();
          return {
            labels: rose.querySelectorAll(".trend-wind-rose-label").length,
            roseInsideLandscape: roseBounds.left >= landscapeBounds.left - 1 &&
              roseBounds.right <= landscapeBounds.right + 1 &&
              roseBounds.top >= landscapeBounds.top - 1 &&
              roseBounds.bottom <= landscapeBounds.bottom + 1,
          };
        },
      ),
      { labels: 8, roseInsideLandscape: true },
    );
    await mobileTrendMetricTrigger.click();
    await page.locator('[data-trend-metric-option="extremeDayCount"]').click();
    await page.locator('[data-trend-chart="extremeDayCount"]').waitFor();
    assert.equal(
      await page.locator("[data-trend-extreme-controls]").evaluate(
        // keep threshold controls inside the rotated chart
        (controls) => {
          const chart = controls.closest(".trend-chart-landscape");

          // require one rotated control owner
          if (!(chart instanceof HTMLElement)) {
            return false;
          }

          const bounds = controls.getBoundingClientRect();
          const chartBounds = chart.getBoundingClientRect();
          return bounds.left >= chartBounds.left - 1 &&
            bounds.right <= chartBounds.right + 1 &&
            bounds.top >= chartBounds.top - 1 &&
            bounds.bottom <= chartBounds.bottom + 1;
        },
      ),
      true,
    );
    assert.equal(
      await page.locator("body").evaluate(
        // reject derived-chart horizontal overflow
        (body) => body.scrollWidth > document.documentElement.clientWidth,
      ),
      false,
    );
    await page.goto(`${fixture.origin}/map`, { waitUntil: "networkidle" });
    assert.deepEqual(
      await page.locator(".property-map-layout").evaluate(
        // stack property sensors into ordinary page scroll on phones
        (layout) => {
          const map = layout.querySelector(".property-map");
          const list = layout.querySelector(".property-sensor-list");

          // require both mobile sections
          if (!(map instanceof HTMLElement) || !(list instanceof HTMLElement)) {
            throw new Error("mobile property map layout is incomplete");
          }

          return {
            listBelowMap: list.getBoundingClientRect().top >= map.getBoundingClientRect().bottom,
            overflowY: getComputedStyle(list).overflowY,
            position: getComputedStyle(list).position,
          };
        },
      ),
      { listBelowMap: true, overflowY: "visible", position: "static" },
    );
    await page.locator(".station-map").scrollIntoViewIfNeeded();
    const markerCentersAreClickable = await page.locator(".station-marker").evaluateAll(
      // keep overlay controls away from every marker center
      (markers) => markers.map((marker) => {
        const bounds = marker.getBoundingClientRect();
        const center = document.elementFromPoint(
          bounds.left + bounds.width / 2,
          bounds.top + bounds.height / 2,
        );
        return center?.closest(".station-marker") === marker;
      }),
    );
    assert.equal(markerCentersAreClickable.every(Boolean), true);
    assert.equal(
      await page.locator("body").evaluate(
        // reject map-route horizontal overflow
        (body) => body.scrollWidth > document.documentElement.clientWidth,
      ),
      false,
    );
    await page.goto(`${fixture.origin}/logs`, { waitUntil: "networkidle" });
    const width = await page.evaluate(() => ({
      body: document.body.scrollWidth,
      viewport: document.documentElement.clientWidth,
    }));
    assert.equal(width.body <= width.viewport, true);
    assert.equal(await page.getByRole("button", { name: "Apply filters" }).isVisible(), false);
    await page.getByText("Filters", { exact: true }).click();
    assert.equal(await page.getByRole("button", { name: "Apply filters" }).isVisible(), true);
    assert.equal(await page.locator(".table-scroll").isVisible(), false);
    assert.equal(await page.locator(".history-cards").isVisible(), true);
  } finally {
    await browser?.close();
    fixture.server.close();
    await once(fixture.server, "close");
  }
});
