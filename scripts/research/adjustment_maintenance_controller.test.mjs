import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { readdirSync, readFileSync } from "node:fs";
import { PassThrough } from "node:stream";
import test from "node:test";

import {
  acknowledgeAdjustmentRevisionColdCustody,
  acknowledgeAdjustmentRevisionGapPayload,
  applyAdjustmentFamilyRelease,
  assembleAdjustmentRevisionColdCatalog,
  buildAdjustmentDevelopmentCustodyAnchor,
  buildAdjustmentDevelopmentCustodyGraphSegment,
  buildAdjustmentConfirmationPlanGraphSegment,
  buildAdjustmentFutureOnlySourceLineage,
  buildAdjustmentFutureOnlyGenesisGraphSegment,
  buildAdjustmentFutureOnlySourceLineageGraphSegment,
  buildAdjustmentFutureOnlyInputSeal,
  buildAdjustmentMaintenanceFinalizationProofV3,
  buildAdjustmentMaintenanceInputClassMembers,
  buildAdjustmentQualifiedTerminalEvidenceMembers,
  buildAdjustmentMaintenanceTerminalEvidenceMembers,
  buildAdjustmentMaintenanceTerminalGraphParts,
  buildAdjustmentMaintenanceScorecardV2,
  buildAdjustmentMaintenanceParityReceipt,
  buildAdjustmentMaintenanceTransferredAnchorV3,
  buildAdjustmentRainControlCustodyAnchor,
  buildAdjustmentPostFitRegistrationMaterial,
  buildAdjustmentRegistrationCohortDescriptor,
  buildAdjustmentRegistrationExpectedKeyPlan,
  buildAdjustmentRegistrationPolicyDescriptor,
  buildAdjustmentRollingShadowRegistration,
  buildAdjustmentTerminalNoActionReceipt,
  buildAdjustmentUnsupportedPolicyReport,
  buildAdjustmentUnsupportedTerminalArtifacts,
  buildAdjustmentUnsupportedTerminalProof,
  burnAdjustmentConfirmationAccessV3,
  completeAdjustmentNoActionDailyTerminal,
  completeAdjustmentQualifiedDailyTerminal,
  completeAdjustmentUnsupportedDailyTerminal,
  createProductionAdjustmentMaintenanceControllerPorts,
  drainAdjustmentRevisionGapSpool,
  executeAdjustmentMaintenanceDue,
  fetchAdjustmentRevisionColdPage,
  fetchAdjustmentRevisionCaptureEpochWitness,
  fetchAdjustmentRevisionCaptureEpochSnapshot,
  fetchAdjustmentRevisionColdCurrentTransferStart,
  fetchAdjustmentRevisionColdTransferStart,
  fetchAdjustmentRevisionGapPayloadPage,
  fetchAdjustmentRevisionGapTransferStart,
  fetchAdjustmentDevelopmentCustodyAnchorCurrent,
  fetchAdjustmentFamilyReleaseCurrent,
  fetchAdjustmentFamilyReleaseLineage,
  fetchAdjustmentMaintenanceDatabaseLedger,
  fetchAdjustmentRegistrationLifecycleStatus,
  fetchAdjustmentRegistrationScheduleStatus,
  fetchAdjustmentRainControlCustodyAnchorCurrent,
  fetchAdjustmentShadowMetadataCustodyStatus,
  fetchAdjustmentUnsupportedTerminalProofCurrent,
  finalizeAdjustmentMaintenanceAnchorV3,
  finalizeAdjustmentShadowMetadataCustody,
  initializeAdjustmentRegistrationSchedule,
  installAdjustmentDevelopmentCustodyAnchor,
  installAdjustmentFutureOnlyInputSeal,
  installAdjustmentMaintenanceAnchorV3,
  installAdjustmentRainControlCustodyAnchor,
  installAdjustmentUnsupportedTerminalProof,
  parseAdjustmentMaintenanceControllerArguments,
  prepareAdjustmentMaintenanceDailyTerminalMaterial,
  projectAdjustmentQualifiedReleaseOutcome,
  publishAdjustmentArchiveGraphSegment,
  planAdjustmentMaintenanceDues,
  publishAdjustmentArchiveMember,
  publishAdjustmentMaintenanceScorecardV2,
  reconcileAdjustmentShadowMetadataCustody,
  resolveAdjustmentCandidateMaterial,
  restoreAdjustmentCandidateMaterial,
  runAdjustmentMaintenanceController,
  runAdjustmentRevisionCaptureCycle,
  selectAdjustmentDailyLifecycleEntry,
  validateAdjustmentMaintenanceAttempt,
  validateAdjustmentRegistrationCohortDescriptor,
  validateAdjustmentRegistrationExpectedKeyPlan,
  validateAdjustmentRegistrationPolicyDescriptor,
  validateAdjustmentTerminalNoActionReceipt,
  validateAdjustmentUnsupportedPolicyReport,
  validateAdjustmentUnsupportedTerminalProof,
  validateAdjustmentFutureOnlyCausalInstants,
  validateAdjustmentFutureOnlyRevisionReceipt,
} from "./adjustment_maintenance_controller.mjs";
import {
  accessRequest,
} from "../../deploy/test/fixtures/adjustment-owner-requests.mjs";
import {
  adjustmentSha256,
  canonicalJsonBytes,
} from "./adjustment_plaintext_archive.mjs";
import {
  buildAdjustmentRollingScheduleBootstrap,
} from "./adjustment_rolling_schedule.mjs";
import {
  buildForecastAdjustmentMaintenancePortableCandidate,
  createRainMaintenanceControlState,
  encodeRainMaintenanceControlState,
} from "./adjustment_maintenance_runtime_adapter.mjs";
import {
  RAIN_HURDLE_WIND_ARTIFACT_JSON,
} from "./adjustment-maintenance-runtime/forecast/rain-hurdle-wind-artifact.js";
import {
  RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256,
} from "./adjustment-maintenance-runtime/forecast/rain-maintenance-controls.js";

const HASH_A = "a".repeat(64);
const HASH_B = "b".repeat(64);
const HASH_C = "c".repeat(64);
const HASH_D = "d".repeat(64);
const HASH_E = "e".repeat(64);
const HASH_F = "f".repeat(64);

// build the exact checked-in full-0021 database ledger projection
function databaseLedgerV3() {
  const root = new URL("../../packages/database/migrations/", import.meta.url);
  const migrationNames = readdirSync(root)
    .filter((name) => name.endsWith(".sql"))
    .sort();
  const migrationChecksums = migrationNames.map(
    // bind every migration byte in database order
    (name) => adjustmentSha256(readFileSync(new URL(name, root))),
  );
  return {
    contractVersion: "adjustment-database-ledger/v3",
    databaseManifest: {
      contract_version: "adjustment-evaluation-export-manifest/v1",
      migration_checksums: migrationChecksums,
      migration_history_sha256: adjustmentSha256(Buffer.from(migrationNames.map(
        // match the database aggregate hash grammar
        (name, index) => `${name}:${migrationChecksums[index]}`,
      ).join("\n"))),
      migration_names: migrationNames,
      query_contract_sha256:
        "c860039c72818a9b813ed9f9d93f5d8e115f4144e99d698734e9159ca3457bd4",
      query_contract_version: "adjustment-evaluation-export-query/v1",
      row_schema_sha256:
        "c21782034130d003e4af48fada4c8f6545f6f1c4dff550f92d0523c7d96d20e2",
      schema_migration: "0017_adjustment_evaluation_export.sql",
      site_key: "ballydidean",
      site_timezone: "America/Los_Angeles",
    },
    snapshotAt: "2026-10-10T08:00:00.000Z",
  };
}

// create one exact database-owned registration slot projection
function registrationSlot(family, state = "free") {
  const free = state === "free";
  return {
    contractVersion: "adjustment-shadow-registration-slot/v3",
    epochWitnessSha256: HASH_A,
    family,
    horizonEndAt: "2029-12-31T08:00:00.000Z",
    registrationSha256: free ? null : HASH_B,
    scheduleContractSha256:
      "7c17f5d1a8e8249cd0aa4820638169e51f6edb3433017f50ab4c959e44c62f1f",
    state,
    terminalAt: free ? null : "2029-01-08T08:00:00.000Z",
  };
}

// build one root-authenticated finite rolling schedule status
function registrationScheduleStatus(snapshotAt = "2026-10-10T12:00:00.000Z") {
  const horizonEndAt = "2027-10-08T07:00:00.000Z";
  return {
    contractVersion: "adjustment-registration-schedule-status/v3",
    epochAt: "2026-10-07T08:00:00.000Z",
    epochWitnessSha256: HASH_A,
    horizonEndAt,
    scheduleContractSha256:
      "7c17f5d1a8e8249cd0aa4820638169e51f6edb3433017f50ab4c959e44c62f1f",
    slots: ["temperature", "wind", "rain"].map((family) => ({
      ...registrationSlot(family),
      horizonEndAt,
    })),
    snapshotAt,
  };
}

// build one genuine archived control reference for a requested future month
function rainControlReferenceMaterial(modelMonth) {
  const artifact = JSON.parse(RAIN_HURDLE_WIND_ARTIFACT_JSON);
  delete artifact.nativeModelSha256;
  delete artifact.provenanceSha256;
  artifact.modelMonth = modelMonth;
  const ordinalArtifactBytes = canonicalJsonBytes(artifact);
  const monthStart = Date.parse(`${modelMonth}-01T00:00:00.000Z`);
  const calibrationEnd = monthStart - 7 * 86_400_000;
  const calibrationStart = calibrationEnd - 90 * 86_400_000;
  const state = createRainMaintenanceControlState({
    calibrationEndAt: new Date(calibrationEnd).toISOString(),
    calibrationStartAt: new Date(calibrationStart).toISOString(),
    contractVersion: "rain-maintenance-control-state/v1",
    epochWitnessSha256: HASH_A,
    generatedAt: new Date(monthStart - 3 * 86_400_000).toISOString(),
    legacyCalibrationStartAt: new Date(calibrationEnd - 45 * 86_400_000).toISOString(),
    legacyRawScale: 1,
    modelMonth,
    ordinalArtifactSha256: adjustmentSha256(ordinalArtifactBytes),
    recentFallbackReason: "recent_calibration",
    recentRawScale: 1,
    recentSupported: true,
    recipeSha256: RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256,
    sameWindowRawScale: 1,
    scheduleContractSha256:
      "7c17f5d1a8e8249cd0aa4820638169e51f6edb3433017f50ab4c959e44c62f1f",
    sourceMemberRootSha256: HASH_C,
    sourceReceiptRootSha256: HASH_D,
    support: {
      calibrationDates: 60,
      calibrationHours: 500,
      calibrationRows: 500,
      calibrationWetDates: 5,
      calibrationWetHours: 20,
      effectiveDates64: "403e000000000000",
      effectiveWetDates64: "4008000000000000",
      legacyCalibrationRows: 200,
      legacyTrainingRows: 1_000,
      legacyTrainingWetRows: 100,
      trainingDates: 180,
      trainingHours: 1_000,
      trainingRows: 1_000,
      trainingWetDates: 20,
      trainingWetHours: 100,
    },
    trainingMaximumValidAt: new Date(calibrationStart - 8 * 86_400_000).toISOString(),
  });
  return {
    controlStateBytes: encodeRainMaintenanceControlState(state),
    graphManifestSha256: HASH_E,
    ordinalArtifactBytes,
    sourceMemberRootSha256: HASH_C,
    sourceReceiptRootSha256: HASH_D,
  };
}

// build one exact first future-only C/T/F proof chain
function futureOnlyProofChain() {
  const inputClassMembers = {
    actual_best_match: [HASH_B],
    artifact: [HASH_C],
    candidate: [HASH_A],
    comparator: [HASH_D],
    native_source: [HASH_E],
    rain_gate_input: [],
    shadow_body: [HASH_F],
    shadow_source: [HASH_A],
    target: [HASH_B],
    target_revision: [HASH_C],
  };
  const sealed = buildAdjustmentFutureOnlyInputSeal({
    archiveCommitOrdinal: "1",
    burnSha256: HASH_A,
    candidateReportSha256: HASH_B,
    captureEpochWitnessSha256: HASH_C,
    confirmationAccessSha256: HASH_D,
    custodyCheckpointSha256: HASH_E,
    dueKey: `confirmation/temperature/${HASH_A}`,
    family: "temperature",
    frontierSha256: HASH_F,
    fullMemberRootSha256: HASH_A,
    graphManifestSha256: HASH_B,
    inputClassMembers,
    lifecycleLedgerRootSha256: HASH_C,
    pageSha256: HASH_D,
    policyReportSha256: HASH_E,
    predecessorSealSha256: null,
    sealedAt: "2027-10-17T08:00:00.000Z",
    sequence: "0",
    sourceCommit: "1".repeat(40),
    workstationJournalHeadSha256: HASH_F,
  });
  const transferred = buildAdjustmentMaintenanceTransferredAnchorV3({
    actionSha256: HASH_A,
    controlSha256: HASH_B,
    controlVersion: "14",
    fullGraphVerifiedAt: "2027-10-17T08:01:00.000Z",
    inputSeal: sealed.seal,
    inputSealSha256: sealed.sealSha256,
    predecessorAnchorSha256: null,
    publishedAt: "2027-10-17T08:02:00.000Z",
  });
  const finalized = buildAdjustmentMaintenanceFinalizationProofV3({
    anchor: transferred.anchor,
    anchorSha256: transferred.anchorSha256,
    finalizedAt: "2027-10-17T08:03:00.000Z",
  });
  return { finalized, sealed, transferred };
}

// build one canonical future-only epoch witness for consumer tests
function futureOnlyEpochWitness() {
  const snapshot = futureOnlyEpochSnapshot();
  const unsigned = {
    activationKind: "inert_v14_pre_activation",
    archiveCommitOrdinal: "0",
    catalogFrontierSha256: adjustmentSha256(Buffer.from(
      "adjustment-revision-frontier/v1\n0\n",
    )),
    contractVersion: "adjustment-revision-capture-epoch-witness/v1",
    controlPlaneSha256: HASH_A,
    controlPlaneVersion: "14",
    databaseMigrationHistorySha256:
      "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
    epochAt: "2026-10-10T08:00:00.000Z",
    servingSnapshotSha256: snapshot.snapshotSha256,
    sourceCommit: "1".repeat(40),
    sourceRelease: "2026.10.10-1",
    sourceServerImageDigest: `sha256:${HASH_D}`,
    sourceWebImageDigest: `sha256:${HASH_E}`,
  };
  return {
    ...unsigned,
    witnessSha256: adjustmentSha256(canonicalJsonBytes(unsigned)),
  };
}

// build the exact empty database snapshot retained with the epoch witness
function futureOnlyEpochSnapshot() {
  const frontierSha256 = adjustmentSha256(Buffer.from(
    "adjustment-revision-frontier/v1\n0\n",
  ));
  const cutoffAt = "2026-10-10T08:00:00.000Z";
  return {
    archiveCommitOrdinal: "0",
    contractVersion: "adjustment-revision-serving-snapshot/v1",
    cutoffAt,
    entries: [],
    entryCount: 0,
    frontierSha256,
    snapshotSha256: adjustmentSha256(Buffer.from([
      "adjustment-revision-serving-snapshot/v1", cutoffAt, "0", frontierSha256, "",
    ].join("\n"))),
  };
}

// build one exact server receipt in the global frontier chain
function coldReceipt({ archiveCommitOrdinal, archiveCommittedAt, predecessorFrontierSha256,
  projectionIdentitySha256, projectionKind, stageReceiptSha256 }) {
  const receipt = {
    archiveCommitOrdinal,
    archiveCommittedAt,
    contractVersion: "adjustment-revision-commit-receipt/v1",
    frontierSha256: "",
    predecessorFrontierSha256,
    projectionIdentitySha256,
    projectionKind,
    projectionSha256: projectionIdentitySha256,
    receiptSha256: "",
    stageReceiptSha256,
  };
  receipt.receiptSha256 = adjustmentSha256(Buffer.from([
    receipt.contractVersion,
    receipt.archiveCommitOrdinal,
    receipt.archiveCommittedAt,
    receipt.projectionKind,
    receipt.projectionIdentitySha256,
    receipt.projectionSha256,
    receipt.stageReceiptSha256,
    receipt.predecessorFrontierSha256,
  ].join("\n")));
  receipt.frontierSha256 = adjustmentSha256(Buffer.from([
    "adjustment-revision-frontier/v1",
    receipt.predecessorFrontierSha256,
    receipt.archiveCommitOrdinal,
    receipt.receiptSha256,
  ].join("\n")));
  return receipt;
}

// hash the compact checkpoint's actual member population
function coldCheckpointMemberRoot(entries, startMemberSha256) {
  return adjustmentSha256(canonicalJsonBytes({
    entries: entries.map(
      // retain grouped receipt arrays without duplicating their shared body
      (entry) => ({
        payloadMemberSha256: entry.payloadIdentitySha256,
        publicationDisposition: entry.publicationDisposition,
        publicationIdentitySha256: entry.publicationIdentitySha256,
        publicationMemberSha256: entry.publicationMemberSha256,
        ...(Object.hasOwn(entry, "receiptMemberSha256")
          ? { receiptMemberSha256: entry.receiptMemberSha256 }
          : { receiptMemberSha256s: entry.receiptMemberSha256s }),
        stageReceiptMemberSha256: entry.stageReceiptMemberSha256,
        ...(Object.hasOwn(entry, "successorMemberSha256")
          ? { successorMemberSha256: entry.successorMemberSha256 }
          : { successorMemberSha256s: entry.successorMemberSha256s }),
      }),
    ),
    startMemberSha256,
  }));
}

// restore requested members from one verified in-memory graph fixture
function coldArchive(graphs) {
  return {
    restoreFullGraph: async (graphManifestSha256, { sink, targets }) => {
      const members = graphs.get(graphManifestSha256);

      // refuse fixture requests outside the selected immutable graph
      if (members === undefined) {
        throw new Error("test graph is unavailable");
      }
      for (const target of targets) {
        const bytes = members.get(target.identitySha256);

        // require every requested content address to exist
        if (bytes === undefined) {
          throw new Error("test member is unavailable");
        }
        const readable = new PassThrough();
        readable.end(bytes);
        await sink.writeExclusive(target.fileName, readable, bytes.length);
      }
    },
  };
}

// project one ordinary receipt into a compact checkpoint entry
function singleColdCheckpointEntry(receipt, payloadBytes, label) {
  const successorSha256 = adjustmentSha256(Buffer.from(`${label}-successor`));
  return {
    payloadIdentitySha256: adjustmentSha256(payloadBytes),
    payloadKind: "adjustment-revision-projection/v1",
    publicationDisposition: "published",
    publicationIdentitySha256: adjustmentSha256(Buffer.from(`${label}-publication-id`)),
    publicationMemberSha256: adjustmentSha256(Buffer.from(`${label}-publication-member`)),
    receiptMemberSha256: adjustmentSha256(canonicalJsonBytes(receipt)),
    receiptSha256: receipt.receiptSha256,
    stageReceiptMemberSha256: adjustmentSha256(Buffer.from(`${label}-stage-member`)),
    stageReceiptSha256: receipt.stageReceiptSha256,
    successorMemberSha256: successorSha256,
    successorSha256,
  };
}

// project one grouped weather body into its ordered receipt checkpoint entry
function groupedColdCheckpointEntry(receipts, payloadBytes, label) {
  const successorSha256s = receipts.map(
    // retain one successor identity for each grouped server ordinal
    (_, index) => adjustmentSha256(Buffer.from(`${label}-successor-${index}`)),
  );
  return {
    payloadIdentitySha256: adjustmentSha256(payloadBytes),
    payloadKind: "adjustment-revision-batch-projection/v2",
    publicationDisposition: "published",
    publicationIdentitySha256: adjustmentSha256(Buffer.from(`${label}-publication-id`)),
    publicationMemberSha256: adjustmentSha256(Buffer.from(`${label}-publication-member`)),
    receiptMemberSha256s: receipts.map(
      // bind each actual canonical receipt member
      (receipt) => adjustmentSha256(canonicalJsonBytes(receipt)),
    ),
    receiptSha256s: receipts.map(
      // retain server receipt identity order
      (receipt) => receipt.receiptSha256,
    ),
    stageReceiptMemberSha256: adjustmentSha256(Buffer.from(`${label}-stage-member`)),
    stageReceiptSha256: receipts[0].stageReceiptSha256,
    successorMemberSha256s: successorSha256s,
    successorSha256s,
  };
}

test("future-only lineage binds the epoch and rejects old receipts and causal clocks", () => {
  const epochWitness = futureOnlyEpochWitness();
  const lineage = buildAdjustmentFutureOnlySourceLineage({
    epochWitness,
    family: "temperature",
    sourceIdentitySha256: HASH_E,
  });
  assert.equal(lineage.descriptor.epochWitnessSha256, epochWitness.witnessSha256);
  assert.equal(lineage.sourceSha256, adjustmentSha256(lineage.bytes));
  const segment = buildAdjustmentFutureOnlyGenesisGraphSegment({
    servingSnapshot: futureOnlyEpochSnapshot(),
    witness: epochWitness,
  });
  assert.equal(segment.members.length, 2);
  assert.equal(segment.members[0].identitySha256, epochWitness.witnessSha256);
  assert.equal(segment.members[1].identitySha256,
    futureOnlyEpochSnapshot().snapshotSha256);
  assert.equal(segment.crossLinks.length, 1);
  assert.deepEqual(segment.crossLinks[0], {
    fromIdentitySha256: epochWitness.witnessSha256,
    relation: "binds_zero_frontier_snapshot",
    toIdentitySha256: futureOnlyEpochSnapshot().snapshotSha256,
  });
  const lineageSegment = buildAdjustmentFutureOnlySourceLineageGraphSegment({
    epochWitness,
    family: "temperature",
    sourceIdentitySha256: HASH_E,
  });
  assert.equal(lineageSegment.members.length, 2);
  assert.deepEqual(lineageSegment.crossLinks, [{
    fromIdentitySha256: lineage.sourceSha256,
    relation: "binds_capture_epoch",
    toIdentitySha256: epochWitness.witnessSha256,
  }]);
  assert.equal(validateAdjustmentFutureOnlyRevisionReceipt({
    epochWitness,
    receipt: {
      archiveCommitOrdinal: "1",
      archiveCommittedAt: "2026-10-10T08:00:01.000Z",
    },
  }).archiveCommitOrdinal, "1");
  assert.throws(
    // reject pre-epoch observations even when presented after activation
    () => validateAdjustmentFutureOnlyCausalInstants({
      epochWitness,
      instants: ["2026-10-10T07:59:59.000Z"],
    }),
    /predates its epoch/u,
  );
  assert.throws(
    // zero is the witness snapshot and never a qualification member
    () => validateAdjustmentFutureOnlyRevisionReceipt({
      epochWitness,
      receipt: {
        archiveCommitOrdinal: "0",
        archiveCommittedAt: "2026-10-10T08:00:01.000Z",
      },
    }),
    /predates its epoch/u,
  );
});

test("semantic catalog refuses a snapshot outside the durable captured cursor", async () => {
  const servingSnapshot = futureOnlyEpochSnapshot();
  const unsigned = {
    contractVersion: "adjustment-revision-cold-transfer-start/v1",
    servingSnapshot,
    watermarkArchiveCommitOrdinal: "0",
    watermarkFrontierSha256: servingSnapshot.frontierSha256,
  };
  const result = await assembleAdjustmentRevisionColdCatalog({
    archive: {},
    journal: {
      readRevisionCursor: async () => ({
        archiveCommitOrdinal: "1",
        frontierSha256: HASH_A,
      }),
    },
    selection: {
      family: null,
      fromAt: null,
      receiptSha256s: [],
      toAt: null,
    },
    start: { ...unsigned, startSha256: adjustmentSha256(canonicalJsonBytes(unsigned)) },
  });
  assert.deepEqual(result, { reason: "semantic_input_blocked", state: "blocked" });
});

