import {
  fitRobustHierarchy,
} from "../../../../packages/forecast-adjustment/dist/index.js";

// encode one JavaScript binary64 result without decimal formatting drift
function bits(value) {
  const bytes = Buffer.allocUnsafe(8);
  bytes.writeDoubleBE(value);
  return bytes.toString("hex");
}

// create one distinct-event hierarchy residual with explicit local features
function observation(index, residual, month, season, daypart) {
  return {
    daypart,
    month,
    referenceAt: null,
    residual,
    season,
    stableId: `row-${String(index).padStart(3, "0")}`,
    targetLeadHours: 24,
    validAt: `2025-01-01T00:00:00.${String(index).padStart(3, "0")}Z`,
    weight: 1,
  };
}

// build exact root, season, and month threshold groups
function thresholdRows() {
  return [
    ...Array.from({ length: 50 }, (_unused, index) =>
      observation(index, 8, 1, "winter", "night")),
    ...Array.from({ length: 50 }, (_unused, index) =>
      observation(index + 50, 8, 2, "winter", "night")),
    ...Array.from({ length: 49 }, (_unused, index) =>
      observation(index + 100, 8, 6, "summer", "afternoon")),
    ...Array.from({ length: 50 }, (_unused, index) =>
      observation(index + 149, 8, 7, "summer", "afternoon")),
    observation(199, 8, 9, "autumn", "morning"),
  ];
}

// build one same-cell residual corpus
function sameCellRows(count, residualFor) {
  return Array.from({ length: count }, (_unused, index) =>
    observation(index, residualFor(index), 1, "winter", "night"));
}

// project generated coefficient fields and exact numeric bits
function project(label, metric, rows) {
  const coefficients = fitRobustHierarchy(metric, "001-024", rows);
  const output = [["CASE", label, String(coefficients.length)]];
  output.push(...coefficients.map((coefficient, index) => [
    "COEFFICIENT",
    label,
    String(index),
    coefficient.metric,
    coefficient.leadBand,
    String(coefficient.level),
    coefficient.daypart ?? "<null>",
    coefficient.month === null ? "<null>" : String(coefficient.month),
    coefficient.season ?? "<null>",
    bits(coefficient.coefficient),
    bits(coefficient.effectiveEventCount),
  ]));
  return output;
}

const threshold = thresholdRows();
const rows = [
  ...project("below-root", "windGustMps", threshold.slice(0, 199)),
  ...project("threshold-shrink", "windGustMps", threshold),
  ...project("speed-cap", "windSpeedMps", sameCellRows(200, () => 100)),
  ...project("gust-cap", "windGustMps", sameCellRows(200, () => 100)),
  ...project(
    "direction-tie",
    "windDirectionDegrees",
    sameCellRows(200, (index) => index < 100 ? -10 : 10),
  ),
  ...project("direction-wrap", "windDirectionDegrees", [
    ...Array.from({ length: 100 }, (_unused, index) =>
      observation(index, -170, 1, "winter", "night")),
    ...Array.from({ length: 101 }, (_unused, index) =>
      observation(index + 100, 170, 7, "summer", "afternoon")),
  ]),
];

process.stdout.write(`${rows.map((row) => row.join("\t")).join("\n")}\n`);
