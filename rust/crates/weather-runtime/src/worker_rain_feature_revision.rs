//! Build the exact rain gate feature revision from settled causal worker inputs.

use std::sync::Arc;

use weather_api::AdmittedAdjustmentRevisionCaptureWitness;
use weather_contracts::{
    AdjustmentRainGateFeatureProjectionContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionProjectionKind, JsonValue,
};
use weather_worker::{
    RAIN_SOURCE_CONFIG_FINGERPRINT, RainAdjustmentCaptureIdentity, RainAdjustmentCaptureKind,
    RainAdjustmentCommitReceipt, RainAdjustmentEvaluation, RainAdjustmentIdentityAdapter,
    RainAdjustmentPendingInput, RainAdjustmentPersistedRevision, RainAdjustmentProjection,
    RainAdjustmentProjectionCodec, RainAdjustmentRevisionPlan, RainAdjustmentStageReceipt,
    RainAdjustmentTimeAdapter, TaskFailure,
};

use crate::api_maintenance_receipts::verify_revision_stage_receipt_for_identity;
use crate::api_revision_projection::revision_logical_key_sha256;
use crate::maintenance_canonical_json::valid_hash;
use crate::worker_rain_shadow::{binary64, encode_original, object, string};
use crate::{ApiMaintenanceShadowEvidenceValues, MaintenanceReceiptScalar};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const ROW_COUNT: usize = 23;
const FEATURE_COUNT: usize = 107;
const MAX_PROJECTION_BYTES: usize = 128 * 1_024;

/// The original feature-only archive codec, selected when monthly controls are absent.
pub struct WorkerRainFeatureRevisionCodec<Values> {
    pub(crate) values: Arc<Values>,
    pub(crate) epoch: Arc<AdmittedAdjustmentRevisionCaptureWitness>,
}

impl<Values> WorkerRainFeatureRevisionCodec<Values> {
    /// Borrow the same verified capture witness and primitive value owner for each revision.
    pub const fn new(
        values: Arc<Values>,
        epoch: Arc<AdmittedAdjustmentRevisionCaptureWitness>,
    ) -> Self {
        Self { values, epoch }
    }
}

impl<Values: ApiMaintenanceShadowEvidenceValues> RainAdjustmentProjectionCodec
    for WorkerRainFeatureRevisionCodec<Values>
{
    /// Derive the original value-free key before any source body validation or encoding.
    fn logical_key_sha256(
        &mut self,
        persisted: &RainAdjustmentPersistedRevision,
    ) -> Result<String, TaskFailure> {
        // admit the complete closed logical key before constructing its preimage
        if !valid_hash(&persisted.input_sha256)
            || !valid_hash(&persisted.model_sha256)
            || self
                .values
                .canonical_utc_epoch_milliseconds(&persisted.run_initialized_at)
                .is_none()
        {
            return Err(failure());
        }
        revision_logical_key_sha256(
            self.values.as_ref(),
            &[
                (
                    "inputSha256",
                    MaintenanceReceiptScalar::Text(&persisted.input_sha256),
                ),
                (
                    "modelSha256",
                    MaintenanceReceiptScalar::Text(&persisted.model_sha256),
                ),
                (
                    "runInitializedAt",
                    MaintenanceReceiptScalar::Text(&persisted.run_initialized_at),
                ),
            ],
        )
        .map_err(|_| failure())
    }

    /// Build ordered canonical v2 feature bytes from the persisted serving revision.
    fn build_revision(
        &mut self,
        pending: &RainAdjustmentPendingInput,
        evaluation: &RainAdjustmentEvaluation,
        persisted: &RainAdjustmentPersistedRevision,
        logical_key_sha256: &str,
        _time: &mut impl RainAdjustmentTimeAdapter,
    ) -> Result<RainAdjustmentRevisionPlan, TaskFailure> {
        let values = self.values.as_ref();
        check_epoch(values, &self.epoch, pending, evaluation, persisted).ok_or_else(failure)?;
        let projection = feature_document(values, evaluation, persisted).ok_or_else(failure)?;
        let bytes = encode_original(values, &projection)
            .filter(|bytes| (2..=MAX_PROJECTION_BYTES).contains(&bytes.len()))
            .ok_or_else(failure)?;
        let projection_identity_sha256 = self
            .values
            .sha256(&bytes)
            .filter(|digest| valid_hash(digest.as_str()))
            .ok_or_else(failure)?;
        Ok(RainAdjustmentRevisionPlan {
            projection: RainAdjustmentProjection {
                bytes,
                logical_key_sha256: logical_key_sha256.to_owned(),
                projection_identity_sha256: projection_identity_sha256.as_str().to_owned(),
            },
        })
    }
}

