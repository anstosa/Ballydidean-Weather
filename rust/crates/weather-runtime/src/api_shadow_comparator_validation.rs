//! Validate one canonical incumbent comparator before separate authority admission.

use weather_contracts::{JsonValue, MaintenanceShadowPredictionMetadata};

use crate::api_shadow_wire_validation::{ShadowFamily, binary64_in, ordered_keys, rows, unsigned};
use crate::maintenance_canonical_json::{field, text};

const COMPARATOR_VERSION: &str = "adjustment-shadow-incumbent-comparator/v1";
const TOP_KEYS: &[&str] = &[
    "contractVersion",
    "family",
    "registrationSha256",
    "candidateSha256",
    "sourceSha256",
    "dueKey",
    "issuedAt",
    "sourceProjectionSha256",
    "predictionBodySha256",
    "servingAuthority",
    "rowCount",
    "rows",
];
const TEMPERATURE_ROW_KEYS: &[&str] = &[
    "validAt",
    "leadHours",
    "sourceRowSha256",
    "incumbentTemperatureC64",
    "applied",
    "reasonCode",
];
const WIND_ROW_KEYS: &[&str] = &[
    "validAt",
    "leadHours",
    "sourceRowSha256",
    "incumbentSpeedMps64",
    "incumbentGustMps64",
    "speedApplied",
    "gustApplied",
    "reasonCode",
];
const RAIN_ROW_KEYS: &[&str] = &[
    "validAt",
    "leadHours",
    "sourceRowSha256",
    "incumbentPrecipitationMm64",
    "occurrenceProbability64",
    "atLeast1_0Probability64",
    "atLeast2_5Probability64",
    "positiveAmountMm64",
    "applied",
    "reasonCode",
];
const TEMPERATURE_REASONS: &[&str] = &[
    "bundle_invalid",
    "bundle_missing",
    "canary_expired",
    "canary_killed",
    "registry_inactive",
    "registry_invalid",
    "policy_raw",
    "unsupported",
    "missing_source_forecast",
    "outside_operational_window",
    "source_identity_mismatch",
    "source_not_available",
    "source_stale",
    "source_time_mismatch",
];
const WIND_REASONS: &[&str] = &[
    "adjustment_error",
    "bundle_invalid",
    "bundle_missing",
    "canary_expired",
    "canary_killed",
    "coefficient_missing",
    "direction_calm",
    "identity_mismatch",
    "metric_not_enabled",
    "metric_out_of_bounds",
    "policy_raw",
    "registry_inactive",
    "registry_invalid",
    "runtime_fingerprint_mismatch",
    "unsupported_lead",
    "wrong_cohort",
];
const RAIN_REASONS: &[&str] = &["phase_unsupported", "policy_raw", "prediction_invalid"];

/// Validate one comparator's closed rows and bind it to an accepted prediction body.
pub(crate) fn validate_shadow_comparator_document(
    doc: &JsonValue,
    body: &JsonValue,
    metadata: &MaintenanceShadowPredictionMetadata,
) -> Option<ShadowFamily> {
    ordered_keys(doc, TOP_KEYS)?;
    // retain only the frozen comparator contract before inspecting its authority separately
    if text(doc, "contractVersion")? != COMPARATOR_VERSION {
        return None;
    }
    let family = ShadowFamily::read(doc)?;
    if ShadowFamily::read(body)? != family {
        return None;
    }
    // bind the complete common header to the already accepted prediction body
    for key in [
        "family",
        "registrationSha256",
        "candidateSha256",
        "sourceSha256",
        "dueKey",
        "issuedAt",
    ] {
        if field(doc, key)? != field(body, key)? {
            return None;
        }
    }
    // retain exact hashes of the raw source projection and compact prediction body
    if !text_equals(doc, "sourceProjectionSha256", &metadata.inputSha256)?
        || !text_equals(doc, "predictionBodySha256", &metadata.predictionBodySha256)?
    {
        return None;
    }

    let comparator_rows = rows(doc, "rows")?;
    let body_rows = rows(body, "rows")?;
    let row_count = unsigned(doc, "rowCount")?;
    // require the same declared and materialized geometry as the accepted body
    if row_count != unsigned(body, "rowCount")?
        || row_count != u64::try_from(comparator_rows.len()).ok()?
        || comparator_rows.len() != body_rows.len()
    {
        return None;
    }
    // bind every comparator result to one exact accepted prediction row
    for (comparator_row, body_row) in comparator_rows.iter().zip(body_rows) {
        validate_comparator_row(family, comparator_row)?;
        for key in ["validAt", "leadHours", "sourceRowSha256"] {
            if field(comparator_row, key)? != field(body_row, key)? {
                return None;
            }
        }
    }
    Some(family)
}

