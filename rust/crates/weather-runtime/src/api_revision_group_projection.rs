//! Project grouped revision documents into capture clocks and ordered database pointers.

use weather_api::{AdjustmentMaintenanceBodyError, CanonicalInstant};
use weather_contracts::{
    AdjustmentRainFixedGaugeTargetProjection, AdjustmentRevisionBatchProjection,
    AdjustmentRevisionBatchProjectionLogicalKeyValue,
    AdjustmentRevisionBatchProjectionProjectionKindEnum,
    AdjustmentRevisionBatchProjectionRowsValue, AdjustmentRevisionProjectionKind,
    AdjustmentWeatherRevisionAdmissionKey, AdjustmentWeatherRevisionPointerSourceKindEnum,
    Nullable,
};
use weather_database::AdjustmentRevisionAdmissionKey;

use crate::api_revision_projection::{
    RevisionProjectionEvidence, admit_revision_projection_clock, revision_logical_key_sha256,
    revision_pointer,
};
use crate::{ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar};

type BodyError = AdjustmentMaintenanceBodyError;

/// Project one canonical grouped weather document in its original row order.
pub(crate) fn project_group_revision(
    document: &AdjustmentRevisionBatchProjection,
    epoch: &CanonicalInstant,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    publish: bool,
) -> Result<RevisionProjectionEvidence, BodyError> {
    let kind = match document.projectionKind {
        AdjustmentRevisionBatchProjectionProjectionKindEnum::ActualBestMatch => {
            AdjustmentRevisionProjectionKind::ActualBestMatch
        }
        AdjustmentRevisionBatchProjectionProjectionKindEnum::TargetRevision => {
            AdjustmentRevisionProjectionKind::TargetRevision
        }
    };
    let product_run_at = batch_nullable_text(&document.logicalKey, "productRunAt")?;
    // bind the source-specific causal clock without allocating a second product-run value
    match kind {
        AdjustmentRevisionProjectionKind::ActualBestMatch => match &product_run_at {
            Nullable::Value(value) => admit_revision_projection_clock(values, epoch, value)?,
            Nullable::Null => return Err(AdjustmentMaintenanceBodyError),
        },
        AdjustmentRevisionProjectionKind::TargetRevision => {
            let valid_at = batch_text(&document.logicalKey, "validAt")?;
            admit_revision_projection_clock(values, epoch, &valid_at)?;
        }
        _ => return Err(AdjustmentMaintenanceBodyError),
    }
    admit_revision_projection_clock(values, epoch, &document.logicalReceivedAt)?;
    // staging needs no database key projection after both document clocks are admitted
    if !publish {
        return Ok(RevisionProjectionEvidence {
            kind,
            pointers: Vec::new(),
        });
    }

    let source_id = batch_text(&document.logicalKey, "sourceId")?;
    let source_kind = batch_source_kind(&document.logicalKey)?;
    let mut pointers = Vec::with_capacity(document.rows.len());
    // retain canonical row order for database admission and later permanent gaps
    for row in &document.rows {
        let content_sha256 = batch_row_text(row, "contentSha256")?;
        let valid_at = batch_row_text(row, "validAt")?;
        let product_run_scalar = match &product_run_at {
            Nullable::Null => MaintenanceReceiptScalar::Null,
            Nullable::Value(value) => MaintenanceReceiptScalar::Text(value),
        };
        let fields = [
            ("productRunAt", product_run_scalar),
            ("sourceId", MaintenanceReceiptScalar::Text(&source_id)),
            (
                "sourceKind",
                MaintenanceReceiptScalar::Text(source_kind_wire_value(source_kind)),
            ),
            ("validAt", MaintenanceReceiptScalar::Text(&valid_at)),
        ];
        let logical_key_sha256 = revision_logical_key_sha256(values, &fields)?;
        let key =
            AdjustmentRevisionAdmissionKey::WeatherRecord(AdjustmentWeatherRevisionAdmissionKey {
                productRunAt: product_run_at.clone(),
                sourceId: source_id.clone(),
                sourceKind: source_kind,
                storedContentSha256: content_sha256,
                validAt: valid_at,
            });
        pointers.push(weather_api::AdjustmentRevisionPublicationPointer {
            key,
            logical_key_sha256,
        });
    }
    // the canonical decoder already requires a nonempty bounded grouped body
    if pointers.is_empty() {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(RevisionProjectionEvidence { kind, pointers })
}

/// Project one fixed-gauge target body with one logical identity shared by all rows.
pub(crate) fn project_fixed_gauge_revision(
    document: &AdjustmentRainFixedGaugeTargetProjection,
    epoch: &CanonicalInstant,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    publish: bool,
) -> Result<RevisionProjectionEvidence, BodyError> {
    admit_revision_projection_clock(values, epoch, &document.validAt)?;
    admit_revision_projection_clock(values, epoch, &document.logicalReceivedAt)?;
    // bind every raw claim clock before the body can be staged or published
    for body in &document.captureBodies {
        // preserve each claim's completed and complete half-open window clocks
        for claim in &body.claims {
            admit_revision_projection_clock(values, epoch, &claim.completedAt)?;
            admit_revision_projection_clock(values, epoch, &claim.windowStart)?;
            admit_revision_projection_clock(values, epoch, &claim.windowEndExclusive)?;
        }
    }
    let kind = AdjustmentRevisionProjectionKind::TargetRevision;
    // staging deliberately retains no pointer or logical-key digest
    if !publish {
        return Ok(RevisionProjectionEvidence {
            kind,
            pointers: Vec::new(),
        });
    }

    let source_ids: Vec<_> = document
        .rows
        .iter()
        .map(|row| row.normalizedRecord.sourceId.clone())
        .collect();
    let fields = [
        (
            "contractVersion",
            MaintenanceReceiptScalar::Text("adjustment-rain-fixed-gauge-target-logical-key/v1"),
        ),
        (
            "sourceIds",
            MaintenanceReceiptScalar::TextArray(&source_ids),
        ),
        ("validAt", MaintenanceReceiptScalar::Text(&document.validAt)),
    ];
    let mut keys = document.rows.iter().map(|row| {
        AdjustmentRevisionAdmissionKey::WeatherRecord(AdjustmentWeatherRevisionAdmissionKey {
            productRunAt: Nullable::Null,
            sourceId: row.normalizedRecord.sourceId.clone(),
            sourceKind: AdjustmentWeatherRevisionPointerSourceKindEnum::PhysicalSensor,
            storedContentSha256: row.storedContentSha256.clone(),
            validAt: document.validAt.clone(),
        })
    });
    let first_key = keys.next().ok_or(AdjustmentMaintenanceBodyError)?;
    let first = revision_pointer(values, first_key, &fields)?;
    let shared_identity = first.logical_key_sha256.clone();
    let mut pointers = Vec::with_capacity(document.rows.len());
    pointers.push(first);
    // reuse the one fixed-catalog identity for every remaining ordered gauge row
    for key in keys {
        pointers.push(weather_api::AdjustmentRevisionPublicationPointer {
            key,
            logical_key_sha256: shared_identity.clone(),
        });
    }
    Ok(RevisionProjectionEvidence { kind, pointers })
}

/// Borrow one named grouped logical-key member without normalizing its UTF-16 spelling.
fn batch_logical_value<'a>(
    fields: &'a [(Vec<u16>, AdjustmentRevisionBatchProjectionLogicalKeyValue)],
    key: &str,
) -> Result<&'a AdjustmentRevisionBatchProjectionLogicalKeyValue, BodyError> {
    fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
        .ok_or(AdjustmentMaintenanceBodyError)
}

