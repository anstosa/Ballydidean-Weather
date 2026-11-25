import assert from "node:assert/strict";
import { mkdtemp, readdir, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  AdjustmentRevisionArchiveStore,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const root = await mkdtemp(join(tmpdir(), "weather-fixed-gauge-gap-"));
const gapAt = "2026-09-14T01:05:00.123Z";
const key = {
  logicalHourAt: "2026-09-14T01:00:00.000Z",
  logicalKeySha256: "a".repeat(64),
};
const input = { ...key, reason: "target_source_oversized" };
let clockReads = 0;

try {
  const store = new AdjustmentRevisionArchiveStore({
    root,
    // supply the first captured archive clock
    now() {
      clockReads += 1;
      return new Date(gapAt);
    },
    // unused by the value-free fixed-gauge workflow
    parseProjection() {
      throw new Error("unexpected projection read");
    },
  });
  const absent = await store.readRainFixedGaugeTargetGap(key);
  assert.equal(absent.state, "absent");
  const first = await store.recordRainFixedGaugeTargetGap(input);
  const retry = await store.recordRainFixedGaugeTargetGap(input);
  assert.deepEqual(retry, first);
  assert.equal(clockReads, 1);
  assert.equal(first.gapAt, gapAt);
  const present = await store.readRainFixedGaugeTargetGap(key);
  assert.equal(present.state, "present");
  assert.deepEqual(present.gap, first);

  const directory = join(root, "rain-fixed-gauge-target-gaps");
  const [name, ...extra] = await readdir(directory);
  assert.equal(extra.length, 0);
  const durable = await readFile(join(directory, name), "utf8");
  const { gapSha256, ...unsigned } = JSON.parse(durable);
  assert.deepEqual(JSON.parse(durable), first);
  console.log(`key_preimage=${JSON.stringify(key)}`);
  console.log(`unsigned=${JSON.stringify(unsigned)}`);
  console.log(`digest=${gapSha256}`);
  console.log(`filename=${name}`);
  console.log(`receipt=${durable.trimEnd()}`);
  console.log(`key_ms=${Date.parse(key.logicalHourAt)}`);
  console.log(`clock_ms=${Date.parse(gapAt)}`);
} finally {
  // clean only the fixture-owned private root
  await rm(root, { recursive: true, force: true });
}
