import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  airQualityBand,
  buildAdminForecastAdjustmentScorecardUrl,
  buildCurrentUrl,
  buildDailyPrecipitationUrl,
  buildForecastUrl,
  buildHistoryUrl,
  buildTidesUrl,
  buildTrendsUrl,
  buildViewerContextUrl,
  clearestCloudRange,
  cloudBand,
  currentWeatherIcon,
  DEFAULT_UNIT_PREFERENCES,
  FORECAST_ADJUSTMENT_MODE_STORAGE_KEY,
  eveningSunTimes,
  forecastMetricValue,
  forecastPressureChanges,
  forecastForSiteDay,
  forecastForSiteDays,
  formatMeasurement,
  fromSiteWallClock,
  humidityBand,
  interpolateForecastInstant,
  interpolateForecastValue,
  loadUnitPreferences,
  parseForecastRecordsResponse,
  parseForecastAdjustmentScorecard,
  pressureBand,
  pressureChangeBand,
  renderWeatherDashboard,
  strongestPressureChange,
  temperatureBand,
  tideLevelLabel,
  toSiteWallClock,
  UNIT_PREFERENCE_STORAGE_KEY,
  uvBand,
  WeatherDashboardController,
  windBand,
} from "../dist/index.js";

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

// represent an incompatible site selection
const secondSite = {
  ...site,
  name: "Coupeville",
  slug: "coupeville",
  stations: [
    {
      ...site.stations[0],
      slug: "coupeville-virtual",
      sources: [
        {
          ...site.stations[0].sources[0],
          id: "20",
        },
      ],
    },
  ],
};

const record = {
  freshness: {
    ageSeconds: 600,
    label: "Model value is current",
    status: "fresh",
  },
  id: "101",
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
    apparentTemperatureC: 15.5,
    blackGlobeTemperatureC: 18.4,
    cloudCoverPercent: 42,
    pm25MicrogramsPerCubicMeter: 7.4,
    precipitationMm: 0.2,
    precipitationRateMmPerHour: 0.4,
    pressureHpa: 1014.2,
    relativeHumidityPercent: 78,
    soilElectricalConductivityMicrosiemensPerCm: 420,
    soilMoisturePercent: 34,
    solarRadiationWm2: 320,
    temperatureC: 16.2,
    uvIndex: 2,
    windDirectionDegrees: 225,
    windGustMps: 7.2,
    windSpeedMps: 4.1,
    wetBulbGlobeTemperatureC: 14.1,
  },
  pressureChange3hHpa: 1.2,
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
  receivedAt: "2026-08-22T04:51:00.000Z",
  revisionCount: 0,
  validAt: "2026-08-22T04:50:00.000Z",
};

const forecastRecord = {
  ...record,
  id: "201",
  productRunAt: "2026-08-22T04:00:00.000Z",
  provenance: {
    ...record.provenance,
    label: "hourly forecast",
    sourceId: "12",
    sourceKey: "open-meteo-forecast-v1",
    sourceKind: "forecast",
  },
  validAt: "2026-08-22T06:00:00.000Z",
};

// build one hourly cloud forecast fixture
function cloudForecastRecord(validAt, cloudCoverPercent, id = validAt) {
  return {
    ...forecastRecord,
    id,
    metrics: { ...forecastRecord.metrics, cloudCoverPercent },
    validAt,
  };
}

// build one cloud forecast fixture with explicit daylight inputs
function classifiedCloudForecastRecord(validAt, cloudCoverPercent, solarRadiationWm2, uvIndex) {
  const forecast = cloudForecastRecord(validAt, cloudCoverPercent);
  return {
    ...forecast,
    metrics: { ...forecast.metrics, solarRadiationWm2, uvIndex },
  };
}

// render one clouds card for focused assertions
function renderCloudTile(forecast, selectedSite = site) {
  const state = {
    ...new WeatherDashboardController({ storage: null }).state,
    current: [record],
    forecast,
    loading: false,
    selectedSite,
  };
  const tile = renderWeatherDashboard(state).match(/<article[^>]*data-condition="clouds"[\s\S]*?<\/article>/u)?.[0];
  assert.ok(tile);
  return tile;
}

// build one hourly pressure forecast fixture
function pressureForecastRecord(
  validAt,
  pressureHpa,
  { id = validAt, productRunAt = "2026-09-12T14:00:00Z", source = "pressure-source" } = {},
) {
  return {
    ...forecastRecord,
    id,
    metrics: { ...forecastRecord.metrics, pressureHpa },
    productRunAt,
    provenance: {
      ...forecastRecord.provenance,
      sourceId: source,
      sourceKey: source,
      stationSlug: source,
    },
    validAt,
  };
}

// isolate one rendered forecast chart
function forecastChartHtml(html, key) {
  const marker = `data-forecast-chart="${key}"`;
  const markerIndex = html.indexOf(marker);
  assert.notEqual(markerIndex, -1);
  const start = html.lastIndexOf("<article", markerIndex);
  const end = html.indexOf("</article>", markerIndex);
  assert.notEqual(start, -1);
  assert.notEqual(end, -1);
  return html.slice(start, end + "</article>".length);
}

// isolate one current-condition card
function conditionCardHtml(html, key) {
  const card = html.match(new RegExp(`<article[^>]*data-condition="${key}"[\\s\\S]*?<\\/article>`, "u"))?.[0];
  assert.ok(card);
  return card;
}

// read one family icon's adjusted marker
function familyIconIsAdjusted(html, view, key) {
  const family = view === "home"
    ? conditionCardHtml(html, key === "rain-rate" ? "rain" : key)
    : forecastChartHtml(html, key);
  return /class="material-symbols-rounded forecast-adjusted-icon"/u.test(family);
}

// decode the serialized chart lines
function forecastChartSeries(html, key) {
  const chart = forecastChartHtml(html, key);
  const encoded = chart.match(/data-forecast-series="([^"]+)"/u)?.[1];
  assert.ok(encoded);
  return JSON.parse(encoded.replaceAll("&quot;", '"').replaceAll("&amp;", "&"));
}

// render one pressure card for focused assertions
function renderPressureTile(current, forecast = [], selectedSite = site) {
  const state = {
    ...new WeatherDashboardController({ storage: null }).state,
    current,
    forecast,
    loading: false,
    selectedSite,
  };
  const tile = renderWeatherDashboard(state).match(/<article[^>]*data-condition="pressure"[\s\S]*?<\/article>/u)?.[0];
  assert.ok(tile);
  return tile;
}

const physicalRecord = {
  ...record,
  freshness: {
    ageSeconds: 120,
    label: "Station reading is current",
    status: "fresh",
  },
  id: "301",
  metrics: {
    ...record.metrics,
    apparentTemperatureC: 12.2,
    pm25MicrogramsPerCubicMeter: null,
    precipitationRateMmPerHour: 0,
    pressureHpa: 1013.5,
    relativeHumidityPercent: 81,
    temperatureC: 12,
    uvIndex: 1,
    windGustMps: 4.5,
    windSpeedMps: 2.5,
  },
  provenance: {
    ...record.provenance,
    label: "nearby physical station",
    providerKey: "weatherflow-tempest",
    sourceId: "13",
    sourceKey: "tempest-38270-observations-v1",
    sourceKind: "physical_sensor",
    stationSlug: "tempest-38270",
  },
};

const ecowittRecord = {
  ...physicalRecord,
  id: "302",
  metadata: {
    ...physicalRecord.metadata,
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
      ],
    },
  },
  metrics: {
    ...physicalRecord.metrics,
    apparentTemperatureC: 9.4,
    pressureHpa: 1012.2,
    relativeHumidityPercent: 88,
    temperatureC: 10,
    uvIndex: 0.5,
    windGustMps: 2,
    windSpeedMps: 1,
  },
  provenance: {
    ...physicalRecord.provenance,
    label: "first-party physical station",
    providerKey: "ecowitt-local",
    sourceId: "14",
    sourceKey: "ecowitt-88f15505d89f-local-live-v1",
    stationSlug: "ballydidean-ecowitt",
  },
};

const trend = {
  metrics: {
    apparentTemperatureC: 15.5,
    precipitationMm: 0.2,
    pressureHpa: 1014.2,
    relativeHumidityPercent: 78,
    temperatureC: 16.2,
    temperatureMaximumC: 19.4,
    temperatureMinimumC: 11.1,
    windDirectionDegrees: 225,
    windGustMps: 7.2,
    windSpeedMps: 4.1,
  },
  validAt: "2026-08-22T04:00:00.000Z",
};

const trendHistory = [
  { ...trend, metrics: { ...trend.metrics, temperatureC: 5, windGustMps: 4 }, validAt: "2019-01-15T08:00:00.000Z" },
  { ...trend, metrics: { ...trend.metrics, temperatureC: 18, windGustMps: 6 }, validAt: "2019-07-15T07:00:00.000Z" },
  { ...trend, metrics: { ...trend.metrics, temperatureC: 7, windGustMps: 5 }, validAt: "2025-01-15T08:00:00.000Z" },
  { ...trend, metrics: { ...trend.metrics, temperatureC: 20, windGustMps: 7 }, validAt: "2025-07-15T07:00:00.000Z" },
  { ...trend, metrics: { ...trend.metrics, temperatureC: 6, windGustMps: 4.5 }, validAt: "2026-01-15T08:00:00.000Z" },
  { ...trend, metrics: { ...trend.metrics, temperatureC: 22, windGustMps: 8 }, validAt: "2026-07-15T07:00:00.000Z" },
];

const dailyPrecipitation = {
  accumulationMm: 2.54,
  source: {
    sourceId: "71",
    stationSlug: "tempest-64255",
  },
  validThrough: "2026-08-22T04:59:00.000Z",
};

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

const adjustmentHashes = {
  bundle: "a".repeat(64),
  candidate: "b".repeat(64),
  report: "c".repeat(64),
  receipt: "d".repeat(64),
  source: "e".repeat(64),
};

// create one exact response runtime
function adjustmentRuntime(state = "active", reasonCode = null) {
  // create one fail-raw runtime
  if (state === "disabled") {
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
      reasonCode,
      state,
      transferReportSha256: null,
    };
  }

  return {
    activationMode: "qualified",
    activeBundle: adjustmentHashes.bundle,
    authorizationSha256: null,
    candidateArtifactSha256: adjustmentHashes.candidate,
    enabledMetrics: ["relativeHumidityPercent", "temperatureC"],
    evaluationReportSha256: adjustmentHashes.report,
    expiresAt: null,
    loadedAt: "2026-08-22T05:00:00.000Z",
    qualificationReceiptSha256: adjustmentHashes.receipt,
    reasonCode: null,
    state: "active",
    transferReportSha256: null,
  };
}

// create one bounded wind-canary runtime
function windCanaryRuntime() {
  return {
    ...adjustmentRuntime(),
    activationMode: "wind_canary",
    authorizationSha256: adjustmentHashes.receipt,
    enabledMetrics: ["windGustMps", "windSpeedMps"],
    evaluationReportSha256: null,
    expiresAt: "2026-09-10T05:00:00.000Z",
    qualificationReceiptSha256: null,
    transferReportSha256: adjustmentHashes.report,
  };
}

// create one bounded active temperature-canary runtime
function temperatureCanaryRuntime(state = "active", reasonCode = null) {
  // create one disabled temperature runtime
  if (state === "disabled") {
    return {
      activeBundle: null,
      authorizationSha256: null,
      expiresAt: null,
      loadedAt: "2026-08-22T05:00:00.000Z",
      reasonCode,
      source: null,
      state,
    };
  }

  return {
    activeBundle: "f".repeat(64),
    authorizationSha256: "9".repeat(64),
    expiresAt: "2026-08-30T00:00:00.000Z",
    loadedAt: "2026-08-22T05:00:00.000Z",
    reasonCode: null,
    source: {
      adaptiveReady: false,
      firstReceivedAt: "2026-08-22T00:05:00.000Z",
      hourCount: 19,
      latestRunInitializedAt: "2026-08-21T18:00:00.000Z",
      stateReason: "cold_start",
      stateStatus: "cold",
    },
    state: "active",
  };
}

// create one explicit ECMWF temperature decision
function temperatureCanaryDecision(recordValue) {
  const runtime = temperatureCanaryRuntime();
  return {
    branch: "direct",
    bundleSha256: runtime.activeBundle,
    contractVersion: "forecast-temperature-canary-decision/v1",
    correctedTemperatureC: 15,
    rawBestMatchTemperatureC: recordValue.metrics.temperatureC,
    reasonCode: null,
    recentErrorStateSha256: "8".repeat(64),
    sourceForecast: {
      adapterVersion: "open-meteo-ecmwf-single-run/v1",
      dataset: "single_run",
      firstReceivedAt: "2026-08-22T00:05:00.000Z",
      modelCycle: "50r1",
      modelLeadHours: 12,
      operationalHorizonHours: 6,
      providerKey: "open-meteo",
      providerResponseSha256: "7".repeat(64),
      rawRelativeHumidityPercent: 80,
      rawTemperatureC: 14,
      rawWindSpeedMps: 2,
      runInitializedAt: "2026-08-21T18:00:00.000Z",
      upstreamModel: "ecmwf_ifs",
      validAt: recordValue.validAt,
    },
    state: "active",
  };
}

// create one active causal rain source and decision
function rainAdjustmentDecision(recordValue) {
  return {
    contractVersion: "forecast-rain-adjustment-decision/v1",
    state: "active",
    reasonCode: null,
    bundleSha256: "4".repeat(64),
    correctedPrecipitationMm: 2.5,
    rawBestMatchPrecipitationMm: recordValue.metrics.precipitationMm,
    sourceForecast: {
      runInitializedAt: "2026-08-21T18:00:00.000Z",
      firstReceivedAt: "2026-08-22T00:05:00.000Z",
      validAt: recordValue.validAt,
      rawPrecipitationMm: 2,
      modelLeadHours: 12,
      decisionAt: "2026-08-22T02:00:00.000Z",
      upstreamModel: "ecmwf_ifs",
      providerKey: "open-meteo",
    },
  };
}

// create one active bounded rain runtime
function rainAdjustmentRuntime() {
  return {
    state: "active",
    activeBundle: "4".repeat(64),
    reasonCode: null,
    loadedAt: "2026-08-22T03:00:00.000Z",
    source: {
      runInitializedAt: "2026-08-21T18:00:00.000Z",
      firstReceivedAt: "2026-08-22T00:05:00.000Z",
      decisionAt: "2026-08-22T02:00:00.000Z",
      hourCount: 23,
    },
  };
}

// create one exact active row decision
function activeAdjustment(recordValue, targetLeadHours = 1) {
  const referenceAt = new Date(
    Date.parse(recordValue.validAt) - targetLeadHours * 3_600_000,
  ).toISOString();
  return {
    adjustedMetrics: {
      relativeHumidityPercent: recordValue.metrics.relativeHumidityPercent - 5,
      temperatureC: recordValue.metrics.temperatureC + 2,
    },
    algorithmContractVersion: "robust-hierarchical-median/v1",
    appliedMetrics: ["temperatureC", "relativeHumidityPercent"],
    candidateArtifactSha256: adjustmentHashes.candidate,
    contractVersion: "forecast-adjustment-decision/v1",
    evaluationReportSha256: adjustmentHashes.report,
    leadBand: targetLeadHours <= 24 ? "001-024" : "145-168",
    qualificationReceiptSha256: adjustmentHashes.receipt,
    rawForecastProvenance: {
      adapterVersion: "open-meteo-forecast-daily/v4",
      cohort: "legacy_v4_retrieval_snapshot",
      contractEpoch:
        "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83",
      dataset: recordValue.metadata.provider.dataset,
      referenceAt,
      referenceKind: "retrieval_snapshot",
      sourceConfigFingerprint: adjustmentHashes.source,
      sourceKey: recordValue.provenance.sourceKey,
      targetLeadHours,
      upstreamModel: recordValue.metadata.upstream.model,
      validAt: recordValue.validAt,
    },
    reasonCode: null,
    state: "active",
  };
}

