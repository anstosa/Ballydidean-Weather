import assert from "node:assert/strict";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_DIRECT_APP_AGGREGATE_INPUT_VERSION,
  evaluateBackendPerformanceDirectAppAggregate,
} from "./backend-performance-direct-app-aggregate.mjs";

const SOURCE_SHA256 = "a".repeat(64);
const ROOT = { device: "8", inode: "100", path: "/sys/fs/cgroup" };

// build one direct service interval fixture
function interval(scopeId, suffix, cpuUsageUsec, samples, peakValueBytes, native = false) {
  const cgroupSha256 = suffix.repeat(64).slice(0, 64);
  const userUsec = Math.floor(cpuUsageUsec * 0.75);
  const systemUsec = cpuUsageUsec - userUsec;
  return {
    accounting: {
      aggregateMethod: "direct-single-cgroup",
      coverage: "measured-process-cgroup-only",
      sampledServiceSumUsed: false,
      unsupportedScopes: ["app-aggregate", "host"],
    },
    accountingScope: "service",
    after: { cgroupId: cgroupSha256, releaseId: "node-v1" },
    authority: native
      ? {
        kind: "native-linux-cgroup-v2",
        liveEligible: true,
        nativeCgroup: { mountId: "42" },
        scopeCoverage: "direct-process-cgroup-only",
        unsupportedScopes: ["app-aggregate", "host"],
      }
      : {
        kind: "injected-test-only",
        liveEligible: false,
        nativeCgroup: {},
        scopeCoverage: "direct-process-cgroup-only",
        unsupportedScopes: ["app-aggregate", "host"],
      },
    before: { cgroupId: cgroupSha256, releaseId: "node-v1" },
    cgroupIdentity: {
      group: { device: "8", inode: suffix, path: `/sys/fs/cgroup/weather/${scopeId}` },
      root: structuredClone(ROOT),
      sha256: cgroupSha256,
    },
    completedAt: "2026-11-23T20:00:01.000Z",
    contractVersion: "weather-backend-resource-interval/v1",
    cpu: {
      nrPeriods: 10,
      nrThrottled: 1,
      systemUsec,
      throttledUsec: 5,
      usageUsec: cpuUsageUsec,
      userUsec,
    },
    memory: {
      peak: {
        readAt: "2026-11-23T20:00:01.000Z",
        resetAt: "2026-11-23T20:00:00.000Z",
        scope: "private-retained-rw-fd",
        source: "cgroup-memory-peak-reset",
        valueBytes: peakValueBytes,
      },
      steady: {
        sampledMaximumBytes: Math.max(...samples),
        samples: samples.map(
          // align one exact deterministic sampling wave
          (memoryCurrentBytes, index) => ({
            memoryCurrentBytes,
            observedAt: `2026-11-23T20:00:00.${String(index * 100).padStart(3, "0")}Z`,
            offsetMs: index * 100,
          }),
        ),
      },
    },
    scopeId,
    sourceBinding: {
      kind: "integration-supplied-live-receipt-binding",
      sha256: SOURCE_SHA256,
    },
    startedAt: "2026-11-23T20:00:00.000Z",
  };
}

// build one complete three-service aggregate input
function input(native = false) {
  return {
    contractVersion: BACKEND_PERFORMANCE_DIRECT_APP_AGGREGATE_INPUT_VERSION,
    exclusiveParentInterval: null,
    serviceIntervals: [
      interval("api", "b", 100, [100, 150, 120], 200, native),
      interval("web", "c", 200, [50, 60, 55], 90, native),
      interval("worker", "d", 50, [20, 30, 25], 40, native),
    ],
  };
}

test("sums nonoverlapping CPU and aligned steady RAM without summing peaks", () => {
  const result = evaluateBackendPerformanceDirectAppAggregate(input());

  assert.equal(result.cpu.usageUsec, 350);
  assert.equal(result.cpu.userUsec, 262);
  assert.equal(result.cpu.systemUsec, 88);
  assert.deepEqual(result.memory.steady.waves.map(
    // retain each exact aggregate wave value
    (wave) => wave.memoryCurrentBytes,
  ), [170, 240, 200]);
  assert.equal(result.memory.steady.maximumBytes, 240);
  assert.deepEqual(result.memory.steady.arithmeticMeanBytes, {
    denominator: 3,
    floor: 203,
    numerator: 610,
    remainder: 1,
  });
  assert.deepEqual(result.memory.componentKernelPeaks.map(
    // retain individual component peak values only
    (peak) => peak.valueBytes,
  ), [200, 90, 40]);
  assert.equal(result.memory.kernelPeak.componentPeaksSummed, false);
  assert.equal(result.memory.kernelPeak.supported, false);
  assert.equal(result.memory.kernelPeak.valueBytes, null);
  assert.equal(result.authority.nativeEligible, false);
});

test("rejects overlapping cgroups and misaligned sampling waves with exact diagnostics", () => {
  const overlapping = input();
  overlapping.serviceIntervals[1].cgroupIdentity.group.path =
    "/sys/fs/cgroup/weather/api/web";
  assert.throws(
    () => evaluateBackendPerformanceDirectAppAggregate(overlapping),
    (error) => error?.code === "app-aggregate.overlap"
      && error?.path === "$.serviceIntervals",
  );

  const misaligned = input();
  misaligned.serviceIntervals[2].memory.steady.samples[1].offsetMs = 101;
  assert.throws(
    () => evaluateBackendPerformanceDirectAppAggregate(misaligned),
    (error) => error?.code === "app-aggregate.wave-mismatch"
      && error?.path.endsWith("samples[1]"),
  );

  const mismatchedRelease = input();
  mismatchedRelease.serviceIntervals[1].before.releaseId = "rust-v1";
  mismatchedRelease.serviceIntervals[1].after.releaseId = "rust-v1";
  assert.throws(
    () => evaluateBackendPerformanceDirectAppAggregate(mismatchedRelease),
    (error) => error?.code === "app-aggregate.identity-mismatch"
      && error?.path === "$.serviceIntervals[1]",
  );
});

test("uses only an exact native exclusive parent-local peak", () => {
  const value = input(true);
  const parent = interval(
    "application-parent",
    "e",
    400,
    [300, 420, 350],
    500,
    true,
  );
  parent.cgroupIdentity.group.path = "/sys/fs/cgroup/weather";
  value.exclusiveParentInterval = parent;
  const result = evaluateBackendPerformanceDirectAppAggregate(value);

  assert.deepEqual(result.memory.kernelPeak, {
    componentPeaksSummed: false,
    contractPrerequisitesSatisfied: true,
    parentCgroupSha256: "e".repeat(64),
    parentScopeId: "application-parent",
    reason: null,
    source: "exclusive-parent-local-cgroup-memory.peak",
    supported: true,
    valueBytes: 500,
  });
  assert.equal(result.cpu.usageUsec, 350);
  assert.equal(result.memory.steady.maximumBytes, 240);
  assert.equal(result.authority.nativeEligible, false);
});

test("rejects parent peak authority when any contributing interval is injected", () => {
  const value = input();
  const parent = interval(
    "application-parent",
    "e",
    400,
    [300, 420, 350],
    500,
    true,
  );
  parent.cgroupIdentity.group.path = "/sys/fs/cgroup/weather";
  value.exclusiveParentInterval = parent;

  assert.throws(
    () => evaluateBackendPerformanceDirectAppAggregate(value),
    (error) => error?.code === "app-aggregate.parent-authority"
      && error?.path === "$.exclusiveParentInterval",
  );
});
