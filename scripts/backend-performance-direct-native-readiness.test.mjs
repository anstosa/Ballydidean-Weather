import assert from "node:assert/strict";
import test from "node:test";

import {
  assessBackendPerformanceDirectNativeReadiness,
} from "./backend-performance-direct-native-readiness.mjs";
import { receiptSha256 } from "./backend-performance-receipts.mjs";
import {
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";
import {
  BACKEND_PERFORMANCE_DIRECT_APP_AGGREGATE_INPUT_VERSION,
  evaluateBackendPerformanceDirectAppAggregate,
} from "./backend-performance-direct-app-aggregate.mjs";
import {
  BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_INPUT_VERSION,
  evaluateBackendPerformanceDirectHostAccounting,
} from "./backend-performance-direct-host-accounting.mjs";

const HASH_A = "a".repeat(64);
const HASH_B = "b".repeat(64);

// select every approved attempt in one operation-stratum bucket
function sourceAttemptIds(operationId, stratum) {
  const schedule = buildBackendPerformanceFunctionalSchedule();
  return schedule.runSlots.flatMap(
    // retain every approved run slot
    (runSlot) => runSlot.groups.flatMap(
      // retain every approved group
      (group) => group.attempts.filter(
        // select the exact requested bucket
        (attempt) => attempt.operationId === operationId && attempt.stratum === stratum,
      ).map(
        // retain one exact attempt identity
        (attempt) => attempt.attemptId,
      ),
    ),
  );
}

// build one complete local operation receipt
function operationReceipt(operationId, stratum, nativeEligible = true) {
  const attemptIds = sourceAttemptIds(operationId, stratum);
  const count = attemptIds.length;
  return {
    attemptDenominator: { observedAttempts: count, plannedAttempts: count },
    evidenceClass: nativeEligible ? "native-batch" : "synthetic-fixture",
    nativeEligible,
    operationStratum: {
      observedAttempts: count,
      operationId,
      stratum,
    },
    sourceBinding: { sourceAttemptIds: attemptIds },
    terminalCounts: { completed: count, error: 0, timeout: 0 },
  };
}

// build the complete reviewed local operation set
function operationReceipts(nativeEligible = true) {
  return [
    operationReceipt("api.health", "warm", nativeEligible),
    operationReceipt("api.health", "process-pool-cold", nativeEligible),
    operationReceipt("admin.live.read-page", "once-per-run", nativeEligible),
    operationReceipt("provider.open-meteo-current.success", "once-per-run", nativeEligible),
    operationReceipt("inference.wind.load-serve", "once-per-run", nativeEligible),
  ];
}

// build one identity-complete published interval fixture
function interval(accountingScope, scopeId, sourceSha256, native = true) {
  const identityDigit = {
    api: "c",
    postgres: "f",
    web: "d",
    worker: "e",
  }[scopeId];
  const cgroupSha256 = identityDigit.repeat(64);
  const authority = native
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
    };
  return {
    accounting: {
      aggregateMethod: "direct-single-cgroup",
      coverage: "measured-process-cgroup-only",
      sampledServiceSumUsed: false,
      unsupportedScopes: ["app-aggregate", "host"],
    },
    accountingScope,
    after: { cgroupId: cgroupSha256, observedAt: "2026-11-23T20:00:01.000Z", pid: 10, releaseId: "node-v1" },
    authority,
    before: { cgroupId: cgroupSha256, observedAt: "2026-11-23T20:00:00.000Z", pid: 10, releaseId: "node-v1" },
    cgroupIdentity: {
      group: { device: "8", inode: scopeId, path: `/weather/application/${scopeId}` },
      root: { device: "8", inode: "root", path: "/weather" },
      sha256: cgroupSha256,
    },
    completedAt: "2026-11-23T20:00:01.000Z",
    contractVersion: "weather-backend-resource-interval/v1",
    cpu: {
      nrPeriods: 1,
      nrThrottled: 0,
      systemUsec: 2,
      throttledUsec: 0,
      usageUsec: 5,
      userUsec: 3,
    },
    memory: {
      peak: {
        readAt: "2026-11-23T20:00:01.000Z",
        resetAt: "2026-11-23T20:00:00.000Z",
        scope: "private-retained-rw-fd",
        source: "cgroup-memory-peak-reset",
        valueBytes: 200,
      },
      steady: {
        sampledMaximumBytes: 100,
        samples: [{
          memoryCurrentBytes: 100,
          observedAt: "2026-11-23T20:00:00.500Z",
          offsetMs: 500,
        }],
      },
    },
    scopeId,
    sourceBinding: {
      kind: "integration-supplied-live-receipt-binding",
      sha256: sourceSha256,
    },
    startedAt: "2026-11-23T20:00:00.000Z",
  };
}