// create one wind-only active decision
function windCanaryAdjustment(recordValue, targetLeadHours = 1) {
  const qualified = activeAdjustment(recordValue, targetLeadHours);
  return {
    activationKind: "wind_transfer_canary",
    adjustedMetrics: {
      windGustMps: Math.max(0, recordValue.metrics.windGustMps - 2),
      windSpeedMps: recordValue.metrics.windSpeedMps + 0.4,
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

// create one exact fail-raw row decision
function failRawAdjustment(state, reasonCode) {
  return {
    adjustedMetrics: {},
    appliedMetrics: [],
    contractVersion: "forecast-adjustment-decision/v1",
    reasonCode,
    state,
  };
}

// create one complete forecast rendering state
function forecastState(records, runtime, forecastDays = 1) {
  return {
    current: [record],
    dailyPrecipitation: null,
    error: null,
    filters: {},
    forecastAdjustmentMode: "adjusted",
    forecastAdjustmentSettings: null,
    forecast: records,
    forecastAdjustmentRuntime: runtime,
    forecastRainAdjustmentRuntime: null,
    forecastTemperatureAdjustmentRuntime: null,
    adminAdjustmentSettingsSaving: false,
    adminAdjustmentSettingsMessage: null,
    forecastDays,
    history: [],
    loading: false,
    mapLayer: "roads",
    nextCursor: null,
    page: 0,
    propertyMapLayer: "satellite",
    propertySensorLayout: [],
    selectedPropertySensorKey: null,
    selectedStationSlug: null,
    selectedSite: site,
    selectedTrendMetric: "temperatureC",
    selectedTrendYear: null,
    sites: [site],
    tideGeneratedAt: null,
    tides: [],
    trendDetail: "rolling",
    trendDisplayMode: "aggregate",
    trendExtremeKind: "heat",
    trendExtremeThreshold: 30,
    trendGeneratedAt: null,
    trends: [],
    units: DEFAULT_UNIT_PREFERENCES,
  };
}

// render one focused home-alert state
function renderAlertState({
  current = [record],
  forecast = [],
  mode = "adjusted",
  rainRuntime = null,
  runtime = null,
  settings = null,
  temperatureRuntime = null,
  units = DEFAULT_UNIT_PREFERENCES,
}) {
  return renderWeatherDashboard({
    ...forecastState(forecast, runtime),
    current,
    forecastAdjustmentMode: mode,
    forecastAdjustmentSettings: settings,
    forecastRainAdjustmentRuntime: rainRuntime,
    forecastTemperatureAdjustmentRuntime: temperatureRuntime,
    units,
  });
}

// create one complete aggregate-only administrator scorecard
function adjustmentScorecard() {
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
  // create one unit-bound metric block
  const metric = (unit, skillPercent = 12.5) => ({
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
    skillPercent,
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
      slices: [{ dimension: "horizon", label: "001-024", rowCount: 120, metrics: metric(unit, 10) }],
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
    // retain every fixed reliability decile
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
      // create one fixed probability threshold
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

// create one sanitized automatic-policy scorecard
function adjustmentScorecardV2(overrides = {}) {
  const legacy = adjustmentScorecard();
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
    actionState: "active",
    contractVersion: "forecast-adjustment-scorecard/v2",
    families: legacy.families,
    generatedAt: "2099-10-01T00:00:00.000Z",
    history: [{
      actionLineageSha256: "6".repeat(64),
      actionProjectionSha256: "5".repeat(64),
      actionState: "active",
      attemptSha256: "2".repeat(64),
      occurredAt: "2099-10-01T00:00:00.000Z",
      policyDecision: "qualified",
      releaseManifestSha256: "3".repeat(64),
    }],
    identities: {
      active: identities("7".repeat(64)),
      prior: identities(null),
      raw: identities("8".repeat(64)),
      shadow: identities(null),
    },
    inputs: {
      ...legacy.inputs,
      frontierSha256: "9".repeat(64),
      inputManifestSha256: "a".repeat(64),
      reportSha256: "b".repeat(64),
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
    ...overrides,
  };
}

// preserve adjustment behavior without the removed infobox
test("forecast adjustment boundary preserves raw and validates active metadata", () => {
  const raw = {
    ...forecastRecord,
    futureApiField: { contractVersion: 2 },
    metadata: {
      ...forecastRecord.metadata,
      provider: { ...forecastRecord.metadata.provider, dataset: "forecast" },
    },
    metrics: { ...forecastRecord.metrics, futureMetric: 123 },
  };
  const active = { ...raw, adjustment: activeAdjustment(raw) };
  const parsed = parseForecastRecordsResponse({
    adjustmentRuntime: adjustmentRuntime(),
    data: [active],
    site,
  });
  const parsedRecord = parsed.data[0];

  assert.equal(parsedRecord.metrics.temperatureC, raw.metrics.temperatureC);
  assert.deepEqual(parsedRecord.futureApiField, raw.futureApiField);
  assert.equal(parsedRecord.metrics.futureMetric, raw.metrics.futureMetric);
  assert.equal(forecastMetricValue(parsedRecord, "temperatureC"), raw.metrics.temperatureC + 2);
  assert.equal(forecastMetricValue(parsedRecord, "temperatureC", false), raw.metrics.temperatureC);
  assert.equal(forecastMetricValue(parsedRecord, "pressureHpa"), raw.metrics.pressureHpa);

  const html = renderWeatherDashboard(
    forecastState(parsed.data, parsed.adjustmentRuntime),
    "forecast",
  );
  assert.match(html, /data-forecast-adjustment-available="true"/u);
  assert.match(html, /aria-checked="true"\s+aria-label="Adjusted"/u);
  assert.match(html, /class="forecast-adjustment-sparkle" data-sparkle-tone="gold"/u);
  assert.doesNotMatch(html, /data-forecast-adjustment-status|Raw and adjusted source details/u);

  const rawHtml = renderWeatherDashboard(
    {
      ...forecastState(parsed.data, parsed.adjustmentRuntime),
      forecastAdjustmentMode: "raw",
    },
    "forecast",
  );
  assert.match(rawHtml, /class="forecast-adjustment-toggle"[\s\S]*aria-checked="false"[\s\S]*data-forecast-adjustment-toggle/u);
  assert.match(rawHtml, /aria-label="Adjusted"/u);
  assert.match(rawHtml, /class="forecast-adjustment-sparkle" data-sparkle-tone="gray"/u);
  assert.doesNotMatch(rawHtml, /data-forecast-adjustment-status|Local adjustment turned off/u);
});

// retain accessible switch state without a visible label or separate chip content
test("sparkle adjustment switch exposes both preferences without visible text", () => {
  // use the same control on both forecast-bearing routes
  for (const view of ["home", "forecast"]) {
    // retain the persisted preference even when corrections are temporarily unavailable
    for (const mode of ["adjusted", "raw"]) {
      const html = renderWeatherDashboard({
        ...forecastState([], null),
        forecastAdjustmentMode: mode,
      }, view);
      const toggle = html.match(/<button\s+type="button"\s+class="forecast-adjustment-toggle"[\s\S]*?<\/button>/u)?.[0];
      assert.ok(toggle);
      assert.match(toggle, /role="switch"/u);
      assert.match(toggle, /aria-label="Adjusted"/u);
      assert.ok(toggle.includes(`aria-checked="${String(mode === "adjusted")}"`));
      assert.equal(toggle.replace(/<[^>]+>/gu, "").trim(), "");
      assert.doesNotMatch(toggle, /forecast-adjustment-toggle-mode/u);
      assert.match(toggle, /class="forecast-adjustment-toggle-thumb"[\s\S]*class="forecast-adjustment-sparkle"/u);
      assert.ok(toggle.includes(`data-sparkle-tone="${mode === "adjusted" ? "gold" : "gray"}"`));
      assert.match(toggle, /fill="currentColor" stroke="currentColor"/u);
      assert.doesNotMatch(toggle, /<linearGradient|<defs>|url\(#forecast-adjustment-gold\)/u);
      assert.match(toggle, /class="forecast-adjustment-toggle-track" aria-hidden="true"/u);
    }
  }
});

// keep one current-weather illustration in navigation rather than the page header
test("Now navigation owns the current weather icon on every route", () => {
  const state = {
    ...forecastState([], null),
    current: [{ ...record, metrics: { ...record.metrics, cloudCoverPercent: 80, precipitationRateMmPerHour: 0 } }],
  };
  // retain identical navigation artwork and naming on active and inactive routes
  for (const view of ["home", "forecast", "map", "trends", "settings", "logs", "admin"]) {
    const html = renderWeatherDashboard(state, view);
    const header = html.match(/<header[\s\S]*?<\/header>/u)?.[0];
    const nowLink = html.match(/<a class="section-nav-home"[\s\S]*?<\/a>/u)?.[0];
    assert.ok(header);
    assert.ok(nowLink);
    assert.doesNotMatch(header, /<img|masthead-brand/u);
    assert.match(nowLink, /aria-label="Now"/u);
    assert.match(nowLink, /<img class="section-nav-weather-icon" src="\/weather-icons\/05-cloudy\.svg" alt="Current weather: Cloudy" width="32" height="32">/u);
    assert.equal((html.match(/class="section-nav-weather-icon"/gu) ?? []).length, 1);
    assert.doesNotMatch(html, /data-nav-icon="dashboard"|masthead-weather-icon/u);
  }
});

// isolate raw current weather from every physical station reading
test("raw mode uses model current values without sensor fallback or homepage supplements", () => {
  const quietModel = {
    ...record,
    pressureChange3hHpa: -1,
    metrics: {
      ...record.metrics,
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
    ...ecowittRecord,
    pressureChange3hHpa: 9,
    metrics: {
      ...ecowittRecord.metrics,
      apparentTemperatureC: 35,
      pm25MicrogramsPerCubicMeter: 50,
      precipitationRateMmPerHour: 8,
      pressureHpa: 1_024,
      temperatureC: 34,
      windGustMps: 20,
      windSpeedMps: 10,
      wetBulbGlobeTemperatureC: 30,
    },
  };
  const baseState = {
    ...forecastState([], null),
    current: [quietModel, hazardousLocal],
    dailyPrecipitation,
    history: [hazardousLocal],
    homeNetwork: true,
    propertySensorLayout: [{
      displayName: "Orchard soil",
      icon: "rain",
      latitude: 47.9505,
      longitude: -122.4281,
      sensorKey: "soil-1",
      updatedAt: "2026-08-22T04:59:00.000Z",
    }],
    trends: trendHistory,
  };
  const adjusted = renderWeatherDashboard(baseState, "home", true);
  const rawState = { ...baseState, forecastAdjustmentMode: "raw" };
  const raw = renderWeatherDashboard(rawState, "home", true);

  assert.match(conditionCardHtml(adjusted, "temperature"), /class="condition-primary"><strong>95<small>°F/u);
  assert.match(conditionCardHtml(adjusted, "wind"), /class="condition-primary"><strong>22<small>mph SW/u);
  assert.match(conditionCardHtml(adjusted, "rain"), /class="condition-primary"><strong>0\.31<small>in\/h/u);
  assert.match(conditionCardHtml(adjusted, "rain"), /Accumulation[\s\S]*?0\.1<small>in/u);
  assert.match(conditionCardHtml(adjusted, "air-quality"), /class="condition-primary"><strong>50/u);
  assert.match(conditionCardHtml(adjusted, "pressure"), /class="condition-primary"><strong>\+9\.0/u);
  assert.match(adjusted, /Heat stress[\s\S]*High wind[\s\S]*Heavy rain[\s\S]*Air quality/u);
  assert.match(adjusted, /\/weather-icons\/10-heavy-rain-wind\.svg/u);
  assert.match(adjusted, /data-indoor-house[\s\S]*data-admin-soil-map/u);

  assert.match(conditionCardHtml(raw, "temperature"), /class="condition-primary"><strong>50<small>°F/u);
  assert.match(conditionCardHtml(raw, "wind"), /class="condition-primary"><strong>2<small>mph SW/u);
  assert.match(conditionCardHtml(raw, "rain"), /class="condition-primary"><strong>0<small>in\/h/u);
  assert.match(conditionCardHtml(raw, "rain"), /Accumulation[\s\S]*?<strong>—<\/strong>/u);
  assert.match(conditionCardHtml(raw, "air-quality"), /class="condition-primary"><strong>5/u);
  assert.match(conditionCardHtml(raw, "pressure"), /class="condition-primary"><strong>-1\.0/u);
  assert.doesNotMatch(raw, /class="alert-list"/u);
  assert.match(raw, /\/weather-icons\/05-cloudy\.svg/u);
  assert.doesNotMatch(raw, /data-indoor-house|data-admin-soil-map/u);

  const hazardousModel = {
    ...quietModel,
    pressureChange3hHpa: -4,
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
    pressureChange3hHpa: 1,
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
  const reverseState = {
    ...baseState,
    current: [hazardousModel, quietLocal],
    forecastAdjustmentMode: "raw",
  };
  const reverseRaw = renderWeatherDashboard(reverseState, "home", true);
  const reverseAdjusted = renderWeatherDashboard({
    ...reverseState,
    forecastAdjustmentMode: "adjusted",
  }, "home", true);
  assert.match(reverseRaw, /Heat stress[\s\S]*High wind[\s\S]*Heavy rain[\s\S]*Air quality/u);
  assert.match(reverseRaw, /\/weather-icons\/10-heavy-rain-wind\.svg/u);
  assert.match(conditionCardHtml(reverseAdjusted, "temperature"), /class="condition-primary"><strong>54<small>°F/u);
  assert.doesNotMatch(reverseAdjusted, /\/weather-icons\/10-heavy-rain-wind\.svg/u);

  const missingMetrics = Object.fromEntries(Object.keys(record.metrics).map(
    // remove every modeled metric without borrowing a station value
    (metric) => [metric, null],
  ));
  const nullModel = { ...quietModel, metrics: missingMetrics, pressureChange3hHpa: null };
  const nullRaw = renderWeatherDashboard({
    ...rawState,
    current: [nullModel, hazardousLocal],
  }, "home", true);
  assert.match(conditionCardHtml(nullRaw, "temperature"), /class="condition-primary"><strong>—<\/strong>/u);
  assert.match(conditionCardHtml(nullRaw, "wind"), /class="condition-primary"><strong>—<\/strong>/u);
  assert.match(conditionCardHtml(nullRaw, "rain"), /class="condition-primary"><strong>—<\/strong>/u);
  assert.match(conditionCardHtml(nullRaw, "pressure"), /class="condition-primary"><strong>—<\/strong>/u);
  assert.match(nullRaw, /\/weather-icons\/12-unavailable\.svg/u);
  assert.doesNotMatch(nullRaw, /class="alert-list"/u);

  const missingModelRaw = renderWeatherDashboard({
    ...rawState,
    current: [hazardousLocal],
  }, "home", true);
  assert.match(missingModelRaw, /No current weather value is available yet\./u);
  assert.match(missingModelRaw, /\/weather-icons\/12-unavailable\.svg/u);
  assert.doesNotMatch(missingModelRaw, /class="current-conditions"|data-indoor-house|data-admin-soil-map/u);

  assert.match(renderWeatherDashboard(rawState, "map", true), /data-property-sensor-view="soil-1"/u);
  assert.match(renderWeatherDashboard(rawState, "admin", true), /data-property-sensor-form/u);
  assert.match(renderWeatherDashboard(rawState, "logs", true), /Past conditions/u);
  assert.match(renderWeatherDashboard(rawState, "trends", true), /data-trend-metric-control/u);
});

// create one nearby station with explicit source and observation identities
function localFallbackRecord(stationSlug, metrics, overrides = {}) {
  return {
    ...physicalRecord,
    id: stationSlug,
    metrics: { ...physicalRecord.metrics, ...metrics },
    provenance: { ...physicalRecord.provenance, stationSlug, sourceKey: `${stationSlug}-current` },
    ...overrides,
  };
}

// average available metrics rather than replacing an outage with one arbitrary station
test("home current readings average non-stale local stations when the farm is absent or stale", () => {
  const first = localFallbackRecord("nearby-a", {
    apparentTemperatureC: 10, temperatureC: 10, relativeHumidityPercent: 40, precipitationRateMmPerHour: 0,
    windSpeedMps: 2, windGustMps: 4, windDirectionDegrees: 350, pm25MicrogramsPerCubicMeter: null, uvIndex: 0,
  }, { pressureChange3hHpa: -2 });
  const second = localFallbackRecord("nearby-b", {
    apparentTemperatureC: 20, temperatureC: 20, relativeHumidityPercent: 60, precipitationRateMmPerHour: 5.08,
    windSpeedMps: 4, windGustMps: 8, windDirectionDegrees: 10, pm25MicrogramsPerCubicMeter: 4, uvIndex: 2,
  }, { pressureChange3hHpa: -4, freshness: { ...physicalRecord.freshness, status: "delayed" } });
  const stale = localFallbackRecord("offline", { apparentTemperatureC: 100, windGustMps: 100 }, {
    freshness: { ...physicalRecord.freshness, status: "stale" },
  });
  // exercise a wholly missing farm and an old farm reading without changing the denominator
  for (const farm of [[], [{ ...ecowittRecord, freshness: stale.freshness }]]) {
    const state = { ...forecastState([], null), current: [record, stale, second, ...farm, first], dailyPrecipitation };
    const original = structuredClone(state.current);
    const html = renderWeatherDashboard(state);
    assert.match(conditionCardHtml(html, "temperature"), /class="condition-primary"><strong>59<small>°F/u);
    assert.match(conditionCardHtml(html, "temperature"), /Air Temp[\s\S]*?<strong>59<small>°F/u);
    assert.match(conditionCardHtml(html, "humidity"), /class="condition-primary"><strong>50<small>%/u);
    assert.match(conditionCardHtml(html, "wind"), /class="condition-primary"><strong>7<small>mph N/u);
    assert.match(conditionCardHtml(html, "wind"), /Gusts[\s\S]*?<strong>13<small>mph/u);
    assert.match(conditionCardHtml(html, "rain"), /class="condition-primary"><strong>0\.1<small>in\/h/u);
    assert.match(conditionCardHtml(html, "rain"), /Accumulation[\s\S]*?<strong>0\.1<small>in/u);
    assert.match(conditionCardHtml(html, "air-quality"), /class="condition-primary"><strong>4<\/strong>/u);
    assert.match(conditionCardHtml(html, "uv-index"), /class="condition-primary"><strong>1/u);
    assert.match(conditionCardHtml(html, "pressure"), /class="condition-primary"><strong>-2\.0/u);
    assert.match(conditionCardHtml(html, "clouds"), /class="condition-primary"><strong>42<small>%/u);
    assert.equal(currentWeatherIcon(state).name, "09-heavy-rain");
    assert.doesNotMatch(html, /class="alert-list"/u);
    assert.deepEqual(state.current, original);
  }
});

// retain zeros and working farm sensors during a partial outage
test("home current readings fill only missing or non-finite farm metrics", () => {
  const farm = { ...ecowittRecord, metrics: {
    ...ecowittRecord.metrics, apparentTemperatureC: null, temperatureC: Number.NaN,
    relativeHumidityPercent: 0, precipitationRateMmPerHour: 0, windSpeedMps: 0, windGustMps: 0,
    windDirectionDegrees: null, pm25MicrogramsPerCubicMeter: 0, uvIndex: Number.POSITIVE_INFINITY,
  } };
  const neighbors = [10, 20].map(
    // deliberately disagree with the farm's usable zero measurements
    (value) => localFallbackRecord(`nearby-${value}`, {
      apparentTemperatureC: value, temperatureC: value, windDirectionDegrees: 90, uvIndex: 2,
    }),
  );
  const html = renderWeatherDashboard({ ...forecastState([], null), current: [record, ...neighbors, farm] });
  assert.match(conditionCardHtml(html, "temperature"), /class="condition-primary"><strong>59<small>°F/u);
  assert.match(conditionCardHtml(html, "wind"), /class="condition-primary"><strong>0<small>mph E/u);
  assert.match(conditionCardHtml(html, "wind"), /Gusts[\s\S]*?<strong>0<small>mph/u);
  assert.match(conditionCardHtml(html, "rain"), /class="condition-primary"><strong>0<small>in\/h/u);
  assert.match(conditionCardHtml(html, "humidity"), /class="condition-primary"><strong>0<small>%/u);
  assert.match(conditionCardHtml(html, "air-quality"), /class="condition-primary"><strong>0/u);
  assert.match(conditionCardHtml(html, "uv-index"), /class="condition-primary"><strong>2/u);
  assert.doesNotMatch(html, /NaN|Infinity|class="alert-list"/u);
});

// preserve valid negative temperatures while rejecting impossible unsigned readings
test("home local fallback rejects impossible metric domains without rejecting freezing temperatures", () => {
  const farm = { ...ecowittRecord, metrics: {
    ...ecowittRecord.metrics, apparentTemperatureC: -10, temperatureC: -10, relativeHumidityPercent: 101,
    precipitationRateMmPerHour: -1, windSpeedMps: -1, windGustMps: 151, windDirectionDegrees: 361,
    pm25MicrogramsPerCubicMeter: -1, uvIndex: -1, pressureHpa: -1,
  } };
  const neighbor = localFallbackRecord("available", {
    relativeHumidityPercent: 50, precipitationRateMmPerHour: 0, windSpeedMps: 1, windGustMps: 2,
    windDirectionDegrees: 0, pm25MicrogramsPerCubicMeter: 0, uvIndex: 0, pressureHpa: 1_013,
  }, { pressureChange3hHpa: -1 });
  const html = renderWeatherDashboard({ ...forecastState([], null), current: [farm, neighbor, record] });
  assert.match(conditionCardHtml(html, "temperature"), /class="condition-primary"><strong>14<small>°F/u);
  assert.match(conditionCardHtml(html, "wind"), /class="condition-primary"><strong>2<small>mph N/u);
  assert.match(conditionCardHtml(html, "wind"), /Gusts[\s\S]*?<strong>4<small>mph/u);
  assert.match(conditionCardHtml(html, "humidity"), /class="condition-primary"><strong>50<small>%/u);
  assert.match(conditionCardHtml(html, "rain"), /class="condition-primary"><strong>0<small>in\/h/u);
  assert.match(conditionCardHtml(html, "air-quality"), /class="condition-primary"><strong>0/u);
  assert.match(conditionCardHtml(html, "uv-index"), /class="condition-primary"><strong>0/u);
  assert.match(conditionCardHtml(html, "pressure"), /class="condition-primary"><strong>-1\.0/u);
});

// count each station once per metric and omit an undefined circular wind direction
test("home local averages deduplicate providers using the newest nonmissing station metric", () => {
  const older = localFallbackRecord("nearby-a", { apparentTemperatureC: 10, temperatureC: 10, windDirectionDegrees: 90 });
  const newer = { ...older, validAt: "2026-08-22T04:55:00Z", metrics: {
    ...older.metrics, apparentTemperatureC: 20, temperatureC: null,
  }, provenance: { ...older.provenance, sourceKey: "alternate-provider" } };
  const second = localFallbackRecord("nearby-b", { apparentTemperatureC: 40, temperatureC: 30, windDirectionDegrees: 270 });
  // never include virtual historical future or stale readings in the nearby denominator
  const invalid = [
    { ...older, metrics: { ...older.metrics, temperatureC: 100 }, freshness: { ...older.freshness, status: "stale" } },
    { ...older, validAt: "2099-01-01T00:00:00Z" },
    { ...record, provenance: { ...record.provenance, sourceKind: "reanalysis" } },
    forecastRecord,
  ];
  // source ordering must not change an average
  for (const current of [[older, second, newer, ...invalid], [...invalid, newer, second, older]]) {
    const html = renderWeatherDashboard({ ...forecastState([], null), current });
    assert.match(conditionCardHtml(html, "temperature"), /class="condition-primary"><strong>86<small>°F/u);
    assert.match(conditionCardHtml(html, "temperature"), /Air Temp[\s\S]*?<strong>68<small>°F/u);
    assert.match(conditionCardHtml(html, "wind"), /class="condition-primary"><strong>6<small>mph<\/small>/u);
  }
});

// keep per-metric farm precedence even when parallel current sources have different sensor gaps
test("home fallback preserves the newest usable farm source for each metric", () => {
  const old = { ...ecowittRecord, metrics: { ...ecowittRecord.metrics, apparentTemperatureC: 12, temperatureC: 12 }, pressureChange3hHpa: 9 };
  const newer = { ...old, validAt: "2026-08-22T04:55:00Z", metrics: {
    ...old.metrics, apparentTemperatureC: 20, temperatureC: null,
  }, pressureChange3hHpa: null, provenance: { ...old.provenance, sourceKey: "new-farm-source" } };
  const neighbor = localFallbackRecord("nearby", { apparentTemperatureC: 30, temperatureC: 30 });
  const html = renderWeatherDashboard({ ...forecastState([], null), current: [old, record, neighbor, newer] });
  assert.match(conditionCardHtml(html, "temperature"), /class="condition-primary"><strong>68<small>°F/u);
  assert.match(conditionCardHtml(html, "temperature"), /Air Temp[\s\S]*?<strong>54<small>°F/u);
  assert.match(conditionCardHtml(html, "pressure"), /class="condition-primary"><strong>—<\/strong>/u);
});

// enforce observed domains before averaging so invalid stations cannot trigger bogus watches
test("home fallback applies canonical upper bounds and preserves valid wet-bulb temperatures", () => {
  const invalid = localFallbackRecord("out-of-domain", {
    pm25MicrogramsPerCubicMeter: 1_000, precipitationRateMmPerHour: 10_001, pressureHpa: 1_201, uvIndex: 21,
    apparentTemperatureC: 71, temperatureC: -101, wetBulbGlobeTemperatureC: 126, windDirectionDegrees: 360,
  }, { pressureChange3hHpa: 20 });
  const state = { ...forecastState([], null), current: [invalid, record] };
  const html = renderWeatherDashboard(state);
  assert.match(conditionCardHtml(html, "temperature"), /class="condition-primary"><strong>60<small>°F/u);
  assert.match(conditionCardHtml(html, "temperature"), /Air Temp[\s\S]*?<strong>61<small>°F/u);
  assert.match(conditionCardHtml(html, "air-quality"), /class="condition-primary"><strong>7/u);
  assert.match(conditionCardHtml(html, "uv-index"), /class="condition-primary"><strong>2/u);
  assert.match(conditionCardHtml(html, "rain"), /class="condition-primary"><strong>0\.02<small>in\/h/u);
  assert.match(conditionCardHtml(html, "pressure"), /class="condition-primary"><strong>\+1\.2/u);
  assert.doesNotMatch(html, /class="alert-list"/u);
  const wetBulb = { ...invalid, metrics: { ...invalid.metrics, wetBulbGlobeTemperatureC: 95 } };
  assert.match(renderWeatherDashboard({ ...state, current: [wetBulb, record] }), /Wet-bulb globe temperature 203°F/u);
  assert.match(renderPressureTile([{ ...invalid, metrics: { ...invalid.metrics, pressureHpa: 99 } }]), /class="condition-primary"><strong>—<\/strong>/u);
});

// use the displayed mean for current watches rather than each neighbor's maximum
test("home outage fallback keeps forecast cards on today after local midnight", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-08-22T07:05:00Z") });
  const neighbor = localFallbackRecord("hourly-neighbor", { apparentTemperatureC: 10 }, {
    validAt: "2026-08-22T06:00:00Z", freshness: { ...physicalRecord.freshness, status: "delayed" },
  });
  const forecast = [
    { ...forecastRecord, validAt: "2026-08-22T06:00:00Z", metrics: { ...forecastRecord.metrics, temperatureC: 40, apparentTemperatureC: 40 } },
    { ...forecastRecord, validAt: "2026-08-22T08:00:00Z", metrics: { ...forecastRecord.metrics, temperatureC: 20, apparentTemperatureC: 20 } },
  ];
  const html = renderWeatherDashboard({ ...forecastState(forecast, null), current: [record, neighbor] });
  const tile = conditionCardHtml(html, "temperature");
  assert.match(tile, /class="condition-primary"><strong>50<small>°F/u);
  assert.match(tile, /Max<\/span> <strong>68<small>°F/u);
  assert.doesNotMatch(tile, /104<small>°F/u);
});

// use the displayed mean for current watches rather than each neighbor's maximum
test("home alerts use local averages and retain forecast watches independently", () => {
  const metrics = { apparentTemperatureC: 40, wetBulbGlobeTemperatureC: 35, windGustMps: 20,
    precipitationRateMmPerHour: 10, pm25MicrogramsPerCubicMeter: 50 };
  const quiet = Object.fromEntries(Object.keys(metrics).map(
    // balance every hazardous neighbor reading below the alert threshold
    (metric) => [metric, 0],
  ));
  const current = [localFallbackRecord("hazardous", metrics), localFallbackRecord("quiet", quiet)];
  const state = { ...forecastState([], null), current };
  assert.doesNotMatch(renderWeatherDashboard(state), /class="alert-list"/u);
  const hazardous = renderWeatherDashboard({ ...state, current: [current[0]] });
  assert.match(hazardous, /Heat stress[\s\S]*High wind[\s\S]*Heavy rain[\s\S]*Air quality/u);
  const forecast = { ...forecastRecord, metrics: { ...forecastRecord.metrics, temperatureC: -1, windGustMps: 20 } };
  assert.match(renderWeatherDashboard({ ...state, forecast: [forecast] }), /Frost possible[\s\S]*High wind/u);
});

// preserve regional fallback and honest missing states without reviving offline stations
test("home local fallback uses regional current only when no station supplies the metric", () => {
  const unavailable = { ...ecowittRecord, freshness: { ...ecowittRecord.freshness, status: "stale" } };
  const html = renderWeatherDashboard({ ...forecastState([], null), current: [unavailable, record] });
  assert.match(conditionCardHtml(html, "temperature"), /class="condition-primary"><strong>60<small>°F/u);
  const missing = renderWeatherDashboard({ ...forecastState([], null), current: [unavailable] });
  assert.match(missing, /No current weather value is available yet/u);
  assert.doesNotMatch(missing, /class="alert-list"/u);
  const empty = { ...physicalRecord, metrics: Object.fromEntries(Object.keys(physicalRecord.metrics).map(
    // keep every absent local metric honestly unavailable
    (metric) => [metric, null],
  )) };
  const emptyHtml = renderWeatherDashboard({ ...forecastState([], null), current: [empty] });
  assert.match(conditionCardHtml(emptyHtml, "temperature"), /class="condition-primary"><strong>—<\/strong>/u);
  assert.match(conditionCardHtml(emptyHtml, "rain"), /class="condition-primary"><strong>—<\/strong>/u);
  assert.match(conditionCardHtml(emptyHtml, "pressure"), /class="condition-primary"><strong>—<\/strong>/u);
  const emptyModel = { ...record, metrics: { ...record.metrics, pressureHpa: null } };
  assert.match(renderPressureTile([unavailable, emptyModel]), /class="condition-primary"><strong>—<\/strong>/u);
});

// keep the local tendency tied to the same current source chosen for each station's pressure
test("home local pressure tendency excludes old duplicate and unavailable contributor history", () => {
  const old = localFallbackRecord("nearby-a", { pressureHpa: 1_010 }, { pressureChange3hHpa: 10 });
  const latest = { ...old, validAt: "2026-08-22T04:55:00Z", pressureChange3hHpa: null,
    provenance: { ...old.provenance, sourceKey: "alternate-provider" } };
  const available = localFallbackRecord("nearby-b", { pressureHpa: 1_012 }, { pressureChange3hHpa: 2 });
  const tile = renderPressureTile([old, record, available, latest]);
  assert.match(tile, /class="condition-primary"><strong>\+2\.0/u);
  const nonfinite = { ...available, pressureChange3hHpa: Number.POSITIVE_INFINITY };
  assert.match(renderPressureTile([nonfinite, latest, old]), /class="condition-primary"><strong>—<\/strong>/u);
});

// create one regional current-hour gap and contradictory station readings
function regionalGapState(forecastOverrides = {}) {
  const regional = {
    ...record,
    validAt: "2026-08-22T20:00:00.000Z",
    metrics: { ...record.metrics, pm25MicrogramsPerCubicMeter: null, precipitationRateMmPerHour: null, uvIndex: null },
  };
  const hour = {
    ...forecastRecord,
    validAt: "2026-08-22T20:00:00.000Z",
    productRunAt: "2026-08-22T19:00:00.000Z",
    receivedAt: "2026-08-22T19:05:00.000Z",
    freshness: { ...forecastRecord.freshness, ageSeconds: 2_700, status: "delayed" },
    metrics: { ...forecastRecord.metrics, pm25MicrogramsPerCubicMeter: 9, precipitationRateMmPerHour: 2.54, uvIndex: 3 },
    ...forecastOverrides,
  };
  const local = {
    ...ecowittRecord,
    metrics: { ...ecowittRecord.metrics, pm25MicrogramsPerCubicMeter: 50, precipitationRateMmPerHour: 8, uvIndex: 10 },
  };
  return {
    ...forecastState([hour], null),
    current: [regional, local],
    forecastAdjustmentMode: "raw",
  };
}

// fill regional cards from raw hourly metrics without modifying source records
test("regional current gaps use the matching fresh forecast hour for cards and artwork", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-08-22T20:45:00.000Z") });
  const state = regionalGapState();
  const original = structuredClone(state);
  const html = renderWeatherDashboard(state);
  assert.match(conditionCardHtml(html, "rain"), /class="condition-primary"><strong>0\.1<small>in\/h/u);
  assert.match(conditionCardHtml(html, "air-quality"), /class="condition-primary"><strong>9/u);
  assert.match(conditionCardHtml(html, "uv-index"), /class="condition-primary"><strong>3/u);
  assert.match(conditionCardHtml(html, "temperature"), /class="condition-primary"><strong>60<small>°F/u);
  assert.doesNotMatch(html, /class="alert-list"/u);
  assert.equal(currentWeatherIcon(state).name, "09-heavy-rain");
  assert.match(html, /\/weather-icons\/09-heavy-rain\.svg/u);
  assert.deepEqual(state, original);

  const adjusted = renderWeatherDashboard({ ...state, forecastAdjustmentMode: "adjusted" });
  assert.match(conditionCardHtml(adjusted, "rain"), /class="condition-primary"><strong>0\.31<small>in\/h/u);
  assert.match(conditionCardHtml(adjusted, "air-quality"), /class="condition-primary"><strong>50/u);
  assert.match(conditionCardHtml(adjusted, "uv-index"), /class="condition-primary"><strong>10/u);
});

// retain valid current zeros and use forecast zeros without adjustment metadata
test("regional forecast supplements preserve current values and accept zero", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-08-22T20:15:00.000Z") });
  const state = regionalGapState();
  const hour = state.forecast[0];
  state.forecast = [{
    ...hour,
    metrics: { ...hour.metrics, pm25MicrogramsPerCubicMeter: 0, precipitationRateMmPerHour: 0, uvIndex: 0 },
    rainAdjustment: { ...rainAdjustmentDecision(hour), correctedPrecipitationMm: 8 },
  }];
  const html = renderWeatherDashboard(state);
  assert.match(conditionCardHtml(html, "rain"), /class="condition-primary"><strong>0<small>in\/h/u);
  assert.match(conditionCardHtml(html, "air-quality"), /class="condition-primary"><strong>0/u);
  assert.match(conditionCardHtml(html, "uv-index"), /class="condition-primary"><strong>0/u);
  assert.equal(currentWeatherIcon(state).name, "03-partly-cloudy");
  state.current = [{ ...state.current[0], metrics: { ...state.current[0].metrics, pm25MicrogramsPerCubicMeter: 12, precipitationRateMmPerHour: 0, uvIndex: 4 } }];
  state.forecast = [hour];
  const existing = renderWeatherDashboard(state);
  assert.match(conditionCardHtml(existing, "rain"), /class="condition-primary"><strong>0<small>in\/h/u);
  assert.match(conditionCardHtml(existing, "air-quality"), /class="condition-primary"><strong>12/u);
  assert.match(conditionCardHtml(existing, "uv-index"), /class="condition-primary"><strong>4/u);
});

// reject stale wrong-hour and incompatible products without station fallback
test("regional current supplements fail unavailable outside the exact compatible hour", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-08-22T20:15:00.000Z") });
  const state = regionalGapState();
  const hour = state.forecast[0];
  const rejected = [
    [],
    [{ ...hour, validAt: "2026-08-22T19:00:00.000Z" }],
    [{ ...hour, validAt: "2026-08-22T21:00:00.000Z" }],
    [{ ...hour, productRunAt: "2026-08-22T18:00:00.000Z", receivedAt: "2026-08-22T20:05:00.000Z" }],
    [{ ...hour, productRunAt: "2026-08-22T21:00:00.000Z" }],
    [{ ...hour, productRunAt: null, receivedAt: "2026-08-22T18:00:00.000Z" }],
    [{ ...hour, provenance: { ...hour.provenance, providerKey: "another-model" } }],
    [{ ...hour, provenance: { ...hour.provenance, stationSlug: "other-location" } }],
    [{ ...hour, provenance: { ...hour.provenance, sourceKind: "physical_sensor" } }],
    [{ ...hour, metrics: { ...hour.metrics, pm25MicrogramsPerCubicMeter: null, precipitationRateMmPerHour: -1, uvIndex: Number.NaN } }],
  ];
  // keep each missing card honest even while a physical station has readings
  for (const forecast of rejected) {
    const html = renderWeatherDashboard({ ...state, forecast });
    // verify all three visible primary values rather than daily summaries
    for (const key of ["rain", "air-quality", "uv-index"]) {
      assert.match(conditionCardHtml(html, key), /class="condition-primary"><strong>—<\/strong>/u);
      assert.match(conditionCardHtml(html, key), /Unavailable/u);
    }
    assert.equal(currentWeatherIcon({ ...state, forecast }).name, "12-unavailable");
  }
  const missingCurrent = renderWeatherDashboard({ ...state, current: [state.current[1]] });
  assert.match(missingCurrent, /No current weather value is available yet\./u);
});

// select the newest regional revision and roll over without nearest-hour reuse
test("regional forecast supplements select the newest run and expire at the hour boundary", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-08-22T20:59:59.999Z") });
  const state = regionalGapState();
  const hour = state.forecast[0];
  const newer = {
    ...hour,
    productRunAt: "2026-08-22T20:00:00.000Z",
    receivedAt: "2026-08-22T20:05:00.000Z",
    metrics: { ...hour.metrics, pm25MicrogramsPerCubicMeter: 40, precipitationRateMmPerHour: 8 },
  };
  state.forecast = [hour, newer];
  const html = renderWeatherDashboard(state);
  assert.match(conditionCardHtml(html, "rain"), /class="condition-primary"><strong>0\.31<small>in\/h/u);
  assert.match(conditionCardHtml(html, "air-quality"), /class="condition-primary"><strong>40/u);
  assert.match(html, /Current rain 0\.31 in\/h/u);
  assert.match(html, /PM2\.5 is 40 µg\/m³/u);
  assert.equal(currentWeatherIcon(state).name, "09-heavy-rain");
  context.mock.timers.tick(1);
  const expired = renderWeatherDashboard(state);
  assert.match(conditionCardHtml(expired, "rain"), /class="condition-primary"><strong>—<\/strong>/u);
  assert.equal(currentWeatherIcon(state).name, "12-unavailable");
  state.forecast = [{ ...hour, validAt: "2026-08-22T21:00:00.000Z" }];
  assert.match(conditionCardHtml(renderWeatherDashboard(state), "rain"), /class="condition-primary"><strong>0\.1<small>in\/h/u);
});

// accept receipt time when a provider omits an explicit forecast run timestamp
test("regional forecast supplements use a recent receipt only when the run timestamp is absent", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-08-22T20:45:00.000Z") });
  const state = regionalGapState({ productRunAt: null, receivedAt: "2026-08-22T20:30:00.000Z" });
  assert.match(conditionCardHtml(renderWeatherDashboard(state), "air-quality"), /class="condition-primary"><strong>9/u);
});

// refresh raw regional forecasts without full reloads or overlapping background reads
test("regional current polling refreshes its forecast on cadence and retains safe failures", async (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-08-22T20:15:00.000Z") });
  const state = regionalGapState();
  const requests = [];
  let forecast = state.forecast;
  let pendingForecast = null;
  let failForecast = false;
  let failCurrent = false;
  // control the two independent public weather products
  const fetcher = async (input) => {
    const url = String(input);
    requests.push(url);
    // isolate current failures from successful regional forecast reads
    if (url.includes("/current")) {
      return failCurrent ? Response.json({}, { status: 503 }) : Response.json({ data: state.current, site });
    }
    // delay or fail only selected forecast attempts
    if (/\/forecast(?:\?|$)/u.test(url)) {
      return pendingForecast ?? (failForecast ? Response.json({}, { status: 503 }) : Response.json({ data: forecast, site }));
    }
    // isolate compact recorded history from model refresh accounting
    if (url.endsWith("/forecast-observations")) {
      return Response.json({ data: [], generatedAt: new Date().toISOString() });
    }
    return Response.json({ data: [], site });
  };
  const controller = new WeatherDashboardController({ fetcher, storage: null, view: "forecast" });
  await controller.initialize();
  controller.toggleForecastAdjustmentMode();
  requests.length = 0;
  context.mock.timers.tick(14 * 60_000);
  await controller.refreshCurrentReadings();
  assert.equal(requests.filter((url) => /\/forecast(?:\?|$)/u.test(url)).length, 0);
  assert.equal(requests.filter((url) => url.endsWith("/forecast-observations")).length, 1);
  context.mock.timers.tick(60_000);
  forecast = [{ ...forecast[0], productRunAt: "2026-08-22T20:20:00.000Z", metrics: { ...forecast[0].metrics, pm25MicrogramsPerCubicMeter: 15 } }];
  let resolveForecast;
  pendingForecast = new Promise(
    // retain one controlled background request
    (resolve) => { resolveForecast = resolve; },
  );
  failCurrent = true;
  const first = controller.refreshCurrentReadings();
  const second = controller.refreshCurrentReadings();
  assert.equal(requests.filter((url) => /\/forecast(?:\?|$)/u.test(url)).length, 1);
  resolveForecast(Response.json({ data: forecast, site }));
  await Promise.all([first, second]);
  pendingForecast = null;
  assert.equal(controller.state.loading, false);
  assert.equal(controller.state.forecastDays, 1);
  assert.match(conditionCardHtml(renderWeatherDashboard(controller.state), "air-quality"), /class="condition-primary"><strong>15/u);
  assert.equal(requests.some((url) => url.includes("/tides") || url.includes("/daily-precipitation")), false);
  const retained = controller.state.forecast;
  failCurrent = false;
  failForecast = true;
  context.mock.timers.tick(15 * 60_000);
  await controller.refreshCurrentReadings();
  assert.equal(controller.state.forecast, retained);
  context.mock.timers.tick(2 * 60 * 60_000);
  await controller.refreshCurrentReadings();
  assert.match(conditionCardHtml(renderWeatherDashboard(controller.state), "air-quality"), /class="condition-primary"><strong>—<\/strong>/u);
});

// keep ECMWF temperature explicit, opt-in, and independent from wind metadata
test("temperature canary overrides only adjusted temperature with truthful provenance", () => {
  const raw = {
    ...forecastRecord,
    metadata: {
      ...forecastRecord.metadata,
      provider: { ...forecastRecord.metadata.provider, dataset: "forecast" },
    },
  };
  const parsed = parseForecastRecordsResponse({
    adjustmentRuntime: adjustmentRuntime(),
    data: [{
      ...raw,
      adjustment: activeAdjustment(raw),
      temperatureAdjustment: temperatureCanaryDecision(raw),
    }],
    site,
    temperatureAdjustmentRuntime: temperatureCanaryRuntime(),
  });
  const parsedRecord = parsed.data[0];

  assert.equal(parsed.temperatureAdjustmentRuntime.state, "active");
  assert.equal(parsedRecord.metrics.temperatureC, 16.2);
  assert.equal(forecastMetricValue(parsedRecord, "temperatureC"), 15);
  assert.equal(forecastMetricValue(parsedRecord, "temperatureC", false), 16.2);
  assert.equal(
    forecastMetricValue(parsedRecord, "relativeHumidityPercent"),
    73,
  );
  assert.equal(
    parsedRecord.temperatureAdjustment.sourceForecast.upstreamModel,
    "ecmwf_ifs",
  );
  assert.equal(
    parsedRecord.temperatureAdjustment.sourceForecast.operationalHorizonHours,
    6,
  );

  const state = {
    ...forecastState(parsed.data, parsed.adjustmentRuntime),
    forecastTemperatureAdjustmentRuntime:
      parsed.temperatureAdjustmentRuntime,
  };
  const html = renderWeatherDashboard(state, "forecast");
  assert.match(
    html,
    /Adjusted temperature uses ECMWF IFS single-run data; raw temperature uses Open-Meteo Best Match\./u,
  );

  const malformed = parseForecastRecordsResponse({
    adjustmentRuntime: adjustmentRuntime(),
    data: [{
      ...raw,
      adjustment: activeAdjustment(raw),
      temperatureAdjustment: {
        ...temperatureCanaryDecision(raw),
        sourceForecast: {
          ...temperatureCanaryDecision(raw).sourceForecast,
          upstreamModel: "best_match",
        },
      },
    }],
    site,
    temperatureAdjustmentRuntime: temperatureCanaryRuntime(),
  });
  assert.equal(malformed.temperatureAdjustmentRuntime.state, "disabled");
  assert.equal(malformed.data[0].temperatureAdjustment, undefined);
  assert.equal(
    forecastMetricValue(malformed.data[0], "temperatureC"),
    18.2,
  );
  assert.equal(malformed.data[0].adjustment.state, "active");


});

// retain canary safeguards behind the concise adjustment label
test("wind canary is explicit, wind-only, and follows the selected gust value", () => {
  const raw = {
    ...forecastRecord,
    metadata: {
      ...forecastRecord.metadata,
      provider: { ...forecastRecord.metadata.provider, dataset: "forecast" },
    },
    metrics: {
      ...forecastRecord.metrics,
      windGustMps: 16,
    },
  };
  const parsed = parseForecastRecordsResponse({
    adjustmentRuntime: windCanaryRuntime(),
    data: [{ ...raw, adjustment: windCanaryAdjustment(raw) }],
    site,
  });
  const adjustedHtml = renderWeatherDashboard(
    forecastState(parsed.data, parsed.adjustmentRuntime),
    "home",
  );
  const regionalHtml = renderWeatherDashboard(
    {
      ...forecastState(parsed.data, parsed.adjustmentRuntime),
      forecastAdjustmentMode: "raw",
    },
    "home",
  );

  assert.equal(forecastMetricValue(parsed.data[0], "temperatureC"), raw.metrics.temperatureC);
  assert.equal(forecastMetricValue(parsed.data[0], "relativeHumidityPercent"), raw.metrics.relativeHumidityPercent);
  assert.notEqual(forecastMetricValue(parsed.data[0], "windSpeedMps"), raw.metrics.windSpeedMps);
  assert.match(adjustedHtml, /aria-checked="true"\s+aria-label="Adjusted"/u);
  assert.match(adjustedHtml, /class="forecast-adjustment-sparkle" data-sparkle-tone="gold"/u);
  assert.doesNotMatch(adjustedHtml, /High wind/u);
  assert.match(regionalHtml, /aria-checked="false"\s+aria-label="Adjusted"/u);
  assert.match(regionalHtml, /class="forecast-adjustment-sparkle" data-sparkle-tone="gray"/u);
  assert.match(
    regionalHtml,
    /<article class="local-alert danger"><strong>High wind<\/strong><span>Gusts reaching 35\.8 mph<\/span><\/article>/u,
  );
  assert.doesNotMatch(regionalHtml, /data-forecast-adjustment-status|Canary expires|Wind canary turned off/u);

  const invalid = parseForecastRecordsResponse({
    adjustmentRuntime: windCanaryRuntime(),
    data: [{ ...raw, adjustment: activeAdjustment(raw) }],
    site,
  });
  assert.equal(invalid.adjustmentRuntime.state, "disabled");
  assert.equal(invalid.adjustmentRuntime.reasonCode, "adjustment_error");
  assert.equal(invalid.data[0].adjustment, undefined);
});

// bind forecast watches to the value selected by the shared adjustment switch
test("forecast watches cross thresholds in both directions with selected adjusted values", () => {
  const metricUnits = {
    ...DEFAULT_UNIT_PREFERENCES,
    precipitation: "millimeters",
    temperature: "celsius",
    windSpeed: "meters_per_second",
  };
  const windRaised = {
    ...forecastRecord,
    metrics: { ...forecastRecord.metrics, windGustMps: 14 },
  };
  const frostRaised = {
    ...forecastRecord,
    metrics: { ...forecastRecord.metrics, temperatureC: 1 },
  };
  const rainRaised = {
    ...forecastRecord,
    metrics: {
      ...forecastRecord.metrics,
      precipitationMm: 6,
      precipitationRateMmPerHour: 6,
    },
  };
  const frostLowered = {
    ...forecastRecord,
    metrics: { ...forecastRecord.metrics, temperatureC: -1 },
  };
  const rainLowered = {
    ...forecastRecord,
    metrics: {
      ...forecastRecord.metrics,
      precipitationMm: 7,
      precipitationRateMmPerHour: 7,
    },
  };
  const cases = [
    {
      adjustedDetail: "Gusts reaching 16 m/s",
      label: "High wind",
      rawDetail: null,
      record: {
        ...windRaised,
        adjustment: {
          ...windCanaryAdjustment(windRaised),
          adjustedMetrics: { windGustMps: 16, windSpeedMps: windRaised.metrics.windSpeedMps + 0.4 },
        },
      },
    },
    {
      adjustedDetail: null,
      label: "High wind",
      rawDetail: "Gusts reaching 16 m/s",
      record: {
        ...forecastRecord,
        metrics: { ...forecastRecord.metrics, windGustMps: 16 },
        adjustment: windCanaryAdjustment({
          ...forecastRecord,
          metrics: { ...forecastRecord.metrics, windGustMps: 16 },
        }),
      },
    },
    {
      adjustedDetail: "Forecast low -1°C",
      label: "Frost possible",
      rawDetail: null,
      record: {
        ...frostRaised,
        temperatureAdjustment: { ...temperatureCanaryDecision(frostRaised), correctedTemperatureC: -1 },
      },
    },
    {
      adjustedDetail: null,
      label: "Frost possible",
      rawDetail: "Forecast low -1°C",
      record: {
        ...frostLowered,
        temperatureAdjustment: { ...temperatureCanaryDecision(frostLowered), correctedTemperatureC: 1 },
      },
    },
    {
      adjustedDetail: "Forecast hourly rain 7.2 mm",
      label: "Heavy rain",
      rawDetail: null,
      record: {
        ...rainRaised,
        rainAdjustment: { ...rainAdjustmentDecision(rainRaised), correctedPrecipitationMm: 7.2 },
      },
    },
    {
      adjustedDetail: null,
      label: "Heavy rain",
      rawDetail: "Forecast hourly rain 7 mm",
      record: {
        ...rainLowered,
        rainAdjustment: { ...rainAdjustmentDecision(rainLowered), correctedPrecipitationMm: 6 },
      },
    },
  ];

  // verify each family on both sides of its unchanged threshold
  for (const alertCase of cases) {
    const adjusted = renderAlertState({ forecast: [alertCase.record], units: metricUnits });
    const raw = renderAlertState({ forecast: [alertCase.record], mode: "raw", units: metricUnits });
    assert.equal(adjusted.includes(`<strong>${alertCase.label}</strong>`), alertCase.adjustedDetail !== null);
    assert.equal(raw.includes(`<strong>${alertCase.label}</strong>`), alertCase.rawDetail !== null);
    assert.equal(alertCase.adjustedDetail === null || adjusted.includes(`<span>${alertCase.adjustedDetail}</span>`), true);
    assert.equal(alertCase.rawDetail === null || raw.includes(`<span>${alertCase.rawDetail}</span>`), true);
  }
});

// preserve current alerts while formatting every qualifying reading
test("current heat rain and wind watches retain real readings in metric and US units", () => {
  const hazardousCurrent = {
    ...record,
    metrics: {
      ...record.metrics,
      apparentTemperatureC: 33,
      precipitationRateMmPerHour: 8,
      wetBulbGlobeTemperatureC: 30,
      windGustMps: 16,
    },
  };
  const quietForecast = {
    ...forecastRecord,
    metrics: {
      ...forecastRecord.metrics,
      precipitationMm: 0,
      precipitationRateMmPerHour: 0,
      temperatureC: 12,
      windGustMps: 4,
    },
  };

  // keep current observations independent from forecast display mode
  for (const forecastAdjustmentMode of ["adjusted", "raw"]) {
    const metricHtml = renderAlertState({
      current: [hazardousCurrent],
      forecast: [quietForecast],
      mode: forecastAdjustmentMode,
      settings: { version: 1, temperature: false, wind: false, rain: false },
      units: {
        ...DEFAULT_UNIT_PREFERENCES,
        precipitation: "millimeters",
        temperature: "celsius",
        windSpeed: "meters_per_second",
      },
    });
    const usHtml = renderAlertState({
      current: [hazardousCurrent],
      forecast: [quietForecast],
      mode: forecastAdjustmentMode,
      settings: { version: 1, temperature: false, wind: false, rain: false },
    });
    assert.match(metricHtml, /Heat stress[\s\S]*?Apparent temperature 33°C; Wet-bulb globe temperature 30°C/u);
    assert.match(metricHtml, /High wind[\s\S]*?Gusts reaching 16 m\/s/u);
    assert.match(metricHtml, /Heavy rain[\s\S]*?Current rain 8 mm\/h/u);
    assert.match(usHtml, /Heat stress[\s\S]*?Apparent temperature 91\.4°F; Wet-bulb globe temperature 86°F/u);
    assert.match(usHtml, /High wind[\s\S]*?Gusts reaching 35\.8 mph/u);
    assert.match(usHtml, /Heavy rain[\s\S]*?Current rain 0\.31 in\/h/u);
  }

  const wetForecast = {
    ...quietForecast,
    metrics: {
      ...quietForecast.metrics,
      precipitationMm: 7,
      precipitationRateMmPerHour: 7,
    },
  };
  const combinedRainHtml = renderAlertState({
    current: [hazardousCurrent],
    forecast: [wetForecast],
    units: {
      ...DEFAULT_UNIT_PREFERENCES,
      precipitation: "millimeters",
    },
  });
  assert.match(
    combinedRainHtml,
    /Heavy rain[\s\S]*?Current rain 8 mm\/h; Forecast hourly rain 7 mm/u,
  );
});

// fail individual adjustment families back to their usable raw forecast
test("forecast watches use raw fallback for disabled settings and invalid metadata", () => {
  const windy = {
    ...forecastRecord,
    metrics: { ...forecastRecord.metrics, windGustMps: 16 },
  };
  const disabledWind = parseForecastRecordsResponse({
    adjustmentSettings: { version: 1, temperature: true, wind: false, rain: true },
    adjustmentRuntime: windCanaryRuntime(),
    data: [{ ...windy, adjustment: windCanaryAdjustment(windy) }],
    site,
  });
  const disabledWindHtml = renderAlertState({
    forecast: disabledWind.data,
    runtime: disabledWind.adjustmentRuntime,
    settings: disabledWind.adjustmentSettings,
  });
  assert.equal(disabledWind.data[0].adjustment, undefined);
  assert.match(disabledWindHtml, /High wind[\s\S]*?Gusts reaching 35\.8 mph/u);

  const invalidWind = parseForecastRecordsResponse({
    adjustmentSettings: { version: 1, temperature: true, wind: true, rain: true },
    adjustmentRuntime: windCanaryRuntime(),
    data: [{ ...windy, adjustment: activeAdjustment(windy) }],
    site,
  });
  const invalidWindHtml = renderAlertState({
    forecast: invalidWind.data,
    runtime: invalidWind.adjustmentRuntime,
    settings: invalidWind.adjustmentSettings,
  });
  assert.equal(invalidWind.adjustmentRuntime.state, "disabled");
  assert.equal(invalidWind.data[0].adjustment, undefined);
  assert.match(invalidWindHtml, /High wind[\s\S]*?Gusts reaching 35\.8 mph/u);

  const inactiveWind = parseForecastRecordsResponse({
    adjustmentRuntime: adjustmentRuntime("disabled", "registry_inactive"),
    data: [{ ...windy, adjustment: failRawAdjustment("disabled", "registry_inactive") }],
    site,
  });
  assert.match(renderAlertState({
    forecast: inactiveWind.data,
    runtime: inactiveWind.adjustmentRuntime,
  }), /High wind[\s\S]*?Gusts reaching 35\.8 mph/u);
});

// attribute temperature only when its independent setting is enabled
test("disabled temperature settings hide ECMWF credit despite active runtime", () => {
  const state = {
    ...forecastState([], windCanaryRuntime()),
    forecastTemperatureAdjustmentRuntime: temperatureCanaryRuntime(),
    forecastAdjustmentSettings: { version: 1, temperature: false, wind: true, rain: true },
  };
  const html = renderWeatherDashboard(state, "forecast");
  assert.doesNotMatch(html, /Adjusted temperature uses ECMWF IFS/u);
});

test("qualified temperature maintenance requires action and confirmation roots", () => {
  const raw = {
    ...forecastRecord,
    metadata: {
      ...forecastRecord.metadata,
      provider: { ...forecastRecord.metadata.provider, dataset: "forecast" },
    },
  };
  const runtime = {
    ...temperatureCanaryRuntime(),
    actionSha256: "a".repeat(64),
    activationMode: "maintenance_qualified",
    authorizationSha256: null,
    expiresAt: null,
    fullMemberRootSha256: "b".repeat(64),
    policyReportSha256: "c".repeat(64),
  };
  const parsed = parseForecastRecordsResponse({
    data: [{ ...raw, temperatureAdjustment: temperatureCanaryDecision(raw) }],
    site,
    temperatureAdjustmentRuntime: runtime,
  });
  assert.equal(parsed.temperatureAdjustmentRuntime.state, "active");
  assert.notEqual(forecastMetricValue(parsed.data[0], "temperatureC"), raw.metrics.temperatureC);
  const rejected = parseForecastRecordsResponse({
    data: [{ ...raw, temperatureAdjustment: temperatureCanaryDecision(raw) }],
    site,
    temperatureAdjustmentRuntime: { ...runtime, fullMemberRootSha256: null },
  });
  assert.equal(rejected.temperatureAdjustmentRuntime.state, "disabled");
});

// retain exact finite and permanent deadlines at the browser trust boundary
test("temperature and wind runtimes preserve null or finite expiry and reject malformed expiry", () => {
  const raw = {
    ...forecastRecord,
    metadata: {
      ...forecastRecord.metadata,
      provider: { ...forecastRecord.metadata.provider, dataset: "forecast" },
    },
  };
  const cases = [
    ["temperatureC", "temperatureAdjustmentRuntime", "temperatureAdjustment", temperatureCanaryRuntime(), temperatureCanaryDecision(raw)],
    ["windSpeedMps", "adjustmentRuntime", "adjustment", windCanaryRuntime(), windCanaryAdjustment(raw)],
  ];

  // exercise each independent runtime without changing its model identity
  for (const [metric, runtimeKey, decisionKey, runtime, decision] of cases) {
    // accept only explicit null or a valid finite instant
    for (const expiresAt of [null, runtime.expiresAt]) {
      const parsed = parseForecastRecordsResponse({
        [runtimeKey]: { ...runtime, expiresAt },
        data: [{ ...raw, [decisionKey]: decision }],
        site,
      });
      assert.equal(parsed[runtimeKey].state, "active", `${metric} with ${expiresAt}`);
      assert.equal(parsed[runtimeKey].expiresAt, expiresAt);
      assert.notEqual(forecastMetricValue(parsed.data[0], metric), raw.metrics[metric]);
    }

    // discard decisions behind an invalid runtime envelope
    for (const expiresAt of [undefined, "never", 42, "2026-99-99T00:00:00Z"]) {
      const parsed = parseForecastRecordsResponse({
        [runtimeKey]: { ...runtime, expiresAt },
        data: [{ ...raw, [decisionKey]: decision }],
        site,
      });
      assert.equal(parsed[runtimeKey].state, "disabled", `${metric} with ${expiresAt}`);
      assert.equal(parsed.data[0][decisionKey], undefined);
      assert.equal(forecastMetricValue(parsed.data[0], metric), raw.metrics[metric]);
    }
  }
});

// retain raw fallback without a status panel
test("inactive and invalid adjustment metadata remain usable raw", () => {
  const raw = {
    ...forecastRecord,
    metadata: {
      ...forecastRecord.metadata,
      provider: { ...forecastRecord.metadata.provider, dataset: "forecast" },
    },
  };
  const inactive = parseForecastRecordsResponse({
    adjustmentRuntime: adjustmentRuntime("disabled", "registry_inactive"),
    data: [{ ...raw, adjustment: failRawAdjustment("disabled", "registry_inactive") }],
    site,
  });
  const inactiveHtml = renderWeatherDashboard(
    forecastState(inactive.data, inactive.adjustmentRuntime),
    "forecast",
  );
  assert.match(
    inactiveHtml,
    /class="forecast-adjustment-toggle"[\s\S]*aria-checked="true"[\s\S]*data-forecast-adjustment-available="false"[\s\S]*data-forecast-adjustment-fallback="true"/u,
  );
  assert.doesNotMatch(inactiveHtml, /\sdisabled(?:\s|>)/u);
  assert.match(inactiveHtml, /aria-label="Adjusted"/u);
  assert.match(inactiveHtml, /class="forecast-adjustment-sparkle" data-sparkle-tone="gold"/u);
  assert.doesNotMatch(inactiveHtml, /data-forecast-adjustment-status|Regional fallback/u);

  const invalid = parseForecastRecordsResponse({
    adjustmentRuntime: adjustmentRuntime(),
    data: [{
      ...raw,
      adjustment: {
        ...activeAdjustment(raw),
        candidateArtifactSha256: "f".repeat(64),
      },
    }],
    site,
  });
  assert.equal(invalid.adjustmentRuntime.state, "disabled");
  assert.equal(invalid.adjustmentRuntime.reasonCode, "adjustment_error");
  assert.equal(invalid.data[0].adjustment, undefined);
  assert.equal(forecastMetricValue(invalid.data[0], "temperatureC"), raw.metrics.temperatureC);
  const invalidHtml = renderWeatherDashboard(
    forecastState(invalid.data, invalid.adjustmentRuntime),
    "forecast",
  );
  assert.match(invalidHtml, /aria-label="Adjusted"/u);
  assert.match(invalidHtml, /class="forecast-adjustment-sparkle" data-sparkle-tone="gold"/u);
  assert.match(invalidHtml, /data-forecast-charts/u);
  assert.doesNotMatch(invalidHtml, /data-forecast-adjustment-status|Local adjustment unavailable/u);

  const missing = parseForecastRecordsResponse({ data: [raw], site });
  assert.equal(missing.adjustmentRuntime.reasonCode, "adjustment_error");
  assert.equal(forecastMetricValue(missing.data[0], "temperatureC"), raw.metrics.temperatureC);
});

// keep each adjusted metric aligned with its own persisted switch
test("temperature wind and rain settings gate their own adjusted values", () => {
  const raw = {
    ...forecastRecord,
    metadata: {
      ...forecastRecord.metadata,
      provider: { ...forecastRecord.metadata.provider, dataset: "forecast" },
    },
  };

  // exercise every independent three-bit setting
  for (let bits = 0; bits < 8; bits += 1) {
    const settings = {
      version: 1,
      temperature: Boolean(bits & 1),
      wind: Boolean(bits & 2),
      rain: Boolean(bits & 4),
    };
    const parsed = parseForecastRecordsResponse({
      adjustmentSettings: settings,
      adjustmentRuntime: windCanaryRuntime(),
      data: [{
        ...raw,
        adjustment: windCanaryAdjustment(raw),
        temperatureAdjustment: temperatureCanaryDecision(raw),
        rainAdjustment: rainAdjustmentDecision(raw),
      }],
      rainAdjustmentRuntime: rainAdjustmentRuntime(),
      site,
      temperatureAdjustmentRuntime: temperatureCanaryRuntime(),
    });
    const row = parsed.data[0];
    const state = {
      ...forecastState(parsed.data, parsed.adjustmentRuntime),
      forecastAdjustmentSettings: parsed.adjustmentSettings,
      forecastRainAdjustmentRuntime: parsed.rainAdjustmentRuntime,
      forecastTemperatureAdjustmentRuntime: parsed.temperatureAdjustmentRuntime,
      units: { ...DEFAULT_UNIT_PREFERENCES, precipitation: "millimeters" },
    };
    const html = renderWeatherDashboard(state, "forecast");
    assert.deepEqual(parsed.adjustmentSettings, settings);
    assert.equal(forecastMetricValue(row, "temperatureC"), settings.temperature ? 15 : raw.metrics.temperatureC);
    assert.equal(forecastMetricValue(row, "windSpeedMps"), settings.wind ? raw.metrics.windSpeedMps + 0.4 : raw.metrics.windSpeedMps);
    assert.equal(forecastMetricValue(row, "precipitationMm"), settings.rain ? 2.5 : raw.metrics.precipitationMm);
    assert.equal(forecastMetricValue(row, "precipitationRateMmPerHour"), settings.rain ? 2.5 : raw.metrics.precipitationRateMmPerHour);
    assert.equal(forecastMetricValue(row, "precipitationMm", false), raw.metrics.precipitationMm);
    assert.equal(forecastMetricValue(row, "precipitationRateMmPerHour", false), raw.metrics.precipitationRateMmPerHour);
    assert.equal(html.includes("data-forecast-adjustment-toggle"), true);
    assert.equal(html.includes("Adjusted temperature uses ECMWF"), settings.temperature);
    assert.doesNotMatch(html, /experimental/i);
    assert.equal(html.includes("2.5 mm"), settings.rain);
  }
});

// mark only active displayed adjustment families in adjusted mode
test("temperature wind and rain icons identify selected active forecast adjustments", () => {
  const raw = {
    ...forecastRecord,
    metadata: {
      ...forecastRecord.metadata,
      provider: { ...forecastRecord.metadata.provider, dataset: "forecast" },
    },
    metrics: {
      ...forecastRecord.metrics,
      precipitationRateMmPerHour: forecastRecord.metrics.precipitationMm,
    },
  };
  const equalWind = {
    ...windCanaryAdjustment(raw),
    adjustedMetrics: {
      windGustMps: raw.metrics.windGustMps,
      windSpeedMps: raw.metrics.windSpeedMps,
    },
  };
  const input = {
    adjustmentRuntime: windCanaryRuntime(),
    data: [{
      ...raw,
      adjustment: equalWind,
      rainAdjustment: {
        ...rainAdjustmentDecision(raw),
        correctedPrecipitationMm: raw.metrics.precipitationMm,
      },
      temperatureAdjustment: {
        ...temperatureCanaryDecision(raw),
        correctedTemperatureC: raw.metrics.temperatureC,
      },
    }],
    rainAdjustmentRuntime: rainAdjustmentRuntime(),
    site,
    temperatureAdjustmentRuntime: temperatureCanaryRuntime(),
  };
  // parse one independently configured family combination
  const parseFamilies = (settings) => parseForecastRecordsResponse({
    ...input,
    adjustmentSettings: settings,
  });
  // render both forecast-bearing routes from one parsed response
  const renderViews = (parsed, mode = "adjusted", forecast = parsed.data) => {
    const state = {
      ...forecastState(forecast, parsed.adjustmentRuntime),
      forecastAdjustmentMode: mode,
      forecastAdjustmentSettings: parsed.adjustmentSettings,
      forecastRainAdjustmentRuntime: parsed.rainAdjustmentRuntime,
      forecastTemperatureAdjustmentRuntime: parsed.temperatureAdjustmentRuntime,
    };
    return {
      forecast: renderWeatherDashboard(state, "forecast"),
      home: renderWeatherDashboard(state, "home"),
    };
  };
  const families = ["temperature", "wind", "rain-rate"];
  const enabled = parseFamilies({ version: 1, temperature: true, wind: true, rain: true });
  const adjusted = renderViews(enabled);
  const regional = renderViews(enabled, "raw");

  // identify every active family without changing its equal-value output
  for (const view of ["home", "forecast"]) {
    for (const family of families) {
      assert.equal(familyIconIsAdjusted(adjusted[view], view, family), true, `${view} ${family}`);
      assert.equal(familyIconIsAdjusted(regional[view], view, family), false, `${view} raw ${family}`);
    }
  }
  assert.equal((adjusted.home.match(/forecast-adjusted-icon/gu) ?? []).length, 7);
  assert.equal((adjusted.forecast.match(/forecast-adjusted-icon/gu) ?? []).length, 7);

  // keep readings and serialized chart values unchanged for zero corrections
  for (const family of families) {
    assert.equal(
      conditionCardHtml(adjusted.home, family === "rain-rate" ? "rain" : family)
        .replace('material-symbols-rounded forecast-adjusted-icon', 'material-symbols-rounded'),
      conditionCardHtml(regional.home, family === "rain-rate" ? "rain" : family),
    );
    assert.deepEqual(
      forecastChartSeries(adjusted.forecast, family),
      forecastChartSeries(regional.forecast, family),
    );
  }

  // keep each persisted family setting independent
  for (const disabled of families) {
    const parsed = parseFamilies({
      version: 1,
      temperature: disabled !== "temperature",
      wind: disabled !== "wind",
      rain: disabled !== "rain-rate",
    });
    const views = renderViews(parsed);

    // check both presentations against the disabled family
    for (const view of ["home", "forecast"]) {
      for (const family of families) {
        assert.equal(
          familyIconIsAdjusted(views[view], view, family),
          family !== disabled,
          `${view} ${disabled} disables ${family}`,
        );
      }
    }
  }

  const missing = parseForecastRecordsResponse({ data: [raw], site });
  const missingViews = renderViews(missing);
  const invalid = parseForecastRecordsResponse({
    ...input,
    adjustmentSettings: { version: 1, temperature: true, wind: true, rain: true },
    data: [{
      ...input.data[0],
      adjustment: { ...equalWind, candidateArtifactSha256: "f".repeat(64) },
    }],
  });
  const invalidViews = renderViews(invalid);
  const unsupported = parseForecastRecordsResponse({
    adjustmentRuntime: windCanaryRuntime(),
    data: [{ ...raw, adjustment: failRawAdjustment("not_applicable", "unsupported_lead") }],
    site,
  });
  const unsupportedViews = renderViews(unsupported);

  // reject missing invalid and unsupported family metadata
  for (const view of ["home", "forecast"]) {
    for (const family of families) {
      assert.equal(familyIconIsAdjusted(missingViews[view], view, family), false);
      assert.equal(familyIconIsAdjusted(unsupportedViews[view], view, family), false);
      assert.equal(
        familyIconIsAdjusted(invalidViews[view], view, family),
        family !== "wind",
      );
    }
  }

  const rawDisplayed = { ...raw, validAt: "2026-08-22T05:30:00.000Z" };
  const mixedViews = renderViews(enabled, "adjusted", [rawDisplayed, enabled.data[0]]);
  const outside = {
    ...enabled.data[0],
    validAt: "2026-08-23T08:00:00.000Z",
  };
  const outsideViews = renderViews(missing, "adjusted", [rawDisplayed, outside]);

  // inspect only members included in the displayed day
  for (const view of ["home", "forecast"]) {
    for (const family of families) {
      assert.equal(familyIconIsAdjusted(mixedViews[view], view, family), true);
      assert.equal(familyIconIsAdjusted(outsideViews[view], view, family), false);
    }
  }

  const loadingState = { ...forecastState([], null), loading: true };
  assert.doesNotMatch(renderWeatherDashboard(loadingState, "home"), /forecast-adjusted-icon/u);
  assert.doesNotMatch(renderWeatherDashboard(loadingState, "forecast"), /forecast-adjusted-icon/u);
});

// identify the selected mode without requiring an active model for these icons
test("humidity air quality pressure and UV icons follow the global adjustment mode", () => {
  const state = forecastState([forecastRecord], null);
  const modes = ["adjusted", "raw"];
  const conditions = ["humidity", "air-quality", "pressure", "uv-index"];
  const rendered = {};

  // render both selected modes without model correction evidence
  for (const mode of modes) {
    rendered[mode] = {
      home: renderWeatherDashboard({ ...state, forecastAdjustmentMode: mode }, "home"),
      forecast: renderWeatherDashboard({ ...state, forecastAdjustmentMode: mode }, "forecast"),
    };

    // apply the global mode consistently on both routes
    for (const view of ["home", "forecast"]) {
      // inspect every newly mode-colored icon
      for (const condition of conditions) {
        assert.equal(familyIconIsAdjusted(rendered[mode][view], view, condition), mode === "adjusted");
      }
      assert.equal(familyIconIsAdjusted(rendered[mode][view], view, "tide"), false);
    }
    assert.equal(familyIconIsAdjusted(rendered[mode].home, "home", "sunset"), false);
  }

  // preserve readings and chart series when adding only the selected-mode marker
  for (const condition of conditions) {
    assert.equal(
      conditionCardHtml(rendered.adjusted.home, condition)
        .replace('material-symbols-rounded forecast-adjusted-icon', 'material-symbols-rounded'),
      conditionCardHtml(rendered.raw.home, condition),
    );
    assert.deepEqual(
      forecastChartSeries(rendered.adjusted.forecast, condition),
      forecastChartSeries(rendered.raw.forecast, condition),
    );
  }

  const noForecast = renderWeatherDashboard({ ...state, forecast: [] }, "home");
  // retain the selected-mode marker even without forecast adjustment evidence
  for (const condition of conditions) {
    assert.equal(familyIconIsAdjusted(noForecast, "home", condition), true);
  }
});

test("malformed settings and rain evidence fall back to unchanged raw values", () => {
  const active = {
    adjustmentSettings: { version: 1, temperature: true, wind: true, rain: true },
    adjustmentRuntime: adjustmentRuntime("disabled", "registry_inactive"),
    data: [{
      ...forecastRecord,
      adjustment: failRawAdjustment("disabled", "registry_inactive"),
      rainAdjustment: rainAdjustmentDecision(forecastRecord),
    }],
    rainAdjustmentRuntime: rainAdjustmentRuntime(),
    site,
  };
  const parsed = parseForecastRecordsResponse(active);
  assert.equal(parsed.rainAdjustmentRuntime.state, "active");
  assert.equal(forecastMetricValue(parsed.data[0], "precipitationMm"), 2.5);

  const badSource = parseForecastRecordsResponse({
    ...active,
    data: [{
      ...active.data[0],
      rainAdjustment: {
        ...rainAdjustmentDecision(forecastRecord),
        sourceForecast: {
          ...rainAdjustmentDecision(forecastRecord).sourceForecast,
          validAt: "2026-08-22T07:00:00.000Z",
        },
      },
    }],
  });
  assert.equal(badSource.rainAdjustmentRuntime.state, "disabled");
  assert.equal(forecastMetricValue(badSource.data[0], "precipitationMm"), forecastRecord.metrics.precipitationMm);
  assert.equal(forecastMetricValue(badSource.data[0], "precipitationRateMmPerHour"), forecastRecord.metrics.precipitationRateMmPerHour);

  const malformedSettings = parseForecastRecordsResponse({
    ...active,
    adjustmentSettings: { version: 1, temperature: true, wind: true },
  });
  assert.deepEqual(malformedSettings.adjustmentSettings, {
    version: 1,
    temperature: false,
    wind: false,
    rain: false,
  });
  assert.equal(forecastMetricValue(malformedSettings.data[0], "precipitationMm"), forecastRecord.metrics.precipitationMm);
  assert.match(renderWeatherDashboard({
    ...forecastState(malformedSettings.data, malformedSettings.adjustmentRuntime),
    forecastAdjustmentSettings: malformedSettings.adjustmentSettings,
  }, "forecast"), /data-forecast-adjustment-toggle/u);
});

test("forecast adjustment boundary rejects malformed raw records", () => {
  assert.throws(
    () => parseForecastRecordsResponse({ data: [{}], site }),
    { message: "Forecast response contains an invalid raw record" },
  );
  assert.throws(
    () => parseForecastRecordsResponse({
      data: [{
        ...forecastRecord,
        metrics: { ...forecastRecord.metrics, temperatureC: Number.NaN },
      }],
      site,
    }),
    { message: "Forecast response contains an invalid raw record" },
  );
});

// keep the model horizon independent of infobox presentation
test("extended forecast retains raw values after 168 hours without an infobox", () => {
  const raw = {
    ...forecastRecord,
    metadata: {
      ...forecastRecord.metadata,
      provider: { ...forecastRecord.metadata.provider, dataset: "forecast" },
    },
  };
  const extendedRaw = {
    ...raw,
    id: "extended-169",
    adjustment: failRawAdjustment("not_applicable", "unsupported_lead"),
    validAt: new Date(Date.parse(raw.validAt) + 169 * 3_600_000).toISOString(),
  };
  const parsed = parseForecastRecordsResponse({
    adjustmentRuntime: adjustmentRuntime(),
    data: [{ ...raw, adjustment: activeAdjustment(raw) }, extendedRaw],
    site,
  });
  const html = renderWeatherDashboard(
    forecastState(parsed.data, parsed.adjustmentRuntime, 10),
    "forecast",
  );

  assert.equal(forecastMetricValue(parsed.data[0], "temperatureC"), raw.metrics.temperatureC + 2);
  assert.equal(forecastMetricValue(parsed.data[1], "temperatureC"), raw.metrics.temperatureC);
  assert.match(html, /data-forecast-days="10"/u);
  assert.doesNotMatch(html, /data-forecast-adjustment-status|No local adjustment beyond 168 hours/u);

  const invalidActive = parseForecastRecordsResponse({
    adjustmentRuntime: adjustmentRuntime(),
    data: [{
      ...extendedRaw,
      adjustment: activeAdjustment(extendedRaw, 169),
    }],
    site,
  });
  assert.equal(invalidActive.adjustmentRuntime.reasonCode, "adjustment_error");
  assert.equal(invalidActive.data[0].adjustment, undefined);
  assert.equal(forecastMetricValue(invalidActive.data[0], "temperatureC"), raw.metrics.temperatureC);
});

// compare solar events against independent seasonal reference instants
// https://gml.noaa.gov/grad/solcalc/table.php?lat=47.95043&lon=-122.42797&year=2026
// https://github.com/mourner/suncalc/blob/0ed9f4981b3e2f7a6bde7b340eb965488128c3c1/index.js
test("solar events match the farm's summer, winter, and autumn reference times", () => {
  const references = [
    { day: "2026-09-12", goldenHourStart: "2026-09-13T01:46:59Z", sunset: "2026-09-13T02:27:49Z" },
    { day: "2026-01-15", goldenHourStart: "2026-01-15T23:54:04Z", sunset: "2026-01-16T00:44:36Z" },
    { day: "2026-06-15", goldenHourStart: "2026-06-16T03:22:16Z", sunset: "2026-06-16T04:10:58Z" },
  ];

  // retain independent expected instants rather than duplicating the algorithm
  for (const reference of references) {
    const actual = eveningSunTimes(site, new Date(`${reference.day}T20:00:00Z`));
    assert.ok(Math.abs(actual.sunset.getTime() - Date.parse(reference.sunset)) < 15_000);
    assert.ok(Math.abs(actual.goldenHourStart.getTime() - Date.parse(reference.goldenHourStart)) < 15_000);
    assert.ok(actual.sunrise < actual.sunset);
    assert.ok(actual.goldenHourStart < actual.sunset);
    assert.notEqual(actual.sunset.getTime() - actual.goldenHourStart.getTime(), 3_600_000);
  }

  // retain an independent NOAA minute reference for both daylight boundaries
  const september = eveningSunTimes(site, new Date("2026-09-17T20:00:00Z"));
  assert.ok(Math.abs(september.sunrise.getTime() - Date.parse("2026-09-17T13:50:00Z")) < 30_000);
  assert.ok(Math.abs(september.sunset.getTime() - Date.parse("2026-09-18T02:17:00Z")) < 30_000);
});

// keep the selected day independent of UTC midnight and the last weather sample
test("sunset remains today's event after dusk and changes at farm midnight", () => {
  const morning = eveningSunTimes(site, new Date("2026-09-12T15:00:00Z"));
  const afterSunset = eveningSunTimes(site, new Date("2026-09-13T06:59:59Z"));
  const nextDay = eveningSunTimes(site, new Date("2026-09-13T07:00:00Z"));
  assert.deepEqual(afterSunset, morning);
  assert.equal(toSiteWallClock(nextDay.sunset.toISOString(), site.timezone).slice(0, 10), "2026-09-13");
  assert.notEqual(nextDay.sunset.getTime(), morning.sunset.getTime());
});

// preserve the local calendar through both daylight-saving transitions
test("solar times use the site day across spring and autumn clock changes", () => {
  // cover the skipped and repeated farm hours
  for (const [before, after, expectedDay] of [
    ["2026-03-08T09:59:59Z", "2026-03-08T10:00:00Z", "2026-03-08"],
    ["2026-11-01T08:59:59Z", "2026-11-01T09:00:00Z", "2026-11-01"],
  ]) {
    const times = eveningSunTimes(site, new Date(before));
    assert.deepEqual(eveningSunTimes(site, new Date(after)), times);
    assert.equal(toSiteWallClock(times.sunrise.toISOString(), site.timezone).slice(0, 10), expectedDay);
    assert.equal(toSiteWallClock(times.sunset.toISOString(), site.timezone).slice(0, 10), expectedDay);
  }
});

// align the civil date where a timezone crosses the longitude date boundary
test("solar events keep the requested local day across the international date line", () => {
  const island = { latitude: 1.8721, longitude: -157.4278, timezone: "Pacific/Kiritimati" };
  const times = eveningSunTimes(island, new Date("2026-09-11T22:00:00Z"));
  assert.equal(toSiteWallClock(times.sunrise.toISOString(), island.timezone).slice(0, 10), "2026-09-12");
  assert.equal(toSiteWallClock(times.sunset.toISOString(), island.timezone).slice(0, 10), "2026-09-12");
  assert.equal(toSiteWallClock(times.goldenHourStart.toISOString(), island.timezone).slice(0, 10), "2026-09-12");
});

// preserve absent crossings during polar day and night
test("solar events return no time when the sun never crosses the requested altitude", () => {
  const polarSite = { latitude: 89, longitude: 0, timezone: "UTC" };

  // cover both polar seasons independently
  for (const day of ["2026-06-21", "2026-12-21"]) {
    assert.deepEqual(eveningSunTimes(polarSite, new Date(`${day}T12:00:00Z`)), {
      goldenHourStart: null,
      sunrise: null,
      sunset: null,
      sunsetChangeMinutes: null,
    });
  }
});

// compare displayed farm-local minutes across seasons and calendar boundaries
test("sunset change compares yesterday's rounded local clock time", () => {
  // include unchanged sunsets and both daylight-saving clock changes
  for (const [day, expectedMinutes] of [
    ["2026-09-12", -2],
    ["2026-01-15", 2],
    ["2026-06-21", 0],
    ["2026-01-01", 1],
    ["2028-03-01", 1],
    ["2026-03-08", 61],
    ["2026-11-01", -62],
  ]) {
    const times = eveningSunTimes(site, new Date(`${day}T20:00:00Z`));
    assert.equal(times.sunsetChangeMinutes, expectedMinutes, day);
  }
});

// avoid inventing a comparison when the previous polar day has no sunset
test("sunset change stays unavailable for the first sunset after polar day", () => {
  const polarSite = { latitude: 69.6492, longitude: 18.9553, timezone: "Europe/Oslo" };
  const previous = eveningSunTimes(polarSite, new Date("2026-07-25T12:00:00Z"));
  const today = eveningSunTimes(polarSite, new Date("2026-07-26T12:00:00Z"));
  assert.equal(previous.sunset, null);
  assert.ok(today.sunset instanceof Date);
  assert.equal(today.sunsetChangeMinutes, null);
});

// preserve the earliest continuous minimum through ordering, gaps and missing values
test("clearest cloud range selects the earliest continuous minimum without mutation", () => {
  assert.deepEqual(
    clearestCloudRange([
      cloudForecastRecord("2026-09-12T20:00:00Z", 12),
      cloudForecastRecord("2026-09-12T21:00:00Z", 12),
      cloudForecastRecord("2026-09-12T22:00:00Z", 30),
    ], site.timezone),
    { unit: "PM", value: "1–3" },
  );
  assert.deepEqual(
    clearestCloudRange([
      cloudForecastRecord("2026-09-12T18:00:00Z", 12),
      cloudForecastRecord("2026-09-12T19:00:00Z", 12),
    ], site.timezone),
    { unit: "PM", value: "11 AM–1" },
  );
  assert.deepEqual(
    clearestCloudRange([
      cloudForecastRecord("2026-09-12T20:00:00Z", 12),
      cloudForecastRecord("2026-09-12T21:00:00Z", 30),
      cloudForecastRecord("2026-09-12T22:00:00Z", 12),
    ], site.timezone),
    { unit: "PM", value: "1–2" },
  );

  const unsorted = [
    cloudForecastRecord("2026-09-12T21:00:00Z", 12, "later"),
    cloudForecastRecord("2026-09-12T20:00:00Z", 12, "earlier"),
  ];
  assert.deepEqual(clearestCloudRange(unsorted, site.timezone), { unit: "PM", value: "1–3" });
  assert.deepEqual(
    unsorted.map(
      // confirm sorting leaves the caller's order untouched
      (entry) => entry.id,
    ),
    ["later", "earlier"],
  );
  assert.deepEqual(
    clearestCloudRange([
      cloudForecastRecord("2026-09-12T20:00:00Z", 12),
      cloudForecastRecord("2026-09-12T22:00:00Z", 12),
    ], site.timezone),
    { unit: "PM", value: "1–2" },
  );
  assert.deepEqual(
    clearestCloudRange([
      cloudForecastRecord("2026-09-12T20:00:00Z", 12),
      cloudForecastRecord("2026-09-12T21:00:00Z", null),
      cloudForecastRecord("2026-09-12T22:00:00Z", 12),
    ], site.timezone),
    { unit: "PM", value: "1–2" },
  );
});

// cap cloud ranges at the site-day boundary and preserve absent forecasts
test("clearest cloud range formats midnight and all-day windows", () => {
  assert.deepEqual(
    clearestCloudRange([cloudForecastRecord("2026-09-13T06:00:00Z", 12)], site.timezone),
    { unit: "", value: "11 PM–midnight" },
  );
  assert.deepEqual(
    clearestCloudRange([cloudForecastRecord("2026-09-13T06:30:00Z", 12)], site.timezone),
    { unit: "", value: "midnight" },
  );
  const allDay = Array.from(
    { length: 24 },
    // fill every hourly bin in the farm day
    (_, hour) => cloudForecastRecord(new Date(Date.parse("2026-09-12T07:00:00Z") + hour * 3_600_000).toISOString(), 12),
  );
  assert.deepEqual(clearestCloudRange(allDay, site.timezone), { unit: "", value: "12 AM–midnight" });
  assert.deepEqual(clearestCloudRange([], site.timezone), { unit: "", value: "—" });
  assert.deepEqual(
    clearestCloudRange([cloudForecastRecord("2026-09-12T20:00:00Z", null)], site.timezone),
    { unit: "", value: "—" },
  );
});

// round displayed endpoints without losing daylight-saving distinctions
test("clearest cloud range rounds to the nearest hour across daylight-saving transitions", () => {
  assert.deepEqual(
    clearestCloudRange([cloudForecastRecord("2026-09-12T20:29:59.999Z", 12)], site.timezone),
    { unit: "PM", value: "1–2" },
  );
  assert.deepEqual(
    clearestCloudRange([cloudForecastRecord("2026-09-12T20:30:00Z", 12)], site.timezone),
    { unit: "PM", value: "2–3" },
  );
  assert.deepEqual(
    clearestCloudRange([
      cloudForecastRecord("2026-03-08T09:00:00Z", 12),
      cloudForecastRecord("2026-03-08T10:00:00Z", 12),
    ], site.timezone),
    { unit: "AM", value: "1–4" },
  );
  assert.deepEqual(
    clearestCloudRange([cloudForecastRecord("2026-11-01T08:00:00Z", 12)], site.timezone),
    { unit: "", value: "1 AM PDT–1 AM PST" },
  );
  assert.deepEqual(
    clearestCloudRange([cloudForecastRecord("2026-09-12T20:40:00Z", 12)], "Asia/Kolkata"),
    { unit: "AM", value: "2–3" },
  );
  assert.deepEqual(
    clearestCloudRange([cloudForecastRecord("2026-09-12T20:40:00Z", 12)], "Pacific/Chatham"),
    { unit: "AM", value: "9–10" },
  );
  assert.deepEqual(
    clearestCloudRange([cloudForecastRecord("2026-10-03T15:20:00Z", 12)], "Australia/Lord_Howe"),
    { unit: "AM", value: "3" },
  );
  assert.deepEqual(
    clearestCloudRange([cloudForecastRecord("2026-04-04T14:45:00Z", 12)], "Australia/Lord_Howe"),
    { unit: "AM", value: "2" },
  );
});

// select exact daylight overlap before rounding its displayed endpoints
test("clearest cloud range rounds clipped sunrise and sunset hours", () => {
  const daylight = {
    sunrise: new Date("2026-09-12T14:17:00Z"),
    sunset: new Date("2026-09-13T02:17:00Z"),
  };
  assert.deepEqual(
    clearestCloudRange([
      cloudForecastRecord("2026-09-12T13:00:00Z", 5),
      cloudForecastRecord("2026-09-12T14:00:00Z", 20),
      cloudForecastRecord("2026-09-12T15:00:00Z", 20),
    ], site.timezone, daylight),
    { unit: "AM", value: "7–9" },
  );
  assert.deepEqual(
    clearestCloudRange([
      cloudForecastRecord("2026-09-13T00:00:00Z", 20),
      cloudForecastRecord("2026-09-13T01:00:00Z", 20),
      cloudForecastRecord("2026-09-13T02:00:00Z", 20),
      cloudForecastRecord("2026-09-13T03:00:00Z", 5),
    ], site.timezone, daylight),
    { unit: "PM", value: "5–7" },
  );
  assert.deepEqual(
    clearestCloudRange([
      cloudForecastRecord("2026-09-12T14:00:00Z", 20),
      cloudForecastRecord("2026-09-12T15:00:00Z", 20),
    ], site.timezone, { ...daylight, sunrise: new Date("2026-09-12T14:40:00Z") }),
    { unit: "AM", value: "8–9" },
  );
  assert.deepEqual(
    clearestCloudRange([
      cloudForecastRecord("2026-09-13T01:00:00Z", 20),
      cloudForecastRecord("2026-09-13T02:00:00Z", 20),
    ], site.timezone, { ...daylight, sunset: new Date("2026-09-13T02:40:00Z") }),
    { unit: "PM", value: "6–8" },
  );
});

// avoid repeated labels when a short daylight window rounds to one hour
test("clearest cloud range collapses identical rounded endpoints", () => {
  assert.deepEqual(
    clearestCloudRange([cloudForecastRecord("2026-09-12T14:00:00Z", 12)], site.timezone, {
      sunrise: new Date("2026-09-12T14:40:00Z"),
      sunset: new Date("2026-09-12T14:55:00Z"),
    }),
    { unit: "AM", value: "8" },
  );
});

// require both astronomical boundaries before publishing a daylight range
test("clearest cloud range stays unavailable without sunrise or sunset", () => {
  const forecast = [cloudForecastRecord("2026-09-12T20:00:00Z", 12)];

  // verify each absent boundary independently
  for (const daylight of [
    { sunrise: null, sunset: new Date("2026-09-13T02:17:00Z") },
    { sunrise: new Date("2026-09-12T14:17:00Z"), sunset: null },
  ]) {
    assert.deepEqual(clearestCloudRange(forecast, site.timezone, daylight), { unit: "", value: "—" });
  }
});

// retain full-day extrema while enforcing astronomical daylight over modeled light
test("clouds tile excludes nighttime extrema from the clearest daylight range", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-12T20:00:00Z") });
  const tile = renderCloudTile([
    classifiedCloudForecastRecord("2026-09-12T08:00:00Z", 5, 100, 2),
    classifiedCloudForecastRecord("2026-09-12T15:00:00Z", 20, 100, 2),
    classifiedCloudForecastRecord("2026-09-12T16:00:00Z", 20, 100, 2),
    classifiedCloudForecastRecord("2026-09-12T17:00:00Z", 30, 100, 2),
    classifiedCloudForecastRecord("2026-09-12T20:00:00Z", 20, 100, 2),
    classifiedCloudForecastRecord("2026-09-12T21:00:00Z", 20, 100, 2),
    classifiedCloudForecastRecord("2026-09-13T06:00:00Z", 95, 100, 2),
  ]);
  assert.match(tile, /Clearest<\/span>\s*<strong>8–10<small>AM<\/small><\/strong>/u);
  assert.match(tile, /class="condition-secondary condition-secondary-paired"/u);
  assert.match(tile, /class="condition-secondary-comparison">\s*<span>Overnight<\/span>\s*<strong>1–2<small>AM<\/small><\/strong>/u);
  assert.match(tile, /class="condition-primary"><strong>42<small>%<\/small><\/strong>/u);
  assert.match(tile, /condition-forecast-label">Max<\/span> <strong>95<small>%<\/small><\/strong>[\s\S]*?condition-forecast-label">Min<\/span> <strong>5<small>%<\/small><\/strong>/u);
});

// preserve nighttime extrema when no daylight forecast is usable
test("clouds tile leaves the clearest daylight range unavailable for night-only forecasts", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-12T20:00:00Z") });
  const tile = renderCloudTile([
    classifiedCloudForecastRecord("2026-09-12T08:00:00Z", 5, 0, 0),
    classifiedCloudForecastRecord("2026-09-13T06:00:00Z", 95, 0, 0),
  ]);
  assert.match(tile, /Clearest<\/span>\s*<strong>—<\/strong>/u);
  assert.match(tile, /class="condition-secondary-comparison">\s*<span>Overnight<\/span>\s*<strong>1–2<small>AM<\/small><\/strong>/u);
  assert.match(tile, /condition-forecast-label">Max<\/span> <strong>95<small>%<\/small><\/strong>[\s\S]*?condition-forecast-label">Min<\/span> <strong>5<small>%<\/small><\/strong>/u);
  assert.equal((tile.match(/<strong>/gu) ?? []).length, 5);
});

// keep astronomically sunlit hours regardless of missing model-light signals
test("clouds tile does not hide sunlit hours with zero or missing modeled light", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-12T20:00:00Z") });
  const tile = renderCloudTile([
    classifiedCloudForecastRecord("2026-09-12T08:00:00Z", 5, 100, 2),
    classifiedCloudForecastRecord("2026-09-12T20:00:00Z", 20, 0, 0),
    classifiedCloudForecastRecord("2026-09-12T21:00:00Z", 20, null, null),
  ]);
  assert.match(tile, /Clearest<\/span>\s*<strong>1–3<small>PM<\/small><\/strong>/u);
  assert.match(tile, /class="condition-secondary-comparison">\s*<span>Overnight<\/span>\s*<strong>1–2<small>AM<\/small><\/strong>/u);
  assert.match(tile, /condition-forecast-label">Max<\/span> <strong>20<small>%<\/small><\/strong>[\s\S]*?condition-forecast-label">Min<\/span> <strong>5<small>%<\/small><\/strong>/u);
});

// apply astronomical bounds using the selected site's coordinates and timezone
test("clouds tile uses the selected site's astronomical daylight", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-11T20:00:00Z") });
  const foreignSite = { ...site, latitude: 1.8721, longitude: -157.4278, timezone: "Pacific/Kiritimati" };
  const tile = renderCloudTile([
    classifiedCloudForecastRecord("2026-09-11T11:00:00Z", 5, null, null),
    classifiedCloudForecastRecord("2026-09-11T17:00:00Z", 20, null, null),
    classifiedCloudForecastRecord("2026-09-11T18:00:00Z", 20, null, null),
    classifiedCloudForecastRecord("2026-09-12T09:00:00Z", 95, null, null),
  ], foreignSite);
  assert.match(tile, /Clearest<\/span>\s*<strong>7–9<small>AM<\/small><\/strong>/u);
  assert.match(tile, /class="condition-secondary-comparison">\s*<span>Overnight<\/span>\s*<strong>1–2<small>AM<\/small><\/strong>/u);
  assert.match(tile, /condition-forecast-label">Max<\/span> <strong>95<small>%<\/small><\/strong>[\s\S]*?condition-forecast-label">Min<\/span> <strong>5<small>%<\/small><\/strong>/u);
});

