import assert from "node:assert/strict";
import test from "node:test";

import {
  runBackendPerformanceDirectAdminReadCheck,
} from "./backend-performance-direct-admin-read-check.mjs";

test("runs the reviewed administrator login and page read through the production web adapter", async () => {
  const result = await runBackendPerformanceDirectAdminReadCheck();

  assert.deepEqual(result.coverage, {
    family: "admin-live-safe",
    kind: "single-reviewed-operation-causal-subset",
    operationId: "admin.live.read-page",
    stratum: "once-per-run",
  });
  assert.deepEqual(result.attemptDenominator, {
    observedAttempts: 5,
    plannedAttempts: 5,
  });
  assert.deepEqual(result.terminalCounts, {
    completed: 5,
    error: 0,
    timeout: 0,
  });
  assert.equal(result.operationStratum.observedAttempts, 5);
  assert.equal(result.operationStratum.operationId, "admin.live.read-page");
  assert.equal(result.operationStratum.stratum, "once-per-run");
  assert.deepEqual(result.execution.loginStatuses, [303, 303, 303, 303, 303]);
  assert.deepEqual(result.execution.logoutStatuses, [303, 303, 303, 303, 303]);
  assert.deepEqual(result.preparationClassifications, [
    "production-web-adapter-same-slot-admin-login",
  ]);
  assert.deepEqual(result.semanticClassifications, [
    "production-web-adapter-authenticated-admin-page-complete",
  ]);
  assert.equal(result.sourceBinding.loginAttemptIds.length, 5);
  assert.equal(result.sourceBinding.readAttemptIds.length, 5);
  assert.equal(result.evidenceClass, "synthetic-fixture");
  assert.equal(result.nativeEligible, false);
  assert.equal(result.unsupportedBoundaries.length, 3);
});