// derive one published offline aggregate from exact service intervals
function applicationAggregate(resources) {
  return evaluateBackendPerformanceDirectAppAggregate({
    contractVersion: BACKEND_PERFORMANCE_DIRECT_APP_AGGREGATE_INPUT_VERSION,
    exclusiveParentInterval: null,
    serviceIntervals: resources.filter(
      // retain the three application services
      (resource) => resource.accountingScope === "service",
    ),
  });
}

// derive one published offline host accounting result
function hostAccounting() {
  const meminfo = (free, available) => [
    "MemTotal:       8000000 kB",
    `MemFree:        ${String(free)} kB`,
    `MemAvailable:   ${String(available)} kB`,
    "SwapTotal:      2000000 kB",
    "SwapFree:       2000000 kB",
    "",
  ].join("\n");
  const snapshot = (capturedAt, monotonicMilliseconds, ticks, free, available) => ({
    capturedAt,
    meminfo: meminfo(free, available),
    monotonicMilliseconds,
    procStat: `cpu ${String(ticks)} 2 3 40 5 6 7 8 1 1\n`,
  });
  return evaluateBackendPerformanceDirectHostAccounting({
    clockId: "native-readiness-host-clock",
    contractVersion: BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_INPUT_VERSION,
    snapshots: [
      snapshot("2026-11-23T20:00:00.000Z", 1_000, 10, 2_000_000, 4_000_000),
      snapshot("2026-11-23T20:00:01.000Z", 2_000, 20, 1_900_000, 3_900_000),
    ],
  });
}

// build one digest-bound receipt projection for injected validation tests
function liveReceipt(architecture = "aarch64") {
  return {
    authority: {
      canonicalEligible: false,
      origin: "synthetic-fixture",
    },
    captures: [{
      hostRole: "blueberry",
      observation: {
        authority: { kind: "injected-test", liveEligible: false },
        identity: { architecture },
      },
      profile: "blueberry-compose",
      source: { kind: "cleared-observation-fixture" },
    }],
    marker: "direct-native-readiness-test",
  };
}

// create one deterministic injected validator result
function validator(receipt) {
  return {
    canonicalEligible: false,
    diagnostics: [],
    ok: true,
    sha256: receiptSha256(receipt),
  };
}

test("reports exact absent native batch prerequisites", () => {
  const result = assessBackendPerformanceDirectNativeReadiness({
    liveStatus: null,
    operationReceipts: [],
    resourceIntervals: [],
  });
  const codes = result.diagnostics.map(
    // retain every exact blocker code
    (diagnostic) => diagnostic.code,
  );

  assert.equal(result.status, "blocked");
  assert.equal(result.physicalNativeReady, false);
  assert.equal(result.host.architectureClass, "absent");
  assert.equal(result.operations.summary.requiredOperations, 104);
  assert.equal(result.operations.summary.requiredBuckets, 163);
  assert.equal(result.operations.summary.requiredAttempts, 6275);
  assert.equal(result.operations.summary.missingNativeBuckets, 163);
  assert.equal(result.operations.summary.missingNativeAttempts, 6275);
  assert.equal(codes.filter((code) => code === "native-readiness.operation-coverage").length, 1);
  assert.equal(codes.filter((code) => code === "native-readiness.resource-missing").length, 4);
  assert.equal(codes.includes("native-readiness.live-status-missing"), true);
  assert.equal(codes.includes("native-readiness.app-aggregate-missing"), true);
  assert.equal(codes.includes("native-readiness.host-accounting-missing"), true);
  assert.equal(result.resources.applicationAggregate.status, "missing");
  assert.equal(result.resources.host.status, "missing");
});

