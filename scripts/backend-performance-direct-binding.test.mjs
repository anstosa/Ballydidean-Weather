import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_DIRECT_BINDING_PAIR_VERSION,
  bindBackendPerformanceDirectSchedule,
  pairBackendPerformanceDirectBindings,
} from "./backend-performance-direct-binding.mjs";
import { evaluateBackendPerformanceDirect } from
  "./backend-performance-direct-evaluator.mjs";
import { buildBackendPerformanceFunctionalSchedule } from
  "./backend-performance-functional-schedule.mjs";

// flatten the fixed functional attempt order
function functionalAttempts() {
  return buildBackendPerformanceFunctionalSchedule().runSlots.flatMap(
    // retain every run-slot group
    (runSlot) => runSlot.groups.flatMap(
      // retain every group attempt
      (group) => group.attempts,
    ),
  );
}

// select one warm attempt for each fixture operation
function selectedAttemptIds() {
  const selected = new Map();
  // retain the first matching source attempt
  for (const attempt of functionalAttempts()) {
    // select only the fixture operations
    if (attempt.stratum === "warm"
      && (attempt.operationId === "page.home" || attempt.operationId === "api.health")
      && !selected.has(attempt.operationId)) {
      selected.set(attempt.operationId, attempt.attemptId);
    }
  }
  return [selected.get("page.home"), selected.get("api.health")];
}

// listen on one disposable loopback port
function listen(server) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      const address = server.address();
      // require one assigned tcp address
      if (address === null || typeof address === "string") {
        reject(new Error("fixture listener has no tcp address"));
        return;
      }
      resolve(`http://127.0.0.1:${String(address.port)}`);
    });
  });
}

// close one disposable server
function close(server) {
  return new Promise((resolve, reject) => {
    server.close((error) => {
      // reject one close failure
      if (error !== undefined) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}

// build one disposable ordinary server
function fixtureServer(label) {
  return createServer((request, response) => {
    // return both selected functional paths
    if (request.url === "/" || request.url === "/api/v1/health") {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ label, ok: true }));
      return;
    }
    response.writeHead(404);
    response.end();
  });
}

// bind one fixture role without generating traffic
function bind(role, generation, runId, baseUrl) {
  return bindBackendPerformanceDirectSchedule({
    attemptIds: selectedAttemptIds(),
    baseUrl,
    generation,
    maximumResponseBytes: 1_024,
    resourceInterval: null,
    role,
    runId,
  });
}

test("binds and runs a matched disposable schedule without candidate evidence", async () => {
  const baselineServer = fixtureServer("baseline-fixture");
  const candidateServer = fixtureServer("candidate-fixture");
  const baselineUrl = await listen(baselineServer);
  const candidateUrl = await listen(candidateServer);
  try {
    const baseline = bind("baseline", "fixture-node", "fixture-baseline", baselineUrl);
    const candidate = bind("candidate", "fixture-rust", "fixture-candidate", candidateUrl);
    const pair = pairBackendPerformanceDirectBindings(baseline, candidate);

    assert.equal(pair.contractVersion, BACKEND_PERFORMANCE_DIRECT_BINDING_PAIR_VERSION);
    assert.equal(pair.executionState, "matched-bindings-only-no-results");
    assert.equal(pair.baseline.workloadSource.selectedAttemptCount, 2);
    assert.equal(pair.baseline.workloadIdentity.value, pair.candidate.workloadIdentity.value);
    assert.deepEqual(
      pair.baseline.directConfiguration.attempts,
      pair.candidate.directConfiguration.attempts,
    );

    const baselineResult = await evaluateBackendPerformanceDirect(
      pair.baseline.directConfiguration,
    );
    const candidateResult = await evaluateBackendPerformanceDirect(
      pair.candidate.directConfiguration,
    );
    assert.deepEqual(baselineResult.summary.terminalCounts, {
      completed: 2,
      error: 0,
      timeout: 0,
    });
    assert.deepEqual(candidateResult.summary.terminalCounts, {
      completed: 2,
      error: 0,
      timeout: 0,
    });
    assert.equal(baselineResult.resourceEvidence, null);
    assert.equal(candidateResult.resourceEvidence, null);
  } finally {
    await Promise.all([close(baselineServer), close(candidateServer)]);
  }
});

test("rejects a writable source operation before traffic binding", () => {
  const writable = functionalAttempts().find(
    // select one source-owned writable attempt
    (attempt) => attempt.operationId === "admin.disposable.settings-write",
  );
  assert.ok(writable);
  assert.throws(
    () => bindBackendPerformanceDirectSchedule({
      attemptIds: [writable.attemptId],
      baseUrl: "http://127.0.0.1:3000",
      generation: "fixture-node",
      maximumResponseBytes: 1_024,
      resourceInterval: null,
      role: "baseline",
      runId: "fixture-write-rejection",
    }),
    { code: "direct-binding.operation" },
  );
});
