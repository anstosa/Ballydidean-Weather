import { pathToFileURL } from "node:url";
import { setTimeout as pause } from "node:timers/promises";

import {
  BACKEND_PERFORMANCE_DIRECT_PROCESS_COLD_PREPARATION,
  evaluateBackendPerformanceDirect,
} from "./backend-performance-direct-evaluator.mjs";
import {
  bindBackendPerformanceDirectSchedule,
} from "./backend-performance-direct-binding.mjs";
import {
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";
import { receiptSha256 } from "./backend-performance-receipts.mjs";

export const BACKEND_PERFORMANCE_DIRECT_NATIVE_HTTP_CLIENT_VERSION =
  "weather-backend-performance-direct-native-http-client/v1";
export const BACKEND_PERFORMANCE_DIRECT_FIXTURE_AUTHORITY_VERSION =
  "weather-backend-performance-direct-fixture-authority/v1";
export const BACKEND_PERFORMANCE_DIRECT_FIXTURE_LIFECYCLE_VERSION =
  "weather-backend-performance-direct-fixture-lifecycle/v1";
export const BACKEND_PERFORMANCE_DIRECT_PHYSICAL_PROOF_VERSION =
  "weather-backend-performance-direct-physical-proof/v1";

const PUBLIC_ORIGIN = "https://weather.ballydidean.farm";
const MAXIMUM_RESPONSE_BYTES = 16 * 1024 * 1024;
const WARM_TIMEOUT_MS = 60_000;
const PACING_MS = 500;
const MAXIMUM_DURATION_MS = 8 * 60 * 60 * 1_000;
const APPROVED_PROCESS_BOUNDARY_AFTER_ATTEMPT = 741;
const APPROVED_PAUSE_BOUNDARIES = Object.freeze([1_840, 2_990, 4_160, 5_203, 5_205, 5_400]);
const APPROVED_PAUSE_DURATION_MS = 252_987;
const APPROVED_WARM_SUMMARY_SHA256 =
  "9a2c32c8e6309539051fdd15a6354f543d54c819d771211833f3795d9d438f08";
const APPROVED_MATCHED_POLICY =
  "same master/client runtime+affinity, same process boundary, same request/error/resource pause rules in candidate; no earlier canceled cohorts";
const APPROVED_PAUSE_POLICY =
  "same evaluator process within each segment; checkpoint after every 40 terminal attempts, >=90s, or ordinary HTTP error; pause before the next request on transient client/server/control headroom veto; issue no request during a pause; require two consecutive clear client/server probes separated by 15s; resume the same exact next attempt without pooling or retry";
const SHA256_PATTERN = /^[a-f0-9]{64}$/u;
const FIXTURE_ID_PATTERN = /^rust-rewrite-[a-f0-9]{16}$/u;
const MODES = new Set(["disposable-fixture", "warm-public"]);

// create one coded client failure
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one client invariant
function requireInvariant(condition, code, message) {
  // reject unsafe or unbound execution
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
  requireInvariant(
    isRecord(value)
      && JSON.stringify(Object.keys(value).sort()) === JSON.stringify([...keys].sort()),
    "native-http-client.schema",
    `${path} has unexpected fields`,
  );
}

// validate one canonical timestamp
function timestamp(value, path) {
  requireInvariant(
    typeof value === "string" && new Date(value).toISOString() === value,
    "native-http-client.chronology",
    `${path} is not a canonical timestamp`,
  );
  return Date.parse(value);
}

// derive the complete source attempt order
function sourceAttempts() {
  const schedule = buildBackendPerformanceFunctionalSchedule();
  return schedule.runSlots.flatMap(
    // retain run-slot order
    (runSlot) => runSlot.groups.flatMap(
      // retain group order
      (group) => group.attempts,
    ),
  );
}

const SOURCE_ATTEMPTS = Object.freeze(sourceAttempts());
const SOURCE_INDEX = new Map(SOURCE_ATTEMPTS.map(
  // index one immutable source identity
  (attempt, index) => [attempt.attemptId, index],
));
const WARM_ATTEMPT_IDS = Object.freeze(SOURCE_ATTEMPTS.filter(
  // select the complete warm schedule
  (attempt) => attempt.stratum === "warm",
).map(
  // retain one original v4 identity
  (attempt) => attempt.attemptId,
));

// validate one fixed disposable loopback authority
function fixtureAuthority(value) {
  requireExactKeys(
    value,
    ["contractVersion", "fixtureId", "origin", "sourceReceiptSha256"],
    "$.fixtureAuthority",
  );
  let origin;
  try {
    origin = new URL(value.origin);
  } catch {
    fail("native-http-client.fixture-authority", "fixture origin is invalid");
  }
  requireInvariant(
    value.contractVersion === BACKEND_PERFORMANCE_DIRECT_FIXTURE_AUTHORITY_VERSION
      && FIXTURE_ID_PATTERN.test(value.fixtureId ?? "")
      && SHA256_PATTERN.test(value.sourceReceiptSha256 ?? "")
      && origin.protocol === "http:"
      && origin.hostname === "127.0.0.1"
      && origin.username === ""
      && origin.password === ""
      && origin.pathname === "/"
      && origin.search === ""
      && origin.hash === ""
      && origin.port !== ""
      && origin.href === value.origin,
    "native-http-client.fixture-authority",
    "fixture authority must bind one credential-free disposable loopback origin",
  );
  return Object.freeze({ ...value });
}

// validate the closed client request
function validateRequest(value) {
  requireExactKeys(
    value,
    ["attemptIds", "fixtureAuthority", "generation", "mode", "role", "runId"],
    "$",
  );
  requireInvariant(
    MODES.has(value.mode)
      && ((value.role === "baseline" && value.generation === "node-v1")
        || (value.role === "candidate" && value.generation === "rust-v1"))
      && typeof value.runId === "string"
      && /^[a-z0-9][a-z0-9.-]{0,127}$/u.test(value.runId),
    "native-http-client.request",
    "client mode role generation or run identity is invalid",
  );

  // bind public traffic to the complete warm schedule
  if (value.mode === "warm-public") {
    requireInvariant(
      value.attemptIds === null && value.fixtureAuthority === null,
      "native-http-client.public-scope",
      "public execution accepts only the fixed complete warm schedule",
    );
    return {
      ...value,
      attemptIds: WARM_ATTEMPT_IDS,
      fixtureAuthority: null,
      origin: PUBLIC_ORIGIN,
    };
  }

  const authority = fixtureAuthority(value.fixtureAuthority);
  requireInvariant(
    Array.isArray(value.attemptIds) && value.attemptIds.length > 0
      && value.attemptIds.every(
        // require one known non-warm source identity
        (attemptId) => typeof attemptId === "string"
          && SOURCE_INDEX.has(attemptId)
          && SOURCE_ATTEMPTS[SOURCE_INDEX.get(attemptId)].stratum !== "warm",
      )
      && new Set(value.attemptIds).size === value.attemptIds.length,
    "native-http-client.fixture-scope",
    "fixture execution requires unique non-warm source attempt identities",
  );
  let previousIndex = -1;
  // retain exact source order
  for (const attemptId of value.attemptIds) {
    const index = SOURCE_INDEX.get(attemptId);
    requireInvariant(
      index > previousIndex,
      "native-http-client.source-order",
      "fixture attempt identities are not in exact source order",
    );
    previousIndex = index;
  }
  return {
    ...value,
    attemptIds: [...value.attemptIds],
    fixtureAuthority: authority,
    origin: authority.origin,
  };
}

// reject credentials and unsafe verbs after source binding
function validateBoundAttempts(attempts) {
  // inspect every source-derived request
  for (const attempt of attempts) {
    const headerNames = Object.keys(attempt.headers).map(
      // compare header names without case ambiguity
      (name) => name.toLowerCase(),
    );
    requireInvariant(
      attempt.body === null
        && (attempt.method === "GET" || attempt.method === "HEAD")
        && !headerNames.includes("authorization")
        && !headerNames.includes("cookie")
        && !headerNames.includes("proxy-authorization"),
      "native-http-client.read-only",
      `${attempt.attemptId} is not a credential-free read-only request`,
    );
  }
}

// build one source-bound execution plan
export function buildBackendPerformanceDirectNativeHttpPlan(request) {
  const validated = validateRequest(request);
  const binding = bindBackendPerformanceDirectSchedule({
    attemptIds: validated.attemptIds,
    baseUrl: validated.origin,
    generation: validated.generation,
    maximumResponseBytes: MAXIMUM_RESPONSE_BYTES,
    resourceInterval: null,
    role: validated.role,
    runId: validated.runId,
  });
  validateBoundAttempts(binding.directConfiguration.attempts);
  const attempts = binding.directConfiguration.attempts.map(
    // apply only the approved warm deadline override
    (attempt) => validated.mode === "warm-public"
      ? { ...attempt, timeoutMs: WARM_TIMEOUT_MS }
      : { ...attempt },
  );
  const comparison = Object.freeze({
    approvedBaselineSummarySha256: APPROVED_WARM_SUMMARY_SHA256,
    approvedObservedPauseBoundaries: APPROVED_PAUSE_BOUNDARIES,
    approvedObservedPauseDurationMs: APPROVED_PAUSE_DURATION_MS,
    approvedPausePolicy: APPROVED_PAUSE_POLICY,
    approvedProcessBoundaryAfterAttempt: APPROVED_PROCESS_BOUNDARY_AFTER_ATTEMPT,
    comparisonEligible: false,
    limitation: "this client does not itself reproduce the approved two-process split or six supervised pause boundaries",
    matchedPolicy: APPROVED_MATCHED_POLICY,
    requiredProcessSegments: 2,
  });
  return Object.freeze({
    attempts: Object.freeze(attempts.map(Object.freeze)),
    binding,
    comparison,
    fixtureAuthority: validated.fixtureAuthority,
    mode: validated.mode,
    origin: validated.origin,
    policy: Object.freeze({
      concurrency: 1,
      maximumDurationMs: MAXIMUM_DURATION_MS,
      noRetries: true,
      pacingMs: PACING_MS,
      warmTimeoutMs: WARM_TIMEOUT_MS,
    }),
  });
}

// classify one complete html or json response
export function classifyBackendPerformanceDirectNativeHttpResponse({
  attempt,
  bodyBytes,
  response,
}) {
  const contentType = response.headers.get("content-type") ?? "";
  const body = bodyBytes.toString("utf8");
  let completed = false;
  try {
    // require one closed html document for pages
    if (attempt.operationId.startsWith("page.")) {
      completed = contentType.includes("text/html")
        && /<!doctype html/iu.test(body)
        && /<\/html>\s*$/iu.test(body);
    } else {
      completed = contentType.includes("application/json") && JSON.parse(body) !== null;
    }
  } catch {
    // classify malformed bodies without hiding them
    completed = false;
  }
  return {
    classification: completed
      ? "complete-html-or-json"
      : "incomplete-or-invalid-html-or-json",
    completed,
  };
}

// validate one zero-residency database proof
function validateDLoopProof(value, authority) {
  requireExactKeys(value, [
    "completedAt",
    "contractVersion",
    "inner",
    "outer",
    "startedAt",
    "storageIdentity",
    "target",
    "volumes",
  ], "$.preparation.physicalProof.sourceProof");
  const inner = value?.inner;
  const outer = value?.outer;
  const target = value?.target;
  const storage = value?.storageIdentity;
  requireExactKeys(inner, [
    "completedAt",
    "completedFiles",
    "fadviseCompleted",
    "fallbackUsed",
    "mincoreCompleted",
    "startedAt",
    "totalPages",
    "totalResidentAfter",
  ], "$.preparation.physicalProof.sourceProof.inner");
  requireExactKeys(outer, [
    "backingFileBytes",
    "completedAt",
    "completedFiles",
    "fadviseCompleted",
    "fallbackUsed",
    "mincoreCompleted",
    "startedAt",
    "totalPages",
    "totalResidentAfter",
  ], "$.preparation.physicalProof.sourceProof.outer");
  requireExactKeys(target, [
    "fileCount",
    "inventoryAfterSha256",
    "inventoryBeforeSha256",
    "pgdataRoot",
    "postmasterPidAbsent",
    "totalBytes",
    "volumeName",
  ], "$.preparation.physicalProof.sourceProof.target");
  requireExactKeys(storage, [
    "backingFile",
    "backingFileBytes",
    "loopDevice",
    "loopUuid",
    "mount",
    "mountSource",
    "volumeNames",
  ], "$.preparation.physicalProof.sourceProof.storageIdentity");
  const fixtureRoot = `/var/lib/weather-native-benchmark/${authority.fixtureId}`;
  const volumeNames = Array.from(
    { length: 5 },
    // derive the approved ordered five-slot namespace
    (_unused, index) => `weather-native-benchmark-${authority.fixtureId}-slot-${String(index + 1).padStart(2, "0")}`,
  );
  const startedAt = timestamp(value.startedAt, "$.preparation.physicalProof.sourceProof.startedAt");
  const innerStartedAt = timestamp(
    inner.startedAt,
    "$.preparation.physicalProof.sourceProof.inner.startedAt",
  );
  const innerCompletedAt = timestamp(
    inner.completedAt,
    "$.preparation.physicalProof.sourceProof.inner.completedAt",
  );
  const outerStartedAt = timestamp(
    outer.startedAt,
    "$.preparation.physicalProof.sourceProof.outer.startedAt",
  );
  const outerCompletedAt = timestamp(
    outer.completedAt,
    "$.preparation.physicalProof.sourceProof.outer.completedAt",
  );
  const completedAt = timestamp(
    value.completedAt,
    "$.preparation.physicalProof.sourceProof.completedAt",
  );
  requireInvariant(
    value.contractVersion === "weather-page-d-loop-cold-proof/v1"
      && startedAt === innerStartedAt
      && innerCompletedAt === outerStartedAt
      && outerCompletedAt === completedAt
      && innerCompletedAt >= innerStartedAt
      && outerCompletedAt >= outerStartedAt
      && Number.isSafeInteger(inner.completedFiles) && inner.completedFiles === target.fileCount
      && Number.isSafeInteger(inner.totalPages) && inner.totalPages > 0
      && inner.fadviseCompleted === true && inner.mincoreCompleted === true
      && inner.fallbackUsed === false && inner.totalResidentAfter === 0
      && outer.completedFiles === 1
      && Number.isSafeInteger(outer.totalPages) && outer.totalPages > 0
      && outer.fadviseCompleted === true && outer.mincoreCompleted === true
      && outer.fallbackUsed === false && outer.totalResidentAfter === 0
      && Number.isSafeInteger(target.fileCount) && target.fileCount > 0
      && Number.isSafeInteger(target.totalBytes) && target.totalBytes > 0
      && target.postmasterPidAbsent === true
      && SHA256_PATTERN.test(target.inventoryBeforeSha256 ?? "")
      && target.inventoryAfterSha256 === target.inventoryBeforeSha256
      && storage.backingFile === `${fixtureRoot}/fixture.ext4`
      && Number.isSafeInteger(storage.backingFileBytes) && storage.backingFileBytes > 0
      && outer.backingFileBytes === storage.backingFileBytes
      && /^\/dev\/loop[0-9]+$/u.test(storage.loopDevice ?? "")
      && storage.mountSource === storage.loopDevice
      && /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/u.test(storage.loopUuid ?? "")
      && storage.mount === `${fixtureRoot}/mnt`
      && JSON.stringify(storage.volumeNames) === JSON.stringify(volumeNames)
      && volumeNames.includes(target.volumeName)
      && target.pgdataRoot === `${storage.mount}/docker-volumes/${target.volumeName}`
      && Array.isArray(value.volumes) && value.volumes.length === 5
      && value.volumes.every(
        // require every ordered frozen volume detached
        (volume, index) => {
          requireExactKeys(
            volume,
            ["name", "runningAttachments"],
            `$.preparation.physicalProof.sourceProof.volumes[${String(index)}]`,
          );
          return volume.name === volumeNames[index]
            && Array.isArray(volume.runningAttachments)
            && volume.runningAttachments.length === 0;
        },
      ),
    "native-http-client.d-loop-proof",
    "db-os-cold requires one detached dual-layer zero-residency proof",
  );
}

// validate one exact preparation evidence bundle
function validatePreparation(attempt, prepared, authority) {
  requireExactKeys(
    prepared,
    ["baseUrl", "classification", "lifecycle", "physicalProof"],
    "$.preparation",
  );
  requireInvariant(
    prepared.baseUrl === authority.origin
      && typeof prepared.classification === "string"
      && prepared.classification.length > 0,
    "native-http-client.preparation",
    "preparation did not bind the admitted fixture origin",
  );
  const lifecycle = prepared.lifecycle;
  requireExactKeys(lifecycle, [
    "attemptId",
    "cleanupIdentitySha256",
    "contractVersion",
    "fixtureAuthoritySha256",
    "processIdentitySha256",
    "readyAt",
    "startedAt",
  ], "$.preparation.lifecycle");
  const authoritySha256 = receiptSha256(authority);
  const lifecycleStartedAt = timestamp(lifecycle.startedAt, "$.preparation.lifecycle.startedAt");
  const readyAt = timestamp(lifecycle.readyAt, "$.preparation.lifecycle.readyAt");
  requireInvariant(
    lifecycle.contractVersion === BACKEND_PERFORMANCE_DIRECT_FIXTURE_LIFECYCLE_VERSION
      && lifecycle.attemptId === attempt.attemptId
      && lifecycle.fixtureAuthoritySha256 === authoritySha256
      && SHA256_PATTERN.test(lifecycle.processIdentitySha256 ?? "")
      && SHA256_PATTERN.test(lifecycle.cleanupIdentitySha256 ?? "")
      && readyAt >= lifecycleStartedAt,
    "native-http-client.lifecycle",
    "preparation lifecycle is not bound to this attempt and fixture",
  );
  const proof = prepared.physicalProof;
  requireExactKeys(proof, [
    "attemptId",
    "completedAt",
    "contractVersion",
    "sourceContractVersion",
    "sourceProof",
    "sourceProofSha256",
  ], "$.preparation.physicalProof");
  const proofCompletedAt = timestamp(proof.completedAt, "$.preparation.physicalProof.completedAt");
  requireInvariant(
    proof.contractVersion === BACKEND_PERFORMANCE_DIRECT_PHYSICAL_PROOF_VERSION
      && proof.attemptId === attempt.attemptId
      && isRecord(proof.sourceProof)
      && typeof proof.sourceContractVersion === "string"
      && proof.sourceContractVersion.length > 0
      && proof.sourceProof.contractVersion === proof.sourceContractVersion
      && proof.sourceProofSha256 === receiptSha256(proof.sourceProof)
      && proofCompletedAt <= lifecycleStartedAt,
    "native-http-client.physical-proof",
    "physical proof is not exact current pre-request evidence",
  );

  // require the existing dual-layer proof for database/os cold work
  if (attempt.stratum === "db-os-cold") {
    validateDLoopProof(proof.sourceProof, authority);
    requireInvariant(
      proof.completedAt === proof.sourceProof.completedAt,
      "native-http-client.d-loop-proof",
      "db-os-cold proof envelope completion differs from the source proof",
    );
  }
  // require the evaluator's exact fresh-process classification
  if (attempt.stratum === "process-pool-cold") {
    requireInvariant(
      prepared.classification === BACKEND_PERFORMANCE_DIRECT_PROCESS_COLD_PREPARATION,
      "native-http-client.process-proof",
      "process-pool-cold lacks the exact fresh process lifecycle",
    );
  }
  return {
    fixtureAuthoritySha256: authoritySha256,
    lifecycleSha256: receiptSha256(lifecycle),
    physicalProofSha256: receiptSha256(proof),
    sourceProofSha256: proof.sourceProofSha256,
  };
}

// validate the narrow adapter surface
function validateAdapters(adapters, fixtureMode) {
  requireInvariant(
    isRecord(adapters)
      && Object.keys(adapters).every((key) => [
        "cleanupAttempt",
        "fetch",
        "monotonicNow",
        "onAttempt",
        "prepareAttempt",
        "sleep",
        "wallNow",
      ].includes(key))
      && (adapters.fetch === undefined || typeof adapters.fetch === "function")
      && (adapters.monotonicNow === undefined || typeof adapters.monotonicNow === "function")
      && (adapters.onAttempt === undefined || typeof adapters.onAttempt === "function")
      && (adapters.sleep === undefined || typeof adapters.sleep === "function")
      && (adapters.wallNow === undefined || typeof adapters.wallNow === "function")
      && (!fixtureMode || (typeof adapters.prepareAttempt === "function"
        && typeof adapters.cleanupAttempt === "function")),
    "native-http-client.adapters",
    "client adapters are invalid or omit the fixture lifecycle",
  );
}

// build fail-closed operation receipts for later evidence adjudication
function operationReceipts(results, preparationEvidence, status) {
  // withhold partial buckets after any stopped execution
  if (status !== "complete") {
    return [];
  }
  const groups = new Map();
  // retain first source-bucket appearance order
  for (let index = 0; index < results.length; index += 1) {
    const result = results[index];
    const key = JSON.stringify([result.operationId, result.stratum]);
    let group = groups.get(key);
    // initialize one exact operation-stratum bucket
    if (group === undefined) {
      group = [];
      groups.set(key, group);
    }
    group.push({ evidence: preparationEvidence[index], result });
  }
  return [...groups.values()].map(
    // project one complete but externally unadjudicated bucket
    (group) => {
      const terminalCounts = { completed: 0, error: 0, timeout: 0 };
      // count each terminal row once
      for (const row of group) {
        terminalCounts[row.result.terminalCategory] += 1;
      }
      return Object.freeze({
        attemptDenominator: {
          observedAttempts: group.length,
          plannedAttempts: group.length,
        },
        evidenceClass: "physical-http-unadjudicated",
        nativeEligible: false,
        operationStratum: {
          observedAttempts: group.length,
          operationId: group[0].result.operationId,
          stratum: group[0].result.stratum,
        },
        sourceBinding: {
          preparationEvidenceSha256: receiptSha256(group.map(
            // retain only hash custody summaries
            (row) => row.evidence,
          )),
          sourceAttemptIds: group.map(
            // retain the original v4 identities
            (row) => row.result.attemptId,
          ),
        },
        terminalCounts,
      });
    },
  );
}

// execute one serial source-bound http plan
export async function runBackendPerformanceDirectNativeHttpClient(request, adapters = {}) {
  const plan = buildBackendPerformanceDirectNativeHttpPlan(request);
  requireInvariant(
    plan.binding.role === "baseline",
    "native-http-client.candidate-gate",
    "candidate traffic is forbidden until an immutable native target has independently validated custody",
  );
  validateAdapters(adapters, plan.mode === "disposable-fixture");
  const sleep = adapters.sleep ?? pause;
  const wallNow = adapters.wallNow ?? (() => new Date());
  const startedAt = wallNow();
  requireInvariant(
    startedAt instanceof Date && Number.isFinite(startedAt.getTime()),
    "native-http-client.clock",
    "client start time is invalid",
  );
  const results = [];
  const preparationEvidence = [];
  let stopReason = null;

  // execute exactly one attempt at a time
  for (let index = 0; index < plan.attempts.length; index += 1) {
    const attempt = plan.attempts[index];
    const now = wallNow();
    // stop at the fixed outer duration
    if (!(now instanceof Date) || !Number.isFinite(now.getTime())
      || now.getTime() - startedAt.getTime() > MAXIMUM_DURATION_MS) {
      stopReason = "bounded outer duration reached";
      break;
    }
    let preparedOriginal = null;
    let preparedEvidence = null;
    const evaluatorAdapters = {
      classifyResponse: classifyBackendPerformanceDirectNativeHttpResponse,
      fetch: adapters.fetch ?? globalThis.fetch,
      monotonicNow: adapters.monotonicNow ?? process.hrtime.bigint,
      wallNow,
    };

    // install the verified disposable lifecycle
    if (plan.mode === "disposable-fixture") {
      evaluatorAdapters.prepareAttempt = async ({ attempt: selected }) => {
        const prepared = await adapters.prepareAttempt({
          attempt: selected,
          fixtureAuthority: plan.fixtureAuthority,
        });
        preparedOriginal = prepared;
        try {
          preparedEvidence = validatePreparation(selected, prepared, plan.fixtureAuthority);
        } catch (error) {
          // clean an allocated but rejected preparation
          await adapters.cleanupAttempt({
            attempt: selected,
            fixtureAuthority: plan.fixtureAuthority,
            preparation: prepared,
          });
          preparedOriginal = null;
          throw error;
        }
        return {
          baseUrl: prepared.baseUrl,
          classification: prepared.classification,
        };
      };
      evaluatorAdapters.cleanupAttempt = async ({ attempt: selected }) => {
        requireInvariant(
          preparedOriginal !== null,
          "native-http-client.cleanup",
          "fixture cleanup lost its exact preparation identity",
        );
        await adapters.cleanupAttempt({
          attempt: selected,
          fixtureAuthority: plan.fixtureAuthority,
          preparation: preparedOriginal,
        });
        preparedOriginal = null;
      };
    }

    let result;
    try {
      result = await evaluateBackendPerformanceDirect({
        ...plan.binding.directConfiguration,
        attempts: [attempt],
      }, evaluatorAdapters);
    } catch (error) {
      stopReason = `evaluator failure: ${error instanceof Error ? error.message : String(error)}`;
      break;
    }
    const entry = result.attempts[0];
    results.push(entry);
    preparationEvidence.push(preparedEvidence);
    // publish one terminal attempt to the controller
    if (adapters.onAttempt !== undefined) {
      await adapters.onAttempt({
        attempt: entry,
        attempted: results.length,
        preparationEvidence: preparedEvidence,
        total: plan.attempts.length,
      });
    }
    const ordinaryHttpError = entry.terminalCategory === "error"
      && entry.errorCategory === "http"
      && Number.isInteger(entry.httpStatus);
    // stop immediately on every non-http incomplete response
    if (entry.terminalCategory !== "completed" && !ordinaryHttpError) {
      stopReason = `first non-http incomplete response: ${entry.attemptId}`;
      break;
    }
    // retain the exact inter-attempt pacing
    if (index + 1 < plan.attempts.length) {
      await sleep(PACING_MS);
    }
  }
  const completedAt = wallNow();
  requireInvariant(
    completedAt instanceof Date && Number.isFinite(completedAt.getTime()),
    "native-http-client.clock",
    "client completion time is invalid",
  );
  const terminalCounts = { completed: 0, error: 0, timeout: 0 };
  // retain every observed terminal row
  for (const result of results) {
    terminalCounts[result.terminalCategory] += 1;
  }
  const status = stopReason === null && results.length === plan.attempts.length
    ? "complete"
    : "stopped";
  const receipts = operationReceipts(results, preparationEvidence, status);
  return Object.freeze({
    attempts: Object.freeze(results.map(Object.freeze)),
    binding: plan.binding,
    completedAt: completedAt.toISOString(),
    comparison: plan.comparison,
    contractVersion: BACKEND_PERFORMANCE_DIRECT_NATIVE_HTTP_CLIENT_VERSION,
    nativeEligibility: "requires-external-proof-adjudication",
    operationReceipts: Object.freeze(receipts),
    policy: plan.policy,
    preparationEvidence: Object.freeze(preparationEvidence.map(
      // freeze present evidence summaries only
      (evidence) => evidence === null ? null : Object.freeze(evidence),
    )),
    sourceAttemptIds: Object.freeze(plan.attempts.map(
      // retain original source identities
      (attempt) => attempt.attemptId,
    )),
    startedAt: startedAt.toISOString(),
    status,
    stopReason,
    terminalCounts: Object.freeze(terminalCounts),
  });
}

// run only the fixed public plan from the command line
async function runCli(argumentsList) {
  requireInvariant(
    argumentsList.length === 3
      && argumentsList[0] === "warm-public"
      && argumentsList[1] === "baseline",
    "native-http-client.cli",
    "usage: backend-performance-direct-native-http-client.mjs warm-public baseline <run-id>",
  );
  const [, role, runId] = argumentsList;
  const result = await runBackendPerformanceDirectNativeHttpClient({
    attemptIds: null,
    fixtureAuthority: null,
    generation: "node-v1",
    mode: "warm-public",
    role,
    runId,
  }, {
    // emit one durable source-bound row as it completes
    async onAttempt(row) {
      process.stdout.write(`${JSON.stringify({ kind: "attempt", ...row })}\n`);
    },
  });
  process.stdout.write(`${JSON.stringify({ kind: "finished", result })}\n`);
  // preserve incomplete execution in the process status
  if (result.status !== "complete") {
    process.exitCode = 1;
  }
}

// execute only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runCli(process.argv.slice(2)).catch(
    // emit one controlled terminal failure
    (error) => {
      process.stderr.write(`${error?.code ?? "native-http-client.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    },
  );
}
