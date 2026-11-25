use std::collections::BTreeSet;

use weather_contracts::{JsonValue, Nullable, RainWindForecastHour, RainWindRunProfile};

use crate::{
    RainAdjustmentCapture, RainAdjustmentCaptureKind, RainAdjustmentEvaluation,
    RainAdjustmentPendingInput,
};

const MAINTENANCE_SHADOW_SOURCE_VERSION: &str = "adjustment-shadow-source-projection/v1";
const RAIN_CAUSAL_INPUT_VERSION: &str = "adjustment-shadow-rain-causal-inputs/v1";
/// Exact source configuration identity used by rain shadow and feature projections.
pub const RAIN_SOURCE_CONFIG_FINGERPRINT: &str =
    "96e4365f74a0fb8694944172b73a65c29d89a68ddf86b1b0a50db38bb39d52aa";
const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Immutable registration header for one rain maintenance source projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RainShadowSourceHeader<'a> {
    pub registration_sha256: &'a str,
    pub candidate_sha256: &'a str,
    pub source_sha256: &'a str,
    pub due_key: &'a str,
    pub issued_at: &'a str,
}

/// Project the exact captured rain input graph and incumbent output source rows.
pub fn project_rain_shadow_source(
    pending: &RainAdjustmentPendingInput,
    evaluation: &RainAdjustmentEvaluation,
    header: RainShadowSourceHeader<'_>,
) -> Option<JsonValue> {
    // bind the retained pending graph to the identities used by inference
    if pending.captures.len() != evaluation.captures.len()
        || !pending
            .captures
            .iter()
            .zip(&evaluation.captures)
            .all(|(capture, identity)| capture.identity == *identity)
    {
        return None;
    }
    let current_capture = forecast_capture(
        pending,
        &evaluation.causal_input.currentRun,
        Some(&evaluation.current_claim_id),
    )?;
    let mut used_claims = BTreeSet::from([current_capture.identity.claim_id.as_str()]);
    let mut prior_runs = Vec::with_capacity(evaluation.causal_input.priorRuns.len());
    // bind every retained prior profile to one different immutable raw capture
    for profile in &evaluation.causal_input.priorRuns {
        let capture = forecast_capture(pending, profile, None)?;
        // reject one capture reused for multiple causal run positions
        if !used_claims.insert(capture.identity.claim_id.as_str()) {
            return None;
        }
        prior_runs.push(project_run(profile, capture)?);
    }

    let capture_set = pending
        .captures
        .iter()
        .map(project_capture)
        .collect::<Option<Vec<_>>>()?;
    let station_hours = evaluation
        .causal_input
        .stationHours
        .iter()
        .map(project_station_hour)
        .collect::<Option<Vec<_>>>()?;
    let rows = evaluation
        .incumbent_performance
        .hours
        .iter()
        .enumerate()
        .map(|(index, hour)| project_source_row(index, hour, current_capture, header.source_sha256))
        .collect::<Option<Vec<_>>>()?;
    // require the preserved scorer output and feature projection to share geometry
    if rows.len() != evaluation.feature_projection.rows.len() {
        return None;
    }
    let causal_inputs = object(vec![
        ("contractVersion", string(RAIN_CAUSAL_INPUT_VERSION)),
        ("captureSet", JsonValue::Array(capture_set)),
        (
            "currentRun",
            project_run(&evaluation.causal_input.currentRun, current_capture)?,
        ),
        ("priorRuns", JsonValue::Array(prior_runs)),
        ("stationHours", JsonValue::Array(station_hours)),
    ]);
    Some(object(vec![
        ("contractVersion", string(MAINTENANCE_SHADOW_SOURCE_VERSION)),
        ("family", string("rain")),
        ("registrationSha256", string(header.registration_sha256)),
        ("candidateSha256", string(header.candidate_sha256)),
        ("sourceSha256", string(header.source_sha256)),
        ("dueKey", string(header.due_key)),
        ("issuedAt", string(header.issued_at)),
        ("causalInputs", causal_inputs),
        ("rowCount", JsonValue::Number(rows.len() as f64)),
        ("rows", JsonValue::Array(rows)),
    ]))
}

