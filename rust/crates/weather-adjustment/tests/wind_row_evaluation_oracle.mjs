import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

import {
  applyCoreAdjustment,
  calendarFingerprintEquals,
  selectHierarchyCoefficient,
} from "../../../../packages/forecast-adjustment/dist/algorithm-v1.js";
import {
  FORECAST_ADJUSTMENT_METRICS,
  FORECAST_ADJUSTMENT_WIND_CANARY_METRICS,
  createForecastAdjustmentFailRawDecision,
  forecastLeadBandFor,
} from "../../../../packages/domain/dist/forecast-adjustment.js";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const source = readFileSync(
  resolve(repositoryRoot, "packages/forecast-adjustment/dist/apply.js"),
  "utf8",
);

// extract one compiled source span between stable owning comments
function sourceSpan(startMarker, endMarker = null) {
  const start = source.indexOf(startMarker);
  const end = endMarker === null ? source.length : source.indexOf(endMarker, start);
  // fail when the compiled production declaration moves
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

// map one representative lead to its candidate band
function candidateLeadBand(targetLeadHours) {
  // keep unsupported fixtures buildable while runtime rejects them first
  try {
    return forecastLeadBandFor(targetLeadHours);
  } catch {
    return "001-024";
  }
}

// build one source-shaped coefficient cell
function coefficient(metric, leadBand, level, value, calendar = {}) {
  return {
    coefficient: value,
    daypart: calendar.daypart ?? null,
    effectiveEventCount: 100,
    leadBand,
    level,
    metric,
    month: calendar.month ?? null,
    season: calendar.season ?? null,
  };
}

// build one compact admitted-row fixture
function fixture(name) {
  let targetLeadHours = name === "lead-168" ? 168 : 24;
  // select explicit unsupported lead fixtures
  if (name === "unsupported-low") {
    targetLeadHours = 0;
  } else if (name === "unsupported-high") {
    targetLeadHours = 169;
  }
  const leadBand = candidateLeadBand(targetLeadHours);
  const calendar = { daypart: "afternoon", month: 9, season: "autumn" };
  const metrics = {
    windDirectionDegrees: 350,
    windGustMps: 10,
    windSpeedMps: 5,
  };
  const candidate = {
    algorithmContractVersion: "robust-hierarchical-median/v1",
    candidateArtifactSha256: "candidate-sha",
    coefficients: [
      coefficient("windDirectionDegrees", leadBand, 1, 30),
      coefficient("windGustMps", leadBand, 1, 2),
      coefficient("windSpeedMps", leadBand, 1, 1),
      coefficient("windSpeedMps", leadBand, 2, 2, {
        daypart: "afternoon",
        season: "autumn",
      }),
      coefficient("windSpeedMps", leadBand, 3, 3, {
        daypart: "afternoon",
        month: 9,
      }),
    ],
    enabledMetricBands: [
      { leadBand, metric: "windDirectionDegrees" },
      { leadBand, metric: "windGustMps" },
      { leadBand, metric: "windSpeedMps" },
    ],
    runtimeFingerprint: {},
    servedForecastIdentity: {},
    trainingEnvelopes: [
      { leadBand, maximum: 30, metric: "windGustMps", minimum: 0 },
      { leadBand, maximum: 30, metric: "windSpeedMps", minimum: 0 },
    ],
  };
  let fingerprintMatches = true;
  let identityMatches = true;
  let cohort = "legacy_v4_retrieval_snapshot";

  // specialize only the row dimension exercised by each fixture
  if (name === "season") {
    metrics.windDirectionDegrees = null;
    metrics.windGustMps = null;
    calendar.month = 8;
  } else if (name === "root") {
    metrics.windDirectionDegrees = null;
    metrics.windGustMps = null;
    calendar.daypart = "evening";
  } else if (name === "identity-mismatch") {
    identityMatches = false;
  } else if (name === "wrong-cohort") {
    identityMatches = false;
    cohort = "other";
  } else if (name === "all-null") {
    metrics.windDirectionDegrees = null;
    metrics.windGustMps = null;
    metrics.windSpeedMps = null;
  } else if (name === "fingerprint") {
    metrics.windDirectionDegrees = null;
    metrics.windGustMps = null;
    fingerprintMatches = false;
  } else if (name === "direction-calm") {
    metrics.windDirectionDegrees = 10;
    metrics.windGustMps = null;
    metrics.windSpeedMps = 0.5;
    candidate.enabledMetricBands = [
      { leadBand, metric: "windDirectionDegrees" },
    ];
  } else if (name === "envelope") {
    metrics.windDirectionDegrees = null;
    metrics.windGustMps = 31;
    metrics.windSpeedMps = null;
  } else if (name === "coefficient-missing") {
    metrics.windDirectionDegrees = null;
    metrics.windGustMps = null;
    candidate.coefficients = candidate.coefficients.filter(
      (entry) => entry.metric !== "windSpeedMps",
    );
  } else if (name === "metric-disabled") {
    metrics.windDirectionDegrees = null;
    metrics.windGustMps = null;
    candidate.enabledMetricBands = [];
  } else if (name === "metric-out-of-bounds") {
    metrics.windDirectionDegrees = null;
    metrics.windGustMps = null;
    metrics.windSpeedMps = -1;
  } else if (name === "partial") {
    metrics.windDirectionDegrees = null;
    metrics.windGustMps = 31;
  }

  return {
    calendar,
    candidate,
    fingerprintMatches,
    identityMatches,
    input: {
      metrics,
      rawForecastProvenance: {
        cohort,
        targetLeadHours,
      },
      runtimeFingerprint: {},
    },
    runtime: {
      bundle: {
        artifactKind: "wind_transfer_canary_runtime",
        authorization: { authorizationSha256: "authorization-sha" },
        candidate,
        transferReport: { transferReportSha256: "transfer-sha" },
      },
      reasonCode: null,
      state: "active",
    },
  };
}

// render the exact contract state and adjusted binary values
function resultSignature(result) {
  // keep fail-raw signatures focused on the public reason
  if (result.state !== "active") {
    return `fail:${result.reasonCode}`;
  }
  const metrics = result.appliedMetrics.map(
    (metric) => `${metric}=${float64Bits(result.adjustedMetrics[metric])}`,
  );
  return [
    "active",
    result.leadBand,
    metrics.join(","),
    result.authorizationSha256,
    result.candidateArtifactSha256,
    result.transferReportSha256,
  ].join(":");
}

const selected = fixture(process.argv[2]);
const runtimeSource = sourceSpan(
  "// share numerical evaluation while keeping activation checks explicit",
  "// compare every immutable served forecast identity",
);
const mapFailureSource = sourceSpan(
  "// map pure-core reasons to the bounded public decision contract",
);
const result = vm.runInNewContext(
  `${runtimeSource}\n${mapFailureSource}\n` +
    "applyForecastAdjustmentRuntime(runtime, input, false);",
  {
    applyCoreAdjustment,
    calendarFingerprintEquals: () => selected.fingerprintMatches,
    createForecastAdjustmentFailRawDecision,
    deepFreeze: (value) => value,
    FORECAST_ADJUSTMENT_METRICS,
    FORECAST_ADJUSTMENT_WIND_CANARY_METRICS,
    FORECAST_ADJUSTMENT_WIND_CANARY_RUNTIME_BUNDLE_CONTRACT_VERSION_V2:
      "forecast-adjustment-wind-canary-runtime-bundle/v2",
    forecastIdentityMatches: () => selected.identityMatches,
    forecastLeadBandFor,
    input: selected.input,
    localCalendarFeaturesFor: () => selected.calendar,
    metricBandKey: ({ leadBand, metric }) => `${metric}:${leadBand}`,
    runtime: selected.runtime,
    runtimeCalendarFingerprint: () => ({}),
    selectHierarchyCoefficient,
    validateForecastAdjustmentActiveDecision: () => undefined,
    validateForecastAdjustmentWindCanaryActiveDecision: () => undefined,
  },
);
process.stdout.write(resultSignature(result));
