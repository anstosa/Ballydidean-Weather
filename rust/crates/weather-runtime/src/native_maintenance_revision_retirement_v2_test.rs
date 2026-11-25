use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionGapInput,
    AdjustmentRevisionGapReason, AdjustmentRevisionProjectionKind, Nullable,
};
use weather_edge::{
    ArchiveWireCodec, PermanentArchiveGapCodec, PermanentArchiveGapInput,
    PrivateRevisionReceiptCodec, RevisionArchiveCanonicalCodec,
};

use super::collect_native_maintenance_revision_retirement_v2;
use crate::{
    native_maintenance_retirement_files_v2::{NativeMaintenanceRetirementKindV2, format_digest},
    native_model_evidence_values::NativeModelEvidenceValues,
    native_revision_gap_codec::NativeRevisionGapCodec,
};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Exercise the full ordinary terminal-gap branch with authenticated native wire values.
#[test]
fn collects_nonempty_revision_terminal_gap_graph() {
    let root = fixture_root();
    let codec = ArchiveWireCodec::new(NativeModelEvidenceValues::new());
    let gap_codec = NativeRevisionGapCodec;
    let projection_bytes = b"ordinary projection\n";
    let projection_sha256 = format_digest(codec.sha256(projection_bytes));
    let predecessor = format_digest(codec.sha256(b"predecessor"));
    let stage_sha256 = format_digest(codec.sha256(b"stage receipt"));
    let archive_committed_at = "2026-11-24T01:02:03.004Z";
    let receipt_preimage = [
        AdjustmentRevisionCommitReceiptContractVersionLiteral::WIRE_VALUE,
        "1",
        archive_committed_at,
        "actual_best_match",
        &projection_sha256,
        &projection_sha256,
        &stage_sha256,
        &predecessor,
    ]
    .join("\n");
    let receipt_sha256 = format_digest(codec.sha256(receipt_preimage.as_bytes()));
    let frontier_preimage = [
        "adjustment-revision-frontier/v1",
        &predecessor,
        "1",
        &receipt_sha256,
    ]
    .join("\n");
    let frontier = format_digest(codec.sha256(frontier_preimage.as_bytes()));
    let receipt = AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: "1".to_owned(),
        archiveCommittedAt: archive_committed_at.to_owned(),
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: frontier.clone(),
        predecessorFrontierSha256: predecessor.clone(),
        projectionIdentitySha256: projection_sha256.clone(),
        projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch,
        projectionSha256: projection_sha256.clone(),
        receiptSha256: receipt_sha256.clone(),
        stageReceiptSha256: stage_sha256,
    };
    let receipt_bytes = codec
        .revision_commit_receipt_bytes(&receipt)
        .expect("valid commit receipt");
    let successor_bytes = codec
        .revision_successor_bytes(&receipt)
        .expect("valid successor");
    let stage_bytes = b"ordinary stage receipt\n";
    let logical_key = format_digest(codec.sha256(b"logical key"));
    let gap_input = AdjustmentRevisionGapInput {
        logicalKeySha256: logical_key,
        projectionIdentitySha256: Nullable::Value(projection_sha256.clone()),
        projectionKind: AdjustmentRevisionProjectionKind::ActualBestMatch,
        projectionSha256: Nullable::Value(projection_sha256.clone()),
        reason: AdjustmentRevisionGapReason::ArchivePublishFailed,
    };
    let gap_unsigned = gap_codec
        .encode_gap_receipt(
            PermanentArchiveGapInput::Revision(&gap_input),
            archive_committed_at,
            None,
        )
        .expect("valid unsigned gap");
    let gap_sha256 = format_digest(gap_codec.sha256(&gap_unsigned));
    let gap_bytes = gap_codec
        .encode_gap_receipt(
            PermanentArchiveGapInput::Revision(&gap_input),
            archive_committed_at,
            Some(&gap_sha256),
        )
        .expect("valid signed gap");
    let gap_key = gap_codec
        .encode_gap_key(PermanentArchiveGapInput::Revision(&gap_input))
        .expect("valid gap key");
    let gap_identity = format_digest(gap_codec.sha256(&gap_key));

    write_member(
        &root,
        NativeMaintenanceRetirementKindV2::RevisionCommitReceipt,
        &frontier,
        &receipt_bytes,
    );
    write_member(
        &root,
        NativeMaintenanceRetirementKindV2::RevisionFrontierSuccessor,
        &predecessor,
        &successor_bytes,
    );
    write_member(
        &root,
        NativeMaintenanceRetirementKindV2::RevisionProjection,
        &projection_sha256,
        projection_bytes,
    );
    write_member(
        &root,
        NativeMaintenanceRetirementKindV2::RevisionStageReceipt,
        &projection_sha256,
        stage_bytes,
    );
    write_member(
        &root,
        NativeMaintenanceRetirementKindV2::RevisionTerminalGap,
        &gap_identity,
        &gap_bytes,
    );

    let entries = collect_native_maintenance_revision_retirement_v2(&root, &codec, &gap_codec)
        .expect("revision retirement graph");
    assert_eq!(entries.len(), 5);
    assert_eq!(
        entries.iter().map(|entry| entry.kind).collect::<Vec<_>>(),
        vec![
            NativeMaintenanceRetirementKindV2::RevisionCommitReceipt,
            NativeMaintenanceRetirementKindV2::RevisionFrontierSuccessor,
            NativeMaintenanceRetirementKindV2::RevisionProjection,
            NativeMaintenanceRetirementKindV2::RevisionStageReceipt,
            NativeMaintenanceRetirementKindV2::RevisionTerminalGap,
        ]
    );
    assert_eq!(entries[4].identity_sha256, gap_identity);
    assert_eq!(
        entries[4].file_sha256,
        format_digest(codec.sha256(&gap_bytes))
    );

    fs::remove_dir_all(root).expect("remove fixture");
}

/// Create one isolated disposable evidence root without depending on a UUID package.
fn fixture_root() -> PathBuf {
    let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "weather-native-maintenance-revision-v2-{}-{sequence}",
        std::process::id()
    ));
    // remove only this process-unique stale fixture after an interrupted local test
    if root.exists() {
        fs::remove_dir_all(&root).expect("remove stale fixture");
    }
    fs::create_dir(&root).expect("create fixture root");
    root
}

/// Write one exact private mode fixture member beneath its source-owned directory.
fn write_member(
    root: &Path,
    kind: NativeMaintenanceRetirementKindV2,
    identity: &str,
    bytes: &[u8],
) {
    let directory = root.join(kind.directory());
    fs::create_dir_all(&directory).expect("create member directory");
    let path = directory.join(format!("sha256-{identity}.json"));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .expect("create member");
    file.write_all(bytes).expect("write member");
}