/// Locate one unique captured forecast whose decoded profile is exactly retained.
fn forecast_capture<'a>(
    pending: &'a RainAdjustmentPendingInput,
    profile: &RainWindRunProfile,
    expected_claim_id: Option<&str>,
) -> Option<&'a RainAdjustmentCapture> {
    let mut matches = pending.captures.iter().filter(|capture| {
        // accept only a forecast with the requested claim and exact decoded profile
        if expected_claim_id.is_some_and(|claim_id| capture.identity.claim_id != claim_id) {
            return false;
        }
        matches!(
            &capture.kind,
            RainAdjustmentCaptureKind::Forecast {
                profile: captured,
                run_initialized_at_ms: _,
            } if captured == profile && capture.identity.completed_at == profile.completedAt
        )
    });
    let capture = matches.next()?;
    // reject ambiguous raw lineage instead of choosing by storage order
    if matches.next().is_some() {
        return None;
    }
    Some(capture)
}

/// Project one capture identity in the canonical rain causal-input field order.
fn project_capture(capture: &RainAdjustmentCapture) -> Option<JsonValue> {
    let (kind, station_id, run_initialized_at, window_start, window_end_exclusive) =
        match &capture.kind {
            RainAdjustmentCaptureKind::Forecast {
                profile,
                run_initialized_at_ms: _,
            } => (
                "forecast",
                JsonValue::Null,
                string(&profile.runInitializedAt),
                JsonValue::Null,
                JsonValue::Null,
            ),
            RainAdjustmentCaptureKind::Station {
                station_id,
                window_start,
                window_end_exclusive,
            } => {
                // retain only station identifiers representable by JavaScript exactly
                if *station_id == 0 || *station_id > MAXIMUM_SAFE_INTEGER {
                    return None;
                }
                (
                    "station",
                    JsonValue::Number(*station_id as f64),
                    JsonValue::Null,
                    string(window_start),
                    string(window_end_exclusive),
                )
            }
        };
    Some(object(vec![
        ("claimId", string(&capture.identity.claim_id)),
        ("kind", string(kind)),
        ("stationId", station_id),
        ("runInitializedAt", run_initialized_at),
        ("windowStart", window_start),
        ("windowEndExclusive", window_end_exclusive),
        ("completedAt", string(&capture.identity.completed_at)),
        ("bodySha256", string(&capture.identity.body_sha256)),
    ]))
}

/// Project one exact decoded forecast run and its immutable raw content identity.
fn project_run(profile: &RainWindRunProfile, capture: &RainAdjustmentCapture) -> Option<JsonValue> {
    // reject a profile detached from the selected immutable capture
    if capture.identity.completed_at != profile.completedAt {
        return None;
    }
    let hours = profile
        .hours
        .iter()
        .map(project_forecast_hour)
        .collect::<Option<Vec<_>>>()?;
    Some(object(vec![
        ("runInitializedAt", string(&profile.runInitializedAt)),
        ("completedAt", string(&profile.completedAt)),
        ("contentSha256", string(&capture.identity.body_sha256)),
        ("hours", JsonValue::Array(hours)),
    ]))
}

/// Project one captured forecast hour without changing nullable covariates.
fn project_forecast_hour(hour: &RainWindForecastHour) -> Option<JsonValue> {
    // preserve only finite one-based lead values accepted by the source validator
    if !hour.leadHours.is_finite() {
        return None;
    }
    Some(object(vec![
        ("leadHours", JsonValue::Number(hour.leadHours)),
        (
            "precipitationMm64",
            nullable_binary64(&hour.precipitationMm)?,
        ),
        ("temperatureC64", nullable_binary64(&hour.temperatureC)?),
        (
            "relativeHumidityPercent64",
            nullable_binary64(&hour.relativeHumidityPercent)?,
        ),
        (
            "cloudCoverPercent64",
            nullable_binary64(&hour.cloudCoverPercent)?,
        ),
        ("pressureHpa64", nullable_binary64(&hour.pressureHpa)?),
        ("windSpeedMps64", nullable_binary64(&hour.windSpeedMps)?),
        (
            "windDirectionDegrees64",
            nullable_binary64(&hour.windDirectionDegrees)?,
        ),
    ]))
}

