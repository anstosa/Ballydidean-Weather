//! validate shared forecast scorecard family fields.

use weather_contracts::JsonValue;

use crate::forecast_scorecard_fields::{
    ScorecardClock, ScorecardContractError, member, require_array, require_count, require_enum,
    require_equal, require_finite, require_keys, require_match, require_nonnegative_finite,
    require_nullable_finite, require_nullable_instant, require_nullable_sha256, require_object,
};
use crate::forecast_scorecard_rain::validate_rain_diagnostics;

const EVIDENCE_CLASSES: &[&str] = &[
    "as_issued",
    "prospective_receipt",
    "retrospective_counterfactual",
    "development",
];
const SUPPORT_STATES: &[&str] = &["sufficient", "insufficient", "invalid"];
const COMPARISON_STATES: &[&str] = &["unscored", "better", "mixed", "worse"];
const QUALIFICATION_STATES: &[&str] = &[
    "development_only",
    "counterfactual_only",
    "pending_support",
    "supported",
    "rejected",
];
const SERVING_STATES: &[&str] = &[
    "authorized_active",
    "admin_disabled",
    "fail_raw",
    "pending_review",
];
const RECOMMENDATIONS: &[&str] = &["retain", "review_candidate", "review_disable", "none"];
const SLICE_DIMENSIONS: &[&str] = &["horizon", "month", "season", "daypart"];
const METRIC_UNITS: &[&str] = &["celsius", "meters_per_second", "millimeters_per_hour"];

/// validate one closed family card.
pub(crate) fn validate_family(
    value: &JsonValue,
    expected_family: &str,
    clock: &impl ScorecardClock,
) -> Result<(), ScorecardContractError> {
    let path = format!("families.{expected_family}");
    require_object(value, &path)?;
    require_keys(
        value,
        &[
            "bestMatchDiagnostic",
            "comparisonState",
            "evidenceClass",
            "evidenceCutoffAt",
            "family",
            "metrics",
            "qualificationState",
            "rainDiagnostics",
            "recommendation",
            "servingIdentitySha256",
            "servingState",
            "slices",
            "support",
            "supportState",
        ],
        &path,
    )?;
    require_equal(
        member(value, "family"),
        &json_string(expected_family),
        &format!("{path}.family"),
    )?;
    require_nullable_sha256(
        member(value, "servingIdentitySha256"),
        &format!("{path}.servingIdentitySha256"),
    )?;
    require_enum(
        member(value, "evidenceClass"),
        EVIDENCE_CLASSES,
        &format!("{path}.evidenceClass"),
    )?;
    require_nullable_instant(
        member(value, "evidenceCutoffAt"),
        &format!("{path}.evidenceCutoffAt"),
        clock,
    )?;
    require_enum(
        member(value, "supportState"),
        SUPPORT_STATES,
        &format!("{path}.supportState"),
    )?;
    require_enum(
        member(value, "comparisonState"),
        COMPARISON_STATES,
        &format!("{path}.comparisonState"),
    )?;
    require_enum(
        member(value, "qualificationState"),
        QUALIFICATION_STATES,
        &format!("{path}.qualificationState"),
    )?;
    require_enum(
        member(value, "servingState"),
        SERVING_STATES,
        &format!("{path}.servingState"),
    )?;
    require_enum(
        member(value, "recommendation"),
        RECOMMENDATIONS,
        &format!("{path}.recommendation"),
    )?;
    validate_support(member(value, "support"), &format!("{path}.support"))?;

    // select the family-specific metric unit
    let expected_unit = if expected_family == "temperature" {
        "celsius"
    } else if expected_family == "wind" {
        "meters_per_second"
    } else {
        "millimeters_per_hour"
    };
    validate_metric(
        member(value, "metrics"),
        &format!("{path}.metrics"),
        expected_unit,
    )?;
    validate_best_match_diagnostic(
        member(value, "bestMatchDiagnostic"),
        &format!("{path}.bestMatchDiagnostic"),
        expected_unit,
        expected_family,
    )?;
    let slices = require_array(
        member(value, "slices"),
        256,
        &format!("{path}.slices"),
        None,
    )?;

    // validate each bounded aggregate slice
    for (index, slice) in slices.iter().enumerate() {
        validate_slice(slice, &format!("{path}.slices[{index}]"), expected_unit)?;
    }

    // admit diagnostics only on the rain card
    if expected_family == "rain" {
        validate_rain_diagnostics(
            member(value, "rainDiagnostics"),
            "families.rain.rainDiagnostics",
        )?;
    } else if !matches!(member(value, "rainDiagnostics"), JsonValue::Null) {
        return Err(ScorecardContractError::new(format!(
            "{path}.rainDiagnostics must be null"
        )));
    }

    Ok(())
}

