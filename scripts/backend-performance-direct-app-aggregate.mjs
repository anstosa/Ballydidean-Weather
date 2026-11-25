import {
  RESOURCE_INTERVAL_CONTRACT_VERSION,
} from "./backend-performance-resources.mjs";

export const BACKEND_PERFORMANCE_DIRECT_APP_AGGREGATE_INPUT_VERSION =
  "weather-backend-performance-direct-app-aggregate-input/v1";
export const BACKEND_PERFORMANCE_DIRECT_APP_AGGREGATE_VERSION =
  "weather-backend-performance-direct-app-aggregate/v1";

const CPU_FIELDS = Object.freeze([
  "nrPeriods", "nrThrottled", "systemUsec", "throttledUsec", "usageUsec", "userUsec",
]);
const MAXIMUM_SERVICES = 64;
const MAXIMUM_SAMPLES = 4_096;
const DIGEST_PATTERN = /^[a-f0-9]{64}$/u;

// create one coded aggregate failure
function fail(code, message, path) {
  const error = new Error(message);
  error.code = code;
  error.path = path;
  throw error;
}

// require one aggregate invariant
function requireInvariant(condition, code, message, path) {
  // reject ambiguous aggregate evidence
  if (!condition) {
    fail(code, message, path);
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
    "app-aggregate.schema",
    `${path} keys are invalid`,
    path,
  );
}

// compare one exact ordered string list
function sameList(actual, expected) {
  return Array.isArray(actual) && actual.length === expected.length && actual.every(
    // compare one list member
    (value, index) => value === expected[index],
  );
}

// require one nonnegative safe integer
function requireCounter(value, path) {
  requireInvariant(
    Number.isSafeInteger(value) && value >= 0,
    "app-aggregate.counter",
    `${path} must be one nonnegative safe integer`,
    path,
  );
  return value;
}

// add safe integer counters without overflow
function addCounter(left, right, path) {
  const sum = left + right;
  requireInvariant(
    Number.isSafeInteger(sum),
    "app-aggregate.counter",
    `${path} exceeds the safe integer range`,
    path,
  );
  return sum;
}

// normalize one absolute cgroup path
function cgroupPath(value, path) {
  requireInvariant(
    typeof value === "string" && value.startsWith("/")
      && value.length > 1 && !value.endsWith("/")
      && !value.includes("//") && !value.split("/").includes(".."),
    "app-aggregate.identity",
    `${path} is not one normalized absolute cgroup path`,
    path,
  );
  return value;
}

