//! Native verification of one retained general forecast-adjustment candidate.

use std::collections::BTreeSet;

use weather_contracts::{
    ForecastAdjustmentCandidateV2AlgorithmContractVersionLiteral,
    ForecastAdjustmentCandidateV2ContractVersionLiteral,
    ForecastAdjustmentCandidateV2SiteKeyLiteral, ForecastAdjustmentCandidateV2TimezoneLiteral,
    JsonValue,
};

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_shadow_legacy_runtime::{
    validate_runtime_bands, validate_runtime_provenance, validate_runtime_text,
};
use crate::api_shadow_legacy_values::{
    canonical_clock, canonical_object_hash_matches, canonical_value_hash_matches, hash_field,
    integer_field, number_field, text_equals,
};
use crate::api_shadow_wire_validation::rows;
use crate::maintenance_canonical_json::{exact_keys, field, text};

const CANDIDATE_KEYS: &[&str] = &[
    "algorithmContractVersion",
    "candidateArtifactSha256",
    "coefficientPayloadSha256",
    "coefficients",
    "contractVersion",
    "developmentReportSha256",
    "enabledMetricBands",
    "evaluationEpochId",
    "exportManifestSha256",
    "finalTrainingCutoff",
    "forecastIdentity",
    "metricPolicies",
    "runtimeFingerprint",
    "siteKey",
    "timezone",
    "trainingEnvelopes",
    "trainingProvenance",
];
const FORECAST_IDENTITY_KEYS: &[&str] = &[
    "adapterVersion",
    "cohort",
    "contractEpoch",
    "dataset",
    "referenceKind",
    "sourceConfigFingerprint",
    "sourceKey",
    "upstreamModel",
];
const RUNTIME_FINGERPRINT_KEYS: &[&str] = &["icuVersion", "tzdataVersion"];
const COEFFICIENT_KEYS: &[&str] = &[
    "coefficient",
    "daypart",
    "effectiveEventCount",
    "leadBand",
    "level",
    "metric",
    "month",
    "season",
];
const TRAINING_ENVELOPE_KEYS: &[&str] = &["leadBand", "maximum", "metric", "minimum"];
pub(crate) const CANONICAL_FORECAST_IDENTITY_SHA256: &str =
    "8348b5c48d7fe614ced9442b04e8a68cfe704e3d541eab3b8d0ab33a41cd4079";
pub(crate) const CANONICAL_TRAINING_PROVENANCE_SHA256: &str =
    "cfa905468fb15fb10b3abf02a39db5823d8d3bb17c6dddc625749eb9fe3bb8c3";
pub(crate) const METRIC_POLICIES_SHA256: &str =
    "aee18effc96464fb669e0fe5f2cec79a3fd114fd37fd76afe6b0bc2ae5820280";

#[derive(Eq, Ord, PartialEq, PartialOrd)]
struct CoefficientCell {
    metric: String,
    lead_band: String,
    level: i64,
    daypart: Option<String>,
    month: Option<i64>,
    season: Option<String>,
}