/// validate one separately matched baseline diagnostic.
fn validate_best_match_diagnostic(
    value: &JsonValue,
    path: &str,
    expected_unit: &str,
    family: &str,
) -> Result<(), ScorecardContractError> {
    // keep the wind primary baseline absent
    if family == "wind" {
        // reject a second wind baseline
        if !matches!(value, JsonValue::Null) {
            return Err(ScorecardContractError::new(format!("{path} must be null")));
        }
        return Ok(());
    }

    // preserve honest absence without matched rows
    if matches!(value, JsonValue::Null) {
        return Ok(());
    }

    require_object(value, path)?;
    require_keys(
        value,
        &[
            "bestMatchRawMae",
            "dateCount",
            "rowCount",
            "sourceAdjustedMae",
            "sourceRawMae",
            "unit",
        ],
        path,
    )?;
    require_equal(
        member(value, "unit"),
        &json_string(expected_unit),
        &format!("{path}.unit"),
    )?;
    require_count(member(value, "dateCount"), &format!("{path}.dateCount"))?;
    require_count(member(value, "rowCount"), &format!("{path}.rowCount"))?;

    // retain only same-paired-subcohort errors
    for key in ["bestMatchRawMae", "sourceRawMae", "sourceAdjustedMae"] {
        let field_path = format!("{path}.{key}");
        let amount = require_nullable_finite(member(value, key), &field_path)?;

        // reject negative matched errors
        if amount.is_some_and(|amount| amount < 0.0) {
            return Err(ScorecardContractError::new(format!(
                "{field_path} must be nonnegative"
            )));
        }
    }

    Ok(())
}

/// validate aggregate support and exclusion counts.
fn validate_support(value: &JsonValue, path: &str) -> Result<(), ScorecardContractError> {
    require_object(value, path)?;
    require_keys(
        value,
        &[
            "dateCount",
            "effectiveWeightSum",
            "eventCount",
            "excludedCount",
            "exclusionReasons",
            "fallbackCount",
            "fallbackReasons",
            "gapCount",
            "rowCount",
            "targetRowCount",
            "validHourCount",
            "vintageCount",
            "wetDateCount",
            "wetRowCount",
        ],
        path,
    )?;

    // admit only bounded nonnegative counts
    for key in [
        "dateCount",
        "eventCount",
        "excludedCount",
        "fallbackCount",
        "gapCount",
        "rowCount",
        "targetRowCount",
        "validHourCount",
        "vintageCount",
        "wetDateCount",
        "wetRowCount",
    ] {
        require_count(member(value, key), &format!("{path}.{key}"))?;
    }
    require_nonnegative_finite(
        member(value, "effectiveWeightSum"),
        &format!("{path}.effectiveWeightSum"),
    )?;
    validate_reason_count_map(
        member(value, "exclusionReasons"),
        &format!("{path}.exclusionReasons"),
    )?;
    validate_reason_count_map(
        member(value, "fallbackReasons"),
        &format!("{path}.fallbackReasons"),
    )?;
    Ok(())
}

