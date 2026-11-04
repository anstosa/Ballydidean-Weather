// source-derived public validation closure; canonical parity is locked by differential tests
// legacy rain authority uses its immutable byte sha instead of embedding private model bytes
// packages/domain/dist/provenance.js
var SOURCE_KINDS = [
  "physical_sensor",
  "model_current",
  "reanalysis",
  "forecast",
  "tide_observation",
  "tide_prediction"
];
function parseSourceKind(value) {
  if (!SOURCE_KINDS.some((sourceKind) => sourceKind === value)) {
    throw new RangeError(`unsupported source kind: ${value}`);
  }
  return value;
}
function canonicalizeJson(value) {
  if (value === null) {
    return "null";
  }
  if (typeof value === "number" && !Number.isFinite(value)) {
    throw new TypeError("JSON numbers must be finite");
  }
  if (typeof value !== "object") {
    const serialized = JSON.stringify(value);
    if (serialized === void 0) {
      throw new TypeError("value is not valid JSON");
    }
    return serialized;
  }
  if (Array.isArray(value)) {
    return `[${value.map((entry) => canonicalizeJson(entry)).join(",")}]`;
  }
  const entries = Object.entries(value).sort(([left], [right]) => compareJsonKeys(left, right));
  return `{${entries.map(([key, entry]) => `${JSON.stringify(key)}:${canonicalizeJson(entry)}`).join(",")}}`;
}
function compareJsonKeys(left, right) {
  if (left < right) {
    return -1;
  }
  if (left > right) {
    return 1;
  }
  return 0;
}

// packages/domain/dist/weather-record.js
var CANONICAL_UNITS = {
  apparentTemperatureC: "celsius",
  blackGlobeTemperatureC: "celsius",
  cloudCoverPercent: "percent",
  pm25MicrogramsPerCubicMeter: "microgram_per_cubic_meter",
  precipitationMm: "millimeter",
  precipitationRateMmPerHour: "millimeter_per_hour",
  pressureHpa: "hectopascal",
  relativeHumidityPercent: "percent",
  soilElectricalConductivityMicrosiemensPerCm: "microsiemens_per_centimeter",
  soilMoisturePercent: "percent",
  solarRadiationWm2: "watt_per_square_meter",
  temperatureC: "celsius",
  uvIndex: "index",
  waterLevelM: "meter",
  windDirectionDegrees: "degree",
  windGustMps: "meter_per_second",
  windSpeedMps: "meter_per_second",
  wetBulbGlobeTemperatureC: "celsius"
};
var METRIC_BOUNDS = {
  apparentTemperatureC: { maximum: 70, minimum: -100 },
  blackGlobeTemperatureC: { maximum: 125, minimum: -100 },
  cloudCoverPercent: { maximum: 100, minimum: 0 },
  pm25MicrogramsPerCubicMeter: { maximum: 999, minimum: 0 },
  precipitationMm: { maximum: 2e3, minimum: 0 },
  precipitationRateMmPerHour: { maximum: 1e4, minimum: 0 },
  pressureHpa: { maximum: 1200, minimum: 100 },
  relativeHumidityPercent: { maximum: 100, minimum: 0 },
  soilElectricalConductivityMicrosiemensPerCm: {
    maximum: 1e4,
    minimum: 0
  },
  soilMoisturePercent: { maximum: 100, minimum: 0 },
  solarRadiationWm2: { maximum: 2500, minimum: 0 },
  temperatureC: { maximum: 70, minimum: -100 },
  uvIndex: { maximum: 20, minimum: 0 },
  waterLevelM: { maximum: 30, minimum: -20 },
  windDirectionDegrees: { maximum: 360, maximumExclusive: true, minimum: 0 },
  windGustMps: { maximum: 150, minimum: 0 },
  windSpeedMps: { maximum: 150, minimum: 0 },
  wetBulbGlobeTemperatureC: { maximum: 125, minimum: -100 }
};
var QUALITY_KEYS = /* @__PURE__ */ new Set([
  "confidence_percent",
  "flags",
  "interpolation",
  "sampling",
  "status"
]);
var PROVIDER_KEYS = /* @__PURE__ */ new Set([
  "battery_volts",
  "dataset",
  "device_id",
  "elevation_m",
  "grid_cell",
  "illuminance_lux",
  "lightning_average_distance_km",
  "lightning_strike_count",
  "location_id",
  "precipitation_type",
  "rain_accumulation_nc_mm",
  "report_interval_minutes",
  "request_id",
  "station_id",
  "datum",
  "product",
  "prediction_type",
  "property_sensors",
  "wind_lull_mps",
  "wind_sample_interval_seconds"
]);
var METRIC_NAMES = Object.keys(CANONICAL_UNITS);
var METADATA_KEYS = /* @__PURE__ */ new Set([
  "device",
  "model",
  "provider",
  "quality",
  "upstreamTimezone"
]);
var DEVICE_KEYS = /* @__PURE__ */ new Set(["model", "serial", "vendor"]);
function createNormalizedWeatherRecord(input) {
  const sourceKind = parseSourceKind(input.sourceKind);
  const validAt = validateUtcInstant(input.validAt, "validAt");
  const receivedAt = validateUtcInstant(input.receivedAt, "receivedAt");
  const productRunAt = input.productRunAt === null || input.productRunAt === void 0 ? null : validateUtcInstant(input.productRunAt, "productRunAt");
  if (sourceKind === "forecast" && productRunAt === null) {
    throw new RangeError("forecast records require productRunAt");
  }
  const metrics = validateCanonicalWeatherMetrics(input.metrics);
  const metadata = validateWeatherRecordMetadata(input.metadata);
  if (input.sourceId.trim().length === 0 || input.sourceId.length > 128) {
    throw new RangeError("sourceId must be non-empty and bounded");
  }
  return {
    metadata,
    metrics,
    productRunAt,
    receivedAt,
    sourceId: input.sourceId,
    sourceKind,
    validAt
  };
}
function validateCanonicalWeatherMetrics(metrics) {
  const metricKeys = Object.keys(metrics);
  if (metricKeys.length !== METRIC_NAMES.length || METRIC_NAMES.some((metric) => !(metric in metrics)) || metricKeys.some((metric) => !METRIC_NAMES.includes(metric))) {
    throw new RangeError("record metrics must use the complete canonical metric set");
  }
  for (const [metric, value] of Object.entries(metrics)) {
    if (value !== null) {
      validateMetricValue(metric, value);
    }
  }
  return { ...metrics };
}
function weatherRecordContent(record2) {
  return canonicalizeJson({
    metadata: record2.metadata,
    metrics: record2.metrics,
    productRunAt: record2.productRunAt,
    sourceId: record2.sourceId,
    sourceKind: record2.sourceKind,
    validAt: record2.validAt
  });
}
function validateUtcInstant(value, fieldName) {
  if (!/(?:Z|[+-]\d{2}:\d{2})$/u.test(value)) {
    throw new RangeError(`${fieldName} must include an explicit timezone`);
  }
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    throw new RangeError(`${fieldName} must be a valid instant`);
  }
  return parsed.toISOString();
}
var VALID_TIMEZONE_CACHE_LIMIT = 64;
var validatedTimeZones = /* @__PURE__ */ new Set();
function validateTimeZone(value) {
  if (value.length === 0 || value.length > 64) {
    throw new RangeError("timezone must be non-empty and bounded");
  }
  if (validatedTimeZones.has(value)) {
    return value;
  }
  try {
    new Intl.DateTimeFormat("en-US", { timeZone: value }).format();
  } catch {
    throw new RangeError(`unsupported IANA timezone: ${value}`);
  }
  if (validatedTimeZones.size >= VALID_TIMEZONE_CACHE_LIMIT) {
    validatedTimeZones.delete(validatedTimeZones.values().next().value);
  }
  validatedTimeZones.add(value);
  return value;
}
function validateMetricValue(metric, value) {
  const range = METRIC_BOUNDS[metric];
  if (!Number.isFinite(value) || value < range.minimum || value > range.maximum) {
    throw new RangeError(`${metric} must be between ${range.minimum} and ${range.maximum}`);
  }
  if (range.maximumExclusive === true && value === range.maximum) {
    throw new RangeError(`${metric} must be less than ${range.maximum}`);
  }
}
function validateWeatherRecordMetadata(metadata) {
  if (Object.keys(metadata).some((key) => !METADATA_KEYS.has(key))) {
    throw new RangeError("record metadata contains an unrecognized field");
  }
  const model = validateOptionalMetadataString(metadata.model, "model");
  const device = validateDeviceMetadata(metadata.device);
  const quality = validateMetadataFragment(metadata.quality, QUALITY_KEYS, "quality", 2048);
  const provider = validateMetadataFragment(metadata.provider, PROVIDER_KEYS, "provider", 8192);
  return {
    device,
    model,
    provider,
    quality,
    upstreamTimezone: validateTimeZone(metadata.upstreamTimezone)
  };
}
function validateDeviceMetadata(device) {
  if (device === null) {
    return null;
  }
  if (Object.keys(device).some((key) => !DEVICE_KEYS.has(key))) {
    throw new RangeError("device metadata contains an unrecognized field");
  }
  const normalized = {
    ...device.model === void 0 ? {} : { model: validateMetadataString(device.model, "device.model") },
    ...device.serial === void 0 ? {} : { serial: validateMetadataString(device.serial, "device.serial") },
    ...device.vendor === void 0 ? {} : { vendor: validateMetadataString(device.vendor, "device.vendor") }
  };
  return normalized;
}
function validateOptionalMetadataString(value, fieldName) {
  if (value === null) {
    return null;
  }
  return validateMetadataString(value, fieldName);
}
function validateMetadataString(value, fieldName) {
  if (value.trim().length === 0 || value.length > 128) {
    throw new RangeError(`${fieldName} must be non-empty and at most 128 chars`);
  }
  return value;
}
function validateMetadataFragment(fragment, allowedKeys, fieldName, maximumBytes) {
  if (fragment === null) {
    return null;
  }
  for (const key of Object.keys(fragment)) {
    if (!allowedKeys.has(key)) {
      throw new RangeError(`${fieldName}.${key} is not allowlisted`);
    }
  }
  const serialized = canonicalizeJson(fragment);
  if (serialized.length > maximumBytes) {
    throw new RangeError(`${fieldName} metadata is too large`);
  }
  return { ...fragment };
}

// packages/domain/dist/forecast-anchor-record.js
function validateSha256Hex(value, fieldName) {
  if (!/^[a-f0-9]{64}$/u.test(value)) {
    throw new RangeError(`${fieldName} must be a lowercase SHA-256 hex digest`);
  }
  return value;
}

