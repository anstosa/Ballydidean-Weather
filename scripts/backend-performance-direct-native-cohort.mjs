import {
  BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
  BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";

export const BACKEND_PERFORMANCE_DIRECT_NATIVE_COHORT_VERSION =
  "weather-backend-performance-direct-native-cohort/v1";

const TERMINAL_CATEGORIES = Object.freeze(["completed", "error", "timeout"]);
const TERMINAL_CATEGORY_SET = new Set(TERMINAL_CATEGORIES);
const ERROR_CATEGORIES = Object.freeze([
  "none",
  "deadline-exceeded",
  "http",
  "observation",
  "process",
  "transport",
]);
const ERROR_CATEGORY_SET = new Set(ERROR_CATEGORIES);
const MEASURED_LATENCY_STRATA = new Set([
  "application-cache-cold",
  "db-os-cold",
  "process-pool-cold",
  "warm",
]);
const TERMINAL_ROW_KEYS = Object.freeze([
  "attemptId",
  "elapsedNanoseconds",
  "errorCategory",
  "httpStatus",
  "operationId",
  "preparationClassification",
  "requestBodyBytes",
  "responseBodyBytes",
  "runSlotId",
  "semanticClassification",
  "stratum",
  "terminalCategory",
]);

// create one coded cohort error
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one cohort invariant
function requireInvariant(condition, code, message) {
  // fail closed on malformed evidence
  if (!condition) {
    fail(code, message);
  }
}

// identify one parsed-json record
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    && (Object.getPrototypeOf(value) === Object.prototype
      || Object.getPrototypeOf(value) === null);
}

// compare text by code units
function compareText(left, right) {
  // preserve equal text
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

// require one exact record shape
function requireExactKeys(value, expectedKeys, path) {
  requireInvariant(isRecord(value), "direct-native-cohort.schema", `${path} must be a record`);
  const actual = Object.keys(value).sort(compareText);
  const expected = [...expectedKeys].sort(compareText);
  requireInvariant(
    JSON.stringify(actual) === JSON.stringify(expected),
    "direct-native-cohort.schema",
    `${path} has unexpected fields`,
  );
}

// freeze one complete result
function deepFreeze(value) {
  // freeze nested values first
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    // visit every nested child
    for (const child of Object.values(value)) {
      deepFreeze(child);
    }
    Object.freeze(value);
  }
  return value;
}

// flatten the authoritative schedule once
function buildRequirements() {
  const schedule = buildBackendPerformanceFunctionalSchedule();
  const attempts = [];
  const attemptById = new Map();
  // retain each fixed run slot
  for (const runSlot of schedule.runSlots) {
    // retain each fixed group
    for (const group of runSlot.groups) {
      // retain each exact source attempt
      for (const attempt of group.attempts) {
        requireInvariant(
          !attemptById.has(attempt.attemptId),
          "direct-native-cohort.schedule",
          `schedule repeats attempt ${attempt.attemptId}`,
        );
        attempts.push(attempt);
        attemptById.set(attempt.attemptId, attempt);
      }
    }
  }
  requireInvariant(
    attempts.length === schedule.totals.plannedAttempts,
    "direct-native-cohort.schedule",
    "authoritative schedule denominator is inconsistent",
  );
  return deepFreeze({
    attemptById,
    attempts,
    plannedAttempts: schedule.totals.plannedAttempts,
    runSlotIds: schedule.runSlots.map(
      // retain actual slot order
      (runSlot) => runSlot.id,
    ),
  });
}

const REQUIREMENTS = buildRequirements();

// validate one nonnegative safe integer
function validateCount(value, path) {
  requireInvariant(
    Number.isSafeInteger(value) && value >= 0,
    "direct-native-cohort.schema",
    `${path} must be a nonnegative safe integer`,
  );
  return value;
}