test("semantic catalog restores every receipt from grouped cold bodies", async () => {
  const genesis = adjustmentSha256(Buffer.from("adjustment-revision-frontier/v1\n0\n"));
  const payloads = [
    canonicalJsonBytes({ contractVersion: "adjustment-revision-batch-projection/v2",
      rows: [1, 2] }),
    canonicalJsonBytes({ contractVersion: "adjustment-revision-projection/v1", rows: [3] }),
    canonicalJsonBytes({ contractVersion: "adjustment-revision-projection/v1", rows: [4] }),
    canonicalJsonBytes({ contractVersion: "adjustment-revision-projection/v1", rows: [5] }),
  ];
  const kinds = [
    "actual_best_match", "actual_best_match", "native_source", "rain_gate_input",
    "target_revision",
  ];
  const payloadIndexes = [0, 0, 1, 2, 3];
  const receipts = [];
  let predecessorFrontierSha256 = genesis;

  // build one grouped body followed by every remaining semantic class
  for (const [index, projectionKind] of kinds.entries()) {
    const payloadIdentitySha256 = adjustmentSha256(payloads[payloadIndexes[index]]);
    const receipt = coldReceipt({
      archiveCommitOrdinal: String(index + 1),
      archiveCommittedAt: `2026-10-10T08:0${index + 1}:00.000Z`,
      predecessorFrontierSha256,
      projectionIdentitySha256: payloadIdentitySha256,
      projectionKind,
      stageReceiptSha256: index < 2
        ? HASH_A
        : adjustmentSha256(Buffer.from(`stage-${index}`)),
    });
    receipts.push(receipt);
    predecessorFrontierSha256 = receipt.frontierSha256;
  }
  const currentReceipts = [receipts[1], receipts[2], receipts[3], receipts[4]];
  const cutoffAt = "2026-10-10T09:00:00.000Z";
  const servingSnapshot = {
    archiveCommitOrdinal: "5",
    contractVersion: "adjustment-revision-serving-snapshot/v1",
    cutoffAt,
    entries: currentReceipts.map(
      // bind each current relation to its exact receipt member
      (receipt, index) => ({
        logicalReceivedAt: `2026-10-10T08:1${index}:00.000Z`,
        receipt,
        relation: receipt.projectionKind === "native_source"
          ? "forecast_anchor_records"
          : receipt.projectionKind === "rain_gate_input"
            ? "rain_adjustment_runs"
            : "weather_records",
      }),
    ),
    entryCount: currentReceipts.length,
    frontierSha256: receipts.at(-1).frontierSha256,
    snapshotSha256: "",
  };
  servingSnapshot.snapshotSha256 = adjustmentSha256(Buffer.from([
    servingSnapshot.contractVersion,
    servingSnapshot.cutoffAt,
    servingSnapshot.archiveCommitOrdinal,
    servingSnapshot.frontierSha256,
    currentReceipts.map(
      // preserve the selected receipt order in the snapshot identity
      (receipt) => receipt.receiptSha256,
    ).join("\n"),
  ].join("\n")));
  const startUnsigned = {
    contractVersion: "adjustment-revision-cold-transfer-start/v1",
    servingSnapshot,
    watermarkArchiveCommitOrdinal: "5",
    watermarkFrontierSha256: receipts.at(-1).frontierSha256,
  };
  const start = {
    ...startUnsigned,
    startSha256: adjustmentSha256(canonicalJsonBytes(startUnsigned)),
  };
  const firstEntries = [
    groupedColdCheckpointEntry(receipts.slice(0, 2), payloads[0], "grouped"),
    singleColdCheckpointEntry(receipts[2], payloads[1], "native"),
  ];
  const firstPageSha256 = adjustmentSha256(Buffer.from("cold-page-one"));
  const startMemberSha256 = adjustmentSha256(canonicalJsonBytes(start));
  const firstCheckpoint = {
    afterArchiveCommitOrdinal: "0",
    afterFrontierSha256: genesis,
    contractVersion: "adjustment-revision-cold-page-checkpoint/v1",
    entries: firstEntries,
    eof: false,
    memberRootSha256: coldCheckpointMemberRoot(firstEntries, startMemberSha256),
    nextArchiveCommitOrdinal: "3",
    nextFrontierSha256: receipts[2].frontierSha256,
    pageSha256: firstPageSha256,
    previousPageSha256: start.startSha256,
    startMemberSha256,
    startSha256: start.startSha256,
    watermarkArchiveCommitOrdinal: "5",
    watermarkFrontierSha256: receipts.at(-1).frontierSha256,
  };
  const secondEntries = [
    singleColdCheckpointEntry(receipts[3], payloads[2], "rain"),
    singleColdCheckpointEntry(receipts[4], payloads[3], "target"),
  ];
  const secondPageSha256 = adjustmentSha256(Buffer.from("cold-page-two"));
  const secondCheckpoint = {
    afterArchiveCommitOrdinal: "3",
    afterFrontierSha256: receipts[2].frontierSha256,
    contractVersion: "adjustment-revision-cold-page-checkpoint/v1",
    entries: secondEntries,
    eof: true,
    memberRootSha256: coldCheckpointMemberRoot(secondEntries, null),
    nextArchiveCommitOrdinal: "5",
    nextFrontierSha256: receipts[4].frontierSha256,
    pageSha256: secondPageSha256,
    previousPageSha256: firstPageSha256,
    startMemberSha256: null,
    startSha256: start.startSha256,
    watermarkArchiveCommitOrdinal: "5",
    watermarkFrontierSha256: receipts.at(-1).frontierSha256,
  };
  const firstGraphSha256 = adjustmentSha256(Buffer.from("cold-graph-one"));
  const secondGraphSha256 = adjustmentSha256(Buffer.from("cold-graph-two"));
  const graphs = new Map([
    [firstGraphSha256, new Map([
      [firstPageSha256, canonicalJsonBytes(firstCheckpoint)],
      [start.startSha256, canonicalJsonBytes(start)],
      ...receipts.slice(0, 3).map(
        // retain every receipt hidden behind the two first-page bodies
        (receipt) => [receipt.receiptSha256, canonicalJsonBytes(receipt)],
      ),
      [adjustmentSha256(payloads[0]), payloads[0]],
      [adjustmentSha256(payloads[1]), payloads[1]],
    ])],
    [secondGraphSha256, new Map([
      [secondPageSha256, canonicalJsonBytes(secondCheckpoint)],
      ...receipts.slice(3).map(
        // retain each second-page receipt as its own graph member
        (receipt) => [receipt.receiptSha256, canonicalJsonBytes(receipt)],
      ),
      [adjustmentSha256(payloads[2]), payloads[2]],
      [adjustmentSha256(payloads[3]), payloads[3]],
    ])],
  ]);
  const result = await assembleAdjustmentRevisionColdCatalog({
    archive: coldArchive(graphs),
    journal: {
      listRevisionCatalogPages: async () => [{
        graphManifestSha256: firstGraphSha256,
        pageSha256: firstPageSha256,
        startSha256: start.startSha256,
      }, {
        graphManifestSha256: secondGraphSha256,
        pageSha256: secondPageSha256,
        startSha256: start.startSha256,
      }],
      readRevisionCursor: async () => ({
        archiveCommitOrdinal: "5",
        frontierSha256: receipts[4].frontierSha256,
      }),
    },
    selection: {
      family: null,
      fromAt: null,
      receiptSha256s: currentReceipts.map((receipt) => receipt.receiptSha256),
      toAt: null,
    },
    start,
  });
  assert.equal(result.state, "ready");
  assert.equal(result.catalog.frontierCount, 5);
  assert.equal(result.members.length, 4);
  assert.deepEqual(result.members.map((member) => member.receipt.receiptSha256),
    currentReceipts.map((receipt) => receipt.receiptSha256));
  assert.deepEqual(result.members[0].payloadBytes, payloads[0]);
});

// restore one raw fit only through its acknowledged private candidate graph
test("candidate material resolver crossbinds the installed shadow and portable artifact", async () => {
  const incumbent = JSON.parse(readFileSync(new URL(
    "../../config/forecast-adjustments/ballydidean/temperature-canary-bundles/" +
      "sha256-4d4e229b42823e53d2db062ec18c625bb2d2378a8a46d641fa95fabb59501b0e.json",
    import.meta.url,
  ), "utf8"));
  const candidateBytes = canonicalJsonBytes({
    arms: {},
    confirmationOpened: false,
    contractVersion: "temperature-maintenance-fit/v2",
    developmentIncumbentMae: 1,
    developmentRawMae: 2,
    dueMonth: "2026-10",
    model: incumbent.model,
    selectedArm: "month_start_expanding",
    servingChanged: false,
    state: "development_candidate",
  });
  const portable = buildForecastAdjustmentMaintenancePortableCandidate({
    candidateBytes,
    family: "temperature",
  });
  const parityReceipt = buildAdjustmentMaintenanceParityReceipt({
    candidateSha256: portable.candidateSha256,
    family: "temperature",
    parity: {
      retainedInput: Buffer.from("retained-input"),
      retainedNativeOutput: Buffer.from("retained-output"),
      retainedPackagedOutput: Buffer.from("retained-output"),
      syntheticInput: Buffer.from("synthetic-input"),
      syntheticNativeOutput: Buffer.from("synthetic-output"),
      syntheticPackagedOutput: Buffer.from("synthetic-output"),
    },
  });
  const graphSha256 = HASH_E;
  const graphMembers = new Map([[
    graphSha256,
    new Map([
      [portable.candidateSha256, candidateBytes],
      [portable.artifactSha256, portable.artifactBytes],
      [parityReceipt.paritySha256, parityReceipt.bytes],
    ]),
  ]]);
  const archive = {
    ...coldArchive(graphMembers),
    // expose only the exact candidate member on the verified predecessor chain
    verifyFullGraph: async (identity) => {
      assert.equal(identity, graphSha256);
      return {
        manifest: {
          entries: [{
            identitySha256: portable.candidateSha256,
            kind: "forecast-adjustment-temperature-development-candidate/v1",
          }, {
            identitySha256: parityReceipt.paritySha256,
            kind: "forecast-adjustment-model-parity/v1",
          }],
          predecessorGraphSha256: null,
        },
      };
    },
  };
  const restored = await restoreAdjustmentCandidateMaterial({
    archive,
    artifactSha256: portable.artifactSha256,
    candidateGraphSha256: graphSha256,
    candidateSha256: portable.candidateSha256,
    family: "temperature",
  });
  assert.deepEqual(restored.candidateBytes, candidateBytes);
  assert.deepEqual(restored.artifactBytes, portable.artifactBytes);
  assert.deepEqual(restored.parityReceiptBytes, parityReceipt.bytes);
  assert.deepEqual(await resolveAdjustmentCandidateMaterial({
    archive,
    artifactSha256: portable.artifactSha256,
    candidateSha256: portable.candidateSha256,
    family: "temperature",
    journal: {
      readActiveDevelopmentMaterial: async () => ({
        actionSha256: HASH_A,
        artifactSha256: portable.artifactSha256,
        candidateGraphSha256: graphSha256,
        candidateSha256: portable.candidateSha256,
        family: "temperature",
        registrationSha256: HASH_B,
        shadowRegistrationSha256: HASH_C,
      }),
    },
    readHead: async () => graphSha256,
  }), { ...restored, candidateGraphSha256: graphSha256 });
  await assert.rejects(resolveAdjustmentCandidateMaterial({
    archive,
    artifactSha256: portable.artifactSha256,
    candidateSha256: portable.candidateSha256,
    family: "temperature",
    journal: {
      readActiveDevelopmentMaterial: async () => ({
        actionSha256: HASH_A,
        artifactSha256: portable.artifactSha256,
        candidateGraphSha256: HASH_F,
        candidateSha256: portable.candidateSha256,
        family: "temperature",
        registrationSha256: HASH_B,
        shadowRegistrationSha256: HASH_C,
      }),
    },
    readHead: async () => graphSha256,
  }), /candidate material graph is unavailable/u);
});

// retain rain v3 without widening the other family candidate contracts
test("development archive accepts only the rain v3 fit contract", async () => {
  const ports = createProductionAdjustmentMaintenanceControllerPorts();
  const rainCandidateJson = canonicalJsonBytes({
    contractVersion: "rain-maintenance-fit/v3",
  }).toString("utf8");

  await assert.rejects(ports.persistCandidate({
    due: { family: "rain" },
    fit: { candidateJson: rainCandidateJson },
    parity: null,
    registrationMaterial: null,
  }), /candidate parity, epoch witness or registration unavailable/u);

  // refuse the rain-only version under temperature and wind identities
  for (const family of ["temperature", "wind"]) {
    await assert.rejects(ports.persistCandidate({
      due: { family },
      fit: { candidateJson: rainCandidateJson },
      parity: null,
      registrationMaterial: null,
    }), /development candidate is invalid/u);
  }
});

// hold deterministic due and lease state for controller tests
class MemoryControllerJournal {
  constructor({ activeLeases = [], completed = [], registered = [] } = {}) {
    this.activeLeases = structuredClone(activeLeases);
    this.attemptContexts = new Map();
    this.due = new Map();
    this.events = [];
    this.locked = false;
    this.terminalRetirements = new Map();
    this.terminalOutcomes = new Map();

    // seed immutable completed due identities
    for (const dueKey of completed) {
      this.due.set(dueKey, {
        inputHeadSha256: HASH_A,
        outputSha256: HASH_F,
        status: "complete",
      });
    }

    // seed interrupted due identities
    for (const dueKey of registered) {
      this.due.set(dueKey, {
        inputHeadSha256: HASH_B,
        outputSha256: null,
        status: "registered",
      });
    }
  }

  // initialize without mutating fixture history
  async initialize() {
    assert.equal(this.locked, true);
    return { head: HASH_A, records: this.due.size };
  }

  // inspect only requested due keys
  async inspectDueKeys({ dueKeys }) {
    return dueKeys.map(
      // project absent or retained due state
      (dueKey) => ({
        dueKey,
        inputHeadSha256: this.due.get(dueKey)?.inputHeadSha256 ?? null,
        outputSha256: this.due.get(dueKey)?.outputSha256 ?? null,
        status: this.due.get(dueKey)?.status ?? "absent",
      }),
    );
  }

  // expose one bounded journal status
  async status() {
    return {
      activeLeases: structuredClone(this.activeLeases),
      fencingToken: "0",
      generation: "1",
      headSha256: this.activeLeases.length === 0 ? HASH_A : HASH_B,
      maxSeenUtc: null,
    };
  }

  // read one retained attempt identity
  async readDueAttemptContext({ dueKey }) {
    return structuredClone(this.attemptContexts.get(dueKey) ?? null);
  }

  // freeze one attempt identity before semantic work
  async recordDueAttemptContext(input) {
    const retained = this.attemptContexts.get(input.dueKey);

    // reject a second identity for the same immutable due
    if (retained !== undefined && (retained.inputHeadSha256 !== input.inputHeadSha256 ||
      retained.lifecycleHeadSha256 !== input.lifecycleHeadSha256)) {
      throw new Error("due attempt context differs");
    }
    const context = retained ?? {
      dueKey: input.dueKey,
      inputHeadSha256: input.inputHeadSha256,
      lifecycleHeadSha256: input.lifecycleHeadSha256,
    };
    this.attemptContexts.set(input.dueKey, context);
    return structuredClone(context);
  }

  // read one irreversible terminal result
  async readDueTerminalOutcome({ dueKey }) {
    return structuredClone(this.terminalOutcomes.get(dueKey) ?? null);
  }

  // retain one irreversible terminal result
  async recordDueTerminalOutcome({ dueKey, outcome }) {
    const retained = this.terminalOutcomes.get(dueKey);

    // require exact idempotency on retry
    if (retained !== undefined &&
      !canonicalJsonBytes(retained).equals(canonicalJsonBytes(outcome))) {
      throw new Error("due terminal outcome differs");
    }
    this.terminalOutcomes.set(dueKey, structuredClone(outcome));
    return structuredClone(outcome);
  }

  // read one retained owner retirement command
  async readDueTerminalRetirement({ dueKey }) {
    return structuredClone(this.terminalRetirements.get(dueKey) ?? null);
  }

  // retain one owner retirement command before slot mutation
  async recordDueTerminalRetirement({ dueKey, kind, request }) {
    const payload = {
      completed: false,
      kind,
      request: structuredClone(request),
      requestSha256: adjustmentSha256(canonicalJsonBytes(request)),
    };
    const retained = this.terminalRetirements.get(dueKey);

    // require exact retry bytes
    if (retained !== undefined &&
      !canonicalJsonBytes({ kind: retained.kind, request: retained.request })
        .equals(canonicalJsonBytes({ kind, request }))) {
      throw new Error("due terminal retirement differs");
    }
    this.terminalRetirements.set(dueKey, retained ?? payload);
    return structuredClone(retained ?? payload);
  }

  // close one exact owner retirement command
  async completeDueTerminalRetirement({ dueKey, requestSha256 }) {
    const retained = this.terminalRetirements.get(dueKey);
    assert.equal(retained?.requestSha256, requestSha256);
    retained.completed = true;
    return structuredClone(retained);
  }

  // acquire one controller lease
  async acquireLease(input) {
    assert.equal(this.locked, true);
    this.events.push({ input, kind: "acquire" });
    this.due.set(input.dueKey, {
      inputHeadSha256: input.inputHeadSha256,
      outputSha256: null,
      status: "registered",
    });
    this.activeLeases = [{
      dueKey: input.dueKey,
      expiresAt: "2099-01-01T00:00:00.000Z",
      runId: input.runId,
      scope: input.scope,
    }];
    return { status: "acquired" };
  }

  // complete one exact due output
  async completeDue(input) {
    assert.equal(this.locked, true);
    this.events.push({ input, kind: "complete" });
    const due = this.due.get(input.dueKey);
    this.due.set(input.dueKey, {
      inputHeadSha256: due.inputHeadSha256,
      outputSha256: input.outputSha256,
      status: "complete",
    });
    return { status: "complete" };
  }

  // release one exact controller lease
  async releaseLease(input) {
    assert.equal(this.locked, true);
    this.events.push({ input, kind: "release" });
    this.activeLeases = this.activeLeases.filter(
      // retain unrelated scopes only
      (lease) => lease.scope !== input.scope,
    );
    return { status: "released" };
  }

  // reconcile one expired controller lease
  async reconcileExpiredLease(input) {
    assert.equal(this.locked, true);
    this.events.push({ input, kind: "reconcile" });
    this.activeLeases = this.activeLeases.filter(
      // close only the exact reconciled scope
      (lease) => lease.scope !== input.scope,
    );
    return { status: "reconciled" };
  }

  // preregister one local confirmation identity before release publication
  async preregisterConfirmation(input) {
    assert.equal(this.locked, true);
    this.events.push({ input, kind: "preregister_confirmation" });
    return {
      registrationSha256: adjustmentSha256(canonicalJsonBytes(input)),
      status: "registered",
    };
  }

  // retain one acknowledged development archive binding
  async recordDevelopmentCandidateInstalled(input) {
    assert.equal(this.locked, true);
    this.events.push({ input, kind: "development_installed" });
    return { ...input };
  }
}

// create one closed unscored family projection
function unavailableFamily(family, unit) {
  const metric = {
    adjustedBias: null,
    adjustedMae: null,
    adjustedP95: null,
    adjustedRmse: null,
    deltaMae: null,
    rawBias: null,
    rawMae: null,
    rawP95: null,
    rawRmse: null,
    skillInterval95: null,
    skillPercent: null,
    unit,
  };
  return {
    bestMatchDiagnostic: null,
    comparisonState: "unscored",
    evidenceClass: "development",
    evidenceCutoffAt: null,
    family,
    metrics: metric,
    qualificationState: "pending_support",
    rainDiagnostics: family === "rain"
      ? {
        accumulations: [6, 12, 23].map(
          // retain every required window without inventing a metric
          (hours) => ({ adjustedMae: null, completeWindows: 0, hours, rawMae: null }),
        ),
        annualBalancedVolumeRatio: null,
        heavyAdjustedMae: null,
        heavyRawMae: null,
        probabilityOrderViolationCount: 0,
        thresholds: [0.1, 1, 2.5].map(
          // retain every required threshold without inventing support
          (thresholdMmPerHour) => ({
            adjustedBrier: null,
            csi: null,
            falseAlarms: 0,
            far: null,
            hits: 0,
            misses: 0,
            pod: null,
            rawBrier: null,
            reliability: Array.from({ length: 10 }, () => ({
              count: 0,
              meanProbability: null,
              observedFrequency: null,
            })),
            thresholdMmPerHour,
          }),
        ),
        wetAdjustedMae: null,
        wetRawMae: null,
        winterBalancedVolumeRatio: null,
      }
      : null,
    recommendation: "retain",
    servingIdentitySha256: null,
    servingState: "fail_raw",
    slices: [],
    support: {
      dateCount: 0,
      effectiveWeightSum: 0,
      eventCount: 0,
      excludedCount: 0,
      exclusionReasons: {},
      fallbackCount: 0,
      fallbackReasons: {},
      gapCount: 0,
      rowCount: 0,
      targetRowCount: 0,
      validHourCount: 0,
      vintageCount: 0,
      wetDateCount: 0,
      wetRowCount: 0,
    },
    supportState: "invalid",
  };
}

// create one complete injected controller surface
function controllerPorts(journal, options = {}) {
  const reports = [];
  const archive = {
    // initialize under the process lock
    initialize: async () => {
      assert.equal(journal.locked, true);
      return { rootKind: "home_native_ext4_primary" };
    },
    // expose a stable path-free capacity projection
    status: async () => ({
      allocatedBytes: "4096",
      backingAllocatedBytes: "4096",
      backingFreeBytes: "20000000000",
      backingFreeInodes: "100000",
      backingRootKind: "windows_disk0_vhdx",
      fileCount: 3,
      freeBytes: "20000000000",
      freeInodes: "100000",
      incomingCount: 0,
      objectCount: 1,
      rootKind: "home_native_ext4_primary",
    }),
  };
  return {
    archive,
    clock: () => new Date(options.now ?? "2026-10-10T12:00:00.000Z"),
    ...(options.initializeLifecycle === undefined
      ? {}
      : { initializeLifecycle: options.initializeLifecycle }),
    inspectSemanticInput: options.inspectSemanticInput ?? (async () => ({
      reason: "semantic_catalog_unavailable",
      state: "blocked",
    })),
    journal,
    persistCandidate: options.persistCandidate ?? null,
    publishAttemptReport: options.publishAttemptReport ?? (async (report) => {
      assert.equal(journal.locked, true);
      validateAdjustmentMaintenanceAttempt(report);
      reports.push(structuredClone(report));
      return {
        manifestObjectSha256: adjustmentSha256(`manifest:${report.dueKey}`),
        reportSha256: adjustmentSha256(JSON.stringify(report)),
      };
    }),
    publishDevelopmentCandidate: options.publishDevelopmentCandidate ?? (async ({ due }) => ({
      actionSha256: adjustmentSha256(`action:${due.dueKey}`),
      state: "acknowledged",
    })),
    reconcileTerminalOutcome: options.reconcileTerminalOutcome ?? (async ({ dueKey }) => {
      const retained = await journal.readDueTerminalRetirement({ dueKey });

      // model exact successful idempotent owner retirement in controller tests
      if (retained !== null && !retained.completed) {
        await journal.completeDueTerminalRetirement({
          dueKey,
          requestSha256: retained.requestSha256,
        });
      }
      return retained;
    }),
    ...(options.publishRainControlReference === undefined
      ? {}
      : { publishRainControlReference: options.publishRainControlReference }),
    readRegistrationSchedule: options.readRegistrationSchedule ??
      (async () => registrationScheduleStatus(
        options.now ?? "2026-10-10T12:00:00.000Z",
      )),
    reports,
    runFit: options.runFit ?? (async () => {
      throw new Error("fit must not run");
    }),
    ...(options.synchronizeEvidence === undefined
      ? {}
      : { synchronizeEvidence: options.synchronizeEvidence }),
    withProcessLock: async (operation) => {
      assert.equal(journal.locked, false);
      journal.locked = true;

      try {
        return await operation();
      } finally {
        journal.locked = false;
      }
    },
  };
}