/// Original compact source identity and receipt admission for worker rain inference.
pub struct WorkerRainRevisionIdentity<Values>(Arc<Values>);

impl<Values> WorkerRainRevisionIdentity<Values> {
    /// Reuse the already admitted UTC, SHA-256 and ordered JSON owners.
    pub const fn new(values: Arc<Values>) -> Self {
        Self(values)
    }
}

impl<Values: ApiMaintenanceShadowEvidenceValues> RainAdjustmentIdentityAdapter
    for WorkerRainRevisionIdentity<Values>
{
    /// Hash the exact database-order capture tuples as compact JSON without a trailing LF.
    fn input_sha256(
        &mut self,
        captures: &[RainAdjustmentCaptureIdentity],
    ) -> Result<String, TaskFailure> {
        let document = JsonValue::Array(
            captures
                .iter()
                .map(|member| {
                    JsonValue::Array(vec![
                        string(&member.claim_id),
                        string(&member.body_sha256),
                        string(&member.completed_at),
                    ])
                })
                .collect(),
        );
        let bytes = self
            .0
            .encode_shadow_ordered_json(&document)
            .ok_or_else(failure)?;
        self.0
            .sha256(&bytes)
            .filter(|digest| valid_hash(digest.as_str()))
            .map(|digest| digest.as_str().to_owned())
            .ok_or_else(failure)
    }

    /// Verify the stage's closed ordered self-address without rehashing the projection.
    fn stage_receipt_is_authentic(&mut self, receipt: &RainAdjustmentStageReceipt) -> bool {
        verify_revision_stage_receipt_for_identity(
            self.0.as_ref(),
            &receipt.projectionIdentitySha256,
            AdjustmentRevisionProjectionKind::RainGateInput,
            receipt,
        )
        .is_ok()
    }

    /// Preserve the original PostgreSQL receipt shape before private API re-admission.
    fn commit_receipt_is_authentic(&mut self, receipt: &RainAdjustmentCommitReceipt) -> bool {
        receipt.projectionKind == AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput
            && self
                .0
                .canonical_utc_epoch_milliseconds(&receipt.archiveCommittedAt)
                .is_some()
            && receipt
                .archiveCommitOrdinal
                .as_bytes()
                .first()
                .is_some_and(|digit| (b'1'..=b'9').contains(digit))
            && receipt
                .archiveCommitOrdinal
                .bytes()
                .all(|digit| digit.is_ascii_digit())
            && [
                &receipt.frontierSha256,
                &receipt.predecessorFrontierSha256,
                &receipt.projectionIdentitySha256,
                &receipt.projectionSha256,
                &receipt.receiptSha256,
                &receipt.stageReceiptSha256,
            ]
            .into_iter()
            .all(|digest| valid_hash(digest))
    }
}

/// Require every raw, decoded and stored clock consumed by the original feature archive.
pub(crate) fn check_epoch(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    witness: &AdmittedAdjustmentRevisionCaptureWitness,
    pending: &RainAdjustmentPendingInput,
    evaluation: &RainAdjustmentEvaluation,
    persisted: &RainAdjustmentPersistedRevision,
) -> Option<()> {
    let epoch = witness.capture_epoch.epoch_milliseconds;
    // compare exact canonical timestamps against the inclusive verified witness
    let after = |clock: &str| {
        values
            .canonical_utc_epoch_milliseconds(clock)
            .is_some_and(|millis| millis >= epoch)
    };

    let captured = pending.captures.iter().all(|capture| {
        after(&capture.identity.completed_at)
            && match &capture.kind {
                RainAdjustmentCaptureKind::Forecast { profile, .. } => {
                    after(&profile.runInitializedAt)
                }
                RainAdjustmentCaptureKind::Station {
                    window_start,
                    window_end_exclusive,
                    ..
                } => after(window_start) && after(window_end_exclusive),
            }
    });
    let input = &evaluation.causal_input;
    let runs = std::iter::once(&input.currentRun)
        .chain(&input.priorRuns)
        .all(|run| after(&run.runInitializedAt) && after(&run.completedAt));
    let observed = input
        .stationHours
        .iter()
        .all(|row| after(&row.hourAt) && after(&row.receivedAt));
    (captured
        && runs
        && observed
        && after(&persisted.run_initialized_at)
        && after(&persisted.generated_at))
    .then_some(())
}