// validate one terminal row against its source attempt
function validateTerminalRow(value, index) {
  const path = `$.terminalRows[${String(index)}]`;
  requireExactKeys(value, TERMINAL_ROW_KEYS, path);
  requireInvariant(
    typeof value.attemptId === "string" && value.attemptId.length > 0,
    "direct-native-cohort.schema",
    `${path}.attemptId is invalid`,
  );
  const source = REQUIREMENTS.attemptById.get(value.attemptId);
  requireInvariant(
    source !== undefined,
    "direct-native-cohort.attempt-id",
    `${path}.attemptId is outside the authoritative v4 schedule`,
  );
  requireInvariant(
    value.operationId === source.operationId
      && value.runSlotId === source.runSlotId
      && value.stratum === source.stratum,
    "direct-native-cohort.attempt-binding",
    `${path} crosses its authoritative operation, stratum, or run slot`,
  );
  requireInvariant(
    TERMINAL_CATEGORY_SET.has(value.terminalCategory),
    "direct-native-cohort.schema",
    `${path}.terminalCategory is invalid`,
  );
  const normalizedError = value.errorCategory === null ? "none" : value.errorCategory;
  requireInvariant(
    ERROR_CATEGORY_SET.has(normalizedError)
      && (value.terminalCategory === "completed") === (normalizedError === "none")
      && (value.terminalCategory === "timeout")
        === (normalizedError === "deadline-exceeded"),
    "direct-native-cohort.terminal",
    `${path} has an inconsistent terminal and error classification`,
  );
  requireInvariant(
    value.httpStatus === null
      || (Number.isSafeInteger(value.httpStatus)
        && value.httpStatus >= 100 && value.httpStatus <= 599),
    "direct-native-cohort.schema",
    `${path}.httpStatus is invalid`,
  );
  requireInvariant(
    typeof value.preparationClassification === "string"
      && value.preparationClassification.length > 0
      && typeof value.semanticClassification === "string"
      && value.semanticClassification.length > 0,
    "direct-native-cohort.schema",
    `${path} has an invalid measured classification`,
  );
  return {
    attemptId: value.attemptId,
    elapsedNanoseconds: validateCount(value.elapsedNanoseconds, `${path}.elapsedNanoseconds`),
    errorCategory: value.errorCategory,
    httpStatus: value.httpStatus,
    operationId: value.operationId,
    preparationClassification: value.preparationClassification,
    requestBodyBytes: validateCount(value.requestBodyBytes, `${path}.requestBodyBytes`),
    responseBodyBytes: validateCount(value.responseBodyBytes, `${path}.responseBodyBytes`),
    runSlotId: value.runSlotId,
    semanticClassification: value.semanticClassification,
    stratum: value.stratum,
    terminalCategory: value.terminalCategory,
  };
}

// validate exact terminal and incomplete custody inputs
function validateInput(value) {
  requireExactKeys(value, ["enteredIncompleteAttemptIds", "terminalRows"], "$");
  requireInvariant(
    Array.isArray(value.terminalRows) && Array.isArray(value.enteredIncompleteAttemptIds),
    "direct-native-cohort.schema",
    "cohort inputs must be arrays",
  );
  const terminalRows = value.terminalRows.map(
    // validate every measured terminal row
    (row, index) => validateTerminalRow(row, index),
  );
  const terminalById = new Map();
  // reject replayed terminal evidence
  for (const row of terminalRows) {
    requireInvariant(
      !terminalById.has(row.attemptId),
      "direct-native-cohort.duplicate-attempt",
      `terminal evidence repeats attempt ${row.attemptId}`,
    );
    terminalById.set(row.attemptId, row);
  }
  const incompleteSet = new Set();
  // validate every entered incomplete identity
  for (let index = 0; index < value.enteredIncompleteAttemptIds.length; index += 1) {
    const attemptId = value.enteredIncompleteAttemptIds[index];
    const path = `$.enteredIncompleteAttemptIds[${String(index)}]`;
    requireInvariant(
      typeof attemptId === "string" && REQUIREMENTS.attemptById.has(attemptId),
      "direct-native-cohort.attempt-id",
      `${path} is outside the authoritative v4 schedule`,
    );
    requireInvariant(
      !incompleteSet.has(attemptId) && !terminalById.has(attemptId),
      "direct-native-cohort.duplicate-attempt",
      `${path} repeats an entered or terminal attempt`,
    );
    incompleteSet.add(attemptId);
  }
  return { incompleteSet, terminalById };
}

// create one empty full-denominator summary
function createSummary(runSlotId = null, operationId = null, stratum = null) {
  return {
    elapsedValues: [],
    enteredIncompleteAttemptIds: [],
    errorCategoryCounts: Object.fromEntries(ERROR_CATEGORIES.map(
      // initialize one exact error class
      (category) => [category, 0],
    )),
    missingAttemptIds: [],
    operationId,
    preparationCounts: new Map(),
    requestBodyBytes: 0,
    responseBodyBytes: 0,
    runSlotId,
    semanticCounts: new Map(),
    sourceAttemptIds: [],
    statusCounts: new Map(),
    stratum,
    terminalAttemptIds: [],
    terminalCounts: { completed: 0, error: 0, timeout: 0 },
  };
}

