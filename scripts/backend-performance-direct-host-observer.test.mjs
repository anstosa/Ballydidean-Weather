import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_DIRECT_HOST_OBSERVER_REQUEST_VERSION,
  collectBackendPerformanceDirectHostObservation,
  parseBackendPerformanceDirectHostObserverCli,
} from "./backend-performance-direct-host-observer.mjs";

const BOOT_ID = "01234567-89ab-cdef-0123-456789abcdef";
const MACHINE_ID = "0123456789abcdef0123456789abcdef";
const SOURCE_RECEIPT_SHA256 = "a".repeat(64);
const OBSERVER_SOURCE = Buffer.from("fixture-observer-source\n");
const SOURCE_RECEIPT_FILE = "/evidence/live-observation.json";
const KERNEL_RELEASE = "6.12.47+rpt-rpi-v8";

// hash one fixture byte sequence
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// build one exact observer request
function request(overrides = {}) {
  return {
    contractVersion: BACKEND_PERFORMANCE_DIRECT_HOST_OBSERVER_REQUEST_VERSION,
    expectedArchitecture: "aarch64",
    expectedBootId: BOOT_ID,
    expectedHostname: "blueberry",
    expectedObserverSha256: sha256(OBSERVER_SOURCE),
    runId: "fixture-run-01",
    sampleCount: 2,
    sampleSpacingMs: 25,
    sourceReceiptSha256: SOURCE_RECEIPT_SHA256,
    ...overrides,
  };
}

// build one exact live observation source receipt
function sourceReceipt(overrides = {}) {
  return Buffer.from(JSON.stringify({
    allIdentitiesStable: true,
    contractVersion: "weather-backend-direct-live-observation/v1",
    evidenceClass: "actual-read-only-host-observation",
    host: {
      architecture: "aarch64",
      bootId: BOOT_ID,
      hostname: "blueberry",
      kernel: KERNEL_RELEASE,
      machineIdSha256: sha256(Buffer.from(`${MACHINE_ID}\n`)),
    },
    readOnly: true,
    ...overrides,
  }));
}

// build one request bound to exact source receipt bytes
function boundRequest(receipt, overrides = {}) {
  return request({
    sourceReceiptFile: SOURCE_RECEIPT_FILE,
    sourceReceiptSha256: sha256(receipt),
    ...overrides,
  });
}

// build one accounting-compatible proc stat fixture
function procStat(userTicks) {
  return [
    `cpu ${String(userTicks)} 10 50 1000 20 5 15 2 4 1`,
    "cpu0 1 0 1 1 0 0 0 0 0 0",
    "intr 1",
    "",
  ].join("\n");
}

// build one accounting-compatible meminfo fixture
function meminfo(freeKiB, availableKiB) {
  return [
    "MemTotal:        8000 kB",
    `MemFree:         ${String(freeKiB)} kB`,
    `MemAvailable:    ${String(availableKiB)} kB`,
    "SwapTotal:       1000 kB",
    "SwapFree:        800 kB",
    "Buffers:         100 kB",
    "",
  ].join("\n");
}