/// Decode one required grouped logical-key string.
fn batch_text(
    fields: &[(Vec<u16>, AdjustmentRevisionBatchProjectionLogicalKeyValue)],
    key: &str,
) -> Result<String, BodyError> {
    let AdjustmentRevisionBatchProjectionLogicalKeyValue::String(value) =
        batch_logical_value(fields, key)?
    else {
        return Err(AdjustmentMaintenanceBodyError);
    };
    String::from_utf16(value).map_err(|_| AdjustmentMaintenanceBodyError)
}

/// Decode the nullable grouped product-run clock without accepting numeric substitution.
fn batch_nullable_text(
    fields: &[(Vec<u16>, AdjustmentRevisionBatchProjectionLogicalKeyValue)],
    key: &str,
) -> Result<Nullable<String>, BodyError> {
    match batch_logical_value(fields, key)? {
        AdjustmentRevisionBatchProjectionLogicalKeyValue::Null => Ok(Nullable::Null),
        AdjustmentRevisionBatchProjectionLogicalKeyValue::String(value) => {
            String::from_utf16(value)
                .map(Nullable::Value)
                .map_err(|_| AdjustmentMaintenanceBodyError)
        }
        AdjustmentRevisionBatchProjectionLogicalKeyValue::Number(_) => {
            Err(AdjustmentMaintenanceBodyError)
        }
    }
}

/// Decode the grouped weather source kind from its exact logical-key spelling.
fn batch_source_kind(
    fields: &[(Vec<u16>, AdjustmentRevisionBatchProjectionLogicalKeyValue)],
) -> Result<AdjustmentWeatherRevisionPointerSourceKindEnum, BodyError> {
    match batch_text(fields, "sourceKind")?.as_str() {
        "forecast" => Ok(AdjustmentWeatherRevisionPointerSourceKindEnum::Forecast),
        "physical_sensor" => Ok(AdjustmentWeatherRevisionPointerSourceKindEnum::PhysicalSensor),
        _ => Err(AdjustmentMaintenanceBodyError),
    }
}

/// Decode one required grouped row string.
fn batch_row_text(
    fields: &[(Vec<u16>, AdjustmentRevisionBatchProjectionRowsValue)],
    key: &str,
) -> Result<String, BodyError> {
    let value = fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
        .ok_or(AdjustmentMaintenanceBodyError)?;
    let AdjustmentRevisionBatchProjectionRowsValue::String(value) = value else {
        return Err(AdjustmentMaintenanceBodyError);
    };
    String::from_utf16(value).map_err(|_| AdjustmentMaintenanceBodyError)
}

/// Render one closed weather pointer source kind with its original wire spelling.
const fn source_kind_wire_value(
    kind: AdjustmentWeatherRevisionPointerSourceKindEnum,
) -> &'static str {
    match kind {
        AdjustmentWeatherRevisionPointerSourceKindEnum::Forecast => "forecast",
        AdjustmentWeatherRevisionPointerSourceKindEnum::PhysicalSensor => "physical_sensor",
    }
}
