use weather_contracts::{Nullable, RainAdjustmentCapture, RainAdjustmentCaptureKindEnum};
use weather_worker::{
    RAIN_COLLECTION_STATIONS, RainFixedGaugeCapture, TempestObservation,
    TempestObservationEnvelope, TempestResponseType,
};

use super::replay_fixed_gauge_target_capture;
use crate::{TaskFailure, WorkerRainGaugeReplayValues, replay_worker_rain_station_hours};

const DECISION_MS: i64 = 1_789_344_000_000;
const WINDOW_START_MS: i64 = DECISION_MS - 3 * 3_600_000 + 1_000;
const WINDOW_END_MS: i64 = DECISION_MS - 3_600_000 + 1_000;
const EARLY_COMPLETION_MS: i64 = DECISION_MS - 57 * 60_000;
const LATE_COMPLETION_MS: i64 = DECISION_MS - 50 * 60_000;

#[derive(Default)]
struct FixtureValues {
    decode_count: usize,
}

impl WorkerRainGaugeReplayValues for FixtureValues {
    // Decode the two fixture markers into trusted typed Tempest envelopes.
    fn decode_tempest(&mut self, bytes: &[u8]) -> Result<TempestObservationEnvelope, TaskFailure> {
        self.decode_count += 1;
        Ok(fixture_envelope(bytes == b"missing"))
    }

    // Admit only the canonical clocks used by this causal fixture.
    fn parse_utc_milliseconds(&mut self, value: &str) -> Result<i64, TaskFailure> {
        match value {
            "2026-09-14T05:00:01.000Z" => Ok(WINDOW_START_MS),
            "2026-09-14T07:00:01.000Z" => Ok(WINDOW_END_MS),
            "2026-09-14T07:00:00.000Z" => Ok(DECISION_MS - 3_600_000),
            "2026-09-14T07:03:00.000Z" => Ok(EARLY_COMPLETION_MS),
            "2026-09-14T07:10:00.000Z" => Ok(LATE_COMPLETION_MS),
            _ => Err(failure()),
        }
    }

    // Format the exact decision and lag clocks without introducing a calendar implementation.
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        // format only the fixed-gauge fixture's same-day minute endpoints from 05:00 through 07:00
        let minutes = 8 * 60 - (DECISION_MS - epoch_ms) / 60_000;
        if epoch_ms % 60_000 == 0 && (5 * 60..=7 * 60).contains(&minutes) {
            return Ok(format!(
                "2026-09-14T{:02}:{:02}:00.000Z",
                minutes / 60,
                minutes % 60
            ));
        }
        let value = match DECISION_MS - epoch_ms {
            0 => "2026-09-14T08:00:00.000Z",
            3_600_000 => "2026-09-14T07:00:00.000Z",
            7_200_000 => "2026-09-14T06:00:00.000Z",
            10_800_000 => "2026-09-14T05:00:00.000Z",
            21_600_000 => "2026-09-14T02:00:00.000Z",
            43_200_000 => "2026-09-13T20:00:00.000Z",
            86_400_000 => "2026-09-13T08:00:00.000Z",
            _ => return Err(failure()),
        };
        Ok(value.to_owned())
    }
}

// Build one admitted station capture over the shared two-hour provider window.
fn capture(claim_id: &str, completed_at: &str, marker: &[u8]) -> RainAdjustmentCapture {
    RainAdjustmentCapture {
        claimId: claim_id.to_owned(),
        kind: RainAdjustmentCaptureKindEnum::Station,
        stationId: Nullable::Value(64_255),
        runInitializedAt: Nullable::Null,
        windowStart: Nullable::Value("2026-09-14T05:00:01.000Z".to_owned()),
        windowEndExclusive: Nullable::Value("2026-09-14T07:00:01.000Z".to_owned()),
        completedAt: completed_at.to_owned(),
        bodySha256: format!("{:064x}", marker[0]),
        body: marker.to_vec(),
    }
}

// Build the real typed provider envelope with one minute per exact endpoint.
fn fixture_envelope(missing_middle: bool) -> TempestObservationEnvelope {
    let mut observations = Vec::new();
    // Cover 05:01 through 07:00 so both adjacent lag hours reuse one normalization.
    for minute in 1_i64..=120 {
        // Remove a single interior interval to prove that replay never zero-fills a gap.
        if missing_middle && minute == 90 {
            continue;
        }
        observations.push(fixture_observation(
            (DECISION_MS - 3 * 3_600_000 + minute * 60_000) / 1_000,
        ));
    }
    TempestObservationEnvelope {
        bucket_step_minutes: Some(1.0),
        device_id: 175_727,
        observations: Some(observations),
        response_type: TempestResponseType::StationObservation,
        status_code: 0,
    }
}

