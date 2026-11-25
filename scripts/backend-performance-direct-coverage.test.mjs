import assert from "node:assert/strict";
import test from "node:test";

import {
  reconcileBackendPerformanceDirectCoverage,
} from "./backend-performance-direct-coverage.mjs";
import {
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";

// select exact source attempts for one operation and stratum
function attemptIds(operationId, stratum) {
  const schedule = buildBackendPerformanceFunctionalSchedule();
  return schedule.runSlots.flatMap(
    // retain every source run slot
    (runSlot) => runSlot.groups.flatMap(
      // retain every source group
      (group) => group.attempts.filter(
        // select the requested exact bucket
        (attempt) => attempt.operationId === operationId && attempt.stratum === stratum,
      ).map(
        // retain one exact attempt identity
        (attempt) => attempt.attemptId,
      ),
    ),
  );
}

// build one exact direct result receipt
function receipt(operationId, stratum, sourceAttemptIds, nativeEligible) {
  const count = sourceAttemptIds.length;
  return {
    attemptDenominator: { observedAttempts: count, plannedAttempts: count },
    evidenceClass: nativeEligible ? "native-batch" : "synthetic-fixture",
    nativeEligible,
    operationStratum: { observedAttempts: count, operationId, stratum },
    sourceBinding: { sourceAttemptIds },
    terminalCounts: { completed: count, error: 0, timeout: 0 },
  };
}

test("derives the complete approved functional operation-stratum denominator", () => {
  const result = reconcileBackendPerformanceDirectCoverage([]);

  assert.deepEqual(result.summary, {
    completeNativeBuckets: 0,
    missingNativeAttempts: 6275,
    missingNativeBuckets: 163,
    nativeBoundAttempts: 0,
    receiptBoundAttempts: 0,
    requiredAttempts: 6275,
    requiredBuckets: 163,
    requiredOperations: 104,
  });
  assert.equal(result.missingBuckets.length, 163);
  assert.deepEqual(result.missingBuckets[0], {
    missingNativeAttempts: 200,
    nativeBoundAttempts: 0,
    operationId: "page.home",
    receiptBoundAttempts: 0,
    requiredAttempts: 200,
    stratum: "warm",
  });
  assert.equal(result.nativeComplete, false);
});

test("keeps fixture rows visible without converting them to native coverage", () => {
  const ids = attemptIds("inference.wind.load-serve", "once-per-run");
  const result = reconcileBackendPerformanceDirectCoverage([
    receipt("inference.wind.load-serve", "once-per-run", ids, false),
  ]);
  const observed = result.observedBuckets[0];

  assert.equal(ids.length, 5);
  assert.deepEqual(observed, {
    missingNativeAttempts: 5,
    nativeBoundAttempts: 0,
    operationId: "inference.wind.load-serve",
    receiptBoundAttempts: 5,
    requiredAttempts: 5,
    stratum: "once-per-run",
  });
  assert.equal(result.summary.receiptBoundAttempts, 5);
  assert.equal(result.summary.nativeBoundAttempts, 0);
  assert.equal(result.summary.missingNativeAttempts, 6275);
});

test("reports exact partial and complete native bucket counts", () => {
  const ids = attemptIds("api.health", "warm");
  const partial = reconcileBackendPerformanceDirectCoverage([
    receipt("api.health", "warm", ids.slice(0, 40), true),
  ]);
  const partialBucket = partial.observedBuckets[0];

  assert.equal(ids.length, 200);
  assert.equal(partialBucket.requiredAttempts, 200);
  assert.equal(partialBucket.nativeBoundAttempts, 40);
  assert.equal(partialBucket.missingNativeAttempts, 160);
  assert.equal(partial.summary.nativeBoundAttempts, 40);
  assert.equal(partial.summary.missingNativeAttempts, 6235);

  const complete = reconcileBackendPerformanceDirectCoverage([
    receipt("api.health", "warm", ids, true),
  ]);
  assert.equal(complete.observedBuckets[0].missingNativeAttempts, 0);
  assert.equal(complete.summary.completeNativeBuckets, 1);
  assert.equal(complete.summary.missingNativeBuckets, 162);
});

test("diagnoses unbound and duplicate receipt attempts without increasing coverage", () => {
  const ids = attemptIds("api.health", "process-pool-cold");
  const unbound = receipt("api.health", "process-pool-cold", ids, true);
  delete unbound.sourceBinding;
  const duplicate = receipt("api.health", "process-pool-cold", ids, true);
  const result = reconcileBackendPerformanceDirectCoverage([
    unbound,
    duplicate,
    duplicate,
  ]);
  const codes = result.diagnostics.map(
    // retain compact diagnostic codes
    (diagnostic) => diagnostic.code,
  );

  assert.equal(codes.includes("direct-coverage.attempt-identities"), true);
  assert.equal(codes.filter(
    // count replayed exact attempts
    (code) => code === "direct-coverage.duplicate-attempt",
  ).length, ids.length);
  assert.equal(result.summary.nativeBoundAttempts, ids.length);
});
