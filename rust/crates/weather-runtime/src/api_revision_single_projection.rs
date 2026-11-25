//! Capture-epoch admission and database pointers for single revision projections.

use weather_api::{
    AdjustmentMaintenanceBodyError, AdjustmentRevisionPublicationPointer, CanonicalInstant,
};
use weather_contracts::{
    AdjustmentEcmwfTemperatureRevisionAdmissionKey, AdjustmentRainGateRevisionAdmissionKey,
    AdjustmentRevisionProjection, AdjustmentRevisionProjectionKind,
    AdjustmentRevisionProjectionLogicalKeyValue, AdjustmentRevisionProjectionRowsValue,
    AdjustmentTemperatureNativeSourceProjection, AdjustmentWeatherRevisionAdmissionKey,
    AdjustmentWeatherRevisionPointerSourceKindEnum, Nullable,
};
use weather_database::AdjustmentRevisionAdmissionKey;

use crate::api_revision_projection::{
    RevisionProjectionEvidence, admit_revision_projection_clock, revision_logical_key_sha256,
    revision_pointer,
};
use crate::{ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar};

type BodyError = AdjustmentMaintenanceBodyError;
type LogicalValue = AdjustmentRevisionProjectionLogicalKeyValue;

const WEATHER_KEY_FIELDS: &[&str] = &["productRunAt", "sourceId", "sourceKind", "validAt"];
const NATIVE_KEY_FIELDS: &[&str] = &[
    "contentSha256",
    "leadHours",
    "providerResponseSha256",
    "runInitializedAt",
    "siteId",
    "sourceId",
    "sourceType",
    "validAt",
];
const RAIN_GATE_KEY_FIELDS: &[&str] = &["inputSha256", "modelSha256", "runInitializedAt"];

/// Admit one original v1 single projection and derive its optional publication pointer.
pub(crate) fn project_single_revision(
    document: &AdjustmentRevisionProjection,
    epoch: &CanonicalInstant,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    publish: bool,
) -> Result<RevisionProjectionEvidence, BodyError> {
    let source_clock = match document.projectionKind {
        AdjustmentRevisionProjectionKind::ActualBestMatch => {
            logical_text(&document.logicalKey, "productRunAt")?
        }
        AdjustmentRevisionProjectionKind::TargetRevision => {
            logical_text(&document.logicalKey, "validAt")?
        }
        AdjustmentRevisionProjectionKind::NativeSource
        | AdjustmentRevisionProjectionKind::RainGateInput => {
            logical_text(&document.logicalKey, "runInitializedAt")?
        }
    };
    admit_revision_projection_clock(values, epoch, &source_clock)?;
    admit_revision_projection_clock(values, epoch, &document.logicalReceivedAt)?;
    let mut pointers = Vec::new();
    // defer all logical-key and digest work until the publication path needs a pointer
    if publish {
        pointers.push(single_pointer(document, values)?);
    }
    Ok(RevisionProjectionEvidence {
        kind: document.projectionKind,
        pointers,
    })
}

/// Admit one original native-temperature v2 projection and derive its publication pointer.
pub(crate) fn project_native_temperature_revision(
    document: &AdjustmentTemperatureNativeSourceProjection,
    epoch: &CanonicalInstant,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    publish: bool,
) -> Result<RevisionProjectionEvidence, BodyError> {
    let source_clock = logical_text(&document.logicalKey, "runInitializedAt")?;
    admit_revision_projection_clock(values, epoch, &source_clock)?;
    admit_revision_projection_clock(values, epoch, &document.logicalReceivedAt)?;
    // admit every retained serving-comparator product clock without inventing one for nulls
    for row in &document.rows {
        match row_value(row, "bestMatchProductRunAt")? {
            AdjustmentRevisionProjectionRowsValue::Null => {}
            AdjustmentRevisionProjectionRowsValue::String(clock) => {
                let clock =
                    String::from_utf16(clock).map_err(|_| AdjustmentMaintenanceBodyError)?;
                admit_revision_projection_clock(values, epoch, &clock)?;
            }
            _ => return Err(AdjustmentMaintenanceBodyError),
        }
    }
    let mut pointers = Vec::new();
    // defer all logical-key and digest work until the publication path needs a pointer
    if publish {
        pointers.push(native_pointer(
            &document.logicalKey,
            &document.storedContentSha256,
            values,
        )?);
    }
    Ok(RevisionProjectionEvidence {
        kind: AdjustmentRevisionProjectionKind::NativeSource,
        pointers,
    })
}

