import assert from "node:assert/strict";
import { chmod, mkdtemp, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_DIRECT_BASELINE_VERSION,
  BACKEND_PERFORMANCE_DIRECT_TARGET_SELECTOR,
  deriveBackendPerformanceDirectTarget,
  writeBackendPerformanceDirectBaseline,
  writeBackendPerformanceDirectTarget,
} from "./backend-performance-direct-target.mjs";

const FROZEN_AT = new Date("2026-11-23T20:00:00.000Z");

// build one explicit synthetic baseline cohort
function baselineRuns(values = [100, 102, 98, 101, 99]) {
  return values.map(
    // build one fixed-condition synthetic run
    (value, index) => ({
      attemptDenominator: {
        observedAttempts: 80,
        plannedAttempts: 80,
        terminalCounts: { completed: 80, error: 0, timeout: 0 },
      },
      bundleSha256: String(index + 1).repeat(64),
      classifications: [{
        classification: "synthetic-complete",
        count: 80,
        terminalCategory: "completed",
      }],
      collectedAt: `2026-11-23T1${String(index)}:01:00.000Z`,
      completedAt: `2026-11-23T1${String(index)}:00:00.000Z`,
      contractVersion: BACKEND_PERFORMANCE_DIRECT_BASELINE_VERSION,
      evidenceClass: "synthetic-fixture",
      generation: "node-v1",
      metrics: [{
        identity: {
          metricId: "api.health.warm.p95",
          scope: "service",
          service: "api",
          statistic: "nearest-rank-p95",
          stratum: "warm",
          unit: "nanoseconds",
          workload: "api.health",
        },
        physicalLowerBound: 0,
        value,
      }],
      resourceEvidenceSha256: null,
      resourceObservation: null,
      role: "baseline",
      runId: `synthetic-run-${String(index + 1)}`,
      sourceResultSha256: "b".repeat(64),
      workloadIdentitySha256: "a".repeat(64),
    }),
  );
}

// build one target input without caller thresholds
function targetInput(runs = baselineRuns()) {
  return {
    baselineRuns: runs,
    contractVersion: "weather-backend-performance-direct-target-input/v1",
  };
}

// create one private temporary evidence directory
async function privateDirectory() {
  const directory = await mkdtemp(join(tmpdir(), "weather-direct-target-"));
  await chmod(directory, 0o700);
  return directory;
}

test("derives the exact strict uncertainty target from five baselines", () => {
  const target = deriveBackendPerformanceDirectTarget(targetInput(), {
    now: () => FROZEN_AT,
  });
  const metric = target.metricTargets[0];

  assert.equal(target.evidenceClass, "synthetic-fixture");
  assert.equal(target.selector, BACKEND_PERFORMANCE_DIRECT_TARGET_SELECTOR);
  assert.equal(target.frozenAt, FROZEN_AT.toISOString());
  assert.equal(metric.baselinePoint, 100);
  assert.deepEqual(metric.baselineValues, [100, 102, 98, 101, 99]);
  assert.deepEqual(metric.sampleSdProof, {
    divisor: 10,
    floor: 1,
    radicandDecimal: "250",
  });
  assert.equal(metric.interval.upperBound, 102);
  assert.equal(metric.strictImprovement, 3);
  assert.equal(metric.threshold, 97);
  assert.equal(metric.comparator, "candidate-value-less-than-or-equal");
});

test("rejects caller thresholds and unstable target rows", () => {
  const withThreshold = { ...targetInput(), threshold: 1 };
  assert.throws(
    () => deriveBackendPerformanceDirectTarget(withThreshold, { now: () => FROZEN_AT }),
    { code: "direct-target.schema" },
  );
  assert.throws(
    () => deriveBackendPerformanceDirectTarget(
      targetInput(baselineRuns([1, 2, 3, 4, 5])),
      { now: () => FROZEN_AT },
    ),
    { code: "direct-target.unstable" },
  );
  const physicalClaim = baselineRuns();
  physicalClaim[0] = { ...physicalClaim[0], evidenceClass: "physical-native-baseline" };
  assert.throws(
    () => deriveBackendPerformanceDirectTarget(
      targetInput(physicalClaim),
      { now: () => FROZEN_AT },
    ),
    { code: "direct-target.baseline" },
  );
});

test("publishes five private baselines and one create-once target", async () => {
  const directory = await privateDirectory();
  const input = targetInput();
  try {
    // publish each synthetic baseline once
    for (const run of input.baselineRuns) {
      await writeBackendPerformanceDirectBaseline(directory, run);
    }
    const publication = await writeBackendPerformanceDirectTarget(directory, input, {
      now: () => FROZEN_AT,
    });
    const targetIdentity = await stat(join(directory, "target.json"));

    assert.equal(publication.target.evidenceClass, "synthetic-fixture");
    assert.match(publication.receiptSha256, /^[a-f0-9]{64}$/);
    assert.equal(targetIdentity.mode & 0o777, 0o600);
    await assert.rejects(
      writeBackendPerformanceDirectTarget(directory, input, { now: () => FROZEN_AT }),
      { code: "direct-target.exists" },
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("rejects candidate evidence before target publication", async () => {
  const directory = await privateDirectory();
  const input = targetInput();
  try {
    // publish every required baseline receipt
    for (const run of input.baselineRuns) {
      await writeBackendPerformanceDirectBaseline(directory, run);
    }
    await writeFile(join(directory, "candidate-started.json"), "{}\n", { mode: 0o600 });
    await assert.rejects(
      writeBackendPerformanceDirectTarget(directory, input, { now: () => FROZEN_AT }),
      { code: "direct-target.chronology" },
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});