test("controller arguments accept only fixed daily and monthly modes", () => {
  assert.equal(parseAdjustmentMaintenanceControllerArguments(["--daily"]), "daily");
  assert.equal(parseAdjustmentMaintenanceControllerArguments(["--monthly"]), "monthly");

  // reject paths, combined modes and caller options
  for (const arguments_ of [[], ["daily"], ["--daily", "--monthly"], ["--daily=/tmp/x"]]) {
    assert.throws(() => parseAdjustmentMaintenanceControllerArguments(arguments_));
  }
});

test("candidate parity receipt binds matching native and packaged byte streams", () => {
  const input = Buffer.from("{\"input\":1}\n");
  const output = Buffer.from("{\"output\":2}\n");
  const receipt = buildAdjustmentMaintenanceParityReceipt({
    candidateSha256: HASH_A,
    family: "temperature",
    parity: {
      retainedInput: input,
      retainedNativeOutput: output,
      retainedPackagedOutput: output,
      syntheticInput: input,
      syntheticNativeOutput: output,
      syntheticPackagedOutput: output,
    },
  });
  assert.equal(receipt.fixture.contractVersion, "forecast-adjustment-model-parity/v1");
  assert.equal(receipt.fixture.candidateSha256, HASH_A);
  assert.equal(receipt.paritySha256, adjustmentSha256(receipt.bytes));
  assert.throws(() => buildAdjustmentMaintenanceParityReceipt({
    candidateSha256: HASH_A,
    family: "temperature",
    parity: {
      retainedInput: input,
      retainedNativeOutput: output,
      retainedPackagedOutput: Buffer.from("{\"output\":3}\n"),
      syntheticInput: input,
      syntheticNativeOutput: output,
      syntheticPackagedOutput: output,
    },
  }), /outputs differ/u);
});

test("development custody graph archives the action, registration, candidate and report", () => {
  const candidateBytes = Buffer.from("{\"contractVersion\":\"candidate/v1\"}\n");
  const candidateSha256 = adjustmentSha256(candidateBytes);
  const artifact = {
    bundleSha256: HASH_B,
    candidateSha256,
    contractVersion: "forecast-adjustment-maintenance-runtime-package/v1",
    dueMonth: "2026-11",
    family: "temperature",
    model: {},
    source: {},
  };
  const artifactBytes = canonicalJsonBytes(artifact);
  const { registration } = buildAdjustmentRollingShadowRegistration({
    artifactSha256: HASH_B,
    candidateSha256,
    cohortSha256: HASH_C,
    epochAt: "2026-01-10T08:00:00.000Z",
    epochWitnessSha256: HASH_D,
    family: "temperature",
    fitMonth: "2026-11",
    policySha256: HASH_E,
    predecessorRegistrationSha256: null,
    predecessorTerminalAt: null,
    requestedAt: "2026-11-30T08:00:00.000Z",
    reservedKeySha256: HASH_F,
    sourceSha256: HASH_A,
  });
  const report = {
    actionEligible: false,
    attemptIdentitySha256: HASH_A,
    candidateGraphSha256: HASH_C,
    candidateSha256,
    contractVersion: "forecast-adjustment-maintenance-attempt/v2",
    dueKey: "monthly/temperature/2026-11",
    family: "temperature",
    fitReceiptSha256: HASH_B,
    inputHeadSha256: HASH_C,
    lifecycleHeadSha256: HASH_D,
    mode: "monthly",
    originalCutoffAt: "2026-11-01T07:00:00.000Z",
    reason: "development_candidate_archived",
    reservedConfirmationExposed: false,
    semanticInputSha256: HASH_E,
    servingChanged: false,
    state: "completed",
  };
  const reportBytes = canonicalJsonBytes(report);
  const action = {
    actionKind: "shadow",
    candidateGraphSha256: HASH_C,
    candidateSha256,
    contractVersion: "forecast-adjustment-model-action/v1",
    createdAt: "2026-12-01T08:00:00.000Z",
    expectedInstalledReceiptSha256: null,
    expectedSettingsSha256: HASH_A,
    expectedSourceCommit: "1".repeat(40),
    expectedSourceRelease: "2026.10.09-1",
    family: "temperature",
    fencingToken: "1",
    fullMemberRootSha256: null,
    lifecycleHeadSha256: HASH_D,
    policyDecision: "pending",
    policyReportSha256: adjustmentSha256(reportBytes),
    predecessorActionSha256: null,
    reason: "development_candidate",
    reportCreatedAt: "2026-12-01T08:00:00.000Z",
    siteKey: "ballydidean",
    validThrough: "2026-12-08T08:00:00.000Z",
  };
  const graph = buildAdjustmentDevelopmentCustodyGraphSegment({
    action,
    artifactBytes,
    candidateBytes,
    candidateGraphSha256: HASH_C,
    registration,
    reportBytes,
  });
  assert.equal(graph.segment.members.length, 7);
  assert.equal(graph.segment.members[0].identitySha256, graph.bindingSha256);
  assert.deepEqual(graph.binding, {
    actionSha256: adjustmentSha256(canonicalJsonBytes(action)),
    artifactSha256: HASH_B,
    candidateGraphSha256: HASH_C,
    candidateSha256,
    contractVersion: "adjustment-development-custody-graph/v1",
    policyReportSha256: adjustmentSha256(reportBytes),
    registrationSha256: registration.registrationSha256,
    sourceSha256: HASH_A,
  });
  assert.equal(graph.segment.crossLinks.length, 6);
});

test("confirmation plan graph retains unique verified cold graph references", () => {
  const entries = [{
    comparatorMemberSha256: HASH_A,
    dueKey: "capture/2027-09-01T00:35:00.000Z",
    graphManifestSha256: HASH_B,
    payloadIdentitySha256: HASH_C,
    predictionBodySha256: HASH_D,
    receiptSha256: HASH_E,
    sourceProjectionSha256: HASH_F,
    sourceReceiptSha256: HASH_A,
  }, {
    comparatorMemberSha256: HASH_B,
    dueKey: "capture/2027-09-01T06:35:00.000Z",
    graphManifestSha256: HASH_B,
    payloadIdentitySha256: HASH_D,
    predictionBodySha256: HASH_E,
    receiptSha256: HASH_F,
    sourceProjectionSha256: HASH_A,
    sourceReceiptSha256: HASH_C,
  }];
  const eligiblePredictionSetSha256 = adjustmentSha256(canonicalJsonBytes(entries));
  const unsigned = {
    contractVersion: "adjustment-maintenance-confirmation-plan-graph/v1",
    eligiblePredictionSetSha256,
    entries,
    family: "wind",
    registrationSha256: HASH_F,
  };
  const planGraph = {
    ...unsigned,
    planGraphSha256: adjustmentSha256(canonicalJsonBytes(unsigned)),
  };
  const segment = buildAdjustmentConfirmationPlanGraphSegment({ planGraph });
  assert.equal(segment.members.length, 2);
  assert.equal(segment.members[0].identitySha256, planGraph.planGraphSha256);
  assert.equal(segment.members[1].kind, "adjustment-archive-graph-reference/v1");
  assert.deepEqual(JSON.parse(segment.members[1].payload), {
    contractVersion: "adjustment-archive-graph-reference/v1",
    graphSha256: HASH_B,
  });
  assert.equal(segment.crossLinks.length, 1);
});

test("controller initializes the authenticated lifecycle before due planning", async () => {
  const journal = new MemoryControllerJournal();
  const calls = [];
  const ports = controllerPorts(journal, {
    initializeLifecycle: async () => {
      assert.equal(journal.locked, true);
      calls.push("lifecycle");
    },
    now: "2026-10-08T12:00:00.000Z",
    synchronizeEvidence: async (cutoffAt) => {
      assert.equal(journal.locked, true);
      assert.equal(cutoffAt, "2026-10-08T12:00:00.000Z");
      calls.push("evidence");
      return { state: "captured_through_watermark" };
    },
  });
  const result = await runAdjustmentMaintenanceController(["--daily"], { ports });
  assert.deepEqual(calls, ["lifecycle", "evidence"]);
  assert.deepEqual(result.attempts, []);
});

test("capture cycle performs exactly one locked evidence synchronization", async () => {
  const journal = new MemoryControllerJournal();
  const calls = [];
  const ports = controllerPorts(journal, {
    initializeLifecycle: async () => {
      assert.equal(journal.locked, true);
      calls.push("lifecycle");
    },
    now: "2026-10-08T12:00:00.000Z",
    synchronizeEvidence: async (cutoffAt) => {
      assert.equal(journal.locked, true);
      calls.push(cutoffAt);
      return { state: "page_captured" };
    },
  });
  const result = await runAdjustmentRevisionCaptureCycle({ ports });
  assert.deepEqual(result, { state: "page_captured" });
  assert.deepEqual(calls, ["lifecycle", "2026-10-08T12:00:00.000Z"]);
});

test("daily planning selects seven oldest dates and one old month per family", async () => {
  const completed = [
    "daily/2026-10-08",
    "daily/2026-10-09",
    "monthly/temperature/2026-10",
    "monthly/wind/2026-10",
    "monthly/rain/2026-10",
  ];
  const journal = new MemoryControllerJournal({ completed });
  const dues = await planAdjustmentMaintenanceDues({
    journal,
    mode: "daily",
    now: new Date("2026-12-10T12:00:00.000Z"),
    schedule: registrationScheduleStatus("2026-12-10T12:00:00.000Z"),
  });
  assert.deepEqual(
    dues.filter((due) => due.mode === "daily").map((due) => due.dueKey),
    [
      "daily/2026-10-10",
      "daily/2026-10-11",
      "daily/2026-10-12",
      "daily/2026-10-13",
      "daily/2026-10-14",
      "daily/2026-10-15",
      "daily/2026-10-16",
    ],
  );
  assert.deepEqual(
    dues.filter((due) => due.mode === "monthly").map((due) => due.dueKey),
    [
      "monthly/temperature/2026-11",
      "monthly/wind/2026-11",
      "monthly/rain/2026-11",
    ],
  );
});

// prefer the earliest eligible terminal instead of fixed family order
test("daily lifecycle selection cannot starve another terminal family", () => {
  const entries = [{
    activeRegistration: { terminalAt: "2027-01-20T08:00:00.000Z" },
    slot: { family: "temperature", state: "busy_v3" },
  }, {
    activeRegistration: { terminalAt: "2027-01-09T08:00:00.000Z" },
    slot: { family: "wind", state: "busy_v3" },
  }, {
    activeRegistration: { terminalAt: "2027-01-15T08:00:00.000Z" },
    slot: { family: "rain", state: "busy_v3" },
  }];
  assert.equal(selectAdjustmentDailyLifecycleEntry(
    entries,
    "2027-01-10T08:00:00.000Z",
  ).slot.family, "wind");
});

test("daily planning follows the authenticated rolling horizon after the legacy cutoff", async () => {
  const snapshotAt = "2028-01-10T12:00:00.000Z";
  const horizonEndAt = "2029-11-19T08:00:00.000Z";
  const schedule = {
    ...registrationScheduleStatus(snapshotAt),
    epochAt: "2028-01-01T08:00:00.000Z",
    horizonEndAt,
    slots: ["temperature", "wind", "rain"].map(
      // bind every test slot to the extended authenticated horizon
      (family) => ({ ...registrationSlot(family), horizonEndAt }),
    ),
  };
  const dues = await planAdjustmentMaintenanceDues({
    journal: new MemoryControllerJournal(),
    mode: "daily",
    now: new Date(snapshotAt),
    schedule,
  });
  assert.deepEqual(
    dues.filter((due) => due.mode === "daily").map((due) => due.dueKey),
    [
      "daily/2028-01-02", "daily/2028-01-03", "daily/2028-01-04",
      "daily/2028-01-05", "daily/2028-01-06", "daily/2028-01-07",
      "daily/2028-01-08",
    ],
  );
  await assert.rejects(planAdjustmentMaintenanceDues({
    journal: new MemoryControllerJournal(),
    mode: "daily",
    now: new Date("2028-01-10T12:00:01.000Z"),
    schedule,
  }), /clock differs/u);
});

// retire one immutable empty monthly cutoff so the next month can advance
test("monthly no-candidate evidence does not starve later rolling months", async () => {
  const journal = new MemoryControllerJournal();
  const inspection = async ({ due }) => due.mode === "daily"
    ? {
        inputManifestSha256: HASH_A,
        reason: "no_registered_candidate",
        state: "idle",
      }
    : {
        inputManifestSha256: adjustmentSha256(`empty:${due.dueKey}`),
        reason: "incomplete_development_population",
        state: "no_candidate",
      };
  const octoberPorts = controllerPorts(journal, {
    inspectSemanticInput: inspection,
    now: "2026-11-10T12:00:00.000Z",
  });
  const october = await runAdjustmentMaintenanceController(["--daily"], {
    ports: octoberPorts,
  });
  assert.deepEqual(october.attempts.filter((attempt) =>
    attempt.dueKey.startsWith("monthly/")).map((attempt) => attempt.reason), [
    "monthly_no_candidate", "monthly_no_candidate", "monthly_no_candidate",
  ]);

  const novemberPorts = controllerPorts(journal, {
    inspectSemanticInput: inspection,
    now: "2026-12-10T12:00:00.000Z",
  });
  const november = await runAdjustmentMaintenanceController(["--daily"], {
    ports: novemberPorts,
  });
  assert.deepEqual(november.attempts.filter((attempt) =>
    attempt.dueKey.startsWith("monthly/")).map((attempt) => attempt.dueKey), [
    "monthly/temperature/2026-11",
    "monthly/wind/2026-11",
    "monthly/rain/2026-11",
  ]);
});

test("daily planning schedules one immutable rain reference only inside the pre-month window", async () => {
  const snapshotAt = "2026-10-29T12:00:00.000Z";
  const journal = new MemoryControllerJournal();
  const dues = await planAdjustmentMaintenanceDues({
    journal,
    mode: "daily",
    now: new Date(snapshotAt),
    schedule: registrationScheduleStatus(snapshotAt),
  });
  assert.deepEqual(dues.filter((due) => due.scope === "control-reference/rain"), [{
    dueKey: "control-reference/rain/2026-11",
    family: "rain",
    mode: "monthly",
    originalCutoffAt: "2026-10-25T00:00:00.000Z",
    scope: "control-reference/rain",
  }]);
  const replay = await planAdjustmentMaintenanceDues({
    journal: new MemoryControllerJournal({ completed: ["control-reference/rain/2026-11"] }),
    mode: "daily",
    now: new Date(snapshotAt),
    schedule: registrationScheduleStatus(snapshotAt),
  });
  assert.equal(replay.some((due) => due.scope === "control-reference/rain"), false);
  const early = await planAdjustmentMaintenanceDues({
    journal: new MemoryControllerJournal(),
    mode: "daily",
    now: new Date("2026-10-24T12:00:00.000Z"),
    schedule: registrationScheduleStatus("2026-10-24T12:00:00.000Z"),
  });
  assert.equal(early.some((due) => due.scope === "control-reference/rain"), false);
});

test("control reference publishes genuine state without fitting or occupying a family slot", async () => {
  const journal = new MemoryControllerJournal();
  const material = rainControlReferenceMaterial("2026-11");
  let published = null;
  const ports = controllerPorts(journal, {
    inspectSemanticInput: async () => ({
      controlReferenceMaterial: material,
      inputManifestSha256: HASH_F,
      state: "ready",
    }),
    now: "2026-10-29T12:00:00.000Z",
    publishRainControlReference: async (request) => {
      published = request;
      return { actionSha256: HASH_B, state: "acknowledged" };
    },
  });
  journal.locked = true;

  try {
    const result = await executeAdjustmentMaintenanceDue({
      due: {
        dueKey: "control-reference/rain/2026-11",
        family: "rain",
        mode: "monthly",
        originalCutoffAt: "2026-10-25T00:00:00.000Z",
        scope: "control-reference/rain",
      },
      now: new Date("2026-10-29T12:00:00.000Z"),
      ports,
    });
    assert.equal(result.reason, "control_reference_archived");
    assert.equal(result.actionSha256, HASH_B);
    assert.equal(journal.due.get("control-reference/rain/2026-11").status, "complete");
    assert.deepEqual(published.material, material);
  } finally {
    journal.locked = false;
  }
});

test("blocked daily run reconciles expiry and persists no fake qualification", async () => {
  const journal = new MemoryControllerJournal({
    activeLeases: [{
      dueKey: "daily/2026-10-08",
      expiresAt: "2026-10-09T00:00:00.000Z",
      runId: "old-daily-run",
      scope: "daily",
    }],
    registered: ["daily/2026-10-08"],
  });
  const ports = controllerPorts(journal);
  const result = await runAdjustmentMaintenanceController(["--daily"], { ports });
  assert.equal(result.attempts.length, 2);
  assert.equal(journal.events.filter((event) => event.kind === "reconcile").length, 1);
  assert.equal(ports.reports.length, 2);

  // retain only honest blocked reports
  for (const report of ports.reports) {
    assert.equal(report.state, "blocked");
    assert.equal(report.reason, "semantic_catalog_unavailable");
    assert.equal(report.actionEligible, false);
    assert.equal(report.servingChanged, false);
    assert.equal(report.reservedConfirmationExposed, false);
    assert.equal(report.candidateSha256, null);
    assert.equal(report.lifecycleHeadSha256, HASH_B);
    assert.equal(JSON.stringify(report).includes("approval"), false);
  }
  assert.equal(journal.due.get("daily/2026-10-08").status, "registered");
  assert.equal(journal.due.get("daily/2026-10-09").status, "registered");
});

test("daily run invokes the closed evaluator and retains incomplete history", async () => {
  const journal = new MemoryControllerJournal();
  const witness = futureOnlyEpochWitness();
  const ports = controllerPorts(journal, {
    inspectSemanticInput: async ({ due }) => ({
      dailyEvaluationInput: {
        catalogInputManifestSha256: HASH_C,
        clockAt: "2026-10-10T12:00:00.000Z",
        due,
        epochWitness: witness,
        lifecycle: {
          family: "temperature",
          registrationSha256: null,
          state: "pending",
        },
      },
      inputManifestSha256: HASH_C,
      state: "ready",
    }),
  });
  const result = await runAdjustmentMaintenanceController(["--daily"], { ports });
  assert.deepEqual(result.attempts.map((attempt) => attempt.reason), [
    "daily_history_pending", "daily_history_pending",
  ]);
  assert.equal(ports.reports.every((report) => report.semanticInputSha256 !== null), true);
  assert.equal(journal.events.some((event) => event.kind === "complete"), false);
});

test("daily run completes a verified snapshot when no candidate is registered", async () => {
  const journal = new MemoryControllerJournal();
  const ports = controllerPorts(journal, {
    inspectSemanticInput: async () => ({
      inputManifestSha256: HASH_C,
      reason: "no_registered_candidate",
      state: "idle",
    }),
  });
  const result = await runAdjustmentMaintenanceController(["--daily"], { ports });
  assert.deepEqual(result.attempts.map((attempt) => attempt.reason), [
    "daily_no_registered_candidate", "daily_no_registered_candidate",
  ]);
  assert.equal(journal.events.filter((event) => event.kind === "complete").length, 2);
});

test("one due failure does not stop later bounded backlog reconciliation", async () => {
  const journal = new MemoryControllerJournal();
  const ports = controllerPorts(journal);
  let publications = 0;
  ports.publishAttemptReport = async (report) => {
    publications += 1;

    // fail only the first due before its completion
    if (publications === 1) {
      throw new Error("injected publication failure");
    }
    return {
      manifestObjectSha256: adjustmentSha256(`manifest:${report.dueKey}`),
      reportSha256: adjustmentSha256(JSON.stringify(report)),
    };
  };
  const result = await runAdjustmentMaintenanceController(["--daily"], { ports });
  assert.deepEqual(result.attempts.map((attempt) => attempt.state), ["failed", "blocked"]);
  assert.equal(result.attempts[0].reason, "controller_error");
  assert.equal(journal.due.get("daily/2026-10-08").status, "registered");
  assert.equal(journal.due.get("daily/2026-10-09").status, "registered");
});

test("monthly run archives and installs one inactive development shadow per family", async () => {
  const journal = new MemoryControllerJournal();
  const candidateJson = "{\"contractVersion\":\"candidate/v1\"}\n";
  const released = [];
  const ports = controllerPorts(journal, {
    inspectSemanticInput: async ({ due }) => ({
      fitInput: { contractVersion: `${due.family}-fit-input/v1` },
      inputManifestSha256: HASH_B,
      registrationSlot: registrationSlot(due.family),
      runtimeReadiness: {},
      runtimeReadinessSha256: HASH_C,
      state: "ready",
    }),
    now: "2026-11-02T15:00:00.000Z",
    persistCandidate: async ({ fit }) => ({
      candidateGraphSha256: adjustmentSha256(`graph:${fit.family}`),
    }),
    publishDevelopmentCandidate: async ({ due }) => {
      released.push(due.family);
      return {
        actionSha256: adjustmentSha256(`action:${due.dueKey}`),
        state: "acknowledged",
      };
    },
    runFit: async ({ family }) => ({
      candidateJson,
      candidateSha256: adjustmentSha256(Buffer.from(candidateJson)),
      codeSnapshotSha256: HASH_D,
      contractVersion: "adjustment-fit-sandbox/v1",
      family,
      inputSnapshotSha256: HASH_E,
      runtimeReadinessSha256: HASH_C,
      stderr: { bytes: 0, label: "stderr", overflow: false, sha256: HASH_A },
      stdout: { bytes: candidateJson.length, label: "stdout", overflow: false, sha256: HASH_B },
    }),
  });
  const result = await runAdjustmentMaintenanceController(["--monthly"], { ports });
  assert.equal(result.attempts.length, 3);
  assert.deepEqual(
    ports.reports.map((report) => report.family),
    ["temperature", "wind", "rain"],
  );
  assert.deepEqual(released, ["temperature", "wind", "rain"]);

  // preserve completed fits as archived inactive candidate records
  for (const report of ports.reports) {
    assert.equal(report.state, "completed");
    assert.equal(report.reason, "development_candidate_archived");
    assert.equal(report.actionEligible, false);
    assert.match(report.candidateGraphSha256, /^[a-f0-9]{64}$/u);
  }
  const replay = await runAdjustmentMaintenanceController(["--monthly"], { ports });
  assert.equal(replay.attempts.length, 0);
});

test("monthly fit archives descriptors and preregisters before shadow publication", async () => {
  const journal = new MemoryControllerJournal();
  const candidateJson = "{\"contractVersion\":\"candidate/v1\"}\n";
  const registrationMaterial = { confirmation: {
    candidateKind: "wind-robust-hierarchical-median/v1",
    candidateSha256: HASH_A,
    cohortLineageSha256: HASH_B,
    family: "wind",
    firstTargetAt: "2027-09-01T07:00:00.000Z",
    gateManifestSha256: HASH_C,
    intervalEndExclusiveLocalDate: "2028-09-01",
    intervalStartLocalDate: "2027-09-01",
    reservedKeySha256: HASH_D,
    sourceLineageSha256: HASH_E,
    terminalAccessAt: "2028-09-08T07:00:00.000Z",
  } };
  let archived = null;
  let published = null;
  const ports = controllerPorts(journal, {
    inspectSemanticInput: async () => ({
      fitInput: { contractVersion: "wind-fit-input/v1" },
      inputManifestSha256: HASH_B,
      registrationSlot: registrationSlot("wind"),
      runtimeReadiness: {},
      runtimeReadinessSha256: HASH_C,
      state: "ready",
    }),
    now: "2027-08-02T12:00:00.000Z",
    persistCandidate: async (request) => {
      archived = request;
      return { candidateGraphSha256: HASH_F };
    },
    publishDevelopmentCandidate: async (request) => {
      published = request;
      return { actionSha256: HASH_E, state: "acknowledged" };
    },
    runFit: async () => ({
      candidateJson,
      candidateSha256: adjustmentSha256(Buffer.from(candidateJson)),
      codeSnapshotSha256: HASH_D,
      contractVersion: "adjustment-fit-sandbox/v1",
      family: "wind",
      inputSnapshotSha256: HASH_E,
      runtimeReadinessSha256: HASH_C,
      stderr: { bytes: 0, label: "stderr", overflow: false, sha256: HASH_A },
      stdout: { bytes: candidateJson.length, label: "stdout", overflow: false, sha256: HASH_B },
    }),
  });
  ports.buildCandidateRegistration = async () => registrationMaterial;
  journal.locked = true;

  try {
    const result = await executeAdjustmentMaintenanceDue({
      due: {
        dueKey: "monthly/wind/2027-08",
        family: "wind",
        mode: "monthly",
        originalCutoffAt: "2027-08-01T07:00:00.000Z",
        scope: "monthly/wind",
      },
      now: new Date("2027-08-02T12:00:00.000Z"),
      ports,
    });
    assert.equal(result.reason, "development_candidate_archived");
    assert.equal(archived.registrationMaterial, registrationMaterial);
    assert.equal(journal.events.some((event) => event.kind ===
      "preregister_confirmation"), true);
    assert.equal(published.material.registrationMaterial, registrationMaterial);
    assert.match(published.material.confirmationRegistrationSha256,
      /^[a-f0-9]{64}$/u);
  } finally {
    journal.locked = false;
  }
});

