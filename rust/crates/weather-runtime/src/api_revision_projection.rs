//! Derive the API's epoch and database pointer evidence from the original typed revision documents.

use weather_api::{
    AdjustmentMaintenanceBodyError, AdjustmentRevisionPublicationPointer, CanonicalInstant,
};
use weather_contracts::{
    AdjustmentRainFixedGaugeTargetProjection, AdjustmentRevisionProjectionDocument,
    AdjustmentRevisionProjectionKind,
};
use weather_database::AdjustmentRevisionAdmissionKey;

use crate::api_revision_group_projection::{project_fixed_gauge_revision, project_group_revision};
use crate::api_revision_rain_stage_projection::{
    project_rain_control_stage, project_rain_feature_stage,
};
use crate::api_revision_single_projection::{
    project_native_temperature_revision, project_single_revision,
};
use crate::maintenance_canonical_json::valid_hash;
use crate::{ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar};

type BodyError = AdjustmentMaintenanceBodyError;

/// The two original revision transport grammars, retaining their generated TS types.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiMaintenanceRevisionTransportDocument {
    Standard(Box<AdjustmentRevisionProjectionDocument>),
    FixedGauge(AdjustmentRainFixedGaugeTargetProjection),
}

/// The existing route chooses which original document variants are admissible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevisionRequestShape {
    Stage,
    Single,
    Grouped,
}

/// Rust-derived kind and optional ordered database marker/receipt pointers.
pub(crate) struct RevisionProjectionEvidence {
    pub kind: AdjustmentRevisionProjectionKind,
    pub pointers: Vec<AdjustmentRevisionPublicationPointer>,
}

/// Admit route shape first, then derive required clocks and exact ordered keys in Rust.
pub(crate) fn project_revision_document(
    document: &ApiMaintenanceRevisionTransportDocument,
    shape: RevisionRequestShape,
    epoch: &CanonicalInstant,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
) -> Result<RevisionProjectionEvidence, BodyError> {
    use AdjustmentRevisionProjectionDocument::{
        Batch, NativeTemperature, RainControl, RainFeature, Single,
    };
    use ApiMaintenanceRevisionTransportDocument::{FixedGauge, Standard};
    use RevisionRequestShape::{Grouped, Stage};

    let publish = shape != Stage;
    // stage accepts every original document, while publication has disjoint pointer shapes
    match (document, shape) {
        (Standard(document), shape) => match (document.as_ref(), shape) {
            (Single(document), Stage | RevisionRequestShape::Single) => {
                project_single_revision(document, epoch, values, publish)
            }
            (NativeTemperature(document), Stage | RevisionRequestShape::Single) => {
                project_native_temperature_revision(document, epoch, values, publish)
            }
            (Batch(document), Stage | Grouped) => {
                project_group_revision(document, epoch, values, publish)
            }
            (RainFeature(document), Stage) => project_rain_feature_stage(document, epoch, values),
            (RainControl(document), Stage) => project_rain_control_stage(document, epoch, values),
            _ => Err(AdjustmentMaintenanceBodyError),
        },
        (FixedGauge(document), Stage | Grouped) => {
            project_fixed_gauge_revision(document, epoch, values, publish)
        }
        _ => Err(AdjustmentMaintenanceBodyError),
    }
}

/// Check one parsed original clock at the inclusive capture boundary.
pub(crate) fn admit_revision_projection_clock(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    epoch: &CanonicalInstant,
    clock: &str,
) -> Result<(), BodyError> {
    values
        .canonical_utc_epoch_milliseconds(clock)
        .filter(|value| *value >= epoch.epoch_milliseconds)
        .map(|_| ())
        .ok_or(AdjustmentMaintenanceBodyError)
}

/// Compute a value-free gap identity from the Rust-selected original key order.
pub(crate) fn revision_pointer(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    key: AdjustmentRevisionAdmissionKey,
    fields: &[(&str, MaintenanceReceiptScalar<'_>)],
) -> Result<AdjustmentRevisionPublicationPointer, BodyError> {
    Ok(AdjustmentRevisionPublicationPointer {
        key,
        logical_key_sha256: revision_logical_key_sha256(values, fields)?,
    })
}

/// Hash borrowed fields before their owned SQL strings move into a pointer.
pub(crate) fn revision_logical_key_sha256(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    fields: &[(&str, MaintenanceReceiptScalar<'_>)],
) -> Result<String, BodyError> {
    values
        .unsigned_receipt_sha256(fields)
        .filter(|digest| valid_hash(digest.as_str()))
        .map(|digest| digest.as_str().to_owned())
        .ok_or(AdjustmentMaintenanceBodyError)
}

#[cfg(test)]
#[path = "api_revision_fixed_gauge_test.rs"]
mod fixed_gauge_tests;

#[cfg(test)]
#[path = "api_revision_rain_projection_test.rs"]
mod rain_tests;
