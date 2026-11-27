import { createHash } from "node:crypto";
import { constants } from "node:fs";
import { open, realpath, statfs } from "node:fs/promises";
import { arch, hostname, machine, release } from "node:os";
import { isAbsolute } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

import {
  BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_INPUT_VERSION,
  evaluateBackendPerformanceDirectHostAccounting,
} from "./backend-performance-direct-host-accounting.mjs";

export const BACKEND_PERFORMANCE_DIRECT_HOST_OBSERVER_REQUEST_VERSION =
  "weather-backend-performance-direct-host-observer-request/v1";
export const BACKEND_PERFORMANCE_DIRECT_HOST_OBSERVER_VERSION =
  "weather-backend-performance-direct-host-observer/v1";
const BACKEND_DIRECT_LIVE_OBSERVATION_VERSION =
  "weather-backend-direct-live-observation/v1";

const BOOT_ID_PATH = "/proc/sys/kernel/random/boot_id";
const MACHINE_ID_PATH = "/etc/machine-id";
const MEMINFO_PATH = "/proc/meminfo";
const PROC_STAT_PATH = "/proc/stat";
const OBSERVER_PATH = fileURLToPath(import.meta.url);
const BOOT_ID_PATTERN = /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/u;
const DIGEST_PATTERN = /^[a-f0-9]{64}$/u;
const MACHINE_ID_PATTERN = /^[a-f0-9]{32}$/u;
const HOSTNAME_PATTERN = /^[a-z0-9](?:[a-z0-9.-]{0,251}[a-z0-9])?$/iu;
const ARCHITECTURE_PATTERN = /^[a-z0-9_]{1,32}$/u;
const RUN_ID_PATTERN = /^[a-zA-Z0-9._:-]{1,128}$/u;
const MAXIMUM_READ_BYTES = 1024 * 1024;
const MAXIMUM_SAMPLES = 4_096;
const MAXIMUM_SPACING_MS = 60_000;
const MAXIMUM_DURATION_MS = 60 * 60 * 1_000;
const PROC_SUPER_MAGIC = 0x9fa0n;
const REQUEST_KEYS = Object.freeze([
  "contractVersion",
  "expectedArchitecture",
  "expectedBootId",
  "expectedHostname",
  "expectedObserverSha256",
  "runId",
  "sampleCount",
  "sampleSpacingMs",
  "sourceReceiptSha256",
]);
const OPTIONAL_REQUEST_KEY = "sourceReceiptFile";
const CLI_FIELDS = Object.freeze([
  ["--run-id", "runId"],
  ["--source-receipt-sha256", "sourceReceiptSha256"],
  ["--expected-observer-sha256", "expectedObserverSha256"],
  ["--expected-architecture", "expectedArchitecture"],
  ["--expected-boot-id", "expectedBootId"],
  ["--expected-hostname", "expectedHostname"],
  ["--sample-count", "sampleCount"],
  ["--sample-spacing-ms", "sampleSpacingMs"],
]);

// create one controlled observer failure
function observerError(code, message, cause) {
  const error = new Error(message, cause === undefined ? undefined : { cause });
  error.code = code;
  return error;
}

// identify one plain request record
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    && (Object.getPrototypeOf(value) === Object.prototype
      || Object.getPrototypeOf(value) === null);
}

// hash one exact byte sequence
function sha256Bytes(value) {
  return createHash("sha256").update(value).digest("hex");
}

// hash one ordered json value
function sha256Value(value) {
  return sha256Bytes(Buffer.from(JSON.stringify(value)));
}

// retain one stable filesystem identity
function fileIdentity(metadata, path) {
  // require one regular file descriptor
  if (!metadata?.isFile?.()) {
    throw observerError("host-observer.file-type", `${path} is not a regular file`);
  }
  return {
    device: String(metadata.dev),
    inode: String(metadata.ino),
    mode: String(metadata.mode),
    linkCount: String(metadata.nlink),
  };
}

