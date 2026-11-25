//! Project retained database rain captures into the worker's typed inference boundary.

use std::borrow::Borrow;

use weather_contracts::{
    Nullable, RainAdjustmentCapture as DatabaseRainAdjustmentCapture,
    RainAdjustmentCaptureKindEnum, RainWindForecastHour, RainWindRunProfile,
};
use weather_worker::{
    RainAdjustmentCapture, RainAdjustmentCaptureIdentity, RainAdjustmentCaptureKind,
    RainAdjustmentPendingInput, TaskFailure,
};

use crate::worker_rain_gauge_replay::{
    WorkerRainGaugeReplayValues, replay_worker_rain_station_hours,
};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const DECISION_DELAY_HOURS: i64 = 8;
const FORECAST_HOUR_COUNT: usize = 48;
const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Typed Open-Meteo hourly columns decoded by the approved JSON owner.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerRainForecastHourly {
    pub time: Vec<String>,
    pub precipitation: Vec<Nullable<f64>>,
    pub temperature_2m: Vec<Nullable<f64>>,
    pub relative_humidity_2m: Vec<Nullable<f64>>,
    pub cloud_cover: Vec<Nullable<f64>>,
    pub surface_pressure: Vec<Nullable<f64>>,
    pub wind_speed_10m: Vec<Nullable<f64>>,
    pub wind_direction_10m: Vec<Nullable<f64>>,
}

/// Typed forecast capture body after approved UTF-8 and JSON decoding.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerRainForecastPayload {
    pub utc_offset_seconds: i64,
    pub hourly: WorkerRainForecastHourly,
}

/// Approved capture decoding, UTC, and fixed-gauge replay primitives.
pub trait WorkerRainAdjustmentCaptureValues: WorkerRainGaugeReplayValues {
    /// Decode one already hash-checked forecast capture without normalizing its cells.
    fn decode_forecast(&mut self, bytes: &[u8]) -> Result<WorkerRainForecastPayload, TaskFailure>;

    /// Format one epoch in Open-Meteo's minute-precision hourly spelling.
    fn format_open_meteo_hour(&mut self, epoch_ms: i64) -> Result<String, TaskFailure>;
}

/// Decode forecast bodies once and retain exact causal station replay in database order.
pub fn project_worker_rain_adjustment_inputs(
    values: &mut impl WorkerRainAdjustmentCaptureValues,
    captures: Vec<DatabaseRainAdjustmentCapture>,
) -> Result<RainAdjustmentPendingInput, TaskFailure> {
    project_borrowed_worker_rain_adjustment_inputs(values, &captures)
}

/// Decode borrowed archive captures without copying retained provider bodies per saved run.
pub fn project_borrowed_worker_rain_adjustment_inputs<Capture>(
    values: &mut impl WorkerRainAdjustmentCaptureValues,
    captures: &[Capture],
) -> Result<RainAdjustmentPendingInput, TaskFailure>
where
    Capture: Borrow<DatabaseRainAdjustmentCapture>,
{
    let mut decoded_forecasts = Vec::with_capacity(captures.len());
    let mut current_initialized_at_ms = None;
    // validate metadata and decode every forecast before consuming raw capture bodies
    for captured in captures {
        let capture = captured.borrow();
        let completed_at_ms = values.parse_utc_milliseconds(&capture.completedAt)?;
        let decoded = match capture.kind {
            RainAdjustmentCaptureKindEnum::Forecast => {
                let Nullable::Value(run_initialized_at) = &capture.runInitializedAt else {
                    return Err(projection_failure());
                };
                // a forecast capture carries no station interval identity
                if !matches!(&capture.stationId, Nullable::Null)
                    || !matches!(&capture.windowStart, Nullable::Null)
                    || !matches!(&capture.windowEndExclusive, Nullable::Null)
                {
                    return Err(projection_failure());
                }
                let run_initialized_at_ms = values.parse_utc_milliseconds(run_initialized_at)?;
                let profile = decode_forecast_profile(
                    values,
                    &capture.body,
                    run_initialized_at,
                    &capture.completedAt,
                    run_initialized_at_ms,
                )?;
                // retain the newest forecast decision clock without reordering captures
                if current_initialized_at_ms.is_none_or(|current| run_initialized_at_ms > current) {
                    current_initialized_at_ms = Some(run_initialized_at_ms);
                }
                Some((profile, run_initialized_at_ms))
            }
            RainAdjustmentCaptureKindEnum::Station => {
                let (
                    Nullable::Value(station_id),
                    Nullable::Value(window_start),
                    Nullable::Value(window_end_exclusive),
                ) = (
                    &capture.stationId,
                    &capture.windowStart,
                    &capture.windowEndExclusive,
                )
                else {
                    return Err(projection_failure());
                };
                // a station capture must retain its exact station interval lineage
                if !matches!(&capture.runInitializedAt, Nullable::Null)
                    || *station_id == 0
                    || *station_id > MAXIMUM_SAFE_INTEGER
                {
                    return Err(projection_failure());
                }
                values.parse_utc_milliseconds(window_start)?;
                values.parse_utc_milliseconds(window_end_exclusive)?;
                None
            }
        };
        decoded_forecasts.push((completed_at_ms, decoded));
    }

    let station_hours = match current_initialized_at_ms {
        Some(initialized_at_ms) => {
            let decision_at_ms = initialized_at_ms
                .checked_add(DECISION_DELAY_HOURS * HOUR_MILLISECONDS)
                .ok_or_else(projection_failure)?;
            replay_worker_rain_station_hours(values, captures, decision_at_ms)?
        }
        None => Vec::new(),
    };
    let mut projected = Vec::with_capacity(captures.len());
    // clone only small metadata while all provider bodies remain borrowed
    for (captured, (completed_at_ms, decoded)) in captures.iter().zip(decoded_forecasts) {
        let capture = captured.borrow();
        let identity = RainAdjustmentCaptureIdentity {
            body_sha256: capture.bodySha256.clone(),
            claim_id: capture.claimId.clone(),
            completed_at: capture.completedAt.clone(),
            completed_at_ms,
        };
        let kind = match (capture.kind, decoded) {
            (RainAdjustmentCaptureKindEnum::Forecast, Some((profile, run_initialized_at_ms))) => {
                RainAdjustmentCaptureKind::Forecast {
                    profile,
                    run_initialized_at_ms,
                }
            }
            (RainAdjustmentCaptureKindEnum::Station, None) => {
                let (
                    Nullable::Value(station_id),
                    Nullable::Value(window_start),
                    Nullable::Value(window_end_exclusive),
                ) = (
                    &capture.stationId,
                    &capture.windowStart,
                    &capture.windowEndExclusive,
                )
                else {
                    return Err(projection_failure());
                };
                RainAdjustmentCaptureKind::Station {
                    station_id: *station_id,
                    window_start: window_start.clone(),
                    window_end_exclusive: window_end_exclusive.clone(),
                }
            }
            _ => return Err(projection_failure()),
        };
        projected.push(RainAdjustmentCapture { identity, kind });
    }
    Ok(RainAdjustmentPendingInput {
        captures: projected,
        station_hours,
    })
}

