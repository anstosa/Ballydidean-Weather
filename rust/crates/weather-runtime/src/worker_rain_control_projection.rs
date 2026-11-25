//! Prepare the v3 rain revision only after the selected auxiliary state is durable.

use std::sync::Arc;

use weather_api::AdmittedAdjustmentRevisionCaptureWitness;
use weather_contracts::{
    JsonValue, MaintenanceShadowComparatorAuthorityKind, MaintenanceShadowServingAuthority,
    Nullable, RainMaintenanceControlStateGapInput,
    RainMaintenanceControlStateGapInputReasonLiteral, RainMaintenanceControlStateStageReceipt,
    RainWindPerformanceHour, RainWindPerformanceHourOccurrenceProbabilities,
};
use weather_worker::{
    RainAdjustmentEvaluation, RainAdjustmentPendingInput, RainAdjustmentPersistedRevision,
    RainAdjustmentProjection, RainAdjustmentProjectionCodec, RainAdjustmentRevisionPlan,
    RainAdjustmentTimeAdapter, TaskFailure,
};

use crate::maintenance_canonical_json::{field, valid_hash};
use crate::worker_rain_feature_revision::{check_epoch, check_feature_binding, feature_document};
use crate::worker_rain_shadow::{binary64, encode_original, object, string};
use crate::{
    ApiMaintenanceShadowEvidenceValues, LoadedWorkerRainControlReference, MaintenanceReceiptScalar,
    WorkerRainControlArchiveClient, WorkerRainControlValues, WorkerRainFeatureRevisionCodec,
    WorkerRainPersistenceTarget, build_worker_rain_persistence_target,
};

const MAX_PROJECTION_BYTES: usize = 128 * 1_024;

/// The selected independent control reference and its approved private archive client.
pub struct WorkerRainControlRevision<Archive> {
    pub reference: LoadedWorkerRainControlReference,
    pub authority: Option<Arc<MaintenanceShadowServingAuthority>>,
    pub archive: Archive,
}

/// Preserve feature-only behavior when absent and never fall back from a selected control.
pub struct WorkerRainRevisionCodec<Evidence, Archive> {
    feature: WorkerRainFeatureRevisionCodec<Evidence>,
    control: Option<WorkerRainControlRevision<Archive>>,
    pending_auxiliary_gap: Option<RainMaintenanceControlStateGapInput>,
}

impl<Evidence, Archive> WorkerRainRevisionCodec<Evidence, Archive> {
    /// Retain the optional independent reference for this process's lifetime.
    pub const fn new(
        feature: WorkerRainFeatureRevisionCodec<Evidence>,
        control: Option<WorkerRainControlRevision<Archive>>,
    ) -> Self {
        Self {
            feature,
            control,
            pending_auxiliary_gap: None,
        }
    }
}

impl<Evidence, Archive> RainAdjustmentProjectionCodec for WorkerRainRevisionCodec<Evidence, Archive>
where
    Evidence: ApiMaintenanceShadowEvidenceValues,
    Archive: WorkerRainControlArchiveClient,
{
    /// Retain the same value-free key for v2 and v3.
    fn logical_key_sha256(
        &mut self,
        row: &RainAdjustmentPersistedRevision,
    ) -> Result<String, TaskFailure> {
        self.feature.logical_key_sha256(row)
    }

    /// Construct the selected contract with only the original optional state effects.
    fn build_revision(
        &mut self,
        pending: &RainAdjustmentPendingInput,
        evaluation: &RainAdjustmentEvaluation,
        persisted: &RainAdjustmentPersistedRevision,
        logical_key_sha256: &str,
        time: &mut impl RainAdjustmentTimeAdapter,
    ) -> Result<RainAdjustmentRevisionPlan, TaskFailure> {
        // only absence of an admitted independent reference selects the original v2 body
        let Some(control) = &mut self.control else {
            return self.feature.build_revision(
                pending,
                evaluation,
                persisted,
                logical_key_sha256,
                time,
            );
        };
        let evidence = self.feature.values.as_ref();
        let decision = evidence
            .canonical_utc_epoch_milliseconds(&evaluation.run.decision_at)
            .ok_or_else(failure)?;
        let prior_hour_at =
            time.utc_string(decision.checked_sub(3_600_000).ok_or_else(failure)?)?;
        let prepared = prepare_worker_rain_control_projection(
            evidence,
            &mut control.archive,
            WorkerRainControlProjectionInput {
                reference: &control.reference,
                epoch: &self.feature.epoch,
                authority: control.authority.as_deref(),
                pending,
                evaluation,
                persisted,
                prior_hour_at: &prior_hour_at,
                logical_key_sha256,
            },
        )?;
        self.pending_auxiliary_gap = Some(prepared.auxiliary_gap);
        Ok(prepared.plan)
    }

    /// Abandon only the selected state that never reached a staged projection.
    fn finish_projection_stage(&mut self, durable: bool) {
        let gap = self.pending_auxiliary_gap.take();
        // a durable body now owns the state, even if database bind or publication later fails
        if !durable && let (Some(control), Some(gap)) = (&mut self.control, gap) {
            let _ = control.archive.record_rain_control_state_gap(&gap);
        }
    }
}

