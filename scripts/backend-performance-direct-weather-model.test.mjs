import assert from "node:assert/strict";
import test from "node:test";

import { runBackendPerformanceDirectWeatherModelCheck } from
  "./backend-performance-direct-weather-model-check.mjs";

test("executes reviewed wind load-serve attempts through the compiled Weather model consumer", async () => {
  const result = await runBackendPerformanceDirectWeatherModelCheck();

  assert.deepEqual(result.attemptDenominator, {
    observedAttempts: 5,
    plannedAttempts: 5,
  });
  assert.deepEqual(result.terminalCounts, {
    completed: 5,
    error: 0,
    timeout: 0,
  });
  assert.equal(result.operationStratum.operationId, "inference.wind.load-serve");
  assert.equal(result.operationStratum.stratum, "once-per-run");
  assert.equal(result.operationStratum.observedAttempts, 5);
  assert.equal(result.operationStratum.latencyNanoseconds.maximum > 0, true);
  assert.deepEqual(result.preparationClassifications, [
    "verified-immutable-wind-runtime-and-disposable-in-process-listener",
  ]);
  assert.deepEqual(result.semanticClassifications, [
    "compiled-weather-api-applied-verified-wind-canary",
  ]);
  assert.deepEqual(result.execution, {
    classification: "compiled-weather-api-handler-with-verified-immutable-wind-runtime",
    cleanupCount: 5,
    clockReads: 5,
    forecastReads: 5,
    preparationCount: 5,
    pressureContextReads: 5,
    runtimeVerificationCount: 5,
    siteReads: 5,
  });
  assert.deepEqual(result.sourceBinding.nodeArgv, [{
    phase: "start-api",
    argv: ["node", "apps/api/dist/main.js"],
  }]);
  assert.equal(result.sourceBinding.sourceAttemptIds.length, 5);
  assert.equal(result.sourceBinding.sourceAttemptIds.every(
    // require one exact functional schedule identity per run slot
    (attemptId, index) => attemptId
      === `planned-functional-v4-r2:run-slot-0${String(index + 1)}:1223:inference.wind.load-serve:once-per-run`,
  ), true);
  assert.deepEqual(result.resourceEvidence, {
    contractVersion: "weather-backend-disposable-process-resource/v1",
    kind: "disposable-process-snapshot-not-cgroup",
    nativeEligible: false,
    sampleCount: 7,
  });
  assert.equal(result.unsupportedBoundaries.length, 3);
  assert.equal(result.evidenceClass, "synthetic-fixture");
  assert.equal(result.nativeEligible, false);
});
