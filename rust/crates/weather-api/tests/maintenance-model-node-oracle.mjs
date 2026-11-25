import assert from "node:assert/strict";

import {
  FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1,
  FORECAST_ADJUSTMENT_CANONICAL_TRAINING_PROVENANCE_V1,
  buildForecastAdjustmentMaintenanceRuntimePackage,
  canonicalJsonBytes,
  canonicalSha256,
  createDevelopmentReport,
  createForecastAdjustmentCandidate,
  encodeMaintenanceBinary64,
  evaluateDevelopmentLosoFold,
  runtimeCalendarFingerprint,
} from "../../../../packages/forecast-adjustment/dist/index.js";
import { evaluateWindMaintenanceCandidate, evaluateWindMaintenanceIncumbent } from "../../../../apps/api/dist/index.js";

const leadBands = ["001-024", "025-048", "049-072", "073-096", "097-120", "121-144", "145-168"];
const pairs = [
  ...leadBands.map((leadBand) => ({ leadBand, metric: "windSpeedMps" })),
  ...leadBands.filter((leadBand) => leadBand !== "049-072")
    .map((leadBand) => ({ leadBand, metric: "windGustMps" })),
];
const referenceAt = "2026-10-08T00:00:00.000Z";
const issuedAt = "2026-10-08T00:35:00.000Z";

// build the source test's five ordinary physical-station score rows
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

// reproduce the original selected development candidate without a fixture file or parser
function maintenanceBundle() {
  const developmentReport = createDevelopmentReport({
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
    trainingEnvelopes: pairs.map((pair) => ({ ...pair, maximum: 40, minimum: 0 })),
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

// build one exact hourly October instant without host-local formatting
function validAt(leadHours) {
  const day = 8 + Math.floor(leadHours / 24);
  const hour = leadHours % 24;
  return `2026-10-${String(day).padStart(2, "0")}T${String(hour).padStart(2, "0")}:00:00.000Z`;
}

const bundle = maintenanceBundle();
const source = {
  family: "wind",
  issuedAt,
  rows: Array.from({ length: 168 }, (_, index) => {
    const leadHours = index + 1;
    return {
      adapterVersion: bundle.source.adapterVersion,
      contractEpoch: bundle.source.contractEpoch,
      dataset: bundle.source.dataset,
      modelLeadHours: leadHours,
      referenceAt,
      sourceConfigFingerprint: bundle.source.sourceConfigFingerprint,
      sourceKey: bundle.source.sourceKey,
      upstreamModel: bundle.source.upstreamModel,
      validAt: validAt(leadHours),
      windGustMps64: index >= 48 && index < 72 ? null : encodeMaintenanceBinary64(8),
      windSpeedMps64: encodeMaintenanceBinary64(5),
    };
  }),
};
const result = evaluateWindMaintenanceCandidate({ bundle }, source);
const incumbent = evaluateWindMaintenanceIncumbent(
  { state: "disabled", bundle: null, reasonCode: "bundle_missing" }, source,
);

assert.equal(result.length, 168);
assert.equal(result[0].speedWouldApply, true);
assert.equal(result[0].gustWouldApply, true);
assert.equal(result[48].candidateGustMps, null);
assert.equal(result[48].gustWouldApply, false);
assert.equal(result[72].gustWouldApply, true);
assert.equal(incumbent.length, result.length);
// preserve the incumbent's loaded disabled reason and the unchanged raw source across all leads
for (const [index, row] of incumbent.entries()) {
  assert.equal(row.reasonCode, "bundle_missing");
  assert.equal(row.incumbentSpeedMps, 5);
  assert.equal(row.incumbentGustMps, index >= 48 && index < 72 ? null : 8);
  assert.equal(row.speedApplied, false);
  assert.equal(row.gustApplied, false);
}

process.stdout.write(`candidate=${bundle.candidate.candidateArtifactSha256}\n`);
process.stdout.write(`coefficient=${bundle.candidate.coefficientPayloadSha256}\n`);
process.stdout.write(`development=${bundle.candidate.developmentReportSha256}\n`);
process.stdout.write(`export=${bundle.candidate.exportManifestSha256}\n`);
process.stdout.write(`bundle=${bundle.bundleSha256}\n`);
process.stdout.write(`fit=${bundle.candidateSha256}\n`);
process.stdout.write(`incumbent=${incumbent[0].reasonCode}\n`);
for (const [index, row] of result.entries()) {
  process.stdout.write([
    "row",
    String(index),
    row.validAt,
    String(row.candidateSpeedMps),
    row.candidateGustMps === null ? "null" : String(row.candidateGustMps),
    String(row.speedWouldApply),
    String(row.gustWouldApply),
  ].join("|") + "\n");
}