// omit the overall comparison when the daytime and full-day ranges match
test("clouds tile hides an identical overall range", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-12T20:00:00Z") });
  const tile = renderCloudTile([
    cloudForecastRecord("2026-09-12T20:00:00Z", 20),
    cloudForecastRecord("2026-09-12T21:00:00Z", 20),
  ]);
  assert.match(tile, /Clearest<\/span>\s*<strong>1–3<small>PM<\/small><\/strong>/u);
  assert.doesNotMatch(tile, /condition-secondary-paired|condition-secondary-comparison|Overnight/u);
  assert.equal((tile.match(/<strong>/gu) ?? []).length, 4);
});

// compare tied minima when the earlier full-day window falls at night
test("clouds tile shows an earlier overall window for equal cloud minima", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-12T20:00:00Z") });
  const tile = renderCloudTile([
    cloudForecastRecord("2026-09-12T08:00:00Z", 20),
    cloudForecastRecord("2026-09-12T15:00:00Z", 20),
    cloudForecastRecord("2026-09-12T16:00:00Z", 20),
  ]);
  assert.match(tile, /Clearest<\/span>\s*<strong>8–10<small>AM<\/small><\/strong>/u);
  assert.match(tile, /class="condition-secondary-comparison">\s*<span>Overnight<\/span>\s*<strong>1–2<small>AM<\/small><\/strong>/u);
  assert.match(tile, /condition-forecast-label">Max<\/span> <strong>20<small>%<\/small><\/strong>[\s\S]*?condition-forecast-label">Min<\/span> <strong>20<small>%<\/small><\/strong>/u);
});