// compare one stable filesystem identity
function sameIdentity(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

// retain one private source receipt identity
function privateSourceIdentity(metadata, path) {
  // require one owner-held private regular file without aliases
  if (!metadata?.isFile?.()
    || metadata.uid !== BigInt(process.geteuid())
    || (metadata.mode & 0o7777n) !== 0o600n
    || metadata.nlink !== 1n
    || metadata.size <= 0n
    || metadata.size > BigInt(MAXIMUM_READ_BYTES)) {
    throw observerError(
      "host-observer.source-receipt-custody",
      `${path} is not a private owner-held regular file`,
    );
  }
  return {
    changedNanoseconds: String(metadata.ctimeNs),
    device: String(metadata.dev),
    inode: String(metadata.ino),
    linkCount: String(metadata.nlink),
    mode: String(metadata.mode),
    modifiedNanoseconds: String(metadata.mtimeNs),
    ownerUserId: String(metadata.uid),
    sizeBytes: String(metadata.size),
  };
}

// read one fixed file through a bounded no-follow descriptor
async function readBoundedFile(filesystem, path) {
  let handle;

  try {
    handle = await filesystem.open(
      path,
      constants.O_RDONLY | constants.O_NOFOLLOW,
    );
    const before = fileIdentity(await handle.stat({ bigint: true }), path);
    const buffer = Buffer.alloc(MAXIMUM_READ_BYTES + 1);
    let total = 0;

    // fill only the fixed observation buffer
    while (total < buffer.length) {
      const result = await handle.read(buffer, total, buffer.length - total, null);

      // reject malformed descriptor reads
      if (!Number.isSafeInteger(result?.bytesRead)
        || result.bytesRead < 0 || result.bytesRead > buffer.length - total) {
        throw observerError("host-observer.read", `${path} returned an invalid read`);
      }
      // stop at the observed file boundary
      if (result.bytesRead === 0) {
        break;
      }
      total += result.bytesRead;
    }
    const after = fileIdentity(await handle.stat({ bigint: true }), path);

    // reject empty oversized or replaced source files
    if (total === 0 || total > MAXIMUM_READ_BYTES || !sameIdentity(before, after)) {
      throw observerError("host-observer.read", `${path} is empty oversized or changed`);
    }
    return {
      bytes: buffer.subarray(0, total),
      identity: before,
    };
  } catch (error) {
    // retain controlled observer errors unchanged
    if (typeof error?.code === "string" && error.code.startsWith("host-observer.")) {
      throw error;
    }
    throw observerError("host-observer.read", `failed to read ${path}`, error);
  } finally {
    // close every acquired descriptor
    if (handle !== undefined) {
      await handle.close();
    }
  }
}

// read one exact private source receipt without following aliases
async function readPrivateSourceReceipt(filesystem, path, expectedSha256) {
  let handle;

  try {
    const canonicalBefore = await filesystem.realpath(path);

    // require the caller to name the one canonical filesystem path
    if (canonicalBefore !== path) {
      throw observerError(
        "host-observer.source-receipt-custody",
        "source receipt path is not canonical",
      );
    }
    handle = await filesystem.open(
      path,
      constants.O_RDONLY | constants.O_NOFOLLOW,
    );
    const before = privateSourceIdentity(await handle.stat({ bigint: true }), path);
    const buffer = Buffer.alloc(MAXIMUM_READ_BYTES + 1);
    let total = 0;

    // fill only the fixed source receipt buffer
    while (total < buffer.length) {
      const result = await handle.read(buffer, total, buffer.length - total, null);

      // reject malformed descriptor reads
      if (!Number.isSafeInteger(result?.bytesRead)
        || result.bytesRead < 0 || result.bytesRead > buffer.length - total) {
        throw observerError(
          "host-observer.source-receipt-read",
          "source receipt returned an invalid read",
        );
      }
      // stop at the observed file boundary
      if (result.bytesRead === 0) {
        break;
      }
      total += result.bytesRead;
    }
    const after = privateSourceIdentity(await handle.stat({ bigint: true }), path);
    const bytes = buffer.subarray(0, total);

    // reject changed truncated or oversized receipt bytes
    if (total !== Number(BigInt(before.sizeBytes))
      || total > MAXIMUM_READ_BYTES
      || !sameIdentity(before, after)) {
      throw observerError(
        "host-observer.source-receipt-read",
        "source receipt is truncated oversized or changed",
      );
    }
    const canonicalAfter = await filesystem.realpath(path);

    // require the same canonical name after the descriptor read
    if (canonicalAfter !== path) {
      throw observerError(
        "host-observer.source-receipt-custody",
        "source receipt path changed or is not canonical",
      );
    }
    // bind the caller's digest to the exact raw receipt bytes
    if (sha256Bytes(bytes) !== expectedSha256) {
      throw observerError(
        "host-observer.source-receipt-digest",
        "source receipt digest differs from the request",
      );
    }
    return { bytes, identity: before };
  } catch (error) {
    // retain controlled observer errors unchanged
    if (typeof error?.code === "string" && error.code.startsWith("host-observer.")) {
      throw error;
    }
    throw observerError(
      "host-observer.source-receipt-read",
      "failed to read source receipt",
      error,
    );
  } finally {
    // close every acquired receipt descriptor
    if (handle !== undefined) {
      await handle.close();
    }
  }
}

// decode one complete utf8 file
function decodeText(observation, path) {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(observation.bytes);
  } catch (error) {
    throw observerError("host-observer.encoding", `${path} is not valid UTF-8`, error);
  }
}

