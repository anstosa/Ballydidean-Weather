import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import {
  BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
} from "./backend-performance-direct-evaluator.mjs";
import {
  BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
  BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";

const WORKLOAD_URL = new URL("../config/backend-performance-workload-v2.json", import.meta.url);
const WORKLOAD_SHA256 = "b44eb296f1145a55202e941e24e750c0b0d5a4c37075c6f8e9ca346470bca53f";
const MAXIMUM_SELECTED_ATTEMPTS = 6_275;

export const BACKEND_PERFORMANCE_DIRECT_BINDING_VERSION =
  "weather-backend-performance-direct-binding/v1";
export const BACKEND_PERFORMANCE_DIRECT_BINDING_PAIR_VERSION =
  "weather-backend-performance-direct-binding-pair/v1";

// create one coded binding error
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one binding invariant
function requireInvariant(condition, code, message) {
  // fail closed on binding drift
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

// serialize one closed binding value
function canonicalJson(value) {
  // retain null directly
  if (value === null) {
    return "null";
  }
  // retain declared array order
  if (Array.isArray(value)) {
    return `[${value.map(
      // encode one array member
      (member) => canonicalJson(member),
    ).join(",")}]`;
  }
  // retain records in sorted order
  if (isRecord(value)) {
    return `{${Object.keys(value).sort().map(
      // encode one record member
      (key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`,
    ).join(",")}}`;
  }
  return JSON.stringify(value);
}

// hash one closed binding value
function digestValue(value) {
  return createHash("sha256").update(canonicalJson(value)).digest("hex");
}

// freeze one complete binding
function deepFreeze(value) {
  // freeze nested binding values first
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    // visit every binding child
    for (const child of Object.values(value)) {
      deepFreeze(child);
    }
    Object.freeze(value);
  }
  return value;
}

const workloadBytes = readFileSync(WORKLOAD_URL);
requireInvariant(
  createHash("sha256").update(workloadBytes).digest("hex") === WORKLOAD_SHA256,
  "direct-binding.workload-pin",
  "backend performance workload identity changed",
);
const workload = JSON.parse(workloadBytes.toString("utf8"));
const operationById = new Map(workload.operations.map(
  // index one source operation
  (operation) => [operation.id, operation],
));
const schedule = buildBackendPerformanceFunctionalSchedule();
const oracleByOperationId = new Map(schedule.functionalOracles.operations.map(
  // index one projected ordinary oracle
  (operation) => [operation.operationId, operation],
));
const attemptById = new Map();
// index the complete source schedule once
for (const runSlot of schedule.runSlots) {
  // visit every source group
  for (const group of runSlot.groups) {
    // retain every planned attempt
    for (const attempt of group.attempts) {
      attemptById.set(attempt.attemptId, attempt);
    }
  }
}
requireInvariant(
  attemptById.size === schedule.totals.plannedAttempts,
  "direct-binding.schedule",
  "functional schedule attempt identities are incomplete",
);

// require one exact binding input shape
function validateBindingInput(value) {
  requireInvariant(isRecord(value), "direct-binding.schema", "binding input must be a record");
  const expectedKeys = [
    "attemptIds",
    "baseUrl",
    "generation",
    "maximumResponseBytes",
    "resourceInterval",
    "role",
    "runId",
  ];
  requireInvariant(
    canonicalJson(Object.keys(value).sort()) === canonicalJson(expectedKeys.sort()),
    "direct-binding.schema",
    "binding input has unexpected fields",
  );
  requireInvariant(
    Array.isArray(value.attemptIds) && value.attemptIds.length > 0
      && value.attemptIds.length <= MAXIMUM_SELECTED_ATTEMPTS
      && value.attemptIds.every(
        // retain only named source attempts
        (attemptId) => typeof attemptId === "string" && attemptId.length > 0,
      ) && new Set(value.attemptIds).size === value.attemptIds.length,
    "direct-binding.schema",
    "binding attempt identities are invalid",
  );
  requireInvariant(
    value.role === "baseline" || value.role === "candidate",
    "direct-binding.schema",
    "binding role is invalid",
  );
}

// derive one exact expected status
function expectedStatuses(oracle, operationId) {
  const values = oracle.projectedTerminalOracle.filter(
    // retain only literal status oracles
    (value) => /^status=[1-5][0-9]{2}$/.test(value),
  );
  requireInvariant(
    values.length === 1,
    "direct-binding.oracle",
    `${operationId} does not have one literal ordinary status`,
  );
  return [Number.parseInt(values[0].slice("status=".length), 10)];
}

// bind one source attempt to direct http execution
function bindAttempt(attemptId) {
  const attempt = attemptById.get(attemptId);
  requireInvariant(
    attempt !== undefined,
    "direct-binding.attempt",
    `unknown functional schedule attempt: ${attemptId}`,
  );
  const operation = operationById.get(attempt.operationId);
  const oracle = oracleByOperationId.get(attempt.operationId);
  requireInvariant(
    operation !== undefined && oracle !== undefined
      && operation.stateEffects === "read-only" && oracle.stateEffects === "read-only"
      && isRecord(operation.exactHttp) && operation.nodeArgv === null,
    "direct-binding.operation",
    `${attempt.operationId} is not an ordinary read-only HTTP operation`,
  );
  const http = operation.exactHttp;
  requireInvariant(
    http.body === null && isRecord(http.headers)
      && !canonicalJson(http).includes("${"),
    "direct-binding.operation",
    `${attempt.operationId} requires unresolved or non-read-only request material`,
  );
  return {
    attemptId: attempt.attemptId,
    body: null,
    expectedStatuses: expectedStatuses(oracle, attempt.operationId),
    headers: { ...http.headers },
    method: http.method,
    operationId: attempt.operationId,
    path: http.target,
    runSlotId: attempt.runSlotId,
    stratum: attempt.stratum,
    timeoutMs: attempt.deadlineMs,
  };
}

// bind selected source attempts to one direct origin
export function bindBackendPerformanceDirectSchedule(input) {
  requireInvariant(
    arguments.length === 1,
    "direct-binding.arguments",
    "direct schedule binding accepts exactly one input",
  );
  validateBindingInput(input);
  const attempts = input.attemptIds.map(
    // bind one selected source attempt
    (attemptId) => bindAttempt(attemptId),
  );
  const workloadIdentity = {
    attempts,
    functionalDefinitionSha256: BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
    functionalScheduleVersion: BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
    maximumResponseBytes: input.maximumResponseBytes,
    workloadSha256: WORKLOAD_SHA256,
  };
  return deepFreeze({
    contractVersion: BACKEND_PERFORMANCE_DIRECT_BINDING_VERSION,
    directConfiguration: {
      attempts,
      baseUrl: input.baseUrl,
      contractVersion: BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
      generation: input.generation,
      maximumResponseBytes: input.maximumResponseBytes,
      resourceInterval: input.resourceInterval,
      role: input.role,
      runId: input.runId,
    },
    executionClass: "caller-supplied-origin-read-only-http",
    role: input.role,
    workloadIdentity: {
      algorithm: "SHA-256",
      value: digestValue(workloadIdentity),
    },
    workloadSource: {
      functionalDefinitionSha256: BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
      functionalScheduleVersion: BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
      selectedAttemptCount: attempts.length,
      workloadSha256: WORKLOAD_SHA256,
    },
  });
}

// pair two bindings before any candidate traffic
export function pairBackendPerformanceDirectBindings(baseline, candidate) {
  requireInvariant(
    arguments.length === 2 && isRecord(baseline) && isRecord(candidate),
    "direct-binding.arguments",
    "direct binding pair accepts exactly two bindings",
  );
  requireInvariant(
    baseline.contractVersion === BACKEND_PERFORMANCE_DIRECT_BINDING_VERSION
      && candidate.contractVersion === BACKEND_PERFORMANCE_DIRECT_BINDING_VERSION
      && baseline.role === "baseline" && candidate.role === "candidate",
    "direct-binding.pair",
    "direct binding roles or versions are invalid",
  );
  requireInvariant(
    baseline.workloadIdentity.value === candidate.workloadIdentity.value,
    "direct-binding.pair",
    "baseline and candidate workload identities differ",
  );
  return deepFreeze({
    baseline,
    candidate,
    contractVersion: BACKEND_PERFORMANCE_DIRECT_BINDING_PAIR_VERSION,
    executionState: "matched-bindings-only-no-results",
    matchedWorkloadIdentity: baseline.workloadIdentity,
  });
}