// hide the optional comparison when neither range is available
test("clouds tile hides the overall range when all forecast cover is missing", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-12T20:00:00Z") });
  const tile = renderCloudTile([cloudForecastRecord("2026-09-12T20:00:00Z", null)]);
  assert.match(tile, /Clearest<\/span>\s*<strong>—<\/strong>/u);
  assert.doesNotMatch(tile, /condition-secondary-paired|condition-secondary-comparison|Overnight/u);
  assert.match(tile, /condition-forecast-label">Max<\/span> <strong>—<\/strong>[\s\S]*?condition-forecast-label">Min<\/span> <strong>—<\/strong>/u);
  assert.equal((tile.match(/<strong>/gu) ?? []).length, 4);
});

// select the earliest minimum range in the farm day regardless of forecast order or stale observations
test("clouds tile shows model cover and the earliest clearest farm-local range", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-13T05:00:00Z") });
  const forecasts = [
    ["2026-09-13T07:00:00Z", 0, 0],
    ["2026-09-13T15:00:00Z", 25, 100],
    ["2026-09-12T06:00:00Z", 0, 0],
    ["2026-09-12T07:00:00Z", null, 0],
    ["2026-09-12T22:00:00Z", 12, 100],
    ["2026-09-12T19:00:00Z", 12, 100],
    ["2026-09-12T18:00:00Z", 30, 100],
  ].map(
    // build deliberately unsorted hourly model values
    ([validAt, cloudCoverPercent, solarRadiationWm2]) => ({
      ...forecastRecord,
      validAt,
      metrics: { ...forecastRecord.metrics, cloudCoverPercent, solarRadiationWm2 },
    }),
  );
  const state = {
    ...new WeatherDashboardController({ storage: null }).state,
    current: [physicalRecord, record],
    forecast: forecasts,
    loading: false,
    selectedSite: site,
  };
  const html = renderWeatherDashboard(state);
  const tile = html.match(/<article[^>]*data-condition="clouds"[\s\S]*?<\/article>/u)?.[0];
  assert.ok(tile);
  assert.match(tile, /class="condition-primary"><strong>42<small>%<\/small><\/strong>/u);
  assert.match(tile, /class="condition-status condition-status-dark">[\s\S]*?<span>Light<\/span>/u);
  assert.match(tile, /class="condition-secondary-divider">Clearest<\/span>\s*<strong>12–1<small>PM<\/small><\/strong>/u);
  assert.doesNotMatch(tile, /condition-secondary-paired|condition-secondary-comparison|Overnight/u);
  assert.match(tile, /condition-forecast-label">Max<\/span> <strong>30<small>%<\/small><\/strong>[\s\S]*?condition-forecast-label">Min<\/span> <strong>12<small>%<\/small><\/strong>/u);
  assert.doesNotMatch(tile, /Modeled|NaN|Invalid/u);
  assert.equal((tile.match(/condition-forecast-reading condition-forecast-tone-neutral/gu) ?? []).length, 2);
  assert.equal(state.forecast, forecasts);
  assert.equal(forecasts[0].validAt, "2026-09-13T07:00:00Z");
  assert.doesNotMatch(renderWeatherDashboard(state, "forecast"), /data-condition="clouds"/u);

  // change the calendar day independently of old current observations
  context.mock.timers.setTime(new Date("2026-09-13T07:00:00Z").getTime());
  const nextDay = renderWeatherDashboard(state).match(/<article[^>]*data-condition="clouds"[\s\S]*?<\/article>/u)?.[0];
  assert.match(nextDay, /Clearest<\/span>\s*<strong>8–9<small>AM<\/small>/u);
  assert.match(nextDay, /class="condition-secondary-comparison">\s*<span>Overnight<\/span>\s*<strong>12–1<small>AM<\/small>/u);
  assert.match(nextDay, /condition-forecast-label">Max<\/span> <strong>25<small>%<\/small><\/strong>[\s\S]*?condition-forecast-label">Min<\/span> <strong>0<small>%<\/small><\/strong>/u);
});

// preserve genuine zero cover and keep absent model values unavailable
test("clouds tile handles clear skies and missing current or forecast cover", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-12T20:00:00Z") });

  // exercise independent availability of both cloud statistics
  for (const [cover, forecastCover, expectedStatus, expectedCover, expectedTime, expectedRange] of [
    [0, 0, /<span>Clear<\/span>/u, /<strong>0<small>%<\/small><\/strong>/u, /<strong>1–2<small>PM<\/small><\/strong>/u, /Max<\/span> <strong>0<small>%<\/small><\/strong>[\s\S]*?Min<\/span> <strong>0<small>%<\/small><\/strong>/u],
    [null, null, /<span>Unavailable<\/span>/u, /<strong>—<\/strong>/u, /<strong>—<\/strong>/u, /Max<\/span> <strong>—<\/strong>[\s\S]*?Min<\/span> <strong>—<\/strong>/u],
    [42, null, /<span>Light<\/span>/u, /<strong>42<small>%<\/small><\/strong>/u, /<strong>—<\/strong>/u, /Max<\/span> <strong>—<\/strong>[\s\S]*?Min<\/span> <strong>—<\/strong>/u],
    [null, 12, /<span>Unavailable<\/span>/u, /<strong>—<\/strong>/u, /<strong>1–2<small>PM<\/small><\/strong>/u, /Max<\/span> <strong>12<small>%<\/small><\/strong>[\s\S]*?Min<\/span> <strong>12<small>%<\/small><\/strong>/u],
  ]) {
    const state = {
      ...new WeatherDashboardController({ storage: null }).state,
      current: [{ ...record, metrics: { ...record.metrics, cloudCoverPercent: cover } }],
      forecast: [{ ...forecastRecord, validAt: "2026-09-12T20:00:00Z", metrics: { ...forecastRecord.metrics, cloudCoverPercent: forecastCover } }],
      loading: false,
      selectedSite: site,
    };
    const tile = renderWeatherDashboard(state).match(/<article[^>]*data-condition="clouds"[\s\S]*?<\/article>/u)?.[0];
    const primary = tile.match(/class="condition-primary">([\s\S]*?)<\/div>/u)?.[1];
    const secondary = tile.match(/class="condition-secondary">([\s\S]*?)<\/div>/u)?.[1];
    assert.match(tile, expectedStatus);
    assert.match(primary, expectedCover);
    assert.match(secondary, expectedTime);
    assert.match(tile, expectedRange);

    // refuse to substitute on-site values for missing model data
    const noModel = renderWeatherDashboard({ ...state, current: [physicalRecord], forecast: [] })
      .match(/<article[^>]*data-condition="clouds"[\s\S]*?<\/article>/u)?.[0];
    assert.equal((noModel.match(/<strong>—<\/strong>/gu) ?? []).length, 4);
    assert.doesNotMatch(noModel, /condition-secondary-paired|condition-secondary-comparison|Overnight/u);
  }
});

// keep the rain chip concise when no rainfall is detected
test("rain tile labels zero precipitation as dry without a temporal qualifier", () => {
  const state = {
    ...new WeatherDashboardController({ storage: null }).state,
    current: [physicalRecord],
    loading: false,
    selectedSite: site,
  };
  const tile = renderWeatherDashboard(state).match(/<article[^>]*data-condition="rain"[\s\S]*?<\/article>/u)?.[0];
  assert.ok(tile);
  assert.match(tile, /class="condition-status condition-status-light">[\s\S]*?<span>Dry<\/span>/u);
  assert.doesNotMatch(tile, /Dry now/u);
});

// render today's solar readings and signed comparison independently of observations
test("sunset tile emphasizes today's sunset with golden hour as the secondary stat", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-13T05:00:00Z") });
  const state = {
    ...new WeatherDashboardController({ storage: null }).state,
    current: [record],
    loading: false,
    selectedSite: site,
  };
  const html = renderWeatherDashboard(state);
  const sunset = html.match(/<article[^>]*data-condition="sunset"[\s\S]*?<\/article>/u)?.[0];
  assert.ok(sunset);
  assert.match(sunset, /<span>Today<\/span>/u);
  assert.match(sunset, /class="condition-primary"><strong>7:28<small>PM<\/small><\/strong>/u);
  assert.match(sunset, /class="condition-secondary-divider">Golden hour<\/span>\s*<strong>6:47<small>PM<\/small><\/strong>/u);
  assert.match(sunset, /class="condition-forecast-label">vs yesterday<\/span>\s*<strong>-2 <small>mins<\/small><\/strong>/u);
  assert.doesNotMatch(renderWeatherDashboard(state, "forecast"), /data-condition="sunset"/u);

  const polarHtml = renderWeatherDashboard({
    ...state,
    selectedSite: { ...site, latitude: 89 },
  });
  const polarTile = polarHtml.match(/<article[^>]*data-condition="sunset"[\s\S]*?<\/article>/u)?.[0];
  assert.match(polarTile, /<strong>—<\/strong>/u);
  assert.match(polarTile, /class="condition-forecast-label">vs yesterday<\/span>\s*<strong>—<\/strong>/u);
  assert.doesNotMatch(polarTile, /Invalid|NaN/u);

  // retain an explicit positive sign and avoid signed zero
  for (const [day, expected] of [["2026-01-15", "+2"], ["2026-06-21", "0"]]) {
    context.mock.timers.setTime(new Date(`${day}T20:00:00Z`).getTime());
    const tile = renderWeatherDashboard(state).match(/<article[^>]*data-condition="sunset"[\s\S]*?<\/article>/u)?.[0];
    assert.ok(tile.includes(`<strong>${expected} <small>mins</small></strong>`));
  }
});

