import { fork } from "node:child_process";
import { pathToFileURL } from "node:url";

import { bindBackendPerformanceDirectSchedule } from
  "./backend-performance-direct-binding.mjs";
import {
  BACKEND_PERFORMANCE_DIRECT_PROCESS_COLD_PREPARATION,
  evaluateBackendPerformanceDirect,
} from "./backend-performance-direct-evaluator.mjs";
import { buildBackendPerformanceFunctionalSchedule } from
  "./backend-performance-functional-schedule.mjs";

const CHILD_URL = new URL("./backend-performance-direct-weather-api-child.mjs", import.meta.url);
const CHILD_START_TIMEOUT_MS = 5_000;
const CHILD_STOP_TIMEOUT_MS = 5_000;

// create one controlled cold-fixture error
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one cold-fixture invariant
function requireInvariant(condition, code, message) {
  // fail closed on lifecycle drift
  if (!condition) {
    fail(code, message);
  }
}

// select one reviewed cold health attempt per run slot
function processColdHealthAttemptIds() {
  const schedule = buildBackendPerformanceFunctionalSchedule();
  const attempts = [];
  // visit every reviewed source run slot
  for (const runSlot of schedule.runSlots) {
    const slotAttempts = [];
    // visit every source group
    for (const group of runSlot.groups) {
      // retain the exact health process-cold attempt
      for (const attempt of group.attempts) {
        // select only the reviewed process-pool stratum
        if (attempt.operationId === "api.health"
          && attempt.stratum === "process-pool-cold") {
          slotAttempts.push(attempt.attemptId);
        }
      }
    }
    requireInvariant(
      slotAttempts.length === 1,
      "direct-weather-api-cold.schedule",
      `${runSlot.runSlotId} does not contain one health process-cold attempt`,
    );
    attempts.push(slotAttempts[0]);
  }
  requireInvariant(
    attempts.length === 5,
    "direct-weather-api-cold.schedule",
    "health process-cold fixture requires all five reviewed run slots",
  );
  return attempts;
}

// wait for one exact child ipc message
function waitForMessage(child, type, timeoutMs) {
  return new Promise((resolve, reject) => {
    const cleanup = () => {
      clearTimeout(timer);
      child.off("error", onError);
      child.off("exit", onExit);
      child.off("message", onMessage);
    };
    const onError = (error) => {
      cleanup();
      reject(error);
    };
    const onExit = (code, signal) => {
      cleanup();
      reject(new Error(`child exited before ${type}: code=${String(code)} signal=${String(signal)}`));
    };
    const onMessage = (message) => {
      // ignore unrelated child messages
      if (message?.type !== type) {
        return;
      }
      cleanup();
      resolve(message);
    };
    const timer = setTimeout(
      // reject one expired child phase
      () => {
        cleanup();
        reject(new Error(`child ${type} message exceeded ${String(timeoutMs)}ms`));
      },
      timeoutMs,
    );
    child.on("error", onError);
    child.on("exit", onExit);
    child.on("message", onMessage);
  });
}

// wait for one child process exit
function waitForExit(child, timeoutMs) {
  // retain an exit observed before listener attachment
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve({ code: child.exitCode, signal: child.signalCode });
  }
  return new Promise((resolve, reject) => {
    const cleanup = () => {
      clearTimeout(timer);
      child.off("error", onError);
      child.off("exit", onExit);
    };
    const onError = (error) => {
      cleanup();
      reject(error);
    };
    const onExit = (code, signal) => {
      cleanup();
      resolve({ code, signal });
    };
    const timer = setTimeout(
      // reject one expired process exit
      () => {
        cleanup();
        reject(new Error(`child exit exceeded ${String(timeoutMs)}ms`));
      },
      timeoutMs,
    );
    child.on("error", onError);
    child.on("exit", onExit);
  });
}

// force one failed disposable child to exit
async function forceStop(child) {
  // skip a child already reaped
  if (child.exitCode !== null || child.signalCode !== null) {
    return;
  }
  child.kill("SIGKILL");
  await waitForExit(child, CHILD_STOP_TIMEOUT_MS);
}

