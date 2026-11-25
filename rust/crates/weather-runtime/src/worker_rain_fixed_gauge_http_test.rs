//! Typed fixed-gauge route coverage for the credential-free maintenance client.

use super::*;
use weather_contracts::{
    AdjustmentRevisionStageReceiptContractVersionLiteral,
    AdjustmentRevisionStageReceiptDurableLiteral,
    RainFixedGaugeTargetGapReceiptContractVersionLiteral,
    RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral,
    RainFixedGaugeTargetGapStatusAbsent, RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
    RainFixedGaugeTargetGapStatusAbsentStateLiteral,
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HOUR: &str = "2026-11-24T09:00:00.000Z";

struct Origin;

impl WorkerRainMaintenanceOriginOwner for Origin {
    type Error = ();

    /// Admit only the fixture private API origin.
    fn admit_worker_rain_maintenance_origin(&mut self, raw: &str) -> Result<String, Self::Error> {
        assert_eq!(raw, "http://api");
        Ok("http://api/".to_owned())
    }
}

#[derive(Default)]
struct Owner {
    paths: Vec<&'static str>,
    published_ordinals: Vec<String>,
}

impl WorkerRainMaintenanceHttpJson for Owner {
    type Error = ();

    /// Make each projection byte sequence observable without implementing base64.
    fn base64(&mut self, bytes: &[u8]) -> Result<String, Self::Error> {
        Ok(format!("base64:{}", String::from_utf8_lossy(bytes)))
    }

    /// Observe each typed request and return its generated success response.
    fn post_json(
        &mut self,
        post: WorkerRainMaintenancePost<'_>,
    ) -> Result<WorkerRainMaintenanceHttpOutcome, Self::Error> {
        assert_eq!(post.origin.as_str(), "http://api/");
        self.paths.push(post.path);
        let response = match post.body {
            WorkerRainMaintenanceRequest::FixedGaugeGapStatus(input) => {
                assert_eq!(input.logicalHourAt, HOUR);
                assert_eq!(input.logicalKeySha256, HASH_A);
                WorkerRainMaintenanceResponse::FixedGaugeGapStatus(
                    RainFixedGaugeTargetGapStatus::Absent(RainFixedGaugeTargetGapStatusAbsent {
                        contractVersion: RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
                        state: RainFixedGaugeTargetGapStatusAbsentStateLiteral,
                    }),
                )
            }
            WorkerRainMaintenanceRequest::FixedGaugeGap(input) => {
                assert_eq!(input.logicalHourAt, HOUR);
                assert_eq!(input.logicalKeySha256, HASH_A);
                WorkerRainMaintenanceResponse::FixedGaugeGap(RainFixedGaugeTargetGapReceipt {
                    logicalHourAt: HOUR.to_owned(),
                    logicalKeySha256: HASH_A.to_owned(),
                    reason: RainFixedGaugeTargetGapInputReasonLiteral,
                    contractVersion: RainFixedGaugeTargetGapReceiptContractVersionLiteral,
                    gapAt: "2026-11-24T09:01:00.000Z".to_owned(),
                    gapSha256: HASH_B.to_owned(),
                    qualificationDisposition:
                        RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral,
                })
            }
            WorkerRainMaintenanceRequest::RevisionStage(body) => {
                assert_eq!(body.projectionBase64, "base64:projection");
                WorkerRainMaintenanceResponse::RevisionStage(stage_wire())
            }
            WorkerRainMaintenanceRequest::RevisionBatchPublish(body) => {
                assert_eq!(body.projectionBase64, "base64:projection");
                assert_eq!(body.revisionReceipts.len(), 12);
                self.published_ordinals = body
                    .revisionReceipts
                    .iter()
                    .map(|receipt| receipt.archiveCommitOrdinal.clone())
                    .collect();
                assert_eq!(
                    body.stageReceipt.projectionKind,
                    AdjustmentRevisionProjectionKind::TargetRevision
                );
                WorkerRainMaintenanceResponse::ParsedJson
            }
            _ => panic!("fixed-gauge fixture received another maintenance request"),
        };
        Ok(WorkerRainMaintenanceHttpOutcome::Success(response))
    }

    /// This successful fixture never waits for archive capacity.
    fn wait_for_revision_archive_capacity(&mut self, _: u64) -> Result<(), Self::Error> {
        panic!("fixed-gauge fixture unexpectedly retried")
    }
}

/// Prove gap status, gap record, stage, and ordered group publication use their typed routes.
#[test]
fn routes_fixed_gauge_requests_and_preserves_all_twelve_receipts() {
    let mut client =
        HttpWorkerRainMaintenanceClient::admit("http://api", &mut Origin, Owner::default())
            .unwrap();

    assert_eq!(
        client.read_oversized_gap(HOUR, HASH_A).unwrap(),
        RainFixedGaugeGapStatus::Absent
    );
    let gap = client.record_oversized_gap(HOUR, HASH_A).unwrap();
    assert_eq!(gap.gap_sha256, HASH_B);
    assert_eq!(gap.reason, RainFixedGaugeGapReason::TargetSourceOversized);

    let stage = RainFixedGaugeClient::stage_revision(&mut client, b"projection").unwrap();
    assert_eq!(stage.projection_identity_sha256, HASH_A);
    let receipts = (1..=12).map(commit_receipt).collect::<Vec<_>>();
    RainFixedGaugeClient::publish_revision_batch(&mut client, b"projection", &stage, &receipts)
        .unwrap();

    let owner = client.into_owner();
    assert_eq!(
        owner.paths,
        [
            FIXED_GAUGE_GAP_STATUS_PATH,
            FIXED_GAUGE_GAP_PATH,
            REVISION_STAGE_PATH,
            REVISION_PUBLISH_PATH,
        ]
    );
    assert_eq!(
        owner.published_ordinals,
        (1..=12)
            .map(|ordinal| ordinal.to_string())
            .collect::<Vec<_>>()
    );
}

/// Build the generated target-revision stage response returned by the private API.
fn stage_wire() -> AdjustmentRevisionStageReceipt {
    AdjustmentRevisionStageReceipt {
        contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
        durable: AdjustmentRevisionStageReceiptDurableLiteral,
        durableAt: "2026-11-24T09:02:00.000Z".to_owned(),
        projectionIdentitySha256: HASH_A.to_owned(),
        projectionKind: AdjustmentRevisionProjectionKind::TargetRevision,
        projectionSha256: HASH_A.to_owned(),
        stageReceiptSha256: HASH_B.to_owned(),
    }
}

/// Build one database-issued target receipt at a distinct contiguous ordinal.
fn commit_receipt(ordinal: u64) -> RainFixedGaugeCommitReceipt {
    RainFixedGaugeCommitReceipt {
        archive_commit_ordinal: ordinal,
        archive_committed_at: "2026-11-24T09:03:00.000Z".to_owned(),
        frontier_sha256: "c".repeat(64),
        predecessor_frontier_sha256: "d".repeat(64),
        projection_identity_sha256: HASH_A.to_owned(),
        projection_sha256: HASH_A.to_owned(),
        receipt_sha256: format!("{ordinal:064x}"),
        stage_receipt_sha256: HASH_B.to_owned(),
    }
}
