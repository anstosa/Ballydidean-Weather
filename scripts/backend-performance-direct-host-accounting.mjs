import { createHash } from "node:crypto";

export const BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_INPUT_VERSION =
  "weather-backend-performance-direct-host-snapshots/v1";
export const BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_VERSION =
  "weather-backend-performance-direct-host-accounting/v1";

const CPU_FIELDS = Object.freeze([
  "user",
  "nice",
  "system",
  "idle",
  "iowait",
  "irq",
  "softirq",
  "steal",
  "guest",
  "guestNice",
]);
const ACCOUNTED_CPU_FIELDS = Object.freeze(CPU_FIELDS.slice(0, 8));
const BUSY_CPU_FIELDS = Object.freeze([
  "user", "nice", "system", "irq", "softirq", "steal",
]);
const MEMINFO_FIELDS = Object.freeze([
  "MemTotal", "MemFree", "MemAvailable", "SwapTotal", "SwapFree",
]);
const MAXIMUM_SNAPSHOTS = 4_096;
const MAXIMUM_PROC_STAT_BYTES = 1024 * 1024;
const MAXIMUM_MEMINFO_BYTES = 64 * 1024;
const KIBIBYTE = 1024n;

// create one coded host-accounting failure
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one host-accounting invariant
function requireInvariant(condition, code, message) {
  // reject malformed or ambiguous offline evidence
  if (!condition) {
    fail(code, message);
  }
}

// identify one plain evidence record
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    && (Object.getPrototypeOf(value) === Object.prototype
      || Object.getPrototypeOf(value) === null);
}

// require one exact record key set
function requireExactKeys(value, expected, path) {
  requireInvariant(
    isRecord(value) && JSON.stringify(Object.keys(value).sort())
      === JSON.stringify([...expected].sort()),
    "host-accounting.schema",
    `${path} keys are invalid`,
  );
}

// parse one nonnegative decimal counter without precision loss
function parseCounter(value, label) {
  requireInvariant(
    typeof value === "string" && /^(?:0|[1-9][0-9]*)$/u.test(value),
    "host-accounting.counter",
    `${label} is not one canonical nonnegative decimal counter`,
  );
  return BigInt(value);
}

// retain one bigint as canonical decimal output
function decimal(value) {
  return value.toString(10);
}

// sum one fixed counter field set
function sumFields(value, fields) {
  return fields.reduce(
    // add one exact counter field
    (total, field) => total + value[field],
    0n,
  );
}

// parse the one aggregate CPU record from bounded proc stat bytes
function parseProcStat(value, path) {
  requireInvariant(
    typeof value === "string" && Buffer.byteLength(value, "utf8") > 0
      && Buffer.byteLength(value, "utf8") <= MAXIMUM_PROC_STAT_BYTES,
    "host-accounting.proc-stat",
    `${path} is empty or exceeds the proc stat byte bound`,
  );
  const aggregateLines = value.split("\n").filter(
    // retain only the aggregate CPU line
    (line) => line.startsWith("cpu "),
  );
  requireInvariant(
    aggregateLines.length === 1,
    "host-accounting.proc-stat",
    `${path} must contain exactly one aggregate cpu line`,
  );
  const columns = aggregateLines[0].trim().split(/\s+/u);
  requireInvariant(
    columns.length === CPU_FIELDS.length + 1 && columns[0] === "cpu",
    "host-accounting.proc-stat",
    `${path} aggregate cpu field count changed`,
  );
  return Object.fromEntries(CPU_FIELDS.map(
    // bind one documented aggregate field by position
    (field, index) => [field, parseCounter(columns[index + 1], `${path}.${field}`)],
  ));
}

