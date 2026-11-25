import { randomUUID } from "node:crypto";
import { constants } from "node:fs";
import {
  link,
  lstat,
  open,
  readFile,
  readdir,
  realpath,
  stat,
  unlink,
} from "node:fs/promises";
import { isAbsolute, resolve } from "node:path";

import {
  canonicalReceiptBytes,
  receiptSha256,
} from "./backend-performance-receipts.mjs";

export const BACKEND_PERFORMANCE_DIRECT_BASELINE_VERSION =
  "weather-backend-performance-direct-baseline/v1";
export const BACKEND_PERFORMANCE_DIRECT_TARGET_VERSION =
  "weather-backend-performance-direct-target/v1";
export const BACKEND_PERFORMANCE_DIRECT_TARGET_SELECTOR =
  "strict-uncertainty-next-unit/v1";

const TARGET_FILENAME = "target.json";
const SHA256_PATTERN = /^[a-f0-9]{64}$/;
const RUN_ID_PATTERN = /^[a-z0-9][a-z0-9.-]{0,127}$/;
const EVIDENCE_CLASSES = new Set(["synthetic-fixture"]);

// create one coded target error
function fail(code, message, cause) {
  const error = new Error(message, cause === undefined ? undefined : { cause });
  error.code = code;
  throw error;
}

// require one target invariant
function requireInvariant(condition, code, message) {
  // fail closed on malformed evidence
  if (!condition) {
    fail(code, message);
  }
}

// identify one plain record
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    && (Object.getPrototypeOf(value) === Object.prototype
      || Object.getPrototypeOf(value) === null);
}

// require one exact record shape
function requireExactKeys(value, keys, path) {
  requireInvariant(isRecord(value), "direct-target.schema", `${path} must be a record`);
  requireInvariant(
    JSON.stringify(Object.keys(value).sort()) === JSON.stringify([...keys].sort()),
    "direct-target.schema",
    `${path} has unexpected fields`,
  );
}

// require one canonical timestamp
function validateTimestamp(value, path) {
  requireInvariant(
    typeof value === "string" && new Date(value).toISOString() === value,
    "direct-target.chronology",
    `${path} is not a canonical timestamp`,
  );
  return Date.parse(value);
}

// validate one metric identity
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
  // require every identity component
  for (const [key, child] of Object.entries(value)) {
    requireInvariant(
      typeof child === "string" && child.length > 0 && child.length <= 256,
      "direct-target.metric",
      `${path}.${key} is invalid`,
    );
  }
  return { ...value };
}

// validate one repetition metric
function validateMetric(value, path) {
  requireExactKeys(value, ["identity", "physicalLowerBound", "value"], path);
  const identity = validateMetricIdentity(value.identity, `${path}.identity`);
  requireInvariant(
    Number.isSafeInteger(value.physicalLowerBound) && value.physicalLowerBound >= 0
      && Number.isSafeInteger(value.value) && value.value >= value.physicalLowerBound,
    "direct-target.metric",
    `${path} value or lower bound is invalid`,
  );
  return {
    identity,
    physicalLowerBound: value.physicalLowerBound,
    value: value.value,
  };
}

