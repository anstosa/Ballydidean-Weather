import assert from "node:assert/strict";
import test from "node:test";

import { evaluateBackendPerformanceAttemptAccounting } from
  "./backend-performance-attempt-accounting.mjs";
import {
  BACKEND_PERFORMANCE_RESOURCE_ACCOUNTING_INPUT_SCHEMA,
  BACKEND_PERFORMANCE_RESOURCE_ACCOUNTING_VERSION,
  evaluateBackendPerformanceResourceAccounting,
} from "./backend-performance-resource-accounting.mjs";
import { buildBackendPerformanceFunctionalSchedule } from
  "./backend-performance-functional-schedule.mjs";
import { buildBackendPerformanceScheduleBinding } from
  "./backend-performance-schedule-binding.mjs";

const RELEASE_ID = "2026.11.20-resource-baseline";
const SCOPE_ID = "api";
const SOURCE_RECEIPT_DIGEST = "a".repeat(64);
const CGROUP_ID = "c".repeat(64);
const CLOCK_ID = "integration-performance-clock-1";

// clone one plain test value
function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

// flatten the complete fixed plan
function plannedAttempts() {
  const plan = buildBackendPerformanceFunctionalSchedule();
  const attempts = [];
  // visit every fixed run slot
  for (const runSlot of plan.runSlots) {
    // visit every fixed group
    for (const group of runSlot.groups) {
      // visit every fixed attempt
      for (const attempt of group.attempts) {
        attempts.push(attempt);
      }
    }
  }
  assert.equal(attempts.length, 6_275);
  return { attempts, plan };
}

// build one complete synthetic baseline ledger
function baselineInput() {
  const { attempts, plan } = plannedAttempts();
  const outcomes = attempts.map(
    // complete one planned attempt
    (attempt, index) => ({
      attemptId: attempt.attemptId,
      elapsedMonotonic: { kind: "observed", nanoseconds: index + 1 },
      errorCategory: null,
      httpResponseBytes: 64,
      processOutputBytes: 0,
      terminalCategory: "completed",
      terminalStatus: {
        httpStatus: 200,
        kind: "http",
        processExitCode: null,
        processSignal: null,
      },
    }),
  );
  return {
    group: plan.runSlots[0].groups[0],
    rawBaseline: {
      binding: buildBackendPerformanceScheduleBinding({
        generation: "node-v1",
        role: "baseline",
      }),
      outcomes,
    },
    runSlot: plan.runSlots[0],
  };
}

