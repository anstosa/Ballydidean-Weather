//! Exact causal rain source projection and raw-versus-packaged model replay.

use std::collections::BTreeSet;

use weather_api::{
    AdmittedRainHurdleWindFeatureInput, AdmittedRainHurdleWindStations,
    build_admitted_rain_hurdle_wind_features,
};
use weather_contracts::{
    JsonValue, Nullable, RainHurdleWindFeatureProjection, RainHurdleWindFeatureRow,
    RainHurdleWindPortableArtifact, RainWindForecastHour, RainWindPerformanceResult,
    RainWindPredictionHourReasonCodeEnum,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_worker::{RainAdjustmentFeatureAdmission, replay_rain_maintenance_candidate};

use crate::maintenance_canonical_json::{field, text};
use crate::native_maintenance_daily_candidate_replay::{
    NativeMaintenanceDailyCandidateReplayError as Error, NativeMaintenanceDailyCandidateReplayPair,
    build_native_rain_maintenance_artifact,
};
use crate::native_maintenance_policy_comparison::{number, object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    MaintenanceCanonicalJson, NativeModelEvidenceValues, NativeRainAdjustmentFeatureAdmission,
    NativeRainPortableCalendar, NativeWirePrimitives,
};

const HOUR: i64 = 3_600_000;
const DECISION_DELAY_HOURS: i64 = 8;
const OUTPUT_HOURS: usize = 23;
const FORECAST_HOURS: usize = 48;
const STATION_IDS: [u64; 12] = [
    64_255, 225_947, 38_270, 168_853, 126_537, 201_058, 203_055, 66_270, 34_768, 88_159, 126_197,
    27_140,
];
const STATION_LAGS: [i64; 6] = [1, 2, 3, 6, 12, 24];

struct AdmittedRun {
    completed_at_ms: i64,
    initialized_at_ms: i64,
    hours: [RainWindForecastHour; FORECAST_HOURS],
    run_initialized_at: String,
}

/// Replay one producer-admitted causal rain source through raw and packaged portable artifacts.
pub fn replay_native_rain_causal_candidate_pair(
    source: &JsonValue,
    candidate_bytes: &[u8],
    artifact_bytes: &[u8],
) -> Result<NativeMaintenanceDailyCandidateReplayPair, Error> {
    let candidate = build_native_rain_maintenance_artifact(candidate_bytes)?;
    let (packaged_artifact, packaged_sha256) = admit_rain_artifact_bytes(artifact_bytes)?;
    let projection = build_native_rain_causal_feature_projection(source)?;
    let mut candidate_calendar =
        NativeRainPortableCalendar::load().map_err(|_| Error::Evaluation)?;
    let mut packaged_calendar =
        NativeRainPortableCalendar::load().map_err(|_| Error::Evaluation)?;
    let candidate_result = replay_rain_maintenance_candidate(
        &projection,
        &candidate.artifact_sha256,
        &candidate.artifact,
        &mut candidate_calendar,
    )
    .map_err(|_| Error::Evaluation)?;
    let packaged_result = replay_rain_maintenance_candidate(
        &projection,
        &packaged_sha256,
        &packaged_artifact,
        &mut packaged_calendar,
    )
    .map_err(|_| Error::Evaluation)?;
    let wire = NativeWirePrimitives;
    Ok(NativeMaintenanceDailyCandidateReplayPair {
        candidate_decision_bytes: wire
            .encode_sorted_json(&rain_performance_json(&candidate_result)?)
            .ok_or(Error::Canonical)?,
        packaged_decision_bytes: wire
            .encode_sorted_json(&rain_performance_json(&packaged_result)?)
            .ok_or(Error::Canonical)?,
        portable_candidate_bytes: candidate.artifact_bytes,
    })
}

/// Build the exact 23-row native feature projection from one admitted causal source graph.
pub fn build_native_rain_causal_feature_projection(
    source: &JsonValue,
) -> Result<RainHurdleWindFeatureProjection, Error> {
    // require the family and current causal grammar before decoding numerical inputs
    if text(source, "family").as_deref() != Some("rain") {
        return Err(Error::Input);
    }
    let now_ms = canonical_milliseconds(&text(source, "issuedAt").ok_or(Error::Input)?)?;
    let causal = field(source, "causalInputs").ok_or(Error::Input)?;
    let current = decode_run(field(causal, "currentRun").ok_or(Error::Input)?)?;
    let decision_ms = current
        .initialized_at_ms
        .checked_add(DECISION_DELAY_HOURS * HOUR)
        .ok_or(Error::Input)?;
    // preserve the source maturity check before preparing prior cycles or stations
    if decision_ms > now_ms {
        return Err(Error::Input);
    }
    // require the selected forecast receipt to exist by its decision boundary
    if current.completed_at_ms > decision_ms {
        return Err(Error::Input);
    }
    let JsonValue::Array(prior_values) = field(causal, "priorRuns").ok_or(Error::Input)? else {
        return Err(Error::Input);
    };
    let mut prior_6h = None;
    let mut prior_12h = None;
    // admit only the two source cycles consumed by the fixed feature contract
    for value in prior_values {
        let run = decode_run(value)?;
        if run.completed_at_ms > decision_ms {
            return Err(Error::Input);
        }
        let lag = current
            .initialized_at_ms
            .checked_sub(run.initialized_at_ms)
            .ok_or(Error::Input)?;
        match lag {
            value if value == 6 * HOUR && prior_6h.is_none() => prior_6h = Some(run),
            value if value == 12 * HOUR && prior_12h.is_none() => prior_12h = Some(run),
            _ => return Err(Error::Input),
        }
    }
    let stations = decode_stations(
        field(causal, "stationHours").ok_or(Error::Input)?,
        decision_ms,
    )?;
    let mut admission = NativeRainAdjustmentFeatureAdmission;
    let station_weights = admission.station_weights().map_err(|_| Error::Evaluation)?;
    let lag_refs = std::array::from_fn(|index| &stations.precipitation_by_lag[index]);
    let admitted_stations = AdmittedRainHurdleWindStations {
        precipitation_by_lag: lag_refs,
        station_weights: &station_weights,
        temperature_lag1: &stations.temperature_lag1,
    };
    let mut rows = Vec::with_capacity(OUTPUT_HOURS);
    // preserve the predecessor's fitted output leads nine through thirty-one
    for output_lead in 1..=OUTPUT_HOURS {
        let source_lead =
            usize::try_from(DECISION_DELAY_HOURS).map_err(|_| Error::Input)? + output_lead;
        let source_hour = current.hours.get(source_lead - 1).ok_or(Error::Input)?;
        let raw = required_nullable(&source_hour.precipitationMm)?;
        let temperature = required_nullable(&source_hour.temperatureC)?;
        let valid_at_ms = current
            .initialized_at_ms
            .checked_add(i64::try_from(source_lead).map_err(|_| Error::Input)? * HOUR)
            .ok_or(Error::Input)?;
        let valid_at = NativeWirePrimitives
            .format_utc_milliseconds(valid_at_ms)
            .ok_or(Error::Input)?;
        let features =
            build_admitted_rain_hurdle_wind_features(AdmittedRainHurdleWindFeatureInput {
                calendar: admission
                    .calendar(valid_at_ms)
                    .map_err(|_| Error::Evaluation)?,
                current_hours: &current.hours,
                model_lead_hours: u8::try_from(output_lead).map_err(|_| Error::Input)?,
                prior_12h_hours: prior_12h.as_ref().map(|run| &run.hours),
                prior_6h_hours: prior_6h.as_ref().map(|run| &run.hours),
                stations: admitted_stations,
            })
            .map_err(|_| Error::Evaluation)?;
        rows.push(RainHurdleWindFeatureRow {
            features: features.to_vec(),
            modelLeadHours: source_lead as f64,
            rawPrecipitationMm: raw,
            rawTargetHourTemperatureC: temperature,
            validAt: valid_at,
        });
    }
    Ok(RainHurdleWindFeatureProjection {
        decisionAt: NativeWirePrimitives
            .format_utc_milliseconds(decision_ms)
            .ok_or(Error::Input)?,
        rows,
        runInitializedAt: current.run_initialized_at,
    })
}

struct AdmittedStations {
    precipitation_by_lag: [[Nullable<f64>; 12]; 6],
    temperature_lag1: [Nullable<f64>; 12],
}

// decode one complete source forecast profile after exact clock and geometry checks
fn decode_run(value: &JsonValue) -> Result<AdmittedRun, Error> {
    let run_initialized_at = text(value, "runInitializedAt").ok_or(Error::Input)?;
    let initialized_at_ms = canonical_hour(&run_initialized_at)?;
    let completed_at_ms = canonical_milliseconds(&text(value, "completedAt").ok_or(Error::Input)?)?;
    let JsonValue::Array(values) = field(value, "hours").ok_or(Error::Input)? else {
        return Err(Error::Input);
    };
    // require receipt after initialization and the fixed 48-hour one-based profile
    if initialized_at_ms > completed_at_ms || values.len() != FORECAST_HOURS {
        return Err(Error::Input);
    }
    let hours = values
        .iter()
        .enumerate()
        .map(|(index, value)| decode_forecast_hour(value, index + 1))
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| Error::Input)?;
    Ok(AdmittedRun {
        completed_at_ms,
        initialized_at_ms,
        hours,
        run_initialized_at,
    })
}

