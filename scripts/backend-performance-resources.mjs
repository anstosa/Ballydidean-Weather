import { createHash } from "node:crypto";
import {
  open,
  opendir,
  readlink,
  realpath,
  stat,
  statfs,
} from "node:fs/promises";
import { isAbsolute, relative, resolve, sep } from "node:path";
import { performance } from "node:perf_hooks";

export const RESOURCE_INTERVAL_CONTRACT_VERSION = "weather-backend-resource-interval/v1";

const digestPattern = /^[a-f0-9]{64}$/u;
const bootIdPattern = /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/u;
const counterNamePattern = /^[a-z][a-z0-9_]*$/u;
const maximumObservationBytes = 1024 * 1024;
const maximumDeadlineMs = 60 * 60 * 1000;
const maximumSampleCount = 4_096;
const maximumProcessDescriptors = 4_096;
const maximumKernelPathBytes = 4_096;
const cgroup2FilesystemMagic = 0x63677270n;
const processScopes = new Set(["postgresql", "secondary-runner", "service"]);
const accountingScopes = new Set(processScopes);
const cpuCounterNames = [
  "nr_periods",
  "nr_throttled",
  "system_usec",
  "throttled_usec",
  "usage_usec",
  "user_usec",
];
const memoryEventNames = ["high", "low", "max", "oom", "oom_group_kill", "oom_kill"];
const allowedConfigurationKeys = new Set([
  "accountingScope",
  "cgroupPath",
  "cgroupRoot",
  "deadlineMs",
  "maximumSamples",
  "minimumSampleSpacingMs",
  "minimumSamples",
  "pid",
  "releaseId",
  "scopeId",
  "sourceIdentitySha256",
]);
const defaultFilesystem = {
  open,
  opendir,
  readlink,
  realpath,
  stat,
  statfs,
};

// construct one coded collector failure
function collectorError(code, message, cause) {
  const error = new Error(message, cause === undefined ? undefined : { cause });
  error.code = code;
  return error;
}

// hash one unambiguous ordered value
function sha256Value(value) {
  return createHash("sha256").update(JSON.stringify(value)).digest("hex");
}