// validate one baseline receipt
function validateBaselineRun(value, index) {
  const path = `$.baselineRuns[${String(index)}]`;
  requireExactKeys(value, [
    "attemptDenominator",
    "bundleSha256",
    "classifications",
    "collectedAt",
    "completedAt",
    "contractVersion",
    "evidenceClass",
    "generation",
    "metrics",
    "resourceEvidenceSha256",
    "resourceObservation",
    "role",
    "runId",
    "sourceResultSha256",
    "workloadIdentitySha256",
  ], path);
  requireInvariant(
    value.contractVersion === BACKEND_PERFORMANCE_DIRECT_BASELINE_VERSION
      && value.role === "baseline" && value.generation === "node-v1"
      && EVIDENCE_CLASSES.has(value.evidenceClass),
    "direct-target.baseline",
    `${path} role generation version or evidence class is invalid`,
  );
  requireInvariant(
    Array.isArray(value.classifications) && value.classifications.length > 0,
    "direct-target.classification",
    `${path}.classifications is empty`,
  );
  let classifiedAttempts = 0;
  // validate every closed classification row
  for (let classificationIndex = 0;
    classificationIndex < value.classifications.length;
    classificationIndex += 1) {
    const classification = value.classifications[classificationIndex];
    requireExactKeys(
      classification,
      ["classification", "count", "terminalCategory"],
      `${path}.classifications[${String(classificationIndex)}]`,
    );
    requireInvariant(
      typeof classification.classification === "string"
        && classification.classification.length > 0
        && Number.isSafeInteger(classification.count) && classification.count > 0
        && ["completed", "error", "timeout"].includes(classification.terminalCategory),
      "direct-target.classification",
      `${path}.classifications[${String(classificationIndex)}] is invalid`,
    );
    classifiedAttempts += classification.count;
  }
  requireInvariant(
    classifiedAttempts === value.attemptDenominator.plannedAttempts,
    "direct-target.classification",
    `${path}.classifications does not cover the attempt denominator`,
  );
  requireInvariant(
    value.resourceObservation === null || isRecord(value.resourceObservation),
    "direct-target.resource",
    `${path}.resourceObservation is invalid`,
  );
  // validate one present resource observation
  if (value.resourceObservation !== null) {
    requireExactKeys(value.resourceObservation, [
      "cpuUsageUsec",
      "memoryPeakBytes",
      "memorySteadyMaximumBytes",
      "sourceContractVersion",
    ], `${path}.resourceObservation`);
    requireInvariant(
      Number.isSafeInteger(value.resourceObservation.cpuUsageUsec)
        && value.resourceObservation.cpuUsageUsec >= 0
        && Number.isSafeInteger(value.resourceObservation.memoryPeakBytes)
        && value.resourceObservation.memoryPeakBytes >= 0
        && Number.isSafeInteger(value.resourceObservation.memorySteadyMaximumBytes)
        && value.resourceObservation.memorySteadyMaximumBytes >= 0
        && typeof value.resourceObservation.sourceContractVersion === "string"
        && value.resourceObservation.sourceContractVersion.length > 0,
      "direct-target.resource",
      `${path}.resourceObservation is incomplete`,
    );
  }
  requireExactKeys(
    value.attemptDenominator,
    ["observedAttempts", "plannedAttempts", "terminalCounts"],
    `${path}.attemptDenominator`,
  );
  requireExactKeys(
    value.attemptDenominator.terminalCounts,
    ["completed", "error", "timeout"],
    `${path}.attemptDenominator.terminalCounts`,
  );
  const terminalTotal = Object.values(value.attemptDenominator.terminalCounts).reduce(
    // add one terminal category count
    (total, count) => total + count,
    0,
  );
  requireInvariant(
    Number.isSafeInteger(value.attemptDenominator.plannedAttempts)
      && value.attemptDenominator.plannedAttempts > 0
      && Number.isSafeInteger(value.attemptDenominator.observedAttempts)
      && Object.values(value.attemptDenominator.terminalCounts).every(
        // require one nonnegative terminal count
        (count) => Number.isSafeInteger(count) && count >= 0,
      )
      && value.attemptDenominator.observedAttempts === value.attemptDenominator.plannedAttempts
      && terminalTotal === value.attemptDenominator.plannedAttempts,
    "direct-target.denominator",
    `${path} attempt denominator is incomplete`,
  );
  requireInvariant(
    typeof value.runId === "string" && RUN_ID_PATTERN.test(value.runId),
    "direct-target.baseline",
    `${path}.runId is invalid`,
  );
  requireInvariant(
    typeof value.bundleSha256 === "string" && SHA256_PATTERN.test(value.bundleSha256)
      && typeof value.workloadIdentitySha256 === "string"
      && SHA256_PATTERN.test(value.workloadIdentitySha256)
      && typeof value.sourceResultSha256 === "string"
      && SHA256_PATTERN.test(value.sourceResultSha256)
      && (value.resourceEvidenceSha256 === null
        || (typeof value.resourceEvidenceSha256 === "string"
          && SHA256_PATTERN.test(value.resourceEvidenceSha256))),
    "direct-target.baseline",
    `${path} identity digest is invalid`,
  );
  const completedAt = validateTimestamp(value.completedAt, `${path}.completedAt`);
  const collectedAt = validateTimestamp(value.collectedAt, `${path}.collectedAt`);
  requireInvariant(
    completedAt <= collectedAt,
    "direct-target.chronology",
    `${path} was collected before measurement completion`,
  );
  requireInvariant(
    Array.isArray(value.metrics) && value.metrics.length > 0,
    "direct-target.metric",
    `${path}.metrics is empty`,
  );
  const metrics = value.metrics.map(
    // validate every baseline metric
    (metric, metricIndex) => validateMetric(metric, `${path}.metrics[${String(metricIndex)}]`),
  );
  requireInvariant(
    new Set(metrics.map(
      // retain one metric identity
      (metric) => metric.identity.metricId,
    )).size === metrics.length,
    "direct-target.metric",
    `${path}.metrics contains duplicate identities`,
  );
  return {
    attemptDenominator: value.attemptDenominator,
    bundleSha256: value.bundleSha256,
    classifications: value.classifications,
    collectedAt: value.collectedAt,
    collectedAtMilliseconds: collectedAt,
    completedAt: value.completedAt,
    contractVersion: value.contractVersion,
    evidenceClass: value.evidenceClass,
    generation: value.generation,
    metrics,
    resourceEvidenceSha256: value.resourceEvidenceSha256,
    resourceObservation: value.resourceObservation,
    role: value.role,
    runId: value.runId,
    sourceResultSha256: value.sourceResultSha256,
    workloadIdentitySha256: value.workloadIdentitySha256,
  };
}