// build one native interval shaped fixture
function resourceInterval() {
  const processSource = {
    cgroupMembership: "0::/weather-api",
    commandSha256: "e".repeat(64),
    executable: { device: "8", inode: "300", path: "/usr/local/bin/node" },
    sha256: "f".repeat(64),
    startTimeTicks: 999_999,
  };
  const processSample = {
    fdCount: 4,
    rssBytes: 98_304_000,
    samplingMethod: "instantaneous-not-peak",
    threadCount: 12,
  };
  const beforeCpu = {
    nr_periods: 10,
    nr_throttled: 1,
    system_usec: 300,
    throttled_usec: 20,
    usage_usec: 1_000,
    user_usec: 700,
  };
  const afterCpu = {
    nr_periods: 14,
    nr_throttled: 3,
    system_usec: 410,
    throttled_usec: 70,
    usage_usec: 1_600,
    user_usec: 1_190,
  };
  const beforeEvents = {
    high: 0,
    low: 0,
    max: 0,
    oom: 0,
    oom_group_kill: 0,
    oom_kill: 0,
  };
  const afterEvents = {
    high: 1,
    low: 0,
    max: 2,
    oom: 1,
    oom_group_kill: 0,
    oom_kill: 1,
  };
  const before = {
    cpu: beforeCpu,
    memoryCurrentBytes: 100_000_000,
    memoryEvents: beforeEvents,
    monotonicMs: 1_000,
    observedAt: "2026-11-20T20:00:00.000Z",
    process: { sample: processSample, source: processSource },
  };
  const after = {
    cpu: afterCpu,
    memoryCurrentBytes: 150_000_000,
    memoryEvents: afterEvents,
    monotonicMs: 1_100,
    observedAt: "2026-11-20T20:00:00.100Z",
    process: { sample: processSample, source: processSource },
  };
  return {
    accounting: {
      aggregateMethod: "direct-single-cgroup",
      coverage: "measured-process-cgroup-only",
      sampledServiceSumUsed: false,
      unsupportedScopes: ["app-aggregate", "host"],
    },
    accountingScope: "service",
    after: {
      cgroupId: CGROUP_ID,
      observedAt: after.observedAt,
      pid: 321,
      releaseId: RELEASE_ID,
    },
    authority: {
      bounds: { descriptorEntries: 4_096, kernelPathBytes: 4_096, observationBytes: 1_048_576 },
      kind: "injected-test-only",
      liveEligible: false,
      nativeCgroup: {},
      scopeCoverage: "direct-process-cgroup-only",
      unsupportedScopes: ["app-aggregate", "host"],
    },
    before: {
      cgroupId: CGROUP_ID,
      observedAt: before.observedAt,
      pid: 321,
      releaseId: RELEASE_ID,
    },
    cgroupIdentity: {
      bootId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
      group: { device: "8", inode: "200", path: "/sys/fs/cgroup/weather-api" },
      root: { device: "8", inode: "100", path: "/sys/fs/cgroup" },
      sha256: CGROUP_ID,
    },
    completedAt: "2026-11-20T20:00:00.100Z",
    contractVersion: "weather-backend-resource-interval/v1",
    cpu: {
      nrPeriods: 4,
      nrThrottled: 2,
      source: "cgroup-cpu-stat-delta",
      systemUsec: 110,
      throttledUsec: 50,
      usageUsec: 600,
      userUsec: 490,
    },
    limits: {
      controllers: ["cpu", "memory", "pids"],
      cpuMax: "50000 100000",
      memoryMax: "268435456",
      pidsMax: "96",
      sha256: "b".repeat(64),
    },
    memory: {
      eventsDelta: { high: 1, low: 0, max: 2, oom: 1, oomGroupKill: 0, oomKill: 1 },
      peak: {
        fdIdentitySha256: "d".repeat(64),
        initializedBytes: 100_000_000,
        readAt: "2026-11-20T20:00:00.100Z",
        resetAt: "2026-11-20T20:00:00.000Z",
        scope: "private-retained-rw-fd",
        source: "cgroup-memory-peak-reset",
        valueBytes: 250_000_000,
      },
      steady: {
        arithmeticMeanBytes: 150_000_000,
        method: "bounded-caller-scheduled-memory.current-samples",
        sampledMaximumBytes: 200_000_000,
        samples: [
          {
            memoryCurrentBytes: 100_000_000,
            observedAt: "2026-11-20T20:00:00.000Z",
            offsetMs: 0,
          },
          {
            memoryCurrentBytes: 200_000_000,
            observedAt: "2026-11-20T20:00:00.050Z",
            offsetMs: 50,
          },
          {
            memoryCurrentBytes: 150_000_000,
            observedAt: "2026-11-20T20:00:00.100Z",
            offsetMs: 100,
          },
        ],
        source: "cgroup-memory-current-window",
      },
    },
    method: {
      cpu: "cgroup-v2-cpu.stat-counter-delta",
      peak: "linux-cgroup-v2-memory.peak-private-retained-rw-fd-reset",
      process: "procfs-instantaneous-samples-not-peaks",
      steady: "caller-triggered-bounded-snapshots",
    },
    observations: { after, before },
    process: {
      samples: [
        { ...processSample, observedAt: before.observedAt, offsetMs: 0 },
        { ...processSample, observedAt: after.observedAt, offsetMs: 100 },
      ],
      source: "procfs-snapshot",
      sourceIdentity: processSource,
    },
    sampling: {
      budgetStartedAt: "2026-11-20T19:59:59.900Z",
      deadlineMs: 5_000,
      maximumSamples: 5,
      minimumSampleSpacingMs: 0,
      minimumSamples: 3,
      sampleCount: 3,
    },
    scopeId: SCOPE_ID,
    sourceBinding: {
      kind: "integration-supplied-live-receipt-binding",
      sha256: SOURCE_RECEIPT_DIGEST,
    },
    startedAt: "2026-11-20T20:00:00.000Z",
  };
}

