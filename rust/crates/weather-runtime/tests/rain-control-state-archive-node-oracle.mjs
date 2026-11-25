import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

import {
  parseAdjustmentRevisionProjectionDocument,
  parseRainMaintenanceControlState,
} from "../../../../apps/web/src/adjustment-maintenance-contract.mjs";
import {
  AdjustmentRevisionArchiveStore,
  createAdjustmentRevisionArchiveHandler,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const parent = process.argv[2];
const root = join(parent, "node-rain-control");
const now = () => new Date("2026-10-08T03:00:00.000Z");
const base = "/internal/adjustment-maintenance/rain-control-state";
const recipeSha256 = "afa04352b82ccb9704a49863c2dd8724d5c129760ca6d9e4760cb1dda72fb34a";

// recursively sort only for the source's canonical self-address preimage
function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map(
      // preserve each original value while sorting its containing object
      (key) => [key, canonical(value[key])],
    ));
  }
  return value;
}

// hash exact bytes with the same Node primitive used by the original store
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// encode one recursively sorted canonical document with its terminating line feed
function canonicalBytes(value) {
  return Buffer.from(`${JSON.stringify(canonical(value))}\n`);
}

// build the exact source-order monthly state without relying on generated dist output
const unsignedState = {
  calibrationEndAt: "2026-08-25T00:00:00.000Z",
  calibrationStartAt: "2026-05-27T00:00:00.000Z",
  contractVersion: "rain-maintenance-control-state/v1",
  epochWitnessSha256: "3".repeat(64),
  generatedAt: "2026-08-26T00:00:00.000Z",
  legacyCalibrationStartAt: "2026-07-11T00:00:00.000Z",
  legacyRawScale64: "3ff199999999999a",
  modelMonth: "2026-09",
  ordinalArtifactSha256: "4".repeat(64),
  recentFallbackReason: "recent_calibration",
  recentRawScale64: "3ff3333333333333",
  recentSupported: true,
  recipeSha256,
  sameWindowRawScale64: "3ff2666666666666",
  scheduleContractSha256: "5".repeat(64),
  sourceMemberRootSha256: "6".repeat(64),
  sourceReceiptRootSha256: "7".repeat(64),
  support: {
    calibrationDates: 90,
    calibrationHours: 2_000,
    calibrationRows: 2_000,
    calibrationWetDates: 20,
    calibrationWetHours: 200,
    effectiveDates64: "404e000000000000",
    effectiveWetDates64: "4008000000000000",
    legacyCalibrationRows: 500,
    legacyTrainingRows: 2_000,
    legacyTrainingWetRows: 200,
    trainingDates: 200,
    trainingHours: 4_000,
    trainingRows: 4_000,
    trainingWetDates: 30,
    trainingWetHours: 300,
  },
  trainingMaximumValidAt: "2026-05-19T23:00:00.000Z",
};
const state = {
  calibrationEndAt: unsignedState.calibrationEndAt,
  calibrationStartAt: unsignedState.calibrationStartAt,
  contractVersion: unsignedState.contractVersion,
  epochWitnessSha256: unsignedState.epochWitnessSha256,
  generatedAt: unsignedState.generatedAt,
  legacyCalibrationStartAt: unsignedState.legacyCalibrationStartAt,
  legacyRawScale64: unsignedState.legacyRawScale64,
  modelMonth: unsignedState.modelMonth,
  ordinalArtifactSha256: unsignedState.ordinalArtifactSha256,
  recentFallbackReason: unsignedState.recentFallbackReason,
  recentRawScale64: unsignedState.recentRawScale64,
  recentSupported: unsignedState.recentSupported,
  recipeSha256: unsignedState.recipeSha256,
  sameWindowRawScale64: unsignedState.sameWindowRawScale64,
  scheduleContractSha256: unsignedState.scheduleContractSha256,
  sourceMemberRootSha256: unsignedState.sourceMemberRootSha256,
  sourceReceiptRootSha256: unsignedState.sourceReceiptRootSha256,
  stateSha256: sha256(canonicalBytes(unsignedState)),
  support: unsignedState.support,
  trainingMaximumValidAt: unsignedState.trainingMaximumValidAt,
};
const stateBytes = Buffer.from(`${JSON.stringify(state)}\n`);
assert.deepEqual(parseRainMaintenanceControlState(stateBytes), state);

await mkdir(root, { mode: 0o700 });
const store = new AdjustmentRevisionArchiveStore({
  root,
  now,
  parseRainControlState: parseRainMaintenanceControlState,
  parseProjection: parseAdjustmentRevisionProjectionDocument,
});
const handler = createAdjustmentRevisionArchiveHandler({ store });
const stageRequest = { stateBase64: stateBytes.toString("base64") };
const stage = await handler(`${base}/stage`, stageRequest);
const stageRetry = await handler(`${base}/stage`, stageRequest);
assert.deepEqual(stageRetry, stage);
const gapRequest = {
  reason: "projection_stage_failed",
  stageReceiptSha256: stage.stageReceiptSha256,
  stateSha256: stage.stateSha256,
};
const gap = await handler(`${base}/gap`, gapRequest);
const gapRetry = await handler(`${base}/gap`, gapRequest);
assert.deepEqual(gapRetry, gap);

const cases = [
  ["rain-control-stage-first", `${base}/stage`, stageRequest, stage],
  ["rain-control-stage-retry", `${base}/stage`, stageRequest, stageRetry],
  ["rain-control-gap-first", `${base}/gap`, gapRequest, gap],
  ["rain-control-gap-retry", `${base}/gap`, gapRequest, gapRetry],
];
// retain exact raw requests and actual handler responses for Rust listener replay
for (const [name, _target, request, response] of cases) {
  await writeFile(join(parent, `${name}.request.json`), JSON.stringify(request));
  await writeFile(join(parent, `${name}.response.json`), `${JSON.stringify(response)}\n`);
}
await writeFile(
  join(parent, "rain-control-cases.tsv"),
  `${cases.map(([name, target]) => `${name}\t${target}`).join("\n")}\n`,
);

// prove the original store retained the exact producer body and first receipts
const name = `sha256-${stage.stateSha256}.json`;
assert.ok((await readFile(join(root, "rain-control-states", name))).equals(stateBytes));
assert.deepEqual(
  JSON.parse(await readFile(join(root, "rain-control-state-stage-receipts", name), "utf8")),
  stage,
);
assert.deepEqual(
  JSON.parse(await readFile(join(root, "rain-control-state-gaps", name), "utf8")),
  gap,
);