// add one safe integer without losing denominator bytes
function addSafeInteger(left, right, label) {
  const total = left + right;
  requireInvariant(
    Number.isSafeInteger(total),
    "direct-native-cohort.unsafe-total",
    `${label} exceeds the safe integer range`,
  );
  return total;
}

// increase one observed classification count
function incrementCount(counts, key) {
  counts.set(key, (counts.get(key) ?? 0) + 1);
}

// add one source attempt and its custody observation
function addAttempt(summary, source, row, incomplete) {
  summary.sourceAttemptIds.push(source.attemptId);
  // retain an entered attempt without inventing a terminal observation
  if (incomplete) {
    summary.enteredIncompleteAttemptIds.push(source.attemptId);
    return;
  }
  // retain an unentered source gap separately
  if (row === undefined) {
    summary.missingAttemptIds.push(source.attemptId);
    return;
  }
  summary.terminalAttemptIds.push(source.attemptId);
  summary.terminalCounts[row.terminalCategory] += 1;
  summary.errorCategoryCounts[row.errorCategory ?? "none"] += 1;
  incrementCount(summary.statusCounts, row.httpStatus === null ? "none" : String(row.httpStatus));
  incrementCount(summary.preparationCounts, row.preparationClassification);
  incrementCount(summary.semanticCounts, row.semanticClassification);
  summary.requestBodyBytes = addSafeInteger(
    summary.requestBodyBytes,
    row.requestBodyBytes,
    "request body byte total",
  );
  summary.responseBodyBytes = addSafeInteger(
    summary.responseBodyBytes,
    row.responseBodyBytes,
    "response body byte total",
  );
  summary.elapsedValues.push(row.elapsedNanoseconds);
}

// close one classification map in stable text order
function closeCounts(counts) {
  return [...counts.entries()].sort(
    // sort one classification identity
    ([left], [right]) => compareText(left, right),
  ).map(
    // expose one closed count row
    ([classification, count]) => ({ classification, count }),
  );
}

// select one raw nearest-rank percentile
function nearestRank(values, percentile) {
  const sorted = [...values].sort(
    // sort exact integer observations
    (left, right) => left - right,
  );
  return sorted[Math.ceil((percentile * sorted.length) / 100) - 1];
}

// close one source-derived full-denominator summary
function closeSummary(summary) {
  const plannedAttempts = summary.sourceAttemptIds.length;
  const terminalAttempts = summary.terminalAttemptIds.length;
  const enteredIncompleteAttempts = summary.enteredIncompleteAttemptIds.length;
  const missingAttempts = summary.missingAttemptIds.length;
  const complete = terminalAttempts === plannedAttempts;
  requireInvariant(
    terminalAttempts + enteredIncompleteAttempts + missingAttempts === plannedAttempts
      && summary.elapsedValues.length === terminalAttempts,
    "direct-native-cohort.denominator",
    "cohort summary denominator is inconsistent",
  );
  return {
    bytes: {
      observedAttempts: terminalAttempts,
      requestBodyBytes: summary.requestBodyBytes,
      responseBodyBytes: summary.responseBodyBytes,
    },
    classifications: {
      preparation: closeCounts(summary.preparationCounts),
      semantic: closeCounts(summary.semanticCounts),
    },
    denominator: {
      complete,
      enteredIncompleteAttempts,
      missingAttempts,
      plannedAttempts,
      terminalAttempts,
    },
    elapsedNanoseconds: {
      observedAttempts: terminalAttempts,
      p95: complete ? nearestRank(summary.elapsedValues, 95) : null,
      plannedAttempts,
      statistic: "nearest-rank-p95",
    },
    enteredIncompleteAttemptIds: summary.enteredIncompleteAttemptIds,
    errorCategoryCounts: summary.errorCategoryCounts,
    missingAttemptIds: summary.missingAttemptIds,
    operationId: summary.operationId,
    runSlotId: summary.runSlotId,
    sourceAttemptIds: summary.sourceAttemptIds,
    statusCounts: Object.fromEntries([["none", 0], ...[...summary.statusCounts.entries()].sort(
      // sort numeric and absent status identities
      ([left], [right]) => compareText(left, right),
    )].map(
      // retain one status count
      ([status, count]) => [status, count],
    )),
    stratum: summary.stratum,
    terminalAttemptIds: summary.terminalAttemptIds,
    terminalCounts: summary.terminalCounts,
  };
}

