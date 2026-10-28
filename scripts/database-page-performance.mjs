import { Agent, request as httpRequest } from "node:http";
import { execFile } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  lstat,
  mkdir,
  readFile,
  writeFile,
} from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { isDeepStrictEqual } from "node:util";
import { promisify } from "node:util";
import { pathToFileURL } from "node:url";

export const CONFIG_CONTRACT_VERSION = "weather-page-performance-config/v1";
export const EVIDENCE_CONTRACT_VERSION = "weather-page-performance-evidence/v1";
export const RESULT_CONTRACT_VERSION = "weather-page-performance-result/v1";
export const STATE_WITNESS_CONTRACT_VERSION = "weather-page-state-witness/v1";
export const SUPERVISOR_CONFIG_CONTRACT_VERSION = "weather-page-supervisor-config/v1";
export const SUPERVISOR_PRODUCER = "bundled-docker-supervisor/v1";
export const WARM_P95_LIMIT_MS = 1_000;
export const COLD_LIMIT_MS = 2_000;
export const WARM_RUNS = 3;
export const WARM_SAMPLES_PER_RUN = 40;
export const COLD_SAMPLES_PER_STRATUM = 3;
export const OBSERVATION_CACHE_TTL_MS = 5 * 60_000;

const sitePrefix = "/api/v1/sites/ballydidean";
const executeFile = promisify(execFile);

const rawEndpointInventory = [
  { id: "current", path: `${sitePrefix}/current`, horizon: "none", eligibility: "unconditional", applicationCache: false },
  { id: "daily-precipitation", path: `${sitePrefix}/daily-precipitation`, horizon: "today", eligibility: "unconditional", applicationCache: false },
  { id: "forecast-default", path: `${sitePrefix}/forecast`, horizon: "default", eligibility: "unconditional", applicationCache: false },
  { id: "forecast-1d", path: `${sitePrefix}/forecast?days=1`, horizon: "1d", eligibility: "unconditional", applicationCache: false },
  { id: "forecast-5d", path: `${sitePrefix}/forecast?days=5`, horizon: "5d", eligibility: "unconditional", applicationCache: false },
  { id: "forecast-10d", path: `${sitePrefix}/forecast?days=10`, horizon: "10d", eligibility: "unconditional", applicationCache: false },
  { id: "tides", path: `${sitePrefix}/tides`, horizon: "bounded", eligibility: "unconditional", applicationCache: false },
  { id: "forecast-observations", path: `${sitePrefix}/forecast-observations`, horizon: "fixed-history", eligibility: "unconditional", applicationCache: true },
  { id: "solar-cloud-evidence", path: `${sitePrefix}/solar-cloud-evidence`, horizon: "fixed-history", eligibility: "unconditional", applicationCache: true },
  { id: "viewer-context", path: "/api/v1/viewer-context", horizon: "none", eligibility: "unconditional", applicationCache: true },
  { id: "property-sensor-layout", path: `${sitePrefix}/property-sensor-layout`, horizon: "none", eligibility: "eligible-live", applicationCache: false },
];

// freeze one inventory entry
function freezeEndpoint(endpoint) {
  return Object.freeze({ ...endpoint });
}

export const ENDPOINT_INVENTORY = Object.freeze(rawEndpointInventory.map(freezeEndpoint));
export const REQUIRED_TIERS = Object.freeze(["warm", "K0", "K1", "K2", "K3"]);

const endpointById = new Map();

// index the immutable endpoint inventory
for (const endpoint of ENDPOINT_INVENTORY) {
  endpointById.set(endpoint.id, endpoint);
}

const identityFields = Object.freeze([
  "release",
  "sourceCommit",
  "webImage",
  "apiImage",
  "databaseImage",
  "schemaVersion",
  "querySetIdentity",
]);
const assignmentEnvironment = Object.freeze({
  warm: "blueberry",
  K0: "blueberry",
  K1: "blueberry",
  K2: null,
  K3: "disposable",
});
const forbiddenConfigurationKeys = /(?:authorization|cookie|credential|headers?|password|providerPayload|requestBody|responseBody|secret|token)/iu;
const allowedConfigurationKeys = new Set([
  "collections",
  "concurrency",
  "contractVersion",
  "evidenceFiles",
  "measurement",
  "requestTimeoutMs",
  "runtimeAssignments",
  "runtimeIdentities",
  "stateWitnessFiles",
]);
const allowedCollectionKeys = new Set([
  "cacheStateId",
  "count",
  "eligibility",
  "endpointId",
  "origin",
  "run",
  "runtimeId",
  "tier",
  "topology",
  "witnessId",
]);
const allowedTopologyKeys = new Set([
  "apiProcess",
  "applicationCache",
  "boundary",
  "collector",
  "connection",
  "databasePool",
  "osCache",
  "postgresCache",
  "webProcess",
]);
const allowedEvidenceKeys = new Set([
  "contractVersion",
  "measurement",
  "runtimeAssignments",
  "runtimeIdentities",
  "samples",
  "stateWitnesses",
]);
const allowedSampleKeys = new Set([
  "bodyComplete",
  "cacheStateId",
  "completedAt",
  "elapsedMs",
  "eligibility",
  "endpointId",
  "error",
  "requestPath",
  "responseBytes",
  "responseGeneratedAt",
  "run",
  "runtimeId",
  "sampleId",
  "startedAt",
  "status",
  "tier",
  "timingBoundary",
  "topology",
  "transport",
  "witnessId",
]);
const allowedStateWitnessKeys = new Set([
  "applicationCache",
  "endpointId",
  "id",
  "observation",
  "observationSha256",
  "observedAt",
  "producer",
  "pgdataColdProof",
  "runtimeId",
  "sampleId",
  "scope",
  "tier",
]);
const allowedStateWitnessFileKeys = new Set([
  "contractVersion",
  "producer",
  "witnesses",
]);
const allowedApplicationCacheWitnessKeys = new Set([
  "cfPathObserved",
  "cohortId",
  "cohortWitnessIds",
  "completedAt",
  "dispatchAt",
  "discoveryRefreshedAt",
  "expiredAt",
  "priorGeneratedAt",
  "responseGeneratedAt",
  "state",
]);
const allowedSupervisorConfigurationKeys = new Set([
  "containers",
  "contractVersion",
  "cycles",
  "database",
  "endpointIds",
  "expiryBufferMs",
  "measurement",
  "requestTimeoutMs",
  "runtimeAssignments",
  "runtimeId",
  "runtimeIdentities",
  "tier",
]);
const allowedSupervisorContainerKeys = new Set(["api", "database", "web"]);
const allowedSupervisorDatabaseKeys = new Set(["applicationName", "name", "user"]);
const allowedSupervisorObservationKeys = new Set([
  "after",
  "before",
  "databasePool",
  "osBootId",
  "postgresStartedAt",
  "schemaLedgerSha256",
  "schemaVersion",
  "serverVersionNum",
]);
const allowedSupervisorContainerObservationKeys = new Set([
  "configImage",
  "containerId",
  "dataVolume",
  "imageId",
  "pid",
  "project",
  "service",
  "sourceCommit",
  "startedAt",
]);
const allowedSupervisorProcessSetKeys = new Set(["api", "database", "web"]);
const allowedSupervisorPoolKeys = new Set(["backends", "generationId"]);
const allowedSupervisorBackendKeys = new Set(["pid", "startedAt"]);
const allowedPgdataColdProofKeys = new Set([
  "bindingSha256",
  "completedAt",
  "contractVersion",
  "databaseContainerId",
  "databaseStartedAt",
  "fileCount",
  "proofSha256",
  "residentPagesAfter",
  "totalPages",
  "volumeName",
]);

// identify plain records only
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

