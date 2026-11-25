import assert from "node:assert/strict";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_PAIR_VERSION,
  BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_SOURCE_PINS,
  BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_VERSION,
  evaluateBackendPerformanceLatencyAccounting,
  pairBackendPerformanceLatencyAccounting,
} from "./backend-performance-latency-accounting.mjs";
import { buildBackendPerformanceFunctionalSchedule } from
  "./backend-performance-functional-schedule.mjs";
import { buildBackendPerformanceScheduleBinding } from
  "./backend-performance-schedule-binding.mjs";

const COLD_STRATA = Object.freeze([
  "application-cache-cold",
  "process-pool-cold",
  "db-os-cold",
]);

// build both exact authoritative role bindings
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

// flatten the full synthetic plan in fixed topology order
function plannedAttempts() {
  const attempts = [];
  const plan = buildBackendPerformanceFunctionalSchedule();
  // retain each actual planned slot
  for (const runSlot of plan.runSlots) {
    // retain each fixed group within the slot
    for (const group of runSlot.groups) {
      // retain each fixed attempt within the group
      for (const attempt of group.attempts) {
        attempts.push(attempt);
      }
    }
  }
  assert.equal(attempts.length, 6275);
  return attempts;
}

const PLAN_ATTEMPTS = plannedAttempts();
const RUN_SLOT_IDS = [...new Set(PLAN_ATTEMPTS.map(
  // retain actual planned run-slot identities
  (attempt) => attempt.runSlotId,
))];

// select one operation with exactly one once-per-run attempt in every slot
function oncePerRunFixture() {
  const byOperation = new Map();
  // group only exact once-per-run plan rows
  for (const attempt of PLAN_ATTEMPTS) {
    // exclude all other strata from this fixture
    if (attempt.stratum !== "once-per-run") {
      continue;
    }
    const rows = byOperation.get(attempt.operationId) ?? [];
    rows.push(attempt);
    byOperation.set(attempt.operationId, rows);
  }
  const fixture = [...byOperation.entries()].find(
    // require exactly one row in each actual run slot
    ([, rows]) => rows.length === RUN_SLOT_IDS.length
      && new Set(rows.map(
        // retain each row's actual slot
        (row) => row.runSlotId,
      )).size === RUN_SLOT_IDS.length,
  );
  assert.ok(fixture);
  return { operationId: fixture[0], rows: fixture[1] };
}

// select one operation represented in all three exact cold strata
function coldFixture() {
  const strataByOperation = new Map();
  // group only exact cold plan rows
  for (const attempt of PLAN_ATTEMPTS) {
    // exclude warm and once-per-run rows from this fixture
    if (!COLD_STRATA.includes(attempt.stratum)) {
      continue;
    }
    const strata = strataByOperation.get(attempt.operationId) ?? new Set();
    strata.add(attempt.stratum);
    strataByOperation.set(attempt.operationId, strata);
  }
  const operationId = [...strataByOperation.entries()].find(
    // require independent coverage of every cold layer
    ([, strata]) => COLD_STRATA.every(
      // retain one exact required layer
      (stratum) => strata.has(stratum),
    ),
  )?.[0];
  assert.ok(operationId);
  return {
    operationId,
    rows: PLAN_ATTEMPTS.filter(
      // retain only the selected operation's cold attempts
      (attempt) => attempt.operationId === operationId
        && COLD_STRATA.includes(attempt.stratum),
    ),
  };
}

// select one operation with a nonempty warm distribution
function warmFixture() {
  const first = PLAN_ATTEMPTS.find(
    // select the first exact warm plan row
    (attempt) => attempt.stratum === "warm",
  );
  assert.ok(first);
  return {
    operationId: first.operationId,
    rows: PLAN_ATTEMPTS.filter(
      // retain only the selected operation's warm attempts
      (attempt) => attempt.operationId === first.operationId
        && attempt.stratum === "warm",
    ),
  };
}

const ONCE_FIXTURE = oncePerRunFixture();
const COLD_FIXTURE = coldFixture();
const WARM_FIXTURE = warmFixture();

