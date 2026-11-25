import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const SOURCE_PATHS = Object.freeze({
  disposition: "config/backend-performance-amended-disposition-v1.json",
  builder: "scripts/backend-performance-workload.mjs",
  workload: "config/backend-performance-workload-v2.json",
});
const SOURCE_URLS = Object.freeze({
  disposition: new URL("../config/backend-performance-amended-disposition-v1.json", import.meta.url),
  builder: new URL("./backend-performance-workload.mjs", import.meta.url),
  workload: new URL("../config/backend-performance-workload-v2.json", import.meta.url),
});
const FIXED_DISPOSITION_PIN = Object.freeze({
  bytes: 5233,
  sha256: "f2236d5105772534b5a9820df30f71f853126f55b0627a9eee659bae5f21dff6",
});
const FIXED_AMENDED_DEFINITION_SHA256 =
  "73883cdde0ec1ddfe7f4cc624e82c652f7f56727744b2e654833fd3b996f98ce";
const PUBLIC_GROUP_ID = "group.http-public-matrix";
const ADMIN_GROUP_ID = "group.admin-live-safe";
const ADMIN_LOGOUT_ID = "admin.live.logout";

// create one stable coded failure
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one fixed projection invariant
function requireInvariant(condition, code, message) {
  // fail closed on any source or projection drift
  if (!condition) {
    fail(code, message);
  }
}

// hash exact checked bytes
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// compare strings by portable ECMAScript code-unit order
function compareText(left, right) {
  // retain equal keys without locale behavior
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

// serialize one closed JSON value without key-order ambiguity
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

  // retain only finite JSON primitives
  if (typeof value === "string" || typeof value === "boolean"
    || (typeof value === "number" && Number.isFinite(value))) {
    return JSON.stringify(value);
  }
  fail("amended-schedule.definition-canonicalization", "amended definition is not closed JSON");
}

// read one source only when its byte count and raw pin match
function readPinnedSource(sourceKey, expectedPin) {
  const bytes = readFileSync(SOURCE_URLS[sourceKey]);
  const actualSha256 = sha256(bytes);

  // reject byte-level drift before parsing or executable import
  if (bytes.length !== expectedPin.bytes || actualSha256 !== expectedPin.sha256) {
    fail(
      "amended-schedule.source-pin",
      `${SOURCE_PATHS[sourceKey]} raw identity mismatch`,
    );
  }
  return bytes;
}

const DISPOSITION_BYTES = readPinnedSource("disposition", FIXED_DISPOSITION_PIN);
const DISPOSITION = JSON.parse(DISPOSITION_BYTES.toString("utf8"));
const AMENDED_DEFINITION = DISPOSITION.amendedDefinition;
const SOURCE_SCHEDULE_DEFINITION_SHA256 = AMENDED_DEFINITION.parent.schedule.definitionSha256;
const SOURCE_PINS = Object.freeze({
  disposition: FIXED_DISPOSITION_PIN,
  builder: Object.freeze({
    bytes: AMENDED_DEFINITION.parent.builder.bytes,
    sha256: AMENDED_DEFINITION.parent.builder.rawSha256,
  }),
  workload: Object.freeze({
    bytes: AMENDED_DEFINITION.parent.workload.bytes,
    sha256: AMENDED_DEFINITION.parent.workload.rawSha256,
  }),
});

requireInvariant(
  DISPOSITION.contractVersion === "weather-backend-performance-amended-disposition/v1"
    && AMENDED_DEFINITION.contractVersion
      === "weather-backend-performance-amended-schedule-definition/v1"
    && DISPOSITION.amendedDefinitionSha256 === FIXED_AMENDED_DEFINITION_SHA256
    && sha256(Buffer.from(canonicalJson(AMENDED_DEFINITION), "utf8"))
      === FIXED_AMENDED_DEFINITION_SHA256
    && FIXED_AMENDED_DEFINITION_SHA256 !== SOURCE_SCHEDULE_DEFINITION_SHA256,
  "amended-schedule.definition-identity",
  "portable amended definition identity changed",
);