// Build one physically valid minute row used by the production Tempest normalizer.
fn fixture_observation(epoch_seconds: i64) -> TempestObservation {
    TempestObservation {
        air_temperature_c: Some(12.0),
        battery_volts: Some(2.6),
        epoch_seconds,
        illuminance_lux: Some(100.0),
        lightning_average_distance_km: None,
        lightning_strike_count: Some(0.0),
        local_daily_rain_accumulation_final_mm: None,
        local_daily_rain_accumulation_mm: None,
        precipitation_analysis_type: None,
        precipitation_mm: Some(0.01),
        precipitation_type: Some(0.0),
        pressure_hpa: Some(1_012.0),
        rain_accumulation_nc_mm: Some(0.01),
        relative_humidity_percent: Some(80.0),
        report_interval_minutes: Some(1.0),
        solar_radiation_wm2: Some(50.0),
        uv_index: Some(1.0),
        wind_direction_degrees: Some(360.0),
        wind_gust_mps: Some(3.0),
        wind_lull_mps: Some(1.0),
        wind_sample_interval_seconds: Some(3.0),
        wind_speed_mps: Some(2.0),
    }
}

// Produce one stable local fixture failure.
fn failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "fixture clock differs".to_owned(),
    }
}

#[test]
fn replay_deduplicates_overlap_and_retains_earliest_receipt() {
    let captures = vec![
        capture("claim-b", "2026-09-14T07:10:00.000Z", b"complete"),
        capture("claim-a", "2026-09-14T07:03:00.000Z", b"complete"),
    ];
    let mut values = FixtureValues::default();

    let hours = replay_worker_rain_station_hours(&mut values, &captures, DECISION_MS)
        .expect("overlapping immutable captures should replay");
    let target = hours
        .iter()
        .find(|hour| hour.hour_at_ms == DECISION_MS - 3_600_000)
        .expect("the newest complete lag hour should be present");

    assert_eq!(hours.len(), 2);
    assert_eq!(values.decode_count, 2);
    assert_eq!(target.row.receivedAt, "2026-09-14T07:03:00.000Z");
    assert_eq!(target.row.temperatureC, Nullable::Value(12.0));
    let Nullable::Value(precipitation_mm) = target.row.precipitationMm else {
        panic!("a complete sixty-minute tiling should retain precipitation");
    };
    assert!((precipitation_mm - 0.6).abs() < 1e-12);
}

#[test]
fn replay_preserves_temperature_and_receipt_when_tiling_has_a_gap() {
    let captures = vec![capture("claim-a", "2026-09-14T07:03:00.000Z", b"missing")];
    let mut values = FixtureValues::default();

    let hours = replay_worker_rain_station_hours(&mut values, &captures, DECISION_MS)
        .expect("one missing minute is an admitted nullable replay");
    let target = hours
        .iter()
        .find(|hour| hour.hour_at_ms == DECISION_MS - 3_600_000)
        .expect("the endpoint temperature should retain the station hour");

    assert_eq!(target.row.precipitationMm, Nullable::Null);
    assert_eq!(target.row.temperatureC, Nullable::Value(12.0));
    assert_eq!(target.row.receivedAt, "2026-09-14T07:03:00.000Z");
}

#[test]
fn fixed_gauge_replay_retains_all_endpoints_oldest_to_newest() {
    let capture = fixed_gauge_capture(b"complete");
    let mut values = FixtureValues::default();

    let replay = replay_fixed_gauge_target_capture(
        &mut values,
        &capture,
        &RAIN_COLLECTION_STATIONS[0],
        "2026-09-14T07:00:00.000Z",
        "2026-09-14T07:03:00.000Z",
    )
    .expect("complete two-hour capture should replay its target hour");

    assert!(
        replay
            .precipitation_mm
            .is_some_and(|amount| (amount - 0.6).abs() < 1e-12)
    );
    assert_eq!(replay.temperature_c, Some(12.0));
    assert_eq!(replay.intervals.len(), 60);
    assert_eq!(replay.intervals[0].valid_at, "2026-09-14T06:01:00.000Z");
    assert_eq!(replay.intervals[59].valid_at, "2026-09-14T07:00:00.000Z");
    assert!(replay.intervals.iter().all(|interval| {
        interval.minutes == 1 && interval.amount == 0.01 && interval.temperature_c == Some(12.0)
    }));
}

#[test]
fn fixed_gauge_incomplete_replay_keeps_temperature_without_interval_evidence() {
    let capture = fixed_gauge_capture(b"missing");
    let mut values = FixtureValues::default();

    let replay = replay_fixed_gauge_target_capture(
        &mut values,
        &capture,
        &RAIN_COLLECTION_STATIONS[0],
        "2026-09-14T07:00:00.000Z",
        "2026-09-14T07:03:00.000Z",
    )
    .expect("one missing minute remains an admitted nullable target");

    assert_eq!(replay.precipitation_mm, None);
    assert_eq!(replay.temperature_c, Some(12.0));
    assert!(replay.intervals.is_empty());
}

// Build the worker's original fixed-gauge capture carrier over the shared fixture body.
fn fixed_gauge_capture(marker: &[u8]) -> RainFixedGaugeCapture {
    RainFixedGaugeCapture {
        body: marker.to_vec(),
        body_sha256: format!("{:064x}", marker[0]),
        claim_id: "claim-a".to_owned(),
        completed_at: "2026-09-14T07:03:00.000Z".to_owned(),
        station_id: RAIN_COLLECTION_STATIONS[0].location_id,
        window_end_exclusive: "2026-09-14T07:00:01.000Z".to_owned(),
        window_start: "2026-09-14T05:00:01.000Z".to_owned(),
    }
}