// accept finite nonnegative durations
function isDuration(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

// accept nonempty identity text
function isNonemptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

// recognize one valid timestamp
function isTimestamp(value) {
  return isNonemptyString(value) && Number.isFinite(Date.parse(value));
}

// order numeric measurements ascending
function compareNumbers(left, right) {
  return left - right;
}

// calculate the fixed nearest-rank percentile
export function nearestRankPercentile(values, percentile) {
  // reject invalid percentile requests
  if (!Array.isArray(values) || values.length === 0 || !Number.isFinite(percentile) || percentile <= 0 || percentile > 1) {
    throw new TypeError("nearest-rank percentile requires values and a percentile in (0, 1]");
  }

  const sorted = [...values];

  // reject invalid measurements
  for (const value of sorted) {
    // preserve millisecond units
    if (!isDuration(value)) {
      throw new TypeError("nearest-rank percentile values must be finite nonnegative milliseconds");
    }
  }

  sorted.sort(compareNumbers);
  return sorted[Math.ceil(percentile * sorted.length) - 1];
}

// list required tiers for one endpoint
export function requiredTiersForEndpoint(endpointId) {
  const endpoint = endpointById.get(endpointId);

  // reject unknown inventory entries
  if (endpoint === undefined) {
    throw new TypeError(`unknown endpoint ${String(endpointId)}`);
  }

  return endpoint.applicationCache
    ? ["warm", "K0", "K1", "K2", "K3"]
    : ["warm", "K0", "K2", "K3"];
}

// append one unique issue
function addIssue(issues, issue) {
  // avoid repeated schema noise
  if (!issues.includes(issue)) {
    issues.push(issue);
  }
}

// reject fields outside one closed record schema
function assertAllowedKeys(value, allowed, label) {
  // require a record before checking its keys
  if (!isRecord(value)) {
    throw new Error(`${label} must be a record`);
  }

  // reject every undeclared field
  for (const key of Object.keys(value)) {
    // prevent hidden collection actions or evidence payloads
    if (!allowed.has(key)) {
      throw new Error(`${label} field ${key} is unsupported`);
    }
  }
}

// validate frozen measurement identity
function validateMeasurement(measurement, issues) {
  // require one complete measurement record
  if (!isRecord(measurement)) {
    addIssue(issues, "measurement identity is missing");
    return;
  }

  // require an explicit baseline or candidate phase
  if (measurement.phase !== "baseline" && measurement.phase !== "candidate") {
    addIssue(issues, "measurement phase must be baseline or candidate");
  }

  // bind candidate query and ingestion identities
  for (const field of ["candidateId", "ingestionWitnessId", "queryIdentity"]) {
    // reject missing identity fields
    if (!isNonemptyString(measurement[field])) {
      addIssue(issues, `measurement ${field} is missing`);
    }
  }

  // bind the evidence clock
  if (!isTimestamp(measurement.collectedAt)) {
    addIssue(issues, "measurement collectedAt is missing or invalid");
  }

  // bind elapsed time to the monotonic collector clock
  if (measurement.clock !== "node-performance-now") {
    addIssue(issues, "measurement clock must be node-performance-now");
  }

  // require the accepted ingestion condition
  if (measurement.loadCondition !== "normal-ingestion") {
    addIssue(issues, "measurement loadCondition must be normal-ingestion");
  }

  // bind the representative page workload ceiling
  if (!Number.isInteger(measurement.maximumHttpConcurrency) || measurement.maximumHttpConcurrency < 1 || measurement.maximumHttpConcurrency > 8) {
    addIssue(issues, "measurement maximumHttpConcurrency must be an integer from 1 through 8");
  }

  // preserve the unresolved live process-cold gate explicitly
  if (
    measurement.blueberryProcessColdStatus !== "pending" &&
    measurement.blueberryProcessColdStatus !== "observed-supported-deployment"
  ) {
    addIssue(issues, "measurement blueberryProcessColdStatus is missing or invalid");
  }
}

// validate runtime identities and tier assignments
function validateRuntimes(runtimeIdentities, runtimeAssignments, issues) {
  // require both runtime maps
  if (!isRecord(runtimeIdentities) || !isRecord(runtimeAssignments)) {
    addIssue(issues, "runtime identities or assignments are missing");
    return;
  }

  // validate every required tier assignment
  for (const tier of REQUIRED_TIERS) {
    const runtimeId = runtimeAssignments[tier];
    const identity = isNonemptyString(runtimeId) ? runtimeIdentities[runtimeId] : undefined;

    // require a referenced runtime
    if (!isRecord(identity)) {
      addIssue(issues, `runtime assignment ${tier} is missing`);
      continue;
    }

    // bind every material runtime identity
    for (const field of identityFields) {
      // reject missing runtime identity fields
      if (!isNonemptyString(identity[field])) {
        addIssue(issues, `runtime ${runtimeId} ${field} is missing`);
      }
    }

    // distinguish Blueberry and disposable evidence
    if (assignmentEnvironment[tier] !== null && identity.environment !== assignmentEnvironment[tier]) {
      addIssue(issues, `runtime ${runtimeId} for ${tier} must use ${assignmentEnvironment[tier]} environment`);
    }
    // allow K2 only in its reviewed disposable or supported live scope
    if (tier === "K2" && identity.environment !== "blueberry" && identity.environment !== "disposable") {
      addIssue(issues, `runtime ${runtimeId} for K2 must use blueberry or disposable environment`);
    }
  }
}

// validate topology labels for one cache tier
function validateTopology(sample, tier) {
  const issues = [];
  const topology = sample.topology;

  // require the frozen same-host boundary
  if (!isRecord(topology)) {
    return ["topology labels are missing"];
  }

  // require explicit transport state
  if (topology.collector !== "web-container" || topology.boundary !== "loopback-web-complete-body") {
    issues.push("collector boundary is not the frozen web-container loopback boundary");
  }

  // require explicit process and cache labels
  for (const field of [
    "connection",
    "applicationCache",
    "webProcess",
    "apiProcess",
    "databasePool",
    "postgresCache",
    "osCache",
  ]) {
    // reject omitted state labels
    if (!isNonemptyString(topology[field])) {
      issues.push(`topology ${field} is missing`);
    }
  }

  const allowedStates = {
    apiProcess: new Set(["existing", "fresh"]),
    applicationCache: new Set(["warm", "empty", "naturally-expired", "not-applicable", "uncontrolled"]),
    connection: new Set(["fresh", "reused"]),
    databasePool: new Set(["existing", "fresh"]),
    osCache: new Set(["uncontrolled", "fresh-disposable"]),
    postgresCache: new Set(["uncontrolled", "fresh-disposable"]),
    webProcess: new Set(["existing", "fresh"]),
  };

  // reject invented cache and process state names
  for (const [field, states] of Object.entries(allowedStates)) {
    // require one documented state
    if (!states.has(topology[field])) {
      issues.push(`topology ${field} label is invalid`);
    }
  }

  // accept measured warm traffic without claiming one connection mode
  if (tier === "warm") {
    // require warm or inapplicable application state
    if (!new Set(["warm", "not-applicable"]).has(topology.applicationCache)) {
      issues.push("warm application cache label is invalid");
    }
    // preserve existing process and uncontrolled database state
    if (
      topology.webProcess !== "existing" ||
      topology.apiProcess !== "existing" ||
      topology.databasePool !== "existing" ||
      topology.postgresCache !== "uncontrolled" ||
      topology.osCache !== "uncontrolled"
    ) {
      issues.push("warm evidence requires existing processes and uncontrolled database caches");
    }
    return issues;
  }

  // require a new client connection for first-load tiers
  if (tier !== "K1" && topology.connection !== "fresh") {
    issues.push(`${tier} requires a fresh client connection`);
  }

  // preserve unknown server cache state for K0
  if (tier === "K0") {
    // keep application cache state independent from fresh client connection
    if (!new Set(["empty", "not-applicable", "warm", "uncontrolled"]).has(topology.applicationCache)) {
      issues.push("K0 application cache label is invalid");
    }
    // reject false process or database cold claims
    if (
      topology.webProcess !== "existing" ||
      topology.apiProcess !== "existing" ||
      topology.databasePool !== "existing" ||
      topology.postgresCache !== "uncontrolled" ||
      topology.osCache !== "uncontrolled"
    ) {
      issues.push("K0 must not claim fresh processes, pools, PostgreSQL, or OS caches");
    }
  }

  // require natural application cache expiry for K1
  if (tier === "K1") {
    // reject hidden manual cache clearing
    if (topology.applicationCache !== "naturally-expired") {
      issues.push("K1 requires a naturally-expired application cache");
    }
    // reject process or database cold substitution
    if (
      topology.webProcess !== "existing" ||
      topology.apiProcess !== "existing" ||
      topology.databasePool !== "existing" ||
      topology.postgresCache !== "uncontrolled" ||
      topology.osCache !== "uncontrolled"
    ) {
      issues.push("K1 must isolate application-cache expiry from process and database cold state");
    }
  }

  // require fresh services and pool for K2
  if (tier === "K2") {
    // require an empty or inapplicable new-process cache
    if (!new Set(["empty", "not-applicable"]).has(topology.applicationCache)) {
      issues.push("K2 application cache must be empty or not-applicable");
    }
    // preserve the exact process boundary
    if (
      topology.webProcess !== "fresh" ||
      topology.apiProcess !== "fresh" ||
      topology.databasePool !== "fresh" ||
      topology.postgresCache !== "uncontrolled" ||
      topology.osCache !== "uncontrolled"
    ) {
      issues.push("K2 requires fresh web/API processes and pool without a database-cache claim");
    }
  }

  // require disposable database and OS cold evidence for K3
  if (tier === "K3") {
    // require an empty or inapplicable disposable cache
    if (!new Set(["empty", "not-applicable"]).has(topology.applicationCache)) {
      issues.push("K3 application cache must be empty or not-applicable");
    }
    // bind every cold layer explicitly
    if (
      topology.webProcess !== "fresh" ||
      topology.apiProcess !== "fresh" ||
      topology.databasePool !== "fresh" ||
      topology.postgresCache !== "fresh-disposable" ||
      topology.osCache !== "fresh-disposable"
    ) {
      issues.push("K3 requires fresh disposable process, pool, PostgreSQL, and OS state");
    }
  }

  return issues;
}

// hash one stable observation payload
function sha256Json(value) {
  return createHash("sha256").update(JSON.stringify(value)).digest("hex");
}

// hash one observed database pool generation
function databasePoolGenerationId(backends) {
  return sha256Json(backends.map(
    // retain only stable backend identity fields
    function selectBackendIdentity(backend) {
      return { pid: backend.pid, startedAt: backend.startedAt };
    },
  ));
}

// bind one cold-proof digest to its exact observed HTTP attempt
export function pgdataColdProofBindingSha256(coldProof, sample, observationSha256) {
  return sha256Json({
    completedAt: coldProof.completedAt,
    contractVersion: coldProof.contractVersion,
    databaseContainerId: coldProof.databaseContainerId,
    databaseStartedAt: coldProof.databaseStartedAt,
    endpointId: sample.endpointId,
    fileCount: coldProof.fileCount,
    observationSha256,
    proofSha256: coldProof.proofSha256,
    residentPagesAfter: coldProof.residentPagesAfter,
    runtimeId: sample.runtimeId,
    sampleId: sample.sampleId,
    startedAt: sample.startedAt,
    tier: sample.tier,
    totalPages: coldProof.totalPages,
    volumeName: coldProof.volumeName,
  });
}

// validate one observed container process
function validateContainerObservation(value, label, issues) {
  // require the closed observation schema
  try {
    assertAllowedKeys(value, allowedSupervisorContainerObservationKeys, label);
  } catch (error) {
    issues.push(error instanceof Error ? error.message : String(error));
    return;
  }

  // require actual docker process identity
  if (
    !isNonemptyString(value.configImage) ||
    !isNonemptyString(value.containerId) ||
    !/^sha256:[a-f0-9]{64}$/u.test(value.imageId ?? "") ||
    !Number.isInteger(value.pid) ||
    value.pid < 1 ||
    !isNonemptyString(value.project) ||
    !isNonemptyString(value.service) ||
    !isTimestamp(value.startedAt)
  ) {
    issues.push(`${label} is incomplete`);
  }
}

// validate one bundled docker observation
function validateSupervisorObservation(witness, runtimeIdentity, issues) {
  const observation = witness.observation;

  // require the closed observation schema
  try {
    assertAllowedKeys(observation, allowedSupervisorObservationKeys, "supervisor observation");
    assertAllowedKeys(observation.before, allowedSupervisorProcessSetKeys, "supervisor before processes");
    assertAllowedKeys(observation.after, allowedSupervisorProcessSetKeys, "supervisor after processes");
    assertAllowedKeys(observation.databasePool, allowedSupervisorPoolKeys, "supervisor database pool");
  } catch (error) {
    issues.push(error instanceof Error ? error.message : String(error));
    return null;
  }

  // validate both sides of the measured request
  for (const phase of ["before", "after"]) {
    // validate every required service process
    for (const service of ["web", "api", "database"]) {
      validateContainerObservation(observation[phase][service], `supervisor ${phase} ${service}`, issues);
    }
  }

  // reject a restart during the measured request
  for (const service of ["web", "api", "database"]) {
    // require stable exact process identity
    if (!isDeepStrictEqual(observation.before[service], observation.after[service])) {
      issues.push(`supervisor observed ${service} restart during the sample`);
    }
  }

  const expectedServices = { api: "api", database: "postgres", web: "web" };
  const projects = new Set();

  // bind actual compose services and project to the frozen runtime
  for (const service of ["web", "api", "database"]) {
    const observed = observation.before[service];
    projects.add(observed.project);

    // reject substituted containers with fresh timestamps
    if (observed.service !== expectedServices[service]) {
      issues.push(`supervisor ${service} compose service is invalid`);
    }
  }
  // require one exact compose project boundary
  if (projects.size !== 1) {
    issues.push("supervisor containers do not share one compose project");
  }
  // bind actual configured image references and source revision
  if (
    observation.before.web.configImage !== runtimeIdentity?.webImage ||
    observation.before.api.configImage !== runtimeIdentity?.apiImage ||
    observation.before.database.configImage !== runtimeIdentity?.databaseImage ||
    observation.before.web.sourceCommit !== runtimeIdentity?.sourceCommit
  ) {
    issues.push("supervisor container images or source revision do not match the runtime identity");
  }

  const backends = observation.databasePool.backends;

  // require an observed application pool generation
  if (!Array.isArray(backends) || backends.length === 0) {
    issues.push("supervisor database pool backends are missing");
  } else {
    // validate every observed application backend
    for (const backend of backends) {
      try {
        assertAllowedKeys(backend, allowedSupervisorBackendKeys, "supervisor database backend");
      } catch (error) {
        issues.push(error instanceof Error ? error.message : String(error));
        continue;
      }

      // require exact backend identity
      if (!Number.isInteger(backend.pid) || backend.pid < 1 || !isTimestamp(backend.startedAt)) {
        issues.push("supervisor database backend is incomplete");
      }
    }

    // bind the pool label to the observed backend set
    if (observation.databasePool.generationId !== databasePoolGenerationId(backends)) {
      issues.push("supervisor database pool generation does not match its backends");
    }
  }

  // bind the witness to its complete observed payload
  if (!/^[a-f0-9]{64}$/u.test(witness.observationSha256 ?? "") || witness.observationSha256 !== sha256Json(observation)) {
    issues.push("supervisor observation digest does not match its payload");
  }
  // require concrete database process observations
  if (!isTimestamp(observation.postgresStartedAt) || !isNonemptyString(observation.osBootId)) {
    issues.push("supervisor PostgreSQL or OS identity is missing");
  }
  // bind actual migration ledger and server identity to the runtime
  if (
    observation.schemaVersion !== runtimeIdentity?.schemaVersion ||
    !/^[a-f0-9]{64}$/u.test(observation.schemaLedgerSha256 ?? "") ||
    !/^17\d{4}$/u.test(observation.serverVersionNum ?? "")
  ) {
    issues.push("supervisor database schema ledger or server version does not match the runtime");
  }

  return observation;
}

// validate one independently supervised cache-state witness
function validateStateWitness(sample, tier, runtimeId, runtimeIdentity, stateWitnesses) {
  const issues = [];

  // require independently produced witnesses for K1 through K3
  if (!new Set(["K1", "K2", "K3"]).has(tier)) {
    return issues;
  }

  const witness = isNonemptyString(sample.witnessId) ? stateWitnesses?.[sample.witnessId] : undefined;

  // reject caller labels without a bound supervisor witness
  if (!isRecord(witness)) {
    return ["supervisor state witness is missing"];
  }

  // bind the witness to the exact sample stratum
  if (
    witness.producer !== SUPERVISOR_PRODUCER ||
    witness.id !== sample.witnessId ||
    witness.endpointId !== sample.endpointId ||
    witness.tier !== tier ||
    witness.runtimeId !== runtimeId ||
    witness.id !== sample.cacheStateId ||
    witness.sampleId !== sample.sampleId
  ) {
    issues.push("supervisor state witness does not match the sample stratum");
  }
  // bind the witness to the selected runtime environment
  if (witness.scope !== runtimeIdentity?.environment) {
    issues.push("supervisor state witness scope does not match the runtime");
  }

  // require a post-request observation clock
  if (!isTimestamp(witness.observedAt) || Date.parse(witness.observedAt) < Date.parse(sample.startedAt)) {
    issues.push("supervisor witness observedAt is missing or precedes dispatch");
  }

  const observation = validateSupervisorObservation(witness, runtimeIdentity, issues);

  // bind natural application-cache expiry
  if (tier === "K1") {
    const applicationCache = witness.applicationCache;

    // reject labels without observed expiry evidence
    if (!isRecord(applicationCache) || applicationCache.state !== "naturally-expired" || !isTimestamp(applicationCache.expiredAt)) {
      issues.push("K1 natural-expiry witness is missing");
    } else if (sample.endpointId === "viewer-context") {
      // require the real CF discovery path rather than loopback false
      if (applicationCache.cfPathObserved !== true || !isTimestamp(applicationCache.discoveryRefreshedAt)) {
        issues.push("viewer K1 requires an observed CF discovery refresh");
      }
    } else if (
      !isTimestamp(applicationCache.priorGeneratedAt) ||
      !isTimestamp(applicationCache.responseGeneratedAt) ||
      applicationCache.dispatchAt !== sample.startedAt ||
      applicationCache.completedAt !== sample.completedAt ||
      sample.responseGeneratedAt !== applicationCache.responseGeneratedAt ||
      Date.parse(applicationCache.responseGeneratedAt) <= Date.parse(applicationCache.priorGeneratedAt) ||
      Date.parse(applicationCache.expiredAt) !== Date.parse(applicationCache.priorGeneratedAt) + OBSERVATION_CACHE_TTL_MS ||
      Date.parse(sample.startedAt) < Date.parse(applicationCache.expiredAt)
    ) {
      issues.push("K1 generatedAt refill evidence is missing or mismatched");
    } else {
      const cohortIds = applicationCache.cohortWitnessIds;

      // require both shared-cache consumers in one overlapping refill cohort
      if (!isNonemptyString(applicationCache.cohortId) || !Array.isArray(cohortIds) || cohortIds.length !== 2 || !cohortIds.includes(witness.id)) {
        issues.push("K1 shared-cache cohort is missing");
      } else {
        const cohort = cohortIds.map(
          // resolve each bound supervisor observation
          function resolveCohortWitness(id) {
            return stateWitnesses?.[id];
          },
        );
        const endpointIds = new Set(cohort.map(
          // select the paired route identity
          function selectCohortEndpoint(item) {
            return item?.endpointId;
          },
        ));
        const dispatches = cohort.map(
          // select each observed dispatch clock
          function selectCohortDispatch(item) {
            return Date.parse(item?.applicationCache?.dispatchAt);
          },
        );
        const completions = cohort.map(
          // select each observed completion clock
          function selectCohortCompletion(item) {
            return Date.parse(item?.applicationCache?.completedAt);
          },
        );
        const shared = cohort.every(
          // require the same exact cache transition
          function matchesSharedTransition(item) {
            return item?.producer === SUPERVISOR_PRODUCER &&
              item?.applicationCache?.cohortId === applicationCache.cohortId &&
              isDeepStrictEqual(item?.applicationCache?.cohortWitnessIds, cohortIds) &&
              item?.applicationCache?.priorGeneratedAt === applicationCache.priorGeneratedAt &&
              item?.applicationCache?.responseGeneratedAt === applicationCache.responseGeneratedAt;
          },
        );

        // prove both requests waited on the same in-flight refill
        if (
          !endpointIds.has("forecast-observations") ||
          !endpointIds.has("solar-cloud-evidence") ||
          dispatches.some(Number.isNaN) ||
          completions.some(Number.isNaN) ||
          Math.max(...dispatches) > Math.min(...completions) ||
          !shared
        ) {
          issues.push("K1 shared-cache requests do not overlap one observed refill");
        }
      }
    }
  }

  // bind fresh-process evidence to the declared K2 scope
  if (tier === "K2" && witness.scope !== "blueberry" && witness.scope !== "disposable") {
    issues.push("K2 witness scope must be blueberry or disposable");
  }

  // require K2 and K3 processes and pools to be freshly observed
  if (tier === "K2" || tier === "K3") {
    const sampleStarted = Date.parse(sample.startedAt);
    const observedAt = Date.parse(witness.observedAt);

    // constrain each fresh service start to five minutes before dispatch
    for (const service of ["web", "api"]) {
      const started = Date.parse(observation?.before?.[service]?.startedAt);

      // reject stale or future service starts
      if (!Number.isFinite(started) || started > sampleStarted || sampleStarted - started > 5 * 60_000) {
        issues.push(`${tier} witness ${service} process is not fresh`);
      }
    }

    // constrain every observed pool backend to the fresh interval
    for (const backend of observation?.databasePool?.backends ?? []) {
      const started = Date.parse(backend.startedAt);

      // allow request-triggered connections before the post-request observation
      if (!Number.isFinite(started) || started > observedAt || observedAt - started > 5 * 60_000) {
        issues.push(`${tier} witness database pool is not fresh`);
      }
    }

    const endpoint = endpointById.get(sample.endpointId);

    // prove process-local cache fill after the fresh web process began
    if (
      endpoint?.applicationCache === true &&
      sample.endpointId !== "viewer-context" &&
      (!isTimestamp(sample.responseGeneratedAt) || Date.parse(sample.responseGeneratedAt) < Date.parse(observation?.before?.web?.startedAt))
    ) {
      issues.push(`${tier} application cache fill does not follow the fresh web process`);
    }
  }

  // require a disposable PostgreSQL and OS identity for K3
  if (tier === "K3") {
    const coldProof = witness.pgdataColdProof;

    // reject generic cold labels without cluster evidence
    if (
      witness.scope !== "disposable" ||
      !isNonemptyString(observation?.before?.database?.containerId) ||
      !isTimestamp(observation?.postgresStartedAt) ||
      !isNonemptyString(observation?.osBootId)
    ) {
      issues.push("K3 disposable cluster witness is missing");
    }
    // require an independently completed zero-residency PGDATA proof
    if (
      !isRecord(coldProof) ||
      coldProof.contractVersion !== "weather-page-pgdata-cold-proof/v1" ||
      !/^[a-f0-9]{64}$/u.test(coldProof.proofSha256 ?? "") ||
      !/^[a-f0-9]{64}$/u.test(coldProof.bindingSha256 ?? "") ||
      coldProof.bindingSha256 !== pgdataColdProofBindingSha256(coldProof, sample, witness.observationSha256) ||
      !isTimestamp(coldProof.completedAt) ||
      !Number.isInteger(coldProof.fileCount) ||
      coldProof.fileCount < 1 ||
      !Number.isInteger(coldProof.totalPages) ||
      coldProof.totalPages < 1 ||
      coldProof.residentPagesAfter !== 0 ||
      coldProof.volumeName !== observation?.before?.database?.dataVolume ||
      coldProof.databaseContainerId !== observation?.before?.database?.containerId ||
      coldProof.databaseStartedAt !== observation?.before?.database?.startedAt ||
      Date.parse(coldProof.completedAt) >= Date.parse(observation?.before?.database?.startedAt)
    ) {
      issues.push("K3 independently bound zero-residency PGDATA proof is missing or invalid");
    }
  }

  return issues;
}

// validate one complete HTTP attempt
function validateSample(sample, endpoint, tier, runtimeId, runtimeIdentity, stateWitnesses) {
  const issues = [];

  // require exact endpoint and runtime bindings
  if (sample.requestPath !== endpoint.path) {
    issues.push("request path does not match the frozen endpoint");
  }
  // prevent conditional fixture evidence from masquerading as live eligibility
  if (sample.eligibility !== endpoint.eligibility) {
    issues.push(`eligibility must be ${endpoint.eligibility}`);
  }
  // bind the sample to the assigned runtime
  if (sample.runtimeId !== runtimeId) {
    issues.push(`runtime must be ${String(runtimeId)}`);
  }
  // bind one explicit cache-state observation
  if (!isNonemptyString(sample.cacheStateId)) {
    issues.push("cacheStateId is missing");
  }
  // keep cold attempts out of warm run groupings
  if (tier !== "warm" && sample.run !== null) {
    issues.push("cold sample run must be null");
  }
  // bind the sample clock
  if (!isTimestamp(sample.startedAt)) {
    issues.push("startedAt is missing or invalid");
  }
  // bind completion after dispatch
  if (sample.completedAt !== undefined && (!isTimestamp(sample.completedAt) || Date.parse(sample.completedAt) < Date.parse(sample.startedAt))) {
    issues.push("completedAt is missing invalid or precedes dispatch");
  }
  // require full HTTP timing rather than SQL-only evidence
  if (sample.transport !== "http" || sample.timingBoundary !== "dispatch-to-complete-body") {
    issues.push("sample is not a complete HTTP-body timing");
  }
  // preserve incomplete and failed bodies in the denominator
  if (sample.bodyComplete !== true) {
    issues.push("response body was not completed");
  }
  // require a successful endpoint response
  if (!Number.isInteger(sample.status) || sample.status < 200 || sample.status >= 300) {
    issues.push(`HTTP status ${String(sample.status)} is not successful`);
  }
  // reject transport failures explicitly
  if (sample.error !== null) {
    issues.push("sample contains a transport error");
  }
  // require one measurable response body
  if (!Number.isInteger(sample.responseBytes) || sample.responseBytes < 0) {
    issues.push("responseBytes is missing or invalid");
  }
  // preserve millisecond units
  if (!isDuration(sample.elapsedMs)) {
    issues.push("elapsedMs is missing or invalid");
  }

  issues.push(...validateTopology(sample, tier));
  issues.push(...validateStateWitness(sample, tier, runtimeId, runtimeIdentity, stateWitnesses));
  return issues;
}

// format one stratum key
function stratumKey(endpointId, tier, run, runtimeId) {
  return `${endpointId}|${tier}|${run === null ? "cold" : String(run)}|${String(runtimeId)}`;
}

// construct one required stratum result
function evaluateStratum(endpoint, tier, run, runtimeId, samples, sampleIssues) {
  const requiredSamples = tier === "warm" ? WARM_SAMPLES_PER_RUN : COLD_SAMPLES_PER_STRATUM;
  const exactCount = tier === "warm";
  const issues = [];

  // enforce fixed warm counts and minimum cold counts
  if ((exactCount && samples.length !== requiredSamples) || (!exactCount && samples.length < requiredSamples)) {
    issues.push(`${tier} requires ${exactCount ? "exactly" : "at least"} ${requiredSamples} samples; observed ${samples.length}`);
  }

  const durations = [];
  const coldStateIds = new Set();

  // retain every observed attempt in the result
  for (const sample of samples) {
    const invalid = sampleIssues.get(sample.sampleId) ?? [];

    // fail the stratum on any invalid attempt
    for (const issue of invalid) {
      issues.push(`${sample.sampleId}: ${issue}`);
    }

    // collect only measurable durations for the statistic
    if (isDuration(sample.elapsedMs)) {
      durations.push(sample.elapsedMs);
    }

    // require an independently established state for every cold attempt
    if (tier !== "warm" && isNonemptyString(sample.cacheStateId)) {
      // reject repeated first-load state labels within one endpoint tier
      if (coldStateIds.has(sample.cacheStateId)) {
        issues.push(`${sample.sampleId}: cold cacheStateId ${sample.cacheStateId} is duplicated`);
      }
      coldStateIds.add(sample.cacheStateId);
    }
  }

  let p95Ms = null;
  let maximumMs = null;

  // calculate warm p95 only from a complete measured denominator
  if (tier === "warm" && durations.length === samples.length && durations.length > 0) {
    p95Ms = nearestRankPercentile(durations, 0.95);
    // enforce the per-run budget
    if (p95Ms > WARM_P95_LIMIT_MS) {
      issues.push(`warm p95 ${String(p95Ms)}ms exceeds ${String(WARM_P95_LIMIT_MS)}ms`);
    }
  }

  // enforce every cold sample independently
  if (tier !== "warm" && durations.length > 0) {
    maximumMs = Math.max(...durations);
    // reject any slow cold response
    if (maximumMs > COLD_LIMIT_MS) {
      issues.push(`cold response ${String(maximumMs)}ms exceeds ${String(COLD_LIMIT_MS)}ms`);
    }
  }

  return {
    endpointId: endpoint.id,
    eligibility: endpoint.eligibility,
    horizon: endpoint.horizon,
    issues,
    maximumMs,
    observedSamples: samples.length,
    pass: issues.length === 0,
    p95Ms,
    requestPath: endpoint.path,
    requiredSamples,
    run,
    runtimeId,
    tier,
  };
}

// evaluate the complete fixed endpoint matrix
export function evaluateEvidence(evidence) {
  const issues = [];
  const samples = Array.isArray(evidence?.samples) ? evidence.samples : [];

  // require the exact evidence contract
  if (!isRecord(evidence) || evidence.contractVersion !== EVIDENCE_CONTRACT_VERSION) {
    addIssue(issues, `evidence contract must be ${EVIDENCE_CONTRACT_VERSION}`);
  }

  validateMeasurement(evidence?.measurement, issues);
  validateRuntimes(evidence?.runtimeIdentities, evidence?.runtimeAssignments, issues);

  const k2RuntimeId = evidence?.runtimeAssignments?.K2;
  const k2Environment = evidence?.runtimeIdentities?.[k2RuntimeId]?.environment;

  // keep an unobserved Blueberry process-cold gate pending
  if (k2Environment === "disposable" && evidence?.measurement?.blueberryProcessColdStatus !== "observed-supported-deployment") {
    addIssue(issues, "Blueberry process-cold evidence remains pending");
  }
  // reject a live-complete claim without live K2 samples
  if (evidence?.measurement?.blueberryProcessColdStatus === "observed-supported-deployment" && k2Environment !== "blueberry") {
    addIssue(issues, "Blueberry process-cold completion requires Blueberry K2 samples");
  }

  const grouped = new Map();
  const sampleIssues = new Map();
  const sampleIds = new Set();

  // validate and group every attempt without dropping failures
  for (const sample of samples) {
    // reject malformed attempt records
    if (!isRecord(sample) || !isNonemptyString(sample.sampleId)) {
      addIssue(issues, "sample without a sampleId is present");
      continue;
    }
    // reject duplicate evidence reuse
    if (sampleIds.has(sample.sampleId)) {
      addIssue(issues, `duplicate sampleId ${sample.sampleId}`);
      continue;
    }
    sampleIds.add(sample.sampleId);

    const endpoint = endpointById.get(sample.endpointId);
    const tiers = endpoint === undefined ? [] : requiredTiersForEndpoint(endpoint.id);

    // reject out-of-inventory samples
    if (endpoint === undefined || !tiers.includes(sample.tier)) {
      addIssue(issues, `sample ${sample.sampleId} has an unexpected endpoint or tier`);
      continue;
    }

    const run = sample.tier === "warm" ? sample.run : null;

    // reject pooled or extra warm runs
    if (sample.tier === "warm" && (!Number.isInteger(run) || run < 1 || run > WARM_RUNS)) {
      addIssue(issues, `sample ${sample.sampleId} has an invalid warm run`);
      continue;
    }

    const runtimeId = evidence?.runtimeAssignments?.[sample.tier];
    const key = stratumKey(endpoint.id, sample.tier, run, runtimeId);
    const group = grouped.get(key) ?? [];
    group.push(sample);
    grouped.set(key, group);
    sampleIssues.set(sample.sampleId, validateSample(
      sample,
      endpoint,
      sample.tier,
      runtimeId,
      evidence?.runtimeIdentities?.[runtimeId],
      evidence?.stateWitnesses,
    ));
  }

  const strata = [];

  // evaluate every fixed inventory entry
  for (const endpoint of ENDPOINT_INVENTORY) {
    const tiers = requiredTiersForEndpoint(endpoint.id);

    // evaluate every required tier separately
    for (const tier of tiers) {
      const runtimeId = evidence?.runtimeAssignments?.[tier];

      // keep all three warm runs separate
      if (tier === "warm") {
        // evaluate one fixed warm run
        for (let run = 1; run <= WARM_RUNS; run += 1) {
          const samplesForStratum = grouped.get(stratumKey(endpoint.id, tier, run, runtimeId)) ?? [];
          strata.push(evaluateStratum(endpoint, tier, run, runtimeId, samplesForStratum, sampleIssues));
        }
        continue;
      }

      const samplesForStratum = grouped.get(stratumKey(endpoint.id, tier, null, runtimeId)) ?? [];
      strata.push(evaluateStratum(endpoint, tier, null, runtimeId, samplesForStratum, sampleIssues));
    }
  }

  // surface every failed stratum at the top level
  for (const stratum of strata) {
    // retain endpoint-specific failure identity
    if (!stratum.pass) {
      addIssue(issues, `${stratum.endpointId} ${stratum.tier}${stratum.run === null ? "" : ` run ${String(stratum.run)}`} failed`);
    }
  }

  return {
    contractVersion: RESULT_CONTRACT_VERSION,
    inventory: ENDPOINT_INVENTORY,
    issues,
    limits: {
      coldMaximumMs: COLD_LIMIT_MS,
      coldSamplesPerStratum: COLD_SAMPLES_PER_STRATUM,
      percentileMethod: "nearest-rank",
      warmP95Ms: WARM_P95_LIMIT_MS,
      warmRuns: WARM_RUNS,
      warmSamplesPerRun: WARM_SAMPLES_PER_RUN,
    },
    pass: issues.length === 0,
    strata,
  };
}

// inspect configuration recursively for secret-bearing fields
function rejectSensitiveConfiguration(value, path = "config") {
  // inspect arrays without accepting executable values
  if (Array.isArray(value)) {
    // inspect each array entry
    for (let index = 0; index < value.length; index += 1) {
      rejectSensitiveConfiguration(value[index], `${path}[${String(index)}]`);
    }
    return;
  }

  // ignore scalar configuration values
  if (!isRecord(value)) {
    return;
  }

  // inspect every record field
  for (const [key, nested] of Object.entries(value)) {
    // reject inline secrets and provider payloads
    if (forbiddenConfigurationKeys.test(key)) {
      throw new Error(`${path}.${key} is forbidden in evaluator configuration`);
    }
    rejectSensitiveConfiguration(nested, `${path}.${key}`);
  }
}

// validate one private owned filesystem object
export async function assertPrivateOwnedPath(path, kind = "file") {
  const metadata = await lstat(path);

  // reject links before following them
  if (metadata.isSymbolicLink()) {
    throw new Error(`${path} must not be a symbolic link`);
  }
  // require the requested filesystem kind
  if ((kind === "file" && !metadata.isFile()) || (kind === "directory" && !metadata.isDirectory())) {
    throw new Error(`${path} must be a ${kind}`);
  }
  // bind evidence to the invoking account
  if (typeof process.getuid === "function" && metadata.uid !== process.getuid()) {
    throw new Error(`${path} must be owned by the invoking user`);
  }
  // reject group or public access
  if ((metadata.mode & 0o077) !== 0) {
    throw new Error(`${path} must not grant group or other permissions`);
  }

  return metadata;
}

// parse one private JSON file
async function readPrivateJson(path) {
  const metadata = await assertPrivateOwnedPath(path, "file");

  // bound untrusted evidence size
  if (metadata.size > 16 * 1024 * 1024) {
    throw new Error(`${path} exceeds the 16 MiB evidence limit`);
  }

  return JSON.parse(await readFile(path, "utf8"));
}

// validate the loopback-only origin
export function validateLoopbackOrigin(origin) {
  let parsed;

  // parse the configured origin safely
  try {
    parsed = new URL(origin);
  } catch {
    throw new Error("collection origin must be a valid loopback HTTP origin");
  }

  const allowedHosts = new Set(["127.0.0.1", "[::1]", "localhost"]);

  // prohibit credentials, paths, and remote hosts
  if (
    parsed.protocol !== "http:" ||
    !allowedHosts.has(parsed.hostname) ||
    parsed.username !== "" ||
    parsed.password !== "" ||
    parsed.pathname !== "/" ||
    parsed.search !== "" ||
    parsed.hash !== ""
  ) {
    throw new Error("collection origin must be a credential-free loopback HTTP origin");
  }

  return parsed.origin;
}

// validate the collection configuration
export function validateConfiguration(configuration) {
  // require the exact configuration contract
  if (!isRecord(configuration) || configuration.contractVersion !== CONFIG_CONTRACT_VERSION) {
    throw new Error(`config contract must be ${CONFIG_CONTRACT_VERSION}`);
  }

  // reject unknown top-level capabilities
  for (const key of Object.keys(configuration)) {
    // prevent future unsafe behavior from being silently ignored
    if (!allowedConfigurationKeys.has(key)) {
      throw new Error(`unsupported configuration field ${key}`);
    }
  }

  rejectSensitiveConfiguration(configuration);
  const identityIssues = [];
  validateMeasurement(configuration.measurement, identityIssues);
  validateRuntimes(configuration.runtimeIdentities, configuration.runtimeAssignments, identityIssues);

  // reject identity failures before collection
  if (identityIssues.length > 0) {
    throw new Error(identityIssues.join("; "));
  }

  // bound evaluator parallelism
  if (!Number.isInteger(configuration.concurrency) || configuration.concurrency < 1 || configuration.concurrency > 8) {
    throw new Error("concurrency must be an integer from 1 through 8");
  }
  // bound each complete-body request
  if (!Number.isInteger(configuration.requestTimeoutMs) || configuration.requestTimeoutMs < 100 || configuration.requestTimeoutMs > 30_000) {
    throw new Error("requestTimeoutMs must be an integer from 100 through 30000");
  }
  // require explicit evidence collection and witness arrays
  if (
    !Array.isArray(configuration.evidenceFiles) ||
    !Array.isArray(configuration.collections) ||
    !Array.isArray(configuration.stateWitnessFiles)
  ) {
    throw new Error("evidenceFiles, collections, and stateWitnessFiles must be arrays");
  }
  // bound external evidence fan-in
  if (configuration.evidenceFiles.length > 100) {
    throw new Error("evidenceFiles exceeds the 100-file limit");
  }
  // bound supervisor witness fan-in
  if (configuration.stateWitnessFiles.length > 100) {
    throw new Error("stateWitnessFiles exceeds the 100-file limit");
  }

  let totalSamples = 0;

  // validate each bounded collection request
  for (const collection of configuration.collections) {
    assertAllowedKeys(collection, allowedCollectionKeys, "collection");
    assertAllowedKeys(collection.topology, allowedTopologyKeys, "collection topology");
    const endpoint = endpointById.get(collection?.endpointId);

    // bind the request to the fixed inventory
    if (endpoint === undefined || !requiredTiersForEndpoint(endpoint.id).includes(collection.tier)) {
      throw new Error("collection endpoint or tier is outside the fixed inventory");
    }
    // prevent pooled warm samples
    if (collection.tier === "warm" && (!Number.isInteger(collection.run) || collection.run < 1 || collection.run > WARM_RUNS)) {
      throw new Error("warm collection run must be 1, 2, or 3");
    }
    // omit run labels from cold evidence
    if (collection.tier !== "warm" && collection.run !== null) {
      throw new Error("cold collection run must be null");
    }
    // bind exact eligibility and runtime identity
    if (collection.eligibility !== endpoint.eligibility || collection.runtimeId !== configuration.runtimeAssignments[collection.tier]) {
      throw new Error("collection eligibility or runtime does not match the fixed stratum");
    }
    // require one explicit cache observation identity
    if (!isNonemptyString(collection.cacheStateId)) {
      throw new Error("collection cacheStateId is required");
    }
    // require an external witness only for K1 through K3
    if (
      new Set(["K1", "K2", "K3"]).has(collection.tier) &&
      (!isNonemptyString(collection.witnessId) || collection.witnessId !== collection.cacheStateId)
    ) {
      throw new Error("K1 through K3 collections require a matching supervisor witnessId");
    }
    // bound each collection count
    if (!Number.isInteger(collection.count) || collection.count < 1 || collection.count > 200) {
      throw new Error("collection count must be an integer from 1 through 200");
    }

    validateLoopbackOrigin(collection.origin);
    const topologyIssues = validateTopology({ topology: collection.topology }, collection.tier);

    // reject dishonest cache and process labels before dispatch
    if (topologyIssues.length > 0) {
      throw new Error(topologyIssues.join("; "));
    }
    totalSamples += collection.count;
  }

  // bound one evaluator invocation
  if (totalSamples > 5_000) {
    throw new Error("collections exceed the 5000-sample limit");
  }

  return configuration;
}

// validate one safe docker or database identifier
function validateSupervisorName(value, label, pattern = /^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}$/u) {
  // reject shell syntax and empty identities
  if (!isNonemptyString(value) || !pattern.test(value)) {
    throw new Error(`${label} is invalid`);
  }

  return value;
}

