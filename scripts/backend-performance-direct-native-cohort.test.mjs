import assert from "node:assert/strict";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_DIRECT_NATIVE_COHORT_VERSION,
  assembleBackendPerformanceDirectNativeCohort,
} from "./backend-performance-direct-native-cohort.mjs";
import { buildBackendPerformanceFunctionalSchedule } from
  "./backend-performance-functional-schedule.mjs";

// flatten the exact v4 schedule
function plannedAttempts() {
  const schedule = buildBackendPerformanceFunctionalSchedule();
  const attempts = [];
  // retain each source run slot
  for (const runSlot of schedule.runSlots) {
    // retain each source group
    for (const group of runSlot.groups) {
      attempts.push(...group.attempts);
    }
  }
  assert.equal(attempts.length, 6275);
  return attempts;
}

const PLAN = plannedAttempts();

// build one complete raw terminal row
function terminalRow(attempt, index, terminalCategory = "completed") {
  const errorCategory = terminalCategory === "completed"
    ? null
    : terminalCategory === "timeout" ? "deadline-exceeded" : "http";
  return {
    attemptId: attempt.attemptId,
    elapsedNanoseconds: 1000 + (index * 17) % 100_003,
    errorCategory,
    httpStatus: terminalCategory === "timeout"
      ? null
      : terminalCategory === "error" ? 502 : 200,
    operationId: attempt.operationId,
    preparationClassification: attempt.stratum === "warm"
      ? "not-required"
      : `prepared-${attempt.stratum}`,
    requestBodyBytes: index % 11,
    responseBodyBytes: 100 + index % 101,
    runSlotId: attempt.runSlotId,
    semanticClassification: index % (Number(attempt.runSlotId.at(-1)) + 2) === 0
      ? "alternate-observed"
      : "ordinary-observed",
    stratum: attempt.stratum,
    terminalCategory,
  };
}

// build one varied complete cohort
function completeRows() {
  return PLAN.map(
    // retain successes and ordinary failures in one denominator
    (attempt, index) => terminalRow(
      attempt,
      index,
      index % 997 === 0 ? "timeout" : index % 251 === 0 ? "error" : "completed",
    ),
  );
}

// add one closed count record
function countTotal(counts) {
  return Object.values(counts).reduce(
    // add one denominator count
    (total, count) => total + count,
    0,
  );
}

// calculate one independent nearest-rank p95
function expectedP95(rows) {
  const sorted = rows.map(
    // retain one raw elapsed observation
    (row) => row.elapsedNanoseconds,
  ).sort(
    // sort exact integer observations
    (left, right) => left - right,
  );
  return sorted[Math.ceil(sorted.length * 0.95) - 1];
}

