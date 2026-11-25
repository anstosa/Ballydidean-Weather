import { readFileSync, writeFileSync } from "node:fs";
import {
  FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1,
  FORECAST_ADJUSTMENT_CANONICAL_TRAINING_PROVENANCE_V1,
  canonicalJsonBytes,
  canonicalSha256,
  createDevelopmentReport,
  createForecastAdjustmentCandidate,
  createInsufficientDataReport,
} from "../../../../packages/forecast-adjustment/dist/index.js";

// decode one bounded test frame without redefining canonical JSON
function readFixtureValue(lines) {
  const frame = lines.next().value;
  const body = frame.slice(1);
  switch (frame[0]) {
    case "z": return null;
    case "t": return true;
    case "f": return false;
    case "n": return Buffer.from(body, "hex").readDoubleBE(0);
    case "s":
      return (body.match(/.{4}/gu) ?? [])
        .map((unit) => String.fromCharCode(Number.parseInt(unit, 16))).join("");
    case "a": return Array.from({ length: Number(body) }, () => readFixtureValue(lines));
    case "o": return Object.fromEntries(Array.from(
      { length: Number(body) }, () => [readFixtureValue(lines), readFixtureValue(lines)],
    ));
    default: throw new Error("invalid wind test frame");
  }
}

// canonicalize real Rust-projected values through the original compiled function
if (process.argv[2] === "--encode-values") {
  const lines = readFileSync(0, "utf8").trimEnd().split("\n").values();
  const material = readFixtureValue(lines);
  if (!lines.next().done) throw new Error("unexpected trailing wind test frames");
  writeFileSync(1, canonicalJsonBytes(material));
  process.exit(0);
}

const SNAPSHOT = "a".repeat(64);
const D0 = "2025-06-26";
const D401 = "2026-08-01";
const SCOREABLE_STATIONS = [
  "ambient-maxweather",
  "ballydidean-ecowitt",
  "netatmo-nearby",
  "tempest-126537",
  "tempest-64255",
];
const PROVIDER_FAMILIES = ["ambient", "ecowitt", "netatmo", "tempest"];
const LEAD_BANDS = [
  "001-024",
  "025-048",
  "049-072",
  "073-096",
  "097-120",
  "121-144",
  "145-168",
];
const PAIRS = [
  ...LEAD_BANDS
    .filter((leadBand) => leadBand !== "049-072")
    .map((leadBand) => ({ leadBand, metric: "windGustMps" })),
  ...LEAD_BANDS.map((leadBand) => ({ leadBand, metric: "windSpeedMps" })),
];

// encode one original canonical preimage as lowercase hexadecimal bytes
function hex(value) {
  return Buffer.from(value, "utf8").toString("hex");
}

// encode one JavaScript binary64 result without decimal formatting drift
function bits(value) {
  const bytes = Buffer.allocUnsafe(8);
  bytes.writeDoubleBE(value);
  return bytes.toString("hex");
}

// construct one distinct auxiliary digest without replacing production hashing
function auxiliaryHash(pairIndex, fold, stationIndex) {
  return (pairIndex * 25 + fold * 5 + stationIndex + 1)
    .toString(16)
    .padStart(64, "0");
}

// construct one passing generated fold for an exact wind pair
function passingFold(pair, pairIndex, fold) {
  return {
    auxiliaryModelSha256s: SCOREABLE_STATIONS.map((_station, stationIndex) =>
      auxiliaryHash(pairIndex, fold, stationIndex)),
    bootstrapLowerBound: 0.01,
    contractVersion: "forecast-adjustment-development-loso-fold/v1",
    fold,
    materialHarmSliceKeys: [],
    metricBand: pair,
    nonnegativeStationFraction: 1,
    passed: true,
    providerBalancedAdjustedLoss: 9,
    providerBalancedRawLoss: 10,
    providerBalancedSkill: 0.1,
    providerFamilies: PROVIDER_FAMILIES,
    scoreableStationKeys: SCOREABLE_STATIONS,
  };
}

// construct one deterministic root coefficient for each qualified pair
function coefficient(pair, index) {
  return {
    coefficient: index + 0.25,
    daypart: null,
    effectiveEventCount: 200 + index,
    leadBand: pair.leadBand,
    level: 1,
    metric: pair.metric,
    month: null,
    season: null,
  };
}

