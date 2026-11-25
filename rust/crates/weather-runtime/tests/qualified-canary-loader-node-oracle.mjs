import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";

import {
  FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1,
  FORECAST_ADJUSTMENT_CANONICAL_TRAINING_PROVENANCE_V1,
  buildForecastAdjustmentMaintenanceRuntimePackage,
  canonicalJsonBytes,
  canonicalSha256,
  createDevelopmentReport,
  createForecastAdjustmentCandidate,
  evaluateDevelopmentLosoFold,
  runtimeCalendarFingerprint,
  verifyForecastAdjustmentMaintenanceRuntimePackage,
} from "../../../../packages/forecast-adjustment/dist/index.js";

const INSTALLED = "2026-10-08T00:00:00.000Z";
const INTERVAL_START = "2026-10-01T00:00:00.000Z";
const INTERVAL_END = "2026-11-01T00:00:00.000Z";
const TARGET = "2026-10-09T00:00:00.000Z";
const VALID_THROUGH = "2026-10-10T00:00:00.000Z";
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const leadBands = ["001-024", "025-048", "049-072", "073-096", "097-120", "121-144", "145-168"];

// create all thirteen supported wind serving pairs
function windPairs() {
  return [
    ...leadBands.map((leadBand) => ({ leadBand, metric: "windSpeedMps" })),
    ...leadBands.filter((leadBand) => leadBand !== "049-072")
      .map((leadBand) => ({ leadBand, metric: "windGustMps" })),
  ];
}

// create the independently addressed development evidence required by the package builder
function windDevelopmentReport() {
  const stationScores = [
    ["ambient-maxweather", "ambient"],
    ["ballydidean-ecowitt", "ecowitt"],
    ["netatmo-nearby", "netatmo"],
    ["tempest-126537", "tempest"],
    ["tempest-168853", "tempest"],
  ].map(([physicalStationKey, providerFamily]) => ({
    adjustedLoss: 9,
    eventCount: 100,
    physicalStationKey,
    pointSkill: 0.1,
    providerFamily,
    rawLoss: 10,
    remainingNetworkScoreEvents: 100,
    scoreMatches: 100,
    trainingMatches: 500,
  }));
  const pairs = windPairs();
  return createDevelopmentReport({
    enabledMetricBands: pairs,
    folds: pairs.flatMap((metricBand) => [1, 2, 3, 4, 5].map((fold) =>
      evaluateDevelopmentLosoFold({
        auxiliaryModelSha256s: ["1", "2", "3", "4", "5"].map((value) => value.repeat(64)),
        bootstrapLowerBound: 0.01,
        fold,
        materialHarmSliceKeys: [],
        metricBand,
        stationScores,
      }))),
  });
}

// build one source-valid selected wind fit through the original package entry point
function buildWindPackage() {
  const developmentReport = windDevelopmentReport();
  const pairs = windPairs();
  const candidate = createForecastAdjustmentCandidate({
    coefficients: pairs.map((pair) => ({
      coefficient: 0.25,
      daypart: null,
      effectiveEventCount: 200,
      leadBand: pair.leadBand,
      level: 1,
      metric: pair.metric,
      month: null,
      season: null,
    })),
    developmentReportSha256: developmentReport.developmentReportSha256,
    enabledMetricBands: pairs,
    evaluationEpochId: "maintenance-2026-10",
    exportManifestSha256: canonicalSha256({ contractVersion: "fixture/v1" }),
    finalTrainingCutoff: "2026-09-01T06:59:59.999Z",
    forecastIdentity: FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1,
    runtimeFingerprint: runtimeCalendarFingerprint(),
    trainingEnvelopes: pairs.map((pair) => ({
      leadBand: pair.leadBand,
      maximum: 40,
      metric: pair.metric,
      minimum: 0,
    })),
    trainingProvenance: FORECAST_ADJUSTMENT_CANONICAL_TRAINING_PROVENANCE_V1,
  });
  const fit = {
    candidate,
    confirmationOpened: false,
    contractVersion: "wind-maintenance-fit/v2",
    developmentReport,
    dueMonth: "2026-10",
    state: "development_candidate",
  };
  return buildForecastAdjustmentMaintenanceRuntimePackage(Buffer.from(canonicalJsonBytes(fit)));
}

