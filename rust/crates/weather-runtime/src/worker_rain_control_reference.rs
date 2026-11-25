//! Load and score the independent fixed monthly rain control reference.

use weather_api::AdmittedAdjustmentRevisionCaptureWitness;
use weather_contracts::{
    RainHurdleWindFeatureProjection, RainMaintenancePersistenceTargetReasonEnum,
    RainWindPerformanceHourOccurrenceProbabilities,
};
use weather_domain::AdmittedRainMaintenanceControlState;
use weather_worker::TaskFailure;

use crate::worker_rain_candidate_package::{
    RainControlMemberSelection, check_worker_rain_control_members,
};
use crate::worker_rain_control_catalog::{
    select_worker_rain_control_catalog, validate_worker_rain_control_catalog_members,
};
use crate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceCandidateError, ApiMaintenanceShadowEvidenceValues,
    PreparedWorkerRainMaintenanceCandidate, WorkerRainControlReferenceFiles,
};

/// Startup-only custody of the exact raw state and its independently selected ordinal model.
pub struct LoadedWorkerRainControlReference {
    state: AdmittedRainMaintenanceControlState,
    state_bytes: Vec<u8>,
    state_sha256: String,
    ordinal: PreparedWorkerRainMaintenanceCandidate,
}

impl LoadedWorkerRainControlReference {
    /// Borrow the exact bytes to stage before any archived controls are scored.
    pub fn state_bytes(&self) -> &[u8] {
        &self.state_bytes
    }

    /// Identify the exact raw state bytes selected by the control reference.
    pub fn control_state_sha256(&self) -> &str {
        &self.state_sha256
    }

    /// Identify the separate portable ordinal artifact selected by the same reference.
    pub fn ordinal_artifact_sha256(&self) -> &str {
        &self.state.ordinal_artifact_sha256
    }

    /// Borrow the original state generation clock for the archive acknowledgement.
    pub fn generated_at(&self) -> &str {
        &self.state.generated_at
    }

    /// Check the startup reference against the actual serving month and admitted zero frontier.
    pub fn applies_to(
        &self,
        epoch: &AdmittedAdjustmentRevisionCaptureWitness,
        run_initialized_at: &str,
    ) -> bool {
        let frontier = epoch.capture_epoch.epoch_milliseconds;
        run_initialized_at.get(..7) == Some(self.state.model_month.as_str())
            && self.state.epoch_witness_sha256 == epoch.witness.witnessSha256
            && self.state.training_maximum_epoch_milliseconds >= frontier
            && self.state.generated_epoch_milliseconds >= frontier
    }

    /// Evaluate all admitted control rows without reparsing the state or ordinal artifact.
    pub fn evaluate(
        &self,
        features: &RainHurdleWindFeatureProjection,
        persistence_prediction: Option<f64>,
    ) -> Result<Vec<WorkerRainControlValues>, TaskFailure> {
        // refuse the original invalid raw inputs instead of using fallbacks to conceal them
        if persistence_prediction.is_some_and(|value| !nonnegative(value))
            || features.rows.len() != 23
            || features.rows.iter().any(|row| {
                !nonnegative(row.rawPrecipitationMm)
                    || row.features.len() != 107
                    || row.features[5] != row.rawPrecipitationMm as f32
            })
        {
            return Err(control_failure());
        }
        let scales = self.state.volume_scale_bits.map(f64::from_bits);
        let ordinal = self.ordinal.score_control_ordinals(features)?;
        Ok(features
            .rows
            .iter()
            .zip(ordinal)
            .map(|(row, unchanged)| {
                evaluate_row(
                    row.rawPrecipitationMm,
                    persistence_prediction,
                    scales,
                    unchanged,
                )
            })
            .collect())
    }
}

/// Read only the v3 catalog's independent rain control slot; no shadow or serving fallback.
pub fn load_worker_rain_control_reference(
    files: &mut impl WorkerRainControlReferenceFiles,
    codec: &mut impl ApiMaintenanceCandidateCodec,
    values: &impl ApiMaintenanceShadowEvidenceValues,
) -> Result<Option<LoadedWorkerRainControlReference>, ApiMaintenanceCandidateError> {
    let raw = files.read_catalog()?;
    let catalog = codec
        .parse_canonical_object(&raw)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    // absence selects feature-only revision behavior without reading any control members
    let Some(selected) = select_worker_rain_control_catalog(&catalog, codec)? else {
        return Ok(None);
    };
    let members = files.read_worker_rain_control_selector(&selected.action_sha256)?;
    let created_at = validate_worker_rain_control_catalog_members(
        codec,
        &selected,
        &members.selector,
        &members.action,
    )?;
    let control_members = files.read_rain_control_members(
        &selected.control_state_sha256,
        &selected.ordinal_artifact_sha256,
    )?;
    let hashes = RainControlMemberSelection {
        control_state_sha256: selected.control_state_sha256,
        ordinal_artifact_sha256: selected.ordinal_artifact_sha256,
    };
    let (state, ordinal) = check_worker_rain_control_members(
        values,
        &hashes,
        &selected.due_month,
        &control_members.state,
        &control_members.ordinal,
    )?;
    // retain the original graph and generation cross-bind that technical packages do not supply
    if state.generated_at != created_at
        || state.source_member_root_sha256 != selected.source_member_root_sha256
        || state.source_receipt_root_sha256 != selected.source_receipt_root_sha256
    {
        return Err(ApiMaintenanceCandidateError::Package);
    }
    Ok(Some(LoadedWorkerRainControlReference {
        state,
        state_bytes: control_members.state,
        state_sha256: hashes.control_state_sha256,
        ordinal,
    }))
}

/// Scores for the exact original fixed control arms.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerRainControlValues {
    pub native_source_probability: RainWindPerformanceHourOccurrenceProbabilities,
    pub persistence_prediction: f64,
    pub persistence_reason: RainMaintenancePersistenceTargetReasonEnum,
    pub recent_volume_scale_prediction: f64,
    pub same_window_volume_scale_prediction: f64,
    pub unchanged_ordinal_prediction: f64,
    pub volume_scale_prediction: f64,
}

/// Preserve unclamped legacy output, both 30 mm caps, and exact native wet thresholds.
fn evaluate_row(
    raw: f64,
    persistence: Option<f64>,
    [legacy, same_window, recent]: [f64; 3],
    unchanged_ordinal_prediction: f64,
) -> WorkerRainControlValues {
    let native = |threshold| f64::from(u8::from(raw >= threshold));
    WorkerRainControlValues {
        native_source_probability: RainWindPerformanceHourOccurrenceProbabilities {
            atLeast0_1: native(0.1),
            atLeast1_0: native(1.0),
            atLeast2_5: native(2.5),
        },
        persistence_prediction: persistence.unwrap_or(raw),
        persistence_reason: if persistence.is_some() {
            RainMaintenancePersistenceTargetReasonEnum::CausalTarget
        } else {
            RainMaintenancePersistenceTargetReasonEnum::RawFallbackUnavailable
        },
        recent_volume_scale_prediction: (raw * recent).min(30.0),
        same_window_volume_scale_prediction: (raw * same_window).min(30.0),
        unchanged_ordinal_prediction,
        volume_scale_prediction: raw * legacy,
    }
}

/// Keep physical amounts finite and nonnegative exactly as the original control input.
fn nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

/// Report invalid control input without retaining its values in diagnostics.
fn control_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "rain_control_input_invalid".to_owned(),
    }
}
