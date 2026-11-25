//! Project already-verified adjustment rows into typed rain archive material.

use std::fmt::{Display, Formatter};

use weather_contracts::{
    JsonValue, Nullable, RainAdjustmentCaptureKindEnum, RainAdjustmentHour, RainAdjustmentRun,
};

use crate::{
    AdmittedRainArchiveClaim, AdmittedRainArchiveReceipt, AdmittedRainArchiveReceiptOutcome,
    AdmittedRainArchiveRun,
};

const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Approved JavaScript-compatible timestamp parsing and canonical UTC formatting.
pub trait RainPerformanceTimestampValues {
    /// Parse one timestamp with the original `Date.parse` millisecond behavior.
    fn parse_js_timestamp_ms(&mut self, value: &[u16]) -> Option<i64>;

    /// Format one epoch millisecond as the original canonical UTC ISO spelling.
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Option<Vec<u16>>;
}

/// Typed rain rows retained from one already-authenticated adjustment package.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainPerformanceAdjustmentRows {
    pub claims: Vec<AdmittedRainArchiveClaim>,
    pub receipts: Vec<AdmittedRainArchiveReceipt>,
    pub runs: Vec<AdmittedRainArchiveRun>,
}

/// Closed projection failures after the outer package verifier succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RainPerformanceAdjustmentRowsError {
    InvalidClaim,
    InvalidReceipt,
    InvalidRow,
    InvalidRun,
    InvalidTimestamp,
    TimestampFormat,
}

impl Display for RainPerformanceAdjustmentRowsError {
    /// Preserve the original projection diagnostic at the loader boundary.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidClaim => "rain claim payload is invalid",
            Self::InvalidReceipt => "rain receipt payload is invalid",
            Self::InvalidRow => "adjustment package row must be an object",
            Self::InvalidRun => "rain adjustment payload is invalid",
            Self::InvalidTimestamp => "verified performance timestamp is invalid",
            Self::TimestampFormat => "verified performance timestamp formatting failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for RainPerformanceAdjustmentRowsError {}

/// Normalize every verified timestamp field while retaining the generic package rows.
pub fn normalize_admitted_rain_performance_adjustment_rows(
    rows: Vec<JsonValue>,
    values: &mut impl RainPerformanceTimestampValues,
) -> Result<Vec<JsonValue>, RainPerformanceAdjustmentRowsError> {
    let mut normalized_rows = Vec::with_capacity(rows.len());
    // preserve package order while normalizing every verified row
    for row in rows {
        let normalized = normalize_verified_performance_timestamps(row, values)?;
        object_entries(&normalized).ok_or(RainPerformanceAdjustmentRowsError::InvalidRow)?;
        normalized_rows.push(normalized);
    }
    Ok(normalized_rows)
}

/// Project already-normalized generic rows into the relevant typed rain archive rows.
pub fn project_normalized_rain_performance_adjustment_rows(
    rows: Vec<JsonValue>,
) -> Result<AdmittedRainPerformanceAdjustmentRows, RainPerformanceAdjustmentRowsError> {
    let mut projected = AdmittedRainPerformanceAdjustmentRows {
        claims: Vec::new(),
        receipts: Vec::new(),
        runs: Vec::new(),
    };

    // preserve normalized package order while selecting relevant rain records
    for row in &rows {
        let entries = object_entries(row).ok_or(RainPerformanceAdjustmentRowsError::InvalidRow)?;
        let Some(record_kind) = optional_text_field(entries, "record_kind") else {
            continue;
        };

        // project only the three rain record kinds used by replay
        match record_kind.as_str() {
            "rain_claim" => projected.claims.push(project_claim(entries)?),
            "rain_receipt" => {
                // retain only parser-valid receipts like the original capture join
                if let Some(receipt) = project_valid_receipt(entries)? {
                    projected.receipts.push(receipt);
                }
            }
            "rain_adjustment_run" => projected.runs.push(project_run(entries)?),
            _ => {}
        }
    }

    Ok(projected)
}

/// Preserve the original combined normalization and typed-projection entrypoint.
pub fn project_admitted_rain_performance_adjustment_rows(
    rows: Vec<JsonValue>,
    values: &mut impl RainPerformanceTimestampValues,
) -> Result<AdmittedRainPerformanceAdjustmentRows, RainPerformanceAdjustmentRowsError> {
    let rows = normalize_admitted_rain_performance_adjustment_rows(rows, values)?;
    project_normalized_rain_performance_adjustment_rows(rows)
}

/// Recursively normalize the timestamp spelling of one already-verified JSON value.
pub fn normalize_verified_performance_timestamps(
    value: JsonValue,
    values: &mut impl RainPerformanceTimestampValues,
) -> Result<JsonValue, RainPerformanceAdjustmentRowsError> {
    normalize_verified_performance_timestamps_for_key(value, &[], values)
}

/// Normalize one named string field through the same verified timestamp rule.
pub fn normalize_verified_performance_timestamp_field(
    key: &[u16],
    value: Vec<u16>,
    values: &mut impl RainPerformanceTimestampValues,
) -> Result<Vec<u16>, RainPerformanceAdjustmentRowsError> {
    // preserve local dates, identifiers, and non-timestamp strings
    if !has_timestamp_suffix(key) || !has_iso_datetime_prefix(&value) {
        return Ok(value);
    }

    let parsed = values
        .parse_js_timestamp_ms(&value)
        .ok_or(RainPerformanceAdjustmentRowsError::InvalidTimestamp)?;
    let availability = has_availability_suffix(key);
    let fractional_remainder = timestamp_fraction(&value)
        .is_some_and(|fraction| fraction.get(3..).is_some_and(has_nonzero_digit));
    let adjusted = if availability && fractional_remainder {
        parsed
            .checked_add(1)
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidTimestamp)?
    } else {
        parsed
    };