test("monthly work refuses fitter execution while the family slot is busy", async () => {
  const journal = new MemoryControllerJournal();
  let fitCalls = 0;
  const ports = controllerPorts(journal, {
    inspectSemanticInput: async ({ due }) => ({
      fitInput: { contractVersion: `${due.family}-fit-input/v1` },
      inputManifestSha256: HASH_B,
      registrationSlot: registrationSlot(due.family, "busy_v3"),
      runtimeReadiness: {},
      runtimeReadinessSha256: HASH_C,
      state: "ready",
    }),
    now: "2026-11-02T15:00:00.000Z",
    runFit: async () => {
      fitCalls += 1;
      throw new Error("busy slot must not fit");
    },
  });
  const result = await runAdjustmentMaintenanceController(["--monthly"], { ports });
  assert.equal(fitCalls, 0);
  assert.deepEqual(result.attempts.map((attempt) => attempt.reason), [
    "family_slot_busy", "family_slot_busy", "family_slot_busy",
  ]);
  assert.deepEqual(ports.reports.map((report) => report.state), [
    "blocked", "blocked", "blocked",
  ]);
});

test("rolling registration builder binds the shared plan and v3 identity", () => {
  const built = buildAdjustmentRollingShadowRegistration({
    artifactSha256: HASH_A,
    candidateSha256: HASH_B,
    cohortSha256: HASH_C,
    epochAt: "2026-10-09T04:10:45.000Z",
    epochWitnessSha256: HASH_D,
    family: "rain",
    fitMonth: "2027-08",
    policySha256: HASH_E,
    predecessorRegistrationSha256: null,
    predecessorTerminalAt: null,
    requestedAt: "2027-08-02T12:00:00.000Z",
    reservedKeySha256: HASH_F,
    sourceSha256: HASH_A,
  });
  assert.equal(built.registration.intervalStartAt, "2027-09-01T07:00:00.000Z");
  assert.equal(built.registration.intervalEndAt, "2028-07-31T07:00:00.000Z");
  assert.equal(built.registration.terminalAt, "2028-08-07T07:00:00.000Z");
  assert.equal(built.registration.epochWitnessSha256, HASH_D);
  assert.equal(built.plan.planSha256,
    adjustmentSha256(canonicalJsonBytes(Object.fromEntries(
      Object.entries(built.plan).filter(([key]) => key !== "planSha256"),
    ))));
  assert.match(built.registration.registrationSha256, /^[a-f0-9]{64}$/u);
});

test("registration descriptors bind actual policy, cohort and all-cycle plan roots", () => {
  const policy = buildAdjustmentRegistrationPolicyDescriptor({ family: "temperature" });
  assert.deepEqual(validateAdjustmentRegistrationPolicyDescriptor(policy.descriptor),
    policy.descriptor);
  assert.equal(policy.descriptor.policyModuleSha256,
    adjustmentSha256(readFileSync(new URL(
      "./adjustment-maintenance-runtime/forecast/maintenance-policy.js",
      import.meta.url,
    ))));
  assert.throws(() => validateAdjustmentRegistrationPolicyDescriptor({
    ...policy.descriptor,
    policyModuleSha256: HASH_A,
  }), /differs/u);
  const cohort = buildAdjustmentRegistrationCohortDescriptor({
    cutoffAt: "2027-08-01T00:00:00.000Z",
    dueMonth: "2027-08",
    epochWitnessSha256: HASH_A,
    family: "temperature",
    historicalMemberRootSha256: HASH_B,
    inputManifestSha256: HASH_C,
  });
  assert.deepEqual(validateAdjustmentRegistrationCohortDescriptor(cohort.descriptor),
    cohort.descriptor);
  const changedCohort = validateAdjustmentRegistrationCohortDescriptor({
    ...cohort.descriptor,
    historicalMemberRootSha256: HASH_D,
  });
  assert.notEqual(adjustmentSha256(canonicalJsonBytes(changedCohort)), cohort.cohortSha256);
  const expected = buildAdjustmentRegistrationExpectedKeyPlan({
    family: "temperature",
    intervalEndAt: "2028-09-01T07:00:00.000Z",
    intervalStartAt: "2027-09-01T07:00:00.000Z",
  });
  assert.deepEqual(validateAdjustmentRegistrationExpectedKeyPlan(expected.descriptor),
    expected.descriptor);
  assert.equal(expected.descriptor.logicalChunkCount, 27);
  assert.equal(expected.bytes.length < 256 * 1_024, true);
  assert.throws(() => validateAdjustmentRegistrationExpectedKeyPlan({
    ...expected.descriptor,
    reservedKeySha256: HASH_E,
  }), /differs/u);
});

test("post-fit registration binds archive provenance before local preregistration", () => {
  const material = buildAdjustmentPostFitRegistrationMaterial({
    artifactSha256: HASH_A,
    candidateSha256: HASH_B,
    cutoffAt: "2027-08-01T00:00:00.000Z",
    dueMonth: "2027-08",
    epochWitness: futureOnlyEpochWitness(),
    family: "wind",
    historicalMemberRootSha256: HASH_C,
    inputManifestSha256: HASH_D,
    predecessorRegistrationSha256: null,
    predecessorTerminalAt: null,
    requestedAt: "2027-08-02T12:00:00.000Z",
    sourceIdentitySha256: HASH_E,
  });
  assert.equal(material.shadowRegistration.candidateSha256, HASH_B);
  assert.equal(material.shadowRegistration.artifactSha256, HASH_A);
  assert.equal(material.shadowRegistration.policySha256, material.policy.policySha256);
  assert.equal(material.shadowRegistration.cohortSha256, material.cohort.cohortSha256);
  assert.equal(material.shadowRegistration.sourceSha256, material.source.sourceSha256);
  assert.equal(material.shadowRegistration.reservedKeySha256,
    material.expectedPlan.reservedKeySha256);
  assert.equal(material.confirmation.candidateKind,
    "wind-robust-hierarchical-median/v1");
  assert.equal(material.confirmation.intervalStartLocalDate, "2027-09-01");
  assert.equal(material.confirmation.intervalEndExclusiveLocalDate, "2028-09-01");
  assert.equal(material.cohort.descriptor.historicalMemberRootSha256, HASH_C);
  assert.equal(material.cohort.descriptor.inputManifestSha256, HASH_D);
});

// keep future daily work unopened until the authenticated registration terminal
test("daily terminal material refuses value access before the registered terminal clock", async () => {
  const epochWitness = futureOnlyEpochWitness();
  const material = buildAdjustmentPostFitRegistrationMaterial({
    artifactSha256: HASH_A,
    candidateSha256: HASH_B,
    cutoffAt: "2027-08-01T00:00:00.000Z",
    dueMonth: "2027-08",
    epochWitness,
    family: "wind",
    historicalMemberRootSha256: HASH_C,
    inputManifestSha256: HASH_D,
    predecessorRegistrationSha256: null,
    predecessorTerminalAt: null,
    requestedAt: "2027-08-02T12:00:00.000Z",
    sourceIdentitySha256: HASH_E,
  });
  let opened = 0;
  const result = await prepareAdjustmentMaintenanceDailyTerminalMaterial({
    archive: {},
    catalogInputManifestSha256: HASH_F,
    clockAt: "2027-09-02T12:00:00.000Z",
    due: {
      dueKey: "daily/2027-09-02",
      family: null,
      mode: "daily",
      originalCutoffAt: "2027-09-03T07:00:00.000Z",
      scope: "daily",
    },
    epochWitness,
    history: {},
    journal: {
      readActiveConfirmation: async () => {
        opened += 1;
        return { registration: { registrationSha256: HASH_C } };
      },
    },
    lifecycleEntry: { activeRegistration: material.shadowRegistration },
    readHead: async () => HASH_A,
    sourceCommit: "a".repeat(40),
  });
  assert.equal(opened, 1);
  assert.equal(result.state, "pending");
  assert.equal(result.dailyEvaluationInput.lifecycle.state, "pending");
  assert.equal(result.dailyEvaluationInput.lifecycle.family, "wind");
});

test("terminal no-action receipts derive one noncircular immutable identity", () => {
  const built = buildAdjustmentTerminalNoActionReceipt({
    completedAt: "2028-02-09T09:11:00.000Z",
    disposition: "rejected",
    finalizedAt: "2028-02-09T09:10:00.000Z",
    fullMemberRootSha256: HASH_A,
    policyReportSha256: HASH_B,
    registrationSha256: HASH_C,
  });
  assert.deepEqual(validateAdjustmentTerminalNoActionReceipt(built.receipt), built.receipt);

  // refuse changes to every field in the identity preimage
  for (const [field, value] of [
    ["disposition", "support_failed"],
    ["policyReportSha256", HASH_D],
    ["registrationSha256", HASH_E],
  ]) {
    assert.throws(() => validateAdjustmentTerminalNoActionReceipt({
      ...built.receipt,
      [field]: value,
    }), /differs/u);
  }
  assert.throws(() => validateAdjustmentTerminalNoActionReceipt({
    ...built.receipt,
    actionSha256: HASH_F,
  }), /differs/u);
});

// derive exact proof classes only from archived terminal graph bindings
test("terminal input classes retain genuine semantic member identities", () => {
  const classes = buildAdjustmentMaintenanceInputClassMembers({
    artifactSha256: HASH_A,
    candidateSha256: HASH_B,
    family: "wind",
    rainGateInputMemberSha256s: [],
    terminalGraph: {
      bindings: [{
        actualBestMatch: {
          memberSha256: HASH_C,
          payloadIdentitySha256: HASH_D,
          receiptSha256: HASH_E,
        },
        capsule: {
          comparatorMemberSha256: HASH_D,
          payloadIdentitySha256: HASH_E,
          predictionBodySha256: HASH_F,
          receiptSha256: HASH_A,
          sourceProjectionSha256: HASH_B,
        },
        key: "capture/2028-01-01T00:35:00.000Z/1/windSpeedMps",
        nativeSource: {
          memberSha256: HASH_C,
          payloadIdentitySha256: HASH_D,
          receiptSha256: HASH_E,
        },
        target: {
          sourceMemberSha256s: [HASH_A, HASH_C],
          targetMemberSha256: HASH_E,
        },
      }],
    },
  });
  assert.deepEqual(classes.candidate, [HASH_B]);
  assert.deepEqual(classes.artifact, [HASH_A]);
  assert.deepEqual(classes.actual_best_match, [HASH_C]);
  assert.deepEqual(classes.native_source, [HASH_C]);
  assert.deepEqual(classes.target_revision, [HASH_A, HASH_C]);
  assert.deepEqual(classes.rain_gate_input, []);
  assert.throws(() => buildAdjustmentMaintenanceInputClassMembers({
    artifactSha256: HASH_A,
    candidateSha256: HASH_B,
    family: "wind",
    rainGateInputMemberSha256s: [HASH_F],
    terminalGraph: { bindings: [] },
  }), /rain gate members/u);
});

test("terminal graph parts retain all four-cycle bindings below archive member bounds", () => {
  const bindings = Array.from({ length: 4_100 },
    // model one deterministic preregistered population larger than one part
    (_, index) => ({ key: `capture/2028-01-01T00:35:00.000Z/${String(index).padStart(4, "0")}/rain` }));
  const unsigned = {
    bindings,
    contractVersion: "adjustment-maintenance-confirmation-unsupported-terminal-graph/v1",
    derivedTargetMemberSha256s: [HASH_A],
    graphManifestSha256s: [HASH_B],
    missingKeySetSha256: HASH_C,
  };
  const terminalGraph = {
    ...unsigned,
    terminalGraphSha256: adjustmentSha256(canonicalJsonBytes(unsigned)),
  };
  const built = buildAdjustmentMaintenanceTerminalGraphParts({
    family: "rain",
    terminalGraph,
  });
  assert.equal(built.parts.length, 2);
  assert.deepEqual(built.parts.flatMap(
    // restore the exact original order from bounded part documents
    (part) => part.document.bindings,
  ), bindings);
  assert.ok(built.parts.every((part) => part.bytes.length <= 8 * 1_024 * 1_024));
  assert.deepEqual(built.manifest.partSha256s,
    built.parts.map((part) => part.partSha256));
  const { terminalGraphManifestSha256: _identity, ...manifestUnsigned } = built.manifest;
  assert.equal(built.manifest.terminalGraphManifestSha256,
    adjustmentSha256(canonicalJsonBytes(manifestUnsigned)));
  assert.equal(built.manifest.missingKeySetSha256, HASH_C);
  const chunkBytes = canonicalJsonBytes({
    contractVersion: "adjustment-maintenance-evaluation-chunk/v3",
    expectedKeys: [],
    missingKeys: [],
    records: [],
  });
  const policy = buildAdjustmentUnsupportedPolicyReport({
    family: "rain",
    missingClassNames: [],
    missingKeyCount: 1,
    missingKeySetSha256: HASH_C,
    registrationSha256: HASH_D,
    targetCutoffAt: "2028-02-09T08:00:00.000Z",
    unsupportedReason: "permanent_capture_gap",
  });
  const actionIdentitySha256 = adjustmentSha256(canonicalJsonBytes({
    contractVersion: "adjustment-terminal-no-action-identity/v3",
    disposition: "support_failed",
    policyReportSha256: policy.policyReportSha256,
    registrationSha256: HASH_D,
  }));
  const evidence = buildAdjustmentMaintenanceTerminalEvidenceMembers({
    actionIdentitySha256,
    assembly: {
      chunks: [{
        metadata: { partSha256: adjustmentSha256(chunkBytes) },
        payloadBase64: chunkBytes.toString("base64"),
      }],
      derivedTargets: [],
      family: "rain",
      graphManifestSha256s: [HASH_B],
      terminalGraph,
    },
    candidateGraphSha256: HASH_E,
    confirmationRegistration: {
      contractVersion: "forecast-adjustment-lifecycle-ledger/v2",
      registrationSha256: HASH_F,
    },
    fullManifest: {
      contractVersion: "adjustment-confirmation-member/v3",
      fullMemberRootSha256: HASH_A,
    },
    localBurn: { accessSha256: HASH_B },
    nativeAccess: { accessSha256: HASH_C },
    planGraphManifestSha256: HASH_F,
    policy,
    registrationSha256: HASH_D,
    shadowRegistration: { registrationSha256: HASH_D },
  });
  assert.equal(evidence.dataMembers.length, 3);
  assert.equal(evidence.finalMembers[0].identitySha256,
    built.manifest.terminalGraphManifestSha256);
  assert.ok(evidence.finalMembers.some((member) =>
    member.identitySha256 === actionIdentitySha256 &&
    member.kind === "adjustment-terminal-no-action-identity/v3"));
});

// retain the genuine promotion policy, candidate report and action in one graph
test("qualified terminal evidence binds the immutable promotion action", () => {
  const access = accessRequest();
  const shadowRegistration = buildAdjustmentRollingShadowRegistration({
    artifactSha256: HASH_F,
    candidateSha256: access.shadowRegistration.candidateSha256,
    cohortSha256: HASH_E,
    epochAt: "2026-10-10T08:00:00.000Z",
    epochWitnessSha256: HASH_D,
    family: "temperature",
    fitMonth: "2027-01",
    policySha256: HASH_C,
    predecessorRegistrationSha256: null,
    predecessorTerminalAt: null,
    requestedAt: "2027-01-02T08:00:00.000Z",
    reservedKeySha256: HASH_B,
    sourceSha256: HASH_A,
  }).registration;
  const policyBytes = canonicalJsonBytes({
    contractVersion: "temperature-maintenance-policy/v2",
  });
  const policyReportSha256 = adjustmentSha256(policyBytes);
  const fullManifest = {
    contractVersion: "adjustment-confirmation-member/v3",
    fullMemberRootSha256: HASH_A,
  };
  const terminalUnsigned = {
    bindings: [],
    contractVersion: "adjustment-maintenance-confirmation-terminal-graph/v1",
    derivedTargetMemberSha256s: [],
    graphManifestSha256s: [],
  };
  const terminalGraph = {
    ...terminalUnsigned,
    terminalGraphSha256: adjustmentSha256(canonicalJsonBytes(terminalUnsigned)),
  };
  const chunkBytes = canonicalJsonBytes({
    contractVersion: "adjustment-maintenance-evaluation-chunk/v3",
    expectedKeys: [],
    missingKeys: [],
    records: [],
  });
  const candidateReportBytes = canonicalJsonBytes({
    candidateSha256: shadowRegistration.candidateSha256,
    confirmationRegistrationSha256: access.confirmationRegistration.registrationSha256,
    contractVersion: "adjustment-maintenance-candidate-report/v1",
    evaluationRowsSha256: HASH_B,
    family: "temperature",
    fullMemberRootSha256: HASH_A,
    policyReportSha256,
    registrationSha256: shadowRegistration.registrationSha256,
    targetCutoffAt: shadowRegistration.targetCutoffAt,
  });
  const action = {
    actionKind: "promote",
    candidateGraphSha256: HASH_C,
    candidateSha256: shadowRegistration.candidateSha256,
    contractVersion: "forecast-adjustment-model-action/v1",
    createdAt: "2028-02-09T09:10:00.000Z",
    expectedInstalledReceiptSha256: null,
    expectedSettingsSha256: HASH_D,
    expectedSourceCommit: "a".repeat(40),
    expectedSourceRelease: "2028.02.08-1",
    family: "temperature",
    fencingToken: "1",
    fullMemberRootSha256: HASH_A,
    lifecycleHeadSha256: HASH_E,
    policyDecision: "qualified",
    policyReportSha256,
    predecessorActionSha256: null,
    reason: "qualified_candidate",
    reportCreatedAt: "2028-02-09T09:10:00.000Z",
    siteKey: "ballydidean",
    validThrough: "2028-02-16T09:10:00.000Z",
  };
  const evidence = buildAdjustmentQualifiedTerminalEvidenceMembers({
    action,
    assembly: {
      chunks: [{
        metadata: { partSha256: adjustmentSha256(chunkBytes) },
        payloadBase64: chunkBytes.toString("base64"),
      }],
      derivedTargets: [],
      family: "temperature",
      graphManifestSha256s: [],
      registrationSha256: access.confirmationRegistration.registrationSha256,
      state: "burned_complete",
      terminalGraph,
    },
    candidateGraphSha256: HASH_C,
    candidateReportBytes,
    confirmationRegistration: access.confirmationRegistration,
    fullManifest,
    localBurn: access.localBurn,
    nativeAccess: { accessSha256: HASH_F },
    planGraphManifestSha256: HASH_B,
    policyBytes,
    shadowRegistration,
  });
  assert.ok(evidence.finalMembers.some((member) =>
    member.kind === "forecast-adjustment-model-action/v1" &&
    member.payload.equals(canonicalJsonBytes(action))));
  assert.ok(evidence.finalMembers.some((member) =>
    member.identitySha256 === policyReportSha256));
  const noActionIdentity = {
    contractVersion: "adjustment-terminal-no-action-identity/v3",
    disposition: "rejected",
    policyReportSha256,
    registrationSha256: shadowRegistration.registrationSha256,
  };
  const noActionEvidence = buildAdjustmentQualifiedTerminalEvidenceMembers({
    action: noActionIdentity,
    assembly: {
      chunks: [{
        metadata: { partSha256: adjustmentSha256(chunkBytes) },
        payloadBase64: chunkBytes.toString("base64"),
      }],
      derivedTargets: [],
      family: "temperature",
      graphManifestSha256s: [],
      registrationSha256: access.confirmationRegistration.registrationSha256,
      state: "burned_complete",
      terminalGraph,
    },
    candidateGraphSha256: HASH_C,
    candidateReportBytes,
    confirmationRegistration: access.confirmationRegistration,
    fullManifest,
    localBurn: access.localBurn,
    nativeAccess: { accessSha256: HASH_F },
    planGraphManifestSha256: HASH_B,
    policyBytes,
    shadowRegistration,
  });
  assert.ok(noActionEvidence.finalMembers.some((member) =>
    member.identitySha256 === adjustmentSha256(canonicalJsonBytes(noActionIdentity)) &&
    member.kind === noActionIdentity.contractVersion));
});

