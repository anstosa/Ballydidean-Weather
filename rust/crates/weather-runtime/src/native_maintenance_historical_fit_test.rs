use std::path::Path;
use std::process::Command;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum,
};

use super::native_maintenance_historical_fit::{
    NativeMaintenanceHistoricalFitError, build_native_adjustment_historical_fit_projection,
};
use crate::{
    NativeMaintenanceColdCatalog, NativeMaintenanceColdCatalogKindCounts,
    NativeMaintenanceHistoricalArchiveIndex, NativeMaintenanceHistoricalFamily,
    NativeMaintenanceHistoricalOccurrence, VerifiedNativeMaintenanceCaptureEpochWitness,
};

/// Match one authentic source projection row and every derived native identity.
#[test]
fn historical_fit_matches_predecessor_projection_and_lineage() {
    let fixture = predecessor_fixture();
    let projection = build_native_adjustment_historical_fit_projection(
        &epoch_witness(
            "2026-10-10T08:00:00.000Z",
            string(&fixture, "witnessSha256"),
        ),
        NativeMaintenanceHistoricalFamily::Temperature,
        "2026-12",
        &history(&fixture),
    )
    .unwrap();

    assert_eq!(projection.cutoff_at, string(&fixture, "cutoffAt"));
    assert_eq!(
        projection.member_root_sha256,
        string(&fixture, "memberRootSha256")
    );
    assert_eq!(projection.row_count, 1);
    assert_eq!(projection.class_counts.actual_best_match, 1);
    assert_eq!(projection.class_counts.native_source, 0);
    assert_eq!(projection.class_counts.target_revision, 0);
    assert_eq!(projection.projection_members.len(), 1);
    assert_eq!(
        projection.projection_members[0].member_sha256,
        string(&fixture, "memberSha256")
    );
    assert_eq!(projection.projection_members[0].row_index, 0);
    assert_eq!(
        projection.projection_members[0]
            .receipt
            .archiveCommitOrdinal,
        "1"
    );
}

/// Reject a valid payload when one causal source clock predates the verified epoch.
#[test]
fn historical_fit_rejects_pre_epoch_causal_history() {
    let fixture = predecessor_fixture();
    let error = build_native_adjustment_historical_fit_projection(
        &epoch_witness(
            "2026-10-12T09:00:00.000Z",
            string(&fixture, "witnessSha256"),
        ),
        NativeMaintenanceHistoricalFamily::Temperature,
        "2026-12",
        &history(&fixture),
    )
    .unwrap_err();

    assert_eq!(error, NativeMaintenanceHistoricalFitError::Clock);
}

/// Ask the installed predecessor for one authentic projection and exact expected identities.
fn predecessor_fixture() -> Value {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.ancestors().nth(3).expect("workspace root");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_ORACLE])
        .current_dir(root)
        .output()
        .expect("run historical fit predecessor fixture");
    assert!(
        output.status.success(),
        "historical fit predecessor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("decode historical fit fixture")
}

/// Build the typed verified archive boundary from the predecessor fixture.
fn history(fixture: &Value) -> NativeMaintenanceHistoricalArchiveIndex {
    let payload_bytes = STANDARD
        .decode(string(fixture, "payloadBase64"))
        .expect("decode projection body");
    NativeMaintenanceHistoricalArchiveIndex {
        catalog: NativeMaintenanceColdCatalog {
            archive_commit_ordinal: "1".to_owned(),
            catalog_root_sha256: repeated('c'),
            frontier_count: 1,
            frontier_root_sha256: repeated('d'),
            kind_counts: NativeMaintenanceColdCatalogKindCounts {
                actual_best_match: 1,
                native_source: 0,
                rain_gate_input: 0,
                target_revision: 0,
            },
            receipt_root_sha256: repeated('e'),
            serving_snapshot_sha256: repeated('f'),
        },
        contract_version: "adjustment-revision-historical-archive-index/v1",
        history_root_sha256: string(fixture, "historyRootSha256").to_owned(),
        occurrences: vec![NativeMaintenanceHistoricalOccurrence {
            graph_manifest_sha256: string(fixture, "graphManifestSha256").to_owned(),
            page_sha256: repeated('9'),
            payload_bytes,
            payload_identity_sha256: string(fixture, "payloadIdentitySha256").to_owned(),
            payload_kind: "adjustment-revision-projection/v1".to_owned(),
            publication_disposition: "published".to_owned(),
            receipts: vec![receipt(fixture)],
        }],
        pages: Vec::new(),
        receipt_count: 1,
    }
}

