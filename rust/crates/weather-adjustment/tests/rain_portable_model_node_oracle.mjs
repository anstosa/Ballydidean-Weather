import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createRainHurdleWindPortableArtifactEvaluator, RAIN_HURDLE_WIND_PROJECTION_IDS } from "../../../../packages/forecast-adjustment/dist/rain-hurdle-wind.js";
import { RAIN_HURDLE_WIND_ARTIFACT_JSON } from "../../../../packages/forecast-adjustment/dist/rain-hurdle-wind-artifact.js";

const { featureNames } = JSON.parse(RAIN_HURDLE_WIND_ARTIFACT_JSON);
const cases = [
  ["scored", 0.05, "2026-03-01T07:59:59.000Z", 2],
  ["scored", 0.05, "2026-03-01T08:00:00.000Z", 3],
  ["scored", 0.05, "2026-06-01T06:59:59.000Z", 5],
  ["scored", 0.05, "2026-06-01T07:00:00.000Z", 6],
  ["raw", 0.25, "2026-01-01T12:00:00.000Z", 1],
  ["raw", 0.25, "2026-07-01T12:00:00.000Z", 7],
  ["scored", 0.25, "2026-07-01T12:00:00.000Z", 7],
  ["raw", 47, "2026-07-01T12:00:00.000Z", 7],
];

// construct a closed model with distinct probabilities so nesting can change them
function head(baseScore, objective) {
  return { baseScore, objective, trees: Array.from({ length: 160 }, () => [[0], [0], [-1], [-1], [0]]) };
}

// use the original portable runtime against the same typed tables as the Rust fixture
function evaluator(projectionId, mode) {
  const artifact = {
    contractVersion: "rain-hurdle-wind-runtime/v2",
    projectionId,
    featureNames,
    modelMonth: "2026-03",
    rules: [0.1, 1, 2.5].map((threshold, index) => ({ threshold, cutoff: mode === "raw" ? null : [0.34, 0.81, 0.81][index] })),
    categoryScales: [1, 1, 1],
    heads: {
      "0.1": head(0.30, "binary:logistic"),
      "1.0": head(0.80, "binary:logistic"),
      "2.5": head(0.75, "binary:logistic"),
      amount: head(0.75, "reg:gamma"),
    },
  };
  const bytes = JSON.stringify(artifact);
  return createRainHurdleWindPortableArtifactEvaluator(bytes, createHash("sha256").update(bytes).digest("hex"));
}

// serialize exact binary64 results without decimal display rounding
function bits(number) {
  const bytes = Buffer.alloc(8);
  bytes.writeDoubleBE(number);
  return bytes.toString("hex");
}

// score each arm across seasonal boundaries, a wet-call floor, and unbounded raw heavy rain
for (const [projectionIndex, projection] of RAIN_HURDLE_WIND_PROJECTION_IDS.entries()) {
  const evaluators = new Map(["raw", "scored"].map((mode) => [mode, evaluator(projection, mode)]));
  // reject malformed feature geometry before requiring a target clock
  assert.throws(() => evaluators.get("raw")(new Float32Array(106)), /feature vector invalid/);
  for (const [mode, raw, validAt, month] of cases) {
    const features = new Float32Array(107);
    features[5] = raw;
    const evaluate = evaluators.get(mode);
    assert.throws(() => evaluate(features), /validAt is required/);
    const result = evaluate(features, validAt);
    console.log([
      projectionIndex, mode, raw, month,
      ...[
        result.correctedPrecipitationMm, result.occurrenceProbabilityAtLeast0_1,
        result.occurrenceProbabilityAtLeast1_0, result.occurrenceProbabilityAtLeast2_5,
        result.positiveAmountMm,
      ].map(bits),
    ].join("\t"));
  }
}
