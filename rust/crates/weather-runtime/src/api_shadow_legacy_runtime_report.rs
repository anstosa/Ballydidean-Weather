//! Verification of one retained forecast-adjustment evaluation report.

use std::collections::BTreeSet;

use weather_contracts::{
    ForecastAdjustmentEvaluationReportV2ContractVersionLiteral, ForecastObservationStationKey,
    ForecastSeasonDaypartKey, JsonValue,
};

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_shadow_legacy_runtime::{
    runtime_band, validate_runtime_bands, validate_runtime_provenance, validate_runtime_text,
};
use crate::api_shadow_legacy_values::{
    canonical_clock, canonical_object_hash_matches, hash_field, integer_field, number_field,
    text_equals,
};
use crate::maintenance_canonical_json::{exact_keys, field, text};

const REPORT_KEYS: &[&str] = &[
    "candidateArtifactSha256",
    "contractVersion",
    "enabledMetricBands",
    "evaluationEpochId",
    "evaluationReportSha256",
    "holdoutAccessMarkerSha256",
    "holdoutEndExclusive",
    "holdoutEndLocalDate",
    "holdoutStartInclusive",
    "holdoutStartLocalDate",
    "metricBandEvaluations",
    "preregistrationSha256",
    "trainingProvenance",
];
const EVALUATION_KEYS: &[&str] = &[
    "criticalSlices",
    "evaluatedSeasonDaypartKeys",
    "metricBand",
    "network",
    "providerBalanced",
    "scoreableStationKeys",
];
const PAIRED_SCORE_KEYS: &[&str] = &[
    "adjustedLoss",
    "bootstrapLowerBound",
    "bootstrapUpperBound",
    "eventCount",
    "rawLoss",
    "skill",
];
const CRITICAL_SLICE_KEYS: &[&str] = &[
    "adjustedLoss",
    "bootstrapLowerBound",
    "bootstrapUpperBound",
    "eventCount",
    "key",
    "kind",
    "rawLoss",
    "skill",
];
const MILLISECONDS_PER_DAY: i64 = 86_400_000;
const MAXIMUM_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy)]
struct PairedScore {
    bootstrap_lower_bound: f64,
    bootstrap_upper_bound: f64,
    event_count: i64,
    skill: f64,
}