// validate one bundled supervisor configuration
export function validateSupervisorConfiguration(configuration) {
  // require the exact supervisor contract
  if (!isRecord(configuration) || configuration.contractVersion !== SUPERVISOR_CONFIG_CONTRACT_VERSION) {
    throw new Error(`supervisor config contract must be ${SUPERVISOR_CONFIG_CONTRACT_VERSION}`);
  }

  assertAllowedKeys(configuration, allowedSupervisorConfigurationKeys, "supervisor configuration");
  assertAllowedKeys(configuration.containers, allowedSupervisorContainerKeys, "supervisor containers");
  assertAllowedKeys(configuration.database, allowedSupervisorDatabaseKeys, "supervisor database");
  rejectSensitiveConfiguration(configuration, "supervisor configuration");

  const identityIssues = [];
  const diagnosticMeasurement = isRecord(configuration.measurement)
    ? { ...configuration.measurement, loadCondition: "normal-ingestion" }
    : configuration.measurement;
  validateMeasurement(diagnosticMeasurement, identityIssues);

  // allow only explicitly separated disposable cold diagnostics
  if (configuration.measurement?.loadCondition === "quiescent-cold-diagnostic") {
    // keep quiescent evidence outside K1 and accepted normal-ingestion matrices
    if (
      !new Set(["K2", "K3"]).has(configuration.tier) ||
      configuration.measurement.evidenceClass !== "quiescent-cold-diagnostic"
    ) {
      identityIssues.push("quiescent cold diagnostics require K2 or K3 diagnostic evidence class");
    }
  } else if (configuration.measurement?.loadCondition !== "normal-ingestion") {
    identityIssues.push("supervisor loadCondition must be normal-ingestion or quiescent-cold-diagnostic");
  }

  // require only the selected supervisor tier identity
  if (!isRecord(configuration.runtimeIdentities) || !isRecord(configuration.runtimeAssignments)) {
    identityIssues.push("supervisor runtime identities or assignments are missing");
  } else {
    const selectedRuntimeId = configuration.runtimeAssignments[configuration.tier];
    const selectedRuntime = configuration.runtimeIdentities[selectedRuntimeId];

    // bind the selected tier to one complete runtime
    if (!isNonemptyString(selectedRuntimeId) || !isRecord(selectedRuntime)) {
      identityIssues.push("supervisor selected runtime identity is missing");
    } else {
      // bind every material runtime identity field
      for (const field of identityFields) {
        // reject an incomplete selected runtime
        if (!isNonemptyString(selectedRuntime[field])) {
          identityIssues.push(`supervisor runtime ${selectedRuntimeId} ${field} is missing`);
        }
      }

      const expectedEnvironment = assignmentEnvironment[configuration.tier];

      // preserve the selected tier environment contract
      if (
        (expectedEnvironment !== null && selectedRuntime.environment !== expectedEnvironment) ||
        (configuration.tier === "K2" && selectedRuntime.environment !== "blueberry" && selectedRuntime.environment !== "disposable")
      ) {
        identityIssues.push(`supervisor runtime ${selectedRuntimeId} environment is invalid for ${configuration.tier}`);
      }
    }
  }

  // reject incomplete evaluator identities
  if (identityIssues.length > 0) {
    throw new Error(identityIssues.join("; "));
  }
  // permit only supervisor-backed cold tiers
  if (!new Set(["K1", "K2", "K3"]).has(configuration.tier)) {
    throw new Error("supervisor tier must be K1 K2 or K3");
  }
  // bind the exact assigned runtime
  if (configuration.runtimeAssignments[configuration.tier] !== configuration.runtimeId) {
    throw new Error("supervisor runtime does not match its tier assignment");
  }

  const runtime = configuration.runtimeIdentities[configuration.runtimeId];

  // require disposable scope only for database-cold supervision
  if (configuration.tier === "K3" && runtime.environment !== "disposable") {
    throw new Error("K3 supervision requires a disposable runtime");
  }
  // validate bounded endpoint identities
  if (!Array.isArray(configuration.endpointIds) || configuration.endpointIds.length < 1 || configuration.endpointIds.length > ENDPOINT_INVENTORY.length) {
    throw new Error("supervisor endpointIds must contain one through eleven endpoints");
  }
  // preserve the frozen maximum HTTP concurrency
  if (configuration.endpointIds.length > configuration.measurement.maximumHttpConcurrency) {
    throw new Error("supervisor endpointIds exceed measurement maximumHttpConcurrency");
  }

  const uniqueEndpoints = new Set(configuration.endpointIds);

  // reject repeated or unknown routes
  if (uniqueEndpoints.size !== configuration.endpointIds.length || [...uniqueEndpoints].some(
    // identify endpoints outside the fixed inventory
    function isUnknownEndpoint(endpointId) {
      return !endpointById.has(endpointId);
    },
  )) {
    throw new Error("supervisor endpointIds contain a duplicate or unknown endpoint");
  }
  // require the exact two consumers of the shared five-minute cache
  if (
    configuration.tier === "K1" &&
    !isDeepStrictEqual([...uniqueEndpoints].sort(), ["forecast-observations", "solar-cloud-evidence"])
  ) {
    throw new Error("K1 supervision requires both shared-cache endpoints");
  }
  // bound natural-expiry cycles without resetting a cache
  if (!Number.isInteger(configuration.cycles) || configuration.cycles < 1 || configuration.cycles > 3) {
    throw new Error("supervisor cycles must be an integer from 1 through 3");
  }
  // prevent repeated process-cold labels without repeated fresh runtimes
  if (configuration.tier !== "K1" && configuration.cycles !== 1) {
    throw new Error("K2 and K3 supervision permit one fresh-runtime cycle");
  }
  // require an expiry race buffer only for K1
  if (
    !Number.isInteger(configuration.expiryBufferMs) ||
    (configuration.tier === "K1" && (configuration.expiryBufferMs < 1_000 || configuration.expiryBufferMs > 60_000)) ||
    (configuration.tier !== "K1" && configuration.expiryBufferMs !== 0)
  ) {
    throw new Error("supervisor expiryBufferMs is invalid for the selected tier");
  }
  // bound each complete-body request
  if (!Number.isInteger(configuration.requestTimeoutMs) || configuration.requestTimeoutMs < 100 || configuration.requestTimeoutMs > 30_000) {
    throw new Error("supervisor requestTimeoutMs must be an integer from 100 through 30000");
  }

  const containerNames = [configuration.containers.web, configuration.containers.api, configuration.containers.database];

  // validate exact distinct container names
  for (const [index, name] of containerNames.entries()) {
    validateSupervisorName(name, `supervisor container ${String(index + 1)}`);
  }
  // reject ambiguous service observations
  if (new Set(containerNames).size !== containerNames.length) {
    throw new Error("supervisor container names must be distinct");
  }

  const databasePattern = /^[a-z_][a-z0-9_]{0,62}$/u;
  validateSupervisorName(configuration.database.name, "supervisor database name", databasePattern);
  validateSupervisorName(configuration.database.user, "supervisor database user", databasePattern);
  validateSupervisorName(configuration.database.applicationName, "supervisor database application name", /^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,62}$/u);
  return configuration;
}

