import {
  BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
  BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";

export const BACKEND_PERFORMANCE_DIRECT_COVERAGE_VERSION =
  "weather-backend-performance-direct-coverage/v1";

// identify one plain coverage record
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    && (Object.getPrototypeOf(value) === Object.prototype
      || Object.getPrototypeOf(value) === null);
}

// create one operation-stratum identity
function bucketKey(operationId, stratum) {
  return JSON.stringify([operationId, stratum]);
}

// project the approved schedule into exact coverage buckets
function buildRequirements() {
  const schedule = buildBackendPerformanceFunctionalSchedule();
  const buckets = new Map();
  const attemptById = new Map();

  // retain every approved functional attempt once
  for (const runSlot of schedule.runSlots) {
    // retain every approved group attempt
    for (const group of runSlot.groups) {
      // add one exact attempt to its operation-stratum bucket
      for (const attempt of group.attempts) {
        const key = bucketKey(attempt.operationId, attempt.stratum);
        let bucket = buckets.get(key);

        // initialize one source-ordered bucket
        if (bucket === undefined) {
          bucket = {
            attemptIds: [],
            operationId: attempt.operationId,
            stratum: attempt.stratum,
          };
          buckets.set(key, bucket);
        }
        bucket.attemptIds.push(attempt.attemptId);
        attemptById.set(attempt.attemptId, {
          operationId: attempt.operationId,
          stratum: attempt.stratum,
        });
      }
    }
  }
  return {
    attemptById,
    buckets: [...buckets.values()],
    definitionSha256: BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
    operationCount: schedule.functionalOracles.operations.length,
    plannedAttempts: schedule.totals.plannedAttempts,
    scheduleVersion: BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
  };
}

// append one compact reconciliation diagnostic
function addDiagnostic(diagnostics, code, receiptIndex, message) {
  diagnostics.push({ code, message, receiptIndex });
}

// extract exact attempt identities from one existing direct result shape
function receiptAttemptIds(value) {
  const candidates = [
    value?.sourceAttemptIds,
    value?.sourceBinding?.sourceAttemptIds,
    value?.sourceBinding?.readAttemptIds,
    value?.coldPolicy?.processPoolCold?.sourceAttemptIds,
  ].filter(
    // retain only explicit attempt identity arrays
    (candidate) => Array.isArray(candidate),
  );
  return candidates.length === 1 ? candidates[0] : null;
}

// inspect one measured receipt without converting fixtures into native evidence
function inspectReceipt(value, index, requirements, diagnostics) {
  // reject an incomplete direct result shape
  if (!isRecord(value) || !isRecord(value.operationStratum)
    || !isRecord(value.attemptDenominator) || !isRecord(value.terminalCounts)) {
    addDiagnostic(
      diagnostics,
      "direct-coverage.receipt",
      index,
      "operation receipt lacks one measured stratum and complete denominator",
    );
    return null;
  }
  const planned = value.attemptDenominator.plannedAttempts;
  const observed = value.attemptDenominator.observedAttempts;
  const terminalTotal = ["completed", "error", "timeout"].reduce(
    // retain the complete terminal denominator
    (total, name) => total + (Number.isSafeInteger(value.terminalCounts[name])
      ? value.terminalCounts[name]
      : Number.NaN),
    0,
  );

  // reject a partial or inconsistent receipt denominator
  if (!Number.isSafeInteger(planned) || planned <= 0 || observed !== planned
    || terminalTotal !== planned || value.operationStratum.observedAttempts !== planned) {
    addDiagnostic(
      diagnostics,
      "direct-coverage.denominator",
      index,
      "operation receipt does not retain its complete planned and terminal denominator",
    );
    return null;
  }
  const operationId = value.operationStratum.operationId;
  const stratum = value.operationStratum.stratum;
  const key = bucketKey(operationId, stratum);
  const requiredBucket = requirements.buckets.find(
    // select the exact approved source bucket
    (bucket) => bucket.operationId === operationId && bucket.stratum === stratum,
  );

  // reject receipts outside the approved normal-disposition schedule
  if (requiredBucket === undefined) {
    addDiagnostic(
      diagnostics,
      "direct-coverage.bucket",
      index,
      `operation receipt names an unapproved bucket ${String(operationId)}/${String(stratum)}`,
    );
    return null;
  }
  const attemptIds = receiptAttemptIds(value);

  // require exact source attempts rather than trusting one aggregate count
  if (attemptIds === null || attemptIds.length !== planned
    || new Set(attemptIds).size !== attemptIds.length) {
    addDiagnostic(
      diagnostics,
      "direct-coverage.attempt-identities",
      index,
      `${operationId}/${stratum} receipt lacks one unique source attempt per observed row`,
    );
    return {
      attemptIds: [],
      key,
      nativeEligible: false,
      observedAttempts: observed,
    };
  }

  // reject cross-bucket or unknown attempt identities
  for (const attemptId of attemptIds) {
    const expected = requirements.attemptById.get(attemptId);

    // bind each row to the exact approved bucket
    if (expected?.operationId !== operationId || expected?.stratum !== stratum) {
      addDiagnostic(
        diagnostics,
        "direct-coverage.attempt-binding",
        index,
        `${operationId}/${stratum} receipt contains an unknown or cross-bucket attempt`,
      );
      return {
        attemptIds: [],
        key,
        nativeEligible: false,
        observedAttempts: observed,
      };
    }
  }
  return {
    attemptIds,
    key,
    nativeEligible: value.nativeEligible === true
      && value.evidenceClass !== "synthetic-fixture",
    observedAttempts: observed,
  };
}

