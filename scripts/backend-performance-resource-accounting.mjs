import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const ATTEMPT_ACCOUNTING_SOURCE_URL =
  new URL("./backend-performance-attempt-accounting.mjs", import.meta.url);
const RESOURCE_INTERVAL_SOURCE_URL =
  new URL("./backend-performance-resources.mjs", import.meta.url);
const SOURCE_PINS = Object.freeze({
  attemptAccounting: Object.freeze({
    bytes: 27_791,
    sha256: "2beb9efa8926f91040d01eda005967dd699f172c4fba614aba56298ce8fff721",
  }),
  resourceInterval: Object.freeze({
    bytes: 42_177,
    sha256: "06137736a87915df16452f1c861d17b97e6951ad9539cbc8022e6d9c12199dde",
  }),
});
const RESOURCE_INTERVAL_CONTRACT_VERSION = "weather-backend-resource-interval/v1";
const DIRECT_ACCOUNTING_SCOPES = new Set(["postgresql", "secondary-runner", "service"]);
const DIGEST_PATTERN = /^[a-f0-9]{64}$/u;
const MAXIMUM_IDENTITY_BYTES = 256;
const MAXIMUM_MEMORY_SAMPLES = 4_096;
const FLOATING_POINT_ROUNDING_UNITS = 8;
const UNSUPPORTED_AGGREGATE_SCOPES = Object.freeze(["app-aggregate", "host"]);
const ATTRIBUTION_KEYS = Object.freeze([
  "accountingDigest",
  "accountingScope",
  "generation",
  "intervalClockId",
  "releaseId",
  "role",
  "scopeId",
  "sourceReceiptDigest",
]);
const WINDOW_KEYS = Object.freeze([
  "clockId",
  "completedMonotonicMs",
  "groupId",
  "runSlotId",
  "startedMonotonicMs",
]);
const CPU_COUNTER_NAMES = Object.freeze([
  "nr_periods",
  "nr_throttled",
  "system_usec",
  "throttled_usec",
  "usage_usec",
  "user_usec",
]);
const CPU_RESULT_FIELDS = Object.freeze({
  nr_periods: "nrPeriods",
  nr_throttled: "nrThrottled",
  system_usec: "systemUsec",
  throttled_usec: "throttledUsec",
  usage_usec: "usageUsec",
  user_usec: "userUsec",
});
const REQUIRED_METHODS = Object.freeze({
  cpu: "cgroup-v2-cpu.stat-counter-delta",
  peak: "linux-cgroup-v2-memory.peak-private-retained-rw-fd-reset",
  process: "procfs-instantaneous-samples-not-peaks",
  steady: "caller-triggered-bounded-snapshots",
});
const PENDING_GATES = Object.freeze({
  appAggregate: "unsupported-pending",
  clockAndWindowAttestation: "pending",
  host: "unsupported-pending",
  productionSource: "pending",
  secondaryRunnerParity: "pending-if-required",
  state: "pending",
  witness: "pending",
});

export const BACKEND_PERFORMANCE_RESOURCE_ACCOUNTING_VERSION =
  "weather-backend-performance-resource-accounting/v1";

// construct one coded evaluation failure
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one accounting invariant
function requireInvariant(condition, code, message) {
  // reject malformed evidence
  if (!condition) {
    fail(code, message);
  }
}

// hash one exact byte sequence
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// freeze one complete output value
function deepFreeze(value) {
  // freeze mutable nested values first
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    // visit every owned child
    for (const child of Object.values(value)) {
      deepFreeze(child);
    }
    Object.freeze(value);
  }
  return value;
}

// require one plain data record
function requirePlainRecord(value, code, message) {
  const valid = value !== null && typeof value === "object" && !Array.isArray(value)
    && (Object.getPrototypeOf(value) === Object.prototype
      || Object.getPrototypeOf(value) === null);
  requireInvariant(valid, code, message);
  return value;
}