// bind one ledger to one interval fixture
function evidenceFor(rawBaseline, runSlot, group, interval = resourceInterval()) {
  const accounting = evaluateBackendPerformanceAttemptAccounting(rawBaseline);
  return {
    attribution: {
      accountingDigest: accounting.accountingDigest.value,
      accountingScope: interval.accountingScope,
      generation: "node-v1",
      intervalClockId: CLOCK_ID,
      releaseId: RELEASE_ID,
      role: "baseline",
      scopeId: SCOPE_ID,
      sourceReceiptDigest: SOURCE_RECEIPT_DIGEST,
    },
    interval,
    window: {
      clockId: CLOCK_ID,
      completedMonotonicMs: 1_090,
      groupId: group.id,
      runSlotId: runSlot.id,
      startedMonotonicMs: 1_010,
    },
  };
}

// assert recursive output immutability
function assertDeepFrozen(value) {
  // inspect every nested output value
  if (value !== null && typeof value === "object") {
    assert.equal(Object.isFrozen(value), true);
    // recurse through every output child
    for (const child of Object.values(value)) {
      assertDeepFrozen(child);
    }
  }
}

// evaluate ordinary exact resource calculations
test("calculates one exact direct resource interval for one completed plan group", () => {
  const { group, rawBaseline, runSlot } = baselineInput();
  const result = evaluateBackendPerformanceResourceAccounting(
    rawBaseline,
    evidenceFor(rawBaseline, runSlot, group),
  );

  assert.equal(result.contractVersion, BACKEND_PERFORMANCE_RESOURCE_ACCOUNTING_VERSION);
  assert.equal(BACKEND_PERFORMANCE_RESOURCE_ACCOUNTING_INPUT_SCHEMA.arguments.length, 2);
  assert.equal(result.group.counts.planned, group.attempts.length);
  assert.equal(result.group.counts.terminalCounts.completed, group.attempts.length);
  assert.deepEqual(result.group.elapsedMonotonic, {
    maximumObservedMilliseconds: group.attempts.length / 1_000_000,
    maximumObservedNanoseconds: group.attempts.length,
    observedAttempts: group.attempts.length,
  });
  assert.deepEqual(result.group.counts.failures, {
    cancelled: 0,
    error: 0,
    timeout: 0,
    total: 0,
  });
  assert.deepEqual(result.resource.cpu.perDeclaredCompletedWork, {
    denominatorCompletedWork: group.attempts.length,
    numeratorObservedIntervalUsageUsec: 600,
  });
  assert.deepEqual(
    {
      nrPeriods: result.resource.cpu.nrPeriods,
      nrThrottled: result.resource.cpu.nrThrottled,
      systemUsec: result.resource.cpu.systemUsec,
      throttledUsec: result.resource.cpu.throttledUsec,
      usageUsec: result.resource.cpu.usageUsec,
      userUsec: result.resource.cpu.userUsec,
    },
    {
      nrPeriods: 4,
      nrThrottled: 2,
      systemUsec: 110,
      throttledUsec: 50,
      usageUsec: 600,
      userUsec: 490,
    },
  );
  assert.deepEqual(result.resource.memory.steady, {
    arithmeticMeanBytes: 150_000_000,
    endpointSpan: {
      offsetSpanMs: 100,
      rawMonotonicSpanMs: 100,
      toleranceMs: 8 * Number.EPSILON * 1_100,
    },
    method: "bounded-caller-scheduled-memory.current-samples",
    sampleCount: 3,
    sampledMaximumBytes: 200_000_000,
    source: "cgroup-memory-current-window",
  });
  assert.equal(result.resource.memory.nativePeak.valueBytes, 250_000_000);
  assert.equal(result.resource.memory.nativePeak.valueBytes === 200_000_000, false);
  assert.deepEqual(result.resource.methods, {
    cpu: "cgroup-v2-cpu.stat-counter-delta",
    peak: "linux-cgroup-v2-memory.peak-private-retained-rw-fd-reset",
    process: "procfs-instantaneous-samples-not-peaks",
    steady: "caller-triggered-bounded-snapshots",
  });
  assertDeepFrozen(result);
});