// compare one ordered JSON value
function valuesEqual(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

// retain one finite clock observation
function readClock(now) {
  const clock = now();

  // reject substituted or nonmonotonic clock shapes
  if (
    clock === null ||
    typeof clock !== "object" ||
    !Number.isFinite(clock.wallMs) ||
    !Number.isFinite(clock.monotonicMs)
  ) {
    throw collectorError("resource.clock", "collector clock is invalid");
  }
  return {
    iso: new Date(clock.wallMs).toISOString(),
    monotonicMs: clock.monotonicMs,
  };
}

// provide real wall and monotonic clocks
function defaultNow() {
  return {
    monotonicMs: performance.now(),
    wallMs: Date.now(),
  };
}

// validate one closed collector configuration
function validateConfiguration(configuration) {
  // require one plain configuration record
  if (configuration === null || typeof configuration !== "object" || Array.isArray(configuration)) {
    throw collectorError("resource.config", "resource collector configuration must be a record");
  }

  // reject hidden collector behavior
  for (const key of Object.keys(configuration)) {
    // reject every unsupported field
    if (!allowedConfigurationKeys.has(key)) {
      throw collectorError("resource.config", `unsupported resource collector field ${key}`);
    }
  }

  // require one recognized accounting scope
  if (!accountingScopes.has(configuration.accountingScope)) {
    throw collectorError("resource.config", "accountingScope is invalid");
  }
  // require stable caller and content identities
  if (
    typeof configuration.scopeId !== "string" ||
    configuration.scopeId.trim().length === 0 ||
    typeof configuration.releaseId !== "string" ||
    configuration.releaseId.trim().length === 0 ||
    !digestPattern.test(configuration.sourceIdentitySha256 ?? "")
  ) {
    throw collectorError("resource.config", "scope release or source identity is invalid");
  }
  // require canonical absolute cgroup paths
  if (
    typeof configuration.cgroupRoot !== "string" ||
    !isAbsolute(configuration.cgroupRoot) ||
    resolve(configuration.cgroupRoot) !== configuration.cgroupRoot ||
    typeof configuration.cgroupPath !== "string" ||
    !isAbsolute(configuration.cgroupPath) ||
    resolve(configuration.cgroupPath) !== configuration.cgroupPath
  ) {
    throw collectorError("resource.config", "cgroup paths must be canonical absolute paths");
  }
  // bound caller-provided identities before filesystem work
  if (
    Buffer.byteLength(configuration.cgroupRoot, "utf8") > maximumKernelPathBytes ||
    Buffer.byteLength(configuration.cgroupPath, "utf8") > maximumKernelPathBytes ||
    Buffer.byteLength(configuration.scopeId, "utf8") > 256 ||
    Buffer.byteLength(configuration.releaseId, "utf8") > 256
  ) {
    throw collectorError("resource.config", "resource collector identity exceeds its input bound");
  }

  const hasProcess = Number.isSafeInteger(configuration.pid) && configuration.pid > 0;

  // require a stable process for process-accounted scopes
  if (processScopes.has(configuration.accountingScope) && !hasProcess) {
    throw collectorError("resource.config", `${configuration.accountingScope} scope requires one process`);
  }
  // require bounded sampling and lifetime controls
  if (
    !Number.isSafeInteger(configuration.deadlineMs) ||
    configuration.deadlineMs < 1 ||
    configuration.deadlineMs > maximumDeadlineMs ||
    !Number.isSafeInteger(configuration.minimumSamples) ||
    configuration.minimumSamples < 2 ||
    !Number.isSafeInteger(configuration.maximumSamples) ||
    configuration.maximumSamples < configuration.minimumSamples ||
    configuration.maximumSamples > maximumSampleCount ||
    !Number.isSafeInteger(configuration.minimumSampleSpacingMs) ||
    configuration.minimumSampleSpacingMs < 0 ||
    configuration.minimumSampleSpacingMs > configuration.deadlineMs
  ) {
    throw collectorError("resource.config", "sampling bounds or deadline are invalid");
  }

  return { ...configuration };
}

// read one bounded filesystem value
async function readBoundedBytes(filesystem, path, allowEmpty = false) {
  const handle = await filesystem.open(path, "r");
  const buffer = Buffer.alloc(maximumObservationBytes + 1);
  let total = 0;

  try {
    // fill only the fixed observation buffer
    while (total < buffer.length) {
      const result = await handle.read(buffer, total, buffer.length - total, null);

      // reject invalid filesystem read results
      if (!Number.isSafeInteger(result?.bytesRead) || result.bytesRead < 0 || result.bytesRead > buffer.length - total) {
        throw collectorError("resource.read", `${path} returned an invalid bounded read`);
      }
      // stop at one observed end of file
      if (result.bytesRead === 0) {
        break;
      }
      total += result.bytesRead;
    }
  } finally {
    await handle.close();
  }

  // reject missing or oversized observations before decoding
  if ((!allowEmpty && total === 0) || total > maximumObservationBytes) {
    throw collectorError("resource.read", `${path} is empty or exceeds the observation limit`);
  }
  return buffer.subarray(0, total);
}

// parse one safe nonnegative integer
function parseSafeInteger(value, label) {
  // accept decimal integers only
  if (!/^(?:0|[1-9][0-9]*)$/u.test(value)) {
    throw collectorError("resource.parse", `${label} is not a nonnegative decimal integer`);
  }
  const parsed = Number(value);

  // reject lossy integer observations
  if (!Number.isSafeInteger(parsed)) {
    throw collectorError("resource.parse", `${label} exceeds the safe integer range`);
  }
  return parsed;
}

// parse one scalar cgroup integer
async function readScalarInteger(filesystem, path) {
  const text = (await readBoundedBytes(filesystem, path)).toString("utf8").trim();
  return parseSafeInteger(text, path);
}

// parse one closed counter file
async function readCounterFile(filesystem, path, requiredNames) {
  const text = (await readBoundedBytes(filesystem, path)).toString("utf8");
  const counters = {};
  const lines = text.endsWith("\n") ? text.slice(0, -1).split("\n") : text.split("\n");

  // parse every counter line
  for (const line of lines) {
    // reject blank interior or empty files
    if (line.length === 0) {
      throw collectorError("resource.parse", `${path} contains a blank counter line`);
    }
    const match = /^([^ ]+) ([^ ]+)$/u.exec(line);

    // reject ambiguous or duplicate counters
    if (match === null || !counterNamePattern.test(match[1]) || counters[match[1]] !== undefined) {
      throw collectorError("resource.parse", `${path} contains a malformed or duplicate counter`);
    }
    counters[match[1]] = parseSafeInteger(match[2], `${path}:${match[1]}`);
  }

  // require every contracted counter
  for (const name of requiredNames) {
    // reject partial kernel observations
    if (counters[name] === undefined) {
      throw collectorError("resource.parse", `${path} is missing ${name}`);
    }
  }
  return counters;
}

// validate one max-or-integer cgroup limit
function validateScalarLimit(value, label) {
  // preserve an unlimited kernel value explicitly
  if (value === "max") {
    return value;
  }
  parseSafeInteger(value, label);
  return value;
}

// read frozen cgroup limits and controllers
async function readCgroupLimits(filesystem, groupPath) {
  const [cpuMaxBytes, memoryMaxBytes, pidsMaxBytes, controllersBytes] = await Promise.all([
    readBoundedBytes(filesystem, `${groupPath}/cpu.max`),
    readBoundedBytes(filesystem, `${groupPath}/memory.max`),
    readBoundedBytes(filesystem, `${groupPath}/pids.max`),
    readBoundedBytes(filesystem, `${groupPath}/cgroup.controllers`, true),
  ]);
  const cpuMax = cpuMaxBytes.toString("utf8").trim();
  const memoryMax = validateScalarLimit(memoryMaxBytes.toString("utf8").trim(), "memory.max");
  const pidsMax = validateScalarLimit(pidsMaxBytes.toString("utf8").trim(), "pids.max");
  const cpuMatch = /^(max|(?:0|[1-9][0-9]*)) ((?:0|[1-9][0-9]*))$/u.exec(cpuMax);

  // reject malformed CPU quotas and periods
  if (cpuMatch === null) {
    throw collectorError("resource.parse", "cpu.max is malformed");
  }
  // validate finite CPU quota and period values
  if (cpuMatch[1] !== "max") {
    const cpuQuota = parseSafeInteger(cpuMatch[1], "cpu.max quota");

    // reject an impossible finite CPU quota
    if (cpuQuota < 1) {
      throw collectorError("resource.parse", "cpu.max quota must be positive");
    }
  }
  const cpuPeriod = parseSafeInteger(cpuMatch[2], "cpu.max period");

  // reject a zero CPU period
  if (cpuPeriod < 1) {
    throw collectorError("resource.parse", "cpu.max period must be positive");
  }
  const controllerText = controllersBytes.toString("utf8").trim();
  const controllers = controllerText.length === 0 ? [] : controllerText.split(/ +/u);

  // reject malformed or duplicate controllers
  if (
    controllers.some(
      // validate one controller token
      (controller) => !/^[a-z][a-z0-9_-]*$/u.test(controller),
    ) ||
    new Set(controllers).size !== controllers.length
  ) {
    throw collectorError("resource.parse", "cgroup.controllers is malformed");
  }
  const limits = { controllers, cpuMax, memoryMax, pidsMax };
  return { ...limits, sha256: sha256Value(limits) };
}

// retain one lossless filesystem identity
function filesystemIdentity(metadata, label) {
  // require stable device and inode values
  if (
    metadata === null ||
    typeof metadata !== "object" ||
    !["bigint", "number"].includes(typeof metadata.dev) ||
    !["bigint", "number"].includes(typeof metadata.ino)
  ) {
    throw collectorError("resource.identity", `${label} filesystem identity is unavailable`);
  }
  return {
    device: String(metadata.dev),
    inode: String(metadata.ino),
  };
}

// read one exact Linux boot identity
async function readBootId(filesystem) {
  const bootId = (await readBoundedBytes(filesystem, "/proc/sys/kernel/random/boot_id")).toString("utf8").trim();

  // reject missing or malformed boot identities
  if (!bootIdPattern.test(bootId)) {
    throw collectorError("resource.identity", "Linux boot identity is invalid");
  }
  return bootId;
}

// parse one process start time
function parseProcessStartTime(text, expectedPid) {
  const closingParenthesis = text.lastIndexOf(")");

  // reject malformed process stat records
  if (closingParenthesis < 0 || !text.startsWith(`${String(expectedPid)} (`)) {
    throw collectorError("resource.process", "process stat identity is malformed");
  }
  const fields = text.slice(closingParenthesis + 1).trim().split(/ +/u);

  // require the Linux starttime field
  if (fields.length < 20) {
    throw collectorError("resource.process", "process stat is missing starttime");
  }
  return parseSafeInteger(fields[19], "process starttime");
}

// parse one process status sample
function parseProcessStatus(text) {
  const rssMatch = /^VmRSS:\s+([0-9]+)\s+kB$/mu.exec(text);
  const threadMatch = /^Threads:\s+([0-9]+)$/mu.exec(text);

  // require exact RSS and thread observations
  if (rssMatch === null || threadMatch === null) {
    throw collectorError("resource.process", "process status lacks RSS or thread count");
  }
  const rssKiB = parseSafeInteger(rssMatch[1], "process VmRSS");
  const threadCount = parseSafeInteger(threadMatch[1], "process Threads");

  // reject an impossible thread count or byte overflow
  if (threadCount < 1 || !Number.isSafeInteger(rssKiB * 1024)) {
    throw collectorError("resource.process", "process status values are invalid");
  }
  return { rssBytes: rssKiB * 1024, threadCount };
}

// derive the kernel cgroup membership path
function expectedMembershipPath(rootPath, groupPath) {
  const child = relative(rootPath, groupPath).split(sep).join("/");
  return child.length === 0 ? "/" : `/${child}`;
}

// count one bounded process descriptor directory
async function countProcessDescriptors(filesystem, path) {
  const directory = await filesystem.opendir(path);
  let count = 0;

  try {
    // stream descriptor entries without one unbounded allocation
    while (true) {
      const entry = await directory.read();

      // stop at the observed directory boundary
      if (entry === null) {
        break;
      }
      // reject malformed descriptor names
      if (typeof entry.name !== "string" || !/^(?:0|[1-9][0-9]*)$/u.test(entry.name)) {
        throw collectorError("resource.process", "process descriptor snapshot is invalid");
      }
      count += 1;

      // reject enumeration above the declared collector budget
      if (count > maximumProcessDescriptors) {
        throw collectorError("resource.process", "process descriptor count exceeds the collector limit");
      }
    }
  } finally {
    await directory.close();
  }
  return count;
}

// read one stable process source identity and sample
async function readProcessObservation(filesystem, processId, rootPath, groupPath) {
  const processRoot = `/proc/${String(processId)}`;
  const [cgroupBytes, commandBytes, statBytes, statusBytes, executablePath, executableMetadata, fdCount] = await Promise.all([
    readBoundedBytes(filesystem, `${processRoot}/cgroup`),
    readBoundedBytes(filesystem, `${processRoot}/cmdline`),
    readBoundedBytes(filesystem, `${processRoot}/stat`),
    readBoundedBytes(filesystem, `${processRoot}/status`),
    filesystem.readlink(`${processRoot}/exe`),
    filesystem.stat(`${processRoot}/exe`, { bigint: true }),
    countProcessDescriptors(filesystem, `${processRoot}/fd`),
  ]);
  const membershipLines = cgroupBytes.toString("utf8").trim().split("\n");
  const expectedMembership = `0::${expectedMembershipPath(rootPath, groupPath)}`;

  // require one exact unified cgroup membership
  if (membershipLines.length !== 1 || membershipLines[0] !== expectedMembership) {
    throw collectorError("resource.process", "process cgroup membership changed or is not unified v2");
  }
  // require one observed executable path
  if (typeof executablePath !== "string" || executablePath.length === 0) {
    throw collectorError("resource.process", "process executable path is unavailable");
  }
  // retain the kernel-bounded executable link only
  if (Buffer.byteLength(executablePath, "utf8") > maximumKernelPathBytes) {
    throw collectorError("resource.process", "process executable path exceeds the kernel path bound");
  }
  const source = {
    cgroupMembership: expectedMembership,
    commandSha256: createHash("sha256").update(commandBytes).digest("hex"),
    executable: {
      ...filesystemIdentity(executableMetadata, "process executable"),
      path: executablePath,
    },
    startTimeTicks: parseProcessStartTime(statBytes.toString("utf8"), processId),
  };
  const statusSample = parseProcessStatus(statusBytes.toString("utf8"));

  return {
    sample: {
      fdCount,
      ...statusSample,
      samplingMethod: "instantaneous-not-peak",
    },
    source: { ...source, sha256: sha256Value(source) },
  };
}

// decode one mountinfo path field
function decodeMountInfoPath(value) {
  return value.replace(
    /\\([0-7]{3})/gu,
    // decode one kernel octal escape
    (_match, octal) => String.fromCodePoint(Number.parseInt(octal, 8)),
  );
}

// identify one exact cgroup v2 mount
function parseCgroupMountInfo(bytes, rootPath) {
  const records = [];

  // parse every mount record independently
  for (const line of bytes.toString("utf8").split("\n")) {
    // ignore one trailing newline
    if (line.length === 0) {
      continue;
    }
    const separator = line.indexOf(" - ");

    // reject malformed mount records
    if (separator < 0) {
      throw collectorError("resource.authority", "mountinfo contains a malformed record");
    }
    const left = line.slice(0, separator).split(" ");
    const right = line.slice(separator + 3).split(" ");

    // reject incomplete mount records
    if (left.length < 6 || right.length < 3) {
      throw collectorError("resource.authority", "mountinfo contains an incomplete record");
    }
    const mountPoint = decodeMountInfoPath(left[4]);

    // retain only the exact configured root
    if (mountPoint === rootPath) {
      records.push({
        device: left[2],
        filesystem: right[0],
        mountId: left[0],
        mountPoint,
        mountRoot: decodeMountInfoPath(left[3]),
        options: left[5],
        source: right[1],
        superOptions: right[2],
      });
    }
  }

  // require one full cgroup v2 mount root
  if (
    records.length !== 1 ||
    records[0].filesystem !== "cgroup2" ||
    records[0].mountRoot !== "/"
  ) {
    throw collectorError("resource.authority", "cgroup root is not one exact native cgroup v2 filesystem mount");
  }
  return records[0];
}

// normalize one statfs filesystem type
function statfsType(metadata, label) {
  // require one numeric filesystem magic value
  if (metadata === null || typeof metadata !== "object" || !["bigint", "number"].includes(typeof metadata.type)) {
    throw collectorError("resource.authority", `${label} filesystem type is unavailable`);
  }
  return BigInt(metadata.type);
}

// read one bounded namespace link identity
async function readNamespaceIdentity(filesystem, namespace) {
  const identity = await filesystem.readlink(`/proc/self/ns/${namespace}`);

  // require the exact kernel namespace identity form
  if (
    typeof identity !== "string" ||
    Buffer.byteLength(identity, "utf8") > maximumKernelPathBytes ||
    !new RegExp(`^${namespace}:\\[[0-9]+\\]$`, "u").test(identity)
  ) {
    throw collectorError("resource.authority", `${namespace} namespace identity is invalid`);
  }
  return identity;
}

// prove one target belongs to the active native cgroup v2 mount
async function readCgroupAuthority(filesystem, rootPath, groupPath) {
  const [rootFilesystem, groupFilesystem, mountInfoBytes, mountNamespace, cgroupNamespace] = await Promise.all([
    filesystem.statfs(rootPath, { bigint: true }),
    filesystem.statfs(groupPath, { bigint: true }),
    readBoundedBytes(filesystem, "/proc/self/mountinfo"),
    readNamespaceIdentity(filesystem, "mnt"),
    readNamespaceIdentity(filesystem, "cgroup"),
  ]);

  // reject ordinary files and cross-filesystem targets
  if (
    statfsType(rootFilesystem, "cgroup root") !== cgroup2FilesystemMagic ||
    statfsType(groupFilesystem, "cgroup target") !== cgroup2FilesystemMagic
  ) {
    throw collectorError("resource.authority", "cgroup target is not on a native cgroup v2 filesystem");
  }
  const mount = parseCgroupMountInfo(mountInfoBytes, rootPath);
  return {
    cgroupNamespace,
    filesystemMagic: `0x${cgroup2FilesystemMagic.toString(16)}`,
    mount,
    mountNamespace,
  };
}

// resolve one confined cgroup target
async function resolveCgroupTarget(filesystem, configuration) {
  const [rootPath, groupPath] = await Promise.all([
    filesystem.realpath(configuration.cgroupRoot),
    filesystem.realpath(configuration.cgroupPath),
  ]);
  const rootPrefix = `${rootPath}${sep}`;

  // reject cgroup aliases outside the configured root
  if (groupPath !== rootPath && !groupPath.startsWith(rootPrefix)) {
    throw collectorError("resource.identity", "cgroup path escapes its root");
  }
  const [rootMetadata, groupMetadata] = await Promise.all([
    filesystem.stat(rootPath, { bigint: true }),
    filesystem.stat(groupPath, { bigint: true }),
  ]);

  // require real cgroup directories
  if (
    typeof rootMetadata.isDirectory !== "function" ||
    !rootMetadata.isDirectory() ||
    typeof groupMetadata.isDirectory !== "function" ||
    !groupMetadata.isDirectory()
  ) {
    throw collectorError("resource.identity", "cgroup root or target is not a directory");
  }
  const authority = await readCgroupAuthority(filesystem, rootPath, groupPath);
  return {
    authority,
    group: {
      ...filesystemIdentity(groupMetadata, "cgroup target"),
      path: groupPath,
    },
    path: groupPath,
    root: {
      ...filesystemIdentity(rootMetadata, "cgroup root"),
      path: rootPath,
    },
  };
}

// read frozen cgroup and process identities
async function readStaticObservation(filesystem, configuration, target) {
  const [bootId, limits, groupMetadata, process] = await Promise.all([
    readBootId(filesystem),
    readCgroupLimits(filesystem, target.path),
    filesystem.stat(target.path, { bigint: true }),
    configuration.pid === null
      ? null
      : readProcessObservation(filesystem, configuration.pid, target.root.path, target.path),
  ]);
  const group = {
    ...filesystemIdentity(groupMetadata, "cgroup target"),
    path: target.path,
  };
  const cgroupIdentity = {
    bootId,
    group,
    root: target.root,
  };

  return {
    bootId,
    cgroup: { ...cgroupIdentity, sha256: sha256Value(cgroupIdentity) },
    limits,
    processSource: process?.source ?? null,
  };
}

// read dynamic counters and sampled process usage
async function readDynamicObservation(filesystem, configuration, target, now) {
  const [cpu, memoryEvents, memoryCurrentBytes, process] = await Promise.all([
    readCounterFile(filesystem, `${target.path}/cpu.stat`, cpuCounterNames),
    readCounterFile(filesystem, `${target.path}/memory.events`, memoryEventNames),
    readScalarInteger(filesystem, `${target.path}/memory.current`),
    configuration.pid === null
      ? null
      : readProcessObservation(filesystem, configuration.pid, target.root.path, target.path),
  ]);
  const observedAt = readClock(now);

  return {
    cpu,
    memoryCurrentBytes,
    memoryEvents,
    monotonicMs: observedAt.monotonicMs,
    observedAt: observedAt.iso,
    process,
  };
}

// write one private descriptor peak reset
async function resetPeakDescriptor(handle) {
  const reset = Buffer.from("1", "utf8");
  const result = await handle.write(reset, 0, reset.length, 0);

  // require one complete positional reset write
  if (result?.bytesWritten !== reset.length) {
    throw collectorError("resource.peak", "memory.peak private descriptor reset was incomplete");
  }
}

// read one integer from the retained descriptor position
async function readPeakDescriptor(handle) {
  const buffer = Buffer.alloc(128);
  const result = await handle.read(buffer, 0, buffer.length, 0);

  // reject empty or truncated descriptor reads
  if (!Number.isSafeInteger(result?.bytesRead) || result.bytesRead < 1 || result.bytesRead >= buffer.length) {
    throw collectorError("resource.peak", "memory.peak private descriptor read was incomplete");
  }
  return parseSafeInteger(buffer.subarray(0, result.bytesRead).toString("utf8").trim(), "memory.peak");
}

// subtract one monotonic kernel counter set
function counterDelta(before, after, names, label) {
  const delta = {};

  // compare every contracted counter
  for (const name of names) {
    // reject counter reset or identity substitution
    if (after[name] < before[name]) {
      throw collectorError("resource.counter", `${label} counter decreased for ${name}`);
    }
    delta[name] = after[name] - before[name];
  }
  return delta;
}

// expose one bounded steady sample
function steadySample(observation, startedMonotonicMs) {
  return {
    memoryCurrentBytes: observation.memoryCurrentBytes,
    observedAt: observation.observedAt,
    offsetMs: observation.monotonicMs - startedMonotonicMs,
  };
}

// expose one sampled process observation
function processSample(observation, startedMonotonicMs) {
  // preserve null process scopes without inferred usage
  if (observation.process === null) {
    return null;
  }
  return {
    ...observation.process.sample,
    observedAt: observation.observedAt,
    offsetMs: observation.monotonicMs - startedMonotonicMs,
  };
}

// bind one dynamic process sample to its frozen source
function validateProcessSource(expectedSource, observation) {
  const observedSource = observation.process?.source ?? null;

  // reject process reuse during any sampled observation
  if (!valuesEqual(expectedSource, observedSource)) {
    throw collectorError("resource.identity", "process identity changed during the resource interval");
  }
}

// construct one verified raw resource interval
function buildResult(configuration, authority, staticBefore, staticAfter, before, after, samples, peak, budgetStartedAt, startedAt, completedAt) {
  // reject cgroup or boot replacement during measurement
  if (staticBefore.cgroup.sha256 !== staticAfter.cgroup.sha256) {
    throw collectorError("resource.identity", "cgroup identity changed during the resource interval");
  }
  // reject resource limit or controller changes
  if (!valuesEqual(staticBefore.limits, staticAfter.limits)) {
    throw collectorError("resource.identity", "cgroup limits changed during the resource interval");
  }
  // reject process reuse or executable substitution
  if (!valuesEqual(staticBefore.processSource, staticAfter.processSource)) {
    throw collectorError("resource.identity", "process identity changed during the resource interval");
  }
  const cpu = counterDelta(before.cpu, after.cpu, cpuCounterNames, "cpu.stat");
  const events = counterDelta(before.memoryEvents, after.memoryEvents, memoryEventNames, "memory.events");
  const steadySamples = samples.map(
    // retain every raw memory sample
    (sample) => steadySample(sample, startedAt.monotonicMs),
  );
  const processSamples = samples.map(
    // retain every raw process sample
    (sample) => processSample(sample, startedAt.monotonicMs),
  ).filter(
    // omit process samples for aggregate scopes
    (sample) => sample !== null,
  );
  const sampledMaximumBytes = Math.max(...steadySamples.map(
    // select one sampled current value
    (sample) => sample.memoryCurrentBytes,
  ));

  // require the kernel interval peak to cover every watched current sample
  if (peak.valueBytes < peak.initializedBytes || peak.valueBytes < sampledMaximumBytes) {
    throw collectorError("resource.peak", "memory.peak is below an in-window memory.current sample");
  }
  const arithmeticMeanBytes = steadySamples.reduce(
    // update one bounded running arithmetic mean
    (mean, sample, index) => mean + (sample.memoryCurrentBytes - mean) / (index + 1),
    0,
  );
  const cgroupId = staticBefore.cgroup.sha256;

  return {
    accounting: {
      aggregateMethod: "direct-single-cgroup",
      coverage: "measured-process-cgroup-only",
      sampledServiceSumUsed: false,
      unsupportedScopes: ["app-aggregate", "host"],
    },
    accountingScope: configuration.accountingScope,
    after: {
      cgroupId,
      observedAt: after.observedAt,
      pid: configuration.pid,
      releaseId: configuration.releaseId,
    },
    before: {
      cgroupId,
      observedAt: before.observedAt,
      pid: configuration.pid,
      releaseId: configuration.releaseId,
    },
    authority,
    cgroupIdentity: staticBefore.cgroup,
    completedAt: completedAt.iso,
    contractVersion: RESOURCE_INTERVAL_CONTRACT_VERSION,
    cpu: {
      nrPeriods: cpu.nr_periods,
      nrThrottled: cpu.nr_throttled,
      source: "cgroup-cpu-stat-delta",
      systemUsec: cpu.system_usec,
      throttledUsec: cpu.throttled_usec,
      usageUsec: cpu.usage_usec,
      userUsec: cpu.user_usec,
    },
    limits: staticBefore.limits,
    memory: {
      eventsDelta: {
        high: events.high,
        low: events.low,
        max: events.max,
        oom: events.oom,
        oomGroupKill: events.oom_group_kill,
        oomKill: events.oom_kill,
      },
      peak,
      steady: {
        arithmeticMeanBytes,
        method: "bounded-caller-scheduled-memory.current-samples",
        sampledMaximumBytes,
        samples: steadySamples,
        source: "cgroup-memory-current-window",
      },
    },
    method: {
      cpu: "cgroup-v2-cpu.stat-counter-delta",
      peak: "linux-cgroup-v2-memory.peak-private-retained-rw-fd-reset",
      process: "procfs-instantaneous-samples-not-peaks",
      steady: "caller-triggered-bounded-snapshots",
    },
    observations: {
      after,
      before,
    },
    process: {
      samples: processSamples,
      source: "procfs-snapshot",
      sourceIdentity: staticBefore.processSource,
    },
    sampling: {
      budgetStartedAt: budgetStartedAt.iso,
      deadlineMs: configuration.deadlineMs,
      maximumSamples: configuration.maximumSamples,
      minimumSampleSpacingMs: configuration.minimumSampleSpacingMs,
      minimumSamples: configuration.minimumSamples,
      sampleCount: samples.length,
    },
    scopeId: configuration.scopeId,
    sourceBinding: {
      kind: "integration-supplied-live-receipt-binding",
      sha256: configuration.sourceIdentitySha256,
    },
    startedAt: startedAt.iso,
  };
}

// begin one bounded raw resource interval
export async function beginBackendResourceInterval(configuration, injected = {}) {
  const validated = validateConfiguration(configuration);
  const filesystem = injected.fs ?? defaultFilesystem;
  const now = injected.now ?? defaultNow;
  const injectedTestOnly = injected.fs !== undefined || injected.now !== undefined;
  const budgetStartedAt = readClock(now);
  const deadlineAt = budgetStartedAt.monotonicMs + validated.deadlineMs;
  let peakHandle = null;
  let closePromise = null;
  let deadlineTimer = null;
  let terminal = false;
  let terminalReason = null;
  let status = "initializing";
  let operationPromise = null;
  let terminalCleanupPromise = null;

  // close the retained descriptor once
  async function closePeak() {
    // allow a later-opened peak descriptor to be closed
    if (peakHandle === null) {
      return;
    }
    // share one in-flight close operation
    if (closePromise !== null) {
      return closePromise;
    }
    closePromise = peakHandle.close();
    return closePromise;
  }

  // clear one active real deadline timer
  function clearDeadlineTimer() {
    // clear only one installed timer
    if (deadlineTimer !== null) {
      clearTimeout(deadlineTimer);
      deadlineTimer = null;
    }
  }

  // construct one stable terminal failure
  function terminalFailure(fallback = "resource interval is closed") {
    return terminalReason ?? collectorError("resource.session", fallback);
  }

  // require a live interval after awaited work
  function requireLive() {
    // reject timeout cancellation or completed sessions
    if (terminal) {
      throw terminalFailure();
    }
  }

  // require the complete initialization and collection deadline
  function requireWithinDeadline(clock) {
    // terminate observations outside the promised interval
    if (clock.monotonicMs > deadlineAt) {
      throw collectorError("resource.deadline", "resource interval deadline was exceeded");
    }
  }

  // terminate from an external abort or real deadline
  function startExternalTermination(error, nextStatus) {
    // share one terminal cleanup operation
    if (terminalCleanupPromise !== null) {
      return terminalCleanupPromise;
    }
    terminal = true;
    terminalReason = error;
    status = nextStatus;
    clearDeadlineTimer();
    const inFlight = operationPromise;
    terminalCleanupPromise = (async () => {
      await closePeak();

      // wait until an in-flight operation observes cancellation
      if (inFlight !== null) {
        try {
          await inFlight;
        } catch {
          // retain the original terminal reason
        }
      }
      status = "closed";
    })();
    return terminalCleanupPromise;
  }

  // install one descriptor lifetime deadline
  function installDeadlineTimer() {
    deadlineTimer = setTimeout(
      // close retained resources at the real deadline
      () => {
        void startExternalTermination(
          collectorError("resource.deadline", "resource interval real deadline was exceeded"),
          "deadline",
        ).catch(
          // avoid one unhandled cleanup rejection from an unobserved session
          () => {},
        );
      },
      validated.deadlineMs,
    );
    deadlineTimer.unref?.();
  }

  installDeadlineTimer();

  try {
    const target = await resolveCgroupTarget(filesystem, validated);

    requireLive();
    requireWithinDeadline(readClock(now));
    const staticBefore = await readStaticObservation(filesystem, validated, target);

    requireLive();
    requireWithinDeadline(readClock(now));
    peakHandle = await filesystem.open(`${target.path}/memory.peak`, "r+");

    requireLive();
    const descriptorMetadata = await peakHandle.stat({ bigint: true });

    requireLive();
    requireWithinDeadline(readClock(now));
    const descriptorIdentity = filesystemIdentity(descriptorMetadata, "memory.peak descriptor");
    const fdIdentitySha256 = sha256Value(descriptorIdentity);

    await resetPeakDescriptor(peakHandle);

    requireLive();
    const initializedBytes = await readPeakDescriptor(peakHandle);

    requireLive();
    const startedAt = readClock(now);

    requireWithinDeadline(startedAt);
    const before = await readDynamicObservation(filesystem, validated, target, now);

    requireLive();
    validateProcessSource(staticBefore.processSource, before);
    // reject initialization that already consumed the interval budget
    if (before.monotonicMs > deadlineAt) {
      throw collectorError("resource.deadline", "resource interval deadline was exceeded during initialization");
    }
    const samples = [before];

    status = "open";

    // append one bounded ordered sample
    function appendSample(observation) {
      // enforce the complete bounded sample budget
      if (samples.length >= validated.maximumSamples) {
        throw collectorError("resource.samples", "resource interval sample limit was exceeded");
      }
      const previous = samples.at(-1);

      // enforce monotonic sample spacing
      if (
        observation.monotonicMs < previous.monotonicMs ||
        observation.monotonicMs - previous.monotonicMs < validated.minimumSampleSpacingMs
      ) {
        throw collectorError("resource.samples", "resource interval sample spacing is invalid");
      }
      samples.push(observation);
    }

    // run one exclusive lifecycle operation
    function runOperation(nextStatus, operation, completeOnSuccess = false) {
      // reject completed or cancelled sessions
      if (terminal) {
        return Promise.reject(terminalFailure());
      }
      // fail closed on concurrent sample or finish calls
      if (operationPromise !== null) {
        const error = collectorError("resource.session", "resource interval already has an active operation");
        void startExternalTermination(error, "failed");
        return Promise.reject(error);
      }
      status = nextStatus;
      let currentOperation;
      currentOperation = (async () => {
        try {
          const value = await operation();

          requireLive();
          // close one successfully completed interval
          if (completeOnSuccess) {
            terminal = true;
            status = "finishing";
            clearDeadlineTimer();
            await closePeak();
            status = "closed";
          } else {
            status = "open";
          }
          return value;
        } catch (error) {
          // retain an external abort or establish one terminal failure
          if (!terminal) {
            terminal = true;
            terminalReason = error;
          }
          status = "failed";
          clearDeadlineTimer();
          await closePeak();
          status = "closed";
          throw terminalFailure(error instanceof Error ? error.message : String(error));
        } finally {
          // release only this exact operation slot
          if (operationPromise === currentOperation) {
            operationPromise = null;
          }
        }
      })();
      operationPromise = currentOperation;
      return currentOperation;
    }

    const authority = {
      bounds: {
        descriptorEntries: maximumProcessDescriptors,
        kernelPathBytes: maximumKernelPathBytes,
        observationBytes: maximumObservationBytes,
      },
      kind: injectedTestOnly ? "injected-test-only" : "native-linux-cgroup-v2",
      liveEligible: !injectedTestOnly,
      nativeCgroup: target.authority,
      scopeCoverage: "direct-process-cgroup-only",
      unsupportedScopes: ["app-aggregate", "host"],
    };

    const session = {
      // cancel one unfinished interval
      async abort() {
        // preserve idempotent cancellation
        if (terminalCleanupPromise !== null) {
          return terminalCleanupPromise;
        }
        // preserve one completed session
        if (terminal && status === "closed") {
          return;
        }
        return startExternalTermination(
          collectorError("resource.session", "resource interval was aborted"),
          "aborting",
        );
      },

      // finish and verify one interval
      async finish() {
        return runOperation("finishing", async () => {
          requireWithinDeadline(readClock(now));
          const after = await readDynamicObservation(filesystem, validated, target, now);

          requireLive();
          requireWithinDeadline({ monotonicMs: after.monotonicMs });
          validateProcessSource(staticBefore.processSource, after);
          appendSample(after);

          // require enough actual steady observations
          if (samples.length < validated.minimumSamples) {
            throw collectorError("resource.samples", "resource interval has too few steady samples");
          }
          const staticAfter = await readStaticObservation(filesystem, validated, target);

          requireLive();
          requireWithinDeadline(readClock(now));
          const authorityAfter = await readCgroupAuthority(
            filesystem,
            target.root.path,
            target.path,
          );

          requireLive();
          requireWithinDeadline(readClock(now));
          // reject mount or namespace substitution during measurement
          if (!valuesEqual(target.authority, authorityAfter)) {
            throw collectorError("resource.authority", "native cgroup authority changed during the interval");
          }
          const descriptorAfter = filesystemIdentity(
            await peakHandle.stat({ bigint: true }),
            "memory.peak descriptor",
          );

          requireLive();
          requireWithinDeadline(readClock(now));
          // require the same retained descriptor identity through completion
          if (!valuesEqual(descriptorIdentity, descriptorAfter)) {
            throw collectorError("resource.peak", "memory.peak descriptor identity changed");
          }
          const valueBytes = await readPeakDescriptor(peakHandle);

          requireLive();
          const completedAt = readClock(now);

          requireWithinDeadline(completedAt);
          return buildResult(
            validated,
            authority,
            staticBefore,
            staticAfter,
            before,
            after,
            samples,
            {
              fdIdentitySha256,
              initializedBytes,
              readAt: completedAt.iso,
              resetAt: startedAt.iso,
              scope: "private-retained-rw-fd",
              source: "cgroup-memory-peak-reset",
              valueBytes,
            },
            budgetStartedAt,
            startedAt,
            completedAt,
          );
        }, true);
      },

      // capture one explicit bounded steady sample
      async sample() {
        return runOperation("sampling", async () => {
          requireWithinDeadline(readClock(now));
          const observation = await readDynamicObservation(filesystem, validated, target, now);

          requireLive();
          requireWithinDeadline({ monotonicMs: observation.monotonicMs });
          validateProcessSource(staticBefore.processSource, observation);
          appendSample(observation);
          return {
            memory: steadySample(observation, startedAt.monotonicMs),
            process: processSample(observation, startedAt.monotonicMs),
          };
        });
      },
    };

    return Object.freeze(session);
  } catch (error) {
    // retain one external deadline reason when it won the race
    if (!terminal) {
      terminal = true;
      terminalReason = error;
    }
    clearDeadlineTimer();
    await closePeak();
    status = "closed";
    throw terminalFailure(error instanceof Error ? error.message : String(error));
  }
}
