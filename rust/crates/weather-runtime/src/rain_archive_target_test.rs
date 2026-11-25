use weather_contracts::{Nullable, RainAdjustmentCapture, RainAdjustmentCaptureKindEnum};
use weather_worker::{
    RAIN_COLLECTION_STATIONS, TaskFailure, TempestObservation, TempestObservationEnvelope,
    TempestResponseType,
};

use super::{
    RainArchiveTargetIdentityValues, RainArchiveTargetPreimage, replay_rain_archive_target,
};
use crate::WorkerRainGaugeReplayValues;

const TARGET_MS: i64 = 1_789_340_400_000;
const DECISION_MS: i64 = TARGET_MS + 3_600_000;
const WINDOW_START_MS: i64 = TARGET_MS - 3_600_000;
const WINDOW_END_MS: i64 = TARGET_MS + 1_000;
const COMPLETED_MS: i64 = TARGET_MS + 3 * 60_000;
const TARGET_AT: &str = "2026-09-14T07:00:00.000Z";
const TARGET_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct FixtureValues {
    decode_count: usize,
    identity_calls: usize,
    preimage: Option<RainArchiveTargetPreimage>,
}

impl WorkerRainGaugeReplayValues for FixtureValues {
    /// Decode one body marker into its station-specific complete or gapped hour.
    fn decode_tempest(&mut self, bytes: &[u8]) -> Result<TempestObservationEnvelope, TaskFailure> {
        self.decode_count += 1;
        let index = usize::from(*bytes.first().ok_or_else(failure)?);
        fixture_envelope(index).ok_or_else(failure)
    }

    /// Admit only the exact clocks used by this target fixture.
    fn parse_utc_milliseconds(&mut self, value: &str) -> Result<i64, TaskFailure> {
        match value {
            "2026-09-14T06:00:00.000Z" => Ok(WINDOW_START_MS),
            "2026-09-14T07:00:00.000Z" => Ok(TARGET_MS),
            "2026-09-14T07:00:01.000Z" => Ok(WINDOW_END_MS),
            "2026-09-14T07:03:00.000Z" => Ok(COMPLETED_MS),
            "2026-09-14T08:00:00.000Z" => Ok(DECISION_MS),
            _ => Err(failure()),
        }
    }

    /// Format the original target and its one-hour-later decision.
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        match epoch_ms {
            TARGET_MS => Ok(TARGET_AT.to_owned()),
            DECISION_MS => Ok("2026-09-14T08:00:00.000Z".to_owned()),
            _ => Err(failure()),
        }
    }
}

impl RainArchiveTargetIdentityValues for FixtureValues {
    /// Capture the typed preimage and return one approved fixture digest.
    fn rain_target_identity_sha256(
        &mut self,
        preimage: &RainArchiveTargetPreimage,
    ) -> Option<String> {
        self.identity_calls += 1;
        self.preimage = Some(preimage.clone());
        Some(TARGET_HASH.to_owned())
    }
}

/// Match the compiled TypeScript weighted median while retaining a null station row.
#[test]
fn archive_target_replays_one_hour_and_hashes_sorted_original_rows() {
    let captures = (0..=4).map(capture).collect::<Vec<_>>();
    let mut values = FixtureValues::default();

    let target = replay_rain_archive_target(&mut values, &captures, TARGET_AT)
        .expect("complete fixed-gauge coverage should replay");

    assert_eq!(target.target, Nullable::Value(2.0));
    assert_eq!(target.target_key, TARGET_HASH);
    assert_eq!(values.decode_count, 5);
    assert_eq!(values.identity_calls, 1);
    let preimage = values
        .preimage
        .expect("a complete target should reach the canonical owner");
    assert_eq!(preimage.valid_at, TARGET_AT);
    assert_eq!(
        preimage
            .rows
            .iter()
            .map(|row| row.station_id)
            .collect::<Vec<_>>(),
        vec![38_270, 64_255, 126_537, 168_853, 225_947],
    );
    let null_row = preimage
        .rows
        .iter()
        .find(|row| row.station_id == 126_537)
        .expect("the temperature-only station row should remain in the identity");
    assert_eq!(null_row.precipitation_mm, Nullable::Null);
    assert_eq!(null_row.received_at, "2026-09-14T07:03:00.000Z");
}

/// Preserve the original missing key without invoking canonical identity work.
#[test]
fn archive_target_keeps_incomplete_coverage_missing() {
    let captures = (0..=1).map(capture).collect::<Vec<_>>();
    let mut values = FixtureValues::default();

    let target = replay_rain_archive_target(&mut values, &captures, TARGET_AT)
        .expect("two admitted gauges should produce an incomplete target");

    assert_eq!(target.target, Nullable::Null);
    assert_eq!(target.target_key, format!("missing:{TARGET_AT}"));
    assert_eq!(values.decode_count, 2);
    assert_eq!(values.identity_calls, 0);
    assert_eq!(values.preimage, None);
}

/// Build one station capture over the exact preceding-hour provider window.
fn capture(index: usize) -> RainAdjustmentCapture {
    let station = &RAIN_COLLECTION_STATIONS[index];
    RainAdjustmentCapture {
        body: vec![u8::try_from(index).expect("fixture station index should fit")],
        bodySha256: format!("{:064x}", index + 1),
        claimId: format!("station-{}", station.location_id),
        completedAt: "2026-09-14T07:03:00.000Z".to_owned(),
        kind: RainAdjustmentCaptureKindEnum::Station,
        runInitializedAt: Nullable::Null,
        stationId: Nullable::Value(station.location_id),
        windowEndExclusive: Nullable::Value("2026-09-14T07:00:01.000Z".to_owned()),
        windowStart: Nullable::Value("2026-09-14T06:00:00.000Z".to_owned()),
    }
}

/// Build sixty exact minute endpoints with one optional interior gap.
fn fixture_envelope(index: usize) -> Option<TempestObservationEnvelope> {
    let station = RAIN_COLLECTION_STATIONS.get(index)?;
    let total = [0.0, 2.0, 9.0, 10.0, 1.0].get(index).copied()?;
    let mut observations = Vec::new();
    // retain a complete hour except for the temperature-only fifth gauge
    for minute in 1_i64..=60 {
        // create one exact tiling gap without removing the newest temperature
        if index == 4 && minute == 30 {
            continue;
        }
        observations.push(fixture_observation(
            (WINDOW_START_MS + minute * 60_000) / 1_000,
            if minute == 60 { total } else { 0.0 },
        ));
    }
    Some(TempestObservationEnvelope {
        bucket_step_minutes: Some(1.0),
        device_id: station.device_id,
        observations: Some(observations),
        response_type: TempestResponseType::StationObservation,
        status_code: 0,
    })
}

/// Build one physically valid provider minute used by the production normalizer.
fn fixture_observation(epoch_seconds: i64, precipitation_mm: f64) -> TempestObservation {
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
        precipitation_mm: Some(precipitation_mm),
        precipitation_type: Some(0.0),
        pressure_hpa: Some(1_012.0),
        rain_accumulation_nc_mm: Some(precipitation_mm),
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

/// Produce one stable local fixture failure.
fn failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "archive target fixture differs".to_owned(),
    }
}
