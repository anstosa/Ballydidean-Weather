import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

import { localCalendarFeaturesFor } from "../../../../packages/forecast-adjustment/dist/calendar.js";
import {
  TEMPERATURE_MOS_DELAYED_RUNTIME_POLICY,
  TEMPERATURE_MOS_RUNTIME_POLICY,
  applyEcmwfTemperatureMosRuntime,
} from "../../../../packages/forecast-adjustment/dist/temperature-mos-runtime.js";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const source = readFileSync(
  resolve(repositoryRoot, "packages/forecast-adjustment/dist/temperature-mos-runtime.js"),
  "utf8",
);

const DIRECT_COEFFICIENTS = Array.from(
  { length: 35 },
  (_unused, index) => ((index % 7) - 3) * 0.017,
);
const ADAPTIVE_COEFFICIENTS = Array.from(
  { length: 49 },
  (_unused, index) => ((index % 9) - 4) * 0.011,
);
const TRAINING_CUTOFF = "2026-08-25T07:00:00.000Z";

// extract one compiled source span between stable owning comments
function sourceSpan(startMarker, endMarker) {
  const start = source.indexOf(startMarker);
  const end = source.indexOf(endMarker, start);
  // fail when the compiled feature implementation moves
  if (start < 0 || end < 0) {
    throw new Error(`unable to extract ${startMarker}`);
  }
  return source.slice(start, end);
}

// encode one JavaScript number as exact binary64 bits
function float64Bits(value) {
  const bytes = new ArrayBuffer(8);
  const view = new DataView(bytes);
  view.setFloat64(0, value, false);
  return view.getBigUint64(0, false).toString(16).padStart(16, "0");
}

// create one supported monthly model fixture
function createModel(overrides = {}) {
  return {
    adaptiveCoefficients: [...ADAPTIVE_COEFFICIENTS],
    cohort: "ecmwf_single_run_hindcast",
    contractVersion: "temperature-shortlead-models-research/v1",
    directCoefficients: [...DIRECT_COEFFICIENTS],
    learnedStrengthContractVersion: "temperature-winner-extensions-research/v1",
    month: "2026-09",
    scope: "initialization_first12",
    strengthBands: {
      "1-6": { alpha: 1, supported: true, trainingCutoffUtc: TRAINING_CUTOFF },
      "7-12": { alpha: 0.8, supported: true, trainingCutoffUtc: TRAINING_CUTOFF },
    },
    supported: true,
    trainingCutoffUtc: TRAINING_CUTOFF,
    ...overrides,
  };
}

// create one causally available permanent model fixture
function createPermanentModel(overrides = {}) {
  const monthly = createModel();
  return {
    adaptiveCoefficients: monthly.adaptiveCoefficients,
    cohort: monthly.cohort,
    contractVersion: "temperature-permanent-model/v1",
    directCoefficients: monthly.directCoefficients,
    effectiveFrom: TRAINING_CUTOFF,
    latestTrainingValidAt: "2026-08-25T00:00:00.000Z",
    learnedStrengthContractVersion: monthly.learnedStrengthContractVersion,
    scope: monthly.scope,
    strengthBands: monthly.strengthBands,
    supported: monthly.supported,
    trainingCutoffUtc: monthly.trainingCutoffUtc,
    ...overrides,
  };
}

// create one exact recent-error receipt
function createState(supported = true) {
  return {
    b24C: supported ? 0.8 : null,
    b72C: supported ? 0.3 : null,
    cohort: "ecmwf_single_run_hindcast",
    localDates: supported ? 3 : 0,
    mad72C: supported ? 0.4 : null,
    maximumSourceRunInitializedAt: supported ? "2026-09-06T12:00:00.000Z" : null,
    maximumSourceValidAt: supported ? "2026-09-07T05:00:00.000Z" : null,
    n24: supported ? 12 : 0,
    n72: supported ? 50 : 0,
    sourceKeys: supported
      ? Array.from({ length: 50 }, (_unused, index) => `source-${index}`)
      : [],
    supported,
    targetRunInitializedAt: "2026-09-07T12:00:00.000Z",
    windowEndValidAt: "2026-09-07T05:00:00.000Z",
  };
}

// create one initialization-relative forecast fixture
function createForecast(lead = 5, rawTemperatureC = 16.4) {
  const hour = 12 + lead;
  const validDate = hour < 24 ? "2026-09-07" : "2026-09-08";
  return {
    cohort: "ecmwf_single_run_hindcast",
    key: `fixture-${lead}`,
    modelCycle: "50r1",
    modelLeadHours: lead,
    rawRelativeHumidityPercent: 78,
    rawTemperatureC,
    rawWindSpeedMps: 3.2,
    runInitializedAt: "2026-09-07T12:00:00.000Z",
    validAt: `${validDate}T${String(hour % 24).padStart(2, "0")}:00:00.000Z`,
  };
}

