import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const SOURCE_PATHS = Object.freeze({
  disposition: "config/backend-performance-functional-disposition-v4.json",
  parentBuilder: "scripts/backend-performance-amended-schedule.mjs",
});
const SOURCE_URLS = Object.freeze({
  disposition: new URL(
    "../config/backend-performance-functional-disposition-v4.json",
    import.meta.url,
  ),
  parentBuilder: new URL("./backend-performance-amended-schedule.mjs", import.meta.url),
});
const FIXED_DISPOSITION_PIN = Object.freeze({
  bytes: 167798,
  sha256: "b8d42fb0ebd193504226b0f802200d2aacf04203444d53bec33233782235c280",
});
const FIXED_DEFINITION_SHA256 =
  "7cef887550b999edd4af2e5095778031abd2ec3a454e5a2324c67556c31f9be9";
const FIXED_PARENT_BUILDER_PIN = Object.freeze({
  bytes: 23310,
  sha256: "740e411e731f8059a7614d3d9d4a57f260f1157123a776fad75e42c0e22a220e",
});
const EXPECTED_PARENT_DEFINITION_SHA256 =
  "73883cdde0ec1ddfe7f4cc624e82c652f7f56727744b2e654833fd3b996f98ce";

// create one coded failure
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one fixed invariant
function requireInvariant(condition, code, message) {
  // fail closed on drift
  if (!condition) {
    fail(code, message);
  }
}

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

  // retain json primitives
  if (typeof value === "string" || typeof value === "boolean"
    || (typeof value === "number" && Number.isFinite(value))) {
    return JSON.stringify(value);
  }
  fail("functional-schedule.canonicalization", "functional definition is not closed JSON");
}

// clone one closed value
function cloneClosed(value) {
  // preserve array values
  if (Array.isArray(value)) {
    return value.map(
      // clone one child
      (child) => cloneClosed(child),
    );
  }

  // preserve object values
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(
      // clone one property
      ([key, child]) => [key, cloneClosed(child)],
    ));
  }
  return value;
}

// freeze one complete value
function deepFreeze(value) {
  // freeze nested objects first
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    // visit every nested child
    for (const child of Object.values(value)) {
      deepFreeze(child);
    }
    Object.freeze(value);
  }
  return value;
}

// read one pinned source
function readPinnedSource(sourceKey, expectedPin) {
  const bytes = readFileSync(SOURCE_URLS[sourceKey]);
  const actualSha256 = sha256(bytes);

  // reject raw drift
  if (bytes.length !== expectedPin.bytes || actualSha256 !== expectedPin.sha256) {
    fail(
      "functional-schedule.source-pin",
      `${SOURCE_PATHS[sourceKey]} raw identity mismatch`,
    );
  }
  return bytes;
}

const DISPOSITION_BYTES = readPinnedSource("disposition", FIXED_DISPOSITION_PIN);
const DISPOSITION = JSON.parse(DISPOSITION_BYTES.toString("utf8"));
const DEFINITION = DISPOSITION.dispositionDefinition;

requireInvariant(
  DISPOSITION.contractVersion
    === "weather-backend-performance-functional-oracle-disposition/v4-r2-draft"
    && DEFINITION.contractVersion
      === "weather-backend-performance-functional-oracle-definition/v4-r2-draft"
    && DISPOSITION.dispositionDefinitionSha256 === FIXED_DEFINITION_SHA256
    && sha256(Buffer.from(canonicalJson(DEFINITION), "utf8")) === FIXED_DEFINITION_SHA256
    && DEFINITION.status === "static-data-draft-not-executable",
  "functional-schedule.definition-identity",
  "reviewed functional definition identity changed",
);

readPinnedSource("parentBuilder", FIXED_PARENT_BUILDER_PIN);
const PARENT_MODULE = await import(SOURCE_URLS.parentBuilder.href);
const {
  BACKEND_PERFORMANCE_AMENDED_DEFINITION_SHA256,
  BACKEND_PERFORMANCE_AMENDED_SCHEDULE_VERSION,
  buildBackendPerformanceAmendedSchedule,
} = PARENT_MODULE;

