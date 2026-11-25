import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  AdjustmentCyclePageDiskPorts,
  ackCyclePage,
  appendCyclePage,
  createCyclePageState,
  recordEvidenceGap,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const DUE_KEY = "capture/2026-10-08T00:35:00.000Z";
const GENERATION = "1791419700000";
const ACKNOWLEDGED_AT = "2026-10-08T00:41:00.456Z";
const GAP_AT = "2026-10-08T00:42:00.789Z";

// hash exact persisted fixture bytes with the source Node owner
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

const root = await mkdtemp(join(tmpdir(), "weather-forecast-cycle-ack-oracle-"));
await chmod(root, 0o700);

try {
  const ports = await new AdjustmentCyclePageDiskPorts({
    now: () => new Date(ACKNOWLEDGED_AT),
    root,
    statfs: async () => ({
      bavail: 600_000n,
      bsize: 4_096n,
      ffree: 65_536n,
    }),
  }).initialize();
  let state = createCyclePageState({
    dailyPageCount: 0,
    dueKey: DUE_KEY,
    generation: GENERATION,
    localDate: "2026-10-08",
  });
  state = await appendCyclePage(state, {
    payload: Buffer.from("scheduler-page-one"),
    projections: [{
      channel: "scheduler_request",
      identitySha256: "1".repeat(64),
    }],
  }, ports);
  state = await appendCyclePage(state, {
    payload: Buffer.from("organic-page-two"),
    projections: [{
      channel: "organic",
      identitySha256: "2".repeat(64),
    }],
  }, ports);
  assert.deepEqual(state.slots.map((slot) => slot.slot), ["current", "next"]);

  const slotDirectory = join(root, "online-pages", "slots");
  const currentBefore = await readFile(join(slotDirectory, "current.page"));
  const nextBefore = await readFile(join(slotDirectory, "next.page"));
  const firstPageSha256 = state.pages[0].pageSha256;
  const firstPage = await ports.readSlot("current");
  await ports.persistTransferAcknowledgement({
    acknowledgedAt: ACKNOWLEDGED_AT,
    checkpointSha256: "a".repeat(64),
    contractVersion: "adjustment-archive-transfer-ack/v1",
    header: firstPage.header,
    pageSha256: firstPageSha256,
    payloadLength: firstPage.payload.length,
    projections: firstPage.projections,
  });
  const transferBytes = await readFile(join(root, "online-pages", "archive-acknowledgements", `sha256-${firstPageSha256}.json`));
  state = await ackCyclePage(state, {
    acknowledgedAt: ACKNOWLEDGED_AT,
    pageSha256: firstPageSha256,
  }, ports);
  assert.equal(state.acknowledgements.length, 1);
  assert.deepEqual(state.slots, [{
    pageIndex: 1,
    pageSha256: state.pages[1].pageSha256,
    slot: "current",
  }]);

  const acknowledgement = state.acknowledgements[0];
  const acknowledgementPath = join(
    root,
    "online-pages",
    "acknowledgements",
    `sha256-${acknowledgement.acknowledgementSha256}.json`,
  );
  const acknowledgementBytes = await readFile(acknowledgementPath);
  assert.equal(sha256(acknowledgementBytes), acknowledgement.acknowledgementSha256);
  const acknowledgementValue = JSON.parse(acknowledgementBytes.toString("utf8"));
  assert.deepEqual(acknowledgementValue, {
    acknowledgedAt: ACKNOWLEDGED_AT,
    contractVersion: "adjustment-cycle-page-ack/v1",
    dueKey: DUE_KEY,
    generation: GENERATION,
    pageIndex: 0,
    pageSha256: firstPageSha256,
  });

  const promotedCurrent = await readFile(join(slotDirectory, "current.page"));
  assert.deepEqual(promotedCurrent, nextBefore);
  assert.deepEqual(await readdir(slotDirectory), ["current.page"]);
  const promoted = await ports.readSlot("current");
  assert.equal(promoted.pageSha256, state.pages[1].pageSha256);

  const withGap = await recordEvidenceGap(state, {
    occurredAt: GAP_AT,
    projectionIdentitySha256: "3".repeat(64),
    reason: "capacity_refused",
  }, ports);
  const gapRecord = withGap.gaps[0];
  const gapBytes = await readFile(join(root, "online-pages", "gaps", `sha256-${gapRecord.gapSha256}.json`));
  assert.equal(sha256(gapBytes), gapRecord.gapSha256);

  process.stdout.write(`${JSON.stringify({
    acknowledgement: {
      base64: acknowledgementBytes.toString("base64"),
      byteLength: acknowledgementBytes.byteLength,
      sha256: acknowledgement.acknowledgementSha256,
      value: acknowledgementValue,
    },
    contractVersion: "weather-forecast-cycle-ack-node-oracle/v1",
    currentBefore: {
      base64: currentBefore.toString("base64"),
      sha256: sha256(currentBefore),
    },
    finalState: state,
    gap: { base64: gapBytes.toString("base64"), sha256: gapRecord.gapSha256 },
    transferCheckpoint: { base64: transferBytes.toString("base64"), sha256: sha256(transferBytes) },
    nextAbsent: true,
    nextBefore: {
      base64: nextBefore.toString("base64"),
      sha256: sha256(nextBefore),
    },
    promotedCurrent: {
      base64: promotedCurrent.toString("base64"),
      pageSha256: promoted.pageSha256,
      sha256: sha256(promotedCurrent),
    },
  })}\n`);
} finally {
  await rm(root, { force: true, recursive: true });
}