// compare one raw identity by deterministic JSON bytes
function sameIdentity(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

// classify native direct interval authority without granting physical custody
function nativeAuthority(value) {
  return isRecord(value)
    && value.kind === "native-linux-cgroup-v2"
    && value.liveEligible === true
    && isRecord(value.nativeCgroup)
    && Object.keys(value.nativeCgroup).length > 0
    && value.scopeCoverage === "direct-process-cgroup-only"
    && sameList(value.unsupportedScopes, ["app-aggregate", "host"]);
}

// validate one existing direct service interval for aggregate derivation
function validateServiceInterval(value, index, parent = false) {
  const path = parent ? "$.exclusiveParentInterval" : `$.serviceIntervals[${String(index)}]`;
  requireInvariant(
    isRecord(value) && value.contractVersion === RESOURCE_INTERVAL_CONTRACT_VERSION,
    "app-aggregate.contract",
    `${path} does not use the direct resource interval contract`,
    path,
  );
  requireInvariant(
    value.accountingScope === "service"
      && typeof value.scopeId === "string" && value.scopeId.length > 0,
    "app-aggregate.scope",
    `${path} must identify one service scope`,
    path,
  );
  requireInvariant(
    isRecord(value.accounting)
      && value.accounting.aggregateMethod === "direct-single-cgroup"
      && value.accounting.coverage === "measured-process-cgroup-only"
      && value.accounting.sampledServiceSumUsed === false
      && sameList(value.accounting.unsupportedScopes, ["app-aggregate", "host"]),
    "app-aggregate.scope",
    `${path} direct accounting descriptor is invalid`,
    path,
  );
  requireInvariant(
    isRecord(value.sourceBinding)
      && value.sourceBinding.kind === "integration-supplied-live-receipt-binding"
      && DIGEST_PATTERN.test(value.sourceBinding.sha256 ?? ""),
    "app-aggregate.source",
    `${path} lacks one live-receipt source binding`,
    path,
  );
  requireInvariant(
    isRecord(value.cgroupIdentity) && DIGEST_PATTERN.test(value.cgroupIdentity.sha256 ?? "")
      && isRecord(value.cgroupIdentity.root) && isRecord(value.cgroupIdentity.group),
    "app-aggregate.identity",
    `${path} cgroup identity is incomplete`,
    path,
  );
  const groupPath = cgroupPath(value.cgroupIdentity.group.path, `${path}.cgroupIdentity.group.path`);
  const rootPath = cgroupPath(value.cgroupIdentity.root.path, `${path}.cgroupIdentity.root.path`);
  requireInvariant(
    groupPath.startsWith(`${rootPath}/`),
    "app-aggregate.identity",
    `${path} group is outside its cgroup root`,
    `${path}.cgroupIdentity`,
  );
  requireInvariant(
    typeof value.startedAt === "string" && typeof value.completedAt === "string"
      && Date.parse(value.startedAt) <= Date.parse(value.completedAt),
    "app-aggregate.interval",
    `${path} interval is invalid`,
    path,
  );
  requireInvariant(
    isRecord(value.before) && isRecord(value.after)
      && value.before.cgroupId === value.cgroupIdentity.sha256
      && value.after.cgroupId === value.cgroupIdentity.sha256
      && value.before.releaseId === value.after.releaseId,
    "app-aggregate.identity",
    `${path} endpoint identity changed`,
    path,
  );
  requireInvariant(
    isRecord(value.cpu),
    "app-aggregate.cpu",
    `${path} CPU result is missing`,
    `${path}.cpu`,
  );
  const cpu = Object.fromEntries(CPU_FIELDS.map(
    // retain one exact direct CPU counter
    (field) => [field, requireCounter(value.cpu[field], `${path}.cpu.${field}`)],
  ));
  requireInvariant(
    cpu.usageUsec === cpu.userUsec + cpu.systemUsec,
    "app-aggregate.cpu",
    `${path} CPU usage differs from user plus system`,
    `${path}.cpu`,
  );
  requireInvariant(
    isRecord(value.memory) && isRecord(value.memory.steady)
      && Array.isArray(value.memory.steady.samples)
      && value.memory.steady.samples.length >= 1
      && value.memory.steady.samples.length <= MAXIMUM_SAMPLES
      && Object.keys(value.memory.steady.samples).length === value.memory.steady.samples.length,
    "app-aggregate.samples",
    `${path} steady-memory samples are invalid`,
    `${path}.memory.steady.samples`,
  );
  const samples = value.memory.steady.samples.map(
    // retain one exact aligned sample coordinate
    (sample, sampleIndex) => {
      const samplePath = `${path}.memory.steady.samples[${String(sampleIndex)}]`;
      requireInvariant(
        isRecord(sample) && typeof sample.observedAt === "string"
          && Number.isFinite(Date.parse(sample.observedAt))
          && typeof sample.offsetMs === "number" && Number.isFinite(sample.offsetMs),
        "app-aggregate.samples",
        `${samplePath} coordinate is invalid`,
        samplePath,
      );
      return {
        memoryCurrentBytes: requireCounter(
          sample.memoryCurrentBytes,
          `${samplePath}.memoryCurrentBytes`,
        ),
        observedAt: sample.observedAt,
        offsetMs: sample.offsetMs,
      };
    },
  );
  const sampledMaximumBytes = Math.max(...samples.map(
    // select one retained service sample value
    (sample) => sample.memoryCurrentBytes,
  ));
  requireInvariant(
    value.memory.steady.sampledMaximumBytes === sampledMaximumBytes,
    "app-aggregate.samples",
    `${path} sampled maximum differs from raw samples`,
    `${path}.memory.steady.sampledMaximumBytes`,
  );
  const peak = value.memory.peak;
  requireInvariant(
    isRecord(peak) && Number.isSafeInteger(peak.valueBytes) && peak.valueBytes >= sampledMaximumBytes,
    "app-aggregate.peak",
    `${path} parent-local peak or component peak is invalid`,
    `${path}.memory.peak`,
  );
  return {
    authority: value.authority,
    cgroupSha256: value.cgroupIdentity.sha256,
    completedAt: value.completedAt,
    cpu,
    groupPath,
    nativeAuthority: nativeAuthority(value.authority),
    peak: {
      readAt: peak.readAt,
      resetAt: peak.resetAt,
      scope: peak.scope,
      source: peak.source,
      valueBytes: peak.valueBytes,
    },
    rawIdentity: value.cgroupIdentity,
    releaseId: value.before.releaseId,
    rootIdentity: value.cgroupIdentity.root,
    samples,
    scopeId: value.scopeId,
    sourceReceiptSha256: value.sourceBinding.sha256,
    startedAt: value.startedAt,
  };
}

// require exact sibling or disjoint service cgroup identities
function validateNonOverlappingServices(services) {
  const identities = new Set();

  // compare every service with every later service once
  for (let leftIndex = 0; leftIndex < services.length; leftIndex += 1) {
    const left = services[leftIndex];
    requireInvariant(
      !identities.has(left.cgroupSha256),
      "app-aggregate.duplicate",
      `service cgroup ${left.cgroupSha256} appears more than once`,
      `$.serviceIntervals[${String(leftIndex)}]`,
    );
    identities.add(left.cgroupSha256);

    // reject equal ancestor or descendant service scopes
    for (let rightIndex = leftIndex + 1; rightIndex < services.length; rightIndex += 1) {
      const right = services[rightIndex];
      const overlaps = left.groupPath === right.groupPath
        || left.groupPath.startsWith(`${right.groupPath}/`)
        || right.groupPath.startsWith(`${left.groupPath}/`);
      requireInvariant(
        !overlaps,
        "app-aggregate.overlap",
        `${left.scopeId} and ${right.scopeId} cgroup paths overlap`,
        "$.serviceIntervals",
      );
    }
  }
}

// require one common source release interval and cgroup root
function validateCommonIdentity(services) {
  const first = services[0];

  // bind every service to the first exact accounting identity
  for (let index = 1; index < services.length; index += 1) {
    const service = services[index];
    requireInvariant(
      service.sourceReceiptSha256 === first.sourceReceiptSha256
        && service.releaseId === first.releaseId
        && service.startedAt === first.startedAt
        && service.completedAt === first.completedAt
        && sameIdentity(service.rootIdentity, first.rootIdentity),
      "app-aggregate.identity-mismatch",
      `${service.scopeId} does not share source release interval and cgroup root identity`,
      `$.serviceIntervals[${String(index)}]`,
    );
  }
  return first;
}

// sum one fixed set of service CPU deltas
function aggregateCpu(services) {
  const totals = Object.fromEntries(CPU_FIELDS.map(
    // initialize one aggregate CPU counter
    (field) => [field, 0],
  ));

  // add every nonoverlapping service exactly once
  for (const service of services) {
    // add every direct CPU field independently
    for (const field of CPU_FIELDS) {
      totals[field] = addCounter(totals[field], service.cpu[field], `$.cpu.${field}`);
    }
  }
  return totals;
}

// sum only samples with exact common wave coordinates
function aggregateSteadyMemory(services) {
  const first = services[0];
  const waves = first.samples.map(
    // initialize one sampling wave from the first service
    (sample, index) => ({
      index,
      memoryCurrentBytes: sample.memoryCurrentBytes,
      observedAt: sample.observedAt,
      offsetMs: sample.offsetMs,
    }),
  );

  // add every later service only at exact aligned coordinates
  for (let serviceIndex = 1; serviceIndex < services.length; serviceIndex += 1) {
    const service = services[serviceIndex];
    requireInvariant(
      service.samples.length === waves.length,
      "app-aggregate.wave-mismatch",
      `${service.scopeId} sample count differs from the common wave count`,
      `$.serviceIntervals[${String(serviceIndex)}].memory.steady.samples`,
    );

    // align each service sample to one exact wave
    for (let waveIndex = 0; waveIndex < waves.length; waveIndex += 1) {
      const wave = waves[waveIndex];
      const sample = service.samples[waveIndex];
      requireInvariant(
        sample.observedAt === wave.observedAt && sample.offsetMs === wave.offsetMs,
        "app-aggregate.wave-mismatch",
        `${service.scopeId} sample ${String(waveIndex)} is not aligned`,
        `$.serviceIntervals[${String(serviceIndex)}].memory.steady.samples[${String(waveIndex)}]`,
      );
      wave.memoryCurrentBytes = addCounter(
        wave.memoryCurrentBytes,
        sample.memoryCurrentBytes,
        `$.memory.steady.waves[${String(waveIndex)}].memoryCurrentBytes`,
      );
    }
  }
  const total = waves.reduce(
    // add one exact sampled wave
    (sum, wave) => addCounter(sum, wave.memoryCurrentBytes, "$.memory.steady.arithmeticMean"),
    0,
  );
  return {
    arithmeticMeanBytes: {
      denominator: waves.length,
      floor: Math.floor(total / waves.length),
      numerator: total,
      remainder: total % waves.length,
    },
    maximumBytes: Math.max(...waves.map(
      // select one aggregate sampled wave
      (wave) => wave.memoryCurrentBytes,
    )),
    sampleCount: waves.length,
    waves,
  };
}

// validate one optional exclusive parent-local peak interval
function parentLocalPeak(value, services, common) {
  // preserve unsupported peak when no parent interval exists
  if (value === null) {
    return {
      componentPeaksSummed: false,
      contractPrerequisitesSatisfied: false,
      reason: "no exclusive application parent interval was supplied",
      source: null,
      supported: false,
      valueBytes: null,
    };
  }
  const parent = validateServiceInterval(value, 0, true);
  requireInvariant(
    parent.scopeId === "application-parent"
      && parent.sourceReceiptSha256 === common.sourceReceiptSha256
      && parent.releaseId === common.releaseId
      && parent.startedAt === common.startedAt
      && parent.completedAt === common.completedAt
      && sameIdentity(parent.rootIdentity, common.rootIdentity),
    "app-aggregate.parent-identity",
    "exclusive application parent does not share the service source release interval and root",
    "$.exclusiveParentInterval",
  );

  // require the supplied parent to be a strict ancestor of every service
  for (const service of services) {
    requireInvariant(
      service.groupPath.startsWith(`${parent.groupPath}/`),
      "app-aggregate.parent-ancestry",
      `${service.scopeId} is not a strict descendant of the application parent`,
      "$.exclusiveParentInterval.cgroupIdentity.group.path",
    );
  }
  requireInvariant(
    parent.nativeAuthority && services.every(
      // require every contributing interval to share native direct authority
      (service) => service.nativeAuthority,
    )
      && parent.peak.scope === "private-retained-rw-fd"
      && parent.peak.source === "cgroup-memory-peak-reset"
      && parent.peak.resetAt === parent.startedAt
      && parent.peak.readAt === parent.completedAt,
    "app-aggregate.parent-authority",
    "exclusive application parent lacks exact native same-interval peak authority",
    "$.exclusiveParentInterval",
  );
  return {
    componentPeaksSummed: false,
    contractPrerequisitesSatisfied: true,
    parentCgroupSha256: parent.cgroupSha256,
    parentScopeId: parent.scopeId,
    reason: null,
    source: "exclusive-parent-local-cgroup-memory.peak",
    supported: true,
    valueBytes: parent.peak.valueBytes,
  };
}

// evaluate one offline application aggregate from exact direct intervals
export function evaluateBackendPerformanceDirectAppAggregate(input) {
  requireInvariant(
    arguments.length === 1,
    "app-aggregate.arguments",
    "application aggregate accepts exactly one interval batch",
    "$",
  );
  requireExactKeys(
    input,
    ["contractVersion", "exclusiveParentInterval", "serviceIntervals"],
    "$",
  );
  requireInvariant(
    input.contractVersion === BACKEND_PERFORMANCE_DIRECT_APP_AGGREGATE_INPUT_VERSION,
    "app-aggregate.version",
    "application aggregate input version is invalid",
    "$.contractVersion",
  );
  requireInvariant(
    Array.isArray(input.serviceIntervals) && input.serviceIntervals.length >= 2
      && input.serviceIntervals.length <= MAXIMUM_SERVICES
      && Object.keys(input.serviceIntervals).length === input.serviceIntervals.length,
    "app-aggregate.services",
    "application aggregate requires two to 64 dense service intervals",
    "$.serviceIntervals",
  );
  const services = input.serviceIntervals.map(
    // validate every direct service interval
    (interval, index) => validateServiceInterval(interval, index),
  );
  validateNonOverlappingServices(services);
  const common = validateCommonIdentity(services);
  const cpu = aggregateCpu(services);
  const steady = aggregateSteadyMemory(services);
  const peak = parentLocalPeak(input.exclusiveParentInterval, services, common);
  return Object.freeze({
    authority: {
      collectionAuthority: "unverified-caller-supplied-offline-intervals",
      nativeEligible: false,
      trafficGenerated: false,
    },
    contractVersion: BACKEND_PERFORMANCE_DIRECT_APP_AGGREGATE_VERSION,
    cpu: {
      aggregateMethod: "sum-nonoverlapping-same-interval-service-cgroup-deltas",
      ...cpu,
    },
    identity: {
      completedAt: common.completedAt,
      releaseId: common.releaseId,
      rootCgroup: common.rootIdentity,
      sourceReceiptSha256: common.sourceReceiptSha256,
      startedAt: common.startedAt,
    },
    memory: {
      componentKernelPeaks: services.map(
        // retain each measured peak without aggregation
        (service) => ({
          cgroupSha256: service.cgroupSha256,
          scopeId: service.scopeId,
          valueBytes: service.peak.valueBytes,
        }),
      ),
      kernelPeak: peak,
      steady: {
        aggregateMethod: "sum-exactly-aligned-service-memory.current-waves",
        ...steady,
      },
    },
    metadataKind: "performance-direct-app-aggregate-offline-noncanonical",
    services: services.map(
      // preserve each raw contributing identity and counter set
      (service) => ({
        cgroupIdentity: service.rawIdentity,
        cpu: service.cpu,
        nativeAuthority: service.nativeAuthority,
        scopeId: service.scopeId,
        steadySamples: service.samples,
      }),
    ),
  });
}
