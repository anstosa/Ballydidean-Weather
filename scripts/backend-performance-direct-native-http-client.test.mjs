import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_DIRECT_FIXTURE_AUTHORITY_VERSION,
  BACKEND_PERFORMANCE_DIRECT_FIXTURE_LIFECYCLE_VERSION,
  BACKEND_PERFORMANCE_DIRECT_PHYSICAL_PROOF_VERSION,
  buildBackendPerformanceDirectNativeHttpPlan,
  runBackendPerformanceDirectNativeHttpClient,
} from "./backend-performance-direct-native-http-client.mjs";
import { buildBackendPerformanceFunctionalSchedule } from
  "./backend-performance-functional-schedule.mjs";
import { receiptSha256 } from "./backend-performance-receipts.mjs";

// collect exact source attempts for one stratum
function attemptsFor(stratum) {
  return buildBackendPerformanceFunctionalSchedule().runSlots.flatMap(
    // retain run-slot order
    (runSlot) => runSlot.groups.flatMap(
      // retain group order
      (group) => group.attempts.filter(
        // select one requested stratum
        (attempt) => attempt.stratum === stratum,
      ),
    ),
  );
}

// listen on one disposable loopback port
function listen(server) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
}

// close one disposable loopback server
function close(server) {
  return new Promise((resolve, reject) => {
    server.close((error) => {
      // reject one close error
      if (error !== undefined) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}

// derive one exact loopback origin
function serverOrigin(server) {
  const address = server.address();
  assert.notEqual(address, null);
  assert.equal(typeof address, "object");
  return `http://127.0.0.1:${String(address.port)}/`;
}

// create one fixed fixture authority
function authority(origin) {
  return {
    contractVersion: BACKEND_PERFORMANCE_DIRECT_FIXTURE_AUTHORITY_VERSION,
    fixtureId: "rust-rewrite-0123456789abcdef",
    origin,
    sourceReceiptSha256: "a".repeat(64),
  };
}

// create one exact preparation proof
function preparation(attempt, fixtureAuthority, overrides = {}) {
  const sourceProof = overrides.sourceProof ?? {
    contractVersion: "weather-test-application-cache-cold-proof/v1",
    observedEmpty: true,
  };
  return {
    baseUrl: fixtureAuthority.origin,
    classification: "fresh-disposable-application-cache",
    lifecycle: {
      attemptId: attempt.attemptId,
      cleanupIdentitySha256: "b".repeat(64),
      contractVersion: BACKEND_PERFORMANCE_DIRECT_FIXTURE_LIFECYCLE_VERSION,
      fixtureAuthoritySha256: receiptSha256(fixtureAuthority),
      processIdentitySha256: "c".repeat(64),
      readyAt: "2026-11-27T00:00:02.000Z",
      startedAt: "2026-11-27T00:00:01.000Z",
    },
    physicalProof: {
      attemptId: attempt.attemptId,
      completedAt: "2026-11-27T00:00:00.000Z",
      contractVersion: BACKEND_PERFORMANCE_DIRECT_PHYSICAL_PROOF_VERSION,
      sourceContractVersion: sourceProof.contractVersion,
      sourceProof,
      sourceProofSha256: overrides.sourceProofSha256 ?? receiptSha256(sourceProof),
    },
  };
}

// build one sanitized real v1 dual-layer proof shape
function dLoopProof(fixtureAuthority) {
  const root = `/var/lib/weather-native-benchmark/${fixtureAuthority.fixtureId}`;
  const volumeNames = Array.from(
    { length: 5 },
    // retain the actual ordered slot namespace
    (_unused, index) => `weather-native-benchmark-${fixtureAuthority.fixtureId}-slot-${String(index + 1).padStart(2, "0")}`,
  );
  return {
    completedAt: "2026-11-27T00:00:00.000Z",
    contractVersion: "weather-page-d-loop-cold-proof/v1",
    inner: {
      completedAt: "2026-11-26T23:59:59.000Z",
      completedFiles: 1_502,
      fadviseCompleted: true,
      fallbackUsed: false,
      mincoreCompleted: true,
      startedAt: "2026-11-26T23:59:58.000Z",
      totalPages: 3_308_765,
      totalResidentAfter: 0,
    },
    outer: {
      backingFileBytes: 19_864_223_744,
      completedAt: "2026-11-27T00:00:00.000Z",
      completedFiles: 1,
      fadviseCompleted: true,
      fallbackUsed: false,
      mincoreCompleted: true,
      startedAt: "2026-11-26T23:59:59.000Z",
      totalPages: 4_849_664,
      totalResidentAfter: 0,
    },
    startedAt: "2026-11-26T23:59:58.000Z",
    storageIdentity: {
      backingFile: `${root}/fixture.ext4`,
      backingFileBytes: 19_864_223_744,
      loopDevice: "/dev/loop1",
      loopUuid: "e2386e28-d95b-40b5-978f-59d5161a1db3",
      mount: `${root}/mnt`,
      mountSource: "/dev/loop1",
      volumeNames,
    },
    target: {
      fileCount: 1_502,
      inventoryAfterSha256: "e".repeat(64),
      inventoryBeforeSha256: "e".repeat(64),
      pgdataRoot: `${root}/mnt/docker-volumes/${volumeNames[0]}`,
      postmasterPidAbsent: true,
      totalBytes: 13_552_629_161,
      volumeName: volumeNames[0],
    },
    volumes: volumeNames.map(
      // retain one detached record per exact slot
      (name) => ({ name, runningAttachments: [] }),
    ),
  };
}

// create one closed fixture request
function fixtureRequest(attemptIds, fixtureAuthority) {
  return {
    attemptIds,
    fixtureAuthority,
    generation: "node-v1",
    mode: "disposable-fixture",
    role: "baseline",
    runId: "native-http-test",
  };
}

test("builds only the complete fixed-origin warm source schedule", () => {
  const plan = buildBackendPerformanceDirectNativeHttpPlan({
    attemptIds: null,
    fixtureAuthority: null,
    generation: "node-v1",
    mode: "warm-public",
    role: "baseline",
    runId: "warm-public-test",
  });

  assert.equal(plan.origin, "https://weather.ballydidean.farm");
  assert.equal(plan.attempts.length, 5_600);
  assert.equal(new Set(plan.attempts.map(
    // retain every source identity
    (attempt) => attempt.attemptId,
  )).size, 5_600);
  assert.equal(plan.attempts.every(
    // require the approved public execution policy
    (attempt) => attempt.stratum === "warm"
      && attempt.method === "GET"
      && attempt.body === null
      && attempt.timeoutMs === 60_000,
  ), true);
  assert.deepEqual(plan.policy, {
    concurrency: 1,
    maximumDurationMs: 28_800_000,
    noRetries: true,
    pacingMs: 500,
    warmTimeoutMs: 60_000,
  });
  assert.deepEqual(plan.comparison, {
    approvedBaselineSummarySha256:
      "9a2c32c8e6309539051fdd15a6354f543d54c819d771211833f3795d9d438f08",
    approvedObservedPauseBoundaries: [1_840, 2_990, 4_160, 5_203, 5_205, 5_400],
    approvedObservedPauseDurationMs: 252_987,
    approvedPausePolicy:
      "same evaluator process within each segment; checkpoint after every 40 terminal attempts, >=90s, or ordinary HTTP error; pause before the next request on transient client/server/control headroom veto; issue no request during a pause; require two consecutive clear client/server probes separated by 15s; resume the same exact next attempt without pooling or retry",
    approvedProcessBoundaryAfterAttempt: 741,
    comparisonEligible: false,
    limitation:
      "this client does not itself reproduce the approved two-process split or six supervised pause boundaries",
    matchedPolicy:
      "same master/client runtime+affinity, same process boundary, same request/error/resource pause rules in candidate; no earlier canceled cohorts",
    requiredProcessSegments: 2,
  });
});

test("forbids candidate execution before immutable native target custody", async () => {
  let traffic = 0;

  await assert.rejects(
    runBackendPerformanceDirectNativeHttpClient({
      attemptIds: null,
      fixtureAuthority: null,
      generation: "rust-v1",
      mode: "warm-public",
      role: "candidate",
      runId: "candidate-before-target",
    }, {
      async fetch() {
        traffic += 1;
        return new Response("unused");
      },
    }),
    { code: "native-http-client.candidate-gate" },
  );
  assert.equal(traffic, 0);
});

test("rejects arbitrary fixture origins and reordered source identities", () => {
  const selected = attemptsFor("application-cache-cold").slice(0, 2);
  const invalidAuthority = authority("http://example.com:3000/");

  assert.throws(
    () => buildBackendPerformanceDirectNativeHttpPlan(fixtureRequest(
      selected.map((attempt) => attempt.attemptId),
      invalidAuthority,
    )),
    { code: "native-http-client.fixture-authority" },
  );
  assert.throws(
    () => buildBackendPerformanceDirectNativeHttpPlan(fixtureRequest(
      selected.toReversed().map((attempt) => attempt.attemptId),
      authority("http://127.0.0.1:3000/"),
    )),
    { code: "native-http-client.source-order" },
  );
});

test("retains ordinary HTTP errors and serial 500ms pacing without retry", async () => {
  let requests = 0;
  const server = createServer((_request, response) => {
    requests += 1;
    // return one ordinary error before one complete body
    if (requests === 1) {
      response.writeHead(503, { "content-type": "application/json" });
      response.end('{"ok":false}');
      return;
    }
    response.writeHead(200, { "content-type": "application/json" });
    response.end('{"ok":true}');
  });
  await listen(server);
  try {
    const selected = attemptsFor("application-cache-cold").slice(0, 2);
    const fixtureAuthority = authority(serverOrigin(server));
    const sleeps = [];
    const prepared = [];
    const cleaned = [];
    const result = await runBackendPerformanceDirectNativeHttpClient(
      fixtureRequest(selected.map((attempt) => attempt.attemptId), fixtureAuthority),
      {
        async cleanupAttempt({ attempt }) {
          cleaned.push(attempt.attemptId);
        },
        async prepareAttempt({ attempt }) {
          prepared.push(attempt.attemptId);
          return preparation(attempt, fixtureAuthority);
        },
        async sleep(milliseconds) {
          sleeps.push(milliseconds);
        },
      },
    );

    assert.equal(result.status, "complete");
    assert.equal(result.stopReason, null);
    assert.deepEqual(result.terminalCounts, { completed: 1, error: 1, timeout: 0 });
    assert.deepEqual(result.attempts.map(
      // retain terminal categories in source order
      (attempt) => attempt.terminalCategory,
    ), ["error", "completed"]);
    assert.deepEqual(prepared, selected.map((attempt) => attempt.attemptId));
    assert.deepEqual(cleaned, prepared);
    assert.deepEqual(sleeps, [500]);
    assert.equal(requests, 2);
    assert.equal(result.nativeEligibility, "requires-external-proof-adjudication");
    assert.equal(result.comparison.comparisonEligible, false);
    assert.equal(result.comparison.approvedProcessBoundaryAfterAttempt, 741);
    assert.equal(result.operationReceipts.length, 2);
    assert.equal(result.operationReceipts.every(
      // preserve receipt coverage without granting native eligibility
      (receipt) => receipt.nativeEligible === false
        && receipt.evidenceClass === "physical-http-unadjudicated"
        && receipt.attemptDenominator.observedAttempts === 1,
    ), true);
    assert.equal(result.preparationEvidence.every(
      // require hash-only preparation custody
      (evidence) => Object.values(evidence).every(
        // retain only exact sha256 identities
        (value) => /^[a-f0-9]{64}$/u.test(value),
      ),
    ), true);
  } finally {
    await close(server);
  }
});

test("stops immediately on one semantically incomplete body", async () => {
  let requests = 0;
  const server = createServer((_request, response) => {
    requests += 1;
    response.writeHead(200, { "content-type": "application/json" });
    response.end("not-json");
  });
  await listen(server);
  try {
    const selected = attemptsFor("application-cache-cold").slice(0, 2);
    const fixtureAuthority = authority(serverOrigin(server));
    const sleeps = [];
    const result = await runBackendPerformanceDirectNativeHttpClient(
      fixtureRequest(selected.map((attempt) => attempt.attemptId), fixtureAuthority),
      {
        async cleanupAttempt() {},
        async prepareAttempt({ attempt }) {
          return preparation(attempt, fixtureAuthority);
        },
        async sleep(milliseconds) {
          sleeps.push(milliseconds);
        },
      },
    );

    assert.equal(result.status, "stopped");
    assert.match(result.stopReason, /^first non-http incomplete response:/u);
    assert.equal(result.attempts.length, 1);
    assert.equal(result.attempts[0].errorCategory, "observation");
    assert.deepEqual(result.operationReceipts, []);
    assert.equal(requests, 1);
    assert.deepEqual(sleeps, []);
  } finally {
    await close(server);
  }
});

test("cleans and stops before traffic when physical proof is not exact", async () => {
  let traffic = 0;
  let cleanups = 0;
  const selected = attemptsFor("application-cache-cold").slice(0, 1);
  const fixtureAuthority = authority("http://127.0.0.1:3000/");
  const result = await runBackendPerformanceDirectNativeHttpClient(
    fixtureRequest(selected.map((attempt) => attempt.attemptId), fixtureAuthority),
    {
      async cleanupAttempt() {
        cleanups += 1;
      },
      async fetch() {
        traffic += 1;
        return new Response('{"ok":true}', {
          headers: { "content-type": "application/json" },
          status: 200,
        });
      },
      async prepareAttempt({ attempt }) {
        return preparation(attempt, fixtureAuthority, {
          sourceProofSha256: "d".repeat(64),
        });
      },
    },
  );

  assert.equal(result.status, "stopped");
  assert.equal(result.attempts.length, 1);
  assert.equal(result.attempts[0].errorCategory, "process");
  assert.equal(traffic, 0);
  assert.equal(cleanups, 1);
});

test("rejects db-os-cold without the real detached dual-layer proof", async () => {
  let traffic = 0;
  let cleanups = 0;
  const selected = attemptsFor("db-os-cold").slice(0, 1);
  const fixtureAuthority = authority("http://127.0.0.1:3000/");
  const result = await runBackendPerformanceDirectNativeHttpClient(
    fixtureRequest(selected.map((attempt) => attempt.attemptId), fixtureAuthority),
    {
      async cleanupAttempt() {
        cleanups += 1;
      },
      async fetch() {
        traffic += 1;
        return new Response("unused");
      },
      async prepareAttempt({ attempt }) {
        return preparation(attempt, fixtureAuthority);
      },
    },
  );

  assert.equal(result.status, "stopped");
  assert.equal(result.attempts[0].errorCategory, "process");
  assert.equal(traffic, 0);
  assert.equal(cleanups, 1);
});

test("accepts the exact sanitized real v1 D-loop proof shape", async () => {
  let traffic = 0;
  let cleanups = 0;
  const server = createServer((_request, response) => {
    traffic += 1;
    response.writeHead(200, { "content-type": "text/html" });
    response.end("<!doctype html><html><body>ok</body></html>");
  });
  await listen(server);
  try {
    const selected = attemptsFor("db-os-cold").slice(0, 1);
    const fixtureAuthority = authority(serverOrigin(server));
    const sourceProof = dLoopProof(fixtureAuthority);
    const result = await runBackendPerformanceDirectNativeHttpClient(
      fixtureRequest(selected.map((attempt) => attempt.attemptId), fixtureAuthority),
      {
        async cleanupAttempt() {
          cleanups += 1;
        },
        async prepareAttempt({ attempt }) {
          return preparation(attempt, fixtureAuthority, { sourceProof });
        },
      },
    );

    assert.equal(result.status, "complete");
    assert.equal(result.attempts[0].terminalCategory, "completed");
    assert.equal(result.preparationEvidence[0].sourceProofSha256, receiptSha256(sourceProof));
    assert.equal(result.operationReceipts[0].nativeEligible, false);
    assert.equal(result.nativeEligibility, "requires-external-proof-adjudication");
    assert.equal(traffic, 1);
    assert.equal(cleanups, 1);
  } finally {
    await close(server);
  }
});