/// Exact causal and database bindings required before archiving a v3 control projection.
pub struct WorkerRainControlProjectionInput<'a> {
    pub reference: &'a LoadedWorkerRainControlReference,
    pub epoch: &'a AdmittedAdjustmentRevisionCaptureWitness,
    pub authority: Option<&'a MaintenanceShadowServingAuthority>,
    pub pending: &'a RainAdjustmentPendingInput,
    pub evaluation: &'a RainAdjustmentEvaluation,
    pub persisted: &'a RainAdjustmentPersistedRevision,
    pub prior_hour_at: &'a str,
    pub logical_key_sha256: &'a str,
}

/// The control revision plan and its exact auxiliary disposition if projection staging fails.
pub struct PreparedWorkerRainControlProjection {
    pub plan: RainAdjustmentRevisionPlan,
    pub auxiliary_gap: RainMaintenanceControlStateGapInput,
}

/// Stage the selected control state before scoring any ordinal control or constructing v3 bytes.
pub fn prepare_worker_rain_control_projection(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    client: &mut impl WorkerRainControlArchiveClient,
    input: WorkerRainControlProjectionInput<'_>,
) -> Result<PreparedWorkerRainControlProjection, TaskFailure> {
    check_epoch(
        values,
        input.epoch,
        input.pending,
        input.evaluation,
        input.persisted,
    )
    .ok_or_else(failure)?;
    check_feature_binding(input.evaluation, input.persisted).ok_or_else(failure)?;
    // stale or future calibration does not silently select the feature-only contract
    if !input
        .reference
        .applies_to(input.epoch, &input.persisted.run_initialized_at)
    {
        return Err(failure());
    }
    let persistence = build_worker_rain_persistence_target(
        values,
        input.pending,
        input.evaluation,
        input.prior_hour_at,
    )
    .ok_or_else(failure)?;
    let receipt = client.stage_rain_control_state(input.reference.state_bytes())?;
    let auxiliary_gap = RainMaintenanceControlStateGapInput {
        reason: RainMaintenanceControlStateGapInputReasonLiteral,
        stateSha256: input.reference.control_state_sha256().to_owned(),
        stageReceiptSha256: receipt.stageReceiptSha256.clone(),
    };
    let built = build_admitted_control_projection(values, &input, persistence, &receipt);
    match built {
        Some(plan) => Ok(PreparedWorkerRainControlProjection {
            plan,
            auxiliary_gap,
        }),
        None => {
            // if the body never reached staging, the exact auxiliary may be abandoned
            let _ = client.record_rain_control_state_gap(&auxiliary_gap);
            Err(failure())
        }
    }
}