// compute one exact integer square root
function integerSqrt(value) {
  requireInvariant(value >= 0n, "direct-target.arithmetic", "square-root input is negative");
  // preserve the two smallest roots
  if (value < 2n) {
    return value;
  }
  let previous = value;
  let current = (previous + value / previous) / 2n;
  // converge without floating point
  while (current < previous) {
    previous = current;
    current = (previous + value / previous) / 2n;
  }
  return previous;
}

// derive one fixed lower-is-better target
function deriveMetricTarget(rows) {
  const sortedValues = rows.map(
    // retain one exact integer observation
    (row) => row.value,
  ).sort(
    // sort safe integer values
    (left, right) => left - right,
  );
  const baselinePoint = sortedValues[2];
  const maximum = sortedValues[4];
  const sum = rows.reduce(
    // add one exact integer observation
    (total, row) => total + BigInt(row.value),
    0n,
  );
  const radicand = rows.reduce(
    // add one scaled squared deviation
    (total, row) => {
      const deviation = 5n * BigInt(row.value) - sum;
      return total + deviation * deviation;
    },
    0n,
  );
  const sdFloor = Number(integerSqrt(radicand) / 10n);
  requireInvariant(
    Number.isSafeInteger(sdFloor),
    "direct-target.arithmetic",
    `${rows[0].identity.metricId} dispersion exceeds the safe integer range`,
  );
  const intervalGap = maximum - baselinePoint;
  const strictImprovement = Math.max(sdFloor, intervalGap) + 1;
  const threshold = baselinePoint - strictImprovement;
  requireInvariant(
    threshold > 0 && threshold >= rows[0].physicalLowerBound,
    "direct-target.unstable",
    `${rows[0].identity.metricId} cannot produce a positive physical target`,
  );
  return {
    baselinePoint,
    baselineValues: rows.map(
      // retain source run order
      (row) => row.value,
    ),
    comparator: "candidate-value-less-than-or-equal",
    identity: rows[0].identity,
    interval: {
      attainedCoverage: "0.96875",
      lowerBound: rows[0].physicalLowerBound,
      method: "exact-binomial-one-sided-order-statistic-median-n5",
      requestedCoverage: "0.95",
      upperBound: maximum,
    },
    physicalLowerBound: rows[0].physicalLowerBound,
    sampleSdProof: {
      divisor: 10,
      floor: sdFloor,
      radicandDecimal: radicand.toString(10),
    },
    selector: BACKEND_PERFORMANCE_DIRECT_TARGET_SELECTOR,
    strictImprovement,
    threshold,
  };
}