/// Validate one family-specific comparator row without interpreting serving authority.
fn validate_comparator_row(family: ShadowFamily, row: &JsonValue) -> Option<()> {
    match family {
        ShadowFamily::Temperature => {
            ordered_keys(row, TEMPERATURE_ROW_KEYS)?;
            binary64_in(row, "incumbentTemperatureC64", -f64::MAX, f64::MAX, false)?;
            boolean(row, "applied")?;
            reason(row, "reasonCode", TEMPERATURE_REASONS)
        }
        ShadowFamily::Wind => {
            ordered_keys(row, WIND_ROW_KEYS)?;
            binary64_in(row, "incumbentSpeedMps64", -f64::MAX, f64::MAX, false)?;
            binary64_in(row, "incumbentGustMps64", -f64::MAX, f64::MAX, true)?;
            boolean(row, "speedApplied")?;
            boolean(row, "gustApplied")?;
            reason(row, "reasonCode", WIND_REASONS)
        }
        ShadowFamily::Rain => validate_rain_row(row),
    }
}

/// Validate one rain comparator row, including the applied-value completeness rule.
fn validate_rain_row(row: &JsonValue) -> Option<()> {
    ordered_keys(row, RAIN_ROW_KEYS)?;
    binary64_in(
        row,
        "incumbentPrecipitationMm64",
        -f64::MAX,
        f64::MAX,
        false,
    )?;
    // preserve explicit unavailable probability and amount outputs
    for key in [
        "occurrenceProbability64",
        "atLeast1_0Probability64",
        "atLeast2_5Probability64",
    ] {
        binary64_in(row, key, 0.0, 1.0, true)?;
    }
    binary64_in(row, "positiveAmountMm64", -f64::MAX, f64::MAX, true)?;
    let applied = boolean(row, "applied")?;
    reason(row, "reasonCode", RAIN_REASONS)?;
    // an applied result must retain every modeled probability and positive amount
    if applied
        && [
            "occurrenceProbability64",
            "atLeast1_0Probability64",
            "atLeast2_5Probability64",
            "positiveAmountMm64",
        ]
        .iter()
        .any(|key| matches!(field(row, key), Some(JsonValue::Null)))
    {
        return None;
    }
    Some(())
}

/// Read one required JSON boolean.
fn boolean(doc: &JsonValue, key: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(doc, key)? else {
        return None;
    };
    Some(*value)
}

/// Admit one nullable or closed family-specific comparator reason.
fn reason(doc: &JsonValue, key: &str, admitted: &[&str]) -> Option<()> {
    match field(doc, key)? {
        JsonValue::Null => Some(()),
        JsonValue::String(value)
            if admitted
                .iter()
                .any(|reason| value.iter().copied().eq(reason.encode_utf16())) =>
        {
            Some(())
        }
        _ => None,
    }
}

/// Compare one UTF-16 JSON string with an admitted UTF-8 identity without allocating.
fn text_equals(doc: &JsonValue, key: &str, expected: &str) -> Option<bool> {
    let JsonValue::String(value) = field(doc, key)? else {
        return None;
    };
    Some(value.iter().copied().eq(expected.encode_utf16()))
}