// reconcile one passing member through release and owner retirement
test("qualified daily completion applies and retires one promoted member", async () => {
  const access = accessRequest();
  const shadowRegistration = buildAdjustmentRollingShadowRegistration({
    artifactSha256: HASH_F,
    candidateSha256: access.shadowRegistration.candidateSha256,
    cohortSha256: HASH_E,
    epochAt: "2026-10-10T08:00:00.000Z",
    epochWitnessSha256: HASH_D,
    family: "temperature",
    fitMonth: "2027-01",
    policySha256: HASH_C,
    predecessorRegistrationSha256: null,
    predecessorTerminalAt: null,
    requestedAt: "2027-01-02T08:00:00.000Z",
    reservedKeySha256: access.confirmationRegistration.reservedKeySha256,
    sourceSha256: HASH_A,
  }).registration;
  const policyBytes = canonicalJsonBytes({
    contractVersion: "temperature-maintenance-policy/v2",
  });
  const policyReportSha256 = adjustmentSha256(policyBytes);
  const fullManifest = {
    contractVersion: "adjustment-confirmation-member/v3",
    fullMemberRootSha256: HASH_A,
  };
  const binding = {
    actualBestMatch: {
      memberSha256: HASH_A,
      payloadIdentitySha256: HASH_B,
      receiptSha256: HASH_C,
    },
    capsule: {
      comparatorMemberSha256: HASH_B,
      payloadIdentitySha256: HASH_C,
      predictionBodySha256: HASH_D,
      receiptSha256: HASH_E,
      sourceProjectionSha256: HASH_F,
    },
    key: "capture/2028-01-01T00:35:00.000Z/1/temperature",
    nativeSource: {
      memberSha256: HASH_C,
      payloadIdentitySha256: HASH_D,
      receiptSha256: HASH_E,
    },
    target: {
      sourceMemberSha256s: [HASH_D],
      targetMemberSha256: HASH_E,
    },
  };
  const terminalUnsigned = {
    bindings: [binding],
    contractVersion: "adjustment-maintenance-confirmation-terminal-graph/v1",
    derivedTargetMemberSha256s: [],
    graphManifestSha256s: [HASH_F],
  };
  const terminalGraph = {
    ...terminalUnsigned,
    terminalGraphSha256: adjustmentSha256(canonicalJsonBytes(terminalUnsigned)),
  };
  const chunkBytes = canonicalJsonBytes({
    contractVersion: "adjustment-maintenance-evaluation-chunk/v3",
    expectedKeys: [],
    missingKeys: [],
    records: [],
  });
  const assembly = {
    chunks: [{
      metadata: { partSha256: adjustmentSha256(chunkBytes) },
      payloadBase64: chunkBytes.toString("base64"),
    }],
    derivedTargets: [],
    family: "temperature",
    graphManifestSha256s: [HASH_F],
    registrationSha256: access.confirmationRegistration.registrationSha256,
    state: "burned_complete",
    terminalGraph,
  };
  const candidateReportBytes = canonicalJsonBytes({
    candidateSha256: shadowRegistration.candidateSha256,
    confirmationRegistrationSha256: access.confirmationRegistration.registrationSha256,
    contractVersion: "adjustment-maintenance-candidate-report/v1",
    evaluationRowsSha256: HASH_B,
    family: "temperature",
    fullMemberRootSha256: fullManifest.fullMemberRootSha256,
    policyReportSha256,
    registrationSha256: shadowRegistration.registrationSha256,
    targetCutoffAt: shadowRegistration.targetCutoffAt,
  });
  const nativeUnsigned = {
    accessedAt: "2028-02-09T09:00:00.000Z",
    eligiblePredictionSetSha256: access.archive.eligiblePredictionSetSha256,
    expectedKeySetSha256: access.localBurn.expectedKeySetSha256,
    gateManifestSha256: access.confirmationRegistration.gateManifestSha256,
    journalHeadSha256: access.journalHeadSha256,
    maintenanceAnchorSha256: HASH_A,
    metadataRootSha256: access.metadata.rootSha256,
    registrationSha256: shadowRegistration.registrationSha256,
    revisionCatalogWatermarkSha256: access.localBurn.revisionCatalogWatermarkSha256,
    targetComparatorSnapshotRootSha256:
      access.localBurn.targetComparatorSnapshotRootSha256,
    targetCutoffAt: access.localBurn.targetCutoffAt,
  };
  const nativeAccess = {
    accessSha256: adjustmentSha256(Buffer.from([
      "adjustment-confirmation-access/v2",
      nativeUnsigned.registrationSha256,
      nativeUnsigned.journalHeadSha256,
      nativeUnsigned.maintenanceAnchorSha256,
      nativeUnsigned.gateManifestSha256,
      nativeUnsigned.eligiblePredictionSetSha256,
      nativeUnsigned.expectedKeySetSha256,
      nativeUnsigned.metadataRootSha256,
      nativeUnsigned.targetComparatorSnapshotRootSha256,
      nativeUnsigned.revisionCatalogWatermarkSha256,
      nativeUnsigned.targetCutoffAt,
    ].join("\n"))),
    ...nativeUnsigned,
  };
  const calls = [];
  let actionSha256 = null;
  let retainedAuthority = null;
  let retainedLifecycle = null;
  let retainedTransaction = null;
  let rootStatus = null;
  let publicationAttempts = 0;
  let releaseAttempts = 0;
  let terminalAttempts = 0;
  let anchorInstallAttempts = 0;
  const installedAnchorSha256s = [];
  const installedSealSha256s = [];
  let tombstone = null;
  const journal = {
    acknowledgeConfirmationAction: async () => calls.push("acknowledge"),
    acquireLease: async () => ({ fencingToken: "1" }),
    continueConfirmationActionLease: async () => ({
      dueKey: `confirmation/temperature/${shadowRegistration.candidateSha256}`,
      fencingToken: "1",
      live: true,
      runId: `release-promote-${shadowRegistration.candidateSha256.slice(0, 32)}`,
    }),
    readConfirmationByCandidate: async () => retainedLifecycle,
    readQualifiedModelReleaseTransaction: async () => retainedTransaction,
    readQualifiedTerminalAuthority: async () => retainedAuthority,
    readConfirmationActionLease: async () => ({
      dueKey: `confirmation/temperature/${shadowRegistration.candidateSha256}`,
      fencingToken: "1",
      live: true,
      runId: `release-promote-${shadowRegistration.candidateSha256.slice(0, 32)}`,
    }),
    readLatestRevisionCustodyAcknowledgement: async () => access.acknowledgement,
    readOwnerTerminalTombstone: async () => null,
    recordConfirmationActionApplied: async () => calls.push("applied"),
    recordConfirmationActionVerified: async () => calls.push("verified"),
    recordDueTerminalOutcome: async ({ outcome }) => outcome,
    recordDueTerminalRetirement: async ({ kind, request }) => ({
      kind,
      request,
      requestSha256: adjustmentSha256(canonicalJsonBytes(request)),
    }),
    completeDueTerminalRetirement: async () => undefined,
    recordOwnerTerminalTombstone: async (input) => {
      tombstone = input.tombstone;
    },
    recordQualifiedTerminalAuthority: async (input) => {
      actionSha256 = adjustmentSha256(canonicalJsonBytes(input.action));
      const localResult = {
        ...access.confirmationRegistration,
        accessState: "opened",
        actionIdentitySha256: actionSha256,
        actionState: "action_pending",
        candidateReportSha256: input.candidateReportSha256,
        disposition: "promoted",
        fullMemberRootSha256: fullManifest.fullMemberRootSha256,
        nextConfirmationEligibleAt: input.nextConfirmationEligibleAt,
      };
      retainedAuthority = {
        action: structuredClone(input.action),
        anchor: structuredClone(input.anchor),
        current: structuredClone(input.current),
        family: input.family,
        finalizationProof: structuredClone(input.finalizationProof),
        registrationSha256: input.registrationSha256,
        seal: structuredClone(input.seal),
      };
      retainedLifecycle = {
        access: null,
        chunks: [],
        manifest: fullManifest,
        registration: access.confirmationRegistration,
        result: localResult,
      };
      calls.push("authority");
      return { authority: retainedAuthority, localResult };
    },
    releaseLease: async () => {
      releaseAttempts += 1;

      // preserve the original fence while its action remains unreconciled
      if (releaseAttempts === 1) {
        calls.push("release_refused");
        throw new Error("confirmation action reconciliation required");
      }
      calls.push("release");
    },
    status: async () => ({ headSha256: HASH_E }),
  };
  const qualifiedInput = {
    archive: {},
    dailyMaterial: {
      assembly,
      candidate: {
        candidateBytes: Buffer.from("candidate\n"),
        candidateGraphSha256: HASH_C,
        parityReceiptBytes: canonicalJsonBytes({ contractVersion: "fixture/v1" }),
      },
      confirmationRegistration: access.confirmationRegistration,
      epochWitness: futureOnlyEpochWitness(),
      fullManifest,
      localBurn: access.localBurn,
      nativeAccess,
      planGraphManifestSha256: HASH_B,
      shadowRegistration,
      sourceCommit: access.sourceCommit,
    },
    due: {
      dueKey: "daily/2028-02-09",
      family: null,
      mode: "daily",
      originalCutoffAt: "2028-02-10T08:00:00.000Z",
      scope: "daily",
    },
    epochWitness: futureOnlyEpochWitness(),
    evaluation: {
      candidateReportBase64: candidateReportBytes.toString("base64"),
      candidateReportSha256: adjustmentSha256(candidateReportBytes),
      family: "temperature",
      fullMemberRootSha256: fullManifest.fullMemberRootSha256,
      policy: { action: "promote", state: "pass" },
      policyBytesBase64: policyBytes.toString("base64"),
      policyReportSha256,
      semanticInputSha256: HASH_F,
      state: "evaluated",
    },
    journal,
    readHead: async () => HASH_A,
  };
  const qualifiedOptions = {
    applyRelease: async (request) => {
      assert.deepEqual(request, retainedTransaction.releaseRequest);
      rootStatus = {
        actionSha256: retainedTransaction.target.actionSha256,
        compensatingRelease: retainedTransaction.compensation.releaseTag,
        compensationState: "absent",
        contractVersion: "adjustment-family-release-status/v1",
        family: "temperature",
        fencingToken: "1",
        outcome: "active",
        state: "acknowledged",
        targetRelease: retainedTransaction.target.releaseTag,
      };
      return rootStatus;
    },
    archiveEvidence: async () => ({ manifestObjectSha256: HASH_D }),
    archiveTombstone: async () => undefined,
    clock: () => new Date("2028-02-09T09:12:00.000Z"),
    fetchCurrent: async () => ({
      activeInstalledReceiptSha256: null,
      catalogSha256: HASH_C,
      commit: access.sourceCommit,
      controlInstalledReceiptSha256: null,
      contractVersion: "adjustment-family-release-current-status/v1",
      family: "temperature",
      release: "2028.02.08-1",
      settingsSha256: HASH_D,
      shadowInstalledReceiptSha256: null,
      sourceServerImageDigest: `sha256:${HASH_E}`,
    }),
    finalizeAnchor: async () => undefined,
    installAnchor: async ({ anchor }) => {
      anchorInstallAttempts += 1;
      installedAnchorSha256s.push(adjustmentSha256(canonicalJsonBytes(anchor)));

      // interrupt once after the exact successor seal was installed
      if (anchorInstallAttempts === 1) {
        throw new Error("anchor install interrupted");
      }
    },
    installSeal: async ({ seal }) => {
      installedSealSha256s.push(adjustmentSha256(canonicalJsonBytes(seal)));
    },
    fetchReleaseStatus: async () => rootStatus,
    publishRelease: async ({ action, current, finalization }) => {
      publicationAttempts += 1;

      // interrupt before the immutable pair transaction is retained
      if (publicationAttempts === 1) {
        throw new Error("release pair publication interrupted");
      }
      const targetActionSha256 = adjustmentSha256(canonicalJsonBytes(action));
      const target = {
        actionSha256: targetActionSha256,
        commitSha: "2".repeat(40),
        releaseTag: "2028.02.09-1",
      };
      const compensationAction = {
        ...action,
        actionKind: "compensate_raw",
        candidateGraphSha256: null,
        candidateSha256: null,
        expectedSourceCommit: target.commitSha,
        expectedSourceRelease: target.releaseTag,
        predecessorActionSha256: targetActionSha256,
        reason: "invalid_incumbent",
      };
      const compensation = {
        actionSha256: adjustmentSha256(canonicalJsonBytes(compensationAction)),
        commitSha: "3".repeat(40),
        releaseTag: "2028.02.09-2",
      };
      rootStatus = {
        actionSha256: targetActionSha256,
        contractVersion: "adjustment-family-release-status/v1",
        state: "absent",
      };
      retainedTransaction = {
        compensation,
        compensationAction,
        family: "temperature",
        finalizationProof: structuredClone(finalization.proof),
        registrationSha256: access.confirmationRegistration.registrationSha256,
        releaseRequest: {
          actionSha256: targetActionSha256,
          compensatingRelease: compensation.releaseTag,
          expectedCurrentRelease: current.release,
          expectedSettingsSha256: current.settingsSha256,
          expectedSourceRelease: current.release,
          family: "temperature",
          fencingToken: "1",
          reportSha256: policyReportSha256,
          targetRelease: target.releaseTag,
        },
        target,
        targetAction: structuredClone(action),
      };
      return {
        compensationActionSha256: compensation.actionSha256,
        compensationKind: "compensate_raw",
        pair: { compensation, target },
        status: rootStatus,
      };
    },
    readAnchor: async () => null,
    readSeal: async () => null,
    recordTerminal: async ({ request }) => {
      terminalAttempts += 1;

      // interrupt once after the acknowledged root mutation
      if (terminalAttempts === 1) {
        throw new Error("owner record interrupted");
      }
      return { terminalRecord: {
        accessSha256: request.nativeAccess.accessSha256,
        actionCompletedAt: request.finalizationProof.finalizedAt,
        actionDisposition: "promoted_verified",
        actionSha256: request.actionReceipt.actionSha256,
        candidateSha256: request.shadowRegistration.candidateSha256,
        contractVersion: "adjustment-shadow-terminal-record/v3",
        family: "temperature",
        finalizedMetadataRootSha256: request.nativeAccess.metadataRootSha256,
        finalizedPredictionCount: 2,
        maintenanceAnchorSha256: request.finalizationProof.transferredAnchorSha256,
        metadataGeneration: 1,
        recordedAt: "2028-02-09T09:12:00.000Z",
        reconciliationSha256: HASH_B,
        registrationSha256: request.shadowRegistration.registrationSha256,
        reservedKeySha256: request.shadowRegistration.reservedKeySha256,
        sourceSha256: request.shadowRegistration.sourceSha256,
        terminalMemberSha256: request.finalizationProof.fullMemberRootSha256,
        terminalResultSha256: HASH_C,
      } };
    },
    retireTerminal: async ({ request }) => {
      assert.deepEqual(request.terminalTombstone, tombstone);
      return { state: "retired" };
    },
  };
  await assert.rejects(
    completeAdjustmentQualifiedDailyTerminal(qualifiedInput, qualifiedOptions),
    /confirmation action reconciliation required/u,
  );
  await assert.rejects(
    completeAdjustmentQualifiedDailyTerminal(qualifiedInput, {
      ...qualifiedOptions,
      fetchCurrent: async () => {
        throw new Error("changed live source must not be read");
      },
    }),
    /release pair publication interrupted/u,
  );
  await assert.rejects(
    completeAdjustmentQualifiedDailyTerminal(qualifiedInput, {
      ...qualifiedOptions,
      fetchCurrent: async () => {
        throw new Error("changed live source must not be read");
      },
    }),
    /qualified family release is not reconciled/u,
  );
  await assert.rejects(
    completeAdjustmentQualifiedDailyTerminal(qualifiedInput, {
      ...qualifiedOptions,
      fetchCurrent: async () => {
        throw new Error("changed live source must not be read");
      },
    }),
    /owner record interrupted/u,
  );
  const result = await completeAdjustmentQualifiedDailyTerminal(qualifiedInput, {
    ...qualifiedOptions,
    fetchCurrent: async () => {
      throw new Error("changed live source must not be read");
    },
  });
  assert.equal(result.action.actionSha256, actionSha256);
  assert.equal(publicationAttempts, 2);
  assert.equal(releaseAttempts, 2);
  assert.equal(terminalAttempts, 2);
  assert.equal(new Set(installedSealSha256s).size, 1);
  assert.equal(new Set(installedAnchorSha256s).size, 1);
  assert.deepEqual(calls, [
    "authority", "release_refused", "applied", "verified", "acknowledge",
    "applied", "verified", "acknowledge", "release",
  ]);

  // retire an authenticated pre-apply operator-off result without model mutation
  const operatorCalls = [];
  let operatorTerminalAttempts = 0;
  const operatorStatus = {
    actionSha256: retainedTransaction.target.actionSha256,
    compensatingRelease: retainedTransaction.compensation.releaseTag,
    compensationState: "absent",
    contractVersion: "adjustment-family-release-status/v1",
    family: "temperature",
    fencingToken: "1",
    outcome: "operator_off_unapplied",
    state: "operator_off_unapplied",
    targetRelease: retainedTransaction.target.releaseTag,
  };
  const operatorJournal = {
    ...journal,
    acknowledgeConfirmationAction: async () => {
      throw new Error("operator-off result must not acknowledge an apply");
    },
    continueConfirmationActionLease: async () => ({
      dueKey: `confirmation/temperature/${shadowRegistration.candidateSha256}`,
      fencingToken: "1",
      live: false,
      runId: `release-promote-${shadowRegistration.candidateSha256.slice(0, 32)}`,
    }),
    recordConfirmationActionApplied: async () => {
      throw new Error("operator-off result must not record apply");
    },
    recordConfirmationActionVerified: async () => {
      throw new Error("operator-off result must not record verification");
    },
    recordConfirmationCompensationResult: async (input) => {
      assert.equal(input.outcome, "unapplied");
      operatorCalls.push("unapplied");
    },
    recordDueTerminalOutcome: async ({ outcome }) => {
      operatorCalls.push("outcome");
      assert.equal(outcome.servingChanged, false);
      return outcome;
    },
    recordDueTerminalRetirement: async ({ kind, request }) => {
      operatorCalls.push("retain-retirement");
      return {
        kind,
        request,
        requestSha256: adjustmentSha256(canonicalJsonBytes(request)),
      };
    },
    completeDueTerminalRetirement: async () => operatorCalls.push("complete-retirement"),
  };
  const operatorOptions = {
    ...qualifiedOptions,
    fetchCurrent: async () => {
      throw new Error("operator-off recovery must not read changed live source");
    },
    fetchReleaseStatus: async () => operatorStatus,
    recordTerminal: async ({ request }) => {
      operatorCalls.push("record-terminal");
      operatorTerminalAttempts += 1;
      assert.equal(request.actionReceipt.state, "operator_off_unapplied");

      // interrupt once after the local unapplied result but before owner retirement
      if (operatorTerminalAttempts === 1) {
        throw new Error("operator-off owner record interrupted");
      }
      return { terminalRecord: {
        accessSha256: request.nativeAccess.accessSha256,
        actionCompletedAt: request.finalizationProof.finalizedAt,
        actionDisposition: "promoted_operator_off_unapplied",
        actionSha256: request.actionReceipt.actionSha256,
        candidateSha256: request.shadowRegistration.candidateSha256,
        contractVersion: "adjustment-shadow-terminal-record/v3",
        family: "temperature",
        finalizedMetadataRootSha256: request.nativeAccess.metadataRootSha256,
        finalizedPredictionCount: 2,
        maintenanceAnchorSha256: request.finalizationProof.transferredAnchorSha256,
        metadataGeneration: 1,
        recordedAt: "2028-02-09T09:12:00.000Z",
        reconciliationSha256: HASH_C,
        registrationSha256: request.shadowRegistration.registrationSha256,
        reservedKeySha256: request.shadowRegistration.reservedKeySha256,
        sourceSha256: request.shadowRegistration.sourceSha256,
        terminalMemberSha256: request.finalizationProof.fullMemberRootSha256,
        terminalResultSha256: HASH_D,
      } };
    },
    retireTerminal: async () => operatorCalls.push("retire-terminal"),
  };
  await assert.rejects(completeAdjustmentQualifiedDailyTerminal({
    ...qualifiedInput,
    journal: operatorJournal,
  }, operatorOptions), /operator-off owner record interrupted/u);
  const operatorResult = await completeAdjustmentQualifiedDailyTerminal({
    ...qualifiedInput,
    journal: operatorJournal,
  }, operatorOptions);
  assert.equal(operatorResult.servingChanged, false);
  assert.deepEqual(operatorCalls, [
    "unapplied", "record-terminal", "unapplied", "record-terminal",
    "retain-retirement", "outcome",
    "retire-terminal", "complete-retirement",
  ]);

  // preserve serving bytes while retiring one fully evaluated rejection
  let noActionAuthority = null;
  let noActionResultAttempts = 0;
  let noActionTombstone = null;
  const noActionJournal = {
    readNoActionTerminalAuthority: async () => noActionAuthority,
    readLatestRevisionCustodyAcknowledgement: async () => access.acknowledgement,
    readOwnerTerminalTombstone: async () => null,
    recordConfirmationResult: async (input) => {
      noActionResultAttempts += 1;

      // interrupt once after F but before the local terminal result
      if (noActionResultAttempts === 1) {
        throw new Error("no-action result interrupted");
      }
      return {
        ...access.confirmationRegistration,
        accessState: "opened",
        actionIdentitySha256: null,
        actionState: "terminal_no_action",
        candidateReportSha256: input.candidateReportSha256,
        disposition: input.disposition,
        fullMemberRootSha256: fullManifest.fullMemberRootSha256,
        nextConfirmationEligibleAt: input.nextConfirmationEligibleAt,
      };
    },
    recordDueTerminalOutcome: async ({ outcome }) => outcome,
    recordDueTerminalRetirement: async ({ kind, request }) => ({
      kind,
      request,
      requestSha256: adjustmentSha256(canonicalJsonBytes(request)),
    }),
    completeDueTerminalRetirement: async () => undefined,
    recordNoActionTerminalAuthority: async (input) => {
      noActionAuthority = {
        actionIdentity: structuredClone(input.actionIdentity),
        anchor: structuredClone(input.anchor),
        current: structuredClone(input.current),
        disposition: input.disposition,
        family: input.family,
        finalizationProof: structuredClone(input.finalizationProof),
        registrationSha256: input.registrationSha256,
        seal: structuredClone(input.seal),
      };
    },
    recordOwnerTerminalTombstone: async (input) => {
      noActionTombstone = input.tombstone;
    },
    status: async () => ({ headSha256: HASH_E }),
  };
  const noActionInput = {
    archive: {},
    dailyMaterial: {
      assembly,
      candidate: { candidateGraphSha256: HASH_C },
      confirmationRegistration: access.confirmationRegistration,
      epochWitness: futureOnlyEpochWitness(),
      fullManifest,
      localBurn: access.localBurn,
      nativeAccess,
      planGraphManifestSha256: HASH_B,
      shadowRegistration,
      sourceCommit: access.sourceCommit,
    },
    disposition: "rejected",
    due: {
      dueKey: "daily/2028-02-09",
      family: null,
      mode: "daily",
      originalCutoffAt: "2028-02-10T08:00:00.000Z",
      scope: "daily",
    },
    epochWitness: futureOnlyEpochWitness(),
    evaluation: {
      candidateReportBase64: candidateReportBytes.toString("base64"),
      candidateReportSha256: adjustmentSha256(candidateReportBytes),
      family: "temperature",
      fullMemberRootSha256: fullManifest.fullMemberRootSha256,
      policy: { action: "retain", state: "fail" },
      policyBytesBase64: policyBytes.toString("base64"),
      policyReportSha256,
      semanticInputSha256: HASH_F,
      state: "evaluated",
    },
    journal: noActionJournal,
    readHead: async () => HASH_A,
  };
  const noActionOptions = {
    archiveEvidence: async () => ({ manifestObjectSha256: HASH_D }),
    archiveTombstone: async () => undefined,
    clock: () => new Date("2028-02-09T09:12:00.000Z"),
    fetchCurrent: async () => ({
      activeInstalledReceiptSha256: null,
      catalogSha256: HASH_C,
      commit: access.sourceCommit,
      controlInstalledReceiptSha256: null,
      contractVersion: "adjustment-family-release-current-status/v1",
      family: "temperature",
      release: "2028.02.08-1",
      settingsSha256: HASH_D,
      shadowInstalledReceiptSha256: null,
      sourceServerImageDigest: `sha256:${HASH_E}`,
    }),
    finalizeAnchor: async () => undefined,
    installAnchor: async () => undefined,
    installSeal: async () => undefined,
    readAnchor: async () => null,
    readSeal: async () => null,
    recordTerminal: async ({ request }) => ({
      terminalRecord: {
        accessSha256: request.nativeAccess.accessSha256,
        actionCompletedAt: request.actionReceipt.completedAt,
        actionDisposition: "rejected_no_action",
        actionSha256: request.actionReceipt.actionSha256,
        candidateSha256: request.shadowRegistration.candidateSha256,
        contractVersion: "adjustment-shadow-terminal-record/v3",
        family: "temperature",
        finalizedMetadataRootSha256: request.nativeAccess.metadataRootSha256,
        finalizedPredictionCount: 2,
        maintenanceAnchorSha256: request.finalizationProof.transferredAnchorSha256,
        metadataGeneration: 1,
        recordedAt: request.actionReceipt.completedAt,
        reconciliationSha256: HASH_B,
        registrationSha256: request.shadowRegistration.registrationSha256,
        reservedKeySha256: request.shadowRegistration.reservedKeySha256,
        sourceSha256: request.shadowRegistration.sourceSha256,
        terminalMemberSha256: request.finalizationProof.fullMemberRootSha256,
        terminalResultSha256: HASH_C,
      },
    }),
    retireTerminal: async ({ request }) => {
      assert.deepEqual(request.terminalTombstone, noActionTombstone);
      return { state: "retired" };
    },
  };
  await assert.rejects(
    completeAdjustmentNoActionDailyTerminal(noActionInput, noActionOptions),
    /no-action result interrupted/u,
  );
  const noAction = await completeAdjustmentNoActionDailyTerminal(noActionInput, {
    ...noActionOptions,
    fetchCurrent: async () => {
      throw new Error("changed no-action source must not be read");
    },
  });
  assert.equal(noActionResultAttempts, 2);
  assert.equal(noAction.action.receipt.disposition, "rejected");
  assert.equal(noAction.retired.state, "retired");
});