requireInvariant(
  BACKEND_PERFORMANCE_AMENDED_DEFINITION_SHA256 === EXPECTED_PARENT_DEFINITION_SHA256
    && typeof BACKEND_PERFORMANCE_AMENDED_SCHEDULE_VERSION === "string"
    && typeof buildBackendPerformanceAmendedSchedule === "function",
  "functional-schedule.parent-contract",
  "checked parent schedule exports changed",
);

export const BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION =
  DEFINITION.namespaces.schedule;
export const BACKEND_PERFORMANCE_FUNCTIONAL_ORACLE_VERSION =
  DEFINITION.namespaces.oracle;
export const BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256 =
  FIXED_DEFINITION_SHA256;

const EXPECTED_TOTALS = Object.freeze({
  ...DEFINITION.topology.totalsInheritedFromAcceptedV3,
});
const PROJECTED_OPERATIONS = Object.freeze(DEFINITION.projectedOperations.map(
  // detach one reviewed oracle row
  (operation) => deepFreeze(cloneClosed(operation)),
));
const PROJECTED_OPERATION_BY_ID = new Map(PROJECTED_OPERATIONS.map(
  // index one reviewed operation
  (operation) => [operation.operationId, operation],
));
const OMITTED_OPERATION_IDS = Object.freeze(DEFINITION.topology.fullOperationOmissions.map(
  // retain one reviewed omission
  (omission) => omission.operationId,
));

// validate reviewed static rows
function verifyFunctionalDefinition() {
  requireInvariant(
    DEFINITION.evidenceLimits.staticProjectionOnly === true
      && DEFINITION.evidenceLimits.executableScheduleProduced === false
      && DEFINITION.comparisonContract.baselineAndCandidateSamePlan === true
      && DEFINITION.legacyExecutionMetadata.inheritedRetainedOracles === false
      && DEFINITION.legacyExecutionMetadata.inheritedScheduleBuilderOutput === false,
    "functional-schedule.evidence-boundary",
    "functional definition evidence boundary changed",
  );
  requireInvariant(
    PROJECTED_OPERATIONS.length === EXPECTED_TOTALS.operations
      && PROJECTED_OPERATION_BY_ID.size === PROJECTED_OPERATIONS.length
      && OMITTED_OPERATION_IDS.length === DEFINITION.coverage.acceptedV3OmittedOperationCount
      && DEFINITION.topology.additionalFullOperationOmissions.length === 0,
    "functional-schedule.operation-count",
    "functional operation topology changed",
  );

  // inspect every reviewed row
  for (let index = 0; index < PROJECTED_OPERATIONS.length; index += 1) {
    const operation = PROJECTED_OPERATIONS[index];
    requireInvariant(
      operation.v3ProjectedOperationIndex === index
        && operation.v3ProjectedOperationOrdinal === index + 1
        && Array.isArray(operation.projectedTerminalOracle)
        && operation.projectedTerminalOracle.length > 0
        && sha256(Buffer.from(canonicalJson(operation.projectedTerminalOracle), "utf8"))
          === operation.projectedTerminalOracleSha256,
      "functional-schedule.operation-row",
      `reviewed functional operation ${operation.operationId} changed`,
    );
  }
}

verifyFunctionalDefinition();

// compare one parent operation list
function sameTextList(left, right) {
  return left.length === right.length
    && left.every(
      // compare one ordered identity
      (value, index) => value === right[index],
    );
}

