import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import vm from "node:vm";

import { AdjustmentRevisionArchiveStore } from "../../../../deploy/scripts/adjustment-evidence-store.mjs";
import {
  createRainMaintenanceControlState,
  encodeRainMaintenanceControlState,
  parseRainMaintenanceControlState,
  RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256,
} from "../../../../packages/forecast-adjustment/dist/index.js";

const apiFixture = await readFile(
  new URL("../../weather-api/tests/maintenance-service-node-oracle.mjs", import.meta.url),
  "utf8",
);
const start = apiFixture.indexOf("function rainControlStateBytes()");
const end = apiFixture.indexOf("// build one canonical current Best Match", start);
assert.ok(start >= 0 && end > start);
// use the real canonical producer and established supported control-state material
const state = vm.runInNewContext(
  `${apiFixture.slice(start, end)};rainControlStateBytes()`,
  {
    captureEpoch: { witnessSha256: "3".repeat(64) },
    // pass the fixture into the actual producer's plain-object realm
    createRainMaintenanceControlState(input) {
      return createRainMaintenanceControlState(structuredClone(input));
    },
    encodeRainMaintenanceControlState,
    RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256,
  },
);

const now = ["2026-09-14T01:05:00.123Z", "2026-09-14T01:09:00.456Z"];
let clock = 0;
const root = await mkdtemp(join(tmpdir(), "weather-rain-state-archive-"));

try {
  const store = new AdjustmentRevisionArchiveStore({
    root,
    parseRainControlState: parseRainMaintenanceControlState,
    // preserve the two independently witnessed first-write clocks
    now() {
      assert.ok(clock < now.length);
      return new Date(now[clock++]);
    },
    // no projection is staged in the normal producer-abandonment scenario
    parseProjection() {
      throw new Error("unexpected projection");
    },
  });
  const stage = await store.stageRainControlState(state);
  assert.deepEqual(await store.stageRainControlState(state), stage);
  const input = {
    reason: "projection_stage_failed",
    stageReceiptSha256: stage.stageReceiptSha256,
    stateSha256: stage.stateSha256,
  };
  const gap = await store.recordRainControlStateGap(input);
  assert.deepEqual(await store.recordRainControlStateGap(input), gap);
  assert.equal(clock, 2);
  const name = `sha256-${stage.stateSha256}.json`;
  const stateBytes = await readFile(join(root, "rain-control-states", name));
  assert.ok(stateBytes.equals(state));
  const stageBytes = await readFile(join(root, "rain-control-state-stage-receipts", name), "utf8");
  const gapBytes = await readFile(join(root, "rain-control-state-gaps", name), "utf8");
  const controlState = JSON.parse(state);
  const { stateSha256: controlStateSha256, ...controlStateUnsigned } = controlState;
  assert.deepEqual(JSON.parse(stageBytes), stage);
  assert.deepEqual(JSON.parse(gapBytes), gap);
  const { stageReceiptSha256, ...stageUnsigned } = JSON.parse(stageBytes);
  const { gapSha256, ...gapUnsigned } = JSON.parse(gapBytes);
  console.log(`body=${state.toString("hex")}`);
  console.log(`state_digest=${stage.stateSha256}`);
  console.log(`stage_digest=${stageReceiptSha256}`);
  console.log(`gap_digest=${gapSha256}`);
  console.log(`stage_unsigned=${JSON.stringify(stageUnsigned)}`);
  console.log(`gap_unsigned=${JSON.stringify(gapUnsigned)}`);
  console.log(`stage_bytes=${stageBytes.trimEnd()}`);
  console.log(`gap_bytes=${gapBytes.trimEnd()}`);
  console.log(`stage_ms=${Date.parse(now[0])}`);
  console.log(`gap_ms=${Date.parse(now[1])}`);
  console.log(`control_state_sha256=${controlStateSha256}`);
  console.log(`control_recipe_sha256=${controlState.recipeSha256}`);
  console.log(`control_unsigned=${JSON.stringify(controlStateUnsigned)}`);
  console.log(
    `control_monthStart_ms=${Date.parse(`${controlState.modelMonth}-01T00:00:00.000Z`)}`,
  );
  // expose the actual producer clocks to the injected Rust calendar fixture
  for (const key of [
    "calibrationEndAt",
    "calibrationStartAt",
    "generatedAt",
    "legacyCalibrationStartAt",
    "trainingMaximumValidAt",
  ]) {
    console.log(`control_${key}_ms=${Date.parse(controlState[key])}`);
  }
} finally {
  // clean only the fixture's private directory and two receipts
  await rm(root, { recursive: true, force: true });
}