test("distinguishes ARM64 fixture identity and native versus injected intervals", () => {
  const receipt = liveReceipt();
  const sha256 = receiptSha256(receipt);
  const resources = [
    interval("service", "api", sha256),
    interval("service", "web", sha256, false),
    interval("service", "worker", sha256),
    interval("postgresql", "postgres", sha256),
  ];
  const result = assessBackendPerformanceDirectNativeReadiness({
    liveStatus: { receipt, validationOptions: {} },
    operationReceipts: operationReceipts(),
    resourceIntervals: resources,
  }, { validateLiveStatusReceipt: validator });
  const api = result.resources.direct.find(
    // select one native service interval
    (resource) => resource.scopeId === "api",
  );
  const web = result.resources.direct.find(
    // select one injected service interval
    (resource) => resource.scopeId === "web",
  );

  assert.equal(result.host.architectureClass, "arm64-noncanonical-or-injected");
  assert.equal(result.host.arm64NativeHost, false);
  assert.equal(result.liveStatus.validatorAuthority, "injected-test-only");
  assert.equal(result.resources.directScopeSetExact, true);
  assert.equal(api.authorityClass, "native-linux-cgroup-v2");
  assert.equal(api.nativeEligible, true);
  assert.equal(web.authorityClass, "injected-test-only");
  assert.equal(web.nativeEligible, false);
  assert.equal(result.physicalNativeReady, false);
  assert.equal(result.diagnostics.some(
    // retain the injected resource blocker
    (diagnostic) => diagnostic.code === "native-readiness.resource-injected",
  ), true);
});

test("does not choose between duplicate direct scopes or admit extra scopes", () => {
  const receipt = liveReceipt();
  const sha256 = receiptSha256(receipt);
  const intervals = [
    interval("service", "api", sha256),
    interval("service", "web", sha256),
    interval("service", "worker", sha256),
    interval("postgresql", "postgres", sha256),
  ];
  const duplicateApi = interval("service", "api", sha256);
  const extra = { ...interval("service", "api", sha256), scopeId: "alternate-api" };
  const result = assessBackendPerformanceDirectNativeReadiness({
    liveStatus: { receipt, validationOptions: {} },
    operationReceipts: operationReceipts(),
    resourceIntervals: [...intervals, duplicateApi, extra],
  }, { validateLiveStatusReceipt: validator });
  const api = result.resources.direct.find(
    // select the duplicated direct scope without choosing one observation
    (resource) => resource.scopeId === "api",
  );
  assert.equal(result.resources.directScopeSetExact, false);
  assert.equal(api.present, true);
  assert.equal(api.nativeEligible, false);
  assert.equal(api.cgroupSha256, null);
  assert.equal(api.authorityClass, "ambiguous");
  assert.equal(result.diagnostics.some(
    // require an explicit conflict rather than last-writer wins
    (diagnostic) => diagnostic.code === "native-readiness.resource-duplicate",
  ), true);
  assert.equal(result.diagnostics.some(
    // require a diagnostic for every unplanned direct scope
    (diagnostic) => diagnostic.code === "native-readiness.resource-extra",
  ), true);
  const extraOnly = assessBackendPerformanceDirectNativeReadiness({
    liveStatus: { receipt, validationOptions: {} },
    operationReceipts: operationReceipts(),
    resourceIntervals: [...intervals, extra],
  }, { validateLiveStatusReceipt: validator });
  assert.equal(extraOnly.resources.directScopeSetExact, false);
});

test("integrates aligned published aggregate results without promoting offline custody", () => {
  const receipt = liveReceipt();
  const sha256 = receiptSha256(receipt);
  const resources = [
    interval("service", "api", sha256),
    interval("service", "web", sha256),
    interval("service", "worker", sha256),
    interval("postgresql", "postgres", sha256),
  ];
  const result = assessBackendPerformanceDirectNativeReadiness({
    applicationAggregateReceipt: applicationAggregate(resources),
    hostAccountingReceipt: hostAccounting(),
    liveStatus: { receipt, validationOptions: {} },
    operationReceipts: operationReceipts(),
    resourceIntervals: resources,
  }, { validateLiveStatusReceipt: validator });

  assert.deepEqual(result.resources.applicationAggregate, {
    contractValid: true,
    nativeEligible: false,
    present: true,
    scopesAligned: true,
    sourceBound: true,
    status: "offline-noncanonical",
  });
  assert.deepEqual(result.resources.host, {
    contractValid: true,
    nativeEligible: false,
    present: true,
    sourceBound: false,
    status: "offline-noncanonical",
  });
  assert.equal(result.host.wholeHostAccounting, "offline-noncanonical");
  assert.equal(result.physicalNativeReady, false);
  assert.equal(result.diagnostics.some(
    // retain the exact application custody gap
    (diagnostic) => diagnostic.code === "native-readiness.app-aggregate-offline-authority",
  ), true);
  assert.equal(result.diagnostics.some(
    // retain the exact absent host source binding
    (diagnostic) => diagnostic.code === "native-readiness.host-accounting-source-unavailable",
  ), true);
});