/// Build the exact generated receipt used by the authentic occurrence.
fn receipt(fixture: &Value) -> AdjustmentRevisionCommitReceipt {
    AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: "1".to_owned(),
        archiveCommittedAt: "2026-10-12T08:01:00.000Z".to_owned(),
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: repeated('a'),
        predecessorFrontierSha256: repeated('b'),
        projectionIdentitySha256: string(fixture, "payloadIdentitySha256").to_owned(),
        projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch,
        projectionSha256: string(fixture, "payloadIdentitySha256").to_owned(),
        receiptSha256: string(fixture, "receiptSha256").to_owned(),
        stageReceiptSha256: string(fixture, "stageReceiptSha256").to_owned(),
    }
}

/// Build the already-verified epoch fields consumed by the pure projector.
fn epoch_witness(
    epoch_at: &str,
    witness_sha256: &str,
) -> VerifiedNativeMaintenanceCaptureEpochWitness {
    VerifiedNativeMaintenanceCaptureEpochWitness {
        catalog_frontier_sha256: repeated('1'),
        control_plane_sha256: repeated('2'),
        control_plane_version: "14".to_owned(),
        database_migration_history_sha256: repeated('3'),
        epoch_at: epoch_at.to_owned(),
        serving_snapshot_sha256: repeated('4'),
        source_commit: "5".repeat(40),
        source_release: "2026.10.10-1".to_owned(),
        source_server_image_digest: format!("sha256:{}", repeated('6')),
        witness_sha256: witness_sha256.to_owned(),
    }
}

/// Borrow one required fixture string.
fn string<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name].as_str().expect("fixture string")
}

/// Construct one valid lowercase digest fixture.
fn repeated(value: char) -> String {
    value.to_string().repeat(64)
}

const NODE_ORACLE: &str = r#"
import { createHash } from "node:crypto";
import {
  encodeAdjustmentRevisionProjection,
} from "./scripts/research/adjustment-maintenance-runtime/forecast/maintenance-revision-projection.js";
import {
  encodeMaintenanceBinary64,
} from "./scripts/research/adjustment-maintenance-runtime/forecast/maintenance-shadow-values.js";
import {
  buildAdjustmentHistoricalFitProjection,
} from "./scripts/research/adjustment_historical_fit_assembler.mjs";
import {
  adjustmentSha256, canonicalJsonBytes,
} from "./scripts/research/adjustment_plaintext_archive.mjs";

