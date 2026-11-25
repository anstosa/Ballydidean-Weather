import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { access, mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  AdjustmentRevisionArchiveStore,
  acknowledgeAdjustmentRevisionGapPayload,
  readAdjustmentRevisionGapPayloadPage,
  readAdjustmentRevisionGapTransferStart,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

// encode the same recursively ordered json bytes used by the archive source
function canonicalBytes(value) {
  const normalize = (entry) => {
    // preserve scalar values directly
    if (entry === null || typeof entry !== "object") {
      return entry;
    }
    // preserve semantic array order
    if (Array.isArray(entry)) {
      return entry.map(normalize);
    }
    return Object.fromEntries(Object.keys(entry).sort().map(
      // normalize each object value under its canonical key
      (key) => [key, normalize(entry[key])],
    ));
  };
  return Buffer.from(`${JSON.stringify(normalize(value))}\n`);
}

// hash one exact source byte sequence
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

const root = await mkdtemp(join(tmpdir(), "weather-revision-gap-ack-oracle-"));

try {
  const store = new AdjustmentRevisionArchiveStore({
    now: () => new Date("2026-11-23T12:00:00.000Z"),
    parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
    root,
  });
  const projectionBytes = canonicalBytes({
    contractVersion: "adjustment-revision-projection/v1",
    family: "temperature",
    logicalKey: { validAt: "2026-11-23T12:00:00.000Z" },
    logicalReceivedAt: "2026-11-23T12:01:00.000Z",
    projectionKind: "native_source",
    rows: [{ value: 1 }],
    source: {},
    storedContentSha256: "a".repeat(64),
  });
  const stage = await store.stageProjection(projectionBytes);
  await store.recordGap({
    logicalKeySha256: "b".repeat(64),
    projectionIdentitySha256: stage.projectionIdentitySha256,
    projectionKind: stage.projectionKind,
    projectionSha256: stage.projectionSha256,
    reason: "database_bind_failed",
  });
  const start = await readAdjustmentRevisionGapTransferStart({ root });
  const page = await readAdjustmentRevisionGapPayloadPage({
    frontierSha256: start.frontierSha256,
    root,
    startSha256: start.startSha256,
  });
  const graphManifestSha256 = "c".repeat(64);
  const acknowledgement = await acknowledgeAdjustmentRevisionGapPayload({
    graphManifestSha256,
    pageSha256: page.pageSha256,
    root,
  });
  const expectedFrontier = sha256(Buffer.from(
    `${acknowledgement.contractVersion}\n${start.frontierSha256}\n` +
      `${page.pageSha256}\n${graphManifestSha256}\n${page.memberRootSha256}`,
  ));
  const unsigned = { ...acknowledgement };
  delete unsigned.acknowledgementSha256;

  assert.equal(acknowledgement.frontierSha256, expectedFrontier);
  assert.equal(acknowledgement.acknowledgementSha256, sha256(canonicalBytes(unsigned)));
  assert.equal(acknowledgement.pageCount, "1");
  assert.deepEqual(
    await readFile(join(root, "revision-gap-transfer", "current.json")),
    canonicalBytes(acknowledgement),
  );
  assert.deepEqual(await readdir(join(root, "revision-projections")), []);
  assert.deepEqual(await readdir(join(root, "revision-stage-receipts")), []);
  assert.equal((await readdir(join(root, "revision-gaps"))).length, 1);
  await assert.rejects(access(join(root, "revision-gap-transfer", "pending.json")));
  assert.deepEqual(await acknowledgeAdjustmentRevisionGapPayload({
    graphManifestSha256,
    pageSha256: page.pageSha256,
    root,
  }), acknowledgement);
  await assert.rejects(acknowledgeAdjustmentRevisionGapPayload({
    graphManifestSha256: "d".repeat(64),
    pageSha256: page.pageSha256,
    root,
  }), /acknowledgement collision/u);

  process.stdout.write([
    `contract=${acknowledgement.contractVersion}`,
    `page_count=${acknowledgement.pageCount}`,
    `checkpoint_first=${acknowledgement.frontierSha256 === expectedFrontier}`,
    "retry_stable=true",
    "hot_retired=true",
    "permanent_gap_retained=true",
  ].join("\n") + "\n");
} finally {
  // remove the disposable archive witness after parity capture
  await rm(root, { force: true, recursive: true });
}