// decode one complete physical forecast row without replacing missing values by zero
fn decode_forecast_hour(
    value: &JsonValue,
    expected_lead: usize,
) -> Result<RainWindForecastHour, Error> {
    if integer(value, "leadHours") != Some(expected_lead as u64) {
        return Err(Error::Input);
    }
    Ok(RainWindForecastHour {
        leadHours: expected_lead as f64,
        precipitationMm: nullable_binary64(value, "precipitationMm64", 0.0, 2_000.0)?,
        temperatureC: nullable_binary64(value, "temperatureC64", -100.0, 70.0)?,
        relativeHumidityPercent: nullable_binary64(value, "relativeHumidityPercent64", 0.0, 100.0)?,
        cloudCoverPercent: nullable_binary64(value, "cloudCoverPercent64", 0.0, 100.0)?,
        pressureHpa: nullable_binary64(value, "pressureHpa64", 100.0, 1_200.0)?,
        windSpeedMps: nullable_binary64(value, "windSpeedMps64", 0.0, 150.0)?,
        windDirectionDegrees: nullable_binary64(value, "windDirectionDegrees64", 0.0, 360.0)?,
    })
}

// bind station rows to fixed catalog order and six exact decision-time lags
fn decode_stations(value: &JsonValue, decision_ms: i64) -> Result<AdmittedStations, Error> {
    let JsonValue::Array(values) = value else {
        return Err(Error::Input);
    };
    let latest_hour_ms = decision_ms.checked_sub(HOUR).ok_or(Error::Input)?;
    let mut precipitation_by_lag = std::array::from_fn(|_| std::array::from_fn(|_| Nullable::Null));
    let mut temperature_lag1 = std::array::from_fn(|_| Nullable::Null);
    let mut seen = BTreeSet::new();
    // validate every station receipt before selecting only the consumed lag rows
    for value in values {
        let station_id = integer(value, "stationId").ok_or(Error::Input)?;
        let station_index = STATION_IDS
            .iter()
            .position(|expected| *expected == station_id)
            .ok_or(Error::Input)?;
        let hour_ms = canonical_hour(&text(value, "hourAt").ok_or(Error::Input)?)?;
        let received_ms = canonical_milliseconds(&text(value, "receivedAt").ok_or(Error::Input)?)?;
        // preserve the source causal and duplicate-key checks before joining values
        if received_ms > decision_ms
            || hour_ms > latest_hour_ms
            || hour_ms > received_ms
            || !seen.insert((hour_ms, station_id))
        {
            return Err(Error::Input);
        }
        let precipitation = nullable_binary64(value, "precipitationMm64", 0.0, 500.0)?;
        let temperature = nullable_binary64(value, "temperatureC64", -80.0, 60.0)?;
        // retain values only at the model's exact six lag boundaries
        for (lag_index, lag_hours) in STATION_LAGS.iter().enumerate() {
            if decision_ms.checked_sub(lag_hours * HOUR) == Some(hour_ms) {
                precipitation_by_lag[lag_index][station_index] = precipitation.clone();
                // expose temperature only at lag one as the source feature owner does
                if lag_index == 0 {
                    temperature_lag1[station_index] = temperature.clone();
                }
            }
        }
    }
    Ok(AdmittedStations {
        precipitation_by_lag,
        temperature_lag1,
    })
}