// retain the complete dashboard and route contracts
test("dashboard separates current conditions from the historical logs route", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-08-22T05:00:00Z") });
  const state = {
    current: [record, physicalRecord],
    dailyPrecipitation,
    error: "The latest refresh failed",
    filters: {
      sourceId: "10",
      stationSlug: "open-meteo-virtual",
    },
    forecast: [forecastRecord],
    forecastDays: 1,
    history: [record],
    loading: false,
    mapLayer: "roads",
    nextCursor: "next-page",
    page: 0,
    propertyMapLayer: "satellite",
    propertySensorLayout: [
      {
        displayName: "Orchard soil",
        icon: "rain",
        latitude: 47.9505,
        longitude: -122.4281,
        sensorKey: "soil-1",
        updatedAt: "2026-08-22T04:59:00.000Z",
      },
    ],
    selectedPropertySensorKey: null,
    selectedStationSlug: null,
    trendDetail: "rolling",
    trendDisplayMode: "aggregate",
    trendExtremeKind: "heat",
    trendExtremeThreshold: 30,
    selectedTrendMetric: "temperatureC",
    selectedTrendYear: null,
    selectedSite: site,
    sites: [site],
    tideGeneratedAt: "2026-08-22T05:00:00.000Z",
    tides,
    trendGeneratedAt: "2026-08-22T05:00:00.000Z",
    trends: trendHistory,
    units: DEFAULT_UNIT_PREFERENCES,
  };
  const html = renderWeatherDashboard(state);
  // model the three occupied house levels
  const indoorRecord = {
    ...ecowittRecord,
    metadata: {
      ...ecowittRecord.metadata,
      provider: {
        ...ecowittRecord.metadata.provider,
        propertySensors: [
          {
            channel: null,
            key: "gateway",
            model: "GW3000",
            readings: { temperatureC: 21.5 },
          },
          {
            channel: 1,
            key: "temperature-1",
            model: "WN31",
            readings: { temperatureC: 20 },
          },
          {
            channel: 2,
            key: "temperature-2",
            model: "WN31",
            readings: { temperatureC: 18.5 },
          },
        ],
      },
    },
  };
  const adminHomeHtml = renderWeatherDashboard({
    ...state,
    current: [indoorRecord, ecowittRecord],
  }, "home", true);
  const firstPartyHtml = renderWeatherDashboard({
    ...state,
    current: [record, physicalRecord, ecowittRecord],
  });
  const nearFeelsHtml = renderWeatherDashboard({
    ...state,
    current: [{
      ...record,
      metrics: { ...record.metrics, apparentTemperatureC: 16 },
    }],
  });
  const forecastHtml = renderWeatherDashboard(state, "forecast");
  const fiveDayForecastHtml = renderWeatherDashboard({
    ...state,
    forecast: Array.from({ length: 5 },
      // provide one local midnight per extended-range day
      (_, day) => ({
        ...forecastRecord,
        id: `forecast-${String(day)}`,
        validAt: `2026-08-${String(21 + day).padStart(2, "0")}T07:00:00.000Z`,
      }),
    ),
    forecastDays: 5,
  }, "forecast");
  const logsHtml = renderWeatherDashboard(state, "logs");
  const mapHtml = renderWeatherDashboard(state, "map");
  const propertyMapHtml = renderWeatherDashboard({
    ...state,
    current: [ecowittRecord, record, physicalRecord],
  }, "map");
  const selectedPropertyMapHtml = renderWeatherDashboard({
    ...state,
    current: [ecowittRecord, record, physicalRecord],
    selectedPropertySensorKey: "soil-1",
  }, "map");
  const adminHtml = renderWeatherDashboard({
    ...state,
    current: [ecowittRecord],
  }, "admin");
  const settingsHtml = renderWeatherDashboard(state, "settings");
  const trendsHtml = renderWeatherDashboard(state, "trends");
  const windTrendsHtml = renderWeatherDashboard({
    ...state,
    selectedTrendMetric: "windGustMps",
  }, "trends");
  const farmChartsHtml = Object.fromEntries([
    "cumulativePrecipitationMm",
    "temperatureAnomalyC",
    "temperatureRangeC",
    "drySpellDays",
    "growingDegreeDaysC",
    "frostDayCount",
    "extremeDayCount",
    "windDirectionRose",
  ].map(
    // render every farm-focused chart option
    (metric) => [metric, renderWeatherDashboard({
      ...state,
      selectedTrendMetric: metric,
    }, "trends")],
  ));
  const selectedTrendsHtml = renderWeatherDashboard({
    ...state,
    trendDisplayMode: "all",
    selectedTrendYear: 2025,
  }, "trends");
  const allTrendsHtml = renderWeatherDashboard({
    ...state,
    trendDisplayMode: "all",
  }, "trends");
  const dailyTrendsHtml = renderWeatherDashboard({
    ...state,
    trendDetail: "daily",
  }, "trends");
  const denseTrendHistory = [2019, 2025, 2026].flatMap(
    // provide seven consecutive daily points per comparison year
    (year) => [0, 0, 0, 21, 21, 21, 21].map(
      // retain one visible step that a rolling average must smooth
      (temperatureC, day) => ({
        ...trend,
        metrics: { ...trend.metrics, temperatureC },
        validAt: `${String(year)}-01-${String(day + 1).padStart(2, "0")}T20:00:00.000Z`,
      }),
    ),
  );
  const denseRollingTrendsHtml = renderWeatherDashboard({
    ...state,
    trends: denseTrendHistory,
  }, "trends");
  const denseDailyTrendsHtml = renderWeatherDashboard({
    ...state,
    trendDetail: "daily",
    trends: denseTrendHistory,
  }, "trends");
  const loadingHtml = renderWeatherDashboard({
    ...state,
    error: null,
    loading: true,
  });
  const initialLoadingState = {
    ...state,
    current: [],
    dailyPrecipitation: null,
    error: null,
    forecast: [],
    history: [],
    loading: true,
    selectedSite: null,
    sites: [],
    tideGeneratedAt: null,
    tides: [],
    trends: [],
  };
  const initialHomeHtml = renderWeatherDashboard(initialLoadingState);
  const initialForecastHtml = renderWeatherDashboard(initialLoadingState, "forecast");
  const initialLogsHtml = renderWeatherDashboard(initialLoadingState, "logs");
  const initialMapHtml = renderWeatherDashboard(initialLoadingState, "map");
  const initialSettingsHtml = renderWeatherDashboard(initialLoadingState, "settings");
  const initialTrendsHtml = renderWeatherDashboard(initialLoadingState, "trends");
  const alertHtml = renderWeatherDashboard({
    ...state,
    current: [
      {
        ...record,
        metrics: {
          ...record.metrics,
          pm25MicrogramsPerCubicMeter: 40,
        },
      },
    ],
  });

  assert.match(html, /<header class="masthead home-masthead">[\s\S]*?<h1><span class="masthead-title-text"><span>Ballydídean<\/span> <span>Weather<\/span><\/span><\/h1>/u);
  assert.match(html, /data-forecast-adjustment-toggle/u);
  assert.match(forecastHtml, /data-forecast-adjustment-toggle/u);
  assert.doesNotMatch(logsHtml, /data-forecast-adjustment-toggle/u);
  assert.doesNotMatch(mapHtml, /data-forecast-adjustment-toggle/u);
  assert.doesNotMatch(settingsHtml, /data-forecast-adjustment-toggle/u);
  assert.doesNotMatch(trendsHtml, /data-forecast-adjustment-toggle/u);
  assert.doesNotMatch(html, /brand-link|brand-mark|ballydidean-wide\.svg/u);
  assert.doesNotMatch(html, /aria-label="Weather location"|data-site-selector/u);
  assert.match(html, /class="section-nav-home" href="\/" data-weather-route aria-label="Now" aria-current="page">[\s\S]*?class="section-nav-weather-icon"[\s\S]*?<\/span><span>Now<\/span><\/a>/u);
  assert.match(html, /class="section-nav-map" href="\/map" data-weather-route>[\s\S]*?>map<\/span><\/span><span>Map<\/span><\/a>/u);
  assert.match(html, /class="section-nav-forecast" href="\/forecast" data-weather-route>[\s\S]*?>partly_cloudy_day<\/span><\/span><span>Forecast<\/span><\/a>/u);
  assert.match(html, /class="section-nav-trends" href="\/trends" data-weather-route>[\s\S]*?>trending_up<\/span><\/span><span>Trends<\/span><\/a>/u);
  assert.match(html, /class="section-nav-settings" href="\/settings" data-weather-route>[\s\S]*?>settings<\/span><\/span><span>Settings<\/span><\/a>/u);
  assert.match(html, /section-nav-home[\s\S]*section-nav-forecast[\s\S]*section-nav-trends[\s\S]*section-nav-map[\s\S]*section-nav-settings/u);
  assert.doesNotMatch(html, /section-nav-logs|href="\/logs"/u);
  assert.doesNotMatch(html, /data-unit-settings-form|<dialog/u);
  assert.doesNotMatch(html, /Current conditions<\/span>|Weather logs|>Units</u);
  assert.doesNotMatch(html, /name="stationSlug"|name="sourceId"|name="sourceKind"/u);
  assert.doesNotMatch(html, /<table/u);
  assert.doesNotMatch(html, /No active local weather watches|class="alert-list"/u);
  assert.match(alertHtml, /<section class="alert-list" aria-label="Conditions to watch">/u);
  assert.doesNotMatch(alertHtml, /Threshold watch|<h2[^>]*>Conditions to watch|alert-strip|alert-disclaimer/u);
  assert.match(alertHtml, /PM2\.5 is 40 µg\/m³/u);
  assert.doesNotMatch(html, /Forecast timeline|Yearly trends|Nearby station map|station-map-panel|data-station-select/u);
  assert.match(forecastHtml, /href="\/forecast" data-weather-route aria-current="page"/u);
  assert.doesNotMatch(forecastHtml, /Forecast timeline|forecast-model|forecast-scrub-help/u);
  assert.match(forecastHtml, /<header class="masthead forecast-masthead">[\s\S]*?class="range-selector forecast-range-selector"[\s\S]*?class="masthead-actions">[\s\S]*?data-forecast-adjustment-toggle[\s\S]*?<\/header>/u);
  assert.match(forecastHtml, /<section class="panel forecast-panel" aria-label="Weather forecast">\s*<div class="forecast-chart-shell">/u);
  assert.equal((forecastHtml.match(/aria-label="Forecast range"/gu) ?? []).length, 1);
  assert.match(initialForecastHtml, /<header class="masthead forecast-masthead">[\s\S]*?data-forecast-days="1" aria-pressed="true" disabled[\s\S]*?<\/header>/u);
  assert.equal((initialForecastHtml.match(/aria-label="Forecast range"/gu) ?? []).length, 1);
  assert.doesNotMatch(html, /aria-label="Forecast range"/u);
  assert.doesNotMatch(forecastHtml, /forecast-controls/u);
  assert.match(forecastHtml, /data-forecast-days="1" aria-pressed="true"/u);
  assert.match(forecastHtml, /data-forecast-days="5" aria-pressed="false"/u);
  assert.match(forecastHtml, /data-forecast-days="10" aria-pressed="false"/u);
  assert.equal((forecastHtml.match(/forecast-chart-heading-top/gu) ?? []).length, 9);
  assert.doesNotMatch(forecastHtml, /forecast-chart-heading-bottom/u);
  assert.equal((forecastHtml.match(/class="forecast-chart"/gu) ?? []).length, 9);
  assert.match(forecastHtml, /data-forecast-charts[\s\S]*data-forecast-times=/u);
  assert.match(forecastHtml, /class="forecast-current-time-line"/u);
  assert.match(forecastHtml, /class="forecast-shared-crosshair"/u);
  assert.doesNotMatch(forecastHtml, /forecast-chart-days|forecast-chart-day-start/u);
  assert.equal((fiveDayForecastHtml.match(/class="forecast-chart-days"/gu) ?? []).length, 9);
  assert.equal((fiveDayForecastHtml.match(/class="forecast-chart-day-start"/gu) ?? []).length, 45);
  assert.match(fiveDayForecastHtml, /data-forecast-day="2026-08-21"><b>Fri 21<\/b>/u);
  assert.match(fiveDayForecastHtml, /data-forecast-day="2026-08-25"><b>Tue 25<\/b>/u);
  assert.equal((forecastHtml.match(/class="forecast-chart-daylight"/gu) ?? []).length, 9);
  assert.equal((forecastHtml.match(/<linearGradient id="forecast-line-/gu) ?? []).length, 11);
  assert.match(forecastHtml, /data-forecast-light="day"/u);
  assert.match(forecastHtml, /data-forecast-chart="temperature"[\s\S]*?data-forecast-min="-1\.1111111111"[\s\S]*?data-forecast-max="26\.6666666667"/u);
  assert.match(forecastHtml, /data-forecast-chart="wind"[\s\S]*?data-forecast-min="0"[\s\S]*?data-forecast-max="22\.3519999995"/u);
  assert.match(forecastHtml, /data-forecast-chart="rain-rate"[\s\S]*?data-forecast-min="0"[\s\S]*?data-forecast-max="25\.4"/u);
  assert.match(forecastHtml, /data-forecast-chart="clouds"[\s\S]*?data-forecast-format="cloudCover"[\s\S]*?data-forecast-min="0"[\s\S]*?data-forecast-max="100"/u);
  assert.match(forecastHtml, /data-forecast-chart="uv-index"[\s\S]*?data-forecast-min="0"[\s\S]*?data-forecast-max="4"/u);
  assert.match(forecastHtml, /data-forecast-chart="tide"[\s\S]*?data-forecast-min="-0\.3048"[\s\S]*?data-forecast-max="3\.6576"/u);
  assert.match(forecastHtml, /data-forecast-chart="uv-index"[\s\S]*?forecast-chart-scale-maximum">4<[\s\S]*?forecast-chart-scale-minimum">0</u);
  assert.match(forecastHtml, /data-forecast-chart="tide"[\s\S]*?forecast-chart-scale-maximum">12 ft<[\s\S]*?forecast-chart-scale-minimum">-1 ft</u);
  assert.doesNotMatch(forecastHtml, /forecast-chart-guide/u);
  assert.match(forecastHtml, /forecast-chart-scale-maximum">80 °F<[\s\S]*forecast-chart-scale-minimum">30 °F</u);
  assert.match(forecastHtml, /forecast-line-air-quality-0[\s\S]*stop-color="rgb\(0, 146, 63\)"/u);
  assert.deepEqual(forecastChartSeries(forecastHtml, "temperature"), [
    { label: "Feels like", values: [forecastRecord.metrics.apparentTemperatureC] },
    { label: "Air Temp", values: [forecastRecord.metrics.temperatureC] },
  ]);
  assert.deepEqual(forecastChartSeries(forecastHtml, "clouds"), [
    { label: "Cover", values: [forecastRecord.metrics.cloudCoverPercent] },
  ]);
  assert.deepEqual(forecastChartSeries(forecastHtml, "pressure"), [
    { label: "3h change", values: [null] },
  ]);
  assert.doesNotMatch(forecastChartHtml(forecastHtml, "air-quality"), /µg\/m³/u);
  assert.doesNotMatch(forecastChartHtml(forecastHtml, "pressure"), /hPa/u);
  assert.match(forecastHtml, /data-forecast-chart="temperature"[\s\S]*data-forecast-chart="wind"[\s\S]*data-forecast-chart="rain-rate"[\s\S]*data-forecast-chart="clouds"[\s\S]*data-forecast-chart="humidity"[\s\S]*data-forecast-chart="air-quality"[\s\S]*data-forecast-chart="uv-index"[\s\S]*data-forecast-chart="pressure"[\s\S]*data-forecast-chart="tide"/u);
  assert.doesNotMatch(forecastHtml, /Drag left or right to scrub time|Swipe vertically to scroll the page/u);
  assert.match(forecastHtml, /class="forecast-x-axis"/u);
  assert.equal((forecastHtml.match(/class="forecast-x-tick"/gu) ?? []).length, 1);
  assert.doesNotMatch(forecastHtml, /forecast-weather-map|data-forecast-map|\/maps\/xweather|Weather maps by Xweather/u);
  assert.doesNotMatch(fiveDayForecastHtml, /forecast-weather-map|data-forecast-map|\/maps\/xweather|Weather maps by Xweather/u);
  assert.doesNotMatch(forecastHtml, /forecast-hour|forecast-timeline/u);
  assert.doesNotMatch(forecastHtml, /class="current-conditions"|Yearly trends|Nearby station map|<table/u);
  assert.match(trendsHtml, /href="\/trends" data-weather-route aria-current="page"/u);
  assert.doesNotMatch(trendsHtml, /Yearly trends|Calendar comparison/u);
  assert.equal((trendsHtml.match(/class="trend-chart"/gu) ?? []).length, 1);
  assert.match(trendsHtml, /data-trend-chart="temperatureC"[^>]*data-trend-domain="visible"/u);
  const trendDomain = trendsHtml.match(/data-trend-maximum="([^"]+)" data-trend-minimum="([^"]+)" data-trend-domain="visible"/u);
  const allTrendDomain = allTrendsHtml.match(/data-trend-maximum="([^"]+)" data-trend-minimum="([^"]+)" data-trend-domain="visible"/u);
  assert.ok(trendDomain !== null);
  assert.ok(allTrendDomain !== null);
  assert.ok(Math.abs(Number(trendDomain[1]) - ((72 - 32) * 5) / 9) < 0.000_001);
  assert.ok(Math.abs(Number(trendDomain[2]) - 5) < 0.000_001);
  assert.ok(Math.abs(Number(allTrendDomain[1]) - ((72 - 32) * 5) / 9) < 0.000_001);
  assert.ok(Math.abs(Number(allTrendDomain[2]) - 5) < 0.000_001);
  assert.match(trendsHtml, /<div class="trend-metric-control" data-trend-metric-control>[\s\S]*?<h2 class="trend-chart-title">[\s\S]*?<button type="button" class="trend-metric-trigger" data-trend-metric-trigger aria-expanded="false" aria-haspopup="menu" aria-controls="trend-metric-flyover">[\s\S]*?<span>Temperature<\/span>[\s\S]*?class="trend-metric-caret"[\s\S]*?<div class="trend-metric-flyover" id="trend-metric-flyover" role="menu" aria-label="Trend measurement" hidden>[\s\S]*?data-trend-metric-option="temperatureC" role="menuitemradio" aria-checked="true">Temperature<\/button>[\s\S]*?data-trend-metric-option="apparentTemperatureC"[^>]*>Feels like<\/button>[\s\S]*?data-trend-metric-option="windSpeedMps"[^>]*>Wind speed<\/button>[\s\S]*?data-trend-metric-option="windGustMps"[^>]*>Wind gust<\/button>[\s\S]*?data-trend-metric-option="precipitationMm"[^>]*>Daily rain<\/button>[\s\S]*?data-trend-metric-option="relativeHumidityPercent"[^>]*>Humidity<\/button>[\s\S]*?data-trend-metric-option="pressureHpa"[^>]*>Pressure<\/button>/u);
  assert.equal((trendsHtml.match(/data-trend-metric-option=/gu) ?? []).length, 15);
  assert.match(trendsHtml, /class="trend-metric-option-group" role="presentation">[\s\S]*?Measurements[\s\S]*?class="trend-metric-option-group" role="presentation">[\s\S]*?Farm insights/u);
  for (const [metric, chartHtml] of Object.entries(farmChartsHtml)) {
    // require one selectable rendered chart for every recommendation
    assert.match(chartHtml, new RegExp(`data-trend-chart="${metric}"`, "u"));
  }
  assert.match(farmChartsHtml.cumulativePrecipitationMm, /Running annual total/u);
  assert.match(farmChartsHtml.cumulativePrecipitationMm, /data-trend-crosshair-value="2026">0\.02 in<\/output>/u);
  assert.doesNotMatch(farmChartsHtml.cumulativePrecipitationMm, /data-trend-detail-toggle/u);
  assert.match(farmChartsHtml.temperatureAnomalyC, /Versus historical daily average/u);
  assert.match(farmChartsHtml.temperatureAnomalyC, /data-trend-crosshair-value="2026">5 °F<\/output>/u);
  assert.match(farmChartsHtml.temperatureRangeC, /Daily high − low/u);
  assert.match(farmChartsHtml.temperatureRangeC, /data-trend-crosshair-value="2026">15 °F<\/output>/u);
  assert.match(farmChartsHtml.drySpellDays, /Rain below 0\.01 in/u);
  assert.match(farmChartsHtml.drySpellDays, /data-trend-crosshair-value="2026">2 days<\/output>/u);
  // retain both temperature scales while labeling accumulated heat consistently
  for (const [temperature, base, value] of [["fahrenheit", "50 °F", 22], ["celsius", "10 °C", 12]]) {
    const growingHeatHtml = renderWeatherDashboard({
      ...state,
      selectedTrendMetric: "growingDegreeDaysC",
      units: { ...state.units, temperature },
    }, "trends");
    assert.match(growingHeatHtml, /<span>Accumulated growing heat<\/span>/u);
    assert.match(growingHeatHtml, /role="img" aria-label="Accumulated growing heat annual progression/u);
    assert.match(growingHeatHtml, /class="trend-y-axis" aria-hidden="true">(?:\s*<span>[\d,]+ GDD<\/span>){5}/u);
    assert.match(growingHeatHtml, /class="trend-chart-range">[\d,]+–[\d,]+ GDD<\/span>/u);
    assert.ok(growingHeatHtml.includes(`Base ${base}`));
    assert.ok(growingHeatHtml.includes(`data-trend-crosshair-value="2026">${value} GDD</output>`));
    assert.doesNotMatch(growingHeatHtml, /°[FC]·days|Growing degree days/u);
  }
  assert.match(farmChartsHtml.frostDayCount, /Daily low ≤ 32 °F/u);
  assert.match(farmChartsHtml.frostDayCount, /data-trend-crosshair-value="2026">0 days<\/output>/u);
  assert.match(farmChartsHtml.extremeDayCount, /data-trend-extreme-kind[\s\S]*?<option value="heat" selected>Heat<\/option>[\s\S]*?data-trend-extreme-threshold[^>]*value="86\.0"/u);
  assert.match(farmChartsHtml.extremeDayCount, /data-trend-crosshair-value="2026">0 days<\/output>/u);
  assert.equal((farmChartsHtml.windDirectionRose.match(/data-wind-rose-sector=/gu) ?? []).length, 2);
  assert.match(farmChartsHtml.windDirectionRose, /class="trend-wind-rose"[\s\S]*?Wind direction rose comparing historical days with 2026[\s\S]*?aria-label="Wind rose legend"[\s\S]*?Historical[\s\S]*?2026/u);
  assert.equal((farmChartsHtml.windDirectionRose.match(/<li>[^<]+: historical/gu) ?? []).length, 16);
  assert.match(trendsHtml, /<svg[^>]*preserveAspectRatio="none"/u);
  assert.match(trendsHtml, /data-trend-detail="rolling" data-trend-display-mode="aggregate"/u);
  assert.equal((trendsHtml.match(/class="trend-historical-quartile-band"/gu) ?? []).length, 1);
  assert.equal((trendsHtml.match(/class="trend-historical-range-line"/gu) ?? []).length, 2);
  assert.equal((trendsHtml.match(/class="trend-aggregate-median-line"/gu) ?? []).length, 1);
  assert.equal((trendsHtml.match(/class="trend-year-line/gu) ?? []).length, 1);
  assert.equal((trendsHtml.match(/class="trend-year-hit-target/gu) ?? []).length, 0);
  assert.match(trendsHtml, /class="trend-year-line trend-year-line-current"[^>]*data-trend-year="2026"[^>]*stroke="var\(--brand-orange\)"/u);
  assert.match(trendsHtml, /data-trend-mode-toggle aria-pressed="false">[\s\S]*?data-trend-toggle-icon="show-all"[\s\S]*?<span>Show all<\/span><\/button>/u);
  assert.match(trendsHtml, /data-trend-detail-toggle aria-pressed="false">[\s\S]*?data-trend-toggle-icon="daily"[\s\S]*?<span>Daily detail<\/span><\/button>/u);
  assert.equal((allTrendsHtml.match(/class="trend-year-line/gu) ?? []).length, 3);
  assert.equal((allTrendsHtml.match(/class="trend-year-hit-target/gu) ?? []).length, 3);
  assert.equal((allTrendsHtml.match(/trend-year-line-current/gu) ?? []).length, 1);
  assert.match(allTrendsHtml, /data-trend-mode-toggle aria-pressed="true">[\s\S]*?data-trend-toggle-icon="aggregate"[\s\S]*?<span>Aggregate<\/span><\/button>/u);
  assert.match(dailyTrendsHtml, /data-trend-detail="daily" data-trend-display-mode="aggregate"/u);
  assert.match(dailyTrendsHtml, /data-trend-detail-toggle aria-pressed="true">[\s\S]*?data-trend-toggle-icon="rolling"[\s\S]*?<span>7-day average<\/span><\/button>/u);
  assert.match(dailyTrendsHtml, /class="trend-chart-viewport">[\s\S]*class="trend-month-axis"[\s\S]*<\/div>\s*<\/div>\s*<div class="trend-chart-fixed-chrome">[\s\S]*aria-label="Trend measurement"[\s\S]*aria-label="Trend legend"/u);
  assert.equal((dailyTrendsHtml.match(/aria-label="Trend measurement"/gu) ?? []).length, 1);
  assert.equal((dailyTrendsHtml.match(/aria-label="Trend legend"/gu) ?? []).length, 1);
  const denseRollingMedian = denseRollingTrendsHtml.match(/<polyline points="([^"]+)" class="trend-aggregate-median-line"/u);
  const denseDailyMedian = denseDailyTrendsHtml.match(/<polyline points="([^"]+)" class="trend-aggregate-median-line"/u);
  assert.ok(denseRollingMedian !== null);
  assert.ok(denseDailyMedian !== null);
  assert.equal(denseRollingMedian[1]?.trim().split(/\s+/u).length, 7);
  assert.notEqual(denseRollingMedian[1], denseDailyMedian[1]);
  assert.equal((trendsHtml.match(/class="trend-month-label"/gu) ?? []).length, 12);
  assert.equal((trendsHtml.match(/class="trend-y-grid-line"/gu) ?? []).length, 4);
  assert.match(trendsHtml, /class="trend-y-axis" aria-hidden="true">(?:\s*<span>[^<]+<\/span>){5}/u);
  assert.match(trendsHtml, /data-trend-crosshair-slider[\s\S]*role="slider"[\s\S]*aria-label="Annual trend date scrubber"/u);
  assert.match(trendsHtml, /class="trend-crosshair-date-pill" data-trend-crosshair-date datetime="2000-07-15" aria-hidden="true">Jul 15<\/time>/u);
  assert.match(trendsHtml, /data-trend-today-position="0\.636612021858"/u);
  assert.match(trendsHtml, /class="trend-today-marker" aria-hidden="true"><span>Today<\/span>/u);
  assert.equal((trendsHtml.match(/data-trend-crosshair-value=/gu) ?? []).length, 2);
  assert.match(trendsHtml, /<strong>Median<\/strong><output data-trend-crosshair-value="median">66 °F<\/output>/u);
  assert.match(trendsHtml, /data-trend-crosshair-value="2026">72 °F<\/output>/u);
  assert.match(trendsHtml, /class="trend-chart-legend" aria-label="Trend legend"[\s\S]*25th–75th[\s\S]*Historical min\/max[\s\S]*Historical median[\s\S]*trend-current-year-color[\s\S]*2026/u);
  assert.match(allTrendsHtml, /class="trend-year-legend" data-trend-year-select="2019" aria-pressed="false"/u);
  assert.match(selectedTrendsHtml, /class="trend-year-legend trend-year-legend-selected" data-trend-year-select="2025" aria-pressed="true"/u);
  assert.match(selectedTrendsHtml, /data-selected-trend-year="2025"/u);
  assert.equal((selectedTrendsHtml.match(/trend-year-line-selected/gu) ?? []).length, 1);
  assert.equal((selectedTrendsHtml.match(/data-trend-crosshair-value=/gu) ?? []).length, 2);
  assert.match(selectedTrendsHtml, /data-trend-crosshair-value="2025">68 °F<\/output>/u);
  assert.match(selectedTrendsHtml, /data-trend-crosshair-value="2026">72 °F<\/output>/u);
  assert.match(selectedTrendsHtml, /data-trend-year="2026"[\s\S]*data-trend-year="2025"/u);
  assert.match(windTrendsHtml, /data-trend-chart="windGustMps"[\s\S]*?<h2 class="trend-chart-title">[\s\S]*?<span>Wind gust<\/span>[\s\S]*?data-trend-metric-option="windGustMps" role="menuitemradio" aria-checked="true">Wind gust<\/button>/u);
  assert.doesNotMatch(windTrendsHtml, /data-trend-chart="temperatureC"/u);
  assert.doesNotMatch(trendsHtml, /class="current-conditions"|Forecast timeline|Nearby station map|<table/u);
  assert.match(settingsHtml, /href="\/settings" data-weather-route aria-current="page"/u);
  assert.match(settingsHtml, /<section class="unit-settings-page" aria-labelledby="unit-settings-heading">/u);
  assert.match(settingsHtml, /<h2 id="unit-settings-heading">Measurement units<\/h2>/u);
  assert.match(settingsHtml, /class="settings-logs-link" href="\/logs" data-weather-route aria-label="Logs">[\s\S]*?>history<\/span>[\s\S]*?<strong>Logs<\/strong>/u);
  assert.match(settingsHtml, /href="\/admin" aria-label="Admin">[\s\S]*?>settings<\/span>[\s\S]*?<strong>Admin<\/strong>/u);
  assert.doesNotMatch(settingsHtml, /href="\/admin" data-weather-route/u);
  assert.match(settingsHtml, /href="\/privacy" aria-label="Privacy policy">[\s\S]*?<strong>Privacy policy<\/strong>/u);
  assert.doesNotMatch(settingsHtml, /<a[^>]*href="\/privacy"[^>]*data-weather-route/u);
  assert.doesNotMatch(settingsHtml, /Property sensors<\/strong>|Name and place/u);
  assert.match(settingsHtml, /data-unit-settings-form/u);
  assert.doesNotMatch(settingsHtml, /<dialog|data-unit-settings-open|data-unit-settings-close/u);
  assert.doesNotMatch(settingsHtml, /class="current-conditions"|Forecast timeline|Yearly trends|Nearby station map|<table/u);
  assert.match(mapHtml, /href="\/map" data-weather-route aria-current="page"/u);
  assert.match(mapHtml, /Nearby station map/u);
  assert.match(mapHtml, /data-map-layer="roads" aria-pressed="true"/u);
  assert.match(mapHtml, /tile\.openstreetmap\.org\/13\//u);
  assert.match(mapHtml, /<image href="https:\/\/tile\.openstreetmap\.org/u);
  assert.match(mapHtml, /<svg class="station-map-svg"[^>]*>[\s\S]*<g class="map-tile-layer"[^>]*>[\s\S]*<g class="station-map-overlay">/u);
  assert.equal((mapHtml.match(/<svg class="station-map-svg"/gu) ?? []).length, 1);
  assert.doesNotMatch(mapHtml, /<image[^>]+style=/u);
  assert.match(mapHtml, /OpenStreetMap contributors/u);
  assert.match(mapHtml, /data-station-select="tempest-38270"/u);
  assert.doesNotMatch(mapHtml, /data-station-current="tempest-38270"/u);
  assert.match(propertyMapHtml, /<h2 id="property-map-heading">Property sensors<\/h2>/u);
  assert.match(propertyMapHtml, /class="property-map-svg"[\s\S]*USGSNAIPImagery[\s\S]*size=1280%2C800[\s\S]*Orchard soil · Moisture 42 %[\s\S]*Orchard soil[\s\S]*Moisture 42 %/u);
  assert.match(propertyMapHtml, /class="property-map-layout"[\s\S]*class="property-map"[\s\S]*class="property-sensor-list"/u);
  assert.match(propertyMapHtml, /class="property-sensor-marker-icon">rainy<\/text>/u);
  assert.match(propertyMapHtml, /class="property-sensor-list-icon">[\s\S]*?>rainy<\/span>/u);
  assert.match(propertyMapHtml, /data-property-sensor-view="soil-1"[\s\S]*aria-expanded="false"/u);
  assert.doesNotMatch(propertyMapHtml, /data-property-sensor-details="soil-1"/u);
  assert.match(selectedPropertyMapHtml, /class="property-sensor-marker selected"[\s\S]*aria-expanded="true"/u);
  assert.match(selectedPropertyMapHtml, /data-property-sensor-details="soil-1"[\s\S]*Temp 63\.9 °F[\s\S]*Moisture 42 %[\s\S]*EcoWitt WH52<\/strong> · channel 1 · soil-1[\s\S]*Position<\/strong> 47\.950500, -122\.428100/u);
  assert.match(propertyMapHtml, /USGS and USDA NAIP aerial imagery[\s\S]*The National Map/u);
  assert.doesNotMatch(propertyMapHtml, /USGSImageryOnly\/MapServer\/tile\/1[67]\//u);
  assert.equal((propertyMapHtml.match(/data-property-map-layer=/gu) ?? []).length, 3);
  assert.match(propertyMapHtml, /data-property-interactive-map[\s\S]*data-property-map-world[\s\S]*data-property-map-zoom="in"/u);
  assert.ok(propertyMapHtml.indexOf("property-map-panel") < propertyMapHtml.indexOf("station-map-panel"));
  assert.match(adminHtml, /href="\/settings" data-weather-route aria-current="page"/u);
  assert.match(adminHtml, /data-property-sensor-select="soil-1" aria-pressed="true"/u);
  assert.match(adminHtml, /data-property-sensor-form data-sensor-key="soil-1"/u);
  assert.match(adminHtml, /data-property-position-map/u);
  assert.match(adminHtml, /data-property-map-anchor[\s\S]*class="property-position-marker-pin" d="M0 0/u);
  assert.equal((adminHtml.match(/type="radio" name="icon"/gu) ?? []).length, 4);
  assert.match(adminHtml, /name="icon" value="rain" aria-label="Rain" checked/u);
  assert.match(adminHtml, /data-property-position-marker-icon[^>]*>rainy<\/text>/u);
  assert.match(adminHtml, /class="material-inline-icon"[\s\S]*Save sensor/u);
  assert.match(adminHtml, /class="admin-logout-form" action="\/admin\/logout" method="post">[\s\S]*Log out/u);
  assert.doesNotMatch(mapHtml, /class="current-conditions"|Forecast timeline|Yearly trends|<table/u);
  assert.doesNotMatch(html, /Farm sensor map/u);
  assert.doesNotMatch(html, /data-indoor-house/u);
  assert.doesNotMatch(html, /data-admin-soil-map/u);
  assert.match(adminHomeHtml, /<section class="indoor-house-panel"[^>]*data-indoor-house/u);
  assert.match(adminHomeHtml, /Second floor[\s\S]*71<small>°F<\/small>[\s\S]*First floor[\s\S]*68<small>°F<\/small>[\s\S]*Basement[\s\S]*65<small>°F<\/small>/u);
  assert.match(adminHomeHtml, /data-admin-soil-map[\s\S]*<h2 id="admin-soil-map-heading">Soil moisture<\/h2>/u);
  assert.match(adminHomeHtml, /data-soil-moisture-sensor="soil-1"[\s\S]*aria-label="Orchard soil: 42% soil moisture"[\s\S]*>42%<\/text>/u);
  assert.equal((adminHomeHtml.match(/data-soil-moisture-sensor=/gu) ?? []).length, 1);
  assert.doesNotMatch(adminHomeHtml, /data-soil-moisture-sensor="(?:gateway|temperature-1|temperature-2)"/u);
  assert.match(html, /<section class="current-conditions" aria-label="Current conditions">/u);
  assert.doesNotMatch(
    html,
    /class="panel current-panel"|id="current-heading"|class="freshness|class="provenance"|Right now|Nearby model value/u,
  );
  assert.equal((html.match(/class="condition-card /gu) ?? []).length, 10);
  assert.equal((html.match(/class="condition-color"/gu) ?? []).length, 10);
  assert.equal((html.match(/<rect width="1\.4"/gu) ?? []).length, 0);
  assert.match(html, /class="condition-card temperature-condition" data-condition="temperature"/u);
  assert.match(html, /class="condition-card wind-condition" data-condition="wind"/u);
  assert.match(html, /class="condition-card rain-condition" data-condition="rain"/u);
  assert.match(html, /class="condition-card compact-condition tide-condition" data-condition="tide"/u);
  assert.equal(html.indexOf('data-condition="wind"') < html.indexOf('data-condition="rain"'), true);
  assert.equal(
    ["rain", "clouds", "humidity", "air-quality", "pressure", "uv-index", "tide", "sunset"].every(
      // keep every requested card after its predecessor
      (condition, index, conditions) => index === 0 ||
        html.indexOf(`data-condition="${conditions[index - 1]}"`) < html.indexOf(`data-condition="${condition}"`),
    ),
    true,
  );
  assert.match(html, /Air Temp/u);
  assert.match(nearFeelsHtml, /Air Temp/u);
  assert.match(html, /Gusts/u);
  assert.match(html, /Cool outdoor conditions/u);
  assert.match(html, /Peak reading 10 mph/u);
  assert.equal((html.match(/class="condition-secondary-divider"/gu) ?? []).length, 6);
  assert.match(html, /data-condition="tide"[\s\S]*?class="condition-status condition-status-dark">[\s\S]*?<span>High<\/span>[\s\S]*?<div class="condition-primary"><strong>8\.2<small>ft<\/small><\/strong>[\s\S]*?class="condition-secondary-divider">Direction<\/span>[\s\S]*?<strong>Rising<\/strong>/u);
  assert.doesNotMatch(html, /data-condition="tide"[\s\S]*?class="condition-detail">Rising<\/p>/u);
  assert.doesNotMatch(html, /condition-forecast-heading/u);
  assert.equal((html.match(/condition-forecast-tone-green/gu) ?? []).length, 8);
  assert.equal((html.match(/condition-forecast-tone-blue/gu) ?? []).length, 1);
  assert.equal((html.match(/condition-forecast-tone-orange/gu) ?? []).length, 0);
  assert.equal((html.match(/condition-forecast-tone-yellow/gu) ?? []).length, 1);
  assert.equal((html.match(/condition-forecast-tone-neutral/gu) ?? []).length, 7);
  assert.doesNotMatch(html, /Next 24h/u);
  assert.match(html, /data-condition="temperature"[\s\S]*?Max[\s\S]*?60<small>°F[\s\S]*?Min[\s\S]*?60<small>°F[\s\S]*?Max[\s\S]*?61<small>°F[\s\S]*?Min[\s\S]*?61<small>°F/u);
  assert.match(html, /data-condition="wind"[\s\S]*?Max[\s\S]*?9 <small>mph[\s\S]*?Max[\s\S]*?16 <small>mph/u);
  assert.match(html, /data-condition="rain"[\s\S]*?Rain[\s\S]*?Accumulation[\s\S]*?0\.1<small>in[\s\S]*?Max[\s\S]*?0\.02 <small>in\/h[\s\S]*?Total[\s\S]*?0\.01 <small>in/u);
  assert.match(html, /data-condition="air-quality"[\s\S]*?Max[\s\S]*?<strong>7<\/strong>/u);
  assert.doesNotMatch(html, /data-condition="air-quality"[\s\S]*?µg\/m³/u);
  assert.match(html, /data-condition="uv-index"[\s\S]*?Max[\s\S]*?2/u);
  assert.match(html, /data-condition="pressure"[\s\S]*?Max[\s\S]*?<strong>—<\/strong>[\s\S]*?condition-forecast-label"><\/span> <strong>—<\/strong>/u);
  // fade the fresh nearby 81-percent reading into the regional 78-percent forecast
  assert.match(html, /data-condition="humidity"[\s\S]*?Max[\s\S]*?81<small>%/u);
  assert.match(renderWeatherDashboard({ ...state, forecastAdjustmentMode: "raw" }), /data-condition="humidity"[\s\S]*?Max[\s\S]*?78<small>%/u);
  assert.match(html, /data-condition="tide"[\s\S]*?Next low[\s\S]*?5:00 AM/u);
  assert.match(html, /PM2\.5 health range/u);
  assert.match(html, />Good</u);
  assert.match(html, /Minimal sun protection needed/u);
  assert.doesNotMatch(html, /-0\.4–\+0\.9 %/u);
  assert.doesNotMatch(html, /class="current-grid"|class="metric/u);
  assert.doesNotMatch(html, /<article class="condition-card[^>]+style=/u);
  assert.equal((html.match(/class="condition-status-color"/gu) ?? []).length, 10);
  assert.match(html, /data-condition="air-quality"[\s\S]*?class="condition-status condition-status-dark">[\s\S]*?fill="rgb\(0, 146, 63\)"/u);
  assert.match(settingsHtml, /class="material-symbols-rounded" aria-hidden="true">settings<\/span>/u);
  assert.match(html, /data-condition="temperature"[\s\S]*?>device_thermostat<\/span>/u);
  assert.match(html, /data-condition="wind"[\s\S]*?>air<\/span>/u);
  assert.match(html, /data-condition="tide"[\s\S]*?>water<\/span>/u);
  assert.match(html, /https:\/\/open-meteo\.com\//u);
  assert.match(html, /https:\/\/creativecommons\.org\/licenses\/by\/4\.0\//u);
  assert.match(html, /CC BY 4\.0/u);
  assert.match(html, /<footer class="credits" aria-label="Weather data credits">[\s\S]*?<details>[\s\S]*?<summary>Data sources &amp; credits<\/summary>/u);
  assert.match(html, /<\/details>\s*<p class="project-credit">Built with love by <a href="https:\/\/ballydidean\.farm" rel="noreferrer">Ballydidean Farm Sanctuary<\/a><\/p>\s*<\/footer>/u);
  assert.equal((html.match(/class="project-credit"/gu) ?? []).length, 1);
  assert.doesNotMatch(html, /Ballydídean Farm Sanctuary<\/a> project/u);
  assert.doesNotMatch(html, /<details open/u);
  assert.match(html, /The latest refresh failed/u);
  assert.match(
    loadingHtml,
    /<p class="sr-only" role="status">Refreshing weather data…<\/p>/u,
  );
  assert.doesNotMatch(
    loadingHtml,
    /<p class="notice" role="status">Refreshing weather data…<\/p>/u,
  );
  assert.equal((initialHomeHtml.match(/class="[^"]*skeleton-region/gu) ?? []).length, 1);
  assert.equal((initialHomeHtml.match(/class="condition-card [^"]*skeleton-card"/gu) ?? []).length, 10);
  assert.match(initialHomeHtml, /data-condition="sunset"[\s\S]*?Golden hour/u);
  assert.match(initialHomeHtml, /data-condition="rain"[\s\S]*?Accumulation[\s\S]*?Max[\s\S]*?Total/u);
  assert.match(initialHomeHtml, /data-condition="clouds"[\s\S]*?Clearest/u);
  const initialPressure = initialHomeHtml.match(/<article[^>]*data-condition="pressure"[\s\S]*?<\/article>/u)?.[0];
  assert.ok(initialPressure);
  assert.match(initialPressure, /Max[\s\S]*?<strong>\+0\.0<\/strong>[\s\S]*?condition-forecast-label"><\/span> <strong>00:00 <small>PM<\/small><\/strong>/u);
  assert.doesNotMatch(initialPressure, /hPa\/3h/u);
  assert.doesNotMatch(initialPressure, /Barometer|Later|>By</u);
  assert.doesNotMatch(initialHomeHtml, /condition-secondary-comparison|Overnight/u);
  assert.equal((initialHomeHtml.match(/class="forecast-chart skeleton-forecast-chart"/gu) ?? []).length, 0);
  assert.equal((initialHomeHtml.match(/class="trend-chart skeleton-trend-chart"/gu) ?? []).length, 0);
  assert.equal((initialForecastHtml.match(/class="forecast-chart skeleton-forecast-chart"/gu) ?? []).length, 9);
  assert.equal((initialForecastHtml.match(/forecast-chart-heading forecast-chart-heading-top/gu) ?? []).length, 9);
  assert.match(initialForecastHtml, /Temperature[\s\S]*Wind[\s\S]*Rain rate[\s\S]*Clouds[\s\S]*Humidity[\s\S]*Air quality[\s\S]*UV index[\s\S]*Pressure[\s\S]*Tide/u);
  assert.doesNotMatch(initialForecastHtml, /forecast-weather-map|skeleton-forecast-map|\/maps\/xweather|Weather maps by Xweather/u);
  assert.equal((initialForecastHtml.match(/class="forecast-x-tick"/gu) ?? []).length, 24);
  assert.match(initialForecastHtml, /data-forecast-light="night"/u);
  assert.match(initialForecastHtml, /data-forecast-light="day"/u);
  assert.equal((initialTrendsHtml.match(/class="trend-chart skeleton-trend-chart"/gu) ?? []).length, 1);
  assert.match(initialTrendsHtml, /class="trend-metric-control trend-metric-control-skeleton">[\s\S]*?<h2 class="trend-chart-title">Temperature<\/h2>/u);
  assert.doesNotMatch(initialTrendsHtml, /data-trend-metric-control|data-trend-metric-option/u);
  assert.doesNotMatch(initialTrendsHtml, /data-trend-mode-toggle|>Show all<\/button>/u);
  assert.doesNotMatch(initialHomeHtml, /station-map|skeleton-map/u);
  assert.equal((initialMapHtml.match(/class="[^"]*skeleton-region/gu) ?? []).length, 2);
  assert.equal((initialSettingsHtml.match(/class="[^"]*skeleton-region/gu) ?? []).length, 0);
  assert.match(initialMapHtml, /class="station-map skeleton-map"/u);
  assert.doesNotMatch(initialHomeHtml, /No current model value|being collected|No normalized trend buckets/u);
  assert.equal((initialLogsHtml.match(/class="skeleton-history-row"/gu) ?? []).length, 25);
  assert.equal((initialLogsHtml.match(/class="history-card skeleton-history-card"/gu) ?? []).length, 25);
  assert.doesNotMatch(initialLogsHtml, /No records match these filters/u);
  assert.doesNotMatch(html, /skeleton-region|skeleton-history-row|skeleton-history-card/u);
  assert.match(html, /data-condition="temperature"[\s\S]*?<div class="condition-primary"><strong>54<small>°F<\/small>/u);
  assert.match(html, /Air Temp[\s\S]*?<strong>54<small>°F<\/small>/u);
  assert.match(html, /data-condition="wind"[\s\S]*?<div class="condition-primary"><strong>6<small>mph SW<\/small>/u);
  assert.match(html, /Gusts[\s\S]*?<strong>10<small>mph<\/small>/u);
  assert.match(html, /data-condition="air-quality"[\s\S]*?<div class="condition-primary"><strong>7<\/strong>/u);
  const pressure = html.match(/<article[^>]*data-condition="pressure"[\s\S]*?<\/article>/u)?.[0];
  assert.ok(pressure);
  assert.match(pressure, /<div class="condition-primary"><strong>\+1\.2<\/strong>/u);
  assert.doesNotMatch(pressure, /hPa\/3h/u);
  assert.doesNotMatch(pressure, /condition-secondary|Barometer/u);
  assert.match(firstPartyHtml, /data-condition="temperature"[\s\S]*?<div class="condition-primary"><strong>49<small>°F<\/small>/u);
  assert.match(firstPartyHtml, /data-condition="wind"[\s\S]*?<div class="condition-primary"><strong>2<small>mph SW<\/small>/u);
  const selectedHtml = renderWeatherDashboard({
    ...state,
    selectedStationSlug: "tempest-38270",
  }, "map");
  assert.match(selectedHtml, /data-station-current="tempest-38270"/u);
  assert.match(selectedHtml, /Current conditions for Fiske Rd &amp; Paris Pl/u);
  assert.match(selectedHtml, /<strong>Temp<\/strong> 53\.6 °F/u);
  assert.match(selectedHtml, /<strong>Wind<\/strong> 5\.6 mph · gust 10\.1 mph/u);
  assert.match(selectedHtml, /<strong>Platform<\/strong> WeatherFlow Tempest/u);
  assert.match(selectedHtml, /Station reading is current/u);
  assert.match(logsHtml, /class="section-nav-settings" href="\/settings" data-weather-route aria-current="page"/u);
  assert.doesNotMatch(logsHtml, /section-nav-logs/u);
  assert.doesNotMatch(logsHtml, /id="current-heading"/u);
  assert.match(logsHtml, /name="stationSlug"/u);
  assert.match(logsHtml, /name="sourceId"/u);
  assert.match(logsHtml, /name="sourceKind"/u);
  assert.match(logsHtml, /<th scope="col">Temperature \(°F\)<\/th>/u);
  assert.match(logsHtml, /<th scope="col">Wind \(mph\)<\/th>/u);
  assert.match(logsHtml, /<th scope="col">Precipitation \(in\)<\/th>/u);
  assert.match(logsHtml, /<th scope="col">Source and provenance<\/th>/u);
  assert.match(logsHtml, /aria-label="History pages"/u);
  assert.equal(
    logsHtml.indexOf('class="credits"') > logsHtml.indexOf('id="history-heading"'),
    true,
  );
});

// keep apparent temperature primary without masking unavailable readings
test("temperature tile leads with feels-like comfort and retains independent air temperature", () => {
  // cover different comfort bands, genuine zero, and missing measurements
  for (const [apparentTemperatureC, temperatureC, expectedPrimary, expectedAir, expectedStatus] of [
    [0, 20, "0", "20", "Freezing"],
    [null, 20, "—", "20", "Unavailable"],
    [20, null, "20", "—", "Comfortable"],
  ]) {
    const state = {
      ...forecastState([], null),
      current: [{ ...record, metrics: { ...record.metrics, apparentTemperatureC, temperatureC } }],
      units: { ...DEFAULT_UNIT_PREFERENCES, temperature: "celsius" },
    };
    const tile = renderWeatherDashboard(state).match(/<article[^>]*data-condition="temperature"[\s\S]*?<\/article>/u)?.[0];
    const primary = tile.match(/class="condition-primary"><strong>([^<]+)/u)?.[1];
    const secondary = tile.match(/class="condition-secondary">[\s\S]*?<strong>([^<]+)/u)?.[1];
    assert.equal(primary, expectedPrimary);
    assert.equal(secondary, expectedAir);
    assert.ok(tile.includes(`<span>${expectedStatus}</span>`));
    assert.match(tile, /class="condition-secondary-divider">Air Temp<\/span>/u);
    assert.doesNotMatch(tile, /Feels like/u);
  }
});

test("measurement formatting uses US consumer defaults and supports every configured alternative", () => {
  assert.deepEqual(
    formatMeasurement(16.2, "temperature", DEFAULT_UNIT_PREFERENCES),
    { unit: "°F", value: "61.2" },
  );
  assert.deepEqual(
    formatMeasurement(4.1, "windSpeed", DEFAULT_UNIT_PREFERENCES),
    { unit: "mph", value: "9.2" },
  );
  assert.deepEqual(
    formatMeasurement(0.2, "precipitation", DEFAULT_UNIT_PREFERENCES),
    { unit: "in", value: "0.01" },
  );
  assert.deepEqual(
    formatMeasurement(1014.2, "pressure", DEFAULT_UNIT_PREFERENCES),
    { unit: "%", value: "+0.1" },
  );
  assert.deepEqual(
    formatMeasurement(16.2, "temperature", DEFAULT_UNIT_PREFERENCES, 0),
    { unit: "°F", value: "61" },
  );
  assert.deepEqual(
    formatMeasurement(4.1, "windSpeed", DEFAULT_UNIT_PREFERENCES, 0),
    { unit: "mph", value: "9" },
  );
  assert.deepEqual(
    formatMeasurement(1014.2, "pressure", DEFAULT_UNIT_PREFERENCES, 0),
    { unit: "%", value: "+0.1" },
  );

  const inchesPressurePreferences = {
    ...DEFAULT_UNIT_PREFERENCES,
    pressure: "inches_of_mercury",
  };
  assert.deepEqual(
    formatMeasurement(1014.2, "pressure", inchesPressurePreferences),
    { unit: "inHg", value: "29.9" },
  );

  const metricPreferences = {
    precipitation: "millimeters",
    pressure: "hectopascals",
    temperature: "celsius",
    waterLevel: "meters",
    windSpeed: "kilometers_per_hour",
  };
  assert.deepEqual(
    formatMeasurement(16.2, "temperature", metricPreferences),
    { unit: "°C", value: "16.2" },
  );
  assert.deepEqual(
    formatMeasurement(4.1, "windSpeed", metricPreferences),
    { unit: "km/h", value: "14.8" },
  );
  assert.deepEqual(
    formatMeasurement(0.2, "precipitation", metricPreferences),
    { unit: "mm", value: "0.2" },
  );
  assert.deepEqual(
    formatMeasurement(1014.2, "pressure", metricPreferences),
    { unit: "hPa", value: "1,014.2" },
  );
  assert.deepEqual(
    formatMeasurement(2.5, "waterLevel", DEFAULT_UNIT_PREFERENCES),
    { unit: "ft", value: "8.2" },
  );
  assert.deepEqual(
    formatMeasurement(2.5, "waterLevel", metricPreferences),
    { unit: "m", value: "2.5" },
  );
});

test("forecast selector timestamps interpolate continuously between hourly samples", () => {
  const times = [
    "2026-08-21T07:00:00.000Z",
    "2026-08-21T08:00:00.000Z",
  ];
  assert.equal(interpolateForecastInstant(times, 0.25), "2026-08-21T07:15:00.000Z");
  assert.equal(interpolateForecastInstant(times, 0.5), "2026-08-21T07:30:00.000Z");
  assert.equal(interpolateForecastInstant(times, 0.75), "2026-08-21T07:45:00.000Z");
});

// control whether sparse chart endpoints may extend into gaps
test("forecast chart interpolation preserves strict pressure gaps", () => {
  assert.equal(interpolateForecastValue([1, null], 0.5), 1);
  assert.equal(interpolateForecastValue([1, null], 0.5, false), null);
  assert.equal(interpolateForecastValue([null, -2], 0.5, false), null);
  assert.equal(interpolateForecastValue([1, 3], 0.5, false), 2);
  assert.equal(interpolateForecastValue([1, null], 0, false), 1);
});

test("daily forecasts use the site calendar", () => {
  const records = [
    { ...forecastRecord, id: "before", validAt: "2026-08-21T06:59:59.000Z" },
    { ...forecastRecord, id: "start", validAt: "2026-08-21T07:00:00.000Z" },
    { ...forecastRecord, id: "end", validAt: "2026-08-22T06:59:59.000Z" },
    { ...forecastRecord, id: "after", validAt: "2026-08-22T07:00:00.000Z" },
  ];
  assert.deepEqual(
    forecastForSiteDay(
      records,
      "2026-08-22T04:50:00.000Z",
      "America/Los_Angeles",
    ).map((entry) => entry.id),
    ["start", "end"],
  );
  assert.deepEqual(
    forecastForSiteDays(
      [
        ...records,
        { ...forecastRecord, id: "day-five", validAt: "2026-08-25T07:00:00.000Z" },
        { ...forecastRecord, id: "day-six", validAt: "2026-08-26T07:00:00.000Z" },
      ],
      "2026-08-22T04:50:00.000Z",
      "America/Los_Angeles",
      5,
    ).map((entry) => entry.id),
    ["start", "end", "after", "day-five"],
  );
});

// align rolling pressure changes to complete forecast windows
test("forecast pressure changes require exact same-source hourly windows", () => {
  const records = [
    pressureForecastRecord("2026-09-12T15:00:00Z", 1_000),
    pressureForecastRecord("2026-09-12T16:00:00Z", 1_001),
    pressureForecastRecord("2026-09-12T17:00:00Z", 1_002),
    pressureForecastRecord("2026-09-12T18:00:00Z", 1_004),
    pressureForecastRecord("2026-09-12T19:00:00Z", 1_005),
  ];
  assert.deepEqual(forecastPressureChanges(records), [null, null, null, 4, 4]);
  assert.deepEqual(forecastPressureChanges(records, records.slice(3)), [4, 4]);

  const withGap = records.map(
    // move one required sample off the hourly boundary
    (entry, index) => index === 2
      ? { ...entry, validAt: "2026-09-12T17:30:00Z" }
      : entry,
  );
  assert.deepEqual(forecastPressureChanges(withGap), [null, null, null, null, null]);

  const mixedSource = records.map(
    // replace one required sample with another provider
    (entry, index) => index === 2
      ? pressureForecastRecord(entry.validAt, entry.metrics.pressureHpa, { source: "other-source" })
      : entry,
  );
  assert.deepEqual(forecastPressureChanges(mixedSource), [null, null, null, null, null]);

  const mixedRun = records.map(
    // replace one required sample with another model run
    (entry, index) => index === 2
      ? { ...entry, productRunAt: "2026-09-12T14:30:00Z" }
      : entry,
  );
  assert.deepEqual(forecastPressureChanges(mixedRun), [null, null, null, null, null]);

  const nonfinite = records.map(
    // invalidate one required numeric pressure sample
    (entry, index) => index === 2
      ? { ...entry, metrics: { ...entry.metrics, pressureHpa: Number.NaN } }
      : entry,
  );
  assert.deepEqual(forecastPressureChanges(nonfinite), [null, null, null, null, null]);

  const observed = records.map(
    // replace the entire forecast run with observations
    (entry) => ({
      ...entry,
      productRunAt: null,
      provenance: { ...entry.provenance, sourceKind: "model_current" },
    }),
  );
  assert.deepEqual(forecastPressureChanges(observed), [null, null, null, null, null]);
});

// use elapsed epoch hours across repeated daylight-saving wall clocks
test("forecast pressure changes remain hourly across daylight-saving transitions", () => {
  const fallback = [
    pressureForecastRecord("2026-11-01T07:00:00Z", 1_000),
    pressureForecastRecord("2026-11-01T08:00:00Z", 1_001),
    pressureForecastRecord("2026-11-01T09:00:00Z", 1_003),
    pressureForecastRecord("2026-11-01T10:00:00Z", 1_006),
  ];
  assert.deepEqual(forecastPressureChanges(fallback), [null, null, null, 6]);
});

// fill midnight pressure changes without joining different model vintages
test("forecast pressure context fills only the opening windows from one retained run", () => {
  // exercise ordinary days and both daylight-saving transitions
  for (const midnight of ["2026-08-21T07:00:00Z", "2026-03-08T08:00:00Z", "2026-11-01T07:00:00Z"]) {
    const base = Date.parse(midnight);
    const pressureContext = [1_000, 1_001, 1_002, 1_004, 1_007, 1_009].map(
      // retain yesterday and the first three target hours in the same older run
      (value, index) => pressureForecastRecord(new Date(base + (index - 3) * 3_600_000).toISOString(), value, {
        productRunAt: new Date(base - 3_600_000).toISOString(),
      }),
    );
    const forecast = [1_100, 1_101, 1_102, 1_105, 1_107, 1_110].map(
      // make cross-vintage subtraction observably incorrect
      (value, index) => pressureForecastRecord(new Date(base + index * 3_600_000).toISOString(), value, {
        productRunAt: new Date(base + 4 * 3_600_000).toISOString(),
      }),
    );
    const parsed = parseForecastRecordsResponse({ data: forecast, pressureContext, site });
    assert.deepEqual(parsed.data, forecast);
    assert.deepEqual(parsed.pressureContext, pressureContext);
    const state = {
      ...forecastState(parsed.data, null),
      current: [{ ...record, validAt: midnight }],
      forecastPressureContext: parsed.pressureContext,
    };
    const html = renderWeatherDashboard(state, "forecast");
    assert.deepEqual(forecastChartSeries(html, "pressure")[0].values, [4, 6, 7, 5, 6, 8]);
    assert.equal(forecastChartSeries(html, "temperature")[0].values.length, 6);
    assert.equal((html.match(/class="forecast-x-tick"/gu) ?? []).length, 6);

    const foreignContext = pressureContext.map(
      // reject another station even when its times line up
      (hour) => ({ ...hour, provenance: { ...hour.provenance, sourceId: "other-source" } }),
    );
    const foreignHtml = renderWeatherDashboard({ ...state, forecastPressureContext: foreignContext }, "forecast");
    assert.deepEqual(forecastChartSeries(foreignHtml, "pressure")[0].values, [null, null, null, 5, 6, 8]);

    // discard malformed optional context without losing normal forecast data
    for (const invalid of [undefined, [], pressureContext.slice(1), [
      ...pressureContext.slice(0, 5), { ...pressureContext[5], productRunAt: forecast[0].productRunAt },
    ], [
      ...pressureContext.slice(0, 5), { ...pressureContext[5], metrics: { ...pressureContext[5].metrics, pressureHpa: null } },
    ]]) {
      const fallback = parseForecastRecordsResponse({ data: forecast, pressureContext: invalid, site });
      assert.deepEqual(fallback.pressureContext, []);
      assert.deepEqual(fallback.data, forecast);
    }
  }
});

// keep unavailable rolling windows as visible breaks in the pressure chart
test("pressure forecast chart separates gaps and keeps gradient colors time-aligned", () => {
  const pressureValues = [1_000, 1_001, 1_002, 1_003, 1_004, null, 1_006, 1_007, 1_008, 1_014, 1_016];
  const forecast = pressureValues.map(
    // create one hourly run with a missing pressure sample between valid windows
    (pressureHpa, index) => pressureForecastRecord(
      new Date(Date.parse("2026-09-12T15:00:00Z") + index * 3_600_000).toISOString(),
      pressureHpa,
    ),
  );
  const html = renderWeatherDashboard({
    ...new WeatherDashboardController({ storage: null }).state,
    current: [{ ...record, validAt: forecast[0].validAt }],
    forecast,
    loading: false,
    selectedSite: site,
  }, "forecast");
  const pressure = forecastChartHtml(html, "pressure");
  assert.deepEqual(forecastChartSeries(html, "pressure"), [{
    label: "3h change",
    values: [null, null, null, 3, 3, null, null, null, null, 8, 9],
  }]);
  assert.match(pressure, /data-forecast-min="-9"[\s\S]*data-forecast-max="9"/u);
  const segments = [...pressure.matchAll(/<polyline points="([^"]*)" class="forecast-chart-line forecast-chart-line-0"/gu)];
  assert.equal(segments.length, 2);
  assert.match(segments[0]?.[1] ?? "", /^196\.36,[\d.]+ 261\.82,[\d.]+$/u);
  assert.match(segments[1]?.[1] ?? "", /^589\.09,[\d.]+ 654\.55,[\d.]+ 720\.00,[\d.]+$/u);
  assert.match(pressure, /<linearGradient id="forecast-line-pressure-0" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="720" y2="0">/u);
  assert.match(pressure, /offset="27\.273%" stop-color="rgb\(230, 181, 25\)"/u);
  assert.match(pressure, /offset="81\.818%" stop-color="rgb\(207, 67, 55\)"/u);
});

// select the earliest strongest complete same-run pressure window
test("strongest pressure change resolves equal magnitudes by earliest ending time", () => {
  const records = [
    pressureForecastRecord("2026-09-12T16:00:00Z", 1_000),
    pressureForecastRecord("2026-09-12T17:00:00Z", 1_001),
    pressureForecastRecord("2026-09-12T18:00:00Z", 1_002),
    pressureForecastRecord("2026-09-12T19:00:00Z", 1_003),
    pressureForecastRecord("2026-09-12T20:00:00Z", 1_004),
  ];
  assert.deepEqual(
    strongestPressureChange(records, new Date("2026-09-12T16:00:00Z"), site.timezone),
    { changeHpa: 3, validAt: "2026-09-12T19:00:00Z" },
  );

  const falling = [
    pressureForecastRecord("2026-09-12T16:00:00Z", 1_010, { source: "falling" }),
    pressureForecastRecord("2026-09-12T17:00:00Z", 1_009, { source: "falling" }),
    pressureForecastRecord("2026-09-12T18:00:00Z", 1_007, { source: "falling" }),
    pressureForecastRecord("2026-09-12T19:00:00Z", 1_004, { source: "falling" }),
  ];
  assert.deepEqual(
    strongestPressureChange([...records, ...falling], new Date("2026-09-12T16:00:00Z"), site.timezone),
    { changeHpa: -6, validAt: "2026-09-12T19:00:00Z" },
  );
});

// reject incomplete or mixed-source forecast windows
test("strongest pressure change requires every hourly value from one source and run", () => {
  const gap = [
    pressureForecastRecord("2026-09-12T16:00:00Z", 1_000),
    pressureForecastRecord("2026-09-12T17:00:00Z", 1_001),
    pressureForecastRecord("2026-09-12T19:00:00Z", 1_004),
    pressureForecastRecord("2026-09-12T20:00:00Z", 1_005),
  ];
  assert.equal(strongestPressureChange(gap, new Date("2026-09-12T15:30:00Z"), site.timezone), null);

  const mixedSource = [
    pressureForecastRecord("2026-09-12T16:00:00Z", 1_000, { source: "a" }),
    pressureForecastRecord("2026-09-12T17:00:00Z", 1_001, { source: "a" }),
    pressureForecastRecord("2026-09-12T18:00:00Z", 1_002, { source: "b" }),
    pressureForecastRecord("2026-09-12T19:00:00Z", 1_004, { source: "a" }),
  ];
  assert.equal(strongestPressureChange(mixedSource, new Date("2026-09-12T15:30:00Z"), site.timezone), null);

  const mixedRun = [
    pressureForecastRecord("2026-09-12T16:00:00Z", 1_000),
    pressureForecastRecord("2026-09-12T17:00:00Z", 1_001),
    pressureForecastRecord("2026-09-12T18:00:00Z", 1_002, { productRunAt: "2026-09-12T15:00:00Z" }),
    pressureForecastRecord("2026-09-12T19:00:00Z", 1_004),
  ];
  assert.equal(strongestPressureChange(mixedRun, new Date("2026-09-12T15:30:00Z"), site.timezone), null);
});

// search the full current farm day while excluding next-day windows
test("strongest pressure change includes past windows but excludes next-day windows", () => {
  const past = [
    pressureForecastRecord("2026-09-12T15:00:00Z", 1_000),
    pressureForecastRecord("2026-09-12T16:00:00Z", 1_001),
    pressureForecastRecord("2026-09-12T17:00:00Z", 1_002),
    pressureForecastRecord("2026-09-12T18:00:00Z", 1_006),
  ];
  assert.deepEqual(
    strongestPressureChange(past, new Date("2026-09-12T20:00:00Z"), site.timezone),
    { changeHpa: 6, validAt: "2026-09-12T18:00:00Z" },
  );

  const endingAtMidnight = [
    pressureForecastRecord("2026-09-13T04:00:00Z", 1_000),
    pressureForecastRecord("2026-09-13T05:00:00Z", 1_001),
    pressureForecastRecord("2026-09-13T06:00:00Z", 1_002),
    pressureForecastRecord("2026-09-13T07:00:00Z", 1_006),
  ];
  assert.equal(
    strongestPressureChange(endingAtMidnight, new Date("2026-09-12T15:30:00Z"), site.timezone),
    null,
  );
});

// classify rounded three-hour pressure movement symmetrically by direction
test("pressure change bands follow meteorological speed thresholds", () => {
  assert.deepEqual(
    [0.4, 0.5, 1.9, 2, 3.4, 3.5, 5.9, 6].map(
      // collect every rising threshold boundary
      (value) => pressureChangeBand(value).label,
    ),
    ["Steady", "Slow rise", "Slow rise", "Rising", "Rising", "Rapid rise", "Rapid rise", "Very rapid rise"],
  );
  assert.deepEqual(
    [-0.4, -0.5, -1.9, -2, -3.4, -3.5, -5.9, -6].map(
      // collect every falling threshold boundary
      (value) => pressureChangeBand(value).label,
    ),
    ["Steady", "Slow fall", "Slow fall", "Falling", "Falling", "Rapid fall", "Rapid fall", "Very rapid fall"],
  );
  assert.deepEqual(
    [0.4, 0.5, 2, 3.5, 6].map(
      // compare colors independent of direction
      (value) => pressureChangeBand(value).color,
    ),
    [
      "rgb(0, 146, 63)",
      "rgb(200, 183, 68)",
      "rgb(230, 181, 25)",
      "rgb(239, 126, 31)",
      "rgb(207, 67, 55)",
    ],
  );
  assert.deepEqual(
    [0.4, 0.5, 2, 3.5, 6].map(
      // collect matching falling colors
      (value) => pressureChangeBand(-value).color,
    ),
    [
      "rgb(0, 146, 63)",
      "rgb(200, 183, 68)",
      "rgb(230, 181, 25)",
      "rgb(239, 126, 31)",
      "rgb(207, 67, 55)",
    ],
  );
  assert.equal(pressureChangeBand(-0).label, "Steady");
  assert.equal(pressureChangeBand(0.49).label, "Slow rise");
  assert.equal(pressureChangeBand(1.96).label, "Rising");
  assert.equal(pressureChangeBand(3.46).label, "Rapid rise");
  assert.equal(pressureChangeBand(5.96).label, "Very rapid rise");

  // preserve every unavailable numeric form
  for (const value of [null, Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
    assert.deepEqual(pressureChangeBand(value), {
      color: "rgb(136, 136, 130)",
      detail: "Three-hour pressure history unavailable",
      label: "Unavailable",
    });
  }
});

// render observed movement separately from the strongest daily forecast
test("pressure tile shows signed tendency and the strongest daily change", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-12T15:30:00Z") });
  const current = {
    ...record,
    pressureChange3hHpa: -2.25,
    validAt: "2026-09-12T15:30:00Z",
  };
  const forecast = [
    pressureForecastRecord("2026-09-12T16:00:00Z", 1_000),
    pressureForecastRecord("2026-09-12T17:00:00Z", 1_001),
    pressureForecastRecord("2026-09-12T18:00:00Z", 1_002),
    pressureForecastRecord("2026-09-12T19:00:00Z", 1_004),
  ];
  const tile = renderPressureTile([current], forecast);
  assert.match(tile, /class="condition-status condition-status-dark">[\s\S]*?<span>Falling<\/span>/u);
  assert.match(tile, /class="condition-primary"><strong>-2\.3<\/strong>/u);
  assert.match(tile, /condition-forecast-reading condition-forecast-tone-orange"><span class="condition-forecast-label">Max<\/span> <strong>\+4\.0<\/strong>/u);
  assert.match(tile, /condition-forecast-reading condition-forecast-tone-neutral"><span class="condition-forecast-label"><\/span> <strong>12:00 <small>PM<\/small><\/strong>/u);
  assert.doesNotMatch(tile, /hPa\/3h|condition-secondary|Barometer|Later|>By</u);
});

// format the strongest window end in the selected site's local time
test("pressure tile formats the maximum ending time in the site timezone", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-12T15:30:00Z") });
  const foreignSite = { ...site, latitude: 1.8721, longitude: -157.4278, timezone: "Pacific/Kiritimati" };
  const forecast = [
    pressureForecastRecord("2026-09-12T16:00:00Z", 1_000),
    pressureForecastRecord("2026-09-12T17:00:00Z", 1_001),
    pressureForecastRecord("2026-09-12T18:00:00Z", 1_002),
    pressureForecastRecord("2026-09-12T19:00:00Z", 1_004),
  ];
  const tile = renderPressureTile([{ ...record, pressureChange3hHpa: 1 }], forecast, foreignSite);
  assert.match(tile, /condition-forecast-label"><\/span> <strong>9:00 <small>AM<\/small><\/strong>/u);
});

// keep the observed tendency attached to the selected pressure source
test("pressure tile does not mix rates across preferred current records", () => {
  const preferred = {
    ...ecowittRecord,
    metrics: { ...ecowittRecord.metrics, pressureHpa: 1_008.4 },
    pressureChange3hHpa: null,
  };
  const fallback = {
    ...record,
    pressureChange3hHpa: 4,
  };
  const tile = renderPressureTile([fallback, preferred]);
  assert.match(tile, /class="condition-primary"><strong>—<\/strong>/u);
  assert.doesNotMatch(tile, /class="condition-primary">[\s\S]*?\+4\.0/u);
  assert.doesNotMatch(tile, /condition-secondary|Barometer/u);

  const withoutPressure = {
    ...preferred,
    metrics: { ...preferred.metrics, pressureHpa: null },
    pressureChange3hHpa: 9,
  };
  const modelTile = renderPressureTile([fallback, withoutPressure]);
  assert.match(modelTile, /class="condition-primary"><strong>\+4\.0<\/strong>/u);
  assert.doesNotMatch(modelTile, /condition-secondary|Barometer/u);
});

// suppress unavailable freshness states without converting them to zero
test("pressure tile hides delayed, stale and missing observed rates", () => {
  // cover every non-fresh status independently
  for (const status of ["delayed", "stale"]) {
    const current = {
      ...record,
      freshness: { ...record.freshness, status },
      pressureChange3hHpa: 1.2,
    };
    const tile = renderPressureTile([current]);
    assert.match(tile, /class="condition-primary"><strong>—<\/strong>/u);
    assert.doesNotMatch(tile, /class="condition-primary">[\s\S]*?(?:0\.0|\+1\.2)/u);
  }

  const missing = renderPressureTile([{ ...record, pressureChange3hHpa: null }]);
  assert.match(missing, /class="condition-primary"><strong>—<\/strong>/u);
  assert.match(missing, /condition-forecast-label">Max<\/span> <strong>—<\/strong>[\s\S]*?condition-forecast-label"><\/span> <strong>—<\/strong>/u);
  assert.doesNotMatch(missing, /class="condition-primary">[\s\S]*?0\.0/u);
});

// preserve genuine zero while suppressing negative-zero formatting
test("pressure tile renders negative zero as steady zero", () => {
  const tile = renderPressureTile([{ ...record, pressureChange3hHpa: -0 }]);
  assert.match(tile, /class="condition-status condition-status-dark">[\s\S]*?<span>Steady<\/span>/u);
  assert.match(tile, /class="condition-primary"><strong>0\.0<\/strong>/u);
  assert.doesNotMatch(tile, /-0\.0/u);
});

// keep modeled daily change on the right without backfilling the observation
test("pressure tile never derives the observed rate from the forecast", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-09-12T15:30:00Z") });
  const forecast = [
    pressureForecastRecord("2026-09-12T16:00:00Z", 1_000),
    pressureForecastRecord("2026-09-12T17:00:00Z", 1_001),
    pressureForecastRecord("2026-09-12T18:00:00Z", 1_002),
    pressureForecastRecord("2026-09-12T19:00:00Z", 1_004),
  ];
  const tile = renderPressureTile([{ ...record, pressureChange3hHpa: null }], forecast);
  assert.match(tile, /class="condition-primary"><strong>—<\/strong>/u);
  assert.match(tile, /condition-forecast-label">Max<\/span> <strong>\+4\.0<\/strong>/u);
  assert.match(tile, /condition-forecast-label"><\/span> <strong>12:00 <small>PM<\/small><\/strong>/u);
});

// keep the cold text cutoff independent of display units and rounding
test("temperature text turns blue below 55F across display units", () => {
  // retain warm, hot and unavailable bands around the moved cold boundary
  for (const [valueF, label, tone] of [
    [32, "Freezing", "blue"],
    [49.9, "Chilly", "blue"],
    [50, "Chilly", "blue"],
    [54.9, "Chilly", "blue"],
    [55, "Cool", "green"],
    [55.1, "Cool", "green"],
    [60, "Comfortable", "green"],
    [70, "Comfortable", "green"],
    [70.1, "Warm", "orange"],
    [80, "Warm", "orange"],
    [80.1, "Hot", "red"],
    [null, "Unavailable", "neutral"],
  ]) {
    const temperatureC = valueF === null ? null : (valueF - 32) * 5 / 9;
    assert.equal(temperatureBand(temperatureC).label, label);
    const forecast = {
      ...forecastRecord,
      metrics: { ...forecastRecord.metrics, temperatureC, apparentTemperatureC: temperatureC },
    };

    // use the unrounded celsius reading with either unit preference
    for (const temperature of ["fahrenheit", "celsius"]) {
      const state = {
        ...forecastState([forecast], null),
        units: { ...DEFAULT_UNIT_PREFERENCES, temperature },
      };
      const tile = renderWeatherDashboard(state).match(/<article[^>]*data-condition="temperature"[\s\S]*?<\/article>/u)?.[0];
      assert.ok(tile);
      const tones = [...tile.matchAll(/condition-forecast-tone-([a-z]+)/gu)].map(
        // collect the apparent and air temperature extrema
        (match) => match[1],
      );
      assert.deepEqual(tones, [tone, tone, tone, tone], `${String(valueF)}F in ${temperature}`);
    }
  }
});

// color the selected adjusted value without changing the raw forecast
test("adjusted temperature crosses the shared 55F blue cutoff", () => {
  const raw = {
    ...forecastRecord,
    metrics: { ...forecastRecord.metrics, temperatureC: 14, apparentTemperatureC: 14 },
  };
  const corrected = {
    ...raw,
    temperatureAdjustment: { ...temperatureCanaryDecision(raw), correctedTemperatureC: 12 },
  };

  // keep the warm raw value green and the corrected 53.6f value blue
  for (const [mode, expectedAirTone] of [["raw", "green"], ["adjusted", "blue"]]) {
    const state = {
      ...forecastState([corrected], null),
      forecastAdjustmentMode: mode,
      forecastTemperatureAdjustmentRuntime: temperatureCanaryRuntime(),
    };
    const tile = renderWeatherDashboard(state).match(/<article[^>]*data-condition="temperature"[\s\S]*?<\/article>/u)?.[0];
    assert.ok(tile);
    const tones = [...tile.matchAll(/condition-forecast-tone-([a-z]+)/gu)].map(
      // retain independent apparent-temperature colors
      (match) => match[1],
    );
    assert.deepEqual(tones, ["green", "green", expectedAirTone, expectedAirTone]);
  }
});

// verify the published current-condition threshold boundaries
test("current condition bands follow requested weather and health thresholds", () => {
  assert.deepEqual(
    [0, 10, 10.1, 50, 50.1].map(
      // collect every cloud-cover boundary
      (value) => cloudBand(value),
    ),
    [
      { color: "rgb(105, 133, 155)", detail: "", label: "Clear" },
      { color: "rgb(105, 133, 155)", detail: "", label: "Clear" },
      { color: "rgb(105, 133, 155)", detail: "", label: "Light" },
      { color: "rgb(105, 133, 155)", detail: "", label: "Light" },
      { color: "rgb(105, 133, 155)", detail: "", label: "Heavy" },
    ],
  );
  assert.deepEqual(cloudBand(null), { color: "rgb(136, 136, 130)", detail: "", label: "Unavailable" });
  assert.equal(temperatureBand(0).color, "rgb(56, 120, 197)");
  assert.equal(temperatureBand((60 - 32) * 5 / 9).color, "rgb(67, 151, 86)");
  assert.equal(temperatureBand((80 - 32) * 5 / 9).color, "rgb(207, 67, 55)");
  assert.notEqual(temperatureBand(10).color, temperatureBand(0).color);
  assert.equal(windBand(10 / 2.236_936_292_1, null).color, "rgb(67, 151, 86)");
  assert.equal(windBand(null, 50 / 2.236_936_292_1).color, "rgb(207, 67, 55)");
  assert.equal(windBand(null, 51 / 2.236_936_292_1).label, "Dangerous");
  assert.equal(
    windBand(4.1, 7.2, {
      precipitation: "millimeters",
      pressure: "hectopascals",
      temperature: "celsius",
      waterLevel: "meters",
      windSpeed: "meters_per_second",
    }).detail,
    "Peak reading 7 m/s",
  );
  assert.deepEqual(
    [9, 9.1, 35.5, 55.5, 125.5, 225.5].map(
      // collect every EPA particulate band
      (value) => airQualityBand(value).label,
    ),
    ["Good", "Moderate", "Sensitive groups", "Unhealthy", "Very unhealthy", "Hazardous"],
  );
  assert.deepEqual(
    [2, 3, 6, 8, 11].map(
      // collect every EPA UV band
      (value) => uvBand(value).label,
    ),
    ["Low", "Moderate", "High", "Very high", "Extreme"],
  );
  assert.equal(pressureBand(1_008.9).label, "Low");
  assert.equal(pressureBand(1_014.2).label, "Normal");
  assert.equal(pressureBand(1_022.8).label, "High");
  assert.equal(
    pressureBand(1_014.2, {
      precipitation: "millimeters",
      pressure: "hectopascals",
      temperature: "celsius",
      waterLevel: "meters",
      windSpeed: "meters_per_second",
    }).detail,
    "1,009.0–1,022.7 hPa",
  );
  assert.deepEqual(
    [29, 30, 60, 61, 71, 81].map(
      // collect every humidity comfort band
      (value) => humidityBand(value, 30).color,
    ),
    [
      "rgb(67, 151, 86)",
      "rgb(67, 151, 86)",
      "rgb(67, 151, 86)",
      "rgb(230, 181, 25)",
      "rgb(239, 126, 31)",
      "rgb(207, 67, 55)",
    ],
  );
  assert.equal(tideLevelLabel(tides[2], tides), "High");
  assert.equal(tideLevelLabel({ ...tides[2], waterLevelM: 1.8 }, tides), "Medium");
  assert.equal(tideLevelLabel({ ...tides[2], waterLevelM: 0.7 }, tides), "Low");
});

// reserve humidity discomfort colors for hot weather
test("humidity comfort requires hot air rather than relative humidity alone", () => {
  // keep cold saturated air and dry hot air out of warning bands
  for (const [humidity, temperature] of [[100, 0], [95, 16], [100, 26], [20, 35], [60, 35]]) {
    assert.equal(humidityBand(humidity, temperature).color, "rgb(67, 151, 86)");
    assert.equal(humidityBand(humidity, temperature).label, "Comfortable");
  }
  const hotThresholdC = (80 - 32) * 5 / 9;
  assert.equal(humidityBand(95, hotThresholdC - 0.000_001).color, "rgb(67, 151, 86)");
  assert.equal(humidityBand(95, hotThresholdC).color, "rgb(207, 67, 55)");
  // distinguish missing or invalid inputs from known comfortable conditions
  for (const [humidity, temperature] of [[null, 30], [95, null], [NaN, 30], [95, Infinity], [-1, 30], [101, 30]]) {
    assert.equal(humidityBand(humidity, temperature).color, "rgb(136, 136, 130)");
    assert.equal(humidityBand(humidity, temperature).label, "Unavailable");
  }
});

// classify humidity using the same resolved air temperature displayed on the homepage
test("humidity card uses observed air temperature with the current fallback for missing air temperature", () => {
  // reject apparent-temperature substitutions while preserving regional fallback
  for (const [temperatureC, apparentTemperatureC, expectedColor, expectedLabel] of [
    [16, 35, "rgb(67, 151, 86)", "Comfortable"],
    [30, 16, "rgb(207, 67, 55)", "Very humid"],
    [null, 35, "rgb(67, 151, 86)", "Comfortable"],
  ]) {
    const state = {
      ...forecastState([], null),
      current: [record, {
        ...ecowittRecord,
        metrics: { ...ecowittRecord.metrics, relativeHumidityPercent: 95, temperatureC, apparentTemperatureC },
      }],
    };
    const tile = renderWeatherDashboard(state).match(/<article[^>]*data-condition="humidity"[\s\S]*?<\/article>/u)?.[0];
    assert.ok(tile);
    assert.ok(tile.includes(`fill="${expectedColor}"`));
    assert.ok(tile.includes(`<span>${expectedLabel}</span>`));
    assert.match(tile, /class="condition-primary"><strong>95<small>%<\/small><\/strong>/u);
  }
});

// preserve same-hour pairings through maxima and the shared raw-adjusted switch
test("humidity forecast colors use paired temperatures in cards and chart gradients", () => {
  const forecast = [[95, 16], [75, 30], [65, 30], [85, null], [70, 25]].map(
    // create distinct moisture and heat peaks in one local day
    ([relativeHumidityPercent, temperatureC], index) => {
      const hour = {
        ...forecastRecord,
        validAt: `2026-08-21T${String(18 + index)}:00:00.000Z`,
        metrics: { ...forecastRecord.metrics, relativeHumidityPercent, temperatureC },
      };
      return index === 4 ? { ...hour, adjustment: activeAdjustment(hour) } : hour;
    },
  );
  // ensure both chart ends and the homepage max use the displayed metric mode
  for (const mode of ["raw", "adjusted"]) {
    // keep this forward-color fixture beyond the current time marker
    const state = { ...forecastState(forecast, null), current: [{ ...record, validAt: "2026-08-21T17:00:00.000Z" }], forecastAdjustmentMode: mode };
    const tile = renderWeatherDashboard(state).match(/<article[^>]*data-condition="humidity"[\s\S]*?<\/article>/u)?.[0];
    assert.match(tile, /condition-forecast-reading condition-forecast-tone-green[\s\S]*?Max<\/span> <strong>95<small>%/u);
    const chart = forecastChartHtml(renderWeatherDashboard(state, "forecast"), "humidity");
    const colors = [...chart.matchAll(/stop-color="([^"]+)"/gu)].map(
      // read the rendered hourly color stops including the final boundary
      (match) => match[1],
    );
    assert.deepEqual(colors, [
      "rgb(67, 151, 86)", "rgb(239, 126, 31)", "rgb(230, 181, 25)", "rgb(136, 136, 130)",
      mode === "raw" ? "rgb(67, 151, 86)" : "rgb(230, 181, 25)",
      mode === "raw" ? "rgb(67, 151, 86)" : "rgb(230, 181, 25)",
    ]);
    assert.deepEqual(forecastChartSeries(chart, "humidity")[0].values, mode === "raw" ? [95, 75, 65, 85, 70] : [95, 75, 65, 85, 65]);
  }

  const hotPeak = { ...forecast[0], metrics: { ...forecast[0].metrics, temperatureC: 30 } };
  const adjustedPeak = { ...hotPeak, adjustment: {
    ...activeAdjustment(hotPeak),
    adjustedMetrics: { temperatureC: 16, relativeHumidityPercent: 95 },
  } };
  // cover a cooler corrected maximum and missing temperature without inventing comfort
  for (const [hours, mode, tone] of [
    [[hotPeak], "raw", "red"],
    [[forecast[0], hotPeak], "raw", "red"],
    [[adjustedPeak], "adjusted", "green"],
    [[adjustedPeak], "raw", "red"],
    [[forecast[3]], "raw", "gray"],
    [[], "raw", "neutral"],
  ]) {
    const state = { ...forecastState(hours, null), forecastAdjustmentMode: mode };
    const tile = renderWeatherDashboard(state).match(/<article[^>]*data-condition="humidity"[\s\S]*?<\/article>/u)?.[0];
    assert.ok(tile.includes(`condition-forecast-reading condition-forecast-tone-${tone}`));
  }
});

test("controller loads validated unit preferences and persists changes", () => {
  const values = new Map([
    [
      UNIT_PREFERENCE_STORAGE_KEY,
      JSON.stringify({
        precipitation: "millimeters",
        pressure: "unsupported",
        temperature: "celsius",
        windSpeed: "meters_per_second",
      }),
    ],
  ]);
  const storage = {
    // read one stored value
    getItem(key) {
      return values.get(key) ?? null;
    },
    // write one stored value
    setItem(key, value) {
      values.set(key, value);
    },
  };
  const controller = new WeatherDashboardController({ storage });

  assert.deepEqual(controller.state.units, {
    precipitation: "millimeters",
    pressure: "atmosphere_percent",
    temperature: "celsius",
    waterLevel: "feet",
    windSpeed: "meters_per_second",
  });
  controller.setUnitPreferences({
    precipitation: "inches",
    pressure: "hectopascals",
    temperature: "fahrenheit",
    waterLevel: "meters",
    windSpeed: "kilometers_per_hour",
  });
  assert.deepEqual(
    loadUnitPreferences(storage),
    controller.state.units,
  );
  assert.deepEqual(JSON.parse(values.get(UNIT_PREFERENCE_STORAGE_KEY)), {
    precipitation: "inches",
    pressure: "hectopascals",
    temperature: "fahrenheit",
    waterLevel: "meters",
    windSpeed: "kilometers_per_hour",
  });
  assert.equal(controller.state.forecastAdjustmentMode, "adjusted");
  controller.toggleForecastAdjustmentMode();
  assert.equal(controller.state.forecastAdjustmentMode, "raw");
  assert.equal(values.get(FORECAST_ADJUSTMENT_MODE_STORAGE_KEY), "raw");
  assert.equal(new WeatherDashboardController({ storage }).state.forecastAdjustmentMode, "raw");
});

test("new wind sessions default adjusted while explicit raw preference persists", async () => {
  const values = new Map();
  const storage = {
    // read one stored choice
    getItem(key) {
      return values.get(key) ?? null;
    },
    // write one stored choice
    setItem(key, value) {
      values.set(key, value);
    },
  };
  const raw = {
    ...forecastRecord,
    metadata: {
      ...forecastRecord.metadata,
      provider: { ...forecastRecord.metadata.provider, dataset: "forecast" },
    },
  };

  // serve one complete forecast view
  async function fetcher(input) {
    const url = String(input);

    // serve the current row
    if (url.includes("/current")) {
      return Response.json({ data: [record], site });
    }

    // serve the wind canary
    if (url.includes("/forecast")) {
      return Response.json({
        adjustmentRuntime: windCanaryRuntime(),
        data: [{ ...raw, adjustment: windCanaryAdjustment(raw) }],
        site,
      });
    }

    return Response.json({ data: [], generatedAt: "2026-09-03T05:00:00.000Z", site });
  }

  const controller = new WeatherDashboardController({ fetcher, storage, view: "forecast" });
  await controller.initialize();
  assert.equal(controller.state.forecastAdjustmentMode, "adjusted");
  assert.equal(values.has(FORECAST_ADJUSTMENT_MODE_STORAGE_KEY), false);

  controller.toggleForecastAdjustmentMode();
  assert.equal(controller.state.forecastAdjustmentMode, "raw");
  assert.equal(values.get(FORECAST_ADJUSTMENT_MODE_STORAGE_KEY), "raw");

  const restored = new WeatherDashboardController({ fetcher, storage, view: "forecast" });
  await restored.initialize();
  assert.equal(restored.state.forecastAdjustmentMode, "raw");
});

test("temperature adjustments follow the shared persisted forecast mode", async () => {
  const values = new Map();
  values.set(FORECAST_ADJUSTMENT_MODE_STORAGE_KEY, "adjusted");
  let temperatureBundle = temperatureCanaryRuntime().activeBundle;
  const storage = {
    // read one stored choice
    getItem(key) {
      return values.get(key) ?? null;
    },
    // write one stored choice
    setItem(key, value) {
      values.set(key, value);
    },
  };
  const raw = {
    ...forecastRecord,
    metadata: {
      ...forecastRecord.metadata,
      provider: { ...forecastRecord.metadata.provider, dataset: "forecast" },
    },
  };

  // serve one temperature canary forecast
  async function fetcher(input) {
    const url = String(input);

    // serve the modeled hour
    if (url.includes("/forecast")) {
      const runtime = {
        ...temperatureCanaryRuntime(),
        activeBundle: temperatureBundle,
      };
      const decision = {
        ...temperatureCanaryDecision(raw),
        bundleSha256: temperatureBundle,
      };
      return Response.json({
        adjustmentRuntime: adjustmentRuntime("disabled", "registry_inactive"),
        data: [{
          ...raw,
          adjustment: failRawAdjustment("disabled", "registry_inactive"),
          temperatureAdjustment: decision,
        }],
        site,
        temperatureAdjustmentRuntime: runtime,
      });
    }

    return Response.json({ data: [], site });
  }

  const controller = new WeatherDashboardController({
    fetcher,
    storage,
    view: "forecast",
  });
  await controller.initialize();
  assert.equal(controller.state.forecastAdjustmentMode, "adjusted");
  assert.equal(controller.state.forecastTemperatureAdjustmentRuntime.state, "active");
  assert.equal(values.get(FORECAST_ADJUSTMENT_MODE_STORAGE_KEY), "adjusted");

  controller.toggleForecastAdjustmentMode();
  assert.equal(controller.state.forecastAdjustmentMode, "raw");
  assert.equal(values.get(FORECAST_ADJUSTMENT_MODE_STORAGE_KEY), "raw");

  const restored = new WeatherDashboardController({
    fetcher,
    storage,
    view: "forecast",
  });
  await restored.initialize();
  assert.equal(restored.state.forecastAdjustmentMode, "raw");

  temperatureBundle = "6".repeat(64);
  const changed = new WeatherDashboardController({
    fetcher,
    storage,
    view: "forecast",
  });
  await changed.initialize();
  assert.equal(changed.state.forecastAdjustmentMode, "raw");
});

test("weather URLs use the versioned API and frozen query contracts", () => {
  const filters = {
    from: "2026-08-01T00:00:00.000Z",
    sourceId: "11",
    sourceKind: "reanalysis",
    stationSlug: "open-meteo-virtual",
    to: "2026-08-22T00:00:00.000Z",
  };
  const current = new URL(
    buildCurrentUrl("/api/v1/", "ballydidean", filters),
    "http://weather.test",
  );
  const dailyPrecipitationUrl = new URL(
    buildDailyPrecipitationUrl("/api/v1/", "ballydidean"),
    "http://weather.test",
  );
  const history = new URL(
    buildHistoryUrl("/api/v1/", "ballydidean", filters, "opaque cursor"),
    "http://weather.test",
  );
  const forecast = new URL(
    buildForecastUrl("/api/v1/", "ballydidean"),
    "http://weather.test",
  );
  const tenDayForecast = new URL(
    buildForecastUrl("/api/v1/", "ballydidean", 10),
    "http://weather.test",
  );
  const trends = new URL(
    buildTrendsUrl("/api/v1/", "ballydidean"),
    "http://weather.test",
  );
  const tidesUrl = new URL(
    buildTidesUrl("/api/v1/", "ballydidean"),
    "http://weather.test",
  );

  assert.equal(current.pathname, "/api/v1/sites/ballydidean/current");
  assert.equal(current.searchParams.get("station"), "open-meteo-virtual");
  assert.equal(current.searchParams.get("source"), "11");
  assert.equal(current.searchParams.has("from"), false);
  assert.equal(dailyPrecipitationUrl.pathname, "/api/v1/sites/ballydidean/daily-precipitation");
  assert.equal(dailyPrecipitationUrl.search, "");
  assert.equal(history.pathname, "/api/v1/sites/ballydidean/history");
  assert.equal(history.searchParams.get("station"), "open-meteo-virtual");
  assert.equal(history.searchParams.get("source"), "11");
  assert.equal(history.searchParams.get("sourceKind"), "reanalysis");
  assert.equal(history.searchParams.has("kind"), false);
  assert.equal(history.searchParams.get("cursor"), "opaque cursor");
  assert.equal(history.searchParams.get("limit"), "25");
  assert.equal(forecast.pathname, "/api/v1/sites/ballydidean/forecast");
  assert.equal(forecast.search, "");
  assert.equal(tenDayForecast.pathname, "/api/v1/sites/ballydidean/forecast");
  assert.equal(tenDayForecast.searchParams.get("days"), "10");
  assert.equal(trends.pathname, "/api/v1/sites/ballydidean/trends");
  assert.equal(trends.search, "");
  assert.equal(tidesUrl.pathname, "/api/v1/sites/ballydidean/tides");
  assert.equal(tidesUrl.search, "");
});

test("history wall clocks use the selected site timezone instead of the browser timezone", () => {
  const browserTimezone = process.env.TZ;
  process.env.TZ = "UTC";

  try {
    assert.equal(
      fromSiteWallClock("2026-08-21T21:30", "America/Los_Angeles"),
      "2026-08-22T04:30:00.000Z",
    );
    assert.equal(
      toSiteWallClock("2026-08-22T04:30:00.000Z", "America/Los_Angeles"),
      "2026-08-21T21:30",
    );
    assert.notEqual(
      new Date("2026-08-21T21:30").toISOString(),
      fromSiteWallClock("2026-08-21T21:30", "America/Los_Angeles"),
    );
    assert.throws(
      () => fromSiteWallClock("2026-03-08T02:30", "America/Los_Angeles"),
      /not valid in the site timezone/u,
    );
    assert.throws(
      // reject repeated fall-back minutes
      () => fromSiteWallClock("2026-11-01T01:30", "America/Los_Angeles"),
      /not valid in the site timezone/u,
    );
  } finally {
    // restore the process timezone
    if (browserTimezone === undefined) {
      delete process.env.TZ;
    } else {
      process.env.TZ = browserTimezone;
    }
  }
});

test("logs controller preserves history filters while raw home loads unfiltered model current", async () => {
  const requested = [];

  // serve deterministic browser contracts
  async function fetcher(input) {
    const url = String(input);
    requested.push(url);

    // return site metadata
    if (url.endsWith("/api/v1/sites")) {
      return Response.json({ data: [site] });
    }

    // return current conditions
    if (url.includes("/current")) {
      return Response.json({ data: [record], site });
    }

    // return today's rain accumulation
    if (url.includes("/daily-precipitation")) {
      return Response.json({
        data: dailyPrecipitation,
        generatedAt: "2026-08-22T05:00:00.000Z",
        site,
      });
    }

    // return normalized forecast data
    if (url.includes("/forecast")) {
      return Response.json({ data: [forecastRecord], site });
    }

    // return observed and predicted tides
    if (url.includes("/tides")) {
      return Response.json({
        data: tides,
        generatedAt: "2026-08-22T05:00:00.000Z",
        site,
      });
    }

    // return recent trend buckets
    if (url.includes("/trends")) {
      return Response.json({ data: [trend], range: "24h", site });
    }

    const nextCursor = url.includes("cursor=") ? null : "page-two";
    return Response.json({
      data: [record],
      page: { limit: 100, nextCursor },
      site,
    });
  }

  const controller = new WeatherDashboardController({
    fetcher,
    view: "logs",
  });
  await controller.initialize();
  await controller.setFilters({
    sourceId: "13",
    sourceKind: "physical_sensor",
    stationSlug: "tempest-38270",
  });
  await controller.nextPage();

  assert.equal(controller.state.selectedSite?.slug, "ballydidean");
  assert.equal(controller.state.current.length, 0);
  assert.equal(controller.state.page, 1);
  assert.equal(controller.state.nextCursor, null);
  assert.equal(requested.some((url) => url.includes("/current")), false);
  assert.equal(requested.some((url) => url.includes("/daily-precipitation")), false);
  assert.ok(
    requested.some(
      // require history selection filters
      (url) =>
        url.includes("sourceKind=physical_sensor") &&
        url.includes("cursor=page-two"),
    ),
  );

  controller.toggleForecastAdjustmentMode();
  await controller.setView("home");
  const currentRequest = requested.filter((url) => url.includes("/current")).at(-1);
  assert.ok(currentRequest);
  assert.equal(new URL(currentRequest, "http://weather.test").search, "");
  assert.equal(controller.state.forecastAdjustmentMode, "raw");
  assert.equal(controller.state.current.length, 1);
  assert.equal(controller.state.current[0]?.provenance.sourceKind, "model_current");
  assert.deepEqual(controller.state.filters, {
    sourceId: "13",
    sourceKind: "physical_sensor",
    stationSlug: "tempest-38270",
  });
  assert.match(renderWeatherDashboard(controller.state, "home"), /Air Temp[\s\S]*?<strong>61<small>°F<\/small>/u);
});

test("settings controller loads browser preferences without weather requests", async () => {
  let requests = 0;

  // reject every unexpected network request
  async function fetcher() {
    requests += 1;
    throw new Error("settings should remain local");
  }

  const controller = new WeatherDashboardController({ fetcher, view: "settings" });
  await controller.initialize();

  assert.equal(requests, 0);
  assert.equal(controller.state.loading, false);
  assert.equal(controller.state.error, null);
});

test("controller retains last-good data through failure and clears the error on recovery", async () => {
  let failReads = false;

  // toggle deterministic API failure
  async function fetcher(input) {
    const url = String(input);

    // retain site discovery
    if (url.endsWith("/api/v1/sites")) {
      return Response.json({ data: [site] });
    }

    // fail one refresh cycle
    if (failReads) {
      return Response.json({ error: { code: "unavailable" } }, { status: 503 });
    }

    // return current data
    if (url.includes("/current")) {
      return Response.json({ data: [record], site });
    }

    // return today's rain accumulation
    if (url.includes("/daily-precipitation")) {
      return Response.json({
        data: dailyPrecipitation,
        generatedAt: "2026-08-22T05:00:00.000Z",
        site,
      });
    }

    // return normalized forecast data
    if (url.includes("/forecast")) {
      return Response.json({ data: [forecastRecord], site });
    }

    // return observed and predicted tides
    if (url.includes("/tides")) {
      return Response.json({
        data: tides,
        generatedAt: "2026-08-22T05:00:00.000Z",
        site,
      });
    }

    // return recent trend buckets
    if (url.includes("/trends")) {
      return Response.json({ data: [trend], range: "24h", site });
    }

    return Response.json({
      data: [record],
      page: { limit: 100, nextCursor: null },
      site,
    });
  }

  const controller = new WeatherDashboardController({ fetcher });
  await controller.initialize();
  failReads = true;
  await controller.loadSelectedSite();

  assert.equal(controller.state.current[0]?.id, "101");
  assert.equal(controller.state.dailyPrecipitation?.accumulationMm, 2.54);
  assert.equal(controller.state.history.length, 0);
  assert.match(controller.state.error ?? "", /status 503/u);

  failReads = false;
  await controller.loadSelectedSite();

  assert.equal(controller.state.error, null);
  assert.equal(controller.state.current[0]?.id, "101");
});

test("controller requests the fixed Ballydidean product without site discovery", async () => {
  const requested = [];

  // serve two incompatible site records
  async function fetcher(input) {
    const url = String(input);
    requested.push(url);

    // return one current row
    if (url.includes("/current")) {
      return Response.json({ data: [record], site });
    }

    // return today's rain accumulation
    if (url.includes("/daily-precipitation")) {
      return Response.json({
        data: dailyPrecipitation,
        generatedAt: "2026-08-22T05:00:00.000Z",
        site,
      });
    }

    // return normalized forecast data
    if (url.includes("/forecast")) {
      return Response.json({ data: [forecastRecord], site });
    }

    // return observed and predicted tides
    if (url.includes("/tides")) {
      return Response.json({
        data: tides,
        generatedAt: "2026-08-22T05:00:00.000Z",
        site,
      });
    }

    // return recent trend buckets
    if (url.includes("/trends")) {
      return Response.json({ data: [trend], range: "24h", site });
    }

    return Response.json({
      data: [record],
      page: { limit: 100, nextCursor: null },
      site,
    });
  }

  const controller = new WeatherDashboardController({ fetcher });
  await controller.initialize();

  assert.equal(controller.state.selectedSite?.slug, "ballydidean");
  assert.equal(requested.some((url) => url.endsWith("/api/v1/sites")), false);
  assert.equal(requested.some((url) => url.includes("/coupeville/")), false);
  assert.equal(requested.some((url) => url.includes("/ballydidean/")), true);
  assert.equal(requested.some((url) => url.includes("/daily-precipitation")), true);
});

test("authenticated homepage loads saved soil sensor positions", async () => {
  const requested = [];
  const savedLayout = {
    displayName: "Orchard soil",
    icon: "temperature",
    latitude: 47.9505,
    longitude: -122.4281,
    sensorKey: "soil-1",
    updatedAt: "2026-08-22T04:59:00.000Z",
  };

  // serve the authenticated homepage contracts
  async function fetcher(input) {
    const url = String(input);
    requested.push(url);

    // return current property readings
    if (url.includes("/current")) {
      return Response.json({ data: [ecowittRecord], site });
    }

    // return persisted sensor placement
    if (url.includes("/property-sensor-layout")) {
      return Response.json({ data: [savedLayout] });
    }

    // return the homepage forecast
    if (url.includes("/forecast")) {
      return Response.json({ data: [forecastRecord], site });
    }

    // return today's rain accumulation
    if (url.includes("/daily-precipitation")) {
      return Response.json({ data: dailyPrecipitation, site });
    }

    return Response.json({ data: [], generatedAt: "2026-08-22T05:00:00.000Z", site });
  }

  const controller = new WeatherDashboardController({ fetcher, isAdmin: true });
  await controller.initialize();

  assert.equal(requested.some((url) => url.includes("/property-sensor-layout")), true);
  assert.equal(controller.state.propertySensorLayoutLoading, false);
  assert.deepEqual(controller.state.propertySensorLayout, [savedLayout]);
});

test("homepage viewer context shows private panels without administrator authority", async () => {
  const requested = [];
  const layout = {
    displayName: "Orchard soil",
    icon: "temperature",
    latitude: 47.9505,
    longitude: -122.4281,
    sensorKey: "soil-1",
    updatedAt: "2026-08-22T04:59:00.000Z",
  };
  let context = { data: { homeNetwork: false } };
  let contextStatus = 200;
  let failWeather = false;

  // serve a mutable display-only location claim
  async function fetcher(input, options = {}) {
    const url = String(input);
    requested.push({ url, options });

    // return the exact ephemeral viewer context
    if (url === buildViewerContextUrl("/api/v1")) {
      return Response.json(context, { status: contextStatus });
    }

    // return shared sensor positions only on eligible routes
    if (url.includes("/property-sensor-layout")) {
      return Response.json({ data: [layout] });
    }

    // return current property readings
    if (url.includes("/current")) {
      // simulate a weather failure after local access was granted
      if (failWeather) {
        return Response.json({ error: "unavailable" }, { status: 503 });
      }

      return Response.json({ data: [ecowittRecord], site });
    }

    // return the homepage forecast
    if (url.includes("/forecast")) {
      return Response.json({ data: [forecastRecord], site });
    }

    // return today's rain accumulation
    if (url.includes("/daily-precipitation")) {
      return Response.json({ data: dailyPrecipitation, site });
    }

    return Response.json({ data: [], site });
  }

  const controller = new WeatherDashboardController({ fetcher, isAdmin: false });
  await controller.initialize();
  await controller.refreshHomeNetwork();
  assert.equal(controller.state.homeNetwork, false);
  assert.equal(controller.isAdmin, false);
  assert.doesNotMatch(renderWeatherDashboard(controller.state, "home", controller.isAdmin), /data-indoor-house|data-admin-soil-map/u);
  assert.equal(requested.some(({ url }) => url.includes("/property-sensor-layout")), false);
  assert.equal(requested.some(({ url }) => url.includes("/admin/")), false);
  const viewerRequest = requested.find(({ url }) => url.endsWith("/viewer-context"));
  assert.equal(viewerRequest?.options.credentials, "omit");
  assert.equal(viewerRequest?.options.cache, "no-store");
  assert.equal(viewerRequest?.options.method, undefined);

  context = { data: { homeNetwork: true } };
  await controller.refreshHomeNetwork();
  assert.equal(controller.state.homeNetwork, true);
  assert.equal(controller.isAdmin, false);
  assert.match(renderWeatherDashboard(controller.state, "home", controller.isAdmin), /data-indoor-house[\s\S]*data-admin-soil-map/u);
  assert.equal(requested.filter(({ url }) => url.includes("/property-sensor-layout")).length, 1);

  await new Promise(
    // settle the initial optional layout before checking redraws
    (resolve) => setImmediate(resolve),
  );

  const stableState = controller.state;
  const positiveRecheck = controller.refreshHomeNetwork();
  assert.equal(controller.state, stableState);
  await positiveRecheck;
  assert.equal(controller.state, stableState);
  assert.equal(requested.filter(({ url }) => url.includes("/property-sensor-layout")).length, 1);

  failWeather = true;
  await controller.loadCurrent();
  assert.equal(controller.state.homeNetwork, false);
  assert.match(controller.state.error ?? "", /status 503/u);
  failWeather = false;
  await controller.loadCurrent();
  await controller.refreshHomeNetwork();
  assert.equal(controller.state.homeNetwork, true);

  context = { data: { homeNetwork: false } };
  const negativeRecheck = controller.refreshHomeNetwork();
  assert.equal(controller.state.homeNetwork, true);
  await negativeRecheck;
  assert.equal(controller.state.homeNetwork, false);
  assert.doesNotMatch(renderWeatherDashboard(controller.state, "home", controller.isAdmin), /data-indoor-house|data-admin-soil-map/u);

  context = { data: { homeNetwork: "true" } };
  await controller.refreshHomeNetwork();
  assert.equal(controller.state.homeNetwork, false);

  contextStatus = 503;
  context = { data: { homeNetwork: true } };
  await controller.refreshHomeNetwork();
  assert.equal(controller.state.homeNetwork, false);

  contextStatus = 200;
  await controller.setView("forecast");
  assert.equal(controller.state.homeNetwork, false);
  context = { data: { homeNetwork: true } };
  await controller.setView("home");
  await controller.refreshHomeNetwork();
  assert.equal(controller.state.homeNetwork, true);

  const adminRequests = [];
  const admin = new WeatherDashboardController({
    fetcher: async (input, options) => {
      adminRequests.push(String(input));
      return fetcher(input, options);
    },
    isAdmin: true,
  });
  await admin.initialize();
  assert.equal(admin.state.homeNetwork, false);
  assert.equal(adminRequests.some((url) => url.endsWith("/viewer-context")), false);
  assert.equal(adminRequests.some((url) => url.includes("/property-sensor-layout")), true);
  assert.match(renderWeatherDashboard(admin.state, "home", admin.isAdmin), /data-indoor-house[\s\S]*data-admin-soil-map/u);
});

// reject stale route responses and late answers after the viewer deadline
test("homepage viewer context times out and ignores stale positive replies", { timeout: 10_000 }, async () => {
  const pending = [];
  let layoutReads = 0;

  // hold context reads while weather remains available
  async function fetcher(input, options = {}) {
    const url = String(input);

    // expose one deferred location claim and its abort signal
    if (url === buildViewerContextUrl("/api/v1")) {
      return new Promise(
        // let each test step complete one chosen response
        (resolve) => pending.push({ resolve, signal: options.signal }),
      );
    }

    // count only eligible layout reads
    if (url.includes("/property-sensor-layout")) {
      layoutReads += 1;
      return Response.json({ data: [] });
    }

    return Response.json({ data: [], site });
  }

  const controller = new WeatherDashboardController({ fetcher, isAdmin: false });
  await controller.initialize();
  assert.equal(pending.length, 1);
  assert.equal(controller.state.homeNetwork, false);

  await controller.setView("forecast");
  await controller.setView("home");
  assert.equal(pending.length, 2);
  pending[0].resolve(Response.json({ data: { homeNetwork: true } }));
  await new Promise(
    // flush the old reply after returning to the homepage
    (resolve) => setImmediate(resolve),
  );
  assert.equal(controller.state.homeNetwork, false);
  assert.equal(layoutReads, 0);

  pending[1].resolve(Response.json({ data: { homeNetwork: true } }));
  await controller.refreshHomeNetwork();
  assert.equal(controller.state.homeNetwork, true);
  assert.equal(layoutReads, 1);

  const timeoutRefresh = controller.refreshHomeNetwork();
  assert.equal(controller.state.homeNetwork, true);
  assert.equal(pending.length, 3);
  await timeoutRefresh;
  assert.equal(pending[2].signal.aborted, true);
  assert.equal(controller.state.homeNetwork, false);
  pending[2].resolve(Response.json({ data: { homeNetwork: true } }));
  await new Promise(
    // flush the reply ignored after the four-second deadline
    (resolve) => setImmediate(resolve),
  );
  assert.equal(controller.state.homeNetwork, false);
  assert.equal(layoutReads, 1);
});

// preserve a delayed soil layout through positive location rechecks only
test("homepage viewer context keeps authorized layouts but rejects revoked ones", async () => {
  const layouts = [];
  const savedLayout = {
    displayName: "Orchard soil",
    icon: "temperature",
    latitude: 47.9505,
    longitude: -122.4281,
    sensorKey: "soil-1",
    updatedAt: "2026-08-22T04:59:00.000Z",
  };
  let homeNetwork = true;

  // delay only the optional property-layout response
  async function fetcher(input) {
    const url = String(input);

    // return the current location claim
    if (url === buildViewerContextUrl("/api/v1")) {
      return Response.json({ data: { homeNetwork } });
    }

    // hold each optional layout independently
    if (url.includes("/property-sensor-layout")) {
      return new Promise(
        // resolve only the selected layout attempt
        (resolve) => layouts.push(resolve),
      );
    }

    // preserve a reporting soil sensor for the rendered marker
    if (url.includes("/current")) {
      return Response.json({ data: [ecowittRecord], site });
    }

    return Response.json({ data: [], site });
  }

  const controller = new WeatherDashboardController({ fetcher, isAdmin: false });
  await controller.initialize();
  await controller.refreshHomeNetwork();
  assert.equal(controller.state.homeNetwork, true);
  assert.equal(layouts.length, 1);
  assert.equal(controller.state.propertySensorLayoutLoading, true);
  const pendingLayout = renderWeatherDashboard(controller.state, "home", false);
  assert.match(pendingLayout, /admin-soil-map-panel skeleton-region/u);
  assert.match(pendingLayout, /class="weather-content" aria-busy="false"/u);
  assert.match(pendingLayout, /role="status">Refreshing weather data…/u);
  assert.doesNotMatch(pendingLayout, /Weather data is up to date/u);
  const beforeRecheck = controller.state;
  await controller.refreshHomeNetwork();
  assert.equal(controller.state, beforeRecheck);
  assert.equal(layouts.length, 1);
  layouts[0](Response.json({ data: [savedLayout] }));
  await new Promise(
    // allow the delayed layout to reach the renderer
    (resolve) => setImmediate(resolve),
  );
  assert.equal(controller.state.propertySensorLayoutLoading, false);
  assert.deepEqual(controller.state.propertySensorLayout, [savedLayout]);
  assert.match(renderWeatherDashboard(controller.state, "home", false), /data-soil-moisture-sensor="soil-1"/u);

  await controller.loadCurrent();
  await controller.refreshHomeNetwork();
  assert.equal(layouts.length, 2);
  homeNetwork = false;
  await controller.refreshHomeNetwork();
  layouts[1](Response.json({ data: [] }));
  await new Promise(
    // reject layout after a negative location claim
    (resolve) => setImmediate(resolve),
  );
  assert.equal(controller.state.homeNetwork, false);
  assert.equal(controller.state.propertySensorLayoutLoading, false);
  assert.equal(controller.state.propertySensorLayout, null);

  homeNetwork = true;
  await controller.refreshHomeNetwork();
  assert.equal(layouts.length, 3);
  await controller.setView("forecast");
  layouts[2](Response.json({ data: [] }));
  await new Promise(
    // reject layout from the previous route
    (resolve) => setImmediate(resolve),
  );
  assert.equal(controller.state.homeNetwork, false);
  assert.equal(controller.state.propertySensorLayoutLoading, false);
  assert.equal(controller.state.propertySensorLayout, null);
});

// retry an unavailable soil layout after a later positive viewer check
test("homepage viewer context retries a failed soil layout without false placement notes", async () => {
  let layoutStatus = 503;
  let layoutReads = 0;
  const savedLayout = {
    displayName: "Orchard soil",
    icon: "temperature",
    latitude: 47.9505,
    longitude: -122.4281,
    sensorKey: "soil-1",
    updatedAt: "2026-08-22T04:59:00.000Z",
  };

  // fail the optional layout once while the location claim stays positive
  async function fetcher(input) {
    const url = String(input);

    // keep the home-network display claim granted
    if (url === buildViewerContextUrl("/api/v1")) {
      return Response.json({ data: { homeNetwork: true } });
    }

    // switch the optional layout from outage to recovery
    if (url.includes("/property-sensor-layout")) {
      layoutReads += 1;
      return Response.json({ data: [savedLayout] }, { status: layoutStatus });
    }

    // keep one reporting soil probe available
    if (url.includes("/current")) {
      return Response.json({ data: [ecowittRecord], site });
    }

    return Response.json({ data: [], site });
  }

  const controller = new WeatherDashboardController({ fetcher, isAdmin: false });
  await controller.initialize();
  await new Promise(
    // settle the failed optional layout
    (resolve) => setImmediate(resolve),
  );
  assert.equal(controller.state.homeNetwork, true);
  assert.equal(controller.state.propertySensorLayoutLoading, false);
  assert.equal(controller.state.propertySensorLayout, null);
  assert.equal(layoutReads, 1);
  const unavailable = renderWeatherDashboard(controller.state, "home", false);
  assert.match(unavailable, /Soil moisture sensor positions are unavailable\./u);
  assert.doesNotMatch(unavailable, /soil moisture sensors? still needs? a position in Admin/u);

  layoutStatus = 200;
  await controller.refreshHomeNetwork();
  await new Promise(
    // settle the recovered optional layout
    (resolve) => setImmediate(resolve),
  );
  assert.equal(layoutReads, 2);
  assert.equal(controller.state.propertySensorLayoutLoading, false);
  assert.deepEqual(controller.state.propertySensorLayout, [savedLayout]);
  const recovered = renderWeatherDashboard(controller.state, "home", false);
  assert.match(recovered, /data-soil-moisture-sensor="soil-1"/u);
  assert.doesNotMatch(recovered, /Soil moisture sensor positions are unavailable\./u);
});

test("admin saves independent adjustment switches and verifies readback", async () => {
  let persisted = { version: 1, temperature: true, wind: true, rain: false };
  let staleReadback = false;
  let rejectWrite = false;
  const writes = [];

  // serve the protected settings and standard admin panels
  async function fetcher(input, init = {}) {
    const url = String(input);

    // serve and mutate the adjustment settings resource
    if (url.endsWith("/admin/sites/ballydidean/forecast-adjustment-settings")) {
      if (init.method === "PUT") {
        writes.push(init);

        // simulate a rejected protected write
        if (rejectWrite) {
          return Response.json({ error: "rejected" }, { status: 403 });
        }

        persisted = JSON.parse(init.body);
        return Response.json({ data: persisted });
      }

      return Response.json({ data: staleReadback ? {
        version: 1, temperature: true, wind: true, rain: false,
      } : persisted });
    }

    // serve current conditions for the admin layout
    if (url.includes("/current")) {
      return Response.json({ data: [record], site });
    }

    // serve saved sensor positions for the admin layout
    if (url.includes("/property-sensor-layout")) {
      return Response.json({ data: [] });
    }

    return Response.json({ data: [], site });
  }

  const controller = new WeatherDashboardController({ fetcher, isAdmin: true, view: "admin" });
  await controller.initialize();
  const initial = { version: 1, temperature: true, wind: true, rain: false };
  assert.deepEqual(controller.state.forecastAdjustmentSettings, initial);
  const initialHtml = renderWeatherDashboard(controller.state, "admin", true);
  assert.match(initialHtml, /data-admin-forecast-adjustments/u);
  assert.match(initialHtml, /name="temperature"/u);
  assert.match(initialHtml, /name="wind"/u);
  assert.match(initialHtml, /name="rain"/u);
  assert.match(initialHtml, /name="temperature" checked/u);
  assert.match(initialHtml, /name="wind" checked/u);
  assert.doesNotMatch(initialHtml, /name="rain" checked/u);

  const enabled = { version: 1, temperature: false, wind: true, rain: true };
  await controller.saveForecastAdjustmentSettings({
    version: 1, temperature: false, wind: true, rain: true,
  });
  assert.deepEqual(controller.state.forecastAdjustmentSettings, enabled);
  assert.equal(controller.state.adminAdjustmentSettingsMessage, "Forecast adjustments saved.");
  assert.equal(writes[0].credentials, "same-origin");
  assert.equal(writes[0].method, "PUT");
  assert.deepEqual(JSON.parse(writes[0].body), enabled);
  const savedHtml = renderWeatherDashboard(controller.state, "admin", true);
  assert.match(savedHtml, /name="rain" checked/u);
  assert.match(savedHtml, /name="wind" checked/u);
  assert.doesNotMatch(savedHtml, /name="temperature" checked/u);

  staleReadback = true;
  await controller.saveForecastAdjustmentSettings(enabled);
  assert.deepEqual(controller.state.forecastAdjustmentSettings, enabled);
  assert.equal(controller.state.adminAdjustmentSettingsMessage, "Forecast adjustment settings did not persist.");
  assert.equal(controller.state.adminAdjustmentSettingsSaving, false);

  staleReadback = false;
  rejectWrite = true;
  await controller.saveForecastAdjustmentSettings({ version: 1, temperature: false, wind: false, rain: false });
  assert.deepEqual(controller.state.forecastAdjustmentSettings, enabled);
  assert.match(controller.state.adminAdjustmentSettingsMessage, /status 403/u);
});

test("admin scorecard preserves readable display-only v1 model families", () => {
  const scorecard = adjustmentScorecard();
  assert.equal(parseForecastAdjustmentScorecard(scorecard), scorecard);
  assert.equal(
    buildAdminForecastAdjustmentScorecardUrl("/api/v1", "ballydidean"),
    "/api/v1/admin/sites/ballydidean/forecast-adjustment-scorecard",
  );
  assert.equal(
    parseForecastAdjustmentScorecard({ ...scorecard, servingChanged: true }),
    null,
  );
  assert.equal(
    parseForecastAdjustmentScorecard({ ...scorecard, validThrough: "2026-10-01T00:00:01.000Z" }, Date.parse("2026-10-02T00:00:00.000Z")),
    null,
  );

  const state = {
    ...forecastState([], null),
    adminAdjustmentScorecard: scorecard,
    adminAdjustmentScorecardState: "ready",
  };
  const html = renderWeatherDashboard(state, "admin", true);
  const scorecardHtml = html.match(/<section class="panel adjustment-scorecard"[\s\S]*?<\/section>/u)?.[0] ?? "";
  assert.match(scorecardHtml, /data-scorecard-state="ready"/u);
  assert.equal((scorecardHtml.match(/data-adjustment-family=/gu) ?? []).length, 3);
  assert.match(scorecardHtml, /Temperature[\s\S]*Wind[\s\S]*Rain/u);
  assert.match(scorecardHtml, /95% interval 4\.2% to 19\.1%/u);
  assert.match(scorecardHtml, /Best Match diagnostic/u);
  assert.match(scorecardHtml, /Rain diagnostics/u);
  assert.match(scorecardHtml, /data-status="worse">Worse/u);
  assert.match(scorecardHtml, /Legacy report/u);
  assert.match(scorecardHtml, /Historical display only/u);
  assert.match(scorecardHtml, /cannot authorize actions/u);
  assert.doesNotMatch(scorecardHtml, /Ansel approval is required/u);
  assert.doesNotMatch(scorecardHtml, /<button|<form|<input/u);
});

test("admin scorecard parses and renders sanitized v2 policy lineage and history", () => {
  const scorecard = adjustmentScorecardV2();
  assert.equal(parseForecastAdjustmentScorecard(scorecard), scorecard);
  assert.equal(
    parseForecastAdjustmentScorecard({ ...scorecard, operatorApprovalRequired: true }),
    null,
  );
  assert.equal(
    parseForecastAdjustmentScorecard({
      ...scorecard,
      history: [{ ...scorecard.history[0], privateMemberResult: { loss: 1 } }],
    }),
    null,
  );
  assert.equal(
    parseForecastAdjustmentScorecard({
      ...scorecard,
      validThrough: "2099-10-08T00:00:00.001Z",
    }),
    null,
  );

  const html = renderWeatherDashboard({
    ...forecastState([], null),
    adminAdjustmentScorecard: scorecard,
    adminAdjustmentScorecardPublicationState: "pending_unapplied",
    adminAdjustmentScorecardState: "ready",
  }, "admin", true);
  const scorecardHtml = html.match(/<section class="panel adjustment-scorecard"[\s\S]*?<\/section>/u)?.[0] ?? "";
  assert.match(scorecardHtml, /Automatic policy/u);
  assert.match(scorecardHtml, /Pending and unapplied/u);
  assert.match(scorecardHtml, /data-scorecard-publication-state="pending_unapplied"/u);
  assert.match(scorecardHtml, /Action lineage/u);
  assert.match(scorecardHtml, /Confirmation epochs/u);
  assert.match(scorecardHtml, /Model identities/u);
  assert.match(scorecardHtml, /next capture reserved/u);
  assert.match(scorecardHtml, /Automatic action history \(1\)/u);
  assert.match(scorecardHtml, /cannot itself authorize or execute changes/u);
  assert.doesNotMatch(scorecardHtml, /operatorApprovalRequired|Ansel approval/u);
  assert.doesNotMatch(scorecardHtml, /<button|<form|<input/u);
});

test("admin scorecard read is isolated and fails closed by response state", async () => {
  const scorecard = adjustmentScorecard();
  const scorecardRequests = [];

  // serve protected administrator panels and one scorecard
  async function fetcher(input, init = {}) {
    const url = String(input);

    // record the dedicated queryless scorecard request
    if (url.endsWith("/admin/sites/ballydidean/forecast-adjustment-scorecard")) {
      scorecardRequests.push({ url, init });
      return Response.json({ data: scorecard, publicationState: "legacy_display" });
    }

    // serve adjustment settings independently
    if (url.endsWith("/admin/sites/ballydidean/forecast-adjustment-settings")) {
      return Response.json({ data: { version: 1, temperature: true, wind: true, rain: true } });
    }

    // serve current conditions for the admin shell
    if (url.endsWith("/sites/ballydidean/current")) {
      return Response.json({ data: [record], site });
    }

    // serve an empty position list
    if (url.endsWith("/sites/ballydidean/property-sensor-layout")) {
      return Response.json({ data: [] });
    }

    return Response.json({ data: [], site });
  }

  const controller = new WeatherDashboardController({ fetcher, isAdmin: true, view: "admin" });
  await controller.initialize();
  assert.equal(controller.state.adminAdjustmentScorecardState, "ready");
  assert.equal(controller.state.adminAdjustmentScorecardPublicationState, "legacy_display");
  assert.deepEqual(controller.state.adminAdjustmentScorecard, scorecard);
  assert.equal(scorecardRequests.length, 1);
  assert.equal(scorecardRequests[0].url.includes("?"), false);
  assert.equal(scorecardRequests[0].init.cache, "no-store");
  assert.equal(scorecardRequests[0].init.credentials, "same-origin");

  const unavailable = renderWeatherDashboard({
    ...controller.state,
    adminAdjustmentScorecard: null,
    adminAdjustmentScorecardState: "unavailable",
  }, "admin", true);
  assert.match(unavailable, /No validated scorecard is available/u);
  assert.match(unavailable, /cannot support an action/u);
  const unauthorized = renderWeatherDashboard({
    ...controller.state,
    adminAdjustmentScorecard: null,
    adminAdjustmentScorecardState: "unauthorized",
  }, "admin", true);
  assert.match(unauthorized, /Administrator session is unavailable/u);
});

test("failed next-page reads keep the prior page label and cursor", async () => {
  let failReads = false;

  // serve one successful page then fail pagination
  async function fetcher(input) {
    const url = String(input);

    // return site metadata
    if (url.endsWith("/api/v1/sites")) {
      return Response.json({ data: [site] });
    }

    // preserve current reads
    if (url.includes("/current")) {
      return Response.json({ data: [record], site });
    }

    // fail the attempted next page
    if (failReads) {
      return Response.json({ error: { code: "unavailable" } }, { status: 503 });
    }

    return Response.json({
      data: [record],
      page: { limit: 100, nextCursor: "page-two" },
      site,
    });
  }

  const controller = new WeatherDashboardController({ fetcher, view: "logs" });
  await controller.initialize();
  failReads = true;
  await controller.nextPage();

  assert.equal(controller.state.page, 0);
  assert.equal(controller.state.nextCursor, "page-two");
  assert.equal(controller.state.history[0]?.id, "101");
  assert.match(controller.state.error ?? "", /status 503/u);
});

// keep pending data out of every public page without a header indicator
test("all data routes use skeletons for initial and repeat reads", () => {
  const initial = new WeatherDashboardController({ storage: null }).state;
  const cached = {
    ...initial,
    current: [record],
    forecast: [forecastRecord],
    history: [record],
    selectedSite: site,
    sites: [site],
  };
  const markers = {
    home: "current-conditions skeleton-region",
    forecast: "forecast-chart skeleton-forecast-chart",
    trends: "trend-chart skeleton-trend-chart",
    map: "station-map skeleton-map",
    logs: "skeleton-history-row",
  };

  // require cached data to use the same loading contract as the first read
  for (const state of [initial, cached]) {
    // inspect every remotely loaded public route
    for (const [view, marker] of Object.entries(markers)) {
      const html = renderWeatherDashboard(state, view);
      assert.ok(html.includes(marker), `${view} must show an in-place skeleton`);
      assert.match(html, /class="weather-content" aria-busy="true"/u);
      assert.match(html, /<p class="sr-only" role="status">Refreshing weather data…<\/p>/u);
      assert.doesNotMatch(html, /refresh-indicator/u);
      assert.doesNotMatch(html.match(/<header[\s\S]*?<\/header>/u)?.[0] ?? "", /role="status"|Loading|Refreshing/u);
    }
  }

  // clear loading surfaces for both success and last-good error recovery
  for (const error of [null, "Read failed"]) {
    // ensure every settled route renders real data or an honest empty state
    for (const view of Object.keys(markers)) {
      const html = renderWeatherDashboard({ ...cached, loading: false, error }, view);
      assert.doesNotMatch(html, /skeleton-region|skeleton-history-row|skeleton-history-card/u);
      assert.match(html, /class="weather-content" aria-busy="false"/u);
      // preserve failures separately from loading feedback
      if (error !== null) {
        assert.match(html, /role="alert">Read failed/u);
      }
    }
  }
  // local settings fields stay usable even when navigation artwork is still loading
  const settingsContent = renderWeatherDashboard(initial, "settings").split('<div class="weather-content"')[1];
  assert.doesNotMatch(settingsContent, /skeleton-region|skeleton-line/u);
});

// distinguish sensor loading from empty and unavailable states
test("sensor maps and admin panels reserve missing data with skeletons", () => {
  const initial = new WeatherDashboardController({ storage: null, isAdmin: true }).state;
  const home = renderWeatherDashboard(initial, "home", true);
  assert.match(home, /admin-soil-map-panel skeleton-region/u);
  assert.match(home, /skeleton-temperature/u);
  const map = renderWeatherDashboard(initial, "map");
  assert.match(map, /property-map-panel skeleton-region/u);
  assert.doesNotMatch(map, /Loading property sensors|No property sensors|<image/u);
  const admin = renderWeatherDashboard(initial, "admin", true);
  assert.match(admin, /forecast-adjustment-admin skeleton-region/u);
  assert.match(admin, /property-admin skeleton-region/u);
  assert.doesNotMatch(admin, /Loading EcoWitt|No EcoWitt|data-property-sensor-form|data-admin-forecast-adjustments/u);
  const settled = renderWeatherDashboard({ ...initial, loading: false }, "admin", true);
  assert.match(settled, /No EcoWitt sensor channels are reporting yet/u);
  assert.doesNotMatch(settled, /skeleton-region/u);
  const saving = renderWeatherDashboard({
    ...initial,
    loading: false,
    forecastAdjustmentSettings: { version: 1, temperature: true, wind: true, rain: true },
    adminAdjustmentSettingsSaving: true,
  }, "admin", true);
  assert.match(saving, /skeleton-action/u);
  assert.match(saving, /class="sr-only">Saving forecast adjustments…/u);
});

// cover the app shell before javascript mounts its route-specific skeletons
test("bootstrap uses skeletons instead of a visible loading notice", () => {
  const html = readFileSync(new URL("../public/index.html", import.meta.url), "utf8");
  assert.match(html, /shell skeleton-region" aria-busy="true"/u);
  assert.match(html, /skeleton-line skeleton-field/u);
  assert.match(html, /class="sr-only" role="status">Loading Ballydídean weather…/u);
  assert.doesNotMatch(html, /class="notice"/u);
});

// revoke optional loading feedback when its parent weather request fails
test("failed homepage weather clears a pending optional layout skeleton", async () => {
  let resolveCurrent;
  let resolveLayout;
  const current = new Promise(
    // control the primary failure independently of the optional read
    (resolve) => { resolveCurrent = resolve; },
  );
  const layout = new Promise(
    // hold the optional positions until after access is revoked
    (resolve) => { resolveLayout = resolve; },
  );
  const controller = new WeatherDashboardController({
    storage: null,
    // grant local display while delaying current and layout responses
    fetcher: async (input) => {
      const url = String(input);
      // allow the private display boundary before the primary read settles
      if (url === buildViewerContextUrl("/api/v1")) {
        return Response.json({ data: { homeNetwork: true } });
      }
      // hold positions independently of current observations
      if (url.includes("/property-sensor-layout")) {
        return layout;
      }
      // inject the primary failure after the optional loader appears
      if (url.includes("/current")) {
        return current;
      }
      return Response.json({ data: [], site });
    },
  });
  const initialized = controller.initialize();
  await controller.refreshHomeNetwork();
  assert.equal(controller.state.propertySensorLayoutLoading, true);
  resolveCurrent(Response.json({ error: "unavailable" }, { status: 503 }));
  await initialized;
  assert.equal(controller.state.propertySensorLayoutLoading, false);
  assert.equal(controller.state.homeNetwork, false);
  assert.match(renderWeatherDashboard(controller.state), /role="status">Weather refresh failed/u);
  assert.doesNotMatch(renderWeatherDashboard(controller.state), /skeleton-region|data-admin-soil-map/u);
  resolveLayout(Response.json({ data: [] }));
  await new Promise(
    // reject the late positions without reactivating their loading state
    (resolve) => setImmediate(resolve),
  );
  assert.equal(controller.state.propertySensorLayoutLoading, false);
  assert.equal(controller.state.propertySensorLayout, null);
});

// reuse unrounded forecast thresholds for every indoor floor and display unit
test("indoor temperatures share the thresholded forecast text colors", () => {
  const initial = new WeatherDashboardController({ storage: null }).state;
  // exercise each exact boundary and readings that round across it
  for (const [valueF, tone] of [
    [32, "blue"],
    [54.9, "blue"],
    [55, "green"],
    [60, "green"],
    [70, "green"],
    [70.1, "orange"],
    [80, "orange"],
    [80.1, "red"],
    [null, "neutral"],
  ]) {
    const temperatureC = valueF === null ? null : (valueF - 32) * 5 / 9;
    const indoorRecord = {
      ...ecowittRecord,
      metadata: {
        ...ecowittRecord.metadata,
        provider: {
          ...ecowittRecord.metadata.provider,
          propertySensors: ["gateway", "temperature-1", "temperature-2"].map(
            // use the same boundary reading at each physical floor
            (key) => ({ key, model: "WH31", channel: null, readings: { temperatureC } }),
          ),
        },
      },
    };
    // ensure unit conversion never changes the selected color
    for (const temperature of ["fahrenheit", "celsius"]) {
      const state = {
        ...initial,
        current: [indoorRecord],
        loading: false,
        units: { ...DEFAULT_UNIT_PREFERENCES, temperature },
      };
      // preserve the independent admin and home-network visibility paths
      for (const isAdmin of [true, false]) {
        const html = renderWeatherDashboard({ ...state, homeNetwork: !isAdmin }, "home", isAdmin);
        const indoor = html.match(/<section class="indoor-house-panel"[\s\S]*?<\/section>/u)?.[0];
        assert.ok(indoor);
        assert.deepEqual([...indoor.matchAll(/indoor-house-temperature condition-forecast-tone-([a-z]+)/gu)].map(
          // compare every floor against the shared discrete threshold
          (match) => match[1],
        ), [tone, tone, tone], `${String(valueF)}F in ${temperature}`);
        const measurement = formatMeasurement(temperatureC, "temperature", state.units, 0);
        assert.ok(indoor.includes(`<strong>${measurement.value}`));
      }
      const loading = renderWeatherDashboard({ ...state, loading: true }, "home", true);
      assert.equal((loading.match(/indoor-house-temperature condition-forecast-tone-neutral/gu) ?? []).length, 3);
      assert.equal((loading.match(/skeleton-temperature/gu) ?? []).length, 3);
      assert.doesNotMatch(renderWeatherDashboard(state, "home", false), /data-indoor-house/u);
    }
  }
  const missing = renderWeatherDashboard({ ...initial, loading: false }, "home", true);
  assert.equal((missing.match(/indoor-house-temperature condition-forecast-tone-neutral/gu) ?? []).length, 3);
});