// require one exact key set
function requireExactKeys(value, expectedKeys, code, message) {
  const actual = Object.keys(value).sort();
  const expected = [...expectedKeys].sort();
  requireInvariant(
    actual.length === expected.length
      && actual.every(
        // compare one key position
        (key, index) => key === expected[index],
      ),
    code,
    message,
  );
}

// require one dense bounded array
function requireDenseArray(value, minimum, maximum, code, message) {
  requireInvariant(
    Array.isArray(value)
      && value.length >= minimum
      && value.length <= maximum
      && Object.keys(value).length === value.length,
    code,
    message,
  );
  return value;
}

// require one bounded identity string
function requireIdentity(value, label) {
  requireInvariant(
    typeof value === "string"
      && value.trim().length > 0
      && Buffer.byteLength(value, "utf8") <= MAXIMUM_IDENTITY_BYTES,
    "resource-accounting.identity",
    `${label} must be one bounded nonempty string`,
  );
  return value;
}

// require one lowercase digest
function requireDigest(value, label) {
  requireInvariant(
    typeof value === "string" && DIGEST_PATTERN.test(value),
    "resource-accounting.identity",
    `${label} must be one lowercase sha256 digest`,
  );
  return value;
}

// require one bounded counter
function requireCounter(value, label) {
  requireInvariant(
    Number.isSafeInteger(value) && value >= 0,
    "resource-accounting.counter",
    `${label} must be one nonnegative safe integer`,
  );
  return value;
}

// require one finite monotonic value
function requireMonotonic(value, label) {
  requireInvariant(
    typeof value === "number" && Number.isFinite(value),
    "resource-accounting.clock",
    `${label} must be one finite monotonic value`,
  );
  return value;
}

// require one canonical utc timestamp
function requireIsoDate(value, label) {
  const milliseconds = typeof value === "string" ? Date.parse(value) : Number.NaN;
  requireInvariant(
    Number.isFinite(milliseconds) && new Date(milliseconds).toISOString() === value,
    "resource-accounting.date",
    `${label} must be one canonical utc timestamp`,
  );
  return milliseconds;
}

// compare one fixed ordered text list
function sameTextList(actual, expected) {
  return Array.isArray(actual)
    && actual.length === expected.length
    && actual.every(
      // compare one list position
      (value, index) => value === expected[index],
    );
}

// verify one source identity before use
function verifySourcePin(sourceUrl, pin, label) {
  const bytes = readFileSync(sourceUrl);
  requireInvariant(
    bytes.length === pin.bytes && sha256(bytes) === pin.sha256,
    "resource-accounting.source-pin",
    `${label} source identity changed`,
  );
}

verifySourcePin(
  ATTEMPT_ACCOUNTING_SOURCE_URL,
  SOURCE_PINS.attemptAccounting,
  "attempt accounting",
);
verifySourcePin(
  RESOURCE_INTERVAL_SOURCE_URL,
  SOURCE_PINS.resourceInterval,
  "resource interval",
);

const attemptAccountingModule = await import(ATTEMPT_ACCOUNTING_SOURCE_URL.href);
const { evaluateBackendPerformanceAttemptAccounting } = attemptAccountingModule;

requireInvariant(
  typeof evaluateBackendPerformanceAttemptAccounting === "function",
  "resource-accounting.source-contract",
  "attempt accounting evaluator contract changed",
);

export const BACKEND_PERFORMANCE_RESOURCE_ACCOUNTING_INPUT_SCHEMA = deepFreeze({
  arguments: ["rawBaseline", "evidence"],
  attributionKeys: ATTRIBUTION_KEYS,
  endpointSpanTolerance: {
    roundingUnits: FLOATING_POINT_ROUNDING_UNITS,
    scale: "maximum absolute endpoint clock or offset operand, at least one",
    unit: "milliseconds",
  },
  evidenceKeys: ["attribution", "interval", "window"],
  intervalContractVersion: RESOURCE_INTERVAL_CONTRACT_VERSION,
  windowKeys: WINDOW_KEYS,
});

