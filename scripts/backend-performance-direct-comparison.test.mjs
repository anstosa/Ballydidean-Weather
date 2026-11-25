import assert from "node:assert/strict";
import test from "node:test";

import { BACKEND_PERFORMANCE_DIRECT_COMPARISON_VERSION } from
  "./backend-performance-direct-comparison.mjs";
import { runBackendPerformanceDirectComparisonCheck } from
  "./backend-performance-direct-comparison-check.mjs";

test("compares synthetic candidate metrics against the durable strict target", async () => {
  const result = await runBackendPerformanceDirectComparisonCheck();

  assert.match(result.targetReceiptSha256, /^[a-f0-9]{64}$/u);
  assert.equal(result.preTargetCandidateRejected, true);
  assert.equal(result.callerThresholdRejected, true);
  assert.equal(result.identityMismatchRejected, true);
  assert.equal(result.incompleteMetricDenominatorRejected, true);
  assert.equal(result.pass.contractVersion, BACKEND_PERFORMANCE_DIRECT_COMPARISON_VERSION);
  assert.equal(result.pass.outcome, "pass");
  assert.equal(result.pass.strictPass, true);
  assert.equal(result.pass.terminalStatus, "pass");
  assert.deepEqual(result.pass.metricDenominator, {
    capacityGaps: 0,
    measured: 1,
    targeted: 1,
  });
  assert.equal(result.pass.metricResults[0].status, "pass");
  assert.equal(result.pass.metricResults[0].margin, 0);
  assert.equal(result.fail.outcome, "fail");
  assert.equal(result.fail.strictPass, false);
  assert.equal(result.fail.terminalStatus, "pass");
  assert.equal(result.fail.metricResults[0].status, "fail");
  assert.equal(result.fail.metricResults[0].margin, -1);
  assert.equal(result.capacityGap.outcome, "capacity-gap");
  assert.equal(result.capacityGap.strictPass, false);
  assert.deepEqual(result.capacityGap.metricDenominator, {
    capacityGaps: 1,
    measured: 0,
    targeted: 1,
  });
  assert.equal(result.capacityGap.metricResults[0].status, "capacity-gap");
  assert.equal(
    result.capacityGap.metricResults[0].capacityGap,
    "measurement-capacity-unavailable",
  );
  assert.equal(result.terminalFail.outcome, "fail");
  assert.equal(result.terminalFail.terminalStatus, "fail");
  assert.deepEqual(result.terminalFail.attemptDenominator.terminalCounts, {
    completed: 0,
    error: 1,
    timeout: 0,
  });
  assert.equal(result.pass.evidenceClass, "synthetic-fixture");
  assert.equal(result.pass.nativeEligible, false);
});