// build one bounded in-memory descriptor filesystem
function fixtureFilesystem({
  driftBootIdentity = false,
  driftSourceReceiptIdentity = false,
  procfsType = 0x9fa0n,
  sourceReceiptBytes,
} = {}) {
  const opens = new Map();
  const values = {
    "/etc/machine-id": [Buffer.from(`${MACHINE_ID}\n`)],
    "/proc/meminfo": [Buffer.from(meminfo(2_000, 3_000)), Buffer.from(meminfo(1_500, 2_500))],
    "/proc/stat": [Buffer.from(procStat(100)), Buffer.from(procStat(130))],
    "/proc/sys/kernel/random/boot_id": [Buffer.from(`${BOOT_ID}\n`)],
  };

  // expose only explicitly supplied source receipt bytes
  if (sourceReceiptBytes !== undefined) {
    values[SOURCE_RECEIPT_FILE] = Array.isArray(sourceReceiptBytes)
      ? sourceReceiptBytes
      : [sourceReceiptBytes];
  }

  // select one bounded value for an exact opened path
  function valueFor(path, openIndex) {
    // supply the observer's own source bytes without relying on its local path
    if (path.endsWith("/backend-performance-direct-host-observer.mjs")) {
      return OBSERVER_SOURCE;
    }
    const candidates = values[path];

    // reject unexpected physical reads in the fixture
    if (candidates === undefined) {
      throw new Error(`unexpected fixture path ${path}`);
    }
    return candidates[Math.min(openIndex, candidates.length - 1)];
  }

  return {
    // open one fixed in-memory descriptor
    async open(path) {
      const openIndex = opens.get(path) ?? 0;
      opens.set(path, openIndex + 1);
      const bytes = valueFor(path, openIndex);
      let offset = 0;
      const inode = (driftBootIdentity
          && path === "/proc/sys/kernel/random/boot_id" && openIndex > 0)
        || (driftSourceReceiptIdentity
          && path === SOURCE_RECEIPT_FILE && openIndex > 0)
        ? 99n
        : 10n;
      return {
        // close one inert fixture descriptor
        async close() {},
        // read one bounded fixture chunk
        async read(buffer, bufferOffset, length) {
          const count = Math.min(length, bytes.length - offset);

          // copy one remaining fixture chunk
          if (count > 0) {
            bytes.copy(buffer, bufferOffset, offset, offset + count);
            offset += count;
          }
          return { bytesRead: count };
        },
        // expose one stable regular-file identity
        async stat() {
          // model the stronger private custody required for source receipts
          if (path === SOURCE_RECEIPT_FILE) {
            return {
              ctimeNs: 1_000n,
              dev: 1n,
              ino: inode,
              isFile: () => true,
              mode: 0o100600n,
              mtimeNs: 1_000n,
              nlink: 1n,
              size: BigInt(bytes.length),
              uid: BigInt(process.geteuid()),
            };
          }
          return {
            dev: 1n,
            ino: inode,
            isFile: () => true,
            mode: 33_188n,
            nlink: 1n,
          };
        },
      };
    },
    // retain each fixture path as its canonical name
    async realpath(path) {
      return path;
    },
    // identify the fixture proc mount
    async statfs() {
      return { type: procfsType };
    },
  };
}

// build one deterministic injected adapter set
function fixtureAdapters(options = {}) {
  let clockIndex = 0;
  const sleeps = [];
  return {
    adapters: {
      clock: {
        // advance one deterministic monotonic coordinate
        monotonicMilliseconds() {
          return 1_000 + clockIndex * 100;
        },
        // advance one deterministic wall coordinate
        wallNow() {
          const value = new Date(1_795_456_800_000 + clockIndex * 100);
          clockIndex += 1;
          return value;
        },
      },
      filesystem: fixtureFilesystem(options),
      host: {
        architecture: () => "aarch64",
        hostname: () => "blueberry",
        kernelRelease: () => KERNEL_RELEASE,
        nodeArchitecture: () => "arm64",
        platform: () => "linux",
      },
      // record one requested sample wait
      sleep: async (milliseconds) => {
        sleeps.push(milliseconds);
      },
    },
    sleeps,
  };
}

test("collects accounting-compatible snapshots with separate injected provenance", async () => {
  const fixture = fixtureAdapters();
  const result = await collectBackendPerformanceDirectHostObservation(
    request(),
    fixture.adapters,
  );

  assert.deepEqual(result.authority, {
    captureKind: "injected-test-only",
    nativeEligible: false,
    physicalCapture: false,
    readOnly: true,
    trafficGenerated: false,
  });
  assert.equal(result.hostIdentity.architecture, "aarch64");
  assert.equal(result.hostIdentity.bootId, BOOT_ID);
  assert.equal(result.hostIdentity.hostname, "blueberry");
  assert.equal(result.hostIdentity.machineIdSha256, sha256(Buffer.from(MACHINE_ID)));
  assert.equal(result.hostIdentity.observerSha256, sha256(OBSERVER_SOURCE));
  assert.equal(result.procfs.filesystemType, "0x9fa0");
  assert.equal(result.accountingInput.snapshots.length, 2);
  assert.equal(result.snapshotProvenance.length, 2);
  assert.equal(result.accountingResult.cpu.aggregate.busyTicksGuestNotAddedSeparately, "30");
  assert.equal(result.accountingResult.authority.nativeEligible, false);
  assert.deepEqual(result.sourceBinding, {
    kind: "caller-supplied-live-status-receipt-digest",
    sha256: SOURCE_RECEIPT_SHA256,
    verifiedByObserver: false,
  });
  assert.deepEqual(fixture.sleeps, [25]);
});

test("rejects host identity drift and non-proc filesystem substitution", async () => {
  const drift = fixtureAdapters({ driftBootIdentity: true });
  await assert.rejects(
    collectBackendPerformanceDirectHostObservation(request(), drift.adapters),
    (error) => error?.code === "host-observer.identity-drift",
  );

  const wrongFilesystem = fixtureAdapters({ procfsType: 0x1234n });
  await assert.rejects(
    collectBackendPerformanceDirectHostObservation(request(), wrongFilesystem.adapters),
    (error) => error?.code === "host-observer.procfs",
  );
});

