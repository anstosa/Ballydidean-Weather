import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  AdjustmentRevisionArchiveStore,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

// encode recursively sorted canonical JSON with the archive's final newline
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

// build the exact database-issued shadow receipt identity
function revisionReceipt(stageReceipt) {
  const receipt = {
    archiveCommitOrdinal: "1",
    archiveCommittedAt: "2026-10-08T00:39:00.000Z",
    contractVersion: "adjustment-revision-commit-receipt/v1",
    frontierSha256: "",
    predecessorFrontierSha256: sha256(Buffer.from(
      "adjustment-revision-frontier/v1\n0\n",
    )),
    projectionIdentitySha256: "6".repeat(64),
    projectionKind: "shadow_prediction",
    projectionSha256: "2".repeat(64),
    receiptSha256: "",
    stageReceiptSha256: stageReceipt.stageReceiptSha256,
  };
  receipt.receiptSha256 = sha256(Buffer.from([
    receipt.contractVersion,
    receipt.archiveCommitOrdinal,
    receipt.archiveCommittedAt,
    receipt.projectionKind,
    receipt.projectionIdentitySha256,
    receipt.projectionSha256,
    receipt.stageReceiptSha256,
    receipt.predecessorFrontierSha256,
  ].join("\n")));
  receipt.frontierSha256 = sha256(Buffer.from([
    "adjustment-revision-frontier/v1",
    receipt.predecessorFrontierSha256,
    receipt.archiveCommitOrdinal,
    receipt.receiptSha256,
  ].join("\n")));
  return receipt;
}

// return one closed metadata fixture shared by both archive versions
function metadata(body) {
  return {
    bodyByteCount: body.length,
    candidateSha256: "1".repeat(64),
    dueKey: "capture/2026-10-08T00:35:00.000Z",
    inputSha256: "2".repeat(64),
    issuedAt: "2026-10-08T00:35:01.000Z",
    maxValidAt: "2026-10-08T12:00:00.000Z",
    minValidAt: "2026-10-08T01:00:00.000Z",
    predictionBodySha256: sha256(body),
    predictionSchemaSha256: "3".repeat(64),
    predictionSha256: "4".repeat(64),
    registrationSha256: "5".repeat(64),
    rowCount: 12,
    sourceReceiptSha256: "6".repeat(64),
    sourceSha256: "7".repeat(64),
  };
}

// execute one complete source stage, capsule, and publication path
async function fixture(version) {
  const root = await mkdtemp(join(tmpdir(), `weather-rust-shadow-${version}-`));
  const body = Buffer.from(`{"body":"${version}"}\n`);
  const source = Buffer.from(`{"source":"${version}"}\n`);
  const comparator = version === "v2" ? Buffer.from("{\"incumbent\":true}\n") : null;
  const values = [
    "2026-10-08T00:36:00.000Z",
    "2026-10-08T00:40:00.000Z",
  ];
  let clock = 0;
  try {
    const store = new AdjustmentRevisionArchiveStore({
      now: () => new Date(values[clock++]),
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
      validateShadowCapsule: () => undefined,
      validateShadowStage: () => undefined,
    });
    const stageInput = {
      bodyBase64: body.toString("base64"),
      ...(comparator === null ? {} : { comparatorBase64: comparator.toString("base64") }),
      metadata: metadata(body),
      sourceProjectionBase64: source.toString("base64"),
      sourceProjectionSha256: sha256(source),
    };
    const stageReceipt = await store.stageShadowRevision(stageInput);
    const firstClock = clock;
    assert.deepEqual(await store.stageShadowRevision(stageInput), stageReceipt);
    assert.equal(clock, firstClock);
    const receipt = revisionReceipt(stageReceipt);
    const publishInput = version === "v1" ? {
      metadata: stageInput.metadata,
      revisionReceipt: receipt,
      sourceProjectionSha256: stageInput.sourceProjectionSha256,
      stageReceiptSha256: stageReceipt.stageReceiptSha256,
    } : {
      ...stageInput,
      predictionCommittedAt: "2026-10-08T00:38:00.000Z",
      revisionReceipt: receipt,
      stageReceipt,
    };
    const publication = await store.publishShadowRevision(publishInput);
    const stageNames = await readdir(join(root, "shadow-revision-stages"));
    const capsuleNames = await readdir(join(root, "shadow-revision-capsules"));
    assert.deepEqual(stageNames, []);
    assert.equal(capsuleNames.length, 1);
    const capsuleBytes = await readFile(join(root, "shadow-revision-capsules", capsuleNames[0]));
    assert.deepEqual(await store.persistShadowRevisionCapsule(capsuleBytes),
      JSON.parse(capsuleBytes.toString("utf8")));
    const stageBytes = canonicalBytes({
      contractVersion: `adjustment-shadow-revision-stage/${version}`,
      input: stageInput,
      stageReceipt,
    });
    const publishBytes = canonicalBytes(publication);
    const revisionBytes = canonicalBytes(receipt);
    const successorBytes = canonicalBytes({
      archiveCommitOrdinal: receipt.archiveCommitOrdinal,
      contractVersion: "adjustment-revision-successor/v1",
      frontierSha256: receipt.frontierSha256,
      predecessorFrontierSha256: receipt.predecessorFrontierSha256,
      receiptSha256: receipt.receiptSha256,
    });
    return {
      body,
      capsuleBytes,
      comparator,
      publication,
      publishBytes,
      receipt,
      revisionBytes,
      source,
      stageBytes,
      stageInput,
      stageReceipt,
      successorBytes,
    };
  } finally {
    await rm(root, { force: true, recursive: true });
  }
}

const outputs = [];
// retain exact artifacts from both supported ordinary source grammars
for (const version of ["v1", "v2"]) {
  const value = await fixture(version);
  const fields = {
    body_hex: value.body.toString("hex"),
    capsule_hex: value.capsuleBytes.toString("hex"),
    comparator_hex: value.comparator?.toString("hex") ?? "none",
    frontier_sha256: value.receipt.frontierSha256,
    predecessor_frontier_sha256: value.receipt.predecessorFrontierSha256,
    prediction_body_sha256: value.stageInput.metadata.predictionBodySha256,
    publish_hex: value.publishBytes.toString("hex"),
    publish_receipt_sha256: value.publication.publishReceiptSha256,
    receipt_sha256: value.receipt.receiptSha256,
    revision_hex: value.revisionBytes.toString("hex"),
    source_hex: value.source.toString("hex"),
    source_projection_sha256: value.stageInput.sourceProjectionSha256,
    stage_hex: value.stageBytes.toString("hex"),
    stage_receipt_sha256: value.stageReceipt.stageReceiptSha256,
    successor_hex: value.successorBytes.toString("hex"),
    ...(version === "v2" ? { comparator_sha256: value.stageReceipt.comparatorSha256 } : {}),
  };
  // expose only bounded single-line fixture values to Rust
  for (const [key, field] of Object.entries(fields)) {
    outputs.push(`${version}_${key}=${field}`);
  }
}

process.stdout.write(`${outputs.join("\n")}\n`);