    values
        .format_utc_milliseconds(adjusted)
        .ok_or(RainPerformanceAdjustmentRowsError::TimestampFormat)
}

/// Carry the enclosing field name only through object members, not array entries.
fn normalize_verified_performance_timestamps_for_key(
    value: JsonValue,
    key: &[u16],
    values: &mut impl RainPerformanceTimestampValues,
) -> Result<JsonValue, RainPerformanceAdjustmentRowsError> {
    match value {
        JsonValue::Array(entries) => {
            let mut normalized = Vec::with_capacity(entries.len());
            // reset the inherited key for every array entry
            for entry in entries {
                normalized.push(normalize_verified_performance_timestamps_for_key(
                    entry,
                    &[],
                    values,
                )?);
            }
            Ok(JsonValue::Array(normalized))
        }
        JsonValue::Object(entries) => {
            let mut normalized = Vec::with_capacity(entries.len());
            // preserve declared object order and exact UTF-16 keys
            for (name, entry) in entries {
                let entry =
                    normalize_verified_performance_timestamps_for_key(entry, &name, values)?;
                normalized.push((name, entry));
            }
            Ok(JsonValue::Object(normalized))
        }
        JsonValue::String(value) => {
            normalize_verified_performance_timestamp_field(key, value, values)
                .map(JsonValue::String)
        }
        scalar => Ok(scalar),
    }
}

/// Project one typed claim while retaining nullable capture fields.
fn project_claim(
    row: &[(Vec<u16>, JsonValue)],
) -> Result<AdmittedRainArchiveClaim, RainPerformanceAdjustmentRowsError> {
    let payload = required_object_field(row, "payload")
        .ok_or(RainPerformanceAdjustmentRowsError::InvalidClaim)?;
    let kind = match required_text_field(payload, "kind")
        .ok_or(RainPerformanceAdjustmentRowsError::InvalidClaim)?
        .as_str()
    {
        "forecast" => RainAdjustmentCaptureKindEnum::Forecast,
        "station" => RainAdjustmentCaptureKindEnum::Station,
        _ => return Err(RainPerformanceAdjustmentRowsError::InvalidClaim),
    };

    Ok(AdmittedRainArchiveClaim {
        id: required_text_field(payload, "id")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidClaim)?,
        kind,
        run_initialized_at: nullable_text_field(payload, "runInitializedAt")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidClaim)?,
        station_id: nullable_safe_unsigned_field(payload, "stationId")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidClaim)?,
        window_end_exclusive: nullable_text_field(payload, "windowEndExclusive")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidClaim)?,
        window_start: nullable_text_field(payload, "windowStart")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidClaim)?,
    })
}