test("unsupported terminal proof closes only genuine missing evidence without promotion authority", async () => {
  const missingKeySetSha256 = adjustmentSha256(canonicalJsonBytes([
    "daily/2028-02-01/wind/00/speed/001",
  ]));
  const policy = buildAdjustmentUnsupportedPolicyReport({
    family: "wind",
    missingClassNames: ["target", "target_revision"],
    missingKeyCount: 1,
    missingKeySetSha256,
    registrationSha256: HASH_A,
    targetCutoffAt: "2028-02-09T08:00:00.000Z",
    unsupportedReason: "permanent_target_gap",
  });
  assert.deepEqual(validateAdjustmentUnsupportedPolicyReport(policy.report), policy.report);
  const action = buildAdjustmentTerminalNoActionReceipt({
    completedAt: "2028-02-09T09:12:00.000Z",
    disposition: "support_failed",
    finalizedAt: "2028-02-09T09:11:00.000Z",
    fullMemberRootSha256: HASH_B,
    policyReportSha256: policy.policyReportSha256,
    registrationSha256: HASH_A,
  });
  const inputClassMembers = {
    actual_best_match: [HASH_A],
    artifact: [HASH_B],
    candidate: [HASH_C],
    comparator: [HASH_D],
    native_source: [HASH_E],
    rain_gate_input: [HASH_F],
    shadow_body: [HASH_A],
    shadow_source: [HASH_B],
    target: [],
    target_revision: [],
  };
  const built = buildAdjustmentUnsupportedTerminalProof({
    actionSha256: action.receipt.actionSha256,
    archiveCommitOrdinal: "42",
    burnSha256: HASH_A,
    candidateReportSha256: policy.policyReportSha256,
    captureEpochWitnessSha256: HASH_B,
    confirmationAccessSha256: HASH_C,
    controlSha256: HASH_D,
    controlVersion: "14",
    custodyCheckpointSha256: HASH_E,
    dueKey: `confirmation/wind/${HASH_C}`,
    eligiblePredictionSetSha256: HASH_F,
    expectedKeySetSha256: HASH_A,
    family: "wind",
    finalizedAt: "2028-02-09T09:11:00.000Z",
    frontierSha256: HASH_B,
    fullGraphVerifiedAt: "2028-02-09T09:10:00.000Z",
    fullMemberRootSha256: HASH_C,
    graphManifestSha256: HASH_D,
    inputClassMembers,
    lifecycleLedgerRootSha256: HASH_E,
    missingClassNames: ["target", "target_revision"],
    missingKeyCount: 1,
    missingKeySetSha256,
    pageSha256: HASH_F,
    policyReportSha256: policy.policyReportSha256,
    predecessorProofSha256: null,
    registrationSha256: HASH_A,
    sequence: "0",
    sourceCommit: "a".repeat(40),
    unsupportedReason: "permanent_target_gap",
    workstationJournalHeadSha256: HASH_A,
  });
  assert.deepEqual(validateAdjustmentUnsupportedTerminalProof(built.proof), built.proof);
  assert.equal(built.proof.inputClasses.target.count, 0);
  assert.equal(built.proof.inputClasses.candidate.count, 1);

  // refuse relabelling unsupported evidence as a different disposition or missing class
  assert.throws(() => validateAdjustmentUnsupportedTerminalProof({
    ...built.proof,
    actionSha256: HASH_F,
  }), /identity differs/u);
  assert.throws(() => validateAdjustmentUnsupportedTerminalProof({
    ...built.proof,
    missingClassNames: ["target"],
  }), /identity differs/u);
  assert.throws(() => buildAdjustmentUnsupportedTerminalProof({
    ...{
      actionSha256: action.receipt.actionSha256,
      archiveCommitOrdinal: "42",
      burnSha256: HASH_A,
      candidateReportSha256: policy.policyReportSha256,
      captureEpochWitnessSha256: HASH_B,
      confirmationAccessSha256: HASH_C,
      controlSha256: HASH_D,
      controlVersion: "14",
      custodyCheckpointSha256: HASH_E,
      dueKey: `confirmation/wind/${HASH_C}`,
      eligiblePredictionSetSha256: HASH_F,
      expectedKeySetSha256: HASH_A,
      family: "wind",
      finalizedAt: "2028-02-09T09:11:00.000Z",
      frontierSha256: HASH_B,
      fullGraphVerifiedAt: "2028-02-09T09:10:00.000Z",
      fullMemberRootSha256: HASH_C,
      graphManifestSha256: HASH_D,
      lifecycleLedgerRootSha256: HASH_E,
      missingClassNames: ["target", "target_revision"],
      missingKeyCount: 1,
      missingKeySetSha256,
      pageSha256: HASH_F,
      policyReportSha256: policy.policyReportSha256,
      predecessorProofSha256: null,
      registrationSha256: HASH_A,
      sequence: "0",
      sourceCommit: "a".repeat(40),
      unsupportedReason: "permanent_target_gap",
      workstationJournalHeadSha256: HASH_A,
    },
    inputClassMembers: { ...inputClassMembers, candidate: [] },
  }), /identity differs/u);
  const rainRegistrationSha256 = HASH_D;
  const rainPolicy = buildAdjustmentUnsupportedPolicyReport({
    family: "rain",
    missingClassNames: [],
    missingKeyCount: 1,
    missingKeySetSha256,
    registrationSha256: rainRegistrationSha256,
    targetCutoffAt: "2028-02-09T08:00:00.000Z",
    unsupportedReason: "permanent_capture_gap",
  });
  const rainAction = buildAdjustmentTerminalNoActionReceipt({
    completedAt: "2028-02-09T09:12:00.000Z",
    disposition: "support_failed",
    finalizedAt: "2028-02-09T09:11:00.000Z",
    fullMemberRootSha256: HASH_B,
    policyReportSha256: rainPolicy.policyReportSha256,
    registrationSha256: rainRegistrationSha256,
  });
  const rainInputClasses = Object.fromEntries([
    "actual_best_match", "artifact", "candidate", "comparator", "native_source",
    "rain_gate_input", "shadow_body", "shadow_source", "target", "target_revision",
  ].map(
    // retain every genuine class while one preregistered rain key remains absent
    (name, index) => [name, [name === "candidate" ? HASH_C :
      [HASH_A, HASH_B, HASH_D, HASH_E, HASH_F][index % 5]]],
  ));
  const partialRain = buildAdjustmentUnsupportedTerminalProof({
    actionSha256: rainAction.receipt.actionSha256,
    archiveCommitOrdinal: "43",
    burnSha256: HASH_A,
    candidateReportSha256: rainPolicy.policyReportSha256,
    captureEpochWitnessSha256: HASH_B,
    confirmationAccessSha256: HASH_C,
    controlSha256: HASH_D,
    controlVersion: "14",
    custodyCheckpointSha256: HASH_E,
    dueKey: `confirmation/rain/${HASH_C}`,
    eligiblePredictionSetSha256: HASH_F,
    expectedKeySetSha256: HASH_A,
    family: "rain",
    finalizedAt: "2028-02-09T09:11:00.000Z",
    frontierSha256: HASH_B,
    fullGraphVerifiedAt: "2028-02-09T09:10:00.000Z",
    fullMemberRootSha256: HASH_C,
    graphManifestSha256: HASH_D,
    inputClassMembers: rainInputClasses,
    lifecycleLedgerRootSha256: HASH_E,
    missingClassNames: [],
    missingKeyCount: 1,
    missingKeySetSha256,
    pageSha256: HASH_F,
    policyReportSha256: rainPolicy.policyReportSha256,
    predecessorProofSha256: null,
    registrationSha256: rainRegistrationSha256,
    sequence: "0",
    sourceCommit: "a".repeat(40),
    unsupportedReason: "permanent_capture_gap",
    workstationJournalHeadSha256: HASH_A,
  });
  assert.deepEqual(validateAdjustmentUnsupportedTerminalProof(partialRain.proof),
    partialRain.proof);
  assert.deepEqual(partialRain.proof.missingClassNames, []);
  const proofBytes = canonicalJsonBytes(built.proof);
  let transferred = Buffer.alloc(0);
  const installSpawn = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), [
      "adjustment-unsupported-terminal-proof-install-v1", built.proofSha256,
    ]);
    const child = new EventEmitter();
    child.stdin = new PassThrough();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    child.stdin.on("data", (chunk) => {
      transferred = Buffer.concat([transferred, Buffer.from(chunk)]);
    });
    child.stdin.once("finish", () => {
      child.stdout.end(canonicalJsonBytes({
        contractVersion: "adjustment-maintenance-unsupported-terminal-proof-installation/v1",
        proofSha256: built.proofSha256,
        state: "installed",
      }));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  await installAdjustmentUnsupportedTerminalProof(
    { proof: built.proof },
    { spawnImpl: installSpawn },
  );
  assert.deepEqual(transferred, proofBytes);
  const current = { proof: built.proof, proofSha256: built.proofSha256 };
  const currentSpawn = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), [
      "adjustment-unsupported-terminal-proof-current-v1",
    ]);
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(canonicalJsonBytes(current));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(await fetchAdjustmentUnsupportedTerminalProofCurrent({
    spawnImpl: currentSpawn,
  }), current);
});

test("unsupported terminal artifacts bind the owner-opened eligible set with all classes present", () => {
  const epochWitness = futureOnlyEpochWitness();
  const missingKeySetSha256 = adjustmentSha256(canonicalJsonBytes([
    "capture/2028-01-01T00:35:00.000Z/1/rain",
  ]));
  const acknowledgementUnsigned = {
    acknowledgedAt: "2028-02-09T09:05:00.000Z",
    afterArchiveCommitOrdinal: "41",
    afterFrontierSha256: HASH_A,
    authority: "cold_custody_only",
    contractVersion: "adjustment-revision-custody-acknowledgement/v2",
    custodyCheckpointSha256: HASH_B,
    memberRootSha256: HASH_C,
    nextArchiveCommitOrdinal: "42",
    nextFrontierSha256: HASH_D,
    pageSha256: HASH_E,
    previousAcknowledgementSha256: HASH_F,
    previousPageSha256: HASH_A,
    retirementEntries: [{
      fileSha256: HASH_A,
      identitySha256: HASH_B,
      kind: "revision_commit_receipt",
    }, {
      fileSha256: HASH_B,
      identitySha256: HASH_C,
      kind: "revision_frontier_successor",
    }, {
      fileSha256: HASH_C,
      identitySha256: HASH_D,
      kind: "revision_projection",
    }, {
      fileSha256: HASH_D,
      identitySha256: HASH_E,
      kind: "revision_stage_receipt",
    }],
    startMemberSha256: HASH_F,
    startSha256: HASH_A,
    watermarkArchiveCommitOrdinal: "42",
    watermarkFrontierSha256: HASH_D,
  };
  const acknowledgement = {
    ...acknowledgementUnsigned,
    acknowledgementSha256: adjustmentSha256(canonicalJsonBytes(acknowledgementUnsigned)),
  };
  const terminalGraph = {
    bindings: [{
      actualBestMatch: {
        memberSha256: HASH_A,
        payloadIdentitySha256: HASH_B,
        receiptSha256: HASH_C,
      },
      capsule: {
        comparatorMemberSha256: HASH_B,
        payloadIdentitySha256: HASH_C,
        predictionBodySha256: HASH_D,
        receiptSha256: HASH_E,
        sourceProjectionSha256: HASH_F,
      },
      key: "capture/2028-01-01T00:35:00.000Z/1/rain",
      nativeSource: {
        memberSha256: HASH_C,
        payloadIdentitySha256: HASH_D,
        receiptSha256: HASH_E,
      },
      target: {
        sourceMemberSha256s: [HASH_D],
        targetMemberSha256: HASH_E,
      },
    }],
  };
  const assembly = {
    family: "rain",
    missingKeyCount: 1,
    missingKeySetSha256,
    registrationSha256: HASH_E,
    state: "burned_unsupported",
    terminalGraph,
  };
  const localBurn = {
    accessSha256: HASH_D,
    expectedKeySetSha256: HASH_A,
    registrationSha256: HASH_E,
    targetComparatorSnapshotRootSha256: HASH_E,
  };
  const fullManifestUnsigned = {
    accessSha256: localBurn.accessSha256,
    chunkCount: 24,
    chunks: Array.from({ length: 24 },
      // retain one deterministic logical placeholder per already-validated journal chunk
      (_, index) => ({ logicalChunkIndex: index })),
    contractVersion: "adjustment-confirmation-member/v3",
    eligiblePredictionSetSha256: HASH_C,
    expectedKeySetSha256: localBurn.expectedKeySetSha256,
    family: "rain",
    intervalEndExclusiveLocalDate: "2028-01-31",
    intervalStartLocalDate: "2027-03-03",
    missingKeySetSha256,
    registrationSha256: assembly.registrationSha256,
    revisionCatalogWatermarkSha256: HASH_F,
    targetComparatorSnapshotRootSha256: localBurn.targetComparatorSnapshotRootSha256,
    targetCutoffAt: "2028-02-07T08:00:00.000Z",
  };
  const fullManifest = {
    ...fullManifestUnsigned,
    fullMemberRootSha256: adjustmentSha256(canonicalJsonBytes(fullManifestUnsigned)),
  };
  const nativeAccess = {
    accessSha256: HASH_F,
    accessedAt: "2028-02-09T09:06:00.000Z",
    eligiblePredictionSetSha256: HASH_B,
    expectedKeySetSha256: localBurn.expectedKeySetSha256,
    registrationSha256: HASH_D,
    targetComparatorSnapshotRootSha256: localBurn.targetComparatorSnapshotRootSha256,
    targetCutoffAt: fullManifest.targetCutoffAt,
  };
  const built = buildAdjustmentUnsupportedTerminalArtifacts({
    acknowledgement,
    artifactSha256: HASH_B,
    assembly,
    candidateSha256: HASH_C,
    epochWitness,
    finalizedAt: "2028-02-09T09:11:00.000Z",
    fullGraphVerifiedAt: "2028-02-09T09:10:00.000Z",
    fullManifest,
    graphManifestSha256: HASH_A,
    journalHeadSha256: HASH_E,
    localBurn,
    nativeAccess,
    predecessor: null,
    rainGateInputMemberSha256s: [HASH_F],
    registrationSha256: nativeAccess.registrationSha256,
    sourceCommit: epochWitness.sourceCommit,
    targetCutoffAt: fullManifest.targetCutoffAt,
    unsupportedReason: "permanent_capture_gap",
  });
  assert.equal(built.proof.proof.eligiblePredictionSetSha256,
    nativeAccess.eligiblePredictionSetSha256);
  assert.notEqual(built.proof.proof.eligiblePredictionSetSha256,
    localBurn.targetComparatorSnapshotRootSha256);
  assert.deepEqual(built.proof.proof.missingClassNames, []);
  assert.equal(built.proof.proof.missingKeyCount, 1);
  assert.equal(built.action.receipt.disposition, "support_failed");

  // refuse a native access document from another blinded expected population
  assert.throws(() => buildAdjustmentUnsupportedTerminalArtifacts({
    acknowledgement,
    artifactSha256: HASH_B,
    assembly,
    candidateSha256: HASH_C,
    epochWitness,
    finalizedAt: "2028-02-09T09:11:00.000Z",
    fullGraphVerifiedAt: "2028-02-09T09:10:00.000Z",
    fullManifest,
    graphManifestSha256: HASH_A,
    journalHeadSha256: HASH_E,
    localBurn,
    nativeAccess: { ...nativeAccess, expectedKeySetSha256: HASH_F },
    predecessor: null,
    rainGateInputMemberSha256s: [HASH_F],
    registrationSha256: nativeAccess.registrationSha256,
    sourceCommit: epochWitness.sourceCommit,
    targetCutoffAt: fullManifest.targetCutoffAt,
    unsupportedReason: "permanent_capture_gap",
  }), /access binding differs/u);
});

test("unsupported daily completion installs proof and retires the exact owner tombstone", async () => {
  const access = accessRequest();
  const missingKeySetSha256 = adjustmentSha256(canonicalJsonBytes([
    "capture/2028-01-01T00:35:00.000Z/1/temperature",
  ]));
  const terminalGraphUnsigned = {
    bindings: [],
    contractVersion: "adjustment-maintenance-confirmation-unsupported-terminal-graph/v1",
    derivedTargetMemberSha256s: [],
    graphManifestSha256s: [],
    missingKeySetSha256,
  };
  const terminalGraph = {
    ...terminalGraphUnsigned,
    terminalGraphSha256: adjustmentSha256(canonicalJsonBytes(terminalGraphUnsigned)),
  };
  const chunkBytes = canonicalJsonBytes({
    contractVersion: "adjustment-maintenance-evaluation-chunk/v3",
    expectedKeys: ["capture/2028-01-01T00:35:00.000Z/1/temperature"],
    missingKeys: ["capture/2028-01-01T00:35:00.000Z/1/temperature"],
    records: [],
  });
  const assembly = {
    chunks: [{
      metadata: { partSha256: adjustmentSha256(chunkBytes) },
      payloadBase64: chunkBytes.toString("base64"),
    }],
    derivedTargets: [],
    family: "temperature",
    graphManifestSha256s: [],
    missingKeyCount: 1,
    missingKeySetSha256,
    registrationSha256: access.confirmationRegistration.registrationSha256,
    state: "burned_unsupported",
    terminalGraph,
  };
  const fullManifestUnsigned = {
    accessSha256: access.localBurn.accessSha256,
    chunkCount: 27,
    chunks: Array.from({ length: 27 },
      // retain the already-journaled logical window population
      (_, index) => ({ logicalChunkIndex: index })),
    contractVersion: "adjustment-confirmation-member/v3",
    eligiblePredictionSetSha256: HASH_A,
    expectedKeySetSha256: access.localBurn.expectedKeySetSha256,
    family: "temperature",
    intervalEndExclusiveLocalDate:
      access.confirmationRegistration.intervalEndExclusiveLocalDate,
    intervalStartLocalDate: access.confirmationRegistration.intervalStartLocalDate,
    missingKeySetSha256,
    registrationSha256: access.confirmationRegistration.registrationSha256,
    revisionCatalogWatermarkSha256: access.localBurn.revisionCatalogWatermarkSha256,
    targetComparatorSnapshotRootSha256:
      access.localBurn.targetComparatorSnapshotRootSha256,
    targetCutoffAt: access.localBurn.targetCutoffAt,
  };
  const fullManifest = {
    ...fullManifestUnsigned,
    fullMemberRootSha256: adjustmentSha256(canonicalJsonBytes(fullManifestUnsigned)),
  };
  const nativeUnsigned = {
    accessedAt: "2028-02-09T09:00:00.000Z",
    eligiblePredictionSetSha256: access.archive.eligiblePredictionSetSha256,
    expectedKeySetSha256: access.localBurn.expectedKeySetSha256,
    gateManifestSha256: access.confirmationRegistration.gateManifestSha256,
    journalHeadSha256: access.journalHeadSha256,
    maintenanceAnchorSha256: HASH_A,
    metadataRootSha256: access.metadata.rootSha256,
    registrationSha256: access.shadowRegistration.registrationSha256,
    revisionCatalogWatermarkSha256: access.localBurn.revisionCatalogWatermarkSha256,
    targetComparatorSnapshotRootSha256:
      access.localBurn.targetComparatorSnapshotRootSha256,
    targetCutoffAt: access.localBurn.targetCutoffAt,
  };
  const nativeAccess = {
    accessSha256: adjustmentSha256(Buffer.from([
      "adjustment-confirmation-access/v2",
      nativeUnsigned.registrationSha256,
      nativeUnsigned.journalHeadSha256,
      nativeUnsigned.maintenanceAnchorSha256,
      nativeUnsigned.gateManifestSha256,
      nativeUnsigned.eligiblePredictionSetSha256,
      nativeUnsigned.expectedKeySetSha256,
      nativeUnsigned.metadataRootSha256,
      nativeUnsigned.targetComparatorSnapshotRootSha256,
      nativeUnsigned.revisionCatalogWatermarkSha256,
      nativeUnsigned.targetCutoffAt,
    ].join("\n"))),
    ...nativeUnsigned,
  };
  const calls = [];
  let tombstone = null;
  const journal = {
    readLatestRevisionCustodyAcknowledgement: async () => access.acknowledgement,
    readOwnerTerminalTombstone: async () => null,
    recordConfirmationResult: async (input) => ({
      ...access.confirmationRegistration,
      accessState: "opened",
      actionIdentitySha256: input.actionIdentitySha256,
      actionState: "terminal_no_action",
      candidateReportSha256: input.candidateReportSha256,
      disposition: input.disposition,
      fullMemberRootSha256: fullManifest.fullMemberRootSha256,
      nextConfirmationEligibleAt: input.nextConfirmationEligibleAt,
    }),
    recordDueTerminalOutcome: async ({ outcome }) => outcome,
    recordDueTerminalRetirement: async ({ kind, request }) => ({
      kind,
      request,
      requestSha256: adjustmentSha256(canonicalJsonBytes(request)),
    }),
    completeDueTerminalRetirement: async () => undefined,
    recordOwnerTerminalTombstone: async (input) => {
      tombstone = input.tombstone;
      calls.push("journal_tombstone");
      return tombstone;
    },
    status: async () => ({ headSha256: HASH_F }),
  };
  const result = await completeAdjustmentUnsupportedDailyTerminal({
    archive: {},
    dailyMaterial: {
      assembly,
      candidate: { candidateGraphSha256: HASH_C },
      confirmationRegistration: access.confirmationRegistration,
      fullManifest,
      localBurn: access.localBurn,
      nativeAccess,
      planGraphManifestSha256: HASH_D,
      shadowRegistration: access.shadowRegistration,
      sourceCommit: access.sourceCommit,
    },
    due: {
      dueKey: "daily/2028-02-09",
      family: null,
      mode: "daily",
      originalCutoffAt: "2028-02-10T08:00:00.000Z",
      scope: "daily",
    },
    epochWitness: futureOnlyEpochWitness(),
    journal,
    readHead: async () => HASH_A,
    semanticInputSha256: HASH_F,
    unsupportedReason: "permanent_capture_gap",
  }, {
    archiveEvidence: async ({ evidence }) => {
      assert.ok(evidence.finalMembers.some((member) =>
        member.kind === "adjustment-terminal-no-action-identity/v3"));
      calls.push("archive_evidence");
      return { manifestObjectSha256: HASH_E };
    },
    archiveTombstone: async (input) => {
      assert.equal(input.kind, "adjustment-shadow-terminal-tombstone/v3");
      calls.push("archive_tombstone");
    },
    clock: () => new Date("2028-02-09T09:12:00.000Z"),
    installProof: async ({ proof }) => {
      assert.deepEqual(validateAdjustmentUnsupportedTerminalProof(proof), proof);
      calls.push("install_proof");
    },
    readProof: async () => null,
    recordTerminal: async ({ request }) => {
      calls.push("record_terminal");
      const proofSha256 = adjustmentSha256(canonicalJsonBytes(request.unsupportedProof));
      return {
        contractVersion: "adjustment-shadow-terminal-record-result/v3",
        reconciliationSha256: HASH_B,
        registrationSha256: request.shadowRegistration.registrationSha256,
        state: "recorded",
        terminalRecord: {
          accessSha256: request.nativeAccess.accessSha256,
          actionCompletedAt: request.unsupportedProof.finalizedAt,
          actionDisposition: "support_failed_no_action",
          actionSha256: request.unsupportedProof.actionSha256,
          candidateSha256: request.shadowRegistration.candidateSha256,
          contractVersion: "adjustment-shadow-terminal-record/v3",
          family: "temperature",
          finalizedMetadataRootSha256: request.nativeAccess.metadataRootSha256,
          finalizedPredictionCount: 2,
          maintenanceAnchorSha256: proofSha256,
          metadataGeneration: 1,
          recordedAt: "2028-02-09T09:12:00.000Z",
          reconciliationSha256: HASH_B,
          registrationSha256: request.shadowRegistration.registrationSha256,
          reservedKeySha256: request.shadowRegistration.reservedKeySha256,
          sourceSha256: request.shadowRegistration.sourceSha256,
          terminalMemberSha256: request.unsupportedProof.fullMemberRootSha256,
          terminalResultSha256: HASH_C,
        },
      };
    },
    retireTerminal: async ({ request }) => {
      assert.deepEqual(request.terminalTombstone, tombstone);
      calls.push("retire_terminal");
      return { state: "retired" };
    },
  });
  assert.deepEqual(calls, [
    "archive_evidence", "install_proof", "record_terminal", "archive_tombstone",
    "journal_tombstone", "retire_terminal",
  ]);
  assert.equal(result.proof.proof.graphManifestSha256, HASH_E);
  assert.deepEqual(result.tombstone, tombstone);
});

