import { lstat, readFile, realpath } from "node:fs/promises";
import { isAbsolute, resolve } from "node:path";

import {
  BACKEND_PERFORMANCE_DIRECT_BINDING_PAIR_VERSION,
  BACKEND_PERFORMANCE_DIRECT_BINDING_VERSION,
} from "./backend-performance-direct-binding.mjs";
import {
  BACKEND_PERFORMANCE_DIRECT_TARGET_VERSION,
  deriveBackendPerformanceDirectTarget,
} from "./backend-performance-direct-target.mjs";
import {
  canonicalReceiptBytes,
  receiptSha256,
} from "./backend-performance-receipts.mjs";

export const BACKEND_PERFORMANCE_DIRECT_CANDIDATE_VERSION =
  "weather-backend-performance-direct-candidate/v1";
export const BACKEND_PERFORMANCE_DIRECT_COMPARISON_VERSION =
  "weather-backend-performance-direct-comparison/v1";

const SHA256_PATTERN = /^[a-f0-9]{64}$/u;
const RUN_ID_PATTERN = /^[a-z0-9][a-z0-9.-]{0,127}$/u;
const CAPACITY_GAP_REASONS = new Set([
  "measurement-capacity-unavailable",
  "resource-provenance-unavailable",
]);

// create one coded comparison error
function fail(code, message, cause) {
  const error = new Error(message, cause === undefined ? undefined : { cause });
  error.code = code;
  throw error;
}

// require one comparison invariant
function requireInvariant(condition, code, message) {
  // fail closed on malformed comparison evidence
  if (!condition) {
    fail(code, message);
  }
}

// identify one plain comparison record
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    && (Object.getPrototypeOf(value) === Object.prototype
      || Object.getPrototypeOf(value) === null);
}

// require one exact comparison record shape
function requireExactKeys(value, keys, path) {
  requireInvariant(isRecord(value), "direct-comparison.schema", `${path} must be a record`);
  requireInvariant(
    JSON.stringify(Object.keys(value).sort()) === JSON.stringify([...keys].sort()),
    "direct-comparison.schema",
    `${path} has unexpected fields`,
  );
}

// validate one canonical comparison timestamp
function validateTimestamp(value, path) {
  requireInvariant(
    typeof value === "string" && new Date(value).toISOString() === value,
    "direct-comparison.chronology",
    `${path} is not a canonical timestamp`,
  );
  return Date.parse(value);
}

// validate one private evidence directory
async function validatePrivateDirectory(directoryPath) {
  requireInvariant(
    typeof directoryPath === "string" && isAbsolute(directoryPath)
      && resolve(directoryPath) === directoryPath && await realpath(directoryPath) === directoryPath,
    "direct-comparison.path",
    "evidence directory must be an absolute canonical path",
  );
  const identity = await lstat(directoryPath, { bigint: true });
  const uid = typeof process.getuid === "function" ? BigInt(process.getuid()) : identity.uid;
  requireInvariant(
    identity.isDirectory() && !identity.isSymbolicLink() && identity.uid === uid
      && (identity.mode & 0o777n) === 0o700n,
    "direct-comparison.path",
    "evidence directory must be private and owned by the current user",
  );
}

// read one exact canonical json document
async function readCanonicalDocument(path, code) {
  let bytes;
  try {
    bytes = await readFile(path);
  } catch (error) {
    fail(code, `${path} is unavailable`, error);
  }
  let value;
  try {
    value = JSON.parse(bytes.toString("utf8"));
  } catch (error) {
    fail(code, `${path} is not JSON`, error);
  }
  requireInvariant(
    bytes.equals(canonicalReceiptBytes(value)),
    code,
    `${path} is not canonical`,
  );
  return { bytes, value };
}

