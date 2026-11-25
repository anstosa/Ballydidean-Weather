import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const FUNCTIONAL_SCHEDULE_SOURCE_URL =
  new URL("./backend-performance-functional-schedule.mjs", import.meta.url);
const SCHEDULE_BINDING_SOURCE_URL =
  new URL("./backend-performance-schedule-binding.mjs", import.meta.url);
const SOURCE_PINS = Object.freeze({
  functionalSchedule: Object.freeze({
    bytes: 15179,
    sha256: "268317431bf2aa78d4a4774be0a8d442970d9a842194f6ff480542b398b05110",
  }),
  scheduleBinding: Object.freeze({
    bytes: 9610,
    sha256: "5bc680dc8a7b0a12d80b71de4da0a0debc5af7e9a2b35d15355bfca91cba1325",
  }),
});
const DIGEST_ALGORITHM = "SHA-256";
const DIGEST_ENCODING = "lowercase-hex";
const CANONICALIZATION = "recursive-json-ecmascript-code-unit-v1";
const EXPECTED_PLANNED_ATTEMPTS = 6275;
const TERMINAL_CATEGORIES = Object.freeze([
  "completed",
  "error",
  "timeout",
  "cancelled",
]);
const TERMINAL_CATEGORY_SET = new Set(TERMINAL_CATEGORIES);
const ERROR_CATEGORIES = Object.freeze({
  error: Object.freeze(["http", "process", "transport", "observation"]),
  timeout: Object.freeze(["deadline-exceeded"]),
  cancelled: Object.freeze(["cancelled-before-start", "cancelled-after-start"]),
});
const TERMINAL_STATUS_KINDS = Object.freeze([
  "none",
  "http",
  "process",
  "http-and-process",
]);
const TERMINAL_STATUS_KIND_SET = new Set(TERMINAL_STATUS_KINDS);
// accept the fixed Linux standard signal names 1 through 31 without aliases
const LINUX_PROCESS_SIGNALS = Object.freeze([
  "SIGHUP",
  "SIGINT",
  "SIGQUIT",
  "SIGILL",
  "SIGTRAP",
  "SIGABRT",
  "SIGBUS",
  "SIGFPE",
  "SIGKILL",
  "SIGUSR1",
  "SIGSEGV",
  "SIGUSR2",
  "SIGPIPE",
  "SIGALRM",
  "SIGTERM",
  "SIGSTKFLT",
  "SIGCHLD",
  "SIGCONT",
  "SIGSTOP",
  "SIGTSTP",
  "SIGTTIN",
  "SIGTTOU",
  "SIGURG",
  "SIGXCPU",
  "SIGXFSZ",
  "SIGVTALRM",
  "SIGPROF",
  "SIGWINCH",
  "SIGIO",
  "SIGPWR",
  "SIGSYS",
]);
const LINUX_PROCESS_SIGNAL_SET = new Set(LINUX_PROCESS_SIGNALS);
const ROLE_GENERATIONS = Object.freeze({
  baseline: "node-v1",
  candidate: "rust-v1",
});
const PENDING_GATES = Object.freeze({
  failedCorrectness: "not-evaluated",
  resource: "not-evaluated",
  state: "not-evaluated",
  target: "not-evaluated",
  witness: "not-evaluated",
});

export const BACKEND_PERFORMANCE_ATTEMPT_ACCOUNTING_VERSION =
  "weather-backend-performance-attempt-accounting/v1";
export const BACKEND_PERFORMANCE_ATTEMPT_ACCOUNTING_PAIR_VERSION =
  "weather-backend-performance-attempt-accounting-pair/v1";

// create one coded accounting error
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one exact accounting invariant
function requireInvariant(condition, code, message) {
  // fail closed on malformed accounting data
  if (!condition) {
    fail(code, message);
  }
}

