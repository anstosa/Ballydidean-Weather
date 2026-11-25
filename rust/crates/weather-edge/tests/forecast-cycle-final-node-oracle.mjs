import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  AdjustmentCyclePageDiskPorts,
  AdjustmentEvidenceArchiveTransport,
  ackCyclePage,
  appendCyclePage,
  createCyclePageState,
  encodeCycleCapsule,
  finalizeCyclePages,
  runAdjustmentArchiveCommand,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const DUE_KEY = "capture/2026-10-08T06:35:00.000Z";
const GENERATION = "1791441300000";
const FIRST_ACKNOWLEDGED_AT = "2026-10-08T06:41:00.123Z";
const SECOND_ACKNOWLEDGED_AT = "2026-10-08T06:42:00.456Z";
const SEALED_AT = "2026-10-08T06:43:00.789Z";
const FINAL_ACKNOWLEDGED_AT = "2026-10-08T06:45:00.246Z";
const NEXT_DUE_KEY = "capture/2026-10-08T12:35:00.000Z";
const NEXT_GENERATION = "1791462900000";
const FIRST_PAGE_PAYLOAD = Buffer.from("scheduler-final-page-one", "utf8");
const SECOND_PAGE_PAYLOAD = Buffer.from("organic-final-page-two", "utf8");

// recursively sort the same JSON object keys before adding the source newline frame
function canonicalValue(value) {
  // preserve arrays in their exact source order while normalizing every member
  if (Array.isArray(value)) {
    return value.map(canonicalValue);
  }
  // preserve JSON scalars without coercion
  if (value === null || typeof value !== "object") {
    return value;
  }
  return Object.fromEntries(
    Object.keys(value).sort().map(
      // recursively canonicalize each lexically ordered field value
      (key) => [key, canonicalValue(value[key])],
    ),
  );
}

// encode one complete canonical source document
function canonicalBytes(value) {
  return Buffer.from(`${JSON.stringify(canonicalValue(value))}\n`, "utf8");
}

// hash exact source or persisted bytes with the Node owner
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// bind one declared producer population to its exact canonical ordered identity root
function requiredInput(identities) {
  return {
    count: identities.length,
    rootSha256: sha256(canonicalBytes(identities)),
  };
}

// return one deterministic healthy filesystem sample to every real disk-port instance
function healthyStatfs() {
  return {
    bavail: 600_000n,
    bsize: 4_096n,
    ffree: 65_536n,
  };
}

const root = await mkdtemp(join(tmpdir(), "weather-forecast-cycle-final-oracle-"));
await chmod(root, 0o700);

// capture exact stdout bytes from one actual exported operator command boundary
async function commandStdout(argv, at) {
  const chunks = [];
  await runAdjustmentArchiveCommand(argv, {
    now: () => new Date(at),
    root,
    statfs: async () => healthyStatfs(),
    stdout: {
      write: (bytes) => chunks.push(Buffer.from(bytes)),
    },
  });
  return Buffer.concat(chunks);
}