// execute one bounded local program
async function executeSupervisorProgram(file, argumentsList) {
  return await executeFile(file, argumentsList, {
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
    timeout: 45_000,
  });
}

// parse one docker inspect observation
export function parseDockerContainerObservation(stdout) {
  const fields = stdout.trim().split("\t");

  // require exactly nine json-formatted inspect fields
  if (fields.length !== 9) {
    throw new Error("docker inspect output is incomplete");
  }

  const containerId = JSON.parse(fields[0]);
  const startedAt = JSON.parse(fields[1]);
  const pid = JSON.parse(fields[2]);
  const imageId = JSON.parse(fields[3]);
  const configImage = JSON.parse(fields[4]);
  const project = JSON.parse(fields[5]);
  const service = JSON.parse(fields[6]);
  const sourceCommit = JSON.parse(fields[7]);
  const mounts = JSON.parse(fields[8]);
  const dataMount = Array.isArray(mounts) ? mounts.find(
    // select only the PostgreSQL data mount
    function findDataMount(mount) {
      return mount?.Destination === "/var/lib/postgresql/data";
    },
  ) : undefined;
  const dataVolume = dataMount?.Name ?? dataMount?.Source ?? null;

  // require a running container identity
  if (
    !isNonemptyString(containerId) ||
    !isTimestamp(startedAt) ||
    !Number.isInteger(pid) ||
    pid < 1 ||
    !/^sha256:[a-f0-9]{64}$/u.test(imageId ?? "") ||
    !isNonemptyString(configImage) ||
    !isNonemptyString(project) ||
    !isNonemptyString(service) ||
    (sourceCommit !== null && !isNonemptyString(sourceCommit))
  ) {
    throw new Error("docker inspect output does not identify a running container");
  }

  return { configImage, containerId, dataVolume, imageId, pid, project, service, sourceCommit, startedAt };
}