// reload and reproduce one durable create-once target
async function loadDurableTarget(directoryPath) {
  await validatePrivateDirectory(directoryPath);
  const targetPath = resolve(directoryPath, "target.json");
  const targetDocument = await readCanonicalDocument(
    targetPath,
    "direct-comparison.target",
  );
  const target = targetDocument.value;
  requireExactKeys(target, [
    "baseline",
    "contractVersion",
    "evidenceClass",
    "frozenAt",
    "generation",
    "metricTargets",
    "selector",
    "workloadIdentitySha256",
  ], "$.target");
  requireInvariant(
    target.contractVersion === BACKEND_PERFORMANCE_DIRECT_TARGET_VERSION
      && target.evidenceClass === "synthetic-fixture"
      && target.generation === "node-v1"
      && Array.isArray(target.baseline?.runIds)
      && target.baseline.runIds.length === 5
      && target.baseline.runIds.every(
        // retain only safe baseline receipt names
        (runId) => typeof runId === "string" && RUN_ID_PATTERN.test(runId),
      ),
    "direct-comparison.target",
    "durable target identity or baseline references are invalid",
  );
  const baselineRuns = [];
  // reload every target-owned baseline receipt
  for (const runId of target.baseline.runIds) {
    const document = await readCanonicalDocument(
      resolve(directoryPath, `baseline-${runId}.json`),
      "direct-comparison.baseline",
    );
    baselineRuns.push(document.value);
  }
  const reproduced = deriveBackendPerformanceDirectTarget({
    baselineRuns,
    contractVersion: "weather-backend-performance-direct-target-input/v1",
  }, {
    now: () => new Date(target.frozenAt),
  });
  requireInvariant(
    targetDocument.bytes.equals(canonicalReceiptBytes(reproduced)),
    "direct-comparison.target",
    "durable target does not reproduce from its baseline receipts",
  );
  const identity = await lstat(targetPath, { bigint: true });
  const uid = typeof process.getuid === "function" ? BigInt(process.getuid()) : identity.uid;
  requireInvariant(
    identity.isFile() && !identity.isSymbolicLink() && identity.uid === uid
      && identity.nlink === 1n && (identity.mode & 0o777n) === 0o600n,
    "direct-comparison.target",
    "durable target file identity is invalid",
  );
  return {
    baselineRuns,
    frozenAtMilliseconds: validateTimestamp(target.frozenAt, "$.target.frozenAt"),
    publishedAtMilliseconds: Number((identity.mtimeNs + 999_999n) / 1_000_000n),
    receiptSha256: receiptSha256(targetDocument.bytes),
    target,
  };
}

// validate one candidate metric identity
function validateMetricIdentity(value, path) {
  requireExactKeys(value, [
    "metricId",
    "scope",
    "service",
    "statistic",
    "stratum",
    "unit",
    "workload",
  ], path);
  // require every closed identity component
  for (const [key, child] of Object.entries(value)) {
    requireInvariant(
      typeof child === "string" && child.length > 0 && child.length <= 256,
      "direct-comparison.metric",
      `${path}.${key} is invalid`,
    );
  }
  return { ...value };
}

// validate one measured candidate metric
function validateMetric(value, index) {
  const path = `$.candidate.metrics[${String(index)}]`;
  requireExactKeys(value, ["identity", "physicalLowerBound", "value"], path);
  const identity = validateMetricIdentity(value.identity, `${path}.identity`);
  requireInvariant(
    Number.isSafeInteger(value.physicalLowerBound) && value.physicalLowerBound >= 0
      && Number.isSafeInteger(value.value) && value.value >= value.physicalLowerBound,
    "direct-comparison.metric",
    `${path} value or lower bound is invalid`,
  );
  return { identity, physicalLowerBound: value.physicalLowerBound, value: value.value };
}

