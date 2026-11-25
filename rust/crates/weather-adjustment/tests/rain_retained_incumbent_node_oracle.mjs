import {
  predictRainHurdleWindFeaturesWithProbabilities,
} from "../../../../packages/forecast-adjustment/dist/rain-hurdle-wind.js";

// encode one original-width float32 input without decimal conversion
function float32Bits(value) {
  const bytes = new ArrayBuffer(4);
  new DataView(bytes).setFloat32(0, value, false);
  return new DataView(bytes).getUint32(0, false).toString(16).padStart(8, "0");
}

// encode one evaluator readout without decimal conversion
function float64Bits(value) {
  const bytes = new ArrayBuffer(8);
  new DataView(bytes).setFloat64(0, value, false);
  return new DataView(bytes).getBigUint64(0, false).toString(16).padStart(16, "0");
}

// build representative full-width rows without replacing the deployed artifact
function fixture(name, rawAmount) {
  const features = new Float32Array(107);
  // vary every feature while retaining deterministic float32 inputs
  for (let index = 0; index < features.length; index += 1) {
    features[index] = Math.fround((((index * 37) + rawAmount * 10) % 101 - 50) / 10);
  }
  // exercise the original missing-value traversal on ordinary non-raw features
  if (name === "missing") {
    for (let index = 0; index < features.length; index += 7) {
      features[index] = Number.NaN;
    }
  }
  features[5] = Math.fround(rawAmount);
  return features;
}

const cases = [
  ["dry", 0],
  ["trace", 0.05],
  ["light", 0.25],
  ["moderate", 1.75],
  ["heavy", 8],
  ["raw-cap", 47],
  ["missing", 0.7],
];

// emit exact inputs and the five public incumbent readouts for Rust
for (const [name, rawAmount] of cases) {
  const features = fixture(name, rawAmount);
  const result = predictRainHurdleWindFeaturesWithProbabilities(features);
  const readouts = [
    result.correctedPrecipitationMm,
    result.occurrenceProbabilityAtLeast0_1,
    result.occurrenceProbabilityAtLeast1_0,
    result.occurrenceProbabilityAtLeast2_5,
    result.positiveAmountMm,
  ];
  console.log([
    name,
    Array.from(features, float32Bits).join(","),
    ...readouts.map(float64Bits),
  ].join("\t"));
}
