import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";

import {
  AdjustmentRevisionArchiveStore,
  readAdjustmentRevisionGapPayloadPage,
  readAdjustmentRevisionGapTransferStart,
  runAdjustmentArchiveCommand,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const PAGE_MAXIMUM_BYTES = 4_832 * 1_024;

// encode the same recursively ordered JSON bytes used by the archive source
function canonicalBytes(value) {
  // preserve scalar values directly
  if (value === null || typeof value !== "object") {
    return Buffer.from(`${JSON.stringify(value)}\n`);
  }
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

// construct one ordinary canonical staged projection of the requested size
function projection(index, paddingBytes) {
  return {
    contractVersion: "adjustment-revision-projection/v1",
    family: "shared",
    logicalKey: { index },
    logicalReceivedAt: `2026-10-08T00:0${index}:00.000Z`,
    projectionKind: index === 1 ? "actual_best_match" : "target_revision",
    rows: [{ index, padding: "x".repeat(paddingBytes) }],
    source: { index: String(index) },
    storedContentSha256: String(index).repeat(64),
  };
}

// build an actual retained permanent-gap spool through the production store
async function buildSpool(root, paddingBytes, indexes = [1, 2], classified = true) {
  const store = new AdjustmentRevisionArchiveStore({
    now: () => new Date("2026-10-08T00:30:00.000Z"),
    parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
    root,
  });
  // fill the requested online staged slots with ordinary permanent gaps
  for (const index of indexes) {
    const stage = await store.stageProjection(canonicalBytes(projection(index, paddingBytes)));
    // preserve one ordinary in-flight stage for the idle witness
    if (!classified) {
      continue;
    }
    await store.recordGap({
      logicalKeySha256: index === 1 ? "a".repeat(64) : "b".repeat(64),
      projectionIdentitySha256: stage.projectionIdentitySha256,
      projectionKind: stage.projectionKind,
      projectionSha256: stage.projectionSha256,
      reason: "database_bind_failed",
    });
  }
}

// execute the real start and page readers for one disposable production-shaped spool
async function readPage(root) {
  const start = await readAdjustmentRevisionGapTransferStart({ root });
  const page = await readAdjustmentRevisionGapPayloadPage({
    frontierSha256: start.frontierSha256,
    root,
    startSha256: start.startSha256,
  });
  return { page, start };
}

// collect the exact standard output frame from the existing archive command owner
async function runCommand(root, argv) {
  const chunks = [];
  await runAdjustmentArchiveCommand(argv, {
    root,
    stdout: {
      // capture the producer's chunks instead of writing them to the fixture channel
      write: (bytes) => chunks.push(bytes),
    },
  });
  return Buffer.concat(chunks);
}

// run the full Node operator flow in a second disposable spool
async function acknowledgementFixture(page, start) {
  const referenceRoot = await mkdtemp(join(tmpdir(), "weather-gap-command-reference-"));
  try {
    await buildSpool(referenceRoot, 32, [1]);
    const startBytes = await runCommand(referenceRoot, ["revision-gap-start-v1"]);
    assert.deepEqual(startBytes, canonicalBytes(start));
    const pageBytes = await runCommand(referenceRoot, [
      "revision-gap-page-v1", start.startSha256, start.frontierSha256,
    ]);
    assert.deepEqual(pageBytes, canonicalBytes(page));
    const graph = "d".repeat(64);
    const ackArguments = ["revision-gap-ack-v1", page.pageSha256, graph];
    const ackBytes = await runCommand(referenceRoot, ackArguments);
    assert.deepEqual(await runCommand(referenceRoot, ackArguments), ackBytes);
    assert.deepEqual(
      await readFile(join(referenceRoot, "revision-gap-transfer", "current.json")),
      ackBytes,
    );
    const ack = JSON.parse(ackBytes);
    const nextStart = JSON.parse(await runCommand(referenceRoot, ["revision-gap-start-v1"]));
    return {
      ack_at: ack.acknowledgedAt,
      ack_clock: Date.parse(ack.acknowledgedAt),
      ack_digest: ack.acknowledgementSha256,
      ack_frontier: ack.frontierSha256,
      ack_graph: graph,
      ack_bytes: ackBytes.toString("utf8").trimEnd(),
      next_start: nextStart.startSha256,
      next_start_bytes: canonicalBytes(nextStart).toString("utf8").trimEnd(),
    };
  } finally {
    await rm(referenceRoot, { force: true, recursive: true });
  }
}

// emit exact Node wire and retained source members for the caller's private fixture
async function nativeFixture(root, idle) {
  assert.match(basename(root), /^weather-native-spool-\d+-\d+-\d+$/u);
  await buildSpool(root, 32, [1], !idle);
  const { page, start } = await readPage(root);
  assert.equal(page.idle, idle);
  assert.equal(page.entries.length, idle ? 0 : 1);
  const pageBytes = canonicalBytes(page);
  const fields = {
    frontier: start.frontierSha256,
    start: start.startSha256,
    start_bytes: canonicalBytes(start).toString("utf8").trimEnd(),
    page: page.pageSha256,
    member_root: page.memberRootSha256,
    page_bytes: pageBytes.toString("utf8").trimEnd(),
  };
  // keep the exact Node reference while leaving only original sources for the Rust collector
  if (!idle) {
    const pendingPath = join(root, "revision-gap-transfer", "pending.json");
    assert.deepEqual(await readFile(pendingPath), pageBytes);
    await rm(pendingPath);
    const [entry] = page.entries;
    const key = canonicalBytes({
      logicalKeySha256: entry.gap.input.logicalKeySha256,
      projectionIdentitySha256: entry.gap.input.projectionIdentitySha256,
    });
    Object.assign(fields, {
      gap_digest: entry.gap.gapSha256,
      gap_hash: entry.gapMemberSha256,
      gap_bytes: canonicalBytes(entry.gap).toString("utf8").trimEnd(),
      gap_key: key.toString("utf8").trimEnd(),
      gap_key_hash: createHash("sha256").update(key).digest("hex"),
      payload_hash: entry.payload.identitySha256,
      payload_bytes: Buffer.from(entry.payload.bytesBase64, "base64").toString("utf8").trimEnd(),
      receipt_hash: entry.stageReceiptMemberSha256,
      receipt_bytes: canonicalBytes(entry.stageReceipt).toString("utf8").trimEnd(),
      recorded_at: entry.gap.recordedAt,
    });
    Object.assign(fields, await acknowledgementFixture(page, start));
  }
  process.stdout.write(Object.entries(fields).map(
    // export the exact newline-framed witness without requiring Rust JSON at test runtime
    ([key, value]) => `${key}=${value}\n`,
  ).join(""));
}

// preserve the standalone bounded-selection witness alongside the native fixture mode
if (process.argv[2] === "--native-fixture") {
  await nativeFixture(process.argv[3], process.argv[4] === "idle");
} else {
  const smallRoot = await mkdtemp(join(tmpdir(), "weather-rust-revision-gap-small-"));
  const boundedRoot = await mkdtemp(join(tmpdir(), "weather-rust-revision-gap-bounded-"));

  try {
    await buildSpool(smallRoot, 32);
    const small = await readPage(smallRoot);
    const smallBytes = canonicalBytes(small.page);
    assert.equal(small.start.pageCount, "0");
    assert.equal(small.page.entries.length, 2);
    assert.equal(small.page.idle, false);
    assert.ok(smallBytes.length <= PAGE_MAXIMUM_BYTES);
    assert.deepEqual(
      small.page.entries.map((entry) => entry.gap.gapSha256),
      small.page.entries.map((entry) => entry.gap.gapSha256).toSorted(),
    );
    const pending = await readAdjustmentRevisionGapPayloadPage({
      frontierSha256: small.start.frontierSha256,
      root: smallRoot,
      startSha256: small.start.startSha256,
    });
    assert.deepEqual(pending, small.page);

    await buildSpool(boundedRoot, 1_900_000);
    const bounded = await readPage(boundedRoot);
    const boundedBytes = canonicalBytes(bounded.page);
    assert.equal(bounded.page.entries.length, 1);
    assert.ok(boundedBytes.length <= PAGE_MAXIMUM_BYTES);
    assert.ok(bounded.page.entries[0].payload.bytesBase64.length > 2_000_000);

    const startIdentity = createHash("sha256").update(canonicalBytes({
      contractVersion: small.start.contractVersion,
      frontierSha256: small.start.frontierSha256,
      pageCount: small.start.pageCount,
    })).digest("hex");
    assert.equal(small.start.startSha256, startIdentity);

    process.stdout.write([
      `small_order=${small.page.entries.map((entry) => entry.gap.gapSha256).join(",")}`,
      `small_bytes=${smallBytes.length}`,
      `bounded_entries=${bounded.page.entries.length}`,
      `bounded_bytes=${boundedBytes.length}`,
      "pending_stable=true",
    ].join("\n") + "\n");
  } finally {
    await rm(smallRoot, { force: true, recursive: true });
    await rm(boundedRoot, { force: true, recursive: true });
  }
}