// validate one complete candidate attempt denominator
function validateAttemptDenominator(value) {
  requireExactKeys(
    value,
    ["observedAttempts", "plannedAttempts", "terminalCounts"],
    "$.candidate.attemptDenominator",
  );
  requireExactKeys(
    value.terminalCounts,
    ["completed", "error", "timeout"],
    "$.candidate.attemptDenominator.terminalCounts",
  );
  const counts = value.terminalCounts;
  requireInvariant(
    Number.isSafeInteger(value.plannedAttempts) && value.plannedAttempts > 0
      && value.observedAttempts === value.plannedAttempts
      && Object.values(counts).every(
        // require one nonnegative terminal category count
        (count) => Number.isSafeInteger(count) && count >= 0,
      )
      && counts.completed + counts.error + counts.timeout === value.plannedAttempts,
    "direct-comparison.denominator",
    "candidate attempt denominator is incomplete",
  );
  return value;
}

// validate candidate semantic classifications against the denominator
function validateClassifications(value, attemptDenominator) {
  requireInvariant(
    Array.isArray(value) && value.length > 0,
    "direct-comparison.classification",
    "candidate classifications are empty",
  );
  const counts = { completed: 0, error: 0, timeout: 0 };
  // validate and count every candidate classification
  for (let index = 0; index < value.length; index += 1) {
    const row = value[index];
    requireExactKeys(
      row,
      ["classification", "count", "terminalCategory"],
      `$.candidate.classifications[${String(index)}]`,
    );
    requireInvariant(
      typeof row.classification === "string" && row.classification.length > 0
        && Number.isSafeInteger(row.count) && row.count > 0
        && Object.hasOwn(counts, row.terminalCategory),
      "direct-comparison.classification",
      `candidate classification ${String(index)} is invalid`,
    );
    counts[row.terminalCategory] += row.count;
  }
  requireInvariant(
    JSON.stringify(counts) === JSON.stringify(attemptDenominator.terminalCounts),
    "direct-comparison.denominator",
    "candidate classifications do not cover every terminal category",
  );
  return value;
}

// validate one complete candidate measurement receipt
function validateCandidateReceipt(value) {
  requireExactKeys(value, [
    "attemptDenominator",
    "capacityGaps",
    "classifications",
    "collectedAt",
    "completedAt",
    "contractVersion",
    "evidenceClass",
    "generation",
    "metrics",
    "role",
    "runId",
    "sourceResultSha256",
    "startedAt",
    "targetReceiptSha256",
    "workloadIdentitySha256",
  ], "$.candidate");
  requireInvariant(
    value.contractVersion === BACKEND_PERFORMANCE_DIRECT_CANDIDATE_VERSION
      && value.evidenceClass === "synthetic-fixture"
      && value.role === "candidate"
      && typeof value.generation === "string" && value.generation.length > 0
      && typeof value.runId === "string" && RUN_ID_PATTERN.test(value.runId)
      && SHA256_PATTERN.test(value.sourceResultSha256)
      && SHA256_PATTERN.test(value.targetReceiptSha256)
      && SHA256_PATTERN.test(value.workloadIdentitySha256),
    "direct-comparison.candidate",
    "candidate role version identity or evidence class is invalid",
  );
  const startedAtMilliseconds = validateTimestamp(value.startedAt, "$.candidate.startedAt");
  const completedAtMilliseconds = validateTimestamp(value.completedAt, "$.candidate.completedAt");
  const collectedAtMilliseconds = validateTimestamp(value.collectedAt, "$.candidate.collectedAt");
  requireInvariant(
    startedAtMilliseconds <= completedAtMilliseconds
      && completedAtMilliseconds <= collectedAtMilliseconds,
    "direct-comparison.chronology",
    "candidate measurement chronology is invalid",
  );
  const attemptDenominator = validateAttemptDenominator(value.attemptDenominator);
  const classifications = validateClassifications(value.classifications, attemptDenominator);
  requireInvariant(
    Array.isArray(value.metrics) && Array.isArray(value.capacityGaps),
    "direct-comparison.metric",
    "candidate metrics or capacity gaps are invalid",
  );
  const metrics = value.metrics.map(
    // validate every measured candidate metric
    (metric, index) => validateMetric(metric, index),
  );
  const capacityGaps = value.capacityGaps.map(
    // validate every explicit capacity gap
    (gap, index) => {
      requireExactKeys(
        gap,
        ["metricId", "reason"],
        `$.candidate.capacityGaps[${String(index)}]`,
      );
      requireInvariant(
        typeof gap.metricId === "string" && gap.metricId.length > 0
          && CAPACITY_GAP_REASONS.has(gap.reason),
        "direct-comparison.capacity",
        `candidate capacity gap ${String(index)} is invalid`,
      );
      return { ...gap };
    },
  );
  const accountedMetricIds = [
    ...metrics.map(
      // retain one measured metric identity
      (metric) => metric.identity.metricId,
    ),
    ...capacityGaps.map(
      // retain one unavailable metric identity
      (gap) => gap.metricId,
    ),
  ];
  requireInvariant(
    new Set(accountedMetricIds).size === accountedMetricIds.length,
    "direct-comparison.metric",
    "candidate metric identities are duplicated",
  );
  return {
    ...value,
    attemptDenominator,
    capacityGaps,
    classifications,
    collectedAtMilliseconds,
    completedAtMilliseconds,
    metrics,
    startedAtMilliseconds,
  };
}