// send one bounded child control message
function sendControl(child, message) {
  return new Promise((resolve, reject) => {
    child.send(message, (error) => {
      // reject one failed ipc delivery
      if (error !== null) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}

// validate one child resource snapshot
function validateResourceSnapshot(value) {
  const fields = [
    "involuntaryContextSwitches",
    "maximumResidentSetBytes",
    "systemCpuMicroseconds",
    "userCpuMicroseconds",
    "voluntaryContextSwitches",
  ];
  requireInvariant(
    value !== null && typeof value === "object" && fields.every(
      // accept only nonnegative finite counters
      (field) => Number.isFinite(value[field]) && value[field] >= 0,
    ),
    "direct-weather-api-cold.resource",
    "child resource snapshot is invalid",
  );
  return Object.fromEntries(fields.map(
    // retain one reviewed resource field
    (field) => [field, value[field]],
  ));
}

// classify one source-owned cold health response
function classifyWeatherHealth({ attempt, bodyBytes }) {
  const value = JSON.parse(bodyBytes.toString("utf8"));
  const completed = attempt.operationId === "api.health"
    && attempt.stratum === "process-pool-cold"
    && value.data?.database === "ready"
    && value.data?.live === true
    && value.data?.migration?.status === "current"
    && value.data?.ready === true
    && value.data?.worker?.freshness === "fresh";
  return {
    classification: completed
      ? "weather-api-health-ready-from-fresh-disposable-process"
      : "weather-api-health-invalid",
    completed,
  };
}

// run one fresh Weather process and listener per reviewed cold attempt
export async function runBackendPerformanceDirectWeatherApiColdCheck() {
  const sourceAttemptIds = processColdHealthAttemptIds();
  const spawnedPids = [];
  const stoppedPids = [];
  const resourceObservations = [];
  const binding = bindBackendPerformanceDirectSchedule({
    attemptIds: sourceAttemptIds,
    baseUrl: "http://127.0.0.1:9",
    generation: "node-v1",
    maximumResponseBytes: 64 * 1024,
    resourceInterval: null,
    role: "baseline",
    runId: "weather-api-process-cold-fixture",
  });
  const result = await evaluateBackendPerformanceDirect(binding.directConfiguration, {
    classifyResponse: classifyWeatherHealth,
    // create one independent os process and listener
    async prepareAttempt({ attempt }) {
      const child = fork(CHILD_URL, [], {
        env: { NODE_ENV: "test", TZ: "UTC" },
        execArgv: [],
        stdio: ["ignore", "ignore", "ignore", "ipc"],
      });
      try {
        const ready = await waitForMessage(child, "ready", CHILD_START_TIMEOUT_MS);
        requireInvariant(
          Number.isSafeInteger(child.pid) && ready.pid === child.pid
            && typeof ready.origin === "string",
          "direct-weather-api-cold.start",
          "child readiness identity is invalid",
        );
        requireInvariant(
          !spawnedPids.includes(child.pid),
          "direct-weather-api-cold.start",
          "process-cold attempt reused a child process identity",
        );
        spawnedPids.push(child.pid);
        return {
          attemptId: attempt.attemptId,
          baseUrl: ready.origin,
          child,
          classification: BACKEND_PERFORMANCE_DIRECT_PROCESS_COLD_PREPARATION,
          pid: child.pid,
        };
      } catch (error) {
        await forceStop(child);
        throw error;
      }
    },
    // stop and reap every prepared child before the next attempt
    async cleanupAttempt({ preparation }) {
      const stoppedMessage = waitForMessage(
        preparation.child,
        "stopped",
        CHILD_STOP_TIMEOUT_MS,
      );
      // prevent an unhandled rejection if control delivery fails first
      stoppedMessage.catch(
        // retain the failure for the awaited lifecycle branch
        () => {},
      );
      try {
        await sendControl(preparation.child, { type: "shutdown" });
        const stopped = await stoppedMessage;
        requireInvariant(
          stopped.pid === preparation.pid,
          "direct-weather-api-cold.stop",
          "child shutdown identity changed",
        );
        const exit = await waitForExit(preparation.child, CHILD_STOP_TIMEOUT_MS);
        requireInvariant(
          exit.code === 0 && exit.signal === null,
          "direct-weather-api-cold.stop",
          "child did not exit cleanly",
        );
        stoppedPids.push(stopped.pid);
        resourceObservations.push({
          attemptId: preparation.attemptId,
          measurementClass: "node-process-resource-usage-snapshot-after-request",
          pid: stopped.pid,
          values: validateResourceSnapshot(stopped.resourceSnapshot),
        });
      } catch (error) {
        await forceStop(preparation.child);
        throw error;
      }
    },
  });
  const operationStratum = result.summary.operationStrata.find(
    // select the exact measured source stratum
    (value) => value.operationId === "api.health"
      && value.stratum === "process-pool-cold",
  );
  requireInvariant(
    operationStratum !== undefined,
    "direct-weather-api-cold.summary",
    "process-cold operation summary is missing",
  );
  return {
    attemptDenominator: result.summary.durationCoverage,
    coldPolicy: {
      dbOsCold: {
        executed: false,
        reason: "requires approved detached-volume allocation and retained fadvise/mincore witness",
      },
      processPoolCold: {
        lifecycle: BACKEND_PERFORMANCE_DIRECT_PROCESS_COLD_PREPARATION,
        sourceAttemptIds,
      },
    },
    evidenceClass: "synthetic-fixture",
    nativeEligible: false,
    operationStratum,
    preparationClassifications: [...new Set(result.attempts.map(
      // retain every lifecycle classification
      (attempt) => attempt.preparationClassification,
    ))],
    resourceObservations,
    semanticClassifications: [...new Set(result.attempts.map(
      // retain every terminal semantic classification
      (attempt) => attempt.semanticClassification,
    ))],
    spawnedPids,
    stoppedPids,
    terminalCounts: result.summary.terminalCounts,
  };
}

// print only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runBackendPerformanceDirectWeatherApiColdCheck().then(
    // emit one machine-readable cold-fixture result
    (result) => process.stdout.write(`${JSON.stringify(result, null, 2)}\n`),
    // emit one controlled cold-fixture failure
    (error) => {
      process.stderr.write(`${error?.code ?? "direct-weather-api-cold.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    },
  );
}