// read one finite wall and monotonic clock pair
function readClock(clock) {
  const wall = clock.wallNow();
  const monotonicMilliseconds = clock.monotonicMilliseconds();

  // reject invalid or lossy clock observations
  if (!(wall instanceof Date) || !Number.isFinite(wall.getTime())
    || !Number.isSafeInteger(monotonicMilliseconds) || monotonicMilliseconds < 0) {
    throw observerError("host-observer.clock", "host observer clock is invalid");
  }
  return {
    capturedAt: wall.toISOString(),
    monotonicMilliseconds,
  };
}

// validate one closed physical collection request
function validateRequest(request) {
  const expectedKeys = request !== null && typeof request === "object"
    && Object.hasOwn(request, OPTIONAL_REQUEST_KEY)
    ? [...REQUEST_KEYS, OPTIONAL_REQUEST_KEY]
    : REQUEST_KEYS;

  // require one exact request schema
  if (!isRecord(request)
    || JSON.stringify(Object.keys(request).sort()) !== JSON.stringify([...expectedKeys].sort())) {
    throw observerError("host-observer.request", "host observer request keys are invalid");
  }
  // require fixed contract and bounded caller identities
  if (request.contractVersion !== BACKEND_PERFORMANCE_DIRECT_HOST_OBSERVER_REQUEST_VERSION
    || !ARCHITECTURE_PATTERN.test(request.expectedArchitecture)
    || !BOOT_ID_PATTERN.test(request.expectedBootId)
    || !HOSTNAME_PATTERN.test(request.expectedHostname)
    || !DIGEST_PATTERN.test(request.expectedObserverSha256)
    || !RUN_ID_PATTERN.test(request.runId)
    || !DIGEST_PATTERN.test(request.sourceReceiptSha256)) {
    throw observerError("host-observer.request", "host observer request identity is invalid");
  }
  // require one bounded absolute source receipt path when supplied
  if (request.sourceReceiptFile !== undefined
    && (typeof request.sourceReceiptFile !== "string"
      || !isAbsolute(request.sourceReceiptFile)
      || request.sourceReceiptFile.includes("\0")
      || Buffer.byteLength(request.sourceReceiptFile) > 4_096)) {
    throw observerError("host-observer.request", "source receipt path is invalid");
  }
  // bound sample count spacing and total observer lifetime
  if (!Number.isSafeInteger(request.sampleCount) || request.sampleCount < 2
    || request.sampleCount > MAXIMUM_SAMPLES
    || !Number.isSafeInteger(request.sampleSpacingMs) || request.sampleSpacingMs < 1
    || request.sampleSpacingMs > MAXIMUM_SPACING_MS
    || (request.sampleCount - 1) * request.sampleSpacingMs > MAXIMUM_DURATION_MS) {
    throw observerError("host-observer.request", "host observer sampling bounds are invalid");
  }
  return { ...request };
}

