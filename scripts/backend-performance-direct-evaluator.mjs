import { createHash } from "node:crypto";

export const BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION =
  "weather-backend-performance-direct-input/v1";
export const BACKEND_PERFORMANCE_DIRECT_RESULT_VERSION =
  "weather-backend-performance-direct-result/v1";
export const BACKEND_PERFORMANCE_DIRECT_PROCESS_COLD_PREPARATION =
  "fresh-disposable-os-process-and-loopback-listener";

const ALLOWED_METHODS = new Set(["DELETE", "GET", "HEAD", "PATCH", "POST", "PUT"]);
const ALLOWED_ROLES = new Set(["baseline", "candidate"]);
const MAXIMUM_ATTEMPTS = 100_000;
const MAXIMUM_BODY_BYTES = 16 * 1024 * 1024;
const MAXIMUM_RESPONSE_BYTES = 64 * 1024 * 1024;

// create one coded evaluator error
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one evaluator invariant
function requireInvariant(condition, code, message) {
  // fail closed on malformed input
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

// compare text without locale behavior
function compareText(left, right) {
  // preserve equal strings
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

// serialize one closed json value
function canonicalJson(value) {
  // retain null directly
  if (value === null) {
    return "null";
  }
  // retain ordered arrays
  if (Array.isArray(value)) {
    return `[${value.map(
      // encode one array member
      (member) => canonicalJson(member),
    ).join(",")}]`;
  }
  // retain sorted records
  if (isRecord(value)) {
    return `{${Object.keys(value).sort(compareText).map(
      // encode one record member
      (key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`,
    ).join(",")}}`;
  }
  // retain finite json primitives
  if (typeof value === "string" || typeof value === "boolean"
    || (typeof value === "number" && Number.isFinite(value))) {
    return JSON.stringify(value);
  }
  fail("direct-evaluator.json", "evaluator output contains a non-json value");
}

// freeze one complete result
function deepFreeze(value) {
  // freeze nested values first
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    // visit every result child
    for (const child of Object.values(value)) {
      deepFreeze(child);
    }
    Object.freeze(value);
  }
  return value;
}

// require one exact record shape
function requireExactKeys(value, keys, path) {
  requireInvariant(isRecord(value), "direct-evaluator.schema", `${path} must be a record`);
  const actual = Object.keys(value).sort(compareText);
  const expected = [...keys].sort(compareText);
  requireInvariant(
    canonicalJson(actual) === canonicalJson(expected),
    "direct-evaluator.schema",
    `${path} has unexpected fields`,
  );
}

// validate one request header record
function validateHeaders(value, path) {
  requireInvariant(isRecord(value), "direct-evaluator.schema", `${path} must be a record`);
  const headers = {};
  // retain only explicit string headers
  for (const [name, headerValue] of Object.entries(value)) {
    requireInvariant(
      /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/.test(name) && typeof headerValue === "string",
      "direct-evaluator.schema",
      `${path} contains an invalid header`,
    );
    headers[name] = headerValue;
  }
  return headers;
}

// validate one direct attempt
function validateAttempt(value, index) {
  const path = `$.attempts[${String(index)}]`;
  requireExactKeys(value, [
    "attemptId",
    "body",
    "expectedStatuses",
    "headers",
    "method",
    "operationId",
    "path",
    "runSlotId",
    "stratum",
    "timeoutMs",
  ], path);
  requireInvariant(
    typeof value.attemptId === "string" && value.attemptId.length > 0,
    "direct-evaluator.schema",
    `${path}.attemptId is invalid`,
  );
  requireInvariant(
    typeof value.operationId === "string" && value.operationId.length > 0,
    "direct-evaluator.schema",
    `${path}.operationId is invalid`,
  );
  requireInvariant(
    typeof value.runSlotId === "string" && value.runSlotId.length > 0
      && typeof value.stratum === "string" && value.stratum.length > 0,
    "direct-evaluator.schema",
    `${path} source schedule identity is invalid`,
  );
  requireInvariant(
    typeof value.method === "string" && ALLOWED_METHODS.has(value.method),
    "direct-evaluator.schema",
    `${path}.method is invalid`,
  );
  requireInvariant(
    typeof value.path === "string" && value.path.startsWith("/")
      && !value.path.startsWith("//") && !value.path.includes("#"),
    "direct-evaluator.schema",
    `${path}.path is invalid`,
  );
  requireInvariant(
    Number.isSafeInteger(value.timeoutMs) && value.timeoutMs > 0,
    "direct-evaluator.schema",
    `${path}.timeoutMs is invalid`,
  );
  requireInvariant(
    Array.isArray(value.expectedStatuses) && value.expectedStatuses.length > 0
      && value.expectedStatuses.every(
        // accept only concrete http statuses
        (status) => Number.isSafeInteger(status) && status >= 100 && status <= 599,
      ) && new Set(value.expectedStatuses).size === value.expectedStatuses.length,
    "direct-evaluator.schema",
    `${path}.expectedStatuses is invalid`,
  );
  requireInvariant(
    value.body === null || typeof value.body === "string",
    "direct-evaluator.schema",
    `${path}.body is invalid`,
  );
  const bodyBytes = value.body === null ? 0 : Buffer.byteLength(value.body);
  requireInvariant(
    bodyBytes <= MAXIMUM_BODY_BYTES,
    "direct-evaluator.bounds",
    `${path}.body exceeds the byte limit`,
  );
  return {
    attemptId: value.attemptId,
    body: value.body,
    bodyBytes,
    expectedStatuses: [...value.expectedStatuses],
    headers: validateHeaders(value.headers, `${path}.headers`),
    method: value.method,
    operationId: value.operationId,
    path: value.path,
    runSlotId: value.runSlotId,
    stratum: value.stratum,
    timeoutMs: value.timeoutMs,
  };
}

// validate one direct evaluator input
function validateConfiguration(value) {
  requireExactKeys(value, [
    "attempts",
    "baseUrl",
    "contractVersion",
    "generation",
    "maximumResponseBytes",
    "resourceInterval",
    "role",
    "runId",
  ], "$" );
  requireInvariant(
    value.contractVersion === BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
    "direct-evaluator.version",
    "direct evaluator input version is invalid",
  );
  requireInvariant(
    typeof value.runId === "string" && value.runId.length > 0,
    "direct-evaluator.schema",
    "$.runId is invalid",
  );
  requireInvariant(
    typeof value.generation === "string" && value.generation.length > 0,
    "direct-evaluator.schema",
    "$.generation is invalid",
  );
  requireInvariant(
    typeof value.role === "string" && ALLOWED_ROLES.has(value.role),
    "direct-evaluator.schema",
    "$.role is invalid",
  );
  let baseUrl;
  try {
    baseUrl = new URL(value.baseUrl);
  } catch {
    fail("direct-evaluator.schema", "$.baseUrl is invalid");
  }
  requireInvariant(
    (baseUrl.protocol === "http:" || baseUrl.protocol === "https:")
      && baseUrl.username === "" && baseUrl.password === ""
      && baseUrl.search === "" && baseUrl.hash === "",
    "direct-evaluator.schema",
    "$.baseUrl must be an http origin without credentials query or fragment",
  );
  requireInvariant(
    Array.isArray(value.attempts) && value.attempts.length > 0
      && value.attempts.length <= MAXIMUM_ATTEMPTS,
    "direct-evaluator.bounds",
    "$.attempts has an invalid cardinality",
  );
  const attempts = value.attempts.map(
    // validate every planned attempt
    (attempt, index) => validateAttempt(attempt, index),
  );
  requireInvariant(
    new Set(attempts.map(
      // select one attempt identity
      (attempt) => attempt.attemptId,
    )).size === attempts.length,
    "direct-evaluator.schema",
    "$.attempts contains duplicate attempt identities",
  );
  requireInvariant(
    Number.isSafeInteger(value.maximumResponseBytes)
      && value.maximumResponseBytes > 0
      && value.maximumResponseBytes <= MAXIMUM_RESPONSE_BYTES,
    "direct-evaluator.bounds",
    "$.maximumResponseBytes is invalid",
  );
  requireInvariant(
    value.resourceInterval === null || isRecord(value.resourceInterval),
    "direct-evaluator.schema",
    "$.resourceInterval is invalid",
  );
  return {
    attempts,
    baseUrl,
    generation: value.generation,
    maximumResponseBytes: value.maximumResponseBytes,
    resourceInterval: value.resourceInterval,
    role: value.role,
    runId: value.runId,
  };
}

// read one bounded response body
async function readResponseBytes(response, maximumBytes) {
  // handle responses without a body
  if (response.body === null) {
    return { byteLength: 0, bytes: Buffer.alloc(0) };
  }
  const reader = response.body.getReader();
  const chunks = [];
  let total = 0;
  try {
    // consume each body chunk once
    while (true) {
      const { done, value } = await reader.read();
      // stop at the body terminator
      if (done) {
        return { byteLength: total, bytes: Buffer.concat(chunks, total) };
      }
      total += value.byteLength;
      // reject oversized observations
      if (total > maximumBytes) {
        fail("direct-evaluator.response-limit", "response body exceeds the byte limit");
      }
      chunks.push(Buffer.from(value));
    }
  } finally {
    await reader.cancel().catch(
      // ignore cancellation after terminal read
      () => {},
    );
  }
}

// execute one planned request
async function executeAttempt(configuration, attempt, injected) {
  const controller = new AbortController();
  let timedOut = false;
  const timer = setTimeout(
    // abort one expired request
    () => {
      timedOut = true;
      controller.abort();
    },
    attempt.timeoutMs,
  );
  const started = injected.monotonicNow();
  let finished = null;
  let preparation = null;
  let preparationBaseUrl = null;
  let preparationClassification = "not-required";
  let response = null;
  let responseBytes = 0;
  let semanticClassification = "status-only";
  let terminalCategory = "error";
  let errorCategory = "transport";
  let phase = "preparing";
  try {
    const requiresPreparation = (attempt.stratum !== "warm"
      && attempt.stratum !== "once-per-run")
      || injected.prepareAttempt !== undefined;
    // require a complete lifecycle for cold or explicitly prepared attempts
    if (requiresPreparation) {
      requireInvariant(
        injected.prepareAttempt !== undefined && injected.cleanupAttempt !== undefined,
        "direct-evaluator.preparation",
        `${attempt.stratum} requires explicit preparation and cleanup adapters`,
      );
      preparation = await injected.prepareAttempt({ attempt });
      requireInvariant(
        isRecord(preparation) && typeof preparation.baseUrl === "string"
          && typeof preparation.classification === "string"
          && preparation.classification.length > 0,
        "direct-evaluator.preparation",
        "cold attempt preparation returned an invalid result",
      );
      try {
        preparationBaseUrl = new URL(preparation.baseUrl);
      } catch {
        fail("direct-evaluator.preparation", "cold attempt preparation returned an invalid origin");
      }
      requireInvariant(
        preparationBaseUrl.protocol === "http:"
          && preparationBaseUrl.hostname === "127.0.0.1"
          && preparationBaseUrl.username === ""
          && preparationBaseUrl.password === ""
          && preparationBaseUrl.pathname === "/"
          && preparationBaseUrl.search === ""
          && preparationBaseUrl.hash === "",
        "direct-evaluator.preparation",
        "cold attempt preparation must return a credential-free loopback origin",
      );
      // require the exact process lifecycle witness for process-pool cold
      if (attempt.stratum === "process-pool-cold") {
        requireInvariant(
          preparation.classification
            === BACKEND_PERFORMANCE_DIRECT_PROCESS_COLD_PREPARATION,
          "direct-evaluator.preparation",
          "process-pool-cold requires a fresh disposable process and listener",
        );
      }
      preparationClassification = preparation.classification;
    }
    phase = "requesting";
    const requestBaseUrl = preparation === null
      ? configuration.baseUrl
      : preparationBaseUrl;
    response = await injected.fetch(new URL(attempt.path, requestBaseUrl), {
      body: attempt.body,
      headers: attempt.headers,
      method: attempt.method,
      redirect: "manual",
      signal: controller.signal,
    });
    const body = await readResponseBytes(response, configuration.maximumResponseBytes);
    responseBytes = body.byteLength;
    const expected = attempt.expectedStatuses.includes(response.status);
    terminalCategory = expected ? "completed" : "error";
    errorCategory = expected ? null : "http";
    // apply one operation-specific ordinary response classifier
    if (expected && injected.classifyResponse !== undefined) {
      const classification = await injected.classifyResponse({
        attempt,
        bodyBytes: body.bytes,
        response,
      });
      requireInvariant(
        isRecord(classification) && typeof classification.classification === "string"
          && classification.classification.length > 0
          && typeof classification.completed === "boolean",
        "direct-evaluator.classifier",
        "response classifier returned an invalid result",
      );
      semanticClassification = classification.classification;
      // retain an ordinary semantic failure in the denominator
      if (!classification.completed) {
        terminalCategory = "error";
        errorCategory = "observation";
      }
    }
  } catch (error) {
    // classify the evaluator-owned deadline distinctly
    if (timedOut) {
      terminalCategory = "timeout";
      errorCategory = "deadline-exceeded";
    // retain bounded response failures separately
    } else if (error?.code === "direct-evaluator.response-limit") {
      terminalCategory = "error";
      errorCategory = "observation";
    // classify cold process preparation failures separately
    } else if (phase === "preparing") {
      terminalCategory = "error";
      errorCategory = "process";
    }
  } finally {
    finished = injected.monotonicNow();
    clearTimeout(timer);
    // clean every prepared cold attempt before continuing
    if (preparation !== null) {
      await injected.cleanupAttempt({ attempt, preparation });
    }
  }
  const elapsed = finished - started;
  requireInvariant(
    elapsed >= 0n && elapsed <= BigInt(Number.MAX_SAFE_INTEGER),
    "direct-evaluator.clock",
    "monotonic duration is outside the safe integer range",
  );
  return {
    attemptId: attempt.attemptId,
    elapsedNanoseconds: Number(elapsed),
    errorCategory,
    httpStatus: response?.status ?? null,
    operationId: attempt.operationId,
    preparationClassification,
    requestBodyBytes: attempt.bodyBytes,
    responseBodyBytes: responseBytes,
    runSlotId: attempt.runSlotId,
    stratum: attempt.stratum,
    semanticClassification,
    terminalCategory,
  };
}

// select one nearest-rank duration
function nearestRank(sortedValues, percentile) {
  const rank = Math.ceil((percentile * sortedValues.length) / 100);
  return sortedValues[rank - 1];
}

// summarize the complete attempt denominator
function summarizeAttempts(attempts) {
  const terminalCounts = { completed: 0, error: 0, timeout: 0 };
  const statusCounts = {};
  let requestBodyBytes = 0;
  let responseBodyBytes = 0;
  // include every terminal attempt
  for (const attempt of attempts) {
    terminalCounts[attempt.terminalCategory] += 1;
    requestBodyBytes += attempt.requestBodyBytes;
    responseBodyBytes += attempt.responseBodyBytes;
    // count every observed status
    if (attempt.httpStatus !== null) {
      const key = String(attempt.httpStatus);
      statusCounts[key] = (statusCounts[key] ?? 0) + 1;
    }
  }
  const durations = attempts.map(
    // retain successful and failed durations
    (attempt) => attempt.elapsedNanoseconds,
  ).sort(
    // sort exact safe integer durations
    (left, right) => left - right,
  );
  const summary = {
    bytes: { requestBodyBytes, responseBodyBytes },
    durationCoverage: {
      observedAttempts: durations.length,
      plannedAttempts: attempts.length,
    },
    latencyNanoseconds: {
      maximum: durations.at(-1),
      p50: nearestRank(durations, 50),
      p95: nearestRank(durations, 95),
      p99: nearestRank(durations, 99),
    },
    statusCounts,
    terminalCounts,
  };
  const groups = new Map();
  // retain first source-group appearance order
  for (const attempt of attempts) {
    const key = JSON.stringify([attempt.operationId, attempt.stratum]);
    let group = groups.get(key);
    // initialize one operation-stratum group
    if (group === undefined) {
      group = [];
      groups.set(key, group);
    }
    group.push(attempt);
  }
  summary.operationStrata = [...groups.values()].map(
    // summarize one source operation and stratum
    (group) => {
      const durations = group.map(
        // retain one group duration
        (attempt) => attempt.elapsedNanoseconds,
      ).sort(
        // sort safe integer durations
        (left, right) => left - right,
      );
      const counts = { completed: 0, error: 0, timeout: 0 };
      // count every grouped terminal observation
      for (const attempt of group) {
        counts[attempt.terminalCategory] += 1;
      }
      return {
        latencyNanoseconds: {
          maximum: durations.at(-1),
          p50: nearestRank(durations, 50),
          p95: nearestRank(durations, 95),
          p99: nearestRank(durations, 99),
        },
        observedAttempts: group.length,
        operationId: group[0].operationId,
        stratum: group[0].stratum,
        terminalCounts: counts,
      };
    },
  );
  return summary;
}

// open one optional existing resource interval
async function beginResourceInterval(configuration, injected) {
  // omit resources only when explicitly unconfigured
  if (configuration.resourceInterval === null) {
    return null;
  }
  // prefer a focused test or integration adapter
  if (injected.beginResourceInterval !== undefined) {
    return injected.beginResourceInterval(configuration.resourceInterval);
  }
  const module = await import("./backend-performance-resources.mjs");
  return module.beginBackendResourceInterval(configuration.resourceInterval);
}

// evaluate one direct matched request plan
export async function evaluateBackendPerformanceDirect(configuration, injected = {}) {
  requireInvariant(
    arguments.length >= 1 && arguments.length <= 2 && isRecord(injected),
    "direct-evaluator.arguments",
    "direct evaluator accepts a configuration and optional adapters",
  );
  const validated = validateConfiguration(configuration);
  const adapters = {
    beginResourceInterval: injected.beginResourceInterval,
    classifyResponse: injected.classifyResponse,
    cleanupAttempt: injected.cleanupAttempt,
    fetch: injected.fetch ?? globalThis.fetch,
    monotonicNow: injected.monotonicNow ?? process.hrtime.bigint,
    prepareAttempt: injected.prepareAttempt,
    wallNow: injected.wallNow ?? (() => new Date()),
  };
  requireInvariant(
    typeof adapters.fetch === "function" && typeof adapters.monotonicNow === "function"
      && typeof adapters.wallNow === "function"
      && (adapters.classifyResponse === undefined
        || typeof adapters.classifyResponse === "function")
      && (adapters.prepareAttempt === undefined
        || typeof adapters.prepareAttempt === "function")
      && (adapters.cleanupAttempt === undefined
        || typeof adapters.cleanupAttempt === "function"),
    "direct-evaluator.adapters",
    "direct evaluator adapters are invalid",
  );
  const startedAt = adapters.wallNow();
  requireInvariant(
    startedAt instanceof Date && Number.isFinite(startedAt.getTime()),
    "direct-evaluator.clock",
    "direct evaluator start time is invalid",
  );
  const resourceSession = await beginResourceInterval(validated, adapters);
  const attempts = [];
  let resources = null;
  try {
    // execute every planned request in declared order
    for (const attempt of validated.attempts) {
      attempts.push(await executeAttempt(validated, attempt, adapters));
      // retain an interval sample after each request
      if (resourceSession !== null) {
        await resourceSession.sample();
      }
    }
    // close the optional resource interval
    if (resourceSession !== null) {
      resources = await resourceSession.finish();
    }
  } catch (error) {
    // release an unfinished resource interval
    if (resourceSession !== null) {
      await resourceSession.abort().catch(
        // preserve the original evaluator failure
        () => {},
      );
    }
    throw error;
  }
  const summary = summarizeAttempts(attempts);
  requireInvariant(
    summary.durationCoverage.observedAttempts === summary.durationCoverage.plannedAttempts
      && Object.values(summary.terminalCounts).reduce(
        // add one terminal category
        (sum, count) => sum + count,
        0,
      ) === validated.attempts.length,
    "direct-evaluator.denominator",
    "direct evaluator did not close the full denominator",
  );
  const completedAt = adapters.wallNow();
  requireInvariant(
    completedAt instanceof Date && Number.isFinite(completedAt.getTime())
      && completedAt.getTime() >= startedAt.getTime(),
    "direct-evaluator.clock",
    "direct evaluator completion time is invalid",
  );
  const result = {
    attempts,
    completedAt: completedAt.toISOString(),
    contractVersion: BACKEND_PERFORMANCE_DIRECT_RESULT_VERSION,
    generation: validated.generation,
    inputIdentity: {
      algorithm: "SHA-256",
      value: createHash("sha256").update(canonicalJson(configuration)).digest("hex"),
    },
    resourceEvidence: resources,
    role: validated.role,
    runId: validated.runId,
    startedAt: startedAt.toISOString(),
    summary,
  };
  return deepFreeze(result);
}
