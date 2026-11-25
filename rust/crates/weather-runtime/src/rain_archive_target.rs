//! Exact fixed-gauge target replay from verified archived rain captures.

use std::borrow::Borrow;

use weather_contracts::{Nullable, RainAdjustmentCapture};
use weather_worker::{RAIN_COLLECTION_STATIONS, RainPerformanceTarget, TaskFailure};

use crate::maintenance_canonical_json::valid_hash;
use crate::worker_rain_gauge_replay::{
    WorkerRainGaugeReplayValues, replay_worker_rain_station_hour,
};
use crate::worker_rain_persistence::fixed_gauge_weighted_median;

const HOUR_MILLISECONDS: i64 = 3_600_000;

/// One original station row admitted to the canonical target identity.
#[derive(Debug, Clone, PartialEq)]
pub struct RainArchiveTargetRow {
    pub precipitation_mm: Nullable<f64>,
    pub received_at: String,
    pub station_id: u64,
}

/// Exact typed preimage encoded and hashed by the approved canonical owner.
#[derive(Debug, Clone, PartialEq)]
pub struct RainArchiveTargetPreimage {
    pub rows: Vec<RainArchiveTargetRow>,
    pub valid_at: String,
}

/// Existing canonical JSON and SHA-256 owner for the original target identity.
pub trait RainArchiveTargetIdentityValues {
    /// Encode and hash the exact typed target preimage.
    fn rain_target_identity_sha256(
        &mut self,
        preimage: &RainArchiveTargetPreimage,
    ) -> Option<String>;
}

/// Replay one preceding gauge hour and return its original performance target.
pub fn replay_rain_archive_target(
    values: &mut (impl WorkerRainGaugeReplayValues + RainArchiveTargetIdentityValues),
    captures: &[impl Borrow<RainAdjustmentCapture>],
    valid_at: &str,
) -> Result<RainPerformanceTarget, TaskFailure> {
    let valid_at_ms = values.parse_utc_milliseconds(valid_at)?;
    let decision_at_ms = valid_at_ms
        .checked_add(HOUR_MILLISECONDS)
        .ok_or_else(target_failure)?;
    // reproduce the original strict row-hour equality after Date-to-ISO conversion
    if values.format_utc_milliseconds(valid_at_ms)? != valid_at {
        return Ok(missing_target(valid_at));
    }
    let station_hours =
        replay_worker_rain_station_hour(values, captures, valid_at_ms, decision_at_ms)?;
    let mut available = Vec::with_capacity(station_hours.len());
    let mut rows = Vec::with_capacity(station_hours.len());
    // retain every original station row, including temperature-only null precipitation rows
    for hour in station_hours {
        let station_id = station_id(&hour.row.stationId).ok_or_else(target_failure)?;
        let station_index = RAIN_COLLECTION_STATIONS
            .iter()
            .position(|station| station.location_id == station_id)
            .ok_or_else(target_failure)?;
        // admit only the same finite nonnegative physical amounts as the TypeScript target
        if let Nullable::Value(amount) = &hour.row.precipitationMm {
            if !amount.is_finite() || *amount < 0.0 {
                return Err(target_failure());
            }
            available.push((station_index, *amount));
        }
        rows.push(RainArchiveTargetRow {
            precipitation_mm: hour.row.precipitationMm,
            received_at: hour.row.receivedAt,
            station_id,
        });
    }
    let Some(target) = fixed_gauge_weighted_median(&mut available) else {
        return Ok(missing_target(valid_at));
    };
    rows.sort_unstable_by_key(|row| row.station_id);
    let preimage = RainArchiveTargetPreimage {
        rows,
        valid_at: valid_at.to_owned(),
    };
    let target_key = values
        .rain_target_identity_sha256(&preimage)
        .filter(|identity| valid_hash(identity))
        .ok_or_else(target_failure)?;
    Ok(RainPerformanceTarget {
        target: Nullable::Value(target),
        target_key,
    })
}

/// Convert one replay-owned JavaScript-safe station identity without rounding.
fn station_id(value: &f64) -> Option<u64> {
    // require the exact positive integer identity emitted by the fixed catalog
    if !value.is_finite() || *value <= 0.0 || value.fract() != 0.0 || *value > u64::MAX as f64 {
        return None;
    }
    Some(*value as u64)
}

/// Preserve the evidence join's original incomplete target key.
fn missing_target(valid_at: &str) -> RainPerformanceTarget {
    RainPerformanceTarget {
        target: Nullable::Null,
        target_key: format!("missing:{valid_at}"),
    }
}

/// Produce one stable target failure without provider attempts.
fn target_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "rain archive target differs".to_owned(),
    }
}

#[cfg(test)]
#[path = "rain_archive_target_test.rs"]
mod tests;