/// Verify one closed, content-addressed holdout evaluation report.
pub(crate) fn verify_legacy_runtime_report(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    doc: &JsonValue,
) -> Option<()> {
    exact_keys(doc, REPORT_KEYS).then_some(())?;
    text_equals(
        doc,
        "contractVersion",
        ForecastAdjustmentEvaluationReportV2ContractVersionLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    hash_field(doc, "candidateArtifactSha256")?;
    let report_sha256 = hash_field(doc, "evaluationReportSha256")?;
    hash_field(doc, "holdoutAccessMarkerSha256")?;
    hash_field(doc, "preregistrationSha256")?;
    validate_runtime_provenance(field(doc, "trainingProvenance")?)?;
    let enabled_bands = validate_runtime_bands(field(doc, "enabledMetricBands")?)?;
    validate_runtime_text(doc, "evaluationEpochId")?;
    validate_holdout_bounds(values, doc)?;
    validate_evaluations(doc, &enabled_bands)?;
    canonical_object_hash_matches(values, doc, "evaluationReportSha256", &report_sha256)
}

/// Validate increasing UTC bounds and exactly thirty inclusive local labels.
fn validate_holdout_bounds(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    report: &JsonValue,
) -> Option<()> {
    let start = canonical_clock(values, report, "holdoutStartInclusive")?;
    let end = canonical_clock(values, report, "holdoutEndExclusive")?;
    let start_local = local_date_epoch(values, report, "holdoutStartLocalDate")?;
    let end_local = local_date_epoch(values, report, "holdoutEndLocalDate")?;
    // preserve the source's increasing instants and 29-day label difference
    (start < end && end_local.checked_sub(start_local)? == 29 * MILLISECONDS_PER_DAY).then_some(())
}

/// Admit one exact Gregorian local-date label through the canonical UTC owner.
fn local_date_epoch(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    node: &JsonValue,
    key: &str,
) -> Option<i64> {
    let value = text(node, key)?;
    let bytes = value.as_bytes();
    // require the exact source lexical shape before using the calendar owner
    if bytes.len() != 10
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || bytes[4] != b'-'
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || bytes[7] != b'-'
        || !bytes[8..].iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    values.canonical_utc_epoch_milliseconds(&format!("{value}T00:00:00.000Z"))
}

/// Validate ordered evaluation coverage for every enabled metric band.
fn validate_evaluations(report: &JsonValue, enabled_bands: &[(String, String)]) -> Option<()> {
    let JsonValue::Array(evaluations) = field(report, "metricBandEvaluations")? else {
        return None;
    };
    // retain one evaluation at the same position as every enabled pair
    if evaluations.len() != enabled_bands.len() {
        return None;
    }
    for (evaluation, expected_band) in evaluations.iter().zip(enabled_bands) {
        validate_evaluation(evaluation, expected_band)?;
    }
    Some(())
}

/// Validate one promotable metric-band evaluation and all critical slices.
fn validate_evaluation(evaluation: &JsonValue, expected_band: &(String, String)) -> Option<()> {
    exact_keys(evaluation, EVALUATION_KEYS).then_some(())?;
    (runtime_band(field(evaluation, "metricBand")?)? == *expected_band).then_some(())?;
    let network = validate_paired_score(field(evaluation, "network")?, PAIRED_SCORE_KEYS)?;
    let provider =
        validate_paired_score(field(evaluation, "providerBalanced")?, PAIRED_SCORE_KEYS)?;
    // require the literal network thresholds and no provider-balanced material harm
    if network.skill < 0.02 || network.bootstrap_lower_bound <= 0.0 || material_harm(provider) {
        return None;
    }

    let JsonValue::Array(slices) = field(evaluation, "criticalSlices")? else {
        return None;
    };
    for slice in slices {
        validate_critical_slice(slice)?;
    }
    validate_critical_slice_coverage(evaluation, slices)
}

/// Validate one paired aggregate score and its literal skill formula.
fn validate_paired_score(node: &JsonValue, keys: &[&str]) -> Option<PairedScore> {
    exact_keys(node, keys).then_some(())?;
    let raw_loss = number_field(node, "rawLoss")?;
    let adjusted_loss = number_field(node, "adjustedLoss")?;
    let bootstrap_lower_bound = number_field(node, "bootstrapLowerBound")?;
    let bootstrap_upper_bound = number_field(node, "bootstrapUpperBound")?;
    let skill = number_field(node, "skill")?;
    let event_count = integer_field(node, "eventCount")?;
    // require finite nonnegative losses, ordered intervals, and one safe event
    if raw_loss < 0.0
        || adjusted_loss < 0.0
        || bootstrap_lower_bound > bootstrap_upper_bound
        || !(1..=MAXIMUM_SAFE_INTEGER).contains(&event_count)
    {
        return None;
    }
    let expected_skill = if raw_loss == 0.0 {
        if adjusted_loss == 0.0 { 0.0 } else { -1.0 }
    } else {
        (raw_loss - adjusted_loss) / raw_loss
    };
    // bind the retained score to its paired-loss calculation
    if (skill - expected_skill).abs() > f64::EPSILON * 8.0 {
        return None;
    }
    Some(PairedScore {
        bootstrap_lower_bound,
        bootstrap_upper_bound,
        event_count,
        skill,
    })
}

/// Validate one closed critical slice and reject sufficiently powered harm.
fn validate_critical_slice(slice: &JsonValue) -> Option<()> {
    validate_runtime_text(slice, "key")?;
    matches!(
        text(slice, "kind")?.as_str(),
        "nearest_three" | "provider_family" | "season_daypart" | "station"
    )
    .then_some(())?;
    let score = validate_paired_score(slice, CRITICAL_SLICE_KEYS)?;
    // reject material harm whenever the slice has enough events
    if score.event_count >= 100 && material_harm(score) {
        return None;
    }
    Some(())
}

/// Apply the frozen material-harm condition.
fn material_harm(score: PairedScore) -> bool {
    score.skill <= -0.02 && score.bootstrap_upper_bound < 0.0
}

/// Validate station/calendar inputs and exact canonical critical-slice order.
fn validate_critical_slice_coverage(evaluation: &JsonValue, slices: &[JsonValue]) -> Option<()> {
    let stations = validate_scoreable_stations(field(evaluation, "scoreableStationKeys")?)?;
    let season_dayparts =
        validate_season_dayparts(field(evaluation, "evaluatedSeasonDaypartKeys")?)?;
    let providers: BTreeSet<&'static str> =
        stations.iter().map(|(_, provider)| *provider).collect();
    // retain the frozen provider-diversity threshold
    if providers.len() < 3 {
        return None;
    }

    let mut expected =
        Vec::with_capacity(1 + providers.len() + season_dayparts.len() + stations.len());
    expected.push(("nearest-three".to_owned(), "nearest_three"));
    expected.extend(
        providers
            .into_iter()
            .map(|provider| (provider.to_owned(), "provider_family")),
    );
    expected.extend(
        season_dayparts
            .into_iter()
            .map(|key| (key, "season_daypart")),
    );
    expected.extend(stations.into_iter().map(|(key, _)| (key, "station")));
    // require every bound slice identity once in the exact source order
    if slices.len() != expected.len() {
        return None;
    }
    slices
        .iter()
        .zip(expected)
        .all(|(slice, (key, kind))| {
            text(slice, "key").is_some_and(|actual| actual == key)
                && text_equals(slice, "kind", kind)
                && integer_field(slice, "eventCount").is_some_and(|count| count >= 100)
        })
        .then_some(())
}

/// Validate the frozen lexical station set and derive each provider family.
fn validate_scoreable_stations(node: &JsonValue) -> Option<Vec<(String, &'static str)>> {
    let JsonValue::Array(values) = node else {
        return None;
    };
    // preserve the exact minimum and maximum station cardinality
    if !(5..=11).contains(&values.len()) {
        return None;
    }
    let mut stations = Vec::with_capacity(values.len());
    for value in values {
        let JsonValue::String(units) = value else {
            return None;
        };
        let key = String::from_utf16(units).ok()?;
        let station = ForecastObservationStationKey::WIRE_VALUES
            .iter()
            .find_map(|(wire, station)| (*wire == key).then_some(*station))?;
        let provider = station_provider(station);
        // require strict lexical order to exclude duplicate station identities
        if stations
            .last()
            .is_some_and(|(previous, _)| previous >= &key)
        {
            return None;
        }
        stations.push((key, provider));
    }
    Some(stations)
}

/// Map the generated frozen station identity to its source provider family.
const fn station_provider(station: ForecastObservationStationKey) -> &'static str {
    match station {
        ForecastObservationStationKey::AmbientMaxweather
        | ForecastObservationStationKey::AmbientMerlin => "ambient",
        ForecastObservationStationKey::BallydideanEcowitt => "ecowitt",
        ForecastObservationStationKey::NetatmoNearby => "netatmo",
        ForecastObservationStationKey::Tempest126537
        | ForecastObservationStationKey::Tempest168853
        | ForecastObservationStationKey::Tempest201058
        | ForecastObservationStationKey::Tempest203055
        | ForecastObservationStationKey::Tempest225947
        | ForecastObservationStationKey::Tempest38270
        | ForecastObservationStationKey::Tempest64255 => "tempest",
    }
}

/// Validate the generated season-daypart allowlist in strict lexical order.
fn validate_season_dayparts(node: &JsonValue) -> Option<Vec<String>> {
    let JsonValue::Array(values) = node else {
        return None;
    };
    // retain at least one and no more than the complete frozen calendar set
    if values.is_empty() || values.len() > ForecastSeasonDaypartKey::WIRE_VALUES.len() {
        return None;
    }
    let mut keys = Vec::with_capacity(values.len());
    for value in values {
        let JsonValue::String(units) = value else {
            return None;
        };
        let key = String::from_utf16(units).ok()?;
        ForecastSeasonDaypartKey::WIRE_VALUES
            .iter()
            .any(|(wire, _)| *wire == key)
            .then_some(())?;
        // require strict lexical order to exclude duplicates and reordering
        if keys.last().is_some_and(|previous| previous >= &key) {
            return None;
        }
        keys.push(key);
    }
    Some(keys)
}