// inspect one exact running container
async function inspectSupervisorContainer(execute, containerName) {
  const result = await execute("docker", [
    "inspect",
    "--format",
    "{{json .Id}}\t{{json .State.StartedAt}}\t{{json .State.Pid}}\t{{json .Image}}\t{{json .Config.Image}}\t{{json (index .Config.Labels \"com.docker.compose.project\")}}\t{{json (index .Config.Labels \"com.docker.compose.service\")}}\t{{json (index .Config.Labels \"org.opencontainers.image.revision\")}}\t{{json .Mounts}}",
    containerName,
  ]);
  return parseDockerContainerObservation(result.stdout);
}

// inspect all three service processes
async function inspectSupervisorProcesses(configuration, execute) {
  const [web, api, database] = await Promise.all([
    inspectSupervisorContainer(execute, configuration.containers.web),
    inspectSupervisorContainer(execute, configuration.containers.api),
    inspectSupervisorContainer(execute, configuration.containers.database),
  ]);
  return { api, database, web };
}

// parse one database supervisor observation
export function parseSupervisorDatabaseObservation(stdout) {
  const value = JSON.parse(stdout.trim());

  // require the closed database result shape
  if (
    !isRecord(value) ||
    !isTimestamp(value.postgresStartedAt) ||
    !Array.isArray(value.backends) ||
    value.backends.length === 0 ||
    !Array.isArray(value.schemaLedger) ||
    value.schemaLedger.length === 0 ||
    !isNonemptyString(value.serverVersionNum)
  ) {
    throw new Error("database supervisor output is incomplete");
  }

  const backends = value.backends.map(
    // normalize one PostgreSQL backend identity
    function normalizeBackend(backend) {
      const normalized = {
        pid: backend?.pid,
        startedAt: backend?.startedAt,
      };

      // reject an incomplete application backend
      if (!Number.isInteger(normalized.pid) || normalized.pid < 1 || !isTimestamp(normalized.startedAt)) {
        throw new Error("database supervisor backend is incomplete");
      }
      return normalized;
    },
  ).sort(
    // retain deterministic pool identity ordering
    function compareBackend(left, right) {
      return left.pid - right.pid || left.startedAt.localeCompare(right.startedAt);
    },
  );

  return {
    databasePool: {
      backends,
      generationId: databasePoolGenerationId(backends),
    },
    postgresStartedAt: value.postgresStartedAt,
    schemaLedgerSha256: sha256Json(value.schemaLedger),
    schemaVersion: String(value.schemaLedger.at(-1)?.name ?? "").slice(0, 4),
    serverVersionNum: value.serverVersionNum,
  };
}