/// Select the exact SQL facade and logical-key preimage for one v1 projection.
fn single_pointer(
    document: &AdjustmentRevisionProjection,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
) -> Result<AdjustmentRevisionPublicationPointer, BodyError> {
    match document.projectionKind {
        AdjustmentRevisionProjectionKind::ActualBestMatch
        | AdjustmentRevisionProjectionKind::TargetRevision => weather_pointer(document, values),
        AdjustmentRevisionProjectionKind::NativeSource => {
            native_pointer(&document.logicalKey, &document.storedContentSha256, values)
        }
        AdjustmentRevisionProjectionKind::RainGateInput => rain_gate_pointer(document, values),
    }
}

/// Derive one weather-record current pointer in frozen logical-key field order.
fn weather_pointer(
    document: &AdjustmentRevisionProjection,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
) -> Result<AdjustmentRevisionPublicationPointer, BodyError> {
    require_logical_fields(&document.logicalKey, WEATHER_KEY_FIELDS)?;
    let product_run_at = logical_nullable_text(&document.logicalKey, "productRunAt")?;
    let source_id = logical_text(&document.logicalKey, "sourceId")?;
    let source_kind = logical_text(&document.logicalKey, "sourceKind")?;
    let valid_at = logical_text(&document.logicalKey, "validAt")?;
    let admitted_kind = match document.projectionKind {
        AdjustmentRevisionProjectionKind::ActualBestMatch
            if matches!(&product_run_at, Nullable::Value(_)) && source_kind == "forecast" =>
        {
            AdjustmentWeatherRevisionPointerSourceKindEnum::Forecast
        }
        AdjustmentRevisionProjectionKind::TargetRevision
            if matches!(&product_run_at, Nullable::Null) && source_kind == "physical_sensor" =>
        {
            AdjustmentWeatherRevisionPointerSourceKindEnum::PhysicalSensor
        }
        _ => return Err(AdjustmentMaintenanceBodyError),
    };
    let product_run_at_scalar = match &product_run_at {
        Nullable::Null => MaintenanceReceiptScalar::Null,
        Nullable::Value(value) => MaintenanceReceiptScalar::Text(value),
    };
    let fields = [
        ("productRunAt", product_run_at_scalar),
        ("sourceId", MaintenanceReceiptScalar::Text(&source_id)),
        ("sourceKind", MaintenanceReceiptScalar::Text(&source_kind)),
        ("validAt", MaintenanceReceiptScalar::Text(&valid_at)),
    ];
    let logical_key_sha256 = revision_logical_key_sha256(values, &fields)?;
    Ok(AdjustmentRevisionPublicationPointer {
        key: AdjustmentRevisionAdmissionKey::WeatherRecord(AdjustmentWeatherRevisionAdmissionKey {
            productRunAt: product_run_at,
            sourceId: source_id,
            sourceKind: admitted_kind,
            storedContentSha256: document.storedContentSha256.clone(),
            validAt: valid_at,
        }),
        logical_key_sha256,
    })
}