/// Verify the combined immutable and domain contract for one general runtime candidate.
pub(crate) fn verify_legacy_runtime_candidate(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    doc: &JsonValue,
) -> Option<()> {
    exact_keys(doc, CANDIDATE_KEYS).then_some(())?;
    // bind the immutable v2 algorithm and Ballydidean serving identity
    if !text_equals(
        doc,
        "algorithmContractVersion",
        ForecastAdjustmentCandidateV2AlgorithmContractVersionLiteral::WIRE_VALUE,
    ) || !text_equals(
        doc,
        "contractVersion",
        ForecastAdjustmentCandidateV2ContractVersionLiteral::WIRE_VALUE,
    ) || !text_equals(
        doc,
        "siteKey",
        ForecastAdjustmentCandidateV2SiteKeyLiteral::WIRE_VALUE,
    ) || !text_equals(
        doc,
        "timezone",
        ForecastAdjustmentCandidateV2TimezoneLiteral::WIRE_VALUE,
    ) {
        return None;
    }

    let candidate_sha256 = hash_field(doc, "candidateArtifactSha256")?;
    let coefficient_sha256 = hash_field(doc, "coefficientPayloadSha256")?;
    hash_field(doc, "developmentReportSha256")?;
    hash_field(doc, "exportManifestSha256")?;
    canonical_object_hash_matches(values, doc, "candidateArtifactSha256", &candidate_sha256)?;
    let coefficients = field(doc, "coefficients")?;
    canonical_value_hash_matches(values, coefficients, &coefficient_sha256)?;
    canonical_clock(values, doc, "finalTrainingCutoff")?;
    validate_runtime_text(doc, "evaluationEpochId")?;

    let forecast_identity = field(doc, "forecastIdentity")?;
    exact_keys(forecast_identity, FORECAST_IDENTITY_KEYS).then_some(())?;
    hash_field(forecast_identity, "sourceConfigFingerprint")?;
    // preserve every bounded lineage label before binding the frozen identity hash
    for key in [
        "adapterVersion",
        "contractEpoch",
        "dataset",
        "sourceKey",
        "upstreamModel",
    ] {
        validate_runtime_text(forecast_identity, key)?;
    }
    canonical_value_hash_matches(
        values,
        forecast_identity,
        CANONICAL_FORECAST_IDENTITY_SHA256,
    )?;

    let training_provenance = field(doc, "trainingProvenance")?;
    validate_runtime_provenance(training_provenance)?;
    canonical_value_hash_matches(
        values,
        training_provenance,
        CANONICAL_TRAINING_PROVENANCE_SHA256,
    )?;

    let runtime_fingerprint = field(doc, "runtimeFingerprint")?;
    exact_keys(runtime_fingerprint, RUNTIME_FINGERPRINT_KEYS).then_some(())?;
    validate_runtime_text(runtime_fingerprint, "icuVersion")?;
    validate_runtime_text(runtime_fingerprint, "tzdataVersion")?;
    canonical_value_hash_matches(
        values,
        field(doc, "metricPolicies")?,
        METRIC_POLICIES_SHA256,
    )?;

    let enabled = validate_runtime_bands(field(doc, "enabledMetricBands")?)?;
    validate_coefficients(doc, &enabled)?;
    validate_envelopes(doc, &enabled)?;
    // retain the original recursive post-fit evidence exclusion
    if contains_forbidden_candidate_field(doc) {
        return None;
    }
    Some(())
}

/// Validate coefficient coverage, hierarchy cells, evidence floors, and correction caps.
fn validate_coefficients(doc: &JsonValue, enabled: &[(String, String)]) -> Option<()> {
    let coefficients = rows(doc, "coefficients")?;
    let mut root_counts = vec![0_usize; enabled.len()];
    let mut cells = BTreeSet::new();

    // admit every fitted cell independently of array order
    for coefficient in coefficients {
        exact_keys(coefficient, COEFFICIENT_KEYS).then_some(())?;
        let metric = text(coefficient, "metric")?;
        let lead_band = text(coefficient, "leadBand")?;
        let index = enabled
            .binary_search_by(|(name, lead)| {
                (name.as_str(), lead.as_str()).cmp(&(metric.as_str(), lead_band.as_str()))
            })
            .ok()?;
        let coefficient_value = number_field(coefficient, "coefficient")?;
        let effective_event_count = number_field(coefficient, "effectiveEventCount")?;
        let (minimum, maximum) = correction_range(&metric)?;
        // require nonnegative evidence and the exact v1 correction cap
        if effective_event_count < 0.0 || coefficient_value < minimum || coefficient_value > maximum
        {
            return None;
        }

        let level = integer_field(coefficient, "level")?;
        let daypart = nullable_text(coefficient, "daypart")?;
        let month = nullable_integer(coefficient, "month")?;
        let season = nullable_text(coefficient, "season")?;
        validate_coefficient_shape(
            level,
            daypart.as_deref(),
            month,
            season.as_deref(),
            effective_event_count,
        )?;
        // count the required root after its complete shape is admitted
        if level == 1 {
            root_counts[index] = root_counts[index].checked_add(1)?;
        }
        // reject duplicate canonical hierarchy coordinates without sorting the input
        if !cells.insert(CoefficientCell {
            metric,
            lead_band,
            level,
            daypart,
            month,
            season,
        }) {
            return None;
        }
    }
    root_counts.iter().all(|count| *count == 1).then_some(())
}

/// Validate exact scalar envelope coverage without imposing an array order.
fn validate_envelopes(doc: &JsonValue, enabled: &[(String, String)]) -> Option<()> {
    let required: BTreeSet<(String, String)> = enabled
        .iter()
        .filter(|(metric, _)| metric != "windDirectionDegrees")
        .cloned()
        .collect();
    let mut actual = BTreeSet::new();
    // validate each unique enabled scalar envelope and its physical bounds
    for envelope in rows(doc, "trainingEnvelopes")? {
        exact_keys(envelope, TRAINING_ENVELOPE_KEYS).then_some(())?;
        let metric = text(envelope, "metric")?;
        let lead_band = text(envelope, "leadBand")?;
        let pair = (metric.clone(), lead_band);
        // exclude direction, disabled pairs, and duplicate coverage
        if !required.contains(&pair) || !actual.insert(pair) {
            return None;
        }
        let minimum = number_field(envelope, "minimum")?;
        let maximum = number_field(envelope, "maximum")?;
        let (domain_minimum, domain_maximum, maximum_exclusive) = metric_domain(&metric)?;
        // retain ordered finite endpoints within the canonical metric domain
        if minimum > maximum
            || minimum < domain_minimum
            || maximum > domain_maximum
            || (maximum_exclusive && maximum == domain_maximum)
        {
            return None;
        }
    }
    (actual == required).then_some(())
}

