import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";

import { AdjustmentRevisionArchiveStore } from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const root = await mkdtemp(join(tmpdir(), "weather-permanent-gaps-"));
const clock = "2026-09-14T01:05:00.123Z";
const shadow = {
  dueKey: "capture/2026-09-14T00:35:00.000Z",
  family: "wind",
  reason: "candidate_unavailable",
  registrationSha256: "a".repeat(64),
};
const stage = {
  logicalKeySha256: "b".repeat(64),
  projectionIdentitySha256: null,
  projectionKind: "actual_best_match",
  projectionSha256: null,
  reason: "archive_stage_failed",
};
const publish = {
  ...stage,
  projectionIdentitySha256: "c".repeat(64),
  projectionSha256: "c".repeat(64),
  reason: "archive_publish_failed",
};
try {
  const archive = new AdjustmentRevisionArchiveStore({
    root,
    // freeze the clock for the exact first permanent disposition
    now() { return new Date(clock); },
    // value-free dispositions do not decode a projection
    parseProjection() { throw new Error("unexpected projection"); },
  });
  // compare the exact retained documents and hash preimages for the ordinary cases
  for (const [label, input] of [["shadow", shadow], ["stage", stage], ["publish", publish]]) {
    const shadowGap = label === "shadow";
    const method = shadowGap ? "recordShadowGap" : "recordGap";
    const first = await archive[method](input);
    assert.deepEqual(await archive[method](input), first);
    const key = shadowGap
      ? { dueKey: input.dueKey, registrationSha256: input.registrationSha256 }
      : {
        logicalKeySha256: input.logicalKeySha256,
        projectionIdentitySha256: input.projectionIdentitySha256,
      };
    const preimage = JSON.stringify(key);
    const filename = `sha256-${createHash("sha256").update(`${preimage}\n`).digest("hex")}.json`;
    const directory = shadowGap ? "shadow-revision-gaps" : "revision-gaps";
    const bytes = await readFile(join(root, directory, filename), "utf8");
    assert.deepEqual(first, JSON.parse(bytes));
    const { gapSha256, ...unsigned } = JSON.parse(bytes);
    console.log(`${label}_filename=${filename}`);
    console.log(`${label}_key=${preimage}`);
    console.log(`${label}_unsigned=${JSON.stringify(unsigned)}`);
    console.log(`${label}_bytes=${bytes.trimEnd()}`);
    console.log(`${label}_digest=${gapSha256}`);
  }
  console.log(`clock_ms=${Date.parse(clock)}`);
} finally {
  // remove only the disposable archive directory created by this test
  await rm(root, { recursive: true, force: true });
}