/// Derive one native-source pointer for a fixed anchor or complete temperature run.
fn native_pointer(
    logical_key: &[(Vec<u16>, LogicalValue)],
    stored_content_sha256: &str,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
) -> Result<AdjustmentRevisionPublicationPointer, BodyError> {
    require_logical_fields(logical_key, NATIVE_KEY_FIELDS)?;
    let content_sha256 = logical_nullable_text(logical_key, "contentSha256")?;
    let lead_hours = logical_nullable_unsigned(logical_key, "leadHours")?;
    let provider_response_sha256 = logical_nullable_text(logical_key, "providerResponseSha256")?;
    let run_initialized_at = logical_nullable_text(logical_key, "runInitializedAt")?;
    let site_id = logical_nullable_text(logical_key, "siteId")?;
    let source_id = logical_nullable_text(logical_key, "sourceId")?;
    let source_type = logical_text(logical_key, "sourceType")?;
    let valid_at = logical_nullable_text(logical_key, "validAt")?;
    let (
        Nullable::Value(_),
        Nullable::Value(provider_response),
        Nullable::Value(run_initialized),
        Nullable::Value(site),
    ) = (
        &content_sha256,
        &provider_response_sha256,
        &run_initialized_at,
        &site_id,
    )
    else {
        return Err(AdjustmentMaintenanceBodyError);
    };
    // the epoch already makes legacy forecast anchors unreachable at this API boundary
    if source_type != "ecmwf_temperature_run"
        || !matches!(&lead_hours, Nullable::Null)
        || !matches!(&source_id, Nullable::Null)
        || !matches!(&valid_at, Nullable::Null)
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    let key = AdjustmentRevisionAdmissionKey::EcmwfTemperature(
        AdjustmentEcmwfTemperatureRevisionAdmissionKey {
            providerResponseSha256: provider_response.clone(),
            runInitializedAt: run_initialized.clone(),
            siteId: site.clone(),
            storedContentSha256: stored_content_sha256.to_owned(),
        },
    );
    let fields = [
        ("contentSha256", nullable_text_scalar(&content_sha256)),
        ("leadHours", nullable_unsigned_scalar(&lead_hours)),
        (
            "providerResponseSha256",
            nullable_text_scalar(&provider_response_sha256),
        ),
        (
            "runInitializedAt",
            nullable_text_scalar(&run_initialized_at),
        ),
        ("siteId", nullable_text_scalar(&site_id)),
        ("sourceId", nullable_text_scalar(&source_id)),
        ("sourceType", MaintenanceReceiptScalar::Text(&source_type)),
        ("validAt", nullable_text_scalar(&valid_at)),
    ];
    revision_pointer(values, key, &fields)
}

/// Derive one immutable rain-gate pointer in frozen logical-key field order.
fn rain_gate_pointer(
    document: &AdjustmentRevisionProjection,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
) -> Result<AdjustmentRevisionPublicationPointer, BodyError> {
    require_logical_fields(&document.logicalKey, RAIN_GATE_KEY_FIELDS)?;
    let input_sha256 = logical_text(&document.logicalKey, "inputSha256")?;
    let model_sha256 = logical_text(&document.logicalKey, "modelSha256")?;
    let run_initialized_at = logical_text(&document.logicalKey, "runInitializedAt")?;
    let fields = [
        ("inputSha256", MaintenanceReceiptScalar::Text(&input_sha256)),
        ("modelSha256", MaintenanceReceiptScalar::Text(&model_sha256)),
        (
            "runInitializedAt",
            MaintenanceReceiptScalar::Text(&run_initialized_at),
        ),
    ];
    let logical_key_sha256 = revision_logical_key_sha256(values, &fields)?;
    Ok(AdjustmentRevisionPublicationPointer {
        key: AdjustmentRevisionAdmissionKey::RainGate(AdjustmentRainGateRevisionAdmissionKey {
            inputSha256: input_sha256,
            modelSha256: model_sha256,
            runInitializedAt: run_initialized_at,
            storedContentSha256: document.storedContentSha256.clone(),
        }),
        logical_key_sha256,
    })
}