try {
  let nowAt = SEALED_AT;
  const ports = await new AdjustmentCyclePageDiskPorts({
    now: () => new Date(SEALED_AT),
    root,
    statfs: async () => healthyStatfs(),
  }).initialize();
  let state = createCyclePageState({
    dailyPageCount: 1,
    dueKey: DUE_KEY,
    generation: GENERATION,
    localDate: "2026-10-08",
  });
  state = await appendCyclePage(state, {
    payload: FIRST_PAGE_PAYLOAD,
    projections: [{
      channel: "scheduler_request",
      identitySha256: "1".repeat(64),
    }],
  }, ports);
  state = await appendCyclePage(state, {
    payload: SECOND_PAGE_PAYLOAD,
    projections: [{
      channel: "organic",
      identitySha256: "2".repeat(64),
    }],
  }, ports);
  assert.deepEqual(state.slots.map((slot) => slot.slot), ["current", "next"]);

  // retain the actual workstation checkpoint before each durable page acknowledgement
  for (const acknowledgedAt of [FIRST_ACKNOWLEDGED_AT, SECOND_ACKNOWLEDGED_AT]) {
    const current = await ports.readSlot("current");
    await ports.persistTransferAcknowledgement({
      acknowledgedAt,
      checkpointSha256: "a".repeat(64),
      contractVersion: "adjustment-archive-transfer-ack/v1",
      header: current.header,
      pageSha256: current.pageSha256,
      payloadLength: current.payload.length,
      projections: current.projections,
    });
    state = await ackCyclePage(state, { acknowledgedAt, pageSha256: current.pageSha256 }, ports);
  }
  assert.deepEqual(state.slots, []);
  assert.equal(state.acknowledgements.length, 2);

  const pages = state.pages.map(
    // project the exact payload-free page fields retained by the sealed manifest
    (page) => ({
      pageIndex: page.pageIndex,
      pageSha256: page.pageSha256,
      predecessorPageSha256: page.predecessorPageSha256,
      payloadSha256: page.payloadSha256,
      payloadOffset: page.payloadOffset,
      payloadLength: page.payloadLength,
    }),
  );
  const acknowledgementHashes = state.acknowledgements.map(
    // retain acknowledgement order from the durable page chain
    (acknowledgement) => acknowledgement.acknowledgementSha256,
  );
  const projectionIdentities = state.pages.flatMap(
    // restore the canonical sorted acknowledged projection population
    (page) => page.projectionIdentities,
  ).sort();
  const emptyRoot = requiredInput([]).rootSha256;
  const requiredInputs = {
    actual_best_match: requiredInput([]),
    native_source: requiredInput(["3".repeat(64)]),
    rain_gate_input: requiredInput([]),
    serving_evidence: requiredInput(projectionIdentities),
    shadow_body: requiredInput(["4".repeat(64)]),
    shadow_metadata: requiredInput(["5".repeat(64)]),
    target_revision: requiredInput(["6".repeat(64)]),
  };
  assert.equal(requiredInputs.actual_best_match.rootSha256, emptyRoot);
  assert.equal(requiredInputs.rain_gate_input.rootSha256, emptyRoot);

  const expectedSeal = {
    acknowledgedProjectionCount: projectionIdentities.length,
    acknowledgedProjectionRootSha256: sha256(canonicalBytes(projectionIdentities)),
    acknowledgementRootSha256: sha256(canonicalBytes(acknowledgementHashes)),
    contractVersion: "adjustment-cycle-input-seal/v1",
    dueKey: DUE_KEY,
    evidenceGapRootSha256: emptyRoot,
    generation: GENERATION,
    pageRootSha256: sha256(canonicalBytes(pages)),
    requiredInputs,
    sealedAt: SEALED_AT,
  };
  const transport = await new AdjustmentEvidenceArchiveTransport({
    now: () => new Date(nowAt),
    root,
    ports,
  }).initialize();
  const seal = await transport.sealCycleInputs({ dueKey: DUE_KEY, generation: GENERATION, requiredInputs });
  assert.deepEqual(seal, expectedSeal);
  assert.deepEqual(await ports.readInputSeal(DUE_KEY, GENERATION), seal);
  const runtimeFinal = await ports.findFinalManifest(DUE_KEY, GENERATION);
  assert.equal(runtimeFinal.manifest.finalizedAt, seal.sealedAt);

  const sealIdentitySha256 = sha256(canonicalBytes({
    dueKey: DUE_KEY,
    generation: GENERATION,
  }));
  const sealBytes = await readFile(join(
    root,
    "online-pages",
    "input-seals",
    `sha256-${sealIdentitySha256}.json`,
  ));
  assert.deepEqual(sealBytes, canonicalBytes(seal));

  const finalized = await finalizeCyclePages(state, {
    finalizedAt: seal.sealedAt,
    inputSeal: seal,
  }, ports);
  assert.equal(finalized.manifest.contractVersion, "adjustment-cycle-final-manifest/v2");
  assert.deepEqual(finalized.manifest.inputSeal, seal);
  assert.equal(finalized.manifestSha256, runtimeFinal.manifestSha256);
  const manifestBytes = await readFile(join(
    root,
    "online-pages",
    "final-manifests",
    `sha256-${finalized.manifestSha256}.json`,
  ));
  assert.deepEqual(manifestBytes, canonicalBytes(finalized.manifest));
  assert.equal(sha256(manifestBytes), finalized.manifestSha256);

  const found = await ports.findFinalManifest(DUE_KEY, GENERATION);
  assert.deepEqual(found, {
    manifest: finalized.manifest,
    manifestSha256: finalized.manifestSha256,
  });
  let laterAppendRefusal;
  try {
    await appendCyclePage(finalized.state, {
      payload: Buffer.from("late-page", "utf8"),
      projections: [{
        channel: "organic",
        identitySha256: "7".repeat(64),
      }],
    }, ports);
    assert.fail("finalized cycle accepted a later page");
  } catch (error) {
    // retain the exact closed refusal projected by the source protocol
    laterAppendRefusal = {
      code: error.code,
      message: error.message,
      reason: error.reason,
    };
  }
  assert.deepEqual(laterAppendRefusal, {
    code: "adjustment_page_refused",
    message: "cycle_already_finalized",
    reason: "cycle_already_finalized",
  });
  assert.deepEqual(await readdir(join(root, "online-pages", "slots")), []);

  const capsuleBytes = encodeCycleCapsule({
    manifest: finalized.manifest,
    manifestSha256: finalized.manifestSha256,
    pagePayloads: [FIRST_PAGE_PAYLOAD, SECOND_PAGE_PAYLOAD],
  });
  assert.equal(capsuleBytes.subarray(0, 8).toString("ascii"), "WXACYCL2");
  assert.equal(capsuleBytes.readUInt32BE(8), manifestBytes.byteLength);
  assert.equal(capsuleBytes.readUInt32BE(12), 2);

  let nextState = createCyclePageState({
    dailyPageCount: 2,
    dueKey: NEXT_DUE_KEY,
    generation: NEXT_GENERATION,
    localDate: "2026-10-08",
  });
  nextState = await appendCyclePage(nextState, {
    payload: Buffer.from("next-due-pending-page", "utf8"),
    projections: [{
      channel: "scheduler_request",
      identitySha256: "8".repeat(64),
    }],
  }, ports);

  const finalCommandBytes = await commandStdout(["archive-next"], nowAt);
  const finalBeforePendingPage = await transport.next();
  assert.equal(finalBeforePendingPage.kind, "final");
  assert.equal(finalBeforePendingPage.manifestSha256, finalized.manifestSha256);
  assert.deepEqual(finalBeforePendingPage.manifest, finalized.manifest);
  assert.deepEqual(finalCommandBytes, canonicalBytes(finalBeforePendingPage));

  nowAt = FINAL_ACKNOWLEDGED_AT;
  const graphSha256 = "9".repeat(64);
  const finalAcknowledgementCommandBytes = await commandStdout([
    "archive-ack-final",
    finalized.manifestSha256,
    graphSha256,
  ], nowAt);
  assert.equal(finalAcknowledgementCommandBytes.byteLength, 0);
  const finalAcknowledgement = await transport.acknowledgeFinal(
    finalized.manifestSha256,
    graphSha256,
  );
  assert.deepEqual(
    await transport.acknowledgeFinal(finalized.manifestSha256, graphSha256),
    finalAcknowledgement,
  );
  let changedGraphCollision;
  try {
    await transport.acknowledgeFinal(finalized.manifestSha256, "0".repeat(64));
    throw new Error("changed final graph unexpectedly succeeded");
  } catch (error) {
    // retain the exact first-writer collision without replacing its durable graph
    assert.match(error.message, /final acknowledgement collision/u);
    changedGraphCollision = { message: error.message };
  }
  const finalAcknowledgementBytes = await readFile(join(
    root,
    "online-pages",
    "final-acknowledgements",
    `sha256-${finalized.manifestSha256}.json`,
  ));
  assert.deepEqual(
    JSON.parse(finalAcknowledgementBytes.toString("utf8")),
    finalAcknowledgement,
  );
  assert.deepEqual(
    await ports.readFinalAcknowledgement(finalized.manifestSha256),
    finalAcknowledgement,
  );

  const pageCommandBytes = await commandStdout(["archive-next"], nowAt);
  const pageAfterFinalAcknowledgement = await transport.next();
  assert.equal(pageAfterFinalAcknowledgement.kind, "page");
  assert.equal(pageAfterFinalAcknowledgement.header.dueKey, NEXT_DUE_KEY);
  assert.equal(pageAfterFinalAcknowledgement.pageSha256, nextState.pages[0].pageSha256);
  assert.deepEqual(pageCommandBytes, canonicalBytes(pageAfterFinalAcknowledgement));
  const pageAcknowledgementCommandBytes = await commandStdout([
    "archive-ack",
    pageAfterFinalAcknowledgement.pageSha256,
    "b".repeat(64),
  ], nowAt);
  assert.equal(pageAcknowledgementCommandBytes.byteLength, 0);
  const pageAcknowledgement = await transport.acknowledge(
    pageAfterFinalAcknowledgement.pageSha256,
    "b".repeat(64),
  );
  const checkpointFilename = `sha256-${pageAfterFinalAcknowledgement.pageSha256}.json`;
  const checkpointBytes = await readFile(join(
    root,
    "online-pages",
    "archive-acknowledgements",
    checkpointFilename,
  ));
  assert.deepEqual(JSON.parse(checkpointBytes.toString("utf8")), pageAcknowledgement);
  const pageProtocolAcknowledgement = {
    acknowledgedAt: pageAcknowledgement.acknowledgedAt,
    contractVersion: "adjustment-cycle-page-ack/v1",
    dueKey: pageAcknowledgement.header.dueKey,
    generation: pageAcknowledgement.header.generation,
    pageIndex: pageAcknowledgement.header.pageIndex,
    pageSha256: pageAcknowledgement.pageSha256,
  };
  const pageProtocolAcknowledgementSha256 = sha256(
    canonicalBytes(pageProtocolAcknowledgement),
  );
  const pageProtocolAcknowledgementFilename =
    `sha256-${pageProtocolAcknowledgementSha256}.json`;
  const pageProtocolAcknowledgementBytes = await readFile(join(
    root,
    "online-pages",
    "acknowledgements",
    pageProtocolAcknowledgementFilename,
  ));
  assert.deepEqual(
    pageProtocolAcknowledgementBytes,
    canonicalBytes(pageProtocolAcknowledgement),
  );
  const idleCommandBytes = await commandStdout(["archive-next"], nowAt);
  const idleAfterPendingPage = await transport.next();
  assert.deepEqual(idleAfterPendingPage, {
    contractVersion: "adjustment-archive-transfer/v1",
    kind: "idle",
  });
  assert.deepEqual(idleCommandBytes, canonicalBytes(idleAfterPendingPage));
  assert.deepEqual(await readdir(join(root, "online-pages", "slots")), []);

  process.stdout.write(`${JSON.stringify({
    capsule: {
      base64: capsuleBytes.toString("base64"),
      byteLength: capsuleBytes.byteLength,
      sha256: sha256(capsuleBytes),
    },
    coldGraphAcknowledgement: {
      base64: finalAcknowledgementBytes.toString("base64"),
      byteLength: finalAcknowledgementBytes.byteLength,
      changedGraphCollision,
      fileSha256: sha256(finalAcknowledgementBytes),
      value: finalAcknowledgement,
    },
    commandStdout: {
      final: finalCommandBytes.toString("base64"),
      finalAcknowledgement: finalAcknowledgementCommandBytes.toString("base64"),
      idle: idleCommandBytes.toString("base64"),
      page: pageCommandBytes.toString("base64"),
      pageAcknowledgement: pageAcknowledgementCommandBytes.toString("base64"),
    },
    contractVersion: "weather-forecast-cycle-final-node-oracle/v1",
    finalizedState: finalized.state,
    identity: {
      dueKey: DUE_KEY,
      generation: GENERATION,
      inputSealSha256: sealIdentitySha256,
    },
    laterAppendRefusal,
    manifest: {
      base64: manifestBytes.toString("base64"),
      byteLength: manifestBytes.byteLength,
      sha256: finalized.manifestSha256,
      value: finalized.manifest,
    },
    pageDurability: {
      checkpoint: {
        base64: checkpointBytes.toString("base64"),
        filename: checkpointFilename,
        sha256: sha256(checkpointBytes),
        value: pageAcknowledgement,
      },
      protocolAcknowledgement: {
        base64: pageProtocolAcknowledgementBytes.toString("base64"),
        filename: pageProtocolAcknowledgementFilename,
        sha256: pageProtocolAcknowledgementSha256,
        value: pageProtocolAcknowledgement,
      },
    },
    requiredInputs,
    transferPriority: {
      finalBeforePendingPage,
      idleAfterPendingPage,
      pageAcknowledgement,
      pageAfterFinalAcknowledgement,
    },
    seal: {
      base64: sealBytes.toString("base64"),
      byteLength: sealBytes.byteLength,
      fileSha256: sha256(sealBytes),
      identitySha256: sealIdentitySha256,
      value: seal,
    },
    slotsAbsent: true,
  })}\n`);
} finally {
  await rm(root, { force: true, recursive: true });
}
