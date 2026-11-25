import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const ATTEMPT_ACCOUNTING_SOURCE_URL =
  new URL("./backend-performance-attempt-accounting.mjs", import.meta.url);
const ATTEMPT_ACCOUNTING_SOURCE_PIN = Object.freeze({
  bytes: 27791,
  sha256: "2beb9efa8926f91040d01eda005967dd699f172c4fba614aba56298ce8fff721",
});
const QUANTILE_STRATA = new Set(["warm", "once-per-run"]);
const MAXIMUM_STRATA = new Set([
  "application-cache-cold",
  "process-pool-cold",
  "db-os-cold",
]);

export const BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_VERSION =
  "weather-backend-performance-latency-accounting/v1";
export const BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_PAIR_VERSION =
  "weather-backend-performance-latency-accounting-pair/v1";
export const BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_SOURCE_PINS = Object.freeze({
  attemptAccounting: ATTEMPT_ACCOUNTING_SOURCE_PIN,
});

// create one coded latency-accounting error
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one exact latency-accounting invariant
function requireInvariant(condition, code, message) {
  // fail closed on source or accounting drift
  if (!condition) {
    fail(code, message);
  }
}

// hash one exact byte sequence
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// freeze one complete latency-accounting value
function deepFreeze(value) {
  // freeze every nested child before its parent
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    // visit each closed child exactly once
    for (const child of Object.values(value)) {
      deepFreeze(child);
    }
    Object.freeze(value);
  }
  return value;
}

// read and verify the only upstream implementation consumed by this reducer
function readPinnedAttemptAccountingSource() {
  const bytes = readFileSync(ATTEMPT_ACCOUNTING_SOURCE_URL);
  requireInvariant(
    bytes.length === ATTEMPT_ACCOUNTING_SOURCE_PIN.bytes
      && sha256(bytes) === ATTEMPT_ACCOUNTING_SOURCE_PIN.sha256,
    "latency-accounting.source-pin",
    "backend-performance-attempt-accounting.mjs raw identity mismatch",
  );
  return bytes;
}

readPinnedAttemptAccountingSource();
const ATTEMPT_ACCOUNTING_MODULE = await import(ATTEMPT_ACCOUNTING_SOURCE_URL.href);
const {
  evaluateBackendPerformanceAttemptAccounting,
  pairBackendPerformanceAttemptAccounting,
} = ATTEMPT_ACCOUNTING_MODULE;

requireInvariant(
  typeof evaluateBackendPerformanceAttemptAccounting === "function"
    && typeof pairBackendPerformanceAttemptAccounting === "function",
  "latency-accounting.upstream-contract",
  "attempt accounting public adapter exports changed",
);

// build one zeroed terminal-status denominator
function emptyTerminalCounts() {
  return { cancelled: 0, completed: 0, error: 0, timeout: 0 };
}

// create one plan-derived duration bucket
function createBucket(operationId, stratum, runSlotId) {
  return {
    durationsNanoseconds: [],
    operationId,
    plannedAttempts: 0,
    runSlotId,
    stratum,
    terminalCounts: emptyTerminalCounts(),
    unavailableNotStartedAttempts: 0,
  };
}

// add one authoritative attempt to one duration bucket without status filtering
function addAttempt(bucket, attempt) {
  bucket.plannedAttempts += 1;
  bucket.terminalCounts[attempt.terminalCategory] += 1;
  // retain every observed success, failure, timeout, or after-start cancellation duration
  if (attempt.elapsedMonotonic.kind === "observed") {
    bucket.durationsNanoseconds.push(attempt.elapsedMonotonic.nanoseconds);
  } else {
    bucket.unavailableNotStartedAttempts += 1;
  }
}