// validate one explicit attribution record
function validateAttribution(value) {
  const attribution = requirePlainRecord(
    value,
    "resource-accounting.arguments",
    "resource attribution must be one plain record",
  );
  requireExactKeys(
    attribution,
    ATTRIBUTION_KEYS,
    "resource-accounting.arguments",
    "resource attribution keys are invalid",
  );
  requireDigest(attribution.accountingDigest, "accountingDigest");
  requireDigest(attribution.sourceReceiptDigest, "sourceReceiptDigest");
  requireIdentity(attribution.releaseId, "releaseId");
  requireIdentity(attribution.scopeId, "scopeId");
  requireIdentity(attribution.intervalClockId, "intervalClockId");
  requireInvariant(
    attribution.role === "baseline" && attribution.generation === "node-v1",
    "resource-accounting.role",
    "resource attribution must declare the exact baseline node role",
  );
  requireInvariant(
    DIRECT_ACCOUNTING_SCOPES.has(attribution.accountingScope),
    "resource-accounting.scope",
    "resource attribution must declare one direct process cgroup scope",
  );
  return attribution;
}

// validate one declared group window
function validateWindow(value, intervalClockId) {
  const window = requirePlainRecord(
    value,
    "resource-accounting.arguments",
    "resource group window must be one plain record",
  );
  requireExactKeys(
    window,
    WINDOW_KEYS,
    "resource-accounting.arguments",
    "resource group window keys are invalid",
  );
  requireIdentity(window.runSlotId, "runSlotId");
  requireIdentity(window.groupId, "groupId");
  requireIdentity(window.clockId, "clockId");
  const started = requireMonotonic(window.startedMonotonicMs, "startedMonotonicMs");
  const completed = requireMonotonic(window.completedMonotonicMs, "completedMonotonicMs");
  requireInvariant(
    window.clockId === intervalClockId,
    "resource-accounting.clock",
    "group and resource interval clock declarations do not match",
  );
  requireInvariant(
    completed >= started,
    "resource-accounting.clock",
    "group window completion precedes its start",
  );
  return window;
}

// validate one raw cpu counter record
function validateRawCpu(value, label) {
  const cpu = requirePlainRecord(
    value,
    "resource-accounting.cpu",
    `${label} cpu observation must be one plain record`,
  );
  requireExactKeys(
    cpu,
    CPU_COUNTER_NAMES,
    "resource-accounting.cpu",
    `${label} cpu counter keys are invalid`,
  );
  // validate every contracted counter
  for (const name of CPU_COUNTER_NAMES) {
    requireCounter(cpu[name], `${label}.cpu.${name}`);
  }
  return cpu;
}

// validate one raw dynamic observation
function validateRawObservation(value, label) {
  const observation = requirePlainRecord(
    value,
    "resource-accounting.interval",
    `${label} raw observation must be one plain record`,
  );
  validateRawCpu(observation.cpu, label);
  requireCounter(observation.memoryCurrentBytes, `${label}.memoryCurrentBytes`);
  requireMonotonic(observation.monotonicMs, `${label}.monotonicMs`);
  requireIsoDate(observation.observedAt, `${label}.observedAt`);
  return observation;
}

// compute one safe raw cpu delta
function cpuDelta(before, after) {
  const delta = {};
  // subtract every pinned cpu counter
  for (const name of CPU_COUNTER_NAMES) {
    requireInvariant(
      after[name] >= before[name],
      "resource-accounting.cpu",
      `raw cpu counter decreased for ${name}`,
    );
    const difference = after[name] - before[name];
    requireInvariant(
      Number.isSafeInteger(difference),
      "resource-accounting.cpu",
      `raw cpu delta is unsafe for ${name}`,
    );
    delta[name] = difference;
  }
  return delta;
}