// obtain one plan-ordered grouping summary
function getSummary(summaries, key, runSlotId, operationId, stratum) {
  let summary = summaries.get(key);
  // initialize one exact grouping once
  if (summary === undefined) {
    summary = createSummary(runSlotId, operationId, stratum);
    summaries.set(key, summary);
  }
  return summary;
}

// expose target-shaped observations without granting target authority
function buildLatencyObservations(runSlotOperationStrata) {
  return runSlotOperationStrata.filter(
    // retain only required warm and cold latency strata
    (summary) => MEASURED_LATENCY_STRATA.has(summary.stratum),
  ).map(
    // expose one exact raw metric observation
    (summary) => ({
      observedAttempts: summary.denominator.terminalAttempts,
      operationId: summary.operationId,
      plannedAttempts: summary.denominator.plannedAttempts,
      runSlotId: summary.runSlotId,
      sourceAttemptIds: summary.sourceAttemptIds,
      statistic: "nearest-rank-p95",
      stratum: summary.stratum,
      unit: "nanoseconds",
      value: summary.elapsedNanoseconds.p95,
    }),
  );
}

// assemble exact v4 attempt custody and raw metric observations
export function assembleBackendPerformanceDirectNativeCohort(input) {
  requireInvariant(
    arguments.length === 1,
    "direct-native-cohort.arguments",
    "native cohort assembly accepts one input record",
  );
  const { incompleteSet, terminalById } = validateInput(input);
  const overall = createSummary();
  const runSlots = new Map();
  const operationStrata = new Map();
  const runSlotOperationStrata = new Map();
  // reduce every authoritative attempt in fixed source order
  for (const source of REQUIREMENTS.attempts) {
    const row = terminalById.get(source.attemptId);
    const incomplete = incompleteSet.has(source.attemptId);
    const slot = getSummary(runSlots, source.runSlotId, source.runSlotId, null, null);
    const operationKey = JSON.stringify([source.operationId, source.stratum]);
    const operation = getSummary(
      operationStrata,
      operationKey,
      null,
      source.operationId,
      source.stratum,
    );
    const slotOperationKey = JSON.stringify([
      source.runSlotId,
      source.operationId,
      source.stratum,
    ]);
    const slotOperation = getSummary(
      runSlotOperationStrata,
      slotOperationKey,
      source.runSlotId,
      source.operationId,
      source.stratum,
    );
    addAttempt(overall, source, row, incomplete);
    addAttempt(slot, source, row, incomplete);
    addAttempt(operation, source, row, incomplete);
    addAttempt(slotOperation, source, row, incomplete);
  }
  const closedOverall = closeSummary(overall);
  const closedRunSlots = [...runSlots.values()].map(
    // close one exact source run slot
    (summary) => closeSummary(summary),
  );
  const closedOperationStrata = [...operationStrata.values()].map(
    // close one pooled operation stratum
    (summary) => closeSummary(summary),
  );
  const closedRunSlotOperationStrata = [...runSlotOperationStrata.values()].map(
    // close one actual-slot operation stratum
    (summary) => closeSummary(summary),
  );
  const custodyComplete = closedOverall.denominator.complete;
  return deepFreeze({
    attemptCustody: {
      complete: custodyComplete,
      enteredAttempts: closedOverall.denominator.terminalAttempts
        + closedOverall.denominator.enteredIncompleteAttempts,
      enteredIncompleteAttemptIds: closedOverall.enteredIncompleteAttemptIds,
      missingAttemptIds: closedOverall.missingAttemptIds,
      plannedAttempts: closedOverall.denominator.plannedAttempts,
      status: custodyComplete ? "complete" : "blocked",
      terminalAttempts: closedOverall.denominator.terminalAttempts,
    },
    contractVersion: BACKEND_PERFORMANCE_DIRECT_NATIVE_COHORT_VERSION,
    gates: {
      attemptCustody: custodyComplete ? "passed" : "blocked",
      officialTarget: "not-evaluated",
      resourceEvidence: "not-evaluated",
      sourceEvidence: "not-evaluated",
    },
    latencyObservations: buildLatencyObservations(closedRunSlotOperationStrata),
    nativeEligible: false,
    officialTarget: null,
    schedule: {
      contractVersion: BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
      definitionSha256: BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
      runSlotIds: REQUIREMENTS.runSlotIds,
    },
    status: custodyComplete ? "attempt-custody-complete" : "blocked",
    summaries: {
      operationStrata: closedOperationStrata,
      overall: closedOverall,
      runSlotOperationStrata: closedRunSlotOperationStrata,
      runSlots: closedRunSlots,
    },
  });
}
