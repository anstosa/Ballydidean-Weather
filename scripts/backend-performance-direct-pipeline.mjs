import {
  BACKEND_PERFORMANCE_DIRECT_BINDING_VERSION,
} from "./backend-performance-direct-binding.mjs";
import {
  BACKEND_PERFORMANCE_DIRECT_RESULT_VERSION,
  evaluateBackendPerformanceDirect,
} from "./backend-performance-direct-evaluator.mjs";
import {
  BACKEND_PERFORMANCE_DIRECT_BASELINE_VERSION,
} from "./backend-performance-direct-target.mjs";
import { receiptSha256 } from "./backend-performance-receipts.mjs";

// create one coded pipeline error
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one pipeline invariant
function requireInvariant(condition, code, message) {
  // fail closed on mismatched measured evidence
  if (!condition) {
    fail(code, message);
  }
}

// identify one plain record
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

// create one targetable integer metric
function metric(identity, value) {
  requireInvariant(
    Number.isSafeInteger(value) && value >= 0,
    "direct-pipeline.metric",
    `${identity.metricId} is not a nonnegative integer base-unit value`,
  );
  return {
    identity,
    physicalLowerBound: 0,
    value,
  };
}

// map one operation-stratum result to targetable metrics
function operationMetrics(result) {
  return result.summary.operationStrata.flatMap(
    // retain exact source operation and stratum identity
    (row) => [
      metric({
        metricId: `${row.operationId}.${row.stratum}.p50`,
        scope: "bound-origin",
        service: row.operationId.split(".")[0],
        statistic: "nearest-rank-p50",
        stratum: row.stratum,
        unit: "nanoseconds",
        workload: row.operationId,
      }, row.latencyNanoseconds.p50),
      metric({
        metricId: `${row.operationId}.${row.stratum}.p95`,
        scope: "bound-origin",
        service: row.operationId.split(".")[0],
        statistic: "nearest-rank-p95",
        stratum: row.stratum,
        unit: "nanoseconds",
        workload: row.operationId,
      }, row.latencyNanoseconds.p95),
    ],
  );
}

// map one resource interval to targetable metrics
function resourceMetrics(resourceEvidence) {
  // retain no metric when resource collection was not configured
  if (resourceEvidence === null) {
    return [];
  }
  requireInvariant(
    isRecord(resourceEvidence.cpu) && Number.isSafeInteger(resourceEvidence.cpu.usageUsec)
      && isRecord(resourceEvidence.memory)
      && isRecord(resourceEvidence.memory.steady)
      && Number.isSafeInteger(resourceEvidence.memory.steady.sampledMaximumBytes)
      && isRecord(resourceEvidence.memory.peak)
      && Number.isSafeInteger(resourceEvidence.memory.peak.valueBytes),
    "direct-pipeline.resource",
    "resource interval does not expose exact CPU steady and peak values",
  );
  // retain resource values as observations until exact physical provenance is wired
  return [];
}

// retain one bounded resource observation in the baseline receipt
function resourceObservation(resourceEvidence) {
  // retain explicit absence
  if (resourceEvidence === null) {
    return null;
  }
  requireInvariant(
    typeof resourceEvidence.contractVersion === "string"
      && isRecord(resourceEvidence.cpu)
      && Number.isSafeInteger(resourceEvidence.cpu.usageUsec)
      && isRecord(resourceEvidence.memory?.steady)
      && Number.isSafeInteger(resourceEvidence.memory.steady.sampledMaximumBytes)
      && isRecord(resourceEvidence.memory?.peak)
      && Number.isSafeInteger(resourceEvidence.memory.peak.valueBytes),
    "direct-pipeline.resource",
    "resource observation is incomplete",
  );
  return {
    cpuUsageUsec: resourceEvidence.cpu.usageUsec,
    memoryPeakBytes: resourceEvidence.memory.peak.valueBytes,
    memorySteadyMaximumBytes: resourceEvidence.memory.steady.sampledMaximumBytes,
    sourceContractVersion: resourceEvidence.contractVersion,
  };
}

// count every terminal semantic classification
function classificationCounts(attempts) {
  const counts = new Map();
  // retain every terminal attempt once
  for (const attempt of attempts) {
    const key = JSON.stringify([
      attempt.semanticClassification,
      attempt.terminalCategory,
    ]);
    const existing = counts.get(key);
    // initialize one classification row
    if (existing === undefined) {
      counts.set(key, {
        classification: attempt.semanticClassification,
        count: 1,
        terminalCategory: attempt.terminalCategory,
      });
    } else {
      existing.count += 1;
    }
  }
  return [...counts.values()];
}

