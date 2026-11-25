const airQualityHourlyContext: ApiAirQualityHourlyContext = {
  hourlyPm25: [null, 3.5, 8],
  through: "2026-11-21T00:00:00.000Z",
};

export const propertyWithoutAirQuality: ApiPropertySensorSnapshot = {
  channel: null,
  key: "outside",
  model: "fixture",
  readings: { temperatureC: 12.5 },
};

export const propertyWithNullAirQuality: ApiPropertySensorSnapshot = {
  airQuality: null,
  channel: 1,
  key: "garage",
  model: "fixture",
  readings: { relativeHumidityPercent: 80 },
};

export const propertyWithAirQuality: ApiPropertySensorSnapshot = {
  airQuality: airQualityHourlyContext,
  channel: 2,
  key: "workshop",
  model: "fixture",
  readings: { pm25MicrogramsPerCubicMeter: 3.5 },
};

export const forecastDays: readonly ForecastDays[] = [1, 5, 10];

export const bootstrapSummary: ForecastAdjustmentBootstrapSummary = {
  contractVersion: "moving-block-bootstrap/v1",
  lowerSkill: -0.1,
  replicates: 2_000,
  upperSkill: 0.2,
};

const reliabilityBin: ForecastAdjustmentReliabilityBin = {
  count: 4,
  meanProbability: null,
  observedFrequency: 0.5,
};

export const rainThresholds: readonly ForecastAdjustmentRainThresholdSummary[] = [
  {
    adjustedBrier: 0.1,
    csi: null,
    falseAlarms: 0,
    far: null,
    hits: 1,
    misses: 0,
    pod: null,
    rawBrier: null,
    reliability: [reliabilityBin],
    thresholdMmPerHour: 0.1,
  },
  {
    adjustedBrier: 0.2,
    csi: 0.5,
    falseAlarms: 1,
    far: 0.25,
    hits: 2,
    misses: 1,
    pod: 0.75,
    rawBrier: null,
    reliability: [reliabilityBin],
    thresholdMmPerHour: 1,
  },
  {
    adjustedBrier: 0.3,
    csi: 0.25,
    falseAlarms: 2,
    far: 0.5,
    hits: 3,
    misses: 2,
    pod: 0.6,
    rawBrier: null,
    reliability: [reliabilityBin],
    thresholdMmPerHour: 2.5,
  },
];

export const controlReference: InstalledRainMaintenanceControlReference = {
  controlStateBytes: new Uint8Array([1, 2]),
  controlStateSha256: "control",
  ordinalArtifactBytes: new Uint8Array([3, 4]),
  ordinalArtifactSha256: "ordinal",
};

export const dailyPrecipitationRows: readonly DailyPrecipitationRow[] = [
  {
    accumulationMm: 1.25,
    sourceId: "date-valid",
    stationSlug: "station",
    validThrough: new Date("2026-11-21T00:00:00.000Z"),
  },
  {
    accumulationMm: 2.5,
    sourceId: "date-invalid",
    stationSlug: "station",
    validThrough: new Date("not-a-date"),
  },
  {
    accumulationMm: 3.75,
    sourceId: "raw-string",
    stationSlug: "station",
    validThrough: "not-normalized",
  },
];

export const jsonPrimitives: readonly JsonPrimitive[] = [
  null,
  false,
  4.5,
  "text",
];

export const recursiveJsonValue: JsonValue = {
  loneSurrogate: "\uD800",
  nested: [
    null,
    true,
    7,
    "value",
    { deeper: ["still-json", null] },
  ],
};

const installedReceiptV1: InstalledMaintenanceShadowReceiptV1 = {
  actionSha256: "action-v1",
  bundleSha256: "bundle-v1",
  candidateGraphSha256: "graph-v1",
  candidateSha256: "candidate-v1",
  contractVersion: "adjustment-installed-candidate-receipt/v1",
  controlPlaneSha256: "control-v1",
  deployedCommit: "commit-v1",
  deployedImageDigest: "image-v1",
  deployedRelease: "release-v1",
  deployedSettingsSha256: "settings-v1",
  fencingToken: "fence-v1",
  installedAt: "raw-installed-at-v1",
  registrationSha256: "registration-v1",
  sourceSha256: "source-v1",
};

const installedReceiptV2Null: InstalledMaintenanceShadowReceiptV2 = {
  actionKind: "shadow",
  actionSha256: "action-v2-null",
  bundleSha256: "bundle-v2-null",
  candidateGraphSha256: "graph-v2-null",
  candidateSha256: "candidate-v2-null",
  contractVersion: "adjustment-installed-candidate-receipt/v2",
  controlPlaneSha256: "control-v2-null",
  deployedCommit: "commit-v2-null",
  deployedImageDigest: "image-v2-null",
  deployedRelease: "release-v2-null",
  deployedSettingsSha256: "settings-v2-null",
  fencingToken: "fence-v2-null",
  fullMemberRootSha256: null,
  installedAt: "raw-installed-at-v2-null",
  lifecycleHeadSha256: "lifecycle-v2-null",
  policyDecision: "pending",
  policyReportSha256: "policy-v2-null",
  registrationSha256: "registration-v2-null",
  sourceSha256: "source-v2-null",
};

const installedReceiptV2Present: InstalledMaintenanceShadowReceiptV2 = {
  actionKind: "promote",
  actionSha256: "action-v2-present",
  bundleSha256: "bundle-v2-present",
  candidateGraphSha256: "graph-v2-present",
  candidateSha256: "candidate-v2-present",
  contractVersion: "adjustment-installed-candidate-receipt/v2",
  controlPlaneSha256: "control-v2-present",
  deployedCommit: "commit-v2-present",
  deployedImageDigest: "image-v2-present",
  deployedRelease: "release-v2-present",
  deployedSettingsSha256: "settings-v2-present",
  fencingToken: "fence-v2-present",
  fullMemberRootSha256: "member-root-v2-present",
  installedAt: "raw-installed-at-v2-present",
  lifecycleHeadSha256: "lifecycle-v2-present",
  policyDecision: "qualified",
  policyReportSha256: "policy-v2-present",
  registrationSha256: "registration-v2-present",
  sourceSha256: "source-v2-present",
};

export const installedReceipts: readonly InstalledMaintenanceShadowReceipt[] = [
  installedReceiptV1,
  installedReceiptV2Null,
  installedReceiptV2Present,
];