// validate the matched fixture bindings and candidate measurement identity
function validateBindings(bindingPair, candidate, target) {
  requireInvariant(
    isRecord(bindingPair)
      && bindingPair.contractVersion === BACKEND_PERFORMANCE_DIRECT_BINDING_PAIR_VERSION
      && bindingPair.executionState === "matched-bindings-only-no-results"
      && bindingPair.baseline?.contractVersion === BACKEND_PERFORMANCE_DIRECT_BINDING_VERSION
      && bindingPair.candidate?.contractVersion === BACKEND_PERFORMANCE_DIRECT_BINDING_VERSION
      && bindingPair.baseline.role === "baseline"
      && bindingPair.candidate.role === "candidate"
      && bindingPair.baseline.directConfiguration?.generation === "node-v1"
      && bindingPair.candidate.directConfiguration?.generation === "rust-v1"
      && bindingPair.matchedWorkloadIdentity?.value === target.workloadIdentitySha256
      && bindingPair.baseline.workloadIdentity?.value === target.workloadIdentitySha256
      && bindingPair.candidate.workloadIdentity?.value === target.workloadIdentitySha256
      && candidate.workloadIdentitySha256 === target.workloadIdentitySha256
      && candidate.generation === "rust-v1"
      && candidate.generation === bindingPair.candidate.directConfiguration.generation
      && candidate.runId === bindingPair.candidate.directConfiguration?.runId,
    "direct-comparison.binding",
    "target candidate or paired binding identities differ",
  );
  requireInvariant(
    candidate.attemptDenominator.plannedAttempts
      === bindingPair.candidate.directConfiguration.attempts.length,
    "direct-comparison.denominator",
    "candidate denominator differs from its bound attempt plan",
  );
}