// parse exact kilobyte memory fields from bounded meminfo bytes
function parseMeminfo(value, path) {
  requireInvariant(
    typeof value === "string" && Buffer.byteLength(value, "utf8") > 0
      && Buffer.byteLength(value, "utf8") <= MAXIMUM_MEMINFO_BYTES,
    "host-accounting.meminfo",
    `${path} is empty or exceeds the meminfo byte bound`,
  );
  const selected = new Map();

  // inspect every bounded meminfo line
  for (const line of value.split("\n")) {
    const match = /^([A-Za-z][A-Za-z0-9_()]*):\s+([0-9]+)\s+(\S+)$/u.exec(line);

    // ignore unrelated documented meminfo fields
    if (match === null || !MEMINFO_FIELDS.includes(match[1])) {
      continue;
    }
    requireInvariant(
      !selected.has(match[1]),
      "host-accounting.meminfo",
      `${path}.${match[1]} appears more than once`,
    );
    requireInvariant(
      match[3] === "kB",
      "host-accounting.meminfo-unit",
      `${path}.${match[1]} must use kernel kB units`,
    );
    selected.set(match[1], parseCounter(match[2], `${path}.${match[1]}`) * KIBIBYTE);
  }
  requireInvariant(
    MEMINFO_FIELDS.every(
      // require every fixed host-memory input
      (field) => selected.has(field),
    ),
    "host-accounting.meminfo",
    `${path} lacks one required memory or swap field`,
  );
  const total = selected.get("MemTotal");
  const free = selected.get("MemFree");
  const available = selected.get("MemAvailable");
  const swapTotal = selected.get("SwapTotal");
  const swapFree = selected.get("SwapFree");
  requireInvariant(
    total > 0n && free <= total && available <= total && swapFree <= swapTotal,
    "host-accounting.meminfo-range",
    `${path} memory or swap values are inconsistent`,
  );
  return {
    memAvailableBytes: available,
    memFreeBytes: free,
    memTotalBytes: total,
    nonFreeBytes: total - free,
    swapFreeBytes: swapFree,
    swapTotalBytes: swapTotal,
    swapUsedBytes: swapTotal - swapFree,
    unavailableEstimateBytes: total - available,
  };
}

// validate one canonical UTC timestamp
function parseTimestamp(value, path) {
  const milliseconds = typeof value === "string" ? Date.parse(value) : Number.NaN;
  requireInvariant(
    Number.isFinite(milliseconds) && new Date(milliseconds).toISOString() === value,
    "host-accounting.clock",
    `${path} is not one canonical UTC timestamp`,
  );
  return milliseconds;
}

// parse one ordered raw host snapshot
function parseSnapshot(value, index) {
  const path = `$.snapshots[${String(index)}]`;
  requireExactKeys(
    value,
    ["capturedAt", "meminfo", "monotonicMilliseconds", "procStat"],
    path,
  );
  requireInvariant(
    Number.isSafeInteger(value.monotonicMilliseconds)
      && value.monotonicMilliseconds >= 0,
    "host-accounting.clock",
    `${path}.monotonicMilliseconds is invalid`,
  );
  return {
    capturedAt: value.capturedAt,
    capturedAtMilliseconds: parseTimestamp(value.capturedAt, `${path}.capturedAt`),
    memory: parseMeminfo(value.meminfo, `${path}.meminfo`),
    monotonicMilliseconds: value.monotonicMilliseconds,
    procStatSha256: createHash("sha256").update(value.procStat).digest("hex"),
    meminfoSha256: createHash("sha256").update(value.meminfo).digest("hex"),
    cpu: parseProcStat(value.procStat, `${path}.procStat`),
  };
}