// retain failures without dividing incomplete work
test("keeps partial group failures and omits the cpu ratio", () => {
  const { group, rawBaseline, runSlot } = baselineInput();
  const failedAttemptId = group.attempts[0].attemptId;
  const failed = rawBaseline.outcomes.find(
    // locate one declared group attempt
    (outcome) => outcome.attemptId === failedAttemptId,
  );
  assert.ok(failed);
  failed.terminalCategory = "error";
  failed.errorCategory = "http";
  failed.terminalStatus.httpStatus = 500;
  const result = evaluateBackendPerformanceResourceAccounting(
    rawBaseline,
    evidenceFor(rawBaseline, runSlot, group),
  );

  assert.equal(result.group.allCompleted, false);
  assert.equal(result.group.counts.terminalCounts.completed, group.attempts.length - 1);
  assert.deepEqual(result.group.counts.failures, {
    cancelled: 0,
    error: 1,
    timeout: 0,
    total: 1,
  });
  assert.equal(result.resource.cpu.perDeclaredCompletedWork, null);
});

// reject elapsed observations wider than the declared group window
test("rejects a selected group attempt longer than its declared window", () => {
  const { group, rawBaseline, runSlot } = baselineInput();
  const attemptId = group.attempts[0].attemptId;
  const outcome = rawBaseline.outcomes.find(
    // locate one selected group attempt
    (candidate) => candidate.attemptId === attemptId,
  );
  assert.ok(outcome);
  outcome.terminalCategory = "error";
  outcome.errorCategory = "http";
  outcome.terminalStatus.httpStatus = 500;
  outcome.elapsedMonotonic.nanoseconds = 80_000_001;

  assert.throws(
    () => evaluateBackendPerformanceResourceAccounting(
      rawBaseline,
      evidenceFor(rawBaseline, runSlot, group),
    ),
    /observed elapsed time exceeds its declared group window/u,
  );
});

// reject endpoint offsets that disagree with raw monotonic time
test("rejects a changed memory endpoint offset span", () => {
  const { group, rawBaseline, runSlot } = baselineInput();
  const evidence = evidenceFor(rawBaseline, runSlot, group);
  evidence.interval.memory.steady.samples.at(-1).offsetMs = 100.01;

  assert.throws(
    () => evaluateBackendPerformanceResourceAccounting(rawBaseline, evidence),
    /endpoint offset span does not match the raw monotonic interval/u,
  );
});

// reject mismatched group and resource evidence
test("rejects group source scope clock cpu and peak mismatches", () => {
  const { group, rawBaseline, runSlot } = baselineInput();
  const base = evidenceFor(rawBaseline, runSlot, group);
  const cases = [
    {
      mutate: (evidence) => { evidence.window.groupId = "missing-group"; },
      pattern: /do not identify one planned group/u,
    },
    {
      mutate: (evidence) => { evidence.attribution.sourceReceiptDigest = "9".repeat(64); },
      pattern: /source receipt does not match/u,
    },
    {
      mutate: (evidence) => { evidence.attribution.accountingScope = "postgresql"; },
      pattern: /scope does not match/u,
    },
    {
      mutate: (evidence) => { evidence.window.clockId = "different-clock"; },
      pattern: /clock declarations do not match/u,
    },
    {
      mutate: (evidence) => { evidence.interval.cpu.usageUsec += 1; },
      pattern: /does not match raw delta/u,
    },
    {
      mutate: (evidence) => { evidence.interval.memory.peak.valueBytes = 199_999_999; },
      pattern: /below an initialized or sampled current value/u,
    },
  ];
  // exercise each independent mismatch
  for (const mismatch of cases) {
    const evidence = clone(base);
    mismatch.mutate(evidence);
    assert.throws(
      () => evaluateBackendPerformanceResourceAccounting(rawBaseline, evidence),
      mismatch.pattern,
    );
  }
});

// avoid creating aggregate or host authority
test("keeps the projection direct and noncanonical", () => {
  const { group, rawBaseline, runSlot } = baselineInput();
  const result = evaluateBackendPerformanceResourceAccounting(
    rawBaseline,
    evidenceFor(rawBaseline, runSlot, group),
  );

  assert.equal(result.canonicalEligible, false);
  assert.equal(result.provenance.applicationAggregateCreated, false);
  assert.equal(result.provenance.hostObserved, false);
  assert.equal(result.provenance.trafficGenerated, false);
  assert.equal(result.provenance.trustedClockAuthority, false);
  assert.equal(result.pendingGates.appAggregate, "unsupported-pending");
  assert.equal(result.pendingGates.host, "unsupported-pending");
  assert.equal(Object.hasOwn(result, "comparison"), false);
  assert.equal(Object.hasOwn(result, "target"), false);
  assert.equal(Object.hasOwn(result, "proofEligibility"), false);
});