// packages/domain/dist/forecast-adjustment.js
var FORECAST_ADJUSTMENT_CONTRACT_VERSIONS = {
  candidate: "forecast-adjustment-candidate/v2",
  decision: "forecast-adjustment-decision/v1",
  evaluationReport: "forecast-adjustment-evaluation-report/v2",
  networkEvent: "forecast-network-event/v1",
  observationManifest: "forecast-observation-station-manifest/v1",
  qualificationReceipt: "forecast-adjustment-qualification-receipt/v2",
  registry: "forecast-adjustment-registry/v1",
  runtimeBundle: "forecast-adjustment-runtime-bundle/v2",
  trainingRow: "forecast-training-row/v1",
  windCanaryAuthorization: "forecast-adjustment-wind-canary-authorization/v1",
  windCanaryCandidate: "forecast-adjustment-wind-canary-candidate/v1",
  windCanaryRegistry: "forecast-adjustment-wind-canary-registry/v1",
  windCanaryRuntimeBundle: "forecast-adjustment-wind-canary-runtime-bundle/v1",
  windCanaryTransferReport: "forecast-adjustment-wind-canary-transfer-report/v1"
};
var FORECAST_ADJUSTMENT_METRICS = [
  "relativeHumidityPercent",
  "temperatureC",
  "windDirectionDegrees",
  "windGustMps",
  "windSpeedMps"
];
var FORECAST_LEAD_BANDS = [
  { key: "001-024", maximumHours: 24, minimumHours: 1 },
  { key: "025-048", maximumHours: 48, minimumHours: 25 },
  { key: "049-072", maximumHours: 72, minimumHours: 49 },
  { key: "073-096", maximumHours: 96, minimumHours: 73 },
  { key: "097-120", maximumHours: 120, minimumHours: 97 },
  { key: "121-144", maximumHours: 144, minimumHours: 121 },
  { key: "145-168", maximumHours: 168, minimumHours: 145 }
];
var FORECAST_OBSERVATION_PROVIDER_FAMILIES = [
  "ambient",
  "ecowitt",
  "netatmo",
  "tempest"
];
var FORECAST_OBSERVATION_STATION_KEYS = [
  "ambient-maxweather",
  "ambient-merlin",
  "ballydidean-ecowitt",
  "netatmo-nearby",
  "tempest-126537",
  "tempest-168853",
  "tempest-201058",
  "tempest-203055",
  "tempest-225947",
  "tempest-38270",
  "tempest-64255"
];
var ABSENT_QUALITY = {
  allowedFlags: [],
  statusRule: "absent"
};
var TEMPEST_QUALITY = {
  allowedFlags: ["uv_index_out_of_range"],
  statusRule: "absent"
};
var WUNDERGROUND_QUALITY = {
  allowedFlags: [],
  statusRule: "absent_or_provider_qc_1"
};
var FORECAST_OBSERVATION_SOURCE_LINEAGES = [
  {
    acceptedEndExclusive: null,
    acceptedStartInclusive: "2026-08-24T00:00:00Z",
    adapterContract: "ambient-device-data/v1",
    checkedFingerprint: "7a7528a6278924ca5280a1a6045b6647b7e660b112d7fa3008c542a17ff99df4",
    physicalStationKey: "ambient-maxweather",
    qualityRule: ABSENT_QUALITY,
    sourceKey: "ambient-maxweather-observations-v1",
    supersededSourceKeys: []
  },
  {
    acceptedEndExclusive: null,
    acceptedStartInclusive: "2021-01-01T00:00:00Z",
    adapterContract: "ambient-device-data/v1",
    checkedFingerprint: "c3829701bfc25a050022dc3965569d3a87376e8a43b5fdcb7621533f1ae3c65d",
    physicalStationKey: "ambient-merlin",
    qualityRule: ABSENT_QUALITY,
    sourceKey: "ambient-merlin-observations-v1",
    supersededSourceKeys: []
  },
  {
    acceptedEndExclusive: null,
    acceptedStartInclusive: null,
    adapterContract: "ecowitt-local-live/v1",
    checkedFingerprint: "0a44488714d0fa807b924f8aea14965b437722e8cf9f8eae4bc8c81da8a0149d",
    physicalStationKey: "ballydidean-ecowitt",
    qualityRule: ABSENT_QUALITY,
    sourceKey: "ecowitt-88f15505d89f-local-live-v1",
    supersededSourceKeys: []
  },
  {
    acceptedEndExclusive: null,
    acceptedStartInclusive: "2022-06-21T00:00:00Z",
    adapterContract: "netatmo-public-measures/v1",
    checkedFingerprint: "5495917dd2465a32d9878e73c68781a229b432901cdc4867875726351efbdbbc",
    physicalStationKey: "netatmo-nearby",
    qualityRule: ABSENT_QUALITY,
    sourceKey: "netatmo-nearby-observations-v1",
    supersededSourceKeys: []
  },
  {
    acceptedEndExclusive: null,
    acceptedStartInclusive: "2023-12-17T00:00:00Z",
    adapterContract: "tempest-observations/v2",
    checkedFingerprint: "34dafbd6584c93d55ed4d3d43dc7e74a0876165d4ddfc921413f7b826dff7ab7",
    physicalStationKey: "tempest-126537",
    qualityRule: TEMPEST_QUALITY,
    sourceKey: "tempest-126537-observations-v2",
    supersededSourceKeys: ["tempest-126537-observations-v1"]
  },
  {
    acceptedEndExclusive: null,
    acceptedStartInclusive: "2025-01-22T00:00:00Z",
    adapterContract: "tempest-observations/v2",
    checkedFingerprint: "1c7a402337a44a5441775246cbc02da7994599dad6ab83dc04b248303facfea5",
    physicalStationKey: "tempest-168853",
    qualityRule: TEMPEST_QUALITY,
    sourceKey: "tempest-168853-observations-v2",
    supersededSourceKeys: ["tempest-168853-observations-v1"]
  },
  {
    acceptedEndExclusive: null,
    acceptedStartInclusive: "2025-12-22T00:00:00Z",
    adapterContract: "tempest-observations/v2",
    checkedFingerprint: "a61cce798cddf682da9608dc245659fc7734a6d5304068939d3115ae7d81a50e",
    physicalStationKey: "tempest-201058",
    qualityRule: TEMPEST_QUALITY,
    sourceKey: "tempest-201058-observations-v2",
    supersededSourceKeys: ["tempest-201058-observations-v1"]
  },
  {
    acceptedEndExclusive: null,
    acceptedStartInclusive: "2025-12-25T00:00:00Z",
    adapterContract: "tempest-observations/v2",
    checkedFingerprint: "9ead4c5359a6a9640f334be91397180aa62b90b0f0ce813b9ff26fe84537acc4",
    physicalStationKey: "tempest-203055",
    qualityRule: TEMPEST_QUALITY,
    sourceKey: "tempest-203055-observations-v2",
    supersededSourceKeys: ["tempest-203055-observations-v1"]
  },
  {
    acceptedEndExclusive: null,
    acceptedStartInclusive: "2026-07-14T00:00:00Z",
    adapterContract: "tempest-observations/v2",
    checkedFingerprint: "b4dd6105d9a56a7c5d0dc4063f830e1cf28d693222a8de15536dd83d3a6178c4",
    physicalStationKey: "tempest-225947",
    qualityRule: TEMPEST_QUALITY,
    sourceKey: "tempest-225947-observations-v2",
    supersededSourceKeys: ["tempest-225947-observations-v1"]
  },
  {
    acceptedEndExclusive: null,
    acceptedStartInclusive: "2021-01-04T00:00:00Z",
    adapterContract: "tempest-observations/v2",
    checkedFingerprint: "ce162067aced4ab3522fb83145a21e608ff24dec189097726188e96fd6cca52f",
    physicalStationKey: "tempest-38270",
    qualityRule: TEMPEST_QUALITY,
    sourceKey: "tempest-38270-observations-v2",
    supersededSourceKeys: ["tempest-38270-observations-v1"]
  },
  {
    acceptedEndExclusive: null,
    acceptedStartInclusive: "2021-12-10T00:00:00Z",
    adapterContract: "tempest-observations/v2",
    checkedFingerprint: "8eb488a358375fc3526347d9ef6c9f23080095a22ea874a42ec400b0317d868a",
    physicalStationKey: "tempest-64255",
    qualityRule: TEMPEST_QUALITY,
    sourceKey: "tempest-64255-observations-v2",
    supersededSourceKeys: ["tempest-64255-observations-v1"]
  },
  {
    acceptedEndExclusive: "2026-08-24T00:00:00Z",
    acceptedStartInclusive: "2024-11-29T00:00:00Z",
    adapterContract: "wunderground-pws-history/v1",
    checkedFingerprint: "52dda6c5444d0a234fbe23d6218027d417ac966ecf291a7d5dfff42fd0dc207c",
    physicalStationKey: "ambient-maxweather",
    qualityRule: WUNDERGROUND_QUALITY,
    sourceKey: "wunderground-maxweather-history-v1",
    supersededSourceKeys: []
  }
];
var FORECAST_OBSERVATION_EXCLUDED_SOURCE_LINEAGES = [
  {
    acceptedIntervals: [],
    physicalStationKey: "tempest-126537",
    reasonCode: "source_superseded",
    sourceKey: "tempest-126537-observations-v1",
    successorSourceKey: "tempest-126537-observations-v2"
  },
  {
    acceptedIntervals: [],
    physicalStationKey: "tempest-168853",
    reasonCode: "source_superseded",
    sourceKey: "tempest-168853-observations-v1",
    successorSourceKey: "tempest-168853-observations-v2"
  },
  {
    acceptedIntervals: [],
    physicalStationKey: "tempest-201058",
    reasonCode: "source_superseded",
    sourceKey: "tempest-201058-observations-v1",
    successorSourceKey: "tempest-201058-observations-v2"
  },
  {
    acceptedIntervals: [],
    physicalStationKey: "tempest-203055",
    reasonCode: "source_superseded",
    sourceKey: "tempest-203055-observations-v1",
    successorSourceKey: "tempest-203055-observations-v2"
  },
  {
    acceptedIntervals: [],
    physicalStationKey: "tempest-225947",
    reasonCode: "source_superseded",
    sourceKey: "tempest-225947-observations-v1",
    successorSourceKey: "tempest-225947-observations-v2"
  },
  {
    acceptedIntervals: [],
    physicalStationKey: "tempest-38270",
    reasonCode: "source_superseded",
    sourceKey: "tempest-38270-observations-v1",
    successorSourceKey: "tempest-38270-observations-v2"
  },
  {
    acceptedIntervals: [],
    physicalStationKey: "tempest-64255",
    reasonCode: "source_superseded",
    sourceKey: "tempest-64255-observations-v1",
    successorSourceKey: "tempest-64255-observations-v2"
  }
];
var FORECAST_OBSERVATION_EXCLUSION_REASON_CODES = [
  "metric_ineligible",
  "metric_missing",
  "quality_flag_rejected",
  "quality_status_rejected",
  "source_interval_out_of_range",
  "source_superseded",
  "station_coverage_insufficient",
  "station_direction_calm",
  "station_gust_coverage_incomplete"
];
var FORECAST_OBSERVATION_STATIONS = [
  {
    acceptedSourceKeys: [
      "ambient-maxweather-observations-v1",
      "wunderground-maxweather-history-v1"
    ],
    distanceMeters: 1183.45263477189,
    eligibleMetrics: FORECAST_ADJUSTMENT_METRICS,
    key: "ambient-maxweather",
    latitude: 47.9438,
    longitude: -122.4404,
    nearestRank: 7,
    providerFamily: "ambient",
    unnormalizedSpatialWeight: 0.740663912119109
  },
  {
    acceptedSourceKeys: ["ambient-merlin-observations-v1"],
    distanceMeters: 910.894120029186,
    eligibleMetrics: FORECAST_ADJUSTMENT_METRICS,
    key: "ambient-merlin",
    latitude: 47.9551126,
    longitude: -122.4179341,
    nearestRank: 3,
    providerFamily: "ambient",
    unnormalizedSpatialWeight: 0.828203973166963
  },
  {
    acceptedSourceKeys: ["ecowitt-88f15505d89f-local-live-v1"],
    distanceMeters: 0,
    eligibleMetrics: FORECAST_ADJUSTMENT_METRICS,
    key: "ballydidean-ecowitt",
    latitude: 47.950429954185445,
    longitude: -122.42797012608193,
    nearestRank: 1,
    providerFamily: "ecowitt",
    unnormalizedSpatialWeight: 1
  },
  {
    acceptedSourceKeys: ["netatmo-nearby-observations-v1"],
    distanceMeters: 1875.65238057652,
    eligibleMetrics: FORECAST_ADJUSTMENT_METRICS,
    key: "netatmo-nearby",
    latitude: 47.964228,
    longitude: -122.442459,
    nearestRank: 11,
    providerFamily: "netatmo",
    unnormalizedSpatialWeight: 0.5320513129347486
  },
  {
    acceptedSourceKeys: ["tempest-126537-observations-v2"],
    distanceMeters: 1398.67236054504,
    eligibleMetrics: FORECAST_ADJUSTMENT_METRICS,
    key: "tempest-126537",
    latitude: 47.9582,
    longitude: -122.44274,
    nearestRank: 8,
    providerFamily: "tempest",
    unnormalizedSpatialWeight: 0.6715596083191008
  },
  {
    acceptedSourceKeys: ["tempest-168853-observations-v2"],
    distanceMeters: 1077.20962495532,
    eligibleMetrics: FORECAST_ADJUSTMENT_METRICS,
    key: "tempest-168853",
    latitude: 47.95498,
    longitude: -122.44074,
    nearestRank: 6,
    providerFamily: "tempest",
    unnormalizedSpatialWeight: 0.775136628203077
  },
  {
    acceptedSourceKeys: ["tempest-201058-observations-v2"],
    distanceMeters: 1401.73955268213,
    eligibleMetrics: FORECAST_ADJUSTMENT_METRICS,
    key: "tempest-201058",
    latitude: 47.96244,
    longitude: -122.43369,
    nearestRank: 9,
    providerFamily: "tempest",
    unnormalizedSpatialWeight: 0.670592564378282
  },
  {
    acceptedSourceKeys: ["tempest-203055-observations-v2"],
    distanceMeters: 1651.02362971156,
    eligibleMetrics: FORECAST_ADJUSTMENT_METRICS,
    key: "tempest-203055",
    latitude: 47.96505,
    longitude: -122.4241,
    nearestRank: 10,
    providerFamily: "tempest",
    unnormalizedSpatialWeight: 0.5947178033706937
  },
  {
    acceptedSourceKeys: ["tempest-225947-observations-v2"],
    distanceMeters: 940.077920837135,
    eligibleMetrics: FORECAST_ADJUSTMENT_METRICS,
    key: "tempest-225947",
    latitude: 47.94215,
    longitude: -122.42542,
    nearestRank: 4,
    providerFamily: "tempest",
    unnormalizedSpatialWeight: 0.8190433312327082
  },
  {
    acceptedSourceKeys: ["tempest-38270-observations-v2"],
    distanceMeters: 1066.83643435427,
    eligibleMetrics: FORECAST_ADJUSTMENT_METRICS,
    key: "tempest-38270",
    latitude: 47.95293,
    longitude: -122.41414,
    nearestRank: 5,
    providerFamily: "tempest",
    unnormalizedSpatialWeight: 0.7784918311659549
  },
  {
    acceptedSourceKeys: ["tempest-64255-observations-v2"],
    distanceMeters: 883.385696754924,
    eligibleMetrics: FORECAST_ADJUSTMENT_METRICS,
    key: "tempest-64255",
    latitude: 47.95008,
    longitude: -122.43982,
    nearestRank: 2,
    providerFamily: "tempest",
    unnormalizedSpatialWeight: 0.8367552632922316
  }
];
var FORECAST_OBSERVATION_MANIFEST_V1 = {
  aggregationContractVersion: "physical-station-network/v1",
  collisionPolicy: "reject",
  contractVersion: FORECAST_ADJUSTMENT_CONTRACT_VERSIONS.observationManifest,
  directionAggregation: "spatially_weighted_vector_mean",
  directionMinimumResultantVector: 0.25,
  directionMinimumWindSpeedMps: 1,
  earthRadiusMeters: 63710088e-1,
  eligibleMetrics: FORECAST_ADJUSTMENT_METRICS,
  gustMaximumGapMinutes: 10,
  gustWindowEndInclusiveMinutes: 0,
  gustWindowStartExclusiveMinutes: -60,
  gapPolicy: "missing",
  instantWindowEndExclusiveMinutes: 5,
  instantWindowStartInclusiveMinutes: -5,
  minimumEligibleStations: 3,
  nearestEligibleStationCount: 3,
  networkEventWeight: 1,
  preAggregationIdentity: "physicalStationKey,validAt,metric",
  requiresNearestEligibleStation: true,
  scalarAggregation: "deterministic_spatially_weighted_median",
  site: {
    key: "ballydidean",
    latitude: 47.950429954185445,
    longitude: -122.42797012608193,
    timezone: "America/Los_Angeles"
  },
  excludedSourceLineages: FORECAST_OBSERVATION_EXCLUDED_SOURCE_LINEAGES,
  sourceLineages: FORECAST_OBSERVATION_SOURCE_LINEAGES,
  spatialFormula: "1/(1+(distanceMeters/2000)^2)",
  spatialNormalizationOrder: "physical_station_key_lexicographic",
  spatialScaleMeters: 2e3,
  stations: FORECAST_OBSERVATION_STATIONS
};
var FORECAST_ADJUSTMENT_METRIC_POLICIES_V1 = [
  {
    correctionMaximum: 20,
    correctionMinimum: -20,
    finalMaximum: 100,
    finalMaximumExclusive: false,
    finalMinimum: 0,
    metric: "relativeHumidityPercent",
    wrapsFinalValue: false
  },
  {
    correctionMaximum: 5,
    correctionMinimum: -5,
    finalMaximum: 70,
    finalMaximumExclusive: false,
    finalMinimum: -100,
    metric: "temperatureC",
    wrapsFinalValue: false
  },
  {
    correctionMaximum: 45,
    correctionMinimum: -45,
    finalMaximum: 360,
    finalMaximumExclusive: true,
    finalMinimum: 0,
    metric: "windDirectionDegrees",
    wrapsFinalValue: true
  },
  {
    correctionMaximum: 12,
    correctionMinimum: -12,
    finalMaximum: 150,
    finalMaximumExclusive: false,
    finalMinimum: 0,
    metric: "windGustMps",
    wrapsFinalValue: false
  },
  {
    correctionMaximum: 8,
    correctionMinimum: -8,
    finalMaximum: 150,
    finalMaximumExclusive: false,
    finalMinimum: 0,
    metric: "windSpeedMps",
    wrapsFinalValue: false
  }
];
var FORECAST_NEAREST_THREE_SLICE_KEY = "nearest-three";
var FORECAST_SEASON_DAYPART_KEYS = [
  "autumn-afternoon",
  "autumn-evening",
  "autumn-morning",
  "autumn-night",
  "spring-afternoon",
  "spring-evening",
  "spring-morning",
  "spring-night",
  "summer-afternoon",
  "summer-evening",
  "summer-morning",
  "summer-night",
  "winter-afternoon",
  "winter-evening",
  "winter-morning",
  "winter-night"
];
var FORECAST_ADJUSTMENT_QUALIFICATION_GATE_NAMES = [
  "pooled_network_improvement",
  "bootstrap_lower_bound",
  "development_fold_skill",
  "critical_slice_no_harm",
  "coefficient_coverage_and_caps",
  "locked_holdout",
  "production_identity"
];
var FORECAST_ADJUSTMENT_REASON_CODES = [
  "adjustment_error",
  "bundle_invalid",
  "bundle_missing",
  "canary_expired",
  "canary_killed",
  "coefficient_missing",
  "cross_link_mismatch",
  "direction_calm",
  "evidence_redundancy_missing",
  "hash_mismatch",
  "identity_mismatch",
  "insufficient_data",
  "metric_not_enabled",
  "metric_out_of_bounds",
  "policy_raw",
  "qualification_failed",
  "registry_inactive",
  "registry_invalid",
  "runtime_fingerprint_mismatch",
  "training_envelope_mismatch",
  "unsupported_lead",
  "wrong_cohort"
];
var CANDIDATE_KEYS = /* @__PURE__ */ new Set([
  "algorithmContractVersion",
  "candidateArtifactSha256",
  "coefficientPayloadSha256",
  "coefficients",
  "contractVersion",
  "developmentReportSha256",
  "enabledMetricBands",
  "evaluationEpochId",
  "exportManifestSha256",
  "finalTrainingCutoff",
  "forecastIdentity",
  "metricPolicies",
  "runtimeFingerprint",
  "siteKey",
  "timezone",
  "trainingEnvelopes",
  "trainingProvenance"
]);
var EVALUATION_REPORT_KEYS = /* @__PURE__ */ new Set([
  "candidateArtifactSha256",
  "contractVersion",
  "enabledMetricBands",
  "evaluationEpochId",
  "evaluationReportSha256",
  "holdoutAccessMarkerSha256",
  "holdoutEndExclusive",
  "holdoutEndLocalDate",
  "holdoutStartInclusive",
  "holdoutStartLocalDate",
  "metricBandEvaluations",
  "preregistrationSha256",
  "trainingProvenance"
]);
var QUALIFICATION_RECEIPT_KEYS = /* @__PURE__ */ new Set([
  "candidateArtifactSha256",
  "contractVersion",
  "enabledMetricBands",
  "evaluationEpochId",
  "evaluationReportSha256",
  "evidenceRedundancy",
  "gates",
  "holdoutAccessMarkerSha256",
  "lifecycleState",
  "passed",
  "preregistrationSha256",
  "qualificationReceiptSha256",
  "trainingProvenance"
]);
var METRIC_BAND_KEYS = /* @__PURE__ */ new Set(["leadBand", "metric"]);
var FORECAST_IDENTITY_KEYS = /* @__PURE__ */ new Set([
  "adapterVersion",
  "cohort",
  "contractEpoch",
  "dataset",
  "referenceKind",
  "sourceConfigFingerprint",
  "sourceKey",
  "upstreamModel"
]);
var RUNTIME_FINGERPRINT_KEYS = /* @__PURE__ */ new Set(["icuVersion", "tzdataVersion"]);
var COEFFICIENT_KEYS = /* @__PURE__ */ new Set([
  "coefficient",
  "daypart",
  "effectiveEventCount",
  "leadBand",
  "level",
  "metric",
  "month",
  "season"
]);
var TRAINING_ENVELOPE_KEYS = /* @__PURE__ */ new Set([
  "leadBand",
  "maximum",
  "metric",
  "minimum"
]);
var METRIC_POLICY_KEYS = /* @__PURE__ */ new Set([
  "correctionMaximum",
  "correctionMinimum",
  "finalMaximum",
  "finalMaximumExclusive",
  "finalMinimum",
  "metric",
  "wrapsFinalValue"
]);
var PAIRED_SCORE_KEYS = /* @__PURE__ */ new Set([
  "adjustedLoss",
  "bootstrapLowerBound",
  "bootstrapUpperBound",
  "eventCount",
  "rawLoss",
  "skill"
]);
var CRITICAL_SLICE_SCORE_KEYS = /* @__PURE__ */ new Set([
  ...PAIRED_SCORE_KEYS,
  "key",
  "kind"
]);
var METRIC_BAND_EVALUATION_KEYS = /* @__PURE__ */ new Set([
  "criticalSlices",
  "evaluatedSeasonDaypartKeys",
  "metricBand",
  "network",
  "providerBalanced",
  "scoreableStationKeys"
]);
var QUALIFICATION_GATE_KEYS = /* @__PURE__ */ new Set([
  "metricBand",
  "name",
  "passed",
  "reasonCode"
]);
var EVIDENCE_REDUNDANCY_KEYS = /* @__PURE__ */ new Set([
  "attestationSha256",
  "status",
  "verified"
]);
var TRAINING_PROVENANCE_KEYS = /* @__PURE__ */ new Set([
  "aggregationContractSha256",
  "coordinateManifestSha256",
  "metricEligibilitySha256",
  "observationSourceLineageSha256",
  "observationStationManifestSha256",
  "spatialWeightSha256"
]);
function parseForecastAdjustmentReasonCode(value) {
  if (!FORECAST_ADJUSTMENT_REASON_CODES.some((reason) => reason === value)) {
    throw new RangeError(`unsupported forecast adjustment reason code: ${value}`);
  }
  return value;
}
function validatePromotableForecastAdjustmentEvidence(evidence) {
  const { candidate, evaluationReport, qualificationReceipt } = evidence;
  validateCandidateContract(candidate);
  validateEvaluationReportContract(evaluationReport);
  validateQualificationReceiptContract(qualificationReceipt);
  if (evaluationReport.candidateArtifactSha256 !== candidate.candidateArtifactSha256) {
    throw new RangeError("evaluation report candidate cross-link mismatch");
  }
  if (qualificationReceipt.candidateArtifactSha256 !== candidate.candidateArtifactSha256 || qualificationReceipt.evaluationReportSha256 !== evaluationReport.evaluationReportSha256 || qualificationReceipt.preregistrationSha256 !== evaluationReport.preregistrationSha256 || qualificationReceipt.holdoutAccessMarkerSha256 !== evaluationReport.holdoutAccessMarkerSha256) {
    throw new RangeError("qualification receipt evidence cross-link mismatch");
  }
  if (candidate.evaluationEpochId !== evaluationReport.evaluationEpochId || candidate.evaluationEpochId !== qualificationReceipt.evaluationEpochId) {
    throw new RangeError("evaluation epoch cross-link mismatch");
  }
  if (Date.parse(candidate.finalTrainingCutoff) >= Date.parse(evaluationReport.holdoutStartInclusive)) {
    throw new RangeError("candidate training cutoff must precede holdout");
  }
  if (canonicalizeJson(candidate.enabledMetricBands) !== canonicalizeJson(evaluationReport.enabledMetricBands) || canonicalizeJson(candidate.enabledMetricBands) !== canonicalizeJson(qualificationReceipt.enabledMetricBands)) {
    throw new RangeError("enabled metric-band cross-link mismatch");
  }
  if (canonicalizeJson(candidate.trainingProvenance) !== canonicalizeJson(evaluationReport.trainingProvenance) || canonicalizeJson(candidate.trainingProvenance) !== canonicalizeJson(qualificationReceipt.trainingProvenance)) {
    throw new RangeError("training provenance cross-link mismatch");
  }
  if (qualificationReceipt.passed !== true || qualificationReceipt.lifecycleState !== "qualified" || qualificationReceipt.gates.some((gate) => gate.passed !== true)) {
    throw new RangeError("qualification receipt is not passing");
  }
  if (qualificationReceipt.evidenceRedundancy.verified !== true) {
    throw new RangeError("qualification evidence redundancy is not verified");
  }
}
function validateCandidateContract(candidate) {
  rejectUnknownKeys(candidate, CANDIDATE_KEYS, "forecast adjustment candidate");
  if (candidate.contractVersion !== FORECAST_ADJUSTMENT_CONTRACT_VERSIONS.candidate) {
    throw new RangeError("unsupported forecast adjustment candidate contract");
  }
  if (candidate.algorithmContractVersion !== "robust-hierarchical-median/v1" || candidate.siteKey !== "ballydidean" || candidate.timezone !== "America/Los_Angeles") {
    throw new RangeError("candidate algorithm or site identity mismatch");
  }
  if (candidate.forecastIdentity.cohort !== "legacy_v4_retrieval_snapshot" || candidate.forecastIdentity.referenceKind !== "retrieval_snapshot") {
    throw new RangeError("candidate forecast cohort identity mismatch");
  }
  rejectUnknownKeys(candidate.forecastIdentity, FORECAST_IDENTITY_KEYS, "candidate forecast identity");
  rejectUnknownKeys(candidate.runtimeFingerprint, RUNTIME_FINGERPRINT_KEYS, "candidate runtime fingerprint");
  validateSha256Hex(candidate.candidateArtifactSha256, "candidateArtifactSha256");
  validateSha256Hex(candidate.coefficientPayloadSha256, "coefficientPayloadSha256");
  validateSha256Hex(candidate.developmentReportSha256, "developmentReportSha256");
  validateSha256Hex(candidate.exportManifestSha256, "exportManifestSha256");
  validateSha256Hex(candidate.forecastIdentity.sourceConfigFingerprint, "forecastIdentity.sourceConfigFingerprint");
  validateTrainingProvenanceHashes(candidate.trainingProvenance);
  validateEnabledMetricBands(candidate.enabledMetricBands);
  validateUtcInstant(candidate.finalTrainingCutoff, "finalTrainingCutoff");
  validateBoundedText(candidate.evaluationEpochId, "evaluationEpochId");
  validateBoundedText(candidate.forecastIdentity.adapterVersion, "adapterVersion");
  validateBoundedText(candidate.forecastIdentity.contractEpoch, "contractEpoch");
  validateBoundedText(candidate.forecastIdentity.dataset, "dataset");
  validateBoundedText(candidate.forecastIdentity.sourceKey, "sourceKey");
  validateBoundedText(candidate.forecastIdentity.upstreamModel, "upstreamModel");
  validateBoundedText(candidate.runtimeFingerprint.icuVersion, "icuVersion");
  validateBoundedText(candidate.runtimeFingerprint.tzdataVersion, "tzdataVersion");
  if (canonicalizeJson(candidate.metricPolicies) !== canonicalizeJson(FORECAST_ADJUSTMENT_METRIC_POLICIES_V1)) {
    throw new RangeError("candidate metric policies do not match v1");
  }
  for (const policy of candidate.metricPolicies) {
    rejectUnknownKeys(policy, METRIC_POLICY_KEYS, "candidate metric policy");
  }
  validateCandidateCoefficientCoverage(candidate);
  validateCandidateTrainingEnvelopeCoverage(candidate);
}
function validateEvaluationReportContract(report) {
  rejectUnknownKeys(report, EVALUATION_REPORT_KEYS, "forecast adjustment evaluation report");
  if (report.contractVersion !== FORECAST_ADJUSTMENT_CONTRACT_VERSIONS.evaluationReport) {
    throw new RangeError("unsupported forecast adjustment evaluation report");
  }
  validateSha256Hex(report.candidateArtifactSha256, "candidateArtifactSha256");
  validateSha256Hex(report.evaluationReportSha256, "evaluationReportSha256");
  validateSha256Hex(report.holdoutAccessMarkerSha256, "holdoutAccessMarkerSha256");
  validateSha256Hex(report.preregistrationSha256, "preregistrationSha256");
  validateTrainingProvenanceHashes(report.trainingProvenance);
  validateEnabledMetricBands(report.enabledMetricBands);
  validateBoundedText(report.evaluationEpochId, "evaluationEpochId");
  validateHoldoutBounds(report);
  if (report.metricBandEvaluations.length !== report.enabledMetricBands.length) {
    throw new RangeError("evaluation coverage must match the enabled metric-band set");
  }
  for (const [index, evaluation] of report.metricBandEvaluations.entries()) {
    rejectUnknownKeys(evaluation, METRIC_BAND_EVALUATION_KEYS, "metric-band evaluation");
    validateMetricBand(evaluation.metricBand);
    const enabledPair = report.enabledMetricBands[index];
    if (enabledPair === void 0 || metricBandKey(evaluation.metricBand) !== metricBandKey(enabledPair)) {
      throw new RangeError("metric-band evaluation order or coverage mismatch");
    }
    validatePairedScore(evaluation.network, "network score");
    validatePairedScore(evaluation.providerBalanced, "provider-balanced score");
    if (evaluation.network.skill < 0.02 || evaluation.network.bootstrapLowerBound <= 0 || isMaterialHarm(evaluation.providerBalanced)) {
      throw new RangeError("metric-band evaluation does not meet promotable thresholds");
    }
    for (const slice of evaluation.criticalSlices) {
      rejectUnknownKeys(slice, CRITICAL_SLICE_SCORE_KEYS, "critical slice score");
      validateBoundedText(slice.key, "criticalSlice.key");
      validateCriticalSliceKind(slice.kind);
      validatePairedScore(slice, "critical slice score", CRITICAL_SLICE_SCORE_KEYS);
      if (slice.eventCount >= 100 && isMaterialHarm(slice)) {
        throw new RangeError("critical slice has material harm");
      }
    }
    validateCriticalSliceCoverage(evaluation.criticalSlices, evaluation.scoreableStationKeys, evaluation.evaluatedSeasonDaypartKeys);
  }
}
function validateQualificationReceiptContract(receipt) {
  rejectUnknownKeys(receipt, QUALIFICATION_RECEIPT_KEYS, "forecast adjustment qualification receipt");
  if (receipt.contractVersion !== FORECAST_ADJUSTMENT_CONTRACT_VERSIONS.qualificationReceipt) {
    throw new RangeError("unsupported forecast adjustment qualification receipt");
  }
  validateSha256Hex(receipt.candidateArtifactSha256, "candidateArtifactSha256");
  validateSha256Hex(receipt.evaluationReportSha256, "evaluationReportSha256");
  validateSha256Hex(receipt.evidenceRedundancy.attestationSha256, "evidenceRedundancy.attestationSha256");
  validateSha256Hex(receipt.holdoutAccessMarkerSha256, "holdoutAccessMarkerSha256");
  validateSha256Hex(receipt.preregistrationSha256, "preregistrationSha256");
  validateSha256Hex(receipt.qualificationReceiptSha256, "qualificationReceiptSha256");
  validateTrainingProvenanceHashes(receipt.trainingProvenance);
  validateEnabledMetricBands(receipt.enabledMetricBands);
  validateBoundedText(receipt.evaluationEpochId, "evaluationEpochId");
  rejectUnknownKeys(receipt.evidenceRedundancy, EVIDENCE_REDUNDANCY_KEYS, "qualification evidence redundancy");
  if (receipt.evidenceRedundancy.status !== "independent_content_addressed_copy" && receipt.evidenceRedundancy.status !== "restorable_encrypted_backup") {
    throw new RangeError("unsupported qualification evidence redundancy status");
  }
  const requiredGateCount = receipt.enabledMetricBands.length * FORECAST_ADJUSTMENT_QUALIFICATION_GATE_NAMES.length;
  if (receipt.gates.length !== requiredGateCount) {
    throw new RangeError("qualification receipt gate cardinality mismatch");
  }
  for (const [index, gate] of receipt.gates.entries()) {
    rejectUnknownKeys(gate, QUALIFICATION_GATE_KEYS, "qualification gate");
    validateMetricBand(gate.metricBand);
    const pairIndex = Math.floor(index / FORECAST_ADJUSTMENT_QUALIFICATION_GATE_NAMES.length);
    const gateIndex = index % FORECAST_ADJUSTMENT_QUALIFICATION_GATE_NAMES.length;
    const expectedPair = receipt.enabledMetricBands[pairIndex];
    const expectedGateName = FORECAST_ADJUSTMENT_QUALIFICATION_GATE_NAMES[gateIndex];
    if (expectedPair === void 0 || expectedGateName === void 0 || metricBandKey(gate.metricBand) !== metricBandKey(expectedPair) || gate.name !== expectedGateName) {
      throw new RangeError("qualification receipt gate coverage mismatch");
    }
    if (gate.passed && gate.reasonCode !== null) {
      throw new RangeError("passing qualification gate must not have a reason code");
    }
    if (!gate.passed && gate.reasonCode === null) {
      throw new RangeError("failed qualification gate requires a reason code");
    }
    if (gate.reasonCode !== null) {
      parseForecastAdjustmentReasonCode(gate.reasonCode);
    }
  }
  const allGatesPass = receipt.gates.every((gate) => gate.passed);
  if (receipt.passed !== allGatesPass || receipt.passed && receipt.lifecycleState !== "qualified" || !receipt.passed && receipt.lifecycleState !== "rejected") {
    throw new RangeError("qualification receipt state does not match its gates");
  }
}
function validateCandidateCoefficientCoverage(candidate) {
  const enabledKeys = new Set(candidate.enabledMetricBands.map(metricBandKey));
  const rootCounts = /* @__PURE__ */ new Map();
  const cellKeys = /* @__PURE__ */ new Set();
  for (const key of enabledKeys) {
    rootCounts.set(key, 0);
  }
  for (const coefficient of candidate.coefficients) {
    rejectUnknownKeys(coefficient, COEFFICIENT_KEYS, "candidate coefficient");
    validateMetricBand({
      leadBand: coefficient.leadBand,
      metric: coefficient.metric
    });
    const pairKey = metricBandKey(coefficient);
    if (!enabledKeys.has(pairKey)) {
      throw new RangeError("candidate coefficient is outside the enabled set");
    }
    if (!Number.isFinite(coefficient.coefficient) || !Number.isFinite(coefficient.effectiveEventCount) || coefficient.effectiveEventCount < 0) {
      throw new RangeError("candidate coefficient values must be finite and nonnegative");
    }
    const policy = FORECAST_ADJUSTMENT_METRIC_POLICIES_V1.find((candidatePolicy) => candidatePolicy.metric === coefficient.metric);
    if (policy === void 0 || coefficient.coefficient < policy.correctionMinimum || coefficient.coefficient > policy.correctionMaximum) {
      throw new RangeError("candidate coefficient exceeds its metric cap");
    }
    validateCoefficientHierarchy(coefficient);
    const cellKey = canonicalizeJson({
      daypart: coefficient.daypart,
      leadBand: coefficient.leadBand,
      level: coefficient.level,
      metric: coefficient.metric,
      month: coefficient.month,
      season: coefficient.season
    });
    if (cellKeys.has(cellKey)) {
      throw new RangeError("candidate contains a duplicate coefficient cell");
    }
    cellKeys.add(cellKey);
    if (coefficient.level === 1) {
      rootCounts.set(pairKey, (rootCounts.get(pairKey) ?? 0) + 1);
    }
  }
  for (const [key, count] of rootCounts) {
    if (count !== 1) {
      throw new RangeError(`enabled metric-band requires exactly one root: ${key}`);
    }
  }
}
function validateCoefficientHierarchy(coefficient) {
  if (coefficient.level === 1) {
    if (coefficient.daypart !== null || coefficient.month !== null || coefficient.season !== null || coefficient.effectiveEventCount < 200) {
      throw new RangeError("level-1 coefficient shape or count is invalid");
    }
    return;
  }
  if (coefficient.level === 2) {
    if (coefficient.daypart === null || coefficient.month !== null || coefficient.season === null || coefficient.effectiveEventCount < 100) {
      throw new RangeError("level-2 coefficient shape or count is invalid");
    }
    validateDaypart(coefficient.daypart);
    validateSeason(coefficient.season);
    return;
  }
  if (coefficient.level === 3) {
    if (coefficient.daypart === null || coefficient.season !== null || !Number.isInteger(coefficient.month) || coefficient.month === null || coefficient.month < 1 || coefficient.month > 12 || coefficient.effectiveEventCount < 50) {
      throw new RangeError("level-3 coefficient shape or count is invalid");
    }
    validateDaypart(coefficient.daypart);
    return;
  }
  throw new RangeError("unsupported candidate coefficient level");
}
function validateCandidateTrainingEnvelopeCoverage(candidate) {
  const enabledScalarKeys = new Set(candidate.enabledMetricBands.filter((pair) => pair.metric !== "windDirectionDegrees").map(metricBandKey));
  const envelopeKeys = /* @__PURE__ */ new Set();
  for (const envelope of candidate.trainingEnvelopes) {
    rejectUnknownKeys(envelope, TRAINING_ENVELOPE_KEYS, "training envelope");
    validateMetricBand({ leadBand: envelope.leadBand, metric: envelope.metric });
    const key = metricBandKey(envelope);
    if (!enabledScalarKeys.has(key) || envelopeKeys.has(key) || !Number.isFinite(envelope.minimum) || !Number.isFinite(envelope.maximum) || envelope.minimum > envelope.maximum) {
      throw new RangeError("candidate training envelope coverage is invalid");
    }
    validateMetricValue(envelope.metric, envelope.minimum);
    validateMetricValue(envelope.metric, envelope.maximum);
    envelopeKeys.add(key);
  }
  if (envelopeKeys.size !== enabledScalarKeys.size || [...enabledScalarKeys].some((key) => !envelopeKeys.has(key))) {
    throw new RangeError("enabled scalar metric-band requires one training envelope");
  }
}
function validatePairedScore(score, fieldName, allowedKeys = PAIRED_SCORE_KEYS) {
  rejectUnknownKeys(score, allowedKeys, fieldName);
  if (!Number.isFinite(score.rawLoss) || score.rawLoss < 0 || !Number.isFinite(score.adjustedLoss) || score.adjustedLoss < 0 || !Number.isFinite(score.bootstrapLowerBound) || !Number.isFinite(score.bootstrapUpperBound) || score.bootstrapLowerBound > score.bootstrapUpperBound || !Number.isFinite(score.skill) || !Number.isSafeInteger(score.eventCount) || score.eventCount < 1) {
    throw new RangeError(`${fieldName} contains invalid values`);
  }
  const expectedSkill = score.rawLoss === 0 ? score.adjustedLoss === 0 ? 0 : -1 : (score.rawLoss - score.adjustedLoss) / score.rawLoss;
  if (Math.abs(score.skill - expectedSkill) > Number.EPSILON * 8) {
    throw new RangeError(`${fieldName} skill does not match paired losses`);
  }
}
function isMaterialHarm(score) {
  return score.skill <= -0.02 && score.bootstrapUpperBound < 0;
}
function validateMetricBand(pair) {
  rejectUnknownKeys(pair, METRIC_BAND_KEYS, "forecast adjustment metric-band");
  validateForecastAdjustmentMetric(pair.metric);
  if (!FORECAST_LEAD_BANDS.some((band) => band.key === pair.leadBand)) {
    throw new RangeError(`unsupported forecast lead band: ${pair.leadBand}`);
  }
}
function metricBandKey(pair) {
  return `${pair.metric}:${pair.leadBand}`;
}
function validateHoldoutBounds(report) {
  const startInclusive = Date.parse(validateUtcInstant(report.holdoutStartInclusive, "holdoutStartInclusive"));
  const endExclusive = Date.parse(validateUtcInstant(report.holdoutEndExclusive, "holdoutEndExclusive"));
  const startLocalDate = parseLocalDate(report.holdoutStartLocalDate, "holdoutStartLocalDate");
  const endLocalDate = parseLocalDate(report.holdoutEndLocalDate, "holdoutEndLocalDate");
  if (startInclusive >= endExclusive || (endLocalDate - startLocalDate) / 864e5 !== 29) {
    throw new RangeError("holdout bounds must be increasing and span 30 local dates");
  }
}
function parseLocalDate(value, fieldName) {
  if (!/^\d{4}-\d{2}-\d{2}$/u.test(value)) {
    throw new RangeError(`${fieldName} must be an exact local date`);
  }
  const milliseconds = Date.parse(`${value}T00:00:00.000Z`);
  if (!Number.isFinite(milliseconds) || new Date(milliseconds).toISOString().slice(0, 10) !== value) {
    throw new RangeError(`${fieldName} must be a valid local date`);
  }
  return milliseconds;
}
function validateCriticalSliceKind(kind) {
  if (kind !== "nearest_three" && kind !== "provider_family" && kind !== "season_daypart" && kind !== "station") {
    throw new RangeError(`unsupported critical slice kind: ${kind}`);
  }
}
function validateCriticalSliceCoverage(slices, scoreableStationKeys, evaluatedSeasonDaypartKeys) {
  validateScoreableStationKeys(scoreableStationKeys);
  validateEvaluatedSeasonDaypartKeys(evaluatedSeasonDaypartKeys);
  const providerFamilies = /* @__PURE__ */ new Set();
  for (const stationKey of scoreableStationKeys) {
    const station = FORECAST_OBSERVATION_STATIONS.find((candidate) => candidate.key === stationKey);
    if (station === void 0) {
      throw new RangeError(`unknown scoreable station: ${stationKey}`);
    }
    providerFamilies.add(station.providerFamily);
  }
  const sortedProviderFamilies = [...providerFamilies].sort();
  if (sortedProviderFamilies.length < 3) {
    throw new RangeError("scoreable stations must span at least three providers");
  }
  const expectedSlices = [
    { key: FORECAST_NEAREST_THREE_SLICE_KEY, kind: "nearest_three" },
    ...sortedProviderFamilies.map((key) => ({ key, kind: "provider_family" })),
    ...evaluatedSeasonDaypartKeys.map((key) => ({
      key,
      kind: "season_daypart"
    })),
    ...scoreableStationKeys.map((key) => ({ key, kind: "station" }))
  ];
  if (slices.length !== expectedSlices.length) {
    throw new RangeError("critical slice coverage cardinality mismatch");
  }
  for (const [index, slice] of slices.entries()) {
    const expected = expectedSlices[index];
    if (expected === void 0 || slice.kind !== expected.kind || slice.key !== expected.key || slice.eventCount < 100) {
      throw new RangeError("critical slice identity or coverage mismatch");
    }
  }
}
function validateScoreableStationKeys(stationKeys) {
  if (stationKeys.length < 5 || stationKeys.length > 11) {
    throw new RangeError("scoreable station set must contain between 5 and 11 stations");
  }
  let previousKey = null;
  for (const stationKey of stationKeys) {
    if (!FORECAST_OBSERVATION_STATION_KEYS.some((key) => key === stationKey) || previousKey !== null && stationKey <= previousKey) {
      throw new RangeError("scoreable station set contains an invalid identity");
    }
    previousKey = stationKey;
  }
}
function validateEvaluatedSeasonDaypartKeys(keys) {
  if (keys.length < 1 || keys.length > FORECAST_SEASON_DAYPART_KEYS.length) {
    throw new RangeError("evaluated season-daypart set has invalid cardinality");
  }
  let previousKey = null;
  for (const key of keys) {
    if (!FORECAST_SEASON_DAYPART_KEYS.some((candidate) => candidate === key) || previousKey !== null && key <= previousKey) {
      throw new RangeError("evaluated season-daypart set contains an invalid identity");
    }
    previousKey = key;
  }
}
function validateDaypart(value) {
  if (value !== "afternoon" && value !== "evening" && value !== "morning" && value !== "night") {
    throw new RangeError(`unsupported coefficient daypart: ${String(value)}`);
  }
}
function validateSeason(value) {
  if (value !== "autumn" && value !== "spring" && value !== "summer" && value !== "winter") {
    throw new RangeError(`unsupported coefficient season: ${String(value)}`);
  }
}
function validateTrainingProvenanceHashes(provenance) {
  rejectUnknownKeys(provenance, TRAINING_PROVENANCE_KEYS, "forecast adjustment training provenance");
  for (const [fieldName, value] of Object.entries(provenance)) {
    validateSha256Hex(value, `trainingProvenance.${fieldName}`);
  }
}
function validateEnabledMetricBands(enabledMetricBands) {
  if (enabledMetricBands.length === 0 || enabledMetricBands.length > 35) {
    throw new RangeError("enabled metric-band set must contain between 1 and 35 pairs");
  }
  let previousKey = null;
  for (const pair of enabledMetricBands) {
    validateMetricBand(pair);
    const key = `${pair.metric}:${pair.leadBand}`;
    if (previousKey !== null && key <= previousKey) {
      throw new RangeError("enabled metric-band set must be unique and lexically sorted");
    }
    previousKey = key;
  }
}
function validateForecastAdjustmentMetric(metric) {
  if (!FORECAST_ADJUSTMENT_METRICS.some((candidate) => candidate === metric)) {
    throw new RangeError(`metric is not adjustable in v1: ${metric}`);
  }
}
function validateBoundedText(value, fieldName) {
  if (value.trim().length === 0 || value.length > 128) {
    throw new RangeError(`${fieldName} must be non-empty and at most 128 chars`);
  }
  return value;
}
function rejectUnknownKeys(value, allowedKeys, fieldName) {
  const keys = Object.keys(value);
  if (keys.length !== allowedKeys.size || keys.some((key) => !allowedKeys.has(key))) {
    throw new RangeError(`${fieldName} does not match its exact schema`);
  }
}

// packages/domain/dist/rain-collection.js
var RAIN_COLLECTION_POLICY = Object.freeze({
  contractVersion: "rain-prospective-capture/v1",
  siteSlug: "ballydidean",
  latitude: 47.950429954185445,
  longitude: -122.42797012608193,
  startsAt: "2026-09-13T00:00:00.000Z",
  expiresAt: "2027-10-08T00:00:00.000Z",
  // enable the fixed catalog after operator-confirmed provider access
  stationAccessAuthorized: true,
  forecastHours: 49,
  decisionDelayHours: 8,
  forecastFirstAttemptHours: 6,
  forecastSecondAttemptHours: 7,
  stationCadenceMinutes: 60,
  stationWindowHours: 2,
  maximumForecastRequestsPerDay: 8,
  maximumStationRequestsPerDay: 288,
  maximumRequestsPerIteration: 2,
  minimumRequestSpacingMs: 1100,
  pendingRequestGuardMs: 12e4,
  rateLimitCooldownHours: 24,
  maximumBodyBytes: 2e6,
  maximumCompressedBodyBytes: 21e5,
  maximumStoredBytesPerDay: 8388608,
  maximumStoredBytes: 2147483648,
  timeoutMs: 45e3,
  modelEnabled: false,
  qualificationEnabled: false
});
var RAIN_COLLECTION_STATIONS = Object.freeze([
  { locationId: 64255, deviceId: 175727, serial: "ST-00054713", latitude: 47.95008, longitude: -122.43982 },
  { locationId: 225947, deviceId: 1239187, serial: "ST-00212830", latitude: 47.94215, longitude: -122.42542 },
  { locationId: 38270, deviceId: 115866, serial: "ST-00157152", latitude: 47.95293, longitude: -122.41414 },
  { locationId: 168853, deviceId: 401592, serial: "ST-00170845", latitude: 47.95498, longitude: -122.44074 },
  { locationId: 126537, deviceId: 313016, serial: "ST-00134621", latitude: 47.9582, longitude: -122.44274 },
  { locationId: 201058, deviceId: 466938, serial: "ST-00194085", latitude: 47.96244, longitude: -122.43369 },
  { locationId: 203055, deviceId: 470937, serial: "ST-00198967", latitude: 47.96505, longitude: -122.4241 },
  { locationId: 66270, deviceId: 180230, serial: "ST-00173167", latitude: 47.93134, longitude: -122.42912 },
  { locationId: 34768, deviceId: 107388, serial: "ST-00020495", latitude: 47.91752, longitude: -122.41112 },
  { locationId: 88159, deviceId: 230560, serial: "ST-00094734", latitude: 47.91563, longitude: -122.41845 },
  { locationId: 126197, deviceId: 312302, serial: "ST-00129187", latitude: 47.91413, longitude: -122.41471 },
  { locationId: 27140, deviceId: 87271, serial: "ST-00000360", latitude: 47.98707, longitude: -122.46295 }
]);