/// Project one valid receipt and skip every non-valid provider outcome.
fn project_valid_receipt(
    row: &[(Vec<u16>, JsonValue)],
) -> Result<Option<AdmittedRainArchiveReceipt>, RainPerformanceAdjustmentRowsError> {
    let payload = required_object_field(row, "payload")
        .ok_or(RainPerformanceAdjustmentRowsError::InvalidReceipt)?;
    let Some(outcome) = optional_text_field(payload, "outcome") else {
        return Ok(None);
    };
    // do not inspect body or claim fields for invalid outcomes
    if outcome != "valid" {
        return Ok(None);
    }

    Ok(Some(AdmittedRainArchiveReceipt {
        body_member_path: required_text_field(row, "body_member_path")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidReceipt)?,
        body_sha256: required_text_field(payload, "bodySha256")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidReceipt)?,
        claim_id: required_text_field(payload, "claimId")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidReceipt)?,
        completed_at: required_text_field(payload, "completedAt")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidReceipt)?,
        outcome: AdmittedRainArchiveReceiptOutcome::Valid,
    }))
}

/// Project one stored run without changing its exact ordered hours.
fn project_run(
    row: &[(Vec<u16>, JsonValue)],
) -> Result<AdmittedRainArchiveRun, RainPerformanceAdjustmentRowsError> {
    let payload = required_object_field(row, "payload")
        .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?;
    let hours = required_array_field(payload, "hours")
        .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?;
    let mut projected_hours = Vec::with_capacity(hours.len());
    // preserve the stored hour order used by replay parity
    for hour in hours {
        projected_hours.push(project_hour(hour)?);
    }

    Ok(AdmittedRainArchiveRun {
        record_revision_identity: required_text_field(row, "record_revision_identity")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
        stored: RainAdjustmentRun {
            runInitializedAt: required_text_field(payload, "runInitializedAt")
                .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
            modelSha256: required_text_field(payload, "modelSha256")
                .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
            inputSha256: required_text_field(payload, "inputSha256")
                .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
            forecastClaimId: required_text_field(payload, "forecastClaimId")
                .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
            firstReceivedAt: required_text_field(payload, "firstReceivedAt")
                .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
            decisionAt: required_text_field(payload, "decisionAt")
                .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
            generatedAt: required_text_field(payload, "generatedAt")
                .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
            hours: projected_hours,
        },
    })
}

/// Project one generated rain adjustment hour.
pub(crate) fn project_hour(
    value: &JsonValue,
) -> Result<RainAdjustmentHour, RainPerformanceAdjustmentRowsError> {
    let entries = object_entries(value).ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?;
    Ok(RainAdjustmentHour {
        validAt: required_text_field(entries, "validAt")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
        modelLeadHours: safe_unsigned_field(entries, "modelLeadHours")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
        rawPrecipitationMm: finite_number_field(entries, "rawPrecipitationMm")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
        correctedPrecipitationMm: finite_number_field(entries, "correctedPrecipitationMm")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
        applied: boolean_field(entries, "applied")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
        reasonCode: nullable_string_field(entries, "reasonCode")
            .ok_or(RainPerformanceAdjustmentRowsError::InvalidRun)?,
    })
}

/// Borrow ordered entries from one JSON object.
fn object_entries(value: &JsonValue) -> Option<&[(Vec<u16>, JsonValue)]> {
    match value {
        JsonValue::Object(entries) => Some(entries),
        _ => None,
    }
}

/// Find the last object field to retain JavaScript object replacement semantics.
fn field<'a>(entries: &'a [(Vec<u16>, JsonValue)], key: &str) -> Option<&'a JsonValue> {
    entries
        .iter()
        .rev()
        .find(|(name, _)| utf16_equals_ascii(name, key))
        .map(|(_, value)| value)
}

/// Borrow one required nested object.
fn required_object_field<'a>(
    entries: &'a [(Vec<u16>, JsonValue)],
    key: &str,
) -> Option<&'a [(Vec<u16>, JsonValue)]> {
    field(entries, key).and_then(object_entries)
}

/// Borrow one required array.
fn required_array_field<'a>(
    entries: &'a [(Vec<u16>, JsonValue)],
    key: &str,
) -> Option<&'a [JsonValue]> {
    match field(entries, key)? {
        JsonValue::Array(values) => Some(values),
        _ => None,
    }
}

