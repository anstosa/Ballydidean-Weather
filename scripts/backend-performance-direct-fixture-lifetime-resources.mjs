import { Buffer } from "node:buffer";

import { buildBackendPerformanceFunctionalSchedule } from
  "./backend-performance-functional-schedule.mjs";

export const BACKEND_PERFORMANCE_DIRECT_FIXTURE_LIFETIME_RESOURCES_VERSION =
  "weather-backend-performance-direct-fixture-lifetime-resources/v1";

const FIXTURE_SCOPE =
  "fresh fixture container lifetime counters, not full application aggregate";
const CONTAINER_ID_PATTERN = /^[0-9a-f]{64}$/u;
const DECIMAL_PATTERN = /^(?:0|[1-9][0-9]*)$/u;
const CPU_MAX_PATTERN = /^(0|[1-9][0-9]*) (0|[1-9][0-9]*)$/u;
const CAPTURED_AT_PATTERN =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/u;
const MAX_RAW_COUNTER_BYTES = 4_096;
const MAX_RAW_CONTEXT_BYTES = 16_384;
const MAX_PIDS_CURRENT = 1_048_576;
const CPU_ACCOUNTING_ROUNDING_TOLERANCE_USEC = 2;
const SNAPSHOT_KEYS = Object.freeze([
  "capturedAt",
  "containers",
  "hostMeminfo",
  "hostProcStat",
  "scope",
]);
const CONTAINER_KEYS = Object.freeze(["cgroup", "containerId", "counters", "pid"]);
const COUNTER_KEYS = Object.freeze([
  "cpu.max",
  "cpu.stat",
  "memory.current",
  "memory.events",
  "memory.max",
  "memory.peak",
  "pids.current",
]);
const CPU_STAT_KEYS = Object.freeze([
  "usage_usec",
  "user_usec",
  "system_usec",
  "nice_usec",
  "nr_periods",
  "nr_throttled",
  "throttled_usec",
  "nr_bursts",
  "burst_usec",
]);
const MEMORY_EVENT_KEYS = Object.freeze([
  "low",
  "high",
  "max",
  "oom",
  "oom_kill",
  "oom_group_kill",
]);
const HOST_MEMINFO_NAMES = Object.freeze([
  "MemTotal",
  "MemFree",
  "MemAvailable",
  "Buffers",
]);
const PROFILE_SERVICES = Object.freeze({
  "api-only": Object.freeze(["api", "postgres"]),
  "api-web": Object.freeze(["api", "postgres", "web"]),
});
const SERVICE_CONTAINER_KEYS = Object.freeze({
  api: "apiContainerId",
  postgres: "postgresContainerId",
  web: "webContainerId",
});
const SERVICE_LIMITS = Object.freeze({
  api: Object.freeze({
    cpuQuotaUsec: 50_000,
    cpuPeriodUsec: 100_000,
    memoryMaxBytes: 268_435_456,
  }),
  postgres: Object.freeze({
    cpuQuotaUsec: 100_000,
    cpuPeriodUsec: 100_000,
    memoryMaxBytes: 536_870_912,
  }),
  web: Object.freeze({
    cpuQuotaUsec: 25_000,
    cpuPeriodUsec: 100_000,
    memoryMaxBytes: 402_653_184,
  }),
});
const AUTHORITY_LIMITATIONS = Object.freeze([
  "fresh fixture containers only; workers and other host or application processes are excluded",
  "ready and end are two lifecycle samples, not a steady-state memory series",
  "per-service memory peaks are independent lifetime counters and are not summed",
  "raw host context is preserved but is not reduced to a host resource interval",
]);

// create one coded resource failure
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one resource invariant
function requireInvariant(condition, code, message) {
  // fail closed on malformed evidence
  if (!condition) {
    fail(code, message);
  }
}

