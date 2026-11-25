import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join } from "node:path";

import { AdjustmentRevisionArchiveStore } from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const [, , root, operation] = process.argv;
const hour = "2026-09-14T01:00:00.000Z";
const key = { logicalHourAt: hour, logicalKeySha256: "a".repeat(64) };

if (operation === "lease") {
  // witness the actual shared lease bytes while the native owner holds them
  const bytes = await readFile(join(root, "revision-spool-locks", "spool.lock"), "utf8");
  const lease = JSON.parse(bytes);
  assert.equal(lease.contractVersion, "adjustment-revision-spool-lock/v1");
  assert.equal(bytes, `${JSON.stringify(lease)}\n`);
  const until = Date.parse(lease.expiresAt);
  assert.ok(until > Date.now());
  assert.match(lease.token, /^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/u);
  console.log(lease.contractVersion);
} else {
  // access the exact native file through the retained Node archive owner
  const archive = new AdjustmentRevisionArchiveStore({
    root,
    // retry observes the first retained clock rather than this fallback
    now() {
      throw new Error("retained native record must keep its own clock");
    },
    // fixed gauge status never asks for a projection
    parseProjection() {
      throw new Error("unexpected projection parse");
    },
  });
  assert.equal(operation, "retry");
  const status = await archive.readRainFixedGaugeTargetGap(key);
  assert.equal(status.state, "present");
  const retry = await archive.recordRainFixedGaugeTargetGap({ ...key, reason: "target_source_oversized" });
  assert.deepEqual(retry, status.gap);
  console.log(`${retry.gapAt}|${retry.gapSha256}`);
}