/// Return the exact v1 correction bounds for one adjustable metric.
fn correction_range(metric: &str) -> Option<(f64, f64)> {
    // map only the five original general-model metrics
    match metric {
        "relativeHumidityPercent" => Some((-20.0, 20.0)),
        "temperatureC" => Some((-5.0, 5.0)),
        "windDirectionDegrees" => Some((-45.0, 45.0)),
        "windGustMps" => Some((-12.0, 12.0)),
        "windSpeedMps" => Some((-8.0, 8.0)),
        _ => None,
    }
}

/// Return the canonical physical range and exclusive-maximum rule for one scalar metric.
fn metric_domain(metric: &str) -> Option<(f64, f64, bool)> {
    // direction remains described for parity but cannot own an envelope
    match metric {
        "relativeHumidityPercent" => Some((0.0, 100.0, false)),
        "temperatureC" => Some((-100.0, 70.0, false)),
        "windDirectionDegrees" => Some((0.0, 360.0, true)),
        "windGustMps" | "windSpeedMps" => Some((0.0, 150.0, false)),
        _ => None,
    }
}

/// Read one nullable string without replacing malformed UTF-16.
fn nullable_text(node: &JsonValue, key: &str) -> Option<Option<String>> {
    // distinguish a retained null from one admitted string
    match field(node, key)? {
        JsonValue::Null => Some(None),
        JsonValue::String(units) => Some(Some(String::from_utf16(units).ok()?)),
        _ => None,
    }
}

/// Read one nullable exactly represented integer.
fn nullable_integer(node: &JsonValue, key: &str) -> Option<Option<i64>> {
    // distinguish a retained null from one admitted integer
    match field(node, key)? {
        JsonValue::Null => Some(None),
        JsonValue::Number(_) => Some(Some(integer_field(node, key)?)),
        _ => None,
    }
}

/// Validate one root, seasonal, or monthly fitted hierarchy cell.
fn validate_coefficient_shape(
    level: i64,
    daypart: Option<&str>,
    month: Option<i64>,
    season: Option<&str>,
    effective_event_count: f64,
) -> Option<()> {
    // admit the root-only shape and evidence floor
    if level == 1 {
        return (daypart.is_none()
            && month.is_none()
            && season.is_none()
            && effective_event_count >= 200.0)
            .then_some(());
    }
    let daypart_is_valid = matches!(daypart, Some("afternoon" | "evening" | "morning" | "night"));
    // admit the season-daypart shape and evidence floor
    if level == 2 {
        return (daypart_is_valid
            && month.is_none()
            && matches!(season, Some("autumn" | "spring" | "summer" | "winter"))
            && effective_event_count >= 100.0)
            .then_some(());
    }
    // admit the month-daypart shape and evidence floor
    if level == 3 {
        return (daypart_is_valid
            && season.is_none()
            && month.is_some_and(|value| (1..=12).contains(&value))
            && effective_event_count >= 50.0)
            .then_some(());
    }
    None
}

/// Detect the original recursively forbidden post-fit evidence field names.
fn contains_forbidden_candidate_field(doc: &JsonValue) -> bool {
    let mut stack = vec![doc];
    // walk arrays and objects without reinterpreting scalar values
    while let Some(value) = stack.pop() {
        match value {
            JsonValue::Array(children) => stack.extend(children),
            JsonValue::Object(fields) => {
                // reject only the original exact case-insensitive field names
                for (key, child) in fields {
                    let forbidden = String::from_utf16(key).ok().is_none_or(|name| {
                        matches!(
                            name.to_lowercase().as_str(),
                            "actual"
                                | "adjustedloss"
                                | "evaluationreport"
                                | "holdout"
                                | "qualification"
                                | "rawloss"
                                | "receipt"
                        )
                    });
                    if forbidden {
                        return true;
                    }
                    stack.push(child);
                }
            }
            _ => {}
        }
    }
    false
}
