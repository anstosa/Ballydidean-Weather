//! Native causal replay of retained Tempest captures into fixed-gauge station hours.

use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::BTreeMap;

use weather_contracts::{Nullable, RainAdjustmentCapture as DatabaseRainAdjustmentCapture};
use weather_worker::{
    NormalizedTempestRecord, RAIN_COLLECTION_STATIONS, RainAdjustmentStationHour,
    RainCollectionStation, RainFixedGaugeCapture, TaskFailure, TempestObservationEnvelope,
    TempestObservationRequest, TempestQualityFlag, normalize_tempest_observations,
};

const MINUTE_MILLISECONDS: i64 = 60_000;
const HOUR_MILLISECONDS: i64 = 60 * MINUTE_MILLISECONDS;
const STATION_LAGS_HOURS: [i64; 6] = [1, 2, 3, 6, 12, 24];

/// Approved raw Tempest decoding and canonical UTC primitives.
pub trait WorkerRainGaugeReplayValues {
    /// Decode one verified raw Tempest response into its typed provider envelope.
    fn decode_tempest(&mut self, bytes: &[u8]) -> Result<TempestObservationEnvelope, TaskFailure>;

    /// Parse one exact canonical UTC instant into epoch milliseconds.
    fn parse_utc_milliseconds(&mut self, value: &str) -> Result<i64, TaskFailure>;

    /// Format one epoch as an exact canonical UTC millisecond instant.
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Result<String, TaskFailure>;
}

struct ReplayInterval {
    amount: Option<f64>,
    capture_index: usize,
    minutes: i64,
    record_temperature: Option<f64>,
}

/// Replay all six frozen lag hours while normalizing each selected capture at most once.
pub fn replay_worker_rain_station_hours(
    values: &mut impl WorkerRainGaugeReplayValues,
    captures: &[impl Borrow<DatabaseRainAdjustmentCapture>],
    decision_at_ms: i64,
) -> Result<Vec<RainAdjustmentStationHour>, TaskFailure> {
    let _decision_at = values.format_utc_milliseconds(decision_at_ms)?;
    let mut normalized = std::iter::repeat_with(|| None)
        .take(captures.len())
        .collect::<Vec<Option<Vec<NormalizedTempestRecord>>>>();
    let mut result = Vec::new();
    // reproduce only the model's frozen causal lag requests
    for lag in STATION_LAGS_HOURS {
        let hour_at_ms = decision_at_ms
            .checked_sub(lag * HOUR_MILLISECONDS)
            .ok_or_else(replay_failure)?;
        replay_worker_rain_station_hour_cached(
            values,
            captures,
            hour_at_ms,
            decision_at_ms,
            &mut normalized,
            &mut result,
        )?;
    }
    Ok(result)
}

/// Replay only one requested gauge hour for archive target construction.
pub fn replay_worker_rain_station_hour(
    values: &mut impl WorkerRainGaugeReplayValues,
    captures: &[impl Borrow<DatabaseRainAdjustmentCapture>],
    hour_at_ms: i64,
    decision_at_ms: i64,
) -> Result<Vec<RainAdjustmentStationHour>, TaskFailure> {
    let _decision_at = values.format_utc_milliseconds(decision_at_ms)?;
    let mut normalized = std::iter::repeat_with(|| None)
        .take(captures.len())
        .collect::<Vec<Option<Vec<NormalizedTempestRecord>>>>();
    let mut result = Vec::new();
    replay_worker_rain_station_hour_cached(
        values,
        captures,
        hour_at_ms,
        decision_at_ms,
        &mut normalized,
        &mut result,
    )?;
    Ok(result)
}