// packages/forecast-adjustment/src/calendar.ts
var FORECAST_ADJUSTMENT_TIMEZONE = "America/Los_Angeles";
var localCalendarFormatter = new Intl.DateTimeFormat("en-US", {
  calendar: "gregory",
  day: "2-digit",
  hour: "2-digit",
  hourCycle: "h23",
  month: "2-digit",
  numberingSystem: "latn",
  timeZone: FORECAST_ADJUSTMENT_TIMEZONE,
  year: "numeric"
});

// packages/forecast-adjustment/src/algorithm-v1.ts
var PROVIDER_FAMILIES = new Set(
  FORECAST_OBSERVATION_PROVIDER_FAMILIES
);
var STATION_EXCLUSION_REASONS = new Set(
  FORECAST_OBSERVATION_EXCLUSION_REASON_CODES
);

// packages/forecast-adjustment/src/candidate.ts
import { createHash } from "node:crypto";
var FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1 = {
  adapterVersion: "open-meteo-forecast-daily/v4",
  cohort: "legacy_v4_retrieval_snapshot",
  contractEpoch: "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83",
  dataset: "forecast",
  referenceKind: "retrieval_snapshot",
  sourceConfigFingerprint: "ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4",
  sourceKey: "open-meteo-forecast-v4",
  upstreamModel: "best_match"
};
var FORECAST_ADJUSTMENT_CANONICAL_TRAINING_PROVENANCE_V1 = {
  aggregationContractSha256: "9c309ef5a00780167570746ad6c31b9128c266db50954fe4645287e1f2b31e64",
  coordinateManifestSha256: "04bfd93a03c393e977c8767a9aca6fe2a4cba9c263cb46e6987fa733b666ba58",
  metricEligibilitySha256: "53731954b347836a26500b05a195ca15cf26214c4d561fe482c5ff87ef56a82e",
  observationSourceLineageSha256: "261a134589a12c1bbbd9a783343950317fd1fbc87e08383e60e805b7761566cc",
  observationStationManifestSha256: "a1f76440c056987bbb434d5315e4916f961deeb2951fe889d785943f559cdd49",
  spatialWeightSha256: "8ed5ce70d33edd4a5166049d9938cbaaf800151b6a0b3345d3005419e9041c74"
};
var CANDIDATE_KEYS2 = /* @__PURE__ */ new Set([
  "algorithmContractVersion",
  "candidateArtifactSha256",
  "coefficientPayloadSha256",
  "coefficients",
  "contractVersion",
  "developmentReportSha256",
  "enabledMetricBands",
  "evaluationEpochId",
  "exportManifestSha256",
  "finalTrainingCutoff",
  "forecastIdentity",
  "metricPolicies",
  "runtimeFingerprint",
  "siteKey",
  "timezone",
  "trainingEnvelopes",
  "trainingProvenance"
]);
function canonicalJsonBytes(value) {
  return `${canonicalizeJson(value)}
`;
}
function canonicalSha256(value) {
  return createHash("sha256").update(canonicalJsonBytes(value)).digest("hex");
}
function canonicalObjectSha256(value, ownHashField) {
  const material = Object.fromEntries(
    Object.entries(value).filter(([key]) => key !== ownHashField)
  );
  return canonicalSha256(material);
}
function verifyForecastAdjustmentCandidate(candidate) {
  rejectUnknownKeys2(candidate, CANDIDATE_KEYS2, "candidate");
  const candidateHash = canonicalObjectSha256(
    candidate,
    "candidateArtifactSha256"
  );
  if (candidateHash !== candidate.candidateArtifactSha256) {
    throw new RangeError("candidate artifact SHA-256 mismatch");
  }
  const coefficientHash = canonicalSha256(
    candidate.coefficients
  );
  if (coefficientHash !== candidate.coefficientPayloadSha256) {
    throw new RangeError("candidate coefficient payload SHA-256 mismatch");
  }
  validateCanonicalCandidateProvenance(
    candidate.forecastIdentity,
    candidate.trainingProvenance
  );
  if (canonicalizeJson(candidate.enabledMetricBands) !== canonicalizeJson(
    sortEnabledMetricBands(candidate.enabledMetricBands)
  )) {
    throw new RangeError("candidate enabled metric-band set is not canonical");
  }
  rejectForbiddenCandidateFields(candidate);
}
function validateCanonicalCandidateProvenance(identity, provenance) {
  if (FORECAST_OBSERVATION_MANIFEST_V1.aggregationContractVersion !== "physical-station-network/v1" || FORECAST_OBSERVATION_MANIFEST_V1.site.key !== "ballydidean" || FORECAST_OBSERVATION_MANIFEST_V1.site.timezone !== "America/Los_Angeles") {
    throw new RangeError("canonical observation manifest identity is invalid");
  }
  if (canonicalizeJson(identity) !== canonicalizeJson(
    FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1
  )) {
    throw new RangeError("candidate forecast identity is not canonical");
  }
  if (canonicalizeJson(provenance) !== canonicalizeJson(
    FORECAST_ADJUSTMENT_CANONICAL_TRAINING_PROVENANCE_V1
  )) {
    throw new RangeError("candidate training provenance is not canonical");
  }
}
function sortEnabledMetricBands(pairs) {
  const sorted = pairs.map((pair) => cloneJson(pair)).sort((left, right) => compareText(metricBandKey2(left), metricBandKey2(right)));
  if (new Set(sorted.map(metricBandKey2)).size !== sorted.length) {
    throw new RangeError("enabled metric-band set contains a duplicate");
  }
  return sorted;
}
function metricBandKey2(pair) {
  return `${pair.metric}:${pair.leadBand}`;
}
function rejectForbiddenCandidateFields(value) {
  const forbidden = /^(?:actual|adjustedLoss|evaluationReport|holdout|qualification|rawLoss|receipt)$/iu;
  visitObject(value, (key) => {
    if (forbidden.test(key)) {
      throw new RangeError(`candidate contains forbidden post-fit field: ${key}`);
    }
  });
}
function visitObject(value, visit) {
  if (value === null || typeof value !== "object") {
    return;
  }
  if (Array.isArray(value)) {
    for (const item of value) {
      visitObject(item, visit);
    }
    return;
  }
  for (const [key, child] of Object.entries(value)) {
    visit(key);
    visitObject(child, visit);
  }
}
function cloneJson(value) {
  return JSON.parse(canonicalizeJson(value));
}
function deepFreeze(value) {
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) {
    return value;
  }
  for (const child of Object.values(value)) {
    deepFreeze(child);
  }
  return Object.freeze(value);
}
function rejectUnknownKeys2(value, allowed, description) {
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) {
      throw new RangeError(`${description} contains an unexpected field: ${key}`);
    }
  }
}
function compareText(left, right) {
  if (left < right) {
    return -1;
  }
  if (left > right) {
    return 1;
  }
  return 0;
}

// packages/forecast-adjustment/src/wind-canary.ts
var FORECAST_ADJUSTMENT_WIND_CANARY_TRAINING_IDENTITY_V1 = {
  adapterVersion: "open-meteo-previous-runs/v1",
  cohort: "fixed_lead_anchor",
  contractEpoch: "open-meteo-previous-runs-best-match/2026-09",
  dataset: "previous_runs",
  referenceKind: "fixed_lead_anchor",
  sourceConfigFingerprint: "3a311d67d08aa3f9dedc2dbb8382d4cf11f945439d50c328a93874fc0a44538e",
  sourceKey: "open-meteo-previous-runs-v1",
  upstreamModel: "best_match"
};
var FORECAST_ADJUSTMENT_WIND_CANARY_MAXIMUM_DURATION_MS = 14 * 24 * 60 * 60 * 1e3;
var FORECAST_ADJUSTMENT_WIND_CANARY_AUTHORIZATION_CONTRACT_VERSION_V2 = "forecast-adjustment-wind-canary-authorization/v2";
var FORECAST_ADJUSTMENT_WIND_CANARY_RUNTIME_BUNDLE_CONTRACT_VERSION_V2 = "forecast-adjustment-wind-canary-runtime-bundle/v2";
var HASH_PATTERN = /^[a-f0-9]{64}$/u;
var CANDIDATE_KEYS3 = [
  "algorithmContractVersion",
  "artifactKind",
  "candidateArtifactSha256",
  "coefficientPayloadSha256",
  "coefficients",
  "contractVersion",
  "enabledMetricBands",
  "exportManifestSha256",
  "finalTrainingCutoff",
  "runtimeFingerprint",
  "servedForecastIdentity",
  "siteKey",
  "timezone",
  "trainingEnvelopes",
  "trainingForecastIdentity",
  "trainingProvenance"
];
var TRANSFER_REPORT_KEYS = [
  "artifactKind",
  "bridgeEndExclusive",
  "bridgeEvaluations",
  "bridgeStartInclusive",
  "candidateArtifactSha256",
  "contractVersion",
  "enabledMetricBands",
  "passed",
  "servedForecastIdentity",
  "trainingForecastIdentity",
  "transferReportSha256"
];
var AUTHORIZATION_KEYS = [
  "activatedAt",
  "artifactKind",
  "authorizationReason",
  "authorizationSha256",
  "authorized",
  "authorizedAt",
  "authorizedBy",
  "candidateArtifactSha256",
  "contractVersion",
  "enabledMetricBands",
  "expiresAt",
  "transferReportSha256"
];
var PERMANENT_AUTHORIZATION_KEYS = [
  "activatedAt",
  "artifactKind",
  "authorizationReason",
  "authorizationSha256",
  "authorized",
  "authorizedAt",
  "authorizedBy",
  "candidateArtifactSha256",
  "contractVersion",
  "enabledMetricBands",
  "expiresAt",
  "permanent",
  "transferReportSha256"
];
var BUNDLE_KEYS = [
  "artifactKind",
  "authorization",
  "bundleSha256",
  "candidate",
  "contractVersion",
  "siteKey",
  "timezone",
  "transferReport"
];
var METRIC_BAND_KEYS2 = ["leadBand", "metric"];
var COEFFICIENT_KEYS2 = [
  "coefficient",
  "daypart",
  "effectiveEventCount",
  "leadBand",
  "level",
  "metric",
  "month",
  "season"
];
var TRAINING_ENVELOPE_KEYS2 = [
  "leadBand",
  "maximum",
  "metric",
  "minimum"
];
var RUNTIME_FINGERPRINT_KEYS2 = ["icuVersion", "tzdataVersion"];
var BRIDGE_EVALUATION_KEYS = ["metricBand", "network"];
var BRIDGE_SCORE_KEYS = [
  "adjustedLoss",
  "eventCount",
  "rawLoss",
  "skill"
];
function verifyForecastAdjustmentWindCanaryCandidate(candidate) {
  requireExactKeys(candidate, CANDIDATE_KEYS3, "wind canary candidate");
  if (candidate.artifactKind !== "wind_transfer_canary_candidate" || candidate.contractVersion !== FORECAST_ADJUSTMENT_CONTRACT_VERSIONS.windCanaryCandidate || candidate.algorithmContractVersion !== "robust-hierarchical-median/v1" || candidate.siteKey !== "ballydidean" || candidate.timezone !== "America/Los_Angeles") {
    throw new RangeError("wind canary candidate identity mismatch");
  }
  validateHash(candidate.candidateArtifactSha256, "candidateArtifactSha256");
  validateHash(candidate.coefficientPayloadSha256, "coefficientPayloadSha256");
  validateHash(candidate.exportManifestSha256, "exportManifestSha256");
  validateUtcInstant2(candidate.finalTrainingCutoff, "finalTrainingCutoff");
  validateText(candidate.runtimeFingerprint.icuVersion, "icuVersion");
  validateText(candidate.runtimeFingerprint.tzdataVersion, "tzdataVersion");
  requireExactKeys(
    candidate.runtimeFingerprint,
    RUNTIME_FINGERPRINT_KEYS2,
    "wind canary runtime fingerprint"
  );
  if (canonicalObjectSha256(
    candidate,
    "candidateArtifactSha256"
  ) !== candidate.candidateArtifactSha256 || canonicalSha256(candidate.coefficients) !== candidate.coefficientPayloadSha256) {
    throw new RangeError("wind canary candidate SHA-256 mismatch");
  }
  validateCanonicalLineage(
    candidate.trainingForecastIdentity,
    candidate.servedForecastIdentity,
    candidate.trainingProvenance
  );
  validateWindCandidateMaterial(candidate);
}
function verifyForecastAdjustmentWindCanaryTransferReport(report, candidate) {
  verifyForecastAdjustmentWindCanaryCandidate(candidate);
  requireExactKeys(report, TRANSFER_REPORT_KEYS, "wind canary transfer report");
  if (report.artifactKind !== "wind_transfer_canary_transfer_report" || report.contractVersion !== FORECAST_ADJUSTMENT_CONTRACT_VERSIONS.windCanaryTransferReport || report.passed !== true || report.candidateArtifactSha256 !== candidate.candidateArtifactSha256) {
    throw new RangeError("wind canary transfer report identity mismatch");
  }
  validateHash(report.transferReportSha256, "transferReportSha256");
  const bridgeStart = Date.parse(
    validateUtcInstant2(report.bridgeStartInclusive, "bridgeStartInclusive")
  );
  const bridgeEnd = Date.parse(
    validateUtcInstant2(report.bridgeEndExclusive, "bridgeEndExclusive")
  );
  if (bridgeStart >= bridgeEnd || Date.parse(candidate.finalTrainingCutoff) >= bridgeStart) {
    throw new RangeError("wind canary bridge window is invalid");
  }
  if (canonicalizeJson(report.enabledMetricBands) !== canonicalizeJson(candidate.enabledMetricBands) || canonicalizeJson(report.trainingForecastIdentity) !== canonicalizeJson(candidate.trainingForecastIdentity) || canonicalizeJson(report.servedForecastIdentity) !== canonicalizeJson(candidate.servedForecastIdentity) || canonicalizeJson(
    report.bridgeEvaluations.map((evaluation) => evaluation.metricBand)
  ) !== canonicalizeJson(candidate.enabledMetricBands)) {
    throw new RangeError("wind canary transfer report cross-link mismatch");
  }
  for (const evaluation of report.bridgeEvaluations) {
    requireExactKeys(
      evaluation,
      BRIDGE_EVALUATION_KEYS,
      "wind canary bridge evaluation"
    );
    requireExactKeys(
      evaluation.metricBand,
      METRIC_BAND_KEYS2,
      "wind canary bridge metric band"
    );
    requireExactKeys(
      evaluation.network,
      BRIDGE_SCORE_KEYS,
      "wind canary bridge score"
    );
    validateBridgeScore(evaluation.network);
  }
  if (canonicalObjectSha256(
    report,
    "transferReportSha256"
  ) !== report.transferReportSha256) {
    throw new RangeError("wind canary transfer report SHA-256 mismatch");
  }
}
function verifyForecastAdjustmentWindCanaryAuthorization(authorization, candidate, report) {
  verifyForecastAdjustmentWindCanaryCandidate(candidate);
  verifyForecastAdjustmentWindCanaryTransferReport(report, candidate);
  requireExactKeys(authorization, AUTHORIZATION_KEYS, "wind canary authorization");
  if (authorization.artifactKind !== "wind_transfer_canary_authorization" || authorization.contractVersion !== FORECAST_ADJUSTMENT_CONTRACT_VERSIONS.windCanaryAuthorization || authorization.authorized !== true || authorization.candidateArtifactSha256 !== candidate.candidateArtifactSha256 || authorization.transferReportSha256 !== report.transferReportSha256 || canonicalizeJson(authorization.enabledMetricBands) !== canonicalizeJson(candidate.enabledMetricBands)) {
    throw new RangeError("wind canary authorization cross-link mismatch");
  }
  validateHash(authorization.authorizationSha256, "authorizationSha256");
  validateText(authorization.authorizationReason, "authorizationReason");
  validateText(authorization.authorizedBy, "authorizedBy");
  const authorizedAt = Date.parse(
    validateUtcInstant2(authorization.authorizedAt, "authorizedAt")
  );
  const activatedAt = Date.parse(
    validateUtcInstant2(authorization.activatedAt, "activatedAt")
  );
  const expiresAt = Date.parse(
    validateUtcInstant2(authorization.expiresAt, "expiresAt")
  );
  if (authorizedAt < Date.parse(report.bridgeEndExclusive) || activatedAt < authorizedAt || expiresAt <= activatedAt || expiresAt - activatedAt > FORECAST_ADJUSTMENT_WIND_CANARY_MAXIMUM_DURATION_MS) {
    throw new RangeError("wind canary authorization window is invalid");
  }
  if (canonicalObjectSha256(
    authorization,
    "authorizationSha256"
  ) !== authorization.authorizationSha256) {
    throw new RangeError("wind canary authorization SHA-256 mismatch");
  }
}
function verifyPermanentForecastAdjustmentWindCanaryAuthorization(authorization, candidate, report) {
  verifyForecastAdjustmentWindCanaryCandidate(candidate);
  verifyForecastAdjustmentWindCanaryTransferReport(report, candidate);
  requireExactKeys(
    authorization,
    PERMANENT_AUTHORIZATION_KEYS,
    "permanent wind canary authorization"
  );
  if (authorization.artifactKind !== "wind_transfer_canary_authorization" || authorization.contractVersion !== FORECAST_ADJUSTMENT_WIND_CANARY_AUTHORIZATION_CONTRACT_VERSION_V2 || authorization.authorized !== true || authorization.permanent !== true || authorization.expiresAt !== null || authorization.candidateArtifactSha256 !== candidate.candidateArtifactSha256 || authorization.transferReportSha256 !== report.transferReportSha256 || canonicalizeJson(authorization.enabledMetricBands) !== canonicalizeJson(candidate.enabledMetricBands)) {
    throw new RangeError("permanent wind canary authorization cross-link mismatch");
  }
  validateHash(authorization.authorizationSha256, "authorizationSha256");
  validateText(authorization.authorizationReason, "authorizationReason");
  validateText(authorization.authorizedBy, "authorizedBy");
  const authorizedAt = Date.parse(
    validateUtcInstant2(authorization.authorizedAt, "authorizedAt")
  );
  const activatedAt = Date.parse(
    validateUtcInstant2(authorization.activatedAt, "activatedAt")
  );
  if (authorizedAt < Date.parse(report.bridgeEndExclusive) || activatedAt < authorizedAt) {
    throw new RangeError("permanent wind canary authorization timing is invalid");
  }
  if (canonicalObjectSha256(
    authorization,
    "authorizationSha256"
  ) !== authorization.authorizationSha256) {
    throw new RangeError("wind canary authorization SHA-256 mismatch");
  }
}
function verifyForecastAdjustmentWindCanaryRuntimeBundle(bundle) {
  requireExactKeys(bundle, BUNDLE_KEYS, "wind canary runtime bundle");
  if (bundle.artifactKind !== "wind_transfer_canary_runtime_bundle" || bundle.siteKey !== "ballydidean" || bundle.timezone !== "America/Los_Angeles") {
    throw new RangeError("wind canary runtime bundle identity mismatch");
  }
  validateHash(bundle.bundleSha256, "bundleSha256");
  verifyForecastAdjustmentWindCanaryCandidate(bundle.candidate);
  verifyForecastAdjustmentWindCanaryTransferReport(
    bundle.transferReport,
    bundle.candidate
  );
  if (bundle.contractVersion === FORECAST_ADJUSTMENT_CONTRACT_VERSIONS.windCanaryRuntimeBundle) {
    verifyForecastAdjustmentWindCanaryAuthorization(
      bundle.authorization,
      bundle.candidate,
      bundle.transferReport
    );
  } else if (bundle.contractVersion === FORECAST_ADJUSTMENT_WIND_CANARY_RUNTIME_BUNDLE_CONTRACT_VERSION_V2) {
    verifyPermanentForecastAdjustmentWindCanaryAuthorization(
      bundle.authorization,
      bundle.candidate,
      bundle.transferReport
    );
  } else {
    throw new RangeError("wind canary runtime bundle identity mismatch");
  }
  if (canonicalObjectSha256(
    bundle,
    "bundleSha256"
  ) !== bundle.bundleSha256) {
    throw new RangeError("wind canary runtime bundle SHA-256 mismatch");
  }
}
function validateCanonicalLineage(trainingIdentity, servedIdentity, trainingProvenance) {
  if (canonicalizeJson(trainingIdentity) !== canonicalizeJson(
    FORECAST_ADJUSTMENT_WIND_CANARY_TRAINING_IDENTITY_V1
  ) || canonicalizeJson(servedIdentity) !== canonicalizeJson(
    FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1
  ) || canonicalizeJson(trainingProvenance) !== canonicalizeJson(
    FORECAST_ADJUSTMENT_CANONICAL_TRAINING_PROVENANCE_V1
  )) {
    throw new RangeError("wind canary lineage is not canonical");
  }
}
function validateWindCandidateMaterial(candidate) {
  const sortedBands = sortEnabledMetricBands(candidate.enabledMetricBands);
  const enabledKeys = new Set(sortedBands.map(metricBandKey2));
  for (const pair of candidate.enabledMetricBands) {
    requireExactKeys(pair, METRIC_BAND_KEYS2, "wind canary metric band");
  }
  if (sortedBands.length === 0 || enabledKeys.size !== sortedBands.length || canonicalizeJson(sortedBands) !== canonicalizeJson(candidate.enabledMetricBands) || sortedBands.some(
    (pair) => !isWindCanaryMetric(pair.metric) || !FORECAST_LEAD_BANDS.some((band) => band.key === pair.leadBand)
  )) {
    throw new RangeError("wind canary enabled set is invalid");
  }
  const rootCounts = new Map([...enabledKeys].map((key) => [key, 0]));
  const coefficientCells = /* @__PURE__ */ new Set();
  for (const coefficient of candidate.coefficients) {
    requireExactKeys(coefficient, COEFFICIENT_KEYS2, "wind canary coefficient");
    const key = metricBandKey2(coefficient);
    const policy = FORECAST_ADJUSTMENT_METRIC_POLICIES_V1.find(
      (item) => item.metric === coefficient.metric
    );
    if (!isWindCanaryMetric(coefficient.metric) || !enabledKeys.has(key) || policy === void 0 || !Number.isFinite(coefficient.coefficient) || !Number.isFinite(coefficient.effectiveEventCount) || coefficient.effectiveEventCount < 0 || coefficient.coefficient < policy.correctionMinimum || coefficient.coefficient > policy.correctionMaximum) {
      throw new RangeError("wind canary coefficient is invalid");
    }
    validateCoefficientShape(coefficient);
    const cell = canonicalizeJson({
      daypart: coefficient.daypart,
      leadBand: coefficient.leadBand,
      level: coefficient.level,
      metric: coefficient.metric,
      month: coefficient.month,
      season: coefficient.season
    });
    if (coefficientCells.has(cell)) {
      throw new RangeError("wind canary contains a duplicate coefficient cell");
    }
    coefficientCells.add(cell);
    if (coefficient.level === 1) {
      rootCounts.set(key, (rootCounts.get(key) ?? 0) + 1);
    }
  }
  if ([...rootCounts.values()].some((count) => count !== 1)) {
    throw new RangeError("wind canary enabled pair lacks one root coefficient");
  }
  const expectedEnvelopes = sortedBands.filter((pair) => pair.metric !== "windDirectionDegrees").map(metricBandKey2);
  const actualEnvelopes = candidate.trainingEnvelopes.map(metricBandKey2);
  for (const envelope of candidate.trainingEnvelopes) {
    requireExactKeys(
      envelope,
      TRAINING_ENVELOPE_KEYS2,
      "wind canary training envelope"
    );
  }
  if (canonicalizeJson(actualEnvelopes) !== canonicalizeJson(expectedEnvelopes) || canonicalizeJson(candidate.coefficients) !== canonicalizeJson(sortCoefficients(candidate.coefficients)) || canonicalizeJson(candidate.trainingEnvelopes) !== canonicalizeJson(
    sortTrainingEnvelopes(candidate.trainingEnvelopes)
  ) || candidate.trainingEnvelopes.some(
    (envelope) => !Number.isFinite(envelope.minimum) || !Number.isFinite(envelope.maximum) || envelope.minimum > envelope.maximum
  )) {
    throw new RangeError("wind canary training envelopes are invalid");
  }
}
function isWindCanaryMetric(metric) {
  return metric === "windDirectionDegrees" || metric === "windGustMps" || metric === "windSpeedMps";
}
function validateCoefficientShape(coefficient) {
  if (coefficient.level === 1) {
    if (coefficient.daypart !== null || coefficient.month !== null || coefficient.season !== null || coefficient.effectiveEventCount < 200) {
      throw new RangeError("wind canary root coefficient is invalid");
    }
    return;
  }
  if (coefficient.level !== 2 && coefficient.level !== 3 || coefficient.daypart === null || !["afternoon", "evening", "morning", "night"].includes(
    coefficient.daypart
  ) || coefficient.level === 2 && (coefficient.month !== null || coefficient.season === null || !["autumn", "spring", "summer", "winter"].includes(
    coefficient.season
  ) || coefficient.effectiveEventCount < 100) || coefficient.level === 3 && (coefficient.season !== null || coefficient.month === null || !Number.isInteger(coefficient.month) || coefficient.month < 1 || coefficient.month > 12 || coefficient.effectiveEventCount < 50)) {
    throw new RangeError("wind canary refined coefficient is invalid");
  }
}
function validateBridgeScore(score) {
  const values = [
    score.adjustedLoss,
    score.eventCount,
    score.rawLoss,
    score.skill
  ];
  const derivedSkill = score.rawLoss === 0 ? score.adjustedLoss === 0 ? 0 : Number.NEGATIVE_INFINITY : (score.rawLoss - score.adjustedLoss) / score.rawLoss;
  if (values.some((value) => !Number.isFinite(value)) || !Number.isSafeInteger(score.eventCount) || score.eventCount < 30 || score.adjustedLoss < 0 || score.rawLoss <= 0 || score.skill <= 0 || Math.abs(derivedSkill - score.skill) > Number.EPSILON * 16) {
    throw new RangeError("wind canary bridge score is not positive finite evidence");
  }
}
function sortCoefficients(coefficients) {
  return [...coefficients].map(cloneJson2).sort(
    (left, right) => metricBandKey2(left).localeCompare(metricBandKey2(right)) || left.level - right.level || (left.season ?? "").localeCompare(right.season ?? "") || (left.month ?? 0) - (right.month ?? 0) || (left.daypart ?? "").localeCompare(right.daypart ?? "")
  );
}
function sortTrainingEnvelopes(envelopes) {
  return [...envelopes].map(cloneJson2).sort(
    (left, right) => metricBandKey2(left).localeCompare(metricBandKey2(right))
  );
}
function requireExactKeys(value, expected, description) {
  const actual = Object.keys(value).sort();
  if (canonicalizeJson(actual) !== canonicalizeJson([...expected].sort())) {
    throw new RangeError(`${description} has unexpected fields`);
  }
}
function validateHash(value, description) {
  if (!HASH_PATTERN.test(value)) {
    throw new RangeError(`${description} must be a SHA-256 hex value`);
  }
  return value;
}
function validateText(value, description) {
  if (value.trim() !== value || value.length < 1 || value.length > 256 || /[\r\n]|:\/\/|\b(?:credential|password|private[-_ ]?key|secret|token)\b/iu.test(
    value
  )) {
    throw new RangeError(`${description} must be bounded nonempty text`);
  }
  return value;
}
function validateUtcInstant2(value, description) {
  const milliseconds = Date.parse(value);
  if (!Number.isFinite(milliseconds) || new Date(milliseconds).toISOString() !== value) {
    throw new RangeError(`${description} must be a canonical UTC instant`);
  }
  return value;
}
function cloneJson2(value) {
  return JSON.parse(canonicalizeJson(value));
}

// packages/forecast-adjustment/src/evaluate.ts
function verifyForecastAdjustmentEvaluationReport(report) {
  requireExactKeys2(report, [
    "candidateArtifactSha256",
    "contractVersion",
    "enabledMetricBands",
    "evaluationEpochId",
    "evaluationReportSha256",
    "holdoutAccessMarkerSha256",
    "holdoutEndExclusive",
    "holdoutEndLocalDate",
    "holdoutStartInclusive",
    "holdoutStartLocalDate",
    "metricBandEvaluations",
    "preregistrationSha256",
    "trainingProvenance"
  ], "evaluation report");
  if (canonicalObjectSha256(
    report,
    "evaluationReportSha256"
  ) !== report.evaluationReportSha256) {
    throw new RangeError("evaluation report SHA-256 mismatch");
  }
}
function verifyForecastAdjustmentQualificationReceipt(receipt) {
  requireExactKeys2(receipt, [
    "candidateArtifactSha256",
    "contractVersion",
    "enabledMetricBands",
    "evaluationEpochId",
    "evaluationReportSha256",
    "evidenceRedundancy",
    "gates",
    "holdoutAccessMarkerSha256",
    "lifecycleState",
    "passed",
    "preregistrationSha256",
    "qualificationReceiptSha256",
    "trainingProvenance"
  ], "qualification receipt");
  if (canonicalObjectSha256(
    receipt,
    "qualificationReceiptSha256"
  ) !== receipt.qualificationReceiptSha256) {
    throw new RangeError("qualification receipt SHA-256 mismatch");
  }
}
function requireExactKeys2(value, expected, description) {
  const actual = Object.keys(value).sort();
  const required = [...expected].sort();
  if (canonicalizeJson(actual) !== canonicalizeJson(required)) {
    throw new RangeError(`${description} has unexpected fields`);
  }
}