// validate the exact five-run cohort
function validateCohort(value, frozenAtMilliseconds) {
  requireInvariant(
    Array.isArray(value) && value.length === 5,
    "direct-target.baseline-count",
    "target derivation requires exactly five baseline runs",
  );
  const runs = value.map(
    // validate one ordered baseline run
    (run, index) => validateBaselineRun(run, index),
  );
  requireInvariant(
    new Set(runs.map(
      // retain one run identity
      (run) => run.runId,
    )).size === 5 && new Set(runs.map(
      // retain one baseline bundle identity
      (run) => run.bundleSha256,
    )).size === 5,
    "direct-target.baseline",
    "baseline run or bundle identities are not independent",
  );
  const first = runs[0];
  const firstMetricIdentities = first.metrics.map(
    // retain one metric identity and bound
    (metric) => ({ identity: metric.identity, physicalLowerBound: metric.physicalLowerBound }),
  );
  // compare all five fixed-condition runs
  for (let index = 0; index < runs.length; index += 1) {
    const run = runs[index];
    requireInvariant(
      run.evidenceClass === first.evidenceClass
        && run.workloadIdentitySha256 === first.workloadIdentitySha256
        && JSON.stringify(run.classifications) === JSON.stringify(first.classifications)
        && JSON.stringify(run.metrics.map(
          // retain one comparable metric identity and bound
          (metric) => ({ identity: metric.identity, physicalLowerBound: metric.physicalLowerBound }),
        )) === JSON.stringify(firstMetricIdentities),
      "direct-target.matched",
      `baseline run ${run.runId} does not match the fixed cohort`,
    );
    requireInvariant(
      run.collectedAtMilliseconds <= frozenAtMilliseconds
        && (index === 0 || runs[index - 1].collectedAtMilliseconds <= run.collectedAtMilliseconds),
      "direct-target.chronology",
      `baseline run ${run.runId} is outside the target chronology`,
    );
  }
  return runs;
}

// derive one source-owned target without caller thresholds
export function deriveBackendPerformanceDirectTarget(input, injected = {}) {
  requireInvariant(
    arguments.length >= 1 && arguments.length <= 2,
    "direct-target.arguments",
    "target derivation accepts baseline runs and an optional clock",
  );
  requireExactKeys(input, ["baselineRuns", "contractVersion"], "$" );
  requireInvariant(
    input.contractVersion === "weather-backend-performance-direct-target-input/v1",
    "direct-target.version",
    "target derivation input version is invalid",
  );
  requireInvariant(
    isRecord(injected) && (injected.now === undefined || typeof injected.now === "function"),
    "direct-target.arguments",
    "target derivation clock is invalid",
  );
  const frozenDate = injected.now === undefined ? new Date() : injected.now();
  requireInvariant(
    frozenDate instanceof Date && Number.isFinite(frozenDate.getTime()),
    "direct-target.chronology",
    "target frozen time is invalid",
  );
  const frozenAt = frozenDate.toISOString();
  const runs = validateCohort(input.baselineRuns, frozenDate.getTime());
  const metricTargets = runs[0].metrics.map(
    // derive one matched metric target
    (_, metricIndex) => deriveMetricTarget(runs.map(
      // select one run's matched metric
      (run) => run.metrics[metricIndex],
    )),
  );
  return Object.freeze({
    baseline: {
      bundleSha256s: runs.map(
        // retain one baseline bundle identity
        (run) => run.bundleSha256,
      ),
      receiptSha256s: input.baselineRuns.map(
        // bind one canonical baseline receipt
        (run) => receiptSha256(run),
      ),
      runIds: runs.map(
        // retain one baseline run identity
        (run) => run.runId,
      ),
    },
    contractVersion: BACKEND_PERFORMANCE_DIRECT_TARGET_VERSION,
    evidenceClass: runs[0].evidenceClass,
    frozenAt,
    generation: "node-v1",
    metricTargets,
    selector: BACKEND_PERFORMANCE_DIRECT_TARGET_SELECTOR,
    workloadIdentitySha256: runs[0].workloadIdentitySha256,
  });
}

// validate one private evidence directory
async function validatePrivateDirectory(directoryPath) {
  requireInvariant(
    typeof directoryPath === "string" && isAbsolute(directoryPath)
      && resolve(directoryPath) === directoryPath && await realpath(directoryPath) === directoryPath,
    "direct-target.path",
    "evidence directory must be an absolute canonical path",
  );
  const identity = await lstat(directoryPath, { bigint: true });
  const uid = typeof process.getuid === "function" ? BigInt(process.getuid()) : identity.uid;
  requireInvariant(
    identity.isDirectory() && !identity.isSymbolicLink() && identity.uid === uid
      && (identity.mode & 0o777n) === 0o700n,
    "direct-target.path",
    "evidence directory must be private and owned by the current user",
  );
}