/// Replay one hour while sharing normalized captures with a wider lag request.
fn replay_worker_rain_station_hour_cached(
    values: &mut impl WorkerRainGaugeReplayValues,
    captures: &[impl Borrow<DatabaseRainAdjustmentCapture>],
    hour_at_ms: i64,
    decision_at_ms: i64,
    normalized: &mut [Option<Vec<NormalizedTempestRecord>>],
    result: &mut Vec<RainAdjustmentStationHour>,
) -> Result<(), TaskFailure> {
    let hour_at = values.format_utc_milliseconds(hour_at_ms)?;
    // preserve the fixed catalog order within the requested hour
    for station in RAIN_COLLECTION_STATIONS {
        let selected = select_captures(
            values,
            captures,
            station.location_id,
            hour_at_ms,
            decision_at_ms,
        )?;
        // legacy model inputs omit stations outside the requested lag window
        if selected.is_empty() {
            continue;
        }
        let observations = collect_observations(
            values,
            captures,
            &selected,
            station.device_id,
            station.location_id,
            station.serial,
            normalized,
        )?;
        let replay = replay_station_hour(
            |index| captures[index].borrow().completedAt.as_str(),
            &observations,
            hour_at_ms,
            None,
        )?;
        let Some(received_at) = replay.received_at else {
            continue;
        };
        // retain only station rows containing at least one model input
        if replay.precipitation_mm.is_none() && replay.temperature_c.is_none() {
            continue;
        }
        let received_at_ms = values.parse_utc_milliseconds(&received_at)?;
        result.push(RainAdjustmentStationHour {
            hour_at_ms,
            received_at_ms,
            row: weather_contracts::RainWindStationHour {
                hourAt: hour_at.clone(),
                receivedAt: received_at,
                stationId: station.location_id as f64,
                precipitationMm: nullable(replay.precipitation_mm),
                temperatureC: nullable(replay.temperature_c),
            },
        });
    }
    Ok(())
}

/// Select exact overlapping immutable captures in Node revision precedence order.
fn select_captures(
    values: &mut impl WorkerRainGaugeReplayValues,
    captures: &[impl Borrow<DatabaseRainAdjustmentCapture>],
    station_id: u64,
    target_ms: i64,
    cutoff_ms: i64,
) -> Result<Vec<usize>, TaskFailure> {
    let earliest_ms = target_ms
        .checked_sub(65 * MINUTE_MILLISECONDS)
        .ok_or_else(replay_failure)?;
    let mut selected = Vec::new();
    // inspect metadata before decoding any favorable provider value
    for (index, capture) in captures.iter().enumerate() {
        let capture = capture.borrow();
        let (
            weather_contracts::RainAdjustmentCaptureKindEnum::Station,
            Nullable::Value(captured_station_id),
            Nullable::Value(window_start),
            Nullable::Value(window_end_exclusive),
        ) = (
            capture.kind,
            &capture.stationId,
            &capture.windowStart,
            &capture.windowEndExclusive,
        )
        else {
            continue;
        };
        // retain only this station's responses able to prove the backward hour
        if *captured_station_id != station_id {
            continue;
        }
        let completed_at_ms = values.parse_utc_milliseconds(&capture.completedAt)?;
        let window_start_ms = values.parse_utc_milliseconds(window_start)?;
        let window_end_exclusive_ms = values.parse_utc_milliseconds(window_end_exclusive)?;
        if completed_at_ms <= cutoff_ms
            && window_start_ms <= target_ms
            && window_end_exclusive_ms > earliest_ms
        {
            selected.push(index);
        }
    }
    selected.sort_by(|left, right| {
        compare_captures(captures[*left].borrow(), captures[*right].borrow())
    });
    Ok(selected)
}

/// Normalize selected raw captures once and reject contradictory overlapping endpoints.
fn collect_observations(
    values: &mut impl WorkerRainGaugeReplayValues,
    captures: &[impl Borrow<DatabaseRainAdjustmentCapture>],
    selected: &[usize],
    device_id: u64,
    station_id: u64,
    serial: &str,
    normalized: &mut [Option<Vec<NormalizedTempestRecord>>],
) -> Result<BTreeMap<i64, ReplayInterval>, TaskFailure> {
    let mut observations = BTreeMap::<i64, ReplayInterval>::new();
    // normalize every overlapping response before choosing any favorable value
    for &capture_index in selected {
        if normalized[capture_index].is_none() {
            let capture = captures[capture_index].borrow();
            let (Nullable::Value(window_start), Nullable::Value(window_end_exclusive)) =
                (&capture.windowStart, &capture.windowEndExclusive)
            else {
                return Err(replay_failure());
            };
            let request = TempestObservationRequest {
                device_id,
                end_exclusive_ms: values.parse_utc_milliseconds(window_end_exclusive)?,
                location_id: station_id,
                serial: serial.to_owned(),
                source_id: format!("rain-prospective-tempest-{station_id}"),
                start_ms: values.parse_utc_milliseconds(window_start)?,
                timezone: "America/Los_Angeles".to_owned(),
            };
            let received_at_ms = values.parse_utc_milliseconds(&capture.completedAt)?;
            let envelope = values.decode_tempest(&capture.body)?;
            normalized[capture_index] = Some(
                normalize_tempest_observations(envelope, &request, received_at_ms, 1)?.records,
            );
        }
        let records = normalized[capture_index]
            .as_ref()
            .ok_or_else(replay_failure)?;
        insert_observations(&mut observations, records, capture_index)?;
    }
    Ok(observations)
}