// inspect PostgreSQL without exposing its admin secret
async function inspectSupervisorDatabase(configuration, execute) {
  const sql = `SELECT json_build_object(
    'postgresStartedAt', pg_postmaster_start_time(),
    'backends', COALESCE((
      SELECT json_agg(json_build_object('pid', pid, 'startedAt', backend_start) ORDER BY pid, backend_start)
      FROM pg_stat_activity
      WHERE application_name = '${configuration.database.applicationName}'
    ), '[]'::json)
    , 'schemaLedger', (
      SELECT json_agg(json_build_object('name', name, 'checksum', checksum) ORDER BY name)
      FROM schema_migrations
    )
    , 'serverVersionNum', current_setting('server_version_num')
  )::text;`;
  const script = `export PGPASSWORD="$(cat /run/secrets/weather_admin_password)"
exec psql -XAtq -v ON_ERROR_STOP=1 -U "$1" -d "$2" -c "$3"`;
  const result = await execute("docker", [
    "exec",
    configuration.containers.database,
    "sh",
    "-ceu",
    script,
    "weather-page-supervisor",
    configuration.database.user,
    configuration.database.name,
    sql,
  ]);
  const boot = await execute("docker", [
    "exec",
    configuration.containers.database,
    "cat",
    "/proc/sys/kernel/random/boot_id",
  ]);
  const parsed = parseSupervisorDatabaseObservation(result.stdout);
  const osBootId = boot.stdout.trim();

  // require one concrete kernel identity
  if (!/^[a-f0-9-]{16,64}$/u.test(osBootId)) {
    throw new Error("database container OS boot identity is invalid");
  }

  return { ...parsed, osBootId };
}

const dockerHttpProbe = String.raw`
import { request } from "node:http";

// measure one complete response
function probe(path, timeoutMs) {
  const startedAt = new Date().toISOString();
  const started = performance.now();

  // resolve one complete response or terminal transport failure
  return new Promise(function observeRequest(resolvePromise) {
    let bodyComplete = false;
    let responseBytes = 0;
    let responsePrefix = "";
    let settled = false;
    let status = null;

    // finish exactly one measured attempt
    function finish(error = null) {
      // ignore duplicate terminal events
      if (settled) {
        return;
      }
      settled = true;
      const generatedAt = /"generatedAt"\s*:\s*"([^"]+)"/u.exec(responsePrefix)?.[1] ?? null;
      resolvePromise({
        bodyComplete,
        completedAt: new Date().toISOString(),
        elapsedMs: performance.now() - started,
        error: error === null ? null : String(error?.message ?? error),
        path,
        responseBytes,
        responseGeneratedAt: generatedAt,
        startedAt,
        status,
      });
    }

    // consume the complete response body
    function receive(response) {
      status = response.statusCode ?? null;

      // retain bytes and only a bounded metadata prefix
      response.on("data", function receiveChunk(chunk) {
        responseBytes += chunk.byteLength;

        // retain only bounded generatedAt evidence
        if (responsePrefix.length < 65536) {
          responsePrefix += Buffer.from(chunk).toString("utf8").slice(0, 65536 - responsePrefix.length);
        }
      });
      // finish only after the body stream ends
      response.on("end", function endBody() {
        bodyComplete = response.complete;
        finish(bodyComplete ? null : new Error("response body ended incomplete"));
      });
      response.on("aborted", function abortBody() {
        finish(new Error("response body aborted"));
      });
      response.on("error", finish);
    }

    const observedRequest = request(new URL(path, "http://127.0.0.1:3000"), {
      agent: false,
      headers: { accept: "application/json" },
      method: "GET",
      signal: AbortSignal.timeout(timeoutMs),
    }, receive);
    observedRequest.on("error", finish);
    observedRequest.end();
  });
}

const paths = JSON.parse(process.argv[1]);
const timeoutMs = Number(process.argv[2]);
const results = await Promise.all(paths.map(
  // dispatch every shared-cache consumer before awaiting either body
  function dispatch(path) {
    return probe(path, timeoutMs);
  },
));
process.stdout.write(JSON.stringify(results));
`;

// collect concurrent complete-body timings inside the web container
async function collectDockerHttpGroup(configuration, execute) {
  const paths = configuration.endpointIds.map(
    // select one exact frozen route
    function selectEndpointPath(endpointId) {
      return endpointById.get(endpointId).path;
    },
  );
  const result = await execute("docker", [
    "exec",
    configuration.containers.web,
    "node",
    "--input-type=module",
    "-e",
    dockerHttpProbe,
    JSON.stringify(paths),
    String(configuration.requestTimeoutMs),
  ]);
  const samples = JSON.parse(result.stdout);

  // require one result per exact requested route
  if (!Array.isArray(samples) || samples.length !== paths.length) {
    throw new Error("web-container probe output is incomplete");
  }

  return samples;
}