test("assembles complete exact five-slot custody and raw p95 observations", () => {
  const terminalRows = completeRows();
  const cohort = assembleBackendPerformanceDirectNativeCohort({
    enteredIncompleteAttemptIds: [],
    terminalRows,
  });
  const overall = cohort.summaries.overall;

  assert.equal(cohort.contractVersion, BACKEND_PERFORMANCE_DIRECT_NATIVE_COHORT_VERSION);
  assert.equal(cohort.status, "attempt-custody-complete");
  assert.deepEqual(cohort.gates, {
    attemptCustody: "passed",
    officialTarget: "not-evaluated",
    resourceEvidence: "not-evaluated",
    sourceEvidence: "not-evaluated",
  });
  assert.equal(cohort.nativeEligible, false);
  assert.equal(cohort.officialTarget, null);
  assert.deepEqual(cohort.attemptCustody, {
    complete: true,
    enteredAttempts: 6275,
    enteredIncompleteAttemptIds: [],
    missingAttemptIds: [],
    plannedAttempts: 6275,
    status: "complete",
    terminalAttempts: 6275,
  });
  assert.equal(overall.sourceAttemptIds.length, 6275);
  assert.equal(overall.terminalAttemptIds.length, 6275);
  assert.equal(overall.denominator.complete, true);
  assert.equal(countTotal(overall.terminalCounts), 6275);
  assert.equal(countTotal(overall.errorCategoryCounts), 6275);
  assert.equal(countTotal(overall.statusCounts), 6275);
  assert.equal(overall.bytes.observedAttempts, 6275);
  assert.equal(overall.bytes.requestBodyBytes, terminalRows.reduce(
    // sum one raw request byte count
    (total, row) => total + row.requestBodyBytes,
    0,
  ));
  assert.equal(overall.bytes.responseBodyBytes, terminalRows.reduce(
    // sum one raw response byte count
    (total, row) => total + row.responseBodyBytes,
    0,
  ));
  assert.equal(overall.elapsedNanoseconds.p95, expectedP95(terminalRows));
  assert.equal(cohort.summaries.runSlots.length, 5);
  assert.deepEqual(cohort.summaries.runSlots.map(
    // retain each exact slot denominator
    (slot) => slot.denominator.plannedAttempts,
  ), [1255, 1255, 1255, 1255, 1255]);
  assert.ok(cohort.summaries.operationStrata.every(
    // require exact source custody for each pooled bucket
    (summary) => summary.sourceAttemptIds.length === summary.denominator.plannedAttempts,
  ));
  assert.ok(cohort.summaries.runSlotOperationStrata.every(
    // require exact source custody for each slot bucket
    (summary) => summary.sourceAttemptIds.length === summary.denominator.plannedAttempts,
  ));
  const warmObservation = cohort.latencyObservations.find(
    // select one full multi-attempt warm observation
    (observation) => observation.stratum === "warm"
      && observation.sourceAttemptIds.length > 1,
  );
  assert.ok(warmObservation);
  const rowsById = new Map(terminalRows.map(
    // index one measured row
    (row) => [row.attemptId, row],
  ));
  const observationRows = warmObservation.sourceAttemptIds.map(
    // recover each exact raw source row
    (attemptId) => rowsById.get(attemptId),
  );
  assert.equal(warmObservation.value, expectedP95(observationRows));
  assert.equal(Object.isFrozen(cohort), true);
  assert.equal(Object.isFrozen(cohort.summaries.runSlots[0].sourceAttemptIds), true);
});

test("keeps differing measured classifications across repetitions", () => {
  const cohort = assembleBackendPerformanceDirectNativeCohort({
    enteredIncompleteAttemptIds: [],
    terminalRows: completeRows(),
  });
  const warmOperation = PLAN.find(
    // select one exact warm operation
    (attempt) => attempt.stratum === "warm",
  ).operationId;
  const repetitions = cohort.summaries.runSlotOperationStrata.filter(
    // retain the selected operation's warm repetitions
    (summary) => summary.operationId === warmOperation && summary.stratum === "warm",
  );

  assert.equal(repetitions.length, 5);
  assert.ok(new Set(repetitions.map(
    // retain each actual classification distribution
    (summary) => JSON.stringify(summary.classifications.semantic),
  )).size > 1);
  assert.ok(repetitions.every(
    // preserve each complete repetition independently
    (summary) => summary.denominator.complete && summary.elapsedNanoseconds.p95 !== null,
  ));
});

