import { pathToFileURL } from "node:url";

import {
  BACKEND_PERFORMANCE_DIRECT_APP_AGGREGATE_INPUT_VERSION,
  evaluateBackendPerformanceDirectAppAggregate,
} from "./backend-performance-direct-app-aggregate.mjs";

const SOURCE_SHA256 = "a".repeat(64);
const ROOT = { device: "8", inode: "100", path: "/sys/fs/cgroup" };

// build one compact disposable service interval
function service(scopeId, digest, usageUsec, values, peak) {
  const userUsec = Math.floor(usageUsec * 0.75);
  return {
    accounting: {
      aggregateMethod: "direct-single-cgroup",
      coverage: "measured-process-cgroup-only",
      sampledServiceSumUsed: false,
      unsupportedScopes: ["app-aggregate", "host"],
    },
    accountingScope: "service",
    after: { cgroupId: digest, releaseId: "node-v1" },
    authority: {
      kind: "injected-test-only",
      liveEligible: false,
      nativeCgroup: {},
      scopeCoverage: "direct-process-cgroup-only",
      unsupportedScopes: ["app-aggregate", "host"],
    },
    before: { cgroupId: digest, releaseId: "node-v1" },
    cgroupIdentity: {
      group: { device: "8", inode: digest.slice(0, 4), path: `/sys/fs/cgroup/weather/${scopeId}` },
      root: structuredClone(ROOT),
      sha256: digest,
    },
    completedAt: "2026-11-23T20:00:01.000Z",
    contractVersion: "weather-backend-resource-interval/v1",
    cpu: {
      nrPeriods: 10,
      nrThrottled: 1,
      systemUsec: usageUsec - userUsec,
      throttledUsec: 5,
      usageUsec,
      userUsec,
    },
    memory: {
      peak: {
        readAt: "2026-11-23T20:00:01.000Z",
        resetAt: "2026-11-23T20:00:00.000Z",
        scope: "private-retained-rw-fd",
        source: "cgroup-memory-peak-reset",
        valueBytes: peak,
      },
      steady: {
        sampledMaximumBytes: Math.max(...values),
        samples: values.map(
          // create one exact shared fixture wave
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

// run one deterministic application aggregate fixture
export function runBackendPerformanceDirectAppAggregateCheck() {
  return evaluateBackendPerformanceDirectAppAggregate({
    contractVersion: BACKEND_PERFORMANCE_DIRECT_APP_AGGREGATE_INPUT_VERSION,
    exclusiveParentInterval: null,
    serviceIntervals: [
      service("api", "b".repeat(64), 100, [100, 150, 120], 200),
      service("web", "c".repeat(64), 200, [50, 60, 55], 90),
      service("worker", "d".repeat(64), 50, [20, 30, 25], 40),
    ],
  });
}

// print only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.stdout.write(`${JSON.stringify(runBackendPerformanceDirectAppAggregateCheck(), null, 2)}\n`);
  } catch (error) {
    process.stderr.write(`${error?.code ?? "direct-app-aggregate.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}
