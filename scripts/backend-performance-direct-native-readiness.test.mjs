import assert from "node:assert/strict";
import test from "node:test";

import {
  assessBackendPerformanceDirectNativeReadiness,
} from "./backend-performance-direct-native-readiness.mjs";
import { receiptSha256 } from "./backend-performance-receipts.mjs";
import {
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";

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
    after: { cgroupId: HASH_B, observedAt: "2026-11-23T20:00:01.000Z", pid: 10, releaseId: "node-v1" },
    authority,
    before: { cgroupId: HASH_B, observedAt: "2026-11-23T20:00:00.000Z", pid: 10, releaseId: "node-v1" },
    cgroupIdentity: { sha256: HASH_B },
    contractVersion: "weather-backend-resource-interval/v1",
    scopeId,
    sourceBinding: {
      kind: "integration-supplied-live-receipt-binding",
      sha256: sourceSha256,
    },
  };
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
  assert.equal(codes.includes("native-readiness.host-accounting-unavailable"), true);
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