/// validate one aggregate metric.
fn validate_metric(
    value: &JsonValue,
    path: &str,
    expected_unit: &str,
) -> Result<(), ScorecardContractError> {
    require_object(value, path)?;
    require_keys(
        value,
        &[
            "adjustedBias",
            "adjustedMae",
            "adjustedP95",
            "adjustedRmse",
            "deltaMae",
            "rawBias",
            "rawMae",
            "rawP95",
            "rawRmse",
            "skillInterval95",
            "skillPercent",
            "unit",
        ],
        path,
    )?;
    require_enum(member(value, "unit"), METRIC_UNITS, &format!("{path}.unit"))?;
    require_equal(
        member(value, "unit"),
        &json_string(expected_unit),
        &format!("{path}.unit"),
    )?;

    // retain each nullable aggregate scalar
    for key in [
        "rawMae",
        "adjustedMae",
        "deltaMae",
        "rawBias",
        "adjustedBias",
        "rawRmse",
        "adjustedRmse",
        "rawP95",
        "adjustedP95",
        "skillPercent",
    ] {
        require_nullable_finite(member(value, key), &format!("{path}.{key}"))?;
    }

    // reject physically impossible error magnitudes
    for key in [
        "rawMae",
        "adjustedMae",
        "rawRmse",
        "adjustedRmse",
        "rawP95",
        "adjustedP95",
    ] {
        let field_path = format!("{path}.{key}");

        // retain only nonnegative errors
        if matches!(member(value, key), JsonValue::Number(amount) if *amount < 0.0) {
            return Err(ScorecardContractError::new(format!(
                "{field_path} must be nonnegative"
            )));
        }
    }

    let interval = member(value, "skillInterval95");
    // validate a complete paired interval when present
    if !matches!(interval, JsonValue::Null) {
        let interval_path = format!("{path}.skillInterval95");
        require_object(interval, &interval_path)?;
        require_keys(interval, &["lower", "upper"], &interval_path)?;
        let lower = require_finite(member(interval, "lower"), &format!("{interval_path}.lower"))?;
        let upper = require_finite(member(interval, "upper"), &format!("{interval_path}.upper"))?;

        // reject reversed confidence bounds
        if lower > upper {
            return Err(ScorecardContractError::new(format!(
                "{interval_path} is reversed"
            )));
        }
    }

    Ok(())
}

/// validate one bounded reason count map.
fn validate_reason_count_map(value: &JsonValue, path: &str) -> Result<(), ScorecardContractError> {
    let entries = require_object(value, path)?;

    // cap diagnostic cardinality
    if entries.len() > 64 {
        return Err(ScorecardContractError::new(format!("{path} is invalid")));
    }

    // reject unsafe codes and unbounded values
    for (code, count) in entries {
        let code_value = JsonValue::String(code.clone());
        require_match(&code_value, &format!("{path} code"), safe_reason_code)?;
        let code = String::from_utf16(code)
            .map_err(|_| ScorecardContractError::new(format!("{path} code is invalid")))?;
        require_count(count, &format!("{path}.{code}"))?;
    }

    Ok(())
}

/// validate one aggregate slice.
fn validate_slice(
    value: &JsonValue,
    path: &str,
    expected_unit: &str,
) -> Result<(), ScorecardContractError> {
    require_object(value, path)?;
    require_keys(value, &["dimension", "label", "metrics", "rowCount"], path)?;
    require_enum(
        member(value, "dimension"),
        SLICE_DIMENSIONS,
        &format!("{path}.dimension"),
    )?;
    require_match(member(value, "label"), &format!("{path}.label"), safe_label)?;
    require_count(member(value, "rowCount"), &format!("{path}.rowCount"))?;
    validate_metric(
        member(value, "metrics"),
        &format!("{path}.metrics"),
        expected_unit,
    )
}

/// recognize one safe slice label.
fn safe_label(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=80).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(*byte, b'_' | b'.' | b':' | b'+' | b'-')
        })
}

/// recognize one safe reason code.
fn safe_reason_code(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=80).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

/// construct one typed json string.
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "forecast_scorecard_family_test.rs"]
mod tests;