// compare one synthetic candidate against one durable frozen target
export async function compareBackendPerformanceDirectCandidate(
  directoryPath,
  bindingPair,
  candidateReceipt,
) {
  requireInvariant(
    arguments.length === 3,
    "direct-comparison.arguments",
    "comparison accepts a directory matched bindings and one candidate receipt",
  );
  const durable = await loadDurableTarget(directoryPath);
  const candidate = validateCandidateReceipt(candidateReceipt);
  validateBindings(bindingPair, candidate, durable.target);
  requireInvariant(
    candidate.targetReceiptSha256 === durable.receiptSha256,
    "direct-comparison.target",
    "candidate was not bound to the durable target receipt",
  );
  requireInvariant(
    candidate.startedAtMilliseconds >= durable.frozenAtMilliseconds
      && candidate.startedAtMilliseconds >= durable.publishedAtMilliseconds,
    "direct-comparison.chronology",
    "durable target was not published before candidate measurement",
  );
  const metricsById = new Map(candidate.metrics.map(
    // index one measured candidate metric
    (metric) => [metric.identity.metricId, metric],
  ));
  const gapsById = new Map(candidate.capacityGaps.map(
    // index one explicit candidate capacity gap
    (gap) => [gap.metricId, gap],
  ));
  const targetMetricIds = durable.target.metricTargets.map(
    // retain one frozen target metric identity
    (metricTarget) => metricTarget.identity.metricId,
  );
  requireInvariant(
    candidate.metrics.length + candidate.capacityGaps.length === targetMetricIds.length
      && candidate.metrics.every(
        // reject extra measured candidate metrics
        (metric) => targetMetricIds.includes(metric.identity.metricId),
      )
      && candidate.capacityGaps.every(
        // reject extra capacity gap identities
        (gap) => targetMetricIds.includes(gap.metricId),
      ),
    "direct-comparison.metric",
    "candidate does not account for every target metric exactly once",
  );
  const metricResults = durable.target.metricTargets.map(
    // evaluate one frozen target without accepting caller thresholds
    (targetMetric) => {
      const metricId = targetMetric.identity.metricId;
      const measured = metricsById.get(metricId);
      // retain one explicit unavailable metric
      if (measured === undefined) {
        const gap = gapsById.get(metricId);
        requireInvariant(
          gap !== undefined,
          "direct-comparison.metric",
          `${metricId} is absent from the candidate denominator`,
        );
        return {
          candidateValue: null,
          capacityGap: gap.reason,
          identity: targetMetric.identity,
          margin: null,
          status: "capacity-gap",
          threshold: targetMetric.threshold,
        };
      }
      requireInvariant(
        JSON.stringify(measured.identity) === JSON.stringify(targetMetric.identity)
          && measured.physicalLowerBound === targetMetric.physicalLowerBound,
        "direct-comparison.metric",
        `${metricId} identity or physical bound differs from its target`,
      );
      const passed = measured.value <= targetMetric.threshold;
      return {
        candidateValue: measured.value,
        capacityGap: null,
        identity: targetMetric.identity,
        margin: targetMetric.threshold - measured.value,
        status: passed ? "pass" : "fail",
        threshold: targetMetric.threshold,
      };
    },
  );
  const terminalFailure = candidate.attemptDenominator.terminalCounts.error > 0
    || candidate.attemptDenominator.terminalCounts.timeout > 0;
  const metricFailure = metricResults.some(
    // detect one strict target failure
    (metric) => metric.status === "fail",
  );
  const capacityGap = metricResults.some(
    // detect one explicit missing measurement capability
    (metric) => metric.status === "capacity-gap",
  );
  const outcome = terminalFailure || metricFailure
    ? "fail"
    : capacityGap
      ? "capacity-gap"
      : "pass";
  return Object.freeze({
    attemptDenominator: candidate.attemptDenominator,
    candidate: {
      generation: candidate.generation,
      runId: candidate.runId,
      sourceResultSha256: candidate.sourceResultSha256,
    },
    classifications: candidate.classifications,
    contractVersion: BACKEND_PERFORMANCE_DIRECT_COMPARISON_VERSION,
    evidenceClass: "synthetic-fixture",
    metricDenominator: {
      capacityGaps: candidate.capacityGaps.length,
      measured: candidate.metrics.length,
      targeted: durable.target.metricTargets.length,
    },
    metricResults,
    nativeEligible: false,
    outcome,
    strictPass: outcome === "pass",
    target: {
      frozenAt: durable.target.frozenAt,
      receiptSha256: durable.receiptSha256,
      selector: durable.target.selector,
    },
    terminalStatus: terminalFailure ? "fail" : "pass",
    workloadIdentitySha256: durable.target.workloadIdentitySha256,
  });
}
