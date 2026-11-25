import assert from "node:assert/strict";
import test from "node:test";

import { runBackendPerformanceDirectWeatherApiCheck } from
  "./backend-performance-direct-weather-api-check.mjs";

test("measures the existing Weather health handler with its injectable clock and store", async () => {
  const result = await runBackendPerformanceDirectWeatherApiCheck();

  assert.deepEqual(result.attemptDenominator, {
    observedAttempts: 40,
    plannedAttempts: 40,
  });
  assert.deepEqual(result.terminalCounts, {
    completed: 40,
    error: 0,
    timeout: 0,
  });
  assert.deepEqual(result.classifications, [
    "weather-api-health-ready-at-explicit-time",
  ]);
  assert.equal(result.handlerClockReads, 40);
  assert.equal(result.healthReads, 40);
  assert.equal(result.resourceEvidence.nativeEligible, false);
  assert.equal(result.resourceEvidence.sampleCount, 42);
});