// build one source-valid selected temperature fit from the checked-in permanent model
async function buildTemperaturePackage() {
  const source = new URL(
    "../../../../config/forecast-adjustments/ballydidean/temperature-canary-bundles/" +
      "sha256-4d4e229b42823e53d2db062ec18c625bb2d2378a8a46d641fa95fabb59501b0e.json",
    import.meta.url,
  );
  const incumbent = JSON.parse(await readFile(source, "utf8"));
  const fit = {
    arms: {},
    confirmationOpened: false,
    contractVersion: "temperature-maintenance-fit/v2",
    developmentIncumbentMae: 1,
    developmentRawMae: 2,
    dueMonth: "2026-10",
    model: incumbent.model,
    selectedArm: "month_start_expanding",
    servingChanged: false,
    state: "development_candidate",
  };
  return buildForecastAdjustmentMaintenanceRuntimePackage(Buffer.from(canonicalJsonBytes(fit)));
}

// derive one stable fixture identity without pretending it came from deployment state
function fixtureHash(family, member) {
  return canonicalSha256({ contractVersion: "qualified-canary-loader-fixture/v1", family, member });
}

// build the exact registration preimage consumed by the native catalog verifier
function registrationPreimage(family, packageValue, identities) {
  return [
    "adjustment-shadow-registration/v2",
    "ballydidean",
    family,
    packageValue.candidateSha256,
    packageValue.bundleSha256,
    identities.policy,
    identities.cohort,
    identities.reserved,
    canonicalSha256(packageValue.source),
    INTERVAL_START,
    INTERVAL_END,
    TARGET,
    TARGET,
  ].join("\n");
}

// compose the public members and catalog receipt around one real generated package
function qualification(family, packageValue) {
  verifyForecastAdjustmentMaintenanceRuntimePackage(packageValue);
  const identities = Object.fromEntries(
    ["candidateGraph", "cohort", "controlPlane", "fullMemberRoot", "installedReceipt", "lifecycleHead",
      "policy", "policyReport", "predecessorAction", "reserved", "settings"]
      .map((member) => [member, fixtureHash(family, member)]),
  );
  const registrationPreimageBytes = Buffer.from(registrationPreimage(family, packageValue, identities));
  const registrationSha256 = hash(registrationPreimageBytes);
  const registration = {
    artifactSha256: packageValue.bundleSha256,
    candidateSha256: packageValue.candidateSha256,
    cohortSha256: identities.cohort,
    family,
    intervalEndAt: INTERVAL_END,
    intervalStartAt: INTERVAL_START,
    policySha256: identities.policy,
    registrationSha256,
    reservedKeySha256: identities.reserved,
    siteKey: "ballydidean",
    sourceSha256: canonicalSha256(packageValue.source),
    targetCutoffAt: TARGET,
    terminalAt: TARGET,
  };
  const action = {
    actionKind: "promote",
    candidateGraphSha256: identities.candidateGraph,
    candidateSha256: packageValue.candidateSha256,
    contractVersion: "forecast-adjustment-model-action/v1",
    createdAt: INSTALLED,
    expectedInstalledReceiptSha256: identities.installedReceipt,
    expectedSettingsSha256: identities.settings,
    expectedSourceCommit: "abcdefabcdefabcdefabcdefabcdefabcdefabcd",
    expectedSourceRelease: "2026.10.08-1",
    family,
    fencingToken: "1",
    fullMemberRootSha256: identities.fullMemberRoot,
    lifecycleHeadSha256: identities.lifecycleHead,
    policyDecision: "qualified",
    policyReportSha256: identities.policyReport,
    predecessorActionSha256: identities.predecessorAction,
    reason: "qualified_candidate",
    reportCreatedAt: INSTALLED,
    siteKey: "ballydidean",
    validThrough: VALID_THROUGH,
  };
  const actionSha256 = canonicalSha256(action);
  const receipt = {
    actionKind: "promote",
    actionSha256,
    bundleSha256: packageValue.bundleSha256,
    candidateGraphSha256: identities.candidateGraph,
    candidateSha256: packageValue.candidateSha256,
    contractVersion: "adjustment-installed-candidate-receipt/v2",
    controlPlaneSha256: identities.controlPlane,
    deployedCommit: "abcdefabcdefabcdefabcdefabcdefabcdefabcd",
    deployedImageDigest: `sha256:${fixtureHash(family, "image")}`,
    deployedRelease: "2026.10.08-1",
    deployedSettingsSha256: identities.settings,
    fencingToken: "1",
    fullMemberRootSha256: identities.fullMemberRoot,
    installedAt: INSTALLED,
    lifecycleHeadSha256: identities.lifecycleHead,
    policyDecision: "qualified",
    policyReportSha256: identities.policyReport,
    registrationSha256,
    sourceSha256: canonicalSha256(packageValue.source),
  };
  const registry = {
    activePackage: {
      actionSha256,
      artifactSha256: packageValue.bundleSha256,
      candidateSha256: packageValue.candidateSha256,
      path: `${family}-canary-bundles/sha256-${packageValue.bundleSha256}.json`,
    },
    contractVersion: `forecast-adjustment-${family}-maintenance-registry/v1`,
    rawReason: null,
    siteKey: "ballydidean",
  };
  return {
    action,
    entry: { family, receipt, registration, slot: "active" },
    packageValue,
    registrationPreimageBytes,
    registry,
  };
}