// compare two integer durations without subtraction overflow
function compareInteger(left, right) {
  // preserve duplicate values exactly
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

// select one nearest-rank order statistic without interpolation
function nearestRank(sortedValues, percentile) {
  const rank = Math.ceil((percentile * sortedValues.length) / 100);
  return sortedValues[rank - 1];
}

// build one fail-closed latency metric for an operation and stratum bucket
function buildMetric(bucket) {
  const complete = bucket.unavailableNotStartedAttempts === 0;
  // summarize warm and once-per-run observations with raw nearest-rank statistics
  if (QUANTILE_STRATA.has(bucket.stratum)) {
    // expose no favorable partial quantile when any planned duration is unavailable
    if (!complete) {
      return {
        kind: "nearest-rank-quantiles",
        p50Nanoseconds: null,
        p95Nanoseconds: null,
        p99Nanoseconds: null,
      };
    }
    const sortedValues = [...bucket.durationsNanoseconds].sort(compareInteger);
    return {
      kind: "nearest-rank-quantiles",
      p50Nanoseconds: nearestRank(sortedValues, 50),
      p95Nanoseconds: nearestRank(sortedValues, 95),
      p99Nanoseconds: nearestRank(sortedValues, 99),
    };
  }
  // summarize each exact cold stratum independently with its raw maximum
  if (MAXIMUM_STRATA.has(bucket.stratum)) {
    // expose no favorable partial maximum when any planned duration is unavailable
    if (!complete) {
      return { kind: "exact-maximum", maxNanoseconds: null };
    }
    let maximum = bucket.durationsNanoseconds[0];
    // compare each raw duration without combining cold strata
    for (let index = 1; index < bucket.durationsNanoseconds.length; index += 1) {
      // retain the greatest exact integer observation
      if (bucket.durationsNanoseconds[index] > maximum) {
        maximum = bucket.durationsNanoseconds[index];
      }
    }
    return { kind: "exact-maximum", maxNanoseconds: maximum };
  }
  fail(
    "latency-accounting.stratum",
    `authoritative attempt accounting contains an unsupported stratum: ${bucket.stratum}`,
  );
}

// close one mutable duration bucket into an explicit immutable denominator row
function closeBucket(bucket) {
  const observedAttempts = bucket.durationsNanoseconds.length;
  requireInvariant(
    bucket.plannedAttempts > 0
      && observedAttempts + bucket.unavailableNotStartedAttempts === bucket.plannedAttempts,
    "latency-accounting.denominator",
    "latency accounting duration denominator is inconsistent",
  );
  return {
    durationCoverage: {
      complete: bucket.unavailableNotStartedAttempts === 0,
      observedAttempts,
      plannedAttempts: bucket.plannedAttempts,
      unavailableNotStartedAttempts: bucket.unavailableNotStartedAttempts,
    },
    latencyNanoseconds: buildMetric(bucket),
    operationId: bucket.operationId,
    plannedAttempts: bucket.plannedAttempts,
    runSlotId: bucket.runSlotId,
    stratum: bucket.stratum,
    terminalCounts: bucket.terminalCounts,
  };
}

// obtain or create one exact grouping bucket in first-plan-appearance order
function getBucket(buckets, key, operationId, stratum, runSlotId) {
  let bucket = buckets.get(key);
  // initialize each exact plan-derived grouping once
  if (bucket === undefined) {
    bucket = createBucket(operationId, stratum, runSlotId);
    buckets.set(key, bucket);
  }
  return bucket;
}

// reduce one authoritative attempt-accounting result to raw latency summaries
function reduceAttemptAccounting(accounting) {
  const pooledBuckets = new Map();
  const runSlotBuckets = new Map();
  // retain authoritative topology and actual run-slot identities from every planned attempt
  for (const attempt of accounting.attempts) {
    const pooledKey = JSON.stringify([attempt.operationId, attempt.stratum]);
    const runSlotKey = JSON.stringify([
      attempt.runSlotId,
      attempt.operationId,
      attempt.stratum,
    ]);
    addAttempt(
      getBucket(
        pooledBuckets,
        pooledKey,
        attempt.operationId,
        attempt.stratum,
        null,
      ),
      attempt,
    );
    addAttempt(
      getBucket(
        runSlotBuckets,
        runSlotKey,
        attempt.operationId,
        attempt.stratum,
        attempt.runSlotId,
      ),
      attempt,
    );
  }
  return {
    pooledOperationStrata: [...pooledBuckets.values()].map(
      // close one pooled operation and stratum denominator
      (bucket) => closeBucket(bucket),
    ),
    runSlotOperationStrata: [...runSlotBuckets.values()].map(
      // close one actual-slot operation and stratum denominator
      (bucket) => closeBucket(bucket),
    ),
  };
}

// retain the complete plan, source, role, and raw accounting identity
function sourceAccountingIdentity(accounting) {
  return {
    accountingDigest: accounting.accountingDigest,
    binding: accounting.binding,
    contractVersion: accounting.contractVersion,
    generation: accounting.generation,
    role: accounting.role,
    sourcePins: accounting.sourcePins,
  };
}

// construct one immutable noncanonical latency-accounting result
function buildLatencyAccounting(accounting) {
  const summaries = reduceAttemptAccounting(accounting);
  const unavailable = accounting.durationCoverage.unavailableNotStartedAttempts;
  return deepFreeze({
    canonicalEligible: false,
    contractVersion: BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_VERSION,
    durationCoverage: accounting.durationCoverage,
    generation: accounting.generation,
    measurementEligibility: {
      eligible: false,
      reasons: unavailable === 0
        ? ["latency-accounting-only-pending-independent-gates"]
        : [
          "latency-accounting-only-pending-independent-gates",
          "one-or-more-elapsed-durations-unavailable",
        ],
    },
    metadataKind: "performance-latency-accounting-nonmeasurement",
    overall: {
      plannedAttempts: accounting.overall.plannedAttempts,
      terminalCounts: accounting.overall.terminalCounts,
    },
    pendingGates: accounting.pendingGates,
    provenance: {
      evaluationMode: "offline-latency-accounting-only",
      measurementEvidence: false,
      syntheticEvidenceEligible: false,
      trafficGenerated: false,
    },
    role: accounting.role,
    sourceAccounting: sourceAccountingIdentity(accounting),
    sourcePins: BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_SOURCE_PINS,
    summaries,
  });
}

// evaluate one raw baseline input through the authoritative public accounting adapter
export function evaluateBackendPerformanceLatencyAccounting(input, ...rest) {
  requireInvariant(
    rest.length === 0,
    "latency-accounting.arguments",
    "latency accounting accepts exactly one raw baseline input",
  );
  return buildLatencyAccounting(
    evaluateBackendPerformanceAttemptAccounting(input),
  );
}

// pair two raw role inputs through the authoritative public accounting pair adapter
export function pairBackendPerformanceLatencyAccounting(
  baselineInput,
  candidateInput,
  ...rest
) {
  requireInvariant(
    rest.length === 0,
    "latency-accounting.arguments",
    "latency accounting pair accepts exactly two raw inputs",
  );
  const sourcePair = pairBackendPerformanceAttemptAccounting(
    baselineInput,
    candidateInput,
  );
  return deepFreeze({
    baseline: buildLatencyAccounting(sourcePair.baseline),
    candidate: buildLatencyAccounting(sourcePair.candidate),
    canonicalEligible: false,
    comparisonEligibility: {
      eligible: false,
      reasons: sourcePair.comparisonEligibility.reasons,
    },
    contractVersion: BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_PAIR_VERSION,
    metadataKind: "performance-latency-accounting-pair-nonmeasurement",
    pendingGates: sourcePair.pendingGates,
    provenance: {
      evaluationMode: "offline-latency-accounting-only",
      measurementEvidence: false,
      syntheticEvidenceEligible: false,
      trafficGenerated: false,
    },
    sourceAccountingPair: {
      contractVersion: sourcePair.contractVersion,
      matched: sourcePair.matched,
      matchedDigest: sourcePair.matchedDigest,
      schedulePair: sourcePair.schedulePair,
    },
    sourcePins: BACKEND_PERFORMANCE_LATENCY_ACCOUNTING_SOURCE_PINS,
  });
}