// pause without resetting any application cache
async function sleepSupervisor(milliseconds) {
  await new Promise(
    // resolve only after natural elapsed time
    function waitForExpiry(resolvePromise) {
      setTimeout(resolvePromise, milliseconds);
    },
  );
}

// observe one complete request group and its stable runtime
async function observeSupervisorGroup(configuration, controls) {
  const before = await controls.inspectProcesses(configuration, controls.execute);
  const rawSamples = await controls.collectGroup(configuration, controls.execute);
  const after = await controls.inspectProcesses(configuration, controls.execute);
  const database = await controls.inspectDatabase(configuration, controls.execute);
  const observation = { after, before, ...database };
  return { observation, rawSamples };
}

// construct one evaluator sample from a web-container observation
function buildSupervisorSample(configuration, endpointId, raw, cacheStateId) {
  const endpoint = endpointById.get(endpointId);
  const applicationCache = endpoint.applicationCache && endpoint.id !== "viewer-context"
    ? configuration.tier === "K1" ? "naturally-expired" : "empty"
    : "not-applicable";
  const freshProcesses = configuration.tier === "K2" || configuration.tier === "K3";
  const disposableDatabase = configuration.tier === "K3";

  return {
    bodyComplete: raw.bodyComplete,
    cacheStateId,
    completedAt: raw.completedAt,
    elapsedMs: raw.elapsedMs,
    eligibility: endpoint.eligibility,
    endpointId,
    error: raw.error,
    requestPath: endpoint.path,
    responseBytes: raw.responseBytes,
    responseGeneratedAt: isTimestamp(raw.responseGeneratedAt) ? raw.responseGeneratedAt : null,
    run: null,
    runtimeId: configuration.runtimeId,
    sampleId: randomUUID(),
    startedAt: raw.startedAt,
    status: raw.status,
    tier: configuration.tier,
    timingBoundary: "dispatch-to-complete-body",
    topology: {
      apiProcess: freshProcesses ? "fresh" : "existing",
      applicationCache,
      boundary: "loopback-web-complete-body",
      collector: "web-container",
      connection: "fresh",
      databasePool: freshProcesses ? "fresh" : "existing",
      osCache: disposableDatabase ? "fresh-disposable" : "uncontrolled",
      postgresCache: disposableDatabase ? "fresh-disposable" : "uncontrolled",
      webProcess: freshProcesses ? "fresh" : "existing",
    },
    transport: "http",
    witnessId: cacheStateId,
  };
}

// construct one hash-bound supervisor witness
function buildSupervisorWitness(configuration, sample, observation, applicationCache) {
  return {
    applicationCache,
    endpointId: sample.endpointId,
    id: sample.witnessId,
    observation,
    observationSha256: sha256Json(observation),
    observedAt: new Date().toISOString(),
    producer: SUPERVISOR_PRODUCER,
    runtimeId: configuration.runtimeId,
    sampleId: sample.sampleId,
    scope: configuration.runtimeIdentities[configuration.runtimeId].environment,
    tier: configuration.tier,
  };
}

// collect independently observed cold samples without cache mutation
export async function superviseColdSamples(configuration, injected = {}) {
  validateSupervisorConfiguration(configuration);
  const controls = {
    collectGroup: injected.collectGroup ?? collectDockerHttpGroup,
    execute: injected.execute ?? executeSupervisorProgram,
    inspectDatabase: injected.inspectDatabase ?? inspectSupervisorDatabase,
    inspectProcesses: injected.inspectProcesses ?? inspectSupervisorProcesses,
    now: injected.now ?? Date.now,
    sleep: injected.sleep ?? sleepSupervisor,
  };
  const samples = [];
  const witnesses = [];
  const supervisionIssues = [];
  const diagnostics = [];
  let priorGeneratedAt = null;

  // prime only the shared cache before its first natural-expiry cycle
  if (configuration.tier === "K1") {
    const primed = await controls.collectGroup(configuration, controls.execute);
    diagnostics.push({ kind: "K1-prime", samples: primed });
    priorGeneratedAt = primed[0]?.responseGeneratedAt;

    // require both consumers to observe the same complete cache generation
    if (
      !isTimestamp(priorGeneratedAt) ||
      primed.some(
        // reject a failed or mismatched prime
        function primeIsInvalid(sample, index) {
          return sample.path !== endpointById.get(configuration.endpointIds[index]).path ||
            sample.bodyComplete !== true ||
            sample.status < 200 ||
            sample.status >= 300 ||
            sample.responseGeneratedAt !== priorGeneratedAt;
        },
      )
    ) {
      supervisionIssues.push("K1 shared-cache prime did not return one complete generation");
      return {
        evidence: {
          contractVersion: EVIDENCE_CONTRACT_VERSION,
          measurement: configuration.measurement,
          runtimeAssignments: configuration.runtimeAssignments,
          runtimeIdentities: configuration.runtimeIdentities,
          samples,
          stateWitnesses: {},
        },
        supervisionResult: { diagnostics, issues: supervisionIssues, pass: false },
        witnessFile: {
          contractVersion: STATE_WITNESS_CONTRACT_VERSION,
          producer: SUPERVISOR_PRODUCER,
          witnesses,
        },
      };
    }
  }

  // observe each bounded natural-expiry or fresh-runtime cycle
  for (let cycle = 0; cycle < configuration.cycles; cycle += 1) {
    // wait until the observed generation has naturally expired
    if (configuration.tier === "K1") {
      const expiresAt = Date.parse(priorGeneratedAt) + OBSERVATION_CACHE_TTL_MS;
      const waitMs = Math.max(0, expiresAt + configuration.expiryBufferMs - controls.now());
      await controls.sleep(waitMs);
    }

    const { observation, rawSamples } = await observeSupervisorGroup(configuration, controls);

    // bind every probe result to its exact requested route
    for (let index = 0; index < rawSamples.length; index += 1) {
      const endpoint = endpointById.get(configuration.endpointIds[index]);

      // reject reordered or substituted probe output
      if (rawSamples[index]?.path !== endpoint.path) {
        throw new Error("supervisor probe route does not match the fixed endpoint");
      }
    }
    const cycleSamples = rawSamples.map(
      // bind one route result to one unique cache-state identity
      function bindSample(raw, index) {
        return buildSupervisorSample(configuration, configuration.endpointIds[index], raw, randomUUID());
      },
    );
    const cohortId = randomUUID();
    const cohortWitnessIds = cycleSamples.map(
      // retain exact witness identities in dispatch order
      function selectWitnessId(sample) {
        return sample.witnessId;
      },
    );

    // bind every measured request to the same observed runtime
    for (const sample of cycleSamples) {
      const applicationCache = configuration.tier === "K1"
        ? {
            cfPathObserved: false,
            cohortId,
            cohortWitnessIds,
            completedAt: sample.completedAt,
            discoveryRefreshedAt: null,
            dispatchAt: sample.startedAt,
            expiredAt: new Date(Date.parse(priorGeneratedAt) + OBSERVATION_CACHE_TTL_MS).toISOString(),
            priorGeneratedAt,
            responseGeneratedAt: sample.responseGeneratedAt,
            state: "naturally-expired",
          }
        : null;
      const witness = buildSupervisorWitness(configuration, sample, observation, applicationCache);
      samples.push(sample);
      witnesses.push(witness);
    }

    const stateWitnesses = Object.fromEntries(witnesses.map(
      // index every accumulated witness for cross-cohort validation
      function indexWitness(witness) {
        return [witness.id, witness];
      },
    ));

    // reject dishonest labels before emitting any artifact
    for (const sample of cycleSamples) {
      const sampleIssues = validateSample(
        sample,
        endpointById.get(sample.endpointId),
        sample.tier,
        sample.runtimeId,
        configuration.runtimeIdentities[sample.runtimeId],
        stateWitnesses,
      );

      // fail closed on incomplete observation
      if (sampleIssues.length > 0) {
        for (const issue of sampleIssues) {
          addIssue(supervisionIssues, `${sample.sampleId}: ${issue}`);
        }
      }
    }

    // carry the measured refill into the next natural-expiry cycle
    if (configuration.tier === "K1") {
      priorGeneratedAt = cycleSamples[0].responseGeneratedAt;
    }
  }

  return {
    evidence: {
      contractVersion: EVIDENCE_CONTRACT_VERSION,
      measurement: configuration.measurement,
      runtimeAssignments: configuration.runtimeAssignments,
      runtimeIdentities: configuration.runtimeIdentities,
      samples,
      stateWitnesses: {},
    },
    witnessFile: {
      contractVersion: STATE_WITNESS_CONTRACT_VERSION,
      producer: SUPERVISOR_PRODUCER,
      witnesses,
    },
    supervisionResult: { diagnostics, issues: supervisionIssues, pass: supervisionIssues.length === 0 },
  };
}

// measure one HTTP response through complete body consumption
export async function collectHttpSample({
  agent,
  cacheStateId,
  eligibility,
  endpointId,
  origin,
  run,
  runtimeId,
  tier,
  timeoutMs,
  topology,
  witnessId,
}) {
  const endpoint = endpointById.get(endpointId);

  // reject unknown endpoints before network activity
  if (endpoint === undefined || !requiredTiersForEndpoint(endpointId).includes(tier)) {
    throw new Error(`unknown endpoint ${String(endpointId)}`);
  }

  // bind direct callers to one exact stratum
  if (
    eligibility !== endpoint.eligibility ||
    !isNonemptyString(runtimeId) ||
    !isNonemptyString(cacheStateId) ||
    !Number.isInteger(timeoutMs) ||
    timeoutMs < 100 ||
    timeoutMs > 30_000 ||
    (tier === "warm" && (!Number.isInteger(run) || run < 1 || run > WARM_RUNS)) ||
    (tier !== "warm" && run !== null)
  ) {
    throw new Error("HTTP sample does not match a bounded fixed stratum");
  }

  const topologyIssues = validateTopology({ topology }, tier);

  // reject dishonest state labels before dispatch
  if (topologyIssues.length > 0) {
    throw new Error(topologyIssues.join("; "));
  }

  const loopbackOrigin = validateLoopbackOrigin(origin);

  const startedAt = new Date().toISOString();
  const started = performance.now();

  // resolve after one terminal transport event
  return await new Promise(function collectRequest(resolvePromise) {
    let bodyComplete = false;
    let responseBytes = 0;
    let status = null;
    let settled = false;
    let transportError = null;
    let request;
    let responseGeneratedAt = null;
    let responsePrefix = "";

    // finish the attempt once
    function finish(error = null) {
      // ignore duplicate terminal events
      if (settled) {
        return;
      }
      settled = true;
      transportError = error === null ? transportError : String(error.message ?? error);
      resolvePromise({
        bodyComplete,
        cacheStateId,
        completedAt: new Date().toISOString(),
        elapsedMs: performance.now() - started,
        eligibility,
        endpointId,
        error: transportError,
        requestPath: endpoint.path,
        responseBytes,
        responseGeneratedAt,
        run,
        runtimeId,
        sampleId: randomUUID(),
        startedAt,
        status,
        tier,
        timingBoundary: "dispatch-to-complete-body",
        topology: {
          ...topology,
          connection: request?.reusedSocket === true ? "reused" : "fresh",
        },
        transport: "http",
        witnessId,
      });
    }

    // consume the response body fully
    function receiveResponse(response) {
      status = response.statusCode ?? null;

      // count every response chunk
      response.on("data", function countBody(chunk) {
        responseBytes += Buffer.byteLength(chunk);

        // retain only a bounded prefix for generatedAt metadata
        if (responsePrefix.length < 65_536) {
          responsePrefix += Buffer.from(chunk).toString("utf8").slice(0, 65_536 - responsePrefix.length);
        }
      });
      // mark only a clean complete body
      response.on("end", function completeBody() {
        bodyComplete = response.complete;
        responseGeneratedAt = extractResponseGeneratedAt(responsePrefix);
        responsePrefix = "";
        finish(bodyComplete ? null : new Error("response body ended incomplete"));
      });
      // retain aborted responses as failures
      response.on("aborted", function abortBody() {
        finish(new Error("response body aborted"));
      });
      // retain response stream failures
      response.on("error", finish);
    }

    request = httpRequest(new URL(endpoint.path, loopbackOrigin), {
      agent,
      headers: { accept: "application/json" },
      method: "GET",
      signal: AbortSignal.timeout(timeoutMs),
    }, receiveResponse);

    // retain request failures in the denominator
    request.on("error", finish);
    request.end();
  });
}