// validate one closed injected adapter set
function validateAdapters(adapters, sourceReceiptFile) {
  const keys = ["clock", "filesystem", "host", "sleep"];

  // require every adapter family and no hidden capability
  if (!isRecord(adapters)
    || JSON.stringify(Object.keys(adapters).sort()) !== JSON.stringify(keys)
    || !isRecord(adapters.clock)
    || typeof adapters.clock.wallNow !== "function"
    || typeof adapters.clock.monotonicMilliseconds !== "function"
    || !isRecord(adapters.filesystem)
    || typeof adapters.filesystem.open !== "function"
    || typeof adapters.filesystem.statfs !== "function"
    || (sourceReceiptFile !== undefined
      && typeof adapters.filesystem.realpath !== "function")
    || !isRecord(adapters.host)
    || typeof adapters.host.architecture !== "function"
    || typeof adapters.host.hostname !== "function"
    || typeof adapters.host.kernelRelease !== "function"
    || typeof adapters.host.nodeArchitecture !== "function"
    || typeof adapters.host.platform !== "function"
    || typeof adapters.sleep !== "function") {
    throw observerError("host-observer.adapters", "host observer adapters are invalid");
  }
  return adapters;
}

// provide the physical read-only host adapters
function physicalAdapters() {
  return {
    clock: {
      // read one monotonic millisecond coordinate
      monotonicMilliseconds: () => Number(process.hrtime.bigint() / 1_000_000n),
      // read one wall-clock coordinate
      wallNow: () => new Date(),
    },
    filesystem: { open, realpath, statfs },
    host: {
      // read one kernel architecture identity
      architecture: () => machine(),
      // read one host name identity
      hostname: () => hostname(),
      // read one kernel release identity
      kernelRelease: () => release(),
      // read one node architecture identity
      nodeArchitecture: () => arch(),
      // retain the fixed supported platform
      platform: () => process.platform,
    },
    // wait without creating host workload
    sleep: (milliseconds) => delay(milliseconds),
  };
}

// inspect one exact proc filesystem mount
async function inspectProcfs(filesystem) {
  const details = await filesystem.statfs("/proc", { bigint: true });
  const filesystemType = typeof details?.type === "bigint"
    ? details.type
    : typeof details?.type === "number" ? BigInt(details.type) : null;

  // require the Linux proc filesystem magic
  if (filesystemType !== PROC_SUPER_MAGIC) {
    throw observerError("host-observer.procfs", "/proc is not a proc filesystem");
  }
  return {
    filesystemType: `0x${filesystemType.toString(16)}`,
    path: "/proc",
  };
}