// construct one scalar final-training envelope for each qualified pair
function envelope(pair, index) {
  return {
    leadBand: pair.leadBand,
    maximum: 30 + index,
    metric: pair.metric,
    minimum: 1 + index,
  };
}

const folds = PAIRS.flatMap((pair, pairIndex) =>
  [1, 2, 3, 4, 5].map((fold) => passingFold(pair, pairIndex, fold)));
const inputCoefficients = PAIRS.map(coefficient).reverse();
const inputEnvelopes = PAIRS.map(envelope).reverse();
const developmentReport = createDevelopmentReport({
  enabledMetricBands: [...PAIRS].reverse(),
  folds: [...folds].reverse(),
});
const evaluationEpochId = canonicalSha256({
  d0: D0,
  d401: D401,
  snapshotManifestSha256: SNAPSHOT,
});
const candidate = createForecastAdjustmentCandidate({
  coefficients: inputCoefficients,
  developmentReportSha256: developmentReport.developmentReportSha256,
  enabledMetricBands: [...PAIRS].reverse(),
  evaluationEpochId,
  exportManifestSha256: SNAPSHOT,
  finalTrainingCutoff: "2026-07-25T06:59:59.999Z",
  forecastIdentity: FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1,
  runtimeFingerprint: { icuVersion: "fixture-icu", tzdataVersion: "fixture-tz" },
  trainingEnvelopes: inputEnvelopes,
  trainingProvenance: FORECAST_ADJUSTMENT_CANONICAL_TRAINING_PROVENANCE_V1,
});
const auxiliary = {
  coefficients: [coefficient(PAIRS[0], 0)],
  fold: 1,
  metricBand: PAIRS[0],
  physicalStationKey: "ambient-maxweather",
};
const insufficient = createInsufficientDataReport({
  failedGates: ["maintenance_exact_thirteen_pairs"],
  snapshotManifestSha256: SNAPSHOT,
});
const unsignedDevelopment = Object.fromEntries(
  Object.entries(developmentReport)
    .filter(([key]) => key !== "developmentReportSha256"),
);
const unsignedCandidate = Object.fromEntries(
  Object.entries(candidate).filter(([key]) => key !== "candidateArtifactSha256"),
);
const unsignedInsufficient = Object.fromEntries(
  Object.entries(insufficient).filter(([key]) => key !== "reportSha256"),
);
const developmentPublication = {
  contractVersion: "wind-maintenance-fit/v2",
  state: "development_candidate",
  candidate,
  developmentReport,
  dueMonth: "2026-09",
  confirmationOpened: false,
};
const insufficientPublication = {
  ...insufficient,
  contractVersion: "wind-maintenance-fit/v2",
  state: "no_candidate",
  dueMonth: "2026-09",
  confirmationOpened: false,
  reason: "insufficient_development_support",
};
const rows = [
  ["PREIMAGE", "auxiliary", hex(canonicalJsonBytes(auxiliary)), canonicalSha256(auxiliary)],
  [
    "PREIMAGE",
    "forecast-identity",
    hex(canonicalJsonBytes(candidate.forecastIdentity)),
    canonicalSha256(candidate.forecastIdentity),
  ],
  [
    "PREIMAGE",
    "training-provenance",
    hex(canonicalJsonBytes(candidate.trainingProvenance)),
    canonicalSha256(candidate.trainingProvenance),
  ],
  [
    "PREIMAGE",
    "metric-policies",
    hex(canonicalJsonBytes(candidate.metricPolicies)),
    canonicalSha256(candidate.metricPolicies),
  ],
  [
    "PREIMAGE",
    "development",
    hex(canonicalJsonBytes(unsignedDevelopment)),
    developmentReport.developmentReportSha256,
  ],
  [
    "PREIMAGE",
    "coefficients",
    hex(canonicalJsonBytes(candidate.coefficients)),
    candidate.coefficientPayloadSha256,
  ],
  [
    "PREIMAGE",
    "epoch",
    hex(canonicalJsonBytes({ d0: D0, d401: D401, snapshotManifestSha256: SNAPSHOT })),
    evaluationEpochId,
  ],
  [
    "PREIMAGE",
    "candidate",
    hex(canonicalJsonBytes(unsignedCandidate)),
    candidate.candidateArtifactSha256,
  ],
  [
    "PREIMAGE",
    "insufficient",
    hex(canonicalJsonBytes(unsignedInsufficient)),
    insufficient.reportSha256,
  ],
  ["BYTES", "development", hex(canonicalJsonBytes(developmentReport))],
  ["BYTES", "candidate", hex(canonicalJsonBytes(candidate))],
  ["BYTES", "insufficient", hex(canonicalJsonBytes(insufficient))],
  ["BYTES", "development-publication", hex(canonicalJsonBytes(developmentPublication))],
  ["BYTES", "insufficient-publication", hex(canonicalJsonBytes(insufficientPublication))],
  ...candidate.coefficients.map((item) => [
    "CELL",
    bits(item.coefficient),
    hex(canonicalJsonBytes(item)),
  ]),
  [
    "SUMMARY",
    candidate.candidateArtifactSha256,
    candidate.coefficientPayloadSha256,
    candidate.developmentReportSha256,
    candidate.evaluationEpochId,
    developmentReport.developmentReportSha256,
    String(candidate.enabledMetricBands.length),
    String(candidate.coefficients.length),
    String(candidate.trainingEnvelopes.length),
    String(developmentReport.folds.length),
    insufficient.reportSha256,
  ],
  ...candidate.enabledMetricBands.map((pair, index) => [
    "PAIR",
    String(index),
    pair.metric,
    pair.leadBand,
  ]),
  ...candidate.coefficients.map((item, index) => [
    "COEFFICIENT",
    String(index),
    item.metric,
    item.leadBand,
    String(item.level),
    item.daypart ?? "<null>",
    item.month ?? "<null>",
    item.season ?? "<null>",
    bits(item.coefficient),
    bits(item.effectiveEventCount),
  ]),
  ...candidate.trainingEnvelopes.map((item, index) => [
    "ENVELOPE",
    String(index),
    item.metric,
    item.leadBand,
    bits(item.minimum),
    bits(item.maximum),
  ]),
  ...developmentReport.folds.map((item, index) => [
    "FOLD",
    String(index),
    item.metricBand.metric,
    item.metricBand.leadBand,
    String(item.fold),
    item.passed ? "true" : "false",
    bits(item.bootstrapLowerBound),
    bits(item.providerBalancedRawLoss),
    bits(item.providerBalancedAdjustedLoss),
    bits(item.providerBalancedSkill),
    bits(item.nonnegativeStationFraction),
    item.auxiliaryModelSha256s.join(","),
    item.providerFamilies.join(","),
    item.scoreableStationKeys.join(","),
  ]),
  ...candidate.metricPolicies.map((item, index) => [
    "POLICY",
    String(index),
    item.metric,
    bits(item.correctionMinimum),
    bits(item.correctionMaximum),
    bits(item.finalMinimum),
    bits(item.finalMaximum),
    item.finalMaximumExclusive ? "true" : "false",
    item.wrapsFinalValue ? "true" : "false",
  ]),
  [
    "IDENTITY",
    candidate.forecastIdentity.adapterVersion,
    candidate.forecastIdentity.cohort,
    candidate.forecastIdentity.contractEpoch,
    candidate.forecastIdentity.dataset,
    candidate.forecastIdentity.referenceKind,
    candidate.forecastIdentity.sourceConfigFingerprint,
    candidate.forecastIdentity.sourceKey,
    candidate.forecastIdentity.upstreamModel,
  ],
  [
    "PROVENANCE",
    candidate.trainingProvenance.aggregationContractSha256,
    candidate.trainingProvenance.coordinateManifestSha256,
    candidate.trainingProvenance.metricEligibilitySha256,
    candidate.trainingProvenance.observationSourceLineageSha256,
    candidate.trainingProvenance.observationStationManifestSha256,
    candidate.trainingProvenance.spatialWeightSha256,
  ],
  [
    "RUNTIME",
    candidate.runtimeFingerprint.icuVersion,
    candidate.runtimeFingerprint.tzdataVersion,
    candidate.finalTrainingCutoff,
    candidate.exportManifestSha256,
  ],
];

process.stdout.write(`${rows.map((row) => row.join("\t")).join("\n")}\n`);