test("rejects source mismatches and native relabeling of offline aggregate results", () => {
  const receipt = liveReceipt();
  const sha256 = receiptSha256(receipt);
  const resources = [
    interval("service", "api", sha256),
    interval("service", "web", sha256),
    interval("service", "worker", sha256),
    interval("postgresql", "postgres", sha256),
  ];
  const sourceMismatch = structuredClone(applicationAggregate(resources));
  sourceMismatch.identity.sourceReceiptSha256 = HASH_A;
  const sourceResult = assessBackendPerformanceDirectNativeReadiness({
    applicationAggregateReceipt: sourceMismatch,
    hostAccountingReceipt: hostAccounting(),
    liveStatus: { receipt, validationOptions: {} },
    operationReceipts: operationReceipts(),
    resourceIntervals: resources,
  }, { validateLiveStatusReceipt: validator });

  assert.equal(sourceResult.resources.applicationAggregate.status, "offline-noncanonical");
  assert.equal(sourceResult.resources.applicationAggregate.sourceBound, false);
  assert.equal(sourceResult.diagnostics.some(
    // reject a mismatched live-status source identity
    (diagnostic) => diagnostic.code === "native-readiness.app-aggregate-source",
  ), true);

  const alignmentMismatch = structuredClone(applicationAggregate(resources));
  alignmentMismatch.services[0].cgroupIdentity.sha256 = HASH_A;
  const alignmentResult = assessBackendPerformanceDirectNativeReadiness({
    applicationAggregateReceipt: alignmentMismatch,
    hostAccountingReceipt: hostAccounting(),
    liveStatus: { receipt, validationOptions: {} },
    operationReceipts: operationReceipts(),
    resourceIntervals: resources,
  }, { validateLiveStatusReceipt: validator });

  assert.equal(alignmentResult.resources.applicationAggregate.scopesAligned, false);
  assert.equal(alignmentResult.diagnostics.some(
    // reject a mismatched direct cgroup identity
    (diagnostic) => diagnostic.code === "native-readiness.app-aggregate-alignment",
  ), true);

  const aggregate = structuredClone(applicationAggregate(resources));
  const host = structuredClone(hostAccounting());
  aggregate.authority.nativeEligible = true;
  host.authority.nativeEligible = true;
  const result = assessBackendPerformanceDirectNativeReadiness({
    applicationAggregateReceipt: aggregate,
    hostAccountingReceipt: host,
    liveStatus: { receipt, validationOptions: {} },
    operationReceipts: operationReceipts(),
    resourceIntervals: resources,
  }, { validateLiveStatusReceipt: validator });

  assert.equal(result.resources.applicationAggregate.status, "invalid");
  assert.equal(result.resources.applicationAggregate.nativeEligible, false);
  assert.equal(result.resources.host.status, "invalid");
  assert.equal(result.resources.host.nativeEligible, false);
  assert.equal(result.physicalNativeReady, false);
  assert.equal(result.diagnostics.some(
    // reject caller-promoted application authority
    (diagnostic) => diagnostic.code === "native-readiness.app-aggregate-authority",
  ), true);
  assert.equal(result.diagnostics.some(
    // reject caller-promoted host authority
    (diagnostic) => diagnostic.code === "native-readiness.host-accounting-authority",
  ), true);
});

test("does not use architecture or resources from a validator-rejected receipt", () => {
  const receipt = liveReceipt("x86_64");
  const result = assessBackendPerformanceDirectNativeReadiness({
    liveStatus: { receipt, validationOptions: {} },
    operationReceipts: operationReceipts(false),
    resourceIntervals: [interval("service", "api", HASH_A)],
  });

  assert.equal(result.host.architectureClass, "unverified");
  assert.equal(result.liveStatus.receiptSha256, null);
  assert.equal(result.resources.direct.find(
    // select the supplied but unbound interval
    (resource) => resource.scopeId === "api",
  ).sourceBound, false);
  assert.equal(result.diagnostics.some(
    // retain the published validator failure
    (diagnostic) => diagnostic.code === "native-readiness.live-status-invalid",
  ), true);
});
