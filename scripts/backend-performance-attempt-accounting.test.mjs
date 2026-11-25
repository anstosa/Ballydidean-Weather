import assert from "node:assert/strict";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_ATTEMPT_ACCOUNTING_PAIR_VERSION,
  BACKEND_PERFORMANCE_ATTEMPT_ACCOUNTING_VERSION,
  evaluateBackendPerformanceAttemptAccounting,
  pairBackendPerformanceAttemptAccounting,
} from "./backend-performance-attempt-accounting.mjs";
import { buildBackendPerformanceFunctionalSchedule } from
  "./backend-performance-functional-schedule.mjs";
import { buildBackendPerformanceScheduleBinding } from
  "./backend-performance-schedule-binding.mjs";

// clone one plain test value
function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

// build both exact schedule bindings
function exactBindings() {
  return {
    baseline: buildBackendPerformanceScheduleBinding({
      generation: "node-v1",
      role: "baseline",
    }),
    candidate: buildBackendPerformanceScheduleBinding({
      generation: "rust-v1",
      role: "candidate",
    }),
  };
}

// flatten every planned attempt in fixed topology order
function plannedAttempts() {
  const attempts = [];
  const plan = buildBackendPerformanceFunctionalSchedule();
  // visit each fixed slot
  for (const runSlot of plan.runSlots) {
    // visit each fixed group
    for (const group of runSlot.groups) {
      // visit each fixed attempt
      for (const attempt of group.attempts) {
        attempts.push(attempt);
      }
    }
  }
  assert.equal(attempts.length, 6275);
  return attempts;
}

// build one valid synthetic terminal observation
function syntheticOutcome(attempt, index) {
  // retain HTTP evidence while the dual-channel API process remains running
  if (attempt.operationId === "inference.temperature.load-serve") {
    return {
      attemptId: attempt.attemptId,
      elapsedMonotonic: { kind: "observed", nanoseconds: index + 1 },
      errorCategory: null,
      httpResponseBytes: 101,
      processOutputBytes: 0,
      terminalCategory: "completed",
      terminalStatus: {
        httpStatus: 200,
        kind: "http",
        processExitCode: null,
        processSignal: null,
      },
    };
  }
  // retain both terminal channels when both were actually observed
  if (attempt.operationId === "inference.wind.load-serve") {
    return {
      attemptId: attempt.attemptId,
      elapsedMonotonic: { kind: "observed", nanoseconds: index + 1 },
      errorCategory: null,
      httpResponseBytes: 103,
      processOutputBytes: 17,
      terminalCategory: "completed",
      terminalStatus: {
        httpStatus: 200,
        kind: "http-and-process",
        processExitCode: 0,
        processSignal: null,
      },
    };
  }
  // retain the fixed process-only interruption signal
  if (attempt.operationId === "provider.open-meteo-current.interruption-resume") {
    return {
      attemptId: attempt.attemptId,
      elapsedMonotonic: { kind: "observed", nanoseconds: index + 1 },
      errorCategory: "cancelled-after-start",
      httpResponseBytes: 0,
      processOutputBytes: 13,
      terminalCategory: "cancelled",
      terminalStatus: {
        httpStatus: null,
        kind: "process",
        processExitCode: null,
        processSignal: "SIGKILL",
      },
    };
  }
  const variant = index % 5;
  // cover ordinary successful completion
  if (variant === 0) {
    return {
      attemptId: attempt.attemptId,
      elapsedMonotonic: { kind: "observed", nanoseconds: index + 1 },
      errorCategory: null,
      httpResponseBytes: 10,
      processOutputBytes: 0,
      terminalCategory: "completed",
      terminalStatus: {
        httpStatus: 200,
        kind: "http",
        processExitCode: null,
        processSignal: null,
      },
    };
  }
  // cover completed process evidence
  if (variant === 1) {
    return {
      attemptId: attempt.attemptId,
      elapsedMonotonic: { kind: "observed", nanoseconds: index + 1 },
      errorCategory: null,
      httpResponseBytes: 0,
      processOutputBytes: 7,
      terminalCategory: "completed",
      terminalStatus: {
        httpStatus: null,
        kind: "process",
        processExitCode: 0,
        processSignal: null,
      },
    };
  }
  // cover declared error without status inference
  if (variant === 2) {
    return {
      attemptId: attempt.attemptId,
      elapsedMonotonic: { kind: "observed", nanoseconds: index + 1 },
      errorCategory: "process",
      httpResponseBytes: 0,
      processOutputBytes: 5,
      terminalCategory: "error",
      terminalStatus: {
        httpStatus: null,
        kind: "process",
        processExitCode: 1,
        processSignal: null,
      },
    };
  }
  // cover terminal timeout accounting
  if (variant === 3) {
    return {
      attemptId: attempt.attemptId,
      elapsedMonotonic: { kind: "observed", nanoseconds: index + 1 },
      errorCategory: "deadline-exceeded",
      httpResponseBytes: 2,
      processOutputBytes: 3,
      terminalCategory: "timeout",
      terminalStatus: {
        httpStatus: null,
        kind: "none",
        processExitCode: null,
        processSignal: null,
      },
    };
  }
  return {
    attemptId: attempt.attemptId,
    elapsedMonotonic: { kind: "unavailable", reason: "not-started" },
    errorCategory: "cancelled-before-start",
    httpResponseBytes: 0,
    processOutputBytes: 0,
    terminalCategory: "cancelled",
    terminalStatus: {
      httpStatus: null,
      kind: "none",
      processExitCode: null,
      processSignal: null,
    },
  };
}