test("rejects mismatched requested host and observer identities", async () => {
  const fixture = fixtureAdapters();
  await assert.rejects(
    collectBackendPerformanceDirectHostObservation(
      request({ expectedHostname: "not-blueberry" }),
      fixture.adapters,
    ),
    (error) => error?.code === "host-observer.identity",
  );

  const sourceFixture = fixtureAdapters();
  await assert.rejects(
    collectBackendPerformanceDirectHostObservation(
      request({ expectedObserverSha256: "b".repeat(64) }),
      sourceFixture.adapters,
    ),
    (error) => error?.code === "host-observer.identity",
  );
});

test("binds matching source bytes but never verifies injected custody", async () => {
  const receipt = sourceReceipt();
  const fixture = fixtureAdapters({ sourceReceiptBytes: receipt });
  const result = await collectBackendPerformanceDirectHostObservation(
    boundRequest(receipt),
    fixture.adapters,
  );

  assert.equal(result.sourceBinding.kind, "exact-private-live-observation-file");
  assert.equal(result.sourceBinding.sha256, sha256(receipt));
  assert.equal(result.sourceBinding.path, SOURCE_RECEIPT_FILE);
  assert.equal(result.sourceBinding.fileIdentity.mode, String(0o100600n));
  assert.equal(result.sourceBinding.fileIdentity.linkCount, "1");
  assert.equal(result.sourceBinding.verifiedByObserver, false);
  assert.equal(result.authority.nativeEligible, false);
  assert.equal(result.authority.physicalCapture, false);
});

test("rejects a source receipt with a different raw machine-id source", async () => {
  const receipt = sourceReceipt({
    host: {
      architecture: "aarch64",
      bootId: BOOT_ID,
      hostname: "blueberry",
      kernel: KERNEL_RELEASE,
      machineIdSha256: sha256(Buffer.from(MACHINE_ID)),
    },
  });
  const fixture = fixtureAdapters({ sourceReceiptBytes: receipt });

  await assert.rejects(
    collectBackendPerformanceDirectHostObservation(
      boundRequest(receipt),
      fixture.adapters,
    ),
    (error) => error?.code === "host-observer.source-receipt",
  );
});

test("rejects a source receipt digest mismatch", async () => {
  const receipt = sourceReceipt();
  const fixture = fixtureAdapters({ sourceReceiptBytes: receipt });

  await assert.rejects(
    collectBackendPerformanceDirectHostObservation(
      boundRequest(receipt, { sourceReceiptSha256: "b".repeat(64) }),
      fixture.adapters,
    ),
    (error) => error?.code === "host-observer.source-receipt-digest",
  );
});

test("rejects source receipt identity drift across the interval", async () => {
  const receipt = sourceReceipt();
  const fixture = fixtureAdapters({
    driftSourceReceiptIdentity: true,
    sourceReceiptBytes: receipt,
  });

  await assert.rejects(
    collectBackendPerformanceDirectHostObservation(
      boundRequest(receipt),
      fixture.adapters,
    ),
    (error) => error?.code === "host-observer.source-receipt-drift",
  );
});

test("parses only the fixed reproducible physical cli", () => {
  const argv = [
    "collect",
    "--run-id", "fixture-run-01",
    "--source-receipt-sha256", SOURCE_RECEIPT_SHA256,
    "--expected-observer-sha256", sha256(OBSERVER_SOURCE),
    "--expected-architecture", "aarch64",
    "--expected-boot-id", BOOT_ID,
    "--expected-hostname", "blueberry",
    "--sample-count", "2",
    "--sample-spacing-ms", "25",
  ];
  const parsed = parseBackendPerformanceDirectHostObserverCli(argv);

  assert.deepEqual(parsed, request());
  assert.deepEqual(
    parseBackendPerformanceDirectHostObserverCli([
      ...argv,
      "--source-receipt-file", SOURCE_RECEIPT_FILE,
    ]),
    request({ sourceReceiptFile: SOURCE_RECEIPT_FILE }),
  );
  assert.throws(
    () => parseBackendPerformanceDirectHostObserverCli([
      ...argv.slice(0, 1),
      "--expected-hostname", "blueberry",
      ...argv.slice(3),
    ]),
    (error) => error?.code === "host-observer.cli",
  );
  assert.throws(
    () => parseBackendPerformanceDirectHostObserverCli([
      ...argv.slice(0, -1),
      "0",
    ]),
    (error) => error?.code === "host-observer.request",
  );
});