test("blocks the current-shaped 5600 plus 18 terminal and one incomplete cohort", () => {
  const warmBySlot = Map.groupBy(PLAN.filter(
    // retain the entire exact warm schedule
    (attempt) => attempt.stratum === "warm",
  ), (attempt) => attempt.runSlotId);
  const warmErrorCounts = new Map([
    ["run-slot-01", 2],
    ["run-slot-02", 4],
    ["run-slot-03", 2],
    ["run-slot-04", 2],
    ["run-slot-05", 2],
  ]);
  const terminalRows = [];
  let index = 0;
  // reproduce the observed per-slot warm error distribution
  for (const [runSlotId, attempts] of warmBySlot) {
    const failures = warmErrorCounts.get(runSlotId);
    // retain every warm terminal attempt
    for (let slotIndex = 0; slotIndex < attempts.length; slotIndex += 1) {
      terminalRows.push(terminalRow(
        attempts[slotIndex],
        index,
        slotIndex < failures ? "error" : "completed",
      ));
      index += 1;
    }
  }
  const firstSlotDbCold = PLAN.filter(
    // retain the exact first-slot db cold source order
    (attempt) => attempt.runSlotId === "run-slot-01"
      && attempt.stratum === "db-os-cold",
  );
  // retain the eighteen measured cold terminal rows
  for (const attempt of firstSlotDbCold.slice(0, 18)) {
    terminalRows.push(terminalRow(attempt, index));
    index += 1;
  }
  const incompleteAttemptId = firstSlotDbCold[18].attemptId;
  const cohort = assembleBackendPerformanceDirectNativeCohort({
    enteredIncompleteAttemptIds: [incompleteAttemptId],
    terminalRows,
  });

  assert.equal(incompleteAttemptId,
    "planned-functional-v4-r2:run-slot-01:1170:api.trends:db-os-cold");
  assert.equal(cohort.status, "blocked");
  assert.equal(cohort.nativeEligible, false);
  assert.equal(cohort.officialTarget, null);
  assert.deepEqual(cohort.attemptCustody, {
    complete: false,
    enteredAttempts: 5619,
    enteredIncompleteAttemptIds: [incompleteAttemptId],
    missingAttemptIds: cohort.attemptCustody.missingAttemptIds,
    plannedAttempts: 6275,
    status: "blocked",
    terminalAttempts: 5618,
  });
  assert.equal(cohort.attemptCustody.missingAttemptIds.length, 656);
  assert.equal(cohort.summaries.overall.terminalCounts.completed, 5606);
  assert.equal(cohort.summaries.overall.terminalCounts.error, 12);
  assert.equal(cohort.summaries.overall.terminalCounts.timeout, 0);
  assert.equal(countTotal(cohort.summaries.overall.statusCounts), 5618);
  assert.equal(cohort.summaries.overall.elapsedNanoseconds.p95, null);
  assert.equal(cohort.summaries.runSlots[0].denominator.terminalAttempts, 1138);
  assert.equal(cohort.summaries.runSlots[0].denominator.enteredIncompleteAttempts, 1);
  assert.equal(cohort.summaries.runSlots[0].denominator.missingAttempts, 116);
  assert.deepEqual(cohort.summaries.runSlots.slice(1).map(
    // retain each later-slot terminal denominator
    (slot) => slot.denominator.terminalAttempts,
  ), [1120, 1120, 1120, 1120]);
  assert.ok(cohort.latencyObservations.filter(
    // retain every warm metric repetition
    (observation) => observation.stratum === "warm",
  ).every(
    // preserve complete warm p95 observations despite blocked total custody
    (observation) => observation.value !== null,
  ));
  const incompleteObservation = cohort.latencyObservations.find(
    // locate the entered incomplete attempt's exact bucket
    (observation) => observation.sourceAttemptIds.includes(incompleteAttemptId),
  );
  assert.ok(incompleteObservation);
  assert.equal(incompleteObservation.value, null);
  assert.equal(incompleteObservation.observedAttempts, 0);
});

test("rejects duplicate unknown cross-slot and malformed terminal evidence", () => {
  const source = PLAN[0];
  const row = terminalRow(source, 0);
  assert.throws(
    () => assembleBackendPerformanceDirectNativeCohort({
      enteredIncompleteAttemptIds: [],
      terminalRows: [row, { ...row }],
    }),
    { code: "direct-native-cohort.duplicate-attempt" },
  );
  assert.throws(
    () => assembleBackendPerformanceDirectNativeCohort({
      enteredIncompleteAttemptIds: [],
      terminalRows: [{ ...row, attemptId: "planned-functional-v4-r2:unknown" }],
    }),
    { code: "direct-native-cohort.attempt-id" },
  );
  assert.throws(
    () => assembleBackendPerformanceDirectNativeCohort({
      enteredIncompleteAttemptIds: [],
      terminalRows: [{ ...row, runSlotId: "run-slot-02" }],
    }),
    { code: "direct-native-cohort.attempt-binding" },
  );
  const { responseBodyBytes, ...incompleteRow } = row;
  assert.equal(responseBodyBytes, 100);
  assert.throws(
    () => assembleBackendPerformanceDirectNativeCohort({
      enteredIncompleteAttemptIds: [],
      terminalRows: [incompleteRow],
    }),
    { code: "direct-native-cohort.schema" },
  );
  assert.throws(
    () => assembleBackendPerformanceDirectNativeCohort({
      enteredIncompleteAttemptIds: [source.attemptId],
      terminalRows: [row],
    }),
    { code: "direct-native-cohort.duplicate-attempt" },
  );
});
