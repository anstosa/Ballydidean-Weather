import {
  createMovingBlockBootstrapStartPlan,
  evaluateDevelopmentLosoFold,
  movingBlockBootstrap,
  pairedSkill,
} from "../../../../packages/forecast-adjustment/dist/index.js";

// encode one JavaScript binary64 result without decimal formatting drift
function bits(value) {
  const bytes = Buffer.allocUnsafe(8);
  bytes.writeDoubleBE(value);
  return bytes.toString("hex");
}

// build one consecutive 42-date window with retained empty sampler slots
function bootstrapWindow() {
  const start = Date.UTC(2025, 0, 1);
  return {
    key: "wind-speed:fold-5",
    dateSlots: Array.from({ length: 42 }, (_unused, index) => ({
      events: index % 6 === 0 ? [] : [{
        actual: 8 + (index % 9) * 0.5,
        adjustedPrediction: 8.25 + (index % 7) * 0.5,
        rawPrediction: 6.5 + (index % 5) * 0.75,
      }],
      localDate: new Date(start + index * 86_400_000).toISOString().slice(0, 10),
    })),
  };
}

// project the frozen plan and every ordered bootstrap result bit
function bootstrapProjection() {
  const window = bootstrapWindow();
  const plan = createMovingBlockBootstrapStartPlan([window.dateSlots.length]);
  const result = movingBlockBootstrap([window]);
  return [
    ...plan.map((replicate, index) => [
      "PLAN",
      String(index),
      replicate[0].join(","),
    ]),
    [
      "BOOTSTRAP",
      String(result.eventCount),
      bits(result.rawLoss),
      bits(result.adjustedLoss),
      bits(result.skill),
      bits(result.bootstrapLowerBound),
      bits(result.bootstrapUpperBound),
    ],
    ...result.replicateSkills.map((skill, index) => [
      "REPLICATE",
      String(index),
      bits(skill),
    ]),
  ];
}

// create one valid station score while deriving the exact original point skill
function station(
  physicalStationKey,
  providerFamily,
  rawLoss,
  adjustedLoss,
) {
  return {
    adjustedLoss,
    eventCount: 100,
    physicalStationKey,
    pointSkill: pairedSkill(rawLoss, adjustedLoss),
    providerFamily,
    rawLoss,
    remainingNetworkScoreEvents: 100,
    scoreMatches: 100,
    trainingMatches: 500,
  };
}

// evaluate one fold case through the compiled original implementation
function foldCase(label, losses, materialHarmSliceKeys) {
  const identities = [
    ["tempest-64255", "tempest"],
    ["netatmo-nearby", "netatmo"],
    ["ambient-maxweather", "ambient"],
    ["tempest-126537", "tempest"],
    ["ballydidean-ecowitt", "ecowitt"],
  ];
  const result = evaluateDevelopmentLosoFold({
    auxiliaryModelSha256s: ["5", "4", "3", "2", "1"].map((value) =>
      value.repeat(64)),
    bootstrapLowerBound: 0.01,
    fold: 5,
    materialHarmSliceKeys,
    metricBand: { leadBand: "001-024", metric: "windSpeedMps" },
    stationScores: identities.map(([key, family], index) =>
      station(key, family, losses[index].raw, losses[index].adjusted)),
  });
  return [[
    "FOLD",
    label,
    result.contractVersion,
    String(result.fold),
    result.metricBand.metric,
    result.metricBand.leadBand,
    result.passed ? "true" : "false",
    bits(result.bootstrapLowerBound),
    bits(result.providerBalancedRawLoss),
    bits(result.providerBalancedAdjustedLoss),
    bits(result.providerBalancedSkill),
    bits(result.nonnegativeStationFraction),
    result.auxiliaryModelSha256s.join(","),
    result.materialHarmSliceKeys.join(","),
    result.providerFamilies.join(","),
    result.scoreableStationKeys.join(","),
  ]];
}

const ordinaryLosses = [
  { raw: 18, adjusted: 15 },
  { raw: 14, adjusted: 12 },
  { raw: 10, adjusted: 9 },
  { raw: 16, adjusted: 14 },
  { raw: 12, adjusted: 10 },
];
const zeroLosses = ordinaryLosses.map(() => ({ raw: 0, adjusted: 0 }));
const rows = [
  ...bootstrapProjection(),
  ...foldCase("ordinary-pass", ordinaryLosses, []),
  ...foldCase("material-harm", ordinaryLosses, ["netatmo-nearby"]),
  ...foldCase("zero-raw", zeroLosses, []),
];

process.stdout.write(`${rows.map((row) => row.join("\t")).join("\n")}\n`);