const WORKLOAD_BYTES = readPinnedSource("workload", SOURCE_PINS.workload);
readPinnedSource("builder", SOURCE_PINS.builder);
const SOURCE_WORKLOAD = JSON.parse(WORKLOAD_BYTES.toString("utf8"));
const BUILDER_MODULE = await import(SOURCE_URLS.builder.href);
const {
  BACKEND_PERFORMANCE_SCHEDULE_VERSION,
  BACKEND_PERFORMANCE_WORKLOAD_VERSION,
  buildBackendPerformanceWorkloadPlan,
} = BUILDER_MODULE;

requireInvariant(
  BACKEND_PERFORMANCE_SCHEDULE_VERSION === AMENDED_DEFINITION.parent.schedule.contractVersion
    && BACKEND_PERFORMANCE_WORKLOAD_VERSION === AMENDED_DEFINITION.parent.workload.contractVersion
    && typeof buildBackendPerformanceWorkloadPlan === "function",
  "amended-schedule.builder-contract",
  "checked parent builder exports changed",
);

export const BACKEND_PERFORMANCE_AMENDED_SCHEDULE_VERSION =
  AMENDED_DEFINITION.scheduleContractVersion;
export const BACKEND_PERFORMANCE_AMENDED_DEFINITION_SHA256 =
  FIXED_AMENDED_DEFINITION_SHA256;

const REVIEWED_OMITTED_OPERATION_IDS = Object.freeze([
  ...AMENDED_DEFINITION.omittedOperationIds,
]);
const ORPHANED_TRANSITION = AMENDED_DEFINITION.removedMeasuredTransitions[0].bindingSlot;
const PASSIVE_HEADER_ORACLE =
  AMENDED_DEFINITION.retainedContracts.publicGetRequiredTerminalOracle;
const REQUIRED_ADMIN_FUNCTIONAL_OPERATION_IDS = Object.freeze([
  ...AMENDED_DEFINITION.retainedContracts.requiredAdminOperationIds,
]);
const EXPECTED_TOTALS = Object.freeze({
  applicationCacheColdAttempts: AMENDED_DEFINITION.totals.applicationCacheColdAttempts,
  dbOsColdAttempts: AMENDED_DEFINITION.totals.dbOsColdAttempts,
  groupInstances: AMENDED_DEFINITION.totals.groupInstances,
  oncePerRunAttempts: AMENDED_DEFINITION.totals.oncePerRunAttempts,
  plannedAttempts: AMENDED_DEFINITION.totals.plannedAttempts,
  preStateReceipts: AMENDED_DEFINITION.totals.preStateReceipts,
  processPoolColdAttempts: AMENDED_DEFINITION.totals.processPoolColdAttempts,
  restoreReceipts: AMENDED_DEFINITION.totals.restoreReceipts,
  runSlots: AMENDED_DEFINITION.totals.runSlots,
  warmAttempts: AMENDED_DEFINITION.totals.warmAttempts,
});
const OMITTED_OPERATION_ID_SET = new Set(REVIEWED_OMITTED_OPERATION_IDS);
const OPERATION_BY_ID = new Map(SOURCE_WORKLOAD.operations.map(
  // index the immutable source operation table
  (operation) => [operation.id, operation],
));
const GROUP_BY_ID = new Map(SOURCE_WORKLOAD.groups.map(
  // index the immutable source group table
  (group) => [group.id, group],
));
const RETAINED_OPERATIONS = SOURCE_WORKLOAD.operations.filter(
  // retain every operation outside the reviewed omission set
  (operation) => !OMITTED_OPERATION_ID_SET.has(operation.id),
);
const RETAINED_OPERATION_ID_SET = new Set(RETAINED_OPERATIONS.map(
  // expose the exact retained source identities
  (operation) => operation.id,
));
const OMITTED_OPERATIONS_IN_SOURCE_ORDER = SOURCE_WORKLOAD.operations.filter(
  // retain exact source order for reviewed omission provenance
  (operation) => OMITTED_OPERATION_ID_SET.has(operation.id),
);
const RETAINED_BINDING_SLOTS = [...new Set(RETAINED_OPERATIONS.flatMap(
  // preserve every input of every retained operation
  (operation) => operation.bindingSlots,
))].sort(
  // expose binding identities in stable code-unit order
  compareText,
);
const RETAINED_BINDING_SLOT_SET = new Set(RETAINED_BINDING_SLOTS);
const RETAINED_TRANSITIONS = Object.entries(SOURCE_WORKLOAD.transitionProducers).filter(
  // remove only transition slots unused by retained operations
  ([bindingSlot]) => RETAINED_BINDING_SLOT_SET.has(bindingSlot),
).sort(
  // retain stable transition order without locale behavior
  ([left], [right]) => compareText(left, right),
).map(
  // expose the retained producer topology
  ([bindingSlot, producerOperationId]) => ({ bindingSlot, producerOperationId }),
);
const PUBLIC_GROUP = GROUP_BY_ID.get(PUBLIC_GROUP_ID);
const ADMIN_GROUP = GROUP_BY_ID.get(ADMIN_GROUP_ID);
const ADMIN_LOGOUT = OPERATION_BY_ID.get(ADMIN_LOGOUT_ID);