// validate the reported cpu projection
function validateCpu(value, rawDelta) {
  const cpu = requirePlainRecord(
    value,
    "resource-accounting.cpu",
    "resource cpu result must be one plain record",
  );
  requireExactKeys(
    cpu,
    ["nrPeriods", "nrThrottled", "source", "systemUsec", "throttledUsec", "usageUsec", "userUsec"],
    "resource-accounting.cpu",
    "resource cpu result keys are invalid",
  );
  requireInvariant(
    cpu.source === "cgroup-cpu-stat-delta",
    "resource-accounting.cpu",
    "resource cpu source is invalid",
  );
  // bind every reported total to its raw counter delta
  for (const name of CPU_COUNTER_NAMES) {
    const resultField = CPU_RESULT_FIELDS[name];
    requireCounter(cpu[resultField], `cpu.${resultField}`);
    requireInvariant(
      cpu[resultField] === rawDelta[name],
      "resource-accounting.cpu",
      `resource cpu result does not match raw delta for ${name}`,
    );
  }
  return cpu;
}

// bound subtraction rounding across endpoint clocks
function endpointSpanTolerance(...values) {
  let scale = 1;
  // retain the largest endpoint magnitude
  for (const value of values) {
    scale = Math.max(scale, Math.abs(value));
  }
  return FLOATING_POINT_ROUNDING_UNITS * Number.EPSILON * scale;
}

// validate one bounded steady memory series
function validateSteadyMemory(value, sampling, rawBefore, rawAfter) {
  const steady = requirePlainRecord(
    value,
    "resource-accounting.memory",
    "steady memory result must be one plain record",
  );
  requireExactKeys(
    steady,
    ["arithmeticMeanBytes", "method", "sampledMaximumBytes", "samples", "source"],
    "resource-accounting.memory",
    "steady memory result keys are invalid",
  );
  requireInvariant(
    steady.method === "bounded-caller-scheduled-memory.current-samples"
      && steady.source === "cgroup-memory-current-window",
    "resource-accounting.memory",
    "steady memory method or source is invalid",
  );
  const samples = requireDenseArray(
    steady.samples,
    2,
    MAXIMUM_MEMORY_SAMPLES,
    "resource-accounting.memory",
    "steady memory samples are not one bounded dense array",
  );
  requireInvariant(
    samples.length === sampling.sampleCount,
    "resource-accounting.memory",
    "steady memory sample count does not match sampling metadata",
  );
  let arithmeticMeanBytes = 0;
  let sampledMaximumBytes = 0;
  let previousDate = Number.NEGATIVE_INFINITY;
  let previousOffset = Number.NEGATIVE_INFINITY;
  // validate and aggregate every raw current sample
  for (let index = 0; index < samples.length; index += 1) {
    const sample = requirePlainRecord(
      samples[index],
      "resource-accounting.memory",
      `steady memory sample ${String(index)} must be one plain record`,
    );
    requireExactKeys(
      sample,
      ["memoryCurrentBytes", "observedAt", "offsetMs"],
      "resource-accounting.memory",
      `steady memory sample ${String(index)} keys are invalid`,
    );
    const memoryCurrentBytes = requireCounter(
      sample.memoryCurrentBytes,
      `steady memory sample ${String(index)} memoryCurrentBytes`,
    );
    const observedAt = requireIsoDate(
      sample.observedAt,
      `steady memory sample ${String(index)} observedAt`,
    );
    const offsetMs = requireMonotonic(
      sample.offsetMs,
      `steady memory sample ${String(index)} offsetMs`,
    );
    requireInvariant(
      offsetMs >= 0 && offsetMs >= previousOffset && observedAt >= previousDate,
      "resource-accounting.memory",
      `steady memory sample ${String(index)} is not chronological`,
    );
    arithmeticMeanBytes += (memoryCurrentBytes - arithmeticMeanBytes) / (index + 1);
    sampledMaximumBytes = Math.max(sampledMaximumBytes, memoryCurrentBytes);
    previousDate = observedAt;
    previousOffset = offsetMs;
  }
  const first = samples[0];
  const last = samples.at(-1);
  requireInvariant(
    first.memoryCurrentBytes === rawBefore.memoryCurrentBytes
      && first.observedAt === rawBefore.observedAt
      && last.memoryCurrentBytes === rawAfter.memoryCurrentBytes
      && last.observedAt === rawAfter.observedAt,
    "resource-accounting.memory",
    "steady memory endpoints do not match raw interval observations",
  );
  const endpointOffsetSpanMs = last.offsetMs - first.offsetMs;
  const rawMonotonicSpanMs = rawAfter.monotonicMs - rawBefore.monotonicMs;
  // allow only scaled double-rounding from the two endpoint subtractions
  const endpointSpanToleranceMs = endpointSpanTolerance(
    first.offsetMs,
    last.offsetMs,
    rawBefore.monotonicMs,
    rawAfter.monotonicMs,
  );
  requireInvariant(
    Math.abs(endpointOffsetSpanMs - rawMonotonicSpanMs) <= endpointSpanToleranceMs,
    "resource-accounting.clock",
    "steady memory endpoint offset span does not match the raw monotonic interval",
  );
  requireInvariant(
    Number.isFinite(steady.arithmeticMeanBytes)
      && steady.arithmeticMeanBytes === arithmeticMeanBytes
      && steady.sampledMaximumBytes === sampledMaximumBytes,
    "resource-accounting.memory",
    "steady memory summaries do not match raw samples",
  );
  return {
    arithmeticMeanBytes,
    endpointSpan: {
      offsetSpanMs: endpointOffsetSpanMs,
      rawMonotonicSpanMs,
      toleranceMs: endpointSpanToleranceMs,
    },
    sampledMaximumBytes,
    sampleCount: samples.length,
    samples,
  };
}