/// Decode one nonempty UTF-16 string.
fn required_text_field(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<String> {
    let JsonValue::String(value) = field(entries, key)? else {
        return None;
    };
    let decoded = String::from_utf16(value).ok()?;
    (!decoded.is_empty()).then_some(decoded)
}

/// Decode a field as text only when it is present and nonempty.
fn optional_text_field(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<String> {
    required_text_field(entries, key)
}

/// Decode one nullable nonempty UTF-16 string.
fn nullable_text_field(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<Nullable<String>> {
    match field(entries, key)? {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::String(value) => {
            let decoded = String::from_utf16(value).ok()?;
            (!decoded.is_empty()).then_some(Nullable::Value(decoded))
        }
        _ => None,
    }
}

/// Decode one nullable unrestricted UTF-16 string.
fn nullable_string_field(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<Nullable<String>> {
    match field(entries, key)? {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::String(value) => String::from_utf16(value).ok().map(Nullable::Value),
        _ => None,
    }
}

/// Decode one nullable generated safe unsigned integer.
fn nullable_safe_unsigned_field(
    entries: &[(Vec<u16>, JsonValue)],
    key: &str,
) -> Option<Nullable<u64>> {
    match field(entries, key)? {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::Number(value) => safe_unsigned(*value).map(Nullable::Value),
        _ => None,
    }
}

/// Decode one generated safe unsigned integer field.
fn safe_unsigned_field(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<u64> {
    let JsonValue::Number(value) = field(entries, key)? else {
        return None;
    };
    safe_unsigned(*value)
}

/// Admit one JavaScript safe unsigned integer.
fn safe_unsigned(value: f64) -> Option<u64> {
    (value.is_finite() && (0.0..=MAXIMUM_SAFE_INTEGER).contains(&value) && value.fract() == 0.0)
        .then_some(value as u64)
}

/// Decode one finite binary64 field.
fn finite_number_field(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<f64> {
    let JsonValue::Number(value) = field(entries, key)? else {
        return None;
    };
    value.is_finite().then_some(*value)
}

/// Decode one boolean field.
fn boolean_field(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(entries, key)? else {
        return None;
    };
    Some(*value)
}

/// Match one exact ASCII field name against its UTF-16 representation.
fn utf16_equals_ascii(value: &[u16], expected: &str) -> bool {
    value.len() == expected.len()
        && value
            .iter()
            .copied()
            .zip(expected.bytes().map(u16::from))
            .all(|(left, right)| left == right)
}

/// Match one ASCII suffix without decoding unrelated UTF-16 material.
fn utf16_ends_with_ascii(value: &[u16], suffix: &str) -> bool {
    let Some(start) = value.len().checked_sub(suffix.len()) else {
        return false;
    };
    value[start..]
        .iter()
        .copied()
        .zip(suffix.bytes().map(u16::from))
        .all(|(left, right)| left == right)
}

/// Recognize the original timestamp field suffix set.
fn has_timestamp_suffix(key: &[u16]) -> bool {
    ["At", "AtUtc", "_at", "windowStart", "windowEndExclusive"]
        .iter()
        .any(|suffix| utf16_ends_with_ascii(key, suffix))
}

/// Recognize the fields whose sub-millisecond availability rounds upward.
fn has_availability_suffix(key: &[u16]) -> bool {
    ["ReceivedAt", "ReceiptAt", "received_at", "createdAtUtc"]
        .iter()
        .any(|suffix| utf16_ends_with_ascii(key, suffix))
}

/// Recognize exactly the original leading `YYYY-MM-DDT` grammar.
fn has_iso_datetime_prefix(value: &[u16]) -> bool {
    value.len() >= 11
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|index| is_ascii_digit(value[*index]))
        && value[4] == u16::from(b'-')
        && value[7] == u16::from(b'-')
        && value[10] == u16::from(b'T')
}

/// Find the first fraction followed directly by a UTC or numeric-zone marker.
fn timestamp_fraction(value: &[u16]) -> Option<&[u16]> {
    // reproduce the unanchored JavaScript fraction expression
    for start in 0..value.len() {
        if value[start] != u16::from(b'.') {
            continue;
        }
        let mut end = start + 1;
        // consume the required ASCII decimal digits
        while end < value.len() && is_ascii_digit(value[end]) {
            end += 1;
        }
        // accept the first complete fraction and zone marker
        if end > start + 1 && end < value.len() && matches!(value[end], 90 | 43 | 45) {
            return Some(&value[start + 1..end]);
        }
    }
    None
}

/// Recognize one ASCII decimal digit.
fn is_ascii_digit(value: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&value)
}

/// Detect any nonzero ASCII fractional digit.
fn has_nonzero_digit(value: &[u16]) -> bool {
    value
        .iter()
        .any(|digit| (u16::from(b'1')..=u16::from(b'9')).contains(digit))
}

#[cfg(test)]
#[path = "rain_performance_adjustment_rows_fixture_test.rs"]
mod tests;
