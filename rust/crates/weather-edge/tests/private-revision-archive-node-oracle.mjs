import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  AdjustmentRevisionArchiveStore,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

// hash one exact byte sequence in the archive wire alphabet
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// create one database-authenticated revision receipt
function revisionReceipt({ ordinal, predecessor, projectionIdentity, projectionKind, stage }) {
  const archiveCommittedAt = `2026-11-23T12:0${ordinal}:00.000Z`;
  const contractVersion = "adjustment-revision-commit-receipt/v1";
  const receiptSha256 = sha256(Buffer.from([
    contractVersion,
    String(ordinal),
    archiveCommittedAt,
    projectionKind,
    projectionIdentity,
    projectionIdentity,
    stage.stageReceiptSha256,
    predecessor,
  ].join("\n")));
  const frontierSha256 = sha256(Buffer.from([
    "adjustment-revision-frontier/v1",
    predecessor,
    String(ordinal),
    receiptSha256,
  ].join("\n")));
  return {
    archiveCommitOrdinal: String(ordinal),
    archiveCommittedAt,
    contractVersion,
    frontierSha256,
    predecessorFrontierSha256: predecessor,
    projectionIdentitySha256: projectionIdentity,
    projectionKind,
    projectionSha256: projectionIdentity,
    receiptSha256,
    stageReceiptSha256: stage.stageReceiptSha256,
  };
}

const root = await mkdtemp(join(tmpdir(), "weather-private-revision-oracle-"));
await chmod(root, 0o700);
let now = "2026-11-23T12:10:00.000Z";
const store = new AdjustmentRevisionArchiveStore({
  // retain the exact disposable archive clock
  now: () => now,
  // parse only the ordinary oracle projections supplied below
  parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
  root,
});

try {
  const singleBytes = Buffer.from(`${JSON.stringify({
    contractVersion: "adjustment-revision-projection/v1",
    family: "temperature",
    logicalKey: { validAt: "2026-11-23T12:00:00.000Z" },
    logicalReceivedAt: "2026-11-23T12:01:00.000Z",
    projectionKind: "native_source",
    rows: [],
    source: {},
    storedContentSha256: "a".repeat(64),
  })}\n`);
  const singleIdentity = sha256(singleBytes);
  const singleStage = await store.stageProjection(singleBytes);
  now = "2026-11-23T13:10:00.000Z";
  const singleStageRetry = await store.stageProjection(singleBytes);
  const singleRevision = revisionReceipt({
    ordinal: 1,
    predecessor: "0".repeat(64),
    projectionIdentity: singleIdentity,
    projectionKind: "native_source",
    stage: singleStage,
  });
  now = "2026-11-23T14:10:00.000Z";
  const singlePublish = await store.publishRevision({
    revisionReceipt: singleRevision,
    stageReceipt: singleStage,
  });
  now = "2026-11-23T15:10:00.000Z";
  const singlePublishRetry = await store.publishRevision({
    revisionReceipt: singleRevision,
    stageReceipt: singleStage,
  });

  const batchBytes = Buffer.from(`${JSON.stringify({
    contractVersion: "adjustment-revision-batch-projection/v2",
    family: "wind",
    logicalKey: { validAt: "2026-11-23T13:00:00.000Z" },
    logicalReceivedAt: "2026-11-23T13:01:00.000Z",
    projectionKind: "actual_best_match",
    rows: [{ value: 1 }, { value: 2 }],
    source: {},
    storedContentSha256: "b".repeat(64),
  })}\n`);
  const batchIdentity = sha256(batchBytes);
  now = "2026-11-23T16:10:00.000Z";
  const batchStage = await store.stageProjection(batchBytes);
  const first = revisionReceipt({
    ordinal: 2,
    predecessor: singleRevision.frontierSha256,
    projectionIdentity: batchIdentity,
    projectionKind: "actual_best_match",
    stage: batchStage,
  });
  const second = revisionReceipt({
    ordinal: 3,
    predecessor: first.frontierSha256,
    projectionIdentity: batchIdentity,
    projectionKind: "actual_best_match",
    stage: batchStage,
  });
  now = "2026-11-23T17:10:00.000Z";
  const batchPublish = await store.publishRevision({
    revisionReceipts: [first, second],
    stageReceipt: batchStage,
  });
  now = "2026-11-23T18:10:00.000Z";
  const batchPublishRetry = await store.publishRevision({
    revisionReceipts: [first, second],
    stageReceipt: batchStage,
  });

  assert.deepEqual(singleStageRetry, singleStage);
  assert.deepEqual(singlePublishRetry, singlePublish);
  assert.deepEqual(batchPublishRetry, batchPublish);

  process.stdout.write([
    `single-stage|${singleStage.projectionKind}|${singleStage.durableAt}|${singleStage.projectionIdentitySha256 === singleIdentity}|${singleStage.projectionSha256 === singleIdentity}|${singleStage.stageReceiptSha256.length}`,
    `single-publish|${singlePublish.projectionKind}|${singlePublish.committedAt}|${singlePublish.projectionIdentitySha256 === singleIdentity}|${singlePublish.revisionReceiptSha256 === singleRevision.receiptSha256}|${singlePublish.publishReceiptSha256.length}`,
    `batch-stage|${batchStage.projectionKind}|${batchStage.durableAt}|${batchStage.projectionIdentitySha256 === batchIdentity}|${batchStage.stageReceiptSha256.length}`,
    `batch-publish|${batchPublish.projectionKind}|${batchPublish.committedAt}|${batchPublish.projectionIdentitySha256 === batchIdentity}|${batchPublish.revisionReceiptSha256s.join(",") === [first.receiptSha256, second.receiptSha256].join(",")}|${batchPublish.revisionReceiptSha256s.length}|${batchPublish.publishReceiptSha256.length}`,
  ].join("\n") + "\n");
} finally {
  // remove the disposable archive fixture after parity capture
  await rm(root, { force: true, recursive: true });
}
