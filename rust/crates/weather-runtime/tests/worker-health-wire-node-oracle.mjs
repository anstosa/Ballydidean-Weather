import { workerHealth } from "../../../../apps/worker/dist/health.js";

const now = 1_790_000_000_000;
const utc = (value) => value === null ? null : new Date(value).toISOString();
const cases = [
  [now - 60_000, now - 120_000, true],
  [now - 1_800_001, now - 1_900_000, true],
  [null, null, false],
  [now + 1, now - 1_000, true],
];

// retain the exact one-shot output of the original health command
for (const [loop, success, ready] of cases) {
  process.stdout.write(JSON.stringify(workerHealth(new Date(now), {
    lastLoopAt: utc(loop), lastSuccessAt: utc(success), ready,
  })) + "\n");
}