/// Encode the exact ordered v3 projection with one hash preamble reused across all rows.
fn build_admitted_control_projection(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    input: &WorkerRainControlProjectionInput<'_>,
    persistence: WorkerRainPersistenceTarget,
    receipt: &RainMaintenanceControlStateStageReceipt,
) -> Option<RainAdjustmentRevisionPlan> {
    verify_state_receipt(values, input.reference, receipt)?;
    let authority = input.authority?;
    let mut core = feature_document(values, input.evaluation, input.persisted)?;
    let logical = take(&mut core, "logicalKey")?;
    let JsonValue::Array(rows) = take(&mut core, "rows")? else {
        return None;
    };
    let controls = input
        .reference
        .evaluate(
            &input.evaluation.feature_projection,
            persistence.prediction_mm,
        )
        .ok()?;
    let mut row_preimage = b"adjustment-rain-gate-feature-row/v2\n".to_vec();
    row_preimage.extend_from_slice(&encode_original(values, &logical)?);
    let prefix_bytes = row_preimage.len();
    let incumbent = &input.evaluation.incumbent_performance.hours;
    // retain every original row from the same one-time incumbent execution
    if incumbent.len() != rows.len() || controls.len() != rows.len() {
        return None;
    }
    let rows = rows
        .into_iter()
        .zip(incumbent)
        .zip(controls)
        .map(|((row, incumbent), control)| {
            row_preimage.truncate(prefix_bytes);
            row_preimage.extend_from_slice(&encode_original(values, &row)?);
            let digest = values
                .sha256(&row_preimage)
                .filter(|digest| valid_hash(digest.as_str()))?;
            control_row(
                row,
                incumbent,
                control,
                authority,
                &persistence.member_sha256,
                digest.as_str(),
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let doc = object([
        (
            "contractVersion",
            string("adjustment-rain-gate-control-projection/v3"),
        ),
        ("family", string("rain")),
        ("logicalKey", logical),
        ("logicalReceivedAt", take(&mut core, "logicalReceivedAt")?),
        (
            "ordinalArtifactSha256",
            string(input.reference.ordinal_artifact_sha256()),
        ),
        ("persistenceTarget", persistence.document),
        ("projectionKind", string("rain_gate_input")),
        ("rows", JsonValue::Array(rows)),
        ("source", take(&mut core, "source")?),
        (
            "stateSha256",
            string(input.reference.control_state_sha256()),
        ),
        (
            "stateStageReceiptSha256",
            string(&receipt.stageReceiptSha256),
        ),
        (
            "storedContentSha256",
            take(&mut core, "storedContentSha256")?,
        ),
    ]);
    let bytes = encode_original(values, &doc)?;
    // preserve the original per-family byte ceiling
    if !(2..=MAX_PROJECTION_BYTES).contains(&bytes.len()) {
        return None;
    }
    let digest = values
        .sha256(&bytes)
        .filter(|digest| valid_hash(digest.as_str()))?;
    Some(RainAdjustmentRevisionPlan {
        projection: RainAdjustmentProjection {
            bytes,
            logical_key_sha256: input.logical_key_sha256.to_owned(),
            projection_identity_sha256: digest.as_str().to_owned(),
        },
    })
}

/// Emit one row with original cumulative-head, raw fallback and source identity bindings.
fn control_row(
    mut source: JsonValue,
    incumbent: &RainWindPerformanceHour,
    controls: WorkerRainControlValues,
    authority: &MaintenanceShadowServingAuthority,
    persistence_sha256: &str,
    source_row_sha256: &str,
) -> Option<JsonValue> {
    // require the incumbent result for the same causal valid time and model lead
    if field(&source, "validAt")? != &string(&incumbent.validAt)
        || field(&source, "modelLeadHours")? != &JsonValue::Number(incumbent.modelLeadHours)
    {
        return None;
    }
    let policy_raw = authority.authorityKind == MaintenanceShadowComparatorAuthorityKind::PolicyRaw;
    let probabilities = match &incumbent.occurrenceProbabilities {
        Nullable::Value(values) if !policy_raw => values,
        _ => &controls.native_source_probability,
    };
    let amount = if policy_raw || !incumbent.applied {
        incumbent.rawPrecipitationMm
    } else {
        incumbent.correctedPrecipitationMm
    };
    let supported = controls.persistence_reason
        == weather_contracts::RainMaintenancePersistenceTargetReasonEnum::CausalTarget;
    let artifact = match &authority.artifactIdentitySha256 {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(identity) => string(identity),
    };
    // preserve physical control and incumbent predictions before encoding the row
    if [
        amount,
        controls.persistence_prediction,
        controls.recent_volume_scale_prediction,
        controls.same_window_volume_scale_prediction,
        controls.unchanged_ordinal_prediction,
        controls.volume_scale_prediction,
    ]
    .into_iter()
    .any(|value| value < 0.0)
    {
        return None;
    }
    Some(object([
        ("features64", take(&mut source, "features64")?),
        ("incumbentArtifactIdentitySha256", artifact),
        ("incumbentPrediction64", binary64(amount)?),
        ("incumbentProbability", probability(probabilities)?),
        (
            "incumbentReceiptMemberSha256",
            string(&authority.receiptMemberSha256),
        ),
        ("modelLeadHours", take(&mut source, "modelLeadHours")?),
        (
            "nativeSourceProbability",
            probability(&controls.native_source_probability)?,
        ),
        (
            "persistencePrediction64",
            binary64(controls.persistence_prediction)?,
        ),
        (
            "persistenceReason",
            string(if supported {
                "causal_target"
            } else {
                "raw_fallback_unavailable"
            }),
        ),
        (
            "persistenceTargetMemberSha256",
            if supported {
                string(persistence_sha256)
            } else {
                JsonValue::Null
            },
        ),
        (
            "rawPrecipitationMm64",
            take(&mut source, "rawPrecipitationMm64")?,
        ),
        (
            "rawTargetHourTemperatureC64",
            take(&mut source, "rawTargetHourTemperatureC64")?,
        ),
        (
            "recentVolumeScalePrediction64",
            binary64(controls.recent_volume_scale_prediction)?,
        ),
        (
            "sameWindowVolumeScalePrediction64",
            binary64(controls.same_window_volume_scale_prediction)?,
        ),
        ("sourceRowSha256", string(source_row_sha256)),
        (
            "unchangedOrdinalPrediction64",
            binary64(controls.unchanged_ordinal_prediction)?,
        ),
        ("validAt", take(&mut source, "validAt")?),
        (
            "volumeScalePrediction64",
            binary64(controls.volume_scale_prediction)?,
        ),
    ]))
}

/// Require physical nested probabilities in the original ordered receipt shape.
fn probability(values: &RainWindPerformanceHourOccurrenceProbabilities) -> Option<JsonValue> {
    let [wet, moderate, heavy] = [values.atLeast0_1, values.atLeast1_0, values.atLeast2_5];
    // every cumulative head is finite and cannot exceed the previous head
    if !(0.0..=1.0).contains(&wet)
        || !(0.0..=wet).contains(&moderate)
        || !(0.0..=moderate).contains(&heavy)
    {
        return None;
    }
    Some(object([
        ("atLeast0_1", binary64(wet)?),
        ("atLeast1_0", binary64(moderate)?),
        ("atLeast2_5", binary64(heavy)?),
    ]))
}

/// Verify the original ordered state acknowledgement and generation lower bound.
fn verify_state_receipt(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    reference: &LoadedWorkerRainControlReference,
    receipt: &RainMaintenanceControlStateStageReceipt,
) -> Option<()> {
    // require the exact selected member and an archive clock at or after generation
    if receipt.stateSha256 != reference.control_state_sha256()
        || values.canonical_utc_epoch_milliseconds(&receipt.durableAt)?
            < values.canonical_utc_epoch_milliseconds(reference.generated_at())?
    {
        return None;
    }
    let text = MaintenanceReceiptScalar::Text;
    let digest = values.unsigned_receipt_sha256(&[
        (
            "contractVersion",
            text("adjustment-rain-control-state-stage-receipt/v1"),
        ),
        ("durable", MaintenanceReceiptScalar::Boolean(true)),
        ("durableAt", text(&receipt.durableAt)),
        ("stateSha256", text(&receipt.stateSha256)),
    ])?;
    (valid_hash(&receipt.stageReceiptSha256) && digest.as_str() == receipt.stageReceiptSha256)
        .then_some(())
}

/// Move a member out of the newly built document without cloning feature buffers.
fn take(document: &mut JsonValue, key: &str) -> Option<JsonValue> {
    let JsonValue::Object(fields) = document else {
        return None;
    };
    let index = fields
        .iter()
        .position(|(name, _)| name.iter().copied().eq(key.encode_utf16()))?;
    Some(fields.remove(index).1)
}

/// Classify failures without including causal or archive values.
fn failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "rain_control_revision_invalid".to_owned(),
    }
}

#[cfg(test)]
#[path = "worker_rain_control_projection_test.rs"]
mod tests;