// derive one nondecreasing aggregate CPU delta
function cpuDelta(before, after, path) {
  const fields = {};

  // subtract every documented aggregate field independently
  for (const field of CPU_FIELDS) {
    requireInvariant(
      after[field] >= before[field],
      "host-accounting.counter-decrease",
      `${path}.${field} decreased`,
    );
    fields[field] = after[field] - before[field];
  }
  const accountedTicks = sumFields(fields, ACCOUNTED_CPU_FIELDS);
  const busyTicks = sumFields(fields, BUSY_CPU_FIELDS);
  requireInvariant(
    accountedTicks > 0n,
    "host-accounting.zero-interval",
    `${path} has no aggregate CPU progress`,
  );
  return {
    accountedTicksExcludingGuestAddition: accountedTicks,
    busyTicksGuestNotAddedSeparately: busyTicks,
    fields,
    guestCountersAlreadyIncluded: fields.guest + fields.guestNice,
    idleTicks: fields.idle,
    iowaitTicks: fields.iowait,
  };
}

// expose exact CPU delta values as decimal strings
function exposeCpuDelta(delta) {
  return {
    accountedTicksExcludingGuestAddition: decimal(delta.accountedTicksExcludingGuestAddition),
    busyFraction: {
      denominatorTicks: decimal(delta.accountedTicksExcludingGuestAddition),
      numeratorTicks: decimal(delta.busyTicksGuestNotAddedSeparately),
    },
    busyTicksGuestNotAddedSeparately: decimal(delta.busyTicksGuestNotAddedSeparately),
    fields: Object.fromEntries(CPU_FIELDS.map(
      // expose one exact positional field delta
      (field) => [field, decimal(delta.fields[field])],
    )),
    guestCountersAlreadyIncluded: decimal(delta.guestCountersAlreadyIncluded),
    idleTicks: decimal(delta.idleTicks),
    iowaitTicks: decimal(delta.iowaitTicks),
    tickUnit: "kernel-USER_HZ-ticks-frequency-not-inferred",
  };
}

// expose one parsed raw snapshot without original file content
function exposeSnapshot(snapshot) {
  return {
    capturedAt: snapshot.capturedAt,
    cpu: Object.fromEntries(CPU_FIELDS.map(
      // expose one retained aggregate raw field
      (field) => [field, decimal(snapshot.cpu[field])],
    )),
    memory: Object.fromEntries(Object.entries(snapshot.memory).map(
      // expose one exact byte-unit memory field
      ([field, value]) => [field, decimal(value)],
    )),
    monotonicMilliseconds: snapshot.monotonicMilliseconds,
    rawSha256: {
      meminfo: snapshot.meminfoSha256,
      procStat: snapshot.procStatSha256,
    },
  };
}

// summarize one sampled bigint series without floating-point loss
function sampledSeries(values) {
  const sum = values.reduce(
    // add one sampled byte value
    (total, value) => total + value,
    0n,
  );
  const count = BigInt(values.length);
  return {
    arithmeticMeanBytes: {
      denominator: decimal(count),
      floor: decimal(sum / count),
      numerator: decimal(sum),
      remainder: decimal(sum % count),
    },
    maximumBytes: decimal(values.reduce(
      // retain one sampled maximum
      (maximum, value) => value > maximum ? value : maximum,
      0n,
    )),
    minimumBytes: decimal(values.reduce(
      // retain one sampled minimum
      (minimum, value) => value < minimum ? value : minimum,
      values[0],
    )),
    sampleCount: values.length,
  };
}