const HASH = "a".repeat(64);
const SECOND = "b".repeat(64);
const epochAt = "2026-10-10T08:00:00.000Z";
const frontier = adjustmentSha256(Buffer.from("adjustment-revision-frontier/v1\n0\n"));
const snapshot = adjustmentSha256(Buffer.from([
  "adjustment-revision-serving-snapshot/v1", epochAt, "0", frontier, "",
].join("\n")));
const unsigned = {
  activationKind: "inert_v14_pre_activation", archiveCommitOrdinal: "0",
  catalogFrontierSha256: frontier,
  contractVersion: "adjustment-revision-capture-epoch-witness/v1",
  controlPlaneSha256: HASH, controlPlaneVersion: "14",
  databaseMigrationHistorySha256:
    "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
  epochAt, servingSnapshotSha256: snapshot, sourceCommit: "1".repeat(40),
  sourceRelease: "2026.10.10-1", sourceServerImageDigest: `sha256:${HASH}`,
  sourceWebImageDigest: `sha256:${SECOND}`,
};
const epochWitness = {
  ...unsigned, witnessSha256: adjustmentSha256(canonicalJsonBytes(unsigned)),
};
const validAt = "2026-10-11T09:00:00.000Z";
const source = {
  adapterVersion: "open-meteo/v4", contractEpoch: "forecast/v4",
  dataset: "best_match", providerKey: "open-meteo",
  sourceConfigFingerprint: "open-meteo-forecast/v4", sourceId: "7",
  sourceKey: "open-meteo-forecast", sourceKind: "forecast",
  upstreamModel: "best_match",
};
const row = {
  apparentTemperatureC64: encodeMaintenanceBinary64(10), blackGlobeTemperatureC64: null,
  cloudCoverPercent64: encodeMaintenanceBinary64(50), contentSha256: HASH,
  pm25MicrogramsPerCubicMeter64: null, precipitationMm64: encodeMaintenanceBinary64(0),
  precipitationRateMmPerHour64: null, pressureHpa64: encodeMaintenanceBinary64(1010),
  relativeHumidityPercent64: encodeMaintenanceBinary64(80),
  soilElectricalConductivityMicrosiemensPerCm64: null, soilMoisturePercent64: null,
  solarRadiationWm264: null, temperatureC64: encodeMaintenanceBinary64(11),
  uvIndex64: null, validAt, waterLevelM64: null, wetBulbGlobeTemperatureC64: null,
  windDirectionDegrees64: encodeMaintenanceBinary64(180),
  windGustMps64: encodeMaintenanceBinary64(8), windSpeedMps64: encodeMaintenanceBinary64(5),
};
const document = {
  contractVersion: "adjustment-revision-projection/v1", family: "shared",
  logicalKey: { productRunAt: "2026-10-11T08:00:00.000Z", sourceId: "7",
    sourceKind: "forecast", validAt },
  logicalReceivedAt: "2026-10-11T08:05:00.000Z", projectionKind: "actual_best_match",
  rows: [row], source, storedContentSha256: HASH,
};
const payloadBytes = encodeAdjustmentRevisionProjection(document);
const payloadIdentitySha256 = adjustmentSha256(payloadBytes);
const receipt = {
  archiveCommitOrdinal: "1", archiveCommittedAt: "2026-10-12T08:01:00.000Z",
  contractVersion: "adjustment-revision-commit-receipt/v1", frontierSha256: HASH,
  predecessorFrontierSha256: SECOND, projectionIdentitySha256: payloadIdentitySha256,
  projectionKind: "actual_best_match", projectionSha256: payloadIdentitySha256,
  receiptSha256: adjustmentSha256(Buffer.from("receipt-1")),
  stageReceiptSha256: adjustmentSha256(Buffer.from("stage-1")),
};
const historyRootSha256 = adjustmentSha256(Buffer.from("history"));
const occurrence = {
  graphManifestSha256: adjustmentSha256(Buffer.from("graph-1")),
  pageSha256: adjustmentSha256(Buffer.from("page-1")), payloadBytes,
  payloadIdentitySha256, payloadKind: document.contractVersion,
  publicationDisposition: "published", receipts: [receipt],
};
const result = buildAdjustmentHistoricalFitProjection({
  epochWitness, family: "temperature", fitMonth: "2026-12",
  history: { catalog: {}, contractVersion: "adjustment-revision-historical-archive-index/v1",
    historyRootSha256, occurrences: [occurrence], pages: [], receiptCount: 1 },
});
process.stdout.write(JSON.stringify({
  cutoffAt: result.cutoffAt, graphManifestSha256: occurrence.graphManifestSha256,
  historyRootSha256, memberRootSha256: result.memberRootSha256,
  memberSha256: result.projectionMembers[0].memberSha256,
  payloadBase64: payloadBytes.toString("base64"), payloadIdentitySha256,
  receiptSha256: receipt.receiptSha256, stageReceiptSha256: receipt.stageReceiptSha256,
  witnessSha256: epochWitness.witnessSha256,
}));
"#;