// build one ordinary successful observation
function completedObservation(nanoseconds) {
  return {
    elapsedMonotonic: { kind: "observed", nanoseconds },
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

// build one observed terminal failure without HTTP or process inference
function errorObservation(nanoseconds) {
  return {
    elapsedMonotonic: { kind: "observed", nanoseconds },
    errorCategory: "transport",
    httpResponseBytes: 0,
    processOutputBytes: 0,
    terminalCategory: "error",
    terminalStatus: {
      httpStatus: null,
      kind: "none",
      processExitCode: null,
      processSignal: null,
    },
  };
}

// build one observed timeout
function timeoutObservation(nanoseconds) {
  return {
    elapsedMonotonic: { kind: "observed", nanoseconds },
    errorCategory: "deadline-exceeded",
    httpResponseBytes: 0,
    processOutputBytes: 0,
    terminalCategory: "timeout",
    terminalStatus: {
      httpStatus: null,
      kind: "none",
      processExitCode: null,
      processSignal: null,
    },
  };
}

// build one observed after-start cancellation
function afterStartCancellation(nanoseconds) {
  return {
    elapsedMonotonic: { kind: "observed", nanoseconds },
    errorCategory: "cancelled-after-start",
    httpResponseBytes: 0,
    processOutputBytes: 1,
    terminalCategory: "cancelled",
    terminalStatus: {
      httpStatus: null,
      kind: "process",
      processExitCode: null,
      processSignal: "SIGTERM",
    },
  };
}

// build one explicitly unavailable before-start cancellation
function beforeStartCancellation() {
  return {
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

// build all planned observations through exact attempt identifiers
function fullOutcomes(overrides = new Map(), multiplier = 1) {
  return PLAN_ATTEMPTS.map(
    // attach one full observation to each exact planned attempt
    (attempt, index) => ({
      attemptId: attempt.attemptId,
      ...(overrides.get(attempt.attemptId)
        ?? completedObservation((index + 1000) * multiplier)),
    }),
  );
}

// find one exact pooled summary row
function pooledRow(accounting, operationId, stratum) {
  const row = accounting.summaries.pooledOperationStrata.find(
    // match one plan-derived operation and stratum
    (candidate) => candidate.operationId === operationId
      && candidate.stratum === stratum,
  );
  assert.ok(row);
  return row;
}

// find one exact actual-slot summary row
function runSlotRow(accounting, operationId, stratum, runSlotId) {
  const row = accounting.summaries.runSlotOperationStrata.find(
    // match one plan-derived operation, stratum, and actual slot
    (candidate) => candidate.operationId === operationId
      && candidate.stratum === stratum
      && candidate.runSlotId === runSlotId,
  );
  assert.ok(row);
  return row;
}

// require every reachable object to be immutable
function assertDeepFrozen(value) {
  // inspect every nested object recursively
  if (value !== null && typeof value === "object") {
    assert.equal(Object.isFrozen(value), true);
    // inspect every child owned by the closed result
    for (const child of Object.values(value)) {
      assertDeepFrozen(child);
    }
  }
}

// verify exact nearest-rank examples, terminal inclusion, and independent cold maxima
test("reduces raw full-plan durations without filtering terminal categories", () => {
  const { baseline } = exactBindings();
  const overrides = new Map();
  const onceValues = [10, 20, 30, 40, 50];
  const onceObservations = [
    completedObservation(onceValues[0]),
    errorObservation(onceValues[1]),
    timeoutObservation(onceValues[2]),
    afterStartCancellation(onceValues[3]),
    completedObservation(onceValues[4]),
  ];
  // assign the known five-value order statistic across actual slots
  for (let index = 0; index < ONCE_FIXTURE.rows.length; index += 1) {
    overrides.set(ONCE_FIXTURE.rows[index].attemptId, onceObservations[index]);
  }

  const expectedColdMaxima = new Map();
  const expectedColdSlotMaxima = new Map();
  const coldCounters = new Map();
  // assign distinct ranges while retaining each exact cold layer independently
  for (const attempt of COLD_FIXTURE.rows) {
    const next = (coldCounters.get(attempt.stratum) ?? 0) + 1;
    coldCounters.set(attempt.stratum, next);
    const base = COLD_STRATA.indexOf(attempt.stratum) * 100;
    const nanoseconds = base + next;
    overrides.set(attempt.attemptId, completedObservation(nanoseconds));
    expectedColdMaxima.set(attempt.stratum, nanoseconds);
    expectedColdSlotMaxima.set(
      JSON.stringify([attempt.stratum, attempt.runSlotId]),
      nanoseconds,
    );
  }
  // assign an independently ordered warm sample from one through its exact denominator
  for (let index = 0; index < WARM_FIXTURE.rows.length; index += 1) {
    overrides.set(
      WARM_FIXTURE.rows[index].attemptId,
      completedObservation(index + 1),
    );
  }

  const accounting = evaluateBackendPerformanceLatencyAccounting({
    binding: baseline,
    outcomes: fullOutcomes(overrides),
  });
  assert.equal(accounting.contractVersion, BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_VERSION);
  assert.equal(accounting.role, "baseline");
  assert.equal(accounting.canonicalEligible, false);
  assert.equal(accounting.measurementEligibility.eligible, false);
  assert.equal(accounting.provenance.trafficGenerated, false);
  assert.deepEqual(accounting.sourceAccounting.binding, baseline);
  assert.equal(accounting.sourceAccounting.role, "baseline");
  assert.match(accounting.sourceAccounting.accountingDigest.value, /^[0-9a-f]{64}$/u);
  assert.deepEqual(BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_SOURCE_PINS, {
    attemptAccounting: {
      bytes: 27791,
      sha256: "2beb9efa8926f91040d01eda005967dd699f172c4fba614aba56298ce8fff721",
    },
  });

  const pooledOnce = pooledRow(accounting, ONCE_FIXTURE.operationId, "once-per-run");
  assert.deepEqual(pooledOnce.latencyNanoseconds, {
    kind: "nearest-rank-quantiles",
    p50Nanoseconds: 30,
    p95Nanoseconds: 50,
    p99Nanoseconds: 50,
  });
  assert.equal(pooledOnce.plannedAttempts, 5);
  assert.deepEqual(pooledOnce.terminalCounts, {
    cancelled: 1,
    completed: 2,
    error: 1,
    timeout: 1,
  });
  const pooledWarm = pooledRow(accounting, WARM_FIXTURE.operationId, "warm");
  assert.deepEqual(pooledWarm.latencyNanoseconds, {
    kind: "nearest-rank-quantiles",
    p50Nanoseconds: Math.ceil(WARM_FIXTURE.rows.length * 0.50),
    p95Nanoseconds: Math.ceil(WARM_FIXTURE.rows.length * 0.95),
    p99Nanoseconds: Math.ceil(WARM_FIXTURE.rows.length * 0.99),
  });
  assert.equal(pooledWarm.plannedAttempts, WARM_FIXTURE.rows.length);
  // verify each slot retains its independent one-attempt distribution
  for (let index = 0; index < ONCE_FIXTURE.rows.length; index += 1) {
    const row = runSlotRow(
      accounting,
      ONCE_FIXTURE.operationId,
      "once-per-run",
      ONCE_FIXTURE.rows[index].runSlotId,
    );
    assert.equal(row.plannedAttempts, 1);
    assert.deepEqual(row.latencyNanoseconds, {
      kind: "nearest-rank-quantiles",
      p50Nanoseconds: onceValues[index],
      p95Nanoseconds: onceValues[index],
      p99Nanoseconds: onceValues[index],
    });
  }
  // verify each cold layer exposes only its own independently derived exact maximum
  for (const stratum of COLD_STRATA) {
    const row = pooledRow(accounting, COLD_FIXTURE.operationId, stratum);
    assert.deepEqual(row.latencyNanoseconds, {
      kind: "exact-maximum",
      maxNanoseconds: expectedColdMaxima.get(stratum),
    });
    // verify every actual slot keeps an independent maximum for this exact layer
    for (const runSlotId of RUN_SLOT_IDS) {
      const slotRows = COLD_FIXTURE.rows.filter(
        // retain this exact cold layer and slot
        (attempt) => attempt.stratum === stratum
          && attempt.runSlotId === runSlotId,
      );
      // skip operations that do not schedule this cold layer in a particular slot
      if (slotRows.length === 0) {
        continue;
      }
      const slot = runSlotRow(
        accounting,
        COLD_FIXTURE.operationId,
        stratum,
        runSlotId,
      );
      assert.equal(slot.plannedAttempts, slotRows.length);
      assert.deepEqual(slot.latencyNanoseconds, {
        kind: "exact-maximum",
        maxNanoseconds: expectedColdSlotMaxima.get(
          JSON.stringify([stratum, runSlotId]),
        ),
      });
    }
  }

  const pooledPlanned = accounting.summaries.pooledOperationStrata.reduce(
    // sum exact pooled denominators
    (total, row) => total + row.plannedAttempts,
    0,
  );
  const runSlotPlanned = accounting.summaries.runSlotOperationStrata.reduce(
    // sum exact actual-slot denominators
    (total, row) => total + row.plannedAttempts,
    0,
  );
  assert.equal(pooledPlanned, 6275);
  assert.equal(runSlotPlanned, 6275);
  assertDeepFrozen(accounting);
});

// verify unavailable durations poison only their exact pooled and slot summary rows
test("retains unavailable denominators and nulls incomplete metrics", () => {
  const { baseline } = exactBindings();
  const overrides = new Map();
  const onceValues = [10, 20, 30, 40, 50];
  // retain four observed values and one truly not-started cancellation
  for (let index = 0; index < ONCE_FIXTURE.rows.length; index += 1) {
    overrides.set(
      ONCE_FIXTURE.rows[index].attemptId,
      index === 0
        ? beforeStartCancellation()
        : completedObservation(onceValues[index]),
    );
  }
  const accounting = evaluateBackendPerformanceLatencyAccounting({
    binding: baseline,
    outcomes: fullOutcomes(overrides),
  });
  const pooled = pooledRow(accounting, ONCE_FIXTURE.operationId, "once-per-run");
  assert.deepEqual(pooled.durationCoverage, {
    complete: false,
    observedAttempts: 4,
    plannedAttempts: 5,
    unavailableNotStartedAttempts: 1,
  });
  assert.deepEqual(pooled.latencyNanoseconds, {
    kind: "nearest-rank-quantiles",
    p50Nanoseconds: null,
    p95Nanoseconds: null,
    p99Nanoseconds: null,
  });
  assert.equal(pooled.terminalCounts.cancelled, 1);

  const unavailableSlot = runSlotRow(
    accounting,
    ONCE_FIXTURE.operationId,
    "once-per-run",
    ONCE_FIXTURE.rows[0].runSlotId,
  );
  assert.deepEqual(unavailableSlot.durationCoverage, {
    complete: false,
    observedAttempts: 0,
    plannedAttempts: 1,
    unavailableNotStartedAttempts: 1,
  });
  assert.deepEqual(unavailableSlot.latencyNanoseconds, {
    kind: "nearest-rank-quantiles",
    p50Nanoseconds: null,
    p95Nanoseconds: null,
    p99Nanoseconds: null,
  });
  const observedSlot = runSlotRow(
    accounting,
    ONCE_FIXTURE.operationId,
    "once-per-run",
    ONCE_FIXTURE.rows[1].runSlotId,
  );
  assert.equal(observedSlot.durationCoverage.complete, true);
  assert.equal(observedSlot.latencyNanoseconds.p50Nanoseconds, 20);
  assert.equal(accounting.durationCoverage.complete, false);
  assert.equal(accounting.measurementEligibility.eligible, false);
});

// verify matched raw role pairing retains independent pooled and slot distributions
test("pairs independently reduced baseline and candidate raw accounting", () => {
  const { baseline, candidate } = exactBindings();
  const baselineOverrides = new Map();
  const candidateOverrides = new Map();
  // assign two distinct five-value distributions to the same planned identities
  for (let index = 0; index < ONCE_FIXTURE.rows.length; index += 1) {
    baselineOverrides.set(
      ONCE_FIXTURE.rows[index].attemptId,
      completedObservation((index + 1) * 10),
    );
    candidateOverrides.set(
      ONCE_FIXTURE.rows[index].attemptId,
      completedObservation((index + 1) * 20),
    );
  }
  const paired = pairBackendPerformanceLatencyAccounting(
    { binding: baseline, outcomes: fullOutcomes(baselineOverrides) },
    { binding: candidate, outcomes: fullOutcomes(candidateOverrides, 2) },
  );
  assert.equal(
    paired.contractVersion,
    BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_PAIR_VERSION,
  );
  assert.equal(paired.baseline.role, "baseline");
  assert.equal(paired.candidate.role, "candidate");
  assert.equal(paired.comparisonEligibility.eligible, false);
  assert.equal(
    pooledRow(paired.baseline, ONCE_FIXTURE.operationId, "once-per-run")
      .latencyNanoseconds.p50Nanoseconds,
    30,
  );
  assert.equal(
    pooledRow(paired.candidate, ONCE_FIXTURE.operationId, "once-per-run")
      .latencyNanoseconds.p50Nanoseconds,
    60,
  );
  assert.equal(
    paired.sourceAccountingPair.matched.baselineAccountingSha256,
    paired.baseline.sourceAccounting.accountingDigest.value,
  );
  assert.equal(
    paired.sourceAccountingPair.matched.candidateAccountingSha256,
    paired.candidate.sourceAccounting.accountingDigest.value,
  );
  assertDeepFrozen(paired);
});

// verify the public boundary cannot accept aggregate, role, or schedule substitutions
test("rejects supplied aggregates and alternate schedule or role inputs", () => {
  const { baseline, candidate } = exactBindings();
  const outcomes = fullOutcomes();
  assert.throws(
    // reject one caller-supplied aggregate field
    () => evaluateBackendPerformanceLatencyAccounting({
      aggregate: {},
      binding: baseline,
      outcomes,
    }),
    /only binding and outcomes/u,
  );
  assert.throws(
    // reject one caller-supplied schedule selector
    () => evaluateBackendPerformanceLatencyAccounting({
      binding: baseline,
      outcomes,
      schedule: "alternate",
    }),
    /only binding and outcomes/u,
  );
  assert.throws(
    // reject candidate use through the baseline-only solo boundary
    () => evaluateBackendPerformanceLatencyAccounting({
      binding: candidate,
      outcomes,
    }),
    /baseline binding/u,
  );
  assert.throws(
    // reject extra public arguments
    () => evaluateBackendPerformanceLatencyAccounting(
      { binding: baseline, outcomes },
      { eligible: true },
    ),
    /exactly one raw baseline input/u,
  );
});
