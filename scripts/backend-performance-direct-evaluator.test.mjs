import assert from "node:assert/strict";
import test from "node:test";

import { runBackendPerformanceDirectCheck } from
  "./backend-performance-direct-check.mjs";
import {
  BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
  BACKEND_PERFORMANCE_DIRECT_RESULT_VERSION,
  evaluateBackendPerformanceDirect,
} from "./backend-performance-direct-evaluator.mjs";

// build one valid direct evaluator input
function configuration(overrides = {}) {
  return {
    attempts: [
      {
        attemptId: "attempt-1",
        body: null,
        expectedStatuses: [200],
        headers: { accept: "application/json" },
        method: "GET",
        operationId: "test.request",
        path: "/value",
        runSlotId: "test-slot",
        stratum: "warm",
        timeoutMs: 1_000,
      },
    ],
    baseUrl: "http://127.0.0.1:3000",
    contractVersion: BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
    generation: "node-v1",
    maximumResponseBytes: 1_024,
    resourceInterval: null,
    role: "baseline",
    runId: "test-run",
    ...overrides,
  };
}

// build one bounded fetch response
function response(status, body) {
  return new Response(body, { status });
}

test("counts successful and ordinary failed requests in the full denominator", async () => {
  const result = await runBackendPerformanceDirectCheck();

  assert.equal(result.contractVersion, BACKEND_PERFORMANCE_DIRECT_RESULT_VERSION);
  assert.deepEqual(result.summary.durationCoverage, {
    observedAttempts: 2,
    plannedAttempts: 2,
  });
  assert.deepEqual(result.summary.terminalCounts, {
    completed: 1,
    error: 1,
    timeout: 0,
  });
  assert.deepEqual(result.summary.statusCounts, { 200: 1, 503: 1 });
  assert.equal(result.attempts[0].terminalCategory, "completed");
  assert.equal(result.attempts[1].terminalCategory, "error");
  assert.equal(result.attempts[1].errorCategory, "http");
  assert.equal(result.summary.latencyNanoseconds.p99 >= 0, true);
  assert.equal(Object.isFrozen(result.attempts[0]), true);
});

test("counts ordinary post request and response bytes", async () => {
  let observedRequest = null;
  const input = configuration({
    attempts: [{
      attemptId: "attempt-1",
      body: '{"value":1}',
      expectedStatuses: [200],
      headers: { "content-type": "application/json" },
      method: "POST",
      operationId: "test.request",
      path: "/value",
      runSlotId: "test-slot",
      stratum: "warm",
      timeoutMs: 1_000,
    }],
  });
  const result = await evaluateBackendPerformanceDirect(input, {
    fetch: async (url, options) => {
      observedRequest = { body: options.body, method: options.method, url: String(url) };
      return response(200, "result");
    },
    monotonicNow: (() => {
      let value = 0n;
      // advance one deterministic clock
      return () => {
        value += 10n;
        return value;
      };
    })(),
  });

  assert.deepEqual(observedRequest, {
    body: '{"value":1}',
    method: "POST",
    url: "http://127.0.0.1:3000/value",
  });
  assert.equal(result.attempts[0].requestBodyBytes, 11);
  assert.equal(result.attempts[0].responseBodyBytes, 6);
});

test("attaches the existing resource interval lifecycle when configured", async () => {
  const calls = [];
  const resourceEvidence = { contractVersion: "test-resource/v1" };
  const result = await evaluateBackendPerformanceDirect(
    configuration({ resourceInterval: { scopeId: "api" } }),
    {
      beginResourceInterval: async (value) => {
        calls.push(["begin", value]);
        return {
          // record one resource abort
          async abort() {
            calls.push(["abort"]);
          },
          // record one resource finish
          async finish() {
            calls.push(["finish"]);
            return resourceEvidence;
          },
          // record one resource sample
          async sample() {
            calls.push(["sample"]);
          },
        };
      },
      fetch: async () => response(200, "ok"),
      monotonicNow: (() => {
        let value = 0n;
        // advance one deterministic clock
        return () => {
          value += 10n;
          return value;
        };
      })(),
    },
  );

  assert.deepEqual(calls, [
    ["begin", { scopeId: "api" }],
    ["sample"],
    ["finish"],
  ]);
  assert.deepEqual(result.resourceEvidence, resourceEvidence);
});

test("rejects duplicate identities before generating traffic", async () => {
  let traffic = 0;
  const input = configuration();
  input.attempts.push({ ...input.attempts[0] });

  await assert.rejects(
    evaluateBackendPerformanceDirect(input, {
      fetch: async () => {
        traffic += 1;
        return response(200, "ok");
      },
    }),
    { code: "direct-evaluator.schema" },
  );
  assert.equal(traffic, 0);
});
