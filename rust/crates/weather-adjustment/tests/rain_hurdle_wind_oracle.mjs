import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const source = readFileSync(
  resolve(repositoryRoot, "packages/forecast-adjustment/dist/rain-hurdle-wind.js"),
  "utf8",
);

// extract one compiled production helper between stable comment markers
function sourceSpan(startMarker, endMarker) {
  const start = source.indexOf(startMarker);
  const end = source.indexOf(endMarker, start);
  // fail if the compiled owning implementation moved
  if (start < 0 || end < 0) {
    throw new Error(`unable to extract ${startMarker}`);
  }
  return source.slice(start, end);
}

// construct one compact source-shaped split head
function splitHead(featureIndex, threshold, leftLeaf, rightLeaf, missingLeft) {
  return {
    objective: "binary:logistic",
    baseScore: 0.5,
    trees: [[
      [featureIndex, 0, 0],
      [threshold, leftLeaf, rightLeaf],
      [1, -1, -1],
      [2, -1, -1],
      [missingLeft, 0, 0],
    ]],
  };
}

// construct one compact positive-amount leaf head
function amountHead() {
  return {
    objective: "reg:gamma",
    baseScore: 2,
    trees: [[[0], [0.1], [-1], [-1], [0]]],
  };
}

// build the bounded typed-table fixture shared with Rust
function artifact() {
  return {
    contractVersion: "rain-hurdle-wind-runtime/v1",
    heads: {
      "0.1": splitHead(0, 0.50000001, 1, -1, 1),
      "1.0": splitHead(1, 0.5, 0.5, -0.5, 0),
      "2.5": splitHead(2, 0.5, 0.25, -0.25, 1),
      amount: amountHead(),
    },
    rules: [
      { threshold: 0.1, cutoff: 0.6 },
      { threshold: 1, cutoff: 0.6 },
      { threshold: 2.5, cutoff: null },
    ],
    categoryScales: [0.8, 1.2, 2],
  };
}

// build one 107-value normalized feature fixture
function featuresFor(name) {
  const features = new Float32Array(107);
  // select the representative branch and raw category case
  if (name === "dry") {
    features[0] = 1;
    features[1] = 1;
    features[2] = 1;
    features[5] = 0;
  } else if (name === "category-1") {
    features[0] = 0;
    features[1] = 1;
    features[2] = 1;
    features[5] = 0.2;
  } else if (name === "category-2") {
    features[0] = 0;
    features[1] = 0;
    features[2] = 1;
    features[5] = 0.2;
  } else if (name === "category-3") {
    features[0] = 1;
    features[1] = 1;
    features[2] = 1;
    features[5] = 3;
  } else if (name === "missing") {
    features[0] = Number.NaN;
    features[1] = Number.NaN;
    features[2] = Number.NaN;
    features[5] = 0.2;
  } else if (name === "float32-threshold") {
    features[0] = 0.5;
    features[1] = 1;
    features[2] = 1;
    features[5] = 0.2;
  } else {
    throw new Error(`unknown fixture ${name}`);
  }
  return features;
}

// encode one JavaScript number as its exact binary64 bits
function float64Bits(value) {
  const bytes = new ArrayBuffer(8);
  const view = new DataView(bytes);
  view.setFloat64(0, value, false);
  return view.getBigUint64(0, false).toString(16).padStart(16, "0");
}

const scoreSource = sourceSpan(
  "function scoreHead(head, features)",
  "// project the frozen amount",
);
const predictSource = sourceSpan(
  "function predictRainHurdleWindFeaturesWithArtifact(artifact, features, validAt)",
  "// apply one reviewed occurrence-head arm",
);
const result = vm.runInNewContext(
  `${scoreSource}\n${predictSource}\npredictRainHurdleWindFeaturesWithArtifact(artifact, features);`,
  {
    artifact: artifact(),
    features: featuresFor(process.argv[2]),
    RAIN_HURDLE_WIND_RUNTIME_V2_VERSION: "rain-hurdle-wind-runtime/v2",
    THRESHOLDS: [0.1, 1, 2.5],
  },
);
process.stdout.write([
  result.correctedPrecipitationMm,
  result.occurrenceProbabilityAtLeast0_1,
  result.occurrenceProbabilityAtLeast1_0,
  result.occurrenceProbabilityAtLeast2_5,
  result.positiveAmountMm,
].map(float64Bits).join("\t"));