// admit exact public artifact bytes and retain their content address for decision output
fn admit_rain_artifact_bytes(
    bytes: &[u8],
) -> Result<(RainHurdleWindPortableArtifact, String), Error> {
    let mut wire = NativeWirePrimitives;
    let document = wire.parse_canonical_object(bytes).ok_or(Error::Package)?;
    let values = NativeModelEvidenceValues::new();
    let artifact = weather_api::admit_rain_portable_artifact(
        &document,
        |value| values.decode_shadow_rain_artifact(value),
        |names| {
            let bytes = values.encode_shadow_ordered_json(names)?;
            ApiMaintenanceReceiptCanonicalValues::sha256(&values, &bytes)
        },
    )
    .ok_or(Error::Package)?;
    Ok((
        artifact,
        format_digest(RevisionArchiveCanonicalCodec::sha256(&wire, bytes)),
    ))
}

// project the generated performance result back to the predecessor's canonical JSON shape
fn rain_performance_json(result: &RainWindPerformanceResult) -> Result<JsonValue, Error> {
    let hours = result
        .hours
        .iter()
        .map(|hour| {
            let reason = match hour.reasonCode {
                Nullable::Null => JsonValue::Null,
                Nullable::Value(reason) => string(enum_wire(
                    reason,
                    RainWindPredictionHourReasonCodeEnum::WIRE_VALUES,
                )?),
            };
            let occurrence = match &hour.occurrenceProbabilities {
                Nullable::Null => JsonValue::Null,
                Nullable::Value(value) => object(vec![
                    ("atLeast0_1", JsonValue::Number(value.atLeast0_1)),
                    ("atLeast1_0", JsonValue::Number(value.atLeast1_0)),
                    ("atLeast2_5", JsonValue::Number(value.atLeast2_5)),
                ]),
            };
            Ok(object(vec![
                ("applied", JsonValue::Boolean(hour.applied)),
                (
                    "correctedPrecipitationMm",
                    JsonValue::Number(hour.correctedPrecipitationMm),
                ),
                ("modelLeadHours", JsonValue::Number(hour.modelLeadHours)),
                ("occurrenceProbabilities", occurrence),
                (
                    "positiveAmountMm",
                    match hour.positiveAmountMm {
                        Nullable::Null => JsonValue::Null,
                        Nullable::Value(value) => JsonValue::Number(value),
                    },
                ),
                (
                    "rawPrecipitationMm",
                    JsonValue::Number(hour.rawPrecipitationMm),
                ),
                ("reasonCode", reason),
                ("validAt", string(&hour.validAt)),
            ]))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(object(vec![
        ("decisionAt", string(&result.decisionAt)),
        ("hours", JsonValue::Array(hours)),
        ("modelMonth", string(&result.modelMonth)),
        ("modelSha256", string(&result.modelSha256)),
    ]))
}

// require one present source value for the output target row
fn required_nullable(value: &Nullable<f64>) -> Result<f64, Error> {
    match value {
        Nullable::Value(value) if value.is_finite() => Ok(*value),
        Nullable::Null | Nullable::Value(_) => Err(Error::Input),
    }
}

// decode exact binary64 hexadecimal source storage while preserving explicit nulls
fn nullable_binary64(
    value: &JsonValue,
    key: &str,
    minimum: f64,
    maximum: f64,
) -> Result<Nullable<f64>, Error> {
    match field(value, key) {
        Some(JsonValue::Null) => Ok(Nullable::Null),
        Some(JsonValue::String(units)) => {
            let encoded = String::from_utf16(units).map_err(|_| Error::Input)?;
            let bits = u64::from_str_radix(&encoded, 16).map_err(|_| Error::Input)?;
            let decoded = f64::from_bits(bits);
            // require exact fixed-width lowercase source spelling and physical range
            if encoded.len() != 16
                || !encoded
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
                || !decoded.is_finite()
                || !(minimum..=maximum).contains(&decoded)
            {
                return Err(Error::Input);
            }
            Ok(Nullable::Value(decoded))
        }
        _ => Err(Error::Input),
    }
}

// parse one exact canonical UTC millisecond source clock
fn canonical_milliseconds(value: &str) -> Result<i64, Error> {
    NativeWirePrimitives
        .canonical_utc_milliseconds(value)
        .ok_or(Error::Input)
}

// parse one canonical top-of-hour clock used by the source join
fn canonical_hour(value: &str) -> Result<i64, Error> {
    canonical_milliseconds(value).and_then(|milliseconds| {
        (milliseconds.rem_euclid(HOUR) == 0)
            .then_some(milliseconds)
            .ok_or(Error::Input)
    })
}

// decode one nonnegative safe integer source field
fn integer(value: &JsonValue, key: &str) -> Option<u64> {
    let value = number(value, key)?;
    (value.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(&value))
        .then_some(value as u64)
}

// return one generated enum's exact wire spelling
fn enum_wire<T: Copy + PartialEq>(
    value: T,
    values: &[(&'static str, T)],
) -> Result<&'static str, Error> {
    values
        .iter()
        .find_map(|(wire, candidate)| (*candidate == value).then_some(*wire))
        .ok_or(Error::Evaluation)
}

#[cfg(test)]
#[path = "native_maintenance_daily_rain_causal_replay_test.rs"]
mod tests;