/// Project one exact derived station hour passed to the feature builder.
fn project_station_hour(hour: &weather_contracts::RainWindStationHour) -> Option<JsonValue> {
    // require the station identifier to retain its exact positive integer value
    if !hour.stationId.is_finite()
        || hour.stationId < 1.0
        || hour.stationId.fract() != 0.0
        || hour.stationId > MAXIMUM_SAFE_INTEGER as f64
    {
        return None;
    }
    Some(object(vec![
        ("hourAt", string(&hour.hourAt)),
        ("receivedAt", string(&hour.receivedAt)),
        ("stationId", JsonValue::Number(hour.stationId)),
        (
            "precipitationMm64",
            nullable_binary64(&hour.precipitationMm)?,
        ),
        ("temperatureC64", nullable_binary64(&hour.temperatureC)?),
    ]))
}

/// Bind one incumbent output target to its exact raw forecast source cell.
fn project_source_row(
    index: usize,
    hour: &weather_contracts::RainWindPerformanceHour,
    current_capture: &RainAdjustmentCapture,
    source_sha256: &str,
) -> Option<JsonValue> {
    let RainAdjustmentCaptureKind::Forecast { profile, .. } = &current_capture.kind else {
        return None;
    };
    // convert only exact positive integer model leads into source indices
    if !hour.modelLeadHours.is_finite()
        || hour.modelLeadHours < 1.0
        || hour.modelLeadHours.fract() != 0.0
    {
        return None;
    }
    let source_index = hour.modelLeadHours as usize - 1;
    let raw = profile.hours.get(source_index)?;
    // reject output geometry detached from the captured forecast cell
    if raw.leadHours != hour.modelLeadHours
        || raw.precipitationMm != Nullable::Value(hour.rawPrecipitationMm)
    {
        return None;
    }
    Some(object(vec![
        ("validAt", string(&hour.validAt)),
        ("leadHours", JsonValue::Number((index + 1) as f64)),
        ("modelLeadHours", JsonValue::Number(hour.modelLeadHours)),
        ("referenceAt", string(&profile.runInitializedAt)),
        ("receivedAt", string(&current_capture.identity.completed_at)),
        ("sourceSha256", string(source_sha256)),
        (
            "contentSha256",
            string(&current_capture.identity.body_sha256),
        ),
        ("adapterVersion", string("open-meteo-rain-capture/v1")),
        ("contractEpoch", string("rain-prospective-capture/v1")),
        ("dataset", string("ecmwf_ifs")),
        ("providerKey", string("open-meteo-single-runs")),
        ("sourceKey", string("rain-prospective-forecast")),
        (
            "sourceConfigFingerprint",
            string(RAIN_SOURCE_CONFIG_FINGERPRINT),
        ),
        ("upstreamModel", string("ecmwf_ifs")),
        ("revisionCount", JsonValue::Number(0.0)),
        ("precipitationMm64", binary64(hour.rawPrecipitationMm)?),
    ]))
}

/// Encode one nullable finite value as its lowercase IEEE-754 binary64 bits.
fn nullable_binary64(value: &Nullable<f64>) -> Option<JsonValue> {
    match value {
        Nullable::Null => Some(JsonValue::Null),
        Nullable::Value(value) => binary64(*value),
    }
}

/// Encode one finite value as its lowercase IEEE-754 binary64 bits.
fn binary64(value: f64) -> Option<JsonValue> {
    // refuse non-finite values before exposing their reserved bit patterns
    if !value.is_finite() {
        return None;
    }
    Some(string(&format!("{:016x}", value.to_bits())))
}

/// Build one dependency-free ordered JSON object.
fn object(entries: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one dependency-free UTF-16 JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
