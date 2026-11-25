import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

import {
  createWeatherApi,
  createWeatherApiServer,
} from "../apps/api/dist/index.js";
import { verifyForecastAdjustmentWindCanaryRuntimeBundle } from
  "../packages/forecast-adjustment/dist/index.js";
import {
  BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
  evaluateBackendPerformanceDirect,
} from "./backend-performance-direct-evaluator.mjs";
import { beginDisposableResourceInterval } from
  "./backend-performance-direct-pipeline-check.mjs";
import {
  BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
  BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";

const WORKLOAD_URL = new URL("../config/backend-performance-workload-v2.json", import.meta.url);
const WORKLOAD_SHA256 = "b44eb296f1145a55202e941e24e750c0b0d5a4c37075c6f8e9ca346470bca53f";
const WIND_BUNDLE_URL = new URL(
  "../config/forecast-adjustments/ballydidean/wind-canary-bundles/sha256-5e8b2e3932111621af6785a1b16dfd22edc0a2d26059c6e396654c70119abbe1.json",
  import.meta.url,
);
const WIND_BUNDLE_SHA256 = "5e8b2e3932111621af6785a1b16dfd22edc0a2d26059c6e396654c70119abbe1";
const DATA_AT = "2026-09-09T05:00:00.000Z";
const SAFETY_AT = "2026-09-09T06:00:00.000Z";
const PREPARATION_CLASS =
  "verified-immutable-wind-runtime-and-disposable-in-process-listener";

// create one controlled model-fixture error
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one model-fixture invariant
function requireInvariant(condition, code, message) {
  // fail closed on source or model drift
  if (!condition) {
    fail(code, message);
  }
}

// bind the reviewed wind load-serve attempts and executable identity
function loadSourceBinding() {
  const workloadBytes = readFileSync(WORKLOAD_URL);
  requireInvariant(
    createHash("sha256").update(workloadBytes).digest("hex") === WORKLOAD_SHA256,
    "direct-weather-model.workload",
    "backend performance workload identity changed",
  );
  const workload = JSON.parse(workloadBytes.toString("utf8"));
  const operation = workload.operations.find(
    // select the reviewed wind inference operation
    (value) => value.id === "inference.wind.load-serve",
  );
  requireInvariant(
    operation !== undefined
      && JSON.stringify(operation.exactHttp) === JSON.stringify({
        method: "GET",
        target: "/api/v1/sites/ballydidean/forecast",
        headers: {},
        body: null,
      })
      && JSON.stringify(operation.nodeArgv) === JSON.stringify([{
        phase: "start-api",
        argv: ["node", "apps/api/dist/main.js"],
      }])
      && operation.stateEffects === "read-only-model-load+database-read",
    "direct-weather-model.operation",
    "wind load-serve source operation changed",
  );
  const attempts = [];
  const schedule = buildBackendPerformanceFunctionalSchedule();
  // visit every reviewed source run slot
  for (const runSlot of schedule.runSlots) {
    const selected = [];
    // visit every source group
    for (const group of runSlot.groups) {
      // select the exact wind load-serve attempt
      for (const attempt of group.attempts) {
        // retain one once-per-run source attempt
        if (attempt.operationId === operation.id) {
          selected.push(attempt);
        }
      }
    }
    requireInvariant(
      selected.length === 1 && selected[0].stratum === "once-per-run",
      "direct-weather-model.schedule",
      "each run slot must contain one wind load-serve attempt",
    );
    attempts.push(selected[0]);
  }
  requireInvariant(
    attempts.length === 5,
    "direct-weather-model.schedule",
    "wind load-serve fixture requires all five reviewed run slots",
  );
  return { attempts, operation };
}

// create one exact forecast source description
function createSiteRow(identity) {
  return {
    attributionLabel: "Weather data by Open-Meteo",
    attributionUrl: "https://open-meteo.com/",
    latitude: 47.950429954185445,
    longitude: -122.42797012608193,
    providerKey: "open-meteo",
    providerName: "Open-Meteo",
    siteName: "Ballydidean",
    siteSlug: "ballydidean",
    sourceId: "12",
    sourceKey: identity.sourceKey,
    sourceKind: "forecast",
    stationKind: "virtual",
    stationLatitude: 47.950429954185445,
    stationLongitude: -122.42797012608193,
    stationName: "Open-Meteo virtual station",
    stationSlug: "open-meteo-virtual",
    timezone: "America/Los_Angeles",
  };
}

// create one candidate-identity forecast row
function createForecastRow(identity) {
  return {
    adapterVersion: identity.adapterVersion,
    apparentTemperatureC: 11.7,
    blackGlobeTemperatureC: null,
    cloudCoverPercent: 72,
    contractEpoch: identity.contractEpoch,
    deviceModel: null,
    deviceSerial: null,
    deviceVendor: null,
    firstReceivedAt: "2026-09-09T05:01:00.000Z",
    id: "direct-wind-model-row",
    lastReceivedAt: "2026-09-09T05:01:00.000Z",
    pm25MicrogramsPerCubicMeter: 5,
    precipitationMm: 0.1,
    precipitationRateMmPerHour: 0.1,
    pressureHpa: 1018.1,
    productRunAt: DATA_AT,
    providerKey: "open-meteo",
    providerMetadata: {
      dataset: identity.dataset,
      elevation_m: 32,
      grid_cell: "47.941,-122.438",
    },
    qualityMetadata: null,
    relativeHumidityPercent: 83,
    revisionCount: 0,
    siteSlug: "ballydidean",
    sourceConfigFingerprint: identity.sourceConfigFingerprint,
    sourceId: "12",
    sourceKey: identity.sourceKey,
    sourceKind: "forecast",
    stationSlug: "open-meteo-virtual",
    soilElectricalConductivityMicrosiemensPerCm: null,
    soilMoisturePercent: null,
    solarRadiationWm2: null,
    temperatureC: 12.1,
    upstreamModel: identity.upstreamModel,
    upstreamTimezone: "GMT",
    uvIndex: 0.2,
    validAt: SAFETY_AT,
    windDirectionDegrees: 180,
    windGustMps: 8.1,
    windSpeedMps: 5.8,
    wetBulbGlobeTemperatureC: null,
  };
}

// listen on one disposable loopback port
function listen(server) {
  return new Promise((resolve, reject) => {
    const onError = (error) => {
      server.off("listening", onListening);
      reject(error);
    };
    const onListening = () => {
      server.off("error", onError);
      const address = server.address();
      // require one assigned loopback address
      if (address === null || typeof address === "string") {
        reject(new Error("model fixture listener has no TCP address"));
        return;
      }
      resolve(`http://127.0.0.1:${String(address.port)}`);
    };
    server.once("error", onError);
    server.once("listening", onListening);
    server.listen(0, "127.0.0.1");
  });
}

// close one disposable listener
function close(server) {
  return new Promise((resolve, reject) => {
    server.close((error) => {
      // reject one close failure
      if (error !== undefined) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}

// classify one actual adjusted forecast response
function classifyWindModelResponse({ attempt, bodyBytes }) {
  const body = JSON.parse(bodyBytes.toString("utf8"));
  const record = body.data?.[0];
  const completed = attempt.operationId === "inference.wind.load-serve"
    && body.generatedAt === DATA_AT
    && body.adjustmentRuntime?.activeBundle === WIND_BUNDLE_SHA256
    && body.adjustmentRuntime?.state === "active"
    && record?.adjustment?.state === "active"
    && JSON.stringify(record.adjustment.appliedMetrics)
      === JSON.stringify(["windGustMps", "windSpeedMps"])
    && record.adjustment.adjustedMetrics.windGustMps !== record.metrics.windGustMps
    && record.adjustment.adjustedMetrics.windSpeedMps !== record.metrics.windSpeedMps;
  return {
    classification: completed
      ? "compiled-weather-api-applied-verified-wind-canary"
      : "compiled-weather-api-wind-model-invalid",
    completed,
  };
}

// execute five reviewed wind model reads through the compiled Weather handler
export async function runBackendPerformanceDirectWeatherModelCheck() {
  const source = loadSourceBinding();
  const sourceAttemptIds = source.attempts.map(
    // retain one exact source identity
    (attempt) => attempt.attemptId,
  );
  let clockReads = 0;
  let cleanupCount = 0;
  let forecastReads = 0;
  let preparationCount = 0;
  let pressureContextReads = 0;
  let runtimeVerificationCount = 0;
  let siteReads = 0;
  const result = await evaluateBackendPerformanceDirect({
    attempts: source.attempts.map(
      // bind one exact source attempt to its reviewed route
      (attempt) => ({
        attemptId: attempt.attemptId,
        body: null,
        expectedStatuses: [200],
        headers: {},
        method: source.operation.exactHttp.method,
        operationId: attempt.operationId,
        path: source.operation.exactHttp.target,
        runSlotId: attempt.runSlotId,
        stratum: attempt.stratum,
        timeoutMs: attempt.deadlineMs,
      }),
    ),
    baseUrl: "http://127.0.0.1:9",
    contractVersion: BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
    generation: "node-v1",
    maximumResponseBytes: 256 * 1024,
    resourceInterval: { scopeId: "weather-wind-model-disposable-fixture" },
    role: "baseline",
    runId: "weather-api-wind-model-fixture",
  }, {
    beginResourceInterval: beginDisposableResourceInterval,
    classifyResponse: classifyWindModelResponse,
    // close every independently prepared listener
    async cleanupAttempt({ preparation }) {
      await close(preparation.server);
      cleanupCount += 1;
    },
    // load and verify the immutable model before each source attempt
    async prepareAttempt() {
      const bundle = JSON.parse(readFileSync(WIND_BUNDLE_URL, "utf8"));
      verifyForecastAdjustmentWindCanaryRuntimeBundle(bundle);
      runtimeVerificationCount += 1;
      requireInvariant(
        bundle.bundleSha256 === WIND_BUNDLE_SHA256,
        "direct-weather-model.bundle",
        "verified wind bundle identity changed",
      );
      const identity = bundle.candidate.servedForecastIdentity;
      const store = {
        // return one exact candidate-identity forecast row
        async getForecast() {
          forecastReads += 1;
          return [createForecastRow(identity)];
        },
        // return no optional retained pressure vintage
        async getForecastPressureContext() {
          pressureContextReads += 1;
          return [];
        },
        // return the exact forecast source description
        async listSites() {
          siteReads += 1;
          return [createSiteRow(identity)];
        },
      };
      const handler = createWeatherApi(store, {
        dataAt: DATA_AT,
        forecastAdjustment: {
          loadedAt: bundle.authorization.activatedAt,
          runtime: { bundle, reasonCode: null, state: "active" },
        },
        // supply one explicit request safety clock
        now() {
          clockReads += 1;
          return new Date(SAFETY_AT);
        },
        temperatureAdjustment: {
          loadedAt: SAFETY_AT,
          runtime: { bundle: null, reasonCode: "registry_inactive", state: "disabled" },
        },
        version: "direct-weather-wind-model-fixture",
      });
      const server = createWeatherApiServer(handler);
      try {
        const baseUrl = await listen(server);
        preparationCount += 1;
        return { baseUrl, classification: PREPARATION_CLASS, server };
      } catch (error) {
        // close a listener that became reachable before preparation failed
        if (server.listening) {
          await close(server);
        }
        throw error;
      }
    },
  });
  return {
    attemptDenominator: result.summary.durationCoverage,
    evidenceClass: "synthetic-fixture",
    execution: {
      classification: "compiled-weather-api-handler-with-verified-immutable-wind-runtime",
      cleanupCount,
      clockReads,
      forecastReads,
      preparationCount,
      pressureContextReads,
      runtimeVerificationCount,
      siteReads,
    },
    nativeEligible: false,
    operationStratum: result.summary.operationStrata[0],
    preparationClassifications: [...new Set(result.attempts.map(
      // retain every explicit preparation classification
      (attempt) => attempt.preparationClassification,
    ))],
    resourceEvidence: {
      contractVersion: result.resourceEvidence.contractVersion,
      kind: result.resourceEvidence.authority.kind,
      nativeEligible: result.resourceEvidence.authority.nativeEligible,
      sampleCount: result.resourceEvidence.sampleCount,
    },
    semanticClassifications: [...new Set(result.attempts.map(
      // retain every terminal semantic classification
      (attempt) => attempt.semanticClassification,
    ))],
    sourceBinding: {
      functionalDefinitionSha256: BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
      functionalScheduleVersion: BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
      nodeArgv: source.operation.nodeArgv,
      operationId: source.operation.id,
      sourceAttemptIds,
      workloadSha256: WORKLOAD_SHA256,
    },
    terminalCounts: result.summary.terminalCounts,
    unsupportedBoundaries: [
      "apps/api/dist/main.js process startup is not executed",
      "the injectable store replaces the PostgreSQL read adapter",
      "the immutable bundle is read and verified directly without registry selection",
    ],
  };
}

// print only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runBackendPerformanceDirectWeatherModelCheck().then(
    // emit one machine-readable model result
    (result) => process.stdout.write(`${JSON.stringify(result, null, 2)}\n`),
    // emit one controlled model failure
    (error) => {
      process.stderr.write(`${error?.code ?? "direct-weather-model.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    },
  );
}