// packages/forecast-adjustment/src/runtime-bundle.ts
var FORBIDDEN_NORMALIZED_KEYS = /* @__PURE__ */ new Set([
  "apikey",
  "credential",
  "credentials",
  "decryptionkey",
  "deviceid",
  "devicemac",
  "encryptedmember",
  "encryptedmembers",
  "encryptionkey",
  "eventlosses",
  "evidencepath",
  "lanaddress",
  "macaddress",
  "memberpath",
  "outputpath",
  "password",
  "pereventlosses",
  "privatekey",
  "rawrow",
  "rawrows",
  "secret",
  "secrets",
  "snapshotpath"
]);
var FORBIDDEN_VALUE_PATTERNS = [
  /-----BEGIN [A-Z ]*PRIVATE KEY-----/iu,
  /\b(?:AKIA|ASIA)[A-Z0-9]{16}\b/u,
  /\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|sk-[A-Za-z0-9_-]{16,})\b/u,
  /\bBearer\s+[A-Za-z0-9._~+/-]+=*\b/iu,
  /\b[a-z][a-z0-9+.-]*:\/\/[^/\s:@]+:[^/\s@]+@/iu,
  /postgres(?:ql)?:\/\//iu,
  /(?:^|[/\\])\.weather-(?:data|models)(?:[/\\]|$)/iu,
  /(?:^|[/\\])\.weather[/\\]model-evidence(?:[/\\]|$)/iu,
  /(?:^|[/\\])(?:evidence|members?|raw(?:[-_ ]?(?:data|rows?))?|encrypted(?:[-_ ]?(?:data|members?))?)(?:[/\\]|$)/iu,
  /\.(?:age|enc|gpg|jsonl(?:\.gz)?|p12|pem|pfx)(?:$|[?#])/iu,
  /(?:^|[^a-z0-9])(?:api[-_ ]?key|access[-_ ]?key|auth[-_ ]?token|credential|password|passwd|private[-_ ]?key|pwd|secret)(?:[^a-z0-9]|$)/iu,
  /(?:^|[^a-z0-9])(?:device[-_ ]?(?:id|mac|serial)|lan[-_ ]?(?:address|host|ip)|mac[-_ ]?address|private[-_ ]?(?:address|host|ip))(?:[^a-z0-9]|$)/iu,
  /(?:^|[^0-9a-f])(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}(?:[^0-9a-f]|$)/iu,
  /(?:^|[^\d])(?:10(?:\.\d{1,3}){3}|127(?:\.\d{1,3}){3}|169\.254(?:\.\d{1,3}){2}|192\.168(?:\.\d{1,3}){2}|172\.(?:1[6-9]|2\d|3[01])(?:\.\d{1,3}){2})(?:[^\d]|$)/u,
  /(?:^|[^a-z0-9])localhost(?:[^a-z0-9]|$)/iu,
  /(?:^|[^a-f0-9])(?:::1|f[cd][a-f0-9]{2}:|fe[89ab][a-f0-9]:)/iu
];
function verifyForecastAdjustmentRuntimeBundle(bundle) {
  rejectForbiddenRuntimeContent(bundle);
  const keys = Object.keys(bundle).sort();
  const expected = [
    "bundleSha256",
    "candidate",
    "contractVersion",
    "evaluationReport",
    "qualificationReceipt",
    "siteKey",
    "timezone"
  ].sort();
  if (canonicalizeJson(keys) !== canonicalizeJson(expected)) {
    throw new RangeError("runtime bundle has unexpected fields");
  }
  if (bundle.contractVersion !== FORECAST_ADJUSTMENT_CONTRACT_VERSIONS.runtimeBundle) {
    throw new RangeError("unsupported forecast adjustment runtime bundle");
  }
  if (canonicalObjectSha256(
    bundle,
    "bundleSha256"
  ) !== bundle.bundleSha256) {
    throw new RangeError("runtime bundle SHA-256 mismatch");
  }
  verifyEmbeddedEvidence(bundle);
  validatePromotableForecastAdjustmentEvidence(bundle);
}
function verifyEmbeddedEvidence(evidence) {
  verifyForecastAdjustmentCandidate(evidence.candidate);
  verifyForecastAdjustmentEvaluationReport(evidence.evaluationReport);
  verifyForecastAdjustmentQualificationReceipt(evidence.qualificationReceipt);
  if (evidence.evaluationReport.candidateArtifactSha256 !== evidence.candidate.candidateArtifactSha256 || evidence.qualificationReceipt.candidateArtifactSha256 !== evidence.candidate.candidateArtifactSha256 || evidence.qualificationReceipt.evaluationReportSha256 !== evidence.evaluationReport.evaluationReportSha256 || evidence.qualificationReceipt.preregistrationSha256 !== evidence.evaluationReport.preregistrationSha256 || evidence.qualificationReceipt.holdoutAccessMarkerSha256 !== evidence.evaluationReport.holdoutAccessMarkerSha256 || canonicalizeJson(
    evidence.evaluationReport.enabledMetricBands
  ) !== canonicalizeJson(evidence.candidate.enabledMetricBands) || canonicalizeJson(
    evidence.qualificationReceipt.enabledMetricBands
  ) !== canonicalizeJson(evidence.candidate.enabledMetricBands)) {
    throw new RangeError("runtime bundle embedded evidence cross-link mismatch");
  }
}
function rejectForbiddenRuntimeContent(value) {
  if (value === null || typeof value !== "object") {
    if (typeof value === "string" && containsForbiddenRuntimeValue(value)) {
      throw new RangeError("runtime bundle contains forbidden sensitive content");
    }
    return;
  }
  if (Array.isArray(value)) {
    for (const child of value) {
      rejectForbiddenRuntimeContent(child);
    }
    return;
  }
  for (const [key, child] of Object.entries(value)) {
    if (FORBIDDEN_NORMALIZED_KEYS.has(normalizeRuntimeKey(key))) {
      throw new RangeError(`runtime bundle contains forbidden field: ${key}`);
    }
    rejectForbiddenRuntimeContent(child);
  }
}
function containsForbiddenRuntimeValue(value) {
  for (const pattern of FORBIDDEN_VALUE_PATTERNS) {
    if (pattern.test(value)) {
      return true;
    }
  }
  return false;
}
function normalizeRuntimeKey(value) {
  return value.replace(/[^a-z0-9]/giu, "").toLowerCase();
}

// packages/forecast-adjustment/src/maintenance-revision-projection.ts
import { createHash as createHash3 } from "node:crypto";
import { TextDecoder as TextDecoder2 } from "node:util";

// packages/forecast-adjustment/src/maintenance-shadow-values.ts
import { createHash as createHash2 } from "node:crypto";
import { TextDecoder } from "node:util";
var MAINTENANCE_SHADOW_VALUES_VERSION = "adjustment-shadow-prediction-values/v3";
var MAINTENANCE_SHADOW_SOURCE_VERSION = "adjustment-shadow-source-projection/v1";
var MAINTENANCE_SHADOW_LIMITS = Object.freeze({
  temperature: { rows: 12, bytes: 8192 },
  wind: { rows: 168, bytes: 65536 },
  rain: { rows: 23, bytes: 12288 }
});
var MAINTENANCE_SHADOW_SOURCE_LIMITS = Object.freeze({
  temperature: { bytes: 64 * 1024 },
  wind: { bytes: 768 * 1024 },
  rain: { bytes: 128 * 1024 }
});
var COMMON_KEYS = [
  "contractVersion",
  "family",
  "registrationSha256",
  "candidateSha256",
  "sourceSha256",
  "dueKey",
  "issuedAt",
  "sourceReceiptSha256",
  "inputSha256",
  "rowCount",
  "rows"
];
var SOURCE_KEYS = [
  "contractVersion",
  "family",
  "registrationSha256",
  "candidateSha256",
  "sourceSha256",
  "dueKey",
  "issuedAt",
  "rowCount",
  "rows"
];
var TEMPERATURE_SOURCE_KEYS = [
  "contractVersion",
  "family",
  "registrationSha256",
  "candidateSha256",
  "sourceSha256",
  "dueKey",
  "issuedAt",
  "recentErrorState",
  "rowCount",
  "rows"
];
var RAIN_SOURCE_KEYS = [
  "contractVersion",
  "family",
  "registrationSha256",
  "candidateSha256",
  "sourceSha256",
  "dueKey",
  "issuedAt",
  "causalInputs",
  "rowCount",
  "rows"
];
var RAIN_CAUSAL_KEYS = ["contractVersion", "captureSet", "currentRun", "priorRuns", "stationHours"];
var RAIN_CAPTURE_KEYS = [
  "claimId",
  "kind",
  "stationId",
  "runInitializedAt",
  "windowStart",
  "windowEndExclusive",
  "completedAt",
  "bodySha256"
];
var RAIN_RUN_KEYS = ["runInitializedAt", "completedAt", "contentSha256", "hours"];
var RAIN_RUN_HOUR_KEYS = [
  "leadHours",
  "precipitationMm64",
  "temperatureC64",
  "relativeHumidityPercent64",
  "cloudCoverPercent64",
  "pressureHpa64",
  "windSpeedMps64",
  "windDirectionDegrees64"
];
var RAIN_STATION_HOUR_KEYS = ["hourAt", "receivedAt", "stationId", "precipitationMm64", "temperatureC64"];
var TEMPERATURE_RECENT_STATE_KEYS = [
  "b24C",
  "b72C",
  "cohort",
  "localDates",
  "mad72C",
  "maximumSourceRunInitializedAt",
  "maximumSourceValidAt",
  "n24",
  "n72",
  "sourceKeys",
  "supported",
  "targetRunInitializedAt",
  "windowEndValidAt"
];
var SOURCE_ROW_KEYS = Object.freeze({
  temperature: [
    "validAt",
    "leadHours",
    "modelLeadHours",
    "referenceAt",
    "receivedAt",
    "sourceSha256",
    "contentSha256",
    "adapterVersion",
    "dataset",
    "providerKey",
    "providerResponseSha256",
    "modelCycle",
    "upstreamModel",
    "rawTemperatureC64",
    "rawRelativeHumidityPercent64",
    "rawWindSpeedMps64",
    "bestMatchContentSha256",
    "bestMatchProductRunAt",
    "bestMatchSourceId",
    "bestMatchTemperatureC64"
  ],
  wind: [
    "validAt",
    "leadHours",
    "modelLeadHours",
    "referenceAt",
    "receivedAt",
    "sourceSha256",
    "contentSha256",
    "adapterVersion",
    "contractEpoch",
    "dataset",
    "providerKey",
    "sourceKey",
    "sourceConfigFingerprint",
    "upstreamModel",
    "revisionCount",
    "windSpeedMps64",
    "windGustMps64"
  ],
  rain: [
    "validAt",
    "leadHours",
    "modelLeadHours",
    "referenceAt",
    "receivedAt",
    "sourceSha256",
    "contentSha256",
    "adapterVersion",
    "contractEpoch",
    "dataset",
    "providerKey",
    "sourceKey",
    "sourceConfigFingerprint",
    "upstreamModel",
    "revisionCount",
    "precipitationMm64"
  ]
});
var ROW_KEYS = Object.freeze({
  temperature: ["validAt", "leadHours", "sourceRowSha256", "candidateTemperatureC64", "wouldApply", "fallbackCode"],
  wind: ["validAt", "leadHours", "sourceRowSha256", "candidateSpeedMps64", "candidateGustMps64", "speedWouldApply", "gustWouldApply"],
  rain: [
    "validAt",
    "leadHours",
    "sourceRowSha256",
    "occurrenceProbability64",
    "positiveAmountMm64",
    "candidatePrecipitationMm64",
    "atLeast1_0Probability64",
    "atLeast2_5Probability64",
    "wouldApply",
    "fallbackCode"
  ]
});
var HASH = /^[a-f0-9]{64}$/u;
var BINARY64 = /^[a-f0-9]{16}$/u;
var FALLBACK_CODES = ["none", "missing_source", "ineligible", "physical_cap", "model_unavailable"];
var MAINTENANCE_SHADOW_SCHEMA_SHA256 = Object.freeze(Object.fromEntries(
  Object.keys(MAINTENANCE_SHADOW_LIMITS).map(
    // use only fixed public schema inputs
    (family) => [family, sha256(Buffer.from(JSON.stringify({
      contractVersion: MAINTENANCE_SHADOW_VALUES_VERSION,
      family,
      commonKeys: COMMON_KEYS,
      rowKeys: ROW_KEYS[family],
      limits: MAINTENANCE_SHADOW_LIMITS[family]
    }) + "\n"))]
  )
));
function decodeMaintenanceBinary64(value) {
  if (typeof value !== "string" || !BINARY64.test(value)) {
    throw new RangeError("invalid shadow binary64");
  }
  const decoded = Buffer.from(value, "hex").readDoubleBE();
  if (!Number.isFinite(decoded)) {
    throw new RangeError("shadow value must be finite");
  }
  return decoded;
}
function encodeMaintenanceShadowSourceProjection(value) {
  validateMaintenanceShadowSourceProjection(value);
  const ordered = {};
  const sourceKeys = value.family === "rain" ? RAIN_SOURCE_KEYS : value.family === "temperature" ? TEMPERATURE_SOURCE_KEYS : SOURCE_KEYS;
  for (const key of sourceKeys) {
    ordered[key] = key === "rows" ? value.rows.map(
      // preserve the exact source row field order
      (row) => Object.fromEntries(SOURCE_ROW_KEYS[value.family].map((field) => [field, row[field]]))
    ) : key === "causalInputs" ? canonicalRainCausalInputs(value.causalInputs) : key === "recentErrorState" ? canonicalTemperatureRecentErrorState(value.recentErrorState) : value[key];
  }
  const bytes = Buffer.from(JSON.stringify(ordered) + "\n");
  if (bytes.length > MAINTENANCE_SHADOW_SOURCE_LIMITS[value.family].bytes) {
    throw new RangeError("shadow source projection exceeds its cap");
  }
  return bytes;
}
function parseMaintenanceShadowSourceProjection(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength > MAINTENANCE_SHADOW_SOURCE_LIMITS.wind.bytes) {
    throw new RangeError("invalid shadow source projection size");
  }
  const value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  const canonical = encodeMaintenanceShadowSourceProjection(value);
  if (!canonical.equals(Buffer.from(bytes))) {
    throw new RangeError("shadow source projection is not canonical");
  }
  return value;
}
function createMaintenanceShadowSourceIdentity(bytes) {
  const source = parseMaintenanceShadowSourceProjection(bytes);
  const inputSha256 = sha256(Buffer.from(bytes));
  const sourceRowSha256 = source.rows.map((row) => {
    const canonicalRow = JSON.stringify(Object.fromEntries(
      SOURCE_ROW_KEYS[source.family].map((field) => [field, row[field]])
    ));
    return sha256(Buffer.from(`adjustment-shadow-source-row/v1
${source.family}
${canonicalRow}
`));
  });
  const sourceReceiptSha256 = sha256(Buffer.from([
    "adjustment-shadow-source-receipt/v1",
    source.registrationSha256,
    source.candidateSha256,
    source.sourceSha256,
    source.dueKey,
    source.issuedAt,
    source.rows[0].validAt,
    source.rows.at(-1).validAt,
    String(source.rowCount),
    inputSha256
  ].join("\n")));
  return { inputSha256, sourceReceiptSha256, sourceRowSha256 };
}
function parseMaintenanceShadowValues(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength > MAINTENANCE_SHADOW_LIMITS.wind.bytes) {
    throw new RangeError("invalid shadow body size");
  }
  const value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  validateMaintenanceShadowValues(value);
  const canonical = encodeCanonicalMaintenanceShadowValues(value);
  if (!canonical.equals(Buffer.from(bytes))) {
    throw new RangeError("shadow body is not canonical");
  }
  return value;
}
function createMaintenanceShadowPredictionMetadata(bytes, sourceProjectionBytes) {
  const body = parseMaintenanceShadowValues(bytes);
  validateMaintenanceShadowSourceBinding(body, sourceProjectionBytes);
  const predictionBodySha256 = sha256(Buffer.from(bytes));
  const predictionSchemaSha256 = MAINTENANCE_SHADOW_SCHEMA_SHA256[body.family];
  const minValidAt = body.rows[0].validAt;
  const maxValidAt = body.rows.at(-1).validAt;
  const predictionSha256 = sha256(Buffer.from([
    "adjustment-shadow-prediction/v3",
    body.registrationSha256,
    body.candidateSha256,
    body.sourceSha256,
    body.dueKey,
    body.issuedAt,
    minValidAt,
    maxValidAt,
    body.sourceReceiptSha256,
    body.inputSha256,
    predictionBodySha256,
    predictionSchemaSha256,
    String(body.rowCount),
    String(bytes.byteLength)
  ].join("\n")));
  return {
    bodyByteCount: bytes.byteLength,
    candidateSha256: body.candidateSha256,
    dueKey: body.dueKey,
    inputSha256: body.inputSha256,
    issuedAt: body.issuedAt,
    maxValidAt,
    minValidAt,
    predictionBodySha256,
    predictionSchemaSha256,
    predictionSha256,
    registrationSha256: body.registrationSha256,
    rowCount: body.rowCount,
    sourceReceiptSha256: body.sourceReceiptSha256,
    sourceSha256: body.sourceSha256
  };
}
function validateMaintenanceShadowValues(value) {
  exactKeys(value, COMMON_KEYS);
  if (value.contractVersion !== MAINTENANCE_SHADOW_VALUES_VERSION || !Object.hasOwn(MAINTENANCE_SHADOW_LIMITS, value.family)) {
    throw new RangeError("invalid shadow contract");
  }
  for (const key of ["registrationSha256", "candidateSha256", "sourceSha256", "sourceReceiptSha256", "inputSha256"]) {
    requireHash(value[key]);
  }
  requireInstant(value.issuedAt);
  validateDueKey(value.dueKey);
  const limit = MAINTENANCE_SHADOW_LIMITS[value.family];
  if (!Array.isArray(value.rows) || value.rowCount !== limit.rows || value.rows.length !== limit.rows) {
    throw new RangeError("invalid shadow row count");
  }
  let previousValidAt = null;
  for (const [index, row] of value.rows.entries()) {
    exactKeys(row, ROW_KEYS[value.family]);
    requireInstant(row.validAt);
    requireHash(row.sourceRowSha256);
    if (row.leadHours !== index + 1 || row.validAt <= value.issuedAt || previousValidAt !== null && Date.parse(row.validAt) - Date.parse(previousValidAt) !== 36e5) {
      throw new RangeError("shadow lead or clock differs");
    }
    previousValidAt = row.validAt;
    if (value.family === "temperature") {
      boundedValue(row.candidateTemperatureC64, -100, 70);
      validateFallback(row);
    } else if (value.family === "rain") {
      boundedValue(row.occurrenceProbability64, 0, 1);
      boundedValue(row.atLeast1_0Probability64, 0, 1);
      boundedValue(row.atLeast2_5Probability64, 0, 1);
      boundedValue(row.positiveAmountMm64, 0, 30);
      boundedValue(row.candidatePrecipitationMm64, 0, 30);
      validateFallback(row);
    } else {
      boundedValue(row.candidateSpeedMps64, 0, 150);
      requireBoolean(row.speedWouldApply);
      requireBoolean(row.gustWouldApply);
      if (index + 1 >= 49 && index + 1 <= 72) {
        if (row.candidateGustMps64 !== null || row.gustWouldApply !== false) {
          throw new RangeError("disabled gust shadow is present");
        }
      } else {
        boundedValue(row.candidateGustMps64, 0, 150);
      }
    }
  }
}
function validateMaintenanceShadowSourceProjection(value) {
  exactKeys(value, value.family === "rain" ? RAIN_SOURCE_KEYS : value.family === "temperature" ? TEMPERATURE_SOURCE_KEYS : SOURCE_KEYS);
  if (value.contractVersion !== MAINTENANCE_SHADOW_SOURCE_VERSION || !Object.hasOwn(MAINTENANCE_SHADOW_LIMITS, value.family)) {
    throw new RangeError("invalid shadow source projection contract");
  }
  for (const key of ["registrationSha256", "candidateSha256", "sourceSha256"]) {
    requireHash(value[key]);
  }
  requireInstant(value.issuedAt);
  validateDueKey(value.dueKey);
  const limit = MAINTENANCE_SHADOW_LIMITS[value.family];
  if (value.family === "rain") {
    validateRainCausalInputs(value.causalInputs, value.issuedAt);
  } else if (value.family === "temperature") {
    validateTemperatureRecentErrorState(value.recentErrorState, value.issuedAt);
  }
  if (!Array.isArray(value.rows) || value.rowCount !== limit.rows || value.rows.length !== limit.rows) {
    throw new RangeError("invalid shadow source projection row count");
  }
  let previousValidAt = null;
  for (const [index, row] of value.rows.entries()) {
    exactKeys(row, SOURCE_ROW_KEYS[value.family]);
    requireInstant(row.validAt);
    requireInstant(row.referenceAt);
    requireInstant(row.receivedAt);
    requireHash(row.sourceSha256);
    requireHash(row.contentSha256);
    const validAt = row.validAt;
    const referenceAt = row.referenceAt;
    const receivedAt = row.receivedAt;
    const computedModelLeadHours = Math.ceil(
      (Date.parse(validAt) - Date.parse(referenceAt)) / 36e5
    );
    if (row.sourceSha256 !== value.sourceSha256 || receivedAt > value.issuedAt || !Number.isInteger(row.modelLeadHours) || row.modelLeadHours !== computedModelLeadHours || computedModelLeadHours < 0 || computedModelLeadHours > 384 || row.leadHours !== index + 1 || validAt <= value.issuedAt || previousValidAt !== null && Date.parse(validAt) - Date.parse(previousValidAt) !== 36e5) {
      throw new RangeError("shadow source projection row differs");
    }
    const lineageFields = value.family === "temperature" ? ["adapterVersion", "dataset", "providerKey", "modelCycle", "upstreamModel"] : [
      "adapterVersion",
      "contractEpoch",
      "dataset",
      "providerKey",
      "sourceKey",
      "sourceConfigFingerprint",
      "upstreamModel"
    ];
    for (const field of lineageFields) {
      requireBoundedSourceString(row[field]);
    }
    if (value.family !== "temperature" && (!Number.isSafeInteger(row.revisionCount) || row.revisionCount < 0)) {
      throw new RangeError("invalid shadow source revision");
    }
    if (value.family === "temperature") {
      requireHash(row.providerResponseSha256);
      requireHash(row.bestMatchContentSha256);
      requireInstant(row.bestMatchProductRunAt);
      if (typeof row.bestMatchSourceId !== "string" || !/^[1-9]\d*$/u.test(row.bestMatchSourceId) || row.bestMatchSourceId.length > 20 || row.bestMatchProductRunAt > value.issuedAt) {
        throw new RangeError("temperature shadow comparator identity differs");
      }
      boundedValue(row.rawTemperatureC64, -100, 70);
      boundedNullableValue(row.rawRelativeHumidityPercent64, 0, 100);
      boundedNullableValue(row.rawWindSpeedMps64, 0, 150);
      boundedNullableValue(row.bestMatchTemperatureC64, -100, 70);
    } else if (value.family === "wind") {
      boundedNullableValue(row.windSpeedMps64, 0, 150);
      boundedNullableValue(row.windGustMps64, 0, 150);
    } else {
      boundedNullableValue(row.precipitationMm64, 0, 2e3);
    }
    previousValidAt = validAt;
  }
}
function canonicalTemperatureRecentErrorState(value) {
  return Object.fromEntries(TEMPERATURE_RECENT_STATE_KEYS.map((key) => [key, value[key]]));
}
function validateTemperatureRecentErrorState(value, issuedAt) {
  exactKeys(value, TEMPERATURE_RECENT_STATE_KEYS);
  const state = value;
  requireInstant(state.targetRunInitializedAt);
  requireInstant(state.windowEndValidAt);
  for (const instant of [state.maximumSourceRunInitializedAt, state.maximumSourceValidAt]) {
    if (instant !== null) {
      requireInstant(instant);
      if (instant > issuedAt) {
        throw new RangeError("temperature recent-error clock is future");
      }
    }
  }
  if (state.cohort !== "ecmwf_single_run_hindcast" || typeof state.supported !== "boolean" || !Number.isSafeInteger(state.localDates) || state.localDates < 0 || state.localDates > 366 || !Number.isSafeInteger(state.n24) || state.n24 < 0 || state.n24 > 1e4 || !Number.isSafeInteger(state.n72) || state.n72 < 0 || state.n72 > 1e4 || !Array.isArray(state.sourceKeys) || state.sourceKeys.length > 1e4) {
    throw new RangeError("temperature recent-error state is invalid");
  }
  for (const sourceKey of state.sourceKeys) {
    requireBoundedSourceString(sourceKey);
  }
  for (const statistic of [state.b24C, state.b72C, state.mad72C]) {
    if (statistic !== null && (typeof statistic !== "number" || !Number.isFinite(statistic))) {
      throw new RangeError("temperature recent-error statistic is invalid");
    }
  }
}
function canonicalRainCausalInputs(value) {
  return Object.fromEntries(RAIN_CAUSAL_KEYS.map((key) => [
    key,
    key === "captureSet" ? value.captureSet.map((capture) => Object.fromEntries(
      RAIN_CAPTURE_KEYS.map((field) => [field, capture[field]])
    )) : key === "currentRun" ? canonicalRainRun(value.currentRun) : key === "priorRuns" ? value.priorRuns.map(canonicalRainRun) : key === "stationHours" ? value.stationHours.map((hour) => Object.fromEntries(
      RAIN_STATION_HOUR_KEYS.map((field) => [field, hour[field]])
    )) : value.contractVersion
  ]));
}
function canonicalRainRun(value) {
  return Object.fromEntries(RAIN_RUN_KEYS.map((key) => [
    key,
    key === "hours" ? value.hours.map((hour) => Object.fromEntries(
      RAIN_RUN_HOUR_KEYS.map((field) => [field, hour[field]])
    )) : value[key]
  ]));
}
function validateRainCausalInputs(value, issuedAt) {
  exactKeys(value, RAIN_CAUSAL_KEYS);
  const causal = value;
  if (causal.contractVersion !== "adjustment-shadow-rain-causal-inputs/v1" || !Array.isArray(causal.captureSet) || causal.captureSet.length < 1 || causal.captureSet.length > 350 || !Array.isArray(causal.priorRuns) || causal.priorRuns.length > 2 || !Array.isArray(causal.stationHours) || causal.stationHours.length > 64) {
    throw new RangeError("invalid rain causal input contract");
  }
  const captureHashes = /* @__PURE__ */ new Set();
  const captureClaims = /* @__PURE__ */ new Set();
  for (const capture of causal.captureSet) {
    exactKeys(capture, RAIN_CAPTURE_KEYS);
    requireBoundedSourceString(capture.claimId);
    requireInstant(capture.completedAt);
    requireHash(capture.bodySha256);
    if (!["forecast", "station"].includes(capture.kind) || capture.stationId !== null && (!Number.isSafeInteger(capture.stationId) || capture.stationId < 1) || capture.kind === "forecast" !== (capture.runInitializedAt !== null) || capture.kind === "station" !== (capture.windowStart !== null && capture.windowEndExclusive !== null) || capture.kind === "forecast" && (capture.stationId !== null || capture.windowStart !== null || capture.windowEndExclusive !== null) || capture.kind === "station" && (capture.stationId === null || capture.runInitializedAt !== null) || capture.runInitializedAt !== null && !validOptionalInstant(capture.runInitializedAt) || capture.windowStart !== null && !validOptionalInstant(capture.windowStart) || capture.windowEndExclusive !== null && !validOptionalInstant(capture.windowEndExclusive) || capture.completedAt > issuedAt || captureClaims.has(capture.claimId)) {
      throw new RangeError("invalid rain capture identity");
    }
    captureHashes.add(capture.bodySha256);
    captureClaims.add(capture.claimId);
  }
  validateRainRun(causal.currentRun, issuedAt, captureHashes);
  for (const run of causal.priorRuns) {
    validateRainRun(run, issuedAt, captureHashes);
  }
  const stationKeys = /* @__PURE__ */ new Set();
  for (const hour of causal.stationHours) {
    exactKeys(hour, RAIN_STATION_HOUR_KEYS);
    requireInstant(hour.hourAt);
    requireInstant(hour.receivedAt);
    const key = `${String(hour.stationId)}:${String(hour.hourAt)}`;
    if (!Number.isSafeInteger(hour.stationId) || hour.stationId < 1 || hour.receivedAt > issuedAt || stationKeys.has(key)) {
      throw new RangeError("invalid rain station input");
    }
    boundedNullableValue(hour.precipitationMm64, 0, 500);
    boundedNullableValue(hour.temperatureC64, -100, 70);
    stationKeys.add(key);
  }
}
function validateRainRun(value, issuedAt, captureHashes) {
  exactKeys(value, RAIN_RUN_KEYS);
  requireInstant(value.runInitializedAt);
  requireInstant(value.completedAt);
  requireHash(value.contentSha256);
  const hours = value.hours;
  if (!captureHashes.has(value.contentSha256) || value.completedAt > issuedAt || !Array.isArray(hours) || hours.length !== 48) {
    throw new RangeError("invalid rain forecast input");
  }
  for (const [index, hour] of hours.entries()) {
    exactKeys(hour, RAIN_RUN_HOUR_KEYS);
    if (hour.leadHours !== index + 1) {
      throw new RangeError("invalid rain forecast input lead");
    }
    boundedNullableValue(hour.precipitationMm64, 0, 2e3);
    boundedNullableValue(hour.temperatureC64, -100, 70);
    boundedNullableValue(hour.relativeHumidityPercent64, 0, 100);
    boundedNullableValue(hour.cloudCoverPercent64, 0, 100);
    boundedNullableValue(hour.pressureHpa64, 100, 1200);
    boundedNullableValue(hour.windSpeedMps64, 0, 150);
    boundedNullableValue(hour.windDirectionDegrees64, 0, 360);
  }
}
function validOptionalInstant(value) {
  try {
    requireInstant(value);
    return true;
  } catch {
    return false;
  }
}
function validateMaintenanceShadowSourceBinding(value, sourceProjectionBytes) {
  const source = parseMaintenanceShadowSourceProjection(sourceProjectionBytes);
  const identity = createMaintenanceShadowSourceIdentity(sourceProjectionBytes);
  if (source.family !== value.family || source.registrationSha256 !== value.registrationSha256 || source.candidateSha256 !== value.candidateSha256 || source.sourceSha256 !== value.sourceSha256 || source.dueKey !== value.dueKey || source.issuedAt !== value.issuedAt || identity.inputSha256 !== value.inputSha256 || identity.sourceReceiptSha256 !== value.sourceReceiptSha256) {
    throw new RangeError("shadow source projection header differs");
  }
  for (const [index, row] of value.rows.entries()) {
    const sourceRow = source.rows[index];
    if (row.validAt !== sourceRow.validAt || row.leadHours !== sourceRow.leadHours || row.sourceRowSha256 !== identity.sourceRowSha256[index]) {
      throw new RangeError("shadow source projection binding differs");
    }
  }
}
function encodeCanonicalMaintenanceShadowValues(value) {
  const ordered = {};
  for (const key of COMMON_KEYS) {
    ordered[key] = key === "rows" ? value.rows.map(
      // preserve the exact family row field order
      (row) => Object.fromEntries(ROW_KEYS[value.family].map((field) => [field, row[field]]))
    ) : value[key];
  }
  const bytes = Buffer.from(JSON.stringify(ordered) + "\n");
  if (bytes.length > MAINTENANCE_SHADOW_LIMITS[value.family].bytes) {
    throw new RangeError("shadow body exceeds its family cap");
  }
  return bytes;
}
function validateFallback(row) {
  requireBoolean(row.wouldApply);
  if (!FALLBACK_CODES.includes(row.fallbackCode) || row.wouldApply !== (row.fallbackCode === "none")) {
    throw new RangeError("shadow fallback differs");
  }
}
function boundedValue(value, minimum, maximum) {
  const decoded = decodeMaintenanceBinary64(value);
  if (decoded < minimum || decoded > maximum) {
    throw new RangeError("shadow value exceeds its physical cap");
  }
}
function boundedNullableValue(value, minimum, maximum) {
  if (value === null) {
    return;
  }
  boundedValue(value, minimum, maximum);
}
function exactKeys(value, keys) {
  if (value === null || typeof value !== "object" || Array.isArray(value) || ![Object.prototype, null].includes(Object.getPrototypeOf(value)) || Object.keys(value).sort().join() !== [...keys].sort().join()) {
    throw new RangeError("shadow fields differ");
  }
}
function requireInstant(value) {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(value) || !Number.isFinite(Date.parse(value)) || new Date(value).toISOString() !== value) {
    throw new RangeError("invalid shadow instant");
  }
}
function validateDueKey(value) {
  if (typeof value !== "string" || !/^capture\/\d{4}-\d{2}-\d{2}T(?:00|06|12|18):35:00\.000Z$/u.test(value)) {
    throw new RangeError("invalid shadow due key");
  }
  requireInstant(value.slice(8));
}
function requireBoundedSourceString(value) {
  if (typeof value !== "string" || value.length < 1 || value.length > 128 || /[\u0000-\u001f\u007f]/u.test(value)) {
    throw new RangeError("invalid shadow source lineage");
  }
}
function requireHash(value) {
  if (typeof value !== "string" || !HASH.test(value)) {
    throw new RangeError("invalid shadow identity");
  }
}
function requireBoolean(value) {
  if (typeof value !== "boolean") {
    throw new RangeError("invalid shadow boolean");
  }
}
function sha256(bytes) {
  return createHash2("sha256").update(bytes).digest("hex");
}