/// Retain the earliest response for each endpoint, refusing contradictory overlaps.
fn insert_observations(
    observations: &mut BTreeMap<i64, ReplayInterval>,
    records: &[NormalizedTempestRecord],
    capture_index: usize,
) -> Result<(), TaskFailure> {
    // retain the earliest immutable response for every exact minute endpoint
    for record in records {
        if record.valid_at_ms % MINUTE_MILLISECONDS != 0 {
            return Err(replay_failure());
        }
        let minute = record.valid_at_ms / MINUTE_MILLISECONDS;
        let duration = record
            .provider
            .report_interval_minutes
            .filter(|value| value.is_finite() && value.fract() == 0.0)
            .map(|value| value as i64)
            .filter(|value| (1..=5).contains(value))
            .ok_or_else(replay_failure)?;
        let flags_accepted = record.quality.flags.as_ref().is_none_or(|flags| {
            flags
                .iter()
                .all(|flag| *flag == TempestQualityFlag::UvIndexOutOfRange)
        });
        let interval = ReplayInterval {
            amount: if flags_accepted {
                record.metrics.precipitation_mm
            } else {
                None
            },
            capture_index,
            minutes: if flags_accepted { duration } else { 0 },
            record_temperature: if flags_accepted {
                record.metrics.temperature_c
            } else {
                None
            },
        };
        // reject contradictory overlapping provider responses before precedence
        if let Some(previous) = observations.get(&minute)
            && (previous.amount != interval.amount
                || previous.minutes != interval.minutes
                || previous.record_temperature != interval.record_temperature)
        {
            return Err(replay_failure());
        }
        observations.entry(minute).or_insert(interval);
    }
    Ok(())
}

/// Exact interval retained only for grouped target archive evidence.
pub(crate) struct GaugeTargetInterval {
    pub valid_at: String,
    pub minutes: u64,
    pub amount: f64,
    pub temperature_c: Option<f64>,
}

/// Normalized gauge values and their exact backward tiling proof.
pub(crate) struct GaugeTargetReplay {
    pub precipitation_mm: Option<f64>,
    pub temperature_c: Option<f64>,
    pub intervals: Vec<GaugeTargetInterval>,
}

/// Replay one admitted target-hour capture without cloning its raw body.
pub(crate) fn replay_fixed_gauge_target_capture(
    values: &mut impl WorkerRainGaugeReplayValues,
    capture: &RainFixedGaugeCapture,
    station: &RainCollectionStation,
    valid_at: &str,
    cutoff: &str,
) -> Result<GaugeTargetReplay, TaskFailure> {
    let valid_at_ms = values.parse_utc_milliseconds(valid_at)?;
    let cutoff_ms = values.parse_utc_milliseconds(cutoff)?;
    let received_at_ms = values.parse_utc_milliseconds(&capture.completed_at)?;
    let start_ms = values.parse_utc_milliseconds(&capture.window_start)?;
    let end_exclusive_ms = values.parse_utc_milliseconds(&capture.window_end_exclusive)?;
    // require exactly the original selected station and backward capture window
    if capture.station_id != station.location_id
        || cutoff_ms < valid_at_ms
        || received_at_ms > cutoff_ms
        || start_ms > valid_at_ms
        || end_exclusive_ms
            <= valid_at_ms
                .checked_sub(65 * MINUTE_MILLISECONDS)
                .ok_or_else(replay_failure)?
    {
        return Err(replay_failure());
    }
    let request = TempestObservationRequest {
        device_id: station.device_id,
        end_exclusive_ms,
        location_id: station.location_id,
        serial: station.serial.to_owned(),
        source_id: format!("rain-prospective-tempest-{}", station.location_id),
        start_ms,
        timezone: "America/Los_Angeles".to_owned(),
    };
    let provider = values.decode_tempest(&capture.body)?;
    let records = normalize_tempest_observations(provider, &request, received_at_ms, 1)?.records;
    let mut observations = BTreeMap::new();
    insert_observations(&mut observations, &records, 0)?;
    let mut endpoints = Vec::new();
    let replay = replay_station_hour(
        |_| &capture.completed_at,
        &observations,
        valid_at_ms,
        Some(&mut endpoints),
    )?;
    // incomplete precipitation has no interval evidence in the original persisted row
    if replay.precipitation_mm.is_none() {
        endpoints.clear();
    }
    let intervals = endpoints
        .into_iter()
        .rev()
        .map(|minute| {
            let interval = observations.get(&minute).ok_or_else(replay_failure)?;
            let epoch_ms = minute
                .checked_mul(MINUTE_MILLISECONDS)
                .ok_or_else(replay_failure)?;
            Ok(GaugeTargetInterval {
                valid_at: values.format_utc_milliseconds(epoch_ms)?,
                minutes: u64::try_from(interval.minutes).map_err(|_| replay_failure())?,
                amount: interval.amount.ok_or_else(replay_failure)?,
                temperature_c: interval.record_temperature,
            })
        })
        .collect::<Result<_, TaskFailure>>()?;
    Ok(GaugeTargetReplay {
        precipitation_mm: replay.precipitation_mm,
        temperature_c: replay.temperature_c,
        intervals,
    })
}

