import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  appendFile,
  cp,
  mkdtemp,
  mkdir,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  BACKEND_PERFORMANCE_AMENDED_DEFINITION_SHA256,
  BACKEND_PERFORMANCE_AMENDED_SCHEDULE_VERSION,
  buildBackendPerformanceAmendedSchedule,
} from "./backend-performance-amended-schedule.mjs";
const {
  BACKEND_PERFORMANCE_SCHEDULE_VERSION,
  BACKEND_PERFORMANCE_WORKLOAD_VERSION,
  buildBackendPerformanceWorkloadPlan,
} = await import("./backend-performance-workload.mjs");

const TEST_DATA = JSON.parse(await readFile(
  new URL("./backend-performance-amended-schedule.test-data.json", import.meta.url),
  "utf8",
));
const SOURCE_WORKLOAD = JSON.parse(await readFile(
  new URL("../config/backend-performance-workload-v2.json", import.meta.url),
  "utf8",
));
const DISPOSITION = JSON.parse(await readFile(
  new URL("../config/backend-performance-amended-disposition-v1.json", import.meta.url),
  "utf8",
));
const OMITTED_OPERATION_ID_SET = new Set(TEST_DATA.reviewedOmittedOperationIds);

// hash exact source bytes
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// compare strings by ECMAScript code-unit order
function compareText(left, right) {
  // retain equal keys without locale behavior
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

// independently canonicalize one closed JSON value
function canonicalJson(value) {
  // preserve null as its JSON primitive
  if (value === null) {
    return "null";
  }

  // preserve array order while normalizing every child
  if (Array.isArray(value)) {
    return `[${value.map(
      // canonicalize one declared array element
      (child) => canonicalJson(child),
    ).join(",")}]`;
  }

  // normalize plain object keys by code-unit order
  if (typeof value === "object") {
    return `{${Object.keys(value).sort(compareText).map(
      // bind each normalized key to its normalized value
      (key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`,
    ).join(",")}}`;
  }
  return JSON.stringify(value);
}

// rebuild one value with deliberately reversed object insertion order
function reverseObjectOrder(value) {
  // preserve declared array order while visiting children
  if (Array.isArray(value)) {
    return value.map(
      // reverse object order inside one array child
      (child) => reverseObjectOrder(child),
    );
  }

  // reverse every plain object key before rebuilding it
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).reverse().map(
      // rebuild one child under its original key
      ([key, child]) => [key, reverseObjectOrder(child)],
    ));
  }
  return value;
}

// flatten every ordered attempt in one plan
function flattenAttempts(plan) {
  return plan.runSlots.flatMap(
    // retain source run-slot order
    (runSlot) => runSlot.groups.flatMap(
      // retain source group and attempt order
      (group) => group.attempts,
    ),
  );
}

// assert recursive immutability of one plan value
function assertDeepFrozen(value) {
  // inspect only nested object values
  if (value !== null && typeof value === "object") {
    assert.equal(Object.isFrozen(value), true);

    // visit every nested plan value
    for (const child of Object.values(value)) {
      assertDeepFrozen(child);
    }
  }
}

// retain only source-owned attempt execution fields
function sourceAttemptFields(attempt) {
  return {
    deadlineMs: attempt.deadlineMs,
    groupId: attempt.groupId,
    operationId: attempt.operationId,
    runSlotId: attempt.runSlotId,
    stratum: attempt.stratum,
    timeoutPolicy: attempt.timeoutPolicy,
  };
}

const MIRROR_FILES = Object.freeze([
  ["scripts/backend-performance-amended-schedule.mjs", new URL("./backend-performance-amended-schedule.mjs", import.meta.url)],
  ["scripts/backend-performance-workload.mjs", new URL("./backend-performance-workload.mjs", import.meta.url)],
  ["config/backend-performance-workload-v2.json", new URL("../config/backend-performance-workload-v2.json", import.meta.url)],
  ["config/backend-performance-amended-disposition-v1.json", new URL("../config/backend-performance-amended-disposition-v1.json", import.meta.url)],
]);

// import one isolated mirror after a single raw-byte mutation
async function importMutatedMirror(relativePath, replacement = null) {
  const mirrorRoot = await mkdtemp(join(tmpdir(), "weather-amended-schedule-"));

  // always remove the bounded fixture mirror
  try {
    // copy only the four exact source inputs
    for (const [destination, source] of MIRROR_FILES) {
      const destinationPath = join(mirrorRoot, destination);
      await mkdir(dirname(destinationPath), { recursive: true });
      await cp(fileURLToPath(source), destinationPath);
    }
    // install a sentinel source or one harmless byte-level drift
    if (replacement === null) {
      await appendFile(join(mirrorRoot, relativePath), "\n");
    } else {
      await writeFile(join(mirrorRoot, relativePath), replacement);
    }
    const candidateUrl = pathToFileURL(join(
      mirrorRoot,
      "scripts/backend-performance-amended-schedule.mjs",
    ));
    candidateUrl.searchParams.set("mutation", relativePath);
    return await import(candidateUrl.href);
  } finally {
    await rm(mirrorRoot, { force: true, recursive: true });
  }
}

// require one deterministic no-input plan for both measurement sides
test("amended schedule is deterministic frozen and accepts no selectors", () => {
  const first = buildBackendPerformanceAmendedSchedule();
  const second = buildBackendPerformanceAmendedSchedule();
  assert.deepEqual(second, first);
  assertDeepFrozen(first);
  assert.equal(first.canonicalEligible, false);
  assert.equal(first.contractVersion, TEST_DATA.amendedScheduleVersion);
  assert.equal(first.contractVersion, BACKEND_PERFORMANCE_AMENDED_SCHEDULE_VERSION);
  assert.equal(first.projection.baselineAndCandidateSamePlan, true);
  assert.throws(
    // reject a baseline selector
    () => buildBackendPerformanceAmendedSchedule("baseline"),
    TypeError,
  );
  assert.throws(
    // reject a candidate selector
    () => buildBackendPerformanceAmendedSchedule("candidate"),
    TypeError,
  );
  assert.throws(
    // reject a caller supplied definition
    () => buildBackendPerformanceAmendedSchedule(SOURCE_WORKLOAD),
    TypeError,
  );
});

// prove exact portable config parent pins and separate definition identities
test("amended schedule pins exact portable disposition and parent identities", async () => {
  const sourceCases = [
    ["workload", new URL("../config/backend-performance-workload-v2.json", import.meta.url)],
    ["builder", new URL("./backend-performance-workload.mjs", import.meta.url)],
    ["disposition", new URL("../config/backend-performance-amended-disposition-v1.json", import.meta.url)],
  ];

  // compare every raw source with the reviewed exact pin
  for (const [sourceKey, sourceUrl] of sourceCases) {
    const sourceBytes = await readFile(sourceUrl);
    assert.equal(sourceBytes.length, TEST_DATA.sourcePins[sourceKey].bytes);
    assert.equal(sha256(sourceBytes), TEST_DATA.sourcePins[sourceKey].sha256);
  }
  const plan = buildBackendPerformanceAmendedSchedule();
  assert.deepEqual(plan.projection.sourcePins, {
    disposition: {
      path: "config/backend-performance-amended-disposition-v1.json",
      ...TEST_DATA.sourcePins.disposition,
    },
    builder: {
      path: "scripts/backend-performance-workload.mjs",
      ...TEST_DATA.sourcePins.builder,
    },
    workload: {
      path: "config/backend-performance-workload-v2.json",
      ...TEST_DATA.sourcePins.workload,
    },
  });
  assert.deepEqual(plan.definition, {
    contractVersion: TEST_DATA.amendedDefinitionVersion,
    sha256: TEST_DATA.amendedDefinitionSha256,
  });
  assert.deepEqual(plan.parentDefinition, {
    contractVersion: BACKEND_PERFORMANCE_SCHEDULE_VERSION,
    sha256: TEST_DATA.sourceScheduleDefinitionSha256,
  });
  assert.deepEqual(plan.projection.sourceSchedule, {
    contractVersion: BACKEND_PERFORMANCE_SCHEDULE_VERSION,
    definitionSha256: TEST_DATA.sourceScheduleDefinitionSha256,
  });
  assert.equal(BACKEND_PERFORMANCE_WORKLOAD_VERSION, TEST_DATA.sourceWorkloadVersion);
  assert.equal(BACKEND_PERFORMANCE_SCHEDULE_VERSION, TEST_DATA.sourceScheduleVersion);
});

// prove one closed canonical amended definition digest
test("amended definition digest is normalized distinct and semantics-sensitive", () => {
  const canonical = canonicalJson(DISPOSITION.amendedDefinition);
  const reordered = reverseObjectOrder(DISPOSITION.amendedDefinition);
  const changed = structuredClone(DISPOSITION.amendedDefinition);
  changed.totals.plannedAttempts += 1;
  assert.equal(sha256(Buffer.from(canonical, "utf8")), TEST_DATA.amendedDefinitionSha256);
  assert.equal(DISPOSITION.amendedDefinitionSha256, TEST_DATA.amendedDefinitionSha256);
  assert.equal(
    sha256(Buffer.from(canonicalJson(reordered), "utf8")),
    TEST_DATA.amendedDefinitionSha256,
  );
  assert.notEqual(
    sha256(Buffer.from(canonicalJson(changed), "utf8")),
    TEST_DATA.amendedDefinitionSha256,
  );
  assert.notEqual(TEST_DATA.amendedDefinitionSha256, TEST_DATA.sourceScheduleDefinitionSha256);
  assert.equal(
    BACKEND_PERFORMANCE_AMENDED_DEFINITION_SHA256,
    TEST_DATA.amendedDefinitionSha256,
  );
});

// prove every retained attempt is an exact source attempt projection
test("amended schedule preserves the complete retained denominator", () => {
  const sourcePlan = buildBackendPerformanceWorkloadPlan();
  const plan = buildBackendPerformanceAmendedSchedule();
  const sourceAttempts = flattenAttempts(sourcePlan);
  const retainedSourceAttempts = sourceAttempts.filter(
    // retain only source attempts outside the reviewed omission set
    (attempt) => !OMITTED_OPERATION_ID_SET.has(attempt.operationId),
  );
  const projectedAttempts = flattenAttempts(plan);
  const sourceByAttemptId = new Map(sourceAttempts.map(
    // index each original source identity
    (attempt) => [attempt.attemptId, attempt],
  ));
  const projectedAttemptIds = new Set();
  const originalProvenanceIds = new Set();
  const stratumCounts = new Map();
  const operationIds = new Set();
  let publicGetAttempts = 0;

  assert.equal(sourceAttempts.length, 6290);
  assert.equal(retainedSourceAttempts.length, TEST_DATA.counts.plannedAttempts);
  assert.equal(projectedAttempts.length, TEST_DATA.counts.plannedAttempts);

  // compare every projected attempt with its exact original row
  for (const attempt of projectedAttempts) {
    const sourceAttempt = sourceByAttemptId.get(attempt.originalAttemptId);
    assert.ok(sourceAttempt);
    assert.equal(OMITTED_OPERATION_ID_SET.has(attempt.operationId), false);
    assert.deepEqual(sourceAttemptFields(attempt), sourceAttemptFields(sourceAttempt));
    assert.equal(attempt.originalOrdinal, sourceAttempt.ordinal);
    assert.notEqual(attempt.attemptId, sourceAttempt.attemptId);
    assert.match(attempt.attemptId, /^planned-v3:/u);
    assert.equal(projectedAttemptIds.has(attempt.attemptId), false);
    assert.equal(originalProvenanceIds.has(attempt.originalAttemptId), false);
    projectedAttemptIds.add(attempt.attemptId);
    originalProvenanceIds.add(attempt.originalAttemptId);
    operationIds.add(attempt.operationId);
    stratumCounts.set(attempt.stratum, (stratumCounts.get(attempt.stratum) ?? 0) + 1);

    // count only ordinary public matrix attempts
    if (attempt.groupId === TEST_DATA.publicGroupId) {
      publicGetAttempts += 1;
    }
  }
  assert.deepEqual(
    [...originalProvenanceIds].sort(),
    retainedSourceAttempts.map(
      // recover each retained original attempt identity
      (attempt) => attempt.attemptId,
    ).sort(),
  );
  assert.equal(projectedAttemptIds.size, TEST_DATA.counts.plannedAttempts);
  assert.equal(originalProvenanceIds.size, TEST_DATA.counts.plannedAttempts);
  assert.deepEqual([...projectedAttemptIds].filter(
    // compare new ids against the complete original identity set
    (attemptId) => sourceByAttemptId.has(attemptId),
  ), []);
  assert.equal(operationIds.size, TEST_DATA.counts.operations);
  assert.equal(publicGetAttempts, TEST_DATA.counts.publicGetAttempts);
  assert.equal(stratumCounts.get("warm"), TEST_DATA.counts.warmAttempts);
  assert.equal(
    stratumCounts.get("application-cache-cold"),
    TEST_DATA.counts.applicationCacheColdAttempts,
  );
  assert.equal(
    stratumCounts.get("process-pool-cold"),
    TEST_DATA.counts.processPoolColdAttempts,
  );
  assert.equal(stratumCounts.get("db-os-cold"), TEST_DATA.counts.dbOsColdAttempts);
  assert.equal(stratumCounts.get("once-per-run"), TEST_DATA.counts.oncePerRunAttempts);
  assert.deepEqual(plan.totals, {
    applicationCacheColdAttempts: TEST_DATA.counts.applicationCacheColdAttempts,
    dbOsColdAttempts: TEST_DATA.counts.dbOsColdAttempts,
    groupInstances: TEST_DATA.counts.groupInstances,
    oncePerRunAttempts: TEST_DATA.counts.oncePerRunAttempts,
    plannedAttempts: TEST_DATA.counts.plannedAttempts,
    preStateReceipts: TEST_DATA.counts.preStateReceipts,
    processPoolColdAttempts: TEST_DATA.counts.processPoolColdAttempts,
    restoreReceipts: TEST_DATA.counts.restoreReceipts,
    runSlots: TEST_DATA.counts.runSlots,
    warmAttempts: TEST_DATA.counts.warmAttempts,
  });
});

// prove strict new ordinals and unchanged atomic group gates
test("amended schedule keeps group order restoration and contiguous new ordinals", () => {
  const sourcePlan = buildBackendPerformanceWorkloadPlan();
  const plan = buildBackendPerformanceAmendedSchedule();
  assert.equal(plan.runSlots.length, TEST_DATA.counts.runSlots);

  // compare every amended slot with the same source slot
  for (const [slotIndex, runSlot] of plan.runSlots.entries()) {
    const sourceRunSlot = sourcePlan.runSlots[slotIndex];
    assert.equal(runSlot.id, sourceRunSlot.id);
    assert.equal(runSlot.order, sourceRunSlot.order);
    assert.equal(runSlot.groups.length, TEST_DATA.counts.groups);
    const slotAttempts = runSlot.groups.flatMap(
      // retain exact projected group order
      (group) => group.attempts,
    );
    assert.equal(slotAttempts.length, TEST_DATA.counts.attemptsPerRunSlot);
    assert.deepEqual(slotAttempts.map(
      // recover strict projected ordinals
      (attempt) => attempt.ordinal,
    ), Array.from(
      { length: TEST_DATA.counts.attemptsPerRunSlot },
      // build the expected one-based contiguous ordinal sequence
      (_value, index) => index + 1,
    ));

    // compare every group identity dependency and mandatory gate
    for (const [groupIndex, group] of runSlot.groups.entries()) {
      const sourceGroup = sourceRunSlot.groups[groupIndex];
      assert.equal(group.id, sourceGroup.id);
      assert.equal(group.order, sourceGroup.order);
      assert.equal(group.originalOrder, sourceGroup.order);
      assert.equal(group.dependsOnGroupId, sourceGroup.dependsOnGroupId);
      assert.deepEqual(group.preState, sourceGroup.preState);
      assert.deepEqual(group.finalRestore, sourceGroup.finalRestore);
      assert.deepEqual(group.operationOrder.originalOperationIds, SOURCE_WORKLOAD.groups[groupIndex].operationIds);
      assert.deepEqual(group.operationOrder.retained.map(
        // recover contiguous projected group operation orders
        (operation) => operation.projectedOrderWithinGroup,
      ), Array.from(
        { length: group.operationOrder.retained.length },
        // build the expected one-based operation sequence
        (_value, index) => index + 1,
      ));
    }
  }
  const groupInstances = plan.runSlots.reduce(
    // total all preserved group gates
    (total, runSlot) => total + runSlot.groups.length,
    0,
  );
  assert.equal(groupInstances, TEST_DATA.counts.groupInstances);
  assert.deepEqual(plan.retainedOracles.restoration, {
    finalRestoreContractVersion: "weather-backend-group-restore/v1",
    groupInstances: TEST_DATA.counts.groupInstances,
    preStateContractVersion: "weather-backend-group-prestate/v1",
  });
});

// prove reviewed omission provenance without a passing verdict
test("amended schedule reports exactly three intentional omissions", () => {
  const sourcePlan = buildBackendPerformanceWorkloadPlan();
  const plan = buildBackendPerformanceAmendedSchedule();
  const sourceOmittedAttempts = flattenAttempts(sourcePlan).filter(
    // select only reviewed source omissions
    (attempt) => OMITTED_OPERATION_ID_SET.has(attempt.operationId),
  );
  const reportedOriginalAttemptIds = plan.omissions.flatMap(
    // recover every intentionally omitted original identity
    (omission) => omission.originalAttempts.map(
      // retain exact old attempt ids
      (attempt) => attempt.originalAttemptId,
    ),
  );
  assert.deepEqual(plan.projection.omittedOperationIds, TEST_DATA.reviewedOmittedOperationIds);
  assert.deepEqual(plan.omissions.map(
    // retain omission report source order
    (omission) => omission.operationId,
  ), TEST_DATA.omittedSourceOrder);
  assert.equal(reportedOriginalAttemptIds.length, TEST_DATA.counts.omittedAttempts);
  assert.deepEqual(
    reportedOriginalAttemptIds.sort(),
    sourceOmittedAttempts.map(
      // recover omitted source attempt ids
      (attempt) => attempt.attemptId,
    ).sort(),
  );

  // inspect every omission disposition and five-slot provenance
  for (const omission of plan.omissions) {
    assert.equal(omission.disposition, "intentionally-not-run");
    assert.equal(omission.originalAttempts.length, TEST_DATA.counts.runSlots);
    assert.equal(Object.hasOwn(omission, "passed"), false);
  }
});

// prove the one unused binding edge is the only transition removed
test("amended schedule retains exact binding inputs and nine measured transitions", () => {
  const plan = buildBackendPerformanceAmendedSchedule();
  const retainedOperations = SOURCE_WORKLOAD.operations.filter(
    // retain every operation outside the reviewed omission set
    (operation) => !OMITTED_OPERATION_ID_SET.has(operation.id),
  );
  const expectedBindingSlots = [...new Set(retainedOperations.flatMap(
    // include every retained operation input
    (operation) => operation.bindingSlots,
  ))].sort();
  const expectedTransitions = Object.entries(SOURCE_WORKLOAD.transitionProducers).filter(
    // retain only transition slots still consumed by an active operation
    ([bindingSlot]) => expectedBindingSlots.includes(bindingSlot),
  ).sort(
    // match stable source transition order without locale behavior
    ([left], [right]) => compareText(left, right),
  ).map(
    // expose the expected fixed topology
    ([bindingSlot, producerOperationId]) => ({ bindingSlot, producerOperationId }),
  );
  assert.equal(expectedBindingSlots.length, TEST_DATA.counts.bindingSlots);
  assert.deepEqual(plan.projection.retainedBindingSlots, expectedBindingSlots);
  assert.equal(plan.transitions.length, TEST_DATA.counts.transitions);
  assert.deepEqual(plan.transitions, expectedTransitions);
  assert.equal(
    plan.transitions.some(
      // reject the reviewed now-unused edge
      (transition) => transition.bindingSlot === TEST_DATA.orphanedTransition,
    ),
    false,
  );
});

// prove ordinary logout and passive response contracts remain unchanged
test("amended schedule preserves logout and public header oracles", () => {
  const plan = buildBackendPerformanceAmendedSchedule();
  const operationById = new Map(SOURCE_WORKLOAD.operations.map(
    // index source operations for oracle comparison
    (operation) => [operation.id, operation],
  ));
  const adminGroup = SOURCE_WORKLOAD.groups.find(
    // find the exact ordinary admin group
    (group) => group.id === "group.admin-live-safe",
  );
  const logout = operationById.get("admin.live.logout");
  assert.deepEqual(plan.retainedOracles.adminLogout.terminalOracle, logout.terminalOracle);
  assert.equal(plan.retainedOracles.adminLogout.originalOrderWithinGroup, 10);
  assert.equal(plan.retainedOracles.adminLogout.projectedOrderWithinGroup, 8);
  assert.deepEqual(adminGroup.operationIds.filter(
    // retain only amended admin operations
    (operationId) => !OMITTED_OPERATION_ID_SET.has(operationId),
  ).map(
    // recover exact original admin operation orders
    (operationId) => operationById.get(operationId).orderWithinGroup,
  ), TEST_DATA.adminRetainedOriginalOrders);
  const amendedAdminGroup = plan.runSlots[0].groups.find(
    // find the projected admin group
    (group) => group.id === adminGroup.id,
  );
  assert.deepEqual(amendedAdminGroup.operationOrder.retained.map(
    // recover projected admin operation orders
    (operation) => operation.projectedOrderWithinGroup,
  ), TEST_DATA.adminProjectedOrders);
  assert.equal(plan.retainedOracles.publicGet.plannedAttempts, TEST_DATA.counts.publicGetAttempts);
  assert.equal(
    plan.retainedOracles.publicGet.requiredTerminalOracle,
    TEST_DATA.passiveHeaderOracle,
  );

  // require all twenty-eight ordinary public operations to retain the oracle
  for (const operationId of plan.retainedOracles.publicGet.operationIds) {
    assert.equal(
      operationById.get(operationId).terminalOracle.includes(TEST_DATA.passiveHeaderOracle),
      true,
    );
  }
});

// prove every exact source mismatch fails before builder evaluation
test("amended schedule rejects portable config workload and builder drift", async (testContext) => {
  const wrongBuilderSource = [
    "const error = new Error(\"wrong builder evaluated\");",
    `error.code = ${JSON.stringify(TEST_DATA.wrongBuilderSentinelCode)};`,
    "throw error;",
    "",
  ].join("\n");
  const mutationCases = [
    { relativePath: "config/backend-performance-workload-v2.json", replacement: null },
    { relativePath: "config/backend-performance-amended-disposition-v1.json", replacement: null },
    { relativePath: "scripts/backend-performance-workload.mjs", replacement: wrongBuilderSource },
  ];

  // isolate each raw source mismatch in a fresh exact mirror
  for (const { relativePath, replacement } of mutationCases) {
    // verify one isolated raw-byte mutation
    await testContext.test(relativePath, async () => {
      await assert.rejects(
        importMutatedMirror(relativePath, replacement),
        // distinguish pin rejection from sentinel builder evaluation
        (error) => {
          assert.notEqual(error?.code, TEST_DATA.wrongBuilderSentinelCode);
          return error?.code === "amended-schedule.source-pin";
        },
      );
    });
  }
});