// evaluate bounded ordered host snapshots without granting collection authority
export function evaluateBackendPerformanceDirectHostAccounting(input) {
  requireInvariant(
    arguments.length === 1,
    "host-accounting.arguments",
    "host accounting accepts exactly one snapshot batch",
  );
  requireExactKeys(input, ["clockId", "contractVersion", "snapshots"], "$" );
  requireInvariant(
    input.contractVersion === BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_INPUT_VERSION,
    "host-accounting.version",
    "host accounting input version is invalid",
  );
  requireInvariant(
    typeof input.clockId === "string" && input.clockId.trim().length > 0
      && Buffer.byteLength(input.clockId, "utf8") <= 256,
    "host-accounting.clock",
    "host accounting clock identity is invalid",
  );
  requireInvariant(
    Array.isArray(input.snapshots) && input.snapshots.length >= 2
      && input.snapshots.length <= MAXIMUM_SNAPSHOTS
      && Object.keys(input.snapshots).length === input.snapshots.length,
    "host-accounting.snapshots",
    "host accounting requires two to 4096 dense snapshots",
  );
  const snapshots = input.snapshots.map(
    // parse every ordered raw snapshot
    (snapshot, index) => parseSnapshot(snapshot, index),
  );

  // require both wall and monotonic clocks to increase strictly
  for (let index = 1; index < snapshots.length; index += 1) {
    requireInvariant(
      snapshots[index].capturedAtMilliseconds > snapshots[index - 1].capturedAtMilliseconds
        && snapshots[index].monotonicMilliseconds > snapshots[index - 1].monotonicMilliseconds,
      "host-accounting.order",
      `$.snapshots[${String(index)}] does not follow its predecessor`,
    );
    requireInvariant(
      snapshots[index].memory.memTotalBytes === snapshots[0].memory.memTotalBytes
        && snapshots[index].memory.swapTotalBytes === snapshots[0].memory.swapTotalBytes,
      "host-accounting.capacity-change",
      `$.snapshots[${String(index)}] host memory or swap capacity changed`,
    );
  }
  const intervals = [];

  // retain every adjacent CPU interval for review
  for (let index = 1; index < snapshots.length; index += 1) {
    intervals.push({
      completedAt: snapshots[index].capturedAt,
      cpu: exposeCpuDelta(cpuDelta(
        snapshots[index - 1].cpu,
        snapshots[index].cpu,
        `$.intervals[${String(index - 1)}].cpu`,
      )),
      elapsedMonotonicMilliseconds:
        snapshots[index].monotonicMilliseconds - snapshots[index - 1].monotonicMilliseconds,
      startedAt: snapshots[index - 1].capturedAt,
    });
  }
  const aggregateCpu = exposeCpuDelta(cpuDelta(
    snapshots[0].cpu,
    snapshots.at(-1).cpu,
    "$.aggregate.cpu",
  ));
  return Object.freeze({
    authority: {
      collectionAuthority: "unverified-caller-supplied-offline-snapshots",
      nativeEligible: false,
      trafficGenerated: false,
    },
    clockId: input.clockId,
    contractVersion: BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_VERSION,
    cpu: {
      aggregate: aggregateCpu,
      caveats: [
        "guest and guest_nice are already included in user and nice and are retained but never added again",
        "iowait is retained separately and is not a reliable measure of time spent waiting for I/O",
        "aggregate proc stat covers application database background page-cache and kernel CPU activity",
      ],
      intervals,
    },
    memory: {
      caveats: [
        "MemAvailable is a kernel estimate and MemTotal excludes some reserved and device memory",
        "sampled host RAM covers application database background page-cache and kernel use",
      ],
      nonFree: {
        definition: "MemTotal-MemFree",
        ...sampledSeries(snapshots.map(
          // retain each nonfree memory sample
          (snapshot) => snapshot.memory.nonFreeBytes,
        )),
      },
      trueKernelPeak: {
        reason: "standard proc and the root cgroup expose no resettable whole-host RAM peak",
        supported: false,
        valueBytes: null,
      },
      unavailableEstimate: {
        definition: "MemTotal-MemAvailable",
        estimated: true,
        ...sampledSeries(snapshots.map(
          // retain each unavailable memory estimate
          (snapshot) => snapshot.memory.unavailableEstimateBytes,
        )),
      },
    },
    metadataKind: "performance-direct-host-accounting-offline-noncanonical",
    snapshots: snapshots.map(
      // expose every parsed sample and raw content identity
      (snapshot) => exposeSnapshot(snapshot),
    ),
    swap: {
      used: sampledSeries(snapshots.map(
        // retain each swap usage sample separately
        (snapshot) => snapshot.memory.swapUsedBytes,
      )),
    },
  });
}