// reconcile supplied result receipts against every approved functional bucket
export function reconcileBackendPerformanceDirectCoverage(operationReceipts) {
  // reject open reconciliation inputs
  if (arguments.length !== 1 || !Array.isArray(operationReceipts)) {
    const error = new Error("direct coverage reconciliation accepts one receipt array");
    error.code = "direct-coverage.arguments";
    throw error;
  }
  const requirements = buildRequirements();
  const diagnostics = [];
  const receiptBoundByBucket = new Map();
  const nativeBoundByBucket = new Map();

  // inspect every supplied receipt once
  for (let index = 0; index < operationReceipts.length; index += 1) {
    const receipt = inspectReceipt(operationReceipts[index], index, requirements, diagnostics);

    // omit receipts without one approved bucket
    if (receipt === null) {
      continue;
    }
    const receiptBound = receiptBoundByBucket.get(receipt.key) ?? new Set();
    const nativeBound = nativeBoundByBucket.get(receipt.key) ?? new Set();

    // add each exact attempt without increasing duplicate coverage
    for (const attemptId of receipt.attemptIds) {
      // report replayed result rows compactly
      if (receiptBound.has(attemptId)) {
        addDiagnostic(
          diagnostics,
          "direct-coverage.duplicate-attempt",
          index,
          "operation receipt repeats an already reconciled source attempt",
        );
      }
      receiptBound.add(attemptId);

      // grant native coverage only to an explicitly native receipt
      if (receipt.nativeEligible) {
        nativeBound.add(attemptId);
      }
    }
    receiptBoundByBucket.set(receipt.key, receiptBound);
    nativeBoundByBucket.set(receipt.key, nativeBound);
  }
  const bucketRows = requirements.buckets.map(
    // derive one compact source-owned bucket result
    (required) => {
      const key = bucketKey(required.operationId, required.stratum);
      const requiredSet = new Set(required.attemptIds);
      const receiptBound = receiptBoundByBucket.get(key) ?? new Set();
      const nativeBound = nativeBoundByBucket.get(key) ?? new Set();
      const receiptBoundAttempts = [...receiptBound].filter(
        // retain only source-required receipt attempts
        (attemptId) => requiredSet.has(attemptId),
      ).length;
      const nativeBoundAttempts = [...nativeBound].filter(
        // retain only source-required native attempts
        (attemptId) => requiredSet.has(attemptId),
      ).length;
      return {
        missingNativeAttempts: required.attemptIds.length - nativeBoundAttempts,
        nativeBoundAttempts,
        operationId: required.operationId,
        receiptBoundAttempts,
        requiredAttempts: required.attemptIds.length,
        stratum: required.stratum,
      };
    },
  );
  const missingBuckets = bucketRows.filter(
    // retain every incomplete native bucket
    (bucket) => bucket.missingNativeAttempts > 0,
  );
  const observedBuckets = bucketRows.filter(
    // retain only buckets with exact supplied receipt rows
    (bucket) => bucket.receiptBoundAttempts > 0,
  );
  const receiptBoundAttempts = bucketRows.reduce(
    // sum one exact receipt-bound bucket
    (total, bucket) => total + bucket.receiptBoundAttempts,
    0,
  );
  const nativeBoundAttempts = bucketRows.reduce(
    // sum one exact native-bound bucket
    (total, bucket) => total + bucket.nativeBoundAttempts,
    0,
  );
  diagnostics.sort(
    // retain deterministic diagnostic order
    (left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right)),
  );
  return Object.freeze({
    contractVersion: BACKEND_PERFORMANCE_DIRECT_COVERAGE_VERSION,
    diagnostics,
    missingBuckets,
    nativeComplete: missingBuckets.length === 0,
    observedBuckets,
    source: {
      functionalDefinitionSha256: requirements.definitionSha256,
      functionalScheduleVersion: requirements.scheduleVersion,
      projection: "accepted-normal-disposition-functional-schedule",
    },
    summary: {
      completeNativeBuckets: bucketRows.length - missingBuckets.length,
      missingNativeAttempts: requirements.plannedAttempts - nativeBoundAttempts,
      missingNativeBuckets: missingBuckets.length,
      nativeBoundAttempts,
      receiptBoundAttempts,
      requiredAttempts: requirements.plannedAttempts,
      requiredBuckets: bucketRows.length,
      requiredOperations: requirements.operationCount,
    },
  });
}
