//! Ordinary request-order and failure coverage for private prediction publication.

use super::*;
use crate::MaintenanceReceiptScalar;
use weather_contracts::{
    AdjustmentMaintenancePublishReceiptCommittedLiteral,
    AdjustmentMaintenancePublishReceiptContractVersionEnum,
    AdjustmentMaintenanceStageReceiptContractVersionEnum,
    AdjustmentMaintenanceStageReceiptDurableLiteral, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum,
};
use weather_database::{ApiMaintenanceRegistrationVersion, Sha256Digest};

const ISSUED: &str = "2026-09-14T00:00:00.000Z";
const COMMITTED: &str = "2026-09-14T00:01:00.000Z";
const REGISTRATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct ReceiptValues;

impl ApiMaintenanceReceiptCanonicalValues for ReceiptValues {
    /// Supply the two known fixture clocks without a runtime dependency.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        match value {
            ISSUED => Some(0),
            COMMITTED => Some(60_000),
            _ => None,
        }
    }

    /// Return the fixture comparator identity for its exact observed bytes.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        (bytes == b"comparator").then(|| Sha256Digest::admitted("b".repeat(64)))
    }

    /// Represent receipt hashing at the approved serializer boundary.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        Some(Sha256Digest::admitted("c".repeat(64)))
    }
}

#[derive(Default)]
struct Ports {
    steps: Vec<&'static str>,
    stop: Option<GapReason>,
}

impl ApiMaintenancePredictionPorts for Ports {
    /// Observe the full stage payload before any compact write.
    fn stage(
        &mut self,
        input: &AdmittedAdjustmentMaintenanceStage,
    ) -> Result<AdjustmentMaintenanceStageReceipt, OperationError> {
        self.steps.push("stage");
        // simulate the archive relay being unavailable
        if self.stop == Some(GapReason::ArchiveStageFailed) {
            return Err(OperationError::Rejected);
        }
        let body = &input.relay_input;
        assert_eq!(body.body, b"prediction");
        assert_eq!(body.sourceProjection, b"source");
        assert_eq!(
            body.comparator,
            FieldPresence::Value(b"comparator".to_vec())
        );
        Ok(AdjustmentMaintenanceStageReceipt {
            comparatorSha256: FieldPresence::Value("b".repeat(64)),
            contractVersion:
                AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV2,
            durable: AdjustmentMaintenanceStageReceiptDurableLiteral,
            durableAt: COMMITTED.to_owned(),
            dueKey: body.metadata.dueKey.clone(),
            predictionBodySha256: body.metadata.predictionBodySha256.clone(),
            registrationSha256: body.metadata.registrationSha256.clone(),
            sourceProjectionSha256: body.sourceProjectionSha256.clone(),
            stageReceiptSha256: "c".repeat(64),
        })
    }

    /// Assert the original compact write carries the durable stage identity.
    fn register_and_append(
        &mut self,
        registration: &AdmittedApiMaintenanceRegistration<'_>,
        prediction: &AdjustmentShadowPrediction,
    ) -> Result<AdjustmentShadowPredictionReceipt, OperationError> {
        self.steps.push("append");
        assert_eq!(
            prediction.registrationSha256,
            registration.registration_sha256
        );
        assert_eq!(prediction.stageReceiptSha256, "c".repeat(64));
        // simulate a rejected compact write
        if self.stop == Some(GapReason::DatabaseAppendFailed) {
            return Err(OperationError::Rejected);
        }
        Ok(AdjustmentShadowPredictionReceipt {
            committedAt: COMMITTED.to_owned(),
            inserted: false,
            predictionSha256: prediction.predictionSha256.clone(),
            revisionReceipt: AdjustmentRevisionCommitReceipt {
                archiveCommitOrdinal: "1".to_owned(),
                archiveCommittedAt: COMMITTED.to_owned(),
                contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
                frontierSha256: "1".repeat(64),
                predecessorFrontierSha256: "2".repeat(64),
                projectionIdentitySha256: prediction.sourceReceiptSha256.clone(),
                projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction,
                projectionSha256: prediction.inputSha256.clone(),
                receiptSha256: "3".repeat(64),
                stageReceiptSha256: prediction.stageReceiptSha256.clone(),
            },
        })
    }

    /// Simulate independent database admission with both original failure classes.
    fn admit(
        &mut self,
        prepared: PreparedAdjustmentMaintenancePublish,
    ) -> Result<AdmittedAdjustmentMaintenancePublish, ShadowPublishAdmissionError> {
        self.steps.push("admit");
        assert_eq!(
            prepared.relay_input.predictionCommittedAt,
            FieldPresence::Value(COMMITTED.to_owned())
        );
        assert_eq!(
            prepared.relay_input.revisionReceipt.stageReceiptSha256,
            prepared.relay_input.stageReceipt.stageReceiptSha256
        );
        match self.stop {
            Some(GapReason::DatabaseAdmissionFailed) => {
                Err(ShadowPublishAdmissionError::NotAdmitted)
            }
            Some(GapReason::ArchivePublishFailed) => {
                Err(ShadowPublishAdmissionError::PreliminaryRead)
            }
            _ => Ok(AdmittedAdjustmentMaintenancePublish {
                relay_input: prepared.relay_input,
            }),
        }
    }

