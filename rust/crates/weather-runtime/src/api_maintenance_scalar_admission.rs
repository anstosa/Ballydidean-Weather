//! admit value-free maintenance inputs and fixed-gauge acknowledgement scalars

use weather_api::{
    AdjustmentMaintenanceBodyError, AdjustmentMaintenanceCaptureCycle,
    AdmittedAdjustmentMaintenanceCapture, AdmittedAdjustmentMaintenanceGap,
    AdmittedAdjustmentRevisionGap, AdmittedRainFixedGaugeTargetGap,
    AdmittedRainFixedGaugeTargetGapStatus, AdmittedRainMaintenanceControlStateGap,
    CanonicalInstant,
};
use weather_contracts::{
    AdjustmentMaintenanceCaptureRequest, AdjustmentMaintenanceGapInput, AdjustmentRevisionGapInput,
    Nullable, RainFixedGaugeTargetGapInput, RainFixedGaugeTargetGapReceipt,
    RainFixedGaugeTargetGapReceiptContractVersionLiteral,
    RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral, RainFixedGaugeTargetGapStatus,
    RainFixedGaugeTargetGapStatusInput, RainFixedGaugeTargetGapStatusPresentGap,
    RainFixedGaugeTargetGapStatusPresentGapContractVersionLiteral,
    RainFixedGaugeTargetGapStatusPresentGapQualificationDispositionLiteral,
    RainFixedGaugeTargetGapStatusPresentGapReasonLiteral, RainMaintenanceControlStateGapInput,
};

use crate::maintenance_canonical_json::valid_hash;
use crate::{ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar};

const HOUR_MILLISECONDS: i64 = 3_600_000;

/// admit one fixed scheduler capture request after canonical utc resolution
pub(crate) fn admit_api_capture(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    request: AdjustmentMaintenanceCaptureRequest,
) -> Result<AdmittedAdjustmentMaintenanceCapture, AdjustmentMaintenanceBodyError> {
    let cycle = capture_cycle(&request.dueKey).ok_or(AdjustmentMaintenanceBodyError)?;
    let due_issued_at = request
        .dueKey
        .strip_prefix("capture/")
        .ok_or(AdjustmentMaintenanceBodyError)?;
    let epoch_milliseconds = values
        .canonical_utc_epoch_milliseconds(&request.issuedAt)
        .ok_or(AdjustmentMaintenanceBodyError)?;
    // bind the immutable scheduler key to the same admitted issue instant
    if due_issued_at != request.issuedAt {
        return Err(AdjustmentMaintenanceBodyError);
    }
    let issued_at = CanonicalInstant {
        epoch_milliseconds,
        utc: request.issuedAt.clone(),
    };
    Ok(AdmittedAdjustmentMaintenanceCapture {
        request,
        issued_at,
        cycle,
    })
}

