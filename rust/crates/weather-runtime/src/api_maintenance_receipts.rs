//! Bind successful private archive acknowledgements to the request that produced them.

use weather_api::{
    AdjustmentMaintenanceBodyError, AdmittedAdjustmentMaintenancePublish,
    AdmittedAdjustmentMaintenanceStage, AdmittedAdjustmentRevisionStage,
};
use weather_contracts::{
    AdjustmentMaintenancePublishReceipt, AdjustmentMaintenancePublishReceiptContractVersionEnum,
    AdjustmentMaintenanceStageReceipt, AdjustmentMaintenanceStageReceiptContractVersionEnum,
    AdjustmentRevisionProjectionKind, AdjustmentRevisionStageReceipt, FieldPresence,
};
use weather_database::Sha256Digest;

use AdjustmentMaintenancePublishReceiptContractVersionEnum as PublishVersion;
use AdjustmentMaintenanceStageReceiptContractVersionEnum as StageVersion;

/// Borrowed fields in the source-defined order of a receipt or logical-key preimage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenanceReceiptScalar<'a> {
    Text(&'a str),
    Boolean(bool),
    TextArray(&'a [String]),
    Unsigned(u64),
    Null,
}

/// The selected runtime owns UTC parsing, canonical JSON and SHA-256.
pub trait ApiMaintenanceReceiptCanonicalValues {
    /// Admit an exact canonical UTC wire instant, returning its Date epoch milliseconds.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64>;

    /// Digest the observed raw bytes without reinterpretation.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest>;

    /// Digest the approved JSON encoding of these ordered fields followed by one LF.
    fn unsigned_receipt_sha256(
        &self,
        fields: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest>;
}

/// Admit the durable acknowledgement of the exact ordinary stage bytes.
pub fn admit_api_shadow_stage_receipt(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    input: &AdmittedAdjustmentMaintenanceStage,
    receipt: AdjustmentMaintenanceStageReceipt,
) -> Result<AdjustmentMaintenanceStageReceipt, AdjustmentMaintenanceBodyError> {
    let body = &input.relay_input;
    let comparator = comparator_digest(values, &body.comparator)?;
    let comparator = comparator.as_ref().map(Sha256Digest::as_str);
    let (version, version_wire) = match comparator {
        None => (
            StageVersion::AdjustmentShadowStageReceiptV1,
            "adjustment-shadow-stage-receipt/v1",
        ),
        Some(_) => (
            StageVersion::AdjustmentShadowStageReceiptV2,
            "adjustment-shadow-stage-receipt/v2",
        ),
    };
    // tie the acknowledgement to the staged compact body and optional comparator
    if receipt.contractVersion != version
        || !instant_not_before(values, &receipt.durableAt, &body.metadata.issuedAt)
        || receipt.dueKey != body.metadata.dueKey
        || receipt.registrationSha256 != body.metadata.registrationSha256
        || receipt.predictionBodySha256 != body.metadata.predictionBodySha256
        || receipt.sourceProjectionSha256 != body.sourceProjectionSha256
        || optional_wire_text(&receipt.comparatorSha256) != comparator
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    let fields = [
        (
            "contractVersion",
            MaintenanceReceiptScalar::Text(version_wire),
        ),
        ("durable", MaintenanceReceiptScalar::Boolean(true)),
        (
            "durableAt",
            MaintenanceReceiptScalar::Text(&receipt.durableAt),
        ),
        ("dueKey", MaintenanceReceiptScalar::Text(&receipt.dueKey)),
        (
            "predictionBodySha256",
            MaintenanceReceiptScalar::Text(&receipt.predictionBodySha256),
        ),
        (
            "registrationSha256",
            MaintenanceReceiptScalar::Text(&receipt.registrationSha256),
        ),
        (
            "sourceProjectionSha256",
            MaintenanceReceiptScalar::Text(&receipt.sourceProjectionSha256),
        ),
        (
            "comparatorSha256",
            MaintenanceReceiptScalar::Text(comparator.unwrap_or_default()),
        ),
    ];
    let unsigned = &fields[..fields.len() - usize::from(comparator.is_none())];
    admit_unsigned_digest(values, unsigned, &receipt.stageReceiptSha256)?;
    Ok(receipt)
}

/// Admit the committed acknowledgement of the exact database-admitted ordinary publication.
pub fn admit_api_shadow_publish_receipt(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    input: &AdmittedAdjustmentMaintenancePublish,
    receipt: AdjustmentMaintenancePublishReceipt,
) -> Result<AdjustmentMaintenancePublishReceipt, AdjustmentMaintenanceBodyError> {
    let body = &input.relay_input;
    let comparator = comparator_digest(values, &body.comparator)?;
    let comparator = comparator.as_ref().map(Sha256Digest::as_str);
    let (version, version_wire) = match comparator {
        None => (
            PublishVersion::AdjustmentShadowPublishReceiptV1,
            "adjustment-shadow-publish-receipt/v1",
        ),
        Some(_) => (
            PublishVersion::AdjustmentShadowPublishReceiptV2,
            "adjustment-shadow-publish-receipt/v2",
        ),
    };
    // bind the publish receipt to the already admitted compact prediction identity
    if receipt.contractVersion != version
        || !instant_not_before(values, &receipt.committedAt, &body.metadata.issuedAt)
        || receipt.predictionSha256 != body.metadata.predictionSha256
        || optional_wire_text(&receipt.comparatorSha256) != comparator
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
            MaintenanceReceiptScalar::Text(version_wire),
        ),
        (
            "predictionSha256",
            MaintenanceReceiptScalar::Text(&receipt.predictionSha256),
        ),
        (
            "comparatorSha256",
            MaintenanceReceiptScalar::Text(comparator.unwrap_or_default()),
        ),
    ];
    let unsigned = &fields[..fields.len() - usize::from(comparator.is_none())];
    admit_unsigned_digest(values, unsigned, &receipt.publishReceiptSha256)?;
    Ok(receipt)
}