test("scorecard producer binds actual roots and publishes exact bytes", async () => {
  const same = (value) => ({ rain: value, temperature: value, wind: value });
  const produced = buildAdjustmentMaintenanceScorecardV2({
    attemptSha256: HASH_A,
    families: {
      rain: unavailableFamily("rain", "millimeters_per_hour"),
      temperature: unavailableFamily("temperature", "celsius"),
      wind: unavailableFamily("wind", "meters_per_second"),
    },
    generatedAt: "2026-10-10T12:00:00.000Z",
    identities: {
      active: same(null),
      prior: same(null),
      raw: same(HASH_B),
      shadow: same(null),
    },
    inputs: {
      adjustmentEvidenceManifestSha256: HASH_C,
      adjustmentEvidenceWatermarkSha256: HASH_D,
      forecastTrainingManifestSha256: HASH_E,
      frontierSha256: HASH_F,
      inputManifestSha256: HASH_A,
      localDateFrom: "2026-10-08",
      localDateTo: "2026-10-09",
      reportSha256: HASH_B,
      reportSha256s: same(HASH_B),
      sourceRevision: "1".repeat(40),
      targetCutoffAt: "2026-10-10T07:00:00.000Z",
    },
    job: {
      attemptState: "blocked",
      backlogState: "blocked",
      dueState: "overdue",
      successState: "failed",
    },
    operatorState: "enabled",
    progress: {
      confirmationCompletedEpochs: 0,
      confirmationRequiredEpochs: 9,
      rainCaptureExpiresAt: "2027-10-08T07:00:00.000Z",
      rollbackCompletedEpochs: 0,
      rollbackRequiredEpochs: 0,
    },
    sourceRevision: "1".repeat(40),
    warnings: {
      capacity: [],
      capture: ["semantic_catalog_unavailable"],
      fallback: [],
      gauge: [],
      source: [],
    },
  });
  assert.equal(produced.scorecard.policyDecision, "pending");
  assert.equal(produced.scorecard.actionState, "none");
  assert.equal(produced.scorecard.actionLineage.attemptSha256, HASH_A);
  assert.ok(produced.bytes.length < 128 * 1_024);
  assert.equal(JSON.stringify(produced.scorecard).includes("approval"), false);
  assert.equal(JSON.stringify(produced.scorecard).includes("reserved"), false);
  let transferred = Buffer.alloc(0);
  let invocation;
  // emulate one successful fixed forced-command ssh process
  const spawnImpl = (command, arguments_, options) => {
    invocation = { arguments_, command, options };
    const child = new EventEmitter();
    child.stdin = new PassThrough();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    child.stdin.on("data", (chunk) => {
      transferred = Buffer.concat([transferred, Buffer.from(chunk)]);
    });
    child.stdin.once("finish", () => {
      child.stdout.end("Installed forecast adjustment scorecard\n");
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  const publication = await publishAdjustmentMaintenanceScorecardV2({
    bytes: produced.bytes,
    scorecardSha256: produced.scorecardSha256,
  }, {
    now: new Date("2026-10-10T12:00:00.000Z"),
    spawnImpl,
  });
  assert.equal(publication.scorecardSha256, produced.scorecardSha256);
  assert.deepEqual(transferred, produced.bytes);
  assert.equal(invocation.command, "/usr/bin/ssh");
  assert.deepEqual(invocation.arguments_.slice(-3), [
    "weather-pi",
    "install-adjustment-scorecard-v2",
    produced.scorecardSha256,
  ]);
});

test("qualified release outcome preserves operator-off and compensation boundaries", () => {
  const base = {
    actionSha256: HASH_A,
    compensatingRelease: "2028.02.09-2",
    compensationState: "absent",
    contractVersion: "adjustment-family-release-status/v1",
    family: "temperature",
    fencingToken: "1",
    outcome: "active",
    state: "acknowledged",
    targetRelease: "2028.02.09-1",
  };
  assert.deepEqual(projectAdjustmentQualifiedReleaseOutcome(base, "compensate_raw"), {
    acknowledgementOutcome: "active",
    compensationOutcome: null,
    servingChanged: true,
    verificationOutcome: "verified",
  });
  assert.deepEqual(projectAdjustmentQualifiedReleaseOutcome({
    ...base,
    outcome: "deployed_operator_off",
  }, "compensate_raw"), {
    acknowledgementOutcome: "deployed_operator_off",
    compensationOutcome: null,
    servingChanged: false,
    verificationOutcome: "deployed_operator_off",
  });
  assert.deepEqual(projectAdjustmentQualifiedReleaseOutcome({
    ...base,
    compensationState: "verified",
    outcome: null,
    state: "compensation_required",
  }, "compensate_raw"), {
    acknowledgementOutcome: null,
    compensationOutcome: "raw_installed",
    servingChanged: false,
    verificationOutcome: "failed",
  });
});

test("family release caller forwards the fixed transaction and accepts only its status", async () => {
  let invocation;
  const input = {
    actionSha256: HASH_A,
    compensatingRelease: "2026.10.10-2",
    expectedCurrentRelease: "2026.10.09-1",
    expectedSettingsSha256: HASH_B,
    expectedSourceRelease: "2026.10.09-1",
    family: "temperature",
    fencingToken: "4",
    reportSha256: HASH_C,
    targetRelease: "2026.10.10-1",
  };
  // emulate one clean forced-command response
  const spawnImpl = (command, arguments_, options) => {
    invocation = { arguments_, command, options };
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(canonicalJsonBytes({
        actionSha256: HASH_A,
        compensationState: "absent",
        contractVersion: "adjustment-family-release-status/v1",
        family: "temperature",
        fencingToken: "4",
        outcome: "active",
        state: "acknowledged",
        targetRelease: "2026.10.10-1",
        compensatingRelease: "2026.10.10-2",
      }));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  const result = await applyAdjustmentFamilyRelease(input, { spawnImpl });
  assert.equal(result.state, "acknowledged");
  assert.equal(invocation.command, "/usr/bin/ssh");
  assert.deepEqual(invocation.arguments_.slice(-10), [
    "adjustment-family-release",
    "2026.10.10-1",
    "2026.10.10-2",
    "2026.10.09-1",
    "2026.10.09-1",
    HASH_B,
    "temperature",
    HASH_A,
    HASH_C,
    "4",
  ]);

  // accept the remote no-action projection only with its exact absent compensation
  const operatorOffSpawn = () => {
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(canonicalJsonBytes({
        actionSha256: HASH_A,
        compensatingRelease: "2026.10.10-2",
        compensationState: "absent",
        contractVersion: "adjustment-family-release-status/v1",
        family: "temperature",
        fencingToken: "4",
        outcome: "operator_off_unapplied",
        state: "operator_off_unapplied",
        targetRelease: "2026.10.10-1",
      }));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.equal(
    (await applyAdjustmentFamilyRelease(input, { spawnImpl: operatorOffSpawn })).state,
    "operator_off_unapplied",
  );

  // reject source drift and alternate fence encodings before ssh
  await assert.rejects(applyAdjustmentFamilyRelease({
    ...input,
    expectedSourceRelease: "2026.10.08-1",
  }, { spawnImpl }));
  await assert.rejects(applyAdjustmentFamilyRelease({
    ...input,
    fencingToken: "04",
  }, { spawnImpl }));
});

test("family current caller accepts only its exact requested source authority", async () => {
  const document = {
    activeInstalledReceiptSha256: null,
    catalogSha256: HASH_A,
    commit: "1".repeat(40),
    controlInstalledReceiptSha256: null,
    contractVersion: "adjustment-family-release-current-status/v1",
    family: "rain",
    release: "2026.10.10-1",
    settingsSha256: HASH_B,
    shadowInstalledReceiptSha256: HASH_C,
    sourceServerImageDigest: `sha256:${HASH_D}`,
  };
  const spawnImpl = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), [
      "adjustment-family-release-current-v1", "rain",
    ]);
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(`${JSON.stringify(document)}\n`);
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(await fetchAdjustmentFamilyReleaseCurrent("rain", { spawnImpl }),
    document);
  await assert.rejects(fetchAdjustmentFamilyReleaseCurrent("wind", { spawnImpl }));
});

// require one canonical root-walk lineage response without caller operands
test("family lineage caller accepts only the closed transitive authority", async () => {
  const document = {
    contractVersion: "adjustment-family-release-lineage/v2",
    controlSha256: HASH_A,
    controlVersion: "14",
    currentActionSha256: HASH_B,
    currentCommit: "2".repeat(40),
    currentRelease: "2026.10.11-1",
    epochAncestorCommit: "1".repeat(40),
    epochWitnessSha256: HASH_C,
    state: "verified_epoch_descendant",
    verifiedAt: "2026-10-11T09:00:00.000Z",
  };
  const spawnImpl = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), ["adjustment-family-release-lineage-v2"]);
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(canonicalJsonBytes(document));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(await fetchAdjustmentFamilyReleaseLineage({ spawnImpl }), document);
  await assert.rejects(fetchAdjustmentFamilyReleaseLineage({
    spawnImpl: (_command, _arguments) => {
      const child = new EventEmitter();
      child.stdout = new PassThrough();
      child.stderr = new PassThrough();
      child.kill = () => undefined;
      queueMicrotask(() => {
        child.stdout.end(canonicalJsonBytes({ ...document, state: "unchecked" }));
        child.stderr.end();
        queueMicrotask(() => child.emit("close", 0, null));
      });
      return child;
    },
  }));
});

test("capture epoch caller accepts only the root-authenticated frozen witness", async () => {
  const witness = futureOnlyEpochWitness();
  const spawnImpl = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), ["adjustment-revision-capture-epoch-v1"]);
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(canonicalJsonBytes(witness));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(await fetchAdjustmentRevisionCaptureEpochWitness({ spawnImpl }), witness);
});

test("capture epoch snapshot caller retains the exact zero frontier member", async () => {
  const snapshot = futureOnlyEpochSnapshot();
  const spawnImpl = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), [
      "adjustment-revision-capture-epoch-snapshot-v1",
    ]);
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(canonicalJsonBytes(snapshot));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(await fetchAdjustmentRevisionCaptureEpochSnapshot({ spawnImpl }), snapshot);
});

test("database ledger caller requires the exact full-0021 projection", async () => {
  const ledger = databaseLedgerV3();
  const invocations = [];
  const spawnImpl = (command, arguments_) => {
    invocations.push({ arguments_, command });
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(canonicalJsonBytes(ledger));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(await fetchAdjustmentMaintenanceDatabaseLedger({ spawnImpl }), ledger);
  assert.deepEqual(invocations[0].arguments_.slice(8), ["adjustment-database-ledger-v3"]);
  await assert.rejects(fetchAdjustmentMaintenanceDatabaseLedger({
    spawnImpl: (_command, _arguments) => {
      const child = new EventEmitter();
      child.stdout = new PassThrough();
      child.stderr = new PassThrough();
      child.kill = () => undefined;
      queueMicrotask(() => {
        child.stdout.end(canonicalJsonBytes({ ...ledger, extra: true }));
        child.stderr.end();
        queueMicrotask(() => child.emit("close", 0, null));
      });
      return child;
    },
  }));
});

test("registration schedule callers bind the owner bootstrap and authenticated status", async () => {
  const bootstrap = buildAdjustmentRollingScheduleBootstrap({
    epochAt: "2026-10-10T08:00:00.000Z",
    epochWitnessSha256: HASH_A,
  });
  const response = {
    bootstrapSha256: bootstrap.bootstrapSha256,
    initialized: true,
    scheduleContractSha256: bootstrap.scheduleContractSha256,
  };
  const received = [];
  const initializeSpawn = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), [
      "adjustment-registration-schedule-initialize-v3",
      bootstrap.bootstrapSha256,
    ]);
    const child = new EventEmitter();
    child.stdin = new PassThrough();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    child.stdin.on("data", (chunk) => received.push(chunk));
    child.stdin.on("finish", () => {
      child.stdout.end(canonicalJsonBytes(response));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(
    await initializeAdjustmentRegistrationSchedule({ bootstrap }, {
      spawnImpl: initializeSpawn,
    }),
    response,
  );
  assert.deepEqual(Buffer.concat(received), canonicalJsonBytes(bootstrap));
  assert.notEqual(bootstrap.bootstrapSha256, adjustmentSha256(canonicalJsonBytes(bootstrap)));
  await assert.rejects(initializeAdjustmentRegistrationSchedule({
    bootstrap: {
      ...bootstrap,
      bootstrapSha256: adjustmentSha256(canonicalJsonBytes(bootstrap)),
    },
  }, { spawnImpl: initializeSpawn }), /bootstrap differs/u);

  const status = registrationScheduleStatus("2026-10-10T12:00:00.000Z");
  const statusSpawn = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), [
      "adjustment-registration-schedule-status-v3",
    ]);
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(canonicalJsonBytes(status));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(await fetchAdjustmentRegistrationScheduleStatus({
    spawnImpl: statusSpawn,
  }), status);

  const lifecycle = {
    contractVersion: "adjustment-registration-lifecycle-status/v4",
    entries: status.slots.map(
      // retain the exact empty predecessor state for each free family
      (slot) => ({
        activeRegistration: null,
        latestRegistrationSha256: null,
        metadata: null,
        predecessor: null,
        slot,
      }),
    ),
    epochAt: status.epochAt,
    epochWitnessSha256: status.epochWitnessSha256,
    horizonEndAt: status.horizonEndAt,
    scheduleContractSha256: status.scheduleContractSha256,
    snapshotAt: status.snapshotAt,
  };
  const lifecycleSpawn = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), [
      "adjustment-registration-lifecycle-status-v4",
    ]);
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(canonicalJsonBytes(lifecycle));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(await fetchAdjustmentRegistrationLifecycleStatus({
    spawnImpl: lifecycleSpawn,
  }), lifecycle);
});

test("future-only seal and v3 anchor callers preserve the exact C/T/F chain", async () => {
  const chain = futureOnlyProofChain();
  assert.equal(chain.sealed.seal.inputClasses.candidate.count, 1);
  assert.equal(chain.sealed.seal.inputClasses.rain_gate_input.count, 0);
  assert.equal(chain.transferred.anchor.inputSealSha256, chain.sealed.sealSha256);
  assert.equal(chain.finalized.proof.transferredAnchorSha256,
    chain.transferred.anchorSha256);
  const received = [];
  const responses = [{
    contractVersion: "adjustment-future-only-input-seal-installation/v2",
    sealSha256: chain.sealed.sealSha256,
    state: "installed",
  }, {
    anchorSha256: chain.transferred.anchorSha256,
    contractVersion: "adjustment-maintenance-anchor-installation/v3",
    state: "transferred",
  }, {
    anchorSha256: HASH_F,
    contractVersion: "adjustment-maintenance-anchor-finalization/v3",
    finalizationProofSha256: chain.finalized.finalizationProofSha256,
    state: "finalized",
  }];
  const expectedArguments = [[
    "adjustment-future-input-seal-install-v2", chain.sealed.sealSha256,
  ], [
    "adjustment-maintenance-anchor-install-v3", chain.transferred.anchorSha256,
  ], [
    "adjustment-maintenance-anchor-finalize-v3",
    chain.finalized.finalizationProofSha256,
  ]];
  let invocation = 0;
  const spawnImpl = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), expectedArguments[invocation]);
    const response = responses[invocation];
    invocation += 1;
    const child = new EventEmitter();
    child.stdin = new PassThrough();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    const chunks = [];
    child.stdin.on("data", (chunk) => chunks.push(chunk));
    child.stdin.on("finish", () => {
      received.push(Buffer.concat(chunks));
      child.stdout.end(canonicalJsonBytes(response));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  await installAdjustmentFutureOnlyInputSeal({ seal: chain.sealed.seal }, { spawnImpl });
  await installAdjustmentMaintenanceAnchorV3({
    anchor: chain.transferred.anchor,
  }, { spawnImpl });
  await finalizeAdjustmentMaintenanceAnchorV3({
    proof: chain.finalized.proof,
  }, { spawnImpl });
  assert.deepEqual(received, [
    chain.sealed.bytes, chain.transferred.bytes, chain.finalized.bytes,
  ]);
});

test("owner burn caller preserves local and native access hash domains", async () => {
  const request = accessRequest();
  const requestBytes = canonicalJsonBytes(request);
  const requestSha256 = adjustmentSha256(requestBytes);
  const nativeUnsigned = {
    accessedAt: "2028-02-09T09:04:00.000Z",
    eligiblePredictionSetSha256: request.archive.eligiblePredictionSetSha256,
    expectedKeySetSha256: request.localBurn.expectedKeySetSha256,
    gateManifestSha256: request.localBurn.gateManifestSha256,
    journalHeadSha256: request.journalHeadSha256,
    maintenanceAnchorSha256: requestSha256,
    metadataRootSha256: HASH_A,
    registrationSha256: request.shadowRegistration.registrationSha256,
    revisionCatalogWatermarkSha256:
      request.localBurn.revisionCatalogWatermarkSha256,
    targetComparatorSnapshotRootSha256:
      request.localBurn.targetComparatorSnapshotRootSha256,
    targetCutoffAt: request.localBurn.targetCutoffAt,
  };
  const nativeAccessSha256 = adjustmentSha256(Buffer.from([
    "adjustment-confirmation-access/v2",
    nativeUnsigned.registrationSha256,
    nativeUnsigned.journalHeadSha256,
    nativeUnsigned.maintenanceAnchorSha256,
    nativeUnsigned.gateManifestSha256,
    nativeUnsigned.eligiblePredictionSetSha256,
    nativeUnsigned.expectedKeySetSha256,
    nativeUnsigned.metadataRootSha256,
    nativeUnsigned.targetComparatorSnapshotRootSha256,
    nativeUnsigned.revisionCatalogWatermarkSha256,
    nativeUnsigned.targetCutoffAt,
  ].join("\n")));
  const response = {
    contractVersion: "adjustment-confirmation-access-burn-result/v3",
    localAccessSha256: request.localBurn.accessSha256,
    nativeAccess: { accessSha256: nativeAccessSha256, ...nativeUnsigned },
    nativeAccessSha256,
    state: "burned",
  };
  let transferred = Buffer.alloc(0);
  const spawnImpl = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), [
      "adjustment-confirmation-access-burn-v3", requestSha256,
    ]);
    const child = new EventEmitter();
    child.stdin = new PassThrough();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    child.stdin.on("data", (chunk) => {
      transferred = Buffer.concat([transferred, Buffer.from(chunk)]);
    });
    child.stdin.on("finish", () => {
      child.stdout.end(canonicalJsonBytes(response));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  const result = await burnAdjustmentConfirmationAccessV3({ request }, { spawnImpl });
  assert.deepEqual(result, response);
  assert.deepEqual(transferred, requestBytes);
  assert.notEqual(result.localAccessSha256, result.nativeAccessSha256);
});