// packages/forecast-adjustment/src/maintenance-revision-projection.ts
var ADJUSTMENT_REVISION_PROJECTION_VERSION = "adjustment-revision-projection/v1";
var ADJUSTMENT_REVISION_BATCH_PROJECTION_VERSION = "adjustment-revision-batch-projection/v2";
var ADJUSTMENT_TEMPERATURE_NATIVE_SOURCE_PROJECTION_VERSION = "adjustment-temperature-native-source-projection/v2";
var ADJUSTMENT_RAIN_GATE_FEATURE_PROJECTION_VERSION = "adjustment-rain-gate-feature-projection/v2";
var ADJUSTMENT_RAIN_GATE_CONTROL_PROJECTION_VERSION = "adjustment-rain-gate-control-projection/v3";
var RAIN_MAINTENANCE_PERSISTENCE_TARGET_VERSION = "rain-maintenance-persistence-target/v1";
var ADJUSTMENT_RAIN_SOURCE_CONFIG_FINGERPRINT = "96e4365f74a0fb8694944172b73a65c29d89a68ddf86b1b0a50db38bb39d52aa";
var HASH2 = /^[a-f0-9]{64}$/u;
var TOP_KEYS = [
  "contractVersion",
  "family",
  "logicalKey",
  "logicalReceivedAt",
  "projectionKind",
  "rows",
  "source",
  "storedContentSha256"
];
var TEMPERATURE_NATIVE_TOP_KEYS = [
  "contractVersion",
  "family",
  "logicalKey",
  "logicalReceivedAt",
  "projectionKind",
  "recentErrorState",
  "recentErrorStateSha256",
  "rows",
  "source",
  "storedContentSha256"
];
var TEMPERATURE_RECENT_STATE_KEYS2 = [
  "b24C",
  "b72C",
  "cohort",
  "localDates",
  "mad72C",
  "maximumSourceRunInitializedAt",
  "maximumSourceValidAt",
  "n24",
  "n72",
  "sourceKeys",
  "supported",
  "targetRunInitializedAt",
  "windowEndValidAt"
];
var SOURCE_KEYS2 = [
  "adapterVersion",
  "contractEpoch",
  "dataset",
  "providerKey",
  "sourceConfigFingerprint",
  "sourceId",
  "sourceKey",
  "sourceKind",
  "upstreamModel"
];
var LOGICAL_KEY_KEYS = Object.freeze({
  actual_best_match: ["productRunAt", "sourceId", "sourceKind", "validAt"],
  native_source: [
    "contentSha256",
    "leadHours",
    "providerResponseSha256",
    "runInitializedAt",
    "siteId",
    "sourceId",
    "sourceType",
    "validAt"
  ],
  rain_gate_input: ["inputSha256", "modelSha256", "runInitializedAt"],
  target_revision: ["productRunAt", "sourceId", "sourceKind", "validAt"]
});
var WEATHER_ROW_KEYS = [
  "apparentTemperatureC64",
  "blackGlobeTemperatureC64",
  "cloudCoverPercent64",
  "contentSha256",
  "pm25MicrogramsPerCubicMeter64",
  "precipitationMm64",
  "precipitationRateMmPerHour64",
  "pressureHpa64",
  "relativeHumidityPercent64",
  "soilElectricalConductivityMicrosiemensPerCm64",
  "soilMoisturePercent64",
  "solarRadiationWm264",
  "temperatureC64",
  "uvIndex64",
  "validAt",
  "waterLevelM64",
  "wetBulbGlobeTemperatureC64",
  "windDirectionDegrees64",
  "windGustMps64",
  "windSpeedMps64"
];
var ANCHOR_ROW_KEYS = [
  "apparentTemperatureC64",
  "cloudCoverPercent64",
  "contentSha256",
  "leadHours",
  "precipitationMm64",
  "pressureHpa64",
  "relativeHumidityPercent64",
  "temperatureC64",
  "validAt",
  "windDirectionDegrees64",
  "windGustMps64",
  "windSpeedMps64"
];
var TEMPERATURE_ROW_KEYS = [
  "bestMatchContentSha256",
  "bestMatchProductRunAt",
  "bestMatchSourceId",
  "bestMatchTemperatureC64",
  "contentSha256",
  "modelCycle",
  "modelLeadHours",
  "rawRelativeHumidityPercent64",
  "rawTemperatureC64",
  "rawWindSpeedMps64",
  "validAt"
];
var RAIN_GATE_ROW_KEYS = [
  "applied",
  "correctedPrecipitationMm64",
  "modelLeadHours",
  "rawPrecipitationMm64",
  "reasonCode",
  "validAt"
];
var RAIN_GATE_FEATURE_ROW_KEYS = [
  "features64",
  "modelLeadHours",
  "rawPrecipitationMm64",
  "rawTargetHourTemperatureC64",
  "validAt"
];
var RAIN_CONTROL_TOP_KEYS = [
  "contractVersion",
  "family",
  "logicalKey",
  "logicalReceivedAt",
  "ordinalArtifactSha256",
  "persistenceTarget",
  "projectionKind",
  "rows",
  "source",
  "stateSha256",
  "stateStageReceiptSha256",
  "storedContentSha256"
];
var RAIN_CONTROL_ROW_KEYS = [
  "features64",
  "incumbentArtifactIdentitySha256",
  "incumbentPrediction64",
  "incumbentProbability",
  "incumbentReceiptMemberSha256",
  "modelLeadHours",
  "nativeSourceProbability",
  "persistencePrediction64",
  "persistenceReason",
  "persistenceTargetMemberSha256",
  "rawPrecipitationMm64",
  "rawTargetHourTemperatureC64",
  "recentVolumeScalePrediction64",
  "sameWindowVolumeScalePrediction64",
  "sourceRowSha256",
  "unchangedOrdinalPrediction64",
  "validAt",
  "volumeScalePrediction64"
];
var PROBABILITY_KEYS = ["atLeast0_1", "atLeast1_0", "atLeast2_5"];
var PERSISTENCE_TARGET_KEYS = [
  "contractVersion",
  "decisionAt",
  "prediction64",
  "reason",
  "rowCount",
  "rows",
  "targetMemberSha256",
  "validAt"
];
var PERSISTENCE_TARGET_ROW_KEYS = ["captureMembers", "precipitationMm64", "receivedAt", "stationId"];
var PERSISTENCE_CAPTURE_KEYS = ["bodySha256", "claimId", "completedAt"];
var FAMILY_LIMITS = Object.freeze({
  rain: 128 * 1024,
  shared: 768 * 1024,
  temperature: 64 * 1024,
  wind: 768 * 1024
});
function encodeAdjustmentRevisionProjection(value) {
  validateAdjustmentRevisionProjection(value);
  return encodeRevisionProjectionDocument(value);
}
function encodeAdjustmentRevisionBatchProjection(value) {
  validateAdjustmentRevisionBatchProjection(value);
  return encodeRevisionProjectionDocument(value);
}
function encodeAdjustmentTemperatureNativeSourceProjection(value) {
  validateAdjustmentTemperatureNativeSourceProjection(value);
  const logicalKey = value.logicalKey;
  const ordered = Object.fromEntries(TEMPERATURE_NATIVE_TOP_KEYS.map((key) => [
    key,
    key === "logicalKey" ? Object.fromEntries(LOGICAL_KEY_KEYS.native_source.map((field) => [field, logicalKey[field]])) : key === "recentErrorState" ? Object.fromEntries(TEMPERATURE_RECENT_STATE_KEYS2.map((field) => [field, value.recentErrorState[field]])) : key === "rows" ? value.rows.map((row) => Object.fromEntries(TEMPERATURE_ROW_KEYS.map(
      (field) => [field, row[field]]
    ))) : key === "source" ? Object.fromEntries(SOURCE_KEYS2.map((field) => [field, value.source[field]])) : value[key]
  ]));
  const bytes = Buffer.from(JSON.stringify(ordered) + "\n");
  if (bytes.byteLength > FAMILY_LIMITS.temperature) {
    throw new RangeError("adjustment temperature native projection exceeds its family cap");
  }
  return bytes;
}
function encodeAdjustmentRainGateFeatureProjection(value) {
  validateAdjustmentRainGateFeatureProjection(value);
  return encodeRevisionProjectionDocument(value);
}
function encodeAdjustmentRainGateControlProjection(value) {
  validateAdjustmentRainGateControlProjection(value);
  const logicalKey = value.logicalKey;
  const ordered = Object.fromEntries(RAIN_CONTROL_TOP_KEYS.map((key) => [
    key,
    key === "logicalKey" ? Object.fromEntries(LOGICAL_KEY_KEYS.rain_gate_input.map((field) => [field, logicalKey[field]])) : key === "rows" ? value.rows.map((row) => Object.fromEntries(RAIN_CONTROL_ROW_KEYS.map((field) => [
      field,
      field === "incumbentProbability" || field === "nativeSourceProbability" ? Object.fromEntries(PROBABILITY_KEYS.map((name) => [name, row[field][name]])) : row[field]
    ]))) : key === "source" ? Object.fromEntries(SOURCE_KEYS2.map((field) => [field, value.source[field]])) : key === "persistenceTarget" ? orderedPersistenceTarget(value.persistenceTarget) : value[key]
  ]));
  const bytes = Buffer.from(JSON.stringify(ordered) + "\n");
  if (bytes.byteLength > FAMILY_LIMITS.rain) {
    throw new RangeError("adjustment rain control projection exceeds its family cap");
  }
  return bytes;
}
function encodeRevisionProjectionDocument(value) {
  const rowKeys = revisionRowKeys(value);
  const logicalKey = value.logicalKey;
  const ordered = Object.fromEntries(TOP_KEYS.map((key) => [
    key,
    key === "logicalKey" ? Object.fromEntries(LOGICAL_KEY_KEYS[value.projectionKind].map((field) => [field, logicalKey[field]])) : key === "rows" ? value.rows.map((row) => {
      const record2 = row;
      return Object.fromEntries(rowKeys.map((field) => [field, record2[field]]));
    }) : key === "source" ? Object.fromEntries(SOURCE_KEYS2.map((field) => [field, value.source[field]])) : value[key]
  ]));
  const bytes = Buffer.from(JSON.stringify(ordered) + "\n");
  if (bytes.byteLength > FAMILY_LIMITS[value.family]) {
    throw new RangeError("adjustment revision projection exceeds its family cap");
  }
  return bytes;
}
function parseAdjustmentRevisionProjection(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength < 2 || bytes.byteLength > FAMILY_LIMITS.wind) {
    throw new RangeError("adjustment revision projection size is invalid");
  }
  const value = JSON.parse(new TextDecoder2("utf-8", { fatal: true }).decode(bytes));
  const canonical = encodeAdjustmentRevisionProjection(value);
  if (!canonical.equals(Buffer.from(bytes))) {
    throw new RangeError("adjustment revision projection is not canonical");
  }
  return value;
}
function parseAdjustmentRevisionBatchProjection(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength < 2 || bytes.byteLength > FAMILY_LIMITS.wind) {
    throw new RangeError("adjustment revision batch projection size is invalid");
  }
  const value = JSON.parse(new TextDecoder2("utf-8", { fatal: true }).decode(bytes));
  const canonical = encodeAdjustmentRevisionBatchProjection(value);
  if (!canonical.equals(Buffer.from(bytes))) {
    throw new RangeError("adjustment revision batch projection is not canonical");
  }
  return value;
}
function parseAdjustmentTemperatureNativeSourceProjection(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength < 2 || bytes.byteLength > FAMILY_LIMITS.temperature) {
    throw new RangeError("adjustment temperature native projection size is invalid");
  }
  const value = JSON.parse(new TextDecoder2("utf-8", { fatal: true }).decode(bytes));
  const canonical = encodeAdjustmentTemperatureNativeSourceProjection(value);
  if (!canonical.equals(Buffer.from(bytes))) {
    throw new RangeError("adjustment temperature native projection is not canonical");
  }
  return value;
}
function parseAdjustmentRainGateFeatureProjection(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength < 2 || bytes.byteLength > FAMILY_LIMITS.rain) {
    throw new RangeError("adjustment rain feature projection size is invalid");
  }
  const value = JSON.parse(new TextDecoder2("utf-8", { fatal: true }).decode(bytes));
  const canonical = encodeAdjustmentRainGateFeatureProjection(value);
  if (!canonical.equals(Buffer.from(bytes))) {
    throw new RangeError("adjustment rain feature projection is not canonical");
  }
  return value;
}
function parseAdjustmentRainGateControlProjection(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength < 2 || bytes.byteLength > FAMILY_LIMITS.rain) {
    throw new RangeError("adjustment rain control projection size is invalid");
  }
  const value = JSON.parse(new TextDecoder2("utf-8", { fatal: true }).decode(bytes));
  const canonical = encodeAdjustmentRainGateControlProjection(value);
  if (!canonical.equals(Buffer.from(bytes))) {
    throw new RangeError("adjustment rain control projection is not canonical");
  }
  return value;
}
function parseAdjustmentRevisionProjectionDocument(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength < 2 || bytes.byteLength > FAMILY_LIMITS.wind) {
    throw new RangeError("adjustment revision projection size is invalid");
  }
  const value = JSON.parse(new TextDecoder2("utf-8", { fatal: true }).decode(bytes));
  if (value.contractVersion === ADJUSTMENT_REVISION_PROJECTION_VERSION) {
    return parseAdjustmentRevisionProjection(bytes);
  }
  if (value.contractVersion === ADJUSTMENT_REVISION_BATCH_PROJECTION_VERSION) {
    return parseAdjustmentRevisionBatchProjection(bytes);
  }
  if (value.contractVersion === ADJUSTMENT_TEMPERATURE_NATIVE_SOURCE_PROJECTION_VERSION) {
    return parseAdjustmentTemperatureNativeSourceProjection(bytes);
  }
  if (value.contractVersion === ADJUSTMENT_RAIN_GATE_FEATURE_PROJECTION_VERSION) {
    return parseAdjustmentRainGateFeatureProjection(bytes);
  }
  if (value.contractVersion === ADJUSTMENT_RAIN_GATE_CONTROL_PROJECTION_VERSION) {
    return parseAdjustmentRainGateControlProjection(bytes);
  }
  throw new RangeError("adjustment revision projection contract is invalid");
}
function validateAdjustmentRevisionProjection(value) {
  exactKeys2(value, TOP_KEYS);
  if (value.contractVersion !== ADJUSTMENT_REVISION_PROJECTION_VERSION || !Object.hasOwn(LOGICAL_KEY_KEYS, value.projectionKind) || !Object.hasOwn(FAMILY_LIMITS, value.family)) {
    throw new RangeError("adjustment revision projection contract is invalid");
  }
  requireInstant2(value.logicalReceivedAt);
  requireHash2(value.storedContentSha256);
  exactKeys2(value.logicalKey, LOGICAL_KEY_KEYS[value.projectionKind]);
  exactKeys2(value.source, SOURCE_KEYS2);
  validateSource(value.source, value.projectionKind);
  validateLogicalKey(value.logicalKey, value.projectionKind);
  const rowKeys = revisionRowKeys(value);
  const expectedRows = expectedRowCount(value);
  if (!Array.isArray(value.rows) || value.rows.length !== expectedRows) {
    throw new RangeError("adjustment revision projection geometry is invalid");
  }
  let previousValidAt = null;
  for (const [index, row] of value.rows.entries()) {
    exactKeys2(row, rowKeys);
    requireInstant2(row.validAt);
    if (previousValidAt !== null && expectedRows > 1 && Date.parse(String(row.validAt)) - Date.parse(previousValidAt) !== 36e5) {
      throw new RangeError("adjustment revision projection rows are not consecutive");
    }
    previousValidAt = String(row.validAt);
    validateRevisionRow(value, row, index);
  }
  validateStoredProjectionBinding(value);
}
function validateAdjustmentRevisionBatchProjection(value) {
  exactKeys2(value, TOP_KEYS);
  if (value.contractVersion !== ADJUSTMENT_REVISION_BATCH_PROJECTION_VERSION || !["actual_best_match", "target_revision"].includes(value.projectionKind) || (value.projectionKind === "actual_best_match" ? value.family !== "wind" : value.family !== "shared")) {
    throw new RangeError("adjustment revision batch projection contract is invalid");
  }
  requireInstant2(value.logicalReceivedAt);
  requireHash2(value.storedContentSha256);
  exactKeys2(value.logicalKey, LOGICAL_KEY_KEYS[value.projectionKind]);
  exactKeys2(value.source, SOURCE_KEYS2);
  validateSource(value.source, value.projectionKind);
  validateLogicalKey(value.logicalKey, value.projectionKind);
  if (value.source.sourceId !== value.logicalKey.sourceId || value.projectionKind === "actual_best_match" && (value.source.dataset !== "best_match" || value.source.upstreamModel !== "best_match")) {
    throw new RangeError("adjustment revision batch source differs");
  }
  if (!Array.isArray(value.rows) || value.rows.length < 1 || value.rows.length > 168) {
    throw new RangeError("adjustment revision batch geometry is invalid");
  }
  let previousValidAt = null;
  for (const row of value.rows) {
    exactKeys2(row, WEATHER_ROW_KEYS);
    requireInstant2(row.validAt);
    requireHash2(row.contentSha256);
    const elapsed = previousValidAt === null ? null : Date.parse(String(row.validAt)) - Date.parse(previousValidAt);
    if (elapsed !== null && (elapsed <= 0 || value.projectionKind === "actual_best_match" && elapsed !== 36e5)) {
      throw new RangeError("adjustment revision batch rows are not ordered");
    }
    previousValidAt = String(row.validAt);
    for (const [field, fieldValue] of Object.entries(row)) {
      if (field.endsWith("64")) {
        requireBinary64(fieldValue, true);
      }
    }
  }
  const first = value.rows[0];
  if (first.contentSha256 !== value.storedContentSha256 || first.validAt !== value.logicalKey.validAt) {
    throw new RangeError("adjustment revision batch stored content differs");
  }
}
function validateAdjustmentTemperatureNativeSourceProjection(value) {
  exactKeys2(value, TEMPERATURE_NATIVE_TOP_KEYS);
  if (value.contractVersion !== ADJUSTMENT_TEMPERATURE_NATIVE_SOURCE_PROJECTION_VERSION || value.family !== "temperature" || value.projectionKind !== "native_source") {
    throw new RangeError("adjustment temperature native projection contract is invalid");
  }
  const common = {
    contractVersion: ADJUSTMENT_REVISION_PROJECTION_VERSION,
    family: value.family,
    logicalKey: value.logicalKey,
    logicalReceivedAt: value.logicalReceivedAt,
    projectionKind: value.projectionKind,
    rows: value.rows,
    source: value.source,
    storedContentSha256: value.storedContentSha256
  };
  validateAdjustmentRevisionProjection(common);
  exactKeys2(value.recentErrorState, TEMPERATURE_RECENT_STATE_KEYS2);
  requireHash2(value.recentErrorStateSha256);
  const state = value.recentErrorState;
  requireInstant2(state.targetRunInitializedAt);
  requireInstant2(state.windowEndValidAt);
  for (const clock of [state.maximumSourceRunInitializedAt, state.maximumSourceValidAt]) {
    if (clock !== null) {
      requireInstant2(clock);
    }
  }
  if (state.cohort !== "ecmwf_single_run_hindcast" || state.targetRunInitializedAt !== value.logicalKey.runInitializedAt || Date.parse(state.windowEndValidAt) !== Date.parse(state.targetRunInitializedAt) - 7 * 36e5 || !Number.isSafeInteger(state.n24) || state.n24 < 0 || state.n24 > 24 || !Number.isSafeInteger(state.n72) || state.n72 < state.n24 || state.n72 > 72 || !Number.isSafeInteger(state.localDates) || state.localDates < 0 || state.localDates > state.n72 || !Array.isArray(state.sourceKeys) || state.sourceKeys.length !== state.n72 || new Set(state.sourceKeys).size !== state.sourceKeys.length) {
    throw new RangeError("adjustment temperature recent-error state differs");
  }
  for (const key of state.sourceKeys) {
    if (typeof key !== "string" || key.length < 1 || key.length > 128) {
      throw new RangeError("adjustment temperature recent-error source differs");
    }
  }
  for (const statistic of [state.b24C, state.b72C, state.mad72C]) {
    if (statistic !== null && (!Number.isFinite(statistic) || Math.abs(statistic) > 6)) {
      throw new RangeError("adjustment temperature recent-error statistic differs");
    }
  }
  const orderedState = Object.fromEntries(TEMPERATURE_RECENT_STATE_KEYS2.map(
    // preserve the database's fixed-order state hash preimage without an added newline
    (key) => [key, state[key]]
  ));
  if (createHash3("sha256").update(JSON.stringify(orderedState)).digest("hex") !== value.recentErrorStateSha256) {
    throw new RangeError("adjustment temperature recent-error identity differs");
  }
}
function validateAdjustmentRainGateFeatureProjection(value) {
  exactKeys2(value, TOP_KEYS);
  if (value.contractVersion !== ADJUSTMENT_RAIN_GATE_FEATURE_PROJECTION_VERSION || value.family !== "rain" || value.projectionKind !== "rain_gate_input") {
    throw new RangeError("adjustment rain feature projection contract is invalid");
  }
  requireInstant2(value.logicalReceivedAt);
  requireHash2(value.storedContentSha256);
  exactKeys2(value.logicalKey, LOGICAL_KEY_KEYS.rain_gate_input);
  validateLogicalKey(value.logicalKey, "rain_gate_input");
  exactKeys2(value.source, SOURCE_KEYS2);
  validateRainFeatureSource(value.source);
  if (!Array.isArray(value.rows) || value.rows.length !== 23) {
    throw new RangeError("adjustment rain feature projection geometry is invalid");
  }
  const initializedAt = Date.parse(value.logicalKey.runInitializedAt);
  for (const [index, row] of value.rows.entries()) {
    exactKeys2(row, RAIN_GATE_FEATURE_ROW_KEYS);
    requireInstant2(row.validAt);
    requireBinary64(row.rawPrecipitationMm64, false);
    requireBinary64(row.rawTargetHourTemperatureC64, false);
    const expectedLead = index + 9;
    if (row.modelLeadHours !== expectedLead || Date.parse(row.validAt) !== initializedAt + expectedLead * 36e5 || !Array.isArray(row.features64) || row.features64.length !== 107) {
      throw new RangeError("adjustment rain feature projection row differs");
    }
    for (const feature of row.features64) {
      requireBinary64(feature, true);
    }
    if (decodeMaintenanceBinary64(row.rawPrecipitationMm64) < 0) {
      throw new RangeError("adjustment rain feature source amount differs");
    }
  }
}
function validateAdjustmentRainGateControlProjection(value) {
  exactKeys2(value, RAIN_CONTROL_TOP_KEYS);
  if (value.contractVersion !== ADJUSTMENT_RAIN_GATE_CONTROL_PROJECTION_VERSION || value.family !== "rain" || value.projectionKind !== "rain_gate_input") {
    throw new RangeError("adjustment rain control projection contract is invalid");
  }
  requireInstant2(value.logicalReceivedAt);
  for (const hash of [
    value.ordinalArtifactSha256,
    value.stateSha256,
    value.stateStageReceiptSha256,
    value.storedContentSha256
  ]) {
    requireHash2(hash);
  }
  exactKeys2(value.logicalKey, LOGICAL_KEY_KEYS.rain_gate_input);
  validateLogicalKey(value.logicalKey, "rain_gate_input");
  exactKeys2(value.source, SOURCE_KEYS2);
  validateRainFeatureSource(value.source);
  validateRainMaintenancePersistenceTarget(value.persistenceTarget);
  if (!Array.isArray(value.rows) || value.rows.length !== 23) {
    throw new RangeError("adjustment rain control projection geometry is invalid");
  }
  const initializedAt = Date.parse(value.logicalKey.runInitializedAt);
  let artifactIdentity;
  let receiptIdentity;
  for (const [index, row] of value.rows.entries()) {
    exactKeys2(row, RAIN_CONTROL_ROW_KEYS);
    exactKeys2(row.incumbentProbability, PROBABILITY_KEYS);
    exactKeys2(row.nativeSourceProbability, PROBABILITY_KEYS);
    requireInstant2(row.validAt);
    for (const field of [
      "incumbentPrediction64",
      "persistencePrediction64",
      "rawPrecipitationMm64",
      "rawTargetHourTemperatureC64",
      "recentVolumeScalePrediction64",
      "sameWindowVolumeScalePrediction64",
      "unchangedOrdinalPrediction64",
      "volumeScalePrediction64"
    ]) {
      requireBinary64(row[field], false);
    }
    validateProbability(row.incumbentProbability);
    validateProbability(row.nativeSourceProbability);
    requireHash2(row.incumbentReceiptMemberSha256);
    requireHash2(row.sourceRowSha256);
    if (row.incumbentArtifactIdentitySha256 !== null) {
      requireHash2(row.incumbentArtifactIdentitySha256);
    }
    const expectedLead = index + 9;
    if (row.modelLeadHours !== expectedLead || Date.parse(row.validAt) !== initializedAt + expectedLead * 36e5 || !Array.isArray(row.features64) || row.features64.length !== 107 || row.sourceRowSha256 !== adjustmentRainGateFeatureRowSha256(value.logicalKey, row)) {
      throw new RangeError("adjustment rain control projection row differs");
    }
    for (const feature of row.features64) {
      requireBinary64(feature, true);
    }
    validateRainControlValues(row, value.persistenceTarget);
    if (index === 0) {
      artifactIdentity = row.incumbentArtifactIdentitySha256;
      receiptIdentity = row.incumbentReceiptMemberSha256;
    }
    if (artifactIdentity !== row.incumbentArtifactIdentitySha256 || receiptIdentity !== row.incumbentReceiptMemberSha256) {
      throw new RangeError("adjustment rain incumbent authority differs");
    }
  }
}
function adjustmentRainGateFeatureRowSha256(logicalKey, row) {
  exactKeys2(logicalKey, LOGICAL_KEY_KEYS.rain_gate_input);
  validateLogicalKey(logicalKey, "rain_gate_input");
  const orderedRow = Object.fromEntries(RAIN_GATE_FEATURE_ROW_KEYS.map((key) => [key, row[key]]));
  const logicalRecord = logicalKey;
  const orderedKey = Object.fromEntries(LOGICAL_KEY_KEYS.rain_gate_input.map(
    // preserve the existing rain gate key order
    (key) => [key, logicalRecord[key]]
  ));
  return createHash3("sha256").update("adjustment-rain-gate-feature-row/v2\n").update(JSON.stringify(orderedKey) + "\n").update(JSON.stringify(orderedRow) + "\n").digest("hex");
}
function validateRainMaintenancePersistenceTarget(value) {
  exactKeys2(value, PERSISTENCE_TARGET_KEYS);
  requireInstant2(value.decisionAt);
  requireInstant2(value.validAt);
  requireHash2(value.targetMemberSha256);
  if (value.contractVersion !== RAIN_MAINTENANCE_PERSISTENCE_TARGET_VERSION || Date.parse(value.decisionAt) - Date.parse(value.validAt) !== 36e5 || value.rowCount !== RAIN_COLLECTION_STATIONS.length || !Array.isArray(value.rows) || value.rows.length !== RAIN_COLLECTION_STATIONS.length || canonicalObjectSha256(
    value,
    "targetMemberSha256"
  ) !== value.targetMemberSha256) {
    throw new RangeError("rain persistence target identity differs");
  }
  const values = [];
  for (const [index, row] of value.rows.entries()) {
    exactKeys2(row, PERSISTENCE_TARGET_ROW_KEYS);
    const station = RAIN_COLLECTION_STATIONS[index];
    if (station === void 0 || row.stationId !== station.locationId || !Array.isArray(row.captureMembers)) {
      throw new RangeError("rain persistence target station differs");
    }
    let previousCapture = null;
    for (const capture of row.captureMembers) {
      exactKeys2(capture, PERSISTENCE_CAPTURE_KEYS);
      requireHash2(capture.bodySha256);
      requireInstant2(capture.completedAt);
      const order = `${capture.completedAt}
${capture.claimId}`;
      if (typeof capture.claimId !== "string" || capture.claimId.length < 1 || capture.claimId.length > 128 || Date.parse(capture.completedAt) > Date.parse(value.decisionAt) || previousCapture !== null && order <= previousCapture) {
        throw new RangeError("rain persistence target was received after decision");
      }
      previousCapture = order;
    }
    if (row.receivedAt !== null) {
      requireInstant2(row.receivedAt);
      if (Date.parse(row.receivedAt) > Date.parse(value.decisionAt) || !row.captureMembers.some((capture) => capture.completedAt === row.receivedAt)) {
        throw new RangeError("rain persistence target receipt differs");
      }
    }
    if (row.precipitationMm64 !== null) {
      if (row.captureMembers.length === 0 || row.receivedAt === null) {
        throw new RangeError("rain persistence value lacks a capture");
      }
      const precipitation = decodeMaintenanceBinary64(row.precipitationMm64);
      if (precipitation < 0) {
        throw new RangeError("rain persistence target amount differs");
      }
      values.push({
        id: station.locationId,
        value: precipitation,
        weight: rainMaintenanceStationWeight(station)
      });
    }
  }
  const nearestAvailable = value.rows.slice(0, 3).some((row) => row.precipitationMm64 !== null);
  const supported = values.length >= 3 && nearestAvailable;
  if (supported) {
    if (value.reason !== "causal_target" || value.prediction64 === null || decodeMaintenanceBinary64(value.prediction64) !== rainMaintenanceWeightedMedian(values)) {
      throw new RangeError("rain persistence target prediction differs");
    }
    return;
  }
  if (value.reason !== "raw_fallback_unavailable" || value.prediction64 !== null) {
    throw new RangeError("rain persistence target fallback differs");
  }
}
function validateProbability(value) {
  const decoded = PROBABILITY_KEYS.map((key) => decodeMaintenanceBinary64(value[key]));
  if (decoded.some((probability) => probability < 0 || probability > 1) || decoded[0] < decoded[1] || decoded[1] < decoded[2]) {
    throw new RangeError("rain control probability differs");
  }
}
function validateRainControlValues(row, persistence) {
  const raw = decodeMaintenanceBinary64(row.rawPrecipitationMm64);
  const prediction = decodeMaintenanceBinary64(row.persistencePrediction64);
  const nonnegative = [
    row.incumbentPrediction64,
    row.recentVolumeScalePrediction64,
    row.sameWindowVolumeScalePrediction64,
    row.unchangedOrdinalPrediction64,
    row.volumeScalePrediction64
  ].map(decodeMaintenanceBinary64);
  if (raw < 0 || prediction < 0 || nonnegative.some((value) => value < 0)) {
    throw new RangeError("rain control amount differs");
  }
  const native = PROBABILITY_KEYS.map((key) => decodeMaintenanceBinary64(row.nativeSourceProbability[key]));
  const expectedNative = [Number(raw >= 0.1), Number(raw >= 1), Number(raw >= 2.5)];
  if (JSON.stringify(native) !== JSON.stringify(expectedNative)) {
    throw new RangeError("rain native source probability differs");
  }
  const targetPrediction = persistence.prediction64 === null ? null : decodeMaintenanceBinary64(persistence.prediction64);
  if (targetPrediction === null) {
    if (row.persistenceReason !== "raw_fallback_unavailable" || row.persistenceTargetMemberSha256 !== null || prediction !== raw) {
      throw new RangeError("rain persistence control fallback differs");
    }
    return;
  }
  if (row.persistenceReason !== "causal_target" || row.persistenceTargetMemberSha256 !== persistence.targetMemberSha256 || prediction !== targetPrediction) {
    throw new RangeError("rain persistence control differs");
  }
}
function orderedPersistenceTarget(value) {
  return Object.fromEntries(PERSISTENCE_TARGET_KEYS.map((key) => [
    key,
    key === "rows" ? value.rows.map((row) => Object.fromEntries(PERSISTENCE_TARGET_ROW_KEYS.map(
      // retain all missing station members explicitly
      (field) => [field, field === "captureMembers" ? row.captureMembers.map((capture) => Object.fromEntries(PERSISTENCE_CAPTURE_KEYS.map(
        // retain exact provider capture identity order
        (captureField) => [captureField, capture[captureField]]
      ))) : row[field]]
    ))) : value[key]
  ]));
}
function rainMaintenanceStationWeight(station) {
  const radians = Math.PI / 180;
  const latitude = station.latitude * radians;
  const longitude = station.longitude * radians;
  const siteLatitude = RAIN_COLLECTION_POLICY.latitude * radians;
  const siteLongitude = RAIN_COLLECTION_POLICY.longitude * radians;
  const a = Math.sin((latitude - siteLatitude) / 2) ** 2 + Math.cos(latitude) * Math.cos(siteLatitude) * Math.sin((longitude - siteLongitude) / 2) ** 2;
  const distance = 6371e3 * 2 * Math.asin(Math.sqrt(a));
  return 1 / (1 + (distance / 2e3) ** 2);
}
function rainMaintenanceWeightedMedian(values) {
  const ordered = [...values].sort((left, right) => left.value - right.value || left.id - right.id);
  const half = ordered.reduce((total, item) => total + item.weight, 0) / 2;
  let cumulative = 0;
  for (const item of ordered) {
    cumulative += item.weight;
    if (cumulative >= half) {
      return item.value;
    }
  }
  throw new RangeError("rain persistence target is empty");
}
function validateRainFeatureSource(source) {
  const fingerprint = source.sourceConfigFingerprint;
  if (source.adapterVersion !== "rain-hurdle-wind-features/v1" || source.contractEpoch !== "rain-prospective-capture/v1" || source.dataset !== "ecmwf_ifs" || source.providerKey !== "open-meteo-single-runs" || fingerprint !== ADJUSTMENT_RAIN_SOURCE_CONFIG_FINGERPRINT || typeof source.sourceId !== "string" || source.sourceId.length < 1 || source.sourceId.length > 128 || source.sourceKey !== "rain-prospective-forecast" || source.sourceKind !== "forecast" || source.upstreamModel !== "ecmwf_ifs") {
    throw new RangeError("adjustment rain feature source differs");
  }
}
function validateStoredProjectionBinding(value) {
  const first = value.rows[0];
  if (value.projectionKind === "actual_best_match" || value.projectionKind === "target_revision") {
    if (first.contentSha256 !== value.storedContentSha256 || first.validAt !== value.logicalKey.validAt) {
      throw new RangeError("weather revision stored content differs");
    }
    return;
  }
  if (value.projectionKind === "native_source" && value.logicalKey.sourceType === "forecast_anchor") {
    if (first.contentSha256 !== value.storedContentSha256 || first.validAt !== value.logicalKey.validAt || first.leadHours !== value.logicalKey.leadHours) {
      throw new RangeError("forecast anchor stored content differs");
    }
    return;
  }
  if (value.projectionKind === "native_source" && value.logicalKey.contentSha256 !== value.storedContentSha256) {
    throw new RangeError("temperature run stored content differs");
  }
}
function revisionRowKeys(value) {
  if (value.contractVersion === ADJUSTMENT_RAIN_GATE_FEATURE_PROJECTION_VERSION) {
    return RAIN_GATE_FEATURE_ROW_KEYS;
  }
  if (value.projectionKind === "rain_gate_input") {
    return RAIN_GATE_ROW_KEYS;
  }
  if (value.projectionKind === "native_source" && value.logicalKey.sourceType === "ecmwf_temperature_run") {
    return TEMPERATURE_ROW_KEYS;
  }
  if (value.projectionKind === "native_source") {
    return ANCHOR_ROW_KEYS;
  }
  return WEATHER_ROW_KEYS;
}
function expectedRowCount(value) {
  if (value.projectionKind === "rain_gate_input") {
    return 23;
  }
  if (value.projectionKind === "native_source" && value.logicalKey.sourceType === "ecmwf_temperature_run") {
    return 18;
  }
  return 1;
}
function validateSource(source, kind) {
  for (const field of SOURCE_KEYS2) {
    const value = source[field];
    if (value !== null && (typeof value !== "string" || value.length < 1 || value.length > 128 || /[\u0000-\u001f\u007f]/u.test(value))) {
      throw new RangeError("adjustment revision source lineage is invalid");
    }
  }
  if (kind === "rain_gate_input") {
    if (SOURCE_KEYS2.some((field) => source[field] !== null)) {
      throw new RangeError("rain gate source lineage must be null");
    }
    return;
  }
  const required = kind === "target_revision" ? SOURCE_KEYS2.filter((field) => field !== "upstreamModel") : SOURCE_KEYS2;
  if (required.some((field) => source[field] === null) || kind === "target_revision" && source.upstreamModel !== null) {
    throw new RangeError("adjustment revision source lineage is incomplete");
  }
}
function validateLogicalKey(key, kind) {
  if (kind === "actual_best_match" || kind === "target_revision") {
    requireDecimal(key.sourceId);
    requireInstant2(key.validAt);
    if (kind === "actual_best_match") {
      requireInstant2(key.productRunAt);
      if (key.sourceKind !== "forecast") throw new RangeError("actual comparator key differs");
    } else if (key.productRunAt !== null || key.sourceKind !== "physical_sensor") {
      throw new RangeError("target revision key differs");
    }
    return;
  }
  if (kind === "rain_gate_input") {
    requireHash2(key.inputSha256);
    requireHash2(key.modelSha256);
    requireInstant2(key.runInitializedAt);
    return;
  }
  if (key.sourceType === "forecast_anchor") {
    requireDecimal(key.sourceId);
    requireInstant2(key.validAt);
    requireInteger(key.leadHours, 1, 384);
    if (key.contentSha256 !== null || key.providerResponseSha256 !== null || key.runInitializedAt !== null || key.siteId !== null) {
      throw new RangeError("forecast anchor key differs");
    }
  } else if (key.sourceType === "ecmwf_temperature_run") {
    requireHash2(key.contentSha256);
    requireHash2(key.providerResponseSha256);
    requireInstant2(key.runInitializedAt);
    requireDecimal(key.siteId);
    if (key.leadHours !== null || key.sourceId !== null || key.validAt !== null) {
      throw new RangeError("temperature run key differs");
    }
  } else {
    throw new RangeError("native source type differs");
  }
}
function validateRevisionRow(projection, row, index) {
  if (projection.projectionKind === "rain_gate_input") {
    requireInteger(row.modelLeadHours, 1, 168);
    requireBoolean2(row.applied);
    requireBinary64(row.rawPrecipitationMm64, false);
    requireBinary64(row.correctedPrecipitationMm64, true);
    if (row.reasonCode !== null) {
      requireText(row.reasonCode);
    }
    return;
  }
  if (projection.projectionKind === "native_source" && projection.logicalKey.sourceType === "ecmwf_temperature_run") {
    requireHash2(row.contentSha256);
    if (row.modelLeadHours !== index + 1) {
      throw new RangeError("temperature native lead differs");
    }
    if (row.modelCycle !== "49r1" && row.modelCycle !== "50r1") {
      throw new RangeError("temperature native model cycle differs");
    }
    if (index < 6) {
      if (row.bestMatchContentSha256 !== null || row.bestMatchProductRunAt !== null || row.bestMatchSourceId !== null || row.bestMatchTemperatureC64 !== null) {
        throw new RangeError("temperature native pre-serving comparator differs");
      }
    } else {
      requireHash2(row.bestMatchContentSha256);
      requireInstant2(row.bestMatchProductRunAt);
      requireDecimal(row.bestMatchSourceId);
      requireBinary64(row.bestMatchTemperatureC64, true);
    }
    requireBinary64(row.rawTemperatureC64, false);
    requireBinary64(row.rawRelativeHumidityPercent64, true);
    requireBinary64(row.rawWindSpeedMps64, true);
    return;
  }
  requireHash2(row.contentSha256);
  if (projection.projectionKind === "native_source") {
    requireInteger(row.leadHours, 1, 384);
  }
  for (const [field, value] of Object.entries(row)) {
    if (field.endsWith("64")) {
      requireBinary64(value, true);
    }
  }
}
function requireBinary64(value, nullable) {
  if (value === null && nullable) return;
  decodeMaintenanceBinary64(value);
}
function requireInstant2(value) {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(value) || !Number.isFinite(Date.parse(value)) || new Date(value).toISOString() !== value) {
    throw new RangeError("adjustment revision clock is invalid");
  }
}
function requireHash2(value) {
  if (typeof value !== "string" || !HASH2.test(value)) {
    throw new RangeError("adjustment revision identity is invalid");
  }
}
function requireDecimal(value) {
  if (typeof value !== "string" || !/^[1-9]\d*$/u.test(value)) {
    throw new RangeError("adjustment revision database identity is invalid");
  }
}
function requireInteger(value, minimum, maximum) {
  if (!Number.isSafeInteger(value) || Number(value) < minimum || Number(value) > maximum) {
    throw new RangeError("adjustment revision integer is invalid");
  }
}
function requireText(value) {
  if (typeof value !== "string" || value.length < 1 || value.length > 128 || /[\u0000-\u001f\u007f]/u.test(value)) {
    throw new RangeError("adjustment revision text is invalid");
  }
}
function requireBoolean2(value) {
  if (typeof value !== "boolean") {
    throw new RangeError("adjustment revision boolean is invalid");
  }
}
function exactKeys2(value, keys) {
  if (value === null || typeof value !== "object" || Array.isArray(value) || Object.keys(value).sort().join() !== [...keys].sort().join()) {
    throw new RangeError("adjustment revision projection fields differ");
  }
}