    /// Acknowledge the same staged payload once the stored receipt is admitted.
    fn publish(
        &mut self,
        admitted: &AdmittedAdjustmentMaintenancePublish,
    ) -> Result<AdjustmentMaintenancePublishReceipt, OperationError> {
        self.steps.push("publish");
        let input = &admitted.relay_input;
        assert_eq!(input.body, b"prediction");
        assert_eq!(input.sourceProjection, b"source");
        assert_eq!(
            input.comparator,
            FieldPresence::Value(b"comparator".to_vec())
        );
        Ok(AdjustmentMaintenancePublishReceipt {
            committed: AdjustmentMaintenancePublishReceiptCommittedLiteral,
            committedAt: COMMITTED.to_owned(),
            comparatorSha256: FieldPresence::Value("b".repeat(64)),
            contractVersion: AdjustmentMaintenancePublishReceiptContractVersionEnum::AdjustmentShadowPublishReceiptV2,
            predictionSha256: input.metadata.predictionSha256.clone(),
            publishReceiptSha256: "c".repeat(64),
        })
    }

    /// Observe a best-effort gap when the archive callback is unavailable.
    fn record_gap(&mut self, gap: AdmittedAdjustmentMaintenanceGap) -> Result<(), OperationError> {
        self.steps.push("gap");
        assert_eq!(gap.request.registrationSha256, REGISTRATION);
        Err(OperationError::Rejected)
    }
}

/// Build a staged prediction fixture with distinct original request identities.
fn stage() -> AdmittedAdjustmentMaintenanceStage {
    AdmittedAdjustmentMaintenanceStage {
        relay_input: AdjustmentMaintenanceStageInput {
            body: b"prediction".to_vec(),
            comparator: FieldPresence::Value(b"comparator".to_vec()),
            metadata: MaintenanceShadowPredictionMetadata {
                bodyByteCount: 10,
                candidateSha256: "1".repeat(64),
                dueKey: "2026-09-14T00:00:00Z".to_owned(),
                inputSha256: "2".repeat(64),
                issuedAt: ISSUED.to_owned(),
                maxValidAt: ISSUED.to_owned(),
                minValidAt: ISSUED.to_owned(),
                predictionBodySha256: "3".repeat(64),
                predictionSchemaSha256: "4".repeat(64),
                predictionSha256: "5".repeat(64),
                registrationSha256: REGISTRATION.to_owned(),
                rowCount: 168,
                sourceReceiptSha256: "7".repeat(64),
                sourceSha256: "8".repeat(64),
            },
            sourceProjection: b"source".to_vec(),
            sourceProjectionSha256: "2".repeat(64),
        },
    }
}

/// Select the corresponding admitted wind registration fixture.
fn registration() -> AdmittedApiMaintenanceRegistration<'static> {
    AdmittedApiMaintenanceRegistration {
        family: AdjustmentShadowFamily::Wind,
        version: ApiMaintenanceRegistrationVersion::V3,
        registration_sha256: REGISTRATION,
        candidate_sha256: "fixture-candidate",
        source_sha256: "fixture-source",
        canonical_json: "{}",
    }
}

/// Prove a staged prediction follows the retained independent admission before publish.
#[test]
fn publishes_only_after_staging_compact_append_and_admission() {
    let mut ports = Ports::default();
    let result = stage_append_publish(stage(), &registration(), &mut ports, &ReceiptValues)
        .expect("matching admitted publication");
    assert_eq!(ports.steps, ["stage", "append", "admit", "publish"]);
    assert_eq!(result.predictionSha256, "5".repeat(64));
    assert_eq!(result.publishReceiptSha256, "c".repeat(64));
}

/// Preserve the failure taxonomy and skip every operation after its failed prerequisite.
#[test]
fn stops_at_the_original_gap_boundary() {
    // cover the ordinary unavailable stages independently
    for (reason, steps) in [
        (GapReason::ArchiveStageFailed, &["stage"][..]),
        (GapReason::DatabaseAppendFailed, &["stage", "append"][..]),
        (
            GapReason::DatabaseAdmissionFailed,
            &["stage", "append", "admit"][..],
        ),
        (
            GapReason::ArchivePublishFailed,
            &["stage", "append", "admit"][..],
        ),
    ] {
        let mut ports = Ports {
            stop: Some(reason),
            ..Ports::default()
        };
        assert_eq!(
            stage_append_publish(stage(), &registration(), &mut ports, &ReceiptValues),
            Err(reason)
        );
        assert_eq!(ports.steps, steps);
    }
}

/// An unavailable gap archive must not change the ordinary returned disposition.
#[test]
fn returns_categorical_gap_even_when_archive_is_unavailable() {
    let mut ports = Ports::default();
    let registration = registration();
    let reason = GapReason::ArchiveStageFailed;
    let outcome = GapContext::from_registration(&registration, "fixture-due")
        .expect("wind registration")
        .record(&mut ports, reason);
    assert_eq!(ports.steps, ["gap"]);
    assert_eq!(
        outcome,
        CaptureResult::Gap(CaptureApiMaintenanceShadowResultGap {
            reason,
            status: CaptureApiMaintenanceShadowResultGapStatusLiteral,
        })
    );
}
