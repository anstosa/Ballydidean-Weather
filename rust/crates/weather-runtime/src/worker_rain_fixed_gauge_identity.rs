//! Own exact target-group logical, weather-content, and receipt identities in Rust.

use std::sync::Arc;

use weather_contracts::{
    AdjustmentRainFixedGaugeTargetLogicalKeyContractVersionLiteral,
    AdjustmentRevisionProjectionKind, AdjustmentRevisionStageReceipt,
    AdjustmentRevisionStageReceiptContractVersionLiteral,
    AdjustmentRevisionStageReceiptDurableLiteral, NormalizedWeatherRecord,
};
use weather_database::Sha256Digest;
use weather_worker::{
    RainFixedGaugeDurableGap, RainFixedGaugeGapReason, RainFixedGaugeIdentityAdapter,
    RainFixedGaugeNormalizedRecord, RainFixedGaugeStageReceipt, TaskFailure,
};

use crate::api_maintenance_receipts::verify_revision_stage_receipt_for_identity;
use crate::api_revision_projection::revision_logical_key_sha256;
use crate::maintenance_canonical_json::valid_hash;
use crate::worker_rain_fixed_gauge_wire::target_record;
use crate::{ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar};

/// Approved serializer/hash for the original TypeScript normalized weather-content preimage.
pub trait WorkerRainFixedGaugeIdentityValues: ApiMaintenanceReceiptCanonicalValues {
    /// Digest the exact weatherRecordContent grammar, not the full provider wire document.
    fn fixed_gauge_record_content_sha256(
        &self,
        record: &NormalizedWeatherRecord,
    ) -> Option<Sha256Digest>;
}

/// One retained canonical owner for grouped custody.
pub struct WorkerRainFixedGaugeIdentity<Values>(Arc<Values>);

impl<Values> WorkerRainFixedGaugeIdentity<Values> {
    /// Reuse the worker's approved UTC, digest, and canonical weather preimage owner.
    pub const fn new(values: Arc<Values>) -> Self {
        Self(values)
    }
}

impl<Values: WorkerRainFixedGaugeIdentityValues> RainFixedGaugeIdentityAdapter
    for WorkerRainFixedGaugeIdentity<Values>
{
    /// Digest the ordered dedicated source vector before querying oversized disposition.
    fn logical_key_sha256(
        &mut self,
        source_ids: &[String],
        valid_at: &str,
    ) -> Result<String, TaskFailure> {
        // validate the canonical UTC before encoding the original logical key
        if self.0.canonical_utc_epoch_milliseconds(valid_at).is_none() {
            return Err(failure());
        }
        revision_logical_key_sha256(
            self.0.as_ref(),
            &[
                (
                    "contractVersion",
                    MaintenanceReceiptScalar::Text(
                        AdjustmentRainFixedGaugeTargetLogicalKeyContractVersionLiteral::WIRE_VALUE,
                    ),
                ),
                ("sourceIds", MaintenanceReceiptScalar::TextArray(source_ids)),
                ("validAt", MaintenanceReceiptScalar::Text(valid_at)),
            ],
        )
        .map_err(|_| failure())
    }

    /// Bind raw capture bytes without interpreting their gzip or UTF-8 framing.
    fn capture_body_matches(&mut self, body: &[u8], expected_sha256: &str) -> bool {
        valid_hash(expected_sha256)
            && self
                .0
                .sha256(body)
                .is_some_and(|digest| digest.as_str() == expected_sha256)
    }

    /// Digest the exact canonical grouped archive body.
    fn projection_identity_sha256(&mut self, bytes: &[u8]) -> Result<String, TaskFailure> {
        self.0
            .sha256(bytes)
            .filter(|digest| valid_hash(digest.as_str()))
            .map(|digest| digest.as_str().to_owned())
            .ok_or_else(failure)
    }

    /// Reconstruct the exact native target record before verifying its database content address.
    fn weather_record_sha256(
        &mut self,
        station_id: u64,
        record: &RainFixedGaugeNormalizedRecord,
    ) -> Result<String, TaskFailure> {
        let record = target_record(station_id, record).ok_or_else(failure)?;
        self.0
            .fixed_gauge_record_content_sha256(&record)
            .filter(|digest| valid_hash(digest.as_str()))
            .map(|digest| digest.as_str().to_owned())
            .ok_or_else(failure)
    }

    /// Verify the permanent oversized disposition's original ordered self-address.
    fn durable_gap_receipt_is_authentic(&mut self, gap: &RainFixedGaugeDurableGap) -> bool {
        // verify the value-free kind and canonical clock rather than synthesizing a receipt
        if gap.reason != RainFixedGaugeGapReason::TargetSourceOversized
            || !valid_hash(&gap.gap_sha256)
            || self
                .0
                .canonical_utc_epoch_milliseconds(&gap.gap_at)
                .is_none()
        {
            return false;
        }
        let text = MaintenanceReceiptScalar::Text;
        self.0
            .unsigned_receipt_sha256(&[
                (
                    "contractVersion",
                    text("adjustment-rain-fixed-gauge-target-gap/v1"),
                ),
                ("gapAt", text(&gap.gap_at)),
                ("logicalHourAt", text(&gap.logical_hour_at)),
                ("logicalKeySha256", text(&gap.logical_key_sha256)),
                ("qualificationDisposition", text("forever_unqualified")),
                ("reason", text("target_source_oversized")),
            ])
            .is_some_and(|digest| digest.as_str() == gap.gap_sha256)
    }

    /// Verify the target staging receipt without rehashing the large raw projection.
    fn stage_receipt_is_authentic(&mut self, stage: &RainFixedGaugeStageReceipt) -> bool {
        // retain the source runtime's durable acknowledgement check
        if !stage.durable {
            return false;
        }
        let receipt = AdjustmentRevisionStageReceipt {
            contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
            durable: AdjustmentRevisionStageReceiptDurableLiteral,
            durableAt: stage.durable_at.clone(),
            projectionIdentitySha256: stage.projection_identity_sha256.clone(),
            projectionKind: AdjustmentRevisionProjectionKind::TargetRevision,
            projectionSha256: stage.projection_sha256.clone(),
            stageReceiptSha256: stage.stage_receipt_sha256.clone(),
        };
        verify_revision_stage_receipt_for_identity(
            self.0.as_ref(),
            &stage.projection_identity_sha256,
            AdjustmentRevisionProjectionKind::TargetRevision,
            &receipt,
        )
        .is_ok()
    }
}

/// Keep identity failures categorical.
fn failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "rain_fixed_gauge_identity_invalid".to_owned(),
    }
}