/// Cross-bind the causal rows to the independent server-persisted serving gate.
pub(crate) fn feature_document(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    evaluation: &RainAdjustmentEvaluation,
    persisted: &RainAdjustmentPersistedRevision,
) -> Option<JsonValue> {
    check_feature_binding(evaluation, persisted)?;
    // the source claim id belongs to the closed original feature document
    if !(1..=128).contains(&evaluation.current_claim_id.encode_utf16().count()) {
        return None;
    }
    let observed = &evaluation.feature_projection;
    let initialized = values.canonical_utc_epoch_milliseconds(&persisted.run_initialized_at)?;
    let rows = observed
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let lead = 9 + index as i64;
            // admit each exact native lead, timestamp, unadjusted target, and feature geometry
            if row.modelLeadHours != lead as f64
                || row.rawPrecipitationMm < 0.0
                || values.canonical_utc_epoch_milliseconds(&row.validAt)?
                    != initialized.checked_add(lead * HOUR_MILLISECONDS)?
                || row.features.len() != FEATURE_COUNT
            {
                return None;
            }
            let features = row
                .features
                .iter()
                .map(|value| {
                    // original Float32Array entries become binary64 or explicit missing values
                    if value.is_nan() {
                        Some(JsonValue::Null)
                    } else {
                        binary64(f64::from(*value))
                    }
                })
                .collect::<Option<Vec<_>>>()?;
            Some(object([
                ("features64", JsonValue::Array(features)),
                ("modelLeadHours", JsonValue::Number(row.modelLeadHours)),
                ("rawPrecipitationMm64", binary64(row.rawPrecipitationMm)?),
                (
                    "rawTargetHourTemperatureC64",
                    binary64(row.rawTargetHourTemperatureC)?,
                ),
                ("validAt", string(&row.validAt)),
            ]))
        })
        .collect::<Option<Vec<_>>>()?;

    Some(object([
        (
            "contractVersion",
            string(AdjustmentRainGateFeatureProjectionContractVersionLiteral::WIRE_VALUE),
        ),
        ("family", string("rain")),
        (
            "logicalKey",
            object([
                ("inputSha256", string(&persisted.input_sha256)),
                ("modelSha256", string(&persisted.model_sha256)),
                ("runInitializedAt", string(&persisted.run_initialized_at)),
            ]),
        ),
        ("logicalReceivedAt", string(&persisted.generated_at)),
        ("projectionKind", string("rain_gate_input")),
        ("rows", JsonValue::Array(rows)),
        (
            "source",
            object([
                ("adapterVersion", string("rain-hurdle-wind-features/v1")),
                ("contractEpoch", string("rain-prospective-capture/v1")),
                ("dataset", string("ecmwf_ifs")),
                ("providerKey", string("open-meteo-single-runs")),
                (
                    "sourceConfigFingerprint",
                    string(RAIN_SOURCE_CONFIG_FINGERPRINT),
                ),
                ("sourceId", string(&evaluation.current_claim_id)),
                ("sourceKey", string("rain-prospective-forecast")),
                ("sourceKind", string("forecast")),
                ("upstreamModel", string("ecmwf_ifs")),
            ]),
        ),
        (
            "storedContentSha256",
            string(&persisted.stored_content_sha256),
        ),
    ]))
}

/// Bind the settled feature projection before staging any auxiliary state.
pub(crate) fn check_feature_binding(
    evaluation: &RainAdjustmentEvaluation,
    persisted: &RainAdjustmentPersistedRevision,
) -> Option<()> {
    let observed = &evaluation.feature_projection;
    // the stored model/run/input and all serving rows must still be the original output
    if observed.runInitializedAt != persisted.run_initialized_at
        || observed.decisionAt != evaluation.run.decision_at
        || evaluation.run.input_sha256 != persisted.input_sha256
        || evaluation.run.model_sha256 != persisted.model_sha256
        || observed.rows.len() != ROW_COUNT
        || persisted.hours.len() != ROW_COUNT
        || persisted.hours != evaluation.run.hours
        || !valid_hash(&persisted.input_sha256)
        || !valid_hash(&persisted.model_sha256)
        || !valid_hash(&persisted.stored_content_sha256)
        || observed
            .rows
            .iter()
            .zip(&persisted.hours)
            .any(|(row, stored)| {
                row.modelLeadHours != stored.model_lead_hours
                    || row.rawPrecipitationMm != stored.raw_precipitation_mm
                    || row.validAt != stored.valid_at
            })
    {
        return None;
    }
    Some(())
}

/// Keep serving diagnostics categorical and free of provider data.
fn failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "rain_feature_revision_invalid".to_owned(),
    }
}

#[cfg(test)]
#[path = "worker_rain_feature_revision_test.rs"]
mod tests;