// build the full synthetic planned domain
function fullOutcomes() {
  return plannedAttempts().map(
    // derive one observation from one planned identity
    (attempt, index) => syntheticOutcome(attempt, index),
  );
}

// sum one terminal denominator
function sumTerminalCounts(counts) {
  return counts.completed + counts.error + counts.timeout + counts.cancelled;
}

// derive expected grouping only from the plan and raw observations
function expectedOperationStrata(planAttempts, outcomes) {
  const outcomesByAttemptId = new Map(outcomes.map(
    // index one raw observation without returned evaluator grouping
    (outcome) => [outcome.attemptId, outcome],
  ));
  const expected = new Map();
  // map every raw observation through the plan-owned operation and stratum
  for (const planned of planAttempts) {
    const key = JSON.stringify([planned.operationId, planned.stratum]);
    let bucket = expected.get(key);
    // initialize one independent expected denominator
    if (bucket === undefined) {
      bucket = {
        bytes: {
          httpResponseBytes: 0,
          processOutputBytes: 0,
          totalBytes: 0,
        },
        elapsedMonotonic: {
          observedAttempts: 0,
          unavailableNotStartedAttempts: 0,
        },
        operationId: planned.operationId,
        plannedAttempts: 0,
        stratum: planned.stratum,
        terminalCounts: { cancelled: 0, completed: 0, error: 0, timeout: 0 },
      };
      expected.set(key, bucket);
    }
    const outcome = outcomesByAttemptId.get(planned.attemptId);
    assert.ok(outcome);
    bucket.plannedAttempts += 1;
    bucket.terminalCounts[outcome.terminalCategory] += 1;
    bucket.bytes.httpResponseBytes += outcome.httpResponseBytes;
    bucket.bytes.processOutputBytes += outcome.processOutputBytes;
    bucket.bytes.totalBytes += outcome.httpResponseBytes + outcome.processOutputBytes;
    // retain both elapsed coverage classes independently
    if (outcome.elapsedMonotonic.kind === "observed") {
      bucket.elapsedMonotonic.observedAttempts += 1;
    } else {
      bucket.elapsedMonotonic.unavailableNotStartedAttempts += 1;
    }
  }
  return expected;
}

// assert recursive output immutability
function assertDeepFrozen(value) {
  // inspect each nested record and array
  if (value !== null && typeof value === "object") {
    assert.equal(Object.isFrozen(value), true);
    // recurse through every child
    for (const child of Object.values(value)) {
      assertDeepFrozen(child);
    }
  }
}