// validate one native interval peak
function validatePeak(value, steady, startedAt, completedAt) {
  const peak = requirePlainRecord(
    value,
    "resource-accounting.peak",
    "resource peak must be one plain record",
  );
  requireExactKeys(
    peak,
    ["fdIdentitySha256", "initializedBytes", "readAt", "resetAt", "scope", "source", "valueBytes"],
    "resource-accounting.peak",
    "resource peak keys are invalid",
  );
  requireDigest(peak.fdIdentitySha256, "memory peak descriptor identity");
  requireCounter(peak.initializedBytes, "memory peak initializedBytes");
  requireCounter(peak.valueBytes, "memory peak valueBytes");
  requireIsoDate(peak.resetAt, "memory peak resetAt");
  requireIsoDate(peak.readAt, "memory peak readAt");
  requireInvariant(
    peak.scope === "private-retained-rw-fd"
      && peak.source === "cgroup-memory-peak-reset",
    "resource-accounting.peak",
    "resource peak method is invalid",
  );
  requireInvariant(
    peak.resetAt === startedAt && peak.readAt === completedAt,
    "resource-accounting.peak",
    "resource peak dates do not match the interval",
  );
  requireInvariant(
    peak.valueBytes >= peak.initializedBytes
      && peak.valueBytes >= steady.sampledMaximumBytes
      && steady.samples.every(
        // bound one current sample by the native peak
        (sample) => peak.valueBytes >= sample.memoryCurrentBytes,
      ),
    "resource-accounting.peak",
    "native interval peak is below an initialized or sampled current value",
  );
  return peak;
}

