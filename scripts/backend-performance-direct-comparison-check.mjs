import { chmod, mkdtemp, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import {
  bindBackendPerformanceDirectSchedule,
  pairBackendPerformanceDirectBindings,
} from "./backend-performance-direct-binding.mjs";
import {
  BACKEND_PERFORMANCE_DIRECT_CANDIDATE_VERSION,
  compareBackendPerformanceDirectCandidate,
} from "./backend-performance-direct-comparison.mjs";
import { buildBackendPerformanceFunctionalSchedule } from
  "./backend-performance-functional-schedule.mjs";
import {
  BACKEND_PERFORMANCE_DIRECT_BASELINE_VERSION,
  writeBackendPerformanceDirectBaseline,
  writeBackendPerformanceDirectTarget,
} from "./backend-performance-direct-target.mjs";

const METRIC_IDENTITY = {
  metricId: "api.health.warm.p95",
  scope: "bound-origin",
  service: "api",
  statistic: "nearest-rank-p95",
  stratum: "warm",
  unit: "nanoseconds",
  workload: "api.health",
};

// select one reviewed warm health attempt
function selectedAttemptId() {
  const schedule = buildBackendPerformanceFunctionalSchedule();
  // visit every source run slot
  for (const runSlot of schedule.runSlots) {
    // visit every source group
    for (const group of runSlot.groups) {
      // select the first warm health attempt
      for (const attempt of group.attempts) {
        // return the exact reviewed fixture attempt
        if (attempt.operationId === "api.health" && attempt.stratum === "warm") {
          return attempt.attemptId;
        }
      }
    }
  }
  throw new Error("comparison fixture has no warm health attempt");
}

// bind one synthetic fixture role without executing traffic
function bind(role, generation, runId) {
  return bindBackendPerformanceDirectSchedule({
    attemptIds: [selectedAttemptId()],
    baseUrl: "http://127.0.0.1:9",
    generation,
    maximumResponseBytes: 1_024,
    resourceInterval: null,
    role,
    runId,
  });
}

// build five matched baseline fixture receipts
function baselineRuns(workloadIdentitySha256, frozenAt) {
  const values = [100, 102, 98, 101, 99];
  return values.map(
    // build one independent fixed-condition baseline
    (value, index) => {
      const completedAt = new Date(frozenAt.getTime() - (10 - index) * 60_000);
      const collectedAt = new Date(completedAt.getTime() + 1_000);
      return {
        attemptDenominator: {
          observedAttempts: 1,
          plannedAttempts: 1,
          terminalCounts: { completed: 1, error: 0, timeout: 0 },
        },
        bundleSha256: String(index + 1).repeat(64),
        classifications: [{
          classification: "synthetic-health-complete",
          count: 1,
          terminalCategory: "completed",
        }],
        collectedAt: collectedAt.toISOString(),
        completedAt: completedAt.toISOString(),
        contractVersion: BACKEND_PERFORMANCE_DIRECT_BASELINE_VERSION,
        evidenceClass: "synthetic-fixture",
        generation: "node-v1",
        metrics: [{ identity: METRIC_IDENTITY, physicalLowerBound: 0, value }],
        resourceEvidenceSha256: null,
        resourceObservation: null,
        role: "baseline",
        runId: `comparison-baseline-${String(index + 1)}`,
        sourceResultSha256: "b".repeat(64),
        workloadIdentitySha256,
      };
    },
  );
}

// build one complete candidate fixture receipt
function candidateReceipt(bindingPair, targetReceiptSha256, startedAt, options = {}) {
  const completedAt = new Date(startedAt.getTime() + 1_000);
  const terminalCounts = options.terminalCounts
    ?? { completed: 1, error: 0, timeout: 0 };
  const terminalCategory = terminalCounts.completed === 1 ? "completed" : "error";
  return {
    attemptDenominator: {
      observedAttempts: 1,
      plannedAttempts: 1,
      terminalCounts,
    },
    capacityGaps: options.capacityGaps ?? [],
    classifications: [{
      classification: terminalCategory === "completed"
        ? "synthetic-health-complete"
        : "synthetic-health-error",
      count: 1,
      terminalCategory,
    }],
    collectedAt: new Date(completedAt.getTime() + 1_000).toISOString(),
    completedAt: completedAt.toISOString(),
    contractVersion: BACKEND_PERFORMANCE_DIRECT_CANDIDATE_VERSION,
    evidenceClass: "synthetic-fixture",
    generation: bindingPair.candidate.directConfiguration.generation,
    metrics: options.metrics ?? [],
    role: "candidate",
    runId: bindingPair.candidate.directConfiguration.runId,
    sourceResultSha256: "c".repeat(64),
    startedAt: startedAt.toISOString(),
    targetReceiptSha256,
    workloadIdentitySha256: bindingPair.matchedWorkloadIdentity.value,
  };
}

// run strict pass fail and capacity-gap synthetic comparisons
export async function runBackendPerformanceDirectComparisonCheck() {
  const directory = await mkdtemp(join(tmpdir(), "weather-direct-comparison-"));
  await chmod(directory, 0o700);
  try {
    const bindingPair = pairBackendPerformanceDirectBindings(
      bind("baseline", "node-v1", "comparison-baseline-binding"),
      bind("candidate", "rust-v1", "comparison-candidate"),
    );
    const frozenAt = new Date();
    const baselines = baselineRuns(bindingPair.matchedWorkloadIdentity.value, frozenAt);
    // persist all five immutable baseline receipts
    for (const baseline of baselines) {
      await writeBackendPerformanceDirectBaseline(directory, baseline);
    }
    const publication = await writeBackendPerformanceDirectTarget(directory, {
      baselineRuns: baselines,
      contractVersion: "weather-backend-performance-direct-target-input/v1",
    }, {
      now: () => frozenAt,
    });
    const targetIdentity = await stat(join(directory, "target.json"));
    const candidateStartedAt = new Date(Math.max(
      targetIdentity.mtimeMs,
      Date.parse(publication.target.frozenAt),
    ) + 1_000);
    const threshold = publication.target.metricTargets[0].threshold;
    const pass = await compareBackendPerformanceDirectCandidate(
      directory,
      bindingPair,
      candidateReceipt(bindingPair, publication.receiptSha256, candidateStartedAt, {
        metrics: [{ identity: METRIC_IDENTITY, physicalLowerBound: 0, value: threshold }],
      }),
    );
    const failResult = await compareBackendPerformanceDirectCandidate(
      directory,
      bindingPair,
      candidateReceipt(bindingPair, publication.receiptSha256, candidateStartedAt, {
        metrics: [{ identity: METRIC_IDENTITY, physicalLowerBound: 0, value: threshold + 1 }],
      }),
    );
    const capacityGap = await compareBackendPerformanceDirectCandidate(
      directory,
      bindingPair,
      candidateReceipt(bindingPair, publication.receiptSha256, candidateStartedAt, {
        capacityGaps: [{
          metricId: METRIC_IDENTITY.metricId,
          reason: "measurement-capacity-unavailable",
        }],
      }),
    );
    const terminalFail = await compareBackendPerformanceDirectCandidate(
      directory,
      bindingPair,
      candidateReceipt(bindingPair, publication.receiptSha256, candidateStartedAt, {
        metrics: [{ identity: METRIC_IDENTITY, physicalLowerBound: 0, value: threshold }],
        terminalCounts: { completed: 0, error: 1, timeout: 0 },
      }),
    );
    let preTargetCandidateRejected = false;
    try {
      await compareBackendPerformanceDirectCandidate(
        directory,
        bindingPair,
        candidateReceipt(
          bindingPair,
          publication.receiptSha256,
          new Date(targetIdentity.mtimeMs - 1_000),
          { metrics: [{ identity: METRIC_IDENTITY, physicalLowerBound: 0, value: threshold }] },
        ),
      );
    } catch (error) {
      preTargetCandidateRejected = error?.code === "direct-comparison.chronology";
    }
    let callerThresholdRejected = false;
    try {
      await compareBackendPerformanceDirectCandidate(directory, bindingPair, {
        ...candidateReceipt(bindingPair, publication.receiptSha256, candidateStartedAt, {
          metrics: [{ identity: METRIC_IDENTITY, physicalLowerBound: 0, value: threshold }],
        }),
        threshold: 1,
      });
    } catch (error) {
      callerThresholdRejected = error?.code === "direct-comparison.schema";
    }
    let identityMismatchRejected = false;
    try {
      await compareBackendPerformanceDirectCandidate(directory, bindingPair, {
        ...candidateReceipt(bindingPair, publication.receiptSha256, candidateStartedAt, {
          metrics: [{ identity: METRIC_IDENTITY, physicalLowerBound: 0, value: threshold }],
        }),
        workloadIdentitySha256: "0".repeat(64),
      });
    } catch (error) {
      identityMismatchRejected = error?.code === "direct-comparison.binding";
    }
    let incompleteMetricDenominatorRejected = false;
    try {
      await compareBackendPerformanceDirectCandidate(
        directory,
        bindingPair,
        candidateReceipt(bindingPair, publication.receiptSha256, candidateStartedAt),
      );
    } catch (error) {
      incompleteMetricDenominatorRejected = error?.code === "direct-comparison.metric";
    }
    return {
      callerThresholdRejected,
      capacityGap,
      fail: failResult,
      identityMismatchRejected,
      incompleteMetricDenominatorRejected,
      pass,
      preTargetCandidateRejected,
      targetReceiptSha256: publication.receiptSha256,
      terminalFail,
    };
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
}

// print only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runBackendPerformanceDirectComparisonCheck().then(
    // emit one machine-readable comparison result
    (result) => process.stdout.write(`${JSON.stringify(result, null, 2)}\n`),
    // emit one controlled comparison failure
    (error) => {
      process.stderr.write(`${error?.code ?? "direct-comparison.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    },
  );
}
