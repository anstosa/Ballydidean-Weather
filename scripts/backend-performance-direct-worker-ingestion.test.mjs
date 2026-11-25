import assert from "node:assert/strict";
import test from "node:test";

import { runBackendPerformanceDirectWorkerIngestionCheck } from
  "./backend-performance-direct-worker-ingestion-check.mjs";

test("executes reviewed current-provider attempts through compiled runWorkerIteration", async () => {
  const result = await runBackendPerformanceDirectWorkerIngestionCheck();

  assert.deepEqual(result.attemptDenominator, {
    observedAttempts: 5,
    plannedAttempts: 5,
  });
  assert.deepEqual(result.terminalCounts, {
    completed: 5,
    error: 0,
    timeout: 0,
  });
  assert.equal(result.operationStratum.operationId, "provider.open-meteo-current.success");
  assert.equal(result.operationStratum.stratum, "once-per-run");
  assert.equal(result.operationStratum.observedAttempts, 5);
  assert.equal(result.operationStratum.latencyNanoseconds.maximum > 0, true);
  assert.deepEqual(result.semanticClassifications, [
    "compiled-worker-current-ingestion-causal-subset-complete",
  ]);
  assert.deepEqual(result.execution, {
    classification: "compiled-runWorkerIteration-current-ingestion-causal-subset",
    completionCalls: 5,
    heartbeatCalls: 5,
    providerCalls: 5,
    recordsHandedOff: 5,
  });
  assert.equal(result.completionHandoffs.length, 5);
  assert.equal(result.completionHandoffs.every(
    // require one checkpointed record handoff per run slot
    (handoff, index) => handoff.attempts === 1
      && handoff.expectedCheckpointVersion === 7
      && handoff.lastValidAt === "2026-08-22T05:19:00.000Z"
      && handoff.recordCount === 1
      && handoff.runId === `direct-worker-run-slot-0${String(index + 1)}`
      && handoff.runSlotId === `run-slot-0${String(index + 1)}`
      && handoff.windowStart === "2026-08-22T05:00:00.000Z"
      && handoff.windowEndExclusive === "2026-08-22T05:15:00.000Z",
  ), true);
  assert.deepEqual(result.sourceBinding.nodeArgv, [{
    phase: "execute",
    argv: ["node", "apps/worker/dist/worker.js", "--once"],
  }]);
  assert.equal(result.sourceBinding.sourceAttemptIds.length, 5);
  assert.equal(result.sourceBinding.sourceAttemptIds.every(
    // require one exact reviewed source identity per run slot
    (attemptId, index) => attemptId
      === `planned-functional-v4-r2:run-slot-0${String(index + 1)}:1198:provider.open-meteo-current.success:once-per-run`,
  ), true);
  assert.deepEqual(result.resourceEvidence, {
    contractVersion: "weather-backend-disposable-process-resource/v1",
    kind: "disposable-process-snapshot-not-cgroup",
    nativeEligible: false,
    sampleCount: 7,
  });
  assert.equal(result.unsupportedBoundaries.length, 3);
  assert.equal(result.evidenceClass, "synthetic-fixture");
  assert.equal(result.nativeEligible, false);
});
