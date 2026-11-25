import { readFile } from "node:fs/promises";

import {
  createNonOverlappingScheduler,
  nextScheduledAt,
} from "../../../../apps/worker/dist/scheduler.js";

const workerSource = await readFile(
  new URL("../../../../apps/worker/dist/worker.js", import.meta.url),
  "utf8",
);
const startupStart = workerSource.indexOf("export async function startWorkerProcess");
const startupEnd = workerSource.indexOf("// load shadow-only rain state", startupStart);

// require the compiled startup function before checking its order
if (startupStart < 0 || startupEnd < 0) {
  throw new Error("compiled worker startup function is unavailable");
}
const startup = workerSource.slice(startupStart, startupEnd);

// require the named markers in exact source order
function assertOrder(label, markers) {
  let previous = -1;

  // advance monotonically through the compiled startup source
  for (const marker of markers) {
    const current = startup.indexOf(marker, previous + 1);

    // reject absent or reordered source behavior
    if (current < 0 || current <= previous) {
      throw new Error(`${label} marker is absent or reordered: ${marker}`);
    }
    previous = current;
  }
}

assertOrder("startup", [
  "loadWorkerConfiguration()",
  "createDatabasePool(configuration.database)",
  "createForecastAdjustmentTemperatureCanaryRuntimeLoader",
  "assertWorkerDatabaseReadiness",
  "readWorkerHealth",
  "createWorkerIterationRunner",
  "createNonOverlappingScheduler",
]);
assertOrder("once", [
  "if (options.once === true)",
  "await scheduler.trigger()",
  "await pool.end()",
  "return",
]);
assertOrder("shutdown", [
  "const shutdown",
  "scheduler.stop()",
  "await startupBackfill.catch",
  "await pool.end()",
]);
assertOrder("normal", [
  "scheduler.start()",
  "await scheduler.trigger()",
  "startupBackfill = runStartupPublicStationBackfill",
]);
let releaseFirst;
const firstBarrier = new Promise((resolve) => {
  releaseFirst = resolve;
});
let overlappingRuns = 0;
const nonoverlap = createNonOverlappingScheduler({
  key: "worker-a",
  run: async () => {
    overlappingRuns += 1;
    await firstBarrier;
  },
});
const first = nonoverlap.trigger();
await Promise.resolve();
const overlapping = await nonoverlap.trigger();
releaseFirst();
const completed = await first;

let manualErrors = 0;
const manualFailure = new Error("expected manual failure");
const manual = createNonOverlappingScheduler({
  key: "worker-a",
  // only a background timer should consume this callback
  onError: () => {
    manualErrors += 1;
  },
  // retain the exact error for identity comparison
  run: async () => {
    throw manualFailure;
  },
});
const manualRejected = await manual.trigger().then(
  // distinguish success and preserve the original rejection
  () => false,
  (error) => error === manualFailure,
);

const timers = [];
const cleared = [];
let pollErrors = 0;
let pollRuns = 0;
const stableNow = new Date(120_000);
const polling = createNonOverlappingScheduler({
  cadenceMs: 60_000,
  clearTimeout: (timer) => {
    cleared.push(timer);
  },
  key: "worker-a",
  now: () => stableNow,
  onError: () => {
    pollErrors += 1;
  },
  run: async () => {
    pollRuns += 1;
    throw new Error("expected poll failure");
  },
  setTimeout: (callback, milliseconds) => {
    const timer = { callback, milliseconds, sequence: timers.length + 1 };
    timers.push(timer);
    return timer;
  },
});
polling.start();
const expectedDelay = nextScheduledAt(stableNow, "worker-a", 60_000).getTime() - stableNow.getTime();

// execute the first timer and allow its contained failure to rearm polling
timers[0].callback();
await new Promise((resolve) => setImmediate(resolve));
await new Promise((resolve) => setImmediate(resolve));
polling.stop();

console.log("startup-order=config,pool,runtime,readiness,health,runner,scheduler");
console.log("once-order=trigger,close");
console.log("shutdown-order=stop,backfill,close");
console.log("normal-order=start,trigger,backfill");
console.log(`nonoverlap=${overlapping},${completed},${overlappingRuns}`);
console.log(`manual-failure=${manualRejected},${manualErrors},${manual.isRunning()}`);
console.log(
  `poll=${pollRuns},${pollErrors},${timers.length},${timers[0].milliseconds === expectedDelay},${cleared.length}`,
);
console.log(
  `anchored-slots=${[120_000, 250_000, 300_000, -1].map(
    // capture the production worker's fixed wall slots
    (at) => nextScheduledAt(new Date(at), "worker-a", 60_000).getTime(),
  ).join(",")}`,
);
