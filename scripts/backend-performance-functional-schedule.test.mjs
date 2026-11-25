import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
  BACKEND_PERFORMANCE_FUNCTIONAL_ORACLE_VERSION,
  BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";
import { buildBackendPerformanceAmendedSchedule } from "./backend-performance-amended-schedule.mjs";

const DISPOSITION_BYTES = await readFile(
  new URL("../config/backend-performance-functional-disposition-v4.json", import.meta.url),
);
const DISPOSITION = JSON.parse(DISPOSITION_BYTES.toString("utf8"));
const DEFINITION = DISPOSITION.dispositionDefinition;

// hash exact bytes
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// compare text portably
function compareText(left, right) {
  // preserve equal values
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

// serialize closed json
function canonicalJson(value) {
  // preserve null
  if (value === null) {
    return "null";
  }

  // preserve array order
  if (Array.isArray(value)) {
    return `[${value.map(
      // serialize one child
      (child) => canonicalJson(child),
    ).join(",")}]`;
  }

  // normalize object keys
  if (typeof value === "object") {
    return `{${Object.keys(value).sort(compareText).map(
      // serialize one property
      (key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`,
    ).join(",")}}`;
  }
  return JSON.stringify(value);
}

// flatten ordered attempts
function flattenAttempts(plan) {
  return plan.runSlots.flatMap(
    // retain run-slot order
    (runSlot) => runSlot.groups.flatMap(
      // retain group order
      (group) => group.attempts,
    ),
  );
}

// assert recursive immutability
function assertDeepFrozen(value) {
  // inspect nested objects
  if (value !== null && typeof value === "object") {
    assert.equal(Object.isFrozen(value), true);

    // visit every child
    for (const child of Object.values(value)) {
      assertDeepFrozen(child);
    }
  }
}

// index reviewed oracles
function functionalOracleMap(plan) {
  return new Map(plan.functionalOracles.operations.map(
    // bind one operation row
    (operation) => [operation.operationId, operation.projectedTerminalOracle],
  ));
}

// verify reviewed identity
test("pins the reviewed functional definition", () => {
  assert.equal(DISPOSITION_BYTES.length, 167798);
  assert.equal(
    sha256(DISPOSITION_BYTES),
    "b8d42fb0ebd193504226b0f802200d2aacf04203444d53bec33233782235c280",
  );
  assert.equal(
    sha256(Buffer.from(canonicalJson(DEFINITION), "utf8")),
    "7cef887550b999edd4af2e5095778031abd2ec3a454e5a2324c67556c31f9be9",
  );
  assert.equal(
    BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
    DISPOSITION.dispositionDefinitionSha256,
  );
  assert.equal(BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION, DEFINITION.namespaces.schedule);
  assert.equal(BACKEND_PERFORMANCE_FUNCTIONAL_ORACLE_VERSION, DEFINITION.namespaces.oracle);
});

// reject runtime selectors
test("rejects every runtime selector", () => {
  // reject one string selector
  assert.throws(
    () => buildBackendPerformanceFunctionalSchedule("node-v1"),
    /accepts no runtime input/,
  );
  // reject one object selector
  assert.throws(
    () => buildBackendPerformanceFunctionalSchedule({ generation: "rust-v1" }),
    /accepts no runtime input/,
  );
});

// verify deterministic output
test("builds one deterministic deeply frozen data plan", () => {
  const first = buildBackendPerformanceFunctionalSchedule();
  const second = buildBackendPerformanceFunctionalSchedule();
  assert.notStrictEqual(first, second);
  assert.deepEqual(first, second);
  assert.equal(first.canonicalEligible, false);
  assert.equal(first.contractVersion, DEFINITION.namespaces.schedule);
  assert.equal(first.definition.sha256, DISPOSITION.dispositionDefinitionSha256);
  assert.equal(
    first.comparison.normalLogoutObservation,
    DEFINITION.comparisonContract.normalLogoutObservation,
  );
  assert.equal(Object.hasOwn(first, "retainedOracles"), false);
  assertDeepFrozen(first);
});

// verify exact attempt projection
test("projects every parent attempt once without changing work", () => {
  const parent = buildBackendPerformanceAmendedSchedule();
  const functional = buildBackendPerformanceFunctionalSchedule();
  const parentAttempts = flattenAttempts(parent);
  const functionalAttempts = flattenAttempts(functional);
  assert.equal(functional.runSlots.length, 5);
  assert.equal(functional.runSlots.every(
    // require all parent groups
    (runSlot) => runSlot.groups.length === 59,
  ), true);
  assert.equal(functional.runSlots.every(
    // require one full slot denominator
    (runSlot) => runSlot.groups.reduce(
      // count one group denominator
      (total, group) => total + group.attempts.length,
      0,
    ) === 1255,
  ), true);
  assert.equal(parentAttempts.length, 6275);
  assert.equal(functionalAttempts.length, parentAttempts.length);
  const parentAttemptIds = new Set(parentAttempts.map(
    // retain one parent id
    (attempt) => attempt.attemptId,
  ));
  const functionalAttemptIds = new Set(functionalAttempts.map(
    // retain one projected id
    (attempt) => attempt.attemptId,
  ));
  assert.equal(parentAttemptIds.size, 6275);
  assert.equal(functionalAttemptIds.size, 6275);

  // compare every projected group
  for (let slotIndex = 0; slotIndex < parent.runSlots.length; slotIndex += 1) {
    const parentSlot = parent.runSlots[slotIndex];
    const functionalSlot = functional.runSlots[slotIndex];
    assert.equal(functionalSlot.id, parentSlot.id);
    assert.equal(functionalSlot.order, parentSlot.order);

    // compare one slot's groups
    for (let groupIndex = 0; groupIndex < parentSlot.groups.length; groupIndex += 1) {
      const parentGroup = parentSlot.groups[groupIndex];
      const functionalGroup = functionalSlot.groups[groupIndex];
      assert.equal(functionalGroup.id, parentGroup.id);
      assert.equal(functionalGroup.order, parentGroup.order);
      assert.equal(functionalGroup.originalOrder, parentGroup.originalOrder);
      assert.equal(functionalGroup.dependsOnGroupId, parentGroup.dependsOnGroupId);
      assert.deepEqual(functionalGroup.preState, parentGroup.preState);
      assert.deepEqual(functionalGroup.finalRestore, parentGroup.finalRestore);
      assert.deepEqual(functionalGroup.operationOrder, parentGroup.operationOrder);
    }
  }

  // compare every projected attempt
  for (let index = 0; index < parentAttempts.length; index += 1) {
    const source = parentAttempts[index];
    const projected = functionalAttempts[index];
    assert.equal(projected.parentAttemptId, source.attemptId);
    assert.equal(projected.operationId, source.operationId);
    assert.equal(projected.groupId, source.groupId);
    assert.equal(projected.runSlotId, source.runSlotId);
    assert.equal(projected.ordinal, source.ordinal);
    assert.equal(projected.stratum, source.stratum);
    assert.equal(projected.deadlineMs, source.deadlineMs);
    assert.equal(projected.timeoutPolicy, source.timeoutPolicy);
    assert.equal(projected.originalAttemptId, source.originalAttemptId);
    assert.equal(projected.originalOrdinal, source.originalOrdinal);
    assert.equal(projected.originalOrderWithinGroup, source.originalOrderWithinGroup);
    assert.equal(projected.projectedOrderWithinGroup, source.projectedOrderWithinGroup);
    assert.equal(projected.attemptId.startsWith(DEFINITION.namespaces.futureAttemptIdPrefix), true);
    assert.equal(parentAttemptIds.has(projected.attemptId), false);
  }
  assert.deepEqual(functional.totals, parent.totals);
  assert.deepEqual(functional.transitions, parent.transitions);
});

// verify reviewed oracles
test("exposes only the reviewed functional oracle map", () => {
  const plan = buildBackendPerformanceFunctionalSchedule();
  const actual = plan.functionalOracles.operations;
  assert.deepEqual(actual, DEFINITION.projectedOperations);
  assert.equal(actual.length, 104);
  const oracleByOperation = functionalOracleMap(plan);
  assert.deepEqual(
    oracleByOperation.get("page.home"),
    [
      "status=200",
      "body-complete",
      "source-validator=html-shell",
      "freshness/source/provenance match immutable binding",
    ],
  );
  assert.deepEqual(
    oracleByOperation.get("admin.live.login"),
    ["status=303", "location=/admin"],
  );
  assert.deepEqual(
    oracleByOperation.get("admin.live.read-page"),
    [
      "status=200",
      "body-complete",
      "source-validator=admin-html-shell",
      "authenticated representation",
    ],
  );
  assert.deepEqual(
    oracleByOperation.get("admin.live.reject-invalid"),
    ["status=400", "body exact UTF-8: invalid request\\n", "settings bytes/semantics unchanged"],
  );
  assert.deepEqual(
    oracleByOperation.get("admin.disposable.logout"),
    ["status=303", "normal logout completed"],
  );
});
