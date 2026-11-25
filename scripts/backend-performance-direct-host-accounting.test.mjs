import assert from "node:assert/strict";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_INPUT_VERSION,
  evaluateBackendPerformanceDirectHostAccounting,
} from "./backend-performance-direct-host-accounting.mjs";

// build one deterministic raw host snapshot
function snapshot(capturedAt, monotonicMilliseconds, cpu, memory) {
  return {
    capturedAt,
    meminfo: [
      `MemTotal:        ${String(memory.total)} kB`,
      `MemFree:         ${String(memory.free)} kB`,
      `MemAvailable:    ${String(memory.available)} kB`,
      `SwapTotal:       ${String(memory.swapTotal)} kB`,
      `SwapFree:        ${String(memory.swapFree)} kB`,
      "Buffers:         100 kB",
      "",
    ].join("\n"),
    monotonicMilliseconds,
    procStat: [
      `cpu  ${cpu.join(" ")}`,
      "cpu0 1 0 1 1 0 0 0 0 0 0",
      "intr 1",
      "",
    ].join("\n"),
  };
}

// build one complete ordered host batch
function batch() {
  return {
    clockId: "fixture-host-monotonic-clock",
    contractVersion: BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_INPUT_VERSION,
    snapshots: [
      snapshot(
        "2026-11-23T20:00:00.000Z",
        1_000,
        [100, 10, 50, 1000, 20, 5, 15, 2, 4, 1],
        { available: 3000, free: 2000, swapFree: 800, swapTotal: 1000, total: 8000 },
      ),
      snapshot(
        "2026-11-23T20:00:01.000Z",
        2_000,
        [130, 12, 65, 1030, 25, 7, 18, 3, 6, 1],
        { available: 2500, free: 1500, swapFree: 700, swapTotal: 1000, total: 8000 },
      ),
    ],
  };
}

test("calculates exact aggregate CPU and sampled host RAM without guest double count", () => {
  const result = evaluateBackendPerformanceDirectHostAccounting(batch());

  assert.equal(result.cpu.aggregate.accountedTicksExcludingGuestAddition, "88");
  assert.equal(result.cpu.aggregate.busyTicksGuestNotAddedSeparately, "53");
  assert.equal(result.cpu.aggregate.idleTicks, "30");
  assert.equal(result.cpu.aggregate.iowaitTicks, "5");
  assert.equal(result.cpu.aggregate.guestCountersAlreadyIncluded, "2");
  assert.deepEqual(result.cpu.aggregate.busyFraction, {
    denominatorTicks: "88",
    numeratorTicks: "53",
  });
  assert.equal(result.cpu.intervals.length, 1);
  assert.equal(result.cpu.intervals[0].elapsedMonotonicMilliseconds, 1_000);
  assert.deepEqual(result.memory.unavailableEstimate.arithmeticMeanBytes, {
    denominator: "2",
    floor: "5376000",
    numerator: "10752000",
    remainder: "0",
  });
  assert.equal(result.memory.unavailableEstimate.maximumBytes, "5632000");
  assert.equal(result.memory.nonFree.maximumBytes, "6656000");
  assert.equal(result.swap.used.maximumBytes, "307200");
  assert.deepEqual(result.memory.trueKernelPeak, {
    reason: "standard proc and the root cgroup expose no resettable whole-host RAM peak",
    supported: false,
    valueBytes: null,
  });
  assert.equal(result.authority.nativeEligible, false);
});

test("rejects decreasing aggregate counters", () => {
  const input = batch();
  input.snapshots[1].procStat = "cpu  99 12 65 1030 25 7 18 3 6 1\n";

  assert.throws(
    () => evaluateBackendPerformanceDirectHostAccounting(input),
    (error) => error?.code === "host-accounting.counter-decrease"
      && /user decreased/u.test(error.message),
  );
});

test("rejects unordered snapshots and non-kB memory units", () => {
  const unordered = batch();
  unordered.snapshots[1].monotonicMilliseconds = 1_000;
  assert.throws(
    () => evaluateBackendPerformanceDirectHostAccounting(unordered),
    (error) => error?.code === "host-accounting.order",
  );

  const wrongUnit = batch();
  wrongUnit.snapshots[0].meminfo = wrongUnit.snapshots[0].meminfo.replace(
    "MemAvailable:    3000 kB",
    "MemAvailable:    3000 MB",
  );
  assert.throws(
    () => evaluateBackendPerformanceDirectHostAccounting(wrongUnit),
    (error) => error?.code === "host-accounting.meminfo-unit",
  );
});