// specialize one actual runtime fixture without changing its unrelated identities
function fixture(name) {
  let forecast = createForecast();
  let model = createModel();
  let state = createState(true);
  // select the one numerical boundary named by each case
  if (name === "adaptive-second") {
    forecast = createForecast(10);
  } else if (name === "direct") {
    state = createState(false);
  } else if (name === "delayed-first") {
    forecast = createForecast(7);
    model = createModel({ scope: "assumed_delay6_next12" });
  } else if (name === "delayed-second") {
    forecast = createForecast(13);
    model = createModel({ scope: "assumed_delay6_next12" });
  } else if (name === "permanent") {
    model = createPermanentModel();
  } else if (name === "cap-positive") {
    forecast = createForecast(5, 69);
    state = createState(false);
    const coefficients = Array(35).fill(0);
    coefficients[0] = 100;
    model = createModel({ directCoefficients: coefficients });
  } else if (name === "cap-negative") {
    forecast = createForecast(5, -99);
    state = createState(false);
    const coefficients = Array(35).fill(0);
    coefficients[0] = -100;
    model = createModel({ directCoefficients: coefficients });
  } else if (name === "lead-initialization") {
    forecast = createForecast(13);
  } else if (name === "lead-delayed") {
    forecast = createForecast(6);
    model = createModel({ scope: "assumed_delay6_next12" });
  } else if (name === "model-unsupported") {
    model = createModel({ supported: false });
  } else if (name === "strength-unsupported") {
    model = createModel({
      strengthBands: {
        ...createModel().strengthBands,
        "1-6": { ...createModel().strengthBands["1-6"], supported: false },
      },
    });
  } else if (name === "missing-direct") {
    state = createState(false);
    model = createModel({ directCoefficients: null });
  } else if (name === "missing-adaptive") {
    model = createModel({ adaptiveCoefficients: null });
  } else if (name === "invalid-alpha") {
    model = createModel({
      strengthBands: {
        ...createModel().strengthBands,
        "1-6": { ...createModel().strengthBands["1-6"], alpha: 0.75 },
      },
    });
  } else if (name === "nonfinite-coefficient") {
    const coefficients = [...ADAPTIVE_COEFFICIENTS];
    coefficients[4] = Number.NaN;
    model = createModel({ adaptiveCoefficients: coefficients });
  } else if (name === "raw-nonfinite") {
    forecast = createForecast(5, Number.NaN);
  } else if (name === "weather-missing") {
    forecast.rawRelativeHumidityPercent = null;
    forecast.rawWindSpeedMps = null;
  } else if (name === "model-49-feature") {
    forecast.modelCycle = "49r1";
  } else if (name === "cycle-zero-feature") {
    forecast.runInitializedAt = "2026-09-07T00:00:00.000Z";
    forecast.validAt = "2026-09-07T05:00:00.000Z";
  } else if (name !== "adaptive-first") {
    throw new Error(`unknown fixture ${name}`);
  }
  return { forecast, model, state };
}

// derive the actual compiled normalized feature arrays
function featureVectors(forecast, state, scope) {
  const calendarSource = sourceSpan(
    "// determine gregorian leap-year length",
    "// validate the initialization-relative forecast identity",
  );
  const featureSource = sourceSpan(
    "// derive the exact frozen seasonal feature vector",
    "// calculate one deterministic coefficient dot product",
  );
  return vm.runInNewContext(
    `${calendarSource}\n${featureSource}\n({` +
      "direct: staticFeatures(forecast)," +
      "adaptive: state.supported ? adaptiveFeatures(forecast, state, scope) : null," +
      "metadata: (() => {" +
      "const direct = staticFeatures(forecast);" +
      "return [...direct.slice(1, 9), direct[32], direct[33]];" +
      "})()" +
      "});",
    {
      fail: (reason) => { throw new Error(reason); },
      forecast,
      localCalendarFeaturesFor,
      MILLISECONDS_PER_HOUR: 3_600_000,
      scope,
      state,
      TEMPERATURE_MOS_DELAYED_RUNTIME_POLICY,
    },
  );
}

// render exact public result values without decimal tolerance
function resultSignature(result) {
  // retain raw reason and bit-identical provider value
  if (!result.applied) {
    return `raw:${result.reason}:${float64Bits(result.rawTemperatureC)}`;
  }
  return [
    "applied",
    result.branch,
    float64Bits(result.alpha),
    float64Bits(result.rawTemperatureC),
    float64Bits(result.unscaledCorrectionC),
    float64Bits(result.incumbentHalfStrengthTemperatureC),
    float64Bits(result.predictionTemperatureC),
  ].join(":");
}

const selected = fixture(process.argv[2]);
let features = null;
// keep invalid-forecast cases focused on the runtime fallback
try {
  features = featureVectors(selected.forecast, selected.state, selected.model.scope);
} catch {
  features = { adaptive: null, direct: null, metadata: null };
}
const result = applyEcmwfTemperatureMosRuntime({
  forecast: selected.forecast,
  model: selected.model,
  recentErrorState: selected.state,
});
const encodeFeatures = (values) => values === null
  ? "-"
  : values.map(float64Bits).join(",");
process.stdout.write([
  encodeFeatures(features.direct),
  encodeFeatures(features.adaptive),
  resultSignature(result),
  encodeFeatures(features.metadata),
].join("\t"));
