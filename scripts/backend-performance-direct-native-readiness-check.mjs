import { pathToFileURL } from "node:url";

import {
  assessBackendPerformanceDirectNativeReadiness,
} from "./backend-performance-direct-native-readiness.mjs";
import {
  runBackendPerformanceDirectAdminReadCheck,
} from "./backend-performance-direct-admin-read-check.mjs";
import {
  runBackendPerformanceDirectWeatherApiColdCheck,
} from "./backend-performance-direct-weather-api-cold-check.mjs";
import {
  runBackendPerformanceDirectWeatherModelCheck,
} from "./backend-performance-direct-weather-model-check.mjs";
import {
  runBackendPerformanceDirectWorkerIngestionCheck,
} from "./backend-performance-direct-worker-ingestion-check.mjs";

// run the implemented local operation receipts through native readiness
export async function runBackendPerformanceDirectNativeReadinessCheck() {
  const [admin, cold, model, worker] = await Promise.all([
    runBackendPerformanceDirectAdminReadCheck(),
    runBackendPerformanceDirectWeatherApiColdCheck(),
    runBackendPerformanceDirectWeatherModelCheck(),
    runBackendPerformanceDirectWorkerIngestionCheck(),
  ]);
  return assessBackendPerformanceDirectNativeReadiness({
    liveStatus: null,
    operationReceipts: [admin, cold, model, worker],
    resourceIntervals: [],
  });
}

// print only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runBackendPerformanceDirectNativeReadinessCheck().then(
    // emit one machine-readable readiness result
    (result) => process.stdout.write(`${JSON.stringify(result, null, 2)}\n`),
    // emit one controlled readiness failure
    (error) => {
      process.stderr.write(`${error?.code ?? "direct-native-readiness.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    },
  );
}