// compare exact operation positions in the source group chain
function compareOperationPosition(left, right) {
  const leftGroup = GROUP_BY_ID.get(left.atomicGroupId);
  const rightGroup = GROUP_BY_ID.get(right.atomicGroupId);

  // compare operation order only inside one atomic group
  if (leftGroup.globalOrder === rightGroup.globalOrder) {
    return left.orderWithinGroup - right.orderWithinGroup;
  }
  return leftGroup.globalOrder - rightGroup.globalOrder;
}

// validate every fixed assumption used by the projection
function verifyFixedProjectionDefinition() {
  requireInvariant(
    SOURCE_WORKLOAD.contractVersion === BACKEND_PERFORMANCE_WORKLOAD_VERSION,
    "amended-schedule.source-contract",
    "source workload contract version changed",
  );
  requireInvariant(
    SOURCE_WORKLOAD.operations.length === 107 && SOURCE_WORKLOAD.groups.length === 59,
    "amended-schedule.source-counts",
    "source workload cardinalities changed",
  );
  requireInvariant(
    canonicalJson(OMITTED_OPERATIONS_IN_SOURCE_ORDER.map(
      // recover exact omitted operation source order
      (operation) => operation.id,
    )) === canonicalJson(AMENDED_DEFINITION.omittedOperationSourceOrder),
    "amended-schedule.omission-order",
    "reviewed omission source order changed",
  );
  requireInvariant(
    RETAINED_OPERATIONS.length === AMENDED_DEFINITION.totals.operations
      && RETAINED_BINDING_SLOTS.length === AMENDED_DEFINITION.totals.usedTypedSlots,
    "amended-schedule.retained-counts",
    "retained operation or binding-slot cardinality changed",
  );
  requireInvariant(
    AMENDED_DEFINITION.removedMeasuredTransitions.length === 1
      && RETAINED_TRANSITIONS.length === AMENDED_DEFINITION.totals.measuredTransitions
      && !RETAINED_BINDING_SLOT_SET.has(ORPHANED_TRANSITION),
    "amended-schedule.transition-counts",
    "retained transition cardinality changed",
  );
  requireInvariant(
    SOURCE_WORKLOAD.transitionProducers[ORPHANED_TRANSITION] === ADMIN_LOGOUT_ID
      && AMENDED_DEFINITION.removedMeasuredTransitions[0].producerOperationId === ADMIN_LOGOUT_ID,
    "amended-schedule.orphan-producer",
    "reviewed orphan transition producer changed",
  );
  requireInvariant(
    PUBLIC_GROUP.operationIds.length
      === AMENDED_DEFINITION.retainedContracts.publicGetOperationCount
      && PUBLIC_GROUP.operationIds.every(
        // require every public operation to retain the passive header oracle
        (operationId) => OPERATION_BY_ID.get(operationId).terminalOracle.includes(PASSIVE_HEADER_ORACLE),
      ),
    "amended-schedule.public-oracles",
    "public GET passive header contract changed",
  );
  requireInvariant(
    REQUIRED_ADMIN_FUNCTIONAL_OPERATION_IDS.every(
      // require normal admin login reads validation and logout to remain
      (operationId) => RETAINED_OPERATION_ID_SET.has(operationId),
    ),
    "amended-schedule.admin-controls",
    "required ordinary admin control was removed",
  );
  requireInvariant(
    ADMIN_LOGOUT.orderWithinGroup === 10
      && ADMIN_GROUP.operationIds.filter(
        // retain only reviewed active admin operations
        (operationId) => RETAINED_OPERATION_ID_SET.has(operationId),
      ).map(
        // recover each original admin group order
        (operationId) => OPERATION_BY_ID.get(operationId).orderWithinGroup,
      ).join(",") === AMENDED_DEFINITION.ordering.adminLiveOriginalRetainedOrders.join(",")
      && canonicalJson(ADMIN_LOGOUT.terminalOracle)
        === canonicalJson(AMENDED_DEFINITION.retainedContracts.adminLogout.terminalOracle),
    "amended-schedule.admin-order",
    "reviewed admin operation order changed",
  );

  // prove every retained measured transition still has a producer and consumer
  for (const transition of RETAINED_TRANSITIONS) {
    const producer = OPERATION_BY_ID.get(transition.producerOperationId);
    const consumers = RETAINED_OPERATIONS.filter(
      // find retained consumers of this exact transition slot
      (operation) => operation.bindingSlots.includes(transition.bindingSlot),
    );
    requireInvariant(
      RETAINED_OPERATION_ID_SET.has(producer?.id) && consumers.length > 0,
      "amended-schedule.transition-topology",
      `transition ${transition.bindingSlot} lost its producer or consumers`,
    );

    // require each producer to precede its retained consumer
    for (const consumer of consumers) {
      requireInvariant(
        compareOperationPosition(producer, consumer) < 0,
        "amended-schedule.transition-order",
        `transition ${transition.bindingSlot} is not forward ordered`,
      );
    }
  }
}