// retain exact canonical bytes, digest, and padded base64 for one original value
function material(value) {
  const bytes = Buffer.from(canonicalJsonBytes(value));
  return {
    base64: bytes.toString("base64"),
    bytes: [...bytes],
    sha256: hash(bytes),
    value,
  };
}

// collect only canonical clocks actually present in the fixture graph
function collectClocks(value, clocks) {
  // preserve strings only when Node accepts their exact canonical ISO spelling
  if (typeof value === "string") {
    const epoch = Date.parse(value);
    if (Number.isFinite(epoch) && new Date(epoch).toISOString() === value) {
      clocks[value] = epoch;
    }
    return;
  }
  // traverse arrays and objects without manufacturing absent members
  if (value !== null && typeof value === "object") {
    Object.values(value).forEach((member) => collectClocks(member, clocks));
  }
}

// encode one bounded JSON value stream for the native differential fixture
function frame(value, output) {
  // retain every JSON primitive before descending into containers
  if (value === null) {
    output.push("z");
  } else if (typeof value === "string") {
    output.push(`s${Buffer.from(value, "utf16le").toString("hex")}`);
  } else if (typeof value === "number") {
    output.push(`n${String(value)}`);
  } else if (typeof value === "boolean") {
    output.push(value ? "t" : "f");
  } else if (Array.isArray(value)) {
    output.push(`a${value.length}`);
    value.forEach((member) => frame(member, output));
  } else {
    const entries = Object.entries(value).sort(([left], [right]) => left < right ? -1 : Number(left > right));
    output.push(`o${entries.length}`);
    entries.forEach(([key, member]) => {
      frame(key, output);
      frame(member, output);
    });
  }
}

const wind = qualification("wind", buildWindPackage());
const temperature = qualification("temperature", await buildTemperaturePackage());
const catalog = {
  contractVersion: "adjustment-installed-candidate-catalog/v2",
  entries: [wind.entry, temperature.entry],
};
const emptyCatalog = {
  contractVersion: "adjustment-installed-candidate-catalog/v2",
  entries: [],
};
const values = [
  catalog,
  emptyCatalog,
  wind.action,
  wind.entry.receipt,
  wind.registry,
  wind.packageValue,
  wind.packageValue.source,
  wind.packageValue.candidate.coefficients,
  wind.packageValue.candidate.trainingProvenance,
  (({ candidateArtifactSha256: _, ...unsigned }) => unsigned)(wind.packageValue.candidate),
  (({ bundleSha256: _, ...unsigned }) => unsigned)(wind.packageValue),
  temperature.action,
  temperature.entry.receipt,
  temperature.registry,
  temperature.packageValue,
  temperature.packageValue.source,
  (({ bundleSha256: _, ...unsigned }) => unsigned)(temperature.packageValue),
];
const materials = values.map(material);
const clocks = {};
values.forEach((value) => collectClocks(value, clocks));
const output = [];
frame({
  clocks,
  emptyCatalogBytes: material(emptyCatalog).bytes,
  hashes: [
    ...materials.map(({ bytes, sha256 }) => ({ bytes, sha256 })),
    { bytes: [...wind.registrationPreimageBytes], sha256: hash(wind.registrationPreimageBytes) },
    { bytes: [...temperature.registrationPreimageBytes], sha256: hash(temperature.registrationPreimageBytes) },
  ],
  materials,
}, output);
assert.equal(materials.length, values.length);
process.stdout.write(`${output.join("\n")}\n`);
