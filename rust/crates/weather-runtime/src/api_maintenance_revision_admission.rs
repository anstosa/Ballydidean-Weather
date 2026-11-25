//! Admit staged revision projections, database receipts, and archive publish checkpoints.

use weather_api::{
    AdjustmentMaintenanceBodyError, AdjustmentRevisionPublishResult,
    AdmittedAdjustmentRevisionStage, CanonicalInstant, DecodedAdjustmentRevisionPublish,
    DecodedAdjustmentRevisionStage, PreparedAdjustmentRevisionPublish,
};
use weather_contracts::{
    AdjustmentRevisionBatchPublishReceiptProjectionKindEnum, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionProjectionKind,
};

use crate::api_maintenance_receipts::verify_api_revision_stage_receipt;
use crate::{ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar};

use crate::ApiMaintenanceRevisionTransportDocument;
use crate::api_revision_projection::{RevisionRequestShape, project_revision_document};
use crate::maintenance_canonical_json::valid_hash;

/// Approved canonical document decoding plus the existing clock and hash owners.
pub trait ApiMaintenanceRevisionEvidenceValues: ApiMaintenanceReceiptCanonicalValues {
    /// Decode only an exact original canonical document; Rust derives its clocks and SQL keys.
    fn decode_revision_document(
        &self,
        projection: &[u8],
    ) -> Option<ApiMaintenanceRevisionTransportDocument>;
}

/// Admit canonical revision bytes and every source clock before archive staging.
pub fn admit_api_revision_stage(
    values: &impl ApiMaintenanceRevisionEvidenceValues,
    epoch: &CanonicalInstant,
    request: DecodedAdjustmentRevisionStage,
) -> Result<AdmittedAdjustmentRevisionStage, AdjustmentMaintenanceBodyError> {
    let decoded = values
        .decode_revision_document(&request.projection)
        .ok_or(AdjustmentMaintenanceBodyError)?;
    let evidence = project_revision_document(&decoded, RevisionRequestShape::Stage, epoch, values)?;
    Ok(AdmittedAdjustmentRevisionStage {
        projection: request.projection,
        projection_kind: evidence.kind,
    })
}

/// Prepare one single or grouped publication after stage and commit-receipt admission.
pub fn prepare_api_revision_publish(
    values: &impl ApiMaintenanceRevisionEvidenceValues,
    epoch: &CanonicalInstant,
    request: DecodedAdjustmentRevisionPublish,
) -> Result<PreparedAdjustmentRevisionPublish, AdjustmentMaintenanceBodyError> {
    let (shape, projection, stage_receipt, revision_receipts) = match &request {
        DecodedAdjustmentRevisionPublish::Single {
            projection,
            revision_receipt,
            stage_receipt,
        } => (
            RevisionRequestShape::Single,
            projection.as_slice(),
            stage_receipt,
            std::slice::from_ref(revision_receipt),
        ),
        DecodedAdjustmentRevisionPublish::Batch {
            projection,
            revision_receipts,
            stage_receipt,
        } => (
            RevisionRequestShape::Grouped,
            projection.as_slice(),
            stage_receipt,
            revision_receipts.as_slice(),
        ),
    };
    let decoded = values
        .decode_revision_document(projection)
        .ok_or(AdjustmentMaintenanceBodyError)?;
    let evidence = project_revision_document(&decoded, shape, epoch, values)?;
    // preserve Node's stage-before-database-receipt admission order
    verify_api_revision_stage_receipt(values, projection, evidence.kind, stage_receipt)?;

    // require one caller receipt for every independently projected canonical pointer
    if evidence.pointers.is_empty() || evidence.pointers.len() != revision_receipts.len() {
        return Err(AdjustmentMaintenanceBodyError);
    }
    // keep grouped receipt authority in the post-read database comparison and gap path
    if shape == RevisionRequestShape::Grouped {
        // require only every caller-supplied grouped commit clock at this transport boundary
        if revision_receipts.iter().any(|receipt| {
            !instant_is_at_or_after_epoch(values, epoch, &receipt.archiveCommittedAt)
        }) {
            return Err(AdjustmentMaintenanceBodyError);
        }
    } else {
        validate_commit_receipt(
            values,
            epoch,
            &revision_receipts[0],
            stage_receipt,
            evidence.kind,
        )?;
    }

    Ok(PreparedAdjustmentRevisionPublish {
        publication: request,
        pointers: evidence.pointers,
    })
}

