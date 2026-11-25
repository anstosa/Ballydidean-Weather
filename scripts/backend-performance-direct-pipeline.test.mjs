import assert from "node:assert/strict";
import test from "node:test";

import { runBackendPerformanceDirectPipelineCheck } from
  "./backend-performance-direct-pipeline-check.mjs";

test("runs reviewed source operations through five baselines to one target", async () => {
  const result = await runBackendPerformanceDirectPipelineCheck();

  assert.equal(result.baselineCount, 5);
  assert.equal(result.evidenceClass, "synthetic-fixture");
  assert.deepEqual(result.attemptDenominator, {
    observedAttempts: 82,
    plannedAttempts: 82,
    terminalCounts: { completed: 82, error: 0, timeout: 0 },
  });
  assert.deepEqual(result.classifications, [
    {
      classification: "ordinary-http-status-and-complete-body",
      count: 80,
      terminalCategory: "completed",
    },
    {
      classification: "forecast-observation-input-v1-at-reference-time",
      count: 1,
      terminalCategory: "completed",
    },
    {
      classification: "maintenance-three-family-schedule-at-reference-time",
      count: 1,
      terminalCategory: "completed",
    },
  ]);
  assert.equal(result.resourceEvidencePresent, true);
  assert.equal(result.metricTargets.length, 8);
  assert.equal(result.metricTargets.every(
    // require one positive strict target
    (target) => target.threshold > 0 && target.strictImprovement > 0,
  ), true);
  assert.match(result.targetReceiptSha256, /^[a-f0-9]{64}$/);
});