// identify one plain record
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
  requireInvariant(isRecord(value), "fixture-resource.schema", `${path} must be a record`);
  const actual = Object.keys(value).sort(compareText);
  const expected = [...expectedKeys].sort(compareText);
  requireInvariant(
    JSON.stringify(actual) === JSON.stringify(expected),
    "fixture-resource.schema",
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

// build the exact attempt index once
function buildAttemptIndex() {
  const schedule = buildBackendPerformanceFunctionalSchedule();
  const attemptById = new Map();
  // retain every authoritative source attempt
  for (const runSlot of schedule.runSlots) {
    // retain every fixed schedule group
    for (const group of runSlot.groups) {
      // index every exact attempt identity
      for (const attempt of group.attempts) {
        requireInvariant(
          !attemptById.has(attempt.attemptId),
          "fixture-resource.schedule",
          `authoritative schedule repeats ${attempt.attemptId}`,
        );
        attemptById.set(attempt.attemptId, attempt);
      }
    }
  }
  return attemptById;
}

const ATTEMPT_BY_ID = buildAttemptIndex();

// derive the fixture profile for one supported operation
function expectedProfile(operationId) {
  // bind api operations to the api-only fixture
  if (operationId.startsWith("api.")) {
    return "api-only";
  }
  // bind browser and edge operations to the api-web fixture
  if (operationId.startsWith("page.") || operationId.startsWith("edge.")) {
    return "api-web";
  }
  return null;
}

// validate one bounded raw string
function validateRawText(value, maximumBytes, path) {
  requireInvariant(
    typeof value === "string"
      && value.length > 0
      && Buffer.byteLength(value, "utf8") <= maximumBytes,
    "fixture-resource.schema",
    `${path} must be bounded nonempty text`,
  );
  return value;
}

// parse one nonnegative safe decimal
function parseDecimal(value, path) {
  validateRawText(value, MAX_RAW_COUNTER_BYTES, path);
  requireInvariant(
    DECIMAL_PATTERN.test(value),
    "fixture-resource.counter",
    `${path} must be an unsigned decimal`,
  );
  const parsed = Number(value);
  requireInvariant(
    Number.isSafeInteger(parsed) && parsed >= 0,
    "fixture-resource.counter",
    `${path} exceeds the safe integer range`,
  );
  return parsed;
}

// parse one closed named counter file
function parseNamedCounters(value, expectedKeys, path) {
  validateRawText(value, MAX_RAW_COUNTER_BYTES, path);
  const parsed = {};
  const lines = value.split("\n");
  requireInvariant(
    lines.length === expectedKeys.length,
    "fixture-resource.counter",
    `${path} has an unexpected counter count`,
  );
  // parse every exact counter row
  for (let index = 0; index < lines.length; index += 1) {
    const match = /^([a-z_]+) (0|[1-9][0-9]*)$/u.exec(lines[index]);
    requireInvariant(
      match !== null && match[1] === expectedKeys[index],
      "fixture-resource.counter",
      `${path} has an unexpected counter at row ${String(index)}`,
    );
    parsed[match[1]] = parseDecimal(match[2], `${path}.${match[1]}`);
  }
  return parsed;
}

// parse one fixed cpu ceiling
function parseCpuMax(value, path) {
  validateRawText(value, MAX_RAW_COUNTER_BYTES, path);
  const match = CPU_MAX_PATTERN.exec(value);
  requireInvariant(
    match !== null,
    "fixture-resource.counter",
    `${path} must contain a numeric quota and period`,
  );
  const quotaUsec = parseDecimal(match[1], `${path}.quotaUsec`);
  const periodUsec = parseDecimal(match[2], `${path}.periodUsec`);
  requireInvariant(
    quotaUsec > 0 && periodUsec > 0,
    "fixture-resource.counter",
    `${path} quota and period must be positive`,
  );
  return { periodUsec, quotaUsec };
}

// validate one cpu accounting relationship
function validateCpuAccounting(cpu, path) {
  const accountedUsage = cpu.user_usec + cpu.system_usec;
  requireInvariant(
    Number.isSafeInteger(accountedUsage)
      && Math.abs(cpu.usage_usec - accountedUsage)
        <= CPU_ACCOUNTING_ROUNDING_TOLERANCE_USEC
      && cpu.nice_usec <= cpu.user_usec
      && cpu.nr_throttled <= cpu.nr_periods,
    "fixture-resource.counter-consistency",
    `${path} has inconsistent cpu accounting`,
  );
}

// validate one captured timestamp
function validateCapturedAt(value, path) {
  validateRawText(value, 128, path);
  requireInvariant(
    CAPTURED_AT_PATTERN.test(value) && Number.isFinite(Date.parse(value)),
    "fixture-resource.schema",
    `${path} must be an RFC 3339 timestamp`,
  );
  return value;
}

// validate one opaque host context snapshot
function validateHostContext(snapshot, path) {
  const hostProcStat = validateRawText(
    snapshot.hostProcStat,
    MAX_RAW_CONTEXT_BYTES,
    `${path}.hostProcStat`,
  );
  requireInvariant(
    /^cpu(?: +[0-9]+){10}$/u.test(hostProcStat),
    "fixture-resource.host-context",
    `${path}.hostProcStat has an unexpected aggregate cpu row`,
  );
  requireInvariant(
    Array.isArray(snapshot.hostMeminfo)
      && snapshot.hostMeminfo.length === HOST_MEMINFO_NAMES.length,
    "fixture-resource.host-context",
    `${path}.hostMeminfo must contain the four captured rows`,
  );
  const hostMeminfo = snapshot.hostMeminfo.map(
    // validate and preserve one raw meminfo row
    (line, index) => {
      validateRawText(line, MAX_RAW_CONTEXT_BYTES, `${path}.hostMeminfo[${String(index)}]`);
      requireInvariant(
        new RegExp(`^${HOST_MEMINFO_NAMES[index]}: +[0-9]+ kB$`, "u").test(line),
        "fixture-resource.host-context",
        `${path}.hostMeminfo[${String(index)}] has an unexpected row`,
      );
      return line;
    },
  );
  return { hostMeminfo, hostProcStat };
}

// validate one service container sample
function validateContainer(value, service, path) {
  requireExactKeys(value, CONTAINER_KEYS, path);
  requireInvariant(
    typeof value.containerId === "string" && CONTAINER_ID_PATTERN.test(value.containerId),
    "fixture-resource.identity",
    `${path}.containerId is invalid`,
  );
  requireInvariant(
    Number.isSafeInteger(value.pid) && value.pid > 0,
    "fixture-resource.identity",
    `${path}.pid is invalid`,
  );
  const cgroup = validateRawText(value.cgroup, 4_096, `${path}.cgroup`);
  requireInvariant(
    cgroup.endsWith(`/docker-${value.containerId}.scope`),
    "fixture-resource.identity",
    `${path}.cgroup does not bind the container identity`,
  );
  requireExactKeys(value.counters, COUNTER_KEYS, `${path}.counters`);
  const cpuMax = parseCpuMax(value.counters["cpu.max"], `${path}.counters.cpu.max`);
  const cpuStat = parseNamedCounters(
    value.counters["cpu.stat"],
    CPU_STAT_KEYS,
    `${path}.counters.cpu.stat`,
  );
  validateCpuAccounting(cpuStat, `${path}.counters.cpu.stat`);
  const memoryCurrentBytes = parseDecimal(
    value.counters["memory.current"],
    `${path}.counters.memory.current`,
  );
  const memoryEvents = parseNamedCounters(
    value.counters["memory.events"],
    MEMORY_EVENT_KEYS,
    `${path}.counters.memory.events`,
  );
  const memoryMaxBytes = parseDecimal(
    value.counters["memory.max"],
    `${path}.counters.memory.max`,
  );
  const memoryPeakBytes = parseDecimal(
    value.counters["memory.peak"],
    `${path}.counters.memory.peak`,
  );
  const pidsCurrent = parseDecimal(
    value.counters["pids.current"],
    `${path}.counters.pids.current`,
  );
  const expectedLimits = SERVICE_LIMITS[service];
  requireInvariant(
    cpuMax.quotaUsec === expectedLimits.cpuQuotaUsec
      && cpuMax.periodUsec === expectedLimits.cpuPeriodUsec
      && memoryMaxBytes === expectedLimits.memoryMaxBytes,
    "fixture-resource.limit",
    `${path} does not match the ${service} fixture ceilings`,
  );
  requireInvariant(
    memoryCurrentBytes <= memoryMaxBytes
      && memoryPeakBytes >= memoryCurrentBytes
      && memoryPeakBytes <= memoryMaxBytes
      && pidsCurrent <= MAX_PIDS_CURRENT,
    "fixture-resource.counter-consistency",
    `${path} has counters outside its fixture ceilings`,
  );
  return {
    identity: {
      cgroup,
      containerId: value.containerId,
      pid: value.pid,
    },
    limits: {
      cpuMax,
      memoryMaxBytes,
    },
    sample: {
      cpuStat,
      memoryCurrentBytes,
      memoryEvents,
      memoryPeakBytes,
      pidsCurrent,
    },
  };
}

// validate one complete fixture snapshot
function validateSnapshot(value, services, path) {
  requireExactKeys(value, SNAPSHOT_KEYS, path);
  requireInvariant(
    value.scope === FIXTURE_SCOPE,
    "fixture-resource.scope",
    `${path}.scope does not describe the fresh fixture boundary`,
  );
  const expectedContainerKeys = services.map(
    // map one service to its captured container key
    (service) => SERVICE_CONTAINER_KEYS[service],
  );
  requireExactKeys(value.containers, expectedContainerKeys, `${path}.containers`);
  const containers = {};
  // validate every required fixture service
  for (const service of services) {
    containers[service] = validateContainer(
      value.containers[SERVICE_CONTAINER_KEYS[service]],
      service,
      `${path}.containers.${SERVICE_CONTAINER_KEYS[service]}`,
    );
  }
  return {
    capturedAt: validateCapturedAt(value.capturedAt, `${path}.capturedAt`),
    containers,
    hostContext: validateHostContext(value, path),
  };
}

// subtract one monotonic counter record
function counterDelta(ready, end, keys, path) {
  const delta = {};
  // validate and subtract every monotonic counter
  for (const key of keys) {
    requireInvariant(
      end[key] >= ready[key],
      "fixture-resource.counter-monotonicity",
      `${path}.${key} decreased between ready and end`,
    );
    delta[key] = end[key] - ready[key];
  }
  return delta;
}

// bind two samples to one unchanged container
function reduceService(service, ready, end) {
  requireInvariant(
    ready.identity.containerId === end.identity.containerId
      && ready.identity.pid === end.identity.pid
      && ready.identity.cgroup === end.identity.cgroup,
    "fixture-resource.identity",
    `${service} container identity changed between ready and end`,
  );
  requireInvariant(
    ready.limits.cpuMax.quotaUsec === end.limits.cpuMax.quotaUsec
      && ready.limits.cpuMax.periodUsec === end.limits.cpuMax.periodUsec
      && ready.limits.memoryMaxBytes === end.limits.memoryMaxBytes,
    "fixture-resource.limit",
    `${service} fixture ceilings changed between ready and end`,
  );
  requireInvariant(
    end.sample.memoryPeakBytes >= ready.sample.memoryPeakBytes,
    "fixture-resource.counter-monotonicity",
    `${service} memory.peak decreased between ready and end`,
  );
  const cpuStatDelta = counterDelta(
    ready.sample.cpuStat,
    end.sample.cpuStat,
    CPU_STAT_KEYS,
    `${service}.cpuStat`,
  );
  const memoryEventsDelta = counterDelta(
    ready.sample.memoryEvents,
    end.sample.memoryEvents,
    MEMORY_EVENT_KEYS,
    `${service}.memoryEvents`,
  );
  return {
    betweenSamples: {
      cpuStatDelta,
      memoryEventsDelta,
    },
    identity: ready.identity,
    limits: ready.limits,
    observations: {
      endMemoryCurrentBytes: end.sample.memoryCurrentBytes,
      lifetimeCpuUsageUsecAtEnd: end.sample.cpuStat.usage_usec,
      lifetimeMemoryPeakBytesAtEnd: end.sample.memoryPeakBytes,
      readyMemoryCurrentBytes: ready.sample.memoryCurrentBytes,
    },
    samples: {
      end: end.sample,
      ready: ready.sample,
    },
    service,
  };
}

// reduce one exact pair of isolated fixture lifecycle snapshots
export function reduceBackendPerformanceDirectFixtureLifetimeResources(value) {
  requireExactKeys(
    value,
    ["attemptId", "profile", "readyObservation", "terminalObservation"],
    "$",
  );
  requireInvariant(
    typeof value.attemptId === "string" && value.attemptId.length > 0,
    "fixture-resource.schema",
    "$.attemptId is invalid",
  );
  const attempt = ATTEMPT_BY_ID.get(value.attemptId);
  requireInvariant(
    attempt !== undefined && attempt.stratum === "db-os-cold",
    "fixture-resource.attempt-binding",
    "$.attemptId must identify an authoritative db-os-cold attempt",
  );
  const boundProfile = expectedProfile(attempt.operationId);
  requireInvariant(
    boundProfile !== null && value.profile === boundProfile,
    "fixture-resource.profile-binding",
    "$.profile does not match the source operation fixture",
  );
  const services = PROFILE_SERVICES[boundProfile];
  const ready = validateSnapshot(value.readyObservation, services, "$.readyObservation");
  const end = validateSnapshot(value.terminalObservation, services, "$.terminalObservation");
  requireInvariant(
    Date.parse(end.capturedAt) >= Date.parse(ready.capturedAt),
    "fixture-resource.sample-order",
    "terminal observation predates the ready observation",
  );
  const serviceObservations = services.map(
    // reduce one service without constructing an aggregate
    (service) => reduceService(service, ready.containers[service], end.containers[service]),
  );
  return deepFreeze({
    authority: {
      kind: "partial-fresh-fixture-lifetime-observation",
      limitations: [...AUTHORITY_LIMITATIONS],
      nativeEligible: false,
      officialTarget: null,
    },
    capturedAt: {
      end: end.capturedAt,
      ready: ready.capturedAt,
    },
    contractVersion: BACKEND_PERFORMANCE_DIRECT_FIXTURE_LIFETIME_RESOURCES_VERSION,
    hostContext: {
      end: end.hostContext,
      limitation: "opaque snapshots only; not a host resource interval or full application aggregate",
      ready: ready.hostContext,
    },
    scope: FIXTURE_SCOPE,
    services: serviceObservations,
    source: {
      attemptId: attempt.attemptId,
      operationId: attempt.operationId,
      profile: boundProfile,
      runSlotId: attempt.runSlotId,
      stratum: attempt.stratum,
    },
  });
}