/// Require each frozen logical-key member exactly once without assuming source vector order.
fn require_logical_fields(
    logical_key: &[(Vec<u16>, LogicalValue)],
    fields: &[&str],
) -> Result<(), BodyError> {
    // reject missing, duplicate, and extension key members before pointer derivation
    if logical_key.len() != fields.len()
        || fields.iter().any(|field| {
            logical_key
                .iter()
                .filter(|(name, _)| utf16_eq(name, field))
                .count()
                != 1
        })
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(())
}

/// Borrow one exact logical-key scalar by its original UTF-16 member name.
fn logical_value<'a>(
    logical_key: &'a [(Vec<u16>, LogicalValue)],
    key: &str,
) -> Result<&'a LogicalValue, BodyError> {
    logical_key
        .iter()
        .find_map(|(name, value)| utf16_eq(name, key).then_some(value))
        .ok_or(AdjustmentMaintenanceBodyError)
}

/// Decode one required logical-key UTF-16 string without replacement.
fn logical_text(logical_key: &[(Vec<u16>, LogicalValue)], key: &str) -> Result<String, BodyError> {
    let LogicalValue::String(value) = logical_value(logical_key, key)? else {
        return Err(AdjustmentMaintenanceBodyError);
    };
    String::from_utf16(value).map_err(|_| AdjustmentMaintenanceBodyError)
}

/// Decode one nullable logical-key UTF-16 string without replacement.
fn logical_nullable_text(
    logical_key: &[(Vec<u16>, LogicalValue)],
    key: &str,
) -> Result<Nullable<String>, BodyError> {
    match logical_value(logical_key, key)? {
        LogicalValue::Null => Ok(Nullable::Null),
        LogicalValue::String(value) => String::from_utf16(value)
            .map(Nullable::Value)
            .map_err(|_| AdjustmentMaintenanceBodyError),
        LogicalValue::Number(_) => Err(AdjustmentMaintenanceBodyError),
    }
}

/// Decode one nullable safe unsigned logical-key number.
fn logical_nullable_unsigned(
    logical_key: &[(Vec<u16>, LogicalValue)],
    key: &str,
) -> Result<Nullable<u64>, BodyError> {
    match logical_value(logical_key, key)? {
        LogicalValue::Null => Ok(Nullable::Null),
        LogicalValue::Number(value)
            if value.is_finite()
                && *value >= 0.0
                && *value <= 9_007_199_254_740_991.0
                && value.fract() == 0.0 =>
        {
            Ok(Nullable::Value(*value as u64))
        }
        _ => Err(AdjustmentMaintenanceBodyError),
    }
}

/// Borrow one exact dynamic row scalar by its original UTF-16 member name.
fn row_value<'a>(
    row: &'a [(Vec<u16>, AdjustmentRevisionProjectionRowsValue)],
    key: &str,
) -> Result<&'a AdjustmentRevisionProjectionRowsValue, BodyError> {
    row.iter()
        .find_map(|(name, value)| utf16_eq(name, key).then_some(value))
        .ok_or(AdjustmentMaintenanceBodyError)
}

/// Compare one generated UTF-16 member name with an exact source key.
fn utf16_eq(value: &[u16], expected: &str) -> bool {
    value.iter().copied().eq(expected.encode_utf16())
}

/// Borrow one nullable text value as an ordered canonical scalar.
fn nullable_text_scalar(value: &Nullable<String>) -> MaintenanceReceiptScalar<'_> {
    match value {
        Nullable::Null => MaintenanceReceiptScalar::Null,
        Nullable::Value(value) => MaintenanceReceiptScalar::Text(value),
    }
}

/// Borrow one nullable unsigned value as an ordered canonical scalar.
fn nullable_unsigned_scalar(value: &Nullable<u64>) -> MaintenanceReceiptScalar<'_> {
    match value {
        Nullable::Null => MaintenanceReceiptScalar::Null,
        Nullable::Value(value) => MaintenanceReceiptScalar::Unsigned(*value),
    }
}