/// Decode one exact 48-hour Open-Meteo forecast profile.
fn decode_forecast_profile(
    values: &mut impl WorkerRainAdjustmentCaptureValues,
    body: &[u8],
    run_initialized_at: &str,
    completed_at: &str,
    initialized_at_ms: i64,
) -> Result<RainWindRunProfile, TaskFailure> {
    let payload = values.decode_forecast(body)?;
    // preserve the original UTC and 49-cell time-axis admission before indexing
    if payload.utc_offset_seconds != 0 || payload.hourly.time.len() != FORECAST_HOUR_COUNT + 1 {
        return Err(projection_failure());
    }
    let hourly = payload.hourly;
    let mut hours = Vec::with_capacity(FORECAST_HOUR_COUNT);
    // omit initialization and retain the original one-based lead grid
    for lead in 1..=FORECAST_HOUR_COUNT {
        let lead_at_ms = initialized_at_ms
            .checked_add(lead as i64 * HOUR_MILLISECONDS)
            .ok_or_else(projection_failure)?;
        let expected = values.format_open_meteo_hour(lead_at_ms)?;
        // reject a source hour shifted away from the claimed initialized run
        if hourly.time.get(lead) != Some(&expected) {
            return Err(projection_failure());
        }
        hours.push(RainWindForecastHour {
            leadHours: lead as f64,
            precipitationMm: forecast_cell(&hourly.precipitation, lead)?,
            temperatureC: forecast_cell(&hourly.temperature_2m, lead)?,
            relativeHumidityPercent: forecast_cell(&hourly.relative_humidity_2m, lead)?,
            cloudCoverPercent: forecast_cell(&hourly.cloud_cover, lead)?,
            pressureHpa: forecast_cell(&hourly.surface_pressure, lead)?,
            windSpeedMps: forecast_cell(&hourly.wind_speed_10m, lead)?,
            windDirectionDegrees: forecast_cell(&hourly.wind_direction_10m, lead)?,
        });
    }
    Ok(RainWindRunProfile {
        runInitializedAt: run_initialized_at.to_owned(),
        completedAt: completed_at.to_owned(),
        hours,
    })
}

/// Preserve one nullable finite forecast cell without fabricating zero.
fn forecast_cell(values: &[Nullable<f64>], index: usize) -> Result<Nullable<f64>, TaskFailure> {
    match values.get(index) {
        Some(Nullable::Null) => Ok(Nullable::Null),
        Some(Nullable::Value(value)) if value.is_finite() => Ok(Nullable::Value(*value)),
        Some(Nullable::Value(_)) | None => Err(projection_failure()),
    }
}

/// Produce one stable capture-projection failure without provider attempts.
fn projection_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "rain capture projection differs".to_owned(),
    }
}