// verify full independent role accounting
test("accounts for every planned baseline outcome without measurement claims", () => {
  const { baseline } = exactBindings();
  const outcomes = fullOutcomes();
  const accounting = evaluateBackendPerformanceAttemptAccounting({
    binding: baseline,
    outcomes,
  });
  assert.equal(accounting.contractVersion, BACKEND_PERFORMANCE_ATTEMPT_ACCOUNTING_VERSION);
  assert.equal(accounting.role, "baseline");
  assert.equal(accounting.generation, "node-v1");
  assert.equal(accounting.attempts.length, 6275);
  assert.equal(accounting.overall.plannedAttempts, 6275);
  assert.equal(sumTerminalCounts(accounting.overall.terminalCounts), 6275);
  assert.ok(accounting.overall.terminalCounts.completed > 0);
  assert.ok(accounting.overall.terminalCounts.error > 0);
  assert.ok(accounting.overall.terminalCounts.timeout > 0);
  assert.ok(accounting.overall.terminalCounts.cancelled > 0);
  assert.equal(
    accounting.overall.elapsedMonotonic.observedAttempts
      + accounting.overall.elapsedMonotonic.unavailableNotStartedAttempts,
    6275,
  );
  assert.equal(accounting.durationCoverage.complete, false);
  assert.equal(accounting.measurementEligibility.eligible, false);
  assert.equal(accounting.canonicalEligible, false);
  assert.equal(accounting.provenance.measurementEvidence, false);
  assert.equal(accounting.provenance.syntheticEvidenceEligible, false);
  assert.equal(accounting.provenance.trafficGenerated, false);
  assert.deepEqual(accounting.pendingGates, {
    failedCorrectness: "not-evaluated",
    resource: "not-evaluated",
    state: "not-evaluated",
    target: "not-evaluated",
    witness: "not-evaluated",
  });
  assert.equal(accounting.attempts[0].operationId, plannedAttempts()[0].operationId);
  assert.equal(accounting.attempts[0].runSlotId, plannedAttempts()[0].runSlotId);
  assert.equal(accounting.attempts[0].stratum, plannedAttempts()[0].stratum);
  const dualRunning = accounting.attempts.find(
    // find one known dual-channel operation whose process remains running
    (attempt) => attempt.operationId === "inference.temperature.load-serve",
  );
  assert.ok(dualRunning);
  assert.deepEqual(dualRunning.terminalStatus, {
    httpStatus: 200,
    kind: "http",
    processExitCode: null,
    processSignal: null,
  });
  assert.equal(dualRunning.httpResponseBytes, 101);
  assert.equal(dualRunning.processOutputBytes, 0);
  const dualTerminal = accounting.attempts.find(
    // find one known dual-channel operation with both terminal statuses
    (attempt) => attempt.operationId === "inference.wind.load-serve",
  );
  assert.ok(dualTerminal);
  assert.deepEqual(dualTerminal.terminalStatus, {
    httpStatus: 200,
    kind: "http-and-process",
    processExitCode: 0,
    processSignal: null,
  });
  assert.equal(dualTerminal.httpResponseBytes, 103);
  assert.equal(dualTerminal.processOutputBytes, 17);
  const interrupted = accounting.attempts.find(
    // find one known process-only interruption operation
    (attempt) => attempt.operationId === "provider.open-meteo-current.interruption-resume",
  );
  assert.ok(interrupted);
  assert.equal(interrupted.terminalCategory, "cancelled");
  assert.equal(interrupted.errorCategory, "cancelled-after-start");
  assert.equal(interrupted.elapsedMonotonic.kind, "observed");
  assert.deepEqual(interrupted.terminalStatus, {
    httpStatus: null,
    kind: "process",
    processExitCode: null,
    processSignal: "SIGKILL",
  });
  const notStarted = accounting.attempts.find(
    // find one separately unavailable before-start cancellation
    (attempt) => attempt.errorCategory === "cancelled-before-start",
  );
  assert.ok(notStarted);
  assert.equal(notStarted.elapsedMonotonic.kind, "unavailable");
  assertDeepFrozen(accounting);
});

// verify operation and stratum aggregate consistency
test("retains every terminal category and byte count in every denominator", () => {
  const { baseline } = exactBindings();
  const outcomes = fullOutcomes();
  const accounting = evaluateBackendPerformanceAttemptAccounting({ binding: baseline, outcomes });
  const expectedByOperationStratum = expectedOperationStrata(plannedAttempts(), outcomes);
  const aggregate = {
    httpResponseBytes: 0,
    plannedAttempts: 0,
    processOutputBytes: 0,
    terminalCounts: { cancelled: 0, completed: 0, error: 0, timeout: 0 },
    unavailable: 0,
  };
  // sum every plan-derived operation and stratum bucket
  for (const bucket of accounting.operationStrata) {
    const expected = expectedByOperationStratum.get(
      JSON.stringify([bucket.operationId, bucket.stratum]),
    );
    assert.deepEqual(bucket, expected);
    assert.equal(sumTerminalCounts(bucket.terminalCounts), bucket.plannedAttempts);
    assert.equal(
      bucket.bytes.totalBytes,
      bucket.bytes.httpResponseBytes + bucket.bytes.processOutputBytes,
    );
    aggregate.plannedAttempts += bucket.plannedAttempts;
    aggregate.httpResponseBytes += bucket.bytes.httpResponseBytes;
    aggregate.processOutputBytes += bucket.bytes.processOutputBytes;
    aggregate.unavailable += bucket.elapsedMonotonic.unavailableNotStartedAttempts;
    // retain all four terminal classes
    for (const category of ["completed", "error", "timeout", "cancelled"]) {
      aggregate.terminalCounts[category] += bucket.terminalCounts[category];
    }
  }
  assert.equal(aggregate.plannedAttempts, accounting.overall.plannedAttempts);
  assert.equal(aggregate.httpResponseBytes, accounting.overall.bytes.httpResponseBytes);
  assert.equal(aggregate.processOutputBytes, accounting.overall.bytes.processOutputBytes);
  assert.equal(aggregate.unavailable, accounting.overall.elapsedMonotonic.unavailableNotStartedAttempts);
  assert.deepEqual(aggregate.terminalCounts, accounting.overall.terminalCounts);
  assert.equal(accounting.operationStrata.length, expectedByOperationStratum.size);
  assert.equal(accounting.role, "baseline");
});