// read one exact host and observer identity set
async function readHostIdentity(request, adapterSet) {
  const [boot, machineId, observer] = await Promise.all([
    readBoundedFile(adapterSet.filesystem, BOOT_ID_PATH),
    readBoundedFile(adapterSet.filesystem, MACHINE_ID_PATH),
    readBoundedFile(adapterSet.filesystem, OBSERVER_PATH),
  ]);
  const bootId = decodeText(boot, BOOT_ID_PATH).trim();
  const rawMachineId = decodeText(machineId, MACHINE_ID_PATH).trim();
  const identity = {
    architecture: adapterSet.host.architecture(),
    bootId,
    hostname: adapterSet.host.hostname(),
    kernelRelease: adapterSet.host.kernelRelease(),
    machineIdSha256: sha256Bytes(Buffer.from(rawMachineId)),
    nodeArchitecture: adapterSet.host.nodeArchitecture(),
    observerSha256: sha256Bytes(observer.bytes),
    platform: adapterSet.host.platform(),
  };

  // require actual Linux identities and the requested capture boundary
  if (identity.platform !== "linux"
    || !ARCHITECTURE_PATTERN.test(identity.architecture)
    || !ARCHITECTURE_PATTERN.test(identity.nodeArchitecture)
    || !BOOT_ID_PATTERN.test(bootId)
    || !MACHINE_ID_PATTERN.test(rawMachineId)
    || !HOSTNAME_PATTERN.test(identity.hostname)
    || identity.architecture !== request.expectedArchitecture
    || bootId !== request.expectedBootId
    || identity.hostname !== request.expectedHostname
    || identity.observerSha256 !== request.expectedObserverSha256) {
    throw observerError("host-observer.identity", "host or observer identity differs from the request");
  }
  return {
    identity: {
      ...identity,
      sha256: sha256Value(identity),
    },
    sources: {
      bootId: { ...boot.identity, sha256: sha256Bytes(boot.bytes) },
      machineId: { ...machineId.identity, sha256: sha256Bytes(machineId.bytes) },
      observer: { ...observer.identity, sha256: identity.observerSha256 },
    },
  };
}

// validate one bound live observation against one host identity
function validateSourceReceipt(observation, path, hostObservation) {
  let receipt;

  try {
    receipt = JSON.parse(decodeText(observation, path));
  } catch (error) {
    // retain exact decoding failures
    if (typeof error?.code === "string" && error.code.startsWith("host-observer.")) {
      throw error;
    }
    throw observerError(
      "host-observer.source-receipt",
      "source receipt is not valid JSON",
      error,
    );
  }
  const sourceHost = receipt?.host;

  // require the actual stable read-only live observation contract
  if (!isRecord(receipt)
    || receipt.contractVersion !== BACKEND_DIRECT_LIVE_OBSERVATION_VERSION
    || receipt.evidenceClass !== "actual-read-only-host-observation"
    || receipt.readOnly !== true
    || receipt.allIdentitiesStable !== true
    || !isRecord(sourceHost)) {
    throw observerError(
      "host-observer.source-receipt",
      "source receipt contract or stability assertion differs",
    );
  }
  // bind every required host identity to the directly observed opening or closing source
  if (sourceHost.architecture !== hostObservation.identity.architecture
    || sourceHost.bootId !== hostObservation.identity.bootId
    || sourceHost.hostname !== hostObservation.identity.hostname
    || sourceHost.kernel !== hostObservation.identity.kernelRelease
    || sourceHost.machineIdSha256 !== hostObservation.sources.machineId.sha256) {
    throw observerError(
      "host-observer.source-receipt",
      "source receipt host identity differs from the direct observation",
    );
  }
  return receipt;
}

// read and validate one optional bound source receipt
async function readSourceReceipt(configuration, adapterSet, hostObservation) {
  // preserve the digest-only source contract when no path was supplied
  if (configuration.sourceReceiptFile === undefined) {
    return null;
  }
  const observation = await readPrivateSourceReceipt(
    adapterSet.filesystem,
    configuration.sourceReceiptFile,
    configuration.sourceReceiptSha256,
  );
  validateSourceReceipt(observation, configuration.sourceReceiptFile, hostObservation);
  return observation;
}