verifyFixedProjectionDefinition();

// recursively freeze one detached amended plan
function deepFreeze(value) {
  // freeze each mutable object after its children
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    // freeze every nested plan value
    for (const child of Object.values(value)) {
      deepFreeze(child);
    }
    Object.freeze(value);
  }
  return value;
}

// require one exact original zero-argument plan
function verifyOriginalPlan(plan) {
  requireInvariant(
    plan.canonicalEligible === false
      && plan.contractVersion === BACKEND_PERFORMANCE_SCHEDULE_VERSION
      && plan.definition.contractVersion === BACKEND_PERFORMANCE_WORKLOAD_VERSION
      && plan.definition.sha256 === SOURCE_SCHEDULE_DEFINITION_SHA256,
    "amended-schedule.source-plan",
    "source schedule identity changed",
  );
  requireInvariant(
    plan.runSlots.length === 5
      && plan.totals.plannedAttempts === 6290
      && plan.totals.groupInstances === 295
      && plan.transitions.length === 10,
    "amended-schedule.source-plan-counts",
    "source schedule cardinalities changed",
  );
  requireInvariant(
    canonicalJson(plan.runSlots.map(
      // recover exact parent run-slot order
      (runSlot) => runSlot.id,
    )) === canonicalJson(AMENDED_DEFINITION.ordering.runSlotIds),
    "amended-schedule.source-slot-order",
    "source schedule run-slot order changed",
  );
}