// packages/forecast-adjustment/src/maintenance-runtime-package.ts
var FORECAST_ADJUSTMENT_MAINTENANCE_RUNTIME_PACKAGE_VERSION = "forecast-adjustment-maintenance-runtime-package/v1";
var HASH3 = /^[a-f0-9]{64}$/u;
var MONTH = /^\d{4}-(?:0[1-9]|1[0-2])$/u;
var INSTANT = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;
var COMMON_KEYS2 = ["bundleSha256", "candidateSha256", "contractVersion", "dueMonth", "family", "source"];
var TEMPERATURE_KEYS = [...COMMON_KEYS2, "model"];
var WIND_KEYS = [...COMMON_KEYS2, "candidate"];
var TEMPERATURE_MODEL_KEYS = [
  "adaptiveCoefficients",
  "cohort",
  "contractVersion",
  "directCoefficients",
  "effectiveFrom",
  "latestTrainingValidAt",
  "learnedStrengthContractVersion",
  "scope",
  "strengthBands",
  "supported",
  "trainingCutoffUtc"
];
var TEMPERATURE_SOURCE = Object.freeze({
  adapterVersion: "open-meteo-ecmwf-single-run/v1",
  cohort: "ecmwf_single_run_hindcast",
  dataset: "single_run",
  maximumReceiptAgeHours: 12,
  providerKey: "open-meteo",
  scope: "assumed_delay6_next12",
  sourceDelayHours: 6,
  upstreamModel: "ecmwf_ifs"
});
var RAIN_SOURCE = Object.freeze({
  contractVersion: "forecast-adjustment-rain-maintenance-source-identity/v1",
  forecast: {
    adapterVersion: "open-meteo-rain-capture/v1",
    contractEpoch: RAIN_COLLECTION_POLICY.contractVersion,
    dataset: "ecmwf_ifs",
    providerKey: "open-meteo-single-runs",
    sourceConfigFingerprint: ADJUSTMENT_RAIN_SOURCE_CONFIG_FINGERPRINT,
    sourceKey: "rain-prospective-forecast",
    upstreamModel: "ecmwf_ifs"
  },
  policy: RAIN_COLLECTION_POLICY,
  stations: RAIN_COLLECTION_STATIONS
});
function verifyForecastAdjustmentMaintenanceRuntimePackage(value) {
  exactKeys3(
    value,
    value.family === "temperature" ? TEMPERATURE_KEYS : WIND_KEYS,
    "maintenance runtime package"
  );
  if (value.contractVersion !== FORECAST_ADJUSTMENT_MAINTENANCE_RUNTIME_PACKAGE_VERSION || !["temperature", "wind"].includes(value.family) || !MONTH.test(value.dueMonth) || !HASH3.test(value.candidateSha256) || !HASH3.test(value.bundleSha256) || canonicalObjectSha256(value, "bundleSha256") !== value.bundleSha256) {
    throw new RangeError("maintenance runtime package identity differs");
  }
  if (value.family === "temperature") {
    validateTemperatureModel(value.model, value.dueMonth);
    if (canonicalJsonBytes(value.source) !== canonicalJsonBytes(TEMPERATURE_SOURCE)) {
      throw new RangeError("temperature maintenance source differs");
    }
    return;
  }
  verifyForecastAdjustmentCandidate(value.candidate);
  validateWindCandidate(value.candidate);
  if (canonicalJsonBytes(value.source) !== canonicalJsonBytes(value.candidate.forecastIdentity)) {
    throw new RangeError("wind maintenance source differs");
  }
}
function validateWindCandidate(candidate) {
  const pairs = candidate.enabledMetricBands.map((pair) => `${pair.metric}:${pair.leadBand}`);
  if (pairs.length !== 13 || new Set(pairs).size !== 13 || pairs.some((pair) => pair.startsWith("windDirectionDegrees:") || pair === "windGustMps:049-072")) {
    throw new RangeError("wind maintenance candidate bands differ");
  }
}
function validateTemperatureModel(value, dueMonth) {
  exactKeys3(value, TEMPERATURE_MODEL_KEYS, "temperature maintenance model");
  const model = value;
  if (model.contractVersion !== "temperature-permanent-model/v1" || model.cohort !== "ecmwf_single_run_hindcast" || model.scope !== "assumed_delay6_next12" || model.learnedStrengthContractVersion !== "temperature-winner-extensions-research/v1" || model.supported !== true || !finiteVector(model.adaptiveCoefficients, 49) || !finiteVector(model.directCoefficients, 35) || !validInstant(model.effectiveFrom) || !validInstant(model.latestTrainingValidAt) || !validInstant(model.trainingCutoffUtc) || !String(model.effectiveFrom).startsWith(`${dueMonth}-`)) {
    throw new RangeError("temperature maintenance model differs");
  }
  exactKeys3(model.strengthBands, ["1-6", "7-12"], "temperature maintenance strength bands");
  for (const key of ["1-6", "7-12"]) {
    const band = model.strengthBands[key];
    exactKeys3(band, ["alpha", "supported", "trainingCutoffUtc"], "temperature maintenance strength band");
    const material = band;
    if (![0.35, 0.5, 0.65, 0.8, 1].includes(Number(material.alpha)) || material.supported !== true || !validInstant(material.trainingCutoffUtc) || Date.parse(String(material.trainingCutoffUtc)) >= Date.parse(String(model.effectiveFrom))) {
      throw new RangeError("temperature maintenance strength band differs");
    }
  }
}
function finiteVector(value, width) {
  return Array.isArray(value) && value.length === width && value.every(
    // reject nonnumeric or nonfinite parameters
    (entry) => typeof entry === "number" && Number.isFinite(entry)
  );
}
function exactKeys3(value, keys, label) {
  if (!record(value) || Object.keys(value).sort().join("\n") !== [...keys].sort().join("\n")) {
    throw new RangeError(`${label} fields differ`);
  }
}
function record(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value) && [Object.prototype, null].includes(Object.getPrototypeOf(value));
}
function validInstant(value) {
  return typeof value === "string" && INSTANT.test(value) && Number.isFinite(Date.parse(value)) && new Date(value).toISOString() === value;
}

// packages/forecast-adjustment/src/rain-hurdle-wind.ts
import { createHash as createHash4 } from "node:crypto";

// packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts
var RAIN_HURDLE_WIND_ARTIFACT_SHA256 = "dfba22520eb66944046f999e96ee51378913ef926a2da27f0562490d9fb7ef64";

// packages/forecast-adjustment/src/rain-hurdle-wind.ts
var RAIN_HURDLE_WIND_RUNTIME_VERSION = "rain-hurdle-wind-runtime/v1";
var RAIN_HURDLE_WIND_RUNTIME_V2_VERSION = "rain-hurdle-wind-runtime/v2";
var RAIN_HURDLE_WIND_PROJECTION_IDS = [
  "R0_exact_refit",
  "R1_winter_scale_0_90",
  "R2_winter_scale_0_95",
  "R3_spring_wet_logit_plus_0_20",
  "R4_summer_wet_logit_plus_0_20",
  "R5_nested_cumulative_min",
  "R6_heavy_raw_blend_0_25"
];
var STATION_IDS = RAIN_COLLECTION_STATIONS.map((station) => station.locationId);
var DECISION_DELAY_HOURS = RAIN_COLLECTION_POLICY.decisionDelayHours;
var THRESHOLDS = [0.1, 1, 2.5];
var PUBLIC_ARTIFACT_V1_KEYS = ["categoryScales", "contractVersion", "featureNames", "heads", "modelMonth", "rules"];
var PUBLIC_ARTIFACT_V2_KEYS = [...PUBLIC_ARTIFACT_V1_KEYS, "projectionId"];
var LEGACY_ACTIVE_ARTIFACT_KEYS = [...PUBLIC_ARTIFACT_V1_KEYS, "nativeModelSha256", "provenanceSha256"];
var RAIN_HURDLE_WIND_FEATURE_NAMES_SHA256 = "6c8be26782bae556a152aa83ecf2f9dec2089561a635a20d6510f16a979a3650";
function validateRainHurdleWindPortableArtifact(artifactJson, expectedSha256) {
  const parsed = parseRainHurdleWindArtifact(artifactJson, expectedSha256);
  if (parsed.contractVersion === RAIN_HURDLE_WIND_RUNTIME_VERSION ? !exactObjectKeys(parsed, PUBLIC_ARTIFACT_V1_KEYS) : !exactObjectKeys(parsed, PUBLIC_ARTIFACT_V2_KEYS)) {
    throw new RangeError("rain model artifact schema mismatch");
  }
  return validateRainHurdleWindArtifactShape(parsed);
}
function parseRainHurdleWindArtifact(artifactJson, expectedSha256) {
  const digest = createHash4("sha256").update(artifactJson).digest("hex");
  if (!/^[a-f0-9]{64}$/u.test(expectedSha256) || digest !== expectedSha256) {
    throw new RangeError("rain model artifact digest mismatch");
  }
  const parsed = JSON.parse(artifactJson);
  if (!plainRecord(parsed)) {
    throw new RangeError("rain model artifact schema mismatch");
  }
  return parsed;
}
function validateRainHurdleWindArtifactShape(parsed) {
  if (![RAIN_HURDLE_WIND_RUNTIME_VERSION, RAIN_HURDLE_WIND_RUNTIME_V2_VERSION].includes(parsed.contractVersion) || typeof parsed.modelMonth !== "string" || !/^\d{4}-(?:0[1-9]|1[0-2])$/u.test(parsed.modelMonth) || !Array.isArray(parsed.featureNames) || parsed.featureNames.length !== 107 || parsed.featureNames.some((name) => typeof name !== "string") || createHash4("sha256").update(JSON.stringify(parsed.featureNames)).digest("hex") !== RAIN_HURDLE_WIND_FEATURE_NAMES_SHA256 || !Array.isArray(parsed.rules) || parsed.rules.length !== THRESHOLDS.length || !Array.isArray(parsed.categoryScales) || parsed.categoryScales.length !== THRESHOLDS.length || !plainRecord(parsed.heads) || !exactObjectKeys(parsed.heads, ["0.1", "1.0", "2.5", "amount"])) {
    throw new RangeError("rain model artifact schema mismatch");
  }
  if (parsed.contractVersion === RAIN_HURDLE_WIND_RUNTIME_V2_VERSION && !RAIN_HURDLE_WIND_PROJECTION_IDS.includes(parsed.projectionId)) {
    throw new RangeError("rain model artifact schema mismatch");
  }
  for (let index = 0; index < THRESHOLDS.length; index += 1) {
    const rule = parsed.rules[index];
    const scale = parsed.categoryScales[index];
    if (!plainRecord(rule) || !exactObjectKeys(rule, ["cutoff", "threshold"]) || rule.threshold !== THRESHOLDS[index] || !(rule.cutoff === null || typeof rule.cutoff === "number" && Number.isFinite(rule.cutoff) && rule.cutoff >= 0 && rule.cutoff <= 1) || typeof scale !== "number" || !Number.isFinite(scale) || scale <= 0) {
      throw new RangeError("rain model artifact schema mismatch");
    }
  }
  const artifact = parsed;
  for (const name of ["0.1", "1.0", "2.5", "amount"]) {
    const head = artifact.heads[name];
    if (!plainRecord(head) || !exactObjectKeys(head, ["baseScore", "objective", "trees"]) || !Array.isArray(head.trees) || head.trees.length !== 160 || head.objective !== (name === "amount" ? "reg:gamma" : "binary:logistic") || !Number.isFinite(head.baseScore) || head.baseScore <= 0 || name !== "amount" && head.baseScore >= 1) {
      throw new RangeError("rain model head schema mismatch");
    }
    for (const tree of head.trees) {
      if (!Array.isArray(tree) || tree.length !== 5 || tree.some((array) => !Array.isArray(array) || array.some((value) => typeof value !== "number"))) {
        throw new RangeError("rain model tree array mismatch");
      }
      const length = tree[0].length;
      if (length === 0 || tree.some((array) => array.length !== length)) {
        throw new RangeError("rain model tree array mismatch");
      }
      for (let node = 0; node < length; node += 1) {
        const left = tree[2][node];
        const right = tree[3][node];
        const feature = tree[0][node];
        const value = tree[1][node];
        const missingLeft = tree[4][node];
        if (left === void 0 || right === void 0 || feature === void 0 || value === void 0 || missingLeft === void 0 || !Number.isFinite(value) || !Number.isInteger(feature) || feature < 0 || feature >= 107 || ![0, 1].includes(missingLeft) || left !== -1 && (!Number.isInteger(left) || left <= node || left >= length) || right !== -1 && (!Number.isInteger(right) || right <= node || right >= length) || left === -1 !== (right === -1)) {
          throw new RangeError("rain model tree node mismatch");
        }
      }
    }
  }
  return artifact;
}
function plainRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value) && [Object.prototype, null].includes(Object.getPrototypeOf(value));
}
function exactObjectKeys(value, keys) {
  return Object.keys(value).sort().join("\n") === [...keys].sort().join("\n");
}