struct StationReplay {
    precipitation_mm: Option<f64>,
    received_at: Option<String>,
    temperature_c: Option<f64>,
}

/// Select the newest endpoint and tile exactly sixty backward minutes.
fn replay_station_hour<'a>(
    completed_at: impl Fn(usize) -> &'a str,
    observations: &BTreeMap<i64, ReplayInterval>,
    valid_at_ms: i64,
    mut retained_endpoints: Option<&mut Vec<i64>>,
) -> Result<StationReplay, TaskFailure> {
    if valid_at_ms % MINUTE_MILLISECONDS != 0 {
        return Err(replay_failure());
    }
    let target = valid_at_ms / MINUTE_MILLISECONDS;
    let mut end = None;
    let mut temperature_c = None;
    let mut received_at: Option<String> = None;
    // preserve the frozen five-minute backward endpoint tolerance
    for offset in 0..=5 {
        let minute = target.checked_sub(offset).ok_or_else(replay_failure)?;
        let interval = observations.get(&minute);
        // select the newest available precipitation endpoint once
        if end.is_none() && interval.is_some() {
            end = Some(minute);
        }
        // select the newest available endpoint temperature independently
        if temperature_c.is_none()
            && let Some(interval) = interval
            && let Some(record_temperature) = interval.record_temperature
        {
            temperature_c = Some(record_temperature);
            received_at = Some(completed_at(interval.capture_index).to_owned());
        }
    }
    let mut precipitation_mm = end.map(|_| 0.0);
    let mut wanted = end.unwrap_or(target);
    let start = wanted.checked_sub(60).ok_or_else(replay_failure)?;
    // tile complete exact intervals without interpolation, proration, or zero filling
    while precipitation_mm.is_some() && wanted > start {
        let Some(interval) = observations.get(&wanted) else {
            precipitation_mm = None;
            break;
        };
        let interval_start = wanted
            .checked_sub(interval.minutes)
            .ok_or_else(replay_failure)?;
        let Some(amount) = interval.amount else {
            precipitation_mm = None;
            break;
        };
        // reject one interval crossing the exact hour boundary
        if interval.minutes <= 0 || interval_start < start {
            precipitation_mm = None;
            break;
        }
        precipitation_mm = precipitation_mm.map(|total| total + amount);
        let completed_at = completed_at(interval.capture_index);
        // retain the latest receipt clock across temperature and precipitation members
        if received_at
            .as_ref()
            .is_none_or(|received| completed_at > received.as_str())
        {
            received_at = Some(completed_at.to_owned());
        }
        // inference skips this allocation while archive production retains exact endpoints
        if let Some(endpoints) = &mut retained_endpoints {
            endpoints.push(wanted);
        }
        wanted = interval_start;
    }
    Ok(StationReplay {
        precipitation_mm,
        received_at,
        temperature_c,
    })
}

/// Compare capture revisions before provider values are inspected.
fn compare_captures(
    left: &DatabaseRainAdjustmentCapture,
    right: &DatabaseRainAdjustmentCapture,
) -> Ordering {
    left.completedAt
        .cmp(&right.completedAt)
        .then_with(|| left.claimId.cmp(&right.claimId))
        .then_with(|| left.bodySha256.cmp(&right.bodySha256))
}

/// Convert one optional value into the generated nullable representation.
fn nullable(value: Option<f64>) -> Nullable<f64> {
    value.map_or(Nullable::Null, Nullable::Value)
}

/// Produce one stable causal-replay failure without provider attempts.
fn replay_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "rain fixed-gauge replay differs".to_owned(),
    }
}

#[cfg(test)]
#[path = "worker_rain_gauge_replay_test.rs"]
mod tests;