// extract only safe cache-generation metadata from a body prefix
function extractResponseGeneratedAt(prefix) {
  const match = /"generatedAt"\s*:\s*"([^"]+)"/u.exec(prefix);
  return isTimestamp(match?.[1]) ? match[1] : null;
}

// expand collection declarations into measured attempts
export async function collectConfiguredSamples(configuration) {
  validateConfiguration(configuration);
  const tasks = [];

  // create only explicitly requested samples
  for (const collection of configuration.collections) {
    // create one attempt per declared count
    for (let index = 0; index < collection.count; index += 1) {
      tasks.push(collection);
    }
  }

  const samples = new Array(tasks.length);
  const agents = new Map();
  let nextTask = 0;

  // share an agent only when reuse is explicitly declared
  function agentFor(collection) {
    // force a new client connection when declared fresh
    if (collection.topology.connection === "fresh") {
      return false;
    }

    const key = collection.origin;
    let agent = agents.get(key);

    // create one bounded keep-alive agent
    if (agent === undefined) {
      agent = new Agent({ keepAlive: true, maxSockets: configuration.concurrency });
      agents.set(key, agent);
    }
    return agent;
  }

  // run one bounded queue worker
  async function worker() {
    // claim work until the queue is empty
    while (nextTask < tasks.length) {
      const index = nextTask;
      nextTask += 1;
      const collection = tasks[index];
      samples[index] = await collectHttpSample({
        agent: agentFor(collection),
        cacheStateId: collection.cacheStateId,
        eligibility: collection.eligibility,
        endpointId: collection.endpointId,
        origin: validateLoopbackOrigin(collection.origin),
        run: collection.run,
        runtimeId: collection.runtimeId,
        tier: collection.tier,
        timeoutMs: configuration.requestTimeoutMs,
        topology: collection.topology,
        witnessId: collection.witnessId,
      });
    }
  }

  const workers = [];
  const workerCount = Math.min(configuration.concurrency, tasks.length);

  // start only the bounded worker count
  for (let index = 0; index < workerCount; index += 1) {
    workers.push(worker());
  }

  try {
    await Promise.all(workers);
  } finally {
    // close every keep-alive agent
    for (const agent of agents.values()) {
      agent.destroy();
    }
  }

  return samples;
}

// load and combine exact-identity evidence files
async function assembleEvidence(configuration, configurationDirectory) {
  const samples = [];
  const stateWitnesses = {};

  // load every private supervisor witness input
  for (const input of configuration.stateWitnessFiles) {
    // require one private witness file path
    if (!isNonemptyString(input)) {
      throw new Error("state witness file paths must be nonempty strings");
    }
    const witnessFile = await readPrivateJson(resolve(configurationDirectory, input));
    rejectSensitiveConfiguration(witnessFile, "state witness file");
    assertAllowedKeys(witnessFile, allowedStateWitnessFileKeys, "state witness file");

    // require the external supervisor contract
    if (
      witnessFile.contractVersion !== STATE_WITNESS_CONTRACT_VERSION ||
      witnessFile.producer !== SUPERVISOR_PRODUCER ||
      !Array.isArray(witnessFile.witnesses)
    ) {
      throw new Error(`${input} is not a supervisor state-witness file`);
    }

    // bind every unique supervisor witness
    for (const witness of witnessFile.witnesses) {
      assertAllowedKeys(witness, allowedStateWitnessKeys, "state witness");

      // reject undeclared nested cache evidence
      if (witness.applicationCache !== null && witness.applicationCache !== undefined) {
        assertAllowedKeys(witness.applicationCache, allowedApplicationCacheWitnessKeys, "application-cache witness");
      }
      // reject undeclared disposable PGDATA proof fields
      if (witness.pgdataColdProof !== null && witness.pgdataColdProof !== undefined) {
        assertAllowedKeys(witness.pgdataColdProof, allowedPgdataColdProofKeys, "PGDATA cold proof");
      }

      // reject duplicate or missing witness identities
      if (!isNonemptyString(witness.id) || stateWitnesses[witness.id] !== undefined) {
        throw new Error(`${input} contains a missing or duplicate witness id`);
      }
      stateWitnesses[witness.id] = witness;
    }
  }

  // load every private evidence input
  for (const input of configuration.evidenceFiles) {
    // require a simple path string
    if (!isNonemptyString(input)) {
      throw new Error("evidence file paths must be nonempty strings");
    }
    const evidence = await readPrivateJson(resolve(configurationDirectory, input));
    rejectSensitiveConfiguration(evidence, "evidence");
    assertAllowedKeys(evidence, allowedEvidenceKeys, "evidence");

    const assignmentSubsetMatches = isRecord(evidence.runtimeAssignments) && Object.entries(evidence.runtimeAssignments).every(
      // require each observed tier to retain its final assignment
      function assignmentMatches([tier, runtimeId]) {
        return configuration.runtimeAssignments[tier] === runtimeId;
      },
    );
    const runtimeSubsetMatches = isRecord(evidence.runtimeIdentities) && Object.entries(evidence.runtimeIdentities).every(
      // require each observed runtime to retain its final exact identity
      function runtimeMatches([runtimeId, identity]) {
        return isDeepStrictEqual(configuration.runtimeIdentities[runtimeId], identity);
      },
    );
    const samplesMatchObservedRuntimes = Array.isArray(evidence.samples) && evidence.samples.every(
      // bind every partial supervisor sample to an included assignment and identity
      function sampleMatchesObservedRuntime(sample) {
        return evidence.runtimeAssignments?.[sample.tier] === sample.runtimeId && isRecord(evidence.runtimeIdentities?.[sample.runtimeId]);
      },
    );

    // reject mixed contracts or conflicting identity subsets
    if (
      evidence.contractVersion !== EVIDENCE_CONTRACT_VERSION ||
      !isDeepStrictEqual(evidence.measurement, configuration.measurement) ||
      !assignmentSubsetMatches ||
      !runtimeSubsetMatches ||
      !samplesMatchObservedRuntimes ||
      (isRecord(evidence.stateWitnesses) && Object.keys(evidence.stateWitnesses).length > 0)
    ) {
      throw new Error(`${input} does not match the configured evidence identity`);
    }

    // reject undeclared fields before retaining evidence
    for (const sample of evidence.samples) {
      assertAllowedKeys(sample, allowedSampleKeys, "evidence sample");
      assertAllowedKeys(sample.topology, allowedTopologyKeys, "evidence sample topology");
      samples.push(sample);
    }
  }

  samples.push(...await collectConfiguredSamples(configuration));
  return {
    contractVersion: EVIDENCE_CONTRACT_VERSION,
    measurement: configuration.measurement,
    runtimeAssignments: configuration.runtimeAssignments,
    runtimeIdentities: configuration.runtimeIdentities,
    samples,
    stateWitnesses,
  };
}

// prepare one private no-overwrite output directory
async function prepareOutputDirectory(outputPath) {
  // create a new private directory when absent
  try {
    await mkdir(outputPath, { mode: 0o700 });
  } catch (error) {
    // accept only an already-existing directory
    if (error?.code !== "EEXIST") {
      throw error;
    }
  }

  await assertPrivateOwnedPath(outputPath, "directory");
}

// write one private immutable JSON artifact
async function writePrivateJson(path, value) {
  await writeFile(path, `${JSON.stringify(value, null, 2)}\n`, {
    flag: "wx",
    mode: 0o600,
  });
}

// parse one fixed evaluator command
function parseArguments(argumentsList) {
  const command = argumentsList[0];

  // require one supported read-only evidence command
  if (command !== "evaluate" && command !== "supervise") {
    throw new Error("usage: database-page-performance.mjs <evaluate|supervise> --config <private-file> --output <private-directory>");
  }

  let configurationPath = null;
  let outputPath = null;

  // parse exact option pairs
  for (let index = 1; index < argumentsList.length; index += 2) {
    const option = argumentsList[index];
    const value = argumentsList[index + 1];

    // require complete option pairs
    if (!isNonemptyString(value)) {
      throw new Error(`missing value for ${String(option)}`);
    }
    // bind the configuration path
    if (option === "--config") {
      configurationPath = resolve(value);
      continue;
    }
    // bind the output path
    if (option === "--output") {
      outputPath = resolve(value);
      continue;
    }
    throw new Error(`unknown option ${String(option)}`);
  }

  // require both private paths
  if (configurationPath === null || outputPath === null) {
    throw new Error("both --config and --output are required");
  }

  return { command, configurationPath, outputPath };
}

// execute one evaluator invocation
export async function runCli(argumentsList = process.argv.slice(2)) {
  const { command, configurationPath, outputPath } = parseArguments(argumentsList);
  const rawConfiguration = await readPrivateJson(configurationPath);
  await prepareOutputDirectory(outputPath);

  // run the bundled observing producer without accepting caller state labels
  if (command === "supervise") {
    const configuration = validateSupervisorConfiguration(rawConfiguration);
    const output = await superviseColdSamples(configuration);
    await writePrivateJson(resolve(outputPath, "evidence.json"), output.evidence);
    await writePrivateJson(resolve(outputPath, "state-witnesses.json"), output.witnessFile);
    await writePrivateJson(resolve(outputPath, "supervision-result.json"), output.supervisionResult);
    return output.supervisionResult.pass ? 0 : 1;
  }

  const configuration = validateConfiguration(rawConfiguration);
  const evidence = await assembleEvidence(configuration, dirname(configurationPath));
  const result = evaluateEvidence(evidence);
  await writePrivateJson(resolve(outputPath, "evidence.json"), evidence);
  await writePrivateJson(resolve(outputPath, "result.json"), result);
  return result.pass ? 0 : 1;
}

// run only when invoked as a script
if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    process.exitCode = await runCli();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 2;
  }
}
