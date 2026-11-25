import { chmod, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { AdjustmentEvidenceScheduler } from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const CAPTURE_END = "2028-10-08T07:00:00.000Z";
const SCHEDULE_CONTRACT_SHA256 =
  "7c17f5d1a8e8249cd0aa4820638169e51f6edb3433017f50ab4c959e44c62f1f";

// project one exact authenticated rolling schedule head
function scheduleSlots() {
  return ["temperature", "wind", "rain"].map(
    // retain identical finite horizon authority for every family
    (family) => ({
      contractVersion: "adjustment-shadow-registration-slot/v3",
      epochWitnessSha256: "a".repeat(64),
      family,
      horizonEndAt: CAPTURE_END,
      registrationSha256: null,
      scheduleContractSha256: SCHEDULE_CONTRACT_SHA256,
      state: "free",
      terminalAt: null,
    }),
  );
}

// capture one result beside the scheduler's complete immutable projection
function stage(result, scheduler) {
  return { result, state: scheduler.status() };
}

const root = await mkdtemp(join(tmpdir(), "weather-forecast-scheduler-oracle-"));
await chmod(root, 0o700);

let nowAt = "2026-10-08T00:40:00.000Z";
let triggerMode = "created";
const triggers = [];

// use only deterministic local controller ports around the real state store
const options = {
  enabled: true,
  migrationReady: true,
  now: () => new Date(nowAt),
  readScheduleSlots: async () => scheduleSlots(),
  root,
  statfs: async () => ({
    bavail: 600_000n,
    bsize: 4_096n,
    ffree: 65_536n,
  }),
  trigger: async (input) => {
    // retain the attempted capture before returning or throwing its outcome
    triggers.push({ ...input, outcome: triggerMode });

    // exercise the durable non-commit failure branch after normal captures
    if (triggerMode === "failed") {
      const error = new Error("injected capture failure");
      error.code = "injected_failure";
      throw error;
    }
    return { status: "created" };
  },
};

try {
  const first = await new AdjustmentEvidenceScheduler(options).initialize();
  const firstCatchup = stage(await first.runDue(), first);

  const restarted = await new AdjustmentEvidenceScheduler(options).initialize();
  const repeatedAfterRestart = stage(await restarted.runDue(), restarted);

  nowAt = "2026-10-08T18:40:00.000Z";
  const missedCatchup = stage(await restarted.runDue(), restarted);

  nowAt = "2026-10-08T12:40:00.000Z";
  const rollback = stage(await restarted.runDue(), restarted);

  nowAt = "2026-10-09T00:40:00.000Z";
  triggerMode = "failed";
  const failedTrigger = stage(await restarted.runDue(), restarted);

  nowAt = CAPTURE_END;
  const terminal = stage(await restarted.runDue(), restarted);
  const currentStateUtf8 = await readFile(join(root, "scheduler", "current.json"), "utf8");
  const previousStateUtf8 = await readFile(join(root, "scheduler", "previous.json"), "utf8");

  process.stdout.write(`${JSON.stringify({
    contractVersion: "weather-forecast-scheduler-node-oracle/v1",
    currentStateUtf8,
    failedTrigger,
    firstCatchup,
    missedCatchup: {
      ...missedCatchup,
      gapDueKeys: missedCatchup.state.gaps.map(
        // retain only the exact two categorical gaps produced at this stage
        (gap) => gap.dueKey,
      ),
    },
    previousStateUtf8,
    repeatedAfterRestart,
    rollback,
    scheduleSlots: scheduleSlots(),
    terminal,
    triggers,
  })}\n`);
} finally {
  await rm(root, { force: true, recursive: true });
}