// prove one direct result belongs to one reviewed binding
function validateResultBinding(binding, result) {
  requireInvariant(
    binding.contractVersion === BACKEND_PERFORMANCE_DIRECT_BINDING_VERSION
      && binding.role === "baseline"
      && result.contractVersion === BACKEND_PERFORMANCE_DIRECT_RESULT_VERSION
      && result.role === "baseline"
      && binding.directConfiguration.generation === "node-v1"
      && result.generation === binding.directConfiguration.generation
      && result.runId === binding.directConfiguration.runId,
    "direct-pipeline.binding",
    "direct result role generation or run identity differs from its binding",
  );
  const expected = binding.directConfiguration.attempts.map(
    // retain one bound source identity
    (attempt) => ({
      attemptId: attempt.attemptId,
      operationId: attempt.operationId,
      runSlotId: attempt.runSlotId,
      stratum: attempt.stratum,
    }),
  );
  const actual = result.attempts.map(
    // retain one measured source identity
    (attempt) => ({
      attemptId: attempt.attemptId,
      operationId: attempt.operationId,
      runSlotId: attempt.runSlotId,
      stratum: attempt.stratum,
    }),
  );
  requireInvariant(
    JSON.stringify(actual) === JSON.stringify(expected)
      && result.summary.durationCoverage.observedAttempts === expected.length
      && result.summary.durationCoverage.plannedAttempts === expected.length,
    "direct-pipeline.denominator",
    "direct result does not cover the complete bound attempt order",
  );
}

// convert one measured result to one baseline receipt
export function buildBackendPerformanceDirectBaseline(binding, result, options) {
  requireInvariant(
    arguments.length === 3 && isRecord(options)
      && options.evidenceClass === "synthetic-fixture",
    "direct-pipeline.arguments",
    "direct pipeline currently emits synthetic fixture evidence only",
  );
  validateResultBinding(binding, result);
  const collectedAt = options.collectedAt;
  requireInvariant(
    collectedAt instanceof Date && Number.isFinite(collectedAt.getTime())
      && collectedAt.getTime() >= Date.parse(result.completedAt),
    "direct-pipeline.chronology",
    "baseline collection time precedes direct evaluation completion",
  );
  const sourceResultSha256 = receiptSha256(result);
  const metrics = [
    ...operationMetrics(result),
    ...resourceMetrics(result.resourceEvidence),
  ];
  return Object.freeze({
    attemptDenominator: {
      observedAttempts: result.summary.durationCoverage.observedAttempts,
      plannedAttempts: result.summary.durationCoverage.plannedAttempts,
      terminalCounts: result.summary.terminalCounts,
    },
    bundleSha256: receiptSha256({
      bindingWorkloadIdentity: binding.workloadIdentity,
      sourceResultSha256,
    }),
    classifications: classificationCounts(result.attempts),
    collectedAt: collectedAt.toISOString(),
    completedAt: result.completedAt,
    contractVersion: BACKEND_PERFORMANCE_DIRECT_BASELINE_VERSION,
    evidenceClass: options.evidenceClass,
    generation: "node-v1",
    metrics,
    resourceEvidenceSha256: result.resourceEvidence === null
      ? null
      : receiptSha256(result.resourceEvidence),
    resourceObservation: resourceObservation(result.resourceEvidence),
    role: "baseline",
    runId: result.runId,
    sourceResultSha256,
    workloadIdentitySha256: binding.workloadIdentity.value,
  });
}

// execute and convert one reviewed baseline binding
export async function collectBackendPerformanceDirectBaseline(binding, options = {}) {
  requireInvariant(
    arguments.length >= 1 && arguments.length <= 2 && isRecord(options),
    "direct-pipeline.arguments",
    "baseline collection accepts one binding and optional adapters",
  );
  const result = await evaluateBackendPerformanceDirect(
    binding.directConfiguration,
    options.evaluatorAdapters ?? {},
  );
  const collectedAt = options.collectedNow === undefined
    ? new Date()
    : options.collectedNow();
  return {
    baseline: buildBackendPerformanceDirectBaseline(binding, result, {
      collectedAt,
      evidenceClass: options.evidenceClass ?? "synthetic-fixture",
    }),
    result,
  };
}