// capture one accounting-compatible proc snapshot and its custody window
async function captureSnapshot(adapterSet, index) {
  const opened = readClock(adapterSet.clock);
  const [procStat, meminfo] = await Promise.all([
    readBoundedFile(adapterSet.filesystem, PROC_STAT_PATH),
    readBoundedFile(adapterSet.filesystem, MEMINFO_PATH),
  ]);
  const closed = readClock(adapterSet.clock);

  // require increasing clocks across one capture window
  if (Date.parse(closed.capturedAt) < Date.parse(opened.capturedAt)
    || closed.monotonicMilliseconds < opened.monotonicMilliseconds) {
    throw observerError("host-observer.clock", "snapshot clock moved backwards");
  }
  return {
    input: {
      capturedAt: closed.capturedAt,
      meminfo: decodeText(meminfo, MEMINFO_PATH),
      monotonicMilliseconds: closed.monotonicMilliseconds,
      procStat: decodeText(procStat, PROC_STAT_PATH),
    },
    provenance: {
      closedAt: closed,
      index,
      meminfo: { ...meminfo.identity, sha256: sha256Bytes(meminfo.bytes) },
      openedAt: opened,
      procStat: { ...procStat.identity, sha256: sha256Bytes(procStat.bytes) },
    },
  };
}

// collect one bounded read-only host accounting observation
export async function collectBackendPerformanceDirectHostObservation(request, adapters) {
  const configuration = validateRequest(request);
  const injected = adapters !== undefined;
  const adapterSet = injected
    ? validateAdapters(adapters, configuration.sourceReceiptFile)
    : physicalAdapters();
  const procfs = await inspectProcfs(adapterSet.filesystem);
  const startedAt = readClock(adapterSet.clock);
  const openingIdentity = await readHostIdentity(configuration, adapterSet);
  const openingSourceReceipt = await readSourceReceipt(
    configuration,
    adapterSet,
    openingIdentity,
  );
  const snapshots = [];
  const provenance = [];

  // capture every requested sample serially
  for (let index = 0; index < configuration.sampleCount; index += 1) {
    // wait only between samples
    if (index > 0) {
      await adapterSet.sleep(configuration.sampleSpacingMs);
    }
    const snapshot = await captureSnapshot(adapterSet, index);
    snapshots.push(snapshot.input);
    provenance.push(snapshot.provenance);
  }
  const closingSourceReceipt = await readSourceReceipt(
    configuration,
    adapterSet,
    openingIdentity,
  );
  const closingIdentity = await readHostIdentity(configuration, adapterSet);
  const completedAt = readClock(adapterSet.clock);

  // reject a host boot source or observer change across collection
  if (openingIdentity.identity.sha256 !== closingIdentity.identity.sha256
    || !sameIdentity(openingIdentity.sources.bootId, closingIdentity.sources.bootId)
    || !sameIdentity(openingIdentity.sources.machineId, closingIdentity.sources.machineId)
    || !sameIdentity(openingIdentity.sources.observer, closingIdentity.sources.observer)) {
    throw observerError("host-observer.identity-drift", "host source identity changed during collection");
  }
  // reject a source receipt byte or filesystem identity change across collection
  if ((openingSourceReceipt === null) !== (closingSourceReceipt === null)
    || (openingSourceReceipt !== null
      && (!sameIdentity(openingSourceReceipt.identity, closingSourceReceipt.identity)
        || !openingSourceReceipt.bytes.equals(closingSourceReceipt.bytes)))) {
    throw observerError(
      "host-observer.source-receipt-drift",
      "source receipt changed during collection",
    );
  }
  // bind the closing source receipt to the closing direct host identity
  if (closingSourceReceipt !== null) {
    validateSourceReceipt(
      closingSourceReceipt,
      configuration.sourceReceiptFile,
      closingIdentity,
    );
  }
  const accountingInput = {
    clockId: `linux-monotonic-boot:${openingIdentity.identity.bootId}`,
    contractVersion: BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_INPUT_VERSION,
    snapshots,
  };
  const accountingResult = evaluateBackendPerformanceDirectHostAccounting(accountingInput);
  // distinguish legacy digest claims from direct private-file custody
  const sourceBinding = openingSourceReceipt === null
    ? {
      kind: "caller-supplied-live-status-receipt-digest",
      sha256: configuration.sourceReceiptSha256,
      verifiedByObserver: false,
    }
    : {
      fileIdentity: openingSourceReceipt.identity,
      kind: "exact-private-live-observation-file",
      path: configuration.sourceReceiptFile,
      sha256: configuration.sourceReceiptSha256,
      verifiedByObserver: !injected,
    };
  return Object.freeze({
    accountingInput,
    accountingInputSha256: sha256Value(accountingInput),
    accountingResult,
    accountingResultSha256: sha256Value(accountingResult),
    authority: {
      captureKind: injected
        ? "injected-test-only"
        : "physical-linux-procfs-read-only",
      nativeEligible: false,
      physicalCapture: !injected,
      readOnly: true,
      trafficGenerated: false,
    },
    completedAt: completedAt.capturedAt,
    contractVersion: BACKEND_PERFORMANCE_DIRECT_HOST_OBSERVER_VERSION,
    hostIdentity: openingIdentity.identity,
    procfs,
    run: {
      runId: configuration.runId,
      sampleCount: configuration.sampleCount,
      sampleSpacingMs: configuration.sampleSpacingMs,
    },
    snapshotProvenance: provenance,
    sourceBinding,
    sourceFiles: openingIdentity.sources,
    startedAt: startedAt.capturedAt,
  });
}

