import { once } from "node:events";
import { pathToFileURL } from "node:url";

import {
  createWeatherApi,
  createWeatherApiServer,
} from "../apps/api/dist/index.js";
import { bindBackendPerformanceDirectSchedule } from
  "./backend-performance-direct-binding.mjs";
import { evaluateBackendPerformanceDirect } from
  "./backend-performance-direct-evaluator.mjs";
import { beginDisposableResourceInterval } from
  "./backend-performance-direct-pipeline-check.mjs";
import { buildBackendPerformanceFunctionalSchedule } from
  "./backend-performance-functional-schedule.mjs";

const REFERENCE_TIME = "2026-08-22T05:00:00.000Z";

// select the forty reviewed warm health attempts
function healthAttemptIds() {
  const attempts = [];
  const schedule = buildBackendPerformanceFunctionalSchedule();
  // visit the first source run slot
  for (const group of schedule.runSlots[0].groups) {
    // retain warm health attempts in source order
    for (const attempt of group.attempts) {
      // retain exactly the reviewed health workload
      if (attempt.operationId === "api.health" && attempt.stratum === "warm") {
        attempts.push(attempt.attemptId);
      }
    }
  }
  // require the source-owned warm cardinality
  if (attempts.length !== 40) {
    throw new Error("weather API fixture did not bind forty health attempts");
  }
  return attempts;
}

// classify one real Weather health response
function classifyWeatherHealth({ attempt, bodyBytes }) {
  const value = JSON.parse(bodyBytes.toString("utf8"));
  const completed = attempt.operationId === "api.health"
    && value.data?.database === "ready"
    && value.data?.live === true
    && value.data?.migration?.status === "current"
    && value.data?.ready === true
    && value.data?.worker?.freshness === "fresh";
  return {
    classification: completed
      ? "weather-api-health-ready-at-explicit-time"
      : "weather-api-health-invalid",
    completed,
  };
}

// close one disposable Weather API server
async function close(server) {
  server.close();
  await once(server, "close");
}

// run the existing Weather handler through the direct evaluator
export async function runBackendPerformanceDirectWeatherApiCheck() {
  let clockReads = 0;
  let healthReads = 0;
  const store = {
    // return the normal ready health fixture
    async getHealth() {
      healthReads += 1;
      return {
        database: "ready",
        migration: { status: "current", version: "0001_initial_weather.sql" },
        workerLastLoopAt: "2026-08-22T04:45:00.000Z",
      };
    },
  };
  const handler = createWeatherApi(store, {
    // supply one explicit request clock
    now() {
      clockReads += 1;
      return new Date(REFERENCE_TIME);
    },
    version: "direct-weather-api-fixture",
  });
  const setupClockReads = clockReads;
  const server = createWeatherApiServer(handler);
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  try {
    const address = server.address();
    // require one assigned loopback address
    if (address === null || typeof address === "string") {
      throw new Error("Weather API fixture listener has no TCP address");
    }
    const binding = bindBackendPerformanceDirectSchedule({
      attemptIds: healthAttemptIds(),
      baseUrl: `http://127.0.0.1:${String(address.port)}`,
      generation: "node-v1",
      maximumResponseBytes: 64 * 1024,
      resourceInterval: { scopeId: "disposable-weather-api-process" },
      role: "baseline",
      runId: "weather-api-handler-fixture",
    });
    const result = await evaluateBackendPerformanceDirect(binding.directConfiguration, {
      beginResourceInterval: beginDisposableResourceInterval,
      classifyResponse: classifyWeatherHealth,
    });
    return {
      attemptDenominator: result.summary.durationCoverage,
      classifications: [...new Set(result.attempts.map(
        // retain one terminal semantic classification
        (attempt) => attempt.semanticClassification,
      ))],
      handlerClockReads: clockReads - setupClockReads,
      healthReads,
      resourceEvidence: {
        contractVersion: result.resourceEvidence.contractVersion,
        nativeEligible: result.resourceEvidence.authority.nativeEligible,
        sampleCount: result.resourceEvidence.sampleCount,
      },
      terminalCounts: result.summary.terminalCounts,
    };
  } finally {
    await close(server);
  }
}

// print only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runBackendPerformanceDirectWeatherApiCheck().then(
    // emit one machine-readable handler result
    (result) => process.stdout.write(`${JSON.stringify(result, null, 2)}\n`),
    // emit one controlled handler failure
    (error) => {
      process.stderr.write(`${error?.code ?? "direct-weather-api.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    },
  );
}