// packages/forecast-adjustment/src/rain-maintenance-controls.ts
import { TextDecoder as TextDecoder3 } from "node:util";
var RAIN_MAINTENANCE_CONTROL_STATE_VERSION = "rain-maintenance-control-state/v1";
var RAIN_MAINTENANCE_CONTROL_STATE_LIMIT_BYTES = 32 * 1024;
var HASH4 = /^[a-f0-9]{64}$/u;
var MONTH2 = /^\d{4}-(?:0[1-9]|1[0-2])$/u;
var UTC_MILLISECOND = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;
var DAY_MILLISECONDS = 24 * 60 * 60 * 1e3;
var STATE_KEYS = [
  "calibrationEndAt",
  "calibrationStartAt",
  "contractVersion",
  "epochWitnessSha256",
  "generatedAt",
  "legacyCalibrationStartAt",
  "legacyRawScale64",
  "modelMonth",
  "ordinalArtifactSha256",
  "recentFallbackReason",
  "recentRawScale64",
  "recentSupported",
  "recipeSha256",
  "sameWindowRawScale64",
  "scheduleContractSha256",
  "sourceMemberRootSha256",
  "sourceReceiptRootSha256",
  "stateSha256",
  "support",
  "trainingMaximumValidAt"
];
var SUPPORT_KEYS = [
  "calibrationDates",
  "calibrationHours",
  "calibrationRows",
  "calibrationWetDates",
  "calibrationWetHours",
  "effectiveDates64",
  "effectiveWetDates64",
  "legacyCalibrationRows",
  "legacyTrainingRows",
  "legacyTrainingWetRows",
  "trainingDates",
  "trainingHours",
  "trainingRows",
  "trainingWetDates",
  "trainingWetHours"
];
var RAIN_MAINTENANCE_CONTROL_RECIPE = Object.freeze({
  calibrationGapDays: 7,
  contractVersion: "rain-maintenance-control-recipe/v1",
  legacyCalibrationDays: 45,
  legacyRawScaleBounds: [0.5, 2],
  legacyScaleFallback: "unit_when_weighted_raw_mean_lte_1e-12",
  maximumPredictionMm: 30,
  ordinalAmountScaleBounds: [0.1, 3],
  ordinalEventThresholdsMm: [0.1, 1, 2.5],
  ordinalNestingFallback: "all_raw_event_rules",
  recentFallback: "same_window_volume_scale",
  recentHalfLifeDays: 30,
  recentMinimumEffectiveDates: 30,
  recentMinimumEffectiveWetDates: 3,
  sameWindowCalibrationDays: 90,
  scalarIterations: 64,
  scalarScaleBounds: [0.1, 3],
  support: {
    calibrationDates: 60,
    calibrationHours: 500,
    calibrationWetDates: 5,
    calibrationWetHours: 20,
    trainingDates: 180,
    trainingHours: 1e3,
    trainingWetDates: 20,
    trainingWetHours: 100
  },
  weight: "equal_date_hour_vintage"
});
var RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256 = canonicalSha256(
  RAIN_MAINTENANCE_CONTROL_RECIPE
);
function encodeRainMaintenanceControlState(value) {
  validateRainMaintenanceControlState(value);
  const ordered = Object.fromEntries(STATE_KEYS.map((key) => [
    key,
    key === "support" ? Object.fromEntries(SUPPORT_KEYS.map((field) => [field, value.support[field]])) : value[key]
  ]));
  const bytes = Buffer.from(JSON.stringify(ordered) + "\n");
  if (bytes.byteLength > RAIN_MAINTENANCE_CONTROL_STATE_LIMIT_BYTES) {
    throw new RangeError("rain control state exceeds its cap");
  }
  return bytes;
}
function parseRainMaintenanceControlState(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength < 2 || bytes.byteLength > RAIN_MAINTENANCE_CONTROL_STATE_LIMIT_BYTES) {
    throw new RangeError("rain control state size is invalid");
  }
  const value = JSON.parse(new TextDecoder3("utf-8", { fatal: true }).decode(bytes));
  const canonical = encodeRainMaintenanceControlState(value);
  if (!canonical.equals(Buffer.from(bytes))) {
    throw new RangeError("rain control state is not canonical");
  }
  return value;
}
function validateRainMaintenanceControlState(value) {
  exactKeys4(value, STATE_KEYS, "rain control state");
  exactKeys4(value.support, SUPPORT_KEYS, "rain control support");
  const monthStart = Date.parse(`${value.modelMonth}-01T00:00:00.000Z`);
  const calibrationStart = Date.parse(value.calibrationStartAt);
  const calibrationEnd = Date.parse(value.calibrationEndAt);
  const legacyStart = Date.parse(value.legacyCalibrationStartAt);
  const trainingMaximum = Date.parse(value.trainingMaximumValidAt);
  const generated = Date.parse(value.generatedAt);
  const legacyScale = decodeMaintenanceBinary64(value.legacyRawScale64);
  const sameWindowScale = decodeMaintenanceBinary64(value.sameWindowRawScale64);
  const recentScale = decodeMaintenanceBinary64(value.recentRawScale64);
  if (value.contractVersion !== RAIN_MAINTENANCE_CONTROL_STATE_VERSION || !MONTH2.test(value.modelMonth) || !HASH4.test(value.epochWitnessSha256) || !HASH4.test(value.ordinalArtifactSha256) || value.recipeSha256 !== RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256 || !HASH4.test(value.scheduleContractSha256) || !HASH4.test(value.sourceMemberRootSha256) || !HASH4.test(value.sourceReceiptRootSha256) || !HASH4.test(value.stateSha256) || canonicalObjectSha256(value, "stateSha256") !== value.stateSha256) {
    throw new RangeError("rain control state identity differs");
  }
  if (![
    value.calibrationStartAt,
    value.calibrationEndAt,
    value.legacyCalibrationStartAt,
    value.trainingMaximumValidAt,
    value.generatedAt
  ].every(validInstant2) || calibrationEnd - calibrationStart !== 90 * DAY_MILLISECONDS || calibrationEnd - legacyStart !== 45 * DAY_MILLISECONDS || monthStart - calibrationEnd !== 7 * DAY_MILLISECONDS || trainingMaximum >= calibrationStart - 7 * DAY_MILLISECONDS || generated < calibrationEnd || generated >= monthStart) {
    throw new RangeError("rain control state chronology differs");
  }
  if (legacyScale < 0.5 || legacyScale > 2 || sameWindowScale < 0.1 || sameWindowScale > 3 || recentScale < 0.1 || recentScale > 3 || value.recentSupported !== (value.recentFallbackReason === "recent_calibration") || !value.recentSupported && recentScale !== sameWindowScale) {
    throw new RangeError("rain control state scales differ");
  }
  validateSupport(value.support, value.recentSupported);
}
function validateSupport(value, recentSupported) {
  const integerFields = SUPPORT_KEYS.filter((key) => !key.endsWith("64"));
  for (const field of integerFields) {
    const count = value[field];
    if (typeof count !== "number" || !Number.isSafeInteger(count) || count < 0) {
      throw new RangeError("rain control support differs");
    }
  }
  const effectiveDates = decodeMaintenanceBinary64(value.effectiveDates64);
  const effectiveWetDates = decodeMaintenanceBinary64(value.effectiveWetDates64);
  const minimum = RAIN_MAINTENANCE_CONTROL_RECIPE.support;
  const commonSupported = value.trainingDates >= minimum.trainingDates && value.trainingHours >= minimum.trainingHours && value.trainingWetDates >= minimum.trainingWetDates && value.trainingWetHours >= minimum.trainingWetHours && value.calibrationDates >= minimum.calibrationDates && value.calibrationHours >= minimum.calibrationHours && value.calibrationWetDates >= minimum.calibrationWetDates && value.calibrationWetHours >= minimum.calibrationWetHours && value.legacyTrainingRows >= 1e3 && value.legacyTrainingWetRows >= 100 && value.legacyCalibrationRows >= 200;
  const effectiveSupported = effectiveDates >= 30 && effectiveWetDates >= 3;
  const expectedRecentSupport = commonSupported && effectiveSupported;
  if (recentSupported !== expectedRecentSupport || value.trainingRows < value.trainingHours || value.calibrationRows < value.calibrationHours) {
    throw new RangeError("rain control support differs");
  }
}
function exactKeys4(value, keys, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value) || ![Object.prototype, null].includes(Object.getPrototypeOf(value)) || Object.keys(value).sort().join("\n") !== [...keys].sort().join("\n")) {
    throw new RangeError(`${label} fields differ`);
  }
}
function validInstant2(value) {
  return UTC_MILLISECOND.test(value) && Number.isFinite(Date.parse(value)) && new Date(value).toISOString() === value;
}

// packages/forecast-adjustment/src/maintenance-shadow-catalog.ts
var RECEIPT_V1_KEYS = [
  "contractVersion",
  "actionSha256",
  "registrationSha256",
  "candidateGraphSha256",
  "candidateSha256",
  "bundleSha256",
  "sourceSha256",
  "deployedCommit",
  "deployedRelease",
  "deployedImageDigest",
  "deployedSettingsSha256",
  "controlPlaneSha256",
  "fencingToken",
  "installedAt"
];
var RECEIPT_V2_KEYS = [
  ...RECEIPT_V1_KEYS,
  "actionKind",
  "fullMemberRootSha256",
  "lifecycleHeadSha256",
  "policyDecision",
  "policyReportSha256"
];
var REGISTRATION_V2_KEYS = [
  "artifactSha256",
  "candidateSha256",
  "cohortSha256",
  "family",
  "intervalEndAt",
  "intervalStartAt",
  "policySha256",
  "registrationSha256",
  "reservedKeySha256",
  "siteKey",
  "sourceSha256",
  "targetCutoffAt",
  "terminalAt"
];
var REGISTRATION_V3_KEYS = [
  ...REGISTRATION_V2_KEYS,
  "epochWitnessSha256",
  "predecessorRegistrationSha256",
  "scheduleContractSha256"
];
var RAIN_PACKAGE_V1_KEYS = [
  "artifactPath",
  "artifactSha256",
  "candidateSha256",
  "compiledSourcePath",
  "compiledSourceSha256",
  "contractVersion",
  "modelMonth",
  "registrySha256"
];
var RAIN_PACKAGE_V2_KEYS = [
  ...RAIN_PACKAGE_V1_KEYS,
  "controlStatePath",
  "controlStateSha256",
  "ordinalArtifactPath",
  "ordinalArtifactSha256"
];

// packages/forecast-adjustment/src/maintenance-shadow-comparator.ts
import { createHash as createHash5 } from "node:crypto";
import { TextDecoder as TextDecoder4 } from "node:util";

// packages/forecast-adjustment/src/temperature-mos-runtime.ts
var TEMPERATURE_MOS_RUNTIME_POLICY = Object.freeze({
  cohort: "ecmwf_single_run_hindcast",
  correctionWeight: 0.5,
  maximumCorrectionC: 3,
  maximumModelLeadHours: 12,
  minimumModelLeadHours: 1,
  physicalMaximumC: 70,
  physicalMinimumC: -100,
  scope: "initialization_first12",
  sourceDelayHours: 7
});
var TEMPERATURE_MOS_DELAYED_RUNTIME_POLICY = Object.freeze({
  cohort: "ecmwf_single_run_hindcast",
  correctionWeight: 0.5,
  maximumCorrectionC: 3,
  maximumModelLeadHours: 18,
  minimumModelLeadHours: 7,
  physicalMaximumC: 70,
  physicalMinimumC: -100,
  scope: "assumed_delay6_next12",
  sourceDelayHours: 7,
  operationalDelayHours: 6
});
var TEMPERATURE_PERMANENT_MODEL_CONTRACT_VERSION = "temperature-permanent-model/v1";
var ECMWF_50R1_CUTOVER_MILLISECONDS = Date.parse(
  "2026-05-12T06:00:00.000Z"
);

// packages/forecast-adjustment/src/temperature-canary.ts
var TEMPERATURE_CANARY_BUNDLE_CONTRACT_VERSION = "forecast-adjustment-temperature-canary-bundle/v1";
var TEMPERATURE_CANARY_AUTHORIZATION_CONTRACT_VERSION_V2 = "forecast-adjustment-temperature-canary-authorization/v2";
var TEMPERATURE_CANARY_BUNDLE_CONTRACT_VERSION_V2 = "forecast-adjustment-temperature-canary-bundle/v2";
var TEMPERATURE_CANARY_MAXIMUM_DURATION_MS = 14 * 24 * 60 * 60 * 1e3;
var HASH_PATTERN2 = /^[a-f0-9]{64}$/u;
var BUNDLE_KEYS2 = [
  "artifactKind",
  "authorization",
  "bundleSha256",
  "contractVersion",
  "evidence",
  "model",
  "runtimeFingerprint",
  "servedForecastIdentity",
  "siteKey",
  "timezone",
  "trainingForecastIdentity"
];
var AUTHORIZATION_KEYS2 = [
  "activatedAt",
  "authorizationReason",
  "authorizationSha256",
  "authorized",
  "authorizedAt",
  "authorizedBy",
  "expiresAt"
];
var PERMANENT_AUTHORIZATION_KEYS2 = [
  "activatedAt",
  "artifactKind",
  "authorizationReason",
  "authorizationSha256",
  "authorized",
  "authorizedAt",
  "authorizedBy",
  "contractVersion",
  "expiresAt",
  "permanent"
];
var EVIDENCE_KEYS = [
  "modelSourceSha256",
  "researchSummarySha256",
  "retentionManifestSha256",
  "strengthSourceSha256"
];
var RUNTIME_FINGERPRINT_KEYS3 = ["icuVersion", "tzdataVersion"];
var SERVED_IDENTITY_KEYS = [
  "adapterVersion",
  "dataset",
  "maximumReceiptAgeHours",
  "providerKey",
  "sourceDelayHours",
  "upstreamModel"
];
var TRAINING_IDENTITY_KEYS = ["cohort", "scope"];
var MODEL_KEYS = [
  "adaptiveCoefficients",
  "cohort",
  "contractVersion",
  "directCoefficients",
  "learnedStrengthContractVersion",
  "month",
  "scope",
  "strengthBands",
  "supported",
  "trainingCutoffUtc"
];
var PERMANENT_MODEL_KEYS = [
  "adaptiveCoefficients",
  "cohort",
  "contractVersion",
  "directCoefficients",
  "effectiveFrom",
  "latestTrainingValidAt",
  "learnedStrengthContractVersion",
  "scope",
  "strengthBands",
  "supported",
  "trainingCutoffUtc"
];
var STRENGTH_KEYS = ["alpha", "supported", "trainingCutoffUtc"];
function verifyForecastAdjustmentTemperatureCanaryRuntimeBundle(bundle) {
  requireExactKeys3(bundle, BUNDLE_KEYS2, "temperature canary bundle");
  if (bundle.artifactKind !== "ecmwf_temperature_transfer_canary" || bundle.siteKey !== "ballydidean" || bundle.timezone !== "America/Los_Angeles") {
    throw new RangeError("temperature canary bundle identity mismatch");
  }
  validateHash2(bundle.bundleSha256, "bundleSha256");
  if (bundle.contractVersion === TEMPERATURE_CANARY_BUNDLE_CONTRACT_VERSION) {
    validateAuthorization(bundle.authorization);
    if (bundle.model.contractVersion !== "temperature-shortlead-models-research/v1") {
      throw new RangeError("temperature canary model contract mismatch");
    }
  } else if (bundle.contractVersion === TEMPERATURE_CANARY_BUNDLE_CONTRACT_VERSION_V2) {
    validatePermanentAuthorization(bundle.authorization);
    if (bundle.model.contractVersion !== TEMPERATURE_PERMANENT_MODEL_CONTRACT_VERSION) {
      throw new RangeError("temperature canary model contract mismatch");
    }
  } else {
    throw new RangeError("temperature canary bundle identity mismatch");
  }
  requireExactKeys3(bundle.evidence, EVIDENCE_KEYS, "temperature canary evidence");
  for (const [key, value] of Object.entries(bundle.evidence)) {
    validateHash2(value, key);
  }
  requireExactKeys3(
    bundle.runtimeFingerprint,
    RUNTIME_FINGERPRINT_KEYS3,
    "temperature canary runtime fingerprint"
  );
  validateText2(bundle.runtimeFingerprint.icuVersion, "icuVersion");
  validateText2(bundle.runtimeFingerprint.tzdataVersion, "tzdataVersion");
  requireExactKeys3(
    bundle.servedForecastIdentity,
    SERVED_IDENTITY_KEYS,
    "temperature canary served identity"
  );
  if (bundle.servedForecastIdentity.dataset !== "single_run" || bundle.servedForecastIdentity.adapterVersion !== "open-meteo-ecmwf-single-run/v1" || bundle.servedForecastIdentity.providerKey !== "open-meteo" || bundle.servedForecastIdentity.upstreamModel !== "ecmwf_ifs" || bundle.servedForecastIdentity.sourceDelayHours !== 6 || !Number.isInteger(bundle.servedForecastIdentity.maximumReceiptAgeHours) || bundle.servedForecastIdentity.maximumReceiptAgeHours < 6 || bundle.servedForecastIdentity.maximumReceiptAgeHours > 12) {
    throw new RangeError("temperature canary served identity mismatch");
  }
  requireExactKeys3(
    bundle.trainingForecastIdentity,
    TRAINING_IDENTITY_KEYS,
    "temperature canary training identity"
  );
  if (bundle.trainingForecastIdentity.cohort !== "ecmwf_single_run_hindcast" || bundle.trainingForecastIdentity.scope !== "assumed_delay6_next12") {
    throw new RangeError("temperature canary training identity mismatch");
  }
  validateSanitizedModel(bundle.model);
  if (canonicalObjectSha256(
    bundle,
    "bundleSha256"
  ) !== bundle.bundleSha256) {
    throw new RangeError("temperature canary bundle SHA-256 mismatch");
  }
}
function validateSanitizedModel(model) {
  const permanent = model.contractVersion === TEMPERATURE_PERMANENT_MODEL_CONTRACT_VERSION;
  requireExactKeys3(
    model,
    permanent ? PERMANENT_MODEL_KEYS : MODEL_KEYS,
    "temperature canary model"
  );
  if (model.contractVersion !== "temperature-shortlead-models-research/v1" && model.contractVersion !== TEMPERATURE_PERMANENT_MODEL_CONTRACT_VERSION || model.learnedStrengthContractVersion !== "temperature-winner-extensions-research/v1" || model.cohort !== "ecmwf_single_run_hindcast" || model.scope !== TEMPERATURE_MOS_DELAYED_RUNTIME_POLICY.scope || model.supported !== true) {
    throw new RangeError("temperature canary model identity mismatch");
  }
  const cutoff = Date.parse(validateUtcInstant3(model.trainingCutoffUtc, "trainingCutoffUtc"));
  if (permanent) {
    const staticModel = model;
    const effectiveFrom = Date.parse(validateUtcInstant3(staticModel.effectiveFrom, "effectiveFrom"));
    const latestTrainingValidAt = Date.parse(validateUtcInstant3(
      staticModel.latestTrainingValidAt,
      "latestTrainingValidAt"
    ));
    if (latestTrainingValidAt + 7 * 36e5 > cutoff || cutoff > effectiveFrom) {
      throw new RangeError("permanent temperature training boundary is invalid");
    }
  } else if (!/^\d{4}-\d{2}$/u.test(model.month)) {
    throw new RangeError("temperature canary model identity mismatch");
  }
  validateCoefficientVector(model.directCoefficients, 35, "directCoefficients");
  validateCoefficientVector(model.adaptiveCoefficients, 49, "adaptiveCoefficients");
  if (model.strengthBands === null || typeof model.strengthBands !== "object" || Object.keys(model.strengthBands).sort().join(",") !== "1-6,7-12") {
    throw new RangeError("temperature canary strength bands are invalid");
  }
  for (const key of ["1-6", "7-12"]) {
    const band = model.strengthBands[key];
    requireExactKeys3(band, STRENGTH_KEYS, `temperature strength ${key}`);
    if (band.supported !== true || band.alpha !== 1 || band.trainingCutoffUtc !== model.trainingCutoffUtc) {
      throw new RangeError("temperature canary strength band mismatch");
    }
  }
}
function validateCoefficientVector(value, width, description) {
  if (!Array.isArray(value) || value.length !== width || value.some((coefficient) => !Number.isFinite(coefficient))) {
    throw new RangeError(`${description} is invalid`);
  }
}
function validateAuthorization(authorization) {
  requireExactKeys3(authorization, AUTHORIZATION_KEYS2, "temperature authorization");
  validateHash2(authorization.authorizationSha256, "authorizationSha256");
  validateText2(authorization.authorizedBy, "authorizedBy");
  validateText2(authorization.authorizationReason, "authorizationReason");
  const authorizedAt = Date.parse(
    validateUtcInstant3(authorization.authorizedAt, "authorizedAt")
  );
  const activatedAt = Date.parse(
    validateUtcInstant3(authorization.activatedAt, "activatedAt")
  );
  const expiresAt = Date.parse(validateUtcInstant3(authorization.expiresAt, "expiresAt"));
  if (authorization.authorized !== true || authorizedAt > activatedAt || expiresAt <= activatedAt || expiresAt - activatedAt > TEMPERATURE_CANARY_MAXIMUM_DURATION_MS) {
    throw new RangeError("temperature canary authorization window is invalid");
  }
  if (canonicalObjectSha256(
    authorization,
    "authorizationSha256"
  ) !== authorization.authorizationSha256) {
    throw new RangeError("temperature canary authorization SHA-256 mismatch");
  }
}
function validatePermanentAuthorization(authorization) {
  requireExactKeys3(
    authorization,
    PERMANENT_AUTHORIZATION_KEYS2,
    "permanent temperature authorization"
  );
  validateHash2(authorization.authorizationSha256, "authorizationSha256");
  validateText2(authorization.authorizedBy, "authorizedBy");
  validateText2(authorization.authorizationReason, "authorizationReason");
  const authorizedAt = Date.parse(
    validateUtcInstant3(authorization.authorizedAt, "authorizedAt")
  );
  const activatedAt = Date.parse(
    validateUtcInstant3(authorization.activatedAt, "activatedAt")
  );
  if (authorization.artifactKind !== "ecmwf_temperature_transfer_canary_authorization" || authorization.contractVersion !== TEMPERATURE_CANARY_AUTHORIZATION_CONTRACT_VERSION_V2 || authorization.authorized !== true || authorization.permanent !== true || authorization.expiresAt !== null || authorizedAt > activatedAt) {
    throw new RangeError("permanent temperature authorization is invalid");
  }
  if (canonicalObjectSha256(
    authorization,
    "authorizationSha256"
  ) !== authorization.authorizationSha256) {
    throw new RangeError("temperature canary authorization SHA-256 mismatch");
  }
}
function requireExactKeys3(value, expected, description) {
  const actual = Object.keys(value).sort();
  if (JSON.stringify(actual) !== JSON.stringify([...expected].sort())) {
    throw new RangeError(`${description} has unexpected fields`);
  }
}
function validateHash2(value, description) {
  if (!HASH_PATTERN2.test(value)) {
    throw new RangeError(`${description} must be a SHA-256 hex value`);
  }
  return value;
}
function validateText2(value, description) {
  if (typeof value !== "string" || value.trim() !== value || value.length < 1 || value.length > 256 || /[\r\n]|:\/\/|\b(?:credential|password|private[-_ ]?key|secret|token)\b/iu.test(
    value
  )) {
    throw new RangeError(`${description} must be bounded nonempty text`);
  }
  return value;
}
function validateUtcInstant3(value, description) {
  const milliseconds = Date.parse(value);
  if (!Number.isFinite(milliseconds) || new Date(milliseconds).toISOString() !== value) {
    throw new RangeError(`${description} must be a canonical UTC instant`);
  }
  return value;
}