/// Admit the durable acknowledgement of the exact raw normalized revision projection.
pub fn admit_api_revision_stage_receipt(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    input: &AdmittedAdjustmentRevisionStage,
    receipt: AdjustmentRevisionStageReceipt,
) -> Result<AdjustmentRevisionStageReceipt, AdjustmentMaintenanceBodyError> {
    verify_api_revision_stage_receipt(values, &input.projection, input.projection_kind, &receipt)?;
    Ok(receipt)
}

/// Verify an existing receipt against the submitted projection without cloning its bytes.
pub(crate) fn verify_api_revision_stage_receipt(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    projection: &[u8],
    kind: AdjustmentRevisionProjectionKind,
    receipt: &AdjustmentRevisionStageReceipt,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    let identity = values
        .sha256(projection)
        .ok_or(AdjustmentMaintenanceBodyError)?;
    verify_revision_stage_receipt_for_identity(values, identity.as_str(), kind, receipt)
}

/// Verify the original ordered stage receipt against an already hashed projection.
pub(crate) fn verify_revision_stage_receipt_for_identity(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    identity: &str,
    kind: AdjustmentRevisionProjectionKind,
    receipt: &AdjustmentRevisionStageReceipt,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    // revision stage binds the same raw hash as both body and projection identity
    if receipt.projectionKind != kind
        || receipt.projectionIdentitySha256 != identity
        || receipt.projectionSha256 != identity
        || values
            .canonical_utc_epoch_milliseconds(&receipt.durableAt)
            .is_none()
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    let fields = [
        (
            "contractVersion",
            MaintenanceReceiptScalar::Text("adjustment-revision-stage-receipt/v1"),
        ),
        ("durable", MaintenanceReceiptScalar::Boolean(true)),
        (
            "durableAt",
            MaintenanceReceiptScalar::Text(&receipt.durableAt),
        ),
        (
            "projectionIdentitySha256",
            MaintenanceReceiptScalar::Text(&receipt.projectionIdentitySha256),
        ),
        (
            "projectionKind",
            MaintenanceReceiptScalar::Text(projection_kind_wire_value(receipt.projectionKind)),
        ),
        (
            "projectionSha256",
            MaintenanceReceiptScalar::Text(&receipt.projectionSha256),
        ),
    ];
    admit_unsigned_digest(values, &fields, &receipt.stageReceiptSha256)
}

/// Borrow an optional generated acknowledgement field without adding wire defaults.
fn optional_wire_text(value: &FieldPresence<String>) -> Option<&str> {
    match value {
        FieldPresence::Missing => None,
        FieldPresence::Value(value) => Some(value),
    }
}

/// Hash the optional exact comparator material without duplicating its buffer.
fn comparator_digest(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    bytes: &FieldPresence<Vec<u8>>,
) -> Result<Option<Sha256Digest>, AdjustmentMaintenanceBodyError> {
    match bytes {
        FieldPresence::Missing => Ok(None),
        FieldPresence::Value(bytes) => values
            .sha256(bytes)
            .map(Some)
            .ok_or(AdjustmentMaintenanceBodyError),
    }
}