test("development custody anchor copies one exact ACK-v2 and installs before apply", async () => {
  const epochWitness = futureOnlyEpochWitness();
  const epochWitnessMemberSha256 = adjustmentSha256(canonicalJsonBytes(epochWitness));

  // keep full member custody distinct from the unsigned semantic epoch identity
  assert.notEqual(epochWitnessMemberSha256, epochWitness.witnessSha256);
  const acknowledgementUnsigned = {
    acknowledgedAt: "2026-10-10T08:00:00.000Z",
    afterArchiveCommitOrdinal: "7",
    afterFrontierSha256: HASH_A,
    authority: "cold_custody_only",
    contractVersion: "adjustment-revision-custody-acknowledgement/v2",
    custodyCheckpointSha256: HASH_B,
    memberRootSha256: HASH_C,
    nextArchiveCommitOrdinal: "8",
    nextFrontierSha256: HASH_D,
    pageSha256: HASH_E,
    previousAcknowledgementSha256: HASH_F,
    previousPageSha256: HASH_A,
    retirementEntries: [{
      fileSha256: HASH_A,
      identitySha256: HASH_A,
      kind: "revision_commit_receipt",
    }, {
      fileSha256: HASH_B,
      identitySha256: HASH_B,
      kind: "revision_frontier_successor",
    }, {
      fileSha256: HASH_C,
      identitySha256: HASH_C,
      kind: "revision_projection",
    }, {
      fileSha256: HASH_D,
      identitySha256: HASH_D,
      kind: "revision_stage_receipt",
    }],
    startMemberSha256: HASH_F,
    startSha256: HASH_A,
    watermarkArchiveCommitOrdinal: "9",
    watermarkFrontierSha256: HASH_E,
  };
  const acknowledgement = {
    ...acknowledgementUnsigned,
    acknowledgementSha256: adjustmentSha256(canonicalJsonBytes(acknowledgementUnsigned)),
  };
  const anchor = buildAdjustmentDevelopmentCustodyAnchor({
    acknowledgement,
    actionSha256: HASH_A,
    artifactSha256: HASH_B,
    candidateGraphSha256: HASH_C,
    candidateSha256: HASH_D,
    captureEpochWitnessSha256: epochWitnessMemberSha256,
    controlSha256: HASH_F,
    controlVersion: "14",
    developmentGraphSha256: HASH_A,
    dueKey: "monthly/temperature/2026-11",
    family: "temperature",
    fullGraphVerifiedAt: "2026-10-10T08:01:00.000Z",
    inputHeadSha256: HASH_B,
    lifecycleLedgerRootSha256: HASH_C,
    policyReportSha256: HASH_D,
    predecessorAnchorSha256: null,
    registrationSha256: HASH_E,
    sequence: "0",
    sourceCommit: "1".repeat(40),
    sourceSha256: HASH_F,
  });
  assert.equal(anchor.archiveCommitOrdinal, "8");
  assert.equal(anchor.frontierSha256, HASH_D);
  assert.equal(anchor.custodyAcknowledgementSha256,
    acknowledgement.acknowledgementSha256);
  const successor = buildAdjustmentDevelopmentCustodyAnchor({
    acknowledgement,
    actionSha256: HASH_B,
    artifactSha256: HASH_B,
    candidateGraphSha256: HASH_C,
    candidateSha256: HASH_D,
    captureEpochWitnessSha256: epochWitnessMemberSha256,
    controlSha256: HASH_F,
    controlVersion: "14",
    developmentGraphSha256: HASH_A,
    dueKey: "monthly/temperature/2026-12",
    family: "temperature",
    fullGraphVerifiedAt: "2026-10-10T08:02:00.000Z",
    inputHeadSha256: HASH_B,
    lifecycleLedgerRootSha256: HASH_C,
    policyReportSha256: HASH_D,
    predecessorAnchorSha256: adjustmentSha256(canonicalJsonBytes(anchor)),
    registrationSha256: HASH_E,
    sequence: "1",
    sourceCommit: "2".repeat(40),
    sourceSha256: HASH_F,
  });
  assert.equal(successor.captureEpochWitnessSha256, anchor.captureEpochWitnessSha256);
  assert.notEqual(successor.sourceCommit, anchor.sourceCommit);
  const controlAnchor = buildAdjustmentRainControlCustodyAnchor({
    acknowledgement,
    actionSha256: HASH_A,
    captureEpochWitnessSha256: epochWitnessMemberSha256,
    controlSha256: HASH_B,
    controlStateSha256: HASH_C,
    controlVersion: "14",
    dueMonth: "2026-11",
    fencingToken: "1",
    fullGraphVerifiedAt: "2026-10-10T08:03:00.000Z",
    graphManifestSha256: HASH_D,
    ordinalArtifactSha256: HASH_E,
    predecessorAnchorSha256: null,
    sequence: "0",
    sourceCommit: "2".repeat(40),
    sourceMemberRootSha256: HASH_F,
    sourceReceiptRootSha256: HASH_A,
    workstationJournalHeadSha256: HASH_B,
  });
  assert.equal(controlAnchor.contractVersion, "adjustment-rain-control-custody-anchor/v1");
  assert.equal(controlAnchor.controlStateSha256, HASH_C);
  assert.equal(controlAnchor.archiveCommitOrdinal, acknowledgement.nextArchiveCommitOrdinal);
  assert.notEqual(controlAnchor.captureEpochWitnessSha256, epochWitness.witnessSha256);
  const anchorBytes = canonicalJsonBytes(anchor);
  const anchorSha256 = adjustmentSha256(anchorBytes);
  let transferred = Buffer.alloc(0);
  const installSpawn = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), [
      "adjustment-development-custody-anchor-install-v1", anchorSha256,
    ]);
    const child = new EventEmitter();
    child.stdin = new PassThrough();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    child.stdin.on("data", (chunk) => {
      transferred = Buffer.concat([transferred, Buffer.from(chunk)]);
    });
    child.stdin.once("finish", () => {
      child.stdout.end(canonicalJsonBytes({
        anchorSha256,
        contractVersion: "adjustment-development-custody-anchor-installation/v1",
        state: "installed",
      }));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  await installAdjustmentDevelopmentCustodyAnchor({ anchor }, { spawnImpl: installSpawn });
  assert.deepEqual(transferred, anchorBytes);
  const current = { anchor, anchorSha256 };
  const currentSpawn = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), [
      "adjustment-development-custody-anchor-current-v1",
    ]);
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(`${JSON.stringify(current)}\n`);
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(await fetchAdjustmentDevelopmentCustodyAnchorCurrent({
    spawnImpl: currentSpawn,
  }), current);
  const controlBytes = canonicalJsonBytes(controlAnchor);
  const controlSha256 = adjustmentSha256(controlBytes);
  let controlTransferred = Buffer.alloc(0);
  const controlInstallSpawn = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), [
      "adjustment-rain-control-custody-anchor-install-v1", controlSha256,
    ]);
    const child = new EventEmitter();
    child.stdin = new PassThrough();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    child.stdin.on("data", (chunk) => {
      controlTransferred = Buffer.concat([controlTransferred, Buffer.from(chunk)]);
    });
    child.stdin.once("finish", () => {
      child.stdout.end(canonicalJsonBytes({
        anchorSha256: controlSha256,
        contractVersion: "adjustment-rain-control-custody-anchor-installation/v1",
        state: "installed",
      }));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  await installAdjustmentRainControlCustodyAnchor(
    { anchor: controlAnchor },
    { spawnImpl: controlInstallSpawn },
  );
  assert.deepEqual(controlTransferred, controlBytes);
  const controlCurrent = { anchor: controlAnchor, anchorSha256: controlSha256 };
  const controlCurrentSpawn = (command, arguments_) => {
    assert.equal(command, "/usr/bin/ssh");
    assert.deepEqual(arguments_.slice(8), [
      "adjustment-rain-control-custody-anchor-current-v1",
    ]);
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(`${JSON.stringify(controlCurrent)}\n`);
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(await fetchAdjustmentRainControlCustodyAnchorCurrent({
    spawnImpl: controlCurrentSpawn,
  }), controlCurrent);
});

test("metadata custody transport consumes only the retained exact proof", async () => {
  const entry = {
    archiveCommitOrdinal: "8",
    archiveCommittedAt: "2026-10-10T08:03:00.000Z",
    bodyByteCount: 4096,
    capsuleSha256: HASH_A,
    dueKey: "capture/2026-10-10T00:35:00.000Z",
    frontierSha256: HASH_B,
    inputSha256: HASH_C,
    issuedAt: "2026-10-10T08:00:00.000Z",
    maxValidAt: "2026-10-11T07:00:00.000Z",
    minValidAt: "2026-10-10T09:00:00.000Z",
    predictionBodySha256: HASH_D,
    predictionCommittedAt: "2026-10-10T08:02:00.000Z",
    predictionSchemaSha256: HASH_E,
    predictionSha256: HASH_F,
    predecessorFrontierSha256: HASH_A,
    receiptSha256: HASH_B,
    registrationSha256: HASH_C,
    rowCount: 23,
    sourceReceiptSha256: HASH_D,
    stageReceiptSha256: HASH_E,
  };
  const proofUnsigned = {
    acknowledgementSha256: HASH_A,
    archiveCommitOrdinal: "8",
    authority: "shadow_metadata_custody_only",
    contractVersion: "adjustment-shadow-metadata-custody-proof/v1",
    custodyCheckpointSha256: HASH_B,
    entries: [entry],
    frontierSha256: HASH_C,
    memberRootSha256: HASH_D,
    pageSha256: HASH_E,
    preparedAt: "2026-10-10T08:04:00.000Z",
  };
  const proof = {
    ...proofUnsigned,
    proofSha256: adjustmentSha256(canonicalJsonBytes(proofUnsigned)),
  };
  const status = {
    contractVersion: "adjustment-shadow-metadata-custody-status/v1",
    preparation: null,
    proof,
  };
  const consumption = {
    consumedAt: "2026-10-10T08:05:00.000Z",
    contractVersion: "adjustment-shadow-metadata-custody-consumption/v1",
    preparationSha256: HASH_F,
    proofSha256: proof.proofSha256,
    state: "consumed",
  };
  const invocations = [];
  const spawnDocument = (document) => (command, arguments_) => {
    invocations.push({ arguments_, command });
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(canonicalJsonBytes(document));
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(await fetchAdjustmentShadowMetadataCustodyStatus({
    spawnImpl: spawnDocument(status),
  }), status);
  assert.deepEqual(await finalizeAdjustmentShadowMetadataCustody({
    proofSha256: proof.proofSha256,
  }, { spawnImpl: spawnDocument(consumption) }), consumption);
  assert.deepEqual(invocations.map(
    // retain only the forced remote operation and closed operands
    (invocation) => invocation.arguments_.slice(8),
  ), [
    ["adjustment-shadow-metadata-custody-status-v1"],
    ["adjustment-shadow-metadata-custody-finalize-v1", proof.proofSha256],
  ]);
  const finalized = [];
  assert.deepEqual(await reconcileAdjustmentShadowMetadataCustody({
    fetchStatus: async () => status,
    finalize: async (request) => {
      finalized.push(request);
      return consumption;
    },
  }), { consumption, state: "consumed" });
  assert.deepEqual(finalized, [{ proofSha256: proof.proofSha256 }]);
  assert.deepEqual(await reconcileAdjustmentShadowMetadataCustody({
    fetchStatus: async () => ({ ...status, proof: null }),
    finalize: async () => { throw new Error("empty custody must not finalize"); },
  }), { state: "idle" });
});

// bind remote revision reads to one canonical frozen start and successor page
test("revision catalog caller validates the immutable forced-command envelopes", async () => {
  const genesis = adjustmentSha256(Buffer.from("adjustment-revision-frontier/v1\n0\n"));
  const snapshot = {
    archiveCommitOrdinal: "0",
    contractVersion: "adjustment-revision-serving-snapshot/v1",
    cutoffAt: "2026-10-10T07:00:00.000Z",
    entries: [],
    entryCount: 0,
    frontierSha256: genesis,
    snapshotSha256: adjustmentSha256(Buffer.from([
      "adjustment-revision-serving-snapshot/v1",
      "2026-10-10T07:00:00.000Z",
      "0",
      genesis,
      "",
    ].join("\n"))),
  };
  const startUnsigned = {
    contractVersion: "adjustment-revision-cold-transfer-start/v1",
    servingSnapshot: snapshot,
    watermarkArchiveCommitOrdinal: "0",
    watermarkFrontierSha256: genesis,
  };
  const start = {
    ...startUnsigned,
    startSha256: adjustmentSha256(canonicalJsonBytes(startUnsigned)),
  };
  const pageUnsigned = {
    afterArchiveCommitOrdinal: "0",
    afterFrontierSha256: genesis,
    contractVersion: "adjustment-revision-cold-page/v1",
    entries: [],
    eof: true,
    nextArchiveCommitOrdinal: "0",
    nextFrontierSha256: genesis,
    previousPageSha256: start.startSha256,
    startSha256: start.startSha256,
    watermarkArchiveCommitOrdinal: "0",
    watermarkFrontierSha256: genesis,
  };
  const page = {
    ...pageUnsigned,
    pageSha256: adjustmentSha256(canonicalJsonBytes(pageUnsigned)),
  };
  const gapStartUnsigned = {
    contractVersion: "adjustment-revision-gap-transfer-start/v1",
    frontierSha256: HASH_A,
    pageCount: "0",
  };
  const gapStart = {
    ...gapStartUnsigned,
    startSha256: adjustmentSha256(canonicalJsonBytes(gapStartUnsigned)),
  };
  const gapPageUnsigned = {
    contractVersion: "adjustment-revision-gap-payload-page/v1",
    entries: [],
    idle: true,
    memberRootSha256: adjustmentSha256(canonicalJsonBytes([])),
    predecessorFrontierSha256: HASH_A,
    startSha256: gapStart.startSha256,
  };
  const gapPage = {
    ...gapPageUnsigned,
    pageSha256: adjustmentSha256(canonicalJsonBytes(gapPageUnsigned)),
  };
  const gapAckUnsigned = {
    acknowledgedAt: "2026-10-10T08:00:00.000Z",
    contractVersion: "adjustment-revision-gap-payload-ack/v1",
    frontierSha256: adjustmentSha256(Buffer.from(
      `adjustment-revision-gap-payload-ack/v1\n${HASH_A}\n${HASH_B}\n${HASH_C}\n${HASH_D}`,
    )),
    graphManifestSha256: HASH_C,
    memberRootSha256: HASH_D,
    pageCount: "1",
    pageSha256: HASH_B,
    predecessorFrontierSha256: HASH_A,
  };
  const gapAck = {
    ...gapAckUnsigned,
    acknowledgementSha256: adjustmentSha256(canonicalJsonBytes(gapAckUnsigned)),
  };
  const custodyAckUnsigned = {
    acknowledgedAt: "2026-10-10T08:00:00.000Z",
    afterArchiveCommitOrdinal: "0",
    afterFrontierSha256: genesis,
    authority: "cold_custody_only",
    contractVersion: "adjustment-revision-custody-acknowledgement/v2",
    custodyCheckpointSha256: HASH_C,
    memberRootSha256: HASH_D,
    nextArchiveCommitOrdinal: "1",
    nextFrontierSha256: HASH_E,
    pageSha256: HASH_B,
    previousAcknowledgementSha256: null,
    previousPageSha256: start.startSha256,
    retirementEntries: [{
      fileSha256: HASH_A,
      identitySha256: HASH_B,
      kind: "revision_commit_receipt",
    }, {
      fileSha256: HASH_B,
      identitySha256: HASH_C,
      kind: "revision_frontier_successor",
    }, {
      fileSha256: HASH_C,
      identitySha256: HASH_D,
      kind: "revision_projection",
    }, {
      fileSha256: HASH_D,
      identitySha256: HASH_E,
      kind: "revision_stage_receipt",
    }],
    startMemberSha256: HASH_F,
    startSha256: start.startSha256,
    watermarkArchiveCommitOrdinal: "1",
    watermarkFrontierSha256: HASH_E,
  };
  const custodyAck = {
    ...custodyAckUnsigned,
    acknowledgementSha256: adjustmentSha256(canonicalJsonBytes(custodyAckUnsigned)),
  };
  const invocations = [];

  // emit one fixed document from a process-shaped test boundary
  const spawnDocument = (document) => (command, arguments_) => {
    invocations.push({ arguments_, command });
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.kill = () => undefined;
    queueMicrotask(() => {
      child.stdout.end(`${JSON.stringify(document)}\n`);
      child.stderr.end();
      queueMicrotask(() => child.emit("close", 0, null));
    });
    return child;
  };
  assert.deepEqual(await fetchAdjustmentRevisionColdTransferStart({
    cutoffAt: snapshot.cutoffAt,
    watermarkArchiveCommitOrdinal: "0",
  }, { spawnImpl: spawnDocument(start) }), start);
  assert.deepEqual(await fetchAdjustmentRevisionColdCurrentTransferStart(
    snapshot.cutoffAt,
    { spawnImpl: spawnDocument(start) },
  ), start);
  assert.deepEqual(await fetchAdjustmentRevisionColdPage({
    afterArchiveCommitOrdinal: "0",
    afterFrontierSha256: genesis,
    previousPageSha256: start.startSha256,
    startSha256: start.startSha256,
    watermarkArchiveCommitOrdinal: "0",
    watermarkFrontierSha256: genesis,
  }, { spawnImpl: spawnDocument(page) }), page);
  assert.deepEqual(await fetchAdjustmentRevisionGapTransferStart({
    spawnImpl: spawnDocument(gapStart),
  }), gapStart);
  assert.deepEqual(await fetchAdjustmentRevisionGapPayloadPage({
    frontierSha256: HASH_A,
    startSha256: gapStart.startSha256,
  }, { spawnImpl: spawnDocument(gapPage) }), gapPage);
  assert.deepEqual(await acknowledgeAdjustmentRevisionGapPayload({
    graphManifestSha256: HASH_C,
    pageSha256: HASH_B,
  }, { spawnImpl: spawnDocument(gapAck) }), gapAck);
  assert.deepEqual(await acknowledgeAdjustmentRevisionColdCustody({
    afterArchiveCommitOrdinal: "0",
    afterFrontierSha256: genesis,
    custodyCheckpointSha256: HASH_C,
    memberRootSha256: HASH_D,
    pageSha256: HASH_B,
    previousPageSha256: start.startSha256,
    startMemberSha256: HASH_F,
    startSha256: start.startSha256,
    watermarkArchiveCommitOrdinal: "1",
    watermarkFrontierSha256: HASH_E,
  }, { spawnImpl: spawnDocument(custodyAck) }), custodyAck);
  assert.deepEqual(await drainAdjustmentRevisionGapSpool({
    archive: {},
    clock: () => new Date("2026-10-10T08:00:00.000Z"),
    journal: {},
    readHead: async () => null,
  }, {
    acknowledge: async () => { throw new Error("idle page must not acknowledge"); },
    fetchPage: async () => gapPage,
    fetchStart: async () => gapStart,
  }), { pageSha256: gapPage.pageSha256, state: "idle" });
  assert.deepEqual(invocations.map((invocation) => invocation.arguments_.slice(8)), [
    ["adjustment-revision-catalog-start-v1", snapshot.cutoffAt, "0"],
    ["adjustment-revision-catalog-current-start-v1", snapshot.cutoffAt],
    [
      "adjustment-revision-catalog-page-v1", "0", genesis, start.startSha256,
      "0", genesis, start.startSha256,
    ],
    ["adjustment-revision-gap-start-v1"],
    ["adjustment-revision-gap-page-v1", gapStart.startSha256, HASH_A],
    ["adjustment-revision-gap-ack-v1", HASH_B, HASH_C],
    [
      "adjustment-revision-custody-ack-v2", "1", HASH_E, start.startSha256,
      "0", genesis, start.startSha256, HASH_B, HASH_C, HASH_D, HASH_F,
    ],
  ]);

  // reject database ordinals outside the actual producer range before ssh
  await assert.rejects(fetchAdjustmentRevisionColdTransferStart({
    cutoffAt: snapshot.cutoffAt,
    watermarkArchiveCommitOrdinal: "9223372036854775808",
  }, { spawnImpl: spawnDocument(start) }));
});

test("attempt validation rejects contradictory fitted evidence", () => {
  const base = {
    actionEligible: false,
    attemptIdentitySha256: HASH_B,
    candidateGraphSha256: null,
    candidateSha256: null,
    contractVersion: "forecast-adjustment-maintenance-attempt/v2",
    dueKey: "monthly/wind/2026-10",
    family: "wind",
    fitReceiptSha256: null,
    inputHeadSha256: HASH_C,
    lifecycleHeadSha256: HASH_D,
    mode: "monthly",
    originalCutoffAt: "2026-10-01T07:00:00.000Z",
    reason: "semantic_catalog_unavailable",
    reservedConfirmationExposed: false,
    semanticInputSha256: null,
    servingChanged: false,
    state: "blocked",
  };
  assert.equal(validateAdjustmentMaintenanceAttempt(base), base);
  assert.throws(
    // refuse a completed fit without its immutable candidate graph
    () => validateAdjustmentMaintenanceAttempt({
      ...base,
      candidateSha256: HASH_E,
      fitReceiptSha256: HASH_F,
      reason: "development_candidate_archived",
      semanticInputSha256: HASH_A,
      state: "completed",
    }),
    /candidate identity is invalid/u,
  );
  const promoted = {
    ...base,
    actionEligible: true,
    dueKey: "daily/2026-10-08",
    family: null,
    mode: "daily",
    originalCutoffAt: "2026-10-09T07:00:00.000Z",
    reason: "daily_candidate_promoted",
    semanticInputSha256: HASH_A,
    servingChanged: true,
    state: "completed",
  };
  assert.equal(validateAdjustmentMaintenanceAttempt(promoted), promoted);
  assert.equal(validateAdjustmentMaintenanceAttempt({
    ...promoted,
    servingChanged: false,
  }).servingChanged, false);
});

test("archive member publication advances one predecessor-linked durable head", async () => {
  const journal = new MemoryControllerJournal();
  journal.locked = true;
  const graphs = new Map();
  let head = null;
  const archive = {
    // publish one deterministic in-memory member pack
    publishCasObject: async (members) => ({
      members: members.map(
        // retain deterministic nonoverlapping member locations
        (member, index) => ({
          identitySha256: member.identitySha256,
          kind: member.kind,
          memberLength: member.payload.length,
          memberOffset: 16 + index * 1_024,
          memberSha256: adjustmentSha256(member.payload),
        }),
      ),
      objectSha256: adjustmentSha256(Buffer.concat([
        Buffer.from("object:"),
        ...members.map((member) => member.payload),
      ])),
    }),
    // retain one immutable graph document
    publishGraphManifest: async (manifest) => {
      const objectSha256 = adjustmentSha256(canonicalJsonBytes(manifest));
      graphs.set(objectSha256, structuredClone(manifest));
      return { objectSha256 };
    },
    // verify one retained graph projection
    verifyFullGraph: async (objectSha256) => ({
      manifest: structuredClone(graphs.get(objectSha256)),
      manifestObjectSha256: objectSha256,
    }),
    // rotate only the exact supplied durable head
    updateHead: async (objectSha256) => {
      head = objectSha256;
      return { manifestObjectSha256: objectSha256 };
    },
  };
  const publish = async (identitySha256, payload) => await publishAdjustmentArchiveMember({
    archive,
    clock: () => new Date("2026-10-10T12:00:00.000Z"),
    identitySha256,
    journal,
    kind: "forecast-adjustment-test-member/v1",
    payload,
    readHead: async () => head,
  });

  try {
    const first = await publish(HASH_A, Buffer.from("first\n"));
    const second = await publish(HASH_B, Buffer.from("second\n"));
    assert.equal(graphs.get(second.manifestObjectSha256).predecessorGraphSha256,
      first.manifestObjectSha256);
    assert.equal(head, second.manifestObjectSha256);
    const replay = await publish(HASH_A, Buffer.from("first\n"));
    assert.equal(replay.manifestObjectSha256, first.manifestObjectSha256);
    assert.equal(head, second.manifestObjectSha256);
    const segment = await publishAdjustmentArchiveGraphSegment({
      archive,
      clock: () => new Date("2026-10-10T12:00:00.000Z"),
      dueKey: `archive/revision-gap-page/${HASH_C}`,
      journal,
      readHead: async () => head,
      segment: {
        crossLinks: [{
          fromIdentitySha256: HASH_C,
          relation: "binds_test",
          toIdentitySha256: HASH_D,
        }],
        members: [{
          identitySha256: HASH_C,
          kind: "adjustment-revision-gap-page-checkpoint/v1",
          payload: Buffer.from("checkpoint\n"),
        }, {
          identitySha256: HASH_D,
          kind: "adjustment-revision-gap/v1",
          payload: Buffer.from("gap\n"),
        }],
      },
    });
    assert.equal(graphs.get(segment.manifestObjectSha256).entries.length, 2);
    assert.deepEqual(graphs.get(segment.manifestObjectSha256).crossLinks, [{
      fromIdentitySha256: HASH_C,
      relation: "binds_test",
      toIdentitySha256: HASH_D,
    }]);
    assert.equal(graphs.get(segment.manifestObjectSha256).predecessorGraphSha256,
      second.manifestObjectSha256);
    const packedMembers = Array.from({ length: 17 },
      // model one custody pack larger than the obsolete transport ceiling
      (_, index) => {
        const payload = Buffer.from(`packed-${index}\n`);
        return {
          identitySha256: adjustmentSha256(payload),
          kind: "adjustment-revision-cold-page-checkpoint/v1",
          payload,
        };
      });
    const packed = await publishAdjustmentArchiveGraphSegment({
      archive,
      clock: () => new Date("2026-10-10T12:00:00.000Z"),
      dueKey: `archive/revision-custody-pack/${packedMembers[0].identitySha256}`,
      journal,
      readHead: async () => head,
      segment: { crossLinks: [], members: packedMembers },
    });
    assert.equal(graphs.get(packed.manifestObjectSha256).entries.length, 17);
    assert.equal(graphs.get(packed.manifestObjectSha256).predecessorGraphSha256,
      segment.manifestObjectSha256);
  } finally {
    journal.locked = false;
  }
});

test("direct due execution refuses an unanchored journal genesis", async () => {
  const journal = new MemoryControllerJournal();
  journal.status = async () => ({ activeLeases: [], headSha256: null });
  const ports = controllerPorts(journal);
  journal.locked = true;

  try {
    const result = await executeAdjustmentMaintenanceDue({
      due: {
        dueKey: "daily/2026-10-08",
        family: null,
        mode: "daily",
        originalCutoffAt: "2026-10-09T07:00:00.000Z",
        scope: "daily",
      },
      now: new Date("2026-10-10T12:00:00.000Z"),
      ports,
    });
    assert.deepEqual(result, {
      dueKey: "daily/2026-10-08",
      reason: "history_unavailable",
      state: "blocked",
    });
  } finally {
    journal.locked = false;
  }
});

test("attempt publication retry reuses the retained terminal outcome and retirement", async () => {
  const due = {
    dueKey: "daily/2028-02-09",
    family: null,
    mode: "daily",
    originalCutoffAt: "2028-02-10T08:00:00.000Z",
    scope: "daily",
  };
  const journal = new MemoryControllerJournal({ registered: [due.dueKey] });
  journal.locked = true;
  const outcome = {
    actionEligible: true,
    candidateGraphSha256: null,
    candidateSha256: null,
    fitReceiptSha256: null,
    reason: "daily_candidate_promoted",
    semanticInputSha256: HASH_C,
    servingChanged: false,
    state: "completed",
  };
  journal.terminalOutcomes.set(due.dueKey, outcome);
  journal.terminalRetirements.set(due.dueKey, {
    completed: false,
    kind: "qualified_v3",
    request: { contractVersion: "fixture-retirement/v1" },
    requestSha256: HASH_D,
  });
  let inspections = 0;
  let publications = 0;
  const publishedReports = [];
  const ports = controllerPorts(journal, {
    inspectSemanticInput: async () => {
      inspections += 1;
      throw new Error("semantic work must not replay");
    },
    publishAttemptReport: async (report) => {
      validateAdjustmentMaintenanceAttempt(report);
      publishedReports.push(structuredClone(report));
      publications += 1;

      // inject one crash after owner retirement but before report publication
      if (publications === 1) {
        throw new Error("attempt publication interrupted");
      }
      return {
        manifestObjectSha256: HASH_E,
        reportSha256: HASH_F,
      };
    },
  });
  await assert.rejects(
    executeAdjustmentMaintenanceDue({ due, now: new Date("2028-02-10T09:00:00.000Z"), ports }),
    /attempt publication interrupted/u,
  );
  assert.equal(journal.terminalRetirements.get(due.dueKey).completed, true);
  const retried = await executeAdjustmentMaintenanceDue({
    due,
    now: new Date("2028-02-10T09:01:00.000Z"),
    ports,
  });
  assert.equal(retried.state, "completed");
  assert.equal(retried.reason, outcome.reason);
  assert.deepEqual(publishedReports[0], publishedReports[1]);
  assert.equal(publishedReports[1].servingChanged, false);
  assert.equal(inspections, 0);
  assert.equal(publications, 2);
});
