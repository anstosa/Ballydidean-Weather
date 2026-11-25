import { createHash } from "node:crypto";

const LEAD_BANDS = ["001-024", "025-048", "049-072", "073-096", "097-120", "121-144", "145-168"];
const FORECAST_IDENTITY = Object.freeze({
  adapterVersion: "open-meteo-forecast-daily/v4",
  cohort: "legacy_v4_retrieval_snapshot",
  contractEpoch: "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83",
  dataset: "forecast",
  referenceKind: "retrieval_snapshot",
  sourceConfigFingerprint: "ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4",
  sourceKey: "open-meteo-forecast-v4",
  upstreamModel: "best_match",
});
const TRAINING_PROVENANCE = Object.freeze({
  aggregationContractSha256: "9c309ef5a00780167570746ad6c31b9128c266db50954fe4645287e1f2b31e64",
  coordinateManifestSha256: "04bfd93a03c393e977c8767a9aca6fe2a4cba9c263cb46e6987fa733b666ba58",
  metricEligibilitySha256: "53731954b347836a26500b05a195ca15cf26214c4d561fe482c5ff87ef56a82e",
  observationSourceLineageSha256: "261a134589a12c1bbbd9a783343950317fd1fbc87e08383e60e805b7761566cc",
  observationStationManifestSha256: "a1f76440c056987bbb434d5315e4916f961deeb2951fe889d785943f559cdd49",
  spatialWeightSha256: "8ed5ce70d33edd4a5166049d9938cbaaf800151b6a0b3345d3005419e9041c74",
});
const METRIC_POLICIES = Object.freeze([
  { correctionMaximum: 20, correctionMinimum: -20, finalMaximum: 100,
    finalMaximumExclusive: false, finalMinimum: 0, metric: "relativeHumidityPercent", wrapsFinalValue: false },
  { correctionMaximum: 5, correctionMinimum: -5, finalMaximum: 70,
    finalMaximumExclusive: false, finalMinimum: -100, metric: "temperatureC", wrapsFinalValue: false },
  { correctionMaximum: 45, correctionMinimum: -45, finalMaximum: 360,
    finalMaximumExclusive: true, finalMinimum: 0, metric: "windDirectionDegrees", wrapsFinalValue: true },
  { correctionMaximum: 12, correctionMinimum: -12, finalMaximum: 150,
    finalMaximumExclusive: false, finalMinimum: 0, metric: "windGustMps", wrapsFinalValue: false },
  { correctionMaximum: 8, correctionMinimum: -8, finalMaximum: 150,
    finalMaximumExclusive: false, finalMinimum: 0, metric: "windSpeedMps", wrapsFinalValue: false },
]);

// recursively order one JSON value exactly as the retained canonical serializer
function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  }
  return value;
}

// encode one recursively sorted retained document with its required final line feed
export function qualifiedWindCanonicalBytes(value) {
  return Buffer.from(`${JSON.stringify(canonical(value))}\n`);
}

// hash one exact byte member or canonical JSON value
export function qualifiedWindSha256(value) {
  const bytes = Buffer.isBuffer(value) ? value : qualifiedWindCanonicalBytes(value);
  return createHash("sha256").update(bytes).digest("hex");
}

// form the original thirteen wind metric-band pairs in canonical key order
function windPairs() {
  return [
    ...LEAD_BANDS.filter((leadBand) => leadBand !== "049-072")
      .map((leadBand) => ({ leadBand, metric: "windGustMps" })),
    ...LEAD_BANDS.map((leadBand) => ({ leadBand, metric: "windSpeedMps" })),
  ];
}

// create one complete self-addressed original wind maintenance runtime package
export function qualifiedWindPackage() {
  const enabledMetricBands = windPairs();
  // preserve the candidate builder's canonical coefficient-cell order
  const coefficients = enabledMetricBands.map((pair) => ({
    coefficient: 0.25,
    daypart: null,
    effectiveEventCount: 200,
    leadBand: pair.leadBand,
    level: 1,
    metric: pair.metric,
    month: null,
    season: null,
  })).sort((left, right) => {
    const leftKey = JSON.stringify(canonical(left));
    const rightKey = JSON.stringify(canonical(right));
    return leftKey < rightKey ? -1 : leftKey > rightKey ? 1 : 0;
  });
  // retain the candidate builder's metric-band envelope order
  const trainingEnvelopes = enabledMetricBands.map((pair) => ({
    leadBand: pair.leadBand,
    maximum: 40,
    metric: pair.metric,
    minimum: 0,
  }));
  const unsignedCandidate = {
    algorithmContractVersion: "robust-hierarchical-median/v1",
    coefficientPayloadSha256: qualifiedWindSha256(coefficients),
    coefficients,
    contractVersion: "forecast-adjustment-candidate/v2",
    developmentReportSha256: "a".repeat(64),
    enabledMetricBands,
    evaluationEpochId: "maintenance-2026-10",
    exportManifestSha256: "b".repeat(64),
    finalTrainingCutoff: "2026-09-01T06:59:59.999Z",
    forecastIdentity: structuredClone(FORECAST_IDENTITY),
    metricPolicies: structuredClone(METRIC_POLICIES),
    runtimeFingerprint: { icuVersion: "fixture-icu", tzdataVersion: "fixture-tzdata" },
    siteKey: "ballydidean",
    timezone: "America/Los_Angeles",
    trainingEnvelopes,
    trainingProvenance: structuredClone(TRAINING_PROVENANCE),
  };
  const candidate = {
    ...unsignedCandidate,
    candidateArtifactSha256: qualifiedWindSha256(unsignedCandidate),
  };
  const unsignedPackage = {
    candidate,
    candidateSha256: qualifiedWindSha256(Buffer.from("qualified-wind-maintenance-fit/v1\n")),
    contractVersion: "forecast-adjustment-maintenance-runtime-package/v1",
    dueMonth: "2026-10",
    family: "wind",
    source: structuredClone(FORECAST_IDENTITY),
  };
  return Object.freeze({
    ...unsignedPackage,
    bundleSha256: qualifiedWindSha256(unsignedPackage),
  });
}