/// Require the archive clock to be canonical and no earlier than the admitted issue clock.
fn instant_not_before(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    actual: &str,
    issued: &str,
) -> bool {
    values
        .canonical_utc_epoch_milliseconds(actual)
        .zip(values.canonical_utc_epoch_milliseconds(issued))
        .is_some_and(|(actual, issued)| actual >= issued)
}

/// Compare the source receipt's lowercase digest with the approved canonical receipt preimage.
fn admit_unsigned_digest(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    fields: &[(&str, MaintenanceReceiptScalar<'_>)],
    expected: &str,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    // rule out a malformed archive digest before invoking the canonical owner
    if expected.len() != 64
        || !expected
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        || !values
            .unsigned_receipt_sha256(fields)
            .is_some_and(|digest| digest.as_str() == expected)
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(())
}

/// Render the closed generated projection enum with its source wire spelling.
const fn projection_kind_wire_value(kind: AdjustmentRevisionProjectionKind) -> &'static str {
    match kind {
        AdjustmentRevisionProjectionKind::ActualBestMatch => "actual_best_match",
        AdjustmentRevisionProjectionKind::NativeSource => "native_source",
        AdjustmentRevisionProjectionKind::RainGateInput => "rain_gate_input",
        AdjustmentRevisionProjectionKind::TargetRevision => "target_revision",
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use weather_contracts::{
        AdjustmentMaintenancePublishInput, AdjustmentMaintenancePublishReceiptCommittedLiteral,
        AdjustmentMaintenanceStageInput, AdjustmentMaintenanceStageReceiptDurableLiteral,
        AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptContractVersionLiteral,
        AdjustmentRevisionCommitReceiptProjectionKindEnum,
        AdjustmentRevisionStageReceiptContractVersionLiteral,
        AdjustmentRevisionStageReceiptDurableLiteral, MaintenanceShadowPredictionMetadata,
    };

    use super::*;

    const ISSUED: &str = "2026-09-14T00:00:00.000Z";
    const DURABLE: &str = "2026-09-14T00:01:00.000Z";

    #[derive(Default)]
    struct Values {
        signed_fields: RefCell<Vec<Vec<String>>>,
    }

    impl ApiMaintenanceReceiptCanonicalValues for Values {
        /// Admit exactly the two observed fixture clocks in their logical order.
        fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
            match value {
                ISSUED => Some(1_789_344_000_000),
                DURABLE => Some(1_789_344_060_000),
                _ => None,
            }
        }

        /// Record the distinct known digest of each raw fixture body.
        fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
            match bytes {
                b"comparator" => Some(Sha256Digest::admitted("c".repeat(64))),
                b"revision" => Some(Sha256Digest::admitted("d".repeat(64))),
                _ => None,
            }
        }

        /// Observe the unsigned preimage order at the approved serialization boundary.
        fn unsigned_receipt_sha256(
            &self,
            fields: &[(&str, MaintenanceReceiptScalar<'_>)],
        ) -> Option<Sha256Digest> {
            self.signed_fields
                .borrow_mut()
                .push(fields.iter().map(|(key, _)| (*key).to_owned()).collect());
            Some(Sha256Digest::admitted("e".repeat(64)))
        }
    }

    /// Keep the Node field order and comparator identity in both ordinary acknowledgements.
    #[test]
    fn validates_shadow_acknowledgements_without_reinterpreting_body_bytes() {
        let values = Values::default();
        let metadata = metadata();
        let stage = admit_api_shadow_stage_receipt(
            &values,
            &AdmittedAdjustmentMaintenanceStage {
                relay_input: AdjustmentMaintenanceStageInput {
                    body: b"prediction".to_vec(),
                    comparator: FieldPresence::Value(b"comparator".to_vec()),
                    metadata: metadata.clone(),
                    sourceProjection: b"source".to_vec(),
                    sourceProjectionSha256: "f".repeat(64),
                },
            },
            AdjustmentMaintenanceStageReceipt {
                comparatorSha256: FieldPresence::Value("c".repeat(64)),
                contractVersion: StageVersion::AdjustmentShadowStageReceiptV2,
                durable: AdjustmentMaintenanceStageReceiptDurableLiteral,
                durableAt: DURABLE.to_owned(),
                dueKey: metadata.dueKey.clone(),
                predictionBodySha256: metadata.predictionBodySha256.clone(),
                registrationSha256: metadata.registrationSha256.clone(),
                sourceProjectionSha256: "f".repeat(64),
                stageReceiptSha256: "e".repeat(64),
            },
        )
        .expect("matching stage acknowledgement");
        let publish = admit_api_shadow_publish_receipt(
            &values,
            &AdmittedAdjustmentMaintenancePublish {
                relay_input: AdjustmentMaintenancePublishInput {
                    body: b"prediction".to_vec(),
                    comparator: FieldPresence::Value(b"comparator".to_vec()),
                    metadata: metadata.clone(),
                    predictionCommittedAt: FieldPresence::Value(DURABLE.to_owned()),
                    revisionReceipt: commit_receipt(&stage),
                    sourceProjection: b"source".to_vec(),
                    sourceProjectionSha256: stage.sourceProjectionSha256.clone(),
                    stageReceipt: stage,
                },
            },
            AdjustmentMaintenancePublishReceipt {
                committed: AdjustmentMaintenancePublishReceiptCommittedLiteral,
                committedAt: DURABLE.to_owned(),
                comparatorSha256: FieldPresence::Value("c".repeat(64)),
                contractVersion: PublishVersion::AdjustmentShadowPublishReceiptV2,
                predictionSha256: metadata.predictionSha256,
                publishReceiptSha256: "e".repeat(64),
            },
        )
        .expect("matching publish acknowledgement");
        assert_eq!(
            publish.contractVersion,
            PublishVersion::AdjustmentShadowPublishReceiptV2
        );
        let observed = values.signed_fields.borrow();
        assert_eq!(observed.len(), 2);
        assert_eq!(
            observed[0],
            [
                "contractVersion",
                "durable",
                "durableAt",
                "dueKey",
                "predictionBodySha256",
                "registrationSha256",
                "sourceProjectionSha256",
                "comparatorSha256",
            ]
        );
        assert_eq!(
            observed[1],
            [
                "committed",
                "committedAt",
                "contractVersion",
                "predictionSha256",
                "comparatorSha256",
            ]
        );
    }

    /// Check the original revision acknowledgement against one exact raw projection.
    #[test]
    fn validates_revision_stage_acknowledgement_in_source_order() {
        let values = Values::default();
        let receipt = AdjustmentRevisionStageReceipt {
            contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
            durable: AdjustmentRevisionStageReceiptDurableLiteral,
            durableAt: DURABLE.to_owned(),
            projectionIdentitySha256: "d".repeat(64),
            projectionKind: AdjustmentRevisionProjectionKind::ActualBestMatch,
            projectionSha256: "d".repeat(64),
            stageReceiptSha256: "e".repeat(64),
        };
        let admitted = admit_api_revision_stage_receipt(
            &values,
            &AdmittedAdjustmentRevisionStage {
                projection: b"revision".to_vec(),
                projection_kind: AdjustmentRevisionProjectionKind::ActualBestMatch,
            },
            receipt.clone(),
        );
        assert_eq!(admitted, Ok(receipt));
        assert_eq!(
            values.signed_fields.borrow()[0],
            [
                "contractVersion",
                "durable",
                "durableAt",
                "projectionIdentitySha256",
                "projectionKind",
                "projectionSha256",
            ]
        );
    }

    /// Build the complete generated compact-prediction identity for a publication fixture.
    fn metadata() -> MaintenanceShadowPredictionMetadata {
        MaintenanceShadowPredictionMetadata {
            bodyByteCount: 10,
            candidateSha256: "1".repeat(64),
            dueKey: "fixture-due".to_owned(),
            inputSha256: "2".repeat(64),
            issuedAt: ISSUED.to_owned(),
            maxValidAt: ISSUED.to_owned(),
            minValidAt: ISSUED.to_owned(),
            predictionBodySha256: "3".repeat(64),
            predictionSchemaSha256: "4".repeat(64),
            predictionSha256: "5".repeat(64),
            registrationSha256: "6".repeat(64),
            rowCount: 1,
            sourceReceiptSha256: "7".repeat(64),
            sourceSha256: "8".repeat(64),
        }
    }

    /// Build the already admitted predecessor receipt retained for publication.
    fn commit_receipt(
        stage: &AdjustmentMaintenanceStageReceipt,
    ) -> AdjustmentRevisionCommitReceipt {
        AdjustmentRevisionCommitReceipt {
            archiveCommitOrdinal: "1".to_owned(),
            archiveCommittedAt: DURABLE.to_owned(),
            contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
            frontierSha256: "1".repeat(64),
            predecessorFrontierSha256: "2".repeat(64),
            projectionIdentitySha256: "7".repeat(64),
            projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction,
            projectionSha256: "2".repeat(64),
            receiptSha256: "3".repeat(64),
            stageReceiptSha256: stage.stageReceiptSha256.clone(),
        }
    }
}