// validate one pinned resource interval
function validateResourceInterval(value, attribution, window) {
  const interval = requirePlainRecord(
    value,
    "resource-accounting.interval",
    "resource interval must be one plain record",
  );
  requireInvariant(
    interval.contractVersion === RESOURCE_INTERVAL_CONTRACT_VERSION,
    "resource-accounting.interval",
    "resource interval contract version is invalid",
  );
  requireInvariant(
    interval.accountingScope === attribution.accountingScope
      && interval.scopeId === attribution.scopeId,
    "resource-accounting.scope",
    "resource interval scope does not match its attribution",
  );
  const sourceBinding = requirePlainRecord(
    interval.sourceBinding,
    "resource-accounting.identity",
    "resource source binding must be one plain record",
  );
  requireInvariant(
    sourceBinding.kind === "integration-supplied-live-receipt-binding"
      && requireDigest(sourceBinding.sha256, "resource source binding")
        === attribution.sourceReceiptDigest,
    "resource-accounting.identity",
    "resource source receipt does not match its attribution",
  );
  const accounting = requirePlainRecord(
    interval.accounting,
    "resource-accounting.scope",
    "resource accounting descriptor must be one plain record",
  );
  requireInvariant(
    accounting.aggregateMethod === "direct-single-cgroup"
      && accounting.coverage === "measured-process-cgroup-only"
      && accounting.sampledServiceSumUsed === false
      && sameTextList(accounting.unsupportedScopes, UNSUPPORTED_AGGREGATE_SCOPES),
    "resource-accounting.scope",
    "resource interval attempts to claim aggregate accounting",
  );
  const authority = requirePlainRecord(
    interval.authority,
    "resource-accounting.scope",
    "resource authority must be one plain record",
  );
  requireInvariant(
    authority.scopeCoverage === "direct-process-cgroup-only"
      && sameTextList(authority.unsupportedScopes, UNSUPPORTED_AGGREGATE_SCOPES),
    "resource-accounting.scope",
    "resource authority scope coverage is invalid",
  );
  const cgroupIdentity = requirePlainRecord(
    interval.cgroupIdentity,
    "resource-accounting.identity",
    "resource cgroup identity must be one plain record",
  );
  const cgroupId = requireDigest(cgroupIdentity.sha256, "resource cgroup identity");
  const before = requirePlainRecord(
    interval.before,
    "resource-accounting.identity",
    "resource before identity must be one plain record",
  );
  const after = requirePlainRecord(
    interval.after,
    "resource-accounting.identity",
    "resource after identity must be one plain record",
  );
  // validate both consumed endpoint identities
  for (const [label, endpoint] of [["before", before], ["after", after]]) {
    requireInvariant(
      endpoint.cgroupId === cgroupId
        && endpoint.releaseId === attribution.releaseId
        && Number.isSafeInteger(endpoint.pid)
        && endpoint.pid > 0,
      "resource-accounting.identity",
      `resource ${label} cgroup release or process identity is invalid`,
    );
    requireIsoDate(endpoint.observedAt, `resource ${label} observedAt`);
  }
  requireInvariant(
    before.pid === after.pid,
    "resource-accounting.identity",
    "resource endpoint process identity changed",
  );
  const observations = requirePlainRecord(
    interval.observations,
    "resource-accounting.interval",
    "resource observations must be one plain record",
  );
  const rawBefore = validateRawObservation(observations.before, "before");
  const rawAfter = validateRawObservation(observations.after, "after");
  requireInvariant(
    before.observedAt === rawBefore.observedAt
      && after.observedAt === rawAfter.observedAt,
    "resource-accounting.date",
    "resource endpoint dates do not match raw observations",
  );
  requireInvariant(
    rawAfter.monotonicMs >= rawBefore.monotonicMs
      && window.startedMonotonicMs >= rawBefore.monotonicMs
      && window.completedMonotonicMs <= rawAfter.monotonicMs,
    "resource-accounting.clock",
    "declared group window is not bracketed by the resource interval",
  );
  const startedAt = requireIsoDate(interval.startedAt, "resource startedAt");
  const completedAt = requireIsoDate(interval.completedAt, "resource completedAt");
  const beforeAt = requireIsoDate(before.observedAt, "resource before observedAt");
  const afterAt = requireIsoDate(after.observedAt, "resource after observedAt");
  const sampling = requirePlainRecord(
    interval.sampling,
    "resource-accounting.interval",
    "resource sampling metadata must be one plain record",
  );
  const budgetStartedAt = requireIsoDate(sampling.budgetStartedAt, "resource budgetStartedAt");
  requireCounter(sampling.sampleCount, "resource sampling sampleCount");
  requireInvariant(
    budgetStartedAt <= startedAt
      && startedAt <= beforeAt
      && beforeAt <= afterAt
      && afterAt <= completedAt,
    "resource-accounting.date",
    "resource interval dates are not chronological",
  );
  const rawDelta = cpuDelta(rawBefore.cpu, rawAfter.cpu);
  const cpu = validateCpu(interval.cpu, rawDelta);
  const memory = requirePlainRecord(
    interval.memory,
    "resource-accounting.memory",
    "resource memory result must be one plain record",
  );
  const steady = validateSteadyMemory(memory.steady, sampling, rawBefore, rawAfter);
  const peak = validatePeak(memory.peak, steady, interval.startedAt, interval.completedAt);
  const method = requirePlainRecord(
    interval.method,
    "resource-accounting.method",
    "resource method record must be one plain record",
  );
  requireInvariant(
    Object.entries(REQUIRED_METHODS).every(
      // match one pinned method identifier
      ([name, expected]) => method[name] === expected,
    ),
    "resource-accounting.method",
    "resource method identifiers do not match the pinned contract",
  );
  return {
    cgroupId,
    completedAt: interval.completedAt,
    cpu,
    method,
    peak,
    startedAt: interval.startedAt,
    steady,
  };
}