// parse one fixed ordered host observer cli
export function parseBackendPerformanceDirectHostObserverCli(argv) {
  // require one fixed command and exact ordered flag count
  if (!Array.isArray(argv)
    || (argv.length !== 17 && argv.length !== 19)
    || argv[0] !== "collect") {
    throw observerError(
      "host-observer.cli",
      "usage: collect --run-id ID --source-receipt-sha256 SHA256 --expected-observer-sha256 SHA256 --expected-architecture ARCH --expected-boot-id UUID --expected-hostname HOST --sample-count N --sample-spacing-ms N [--source-receipt-file ABSOLUTE_PATH]",
    );
  }
  const request = {
    contractVersion: BACKEND_PERFORMANCE_DIRECT_HOST_OBSERVER_REQUEST_VERSION,
  };

  // consume every fixed flag-value pair
  for (let index = 0; index < CLI_FIELDS.length; index += 1) {
    const [flag, field] = CLI_FIELDS[index];
    const argumentIndex = 1 + index * 2;

    // reject reordered or duplicated flags
    if (argv[argumentIndex] !== flag) {
      throw observerError("host-observer.cli", `expected ${flag}`);
    }
    const rawValue = argv[argumentIndex + 1];
    request[field] = field === "sampleCount" || field === "sampleSpacingMs"
      ? Number(rawValue)
      : rawValue;
  }
  // accept the source receipt path only as the final fixed optional pair
  if (argv.length === 19) {
    // reject any reordered optional flag
    if (argv[17] !== "--source-receipt-file") {
      throw observerError("host-observer.cli", "expected --source-receipt-file");
    }
    request.sourceReceiptFile = argv[18];
  }
  return validateRequest(request);
}

// run one physical read-only host observer cli
async function runCli() {
  const request = parseBackendPerformanceDirectHostObserverCli(process.argv.slice(2));
  const result = await collectBackendPerformanceDirectHostObservation(request);
  process.stdout.write(`${JSON.stringify(result)}\n`);
}

// execute only for direct cli invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runCli().catch(
    // emit one bounded diagnostic without raw proc bytes
    (error) => {
      process.stderr.write(`${JSON.stringify({
        code: typeof error?.code === "string" ? error.code : "host-observer.failed",
        message: typeof error?.message === "string" ? error.message : "host observer failed",
      })}\n`);
      process.exitCode = 1;
    },
  );
}