// build the fixed amended projection from the exact zero-argument v2 plan
export function buildBackendPerformanceAmendedSchedule(...argumentsList) {
  // reject baseline candidate selectors and every other runtime override
  if (argumentsList.length !== 0) {
    throw new TypeError("amended workload plan accepts no runtime input");
  }
  const sourcePlan = buildBackendPerformanceWorkloadPlan();
  verifyOriginalPlan(sourcePlan);

  const omittedAttemptsByOperation = new Map(REVIEWED_OMITTED_OPERATION_IDS.map(
    // prepare exact omission provenance buckets
    (operationId) => [operationId, []],
  ));
  const originalAttemptIds = new Set();
  const retainedOriginalAttemptIds = new Set();
  const projectedAttemptIds = new Set();
  const stratumCounts = {
    "application-cache-cold": 0,
    "db-os-cold": 0,
    "once-per-run": 0,
    "process-pool-cold": 0,
    warm: 0,
  };
  let publicGetAttempts = 0;
  const runSlots = sourcePlan.runSlots.map(
    // project one complete source run slot
    (sourceRunSlot) => {
      let projectedOrdinal = 1;
      const groups = sourceRunSlot.groups.map(
        // retain one atomic group and its mandatory gates
        (sourcePlanGroup) => {
          const sourceGroup = GROUP_BY_ID.get(sourcePlanGroup.id);
          const retainedOperationIds = sourceGroup.operationIds.filter(
            // omit only the independently reviewed operation identities
            (operationId) => RETAINED_OPERATION_ID_SET.has(operationId),
          );
          const projectedOrderByOperation = new Map(retainedOperationIds.map(
            // assign contiguous execution order inside this retained group
            (operationId, index) => [operationId, index + 1],
          ));
          const attempts = [];

          // translate every original attempt without re-expanding the schedule
          for (const sourceAttempt of sourcePlanGroup.attempts) {
            requireInvariant(
              !originalAttemptIds.has(sourceAttempt.attemptId),
              "amended-schedule.source-attempt-id",
              `duplicate source attempt ${sourceAttempt.attemptId}`,
            );
            originalAttemptIds.add(sourceAttempt.attemptId);
            const sourceOperation = OPERATION_BY_ID.get(sourceAttempt.operationId);

            // record reviewed omissions without scheduling them
            if (OMITTED_OPERATION_ID_SET.has(sourceAttempt.operationId)) {
              omittedAttemptsByOperation.get(sourceAttempt.operationId).push({
                originalAttemptId: sourceAttempt.attemptId,
                originalOrdinal: sourceAttempt.ordinal,
                runSlotId: sourceAttempt.runSlotId,
                stratum: sourceAttempt.stratum,
              });
              continue;
            }

            const attemptId = `${AMENDED_DEFINITION.ordering.attemptIdNamespace}${sourceRunSlot.id}:${String(projectedOrdinal).padStart(4, "0")}:${sourceAttempt.operationId}:${sourceAttempt.stratum}`;
            requireInvariant(
              !projectedAttemptIds.has(attemptId),
              "amended-schedule.projected-attempt-id",
              `duplicate projected attempt ${attemptId}`,
            );
            projectedAttemptIds.add(attemptId);
            retainedOriginalAttemptIds.add(sourceAttempt.attemptId);
            stratumCounts[sourceAttempt.stratum] += 1;

            // count the complete retained public denominator
            if (sourceAttempt.groupId === PUBLIC_GROUP_ID) {
              publicGetAttempts += 1;
            }
            attempts.push({
              ...sourceAttempt,
              attemptId,
              ordinal: projectedOrdinal,
              originalAttemptId: sourceAttempt.attemptId,
              originalOrdinal: sourceAttempt.ordinal,
              originalOrderWithinGroup: sourceOperation.orderWithinGroup,
              projectedOrderWithinGroup: projectedOrderByOperation.get(sourceAttempt.operationId),
            });
            projectedOrdinal += 1;
          }
          return {
            ...sourcePlanGroup,
            attempts,
            operationOrder: {
              originalOperationIds: [...sourceGroup.operationIds],
              retained: retainedOperationIds.map(
                // retain source and projected operation orders together
                (operationId, index) => ({
                  operationId,
                  originalOrderWithinGroup: OPERATION_BY_ID.get(operationId).orderWithinGroup,
                  projectedOrderWithinGroup: index + 1,
                }),
              ),
            },
            originalOrder: sourcePlanGroup.order,
          };
        },
      );
      requireInvariant(
        projectedOrdinal === 1256,
        "amended-schedule.slot-denominator",
        `run slot ${sourceRunSlot.id} did not retain 1255 attempts`,
      );
      return { ...sourceRunSlot, groups };
    },
  );

  const omissions = OMITTED_OPERATIONS_IN_SOURCE_ORDER.map(
    // retain recoverable source identity without a passing verdict
    (operation) => ({
      disposition: "intentionally-not-run",
      groupId: operation.atomicGroupId,
      operationId: operation.id,
      originalAttempts: omittedAttemptsByOperation.get(operation.id),
      originalOrderWithinGroup: operation.orderWithinGroup,
    }),
  );

  // prove disjointness only after both complete identity sets exist
  for (const projectedAttemptId of projectedAttemptIds) {
    requireInvariant(
      !originalAttemptIds.has(projectedAttemptId),
      "amended-schedule.attempt-id-disjointness",
      `projected attempt collides with complete parent set: ${projectedAttemptId}`,
    );
  }

  requireInvariant(
    originalAttemptIds.size === 6290
      && retainedOriginalAttemptIds.size === EXPECTED_TOTALS.plannedAttempts
      && projectedAttemptIds.size === EXPECTED_TOTALS.plannedAttempts
      && omissions.every(
        // require all five source slots for each reviewed omission
        (omission) => omission.originalAttempts.length === 5,
      ),
    "amended-schedule.attempt-denominator",
    "retained or omitted attempt denominator changed",
  );
  requireInvariant(
    publicGetAttempts === AMENDED_DEFINITION.totals.publicGetAttempts
      && stratumCounts.warm === EXPECTED_TOTALS.warmAttempts
      && stratumCounts["application-cache-cold"] === EXPECTED_TOTALS.applicationCacheColdAttempts
      && stratumCounts["process-pool-cold"] === EXPECTED_TOTALS.processPoolColdAttempts
      && stratumCounts["db-os-cold"] === EXPECTED_TOTALS.dbOsColdAttempts
      && stratumCounts["once-per-run"] === EXPECTED_TOTALS.oncePerRunAttempts,
    "amended-schedule.stratum-denominator",
    "retained stratum denominator changed",
  );

  return deepFreeze({
    canonicalEligible: false,
    contractVersion: BACKEND_PERFORMANCE_AMENDED_SCHEDULE_VERSION,
    definition: {
      contractVersion: AMENDED_DEFINITION.contractVersion,
      sha256: BACKEND_PERFORMANCE_AMENDED_DEFINITION_SHA256,
    },
    omissions,
    parentDefinition: {
      contractVersion: BACKEND_PERFORMANCE_SCHEDULE_VERSION,
      sha256: SOURCE_SCHEDULE_DEFINITION_SHA256,
    },
    projection: {
      baselineAndCandidateSamePlan: true,
      omittedOperationIds: [...REVIEWED_OMITTED_OPERATION_IDS],
      retainedBindingSlots: [...RETAINED_BINDING_SLOTS],
      retainedOperationIds: RETAINED_OPERATIONS.map(
        // retain source operation order in the amended manifest
        (operation) => operation.id,
      ),
      sourcePins: {
        disposition: { path: SOURCE_PATHS.disposition, ...SOURCE_PINS.disposition },
        builder: { path: SOURCE_PATHS.builder, ...SOURCE_PINS.builder },
        workload: { path: SOURCE_PATHS.workload, ...SOURCE_PINS.workload },
      },
      sourceSchedule: {
        contractVersion: BACKEND_PERFORMANCE_SCHEDULE_VERSION,
        definitionSha256: SOURCE_SCHEDULE_DEFINITION_SHA256,
      },
    },
    retainedOracles: {
      adminLogout: {
        operationId: ADMIN_LOGOUT.id,
        originalOrderWithinGroup: ADMIN_LOGOUT.orderWithinGroup,
        projectedOrderWithinGroup:
          AMENDED_DEFINITION.retainedContracts.adminLogout.projectedOrderWithinGroup,
        terminalOracle: [...ADMIN_LOGOUT.terminalOracle],
      },
      publicGet: {
        operationIds: [...PUBLIC_GROUP.operationIds],
        plannedAttempts: publicGetAttempts,
        requiredTerminalOracle: PASSIVE_HEADER_ORACLE,
      },
      restoration: {
        finalRestoreContractVersion: "weather-backend-group-restore/v1",
        groupInstances: EXPECTED_TOTALS.groupInstances,
        preStateContractVersion: "weather-backend-group-prestate/v1",
      },
    },
    runSlots,
    totals: { ...EXPECTED_TOTALS },
    transitions: RETAINED_TRANSITIONS.map(
      // detach the retained fixed transition topology
      (transition) => ({ ...transition }),
    ),
  });
}