// count one exact plan group
function countGroup(attempts, runSlotId, groupId) {
  const terminalCounts = { cancelled: 0, completed: 0, error: 0, timeout: 0 };
  let maximumObservedNanoseconds = null;
  let observedAttempts = 0;
  let planned = 0;
  // derive every denominator from verified plan attempts
  for (const attempt of attempts) {
    // retain only the declared exact plan group
    if (attempt.runSlotId === runSlotId && attempt.groupId === groupId) {
      planned += 1;
      terminalCounts[attempt.terminalCategory] += 1;
      // track every observed terminal category
      if (attempt.elapsedMonotonic.kind === "observed") {
        observedAttempts += 1;
        maximumObservedNanoseconds = Math.max(
          maximumObservedNanoseconds ?? 0,
          attempt.elapsedMonotonic.nanoseconds,
        );
      }
    }
  }
  requireInvariant(
    Number.isSafeInteger(planned) && planned > 0,
    "resource-accounting.group",
    "declared run slot and group do not identify one planned group",
  );
  const failures = terminalCounts.cancelled + terminalCounts.error + terminalCounts.timeout;
  requireInvariant(
    Number.isSafeInteger(failures)
      && terminalCounts.completed + failures === planned,
    "resource-accounting.group",
    "declared group terminal counts are inconsistent",
  );
  return {
    counts: {
      failures: {
        cancelled: terminalCounts.cancelled,
        error: terminalCounts.error,
        timeout: terminalCounts.timeout,
        total: failures,
      },
      planned,
      terminalCounts,
    },
    elapsedMonotonic: {
      maximumObservedNanoseconds,
      observedAttempts,
    },
  };
}

