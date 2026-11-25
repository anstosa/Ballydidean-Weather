import { pathToFileURL } from "node:url";

import {
  BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_INPUT_VERSION,
  evaluateBackendPerformanceDirectHostAccounting,
} from "./backend-performance-direct-host-accounting.mjs";

const input = {
  clockId: "deterministic-fixture-monotonic-clock",
  contractVersion: BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_INPUT_VERSION,
  snapshots: [
    {
      capturedAt: "2026-11-23T20:00:00.000Z",
      meminfo: "MemTotal: 8000 kB\nMemFree: 2000 kB\nMemAvailable: 3000 kB\nSwapTotal: 1000 kB\nSwapFree: 800 kB\n",
      monotonicMilliseconds: 1_000,
      procStat: "cpu 100 10 50 1000 20 5 15 2 4 1\ncpu0 1 0 1 1 0 0 0 0 0 0\n",
    },
    {
      capturedAt: "2026-11-23T20:00:01.000Z",
      meminfo: "MemTotal: 8000 kB\nMemFree: 1500 kB\nMemAvailable: 2500 kB\nSwapTotal: 1000 kB\nSwapFree: 700 kB\n",
      monotonicMilliseconds: 2_000,
      procStat: "cpu 130 12 65 1030 25 7 18 3 6 1\ncpu0 1 0 1 1 0 0 0 0 0 0\n",
    },
  ],
};

// run one deterministic offline host-accounting fixture
export function runBackendPerformanceDirectHostAccountingCheck() {
  return evaluateBackendPerformanceDirectHostAccounting(input);
}

// print only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.stdout.write(`${JSON.stringify(runBackendPerformanceDirectHostAccountingCheck(), null, 2)}\n`);
  } catch (error) {
    process.stderr.write(`${error?.code ?? "direct-host-accounting.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}