// validate one parent schedule
function verifyParentSchedule(parent) {
  requireInvariant(
    parent.canonicalEligible === false
      && parent.contractVersion === BACKEND_PERFORMANCE_AMENDED_SCHEDULE_VERSION
      && parent.definition?.sha256 === EXPECTED_PARENT_DEFINITION_SHA256
      && parent.projection?.baselineAndCandidateSamePlan === true
      && parent.projection?.sourcePins?.disposition?.sha256
        === DEFINITION.parents.acceptedV3Disposition.sha256
      && sameTextList(parent.projection.retainedOperationIds, PROJECTED_OPERATIONS.map(
        // expose one reviewed operation identity
        (operation) => operation.operationId,
      ))
      && sameTextList(parent.projection.omittedOperationIds, OMITTED_OPERATION_IDS),
    "functional-schedule.parent-identity",
    "accepted parent schedule identity changed",
  );
  requireInvariant(
    parent.runSlots.length === EXPECTED_TOTALS.runSlots
      && parent.totals.plannedAttempts === EXPECTED_TOTALS.plannedAttempts
      && parent.totals.groupInstances === EXPECTED_TOTALS.groupInstances
      && parent.transitions.length === EXPECTED_TOTALS.measuredTransitions,
    "functional-schedule.parent-totals",
    "accepted parent schedule totals changed",
  );
}

// project one attempt identity
function projectAttempt(sourceAttempt, projectedAttemptIds, parentAttemptIds) {
  const operation = PROJECTED_OPERATION_BY_ID.get(sourceAttempt.operationId);
  requireInvariant(
    operation !== undefined
      && operation.groupId === sourceAttempt.groupId
      && operation.sourceOrderWithinGroup === sourceAttempt.originalOrderWithinGroup
      && operation.v3ProjectedOrderWithinGroup === sourceAttempt.projectedOrderWithinGroup,
    "functional-schedule.attempt-operation",
    `attempt operation ${sourceAttempt.operationId} is not reviewed`,
  );
  requireInvariant(
    !parentAttemptIds.has(sourceAttempt.attemptId),
    "functional-schedule.parent-attempt-id",
    `duplicate parent attempt ${sourceAttempt.attemptId}`,
  );
  parentAttemptIds.add(sourceAttempt.attemptId);
  const attemptId = [
    `${DEFINITION.namespaces.futureAttemptIdPrefix}${sourceAttempt.runSlotId}`,
    String(sourceAttempt.ordinal).padStart(4, "0"),
    sourceAttempt.operationId,
    sourceAttempt.stratum,
  ].join(":");
  requireInvariant(
    !projectedAttemptIds.has(attemptId) && !parentAttemptIds.has(attemptId),
    "functional-schedule.projected-attempt-id",
    `duplicate functional attempt ${attemptId}`,
  );
  projectedAttemptIds.add(attemptId);
  return {
    attemptId,
    deadlineMs: sourceAttempt.deadlineMs,
    groupId: sourceAttempt.groupId,
    operationId: sourceAttempt.operationId,
    ordinal: sourceAttempt.ordinal,
    originalAttemptId: sourceAttempt.originalAttemptId,
    originalOrdinal: sourceAttempt.originalOrdinal,
    originalOrderWithinGroup: sourceAttempt.originalOrderWithinGroup,
    parentAttemptId: sourceAttempt.attemptId,
    projectedOrderWithinGroup: sourceAttempt.projectedOrderWithinGroup,
    runSlotId: sourceAttempt.runSlotId,
    stratum: sourceAttempt.stratum,
    timeoutPolicy: sourceAttempt.timeoutPolicy,
  };
}

// project one complete group
function projectGroup(sourceGroup, projectedAttemptIds, parentAttemptIds) {
  const attempts = sourceGroup.attempts.map(
    // project one exact attempt
    (attempt) => projectAttempt(attempt, projectedAttemptIds, parentAttemptIds),
  );
  return {
    attempts,
    dependsOnGroupId: sourceGroup.dependsOnGroupId,
    finalRestore: cloneClosed(sourceGroup.finalRestore),
    id: sourceGroup.id,
    operationOrder: cloneClosed(sourceGroup.operationOrder),
    order: sourceGroup.order,
    originalOrder: sourceGroup.originalOrder,
    preState: cloneClosed(sourceGroup.preState),
  };
}