// hash one exact byte sequence
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// compare strings by ECMAScript code units
function compareText(left, right) {
  // preserve equal strings without locale behavior
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

// serialize one closed plain JSON value
function canonicalJson(value, path = "$") {
  // preserve null as one primitive
  if (value === null) {
    return "null";
  }

  // preserve every declared array position
  if (Array.isArray(value)) {
    const children = [];
    // reject sparse arrays before serialization
    for (let index = 0; index < value.length; index += 1) {
      requireInvariant(
        Object.hasOwn(value, index),
        "attempt-accounting.schema",
        `attempt accounting contains an array hole at ${path}[${String(index)}]`,
      );
      children.push(canonicalJson(value[index], `${path}[${String(index)}]`));
    }
    return `[${children.join(",")}]`;
  }

  // serialize only ordinary parsed-json records
  if (typeof value === "object") {
    const prototype = Object.getPrototypeOf(value);
    requireInvariant(
      prototype === Object.prototype || prototype === null,
      "attempt-accounting.schema",
      `attempt accounting contains an exotic object at ${path}`,
    );
    const keys = Object.keys(value).sort(compareText);
    return `{${keys.map(
      // bind one normalized key to its child
      (key) => `${JSON.stringify(key)}:${canonicalJson(value[key], `${path}.${key}`)}`,
    ).join(",")}}`;
  }

  // retain only finite JSON primitives
  if (typeof value === "string" || typeof value === "boolean"
    || (typeof value === "number" && Number.isFinite(value))) {
    return JSON.stringify(value);
  }
  fail("attempt-accounting.schema", `attempt accounting contains a non-json value at ${path}`);
}

// compare two exact closed values
function sameCanonicalValue(left, right) {
  return canonicalJson(left) === canonicalJson(right);
}

// freeze one complete accounting value
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

// require one ordinary parsed-json record
function requirePlainRecord(value, code, message) {
  const valid = value !== null && typeof value === "object" && !Array.isArray(value)
    && (Object.getPrototypeOf(value) === Object.prototype
      || Object.getPrototypeOf(value) === null);
  requireInvariant(valid, code, message);
  return value;
}

// require one exact record key set
function requireExactKeys(value, expectedKeys, code, message) {
  const actualKeys = Object.keys(value).sort(compareText);
  const normalizedExpectedKeys = [...expectedKeys].sort(compareText);
  requireInvariant(
    sameCanonicalValue(actualKeys, normalizedExpectedKeys),
    code,
    message,
  );
}

// add two safe nonnegative counts
function addSafeCount(left, right, label) {
  const total = left + right;
  requireInvariant(
    Number.isSafeInteger(total) && total >= 0,
    "attempt-accounting.unsafe-count",
    `${label} exceeds the safe integer range`,
  );
  return total;
}

// verify one pinned source before importing it
function verifySourcePin(sourceUrl, pin, label) {
  const bytes = readFileSync(sourceUrl);
  requireInvariant(
    bytes.length === pin.bytes && sha256(bytes) === pin.sha256,
    "attempt-accounting.source-pin",
    `${label} raw identity changed`,
  );
}

verifySourcePin(
  FUNCTIONAL_SCHEDULE_SOURCE_URL,
  SOURCE_PINS.functionalSchedule,
  "functional schedule builder",
);
verifySourcePin(
  SCHEDULE_BINDING_SOURCE_URL,
  SOURCE_PINS.scheduleBinding,
  "schedule binding builder",
);

const functionalScheduleModule = await import(FUNCTIONAL_SCHEDULE_SOURCE_URL.href);
const scheduleBindingModule = await import(SCHEDULE_BINDING_SOURCE_URL.href);
const { buildBackendPerformanceFunctionalSchedule } = functionalScheduleModule;
const {
  buildBackendPerformanceScheduleBinding,
  pairBackendPerformanceScheduleBindings,
} = scheduleBindingModule;

requireInvariant(
  typeof buildBackendPerformanceFunctionalSchedule === "function"
    && typeof buildBackendPerformanceScheduleBinding === "function"
    && typeof pairBackendPerformanceScheduleBindings === "function",
  "attempt-accounting.builder-contract",
  "authoritative schedule or binding builder contract changed",
);

const authoritativePlan = buildBackendPerformanceFunctionalSchedule();
const authoritativeBindings = deepFreeze({
  baseline: buildBackendPerformanceScheduleBinding({
    generation: ROLE_GENERATIONS.baseline,
    role: "baseline",
  }),
  candidate: buildBackendPerformanceScheduleBinding({
    generation: ROLE_GENERATIONS.candidate,
    role: "candidate",
  }),
});

// flatten the fixed schedule into authoritative attempt topology
function buildAuthoritativeAttempts(plan) {
  const attempts = [];
  const attemptIds = new Set();
  // retain fixed run-slot order
  for (const runSlot of plan.runSlots) {
    // retain fixed group order within the slot
    for (const group of runSlot.groups) {
      // retain fixed attempt order within the group
      for (const attempt of group.attempts) {
        requireInvariant(
          attempt.runSlotId === runSlot.id && attempt.groupId === group.id,
          "attempt-accounting.topology",
          `authoritative topology changed for ${attempt.attemptId}`,
        );
        requireInvariant(
          typeof attempt.attemptId === "string" && !attemptIds.has(attempt.attemptId),
          "attempt-accounting.topology",
          `authoritative attempt identity changed for ${attempt.attemptId}`,
        );
        attemptIds.add(attempt.attemptId);
        attempts.push(deepFreeze({
          attemptId: attempt.attemptId,
          groupId: attempt.groupId,
          operationId: attempt.operationId,
          ordinal: attempt.ordinal,
          runSlotId: attempt.runSlotId,
          stratum: attempt.stratum,
        }));
      }
    }
  }
  requireInvariant(
    attempts.length === EXPECTED_PLANNED_ATTEMPTS
      && attemptIds.size === EXPECTED_PLANNED_ATTEMPTS
      && plan.totals.plannedAttempts === EXPECTED_PLANNED_ATTEMPTS,
    "attempt-accounting.topology",
    "authoritative planned attempt denominator changed",
  );
  return deepFreeze(attempts);
}

const AUTHORITATIVE_ATTEMPTS = buildAuthoritativeAttempts(authoritativePlan);
const AUTHORITATIVE_ATTEMPT_IDS = new Set(
  AUTHORITATIVE_ATTEMPTS.map(
    // index one exact planned identity
    (attempt) => attempt.attemptId,
  ),
);

// identify one exact authoritative role binding
function identifyAuthoritativeBinding(binding) {
  // accept the baseline only by full authoritative equality
  if (sameCanonicalValue(binding, authoritativeBindings.baseline)) {
    return {
      binding: authoritativeBindings.baseline,
      generation: ROLE_GENERATIONS.baseline,
      role: "baseline",
    };
  }
  // accept the candidate only by full authoritative equality
  if (sameCanonicalValue(binding, authoritativeBindings.candidate)) {
    return {
      binding: authoritativeBindings.candidate,
      generation: ROLE_GENERATIONS.candidate,
      role: "candidate",
    };
  }
  fail(
    "attempt-accounting.binding",
    "attempt accounting binding does not match either authoritative role",
  );
}

// require one bounded safe integer
function requireSafeInteger(value, minimum, maximum, label) {
  requireInvariant(
    Number.isSafeInteger(value) && value >= minimum && value <= maximum,
    "attempt-accounting.observation",
    `${label} must be one safe integer from ${String(minimum)} through ${String(maximum)}`,
  );
  return value;
}

// normalize one closed monotonic elapsed observation
function normalizeElapsed(elapsed, terminalCategory, index) {
  requirePlainRecord(
    elapsed,
    "attempt-accounting.observation",
    `outcome ${String(index)} elapsedMonotonic must be one plain record`,
  );
  requireInvariant(
    elapsed.kind === "observed" || elapsed.kind === "unavailable",
    "attempt-accounting.observation",
    `outcome ${String(index)} elapsedMonotonic kind is invalid`,
  );

  // retain one positive monotonic observation
  if (elapsed.kind === "observed") {
    requireExactKeys(
      elapsed,
      ["kind", "nanoseconds"],
      "attempt-accounting.observation",
      `outcome ${String(index)} observed elapsedMonotonic keys are invalid`,
    );
    return {
      kind: "observed",
      nanoseconds: requireSafeInteger(
        elapsed.nanoseconds,
        1,
        Number.MAX_SAFE_INTEGER,
        `outcome ${String(index)} elapsedMonotonic.nanoseconds`,
      ),
    };
  }

  requireExactKeys(
    elapsed,
    ["kind", "reason"],
    "attempt-accounting.observation",
    `outcome ${String(index)} unavailable elapsedMonotonic keys are invalid`,
  );
  requireInvariant(
    terminalCategory === "cancelled" && elapsed.reason === "not-started",
    "attempt-accounting.observation",
    `outcome ${String(index)} may omit elapsed time only for cancellation before start`,
  );
  return { kind: "unavailable", reason: "not-started" };
}

// normalize one real terminal status record
function normalizeTerminalStatus(status, index) {
  requirePlainRecord(
    status,
    "attempt-accounting.observation",
    `outcome ${String(index)} terminalStatus must be one plain record`,
  );
  requireExactKeys(
    status,
    ["httpStatus", "kind", "processExitCode", "processSignal"],
    "attempt-accounting.observation",
    `outcome ${String(index)} terminalStatus keys are invalid`,
  );
  requireInvariant(
    TERMINAL_STATUS_KIND_SET.has(status.kind),
    "attempt-accounting.observation",
    `outcome ${String(index)} terminalStatus kind is invalid`,
  );
  const hasHttpStatus = status.httpStatus !== null;
  const hasProcessExitCode = status.processExitCode !== null;
  const hasProcessSignal = status.processSignal !== null;
  // validate a present HTTP status without interpreting correctness
  if (hasHttpStatus) {
    requireSafeInteger(
      status.httpStatus,
      100,
      599,
      `outcome ${String(index)} terminalStatus.httpStatus`,
    );
  }
  // validate a present process status without interpreting correctness
  if (hasProcessExitCode) {
    requireSafeInteger(
      status.processExitCode,
      0,
      255,
      `outcome ${String(index)} terminalStatus.processExitCode`,
    );
  }
  // validate a present fixed Linux process signal
  if (hasProcessSignal) {
    requireInvariant(
      typeof status.processSignal === "string"
        && LINUX_PROCESS_SIGNAL_SET.has(status.processSignal),
      "attempt-accounting.observation",
      `outcome ${String(index)} terminalStatus.processSignal is invalid`,
    );
  }
  requireInvariant(
    !(hasProcessExitCode && hasProcessSignal),
    "attempt-accounting.observation",
    `outcome ${String(index)} process exit code and signal are mutually exclusive`,
  );
  const hasProcessStatus = hasProcessExitCode || hasProcessSignal;
  const expectedKind = hasHttpStatus
    ? (hasProcessStatus ? "http-and-process" : "http")
    : (hasProcessStatus ? "process" : "none");
  requireInvariant(
    status.kind === expectedKind,
    "attempt-accounting.observation",
    `outcome ${String(index)} terminalStatus kind does not match its statuses`,
  );
  return {
    httpStatus: status.httpStatus,
    kind: status.kind,
    processExitCode: status.processExitCode,
    processSignal: status.processSignal,
  };
}

// normalize one stable non-secret error category
function normalizeErrorCategory(errorCategory, terminalCategory, elapsed, index) {
  // completed observations carry no error classification
  if (terminalCategory === "completed") {
    requireInvariant(
      errorCategory === null,
      "attempt-accounting.observation",
      `outcome ${String(index)} completed errorCategory must be null`,
    );
    return null;
  }
  const allowed = ERROR_CATEGORIES[terminalCategory];
  requireInvariant(
    typeof errorCategory === "string" && allowed.includes(errorCategory),
    "attempt-accounting.observation",
    `outcome ${String(index)} errorCategory is not a stable permitted category`,
  );
  // couple before-start cancellation to unavailable elapsed time
  if (terminalCategory === "cancelled" && elapsed.kind === "unavailable") {
    requireInvariant(
      errorCategory === "cancelled-before-start",
      "attempt-accounting.observation",
      `outcome ${String(index)} unavailable cancellation category is inconsistent`,
    );
  }
  // couple started cancellation to an observed elapsed time
  if (terminalCategory === "cancelled" && elapsed.kind === "observed") {
    requireInvariant(
      errorCategory === "cancelled-after-start",
      "attempt-accounting.observation",
      `outcome ${String(index)} observed cancellation category is inconsistent`,
    );
  }
  return errorCategory;
}

// normalize one raw terminal outcome without plan grouping claims
function normalizeOutcome(outcome, index) {
  requirePlainRecord(
    outcome,
    "attempt-accounting.observation",
    `outcome ${String(index)} must be one plain record`,
  );
  requireExactKeys(
    outcome,
    [
      "attemptId",
      "elapsedMonotonic",
      "errorCategory",
      "httpResponseBytes",
      "processOutputBytes",
      "terminalCategory",
      "terminalStatus",
    ],
    "attempt-accounting.observation",
    `outcome ${String(index)} keys are invalid`,
  );
  requireInvariant(
    typeof outcome.attemptId === "string" && outcome.attemptId.length > 0,
    "attempt-accounting.observation",
    `outcome ${String(index)} attemptId must be one nonempty string`,
  );
  requireInvariant(
    TERMINAL_CATEGORY_SET.has(outcome.terminalCategory),
    "attempt-accounting.observation",
    `outcome ${String(index)} terminalCategory is invalid`,
  );
  const elapsedMonotonic = normalizeElapsed(
    outcome.elapsedMonotonic,
    outcome.terminalCategory,
    index,
  );
  const terminalStatus = normalizeTerminalStatus(outcome.terminalStatus, index);
  // completed work must expose at least one real terminal status
  if (outcome.terminalCategory === "completed") {
    requireInvariant(
      terminalStatus.kind !== "none",
      "attempt-accounting.observation",
      `outcome ${String(index)} completed terminalStatus must not be none`,
    );
  }
  const errorCategory = normalizeErrorCategory(
    outcome.errorCategory,
    outcome.terminalCategory,
    elapsedMonotonic,
    index,
  );
  // require a truly not-started cancellation to have no terminal or byte evidence
  if (elapsedMonotonic.kind === "unavailable") {
    requireInvariant(
      terminalStatus.kind === "none"
        && outcome.httpResponseBytes === 0
        && outcome.processOutputBytes === 0,
      "attempt-accounting.observation",
      `outcome ${String(index)} not-started cancellation has inconsistent evidence`,
    );
  }
  return {
    attemptId: outcome.attemptId,
    elapsedMonotonic,
    errorCategory,
    httpResponseBytes: requireSafeInteger(
      outcome.httpResponseBytes,
      0,
      Number.MAX_SAFE_INTEGER,
      `outcome ${String(index)} httpResponseBytes`,
    ),
    processOutputBytes: requireSafeInteger(
      outcome.processOutputBytes,
      0,
      Number.MAX_SAFE_INTEGER,
      `outcome ${String(index)} processOutputBytes`,
    ),
    terminalCategory: outcome.terminalCategory,
    terminalStatus,
  };
}

// build one zeroed terminal denominator
function emptyTerminalCounts() {
  return { cancelled: 0, completed: 0, error: 0, timeout: 0 };
}

// build one operation and stratum accounting bucket
function createBucket(operationId, stratum) {
  return {
    bytes: {
      httpResponseBytes: 0,
      processOutputBytes: 0,
      totalBytes: 0,
    },
    elapsedMonotonic: {
      observedAttempts: 0,
      unavailableNotStartedAttempts: 0,
    },
    operationId,
    plannedAttempts: 0,
    stratum,
    terminalCounts: emptyTerminalCounts(),
  };
}

// add one normalized attempt to one accounting bucket
function accountAttempt(bucket, attempt) {
  bucket.plannedAttempts = addSafeCount(
    bucket.plannedAttempts,
    1,
    "planned attempt count",
  );
  bucket.terminalCounts[attempt.terminalCategory] = addSafeCount(
    bucket.terminalCounts[attempt.terminalCategory],
    1,
    `${attempt.terminalCategory} terminal count`,
  );
  bucket.bytes.httpResponseBytes = addSafeCount(
    bucket.bytes.httpResponseBytes,
    attempt.httpResponseBytes,
    "HTTP response byte count",
  );
  bucket.bytes.processOutputBytes = addSafeCount(
    bucket.bytes.processOutputBytes,
    attempt.processOutputBytes,
    "process output byte count",
  );
  bucket.bytes.totalBytes = addSafeCount(
    bucket.bytes.totalBytes,
    addSafeCount(
      attempt.httpResponseBytes,
      attempt.processOutputBytes,
      "attempt total byte count",
    ),
    "total byte count",
  );
  // retain observed and unavailable duration denominators separately
  if (attempt.elapsedMonotonic.kind === "observed") {
    bucket.elapsedMonotonic.observedAttempts = addSafeCount(
      bucket.elapsedMonotonic.observedAttempts,
      1,
      "observed duration count",
    );
  } else {
    bucket.elapsedMonotonic.unavailableNotStartedAttempts = addSafeCount(
      bucket.elapsedMonotonic.unavailableNotStartedAttempts,
      1,
      "unavailable duration count",
    );
  }
}

// construct one immutable digest record
function digestRecord(value) {
  const serialized = canonicalJson(value);
  return {
    algorithm: DIGEST_ALGORITHM,
    canonicalByteLength: Buffer.byteLength(serialized, "utf8"),
    canonicalization: CANONICALIZATION,
    encoding: DIGEST_ENCODING,
    value: sha256(Buffer.from(serialized, "utf8")),
  };
}

// evaluate one expected role independently from raw terminal observations
function evaluateRoleAttemptAccounting(input, expectedRole) {
  requirePlainRecord(
    input,
    "attempt-accounting.arguments",
    "attempt accounting input must be one plain record",
  );
  requireExactKeys(
    input,
    ["binding", "outcomes"],
    "attempt-accounting.arguments",
    "attempt accounting input must contain only binding and outcomes",
  );
  const role = identifyAuthoritativeBinding(input.binding);
  requireInvariant(
    role.role === expectedRole,
    "attempt-accounting.role",
    `${expectedRole} binding is not the authoritative ${expectedRole} role`,
  );
  requireInvariant(
    Array.isArray(input.outcomes),
    "attempt-accounting.outcomes",
    "attempt accounting outcomes must be one array",
  );
  requireInvariant(
    Object.keys(input.outcomes).length === input.outcomes.length,
    "attempt-accounting.outcomes",
    "attempt accounting outcomes must be one dense plain JSON array",
  );
  const suppliedByAttemptId = new Map();
  // validate raw observations before any plan-order aggregation
  for (let index = 0; index < input.outcomes.length; index += 1) {
    requireInvariant(
      Object.hasOwn(input.outcomes, index),
      "attempt-accounting.outcomes",
      `attempt accounting outcomes contains a hole at ${String(index)}`,
    );
    const outcome = normalizeOutcome(input.outcomes[index], index);
    requireInvariant(
      AUTHORITATIVE_ATTEMPT_IDS.has(outcome.attemptId),
      "attempt-accounting.unplanned-attempt",
      `outcome attemptId is not in the authoritative plan: ${outcome.attemptId}`,
    );
    requireInvariant(
      !suppliedByAttemptId.has(outcome.attemptId),
      "attempt-accounting.duplicate-attempt",
      `duplicate outcome attemptId: ${outcome.attemptId}`,
    );
    suppliedByAttemptId.set(outcome.attemptId, outcome);
  }
  requireInvariant(
    suppliedByAttemptId.size === EXPECTED_PLANNED_ATTEMPTS,
    "attempt-accounting.missing-attempt",
    `attempt accounting requires all ${String(EXPECTED_PLANNED_ATTEMPTS)} planned outcomes`,
  );

  const overall = createBucket(null, null);
  const operationStrataByKey = new Map();
  const attempts = [];
  // match every outcome in authoritative topology order
  for (const planned of AUTHORITATIVE_ATTEMPTS) {
    const observed = suppliedByAttemptId.get(planned.attemptId);
    requireInvariant(
      observed !== undefined,
      "attempt-accounting.missing-attempt",
      `missing outcome attemptId: ${planned.attemptId}`,
    );
    const attempt = {
      attemptId: planned.attemptId,
      elapsedMonotonic: observed.elapsedMonotonic,
      errorCategory: observed.errorCategory,
      groupId: planned.groupId,
      httpResponseBytes: observed.httpResponseBytes,
      operationId: planned.operationId,
      ordinal: planned.ordinal,
      processOutputBytes: observed.processOutputBytes,
      runSlotId: planned.runSlotId,
      stratum: planned.stratum,
      terminalCategory: observed.terminalCategory,
      terminalStatus: observed.terminalStatus,
    };
    attempts.push(attempt);
    accountAttempt(overall, attempt);
    const bucketKey = canonicalJson([planned.operationId, planned.stratum]);
    let bucket = operationStrataByKey.get(bucketKey);
    // initialize one plan-derived operation and stratum denominator
    if (bucket === undefined) {
      bucket = createBucket(planned.operationId, planned.stratum);
      operationStrataByKey.set(bucketKey, bucket);
    }
    accountAttempt(bucket, attempt);
  }

  const operationStrata = [...operationStrataByKey.values()];
  const unavailableDurations = overall.elapsedMonotonic.unavailableNotStartedAttempts;
  const digestInput = {
    attempts,
    bindingIdentitySha256: role.binding.identityDigest.value,
    role: role.role,
  };
  return deepFreeze({
    accountingDigest: digestRecord(digestInput),
    attempts,
    binding: role.binding,
    canonicalEligible: false,
    contractVersion: BACKEND_PERFORMANCE_ATTEMPT_ACCOUNTING_VERSION,
    durationCoverage: {
      complete: unavailableDurations === 0,
      observedAttempts: overall.elapsedMonotonic.observedAttempts,
      unavailableNotStartedAttempts: unavailableDurations,
    },
    generation: role.generation,
    measurementEligibility: {
      eligible: false,
      reasons: unavailableDurations === 0
        ? ["accounting-only-pending-independent-gates"]
        : [
          "accounting-only-pending-independent-gates",
          "elapsed-duration-unavailable-for-not-started-cancellation",
        ],
    },
    metadataKind: "performance-attempt-accounting-nonmeasurement",
    operationStrata,
    overall,
    pendingGates: PENDING_GATES,
    provenance: {
      evaluationMode: "offline-accounting-only",
      measurementEvidence: false,
      syntheticEvidenceEligible: false,
      trafficGenerated: false,
    },
    role: role.role,
    sourcePins: SOURCE_PINS,
  });
}

// evaluate only the baseline role through the public solo boundary
export function evaluateBackendPerformanceAttemptAccounting(input, ...rest) {
  requireInvariant(
    rest.length === 0,
    "attempt-accounting.arguments",
    "attempt accounting accepts exactly one raw baseline input",
  );
  return evaluateRoleAttemptAccounting(input, "baseline");
}

// pair two independently evaluated raw role inputs
export function pairBackendPerformanceAttemptAccounting(
  baselineInput,
  candidateInput,
  ...rest
) {
  requireInvariant(
    rest.length === 0,
    "attempt-accounting.arguments",
    "attempt accounting pair accepts exactly two raw inputs",
  );
  const baseline = evaluateRoleAttemptAccounting(baselineInput, "baseline");
  const candidate = evaluateRoleAttemptAccounting(candidateInput, "candidate");
  const schedulePair = pairBackendPerformanceScheduleBindings(
    baselineInput.binding,
    candidateInput.binding,
  );
  const matched = {
    baselineAccountingSha256: baseline.accountingDigest.value,
    candidateAccountingSha256: candidate.accountingDigest.value,
    schedulePairSha256: schedulePair.matchedDigest.value,
  };
  const hasUnavailableDuration = !baseline.durationCoverage.complete
    || !candidate.durationCoverage.complete;
  return deepFreeze({
    baseline,
    candidate,
    canonicalEligible: false,
    comparisonEligibility: {
      eligible: false,
      reasons: hasUnavailableDuration
        ? [
          "accounting-only-pending-independent-gates",
          "one-or-more-elapsed-durations-unavailable",
        ]
        : ["accounting-only-pending-independent-gates"],
    },
    contractVersion: BACKEND_PERFORMANCE_ATTEMPT_ACCOUNTING_PAIR_VERSION,
    matched,
    matchedDigest: digestRecord(matched),
    metadataKind: "performance-attempt-accounting-pair-nonmeasurement",
    pendingGates: PENDING_GATES,
    provenance: {
      evaluationMode: "offline-accounting-only",
      measurementEvidence: false,
      syntheticEvidenceEligible: false,
      trafficGenerated: false,
    },
    schedulePair,
  });
}
