import assert from "node:assert/strict";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
  BACKEND_PERFORMANCE_DIRECT_PROCESS_COLD_PREPARATION,
  evaluateBackendPerformanceDirect,
} from "./backend-performance-direct-evaluator.mjs";
import { runBackendPerformanceDirectWeatherApiColdCheck } from
  "./backend-performance-direct-weather-api-cold-check.mjs";

test("runs each reviewed health process-cold attempt in a fresh disposable process", async () => {
  const result = await runBackendPerformanceDirectWeatherApiColdCheck();

  assert.deepEqual(result.attemptDenominator, {
    observedAttempts: 5,
    plannedAttempts: 5,
  });
  assert.deepEqual(result.terminalCounts, {
    completed: 5,
    error: 0,
    timeout: 0,
  });
  assert.equal(result.operationStratum.operationId, "api.health");
  assert.equal(result.operationStratum.stratum, "process-pool-cold");
  assert.equal(result.operationStratum.observedAttempts, 5);
  assert.equal(result.operationStratum.latencyNanoseconds.maximum > 0, true);
  assert.deepEqual(result.preparationClassifications, [
    BACKEND_PERFORMANCE_DIRECT_PROCESS_COLD_PREPARATION,
  ]);
  assert.deepEqual(result.semanticClassifications, [
    "weather-api-health-ready-from-fresh-disposable-process",
  ]);
  assert.equal(new Set(result.spawnedPids).size, 5);
  assert.deepEqual(result.stoppedPids, result.spawnedPids);
  assert.equal(result.resourceObservations.length, 5);
  assert.deepEqual(
    result.resourceObservations.map(
      // retain one source attempt identity
      (observation) => observation.attemptId,
    ),
    result.coldPolicy.processPoolCold.sourceAttemptIds,
  );
  assert.equal(result.resourceObservations.every(
    // require one bounded process resource snapshot
    (observation) => observation.measurementClass
      === "node-process-resource-usage-snapshot-after-request"
      && observation.values.maximumResidentSetBytes > 0
      && observation.values.userCpuMicroseconds >= 0
      && observation.values.systemCpuMicroseconds >= 0,
  ), true);
  assert.equal(result.coldPolicy.dbOsCold.executed, false);
  assert.equal(result.evidenceClass, "synthetic-fixture");
  assert.equal(result.nativeEligible, false);
});

test("rejects an imprecise process-cold lifecycle label before request traffic", async () => {
  let cleanups = 0;
  let requests = 0;
  const result = await evaluateBackendPerformanceDirect({
    attempts: [{
      attemptId: "process-cold-1",
      body: null,
      expectedStatuses: [200],
      headers: {},
      method: "GET",
      operationId: "api.health",
      path: "/api/v1/health",
      runSlotId: "run-slot-01",
      stratum: "process-pool-cold",
      timeoutMs: 1_000,
    }],
    baseUrl: "http://127.0.0.1:9",
    contractVersion: BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
    generation: "node-v1",
    maximumResponseBytes: 1_024,
    resourceInterval: null,
    role: "baseline",
    runId: "invalid-process-cold-label",
  }, {
    // retain cleanup after rejected preparation
    async cleanupAttempt() {
      cleanups += 1;
    },
    // reject traffic after an imprecise lifecycle label
    async fetch() {
      requests += 1;
      return new Response("ok", { status: 200 });
    },
    // provide a deliberately insufficient lifecycle label
    async prepareAttempt() {
      return {
        baseUrl: "http://127.0.0.1:3000",
        classification: "listener-restarted",
      };
    },
  });

  assert.equal(cleanups, 1);
  assert.equal(requests, 0);
  assert.equal(result.attempts[0].terminalCategory, "error");
  assert.equal(result.attempts[0].errorCategory, "process");
  assert.equal(result.attempts[0].preparationClassification, "not-required");
});