// project one complete run slot
function projectRunSlot(sourceRunSlot, projectedAttemptIds, parentAttemptIds) {
  requireInvariant(
    sourceRunSlot.groups.length === EXPECTED_TOTALS.groups,
    "functional-schedule.group-count",
    `run slot ${sourceRunSlot.id} group count changed`,
  );
  const groups = sourceRunSlot.groups.map(
    // project one exact group
    (group) => projectGroup(group, projectedAttemptIds, parentAttemptIds),
  );
  const attemptCount = groups.reduce(
    // count one group denominator
    (total, group) => total + group.attempts.length,
    0,
  );
  requireInvariant(
    attemptCount === EXPECTED_TOTALS.plannedAttempts / EXPECTED_TOTALS.runSlots,
    "functional-schedule.slot-denominator",
    `run slot ${sourceRunSlot.id} attempt denominator changed`,
  );
  return { groups, id: sourceRunSlot.id, order: sourceRunSlot.order };
}

// build one dormant functional schedule
export function buildBackendPerformanceFunctionalSchedule(...argumentsList) {
  // reject runtime selectors
  if (argumentsList.length !== 0) {
    throw new TypeError("functional workload plan accepts no runtime input");
  }
  const parent = buildBackendPerformanceAmendedSchedule();
  verifyParentSchedule(parent);
  const projectedAttemptIds = new Set();
  const parentAttemptIds = new Set();
  const runSlots = parent.runSlots.map(
    // project one exact run slot
    (runSlot) => projectRunSlot(runSlot, projectedAttemptIds, parentAttemptIds),
  );
  requireInvariant(
    projectedAttemptIds.size === EXPECTED_TOTALS.plannedAttempts
      && parentAttemptIds.size === EXPECTED_TOTALS.plannedAttempts,
    "functional-schedule.attempt-count",
    "functional attempt denominator changed",
  );

  // prove namespace disjointness
  for (const attemptId of projectedAttemptIds) {
    requireInvariant(
      !parentAttemptIds.has(attemptId),
      "functional-schedule.attempt-id-disjointness",
      `functional attempt collides with parent: ${attemptId}`,
    );
  }

  return deepFreeze({
    canonicalEligible: false,
    comparison: {
      baselineAndCandidateSamePlan: true,
      definitionSha256: FIXED_DEFINITION_SHA256,
      mixedDispositionForbidden: DEFINITION.comparisonContract.mixedDispositionForbidden,
      normalLogoutObservation: DEFINITION.comparisonContract.normalLogoutObservation,
    },
    contractVersion: BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
    definition: {
      contractVersion: DEFINITION.contractVersion,
      sha256: FIXED_DEFINITION_SHA256,
    },
    functionalOracles: {
      contractVersion: BACKEND_PERFORMANCE_FUNCTIONAL_ORACLE_VERSION,
      dataHandlingConventions: cloneClosed(DEFINITION.dataHandlingConventionsNotPassOracles),
      operations: PROJECTED_OPERATIONS.map(
        // expose one detached reviewed oracle row
        (operation) => cloneClosed(operation),
      ),
    },
    omissions: cloneClosed(parent.omissions),
    parentSchedule: {
      contractVersion: parent.contractVersion,
      definition: cloneClosed(parent.definition),
      parentDefinition: cloneClosed(parent.parentDefinition),
      sourcePins: cloneClosed(parent.projection.sourcePins),
    },
    projection: {
      attemptIdNamespace: DEFINITION.namespaces.futureAttemptIdPrefix,
      omittedOperationIds: [...OMITTED_OPERATION_IDS],
      retainedOperationIds: PROJECTED_OPERATIONS.map(
        // expose exact reviewed order
        (operation) => operation.operationId,
      ),
    },
    runSlots,
    totals: cloneClosed(parent.totals),
    transitions: cloneClosed(parent.transitions),
  });
}