// verify raw independent evaluation and exact matched pairing
test("pairs raw role inputs after baseline-first independent validation", () => {
  const { baseline, candidate } = exactBindings();
  const baselineOutcomes = fullOutcomes();
  const candidateOutcomes = clone(baselineOutcomes).reverse();
  assert.throws(
    // reject candidate-only use before reading its malformed outcomes
    () => evaluateBackendPerformanceAttemptAccounting({
      binding: candidate,
      outcomes: null,
    }),
    /baseline binding/,
  );
  const paired = pairBackendPerformanceAttemptAccounting(
    { binding: baseline, outcomes: baselineOutcomes },
    { binding: candidate, outcomes: candidateOutcomes },
  );
  assert.equal(paired.contractVersion, BACKEND_PERFORMANCE_ATTEMPT_ACCOUNTING_PAIR_VERSION);
  assert.equal(paired.baseline.role, "baseline");
  assert.equal(paired.candidate.role, "candidate");
  assert.equal(paired.baseline.attempts.length, 6275);
  assert.equal(paired.candidate.attempts.length, 6275);
  assert.equal(paired.baseline.attempts[0].attemptId, paired.candidate.attempts[0].attemptId);
  assert.equal(
    paired.schedulePair.matched.planSha256,
    baseline.identity.schedule.digest.value,
  );
  assert.equal(paired.comparisonEligibility.eligible, false);
  assert.equal(paired.provenance.measurementEvidence, false);
  assertDeepFrozen(paired);

  assert.throws(
    // reject swapped authoritative roles
    () => pairBackendPerformanceAttemptAccounting(
      { binding: candidate, outcomes: candidateOutcomes },
      { binding: baseline, outcomes: baselineOutcomes },
    ),
    /baseline binding/,
  );
  assert.throws(
    // reject the baseline role in the pair candidate position before outcomes
    () => pairBackendPerformanceAttemptAccounting(
      { binding: baseline, outcomes: baselineOutcomes },
      { binding: baseline, outcomes: null },
    ),
    /candidate binding/,
  );
  assert.throws(
    // reject an evaluated summary in place of raw input
    () => pairBackendPerformanceAttemptAccounting(
      paired.baseline,
      { binding: candidate, outcomes: candidateOutcomes },
    ),
    /only binding and outcomes/,
  );
});

// verify one-to-one full-domain rejection paths
test("rejects incomplete, duplicate, extra, and legacy attempt domains", () => {
  const { baseline } = exactBindings();
  const outcomes = fullOutcomes();
  assert.throws(
    // omit one planned attempt
    () => evaluateBackendPerformanceAttemptAccounting({
      binding: baseline,
      outcomes: outcomes.slice(1),
    }),
    /requires all 6275 planned outcomes/,
  );

  const duplicate = clone(outcomes);
  duplicate[1].attemptId = duplicate[0].attemptId;
  assert.throws(
    // duplicate one planned identity
    () => evaluateBackendPerformanceAttemptAccounting({ binding: baseline, outcomes: duplicate }),
    /duplicate outcome attemptId/,
  );

  const extra = clone(outcomes);
  extra.push({ ...clone(outcomes[0]), attemptId: "unplanned-attempt" });
  assert.throws(
    // add one unplanned identity
    () => evaluateBackendPerformanceAttemptAccounting({ binding: baseline, outcomes: extra }),
    /not in the authoritative plan/,
  );

  const legacy = clone(outcomes);
  legacy[0].attemptId = "planned-v3-r1:run-1:0001:legacy:control";
  assert.throws(
    // substitute one legacy planned identity
    () => evaluateBackendPerformanceAttemptAccounting({ binding: baseline, outcomes: legacy }),
    /not in the authoritative plan/,
  );
});