/// Verify the archive's canonical commit clock and original ordered self-hash.
pub fn verify_api_revision_publish_receipt(
    values: &impl ApiMaintenanceRevisionEvidenceValues,
    result: &AdjustmentRevisionPublishResult,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    match result {
        AdjustmentRevisionPublishResult::Single(receipt) => {
            // require the source canonical clock and every output identity spelling
            if values
                .canonical_utc_epoch_milliseconds(&receipt.committedAt)
                .is_none()
                || !valid_hash(&receipt.projectionIdentitySha256)
                || !valid_hash(&receipt.projectionSha256)
                || !valid_hash(&receipt.revisionReceiptSha256)
            {
                return Err(AdjustmentMaintenanceBodyError);
            }
            let fields = [
                ("committed", MaintenanceReceiptScalar::Boolean(true)),
                (
                    "committedAt",
                    MaintenanceReceiptScalar::Text(&receipt.committedAt),
                ),
                (
                    "contractVersion",
                    MaintenanceReceiptScalar::Text("adjustment-revision-publish-receipt/v1"),
                ),
                (
                    "projectionIdentitySha256",
                    MaintenanceReceiptScalar::Text(&receipt.projectionIdentitySha256),
                ),
                (
                    "projectionKind",
                    MaintenanceReceiptScalar::Text(projection_kind_wire_value(
                        receipt.projectionKind,
                    )),
                ),
                (
                    "projectionSha256",
                    MaintenanceReceiptScalar::Text(&receipt.projectionSha256),
                ),
                (
                    "revisionReceiptSha256",
                    MaintenanceReceiptScalar::Text(&receipt.revisionReceiptSha256),
                ),
            ];
            admit_publish_digest(values, &fields, &receipt.publishReceiptSha256)
        }
        AdjustmentRevisionPublishResult::Batch(receipt) => {
            // require the canonical clock and every grouped checkpoint identity
            if values
                .canonical_utc_epoch_milliseconds(&receipt.committedAt)
                .is_none()
                || !valid_hash(&receipt.projectionIdentitySha256)
                || !valid_hash(&receipt.projectionSha256)
                || receipt.revisionReceiptSha256s.is_empty()
                || receipt
                    .revisionReceiptSha256s
                    .iter()
                    .any(|identity| !valid_hash(identity))
            {
                return Err(AdjustmentMaintenanceBodyError);
            }
            let fields = [
                ("committed", MaintenanceReceiptScalar::Boolean(true)),
                (
                    "committedAt",
                    MaintenanceReceiptScalar::Text(&receipt.committedAt),
                ),
                (
                    "contractVersion",
                    MaintenanceReceiptScalar::Text("adjustment-revision-batch-publish-receipt/v2"),
                ),
                (
                    "projectionIdentitySha256",
                    MaintenanceReceiptScalar::Text(&receipt.projectionIdentitySha256),
                ),
                (
                    "projectionKind",
                    MaintenanceReceiptScalar::Text(batch_projection_kind_wire_value(
                        receipt.projectionKind,
                    )),
                ),
                (
                    "projectionSha256",
                    MaintenanceReceiptScalar::Text(&receipt.projectionSha256),
                ),
                (
                    "revisionReceiptSha256s",
                    MaintenanceReceiptScalar::TextArray(&receipt.revisionReceiptSha256s),
                ),
            ];
            admit_publish_digest(values, &fields, &receipt.publishReceiptSha256)
        }
    }
}

/// Bind one database-assigned commit receipt to the exact staged projection.
fn validate_commit_receipt(
    values: &impl ApiMaintenanceRevisionEvidenceValues,
    epoch: &CanonicalInstant,
    receipt: &AdjustmentRevisionCommitReceipt,
    stage: &weather_contracts::AdjustmentRevisionStageReceipt,
    projection_kind: AdjustmentRevisionProjectionKind,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    // retain only positive unbounded decimal ordinals like the source BigInt path
    if !positive_decimal(&receipt.archiveCommitOrdinal)
        || !instant_is_at_or_after_epoch(values, epoch, &receipt.archiveCommittedAt)
        || !commit_kind_matches(receipt.projectionKind, projection_kind)
        || receipt.projectionIdentitySha256 != stage.projectionIdentitySha256
        || receipt.projectionSha256 != stage.projectionSha256
        || receipt.stageReceiptSha256 != stage.stageReceiptSha256
        || !valid_hash(&receipt.frontierSha256)
        || !valid_hash(&receipt.predecessorFrontierSha256)
        || !valid_hash(&receipt.receiptSha256)
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(())
}