/// admit one bounded categorical shadow gap without interpreting its due clock
pub(crate) fn admit_api_shadow_gap(
    request: AdjustmentMaintenanceGapInput,
) -> Result<AdmittedAdjustmentMaintenanceGap, AdjustmentMaintenanceBodyError> {
    // retain node's shape-only scheduler key admission for value-free gaps
    if capture_cycle(&request.dueKey).is_none() || !valid_hash(&request.registrationSha256) {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(AdmittedAdjustmentMaintenanceGap { request })
}

/// admit one frozen value-free revision gap identity
pub(crate) fn admit_api_revision_gap(
    request: AdjustmentRevisionGapInput,
) -> Result<AdmittedAdjustmentRevisionGap, AdjustmentMaintenanceBodyError> {
    // require paired nullable projection identities and their original equality
    let projection_matches = match (&request.projectionSha256, &request.projectionIdentitySha256) {
        (Nullable::Null, Nullable::Null) => true,
        (Nullable::Value(projection), Nullable::Value(identity)) => {
            valid_hash(projection) && valid_hash(identity) && projection == identity
        }
        _ => false,
    };
    if !valid_hash(&request.logicalKeySha256) || !projection_matches {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(AdmittedAdjustmentRevisionGap { request })
}

/// admit one exact rain control-state staging gap
pub(crate) fn admit_api_rain_control_gap(
    request: RainMaintenanceControlStateGapInput,
) -> Result<AdmittedRainMaintenanceControlStateGap, AdjustmentMaintenanceBodyError> {
    // the generated reason is closed, leaving only the two digest identities
    if !valid_hash(&request.stateSha256) || !valid_hash(&request.stageReceiptSha256) {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(AdmittedRainMaintenanceControlStateGap { request })
}

/// admit one oversized fixed-gauge target disposition at an exact utc hour
pub(crate) fn admit_api_fixed_gauge_gap(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    request: RainFixedGaugeTargetGapInput,
) -> Result<AdmittedRainFixedGaugeTargetGap, AdjustmentMaintenanceBodyError> {
    // require the canonical closed hour and immutable logical target identity
    if !valid_closed_hour(values, &request.logicalHourAt) || !valid_hash(&request.logicalKeySha256)
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(AdmittedRainFixedGaugeTargetGap { request })
}

/// admit one exact fixed-gauge durable-gap lookup key
pub(crate) fn admit_api_fixed_gauge_status(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    request: RainFixedGaugeTargetGapStatusInput,
) -> Result<AdmittedRainFixedGaugeTargetGapStatus, AdjustmentMaintenanceBodyError> {
    // require the same canonical hour and logical identity as the gap writer
    if !valid_closed_hour(values, &request.logicalHourAt) || !valid_hash(&request.logicalKeySha256)
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(AdmittedRainFixedGaugeTargetGapStatus { request })
}

/// revalidate one durable fixed-gauge receipt against its admitted request
pub(crate) fn admit_api_fixed_gauge_receipt(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    request: &AdmittedRainFixedGaugeTargetGap,
    receipt: RainFixedGaugeTargetGapReceipt,
) -> Result<RainFixedGaugeTargetGapReceipt, AdjustmentMaintenanceBodyError> {
    // reject relay output for another logical target before trusting its self-address
    if receipt.logicalHourAt != request.request.logicalHourAt
        || receipt.logicalKeySha256 != request.request.logicalKeySha256
        || receipt.reason != request.request.reason
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    admit_fixed_gauge_receipt_values(
        values,
        RainFixedGaugeTargetGapReceiptContractVersionLiteral::WIRE_VALUE,
        &receipt.gapAt,
        &receipt.logicalHourAt,
        &receipt.logicalKeySha256,
        RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral::WIRE_VALUE,
        weather_contracts::RainFixedGaugeTargetGapInputReasonLiteral::WIRE_VALUE,
        &receipt.gapSha256,
    )?;
    Ok(receipt)
}

/// revalidate one fixed-gauge status outcome against its admitted lookup key
pub(crate) fn admit_api_fixed_gauge_outcome(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    request: &AdmittedRainFixedGaugeTargetGapStatus,
    status: RainFixedGaugeTargetGapStatus,
) -> Result<RainFixedGaugeTargetGapStatus, AdjustmentMaintenanceBodyError> {
    // absent is a complete generated outcome while present carries a durable receipt
    if let RainFixedGaugeTargetGapStatus::Present(present) = &status {
        admit_present_gap(values, request, &present.gap)?;
    }
    Ok(status)
}

/// validate one present status receipt without changing its generated wire type
fn admit_present_gap(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    request: &AdmittedRainFixedGaugeTargetGapStatus,
    gap: &RainFixedGaugeTargetGapStatusPresentGap,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    // bind the returned durable gap to the exact lookup hour and key
    if gap.logicalHourAt != request.request.logicalHourAt
        || gap.logicalKeySha256 != request.request.logicalKeySha256
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    admit_fixed_gauge_receipt_values(
        values,
        RainFixedGaugeTargetGapStatusPresentGapContractVersionLiteral::WIRE_VALUE,
        &gap.gapAt,
        &gap.logicalHourAt,
        &gap.logicalKeySha256,
        RainFixedGaugeTargetGapStatusPresentGapQualificationDispositionLiteral::WIRE_VALUE,
        RainFixedGaugeTargetGapStatusPresentGapReasonLiteral::WIRE_VALUE,
        &gap.gapSha256,
    )
}

/// validate the canonical gap clock and exact ordered unsigned receipt address
#[allow(clippy::too_many_arguments)]
fn admit_fixed_gauge_receipt_values(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    contract_version: &str,
    gap_at: &str,
    logical_hour_at: &str,
    logical_key_sha256: &str,
    qualification_disposition: &str,
    reason: &str,
    gap_sha256: &str,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    // close the relay clock and digest grammar before invoking canonical hashing
    if values.canonical_utc_epoch_milliseconds(gap_at).is_none() || !valid_hash(gap_sha256) {
        return Err(AdjustmentMaintenanceBodyError);
    }
    let fields = [
        (
            "contractVersion",
            MaintenanceReceiptScalar::Text(contract_version),
        ),
        ("gapAt", MaintenanceReceiptScalar::Text(gap_at)),
        (
            "logicalHourAt",
            MaintenanceReceiptScalar::Text(logical_hour_at),
        ),
        (
            "logicalKeySha256",
            MaintenanceReceiptScalar::Text(logical_key_sha256),
        ),
        (
            "qualificationDisposition",
            MaintenanceReceiptScalar::Text(qualification_disposition),
        ),
        ("reason", MaintenanceReceiptScalar::Text(reason)),
    ];
    if !values
        .unsigned_receipt_sha256(&fields)
        .is_some_and(|digest| digest.as_str() == gap_sha256)
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(())
}

/// parse the closed scheduler due-key shape and fixed six-hour cycle
pub(crate) fn capture_cycle(due_key: &str) -> Option<AdjustmentMaintenanceCaptureCycle> {
    let issued_at = due_key.strip_prefix("capture/")?;
    let bytes = issued_at.as_bytes();
    // preserve the original regex shape without adding calendar validity to gap keys
    if bytes.len() != 24
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || bytes[4] != b'-'
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || bytes[7] != b'-'
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
        || bytes[10] != b'T'
        || &bytes[13..] != b":35:00.000Z"
    {
        return None;
    }
    match &bytes[11..13] {
        b"00" => Some(AdjustmentMaintenanceCaptureCycle::Hour00),
        b"06" => Some(AdjustmentMaintenanceCaptureCycle::Hour06),
        b"12" => Some(AdjustmentMaintenanceCaptureCycle::Hour12),
        b"18" => Some(AdjustmentMaintenanceCaptureCycle::Hour18),
        _ => None,
    }
}

/// admit only exact canonical utc hour boundaries
fn valid_closed_hour(values: &impl ApiMaintenanceReceiptCanonicalValues, value: &str) -> bool {
    values
        .canonical_utc_epoch_milliseconds(value)
        .is_some_and(|epoch| epoch % HOUR_MILLISECONDS == 0)
}