// verify exact binding and role rejection paths
test("rejects fabricated plans, wrong roles, and schedule overrides", () => {
  const { baseline, candidate } = exactBindings();
  const outcomes = fullOutcomes();
  const fabricated = clone(baseline);
  fabricated.identity.schedule.digest.value = "0".repeat(64);
  assert.throws(
    // reject one fabricated plan binding
    () => evaluateBackendPerformanceAttemptAccounting({ binding: fabricated, outcomes }),
    /does not match either authoritative role/,
  );
  const legacyRole = clone(baseline);
  legacyRole.identity.role = "baseline-v0";
  assert.throws(
    // reject one legacy role label
    () => evaluateBackendPerformanceAttemptAccounting({ binding: legacyRole, outcomes }),
    /does not match either authoritative role/,
  );
  assert.throws(
    // reject the wrong role in the baseline pair position
    () => pairBackendPerformanceAttemptAccounting(
      { binding: candidate, outcomes },
      { binding: candidate, outcomes },
    ),
    /baseline binding/,
  );
  assert.throws(
    // reject one schedule selector on independent input
    () => evaluateBackendPerformanceAttemptAccounting({
      binding: baseline,
      outcomes,
      schedule: "override",
    }),
    /only binding and outcomes/,
  );
});

// verify malformed and inconsistent observations
test("rejects malformed status, duration, category, byte, and row shapes", () => {
  const { baseline } = exactBindings();
  const mutations = [
    // reject a terminal class outside the closed schema
    (row) => { row.terminalCategory = "success"; },
    // reject a zero stand-in for elapsed duration
    (row) => { row.elapsedMonotonic = { kind: "observed", nanoseconds: 0 }; },
    // reject unavailable duration outside before-start cancellation
    (row) => { row.elapsedMonotonic = { kind: "unavailable", reason: "not-started" }; },
    // reject raw error text in place of stable categories
    (row) => {
      row.terminalCategory = "error";
      row.errorCategory = "database password leaked";
    },
    // reject inconsistent terminal status discriminants
    (row) => {
      row.terminalStatus = {
        httpStatus: 200,
        kind: "none",
        processExitCode: null,
        processSignal: null,
      };
    },
    // reject completed work without any terminal status
    (row) => {
      row.terminalStatus = {
        httpStatus: null,
        kind: "none",
        processExitCode: null,
        processSignal: null,
      };
    },
    // reject simultaneous process exit and signal evidence
    (row) => {
      row.terminalStatus = {
        httpStatus: null,
        kind: "process",
        processExitCode: 137,
        processSignal: "SIGKILL",
      };
    },
    // reject process signals outside the fixed Linux scheme
    (row) => {
      row.terminalStatus = {
        httpStatus: null,
        kind: "process",
        processExitCode: null,
        processSignal: "SIGCUSTOM",
      };
    },
    // reject unsafe byte counts
    (row) => { row.httpResponseBytes = Number.MAX_SAFE_INTEGER + 1; },
    // reject unreviewed supplied grouping claims
    (row) => { row.groupId = "caller-group"; },
  ];
  // exercise each malformed observation over the complete domain
  for (const mutate of mutations) {
    const outcomes = fullOutcomes();
    mutate(outcomes[0]);
    assert.throws(
      // reject one malformed first row
      () => evaluateBackendPerformanceAttemptAccounting({ binding: baseline, outcomes }),
    );
  }

  const inconsistentCancellation = fullOutcomes();
  inconsistentCancellation[4].httpResponseBytes = 1;
  assert.throws(
    // reject byte evidence for an explicitly not-started cancellation
    () => evaluateBackendPerformanceAttemptAccounting({
      binding: baseline,
      outcomes: inconsistentCancellation,
    }),
    /not-started cancellation has inconsistent evidence/,
  );

  const unsafeAggregate = fullOutcomes();
  unsafeAggregate[0].httpResponseBytes = Number.MAX_SAFE_INTEGER;
  unsafeAggregate[1].httpResponseBytes = Number.MAX_SAFE_INTEGER;
  unsafeAggregate[1].processOutputBytes = 0;
  assert.throws(
    // reject aggregate overflow even when each field is safe
    () => evaluateBackendPerformanceAttemptAccounting({
      binding: baseline,
      outcomes: unsafeAggregate,
    }),
    /exceeds the safe integer range/,
  );
});