// publish one canonical private file atomically
async function writeCreateOnce(directoryPath, filename, document) {
  const finalPath = resolve(directoryPath, filename);
  const temporaryPath = resolve(
    directoryPath,
    `.${filename}.tmp-${String(process.pid)}-${randomUUID()}`,
  );
  const bytes = canonicalReceiptBytes(document);
  let temporaryCreated = false;
  try {
    const handle = await open(
      temporaryPath,
      constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW,
      0o600,
    );
    temporaryCreated = true;
    try {
      await handle.writeFile(bytes);
      await handle.sync();
    } finally {
      await handle.close();
    }
    try {
      await link(temporaryPath, finalPath);
    } catch (error) {
      // classify a prior immutable receipt
      if (error?.code === "EEXIST") {
        fail("direct-target.exists", `${filename} already exists`, error);
      }
      throw error;
    }
    await unlink(temporaryPath);
    temporaryCreated = false;
    const directory = await open(directoryPath, constants.O_RDONLY | constants.O_DIRECTORY);
    try {
      await directory.sync();
    } finally {
      await directory.close();
    }
    const identity = await stat(finalPath, { bigint: true });
    requireInvariant(
      identity.isFile() && identity.nlink === 1n && (identity.mode & 0o777n) === 0o600n
        && identity.size === BigInt(bytes.length),
      "direct-target.write",
      `${filename} has invalid durable identity`,
    );
    requireInvariant(
      (await readFile(finalPath)).equals(bytes),
      "direct-target.write",
      `${filename} differs after publication`,
    );
  } finally {
    // remove only this unpublished temporary file
    if (temporaryCreated) {
      await unlink(temporaryPath).catch(
        // preserve the primary publication failure
        () => {},
      );
    }
  }
  return Object.freeze({ receiptSha256: receiptSha256(bytes) });
}

// persist one immutable baseline receipt
export async function writeBackendPerformanceDirectBaseline(directoryPath, baselineRun) {
  requireInvariant(
    arguments.length === 2,
    "direct-target.arguments",
    "baseline writer accepts a directory and one baseline run",
  );
  await validatePrivateDirectory(directoryPath);
  const run = validateBaselineRun(baselineRun, 0);
  return writeCreateOnce(directoryPath, `baseline-${run.runId}.json`, baselineRun);
}

// persist one immutable target after all five baselines
export async function writeBackendPerformanceDirectTarget(directoryPath, input, injected = {}) {
  requireInvariant(
    arguments.length >= 2 && arguments.length <= 3,
    "direct-target.arguments",
    "target writer accepts a directory baseline cohort and optional clock",
  );
  await validatePrivateDirectory(directoryPath);
  requireExactKeys(input, ["baselineRuns", "contractVersion"], "$" );
  requireInvariant(
    Array.isArray(input.baselineRuns) && input.baselineRuns.length === 5,
    "direct-target.baseline-count",
    "target writer requires five baseline receipts",
  );
  // prove every canonical baseline receipt is already durable
  for (const run of input.baselineRuns) {
    const path = resolve(directoryPath, `baseline-${run.runId}.json`);
    requireInvariant(
      (await readFile(path)).equals(canonicalReceiptBytes(run)),
      "direct-target.baseline",
      `durable baseline ${run.runId} is missing or differs`,
    );
  }
  requireInvariant(
    !(await readdir(directoryPath)).some(
      // reject any earlier candidate result
      (name) => name.startsWith("candidate-"),
    ),
    "direct-target.chronology",
    "candidate evidence exists before target publication",
  );
  const target = deriveBackendPerformanceDirectTarget(input, injected);
  const publication = await writeCreateOnce(directoryPath, TARGET_FILENAME, target);
  requireInvariant(
    !(await readdir(directoryPath)).some(
      // reject a concurrent earlier candidate result
      (name) => name.startsWith("candidate-"),
    ),
    "direct-target.chronology",
    "candidate evidence overlaps target publication",
  );
  return Object.freeze({ ...publication, target });
}
