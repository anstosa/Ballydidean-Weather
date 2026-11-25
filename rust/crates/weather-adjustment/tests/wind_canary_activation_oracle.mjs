import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const windSource = readFileSync(
  resolve(repositoryRoot, "packages/forecast-adjustment/dist/wind-canary.js"),
  "utf8",
);
const candidateSource = readFileSync(
  resolve(repositoryRoot, "packages/forecast-adjustment/dist/candidate.js"),
  "utf8",
);
const domainSource = readFileSync(
  resolve(repositoryRoot, "packages/domain/dist/forecast-adjustment.js"),
  "utf8",
);

// extract one compiled source span between stable neighboring comments
function sourceSpan(source, startMarker, endMarker) {
  const start = source.indexOf(startMarker);
  const end = source.indexOf(endMarker, start);
  // fail if either compiled production declaration moved
  if (start < 0 || end < 0) {
    throw new Error(`unable to extract ${startMarker}`);
  }
  return source.slice(start, end).replaceAll("export ", "");
}

const activeSource = sourceSpan(
  windSource,
  "// report whether the current instant is authorized after activation",
  "// interpret only the literal one-way disable value",
);
const killedSource = sourceSpan(
  windSource,
  "// interpret only the literal one-way disable value",
  "// validate exact canonical source and station lineage",
);
const metricBandKeySource = sourceSpan(
  candidateSource,
  "// identify one metric-band pair",
  "// sort coefficient cells deterministically",
);
const windMetricsSource = sourceSpan(
  domainSource,
  "// freeze the canary-only metric allowlist",
  "// freeze lexically sortable lead-band keys",
);

const killSwitch = process.argv[2] === "undefined" ? undefined : process.argv[2];
const runtimeName = process.argv[3];
const evaluatedAtEpochMs = Number(process.argv[4]);
const metric = process.argv[5];
const leadBand = process.argv[6];
const activatedAtEpochMs = runtimeName === "permanent"
  ? 1_790_872_400_936
  : 1_788_418_800_000;
const bundle = {
  authorization: {
    activatedAt: new Date(activatedAtEpochMs).toISOString(),
    expiresAt: runtimeName === "permanent"
      ? null
      : new Date(1_789_023_600_000).toISOString(),
  },
  candidate: {
    enabledMetricBands: [
      { leadBand: "001-024", metric: "windGustMps" },
      { leadBand: "001-024", metric: "windSpeedMps" },
    ],
  },
  contractVersion: runtimeName === "permanent"
    ? "forecast-adjustment-wind-canary-runtime-bundle/v2"
    : "forecast-adjustment-wind-canary-runtime-bundle/v1",
};

const result = vm.runInNewContext(
  `${activeSource}\n${killedSource}\n${metricBandKeySource}\n${windMetricsSource}
   if (forecastAdjustmentWindCanaryIsKilled(killSwitch)) {
     "disabled:canary_killed";
   } else if (runtimeName === "inactive") {
     "legacy_fallback";
   } else if (!forecastAdjustmentWindCanaryIsActiveAt(
     bundle,
     new Date(evaluatedAtEpochMs).toISOString(),
   )) {
     "disabled:canary_expired";
   } else {
     const enabledKeys = new Set(bundle.candidate.enabledMetricBands.map(metricBandKey));
     const enabled = FORECAST_ADJUSTMENT_WIND_CANARY_METRICS.includes(metric) &&
       enabledKeys.has(metricBandKey({ leadBand, metric }));
     enabled ? "active:enabled" : "active:not_enabled";
   }`,
  {
    bundle,
    evaluatedAtEpochMs,
    FORECAST_ADJUSTMENT_WIND_CANARY_RUNTIME_BUNDLE_CONTRACT_VERSION_V2:
      "forecast-adjustment-wind-canary-runtime-bundle/v2",
    killSwitch,
    leadBand,
    metric,
    runtimeName,
    validateUtcInstant: (value) => value,
    verifyForecastAdjustmentWindCanaryRuntimeBundle: () => undefined,
  },
);
process.stdout.write(result);
