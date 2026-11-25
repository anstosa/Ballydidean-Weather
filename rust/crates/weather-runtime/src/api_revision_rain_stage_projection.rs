//! Admit the two rain revision documents that remain stage-only.

use weather_api::{AdjustmentMaintenanceBodyError, CanonicalInstant};
use weather_contracts::{
    AdjustmentRainGateControlProjection, AdjustmentRainGateFeatureProjection,
    AdjustmentRevisionProjectionKind,
};

use crate::ApiMaintenanceReceiptCanonicalValues;
use crate::api_revision_projection::{RevisionProjectionEvidence, admit_revision_projection_clock};

/// Admit the feature document's actual model-run and receipt clocks without publication keys.
pub(crate) fn project_rain_feature_stage(
    document: &AdjustmentRainGateFeatureProjection,
    epoch: &CanonicalInstant,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
) -> Result<RevisionProjectionEvidence, AdjustmentMaintenanceBodyError> {
    project_rain_stage(
        &document.logicalKey.runInitializedAt,
        &document.logicalReceivedAt,
        epoch,
        values,
    )
}

/// Admit the control document's actual model-run and receipt clocks without publication keys.
pub(crate) fn project_rain_control_stage(
    document: &AdjustmentRainGateControlProjection,
    epoch: &CanonicalInstant,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
) -> Result<RevisionProjectionEvidence, AdjustmentMaintenanceBodyError> {
    project_rain_stage(
        &document.logicalKey.runInitializedAt,
        &document.logicalReceivedAt,
        epoch,
        values,
    )
}

/// Apply the shared inclusive capture-epoch rule to one stage-only rain projection.
fn project_rain_stage(
    run_initialized_at: &str,
    logical_received_at: &str,
    epoch: &CanonicalInstant,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
) -> Result<RevisionProjectionEvidence, AdjustmentMaintenanceBodyError> {
    admit_revision_projection_clock(values, epoch, run_initialized_at)?;
    admit_revision_projection_clock(values, epoch, logical_received_at)?;
    Ok(RevisionProjectionEvidence {
        kind: AdjustmentRevisionProjectionKind::RainGateInput,
        pointers: Vec::new(),
    })
}