// packages/forecast-adjustment/src/maintenance-shadow-comparator.ts
var MAINTENANCE_SHADOW_COMPARATOR_VERSION = "adjustment-shadow-incumbent-comparator/v1";
var MAINTENANCE_SHADOW_COMPARATOR_LIMIT_BYTES = 512 * 1024;
var HASH5 = /^[a-f0-9]{64}$/u;
var BINARY642 = /^[a-f0-9]{16}$/u;
var UTC_MILLISECOND2 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;
var TOP_KEYS2 = [
  "contractVersion",
  "family",
  "registrationSha256",
  "candidateSha256",
  "sourceSha256",
  "dueKey",
  "issuedAt",
  "sourceProjectionSha256",
  "predictionBodySha256",
  "servingAuthority",
  "rowCount",
  "rows"
];
var AUTHORITY_KEYS = [
  "artifactBase64",
  "artifactIdentitySha256",
  "artifactMemberSha256",
  "authorityKind",
  "receiptBase64",
  "receiptMemberSha256"
];
var MAINTENANCE_SHADOW_COMPARATOR_ROW_KEYS = Object.freeze({
  rain: [
    "validAt",
    "leadHours",
    "sourceRowSha256",
    "incumbentPrecipitationMm64",
    "occurrenceProbability64",
    "atLeast1_0Probability64",
    "atLeast2_5Probability64",
    "positiveAmountMm64",
    "applied",
    "reasonCode"
  ],
  temperature: ["validAt", "leadHours", "sourceRowSha256", "incumbentTemperatureC64", "applied", "reasonCode"],
  wind: [
    "validAt",
    "leadHours",
    "sourceRowSha256",
    "incumbentSpeedMps64",
    "incumbentGustMps64",
    "speedApplied",
    "gustApplied",
    "reasonCode"
  ]
});
var QUALIFIED_RECEIPT_KEYS = [
  "actionKind",
  "actionSha256",
  "bundleSha256",
  "candidateGraphSha256",
  "candidateSha256",
  "contractVersion",
  "controlPlaneSha256",
  "deployedCommit",
  "deployedImageDigest",
  "deployedRelease",
  "deployedSettingsSha256",
  "fencingToken",
  "fullMemberRootSha256",
  "installedAt",
  "lifecycleHeadSha256",
  "policyDecision",
  "policyReportSha256",
  "registrationSha256",
  "sourceSha256"
];
var TEMP_RAW_REGISTRY_KEYS = ["activeBundle", "contractVersion", "rawReason", "siteKey"];
var WIND_RAW_REGISTRY_KEYS = ["activeBundle", "contractVersion", "enabledMetricBands", "rawReason", "siteKey"];
var MAINTENANCE_RAW_REGISTRY_KEYS = ["activePackage", "contractVersion", "rawReason", "siteKey"];
var RAIN_RAW_REGISTRY_KEYS = ["activeArtifact", "contractVersion", "rawReason", "siteKey"];
var RAIN_ACTIVE_REGISTRY_KEYS = ["activeArtifact", "contractVersion", "rawReason", "siteKey"];
var RAIN_ACTIVE_ARTIFACT_KEYS = ["artifactSha256"];
var TEMP_REASON_CODES = /* @__PURE__ */ new Set([
  null,
  "bundle_invalid",
  "bundle_missing",
  "canary_expired",
  "canary_killed",
  "registry_inactive",
  "registry_invalid",
  "policy_raw",
  "unsupported",
  "missing_source_forecast",
  "outside_operational_window",
  "source_identity_mismatch",
  "source_not_available",
  "source_stale",
  "source_time_mismatch"
]);
var WIND_REASON_CODES = /* @__PURE__ */ new Set([
  null,
  "adjustment_error",
  "bundle_invalid",
  "bundle_missing",
  "canary_expired",
  "canary_killed",
  "coefficient_missing",
  "direction_calm",
  "identity_mismatch",
  "metric_not_enabled",
  "metric_out_of_bounds",
  "policy_raw",
  "registry_inactive",
  "registry_invalid",
  "runtime_fingerprint_mismatch",
  "unsupported_lead",
  "wrong_cohort"
]);
var RAIN_REASON_CODES = /* @__PURE__ */ new Set([
  null,
  "phase_unsupported",
  "policy_raw",
  "prediction_invalid"
]);
function parseMaintenanceShadowComparator(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength > MAINTENANCE_SHADOW_COMPARATOR_LIMIT_BYTES) {
    throw new RangeError("invalid shadow comparator size");
  }
  const value = JSON.parse(new TextDecoder4("utf-8", { fatal: true }).decode(bytes));
  validateMaintenanceShadowComparator(value);
  const canonical = encodeComparatorUnchecked(value);
  if (!canonical.equals(Buffer.from(bytes))) {
    throw new RangeError("shadow comparator is not canonical");
  }
  return value;
}
function validateMaintenanceShadowComparatorBinding(comparator, sourceProjectionBytes, predictionBodyBytes) {
  const source = parseMaintenanceShadowSourceProjection(sourceProjectionBytes);
  const body = parseMaintenanceShadowValues(predictionBodyBytes);
  const metadata = createMaintenanceShadowPredictionMetadata(predictionBodyBytes, sourceProjectionBytes);
  const sourceIdentity = createMaintenanceShadowSourceIdentity(sourceProjectionBytes);
  if (comparator.family !== source.family || comparator.family !== body.family || comparator.registrationSha256 !== source.registrationSha256 || comparator.registrationSha256 !== body.registrationSha256 || comparator.candidateSha256 !== source.candidateSha256 || comparator.candidateSha256 !== body.candidateSha256 || comparator.sourceSha256 !== source.sourceSha256 || comparator.sourceSha256 !== body.sourceSha256 || comparator.dueKey !== source.dueKey || comparator.dueKey !== body.dueKey || comparator.issuedAt !== source.issuedAt || comparator.issuedAt !== body.issuedAt || comparator.sourceProjectionSha256 !== sha2562(Buffer.from(sourceProjectionBytes)) || comparator.predictionBodySha256 !== metadata.predictionBodySha256 || comparator.rowCount !== source.rowCount || comparator.rowCount !== body.rowCount) {
    throw new RangeError("shadow comparator capsule binding differs");
  }
  for (let index = 0; index < comparator.rows.length; index += 1) {
    const row = comparator.rows[index];
    const sourceRow = source.rows[index];
    const bodyRow = body.rows[index];
    if (row.validAt !== sourceRow.validAt || row.validAt !== bodyRow.validAt || row.leadHours !== index + 1 || bodyRow.leadHours !== index + 1 || row.sourceRowSha256 !== sourceIdentity.sourceRowSha256[index] || bodyRow.sourceRowSha256 !== row.sourceRowSha256) {
      throw new RangeError("shadow comparator row binding differs");
    }
  }
}
function validateMaintenanceShadowComparator(value) {
  exactKeys5(value, TOP_KEYS2, "shadow comparator");
  if (value.contractVersion !== MAINTENANCE_SHADOW_COMPARATOR_VERSION || !["temperature", "wind", "rain"].includes(value.family) || !HASH5.test(value.registrationSha256) || !HASH5.test(value.candidateSha256) || !HASH5.test(value.sourceSha256) || !HASH5.test(value.sourceProjectionSha256) || !HASH5.test(value.predictionBodySha256) || !UTC_MILLISECOND2.test(value.issuedAt) || !Number.isFinite(Date.parse(value.issuedAt)) || typeof value.dueKey !== "string" || value.dueKey.length < 1 || value.dueKey.length > 128 || !Number.isSafeInteger(value.rowCount) || value.rowCount < 1 || !Array.isArray(value.rows) || value.rows.length !== value.rowCount) {
    throw new RangeError("shadow comparator identity differs");
  }
  validateServingAuthority(value.servingAuthority, value.family);
  for (const row of value.rows) {
    validateComparatorRow(row, value.family);
  }
}
function encodeComparatorUnchecked(value) {
  const ordered = {};
  for (const key of TOP_KEYS2) {
    if (key === "servingAuthority") {
      ordered[key] = Object.fromEntries(AUTHORITY_KEYS.map((field) => [field, value.servingAuthority[field]]));
    } else if (key === "rows") {
      ordered[key] = value.rows.map(
        // preserve the family row ordering
        (row) => Object.fromEntries(MAINTENANCE_SHADOW_COMPARATOR_ROW_KEYS[value.family].map((field) => [field, row[field]]))
      );
    } else {
      ordered[key] = value[key];
    }
  }
  return Buffer.from(JSON.stringify(ordered) + "\n");
}
function validateServingAuthority(authority, family) {
  exactKeys5(authority, AUTHORITY_KEYS, "shadow comparator authority");
  if (!HASH5.test(authority.receiptMemberSha256) || !["legacy_active", "maintenance_qualified", "policy_raw"].includes(authority.authorityKind)) {
    throw new RangeError("shadow comparator authority differs");
  }
  const receiptBytes = strictBase64(authority.receiptBase64, "serving receipt");
  if (sha2562(receiptBytes) !== authority.receiptMemberSha256) {
    throw new RangeError("shadow comparator receipt hash differs");
  }
  const receipt = parseCanonicalJson(receiptBytes, "serving receipt");
  if (authority.authorityKind === "policy_raw") {
    if (authority.artifactBase64 !== null || authority.artifactIdentitySha256 !== null || authority.artifactMemberSha256 !== null) {
      throw new RangeError("raw comparator contains an artifact");
    }
    validateRawReceipt(receipt, family);
    return;
  }
  if (typeof authority.artifactBase64 !== "string" || !HASH5.test(String(authority.artifactIdentitySha256)) || !HASH5.test(String(authority.artifactMemberSha256))) {
    throw new RangeError("active comparator artifact differs");
  }
  const artifactBytes = strictBase64(authority.artifactBase64, "serving artifact");
  if (sha2562(artifactBytes) !== authority.artifactMemberSha256) {
    throw new RangeError("shadow comparator artifact hash differs");
  }
  const artifact = parseJson(artifactBytes, "serving artifact");
  if (authority.authorityKind === "maintenance_qualified") {
    validateQualifiedAuthority(artifact, artifactBytes, receipt, authority.artifactIdentitySha256, family);
  } else {
    validateLegacyAuthority(artifact, artifactBytes, receipt, authority.artifactIdentitySha256, family);
  }
}
function validateQualifiedAuthority(artifact, artifactBytes, receipt, artifactIdentitySha256, family) {
  exactKeys5(receipt, QUALIFIED_RECEIPT_KEYS, "qualified serving receipt");
  if (receipt.contractVersion !== "adjustment-installed-candidate-receipt/v2" || receipt.actionKind !== "promote" || receipt.policyDecision !== "qualified" || !HASH5.test(String(receipt.fullMemberRootSha256)) || receipt.bundleSha256 !== artifactIdentitySha256 || !HASH5.test(String(receipt.actionSha256)) || !HASH5.test(String(receipt.registrationSha256)) || !HASH5.test(String(receipt.candidateGraphSha256)) || !HASH5.test(String(receipt.candidateSha256)) || !HASH5.test(String(receipt.sourceSha256)) || !HASH5.test(String(receipt.policyReportSha256)) || !HASH5.test(String(receipt.lifecycleHeadSha256))) {
    throw new RangeError("qualified serving receipt differs");
  }
  if (family === "rain") {
    validateRainHurdleWindPortableArtifact(artifactBytes.toString("utf8"), artifactIdentitySha256);
    return;
  }
  verifyForecastAdjustmentMaintenanceRuntimePackage(artifact);
  if (artifact.family !== family || artifact.bundleSha256 !== artifactIdentitySha256) {
    throw new RangeError("qualified serving package differs");
  }
}
function validateLegacyAuthority(artifact, artifactBytes, receipt, artifactIdentitySha256, family) {
  if (family === "temperature") {
    verifyForecastAdjustmentTemperatureCanaryRuntimeBundle(artifact);
    if (artifact.bundleSha256 !== artifactIdentitySha256 || canonicalJsonBytes(artifact.authorization) !== receiptBytesText(receipt)) {
      throw new RangeError("legacy temperature serving authority differs");
    }
    return;
  }
  if (family === "wind") {
    if (artifact.contractVersion === "forecast-adjustment-runtime-bundle/v2") {
      verifyForecastAdjustmentRuntimeBundle(artifact);
      if (artifact.bundleSha256 !== artifactIdentitySha256 || canonicalJsonBytes(artifact.qualificationReceipt) !== receiptBytesText(receipt)) {
        throw new RangeError("legacy wind serving authority differs");
      }
      return;
    }
    verifyForecastAdjustmentWindCanaryRuntimeBundle(artifact);
    if (artifact.bundleSha256 !== artifactIdentitySha256 || canonicalJsonBytes(artifact.authorization) !== receiptBytesText(receipt)) {
      throw new RangeError("legacy wind serving authority differs");
    }
    return;
  }
  if (artifactIdentitySha256 !== RAIN_HURDLE_WIND_ARTIFACT_SHA256 || sha2562(artifactBytes) !== RAIN_HURDLE_WIND_ARTIFACT_SHA256) {
    throw new RangeError("legacy rain artifact differs");
  }
  exactKeys5(receipt, RAIN_ACTIVE_REGISTRY_KEYS, "legacy rain registry");
  const activeArtifact = receipt.activeArtifact;
  exactKeys5(activeArtifact, RAIN_ACTIVE_ARTIFACT_KEYS, "legacy rain active artifact");
  if (receipt.contractVersion !== "forecast-adjustment-rain-runtime-registry/v1" || receipt.rawReason !== null || activeArtifact.artifactSha256 !== artifactIdentitySha256) {
    throw new RangeError("legacy rain registry differs");
  }
}
function validateRawReceipt(receipt, family) {
  const contractVersion = receipt.contractVersion;
  if (family === "temperature" && contractVersion === "forecast-adjustment-temperature-canary-registry/v2") {
    exactKeys5(receipt, TEMP_RAW_REGISTRY_KEYS, "temperature raw registry");
    if (receipt.activeBundle !== null || receipt.rawReason !== "policy_raw" || receipt.siteKey !== "ballydidean") {
      throw new RangeError("temperature raw registry differs");
    }
    return;
  }
  if (family === "wind" && contractVersion === "forecast-adjustment-wind-canary-registry/v2") {
    exactKeys5(receipt, WIND_RAW_REGISTRY_KEYS, "wind raw registry");
    if (receipt.activeBundle !== null || receipt.rawReason !== "policy_raw" || receipt.siteKey !== "ballydidean" || !Array.isArray(receipt.enabledMetricBands) || receipt.enabledMetricBands.length !== 13) {
      throw new RangeError("wind raw registry differs");
    }
    return;
  }
  if (contractVersion === `forecast-adjustment-${family}-maintenance-registry/v1`) {
    exactKeys5(receipt, MAINTENANCE_RAW_REGISTRY_KEYS, "maintenance raw registry");
    if (receipt.activePackage !== null || receipt.rawReason !== "policy_raw" || receipt.siteKey !== "ballydidean") {
      throw new RangeError("maintenance raw registry differs");
    }
    return;
  }
  if (family === "rain" && contractVersion === "forecast-adjustment-rain-runtime-registry/v1") {
    exactKeys5(receipt, RAIN_RAW_REGISTRY_KEYS, "rain raw registry");
    if (receipt.activeArtifact !== null || receipt.rawReason !== "policy_raw" || receipt.siteKey !== "ballydidean") {
      throw new RangeError("rain raw registry differs");
    }
    return;
  }
  throw new RangeError("raw serving receipt differs");
}
function validateComparatorRow(row, family) {
  exactKeys5(row, MAINTENANCE_SHADOW_COMPARATOR_ROW_KEYS[family], "shadow comparator row");
  if (!UTC_MILLISECOND2.test(String(row.validAt)) || !Number.isFinite(Date.parse(String(row.validAt))) || !Number.isSafeInteger(row.leadHours) || Number(row.leadHours) < 1 || !HASH5.test(String(row.sourceRowSha256))) {
    throw new RangeError("shadow comparator row geometry differs");
  }
  if (family === "temperature") {
    requireBinary642(row.incumbentTemperatureC64);
    if (typeof row.applied !== "boolean" || !TEMP_REASON_CODES.has(row.reasonCode)) {
      throw new RangeError("temperature comparator decision differs");
    }
    return;
  }
  if (family === "wind") {
    requireBinary642(row.incumbentSpeedMps64);
    if (row.incumbentGustMps64 !== null) requireBinary642(row.incumbentGustMps64);
    if (typeof row.speedApplied !== "boolean" || typeof row.gustApplied !== "boolean" || !WIND_REASON_CODES.has(row.reasonCode)) {
      throw new RangeError("wind comparator decision differs");
    }
    return;
  }
  requireBinary642(row.incumbentPrecipitationMm64);
  if (row.occurrenceProbability64 !== null) requireProbability64(row.occurrenceProbability64);
  if (row.atLeast1_0Probability64 !== null) requireProbability64(row.atLeast1_0Probability64);
  if (row.atLeast2_5Probability64 !== null) requireProbability64(row.atLeast2_5Probability64);
  if (row.positiveAmountMm64 !== null) requireBinary642(row.positiveAmountMm64);
  if (typeof row.applied !== "boolean" || !RAIN_REASON_CODES.has(row.reasonCode) || row.applied && [
    row.occurrenceProbability64,
    row.atLeast1_0Probability64,
    row.atLeast2_5Probability64,
    row.positiveAmountMm64
  ].some((item) => item === null)) {
    throw new RangeError("rain comparator decision differs");
  }
}
function requireBinary642(value) {
  if (typeof value !== "string" || !BINARY642.test(value)) {
    throw new RangeError("comparator binary64 differs");
  }
  decodeMaintenanceBinary64(value);
}
function requireProbability64(value) {
  requireBinary642(value);
  const decoded = decodeMaintenanceBinary64(value);
  if (decoded < 0 || decoded > 1) {
    throw new RangeError("comparator probability differs");
  }
}
function parseCanonicalJson(bytes, label) {
  const value = parseJson(bytes, label);
  if (canonicalJsonBytes(value) !== bytes.toString("utf8")) {
    throw new RangeError(`${label} is not canonical`);
  }
  return value;
}
function parseJson(bytes, label) {
  const value = JSON.parse(new TextDecoder4("utf-8", { fatal: true }).decode(bytes));
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new RangeError(`${label} differs`);
  }
  return value;
}
function receiptBytesText(receipt) {
  return canonicalJsonBytes(receipt);
}
function strictBase64(value, label) {
  if (typeof value !== "string" || value.length < 4 || value.length > 1e6 || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(value)) {
    throw new RangeError(`${label} base64 differs`);
  }
  const bytes = Buffer.from(value, "base64");
  if (bytes.toString("base64") !== value) {
    throw new RangeError(`${label} base64 differs`);
  }
  return bytes;
}
function exactKeys5(value, expected, label) {
  if (typeof value !== "object" || value === null || Array.isArray(value) || Object.keys(value).join(",") !== expected.join(",")) {
    throw new RangeError(`${label} schema differs`);
  }
}
function sha2562(bytes) {
  return createHash5("sha256").update(bytes).digest("hex");
}

// packages/forecast-adjustment/src/runtime-loader.ts
var FORECAST_ADJUSTMENT_WIND_MAINTENANCE_METRIC_BANDS = deepFreeze([
  { leadBand: "001-024", metric: "windGustMps" },
  { leadBand: "025-048", metric: "windGustMps" },
  { leadBand: "073-096", metric: "windGustMps" },
  { leadBand: "097-120", metric: "windGustMps" },
  { leadBand: "121-144", metric: "windGustMps" },
  { leadBand: "145-168", metric: "windGustMps" },
  { leadBand: "001-024", metric: "windSpeedMps" },
  { leadBand: "025-048", metric: "windSpeedMps" },
  { leadBand: "049-072", metric: "windSpeedMps" },
  { leadBand: "073-096", metric: "windSpeedMps" },
  { leadBand: "097-120", metric: "windSpeedMps" },
  { leadBand: "121-144", metric: "windSpeedMps" },
  { leadBand: "145-168", metric: "windSpeedMps" }
]);

// packages/forecast-adjustment/src/performance-scorecard.ts
var RAIN_GAUGE_IDS = new Set(
  RAIN_COLLECTION_STATIONS.map((station) => station.locationId)
);
var RAIN_NEAREST_GAUGE_IDS = new Set(
  RAIN_COLLECTION_STATIONS.slice(0, 3).map((station) => station.locationId)
);

// packages/forecast-adjustment/src/maintenance-policy.ts
var MAINTENANCE_POLICY_REPORT_EXPIRY_MILLISECONDS = 7 * 24 * 60 * 60 * 1e3;
var BASE_ROW_KEYS = [
  "actualBestMatchPrediction",
  "applied",
  "candidatePrediction",
  "contractVersion",
  "evidenceClass",
  "family",
  "farmTarget",
  "firstEdgeCommittedAt",
  "horizonHours",
  "incumbentPrediction",
  "key",
  "localDate",
  "nativeSourcePrediction",
  "nearestThree",
  "pairKey",
  "provenanceComplete",
  "providerFamily",
  "rowSha256",
  "sourceReceiptAt",
  "sourceRowSha256",
  "stationKey",
  "target",
  "targetRowSha256",
  "validAt"
];
var RAIN_ROW_KEYS = [
  ...BASE_ROW_KEYS,
  "candidateProbability",
  "incumbentProbability",
  "nativeSourceProbability",
  "persistencePrediction",
  "rawTargetHourTemperatureC",
  "recentVolumeScalePrediction",
  "runKey",
  "sameWindowVolumeScalePrediction",
  "unchangedOrdinalPrediction",
  "volumeScalePrediction"
];
var WIND_MAINTENANCE_PAIR_KEYS = deepFreeze(
  FORECAST_ADJUSTMENT_WIND_MAINTENANCE_METRIC_BANDS.map(
    (pair) => `${pair.metric}:${pair.leadBand}`
  )
);
var RAIN_ORIGINAL_STABLE_GATE_IDS = deepFreeze([
  "maeImprovesFivePercent",
  "beatsVolumeScale",
  "beatsPersistence",
  "rmseNoWorse",
  "wetIntensityNoWorse",
  "heavyIntensityBounded",
  "annualVolumeBalanced",
  "overallSupport",
  "allSeasonsPresent",
  "allLeadBandsPresent",
  "rawHeavyAmountsUnchanged",
  "rawWetCallsPreserved",
  "event0.1Safety",
  "event1.0Safety",
  "event2.5Safety",
  "seasonDJFSupport",
  "seasonDJFVolume",
  "seasonDJFMae",
  "seasonDJFWetHeavy",
  "seasonDJFDetection",
  "seasonMAMSupport",
  "seasonMAMVolume",
  "seasonMAMMae",
  "seasonMAMWetHeavy",
  "seasonMAMDetection",
  "seasonJJASupport",
  "seasonJJAVolume",
  "seasonJJAMae",
  "seasonJJAWetHeavy",
  "seasonJJADetection",
  "seasonSONSupport",
  "seasonSONVolume",
  "seasonSONMae",
  "seasonSONWetHeavy",
  "seasonSONDetection",
  "lead1-6Mae",
  "lead1-6Support",
  "lead7-12Mae",
  "lead7-12Support",
  "lead13-23Mae",
  "lead13-23Support",
  "accumulation6Mae",
  "accumulation12Mae",
  "accumulation23Mae",
  "beatsSameWindowVolumeScale",
  "beatsRecentVolumeScale",
  "beatsUnchangedOrdinal",
  "seasonalBalanceImproves",
  "heavySkillRetained"
]);

// packages/forecast-adjustment/src/maintenance-capture-epoch.ts
var CAPTURE_EPOCH_IDENTITIES = Object.freeze([
  {
    controlPlaneVersion: "14",
    databaseMigrationHistorySha256: "c683c4f937c7f02b00f6ab49f75268eead81d2a221a8a9a23e38f9e4802a11b0"
  },
  {
    controlPlaneVersion: "14",
    databaseMigrationHistorySha256: "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424"
  },
  {
    controlPlaneSha256: "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
    controlPlaneVersion: "17",
    databaseMigrationHistorySha256: "107ce430f1ac9478d54d94646da7fa4a54cbe7112db13efd7244d8894d0defdf"
  },
  {
    controlPlaneSha256: "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
    controlPlaneVersion: "17",
    databaseMigrationHistorySha256: "3aec6846d009a2bb2cee02279dceb9bd2d3572dc1b3a83eed2ce9a1724311345"
  }
]);

// packages/forecast-adjustment/src/rain-fixed-gauge-target.ts
import { createHash as createHash6 } from "node:crypto";
import { TextDecoder as TextDecoder5 } from "node:util";
var ADJUSTMENT_RAIN_FIXED_GAUGE_TARGET_PROJECTION_VERSION = "adjustment-rain-fixed-gauge-target-projection/v1";
var ADJUSTMENT_RAIN_FIXED_GAUGE_TARGET_MAX_BYTES = 3500 * 1024;
var HASH6 = /^[a-f0-9]{64}$/u;
var INSTANT2 = /^20\d{2}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;
var TOP_KEYS3 = [
  "captureBodies",
  "contractVersion",
  "family",
  "logicalReceivedAt",
  "projectionKind",
  "rows",
  "validAt"
];
var BODY_KEYS = ["bodyBase64", "bodySha256", "claims"];
var CLAIM_KEYS = ["claimId", "completedAt", "stationId", "windowEndExclusive", "windowStart"];
var ROW_KEYS2 = ["captureMembers", "intervals", "normalizedRecord", "stationId", "storedContentSha256"];
var MEMBER_KEYS = ["bodySha256", "claimId", "completedAt"];
var INTERVAL_KEYS = [
  "bodySha256",
  "claimId",
  "completedAt",
  "durationMinutes",
  "precipitationMm64",
  "temperatureC64",
  "validAt"
];
var RECORD_KEYS = ["metadata", "metrics", "productRunAt", "receivedAt", "sourceId", "sourceKind", "validAt"];
var METRIC_KEYS = [
  "apparentTemperatureC",
  "blackGlobeTemperatureC",
  "cloudCoverPercent",
  "pm25MicrogramsPerCubicMeter",
  "precipitationMm",
  "precipitationRateMmPerHour",
  "pressureHpa",
  "relativeHumidityPercent",
  "soilElectricalConductivityMicrosiemensPerCm",
  "soilMoisturePercent",
  "solarRadiationWm2",
  "temperatureC",
  "uvIndex",
  "waterLevelM",
  "wetBulbGlobeTemperatureC",
  "windDirectionDegrees",
  "windGustMps",
  "windSpeedMps"
];
function encodeAdjustmentRainFixedGaugeTargetProjection(value) {
  validateAdjustmentRainFixedGaugeTargetProjection(value);
  const bytes = Buffer.from(canonicalJsonBytes(value));
  if (bytes.byteLength > ADJUSTMENT_RAIN_FIXED_GAUGE_TARGET_MAX_BYTES) {
    throw new RangeError("adjustment rain fixed-gauge target exceeds its canonical cap");
  }
  return bytes;
}
function parseAdjustmentRainFixedGaugeTargetProjection(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength < 2 || bytes.byteLength > ADJUSTMENT_RAIN_FIXED_GAUGE_TARGET_MAX_BYTES) {
    throw new RangeError("adjustment rain fixed-gauge target size is invalid");
  }
  const value = JSON.parse(new TextDecoder5("utf-8", { fatal: true }).decode(bytes));
  const canonical = encodeAdjustmentRainFixedGaugeTargetProjection(value);
  if (!canonical.equals(Buffer.from(bytes))) {
    throw new TypeError("adjustment rain fixed-gauge target is not canonical");
  }
  return value;
}
function validateAdjustmentRainFixedGaugeTargetProjection(value) {
  exactKeys6(value, TOP_KEYS3);
  requireInstant3(value.logicalReceivedAt, "logicalReceivedAt");
  requireInstant3(value.validAt, "validAt");
  if (value.contractVersion !== ADJUSTMENT_RAIN_FIXED_GAUGE_TARGET_PROJECTION_VERSION || value.family !== "rain" || value.projectionKind !== "target_revision" || !Array.isArray(value.captureBodies) || value.captureBodies.length < 1 || !Array.isArray(value.rows) || value.rows.length !== RAIN_COLLECTION_STATIONS.length) {
    throw new TypeError("adjustment rain fixed-gauge target identity differs");
  }
  const claims = validateCaptureBodies(value.captureBodies, value.validAt);
  const referencedClaims = /* @__PURE__ */ new Set();
  let latestCompletedAt = null;
  for (const [index, row] of value.rows.entries()) {
    exactKeys6(row, ROW_KEYS2);
    const station = RAIN_COLLECTION_STATIONS[index];
    if (station === void 0 || row.stationId !== station.locationId || !Array.isArray(row.captureMembers) || !Array.isArray(row.intervals)) {
      throw new TypeError("adjustment rain fixed-gauge station order differs");
    }
    const rowClaims = validateCaptureMembers(row.captureMembers, row.stationId, claims);
    for (const member of row.captureMembers) {
      const key = claimKey(member.bodySha256, member.claimId);
      referencedClaims.add(key);
      latestCompletedAt = latestCompletedAt === null || member.completedAt > latestCompletedAt ? member.completedAt : latestCompletedAt;
    }
    validateIntervals(row.intervals, rowClaims, value.validAt);
    validateNormalizedTargetRecord(row, station, value.validAt);
  }
  if (referencedClaims.size !== claims.size || [...claims.keys()].some((key) => !referencedClaims.has(key)) || latestCompletedAt !== value.logicalReceivedAt) {
    throw new TypeError("adjustment rain fixed-gauge capture graph differs");
  }
}
function validateCaptureBodies(bodies, validAt) {
  const claims = /* @__PURE__ */ new Map();
  let previousBodySha256 = null;
  for (const body of bodies) {
    exactKeys6(body, BODY_KEYS);
    requireHash3(body.bodySha256, "bodySha256");
    if (previousBodySha256 !== null && body.bodySha256 <= previousBodySha256) {
      throw new TypeError("adjustment rain fixed-gauge bodies are not ordered");
    }
    previousBodySha256 = body.bodySha256;
    if (typeof body.bodyBase64 !== "string" || body.bodyBase64.length < 4) {
      throw new TypeError("adjustment rain fixed-gauge body is invalid");
    }
    const raw = Buffer.from(body.bodyBase64, "base64");
    if (raw.toString("base64") !== body.bodyBase64 || sha2563(raw) !== body.bodySha256 || !Array.isArray(body.claims) || body.claims.length < 1) {
      throw new TypeError("adjustment rain fixed-gauge body identity differs");
    }
    let previousClaim = null;
    for (const claim of body.claims) {
      exactKeys6(claim, CLAIM_KEYS);
      requireClaimId(claim.claimId);
      requireInstant3(claim.completedAt, "completedAt");
      requireInstant3(claim.windowStart, "windowStart");
      requireInstant3(claim.windowEndExclusive, "windowEndExclusive");
      const order = `${claim.completedAt}
${claim.claimId}`;
      const station = RAIN_COLLECTION_STATIONS.find((entry) => entry.locationId === claim.stationId);
      if (station === void 0 || Date.parse(claim.windowStart) >= Date.parse(claim.windowEndExclusive) || Date.parse(claim.windowStart) > Date.parse(validAt) || Date.parse(claim.windowEndExclusive) <= Date.parse(validAt) - 65 * 6e4 || previousClaim !== null && order <= previousClaim) {
        throw new TypeError("adjustment rain fixed-gauge claim differs");
      }
      previousClaim = order;
      const key = claimKey(body.bodySha256, claim.claimId);
      if (claims.has(key)) {
        throw new TypeError("adjustment rain fixed-gauge claim is duplicated");
      }
      claims.set(key, claim);
    }
  }
  return claims;
}
function validateCaptureMembers(members, stationId, claims) {
  if (members.length < 1) {
    throw new TypeError("adjustment rain fixed-gauge row lacks capture evidence");
  }
  const selected = /* @__PURE__ */ new Map();
  let previous = null;
  for (const member of members) {
    exactKeys6(member, MEMBER_KEYS);
    requireHash3(member.bodySha256, "bodySha256");
    requireClaimId(member.claimId);
    requireInstant3(member.completedAt, "completedAt");
    const order = `${member.completedAt}
${member.claimId}
${member.bodySha256}`;
    const key = claimKey(member.bodySha256, member.claimId);
    const claim = claims.get(key);
    if (claim === void 0 || claim.stationId !== stationId || claim.completedAt !== member.completedAt || selected.has(key) || previous !== null && order <= previous) {
      throw new TypeError("adjustment rain fixed-gauge capture membership differs");
    }
    previous = order;
    selected.set(key, claim);
  }
  return selected;
}
function validateIntervals(intervals, claims, validAt) {
  let previousValidAt = null;
  for (const interval of intervals) {
    exactKeys6(interval, INTERVAL_KEYS);
    requireHash3(interval.bodySha256, "bodySha256");
    requireClaimId(interval.claimId);
    requireInstant3(interval.completedAt, "completedAt");
    requireInstant3(interval.validAt, "interval validAt");
    const claim = claims.get(claimKey(interval.bodySha256, interval.claimId));
    if (claim === void 0 || claim.completedAt !== interval.completedAt || !Number.isInteger(interval.durationMinutes) || interval.durationMinutes < 1 || interval.durationMinutes > 5 || previousValidAt !== null && interval.validAt <= previousValidAt || Date.parse(interval.validAt) > Date.parse(validAt) || Date.parse(interval.validAt) < Date.parse(validAt) - 65 * 6e4) {
      throw new TypeError("adjustment rain fixed-gauge interval differs");
    }
    previousValidAt = interval.validAt;
    const precipitation = decodeMaintenanceBinary64(interval.precipitationMm64);
    if (precipitation < 0) {
      throw new TypeError("adjustment rain fixed-gauge interval amount differs");
    }
    if (interval.temperatureC64 !== null) {
      decodeMaintenanceBinary64(interval.temperatureC64);
    }
  }
}
function validateNormalizedTargetRecord(row, station, validAt) {
  requireHash3(row.storedContentSha256, "storedContentSha256");
  exactKeys6(row.normalizedRecord, RECORD_KEYS);
  const record2 = createNormalizedWeatherRecord(row.normalizedRecord);
  if (record2.sourceKind !== "physical_sensor" || record2.productRunAt !== null || record2.validAt !== validAt || record2.receivedAt !== row.captureMembers.at(-1)?.completedAt || sha2563(weatherRecordContent(record2)) !== row.storedContentSha256) {
    throw new TypeError("adjustment rain fixed-gauge normalized record differs");
  }
  validateTargetMetadata(record2, station, row.intervals.length > 0);
  exactKeys6(record2.metrics, METRIC_KEYS);
  const expectedNull = METRIC_KEYS.filter((key) => ![
    "precipitationMm",
    "precipitationRateMmPerHour",
    "temperatureC"
  ].includes(key));
  if (expectedNull.some((key) => record2.metrics[key] !== null)) {
    throw new TypeError("adjustment rain fixed-gauge record has unrelated metrics");
  }
  const tiled = validateCompleteTiling(row.intervals, validAt);
  if (tiled !== null) {
    const amount = row.intervals.reduce(
      // sum only parser-validated finite binary64 values
      (sum, interval) => sum + decodeMaintenanceBinary64(interval.precipitationMm64),
      0
    );
    if (record2.metrics.precipitationMm !== amount || record2.metrics.precipitationRateMmPerHour !== amount) {
      throw new TypeError("adjustment rain fixed-gauge record amount differs");
    }
    return;
  }
  if (record2.metrics.precipitationMm !== null || record2.metrics.precipitationRateMmPerHour !== null || row.intervals.length !== 0) {
    throw new TypeError("adjustment rain fixed-gauge incomplete record differs");
  }
}
function validateTargetMetadata(record2, station, complete) {
  const expected = {
    device: { model: "Tempest", serial: station.serial, vendor: "WeatherFlow" },
    model: null,
    provider: {
      dataset: "rain_fixed_gauge_hourly",
      device_id: station.deviceId,
      location_id: station.locationId,
      report_interval_minutes: 60
    },
    quality: {
      sampling: "backward_exact_interval_tiling",
      status: complete ? "complete_interval_coverage" : "incomplete_interval_coverage"
    },
    upstreamTimezone: "America/Los_Angeles"
  };
  if (JSON.stringify(record2.metadata) !== JSON.stringify(expected)) {
    throw new TypeError("adjustment rain fixed-gauge metadata differs");
  }
}
function validateCompleteTiling(intervals, validAt) {
  if (intervals.length === 0) {
    return null;
  }
  let elapsedMinutes = 0;
  let previousEnd = null;
  for (const interval of intervals) {
    const end = Date.parse(interval.validAt);
    const start = end - interval.durationMinutes * 6e4;
    if (previousEnd !== null && start !== previousEnd) {
      throw new TypeError("adjustment rain fixed-gauge intervals do not tile");
    }
    previousEnd = end;
    elapsedMinutes += interval.durationMinutes;
  }
  const finalEnd = Date.parse(intervals.at(-1).validAt);
  const offsetMinutes = (Date.parse(validAt) - finalEnd) / 6e4;
  if (elapsedMinutes !== 60 || !Number.isInteger(offsetMinutes) || offsetMinutes < 0 || offsetMinutes > 5) {
    throw new TypeError("adjustment rain fixed-gauge hour geometry differs");
  }
  return true;
}
function claimKey(bodySha256, claimId) {
  return `${bodySha256}
${claimId}`;
}
function exactKeys6(value, keys) {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new TypeError("adjustment rain fixed-gauge object is invalid");
  }
  const actual = Object.keys(value);
  if (actual.length !== keys.length || actual.some((key, index) => key !== keys[index])) {
    throw new TypeError("adjustment rain fixed-gauge keys differ");
  }
}
function requireInstant3(value, name) {
  if (typeof value !== "string" || !INSTANT2.test(value) || new Date(value).toISOString() !== value) {
    throw new TypeError(`adjustment rain fixed-gauge ${name} is invalid`);
  }
}
function requireClaimId(value) {
  if (typeof value !== "string" || value.length < 1 || value.length > 128) {
    throw new TypeError("adjustment rain fixed-gauge claim identity is invalid");
  }
}
function requireHash3(value, name) {
  if (typeof value !== "string" || !HASH6.test(value)) {
    throw new TypeError(`adjustment rain fixed-gauge ${name} is invalid`);
  }
}
function sha2563(value) {
  return createHash6("sha256").update(value).digest("hex");
}
export {
  createMaintenanceShadowPredictionMetadata,
  parseAdjustmentRainFixedGaugeTargetProjection,
  parseAdjustmentRevisionProjectionDocument,
  parseMaintenanceShadowComparator,
  parseMaintenanceShadowSourceProjection,
  parseMaintenanceShadowValues,
  parseRainMaintenanceControlState,
  validateMaintenanceShadowComparatorBinding
};
