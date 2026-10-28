import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import { EventEmitter } from "node:events";
import { createServer as createHttpServer } from "node:http";
import {
  chmod,
  lstat,
  mkdir,
  mkdtemp,
  open,
  readFile,
  readdir,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { createServer as createNetServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { gunzipSync, gzipSync } from "node:zlib";

import {
  ADJUSTMENT_EVIDENCE_SCHEDULER_REQUIRED_FREE_BYTES,
  ADJUSTMENT_ARCHIVE_TRANSFER_CONTRACT_VERSION,
  AdjustmentCyclePageDiskPorts,
  AdjustmentDevelopmentCustodyAnchorStore,
  AdjustmentEvidenceArchiveTransport,
  AdjustmentEvidenceScheduler,
  AdjustmentEvidenceStore,
  AdjustmentFutureOnlyInputSealStore,
  AdjustmentMaintenanceAnchorStore,
  AdjustmentRainControlCustodyAnchorStore,
  AdjustmentUnsupportedTerminalProofStore,
  AdjustmentRevisionArchiveStore,
  AdjustmentShadowMetadataCustodyProofStore,
  acknowledgeAdjustmentRevisionColdPage,
  acknowledgeAdjustmentRevisionColdCustodyCheckpoint,
  buildAdjustmentRevisionColdGraphSegment,
  buildAdjustmentRevisionGapGraphSegment,
  createAdjustmentEvidenceCaptureV2,
  createAdjustmentEvidenceCaptureFromForecast,
  createAdjustmentEvidenceCaptureV2FromForecast,
  createAdjustmentRevisionArchiveHandler,
  currentAdjustmentEvidenceDue,
  evaluateAdjustmentEvidenceSchedulerAdmission,
  createAdjustmentEvidenceCapture,
  freezeAdjustmentEvidenceSnapshot,
  acknowledgeAdjustmentRevisionGapPayload,
  deriveAdjustmentRevisionCatalogFrontier,
  initializeAdjustmentRevisionColdArchiveRoot,
  readAdjustmentRevisionColdPage,
  readAdjustmentRevisionColdTransferStart,
  readAdjustmentRevisionGapPayloadPage,
  readAdjustmentRevisionGapTransferStart,
  readAdjustmentRevisionCaptureEpochWitness,
  readAdjustmentRevisionCaptureEpochSnapshot,
  validateAdjustmentRevisionColdPage,
  validateAdjustmentRevisionColdCustodyAcknowledgement,
  validateAdjustmentRevisionColdCustodyAcknowledgementV2,
  validateAdjustmentRevisionColdTransferStart,
  validateAdjustmentRevisionCaptureEpochWitness,
  validateAdjustmentRevisionGapPayloadPage,
  validateAdjustmentRevisionGapPayloadAcknowledgement,
  validateAdjustmentRevisionGapTransferStart,
  validateAdjustmentRevisionServingSnapshot,
  validateAdjustmentShadowMetadataCustodyProof,
  validateAdjustmentShadowMetadataCustodyPreparation,
  validateAdjustmentShadowMetadataCustodyStatus,
  validateAdjustmentShadowMetadataCustodyConsumption,
  validateAdjustmentFutureOnlyInputSeal,
  validateAdjustmentFutureOnlyInputSealCurrent,
  validateAdjustmentFutureOnlyInputSealInstallation,
  validateAdjustmentDevelopmentCustodyAnchorInstallation,
  validateAdjustmentDevelopmentCustodyAnchorCurrent,
  validateAdjustmentDevelopmentCustodyAnchorV1,
  validateAdjustmentRainControlCustodyAnchorCurrent,
  validateAdjustmentRainControlCustodyAnchorInstallation,
  validateAdjustmentRainControlCustodyAnchorV1,
  validateAdjustmentUnsupportedTerminalProofCurrent,
  validateAdjustmentUnsupportedTerminalProofInstallation,
  validateAdjustmentUnsupportedTerminalProofV1,
  validateAdjustmentMaintenanceAnchorV3,
  validateAdjustmentMaintenanceAnchorCurrentV3,
  validateAdjustmentMaintenanceAnchorInstallationV3,
  validateAdjustmentMaintenanceAnchorFinalizationV3,
  validateAdjustmentMaintenanceFinalizationProofV3,
  verifyAdjustmentRevisionColdCatalog,
  writeAdjustmentRevisionCaptureEpochWitness,
  normalizeAdjustmentEvidenceWindow,
  runAdjustmentArchiveCommand,
} from "../scripts/adjustment-evidence-store.mjs";
import {
  ackCyclePage,
  appendCyclePage,
  appendCyclePageFailOpen,
  createCyclePageState,
} from "../../scripts/research/adjustment_cycle_pages.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const ROLLING_CAPTURE_END = "2028-10-08T07:00:00.000Z";

// project one consistent authenticated rolling schedule head for scheduler tests
function rollingScheduleSlots(horizonEndAt = ROLLING_CAPTURE_END) {
  return ["temperature", "wind", "rain"].map(
    // retain identical global horizon authority across every family projection
    (family) => ({
      contractVersion: "adjustment-shadow-registration-slot/v3",
      epochWitnessSha256: "a".repeat(64),
      family,
      horizonEndAt,
      registrationSha256: null,
      scheduleContractSha256:
        "7c17f5d1a8e8249cd0aa4820638169e51f6edb3433017f50ab4c959e44c62f1f",
      state: "free",
      terminalAt: null,
    }),
  );
}

const hashes = {
  rain: "a".repeat(64),
  temperature: "b".repeat(64),
  temperatureAuthorization: "c".repeat(64),
  wind: "d".repeat(64),
  windAuthorization: "e".repeat(64),
  windCandidate: "f".repeat(64),
};

// prove a root-authorized initializer enables the genuine empty cold start
test("revision cold archive initializer creates only fixed empty reader directories", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-revision-empty-cold-root-"));

  try {
    await chmod(root, 0o700);
    await initializeAdjustmentRevisionColdArchiveRoot({
      expectedGid: process.getegid(),
      expectedUid: process.geteuid(),
      root,
    });
    assert.deepEqual((await readdir(root)).sort(), [
      "rain-control-state-stage-receipts",
      "rain-control-states",
      "revision-commit-receipts",
      "revision-frontier-successors",
      "revision-projections",
      "shadow-revision-capsules",
    ]);
    const genesis = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    const snapshot = {
      archiveCommitOrdinal: "0",
      contractVersion: "adjustment-revision-serving-snapshot/v1",
      cutoffAt: "2026-10-08T01:10:00.000Z",
      entries: [],
      entryCount: 0,
      frontierSha256: genesis,
      snapshotSha256: "",
    };
    snapshot.snapshotSha256 = createHash("sha256").update([
      snapshot.contractVersion,
      snapshot.cutoffAt,
      snapshot.archiveCommitOrdinal,
      snapshot.frontierSha256,
      "",
    ].join("\n")).digest("hex");
    const start = await readAdjustmentRevisionColdTransferStart({
      root,
      servingSnapshot: snapshot,
    });
    assert.equal(start.watermarkArchiveCommitOrdinal, "0");
    assert.equal(start.watermarkFrontierSha256, genesis);

    // refuse adoption of a foreign existing evidence root
    const foreign = join(root, "foreign");
    await mkdir(foreign, { mode: 0o700 });
    await assert.rejects(initializeAdjustmentRevisionColdArchiveRoot({
      expectedGid: process.getegid(),
      expectedUid: process.geteuid() + 1,
      root: foreign,
    }), /owned private directory/u);
    assert.deepEqual(await readdir(foreign), []);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// exercise the closed revision archive stage, publish, retry, and gap boundary
test("revision archive binds canonical bodies to database receipts and permanent gaps", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-revision-archive-"));
  const clocks = [
    new Date("2026-10-08T01:00:00.000Z"),
    new Date("2026-10-08T01:01:00.000Z"),
    new Date("2026-10-08T01:02:00.000Z"),
  ];
  try {
    const store = new AdjustmentRevisionArchiveStore({
      now: () => clocks.shift() ?? new Date("2026-10-08T01:03:00.000Z"),
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
    });
    const handler = createAdjustmentRevisionArchiveHandler({ store });
    const projection = {
      contractVersion: "adjustment-revision-projection/v1",
      family: "shared",
      logicalKey: {
        productRunAt: "2026-10-08T00:00:00.000Z",
        sourceId: "1",
        sourceKind: "forecast",
        validAt: "2026-10-08T01:00:00.000Z",
      },
      logicalReceivedAt: "2026-10-08T00:05:00.000Z",
      projectionKind: "actual_best_match",
      rows: [{ contentSha256: "1".repeat(64), validAt: "2026-10-08T01:00:00.000Z" }],
      source: { sourceId: "1" },
      storedContentSha256: "1".repeat(64),
    };
    const projectionBytes = canonicalBytes(projection);
    const stage = await handler("/internal/adjustment-maintenance/revision/stage", {
      projectionBase64: projectionBytes.toString("base64"),
    });
    const stageRetry = await handler("/internal/adjustment-maintenance/revision/stage", {
      projectionBase64: projectionBytes.toString("base64"),
    });
    assert.deepEqual(stageRetry, stage);
    assert.equal(stage.projectionIdentitySha256, createHash("sha256")
      .update(projectionBytes).digest("hex"));
    assert.equal(stage.projectionSha256, stage.projectionIdentitySha256);
    const predecessorFrontierSha256 = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    const revisionReceipt = {
      archiveCommitOrdinal: "1",
      archiveCommittedAt: "2026-10-08T01:00:30.000Z",
      contractVersion: "adjustment-revision-commit-receipt/v1",
      frontierSha256: "",
      predecessorFrontierSha256,
      projectionIdentitySha256: stage.projectionIdentitySha256,
      projectionKind: stage.projectionKind,
      projectionSha256: stage.projectionSha256,
      receiptSha256: "",
      stageReceiptSha256: stage.stageReceiptSha256,
    };
    revisionReceipt.receiptSha256 = createHash("sha256").update([
      revisionReceipt.contractVersion,
      revisionReceipt.archiveCommitOrdinal,
      revisionReceipt.archiveCommittedAt,
      revisionReceipt.projectionKind,
      revisionReceipt.projectionIdentitySha256,
      revisionReceipt.projectionSha256,
      revisionReceipt.stageReceiptSha256,
      revisionReceipt.predecessorFrontierSha256,
    ].join("\n")).digest("hex");
    revisionReceipt.frontierSha256 = createHash("sha256").update([
      "adjustment-revision-frontier/v1",
      predecessorFrontierSha256,
      revisionReceipt.archiveCommitOrdinal,
      revisionReceipt.receiptSha256,
    ].join("\n")).digest("hex");
    const published = await handler("/internal/adjustment-maintenance/revision/publish", {
      revisionReceipt,
      stageReceipt: stage,
    });
    assert.equal(published.committed, true);
    assert.deepEqual(await handler("/internal/adjustment-maintenance/revision/publish", {
      revisionReceipt,
      stageReceipt: stage,
    }), published);
    const gap = await handler("/internal/adjustment-maintenance/revision/gap", {
      logicalKeySha256: "8".repeat(64),
      projectionIdentitySha256: null,
      projectionKind: "target_revision",
      projectionSha256: null,
      reason: "archive_stage_failed",
    });
    assert.equal(gap.qualificationDisposition, "forever_unqualified");
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// retain one complete shadow capsule and its fifth-kind database receipt before ack
test("shadow archive routes persist exact staged bytes and shadow revision receipt", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-shadow-revision-"));
  const body = Buffer.from("{\"body\":true}\n");
  const sourceProjection = Buffer.from("{\"source\":true}\n");
  const metadata = {
    bodyByteCount: body.length,
    candidateSha256: "1".repeat(64),
    dueKey: "capture/2026-10-08T00:35:00.000Z",
    inputSha256: "2".repeat(64),
    issuedAt: "2026-10-08T00:35:01.000Z",
    maxValidAt: "2026-10-08T12:00:00.000Z",
    minValidAt: "2026-10-08T01:00:00.000Z",
    predictionBodySha256: createHash("sha256").update(body).digest("hex"),
    predictionSchemaSha256: "3".repeat(64),
    predictionSha256: "4".repeat(64),
    registrationSha256: "5".repeat(64),
    rowCount: 12,
    sourceReceiptSha256: "6".repeat(64),
    sourceSha256: "7".repeat(64),
  };
  try {
    let stageValidated = false;
    let capsuleValidated = false;
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
      validateShadowCapsule: (capsule) => {
        capsuleValidated = capsule.metadata.predictionSha256 === metadata.predictionSha256;
      },
      validateShadowStage: (input) => {
        stageValidated = input.body.equals(body) &&
          input.sourceProjection.equals(sourceProjection);
      },
    });
    const handler = createAdjustmentRevisionArchiveHandler({ store });
    const sourceProjectionSha256 = createHash("sha256")
      .update(sourceProjection).digest("hex");
    const stage = await handler("/internal/adjustment-maintenance/archive/stage", {
      bodyBase64: body.toString("base64"),
      metadata,
      sourceProjectionBase64: sourceProjection.toString("base64"),
      sourceProjectionSha256,
    });
    assert.equal(stageValidated, true);
    const revisionReceipt = shadowRevisionReceipt(metadata, stage);
    const published = await handler("/internal/adjustment-maintenance/archive/publish", {
      metadata,
      revisionReceipt,
      sourceProjectionSha256,
      stageReceiptSha256: stage.stageReceiptSha256,
    });
    assert.equal(capsuleValidated, true);
    assert.equal(published.predictionSha256, metadata.predictionSha256);
    assert.deepEqual(await readdir(join(root, "shadow-revision-stages")), []);
    assert.deepEqual(await readdir(join(root, "shadow-revision-capsules")), [
      `sha256-${revisionReceipt.receiptSha256}.json`,
    ]);
    assert.deepEqual(await readdir(join(root, "revision-commit-receipts")), [
      `sha256-${revisionReceipt.frontierSha256}.json`,
    ]);
    const gap = await handler("/internal/adjustment-maintenance/archive/gap", {
      dueKey: "capture/2026-10-08T06:35:00.000Z",
      family: "wind",
      reason: "comparator_unavailable",
      registrationSha256: "8".repeat(64),
    });
    assert.equal(gap.qualificationDisposition, "forever_unqualified");
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// retain one actual incumbent comparator as a separately crosslinked cold member
test("shadow v2 capsule binds and archives exact incumbent comparator bytes", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-shadow-comparator-"));
  const body = Buffer.from("{\"body\":true}\n");
  const comparator = Buffer.from("{\"comparator\":true}\n");
  const sourceProjection = Buffer.from("{\"source\":true}\n");
  const metadata = shadowMetadataFixture(body);
  try {
    let stageValidated = false;
    let capsuleValidated = false;
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
      validateShadowCapsule: (capsule) => {
        capsuleValidated = Buffer.from(capsule.comparatorBase64, "base64")
          .equals(comparator);
      },
      validateShadowStage: (input) => {
        stageValidated = input.body.equals(body) &&
          input.comparator.equals(comparator) &&
          input.sourceProjection.equals(sourceProjection);
      },
    });
    const handler = createAdjustmentRevisionArchiveHandler({ store });
    const sourceProjectionSha256 = createHash("sha256")
      .update(sourceProjection).digest("hex");
    const stageInput = {
      bodyBase64: body.toString("base64"),
      comparatorBase64: comparator.toString("base64"),
      metadata,
      sourceProjectionBase64: sourceProjection.toString("base64"),
      sourceProjectionSha256,
    };
    const stage = await handler("/internal/adjustment-maintenance/archive/stage",
      stageInput);
    assert.equal(stageValidated, true);
    assert.equal(stage.contractVersion, "adjustment-shadow-stage-receipt/v2");
    assert.equal(stage.comparatorSha256,
      createHash("sha256").update(comparator).digest("hex"));
    const revisionReceipt = shadowRevisionReceipt(metadata, stage);
    const published = await handler("/internal/adjustment-maintenance/archive/publish", {
      ...stageInput,
      predictionCommittedAt: "2026-10-08T00:35:30.000Z",
      revisionReceipt,
      stageReceipt: stage,
    });
    assert.equal(capsuleValidated, true);
    assert.equal(published.contractVersion, "adjustment-shadow-publish-receipt/v2");
    assert.equal(published.comparatorSha256, stage.comparatorSha256);
    const snapshot = revisionServingSnapshot([revisionReceipt]);
    snapshot.entries = [];
    snapshot.entryCount = 0;
    snapshot.snapshotSha256 = createHash("sha256").update([
      snapshot.contractVersion,
      snapshot.cutoffAt,
      snapshot.archiveCommitOrdinal,
      snapshot.frontierSha256,
      "",
    ].join("\n")).digest("hex");
    const start = await readAdjustmentRevisionColdTransferStart({
      root,
      servingSnapshot: snapshot,
    });
    const page = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: "0",
      afterFrontierSha256: revisionReceipt.predecessorFrontierSha256,
      previousPageSha256: start.startSha256,
      root,
      startSha256: start.startSha256,
      watermarkArchiveCommitOrdinal: start.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: start.watermarkFrontierSha256,
    });
    assert.equal(page.entries[0].payload.kind,
      "adjustment-shadow-revision-capsule/v2");
    const graph = buildAdjustmentRevisionColdGraphSegment({ page, start });
    const comparatorMember = graph.members.find(
      // select only the actual semantic comparator member
      (member) => member.kind === "adjustment-shadow-incumbent-comparator/v1",
    );
    assert.equal(comparatorMember.identitySha256, stage.comparatorSha256);
    assert.deepEqual(comparatorMember.payload, comparator);
    assert.ok(graph.crossLinks.some((link) =>
      link.fromIdentitySha256 === revisionReceipt.receiptSha256 &&
      link.relation === "binds_incumbent_comparator" &&
      link.toIdentitySha256 === stage.comparatorSha256));
    const checkpoint = JSON.parse(graph.members[0].payload.toString("utf8"));
    assert.equal(checkpoint.entries[0].comparatorMemberSha256,
      stage.comparatorSha256);
    const custodyInput = revisionCustodyCheckpointInput({
      custodyCheckpointSha256: "9".repeat(64),
      memberRootSha256: checkpoint.memberRootSha256,
      page,
      root,
      startMemberSha256: checkpoint.startMemberSha256,
    });
    const custody = await acknowledgeAdjustmentRevisionColdCustodyCheckpoint(
      custodyInput,
    );
    assert.equal(custody.memberRootSha256, checkpoint.memberRootSha256);
    const metadataProofStore = new AdjustmentShadowMetadataCustodyProofStore({
      now: () => new Date("2026-10-08T00:37:00.000Z"),
      root,
    });
    const retainedProof = await metadataProofStore.readCurrent();
    assert.equal(validateAdjustmentShadowMetadataCustodyProof(
      retainedProof.proof,
    ), retainedProof.proof);
    assert.equal(retainedProof.proof.authority, "shadow_metadata_custody_only");
    assert.equal(retainedProof.proof.entries[0].predictionCommittedAt,
      "2026-10-08T00:35:30.000Z");
    const alternateUnsigned = {
      ...custody,
      acknowledgedAt: "2026-10-08T00:36:01.000Z",
    };
    delete alternateUnsigned.acknowledgementSha256;
    await assert.rejects(metadataProofStore.prepare(page, {
      ...alternateUnsigned,
      acknowledgementSha256: createHash("sha256")
        .update(canonicalBytes(alternateUnsigned)).digest("hex"),
    }), (error) => error.code === "adjustment_shadow_metadata_custody_pending");
    const manifestSha256 = "a".repeat(64);
    const previousRootSha256 = "b".repeat(64);
    const coldCommitSha256 = retainedProof.proof.custodyCheckpointSha256;
    const maintenanceAnchorSha256 = retainedProof.proofSha256;
    const predictionCommittedAt = retainedProof.proof.entries[0].predictionCommittedAt;
    const newMetadataRootSha256 = createHash("sha256").update([
      previousRootSha256,
      manifestSha256,
      "1",
      "1",
      predictionCommittedAt,
      coldCommitSha256,
      maintenanceAnchorSha256,
    ].join("\n")).digest("hex");
    const preparationInput = {
      finalizations: [{
        coldCommitSha256,
        expectedPreviousMetadataRootSha256: previousRootSha256,
        finalDisposition: "retain",
        fromCommittedAt: predictionCommittedAt,
        generation: 1,
        maintenanceAnchorSha256,
        metadataManifestSha256: manifestSha256,
        newMetadataRootSha256,
        registrationSha256: metadata.registrationSha256,
        rowCount: 1,
        throughCommittedAt: predictionCommittedAt,
      }],
      proofSha256: retainedProof.proofSha256,
    };
    const preparationOutput = [];
    await runAdjustmentArchiveCommand([
      "shadow-metadata-custody-prepare-v1",
      retainedProof.proofSha256,
    ], {
      now: () => new Date("2026-10-08T00:37:00.000Z"),
      root,
      stdinBytes: canonicalBytes(preparationInput),
      stdout: { write: (bytes) => preparationOutput.push(Buffer.from(bytes)) },
    });
    const preparation = validateAdjustmentShadowMetadataCustodyPreparation(
      JSON.parse(Buffer.concat(preparationOutput).toString("utf8")),
    );
    const restartedProofStore = new AdjustmentShadowMetadataCustodyProofStore({
      now: () => new Date("2026-10-08T00:38:00.000Z"),
      root,
    });
    const durableStatus = validateAdjustmentShadowMetadataCustodyStatus(
      await restartedProofStore.readStatus(),
    );
    assert.equal(durableStatus.preparation.preparationSha256,
      preparation.preparationSha256);
    assert.deepEqual(await restartedProofStore.prepareFinalizations(preparationInput),
      preparation);
    const consumeInput = {
      preparationSha256: preparation.preparationSha256,
      proofSha256: retainedProof.proofSha256,
      results: [{
        generation: 1,
        metadataManifestSha256: manifestSha256,
        newMetadataRootSha256,
        registrationSha256: metadata.registrationSha256,
        status: "already_finalized",
      }],
    };
    await assert.rejects(restartedProofStore.consume({
      ...consumeInput,
      results: [{
        ...consumeInput.results[0],
        metadataManifestSha256: "c".repeat(64),
      }],
    }), /result set differs/u);
    const consumptionOutput = [];
    await runAdjustmentArchiveCommand([
      "shadow-metadata-custody-consume-v1",
      preparation.preparationSha256,
    ], {
      now: () => new Date("2026-10-08T00:38:00.000Z"),
      root,
      stdinBytes: canonicalBytes(consumeInput),
      stdout: { write: (bytes) => consumptionOutput.push(Buffer.from(bytes)) },
    });
    const consumed = validateAdjustmentShadowMetadataCustodyConsumption(
      JSON.parse(Buffer.concat(consumptionOutput).toString("utf8")),
    );
    assert.equal(consumed.state, "consumed");
    assert.deepEqual(await restartedProofStore.consume(consumeInput), consumed);
    const statusOutput = [];
    await runAdjustmentArchiveCommand([
      "shadow-metadata-custody-status-v1",
    ], {
      root,
      stdout: { write: (bytes) => statusOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(validateAdjustmentShadowMetadataCustodyStatus(
      JSON.parse(Buffer.concat(statusOutput).toString("utf8")),
    ), {
      contractVersion: "adjustment-shadow-metadata-custody-status/v1",
      preparation: null,
      proof: null,
    });
    const consumedOutput = [];
    await runAdjustmentArchiveCommand([
      "shadow-metadata-custody-consumed-v1",
      retainedProof.proofSha256,
    ], {
      root,
      stdout: { write: (bytes) => consumedOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(validateAdjustmentShadowMetadataCustodyConsumption(
      JSON.parse(Buffer.concat(consumedOutput).toString("utf8")),
    ), consumed);
    await assert.rejects(restartedProofStore.readConsumed("f".repeat(64)),
      /consumption proof differs/u);
    assert.deepEqual(await readdir(join(root, "shadow-revision-capsules")), []);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// page only two bounded cold successors and refuse a third online value slot
test("revision cold transfer pages two exact successors within the online spool", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-revision-catalog-"));
  try {
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
    });
    const receipts = [];
    let predecessorFrontierSha256 = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    // stage and publish exactly the two admitted online payload slots
    for (const [index, projectionKind] of ["actual_best_match", "target_revision"].entries()) {
      const bytes = canonicalBytes(revisionProjection(projectionKind, index));
      const stage = await store.stageProjection(bytes);
      const receipt = revisionReceiptForStage(stage, index + 1,
        predecessorFrontierSha256);
      await store.publishRevision({ revisionReceipt: receipt, stageReceipt: stage });
      receipts.push(receipt);
      predecessorFrontierSha256 = receipt.frontierSha256;
    }
    const servingSnapshot = revisionServingSnapshot(receipts);
    const start = await readAdjustmentRevisionColdTransferStart({ root, servingSnapshot });
    assert.equal(validateAdjustmentRevisionColdTransferStart(start), start);
    const databaseEnvelopePath = join(root, "database-envelope.json");
    await writeFile(databaseEnvelopePath, JSON.stringify({
      databaseManifest: {},
      payload: servingSnapshot,
      transaction: {
        created_at_utc: "2026-10-08T01:11:00.000000Z",
        idle_in_transaction_session_timeout: "30s",
        isolation_level: "repeatable read",
        lock_timeout: "5s",
        read_only: "on",
        statement_timeout: "5min",
      },
    }));
    const piped = spawnSync("bash", ["-c",
      "cat \"$1\" | WEATHER_ADJUSTMENT_EVIDENCE_ROOT=\"$3\" MODULE_URL=\"file://$2\" node --input-type=module -e 'const module = await import(process.env.MODULE_URL); await module.runAdjustmentArchiveCommand([\"revision-cold-start-v1\"], { root: process.env.WEATHER_ADJUSTMENT_EVIDENCE_ROOT });'",
      "revision-cold-pipe", databaseEnvelopePath,
      join(repoRoot, "deploy/scripts/adjustment-evidence-store.mjs"), root]);
    assert.equal(piped.status, 0, piped.stderr.toString());
    assert.deepEqual(JSON.parse(piped.stdout.toString("utf8")), start);
    const page = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: "0",
      afterFrontierSha256: createHash("sha256")
        .update("adjustment-revision-frontier/v1\n0\n").digest("hex"),
      previousPageSha256: start.startSha256,
      root,
      startSha256: start.startSha256,
      watermarkArchiveCommitOrdinal: start.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: start.watermarkFrontierSha256,
    });
    assert.equal(validateAdjustmentRevisionColdPage(page), page);
    assert.equal(page.entries.length, 2);
    assert.equal(page.eof, true);
    assert.equal(page.nextFrontierSha256, receipts[1].frontierSha256);
    const coldSegment = buildAdjustmentRevisionColdGraphSegment({ page, start });
    assert.equal(coldSegment.members.length, 12);
    assert.equal(coldSegment.crossLinks.length, 11);
    assert.deepEqual(coldSegment.members.map((member) => member.identitySha256).sort(), [
      page.pageSha256,
      start.startSha256,
      ...page.entries.flatMap((entry) => [
        entry.payload.identitySha256,
        entry.publication.value.publishReceiptSha256,
        entry.receipt.receiptSha256,
        entry.stageReceipt.stageReceiptSha256,
        createHash("sha256").update(canonicalBytes(entry.successor)).digest("hex"),
      ]),
    ].sort());
    let spoolFileCount = 0;
    let spoolAllocatedBytes = 0n;
    // measure every successful-slot file retained until exact cold retirement
    for (const directory of [
      "revision-commit-receipts", "revision-frontier-successors",
      "revision-projections", "revision-publish-receipts", "revision-stage-receipts",
    ]) {
      const names = await readdir(join(root, directory));
      spoolFileCount += names.length;
      // charge actual filesystem blocks instead of logical byte estimates
      for (const name of names) {
        const details = await lstat(join(root, directory, name), { bigint: true });
        spoolAllocatedBytes += details.blocks * 512n;
      }
    }
    assert.equal(spoolFileCount, 10);
    assert.ok(spoolAllocatedBytes <= 4_832n * 1_024n + 10n * 4_096n);

    const third = canonicalBytes(revisionProjection("actual_best_match", 2));
    await assert.rejects(
      store.stageProjection(third),
      /online spool is full/u,
    );
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// prove custody durability precedes exact retirement and survives one crash seam
test("revision custody acknowledgement resumes exact retirement without qualification authority", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-revision-custody-"));
  try {
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
    });
    const genesis = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    const firstBytes = canonicalBytes(revisionProjection("actual_best_match", 0));
    const firstStage = await store.stageProjection(firstBytes);
    const firstReceipt = revisionReceiptForStage(firstStage, 1, genesis);
    await store.publishRevision({ revisionReceipt: firstReceipt, stageReceipt: firstStage });
    const firstStart = await readAdjustmentRevisionColdTransferStart({
      root,
      servingSnapshot: revisionServingSnapshot([firstReceipt]),
    });
    const firstPage = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: "0",
      afterFrontierSha256: genesis,
      previousPageSha256: firstStart.startSha256,
      root,
      startSha256: firstStart.startSha256,
      watermarkArchiveCommitOrdinal: firstStart.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: firstStart.watermarkFrontierSha256,
    });
    const firstGraph = buildAdjustmentRevisionColdGraphSegment({
      page: firstPage,
      start: firstStart,
    });
    const firstCheckpoint = JSON.parse(firstGraph.members[0].payload.toString("utf8"));
    const firstInput = revisionCustodyInput({
      graphManifestSha256: "a".repeat(64),
      memberRootSha256: firstCheckpoint.memberRootSha256,
      page: firstPage,
      root,
      startMemberSha256: firstCheckpoint.startMemberSha256,
    });
    await assert.rejects(acknowledgeAdjustmentRevisionColdPage(firstInput, {
      // fail only after the custody checkpoint is durable
      afterDurableAcknowledgement: async () => {
        throw new Error("injected post-ack crash");
      },
      now: () => new Date("2026-10-08T01:12:00.000Z"),
    }), /post-ack crash/u);
    assert.equal((await readdir(join(root, "revision-commit-receipts"))).length, 1);
    const resumed = await acknowledgeAdjustmentRevisionColdPage(firstInput);
    assert.equal(validateAdjustmentRevisionColdCustodyAcknowledgement(resumed), resumed);
    assert.equal(resumed.authority, "cold_custody_only");
    assert.equal(Object.hasOwn(resumed, "qualified"), false);
    assert.deepEqual(await readdir(join(root, "revision-commit-receipts")), []);
    assert.deepEqual(await readdir(join(root, "revision-projections")), []);
    assert.deepEqual(await acknowledgeAdjustmentRevisionColdPage(firstInput), resumed);

    const secondBytes = canonicalBytes(revisionProjection("target_revision", 1));
    const secondStage = await store.stageProjection(secondBytes);
    const secondReceipt = revisionReceiptForStage(
      secondStage,
      2,
      firstReceipt.frontierSha256,
    );
    await store.publishRevision({ revisionReceipt: secondReceipt, stageReceipt: secondStage });
    const secondStart = await readAdjustmentRevisionColdTransferStart({
      root,
      servingSnapshot: revisionServingSnapshot([secondReceipt]),
    });
    const secondPage = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: "1",
      afterFrontierSha256: firstReceipt.frontierSha256,
      previousPageSha256: secondStart.startSha256,
      root,
      startSha256: secondStart.startSha256,
      watermarkArchiveCommitOrdinal: secondStart.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: secondStart.watermarkFrontierSha256,
    });
    const secondGraph = buildAdjustmentRevisionColdGraphSegment({
      page: secondPage,
      start: secondStart,
    });
    const secondCheckpoint = JSON.parse(secondGraph.members[0].payload.toString("utf8"));
    const secondInput = revisionCustodyCheckpointInput({
      custodyCheckpointSha256: "b".repeat(64),
      memberRootSha256: secondCheckpoint.memberRootSha256,
      page: secondPage,
      root,
      startMemberSha256: secondCheckpoint.startMemberSha256,
    });
    await writeFile(join(root, "revision-commit-receipts", "foreign.txt"), "foreign\n");
    await assert.rejects(
      acknowledgeAdjustmentRevisionColdCustodyCheckpoint(secondInput),
      /spool census/u,
    );
    await rm(join(root, "revision-commit-receipts", "foreign.txt"));
    await assert.rejects(
      acknowledgeAdjustmentRevisionColdCustodyCheckpoint(secondInput, {
        // fail only after the packed custody checkpoint is durable
        afterDurableAcknowledgement: async () => {
          throw new Error("injected packed post-ack crash");
        },
        now: () => new Date("2026-10-08T01:13:00.000Z"),
      }),
      /packed post-ack crash/u,
    );
    assert.equal((await readdir(join(root, "revision-commit-receipts"))).length, 1);
    const output = [];
    await runAdjustmentArchiveCommand([
      "revision-cold-custody-ack-v2",
      secondInput.watermarkArchiveCommitOrdinal,
      secondInput.watermarkFrontierSha256,
      secondInput.startSha256,
      secondInput.afterArchiveCommitOrdinal,
      secondInput.afterFrontierSha256,
      secondInput.previousPageSha256,
      secondInput.pageSha256,
      secondInput.custodyCheckpointSha256,
      secondInput.memberRootSha256,
      secondInput.startMemberSha256,
    ], {
      root,
      stdout: { write: (bytes) => output.push(Buffer.from(bytes)) },
    });
    const secondAcknowledgement = validateAdjustmentRevisionColdCustodyAcknowledgementV2(
      JSON.parse(Buffer.concat(output).toString("utf8")),
    );
    assert.equal(secondAcknowledgement.contractVersion,
      "adjustment-revision-custody-acknowledgement/v2");
    assert.equal(secondAcknowledgement.custodyCheckpointSha256,
      secondInput.custodyCheckpointSha256);
    assert.equal(Object.hasOwn(secondAcknowledgement, "graphManifestSha256"), false);
    assert.equal(secondAcknowledgement.nextArchiveCommitOrdinal, "2");
    assert.equal((await readdir(join(root, "revision-custody-acknowledgements"))).length, 2);
    assert.deepEqual(await readdir(join(root, "revision-commit-receipts")), []);
    await assert.rejects(
      acknowledgeAdjustmentRevisionColdCustodyCheckpoint({
        ...secondInput,
        custodyCheckpointSha256: "c".repeat(64),
      }),
      /acknowledgement collision/u,
    );
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// bind future-only T and F transitions to one exact seal and packed custody page
test("future-only seal and v3 anchors preserve exact cross-plane authority", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-future-anchor-"));
  try {
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
    });
    const genesis = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    const body = canonicalBytes(revisionProjection("actual_best_match", 0));
    const stage = await store.stageProjection(body);
    const receipt = revisionReceiptForStage(stage, 1, genesis);
    await store.publishRevision({ revisionReceipt: receipt, stageReceipt: stage });
    const start = await readAdjustmentRevisionColdTransferStart({
      root,
      servingSnapshot: revisionServingSnapshot([receipt]),
    });
    const page = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: "0",
      afterFrontierSha256: genesis,
      previousPageSha256: start.startSha256,
      root,
      startSha256: start.startSha256,
      watermarkArchiveCommitOrdinal: start.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: start.watermarkFrontierSha256,
    });
    const graph = buildAdjustmentRevisionColdGraphSegment({ page, start });
    const checkpoint = JSON.parse(graph.members[0].payload.toString("utf8"));
    const custodyInput = revisionCustodyCheckpointInput({
      custodyCheckpointSha256: "9".repeat(64),
      memberRootSha256: checkpoint.memberRootSha256,
      page,
      root,
      startMemberSha256: checkpoint.startMemberSha256,
    });
    const custody = await acknowledgeAdjustmentRevisionColdCustodyCheckpoint(
      custodyInput,
      { now: () => new Date("2026-10-08T02:10:00.000Z") },
    );
    const witnessUnsigned = {
      activationKind: "inert_v14_pre_activation",
      archiveCommitOrdinal: "0",
      catalogFrontierSha256: genesis,
      contractVersion: "adjustment-revision-capture-epoch-witness/v1",
      controlPlaneSha256: "1".repeat(64),
      controlPlaneVersion: "14",
      databaseMigrationHistorySha256:
        "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
      epochAt: "2026-10-08T02:00:00.000Z",
      servingSnapshotSha256: "2".repeat(64),
      sourceCommit: "3".repeat(40),
      sourceRelease: "2026.10.08-1",
      sourceServerImageDigest: `sha256:${"4".repeat(64)}`,
      sourceWebImageDigest: `sha256:${"5".repeat(64)}`,
    };
    const witness = validateAdjustmentRevisionCaptureEpochWitness({
      ...witnessUnsigned,
      witnessSha256: createHash("sha256")
        .update(canonicalBytes(witnessUnsigned)).digest("hex"),
    });
    const developmentAnchor = validateAdjustmentDevelopmentCustodyAnchorV1({
      actionKind: "shadow",
      actionSha256: "a".repeat(64),
      archiveCommitOrdinal: custody.nextArchiveCommitOrdinal,
      artifactSha256: "b".repeat(64),
      candidateGraphSha256: "c".repeat(64),
      candidateSha256: "d".repeat(64),
      captureEpochWitnessSha256: createHash("sha256")
        .update(canonicalBytes(witness)).digest("hex"),
      contractVersion: "adjustment-development-custody-anchor/v1",
      controlSha256: "e".repeat(64),
      controlVersion: "14",
      custodyAcknowledgedAt: custody.acknowledgedAt,
      custodyAcknowledgementSha256: custody.acknowledgementSha256,
      custodyCheckpointSha256: custody.custodyCheckpointSha256,
      developmentGraphSha256: "f".repeat(64),
      dueKey: "monthly/temperature/2026-10",
      family: "temperature",
      frontierSha256: custody.nextFrontierSha256,
      fullGraphVerifiedAt: "2026-10-08T02:10:01.000Z",
      inputHeadSha256: "0".repeat(64),
      lifecycleLedgerRootSha256: "1".repeat(64),
      memberRootSha256: custody.memberRootSha256,
      pageSha256: custody.pageSha256,
      policyReportSha256: "2".repeat(64),
      predecessorAnchorSha256: null,
      registrationSha256: "4".repeat(64),
      sequence: "0",
      sourceCommit: witness.sourceCommit,
      sourceSha256: "5".repeat(64),
      startMemberSha256: custody.startMemberSha256,
      startSha256: custody.startSha256,
    });
    const developmentBytes = canonicalBytes(developmentAnchor);
    const developmentSha256 = createHash("sha256")
      .update(developmentBytes).digest("hex");
    const emptyDevelopmentOutput = [];
    await runAdjustmentArchiveCommand([
      "development-custody-anchor-current-v1",
    ], {
      root,
      stdout: { write: (bytes) => emptyDevelopmentOutput.push(Buffer.from(bytes)) },
    });
    assert.equal(Buffer.concat(emptyDevelopmentOutput).toString("utf8"), "null\n");
    const developmentOutput = [];
    await runAdjustmentArchiveCommand([
      "development-custody-anchor-install-v1",
      developmentSha256,
    ], {
      expectedControlSha256: developmentAnchor.controlSha256,
      expectedControlVersion: developmentAnchor.controlVersion,
      readCaptureEpochWitness: async () => witness,
      root,
      stdinBytes: developmentBytes,
      stdout: { write: (bytes) => developmentOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(validateAdjustmentDevelopmentCustodyAnchorInstallation(
      JSON.parse(Buffer.concat(developmentOutput).toString("utf8")),
    ), {
      anchorSha256: developmentSha256,
      contractVersion: "adjustment-development-custody-anchor-installation/v1",
      state: "installed",
    });
    const currentDevelopmentOutput = [];
    await runAdjustmentArchiveCommand([
      "development-custody-anchor-current-v1",
    ], {
      root,
      stdout: { write: (bytes) => currentDevelopmentOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(validateAdjustmentDevelopmentCustodyAnchorCurrent(
      JSON.parse(Buffer.concat(currentDevelopmentOutput).toString("utf8")),
    ), {
      anchor: developmentAnchor,
      anchorSha256: developmentSha256,
    });
    const developmentStore = new AdjustmentDevelopmentCustodyAnchorStore({
      expectedControlSha256: developmentAnchor.controlSha256,
      expectedControlVersion: developmentAnchor.controlVersion,
      readCaptureEpochWitness: async () => witness,
      root,
    });
    assert.equal((await developmentStore.readCurrent()).anchorSha256,
      developmentSha256);
    assert.equal((await developmentStore.authorizeDevelopmentShadowAction({
      actionKind: developmentAnchor.actionKind,
      actionSha256: developmentAnchor.actionSha256,
      artifactSha256: developmentAnchor.artifactSha256,
      candidateGraphSha256: developmentAnchor.candidateGraphSha256,
      candidateSha256: developmentAnchor.candidateSha256,
      family: developmentAnchor.family,
      lifecycleLedgerRootSha256: developmentAnchor.lifecycleLedgerRootSha256,
      policyReportSha256: developmentAnchor.policyReportSha256,
      registrationSha256: developmentAnchor.registrationSha256,
      sourceCommit: developmentAnchor.sourceCommit,
    })).developmentGraphSha256, developmentAnchor.developmentGraphSha256);
    await assert.rejects(developmentStore.authorizeDevelopmentShadowAction({
      actionKind: "compensate_shadow",
      actionSha256: developmentAnchor.actionSha256,
      artifactSha256: developmentAnchor.artifactSha256,
      candidateGraphSha256: developmentAnchor.candidateGraphSha256,
      candidateSha256: developmentAnchor.candidateSha256,
      family: developmentAnchor.family,
      lifecycleLedgerRootSha256: developmentAnchor.lifecycleLedgerRootSha256,
      policyReportSha256: developmentAnchor.policyReportSha256,
      registrationSha256: developmentAnchor.registrationSha256,
      sourceCommit: developmentAnchor.sourceCommit,
    }), /authorization is invalid/u);
    const successorDevelopmentAnchor = validateAdjustmentDevelopmentCustodyAnchorV1({
      ...developmentAnchor,
      fullGraphVerifiedAt: "2026-10-08T02:10:02.000Z",
      predecessorAnchorSha256: developmentSha256,
      sequence: "1",
      sourceCommit: "6".repeat(40),
    });
    const successorDevelopmentBytes = canonicalBytes(successorDevelopmentAnchor);
    const successorDevelopmentSha256 = createHash("sha256")
      .update(successorDevelopmentBytes).digest("hex");
    await developmentStore.install(successorDevelopmentBytes,
      successorDevelopmentSha256);
    assert.equal((await developmentStore.readCurrent()).anchor.sourceCommit,
      successorDevelopmentAnchor.sourceCommit);
    await assert.rejects(developmentStore.authorizeDevelopmentShadowAction({
      actionKind: developmentAnchor.actionKind,
      actionSha256: developmentAnchor.actionSha256,
      artifactSha256: developmentAnchor.artifactSha256,
      candidateGraphSha256: developmentAnchor.candidateGraphSha256,
      candidateSha256: developmentAnchor.candidateSha256,
      family: developmentAnchor.family,
      lifecycleLedgerRootSha256: developmentAnchor.lifecycleLedgerRootSha256,
      policyReportSha256: developmentAnchor.policyReportSha256,
      registrationSha256: developmentAnchor.registrationSha256,
      sourceCommit: developmentAnchor.sourceCommit,
    }), /authorization differs/u);
    const candidateSha256 = "6".repeat(64);
    const emptyRootSha256 = createHash("sha256").update(canonicalBytes([])).digest("hex");
    const populatedRootSha256 = createHash("sha256")
      .update(canonicalBytes(["7".repeat(64)])).digest("hex");
    const candidateRootSha256 = createHash("sha256")
      .update(canonicalBytes([candidateSha256])).digest("hex");
    const inputClasses = Object.fromEntries([
      "actual_best_match", "artifact", "candidate", "comparator", "native_source",
      "rain_gate_input", "shadow_body", "shadow_source", "target", "target_revision",
    ].map(
      // declare one exact family population without synthetic rain inputs
      (name) => [name, name === "rain_gate_input"
        ? { count: 0, rootSha256: emptyRootSha256 }
        : { count: 1, rootSha256: name === "candidate"
          ? candidateRootSha256 : populatedRootSha256 }],
    ));
    const requiredInputRootSha256 = createHash("sha256")
      .update(canonicalBytes(inputClasses)).digest("hex");
    const unsupportedProof = validateAdjustmentUnsupportedTerminalProofV1({
      actionSha256: "8".repeat(64),
      archiveCommitOrdinal: custody.nextArchiveCommitOrdinal,
      burnSha256: "9".repeat(64),
      candidateReportSha256: "a".repeat(64),
      captureEpochWitnessSha256: createHash("sha256")
        .update(canonicalBytes(witness)).digest("hex"),
      confirmationAccessSha256: "b".repeat(64),
      confirmationChunkCount: 27,
      contractVersion: "adjustment-maintenance-unsupported-terminal-proof/v1",
      controlSha256: "c".repeat(64),
      controlVersion: "14",
      custodyCheckpointSha256: custody.custodyCheckpointSha256,
      dueKey: `confirmation/temperature/${candidateSha256}`,
      eligiblePredictionSetSha256: "d".repeat(64),
      expectedKeySetSha256: "e".repeat(64),
      family: "temperature",
      finalizedAt: "2026-10-08T02:12:00.000Z",
      frontierSha256: custody.nextFrontierSha256,
      fullGraphVerifiedAt: "2026-10-08T02:11:00.000Z",
      fullMemberRootSha256: "f".repeat(64),
      graphManifestSha256: "0".repeat(64),
      inputClasses,
      lifecycleLedgerRootSha256: "1".repeat(64),
      missingClassNames: [],
      missingKeyCount: 1,
      missingKeySetSha256: "2".repeat(64),
      pageSha256: custody.pageSha256,
      policyReportSha256: "3".repeat(64),
      predecessorProofSha256: null,
      registrationSha256: "4".repeat(64),
      requiredInputRootSha256,
      sequence: "0",
      sourceCommit: witness.sourceCommit,
      unsupportedReason: "permanent_source_gap",
      workstationJournalHeadSha256: "5".repeat(64),
    });
    assert.throws(() => validateAdjustmentUnsupportedTerminalProofV1({
      ...unsupportedProof,
      missingClassNames: ["rain_gate_input"],
    }), /population differs/u);
    const rainInputClasses = Object.fromEntries(Object.entries(inputClasses).map(
      // populate every rain class while preserving one genuinely missing expected key
      ([name, entry]) => [name, name === "rain_gate_input"
        ? { count: 1, rootSha256: populatedRootSha256 }
        : entry],
    ));
    const rainRequiredInputRootSha256 = createHash("sha256")
      .update(canonicalBytes(rainInputClasses)).digest("hex");
    const rainUnsupportedProof = validateAdjustmentUnsupportedTerminalProofV1({
      ...unsupportedProof,
      confirmationChunkCount: 24,
      dueKey: `confirmation/rain/${candidateSha256}`,
      family: "rain",
      inputClasses: rainInputClasses,
      missingClassNames: [],
      requiredInputRootSha256: rainRequiredInputRootSha256,
    });
    const rainUnsupportedBytes = canonicalBytes(rainUnsupportedProof);
    const rainUnsupportedSha256 = createHash("sha256")
      .update(rainUnsupportedBytes).digest("hex");
    const rainUnsupportedRoot = join(root, "partial-rain-proof");
    await mkdir(rainUnsupportedRoot, { mode: 0o700 });
    await initializeAdjustmentRevisionColdArchiveRoot({
      expectedGid: process.getegid(),
      expectedUid: process.geteuid(),
      root: rainUnsupportedRoot,
    });
    // reproduce the fixed successful-custody roots without reader-side repair
    for (const directory of [
      "revision-custody-acknowledgements", "revision-gaps",
      "revision-publish-receipts", "revision-spool-locks",
      "revision-stage-receipts", "shadow-revision-gaps",
      "shadow-revision-publish-receipts",
    ]) {
      await mkdir(join(rainUnsupportedRoot, directory), { mode: 0o700 });
    }
    await writeFile(join(rainUnsupportedRoot,
      "revision-custody-acknowledgements/current.json"), canonicalBytes(custody), {
      mode: 0o600,
    });
    const rainUnsupportedStore = new AdjustmentUnsupportedTerminalProofStore({
      expectedControlSha256: rainUnsupportedProof.controlSha256,
      expectedControlVersion: rainUnsupportedProof.controlVersion,
      readCaptureEpochWitness: async () => witness,
      readCurrentFamily: async () => ({
        commit: rainUnsupportedProof.sourceCommit,
        contractVersion: "adjustment-family-release-current-status/v1",
        family: "rain",
      }),
      root: rainUnsupportedRoot,
    });
    await rainUnsupportedStore.install(rainUnsupportedBytes, rainUnsupportedSha256);
    assert.deepEqual(await rainUnsupportedStore.readCurrent(), {
      proof: rainUnsupportedProof,
      proofSha256: rainUnsupportedSha256,
    });
    const unsupportedBytes = canonicalBytes(unsupportedProof);
    const unsupportedSha256 = createHash("sha256")
      .update(unsupportedBytes).digest("hex");
    const emptyUnsupportedOutput = [];
    await runAdjustmentArchiveCommand([
      "unsupported-terminal-proof-current-v1",
    ], {
      root,
      stdout: { write: (bytes) => emptyUnsupportedOutput.push(Buffer.from(bytes)) },
    });
    assert.equal(Buffer.concat(emptyUnsupportedOutput).toString("utf8"), "null\n");
    const unsupportedOutput = [];
    await runAdjustmentArchiveCommand([
      "unsupported-terminal-proof-install-v1",
      unsupportedSha256,
    ], {
      expectedControlSha256: unsupportedProof.controlSha256,
      expectedControlVersion: unsupportedProof.controlVersion,
      readCaptureEpochWitness: async () => witness,
      readCurrentFamily: async () => ({
        commit: unsupportedProof.sourceCommit,
        contractVersion: "adjustment-family-release-current-status/v1",
        family: unsupportedProof.family,
      }),
      root,
      stdinBytes: unsupportedBytes,
      stdout: { write: (bytes) => unsupportedOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(validateAdjustmentUnsupportedTerminalProofInstallation(
      JSON.parse(Buffer.concat(unsupportedOutput).toString("utf8")),
    ), {
      contractVersion:
        "adjustment-maintenance-unsupported-terminal-proof-installation/v1",
      proofSha256: unsupportedSha256,
      state: "installed",
    });
    const currentUnsupportedOutput = [];
    await runAdjustmentArchiveCommand([
      "unsupported-terminal-proof-current-v1",
    ], {
      root,
      stdout: { write: (bytes) => currentUnsupportedOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(validateAdjustmentUnsupportedTerminalProofCurrent(
      JSON.parse(Buffer.concat(currentUnsupportedOutput).toString("utf8")),
    ), {
      proof: unsupportedProof,
      proofSha256: unsupportedSha256,
    });
    const unsupportedStore = new AdjustmentUnsupportedTerminalProofStore({
      expectedControlSha256: unsupportedProof.controlSha256,
      expectedControlVersion: unsupportedProof.controlVersion,
      readCaptureEpochWitness: async () => witness,
      readCurrentFamily: async () => ({
        commit: "6".repeat(40),
        contractVersion: "adjustment-family-release-current-status/v1",
        family: unsupportedProof.family,
      }),
      root,
    });
    await assert.rejects(unsupportedStore.install(unsupportedBytes,
      unsupportedSha256), /source differs/u);
    const successorUnsupportedProof = validateAdjustmentUnsupportedTerminalProofV1({
      ...unsupportedProof,
      finalizedAt: "2026-10-08T02:13:00.000Z",
      predecessorProofSha256: unsupportedSha256,
      sequence: "1",
      sourceCommit: "6".repeat(40),
    });
    const successorUnsupportedBytes = canonicalBytes(successorUnsupportedProof);
    const successorUnsupportedSha256 = createHash("sha256")
      .update(successorUnsupportedBytes).digest("hex");
    await unsupportedStore.install(successorUnsupportedBytes,
      successorUnsupportedSha256);
    assert.equal((await unsupportedStore.readCurrent()).proofSha256,
      successorUnsupportedSha256);
    assert.deepEqual((await readdir(join(root, "unsupported-terminal-proofs"))).sort(), [
      "current.json", "previous.json",
    ]);
    assert.deepEqual(JSON.parse(await readFile(join(root,
      "unsupported-terminal-proofs/previous.json"), "utf8")), unsupportedProof);
    const wrongPredecessorBytes = canonicalBytes({
      ...successorUnsupportedProof,
      finalizedAt: "2026-10-08T02:14:00.000Z",
      predecessorProofSha256: unsupportedSha256,
      sequence: "2",
    });
    await assert.rejects(unsupportedStore.install(wrongPredecessorBytes,
      createHash("sha256").update(wrongPredecessorBytes).digest("hex")),
    /predecessor is invalid/u);
    const seal = {
      archiveCommitOrdinal: custody.nextArchiveCommitOrdinal,
      burnSha256: "8".repeat(64),
      candidateArtifactRootSha256: createHash("sha256").update(canonicalBytes([
        inputClasses.candidate.rootSha256,
        inputClasses.artifact.rootSha256,
      ])).digest("hex"),
      candidateReportSha256: "a".repeat(64),
      captureEpochWitnessSha256: createHash("sha256")
        .update(canonicalBytes(witness)).digest("hex"),
      comparatorRootSha256: inputClasses.comparator.rootSha256,
      confirmationAccessSha256: "b".repeat(64),
      confirmationChunkCount: 27,
      contractVersion: "adjustment-future-only-input-seal/v2",
      custodyCheckpointSha256: custody.custodyCheckpointSha256,
      dueKey: `confirmation/temperature/${candidateSha256}`,
      family: "temperature",
      frontierSha256: custody.nextFrontierSha256,
      fullMemberCount: 1,
      fullMemberRootSha256: "c".repeat(64),
      graphManifestSha256: "d".repeat(64),
      inputClasses,
      lifecycleLedgerRootSha256: "e".repeat(64),
      pageSha256: custody.pageSha256,
      policyReportSha256: "f".repeat(64),
      predecessorSealSha256: null,
      requiredInputRootSha256,
      sealedAt: "2026-10-08T02:11:00.000Z",
      sequence: "0",
      sourceCommit: witness.sourceCommit,
      targetRootSha256: inputClasses.target.rootSha256,
      workstationJournalHeadSha256: "0".repeat(64),
    };
    assert.equal(validateAdjustmentFutureOnlyInputSeal(seal), seal);
    assert.throws(() => validateAdjustmentFutureOnlyInputSeal({
      ...seal,
      inputClasses: {
        ...inputClasses,
        comparator: { count: 0, rootSha256: emptyRootSha256 },
      },
    }), /population|roots differ/u);
    const sealBytes = canonicalBytes(seal);
    const sealSha256 = createHash("sha256").update(sealBytes).digest("hex");
    // require both predecessor readers to expose the exact null genesis marker
    for (const action of [
      "future-input-seal-current-v2", "maintenance-anchor-current-v3",
    ]) {
      const genesisOutput = [];
      await runAdjustmentArchiveCommand([action], {
        expectedControlSha256: witness.controlPlaneSha256,
        expectedControlVersion: witness.controlPlaneVersion,
        readCaptureEpochWitness: async () => witness,
        readSourceClosure: async () => true,
        root,
        stdout: { write: (bytes) => genesisOutput.push(Buffer.from(bytes)) },
      });
      assert.equal(Buffer.concat(genesisOutput).toString("utf8"), "null\n");
    }
    const sealOutput = [];
    await runAdjustmentArchiveCommand([
      "future-input-seal-install-v2",
      sealSha256,
    ], {
      readCaptureEpochWitness: async () => witness,
      root,
      stdinBytes: sealBytes,
      stdout: { write: (bytes) => sealOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(validateAdjustmentFutureOnlyInputSealInstallation(
      JSON.parse(Buffer.concat(sealOutput).toString("utf8")),
    ), {
      contractVersion: "adjustment-future-only-input-seal-installation/v2",
      sealSha256,
      state: "installed",
    });
    assert.equal((await new AdjustmentFutureOnlyInputSealStore({ root }).readCurrent())
      .sealSha256, sealSha256);
    const sealCurrentOutput = [];
    await runAdjustmentArchiveCommand([
      "future-input-seal-current-v2",
    ], {
      expectedControlSha256: witness.controlPlaneSha256,
      expectedControlVersion: witness.controlPlaneVersion,
      readCaptureEpochWitness: async () => witness,
      readSourceClosure: async (sourceCommit) => sourceCommit === witness.sourceCommit,
      root,
      stdout: { write: (bytes) => sealCurrentOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(validateAdjustmentFutureOnlyInputSealCurrent(
      JSON.parse(Buffer.concat(sealCurrentOutput).toString("utf8")),
    ), { seal, sealSha256 });
    await assert.rejects(new AdjustmentFutureOnlyInputSealStore({
      expectedControlSha256: witness.controlPlaneSha256,
      expectedControlVersion: witness.controlPlaneVersion,
      readCaptureEpochWitness: async () => witness,
      readSourceClosure: async () => false,
      root,
    }).readCurrentVerified(), /source closure differs/u);
    const anchor = validateAdjustmentMaintenanceAnchorV3({
      actionSha256: "1".repeat(64),
      archiveCommitOrdinal: seal.archiveCommitOrdinal,
      burnSha256: seal.burnSha256,
      candidateArtifactRootSha256: seal.candidateArtifactRootSha256,
      candidateReportSha256: seal.candidateReportSha256,
      captureEpochWitnessSha256: seal.captureEpochWitnessSha256,
      confirmationAccessSha256: seal.confirmationAccessSha256,
      confirmationChunkCount: seal.confirmationChunkCount,
      contractVersion: "adjustment-maintenance-anchor/v3",
      controlSha256: witness.controlPlaneSha256,
      controlVersion: "14",
      ctfState: "transferred",
      custodyCheckpointSha256: seal.custodyCheckpointSha256,
      dueKey: seal.dueKey,
      family: seal.family,
      frontierSha256: seal.frontierSha256,
      fullGraphVerifiedAt: "2026-10-08T02:12:00.000Z",
      fullMemberRootSha256: seal.fullMemberRootSha256,
      graphManifestSha256: seal.graphManifestSha256,
      inputSealSha256: sealSha256,
      lifecycleLedgerRootSha256: seal.lifecycleLedgerRootSha256,
      pageSha256: seal.pageSha256,
      policyReportSha256: seal.policyReportSha256,
      predecessorAnchorSha256: null,
      publishedAt: "2026-10-08T02:13:00.000Z",
      requiredInputRootSha256: seal.requiredInputRootSha256,
      sequence: seal.sequence,
      sourceCommit: seal.sourceCommit,
      workstationJournalHeadSha256: seal.workstationJournalHeadSha256,
    });
    const anchorBytes = canonicalBytes(anchor);
    const anchorSha256 = createHash("sha256").update(anchorBytes).digest("hex");
    const anchorOutput = [];
    await runAdjustmentArchiveCommand([
      "maintenance-anchor-install-v3",
      anchorSha256,
    ], {
      expectedControlSha256: anchor.controlSha256,
      expectedControlVersion: anchor.controlVersion,
      root,
      stdinBytes: anchorBytes,
      stdout: { write: (bytes) => anchorOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(validateAdjustmentMaintenanceAnchorInstallationV3(
      JSON.parse(Buffer.concat(anchorOutput).toString("utf8")),
    ), {
      anchorSha256,
      contractVersion: "adjustment-maintenance-anchor-installation/v3",
      state: "transferred",
    });
    await assert.rejects(runAdjustmentArchiveCommand([
      "maintenance-anchor-install-v2",
      anchorSha256,
    ], { root, stdinBytes: anchorBytes }), /requires a v2 anchor/u);
    const { contractVersion: ignoredVersion, ctfState: ignoredState, ...proofFields } = anchor;
    assert.equal(ignoredVersion, "adjustment-maintenance-anchor/v3");
    assert.equal(ignoredState, "transferred");
    const proof = validateAdjustmentMaintenanceFinalizationProofV3({
      ...proofFields,
      contractVersion: "adjustment-maintenance-finalization-proof/v3",
      finalizedAt: "2026-10-08T02:14:00.000Z",
      transferredAnchorSha256: anchorSha256,
    });
    const badProofBytes = canonicalBytes({ ...proof, policyReportSha256: "3".repeat(64) });
    await assert.rejects(runAdjustmentArchiveCommand([
      "maintenance-anchor-finalize-v3",
      createHash("sha256").update(badProofBytes).digest("hex"),
    ], { root, stdinBytes: badProofBytes }), /proof differs/u);
    const proofBytes = canonicalBytes(proof);
    const proofSha256 = createHash("sha256").update(proofBytes).digest("hex");
    const finalized = validateAdjustmentMaintenanceAnchorV3({
      ...anchor,
      ctfState: "finalized",
      finalizationProofSha256: proofSha256,
      finalizedAt: proof.finalizedAt,
    });
    await writeFile(join(root, "maintenance-anchors", "pending.json"),
      canonicalBytes(finalized), { mode: 0o600 });
    const finalOutput = [];
    await runAdjustmentArchiveCommand([
      "maintenance-anchor-finalize-v3",
      proofSha256,
    ], {
      root,
      stdinBytes: proofBytes,
      stdout: { write: (bytes) => finalOutput.push(Buffer.from(bytes)) },
    });
    const finalResponse = validateAdjustmentMaintenanceAnchorFinalizationV3(
      JSON.parse(Buffer.concat(finalOutput).toString("utf8")),
    );
    assert.equal(finalResponse.contractVersion,
      "adjustment-maintenance-anchor-finalization/v3");
    assert.equal(finalResponse.finalizationProofSha256, proofSha256);
    assert.equal(finalResponse.state, "finalized");
    assert.equal((await new AdjustmentMaintenanceAnchorStore({ root }).status())
      .slots.current.state, "finalized");
    const anchorCurrentOutput = [];
    await runAdjustmentArchiveCommand([
      "maintenance-anchor-current-v3",
    ], {
      expectedControlSha256: witness.controlPlaneSha256,
      expectedControlVersion: witness.controlPlaneVersion,
      readCaptureEpochWitness: async () => witness,
      readSourceClosure: async (sourceCommit) => sourceCommit === witness.sourceCommit,
      root,
      stdout: { write: (bytes) => anchorCurrentOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(validateAdjustmentMaintenanceAnchorCurrentV3(
      JSON.parse(Buffer.concat(anchorCurrentOutput).toString("utf8")),
    ), {
      anchor: finalized,
      anchorSha256: finalResponse.anchorSha256,
    });
    const finalizedStore = new AdjustmentMaintenanceAnchorStore({ root });
    assert.equal((await finalizedStore.authorizeFutureOnlyQualifiedAction({
      actionSha256: anchor.actionSha256,
      fullMemberRootSha256: anchor.fullMemberRootSha256,
      lifecycleLedgerRootSha256: anchor.lifecycleLedgerRootSha256,
      policyReportSha256: anchor.policyReportSha256,
      sourceCommit: anchor.sourceCommit,
    })).finalizationProofSha256, proofSha256);
    await assert.rejects(finalizedStore.authorizeFutureOnlyQualifiedAction({
      actionSha256: anchor.actionSha256,
      fullMemberRootSha256: anchor.fullMemberRootSha256,
      lifecycleLedgerRootSha256: anchor.lifecycleLedgerRootSha256,
      policyReportSha256: "4".repeat(64),
      sourceCommit: anchor.sourceCommit,
    }), /authorization differs/u);

    // replay the controller's complete C to T to F recovery after finalization
    const recoverySealStore = new AdjustmentFutureOnlyInputSealStore({
      expectedControlSha256: witness.controlPlaneSha256,
      expectedControlVersion: witness.controlPlaneVersion,
      readCaptureEpochWitness: async () => witness,
      readSourceClosure: async (sourceCommit) => sourceCommit === witness.sourceCommit,
      root,
    });
    const recoveryAnchorStore = new AdjustmentMaintenanceAnchorStore({
      expectedControlSha256: witness.controlPlaneSha256,
      expectedControlVersion: witness.controlPlaneVersion,
      readCaptureEpochWitness: async () => witness,
      readSourceClosure: async (sourceCommit) => sourceCommit === witness.sourceCommit,
      root,
    });
    assert.deepEqual(await recoverySealStore.install(sealBytes, sealSha256), {
      contractVersion: "adjustment-future-only-input-seal-installation/v2",
      sealSha256,
      state: "installed",
    });
    assert.deepEqual(await recoveryAnchorStore.installTransferred(
      anchorBytes,
      anchorSha256,
    ), {
      anchorSha256,
      contractVersion: "adjustment-maintenance-anchor-installation/v3",
      state: "transferred",
    });
    assert.deepEqual(await recoveryAnchorStore.finalizeRetirement(
      proofBytes,
      proofSha256,
    ), finalResponse);

    // reject a different T even when it retains the same active seal
    const differentAnchorBytes = canonicalBytes({
      ...anchor,
      actionSha256: "4".repeat(64),
    });
    await assert.rejects(recoveryAnchorStore.installTransferred(
      differentAnchorBytes,
      createHash("sha256").update(differentAnchorBytes).digest("hex"),
    ), /predecessor is invalid/u);

    // reject F bytes whose finalization identity is not derived from the supplied T
    const finalizedPath = join(root, "maintenance-anchors", "current.json");
    await writeFile(finalizedPath, canonicalBytes({
      ...finalized,
      finalizationProofSha256: "4".repeat(64),
    }), { mode: 0o600 });
    await assert.rejects(recoveryAnchorStore.installTransferred(
      anchorBytes,
      anchorSha256,
    ), /predecessor is invalid/u);
    await writeFile(finalizedPath, canonicalBytes(finalized), { mode: 0o600 });

    const verifiedSealStore = new AdjustmentFutureOnlyInputSealStore({
      expectedControlSha256: witness.controlPlaneSha256,
      expectedControlVersion: witness.controlPlaneVersion,
      readCaptureEpochWitness: async () => witness,
      readSourceClosure: async (sourceCommit) => sourceCommit === witness.sourceCommit,
      root,
    });
    const verifiedAnchorStore = new AdjustmentMaintenanceAnchorStore({
      expectedControlSha256: witness.controlPlaneSha256,
      expectedControlVersion: witness.controlPlaneVersion,
      readCaptureEpochWitness: async () => witness,
      readSourceClosure: async (sourceCommit) => sourceCommit === witness.sourceCommit,
      root,
    });
    // freeze both exact predecessor documents before advancing the seal
    const frozenPredecessorSeal = await verifiedSealStore.readCurrentVerified();
    const frozenPredecessorAnchor = await verifiedAnchorStore.readCurrentV3();
    assert.deepEqual(frozenPredecessorSeal, { seal, sealSha256 });
    assert.deepEqual(frozenPredecessorAnchor, {
      anchor: finalized,
      anchorSha256: finalResponse.anchorSha256,
    });
    assert.equal(frozenPredecessorAnchor.anchor.finalizationProofSha256,
      proofSha256);
    assert.equal(createHash("sha256").update(proofBytes).digest("hex"), proofSha256);

    const successorCandidateSha256 = "9".repeat(64);
    const successorCandidateRootSha256 = createHash("sha256")
      .update(canonicalBytes([successorCandidateSha256])).digest("hex");
    const successorInputClasses = Object.fromEntries(Object.entries(inputClasses).map(
      // replace only the next candidate population identity
      ([name, entry]) => [name, name === "candidate"
        ? { count: 1, rootSha256: successorCandidateRootSha256 }
        : entry],
    ));
    const successorRequiredInputRootSha256 = createHash("sha256")
      .update(canonicalBytes(successorInputClasses)).digest("hex");
    const successorSeal = validateAdjustmentFutureOnlyInputSeal({
      ...seal,
      burnSha256: "1".repeat(64),
      candidateArtifactRootSha256: createHash("sha256").update(canonicalBytes([
        successorInputClasses.candidate.rootSha256,
        successorInputClasses.artifact.rootSha256,
      ])).digest("hex"),
      candidateReportSha256: "2".repeat(64),
      confirmationAccessSha256: "3".repeat(64),
      dueKey: `confirmation/temperature/${successorCandidateSha256}`,
      fullMemberRootSha256: "4".repeat(64),
      graphManifestSha256: "5".repeat(64),
      inputClasses: successorInputClasses,
      lifecycleLedgerRootSha256: "6".repeat(64),
      policyReportSha256: "7".repeat(64),
      predecessorSealSha256: frozenPredecessorSeal.sealSha256,
      requiredInputRootSha256: successorRequiredInputRootSha256,
      sealedAt: "2026-10-08T02:15:00.000Z",
      sequence: "1",
      workstationJournalHeadSha256: "8".repeat(64),
    });
    const successorSealBytes = canonicalBytes(successorSeal);
    const successorSealSha256 = createHash("sha256")
      .update(successorSealBytes).digest("hex");
    await verifiedSealStore.install(successorSealBytes, successorSealSha256);

    // reproduce an interruption after seal installation and before successor T
    const restartedSealStore = new AdjustmentFutureOnlyInputSealStore({
      expectedControlSha256: witness.controlPlaneSha256,
      expectedControlVersion: witness.controlPlaneVersion,
      readCaptureEpochWitness: async () => witness,
      readSourceClosure: async (sourceCommit) => sourceCommit === witness.sourceCommit,
      root,
    });
    const restartedAnchorStore = new AdjustmentMaintenanceAnchorStore({
      expectedControlSha256: witness.controlPlaneSha256,
      expectedControlVersion: witness.controlPlaneVersion,
      readCaptureEpochWitness: async () => witness,
      readSourceClosure: async (sourceCommit) => sourceCommit === witness.sourceCommit,
      root,
    });
    assert.equal((await restartedSealStore.install(
      successorSealBytes,
      successorSealSha256,
    )).sealSha256, successorSealSha256);
    await assert.rejects(restartedAnchorStore.readCurrentV3(),
      /current seal differs/u);

    const successorAnchor = validateAdjustmentMaintenanceAnchorV3({
      ...anchor,
      actionSha256: "9".repeat(64),
      archiveCommitOrdinal: successorSeal.archiveCommitOrdinal,
      burnSha256: successorSeal.burnSha256,
      candidateArtifactRootSha256: successorSeal.candidateArtifactRootSha256,
      candidateReportSha256: successorSeal.candidateReportSha256,
      captureEpochWitnessSha256: successorSeal.captureEpochWitnessSha256,
      confirmationAccessSha256: successorSeal.confirmationAccessSha256,
      confirmationChunkCount: successorSeal.confirmationChunkCount,
      custodyCheckpointSha256: successorSeal.custodyCheckpointSha256,
      dueKey: successorSeal.dueKey,
      family: successorSeal.family,
      frontierSha256: successorSeal.frontierSha256,
      fullGraphVerifiedAt: "2026-10-08T02:16:00.000Z",
      fullMemberRootSha256: successorSeal.fullMemberRootSha256,
      graphManifestSha256: successorSeal.graphManifestSha256,
      inputSealSha256: successorSealSha256,
      lifecycleLedgerRootSha256: successorSeal.lifecycleLedgerRootSha256,
      pageSha256: successorSeal.pageSha256,
      policyReportSha256: successorSeal.policyReportSha256,
      predecessorAnchorSha256: frozenPredecessorAnchor.anchorSha256,
      publishedAt: "2026-10-08T02:17:00.000Z",
      requiredInputRootSha256: successorSeal.requiredInputRootSha256,
      sequence: successorSeal.sequence,
      sourceCommit: successorSeal.sourceCommit,
      workstationJournalHeadSha256: successorSeal.workstationJournalHeadSha256,
    });
    const successorAnchorBytes = canonicalBytes(successorAnchor);
    const successorAnchorSha256 = createHash("sha256")
      .update(successorAnchorBytes).digest("hex");
    const successorInstallation = await restartedAnchorStore.installTransferred(
      successorAnchorBytes,
      successorAnchorSha256,
    );
    assert.equal(successorInstallation.anchorSha256, successorAnchorSha256);
    assert.deepEqual(await restartedAnchorStore.installTransferred(
      successorAnchorBytes,
      successorAnchorSha256,
    ), successorInstallation);
    const {
      contractVersion: ignoredSuccessorVersion,
      ctfState: ignoredSuccessorState,
      ...successorProofFields
    } = successorAnchor;
    assert.equal(ignoredSuccessorVersion, "adjustment-maintenance-anchor/v3");
    assert.equal(ignoredSuccessorState, "transferred");
    const successorProof = validateAdjustmentMaintenanceFinalizationProofV3({
      ...successorProofFields,
      contractVersion: "adjustment-maintenance-finalization-proof/v3",
      finalizedAt: "2026-10-08T02:18:00.000Z",
      transferredAnchorSha256: successorAnchorSha256,
    });
    const successorProofBytes = canonicalBytes(successorProof);
    const successorProofSha256 = createHash("sha256")
      .update(successorProofBytes).digest("hex");
    const successorFinalization = await restartedAnchorStore.finalizeRetirement(
      successorProofBytes,
      successorProofSha256,
    );
    assert.equal(successorFinalization.state, "finalized");
    assert.deepEqual(await restartedAnchorStore.finalizeRetirement(
      successorProofBytes,
      successorProofSha256,
    ), successorFinalization);
    const currentSuccessor = await restartedAnchorStore.readCurrentV3();
    assert.equal(currentSuccessor.anchor.sequence, "1");
    assert.equal(currentSuccessor.anchor.predecessorAnchorSha256,
      frozenPredecessorAnchor.anchorSha256);
    assert.equal(currentSuccessor.anchor.finalizationProofSha256,
      successorProofSha256);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// retain one grouped body while preserving every database receipt in the global chain
test("revision batch transfer and custody keep one body atomic across all receipts", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-revision-batch-"));
  try {
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
    });
    const genesis = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    const body = canonicalBytes(revisionBatchProjection("actual_best_match", 3));
    const stage = await store.stageProjection(body);
    const receipts = [];
    let predecessor = genesis;
    // issue every row receipt as one direct global-frontier chain
    for (let index = 0; index < 3; index += 1) {
      const receipt = revisionReceiptForStage(stage, index + 1, predecessor);
      receipts.push(receipt);
      predecessor = receipt.frontierSha256;
    }
    const publication = await store.publishRevision({
      revisionReceipts: receipts,
      stageReceipt: stage,
    });
    assert.equal(publication.contractVersion,
      "adjustment-revision-batch-publish-receipt/v2");
    assert.deepEqual(publication.revisionReceiptSha256s,
      receipts.map((receipt) => receipt.receiptSha256));
    assert.deepEqual(await readdir(join(root, "revision-commit-receipts")), [
      `batch-sha256-${stage.projectionIdentitySha256}.json`,
    ]);
    assert.deepEqual(await readdir(join(root, "revision-frontier-successors")), []);

    const servingSnapshot = revisionServingSnapshot(receipts);
    const start = await readAdjustmentRevisionColdTransferStart({ root, servingSnapshot });
    await assert.rejects(readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: "0",
      afterFrontierSha256: genesis,
      previousPageSha256: start.startSha256,
      root,
      startSha256: start.startSha256,
      watermarkArchiveCommitOrdinal: "2",
      watermarkFrontierSha256: receipts[1].frontierSha256,
    }), /whole direct group|cold successor differs/u);
    const page = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: "0",
      afterFrontierSha256: genesis,
      previousPageSha256: start.startSha256,
      root,
      startSha256: start.startSha256,
      watermarkArchiveCommitOrdinal: start.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: start.watermarkFrontierSha256,
    });
    assert.equal(page.entries.length, 1);
    assert.equal(page.entries[0].receipts.length, 3);
    assert.equal(page.entries[0].successors.length, 3);
    assert.equal(page.entries[0].payload.kind,
      "adjustment-revision-batch-projection/v2");
    assert.equal(page.nextArchiveCommitOrdinal, "3");
    assert.equal(page.eof, true);
    const graph = buildAdjustmentRevisionColdGraphSegment({ page, start });
    assert.equal(graph.members.length, 11);
    assert.equal(graph.crossLinks.length, 16);
    assert.equal(graph.members.filter((member) =>
      member.identitySha256 === page.entries[0].payload.identitySha256).length, 1);
    assert.equal(graph.members.filter((member) =>
      member.identitySha256 === page.entries[0].stageReceipt.stageReceiptSha256).length, 1);
    assert.equal(graph.members.filter((member) =>
      member.identitySha256 === publication.publishReceiptSha256).length, 1);

    const checkpoint = JSON.parse(graph.members[0].payload.toString("utf8"));
    const custodyInput = revisionCustodyInput({
      graphManifestSha256: "c".repeat(64),
      memberRootSha256: checkpoint.memberRootSha256,
      page,
      root,
      startMemberSha256: checkpoint.startMemberSha256,
    });
    await assert.rejects(acknowledgeAdjustmentRevisionColdPage(custodyInput, {
      // fail only after the complete grouped custody checkpoint is durable
      afterDurableAcknowledgement: async () => {
        throw new Error("injected grouped post-ack crash");
      },
    }), /grouped post-ack crash/u);
    assert.equal((await readdir(join(root, "revision-commit-receipts"))).length, 1);
    const acknowledgement = await acknowledgeAdjustmentRevisionColdPage(custodyInput);
    assert.equal(acknowledgement.retirementEntries.length, 4);
    assert.equal(acknowledgement.retirementEntries.some(
      (entry) => entry.kind === "revision_commit_group"), true);
    assert.deepEqual(await readdir(join(root, "revision-commit-receipts")), []);
    assert.deepEqual(await readdir(join(root, "revision-projections")), []);
    assert.deepEqual(await readdir(join(root, "revision-stage-receipts")), []);
    assert.deepEqual(await readdir(join(root, "revision-publish-receipts")), []);
    assert.deepEqual(await acknowledgeAdjustmentRevisionColdPage(custodyInput),
      acknowledgement);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// prove the conservative fixed-gauge body cap survives complete cold-page base64 framing
test("fixed-gauge target maximum body fits one grouped cold page", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-rain-target-cold-page-"));
  try {
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
    });
    const body = canonicalBytes({
      captureBodies: [{ padding: "x".repeat(3_450 * 1024) }],
      contractVersion: "adjustment-rain-fixed-gauge-target-projection/v1",
      family: "rain",
      logicalReceivedAt: "2026-10-08T01:00:00.000Z",
      projectionKind: "target_revision",
      rows: Array.from({ length: 12 }, (_, index) => ({ index })),
      validAt: "2026-10-08T01:00:00.000Z",
    });
    assert.ok(body.length <= 3_500 * 1024);
    const stage = await store.stageProjection(body);
    const genesis = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    const receipts = [];
    let predecessor = genesis;
    // retain all twelve row receipts under the single staged body
    for (let index = 0; index < 12; index += 1) {
      const receipt = revisionReceiptForStage(stage, index + 1, predecessor);
      receipts.push(receipt);
      predecessor = receipt.frontierSha256;
    }
    await store.publishRevision({ revisionReceipts: receipts, stageReceipt: stage });
    const servingSnapshot = revisionServingSnapshot(receipts);
    const start = await readAdjustmentRevisionColdTransferStart({ root, servingSnapshot });
    const page = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: "0",
      afterFrontierSha256: genesis,
      previousPageSha256: start.startSha256,
      root,
      startSha256: start.startSha256,
      watermarkArchiveCommitOrdinal: start.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: start.watermarkFrontierSha256,
    });
    assert.equal(page.entries[0].receipts.length, 12);
    assert.equal(page.entries[0].payload.kind,
      "adjustment-rain-fixed-gauge-target-projection/v1");
    assert.ok(canonicalBytes(page).length <= 4_832 * 1024);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// retain the additive pre-fit rain feature body as one ordinary receipt payload
test("rain feature projection transfers as one exact cold payload", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-rain-feature-revision-"));
  try {
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
    });
    const genesis = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    const projection = {
      ...revisionProjection("rain_gate_input", 0),
      contractVersion: "adjustment-rain-gate-feature-projection/v2",
      family: "rain",
    };
    const stage = await store.stageProjection(canonicalBytes(projection));
    const receipt = revisionReceiptForStage(stage, 1, genesis);
    await store.publishRevision({ revisionReceipt: receipt, stageReceipt: stage });
    const start = await readAdjustmentRevisionColdTransferStart({
      root,
      servingSnapshot: revisionServingSnapshot([receipt]),
    });
    const page = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: "0",
      afterFrontierSha256: genesis,
      previousPageSha256: start.startSha256,
      root,
      startSha256: start.startSha256,
      watermarkArchiveCommitOrdinal: start.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: start.watermarkFrontierSha256,
    });
    assert.equal(page.entries[0].payload.kind,
      "adjustment-rain-gate-feature-projection/v2");
    assert.equal(buildAdjustmentRevisionColdGraphSegment({ page, start })
      .members.some((member) => member.kind ===
        "adjustment-rain-gate-feature-projection/v2"), true);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// carry one whole-byte-addressed rain state through route, control body and custody
test("rain control state route binds exact bytes through cold custody retirement", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-rain-control-revision-"));
  try {
    const ordinalArtifactSha256 = "c".repeat(64);
    const state = {
      contractVersion: "rain-maintenance-control-state/v1",
      ordinalArtifactSha256,
      stateSha256: "d".repeat(64),
    };
    const stateBytes = canonicalBytes(state);
    const controlStateSha256 = createHash("sha256").update(stateBytes).digest("hex");
    assert.notEqual(controlStateSha256, state.stateSha256);
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      parseRainControlState: (bytes) => {
        const value = JSON.parse(bytes.toString("utf8"));
        assert.deepEqual(bytes, canonicalBytes(value));
        return value;
      },
      root,
    });
    const handler = createAdjustmentRevisionArchiveHandler({ store });
    const stateReceipt = await handler(
      "/internal/adjustment-maintenance/rain-control-state/stage",
      { stateBase64: stateBytes.toString("base64") },
    );
    assert.equal(stateReceipt.stateSha256, controlStateSha256);
    assert.deepEqual(await handler(
      "/internal/adjustment-maintenance/rain-control-state/stage",
      { stateBase64: stateBytes.toString("base64") },
    ), stateReceipt);
    const projection = {
      contractVersion: "adjustment-rain-gate-control-projection/v3",
      family: "rain",
      logicalKey: { index: 0 },
      logicalReceivedAt: "2026-10-08T00:05:00.000Z",
      ordinalArtifactSha256,
      persistenceTarget: { contractVersion: "rain-maintenance-persistence-target/v1" },
      projectionKind: "rain_gate_input",
      rows: [{ index: 0 }],
      source: { index: "0" },
      stateSha256: controlStateSha256,
      stateStageReceiptSha256: stateReceipt.stageReceiptSha256,
      storedContentSha256: "f".repeat(64),
    };
    const stage = await handler("/internal/adjustment-maintenance/revision/stage", {
      projectionBase64: canonicalBytes(projection).toString("base64"),
    });
    const genesis = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    const receipt = revisionReceiptForStage(stage, 1, genesis);
    await handler("/internal/adjustment-maintenance/revision/publish", {
      revisionReceipt: receipt,
      stageReceipt: stage,
    });
    const start = await readAdjustmentRevisionColdTransferStart({
      root,
      servingSnapshot: revisionServingSnapshot([receipt]),
    });
    const page = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: "0",
      afterFrontierSha256: genesis,
      previousPageSha256: start.startSha256,
      root,
      startSha256: start.startSha256,
      watermarkArchiveCommitOrdinal: start.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: start.watermarkFrontierSha256,
    });
    assert.equal(page.entries[0].rainControlState.stateSha256,
      controlStateSha256);
    const graph = buildAdjustmentRevisionColdGraphSegment({ page, start });
    assert.ok(graph.members.some((member) =>
      member.kind === "rain-maintenance-control-state/v1" &&
      member.identitySha256 === controlStateSha256));
    assert.ok(graph.crossLinks.some((link) =>
      link.relation === "binds_rain_control_state" &&
      link.toIdentitySha256 === controlStateSha256));
    const checkpoint = JSON.parse(graph.members[0].payload.toString("utf8"));
    const custody = await acknowledgeAdjustmentRevisionColdCustodyCheckpoint(
      revisionCustodyCheckpointInput({
        custodyCheckpointSha256: "9".repeat(64),
        memberRootSha256: checkpoint.memberRootSha256,
        page,
        root,
        startMemberSha256: checkpoint.startMemberSha256,
      }),
      { now: () => new Date("2026-10-08T02:00:00.000Z") },
    );
    assert.deepEqual(await readdir(join(root, "rain-control-states")), []);
    assert.deepEqual(await readdir(join(root, "rain-control-state-stage-receipts")), []);
    const witnessUnsigned = {
      activationKind: "inert_v14_pre_activation",
      archiveCommitOrdinal: "0",
      catalogFrontierSha256: genesis,
      contractVersion: "adjustment-revision-capture-epoch-witness/v1",
      controlPlaneSha256: "1".repeat(64),
      controlPlaneVersion: "14",
      databaseMigrationHistorySha256:
        "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
      epochAt: "2026-10-08T00:00:00.000Z",
      servingSnapshotSha256: "2".repeat(64),
      sourceCommit: "3".repeat(40),
      sourceRelease: "2026.10.08-1",
      sourceServerImageDigest: `sha256:${"4".repeat(64)}`,
      sourceWebImageDigest: `sha256:${"5".repeat(64)}`,
    };
    const witness = validateAdjustmentRevisionCaptureEpochWitness({
      ...witnessUnsigned,
      witnessSha256: createHash("sha256")
        .update(canonicalBytes(witnessUnsigned)).digest("hex"),
    });
    const anchor = validateAdjustmentRainControlCustodyAnchorV1({
      actionKind: "control_reference",
      actionSha256: "a".repeat(64),
      archiveCommitOrdinal: custody.nextArchiveCommitOrdinal,
      captureEpochWitnessSha256: createHash("sha256")
        .update(canonicalBytes(witness)).digest("hex"),
      contractVersion: "adjustment-rain-control-custody-anchor/v1",
      controlSha256: "b".repeat(64),
      controlStateSha256,
      controlVersion: "14",
      custodyAcknowledgedAt: custody.acknowledgedAt,
      custodyAcknowledgementSha256: custody.acknowledgementSha256,
      custodyCheckpointSha256: custody.custodyCheckpointSha256,
      dueMonth: "2026-08",
      fencingToken: "1",
      frontierSha256: custody.nextFrontierSha256,
      fullGraphVerifiedAt: "2026-10-08T02:00:01.000Z",
      graphManifestSha256: "c".repeat(64),
      memberRootSha256: custody.memberRootSha256,
      ordinalArtifactSha256,
      pageSha256: custody.pageSha256,
      predecessorAnchorSha256: null,
      sequence: "0",
      sourceCommit: witness.sourceCommit,
      sourceMemberRootSha256: "d".repeat(64),
      sourceReceiptRootSha256: "e".repeat(64),
      startMemberSha256: custody.startMemberSha256,
      startSha256: custody.startSha256,
      workstationJournalHeadSha256: "f".repeat(64),
    });
    const anchorBytes = canonicalBytes(anchor);
    const anchorSha256 = createHash("sha256").update(anchorBytes).digest("hex");
    const anchorStore = new AdjustmentRainControlCustodyAnchorStore({
      expectedControlSha256: anchor.controlSha256,
      expectedControlVersion: anchor.controlVersion,
      readCaptureEpochWitness: async () => witness,
      root,
    });
    assert.equal(await anchorStore.readCurrent(), null);
    assert.deepEqual(validateAdjustmentRainControlCustodyAnchorInstallation(
      await anchorStore.install(anchorBytes, anchorSha256),
    ), {
      anchorSha256,
      contractVersion: "adjustment-rain-control-custody-anchor-installation/v1",
      state: "installed",
    });
    assert.deepEqual(validateAdjustmentRainControlCustodyAnchorCurrent(
      await anchorStore.readCurrent(),
    ), { anchor, anchorSha256 });
    assert.equal((await anchorStore.authorizeRainControlReferenceAction({
      actionKind: anchor.actionKind,
      actionSha256: anchor.actionSha256,
      controlStateSha256: anchor.controlStateSha256,
      dueMonth: anchor.dueMonth,
      fencingToken: anchor.fencingToken,
      graphManifestSha256: anchor.graphManifestSha256,
      ordinalArtifactSha256: anchor.ordinalArtifactSha256,
      sourceCommit: anchor.sourceCommit,
      sourceMemberRootSha256: anchor.sourceMemberRootSha256,
      sourceReceiptRootSha256: anchor.sourceReceiptRootSha256,
    })).actionSha256, anchor.actionSha256);
    await assert.rejects(anchorStore.authorizeRainControlReferenceAction({
      actionKind: anchor.actionKind,
      actionSha256: anchor.actionSha256,
      controlStateSha256: "0".repeat(64),
      dueMonth: anchor.dueMonth,
      fencingToken: anchor.fencingToken,
      graphManifestSha256: anchor.graphManifestSha256,
      ordinalArtifactSha256: anchor.ordinalArtifactSha256,
      sourceCommit: anchor.sourceCommit,
      sourceMemberRootSha256: anchor.sourceMemberRootSha256,
      sourceReceiptRootSha256: anchor.sourceReceiptRootSha256,
    }), /authorization differs/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// preserve the public rain state when its projection never receives a database ordinal
test("rain control state gap retains exact auxiliaries before bounded retirement", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-rain-control-gap-"));
  try {
    const ordinalArtifactSha256 = "1".repeat(64);
    const stateBytes = canonicalBytes({
      contractVersion: "rain-maintenance-control-state/v1",
      ordinalArtifactSha256,
      stateSha256: "2".repeat(64),
    });
    const stateSha256 = createHash("sha256").update(stateBytes).digest("hex");
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      parseRainControlState: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
    });
    const stateReceipt = await store.stageRainControlState(stateBytes);
    const projection = {
      contractVersion: "adjustment-rain-gate-control-projection/v3",
      family: "rain",
      logicalKey: { index: 1 },
      logicalReceivedAt: "2026-10-08T00:06:00.000Z",
      ordinalArtifactSha256,
      persistenceTarget: { contractVersion: "rain-maintenance-persistence-target/v1" },
      projectionKind: "rain_gate_input",
      rows: [{ index: 1 }],
      source: { index: "1" },
      stateSha256,
      stateStageReceiptSha256: stateReceipt.stageReceiptSha256,
      storedContentSha256: "3".repeat(64),
    };
    const stage = await store.stageProjection(canonicalBytes(projection));
    await store.recordGap({
      logicalKeySha256: "4".repeat(64),
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
    assert.equal(page.entries.length, 1);
    assert.equal(page.entries[0].rainControlState.identitySha256, stateSha256);
    assert.equal(page.entries[0].rainControlStateStageReceipt.stageReceiptSha256,
      stateReceipt.stageReceiptSha256);
    const segment = buildAdjustmentRevisionGapGraphSegment(page);
    assert.equal(segment.members.length, 6);
    assert.equal(segment.crossLinks.length, 5);
    assert.ok(segment.crossLinks.some((link) =>
      link.relation === "binds_rain_control_state" &&
      link.toIdentitySha256 === stateSha256));
    await acknowledgeAdjustmentRevisionGapPayload({
      graphManifestSha256: "5".repeat(64),
      pageSha256: page.pageSha256,
      root,
    });
    assert.deepEqual(await readdir(join(root, "revision-projections")), []);
    assert.deepEqual(await readdir(join(root, "revision-stage-receipts")), []);
    assert.deepEqual(await readdir(join(root, "rain-control-states")), []);
    assert.deepEqual(await readdir(join(root,
      "rain-control-state-stage-receipts")), []);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// keep an oversized target hour permanently value-free and queryable after restart
test("fixed-gauge target oversized gap is stable and prevents reconstruction", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-rain-target-gap-"));
  try {
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
    });
    const handler = createAdjustmentRevisionArchiveHandler({ store });
    const input = {
      logicalHourAt: "2026-10-08T01:00:00.000Z",
      logicalKeySha256: "9".repeat(64),
      reason: "target_source_oversized",
    };
    const first = await handler(
      "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap",
      input,
    );
    const retry = await handler(
      "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap",
      input,
    );
    assert.deepEqual(retry, first);
    assert.equal(first.qualificationDisposition, "forever_unqualified");
    assert.deepEqual(await handler(
      "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap/status",
      { logicalHourAt: input.logicalHourAt, logicalKeySha256: input.logicalKeySha256 },
    ), {
      contractVersion: "adjustment-rain-fixed-gauge-target-gap-status/v1",
      gap: first,
      state: "present",
    });
    assert.deepEqual(await handler(
      "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap/status",
      { logicalHourAt: "2026-10-08T02:00:00.000Z", logicalKeySha256: "8".repeat(64) },
    ), {
      contractVersion: "adjustment-rain-fixed-gauge-target-gap-status/v1",
      state: "absent",
    });
    await assert.rejects(handler(
      "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap",
      { ...input, reason: "archive_stage_failed" },
    ), /reason|input/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// recover one explicit crash-abandoned state without racing an acquired projection
test("rain control state abandonment drains without a projection ordinal", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-rain-control-orphan-"));
  try {
    const ordinalArtifactSha256 = "6".repeat(64);
    const stateBytes = canonicalBytes({
      contractVersion: "rain-maintenance-control-state/v1",
      ordinalArtifactSha256,
      stateSha256: "7".repeat(64),
    });
    const stateSha256 = createHash("sha256").update(stateBytes).digest("hex");
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      parseRainControlState: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
    });
    const handler = createAdjustmentRevisionArchiveHandler({ store });
    const stageReceipt = await handler(
      "/internal/adjustment-maintenance/rain-control-state/stage",
      { stateBase64: stateBytes.toString("base64") },
    );
    const gap = await handler(
      "/internal/adjustment-maintenance/rain-control-state/gap",
      {
        reason: "projection_stage_failed",
        stageReceiptSha256: stageReceipt.stageReceiptSha256,
        stateSha256,
      },
    );
    assert.equal(gap.contractVersion, "adjustment-rain-control-state-gap/v1");
    assert.deepEqual(await handler(
      "/internal/adjustment-maintenance/rain-control-state/gap",
      {
        reason: "projection_stage_failed",
        stageReceiptSha256: stageReceipt.stageReceiptSha256,
        stateSha256,
      },
    ), gap);
    const projection = {
      contractVersion: "adjustment-rain-gate-control-projection/v3",
      family: "rain",
      logicalKey: { index: 2 },
      logicalReceivedAt: "2026-10-08T00:07:00.000Z",
      ordinalArtifactSha256,
      persistenceTarget: { contractVersion: "rain-maintenance-persistence-target/v1" },
      projectionKind: "rain_gate_input",
      rows: [{ index: 2 }],
      source: { index: "2" },
      stateSha256,
      stateStageReceiptSha256: stageReceipt.stageReceiptSha256,
      storedContentSha256: "8".repeat(64),
    };
    await assert.rejects(store.stageProjection(canonicalBytes(projection)),
      /permanently abandoned/u);
    const start = await readAdjustmentRevisionGapTransferStart({ root });
    const page = await readAdjustmentRevisionGapPayloadPage({
      frontierSha256: start.frontierSha256,
      root,
      startSha256: start.startSha256,
    });
    assert.equal(page.entries.length, 1);
    assert.equal(page.entries[0].payload.kind, "rain-maintenance-control-state/v1");
    assert.equal(page.entries[0].payload.identitySha256, stateSha256);
    const segment = buildAdjustmentRevisionGapGraphSegment(page);
    assert.equal(segment.members.length, 4);
    assert.equal(segment.crossLinks.length, 3);
    await acknowledgeAdjustmentRevisionGapPayload({
      graphManifestSha256: "9".repeat(64),
      pageSha256: page.pageSha256,
      root,
    });
    assert.deepEqual(await readdir(join(root, "rain-control-states")), []);
    assert.deepEqual(await readdir(join(root,
      "rain-control-state-stage-receipts")), []);
    assert.deepEqual(await readdir(join(root, "rain-control-state-gaps")), []);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// retain one body-level terminal gap for an admitted grouped weather revision
test("revision batch terminal gap remains one ineligible atomic cold entry", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-revision-batch-gap-"));
  try {
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
    });
    const genesis = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    const batchStage = await store.stageProjection(
      canonicalBytes(revisionBatchProjection("actual_best_match", 2)),
    );
    const receipts = [];
    let predecessor = genesis;

    // issue one contiguous two-row grouped chain
    for (let index = 0; index < 2; index += 1) {
      const receipt = revisionReceiptForStage(batchStage, index + 1, predecessor);
      receipts.push(receipt);
      predecessor = receipt.frontierSha256;
    }
    const blockedPublishPath = join(root, "revision-publish-receipts",
      `sha256-${batchStage.projectionIdentitySha256}.json`);
    await mkdir(blockedPublishPath);
    await assert.rejects(store.publishRevision({
      revisionReceipts: receipts,
      stageReceipt: batchStage,
    }), /evidence file is invalid|directory|EISDIR/u);
    await rm(blockedPublishPath, { recursive: true });
    const terminalGap = await store.recordGap({
      logicalKeySha256: "8".repeat(64),
      projectionIdentitySha256: batchStage.projectionIdentitySha256,
      projectionKind: batchStage.projectionKind,
      projectionSha256: batchStage.projectionSha256,
      reason: "archive_publish_failed",
    });
    const goodStage = await store.stageProjection(
      canonicalBytes(revisionProjection("target_revision", 3)),
    );
    const goodReceipt = revisionReceiptForStage(goodStage, 3, predecessor);
    await store.publishRevision({ revisionReceipt: goodReceipt, stageReceipt: goodStage });
    const servingSnapshot = revisionServingSnapshot([goodReceipt]);
    const start = await readAdjustmentRevisionColdTransferStart({ root, servingSnapshot });
    const page = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: "0",
      afterFrontierSha256: genesis,
      previousPageSha256: start.startSha256,
      root,
      startSha256: start.startSha256,
      watermarkArchiveCommitOrdinal: start.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: start.watermarkFrontierSha256,
    });

    assert.equal(page.entries.length, 2);
    assert.equal(page.entries[0].receipts.length, 2);
    assert.equal(page.entries[0].publication.disposition, "committed_unpublished_gap");
    assert.deepEqual(page.entries[0].publication.value, terminalGap);
    assert.equal(page.entries[1].receipt.receiptSha256, goodReceipt.receiptSha256);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// preserve an admitted failed publication as an ineligible ordinal-chain member
test("revision cold transfer carries one terminal publication gap before the next good revision", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-revision-terminal-gap-"));
  try {
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
    });
    const genesis = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    const badBytes = canonicalBytes(revisionProjection("actual_best_match", 0));
    const badStage = await store.stageProjection(badBytes);
    const badReceipt = revisionReceiptForStage(badStage, 1, genesis);
    const blockedPublishPath = join(root, "revision-publish-receipts",
      `sha256-${badReceipt.projectionIdentitySha256}.json`);
    await mkdir(blockedPublishPath);
    await assert.rejects(
      store.publishRevision({ revisionReceipt: badReceipt, stageReceipt: badStage }),
      /evidence file is invalid|directory|EISDIR/u,
    );
    await rm(blockedPublishPath, { recursive: true });
    const terminalGap = await store.recordGap({
      logicalKeySha256: "8".repeat(64),
      projectionIdentitySha256: badReceipt.projectionIdentitySha256,
      projectionKind: badReceipt.projectionKind,
      projectionSha256: badReceipt.projectionSha256,
      reason: "archive_publish_failed",
    });
    await assert.rejects(
      store.publishRevision({ revisionReceipt: badReceipt, stageReceipt: badStage }),
      /permanently unqualified/u,
    );
    const goodBytes = canonicalBytes(revisionProjection("target_revision", 1));
    const goodStage = await store.stageProjection(goodBytes);
    const goodReceipt = revisionReceiptForStage(goodStage, 2, badReceipt.frontierSha256);
    await store.publishRevision({ revisionReceipt: goodReceipt, stageReceipt: goodStage });
    const servingSnapshot = revisionServingSnapshot([goodReceipt]);
    servingSnapshot.archiveCommitOrdinal = goodReceipt.archiveCommitOrdinal;
    servingSnapshot.frontierSha256 = goodReceipt.frontierSha256;
    servingSnapshot.snapshotSha256 = createHash("sha256").update([
      servingSnapshot.contractVersion,
      servingSnapshot.cutoffAt,
      servingSnapshot.archiveCommitOrdinal,
      servingSnapshot.frontierSha256,
      goodReceipt.receiptSha256,
    ].join("\n")).digest("hex");
    const start = await readAdjustmentRevisionColdTransferStart({ root, servingSnapshot });
    const page = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: "0",
      afterFrontierSha256: genesis,
      previousPageSha256: start.startSha256,
      root,
      startSha256: start.startSha256,
      watermarkArchiveCommitOrdinal: start.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: start.watermarkFrontierSha256,
    });
    assert.equal(page.eof, true);
    assert.deepEqual(page.entries.map((entry) => entry.publication.disposition), [
      "committed_unpublished_gap", "published",
    ]);
    assert.deepEqual(page.entries[0].publication.value, terminalGap);
    assert.equal(page.entries[1].receipt.receiptSha256, goodReceipt.receiptSha256);
    assert.deepEqual(start.servingSnapshot.entries.map((entry) =>
      entry.receipt.receiptSha256), [goodReceipt.receiptSha256]);
    const segment = buildAdjustmentRevisionColdGraphSegment({ page, start });
    const terminalMemberSha256 = createHash("sha256")
      .update(canonicalBytes(terminalGap)).digest("hex");
    assert.ok(segment.members.some((member) =>
      member.identitySha256 === terminalGap.gapSha256 &&
      member.kind === "adjustment-revision-gap/v1"));
    assert.ok(segment.crossLinks.some((link) =>
      link.fromIdentitySha256 === badReceipt.receiptSha256 &&
      link.relation === "binds_terminal_gap" &&
      link.toIdentitySha256 === terminalGap.gapSha256));
    const checkpoint = JSON.parse(segment.members[0].payload);
    assert.equal(checkpoint.entries[0].publicationMemberSha256,
      terminalMemberSha256);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// create one exact future-only epoch and preserve its first database server clock
test("revision capture epoch accepts disjoint full-0020 and full-0021 zero frontiers", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-revision-epoch-"));
  const path = join(root, "adjustment-capture-epoch.json");
  try {
    const migrationRoot = join(repoRoot, "packages/database/migrations");
    const availableMigrationNames = (await readdir(migrationRoot))
      .filter((name) => name.endsWith(".sql")).sort();
    const frontierMigrationIndex = availableMigrationNames.indexOf(
      "0020_adjustment_revision_frontier.sql",
    );
    assert.notEqual(frontierMigrationIndex, -1);
    const migrationNames = availableMigrationNames.slice(0, frontierMigrationIndex + 1);
    const migrationChecksums = [];
    // hash the exact reviewed ledger in database order
    for (const name of migrationNames) {
      // retain the deployed full-0020 ledger after the unapplied 0019 source advanced
      migrationChecksums.push(name === "0019_adjustment_maintenance_recurring.sql"
        ? "21a737e4efe8c42f76db186cb207a6b838435706e74c6c514f221ba83adb9513"
        : createHash("sha256").update(await readFile(join(migrationRoot, name))).digest("hex"));
    }
    const epochAt = "2026-10-08T02:00:00.000Z";
    const genesis = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    const snapshot = {
      archiveCommitOrdinal: "0",
      contractVersion: "adjustment-revision-serving-snapshot/v1",
      cutoffAt: epochAt,
      entries: [],
      entryCount: 0,
      frontierSha256: genesis,
      snapshotSha256: "",
    };
    snapshot.snapshotSha256 = createHash("sha256").update([
      snapshot.contractVersion, snapshot.cutoffAt, "0", genesis, "",
    ].join("\n")).digest("hex");
    const databaseManifest = {
      contract_version: "adjustment-evaluation-export-manifest/v1",
      migration_checksums: migrationChecksums,
      migration_history_sha256: createHash("sha256").update(migrationNames.map(
        // serialize the same exact database manifest preimage
        (name, index) => `${name}:${migrationChecksums[index]}`,
      ).join("\n")).digest("hex"),
      migration_names: migrationNames,
      query_contract_sha256: "1".repeat(64),
      query_contract_version: "adjustment-evaluation-export-query/v1",
      row_schema_sha256: "2".repeat(64),
      schema_migration: "0017_adjustment_evaluation_export.sql",
      site_key: "ballydidean",
      site_timezone: "America/Los_Angeles",
    };
    const envelope = {
      databaseManifest,
      payload: snapshot,
      transaction: {
        created_at_utc: epochAt,
        idle_in_transaction_session_timeout: "30s",
        isolation_level: "repeatable read",
        lock_timeout: "5s",
        read_only: "on",
        statement_timeout: "5min",
      },
    };
    const deployment = {
      controlPlaneSha256: "3".repeat(64),
      controlPlaneVersion: "14",
      sourceCommit: "4".repeat(40),
      sourceRelease: "2026.10.08-1",
      sourceServerImageDigest: `sha256:${"5".repeat(64)}`,
      sourceWebImageDigest: `sha256:${"6".repeat(64)}`,
    };
    const witness = await writeAdjustmentRevisionCaptureEpochWitness({
      databaseEnvelope: envelope,
      deployment,
      path,
    });
    assert.equal(validateAdjustmentRevisionCaptureEpochWitness(witness), witness);
    assert.equal(witness.archiveCommitOrdinal, "0");
    assert.equal(witness.epochAt, epochAt);
    assert.equal(witness.catalogFrontierSha256, genesis);
    const details = await lstat(path);
    assert.equal(details.mode & 0o777, 0o644);
    assert.equal(details.nlink, 1);
    const retainedSnapshot = await readAdjustmentRevisionCaptureEpochSnapshot({
      expectedGid: process.getegid(),
      expectedUid: process.geteuid(),
      path: join(root, "adjustment-capture-epoch-snapshot.json"),
    });
    assert.equal(retainedSnapshot.snapshotSha256, witness.servingSnapshotSha256);
    assert.deepEqual(await readAdjustmentRevisionCaptureEpochWitness({
      expectedGid: process.getegid(),
      expectedUid: process.geteuid(),
      path,
    }), witness);
    const laterEpochAt = "2026-10-08T02:01:00.000Z";
    const retry = await writeAdjustmentRevisionCaptureEpochWitness({
      databaseEnvelope: {
        ...envelope,
        payload: null,
        transaction: { ...envelope.transaction, created_at_utc: laterEpochAt },
      },
      deployment,
      path,
    });
    assert.deepEqual(retry, witness);
    const rollingMigrationName = "0021_adjustment_rolling_registration.sql";
    const rollingMigrationNames = [...migrationNames, rollingMigrationName];
    const rollingMigrationChecksums = [];
    // hash the intended full-0021 source independently from the retained full-0020 ledger
    for (const name of rollingMigrationNames) {
      rollingMigrationChecksums.push(createHash("sha256")
        .update(await readFile(join(migrationRoot, name))).digest("hex"));
    }
    const rollingManifest = {
      ...databaseManifest,
      migration_checksums: rollingMigrationChecksums,
      migration_history_sha256: createHash("sha256").update(rollingMigrationNames.map(
        // serialize the exact additive full-0021 activation ledger
        (name, index) => `${name}:${rollingMigrationChecksums[index]}`,
      ).join("\n")).digest("hex"),
      migration_names: rollingMigrationNames,
    };
    const rollingRoot = join(root, "rolling");
    await mkdir(rollingRoot);
    const rollingWitness = await writeAdjustmentRevisionCaptureEpochWitness({
      databaseEnvelope: { ...envelope, databaseManifest: rollingManifest },
      deployment,
      path: join(rollingRoot, "adjustment-capture-epoch.json"),
    });
    assert.equal(rollingWitness.databaseMigrationHistorySha256,
      "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424");
    const advancedRoot = join(root, "advanced");
    await mkdir(advancedRoot);
    const advancedPath = join(advancedRoot, "adjustment-capture-epoch.json");
    const advancedSnapshot = {
      ...snapshot,
      archiveCommitOrdinal: "1",
      snapshotSha256: createHash("sha256").update([
        snapshot.contractVersion, epochAt, "1", genesis, "",
      ].join("\n")).digest("hex"),
    };
    await assert.rejects(writeAdjustmentRevisionCaptureEpochWitness({
      databaseEnvelope: { ...envelope, payload: advancedSnapshot },
      deployment,
      path: advancedPath,
    }), /not zero frontier/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// archive permanent unqualified stages before releasing both bounded online slots
test("revision gap transfer acknowledges exact staged bytes without ordinals", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-revision-gap-transfer-"));
  const body = Buffer.from("{\"body\":true}\n");
  const comparator = Buffer.from("{\"comparator\":true}\n");
  const sourceProjection = Buffer.from("{\"source\":true}\n");
  const metadata = {
    bodyByteCount: body.length,
    candidateSha256: "1".repeat(64),
    dueKey: "capture/2026-10-08T00:35:00.000Z",
    inputSha256: "2".repeat(64),
    issuedAt: "2026-10-08T00:35:01.000Z",
    maxValidAt: "2026-10-08T12:00:00.000Z",
    minValidAt: "2026-10-08T01:00:00.000Z",
    predictionBodySha256: createHash("sha256").update(body).digest("hex"),
    predictionSchemaSha256: "3".repeat(64),
    predictionSha256: "4".repeat(64),
    registrationSha256: "5".repeat(64),
    rowCount: 12,
    sourceReceiptSha256: "6".repeat(64),
    sourceSha256: "7".repeat(64),
  };
  try {
    const store = new AdjustmentRevisionArchiveStore({
      parseProjection: (bytes) => JSON.parse(bytes.toString("utf8")),
      root,
      validateShadowStage: () => undefined,
    });
    const projectionBytes = canonicalBytes(revisionBatchProjection("actual_best_match", 3));
    const stage = await store.stageProjection(projectionBytes);
    await store.recordGap({
      logicalKeySha256: "8".repeat(64),
      projectionIdentitySha256: stage.projectionIdentitySha256,
      projectionKind: stage.projectionKind,
      projectionSha256: stage.projectionSha256,
      reason: "database_bind_failed",
    });
    const handler = createAdjustmentRevisionArchiveHandler({ store });
    const sourceProjectionSha256 = createHash("sha256")
      .update(sourceProjection).digest("hex");
    await handler("/internal/adjustment-maintenance/archive/stage", {
      bodyBase64: body.toString("base64"),
      comparatorBase64: comparator.toString("base64"),
      metadata,
      sourceProjectionBase64: sourceProjection.toString("base64"),
      sourceProjectionSha256,
    });
    await store.recordShadowGap({
      dueKey: metadata.dueKey,
      family: "temperature",
      reason: "database_append_failed",
      registrationSha256: metadata.registrationSha256,
    });
    const start = await readAdjustmentRevisionGapTransferStart({ root });
    assert.equal(validateAdjustmentRevisionGapTransferStart(start), start);
    const page = await readAdjustmentRevisionGapPayloadPage({
      frontierSha256: start.frontierSha256,
      root,
      startSha256: start.startSha256,
    });
    assert.equal(validateAdjustmentRevisionGapPayloadPage(page), page);
    assert.equal(page.entries.length, 2);
    assert.equal(page.idle, false);
    assert.ok(page.entries.some((entry) =>
      entry.payload.kind === "adjustment-revision-batch-projection/v2"));
    assert.ok(page.entries.some((entry) =>
      entry.payload.kind === "adjustment-shadow-revision-stage/v2"));
    assert.doesNotMatch(JSON.stringify(page), /archiveCommitOrdinal|revisionReceipt/u);
    const segment = buildAdjustmentRevisionGapGraphSegment(page);
    assert.equal(segment.members.length, 7);
    assert.equal(segment.crossLinks.length, 6);
    assert.deepEqual(segment.members.map((member) => member.identitySha256).sort(), [
      page.pageSha256,
      ...page.entries.flatMap((entry) => [
        entry.gap.gapSha256,
        entry.payload.identitySha256,
        entry.stageReceipt.stageReceiptSha256,
      ]),
    ].sort());
    const graphManifestSha256 = "9".repeat(64);
    const acknowledgement = await acknowledgeAdjustmentRevisionGapPayload({
      graphManifestSha256,
      pageSha256: page.pageSha256,
      root,
    });
    assert.equal(validateAdjustmentRevisionGapPayloadAcknowledgement(acknowledgement),
      acknowledgement);
    assert.equal(acknowledgement.pageCount, "1");
    assert.deepEqual(await acknowledgeAdjustmentRevisionGapPayload({
      graphManifestSha256,
      pageSha256: page.pageSha256,
      root,
    }), acknowledgement);
    assert.deepEqual(await readdir(join(root, "revision-projections")), []);
    assert.deepEqual(await readdir(join(root, "revision-stage-receipts")), []);
    assert.deepEqual(await readdir(join(root, "shadow-revision-stages")), []);
    assert.equal((await readdir(join(root, "revision-gaps"))).length, 1);
    assert.equal((await readdir(join(root, "shadow-revision-gaps"))).length, 1);
    const next = await readAdjustmentRevisionGapTransferStart({ root });
    assert.equal(next.pageCount, "1");
    assert.equal(next.frontierSha256, acknowledgement.frontierSha256);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// encode recursively sorted canonical json bytes
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
      // normalize every sorted object field
      (key) => [key, normalize(entry[key])],
    ));
  };
  return Buffer.from(`${JSON.stringify(normalize(value))}\n`);
}

// build one compact shadow metadata fixture around exact body bytes
function shadowMetadataFixture(body) {
  return {
    bodyByteCount: body.length,
    candidateSha256: "1".repeat(64),
    dueKey: "capture/2026-10-08T00:35:00.000Z",
    inputSha256: "2".repeat(64),
    issuedAt: "2026-10-08T00:35:01.000Z",
    maxValidAt: "2026-10-08T12:00:00.000Z",
    minValidAt: "2026-10-08T01:00:00.000Z",
    predictionBodySha256: createHash("sha256").update(body).digest("hex"),
    predictionSchemaSha256: "3".repeat(64),
    predictionSha256: "4".repeat(64),
    registrationSha256: "5".repeat(64),
    rowCount: 12,
    sourceReceiptSha256: "6".repeat(64),
    sourceSha256: "7".repeat(64),
  };
}

// build one closed canonical projection fixture with a unique logical identity
function revisionProjection(projectionKind, index) {
  return {
    contractVersion: "adjustment-revision-projection/v1",
    family: "shared",
    logicalKey: { index },
    logicalReceivedAt: `2026-10-08T00:0${String(index)}:00.000Z`,
    projectionKind,
    rows: [{ index }],
    source: { index: String(index) },
    storedContentSha256: String(index + 1).repeat(64),
  };
}

// build one additive grouped weather projection fixture
function revisionBatchProjection(projectionKind, rowCount) {
  return {
    contractVersion: "adjustment-revision-batch-projection/v2",
    family: "shared",
    logicalKey: { index: 0 },
    logicalReceivedAt: "2026-10-08T00:00:00.000Z",
    projectionKind,
    rows: Array.from({ length: rowCount }, (_, index) => ({ index })),
    source: { index: "0" },
    storedContentSha256: "1".repeat(64),
  };
}

// bind one deterministic database-shaped receipt to its exact durable stage
function revisionReceiptForStage(stage, ordinal, predecessorFrontierSha256) {
  const receipt = {
    archiveCommitOrdinal: String(ordinal),
    archiveCommittedAt: new Date(Date.parse("2026-10-08T01:00:00.000Z") +
      ordinal * 60_000).toISOString(),
    contractVersion: "adjustment-revision-commit-receipt/v1",
    frontierSha256: "",
    predecessorFrontierSha256,
    projectionIdentitySha256: stage.projectionIdentitySha256,
    projectionKind: stage.projectionKind,
    projectionSha256: stage.projectionSha256,
    receiptSha256: "",
    stageReceiptSha256: stage.stageReceiptSha256,
  };
  receipt.receiptSha256 = createHash("sha256").update([
    receipt.contractVersion,
    receipt.archiveCommitOrdinal,
    receipt.archiveCommittedAt,
    receipt.projectionKind,
    receipt.projectionIdentitySha256,
    receipt.projectionSha256,
    receipt.stageReceiptSha256,
    receipt.predecessorFrontierSha256,
  ].join("\n")).digest("hex");
  receipt.frontierSha256 = createHash("sha256").update([
    "adjustment-revision-frontier/v1",
    receipt.predecessorFrontierSha256,
    receipt.archiveCommitOrdinal,
    receipt.receiptSha256,
  ].join("\n")).digest("hex");
  return receipt;
}

// issue one distinct shadow receipt with source and input identities kept separate
function shadowRevisionReceipt(metadata, stage) {
  const predecessorFrontierSha256 = createHash("sha256")
    .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
  const receipt = {
    archiveCommitOrdinal: "1",
    archiveCommittedAt: "2026-10-08T00:36:00.000Z",
    contractVersion: "adjustment-revision-commit-receipt/v1",
    frontierSha256: "",
    predecessorFrontierSha256,
    projectionIdentitySha256: metadata.sourceReceiptSha256,
    projectionKind: "shadow_prediction",
    projectionSha256: metadata.inputSha256,
    receiptSha256: "",
    stageReceiptSha256: stage.stageReceiptSha256,
  };
  receipt.receiptSha256 = createHash("sha256").update([
    receipt.contractVersion,
    receipt.archiveCommitOrdinal,
    receipt.archiveCommittedAt,
    receipt.projectionKind,
    receipt.projectionIdentitySha256,
    receipt.projectionSha256,
    receipt.stageReceiptSha256,
    receipt.predecessorFrontierSha256,
  ].join("\n")).digest("hex");
  receipt.frontierSha256 = createHash("sha256").update([
    "adjustment-revision-frontier/v1",
    receipt.predecessorFrontierSha256,
    receipt.archiveCommitOrdinal,
    receipt.receiptSha256,
  ].join("\n")).digest("hex");
  return receipt;
}

// build one cutoff-bound current-pointer snapshot from exact receipt members
function revisionServingSnapshot(receipts) {
  const snapshot = {
    archiveCommitOrdinal: receipts.at(-1).archiveCommitOrdinal,
    contractVersion: "adjustment-revision-serving-snapshot/v1",
    cutoffAt: "2026-10-08T02:00:00.000Z",
    entries: receipts.map(
      // bind one serving relation to its exact authoritative receipt
      (receipt, index) => ({
        logicalReceivedAt: new Date(Date.parse("2026-10-08T00:00:00.000Z") +
          index * 60_000).toISOString(),
        receipt,
        relation: receipt.projectionKind === "rain_gate_input"
          ? "rain_adjustment_runs"
          : receipt.projectionKind === "native_source"
            ? "forecast_anchor_records"
            : "weather_records",
      }),
    ),
    entryCount: receipts.length,
    frontierSha256: receipts.at(-1).frontierSha256,
    snapshotSha256: "",
  };
  snapshot.snapshotSha256 = createHash("sha256").update([
    snapshot.contractVersion,
    snapshot.cutoffAt,
    snapshot.archiveCommitOrdinal,
    snapshot.frontierSha256,
    receipts.map(
      // retain exact server receipt order in the snapshot identity
      (receipt) => receipt.receiptSha256,
    ).join("\n"),
  ].join("\n")).digest("hex");
  return snapshot;
}

// bind one test acknowledgement to an already archived page checkpoint
function revisionCustodyInput(input) {
  return {
    afterArchiveCommitOrdinal: input.page.afterArchiveCommitOrdinal,
    afterFrontierSha256: input.page.afterFrontierSha256,
    graphManifestSha256: input.graphManifestSha256,
    memberRootSha256: input.memberRootSha256,
    pageSha256: input.page.pageSha256,
    previousPageSha256: input.page.previousPageSha256,
    root: input.root,
    startMemberSha256: input.startMemberSha256,
    startSha256: input.page.startSha256,
    watermarkArchiveCommitOrdinal: input.page.watermarkArchiveCommitOrdinal,
    watermarkFrontierSha256: input.page.watermarkFrontierSha256,
  };
}

// bind one packed-custody checkpoint to an already archived page
function revisionCustodyCheckpointInput(input) {
  const custody = revisionCustodyInput({
    ...input,
    graphManifestSha256: input.custodyCheckpointSha256,
  });
  delete custody.graphManifestSha256;
  return {
    ...custody,
    custodyCheckpointSha256: input.custodyCheckpointSha256,
  };
}

// build one public filtered forecast row
function forecastRow(index = 0, overrides = {}) {
  const validAt = new Date(Date.parse("2026-10-07T08:00:00.000Z") + index * 3_600_000).toISOString();
  return {
    adjustment: {
      adjustedMetrics: { windGustMps: 3.5, windSpeedMps: 2.25 },
      appliedMetrics: ["windGustMps", "windSpeedMps"],
      authorizationSha256: hashes.windAuthorization,
      candidateArtifactSha256: hashes.windCandidate,
      leadBand: "001-024",
      reasonCode: null,
      state: "active",
    },
    freshness: { ageSeconds: 1, label: "fresh", status: "fresh" },
    id: String(index + 1),
    metadata: {
      provider: { dataset: "best_match" },
      upstream: { model: "best_match", timezone: "America/Los_Angeles" },
    },
    metrics: {
      precipitationMm: 0.2,
      temperatureC: 12.5,
      windGustMps: 4,
      windSpeedMps: 2.5,
    },
    productRunAt: "2026-10-07T00:00:00.000Z",
    provenance: {
      providerKey: "open-meteo",
      sourceId: "12",
      sourceKey: "open-meteo-forecast-v4",
    },
    rainAdjustment: {
      bundleSha256: hashes.rain,
      correctedPrecipitationMm: 0.15,
      rawBestMatchPrecipitationMm: 0.2,
      reasonCode: null,
      sourceForecast: {
        decisionAt: "2026-10-07T02:00:00.000Z",
        firstReceivedAt: "2026-10-07T00:05:00.000Z",
        modelLeadHours: 8 + index,
        providerKey: "open-meteo",
        rawPrecipitationMm: 0.25,
        runInitializedAt: "2026-10-07T00:00:00.000Z",
        upstreamModel: "ecmwf_ifs",
        validAt,
      },
      state: "active",
    },
    receivedAt: "2026-10-07T00:06:00.000Z",
    revisionCount: 2,
    temperatureAdjustment: {
      branch: "direct",
      bundleSha256: hashes.temperature,
      correctedTemperatureC: 12,
      rawBestMatchTemperatureC: 12.5,
      reasonCode: null,
      sourceForecast: {
        adapterVersion: "v1",
        dataset: "single_run",
        firstReceivedAt: "2026-10-07T00:04:00.000Z",
        modelCycle: "50r1",
        modelLeadHours: 8 + index,
        operationalHorizonHours: 2 + index,
        providerKey: "open-meteo",
        providerResponseSha256: "1".repeat(64),
        rawRelativeHumidityPercent: 75,
        rawTemperatureC: 11.5,
        rawWindSpeedMps: 3,
        runInitializedAt: "2026-10-07T00:00:00.000Z",
        upstreamModel: "ecmwf_ifs",
        validAt,
      },
      state: "active",
    },
    validAt,
    ...overrides,
  };
}

// build one post-settings response body
function forecastBody(rows = [forecastRow()], overrides = {}) {
  return Buffer.from(`${JSON.stringify({
    adjustmentRuntime: {
      activeBundle: hashes.wind,
      authorizationSha256: hashes.windAuthorization,
      candidateArtifactSha256: hashes.windCandidate,
      loadedAt: "2026-10-07T00:07:00.000Z",
      state: "active",
    },
    adjustmentSettings: { rain: true, temperature: true, version: 1, wind: true },
    data: rows,
    days: 1,
    generatedAt: "2026-10-07T00:08:00.000Z",
    rainAdjustmentRuntime: {
      activeBundle: hashes.rain,
      loadedAt: "2026-10-07T00:07:00.000Z",
      state: "active",
    },
    temperatureAdjustmentRuntime: {
      activeBundle: hashes.temperature,
      authorizationSha256: hashes.temperatureAuthorization,
      loadedAt: "2026-10-07T00:07:00.000Z",
      state: "active",
    },
    ...overrides,
  })}\n`);
}

// provide measured production headroom and ample inodes
function healthyStatfs() {
  return {
    bavail: 2_050_711_552n / 4_096n,
    bsize: 4_096n,
    ffree: 100_000n,
  };
}

// measure the immutable ledger's real filesystem allocation
async function evidenceAllocation(root) {
  let bytes = 0;
  let objects = 0;

  // include every final object and receipt allocation
  for (const directory of ["objects", "receipts"]) {
    const names = await readdir(join(root, directory));

    // measure one retained immutable entry
    for (const name of names) {
      const details = await lstat(join(root, directory, name), { bigint: true });
      bytes += Number(details.blocks * 512n);
    }

    // retain only the content-object count
    if (directory === "objects") {
      objects = names.length;
    }
  }

  return { bytes, objects };
}

// preserve stable content across response-ephemeral fields and mutable receipt time
test("capture excludes generated, freshness, loaded and mutable received times", () => {
  const first = createAdjustmentEvidenceCapture(forecastBody(), "days=1");
  const changedRow = forecastRow(0, {
    freshness: { ageSeconds: 900, label: "late", status: "stale" },
    receivedAt: "2026-10-07T00:16:00.000Z",
  });
  const second = createAdjustmentEvidenceCapture(forecastBody([changedRow], {
    generatedAt: "2026-10-07T00:18:00.000Z",
  }), "days=1");

  assert.equal(first.edgeReceiptIdentitySha256, second.edgeReceiptIdentitySha256);
  assert.equal(first.objectSha256, second.objectSha256);
  assert.deepEqual(first.object, second.object);
  assert.notDeepEqual(first.availability, second.availability);
  assert.equal(first.object.rows[0].provenanceComplete, false);
});

// retain each family's exact public decision states
test("capture accepts wind not_applicable without widening source-family states", () => {
  const fallback = forecastRow(0, {
    adjustment: {
      adjustedMetrics: {},
      appliedMetrics: [],
      reasonCode: "unsupported_lead",
      state: "not_applicable",
    },
    rainAdjustment: { ...forecastRow().rainAdjustment, state: "raw_fallback" },
    temperatureAdjustment: { ...forecastRow().temperatureAdjustment, state: "raw_fallback" },
  });
  const capture = createAdjustmentEvidenceCapture(forecastBody([fallback, forecastRow(1)]), "days=1");
  assert.equal(capture.object.rows[0].windAdjustment.state, "not_applicable");
  assert.equal(capture.object.rows[0].rainAdjustment.state, "raw_fallback");
  assert.equal(capture.object.rows[0].temperatureAdjustment.state, "raw_fallback");
  assert.equal(capture.object.rows[1].windAdjustment.state, "active");

  // reject states belonging only to another family
  for (const [family, state] of [
    ["adjustment", "raw_fallback"],
    ["temperatureAdjustment", "not_applicable"],
    ["rainAdjustment", "not_applicable"],
    ["adjustment", "unknown"],
    ["temperatureAdjustment", "unknown"],
    ["rainAdjustment", "unknown"],
    ["adjustment", null],
  ]) {
    const row = forecastRow();
    row[family] = { ...row[family], state };
    assert.throws(() => createAdjustmentEvidenceCapture(forecastBody([row]), "days=1"), /state is invalid/u);
  }
});

// detect stable identity collisions without mutating record identity
test("same stable row identity with changed scoring content creates a different object", () => {
  const first = createAdjustmentEvidenceCapture(forecastBody(), "days=1");
  const changed = forecastRow(0, {
    metrics: {
      ...forecastRow().metrics,
      windSpeedMps: 9,
    },
  });
  const second = createAdjustmentEvidenceCapture(forecastBody([changed]), "days=1");

  assert.equal(first.edgeReceiptIdentitySha256, second.edgeReceiptIdentitySha256);
  assert.notEqual(first.objectSha256, second.objectSha256);
});

// retain one exclusive first receipt and freeze all valid pairs
test("store converges duplicates, blocks collisions, and freezes a validated watermark", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-evidence-"));
  const store = await new AdjustmentEvidenceStore({
    now: () => new Date("2026-10-07T01:00:00.000Z"),
    root,
    statfs: async () => healthyStatfs(),
  }).initialize();

  try {
    const first = createAdjustmentEvidenceCapture(forecastBody(), "days=1");
    assert.deepEqual(
      await store.commit(first, "2026-10-07T01:00:00.000Z"),
      { status: "created" },
    );

    const retry = createAdjustmentEvidenceCapture(forecastBody([
      forecastRow(0, { receivedAt: "2026-10-07T00:30:00.000Z" }),
    ]), "days=1");
    assert.deepEqual(
      await store.commit(retry, "2026-10-07T01:30:00.000Z"),
      { status: "duplicate" },
    );

    const receiptPath = join(
      root,
      "receipts",
      `sha256-${first.edgeReceiptIdentitySha256}.json`,
    );
    const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
    assert.equal(receipt.firstEdgeCommittedAt, "2026-10-07T01:00:00.000Z");
    assert.deepEqual(receipt.availability.timestamps, ["2026-10-07T00:06:00.000Z"]);

    const changed = forecastRow(0, {
      metrics: { ...forecastRow().metrics, temperatureC: 99 },
    });
    const collision = createAdjustmentEvidenceCapture(forecastBody([changed]), "days=1");
    assert.deepEqual(
      await store.commit(collision, "2026-10-07T02:00:00.000Z"),
      { status: "identity_collision" },
    );

    // refuse repeated collisions before creating any new content object
    for (let attempt = 0; attempt < 5; attempt += 1) {
      assert.deepEqual(
        await store.commit(collision, "2026-10-07T02:00:00.000Z"),
        { status: "identity_collision" },
      );
    }
    assert.equal(store.status().collisions, 1);
    assert.deepEqual(
      { bytes: store.status().bytes, objects: store.status().objects },
      await evidenceAllocation(root),
    );

    const snapshot = await freezeAdjustmentEvidenceSnapshot({
      now: () => new Date("2026-10-07T02:30:00.000Z"),
      root,
    });
    assert.equal(snapshot.entries.length, 1);
    assert.equal(snapshot.entries[0].edgeReceiptIdentitySha256, first.edgeReceiptIdentitySha256);
    assert.match(snapshot.watermarkSha256, /^[a-f0-9]{64}$/u);
    const compressed = await readFile(snapshot.entries[0].objectPath);
    assert.deepEqual(JSON.parse(gunzipSync(compressed)), first.object);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// account both immutable objects when two writers race one stable receipt
test("cross-writer receipt collision accounts the retained orphan object", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-race-"));
  let waiting = 0;
  let release;
  const gate = new Promise(
    // release both writers only after their receipt prechecks
    (resolveGate) => { release = resolveGate; },
  );
  // align two independent store instances at object publication
  async function beforeObjectWrite() {
    waiting += 1;

    // release the pair after both writers reach the barrier
    if (waiting === 2) {
      release();
    }
    await gate;
  }
  const firstStore = await new AdjustmentEvidenceStore({
    beforeObjectWrite,
    root,
    statfs: async () => healthyStatfs(),
  }).initialize();
  const secondStore = await new AdjustmentEvidenceStore({
    beforeObjectWrite,
    root,
    statfs: async () => healthyStatfs(),
  }).initialize();

  try {
    const first = createAdjustmentEvidenceCapture(forecastBody(), "days=1");
    const changed = forecastRow(0, {
      metrics: { ...forecastRow().metrics, temperatureC: 13.25 },
    });
    const second = createAdjustmentEvidenceCapture(forecastBody([changed]), "days=1");
    const results = await Promise.all([
      firstStore.commit(first, "2026-10-07T01:00:00.000Z"),
      secondStore.commit(second, "2026-10-07T01:00:00.000Z"),
    ]);
    assert.deepEqual(
      results.map((result) => result.status).sort(),
      ["created", "identity_collision"],
    );
    const collisionStore = results[0].status === "identity_collision"
      ? firstStore
      : secondStore;
    assert.equal(collisionStore.status().collisions, 1);
    assert.deepEqual(
      { bytes: collisionStore.status().bytes, objects: collisionStore.status().objects },
      await evidenceAllocation(root),
    );
    assert.equal(collisionStore.status().objects, 2);
    assert.equal((await freezeAdjustmentEvidenceSnapshot({ root })).entries.length, 1);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// account a retained partial final and stop all later capture after a write fault
test("partial exclusive-write failure is accounted and disables capture", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-partial-"));
  let faulted = false;
  // inject the former direct-final partial-write failure mode
  async function partialWriter(path, bytes) {
    const handle = await open(path, "wx", 0o600);

    try {
      await handle.writeFile(bytes.subarray(0, Math.max(1, Math.floor(bytes.byteLength / 2))));
      await handle.sync();
    } finally {
      await handle.close();
    }
    faulted = true;
    throw new Error("injected partial write failure");
  }
  const store = await new AdjustmentEvidenceStore({
    root,
    statfs: async () => healthyStatfs(),
    writeExclusive: partialWriter,
  }).initialize();

  try {
    const capture = createAdjustmentEvidenceCapture(forecastBody(), "days=1");
    await assert.rejects(
      store.commit(capture, "2026-10-07T01:00:00.000Z"),
      /injected partial write failure/u,
    );
    assert.equal(faulted, true);
    assert.deepEqual(
      { bytes: store.status().bytes, objects: store.status().objects },
      await evidenceAllocation(root),
    );
    assert.equal(store.status().objects, 1);
    assert.equal(store.prepare(forecastBody(), "days=1"), null);
    assert.deepEqual(
      await store.commit(capture, "2026-10-07T01:01:00.000Z"),
      { status: "store_unavailable" },
    );
    assert.equal((await readdir(join(root, "objects"))).length, 1);
    assert.equal((await freezeAdjustmentEvidenceSnapshot({ root })).entries.length, 0);
    const restarted = await new AdjustmentEvidenceStore({ root }).initialize();
    assert.equal(restarted.status().asyncErrors, 1);
    assert.equal(restarted.prepare(forecastBody(), "days=1"), null);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// preserve the exact shared-filesystem floor without pruning
test("capacity refusal leaves immutable directories empty", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-capacity-"));
  const store = await new AdjustmentEvidenceStore({
    root,
    statfs: async () => ({
      bavail: 1_950_000_000n / 4_096n,
      bsize: 4_096n,
      ffree: 100_000n,
    }),
  }).initialize();

  try {
    const capture = createAdjustmentEvidenceCapture(forecastBody(), "days=1");
    assert.deepEqual(
      await store.commit(capture, "2026-10-07T01:00:00.000Z"),
      { status: "capacity_exhausted" },
    );
    assert.equal(store.status().freeSpaceRefusal, 1);
    assert.equal((await freezeAdjustmentEvidenceSnapshot({ root })).entries.length, 0);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// keep immediate writes from borrowing the reserved next-capture allocation
test("evidence and page writes preserve the next-capture reservation", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-reservation-"));
  const statfs =
    // expose exactly the readiness boundary before any allocation
    async () => ({
      bavail: BigInt(ADJUSTMENT_EVIDENCE_SCHEDULER_REQUIRED_FREE_BYTES),
      bsize: 1n,
      ffree: 100_000n,
    });
  const store = await new AdjustmentEvidenceStore({ root, statfs }).initialize();
  const ports = await new AdjustmentCyclePageDiskPorts({ root, statfs }).initialize();

  try {
    const capture = createAdjustmentEvidenceCapture(forecastBody(), "days=1");
    assert.deepEqual(
      await store.commit(capture, "2026-10-07T01:00:00.000Z"),
      { status: "capacity_exhausted" },
    );
    const state = createCyclePageState({
      dailyPageCount: 0,
      dueKey: "capture/2026-10-08T00:35:00.000Z",
      generation: "1",
      localDate: "2026-10-08",
    });
    const page = await appendCyclePageFailOpen(state, {
      payload: Buffer.from("reserved-page"),
      projections: [{ channel: "scheduler_request", identitySha256: "8".repeat(64) }],
    }, ports);
    assert.equal(page.status, "evidence_gap_persistence_refused");
    assert.deepEqual(await readdir(join(root, "objects")), []);
    assert.deepEqual(await readdir(join(root, "receipts")), []);
    assert.deepEqual(await readdir(join(root, "online-pages", "slots")), []);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// reject a linked root before creating or changing any target child
test("store and scheduler do not mutate through a configured root symlink", async () => {
  const parent = await mkdtemp(join(tmpdir(), "weather-adjustment-linked-"));
  const target = join(parent, "target");
  const linkedRoot = join(parent, "linked-root");
  await mkdir(target, { mode: 0o700 });
  await symlink(target, linkedRoot);

  try {
    const store = await new AdjustmentEvidenceStore({ root: linkedRoot }).initialize();
    assert.equal(store.prepare(forecastBody(), "days=1"), null);
    const scheduler = await new AdjustmentEvidenceScheduler({
      enabled: true,
      migrationReady: true,
      root: linkedRoot,
      statfs: async () => healthyStatfs(),
    }).initialize();
    assert.equal(scheduler.status().activation, "state_unavailable");
    assert.deepEqual(await readdir(target), []);
  } finally {
    await rm(parent, { force: true, recursive: true });
  }
});

// retain the representative maximum row count under both object limits
test("deterministic gzip accepts 240 forecast rows", () => {
  const rows = Array.from({ length: 240 },
    // build one ordered ten-day response
    (_value, index) => forecastRow(index));
  const first = createAdjustmentEvidenceCapture(forecastBody(rows, { days: 10 }), "days=10");
  const second = createAdjustmentEvidenceCapture(forecastBody(rows, {
    days: 10,
    generatedAt: "2026-10-07T00:20:00.000Z",
  }), "days=10");

  assert.ok(Buffer.byteLength(JSON.stringify(first.object)) <= 512 * 1_024);
  assert.ok(first.compressedObject.byteLength <= 16 * 1_024);
  assert.deepEqual(first.compressedObject, second.compressedObject);
});

// lock the parsed-object seam to the legacy buffer contract
test("parsed forecast capture preserves every legacy evidence byte", () => {
  for (const [days, rowCount] of [[1, 24], [5, 120], [10, 240]]) {
    const rows = Array.from({ length: rowCount },
      // build each supported public forecast window
      (_value, index) => forecastRow(index));
    const body = forecastBody(rows, { days });
    const forecast = JSON.parse(body.toString("utf8"));
    const window = `days=${String(days)}`;
    const legacy = createAdjustmentEvidenceCapture(body, window);
    const parsed = createAdjustmentEvidenceCaptureFromForecast(forecast, window);
    assert.deepEqual(parsed, legacy);
    assert.deepEqual(parsed.compressedObject, legacy.compressedObject);
    assert.equal(parsed.objectSha256, legacy.objectSha256);
    assert.equal(parsed.edgeReceiptIdentitySha256, legacy.edgeReceiptIdentitySha256);
  }

  const rows = Array.from({ length: 241 },
    // retain the one fall-back row admitted only by v2
    (_value, index) => forecastRow(index));
  const body = forecastBody(rows, { days: 10 });
  assert.deepEqual(
    createAdjustmentEvidenceCaptureV2FromForecast(
      JSON.parse(body.toString("utf8")),
      "days=10",
    ),
    createAdjustmentEvidenceCaptureV2(body, "days=10"),
  );

  const changedForecasts = [
    { adjustmentSettings: { rain: false, temperature: true, version: 1, wind: false } },
    { adjustmentRuntime: {
      activeBundle: "e".repeat(64),
      authorizationSha256: "f".repeat(64),
      candidateArtifactSha256: "0".repeat(64),
    } },
    { data: [forecastRow(0, { metrics: { ...forecastRow().metrics, windSpeedMps: 12 } })] },
  ];
  const baseline = createAdjustmentEvidenceCapture(forecastBody(), "days=1");

  // retain parity across each request-varying projection class
  for (const overrides of changedForecasts) {
    const changedBody = forecastBody(undefined, overrides);
    const legacy = createAdjustmentEvidenceCapture(changedBody, "days=1");
    const parsed = createAdjustmentEvidenceCaptureFromForecast(
      JSON.parse(changedBody.toString("utf8")),
      "days=1",
    );
    assert.deepEqual(parsed, legacy);
    assert.notEqual(parsed.objectSha256, baseline.objectSha256);
  }

  // refuse the same malformed parsed and serialized envelopes
  for (const invalid of [null, {}, { data: [] }]) {
    assert.throws(
      () => createAdjustmentEvidenceCaptureFromForecast(invalid, "days=1"),
      /forecast evidence body is invalid/u,
    );
    assert.throws(
      () => createAdjustmentEvidenceCapture(Buffer.from(JSON.stringify(invalid)), "days=1"),
      /forecast evidence body is invalid/u,
    );
  }
});

// retain only deterministic compressed bytes in the bounded store cache
test("store compression cache isolates buffers refuses collisions and evicts least-recent", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-compression-cache-"));
  let compressionCalls = 0;
  const store = await new AdjustmentEvidenceStore({
    compressObject: (bytes) => {
      compressionCalls += 1;
      return gzipSync(bytes, { level: 9, mtime: 0 });
    },
    root,
    statfs: async () => healthyStatfs(),
  }).initialize();
  const forecast = JSON.parse(forecastBody().toString("utf8"));

  try {
    const first = store.prepareForecast(forecast, "days=1");
    const expected = Buffer.from(first.compressedObject);
    const second = store.prepareForecast(structuredClone(forecast), "days=1");
    assert.equal(compressionCalls, 1);
    assert.deepEqual(second, first);
    first.compressedObject.fill(0);
    assert.deepEqual(
      store.prepareForecast(structuredClone(forecast), "days=1").compressedObject,
      expected,
    );
    const availabilityForecast = structuredClone(forecast);
    availabilityForecast.data[0].receivedAt = "2026-10-07T00:16:00.000Z";
    const availabilityCapture = store.prepareForecast(availabilityForecast, "days=1");
    assert.equal(compressionCalls, 1);
    assert.equal(availabilityCapture.objectSha256, second.objectSha256);
    assert.equal(availabilityCapture.edgeReceiptIdentitySha256,
      second.edgeReceiptIdentitySha256);
    assert.notDeepEqual(availabilityCapture.availability, second.availability);
    const bufferChannel = store.prepareV2(forecastBody(), "days=1", "public_get");
    const parsedChannel = store.prepareForecastV2(
      structuredClone(forecast),
      "days=1",
      "public_get",
    );
    assert.deepEqual(parsedChannel, bufferChannel);
    assert.equal(compressionCalls, 1);
    assert.equal(store.prepareForecastV2(forecast, "days=1", "invalid"), null);
    assert.equal(compressionCalls, 1);

    // promote the first entry before filling and evicting the second
    const variants = Array.from({ length: 9 }, (_value, index) => {
      const value = structuredClone(forecast);
      value.data[0].metrics.windSpeedMps = index + 20;
      return value;
    });
    store.prepareForecast(variants[0], "days=1");
    store.prepareForecast(structuredClone(forecast), "days=1");
    for (const value of variants.slice(1, 7)) {
      store.prepareForecast(value, "days=1");
    }
    store.prepareForecast(structuredClone(forecast), "days=1");
    store.prepareForecast(variants[7], "days=1");
    const beforeEviction = compressionCalls;
    store.prepareForecast(variants[8], "days=1");
    assert.equal(compressionCalls, beforeEviction + 1);
    store.prepareForecast(variants[0], "days=1");
    assert.equal(compressionCalls, beforeEviction + 2);
    store.prepareForecast(structuredClone(forecast), "days=1");
    assert.equal(compressionCalls, beforeEviction + 2);
  } finally {
    await rm(root, { force: true, recursive: true });
  }

  const collisionRoot = await mkdtemp(join(tmpdir(), "weather-adjustment-compression-collision-"));
  let collisionCompressionCalls = 0;
  const collisionStore = await new AdjustmentEvidenceStore({
    compressObject: (bytes) => {
      collisionCompressionCalls += 1;
      return gzipSync(bytes, { level: 9, mtime: 0 });
    },
    objectSha256: () => "a".repeat(64),
    root: collisionRoot,
    statfs: async () => healthyStatfs(),
  }).initialize();
  const changed = structuredClone(forecast);
  changed.data[0].metrics.windSpeedMps = 99;

  try {
    assert.notEqual(collisionStore.prepareForecast(forecast, "days=1"), null);
    assert.equal(collisionStore.prepareForecast(changed, "days=1"), null);
    assert.notEqual(collisionStore.prepareForecast(forecast, "days=1"), null);
    assert.equal(collisionCompressionCalls, 1);
  } finally {
    await rm(collisionRoot, { force: true, recursive: true });
  }

  const oversizeRoot = await mkdtemp(join(tmpdir(), "weather-adjustment-compression-oversize-"));
  let oversizeCompressionCalls = 0;
  let refuseOversize = false;
  const oversizeStore = await new AdjustmentEvidenceStore({
    compressObject: (bytes) => {
      oversizeCompressionCalls += 1;

      // inject one compressed-size refusal
      if (refuseOversize) {
        return Buffer.alloc(16 * 1_024 + 1);
      }
      return gzipSync(bytes, { level: 9, mtime: 0 });
    },
    root: oversizeRoot,
    statfs: async () => healthyStatfs(),
  }).initialize();
  const oversizeVariants = Array.from({ length: 9 }, (_value, index) => {
    const value = structuredClone(forecast);
    value.data[0].metrics.windSpeedMps = index + 40;
    return value;
  });

  try {
    // fill the cache to its exact bound
    for (const value of oversizeVariants.slice(0, 8)) {
      assert.notEqual(oversizeStore.prepareForecast(value, "days=1"), null);
    }
    assert.equal(oversizeCompressionCalls, 8);
    assert.notEqual(oversizeStore.prepareForecast(oversizeVariants[0], "days=1"), null);
    refuseOversize = true;
    assert.equal(oversizeStore.prepareForecast(oversizeVariants[8], "days=1"), null);
    assert.equal(oversizeCompressionCalls, 9);
    refuseOversize = false;
    assert.notEqual(oversizeStore.prepareForecast(oversizeVariants[1], "days=1"), null);
    assert.equal(oversizeCompressionCalls, 9);
    assert.notEqual(oversizeStore.prepareForecast(oversizeVariants[8], "days=1"), null);
    assert.equal(oversizeCompressionCalls, 10);
    assert.notEqual(oversizeStore.prepareForecast(oversizeVariants[8], "days=1"), null);
    assert.equal(oversizeCompressionCalls, 10);
    assert.equal(oversizeStore.status().objectTooLarge, 1);
  } finally {
    await rm(oversizeRoot, { force: true, recursive: true });
  }
});

// prove frozen v14 can read and duplicate current content-only cache output
test("frozen v14 reader accepts current evidence and rollback duplicate", async () => {
  const parent = await mkdtemp(join(tmpdir(), "weather-adjustment-v14-cross-read-"));
  const root = join(parent, "evidence");
  const frozenPath = join(parent, "adjustment-evidence-store-v14.mjs");
  const frozenSource = spawnSync("git", [
    "show",
    "2026.10.14-1:deploy/scripts/adjustment-evidence-store.mjs",
  ], { cwd: repoRoot, encoding: "buffer" });
  assert.equal(frozenSource.status, 0, frozenSource.stderr?.toString("utf8"));
  await writeFile(frozenPath, frozenSource.stdout, { mode: 0o600 });
  const frozen = await import(`${pathToFileURL(frozenPath).href}?test=${Date.now()}`);
  const current = await new AdjustmentEvidenceStore({
    root,
    statfs: async () => healthyStatfs(),
  }).initialize();
  const body = forecastBody();
  const forecast = JSON.parse(body.toString("utf8"));

  try {
    const prepared = current.prepareForecastV2(forecast, "days=1", "public_get");
    assert.deepEqual(
      await current.commit(prepared, "2026-10-08T00:40:00.000Z"),
      { status: "created" },
    );
    assert.equal((await frozen.freezeAdjustmentEvidenceSnapshot({ root })).entries.length, 1);
    const rolledBack = await new frozen.AdjustmentEvidenceStore({
      root,
      statfs: async () => healthyStatfs(),
    }).initialize();
    const oldPrepared = rolledBack.prepareV2(body, "days=1", "public_get");
    assert.deepEqual(
      await rolledBack.commit(oldPrepared, "2026-10-08T00:41:00.000Z"),
      { status: "duplicate" },
    );
    assert.equal((await freezeAdjustmentEvidenceSnapshot({ root })).entries.length, 1);
  } finally {
    await rm(parent, { force: true, recursive: true });
  }
});

// attach writes only to finish and count an aborted close
test("response tracking never writes on close without finish", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-finish-"));
  const store = await new AdjustmentEvidenceStore({
    now: () => new Date("2026-10-07T01:00:00.000Z"),
    root,
    statfs: async () => healthyStatfs(),
  }).initialize();

  try {
    const aborted = new EventEmitter();
    let abortedStatus;
    store.trackResponse(
      aborted,
      createAdjustmentEvidenceCapture(forecastBody(), "days=1"),
      // retain the genuine close-without-finish outcome
      (result) => { abortedStatus = result.status; },
    );
    aborted.emit("close");
    assert.equal(store.status().closeWithoutFinish, 1);
    assert.equal(abortedStatus, "response_aborted");
    assert.equal((await freezeAdjustmentEvidenceSnapshot({ root })).entries.length, 0);

    const finished = new EventEmitter();
    store.trackResponse(finished, createAdjustmentEvidenceCapture(forecastBody(), "days=1"));
    finished.emit("finish");
    finished.emit("close");
    await new Promise(
      // wait for the asynchronous post-finish write
      (resolveWait) => setTimeout(resolveWait, 20),
    );
    assert.equal((await freezeAdjustmentEvidenceSnapshot({ root })).entries.length, 1);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// normalize only direct daily and overnight API queries
test("window normalization rejects duplicate and caller-controlled shapes", () => {
  assert.equal(normalizeAdjustmentEvidenceWindow(new URL("https://weather.test/forecast")), "days=1");
  assert.equal(normalizeAdjustmentEvidenceWindow(new URL("https://weather.test/forecast?days=5")), "days=5");
  assert.equal(normalizeAdjustmentEvidenceWindow(new URL("https://weather.test/forecast?window=overnight")), "overnight");
  assert.equal(normalizeAdjustmentEvidenceWindow(new URL("https://weather.test/forecast?days=1&days=1")), null);
  assert.equal(normalizeAdjustmentEvidenceWindow(new URL("https://weather.test/forecast?surface=widget")), null);
});

// retain one in-memory scheduler state port across restart tests
function memorySchedulerStateStore(initial = null) {
  let state = initial;
  return {
    // keep the injected port initialization side-effect free
    async initialize() {
      return this;
    },
    // return one detached durable state image
    async read() {
      return state === null ? {
        activation: "not_initialized",
        attempts: [],
        contractVersion: "adjustment-evidence-scheduler-state/v1",
        errors: [],
        gaps: [],
        lastCheckedAt: null,
        updatedAt: null,
      } : structuredClone(state);
    },
    // retain only one detached current state image
    async write(value) {
      state = structuredClone(value);
    },
    // expose test-only durable state
    value() {
      return structuredClone(state);
    },
  };
}

// admit the fall-back DST row only through the v2 object reader
test("v2 capture accepts exactly 241 days=10 rows while v1 stays byte-compatible", async () => {
  const rows = Array.from({ length: 241 },
    // build one maximum fall-back forecast result
    (_value, index) => forecastRow(index, {
      receivedAt: new Date(Date.parse("2026-10-07T00:06:00.000Z") + index * 60_000).toISOString(),
    }));
  assert.throws(
    () => createAdjustmentEvidenceCapture(forecastBody(rows, { days: 10 }), "days=10"),
    /forecast evidence body is invalid/u,
  );
  const capture = createAdjustmentEvidenceCaptureV2(
    forecastBody(rows, { days: 10 }),
    "days=10",
  );
  assert.equal(capture.object.rows.length, 241);
  assert.equal(capture.object.contractVersion, "forecast-adjustment-evidence-object/v2");
  assert.ok(capture.compressedObject.byteLength <= 16 * 1_024);
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-v2-"));
  const store = await new AdjustmentEvidenceStore({
    root,
    statfs: async () => healthyStatfs(),
  }).initialize();

  try {
    const prepared = store.prepareV2(forecastBody(rows, { days: 10 }), "days=10", "scheduler_request");
    assert.deepEqual(
      await store.commit(prepared, "2026-10-08T06:40:00.000Z"),
      { status: "created" },
    );
    const receipt = await readFile(
      join(root, "receipts", `sha256-${prepared.edgeReceiptIdentitySha256}.json`),
    );
    assert.ok(receipt.byteLength <= 2_048);
    assert.equal((await freezeAdjustmentEvidenceSnapshot({ root })).entries.length, 1);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// bind the first trusted channel without relabelling an existing receipt
test("v2 channel assertion keeps separate identity and legacy first receipt", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-channel-"));
  const store = await new AdjustmentEvidenceStore({
    root,
    statfs: async () => healthyStatfs(),
  }).initialize();

  try {
    const prepared = store.prepareV2(forecastBody(), "days=1", "public_get");
    assert.deepEqual(
      await store.commit(prepared, "2026-10-08T00:40:00.000Z"),
      { status: "created" },
    );
    const retry = store.prepareV2(forecastBody(), "days=1", "scheduler_request");
    assert.deepEqual(
      await store.commit(retry, "2026-10-08T06:40:00.000Z"),
      { status: "duplicate" },
    );
    const bindingName = `sha256-${prepared.edgeReceiptIdentitySha256}.json`;
    const binding = JSON.parse(await readFile(join(root, "channel-bindings", bindingName), "utf8"));
    const assertion = JSON.parse(await readFile(
      join(root, "channels", `sha256-${binding.assertionSha256}.json`),
      "utf8",
    ));
    assert.equal(assertion.channel, "public_get");
    assert.notEqual(binding.assertionSha256, prepared.edgeReceiptIdentitySha256);

    const legacyBody = forecastBody([forecastRow(1)]);
    const legacy = createAdjustmentEvidenceCapture(legacyBody, "days=1");
    assert.deepEqual(
      await store.commit(legacy, "2026-10-08T00:41:00.000Z"),
      { status: "created" },
    );
    const migrated = store.prepareV2(legacyBody, "days=1", "scheduler_request");
    assert.deepEqual(
      await store.commit(migrated, "2026-10-08T06:41:00.000Z"),
      { status: "duplicate" },
    );
    const legacyBinding = JSON.parse(await readFile(
      join(root, "channel-bindings", `sha256-${legacy.edgeReceiptIdentitySha256}.json`),
      "utf8",
    ));
    const legacyAssertion = JSON.parse(await readFile(
      join(root, "channels", `sha256-${legacyBinding.assertionSha256}.json`),
      "utf8",
    ));
    assert.equal(legacyAssertion.channel, "legacy_unattributed");
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// calculate fixed UTC cycles with deterministic bounded jitter
test("scheduler selects one jittered current due within the authenticated rolling horizon", () => {
  const due = currentAdjustmentEvidenceDue(
    "2026-10-08T06:40:00.000Z", ROLLING_CAPTURE_END,
  );
  assert.equal(due.dueKey, "capture/2026-10-08T06:35:00.000Z");
  assert.ok(due.jitterSeconds >= 0 && due.jitterSeconds <= 299);
  assert.equal(due.scheduledAt, "2026-10-08T06:38:22.000Z");
  const beforeJitter = currentAdjustmentEvidenceDue(
    "2026-10-08T06:36:00.000Z", ROLLING_CAPTURE_END,
  );
  assert.equal(beforeJitter.dueKey, "capture/2026-10-08T00:35:00.000Z");
  assert.equal(currentAdjustmentEvidenceDue(
    "2027-10-08T06:40:00.000Z", ROLLING_CAPTURE_END,
  ).dueKey, "capture/2027-10-08T06:35:00.000Z");
  assert.equal(currentAdjustmentEvidenceDue(ROLLING_CAPTURE_END, ROLLING_CAPTURE_END), null);
});

// block activation at the measured Blueberry free-space value
test("scheduler capacity gate rejects the measured host headroom", async () => {
  const mismatchedSchedule = rollingScheduleSlots();
  mismatchedSchedule[2] = {
    ...mismatchedSchedule[2],
    horizonEndAt: "2028-10-09T07:00:00.000Z",
  };
  const scheduleRefusal = await evaluateAdjustmentEvidenceSchedulerAdmission({
    enabled: true,
    migrationReady: true,
    now: new Date("2026-10-08T06:40:00.000Z"),
    root: "/unused",
    scheduleSlots: mismatchedSchedule,
    // detect any capacity probe before schedule authority succeeds
    statfs: async () => { throw new Error("unexpected capacity probe"); },
  });
  assert.equal(scheduleRefusal.reason, "registration_schedule_unavailable");
  const admission = await evaluateAdjustmentEvidenceSchedulerAdmission({
    enabled: true,
    migrationReady: true,
    now: new Date("2026-10-08T06:40:00.000Z"),
    root: "/unused",
    scheduleSlots: rollingScheduleSlots(),
    statfs: async () => ({
      bavail: 1_950_453_760n / 4_096n,
      bsize: 4_096n,
      ffree: 100_000n,
    }),
  });
  assert.equal(admission.active, false);
  assert.equal(admission.reason, "capacity_blocked");
  assert.equal(ADJUSTMENT_EVIDENCE_SCHEDULER_REQUIRED_FREE_BYTES, 2_034_155_520);

  const exact = await evaluateAdjustmentEvidenceSchedulerAdmission({
    enabled: true,
    migrationReady: true,
    now: new Date("2026-10-08T06:40:00.000Z"),
    root: "/unused",
    scheduleSlots: rollingScheduleSlots(),
    // expose the exact protected-floor plus reservation boundary
    statfs: async () => ({
      bavail: BigInt(ADJUSTMENT_EVIDENCE_SCHEDULER_REQUIRED_FREE_BYTES),
      bsize: 1n,
      ffree: 32_768n,
    }),
  });
  assert.equal(exact.active, true);
  const oneByteUnder = await evaluateAdjustmentEvidenceSchedulerAdmission({
    enabled: true,
    migrationReady: true,
    now: new Date("2026-10-08T06:40:00.000Z"),
    root: "/unused",
    scheduleSlots: rollingScheduleSlots(),
    // expose one byte less than the complete readiness boundary
    statfs: async () => ({
      bavail: BigInt(ADJUSTMENT_EVIDENCE_SCHEDULER_REQUIRED_FREE_BYTES - 1),
      bsize: 1n,
      ffree: 32_768n,
    }),
  });
  assert.equal(oneByteUnder.reason, "capacity_blocked");
  const oneBlockUnder = await evaluateAdjustmentEvidenceSchedulerAdmission({
    enabled: true,
    migrationReady: true,
    now: new Date("2026-10-08T06:40:00.000Z"),
    root: "/unused",
    scheduleSlots: rollingScheduleSlots(),
    // expose one filesystem block less than the readiness boundary
    statfs: async () => ({
      bavail: BigInt(ADJUSTMENT_EVIDENCE_SCHEDULER_REQUIRED_FREE_BYTES / 4_096 - 1),
      bsize: 4_096n,
      ffree: 32_768n,
    }),
  });
  assert.equal(oneBlockUnder.reason, "capacity_blocked");
  const protectedFloorOnly = await evaluateAdjustmentEvidenceSchedulerAdmission({
    enabled: true,
    migrationReady: true,
    now: new Date("2026-10-08T06:40:00.000Z"),
    root: "/unused",
    scheduleSlots: rollingScheduleSlots(),
    // expose the protected floor without the mandatory reservation
    statfs: async () => ({
      bavail: 2_030_043_136n,
      bsize: 1n,
      ffree: 32_768n,
    }),
  });
  assert.equal(protectedFloorOnly.reason, "capacity_blocked");
});

// run only the current catch-up and record older cycles as permanent gaps
test("scheduler catch-up runs once at current time without backdating", async () => {
  let now = new Date("2026-10-08T18:40:00.000Z");
  const stateStore = memorySchedulerStateStore({
    activation: "active",
    attempts: [],
    contractVersion: "adjustment-evidence-scheduler-state/v1",
    errors: [],
    gaps: [],
    lastCheckedAt: "2026-10-08T00:40:00.000Z",
    updatedAt: "2026-10-08T00:40:00.000Z",
  });
  const triggers = [];
  const scheduler = await new AdjustmentEvidenceScheduler({
    enabled: true,
    migrationReady: true,
    now: () => now,
    readScheduleSlots: async () => rollingScheduleSlots(),
    root: "/unused",
    stateStore,
    statfs: async () => healthyStatfs(),
    trigger: async (input) => {
      triggers.push(input);
      return { status: "created" };
    },
  }).initialize();
  const result = await scheduler.runDue();
  assert.equal(result.dueKey, "capture/2026-10-08T18:35:00.000Z");
  assert.equal(triggers[0].commitAt, now.toISOString());
  assert.equal(triggers[0].path, "/api/v1/sites/ballydidean/forecast?days=10");
  assert.deepEqual(
    stateStore.value().gaps.map((gap) => gap.dueKey),
    [
      "capture/2026-10-08T06:35:00.000Z",
      "capture/2026-10-08T12:35:00.000Z",
    ],
  );
  const restarted = await new AdjustmentEvidenceScheduler({
    enabled: true,
    migrationReady: true,
    now: () => now,
    readScheduleSlots: async () => rollingScheduleSlots(),
    root: "/unused",
    stateStore,
    statfs: async () => healthyStatfs(),
    trigger: async (input) => {
      triggers.push(input);
      return { status: "created" };
    },
  }).initialize();
  assert.equal((await restarted.runDue()).status, "current_already_processed");
  assert.equal(triggers.length, 1);
  now = new Date("2026-10-08T12:40:00.000Z");
  assert.equal((await restarted.runDue()).status, "clock_rollback");
  assert.equal(triggers.length, 1);
  now = new Date("2026-10-08T18:41:00.000Z");
});

// trigger the first clock tick without any incoming public request
test("scheduler start is traffic-independent and keeps bounded restart history", async () => {
  let now = new Date("2026-10-08T00:40:00.000Z");
  const stateStore = memorySchedulerStateStore();
  const triggers = [];
  let intervalCallback;
  let intervalCleared = false;
  const timer = { unref() {} };
  const scheduler = await new AdjustmentEvidenceScheduler({
    clearInterval: () => { intervalCleared = true; },
    enabled: true,
    migrationReady: true,
    now: () => now,
    readScheduleSlots: async () => rollingScheduleSlots(),
    root: "/unused",
    setInterval: (callback) => {
      intervalCallback = callback;
      return timer;
    },
    stateStore,
    statfs: async () => healthyStatfs(),
    trigger: async (input) => {
      triggers.push(input);
      const error = new Error("injected capture failure");
      error.code = "injected_failure";
      throw error;
    },
  }).initialize();
  assert.equal(scheduler.start(), true);
  await new Promise(
    // wait for the immediate asynchronous startup tick
    (resolveWait) => setTimeout(resolveWait, 10),
  );
  assert.equal(triggers.length, 1);
  assert.equal(typeof intervalCallback, "function");

  // exercise enough later cycles to prove bounded retention
  for (let index = 1; index < 40; index += 1) {
    now = new Date(Date.parse("2026-10-08T00:40:00.000Z") + index * 6 * 60 * 60 * 1_000);
    await scheduler.runDue();
  }
  assert.equal(scheduler.status().attempts.length, 40);
  assert.equal(scheduler.status().errors.length, 40);
  assert.equal(scheduler.status().gaps.length, 40);
  scheduler.stop();
  assert.equal(intervalCleared, true);
});

// keep only current and next payload slots and fsync a gap on refusal
test("online page disk ports rotate two slots and fail open on a third", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-pages-"));
  const ports = await new AdjustmentCyclePageDiskPorts({
    now: () => new Date("2026-10-08T00:40:00.000Z"),
    root,
  }).initialize();
  let state = createCyclePageState({
    dailyPageCount: 0,
    dueKey: "capture/2026-10-08T00:35:00.000Z",
    generation: "1",
    localDate: "2026-10-08",
  });

  try {
    const identities = ["1".repeat(64), "2".repeat(64), "3".repeat(64)];

    // fill the exact current and next slots
    for (let index = 0; index < 2; index += 1) {
      const result = await appendCyclePageFailOpen(state, {
        payload: Buffer.from(`page-${String(index)}`),
        projections: [{ channel: "scheduler_request", identitySha256: identities[index] }],
      }, ports);
      assert.equal(result.servingBlocked, false);
      assert.equal(result.status, "page_pending_acknowledgement");
      state = result.state;
    }

    const refused = await appendCyclePageFailOpen(state, {
      payload: Buffer.from("page-2"),
      projections: [{ channel: "scheduler_request", identitySha256: identities[2] }],
    }, ports);
    assert.equal(refused.servingBlocked, false);
    assert.equal(refused.status, "evidence_gap");
    state = refused.state;
    assert.deepEqual((await readdir(join(root, "online-pages", "slots"))).sort(), [
      "current.page",
      "next.page",
    ]);
    state = await ackCyclePage(state, {
      acknowledgedAt: "2026-10-08T00:41:00.000Z",
      pageSha256: state.pages[0].pageSha256,
    }, ports);
    assert.deepEqual(await readdir(join(root, "online-pages", "slots")), ["current.page"]);
    assert.equal(state.slots.length, 1);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// preserve serving when page and gap persistence both refuse capacity
test("online page capacity refusal stays fail open and honest", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-page-capacity-"));
  const ports = await new AdjustmentCyclePageDiskPorts({
    now: () => new Date("2026-10-08T00:40:00.000Z"),
    root,
    statfs: async () => ({
      bavail: 1_950_453_760n,
      bsize: 1n,
      ffree: 100_000n,
    }),
  }).initialize();
  const state = createCyclePageState({
    dailyPageCount: 0,
    dueKey: "capture/2026-10-08T00:35:00.000Z",
    generation: "1",
    localDate: "2026-10-08",
  });

  try {
    const result = await appendCyclePageFailOpen(state, {
      payload: Buffer.from("refused-page"),
      projections: [{ channel: "scheduler_request", identitySha256: "4".repeat(64) }],
    }, ports);
    assert.equal(result.servingBlocked, false);
    assert.equal(result.status, "evidence_gap_persistence_refused");
    assert.equal(result.reason, "capacity_refused");
    assert.deepEqual(await readdir(join(root, "online-pages", "slots")), []);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// transfer one committed scheduler capture through page checkpoint and genuine final graph
test("archive transport exports canonical page and final envelopes with idempotent acknowledgements", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-archive-transfer-"));
  const now = new Date("2026-10-08T00:40:00.123Z");
  const store = await new AdjustmentEvidenceStore({
    now: () => now,
    root,
    statfs: async () => healthyStatfs(),
  }).initialize();
  const prepared = store.prepareV2(forecastBody(), "days=10", "scheduler_request");

  try {
    assert.notEqual(prepared, null);
    assert.deepEqual(await store.commit(prepared, now.toISOString()), { status: "created" });
    const transport = await new AdjustmentEvidenceArchiveTransport({
      now: () => now,
      root,
      statfs: async () => healthyStatfs(),
    }).initialize();
    const captured = await transport.captureCommittedSchedulerEvidence({
      dueKey: "capture/2026-10-08T00:35:00.000Z",
      prepared,
    });
    assert.equal(captured.status, "page_pending_acknowledgement");
    const firstEnvelope = await transport.next();
    assert.equal(firstEnvelope.contractVersion, ADJUSTMENT_ARCHIVE_TRANSFER_CONTRACT_VERSION);
    assert.equal(firstEnvelope.kind, "page");
    assert.equal(firstEnvelope.header.dueKey, "capture/2026-10-08T00:35:00.000Z");
    assert.deepEqual(firstEnvelope.projections, [{
      channel: "scheduler_request",
      identitySha256: prepared.edgeReceiptIdentitySha256,
    }]);
    const payload = JSON.parse(Buffer.from(firstEnvelope.payloadBase64, "base64").toString("utf8"));
    assert.equal(payload.contractVersion, "adjustment-evidence-page-payload/v1");
    assert.equal(payload.edgeReceiptIdentitySha256, prepared.edgeReceiptIdentitySha256);

    const restarted = await new AdjustmentEvidenceArchiveTransport({
      now: () => new Date("2026-10-08T00:41:00.456Z"),
      root,
      statfs: async () => healthyStatfs(),
    }).initialize();
    assert.deepEqual(await restarted.next(), firstEnvelope);
    const checkpointSha256 = "9".repeat(64);
    const nextOutput = [];
    await runAdjustmentArchiveCommand(["archive-next"], {
      now: () => new Date("2026-10-08T00:41:00.456Z"),
      root,
      statfs: async () => healthyStatfs(),
      stdout: { write: (bytes) => nextOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(JSON.parse(Buffer.concat(nextOutput).toString("utf8")), firstEnvelope);
    const ackOutput = [];
    // simulate a crash after durable checkpoint binding but before slot release
    const crashPorts = await new AdjustmentCyclePageDiskPorts({
      now: () => new Date("2026-10-08T00:41:00.456Z"),
      root,
      statfs: async () => healthyStatfs(),
    }).initialize();
    await crashPorts.persistTransferAcknowledgement({
      acknowledgedAt: "2026-10-08T00:41:00.456Z",
      checkpointSha256,
      contractVersion: "adjustment-archive-transfer-ack/v1",
      header: firstEnvelope.header,
      pageSha256: firstEnvelope.pageSha256,
      payloadLength: Buffer.from(firstEnvelope.payloadBase64, "base64").length,
      projections: firstEnvelope.projections,
    });
    await runAdjustmentArchiveCommand([
      "archive-ack",
      firstEnvelope.pageSha256,
      checkpointSha256,
    ], {
      now: () => new Date("2026-10-08T00:41:00.456Z"),
      root,
      statfs: async () => healthyStatfs(),
      stdout: { write: (bytes) => ackOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(ackOutput, []);
    // a page checkpoint cannot close unfinished body and revision producers
    assert.equal((await restarted.next()).kind, "idle");
    const emptyRoot = createHash("sha256").update("[]\n").digest("hex");
    const requiredInputs = Object.fromEntries([
      "actual_best_match", "native_source", "rain_gate_input", "serving_evidence",
      "shadow_body", "shadow_metadata", "target_revision",
    ].map(
      // declare every input class explicitly for this transport-only fixture
      (kind) => [kind, kind === "serving_evidence"
        ? { count: 0, rootSha256: emptyRoot }
        : { count: 1, rootSha256: createHash("sha256").update(kind).digest("hex") }],
    ));
    requiredInputs.serving_evidence = { count: 1, rootSha256: prepared.edgeReceiptIdentitySha256 };
    const seal = await restarted.sealCycleInputs({
      dueKey: firstEnvelope.header.dueKey,
      generation: firstEnvelope.header.generation,
      requiredInputs,
    });
    assert.deepEqual(await restarted.sealCycleInputs({
      dueKey: firstEnvelope.header.dueKey,
      generation: firstEnvelope.header.generation,
      requiredInputs,
    }), seal);
    await assert.rejects(restarted.sealCycleInputs({
      dueKey: firstEnvelope.header.dueKey,
      generation: firstEnvelope.header.generation,
      requiredInputs: { ...requiredInputs,
        target_revision: { count: 1, rootSha256: "5".repeat(64) } },
    }), /collision|bytes|content/u);
    const finalEnvelope = await restarted.next();
    assert.equal(finalEnvelope.contractVersion, ADJUSTMENT_ARCHIVE_TRANSFER_CONTRACT_VERSION);
    assert.equal(finalEnvelope.kind, "final");
    assert.equal(finalEnvelope.manifest.dueKey, firstEnvelope.header.dueKey);
    assert.equal(finalEnvelope.manifest.pageCount, 1);
    assert.equal(finalEnvelope.manifest.contractVersion, "adjustment-cycle-final-manifest/v2");
    assert.deepEqual(finalEnvelope.manifest.inputSeal, seal);
    assert.equal(finalEnvelope.manifest.pages[0].pageSha256, firstEnvelope.pageSha256);
    const finalGraphSha256 = "7".repeat(64);
    const finalAckOutput = [];
    await runAdjustmentArchiveCommand([
      "archive-ack-final",
      finalEnvelope.manifestSha256,
      finalGraphSha256,
    ], {
      now: () => new Date("2026-10-08T00:42:00.789Z"),
      root,
      statfs: async () => healthyStatfs(),
      stdout: { write: (bytes) => finalAckOutput.push(Buffer.from(bytes)) },
    });
    assert.deepEqual(finalAckOutput, []);
    const genesisFrontierSha256 = createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
    const revisionReceipts = [];
    let predecessorFrontierSha256 = genesisFrontierSha256;
    // build all four genuine revision classes into one server-shaped chain
    for (const [index, projectionKind] of ["actual_best_match", "native_source",
      "rain_gate_input", "target_revision"].entries()) {
      const revisionReceipt = {
        archiveCommitOrdinal: String(index + 1),
        archiveCommittedAt: `2026-10-08T00:4${index}:00.000Z`,
        contractVersion: "adjustment-revision-commit-receipt/v1",
        frontierSha256: "",
        predecessorFrontierSha256,
        projectionIdentitySha256: String(index + 1).repeat(64),
        projectionKind,
        projectionSha256: String(index + 1).repeat(64),
        receiptSha256: "",
        stageReceiptSha256: String(index + 6).repeat(64),
      };
      revisionReceipt.receiptSha256 = createHash("sha256").update([
        revisionReceipt.contractVersion,
        revisionReceipt.archiveCommitOrdinal,
        revisionReceipt.archiveCommittedAt,
        revisionReceipt.projectionKind,
        revisionReceipt.projectionIdentitySha256,
        revisionReceipt.projectionSha256,
        revisionReceipt.stageReceiptSha256,
        revisionReceipt.predecessorFrontierSha256,
      ].join("\n")).digest("hex");
      revisionReceipt.frontierSha256 = createHash("sha256").update([
        "adjustment-revision-frontier/v1",
        revisionReceipt.predecessorFrontierSha256,
        revisionReceipt.archiveCommitOrdinal,
        revisionReceipt.receiptSha256,
      ].join("\n")).digest("hex");
      revisionReceipts.push(revisionReceipt);
      predecessorFrontierSha256 = revisionReceipt.frontierSha256;
    }
    const revisionFrontier = deriveAdjustmentRevisionCatalogFrontier(revisionReceipts);
    const revisionSnapshot = {
      archiveCommitOrdinal: revisionFrontier.archiveCommitOrdinal,
      contractVersion: "adjustment-revision-serving-snapshot/v1",
      cutoffAt: "2026-10-08T00:45:00.000Z",
      entries: revisionReceipts.map((receipt, index) => ({
        logicalReceivedAt: `2026-10-08T00:3${index}:00.000Z`,
        receipt,
        relation: index === 0 ? "weather_records" : index === 1
          ? "forecast_anchor_records" : index === 2
            ? "rain_adjustment_runs" : "weather_records",
      })),
      entryCount: revisionReceipts.length,
      frontierSha256: revisionFrontier.frontierRootSha256,
      snapshotSha256: "",
    };
    revisionSnapshot.snapshotSha256 = createHash("sha256").update([
      revisionSnapshot.contractVersion,
      revisionSnapshot.cutoffAt,
      revisionSnapshot.archiveCommitOrdinal,
      revisionSnapshot.frontierSha256,
      revisionReceipts.map((receipt) => receipt.receiptSha256).join("\n"),
    ].join("\n")).digest("hex");
    assert.equal(validateAdjustmentRevisionServingSnapshot(revisionSnapshot), revisionSnapshot);
    const coldCatalog = verifyAdjustmentRevisionColdCatalog({
      receipts: revisionReceipts,
      servingSnapshot: revisionSnapshot,
    });
    assert.equal(coldCatalog.frontierCount, 4);
    assert.match(coldCatalog.catalogRootSha256, /^[a-f0-9]{64}$/u);
    assert.throws(() => validateAdjustmentRevisionServingSnapshot({
      ...revisionSnapshot,
      archiveCommitOrdinal: "3",
    }), /ineligible/u);
    assert.throws(
      () => deriveAdjustmentRevisionCatalogFrontier([{
        ...revisionReceipts[0],
        projectionSha256: "0".repeat(64),
      }]),
      /chain|projection identity/u,
    );
    const anchorStore = new AdjustmentMaintenanceAnchorStore({ root });
    const absentAnchorStatus = await anchorStore.status();
    assert.equal(absentAnchorStatus.schemaReadiness, "not_established");
    const anchor = {
      actionSha256: "c".repeat(64),
      archiveObjectSha256: "1".repeat(64),
      catalogGeneration: "1",
      catalogRootSha256: "2".repeat(64),
      catalogWatermarkSha256: "3".repeat(64),
      contractVersion: "adjustment-maintenance-anchor/v2",
      controlSha256: "0".repeat(64),
      controlVersion: "13",
      ctfState: "transferred",
      dueKey: finalEnvelope.manifest.dueKey,
      frontierCount: revisionFrontier.frontierCount,
      frontierRootSha256: revisionFrontier.frontierRootSha256,
      fullGraphVerifiedAt: "2026-10-08T00:42:00.700Z",
      fullMemberRootSha256: "5".repeat(64),
      generation: finalEnvelope.manifest.generation,
      graphManifestSha256: finalGraphSha256,
      lifecycleLedgerRootSha256: "6".repeat(64),
      manifestSha256: finalEnvelope.manifestSha256,
      predecessorAnchorSha256: null,
      publishedAt: "2026-10-08T00:43:00.000Z",
      reportSha256: "d".repeat(64),
      revisionSnapshotRootSha256: "e".repeat(64),
      sequence: "0",
      sourceCommit: "a".repeat(40),
      workstationJournalHeadSha256: "8".repeat(64),
    };
    const anchorBytes = Buffer.from(`${JSON.stringify(Object.fromEntries(
      Object.entries(anchor).sort(([left], [right]) => left.localeCompare(right, "en")),
    ))}\n`);
    const anchorSha256 = createHash("sha256").update(anchorBytes).digest("hex");
    assert.deepEqual(await anchorStore.installTransferred(anchorBytes, anchorSha256), {
      anchorSha256,
      state: "transferred",
    });
    const mismatchedControlStore = new AdjustmentMaintenanceAnchorStore({
      expectedControlSha256: "f".repeat(64),
      expectedControlVersion: "13",
      root,
    });
    await assert.rejects(
      mismatchedControlStore.installTransferred(anchorBytes, anchorSha256),
      /control identity/u,
    );
    const mismatchedVersionStore = new AdjustmentMaintenanceAnchorStore({
      expectedControlSha256: anchor.controlSha256,
      expectedControlVersion: "12",
      root,
    });
    await assert.rejects(
      mismatchedVersionStore.installTransferred(anchorBytes, anchorSha256),
      /control identity/u,
    );
    const anchorStatus = await anchorStore.status();
    assert.equal(anchorStatus.actionEligible, false);
    assert.equal(anchorStatus.rootState, "verified");
    assert.equal(anchorStatus.schemaReadiness, "transferred_anchor_only");
    assert.equal(anchorStatus.slots.current.sha256, anchorSha256);
    assert.equal(anchorStatus.slots.pending.state, "absent");
    assert.equal(
      (await lstat(join(root, "maintenance-anchors", "current.json"))).mode & 0o777,
      0o600,
    );
    const finalizedBytes = Buffer.from(anchorBytes.toString("utf8").replace(
      '"ctfState":"transferred"',
      '"ctfState":"finalized"',
    ));
    await assert.rejects(
      anchorStore.installTransferred(
        finalizedBytes,
        createHash("sha256").update(finalizedBytes).digest("hex"),
      ),
      /contract|identity|keys/u,
    );
    const wrongGraphBytes = Buffer.from(anchorBytes.toString("utf8").replace(
      `"graphManifestSha256":"${finalGraphSha256}"`,
      `"graphManifestSha256":"${"0".repeat(64)}"`,
    ));
    await assert.rejects(
      anchorStore.installTransferred(
        wrongGraphBytes,
        createHash("sha256").update(wrongGraphBytes).digest("hex"),
      ),
      /transfer binding/u,
    );
    await assert.rejects(anchorStore.finalizeRetirement(), /SHA256/u);
    await runAdjustmentArchiveCommand([
      "maintenance-anchor-install-v2",
      anchorSha256,
    ], { root, stdinBytes: anchorBytes });
    assert.equal((await readdir(join(root, "receipts"))).length, 1);
    assert.equal((await readdir(join(root, "channel-bindings"))).length, 1);
    assert.deepEqual(await restarted.next(), {
      contractVersion: ADJUSTMENT_ARCHIVE_TRANSFER_CONTRACT_VERSION,
      kind: "idle",
    });
    await restarted.acknowledge(firstEnvelope.pageSha256, checkpointSha256);
    await assert.rejects(
      restarted.acknowledge(firstEnvelope.pageSha256, "8".repeat(64)),
      /acknowledgement collision/u,
    );
    await restarted.acknowledgeFinal(finalEnvelope.manifestSha256, finalGraphSha256);
    await assert.rejects(
      restarted.acknowledgeFinal(finalEnvelope.manifestSha256, "6".repeat(64)),
      /final acknowledgement collision/u,
    );
    const retirementSources = [
      ["archive_acknowledgement", firstEnvelope.pageSha256,
        join(root, "online-pages", "archive-acknowledgements",
          `sha256-${firstEnvelope.pageSha256}.json`)],
      ["final_acknowledgement", finalEnvelope.manifestSha256,
        join(root, "online-pages", "final-acknowledgements",
          `sha256-${finalEnvelope.manifestSha256}.json`)],
      ["final_manifest", finalEnvelope.manifestSha256,
        join(root, "online-pages", "final-manifests",
          `sha256-${finalEnvelope.manifestSha256}.json`)],
    ];
    const acknowledgementName = (await readdir(
      join(root, "online-pages", "acknowledgements"),
    ))[0];
    retirementSources.push([
      "cycle_acknowledgement",
      acknowledgementName.slice(7, -5),
      join(root, "online-pages", "acknowledgements", acknowledgementName),
    ]);
    const sealName = (await readdir(join(root, "online-pages", "input-seals")))[0];
    retirementSources.push([
      "input_seal",
      sealName.slice(7, -5),
      join(root, "online-pages", "input-seals", sealName),
    ]);
    const retirementEntries = [];
    // bind every exact hot file to one byte-identical cold member
    for (const [kind, identitySha256, path] of retirementSources) {
      const fileSha256 = createHash("sha256").update(await readFile(path)).digest("hex");
      retirementEntries.push({ coldMemberSha256: fileSha256, fileSha256,
        identitySha256, kind });
    }
    retirementEntries.sort((left, right) => left.kind.localeCompare(right.kind, "en") ||
      left.identitySha256.localeCompare(right.identitySha256, "en"));
    const proof = {
      actionSha256: anchor.actionSha256,
      archiveCommitOrdinal: revisionFrontier.archiveCommitOrdinal,
      burnSha256: "f".repeat(64),
      catalogGeneration: anchor.catalogGeneration,
      catalogRevisionReceiptRootSha256: revisionFrontier.receiptRootSha256,
      catalogRootSha256: anchor.catalogRootSha256,
      catalogWatermarkSha256: anchor.catalogWatermarkSha256,
      confirmationChunkCount: 27,
      contractVersion: "adjustment-maintenance-finalization-proof/v1",
      controlSha256: anchor.controlSha256,
      controlVersion: anchor.controlVersion,
      finalizedAt: "2026-10-08T00:44:00.000Z",
      frontierCount: revisionFrontier.frontierCount,
      frontierRootSha256: revisionFrontier.frontierRootSha256,
      fullMemberRootSha256: anchor.fullMemberRootSha256,
      graphManifestSha256: anchor.graphManifestSha256,
      inputSealSha256: createHash("sha256").update(canonicalBytes(seal)).digest("hex"),
      lifecycleLedgerRootSha256: anchor.lifecycleLedgerRootSha256,
      manifestSha256: anchor.manifestSha256,
      reportSha256: anchor.reportSha256,
      requiredInputRootSha256: createHash("sha256")
        .update(canonicalBytes(requiredInputs)).digest("hex"),
      retirementEntries,
      retirementSetRootSha256: createHash("sha256")
        .update(canonicalBytes(retirementEntries)).digest("hex"),
      revisionSnapshotRootSha256: anchor.revisionSnapshotRootSha256,
      sourceCommit: anchor.sourceCommit,
      transferredAnchorSha256: anchorSha256,
      workstationJournalHeadSha256: anchor.workstationJournalHeadSha256,
    };
    const proofBytes = canonicalBytes(proof);
    const proofSha256 = createHash("sha256").update(proofBytes).digest("hex");
    assert.deepEqual(await anchorStore.finalizeRetirement(proofBytes, proofSha256), {
      anchorSha256: (await anchorStore.status()).slots.current.sha256,
      retiredFiles: 5,
      retirementFileCount: 5,
      status: "finalized",
    });
    const finalizedStatus = await anchorStore.status();
    assert.equal(finalizedStatus.actionEligible, false);
    assert.equal(finalizedStatus.schemaReadiness, "finalized_anchor_installed");
    assert.equal((await readdir(join(root, "online-pages", "final-manifests"))).length, 0);
    assert.equal((await readdir(join(root, "receipts"))).length, 1);
    assert.equal((await readdir(join(root, "channel-bindings"))).length, 1);
    assert.equal((await anchorStore.authorizeQualifiedAction({
      actionSha256: anchor.actionSha256,
      burnSha256: proof.burnSha256,
      controlSha256: anchor.controlSha256,
      controlVersion: anchor.controlVersion,
      fullMemberRootSha256: anchor.fullMemberRootSha256,
      lifecycleLedgerRootSha256: anchor.lifecycleLedgerRootSha256,
      reportSha256: anchor.reportSha256,
      revisionSnapshotRootSha256: anchor.revisionSnapshotRootSha256,
      sourceCommit: anchor.sourceCommit,
      workstationJournalHeadSha256: anchor.workstationJournalHeadSha256,
    })).ctfState, "finalized");
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// keep mixed inputs open until the complete producer set is explicitly sealed
test("combined cycle waits for explicit input seal across page acknowledgements and restart", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-combined-cycle-"));
  const dueKey = "capture/2026-10-08T00:35:00.000Z";
  const now = () => new Date("2026-10-08T00:45:00.000Z");
  const ports = await new AdjustmentCyclePageDiskPorts({ root, now,
    statfs: async () => healthyStatfs() }).initialize();
  let state = createCyclePageState({ dailyPageCount: 0, dueKey, generation: "7",
    localDate: "2026-10-08" });
  try {
    let transport = await new AdjustmentEvidenceArchiveTransport({ root, now,
      statfs: async () => healthyStatfs() }).initialize();
    // capture two distinct causal producers without introducing a third payload slot
    for (const [index, channel] of ["scheduler_request", "organic"].entries()) {
      state = await appendCyclePage(state, {
        payload: Buffer.from(`producer-${index}`),
        projections: [{ channel, identitySha256: String(index + 1).repeat(64) }],
      }, ports);
      const envelope = await transport.next();
      assert.equal(envelope.kind, "page");
      const receipt = await transport.acknowledge(envelope.pageSha256, "9".repeat(64));
      state = await ackCyclePage(state, { acknowledgedAt: receipt.acknowledgedAt,
        pageSha256: envelope.pageSha256 }, {
        // mirror the already durable receipt into the test's protocol state
        persistAcknowledgement: async (input) => ({ fsynced: true,
          acknowledgementSha256: input.acknowledgementSha256 }),
      });
      assert.equal((await transport.next()).kind, "idle");
      transport = await new AdjustmentEvidenceArchiveTransport({ root, now,
        statfs: async () => healthyStatfs() }).initialize();
    }
    const emptyRoot = createHash("sha256").update("[]\n").digest("hex");
    const requiredInputs = Object.fromEntries([
      "actual_best_match", "native_source", "rain_gate_input", "serving_evidence",
      "shadow_body", "shadow_metadata", "target_revision",
    ].map(
      // explicitly declare every producer class in this transport fixture
      (kind) => [kind, { count: 0, rootSha256: emptyRoot }],
    ));
    requiredInputs.serving_evidence = { count: 1, rootSha256: "1".repeat(64) };
    requiredInputs.native_source = { count: 1, rootSha256: "2".repeat(64) };
    const seal = await transport.sealCycleInputs({ dueKey, generation: "7", requiredInputs });
    const final = await transport.next();
    assert.equal(final.kind, "final");
    assert.equal(final.manifest.pageCount, 2);
    assert.equal(final.manifest.acknowledgedProjectionCount, 2);
    assert.deepEqual(final.manifest.inputSeal, seal);
    assert.equal(final.manifest.pages[1].predecessorPageSha256,
      final.manifest.pages[0].pageSha256);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// preserve the immutable acknowledged prefix when the next producer appends later
test("transport append after checkpoint continues the original page chain", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-prefix-"));
  const dueKey = "capture/2026-10-08T00:35:00.000Z";
  const now = () => new Date("2026-10-08T00:40:00.000Z");
  const store = await new AdjustmentEvidenceStore({ root, now,
    statfs: async () => healthyStatfs() }).initialize();
  try {
    const first = store.prepareV2(forecastBody(), "days=10", "scheduler_request");
    const second = store.prepareV2(forecastBody([forecastRow(0), forecastRow(1)]),
      "days=10", "scheduler_request");
    await store.commit(first, now().toISOString());
    await store.commit(second, now().toISOString());
    let transport = await new AdjustmentEvidenceArchiveTransport({ root, now,
      statfs: async () => healthyStatfs() }).initialize();
    await transport.captureCommittedSchedulerEvidence({ dueKey, prepared: first });
    const page = await transport.next();
    await transport.acknowledge(page.pageSha256, "9".repeat(64));
    transport = await new AdjustmentEvidenceArchiveTransport({ root, now,
      statfs: async () => healthyStatfs() }).initialize();
    assert.equal((await transport.captureCommittedSchedulerEvidence({
      dueKey, prepared: first,
    })).status, "page_checkpointed");
    assert.equal((await transport.captureCommittedSchedulerEvidence({
      dueKey, prepared: second,
    })).status, "page_pending_acknowledgement");
    const successor = await transport.next();
    assert.equal(successor.header.pageIndex, 1);
    assert.equal(successor.header.predecessorPageSha256, page.pageSha256);
    assert.equal(successor.header.payloadOffset, Buffer.from(page.payloadBase64, "base64").length);
    assert.deepEqual(await readdir(join(root, "online-pages", "gaps")), []);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// preserve the first pending page and record a permanent gap for a later due cycle
test("archive transport fails open without a third spool or after-valid backfill", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-archive-gap-"));
  const store = await new AdjustmentEvidenceStore({
    root,
    statfs: async () => healthyStatfs(),
  }).initialize();
  const first = store.prepareV2(forecastBody(), "days=10", "scheduler_request");
  const secondBody = forecastBody([forecastRow(0), forecastRow(1)]);
  const second = store.prepareV2(secondBody, "days=10", "scheduler_request");

  try {
    assert.notEqual(first, null);
    assert.notEqual(second, null);
    await store.commit(first, "2026-10-08T00:40:00.123Z");
    await store.commit(second, "2026-10-08T06:40:00.123Z");
    const transport = await new AdjustmentEvidenceArchiveTransport({
      now: () => new Date("2026-10-08T06:40:00.123Z"),
      root,
      statfs: async () => healthyStatfs(),
    }).initialize();
    assert.equal((await transport.captureCommittedSchedulerEvidence({
      dueKey: "capture/2026-10-08T00:35:00.000Z",
      prepared: first,
    })).status, "page_pending_acknowledgement");
    const refused = await transport.captureCommittedSchedulerEvidence({
      dueKey: "capture/2026-10-08T06:35:00.000Z",
      prepared: second,
    });
    assert.equal(refused.status, "evidence_gap");
    assert.equal((await transport.next()).projections[0].identitySha256, first.edgeReceiptIdentitySha256);
    assert.equal((await readdir(join(root, "online-pages", "gaps"))).length, 1);
    const retried = await transport.captureCommittedSchedulerEvidence({
      dueKey: "capture/2026-10-08T06:35:00.000Z",
      prepared: second,
    });
    assert.equal(retried.status, "evidence_gap");
    assert.equal((await readdir(join(root, "online-pages", "gaps"))).length, 1);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// keep both archive forced commands closed against argument and shell expansion
test("archive SSH dispatch accepts only the exact next and acknowledgement grammars", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-adjustment-archive-dispatch-"));
  const sudo = join(directory, "sudo");
  const dispatch = join(repoRoot, "deploy/scripts/ssh-dispatch.sh");

  try {
    await writeFile(sudo, "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\"\n");
    await chmod(sudo, 0o700);

    // accept only complete fixed archive and maintenance grammars
    for (const [command, expected] of [
      ["adjustment-archive-next", "-n /usr/local/sbin/weather-remote-ops adjustment-archive-next\n"],
      [
        `adjustment-archive-ack ${"a".repeat(64)} ${"b".repeat(64)}`,
        `-n /usr/local/sbin/weather-remote-ops adjustment-archive-ack ${"a".repeat(64)} ${"b".repeat(64)}\n`,
      ],
      [
        `adjustment-archive-ack-final ${"c".repeat(64)} ${"d".repeat(64)}`,
        `-n /usr/local/sbin/weather-remote-ops adjustment-archive-ack-final ${"c".repeat(64)} ${"d".repeat(64)}\n`,
      ],
      [
        `adjustment-maintenance-anchor-install-v2 ${"e".repeat(64)}`,
        `-n /usr/local/sbin/weather-remote-ops adjustment-maintenance-anchor-install-v2 ${"e".repeat(64)}\n`,
      ],
      [
        `adjustment-future-input-seal-install-v2 ${"f".repeat(64)}`,
        `-n /usr/local/sbin/weather-remote-ops adjustment-future-input-seal-install-v2 ${"f".repeat(64)}\n`,
      ],
      [
        `adjustment-maintenance-anchor-install-v3 ${"1".repeat(64)}`,
        `-n /usr/local/sbin/weather-remote-ops adjustment-maintenance-anchor-install-v3 ${"1".repeat(64)}\n`,
      ],
      [
        `adjustment-maintenance-anchor-finalize-v3 ${"2".repeat(64)}`,
        `-n /usr/local/sbin/weather-remote-ops adjustment-maintenance-anchor-finalize-v3 ${"2".repeat(64)}\n`,
      ],
    ]) {
      const result = spawnSync(dispatch, [], {
        cwd: repoRoot,
        encoding: "utf8",
        env: { ...process.env, PATH: `${directory}:${process.env.PATH}`, SSH_ORIGINAL_COMMAND: command },
      });
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stdout, expected);
    }

    // reject every extra, malformed or shell-expanded argument
    for (const command of [
      "adjustment-archive-next extra",
      `adjustment-archive-ack ${"A".repeat(64)} ${"b".repeat(64)}`,
      `adjustment-archive-ack ${"a".repeat(64)} ${"b".repeat(63)}`,
      `adjustment-archive-ack ${"a".repeat(64)} ${"b".repeat(64)} extra`,
      `adjustment-archive-ack ${"a".repeat(64)};id ${"b".repeat(64)}`,
      `adjustment-archive-ack-final ${"a".repeat(64)} ${"B".repeat(64)}`,
      `adjustment-archive-ack-final ${"a".repeat(64)} ${"b".repeat(64)} extra`,
      `adjustment-maintenance-anchor-install-v2 ${"E".repeat(64)}`,
      `adjustment-maintenance-anchor-install-v2 ${"e".repeat(64)} extra`,
      `adjustment-future-input-seal-install-v2 ${"F".repeat(64)}`,
      `adjustment-future-input-seal-install-v2 ${"f".repeat(64)} extra`,
      `adjustment-maintenance-anchor-install-v3 ${"1".repeat(63)}`,
      `adjustment-maintenance-anchor-install-v3 ${"1".repeat(64)} extra`,
      `adjustment-maintenance-anchor-finalize-v3 ${"2".repeat(63)}`,
      `adjustment-maintenance-anchor-finalize-v3 ${"2".repeat(64)} extra`,
    ]) {
      const result = spawnSync(dispatch, [], {
        cwd: repoRoot,
        encoding: "utf8",
        env: { ...process.env, PATH: `${directory}:${process.env.PATH}`, SSH_ORIGINAL_COMMAND: command },
      });
      assert.equal(result.status, 126, command);
      assert.equal(result.stdout, "");
      assert.equal(result.stderr, "operation denied\n");
    }
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// reserve one disposable listener port
async function unusedPort() {
  const server = createNetServer();
  server.listen(0, "127.0.0.1");
  await new Promise(
    // wait for the kernel-selected listener
    (resolveWait) => server.once("listening", resolveWait),
  );
  const address = server.address();
  const port = typeof address === "object" && address !== null ? address.port : null;
  await new Promise(
    // release the temporary listener
    (resolveClose) => server.close(resolveClose),
  );
  assert.notEqual(port, null);
  return port;
}

// wait for the edge listener without extending the test indefinitely
async function waitForServer(origin) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const response = await fetch(origin);

      if (response.status > 0) {
        return;
      }
    } catch {
      await new Promise(
        // wait briefly between connection attempts
        (resolveWait) => setTimeout(resolveWait, 20),
      );
    }
  }
  throw new Error("edge server did not start");
}

// prove direct GET capture is post-finish and byte-preserving at the edge
test("web edge captures only a finished normal forecast GET without changing bytes", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-edge-"));
  const evidenceRoot = join(root, "evidence");
  const upstreamBody = forecastBody([
    forecastRow(0, {
      adjustment: {
        adjustedMetrics: {},
        appliedMetrics: [],
        reasonCode: "unsupported_lead",
        state: "not_applicable",
      },
    }),
    forecastRow(1),
  ]);
  const upstream = createHttpServer(
    // serve one exact successful forecast body
    (request, response) => {
      if (request.url?.startsWith("/api/v1/sites/ballydidean/forecast")) {
        response.writeHead(200, {
          "Content-Length": String(upstreamBody.byteLength),
          "Content-Type": "application/json; charset=utf-8",
        });
        response.end(request.method === "HEAD" ? undefined : upstreamBody);
        return;
      }

      response.writeHead(404);
      response.end();
    },
  );
  upstream.listen(0, "127.0.0.1");
  await new Promise(
    // wait for the disposable upstream
    (resolveWait) => upstream.once("listening", resolveWait),
  );
  const upstreamAddress = upstream.address();
  assert.equal(typeof upstreamAddress, "object");
  const edgePort = await unusedPort();
  const edge = spawn(process.execPath, [join(repoRoot, "deploy/scripts/web-server.mjs")], {
    cwd: repoRoot,
    env: {
      ...process.env,
      PORT: String(edgePort),
      WEATHER_ADJUSTMENT_EVIDENCE_ROOT: evidenceRoot,
      WEATHER_ADMIN_AUTH_PATH: join(root, "admin-auth.json"),
      WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH: join(root, "missing-bootstrap"),
      WEATHER_API_ORIGIN: `http://127.0.0.1:${String(upstreamAddress.port)}`,
      WEATHER_PROPERTY_SENSOR_LAYOUT_PATH: join(root, "property-sensor-layout.json"),
      WEATHER_RELEASE: "2026.10.07-1",
    },
    stdio: ["ignore", "ignore", "pipe"],
  });
  const diagnostics = [];
  edge.stderr.on(
    "data",
    // retain startup diagnostics for one failed assertion
    (chunk) => diagnostics.push(String(chunk)),
  );

  try {
    const origin = `http://127.0.0.1:${String(edgePort)}`;
    await waitForServer(`${origin}/`);
    const forecast = await fetch(`${origin}/api/v1/sites/ballydidean/forecast`);
    const edgeBytes = Buffer.from(await forecast.arrayBuffer());
    assert.equal(forecast.status, 200);
    const expectedReceiptName = `sha256-${createAdjustmentEvidenceCapture(
      edgeBytes,
      "days=1",
    ).edgeReceiptIdentitySha256}.json`;
    const duplicate = await fetch(`${origin}/api/v1/sites/ballydidean/forecast`);
    assert.deepEqual(Buffer.from(await duplicate.arrayBuffer()), edgeBytes);

    let receiptNames;
    let receiptDirectoryObserved = false;

    // wait for the asynchronous receipt publication only
    for (let attempt = 0; attempt < 50; attempt += 1) {
      try {
        receiptNames = (await readdir(join(evidenceRoot, "receipts"))).sort();
        receiptDirectoryObserved = true;
      } catch (error) {
        // tolerate only directory creation still in progress
        if (error?.code !== "ENOENT" || receiptDirectoryObserved) {
          throw error;
        }
        receiptNames = [];
      }
      const unexpectedNames = receiptNames.filter(
        // allow only the writer's private temporary before publication
        (name) => name !== expectedReceiptName && !/^\.capture-[a-f0-9-]+\.tmp$/u.test(name),
      );
      assert.deepEqual(unexpectedNames, []);

      // freeze only after the exact final remains alone
      if (receiptNames.length === 1 && receiptNames[0] === expectedReceiptName) {
        break;
      }

      await new Promise(
        // wait briefly for durable receipt creation
        (resolveWait) => setTimeout(resolveWait, 10),
      );
    }

    assert.deepEqual(receiptNames, [expectedReceiptName]);
    const snapshot = await freezeAdjustmentEvidenceSnapshot({ root: evidenceRoot });
    assert.equal(snapshot.entries.length, 1);
    const head = await fetch(`${origin}/api/v1/sites/ballydidean/forecast`, {
      method: "HEAD",
    });
    assert.equal(head.status, 200);
    assert.equal(await head.text(), "");
    assert.equal((await freezeAdjustmentEvidenceSnapshot({ root: evidenceRoot })).entries.length, 1);
    const spoofed = await fetch(`${origin}/api/v1/sites/ballydidean/forecast?days=1`, {
      headers: { "X-Weather-Issuance-Channel": "scheduler_request" },
    });
    assert.equal(spoofed.status, 200);
    await spoofed.arrayBuffer();
    assert.deepEqual(await readdir(join(evidenceRoot, "channels")), []);
    const widget = await fetch(`${origin}/api/v1/sites/ballydidean/widget-forecast`);
    assert.equal(widget.status, 502);
    assert.equal((await freezeAdjustmentEvidenceSnapshot({ root: evidenceRoot })).entries.length, 1);
  } catch (error) {
    error.message += `\nedge diagnostics:\n${diagnostics.join("")}`;
    throw error;
  } finally {
    // stop only still-running disposable servers
    if (edge.exitCode === null && edge.signalCode === null) {
      edge.kill("SIGTERM");
      await new Promise(
        // wait for child cleanup before removing state
        (resolveExit) => edge.once("exit", resolveExit),
      );
    }
    await new Promise(
      // close the disposable upstream listener
      (resolveClose) => upstream.close(resolveClose),
    );
    await rm(root, { force: true, recursive: true });
  }
});
