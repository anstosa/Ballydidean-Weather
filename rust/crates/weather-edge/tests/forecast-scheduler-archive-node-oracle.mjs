import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { chmod, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  AdjustmentEvidenceArchiveTransport,
  AdjustmentEvidenceStore,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const NOW = "2026-10-08T00:40:00.123Z";
const DUE_KEY = "capture/2026-10-08T00:35:00.000Z";
const testSource = readFileSync(
  new URL("../../../../deploy/test/adjustment-evidence-store.test.mjs", import.meta.url),
  "utf8",
);

// extract one unchanged producer span from the original Node test fixture
function span(source, startMarker, endMarker) {
  const start = source.indexOf(startMarker);
  const end = source.indexOf(endMarker, start);
  assert.ok(start >= 0 && end > start, startMarker);
  return source.slice(start, end);
}

const { forecastBody, forecastRow } = Function(
  `${span(testSource, "const hashes = {", "// prove a root-authorized initializer")}
    ${span(testSource, "function forecastRow(", "// provide measured production headroom")}
    return { forecastBody, forecastRow };`,
)();

// hash exact persisted bytes through the same Node SHA-256 owner
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

const root = await mkdtemp(join(tmpdir(), "weather-forecast-scheduler-archive-oracle-"));
await chmod(root, 0o700);
const statfs = async () => ({
  bavail: 600_000n,
  bsize: 4_096n,
  ffree: 65_536n,
});

try {
  const store = await new AdjustmentEvidenceStore({
    now: () => new Date(NOW),
    root,
    statfs,
  }).initialize();
  const rows = Array.from(
    { length: 241 },
    // retain the source fixture's ordered fall-back-hour row set
    (_, index) => forecastRow(index),
  );
  const prepared = store.prepareV2(
    forecastBody(rows, { days: 10 }),
    "days=10",
    "scheduler_request",
  );
  assert.notEqual(prepared, null);
  const commitResult = await store.commit(prepared, NOW);
  assert.deepEqual(commitResult, { status: "created" });

  const transport = await new AdjustmentEvidenceArchiveTransport({
    now: () => new Date(NOW),
    root,
    statfs,
  }).initialize();
  const captureResult = await transport.captureCommittedSchedulerEvidence({
    dueKey: DUE_KEY,
    prepared,
  });
  assert.equal(captureResult.status, "page_pending_acknowledgement");
  const envelope = await transport.next();
  assert.equal(envelope.kind, "page");
  const payload = Buffer.from(envelope.payloadBase64, "base64");
  const payloadValue = JSON.parse(payload.toString("utf8"));
  assert.equal(payloadValue.contractVersion, "adjustment-evidence-page-payload/v1");
  assert.equal(payloadValue.edgeReceiptIdentitySha256, prepared.edgeReceiptIdentitySha256);
  assert.equal(sha256(payload), envelope.header.payloadSha256);

  const currentSlot = await readFile(join(root, "online-pages", "slots", "current.page"));
  const metadataLength = currentSlot.readUInt32BE(0);
  const metadataEnd = 4 + metadataLength;
  const metadataBytes = currentSlot.subarray(4, metadataEnd);
  const persistedPayload = currentSlot.subarray(metadataEnd);
  assert.deepEqual(persistedPayload, payload);
  const metadata = JSON.parse(metadataBytes.toString("utf8"));
  assert.deepEqual(metadata.header, envelope.header);
  assert.equal(metadata.pageSha256, envelope.pageSha256);
  assert.deepEqual(metadata.projections, envelope.projections);

  process.stdout.write(`${JSON.stringify({
    captureResult,
    commitResult,
    contractVersion: "weather-forecast-scheduler-archive-node-oracle/v1",
    currentSlot: {
      base64: currentSlot.toString("base64"),
      byteLength: currentSlot.byteLength,
      metadata,
      metadataLength,
      payloadOffset: metadataEnd,
      sha256: sha256(currentSlot),
    },
    page: {
      contractVersion: envelope.contractVersion,
      header: envelope.header,
      pageSha256: envelope.pageSha256,
      projections: envelope.projections,
    },
    payload: {
      base64: envelope.payloadBase64,
      byteLength: payload.byteLength,
      sha256: sha256(payload),
      value: payloadValue,
    },
    prepared: {
      edgeReceiptIdentitySha256: prepared.edgeReceiptIdentitySha256,
      objectSha256: prepared.objectSha256,
      rowCount: rows.length,
    },
  })}\n`);
} finally {
  await rm(root, { force: true, recursive: true });
}