/// Recognize the source's canonical positive decimal ordinal grammar.
fn positive_decimal(value: &str) -> bool {
    !value.is_empty()
        && value.as_bytes()[0] != b'0'
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Admit one canonical instant at or after the inclusive capture epoch.
fn instant_is_at_or_after_epoch(
    values: &impl ApiMaintenanceRevisionEvidenceValues,
    epoch: &CanonicalInstant,
    value: &str,
) -> bool {
    values
        .canonical_utc_epoch_milliseconds(value)
        .is_some_and(|value| value >= epoch.epoch_milliseconds)
}

/// Match the commit-receipt projection enum to the decoded document class.
const fn commit_kind_matches(
    receipt: AdjustmentRevisionCommitReceiptProjectionKindEnum,
    projection: AdjustmentRevisionProjectionKind,
) -> bool {
    matches!(
        (receipt, projection),
        (
            AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch,
            AdjustmentRevisionProjectionKind::ActualBestMatch
        ) | (
            AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource,
            AdjustmentRevisionProjectionKind::NativeSource
        ) | (
            AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput,
            AdjustmentRevisionProjectionKind::RainGateInput
        ) | (
            AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision,
            AdjustmentRevisionProjectionKind::TargetRevision
        )
    )
}

/// Admit the publish receipt's lowercase digest against the approved ordered JSON owner.
fn admit_publish_digest(
    values: &impl ApiMaintenanceRevisionEvidenceValues,
    fields: &[(&str, MaintenanceReceiptScalar<'_>)],
    expected: &str,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    // reject malformed output identities before invoking the canonical encoder
    if !valid_hash(expected)
        || !values
            .unsigned_receipt_sha256(fields)
            .is_some_and(|digest| digest.as_str() == expected)
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(())
}

/// Render one single publication projection kind with its exact source spelling.
const fn projection_kind_wire_value(kind: AdjustmentRevisionProjectionKind) -> &'static str {
    match kind {
        AdjustmentRevisionProjectionKind::ActualBestMatch => "actual_best_match",
        AdjustmentRevisionProjectionKind::NativeSource => "native_source",
        AdjustmentRevisionProjectionKind::RainGateInput => "rain_gate_input",
        AdjustmentRevisionProjectionKind::TargetRevision => "target_revision",
    }
}

/// Render one grouped publication projection kind with its exact source spelling.
const fn batch_projection_kind_wire_value(
    kind: AdjustmentRevisionBatchPublishReceiptProjectionKindEnum,
) -> &'static str {
    match kind {
        AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::ActualBestMatch => {
            "actual_best_match"
        }
        AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::TargetRevision => {
            "target_revision"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_contracts::{
        AdjustmentRevisionBatchProjection, AdjustmentRevisionBatchProjectionContractVersionLiteral,
        AdjustmentRevisionBatchProjectionFamilyEnum,
        AdjustmentRevisionBatchProjectionLogicalKeyValue,
        AdjustmentRevisionBatchProjectionProjectionKindEnum,
        AdjustmentRevisionBatchProjectionRowsValue,
        AdjustmentRevisionCommitReceiptContractVersionLiteral,
        AdjustmentRevisionProjectionDocument, AdjustmentRevisionStageReceipt,
        AdjustmentRevisionStageReceiptContractVersionLiteral,
        AdjustmentRevisionStageReceiptDurableLiteral, Nullable,
    };
    use weather_database::{AdjustmentRevisionAdmissionKey, Sha256Digest};

    const FIRST: &str = "2026-10-08T16:00:00.000Z";
    const SECOND: &str = "2026-10-08T17:00:00.000Z";
    // compiled original Node adjustmentRevisionLogicalKeySha256 output for the two rows
    const FIRST_KEY: &str = "8f5fc09749e43c40e5bfda6f9a51a4815fcc20b1ad2838165adf038d08bc9a17";
    const SECOND_KEY: &str = "34fdedc7a111f84c4f45f324b5a77e882aa39b4830433cb7b025e5383a27254e";

    struct Values;

    impl ApiMaintenanceReceiptCanonicalValues for Values {
        /// Preserve the fixture's exact inclusive epoch clock.
        fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
            match value {
                FIRST => Some(0),
                SECOND => Some(3_600_000),
                _ => None,
            }
        }

        /// Bind the exactly observed stage bytes.
        fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
            (bytes == b"projection").then(|| Sha256Digest::admitted("a".repeat(64)))
        }

        /// Admit only the ordered field arrays used by the compiled Node keys or the fixture stage.
        fn unsigned_receipt_sha256(
            &self,
            fields: &[(&str, MaintenanceReceiptScalar<'_>)],
        ) -> Option<Sha256Digest> {
            use MaintenanceReceiptScalar::{Null, Text};
            // check the exact original JSON object order before supplying its Node digest
            let digest = match fields {
                [
                    ("productRunAt", Null),
                    ("sourceId", Text("42")),
                    ("sourceKind", Text("physical_sensor")),
                    ("validAt", Text(FIRST)),
                ] => FIRST_KEY.to_owned(),
                [
                    ("productRunAt", Null),
                    ("sourceId", Text("42")),
                    ("sourceKind", Text("physical_sensor")),
                    ("validAt", Text(SECOND)),
                ] => SECOND_KEY.to_owned(),
                [
                    ("contractVersion", Text("adjustment-revision-stage-receipt/v1")),
                    ..,
                ] => "b".repeat(64),
                _ => return None,
            };
            Some(Sha256Digest::admitted(digest))
        }
    }

    impl ApiMaintenanceRevisionEvidenceValues for Values {
        /// Return only the original typed document, without database keys or epoch proof.
        fn decode_revision_document(
            &self,
            projection: &[u8],
        ) -> Option<ApiMaintenanceRevisionTransportDocument> {
            use AdjustmentRevisionBatchProjectionLogicalKeyValue as KeyValue;
            use AdjustmentRevisionBatchProjectionRowsValue as RowValue;
            assert_eq!(projection, b"projection");
            Some(ApiMaintenanceRevisionTransportDocument::Standard(Box::new(
                AdjustmentRevisionProjectionDocument::Batch(AdjustmentRevisionBatchProjection {
                    contractVersion: AdjustmentRevisionBatchProjectionContractVersionLiteral,
                    family: AdjustmentRevisionBatchProjectionFamilyEnum::Shared,
                    logicalKey: [
                        ("productRunAt", KeyValue::Null),
                        ("sourceId", KeyValue::String("42".encode_utf16().collect())),
                        (
                            "sourceKind",
                            KeyValue::String("physical_sensor".encode_utf16().collect()),
                        ),
                        ("validAt", KeyValue::String(FIRST.encode_utf16().collect())),
                    ]
                    .into_iter()
                    .map(|(key, value)| (key.encode_utf16().collect(), value))
                    .collect(),
                    logicalReceivedAt: FIRST.to_owned(),
                    projectionKind:
                        AdjustmentRevisionBatchProjectionProjectionKindEnum::TargetRevision,
                    rows: [(FIRST, "c"), (SECOND, "d")]
                        .into_iter()
                        .map(|(clock, hash)| {
                            [
                                (
                                    "contentSha256".encode_utf16().collect(),
                                    RowValue::String(hash.repeat(64).encode_utf16().collect()),
                                ),
                                (
                                    "validAt".encode_utf16().collect(),
                                    RowValue::String(clock.encode_utf16().collect()),
                                ),
                            ]
                            .into_iter()
                            .collect()
                        })
                        .collect(),
                    source: Vec::new(),
                    storedContentSha256: "c".repeat(64),
                }),
            )))
        }
    }

    /// Leave stale caller group state to persisted admission so the ordinary gap can be recorded.
    #[test]
    fn grouped_publication_at_epoch_retains_pointers_for_persisted_comparison() {
        let supplied = AdjustmentRevisionCommitReceipt {
            archiveCommitOrdinal: "1".to_owned(),
            archiveCommittedAt: FIRST.to_owned(),
            contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
            frontierSha256: "d".repeat(64),
            predecessorFrontierSha256: "e".repeat(64),
            projectionIdentitySha256: "f".repeat(64),
            projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision,
            projectionSha256: "f".repeat(64),
            receiptSha256: "d".repeat(64),
            stageReceiptSha256: "b".repeat(64),
        };
        let prepared = prepare_api_revision_publish(
            &Values,
            &CanonicalInstant {
                utc: FIRST.to_owned(),
                epoch_milliseconds: 0,
            },
            DecodedAdjustmentRevisionPublish::Batch {
                projection: b"projection".to_vec(),
                revision_receipts: vec![supplied.clone(), supplied],
                stage_receipt: AdjustmentRevisionStageReceipt {
                    contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
                    durable: AdjustmentRevisionStageReceiptDurableLiteral,
                    durableAt: FIRST.to_owned(),
                    projectionIdentitySha256: "a".repeat(64),
                    projectionKind: AdjustmentRevisionProjectionKind::TargetRevision,
                    projectionSha256: "a".repeat(64),
                    stageReceiptSha256: "b".repeat(64),
                },
            },
        )
        .expect("defer grouped receipt binding to persisted admission");
        let (pointers, receipts) = prepared.rows();
        assert_eq!(pointers.len(), 2);
        assert_eq!(receipts[0].projectionSha256, "f".repeat(64));
        assert_eq!(pointers[0].logical_key_sha256, FIRST_KEY);
        assert_eq!(pointers[1].logical_key_sha256, SECOND_KEY);
        let AdjustmentRevisionAdmissionKey::WeatherRecord(key) = &pointers[1].key else {
            panic!("weather row")
        };
        assert_eq!(key.sourceId, "42");
        assert_eq!(key.productRunAt, Nullable::Null);
        assert_eq!(key.validAt, SECOND);
        assert_eq!(key.storedContentSha256, "d".repeat(64));
    }
}