// evaluate one offline direct resource interval
export function evaluateBackendPerformanceResourceAccounting(
  rawBaseline,
  evidence,
  ...rest
) {
  requireInvariant(
    rest.length === 0,
    "resource-accounting.arguments",
    "resource accounting accepts exactly one raw baseline and one evidence record",
  );
  const input = requirePlainRecord(
    evidence,
    "resource-accounting.arguments",
    "resource accounting evidence must be one plain record",
  );
  requireExactKeys(
    input,
    ["attribution", "interval", "window"],
    "resource-accounting.arguments",
    "resource accounting evidence keys are invalid",
  );
  const attribution = validateAttribution(input.attribution);
  const window = validateWindow(input.window, attribution.intervalClockId);
  const attemptAccounting = evaluateBackendPerformanceAttemptAccounting(rawBaseline);
  requireInvariant(
    attemptAccounting.role === attribution.role
      && attemptAccounting.generation === attribution.generation
      && attemptAccounting.accountingDigest.value === attribution.accountingDigest,
    "resource-accounting.attribution",
    "attempt accounting identity does not match resource attribution",
  );
  const interval = validateResourceInterval(input.interval, attribution, window);
  const groupAccounting = countGroup(
    attemptAccounting.attempts,
    window.runSlotId,
    window.groupId,
  );
  const counts = groupAccounting.counts;
  const windowWidthMs = window.completedMonotonicMs - window.startedMonotonicMs;
  const maximumObservedMilliseconds =
    groupAccounting.elapsedMonotonic.maximumObservedNanoseconds === null
      ? null
      : groupAccounting.elapsedMonotonic.maximumObservedNanoseconds / 1_000_000;
  requireInvariant(
    maximumObservedMilliseconds === null || maximumObservedMilliseconds <= windowWidthMs,
    "resource-accounting.clock",
    "selected group observed elapsed time exceeds its declared group window",
  );
  const allCompleted = counts.planned > 0
    && counts.terminalCounts.completed === counts.planned;
  // omit ratios for any incomplete group
  const cpuUsePerDeclaredCompletedWork = allCompleted
    ? {
      denominatorCompletedWork: counts.terminalCounts.completed,
      numeratorObservedIntervalUsageUsec: interval.cpu.usageUsec,
    }
    : null;
  return deepFreeze({
    attribution: {
      accountingDigest: attribution.accountingDigest,
      accountingScope: attribution.accountingScope,
      generation: attribution.generation,
      intervalClockId: attribution.intervalClockId,
      releaseId: attribution.releaseId,
      role: attribution.role,
      scopeId: attribution.scopeId,
      sourceReceiptDigest: attribution.sourceReceiptDigest,
    },
    canonicalEligible: false,
    contractVersion: BACKEND_PERFORMANCE_RESOURCE_ACCOUNTING_VERSION,
    group: {
      allCompleted,
      counts,
      elapsedMonotonic: {
        maximumObservedMilliseconds,
        maximumObservedNanoseconds:
          groupAccounting.elapsedMonotonic.maximumObservedNanoseconds,
        observedAttempts: groupAccounting.elapsedMonotonic.observedAttempts,
      },
      groupId: window.groupId,
      runSlotId: window.runSlotId,
      window: {
        clockId: window.clockId,
        completedMonotonicMs: window.completedMonotonicMs,
        relationship: "integration-declared-and-resource-bracketed",
        startedMonotonicMs: window.startedMonotonicMs,
        trustedHostAuthority: false,
        widthMs: windowWidthMs,
      },
    },
    metadataKind: "performance-direct-resource-accounting-noncanonical",
    pendingGates: PENDING_GATES,
    provenance: {
      applicationAggregateCreated: false,
      evaluationMode: "offline-direct-resource-accounting",
      hostObserved: false,
      trafficGenerated: false,
      trustedClockAuthority: false,
    },
    resource: {
      cgroupId: interval.cgroupId,
      cpu: {
        nrPeriods: interval.cpu.nrPeriods,
        nrThrottled: interval.cpu.nrThrottled,
        perDeclaredCompletedWork: cpuUsePerDeclaredCompletedWork,
        source: interval.cpu.source,
        systemUsec: interval.cpu.systemUsec,
        throttledUsec: interval.cpu.throttledUsec,
        usageUsec: interval.cpu.usageUsec,
        userUsec: interval.cpu.userUsec,
      },
      intervalObservedAt: {
        completedAt: interval.completedAt,
        startedAt: interval.startedAt,
      },
      memory: {
        nativePeak: {
          fdIdentitySha256: interval.peak.fdIdentitySha256,
          initializedBytes: interval.peak.initializedBytes,
          readAt: interval.peak.readAt,
          resetAt: interval.peak.resetAt,
          scope: interval.peak.scope,
          source: interval.peak.source,
          valueBytes: interval.peak.valueBytes,
        },
        steady: {
          arithmeticMeanBytes: interval.steady.arithmeticMeanBytes,
          endpointSpan: interval.steady.endpointSpan,
          method: "bounded-caller-scheduled-memory.current-samples",
          sampleCount: interval.steady.sampleCount,
          sampledMaximumBytes: interval.steady.sampledMaximumBytes,
          source: "cgroup-memory-current-window",
        },
      },
      methods: {
        cpu: interval.method.cpu,
        peak: interval.method.peak,
        process: interval.method.process,
        steady: interval.method.steady,
      },
    },
    sourcePins: SOURCE_PINS,
  });
}
