import { pathToFileURL } from "node:url";

import {
  reconcileBackendPerformanceDirectCoverage,
} from "./backend-performance-direct-coverage.mjs";
import {
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";

// run one deterministic partial offline coverage reconciliation
export function runBackendPerformanceDirectCoverageCheck() {
  const schedule = buildBackendPerformanceFunctionalSchedule();
  const sourceAttemptIds = schedule.runSlots.flatMap(
    // retain every functional run slot
    (runSlot) => runSlot.groups.flatMap(
      // retain every functional group
      (group) => group.attempts.filter(
        // select the reviewed health warm bucket
        (attempt) => attempt.operationId === "api.health" && attempt.stratum === "warm",
      ).slice(0, 8).map(
        // retain a compact partial fixture per run slot
        (attempt) => attempt.attemptId,
      ),
    ),
  );
  return reconcileBackendPerformanceDirectCoverage([{
    attemptDenominator: {
      observedAttempts: sourceAttemptIds.length,
      plannedAttempts: sourceAttemptIds.length,
    },
    evidenceClass: "synthetic-fixture",
    nativeEligible: false,
    operationStratum: {
      observedAttempts: sourceAttemptIds.length,
      operationId: "api.health",
      stratum: "warm",
    },
    sourceBinding: { sourceAttemptIds },
    terminalCounts: { completed: sourceAttemptIds.length, error: 0, timeout: 0 },
  }]);
}

// print only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.stdout.write(`${JSON.stringify(runBackendPerformanceDirectCoverageCheck(), null, 2)}\n`);
  } catch (error) {
    process.stderr.write(`${error?.code ?? "direct-coverage.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}
