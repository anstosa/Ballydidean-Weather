//! Typed Tempest observation normalization without transport or JSON dependencies.

use std::collections::BTreeSet;

use super::{
    CanonicalWeatherMetrics, ScheduledProviderRecord, TaskFailure, invalid_provider_payload,
    normalize_metric,
};

pub const TEMPEST_OBSERVATION_ADAPTER_VERSION: &str = "tempest-observations-minute/v1";
pub const TEMPEST_OBSERVATION_CHUNK_PLAN_VERSION: &str = "tempest-observations-one-day-minute/v1";
pub const TEMPEST_MAXIMUM_RANGE_SECONDS: u64 = 24 * 60 * 60;
pub const TEMPEST_OBSERVATION_ENDPOINT: &str = "observations/device";
pub const TEMPEST_SOURCE_CONTRACT_VERSION: &str = "tempest-observations/v2";

const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAXIMUM_PROVIDER_NUMBER: f64 = 1_000_000_000.0;
const MILLISECONDS_PER_SECOND: i64 = 1_000;

/// One transport-parsed Tempest request with resolved UTC timestamps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TempestObservationRequest {
    pub device_id: u64,
    pub end_exclusive_ms: i64,
    pub location_id: u64,
    pub serial: String,
    pub source_id: String,
    pub start_ms: i64,
    pub timezone: String,
}

/// One nullable Tempest durable cursor expressed as the final valid instant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TempestProviderCursor {
    pub valid_at_ms: i64,
}

/// One scheduled Tempest database identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TempestScheduledSource {
    pub active: bool,
    pub cadence_seconds: u64,
    pub id: String,
    pub material_config_identity: String,
    pub provider_key: String,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub source_kind: TempestSourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// One loaded Tempest station configuration identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TempestRuntimeConfiguration {
    pub adapter_contract_version: &'static str,
    pub device_id: u64,
    pub location_id: u64,
    pub material_config_identity: String,
    pub provider_key: String,
    pub serial: String,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub source_kind: TempestSourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// Inputs for one admitted scheduled Tempest execution.
#[derive(Clone, Copy, Debug)]
pub struct TempestScheduledExecution<'a> {
    pub configuration: &'a TempestRuntimeConfiguration,
    pub now_ms: i64,
    pub run_deadline_at_ms: i64,
    pub source: &'a TempestScheduledSource,
}

/// One typed Tempest response discriminator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TempestResponseType {
    Other,
    StationObservation,
}

/// One transport-parsed Tempest observation response.
#[derive(Clone, Debug, PartialEq)]
pub struct TempestObservationEnvelope {
    pub bucket_step_minutes: Option<f64>,
    pub device_id: u64,
    pub observations: Option<Vec<TempestObservation>>,
    pub response_type: TempestResponseType,
    pub status_code: i64,
}

/// One typed row from Tempest's twenty-two-column obs_st contract.
#[derive(Clone, Debug, PartialEq)]
pub struct TempestObservation {
    pub air_temperature_c: Option<f64>,
    pub battery_volts: Option<f64>,
    pub epoch_seconds: i64,
    pub illuminance_lux: Option<f64>,
    pub lightning_average_distance_km: Option<f64>,
    pub lightning_strike_count: Option<f64>,
    pub local_daily_rain_accumulation_final_mm: Option<f64>,
    pub local_daily_rain_accumulation_mm: Option<f64>,
    pub precipitation_analysis_type: Option<f64>,
    pub precipitation_mm: Option<f64>,
    pub precipitation_type: Option<f64>,
    pub pressure_hpa: Option<f64>,
    pub rain_accumulation_nc_mm: Option<f64>,
    pub relative_humidity_percent: Option<f64>,
    pub report_interval_minutes: Option<f64>,
    pub solar_radiation_wm2: Option<f64>,
    pub uv_index: Option<f64>,
    pub wind_direction_degrees: Option<f64>,
    pub wind_gust_mps: Option<f64>,
    pub wind_lull_mps: Option<f64>,
    pub wind_sample_interval_seconds: Option<f64>,
    pub wind_speed_mps: Option<f64>,
}

/// Tempest's physical-sensor source identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TempestSourceKind {
    PhysicalSensor,
}

/// Stable device provenance retained on every Tempest record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TempestDeviceMetadata {
    pub model: &'static str,
    pub serial: String,
    pub vendor: &'static str,
}

/// Bounded provider metadata retained from one Tempest observation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempestProviderMetadata {
    pub battery_volts: Option<f64>,
    pub device_id: u64,
    pub illuminance_lux: Option<f64>,
    pub lightning_average_distance_km: Option<f64>,
    pub lightning_strike_count: Option<f64>,
    pub location_id: u64,
    pub precipitation_type: Option<f64>,
    pub rain_accumulation_nc_mm: Option<f64>,
    pub report_interval_minutes: Option<f64>,
    pub wind_lull_mps: Option<f64>,
    pub wind_sample_interval_seconds: Option<f64>,
}

/// One bounded quality flag emitted by Tempest normalization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TempestQualityFlag {
    UvIndexOutOfRange,
}

/// Stable sampling and discarded-metric evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TempestQualityMetadata {
    pub flags: Option<Vec<TempestQualityFlag>>,
    pub sampling: &'static str,
}

/// One normalized physical-sensor record from Tempest.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedTempestRecord {
    pub device: TempestDeviceMetadata,
    pub metrics: CanonicalWeatherMetrics,
    pub model: Option<String>,
    pub product_run_at_ms: Option<i64>,
    pub provider: TempestProviderMetadata,
    pub quality: TempestQualityMetadata,
    pub received_at_ms: i64,
    pub source_id: String,
    pub source_kind: TempestSourceKind,
    pub upstream_timezone: String,
    pub valid_at_ms: i64,
}

/// Bounded response diagnostics emitted with one Tempest batch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempestResponseMetadata {
    pub bucket_step_minutes: Option<f64>,
    pub device_id: u64,
    pub minute_record_count: usize,
    pub raw_observation_count: usize,
}

/// One normalized Tempest provider batch.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedTempestBatch {
    pub adapter_version: &'static str,
    pub attempts: u32,
    pub provider_cursor_valid_at_ms: Option<i64>,
    pub records: Vec<NormalizedTempestRecord>,
    pub response_metadata: TempestResponseMetadata,
}

impl ScheduledProviderRecord for NormalizedTempestRecord {
    // expose the normalized provider timestamp
    fn valid_at_ms(&self) -> i64 {
        self.valid_at_ms
    }
}

/// Normalize every first distinct in-window Tempest obs_st row.
pub fn normalize_tempest_observations(
    envelope: TempestObservationEnvelope,
    request: &TempestObservationRequest,
    received_at_ms: i64,
    attempts: u32,
) -> Result<NormalizedTempestBatch, TaskFailure> {
    let window = validate_tempest_request(request)?;

    // preserve provider-level rejection identity
    if envelope.status_code != 0 {
        return Err(failure_with_code("provider_request_rejected"));
    }

    // require the requested station response
    if envelope.response_type != TempestResponseType::StationObservation
        || envelope.device_id != request.device_id
    {
        return Err(invalid_provider_payload());
    }

    // retain the optional JSON response diagnostic
    if envelope
        .bucket_step_minutes
        .is_some_and(|value| !value.is_finite())
    {
        return Err(invalid_provider_payload());
    }
    let bucket_step_minutes = envelope.bucket_step_minutes;
    let observations = envelope.observations.as_deref().unwrap_or(&[]);
    let raw_observation_count = observations.len();
    let mut distinct_epochs = BTreeSet::new();
    let mut records = Vec::with_capacity(raw_observation_count);

    // retain the first report at every in-window provider timestamp
    for observation in observations {
        // require the provider's positive integer timestamp
        if observation.epoch_seconds < 1
            || u64::try_from(observation.epoch_seconds)
                .map_or(true, |value| value > MAXIMUM_SAFE_INTEGER)
        {
            return Err(invalid_provider_payload());
        }

        // ignore the provider's out-of-window rows
        if observation.epoch_seconds < window.start_epoch_seconds
            || observation.epoch_seconds > window.end_epoch_seconds_inclusive
        {
            continue;
        }

        // preserve the first exact duplicate
        if distinct_epochs.insert(observation.epoch_seconds) {
            records.push(normalize_tempest_observation(
                observation,
                request,
                received_at_ms,
            )?);
        }
    }

    let provider_cursor_valid_at_ms = records.last().map(|record| record.valid_at_ms);
    Ok(NormalizedTempestBatch {
        adapter_version: TEMPEST_OBSERVATION_ADAPTER_VERSION,
        attempts,
        provider_cursor_valid_at_ms,
        response_metadata: TempestResponseMetadata {
            bucket_step_minutes,
            device_id: envelope.device_id,
            minute_record_count: records.len(),
            raw_observation_count,
        },
        records,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TempestWindow {
    end_epoch_seconds_inclusive: i64,
    start_epoch_seconds: i64,
}

// validate one resolved half-open request window
fn validate_tempest_request(
    request: &TempestObservationRequest,
) -> Result<TempestWindow, TaskFailure> {
    // require JavaScript-compatible positive identifiers
    if request.device_id == 0
        || request.device_id > MAXIMUM_SAFE_INTEGER
        || request.location_id == 0
        || request.location_id > MAXIMUM_SAFE_INTEGER
    {
        return Err(invalid_provider_payload());
    }

    // require bounded parsed identity fields
    if !bounded_text(&request.serial, 128)
        || !bounded_text(&request.source_id, 128)
        || !bounded_text(&request.timezone, 64)
    {
        return Err(invalid_provider_payload());
    }

    let Some(duration_ms) = request.end_exclusive_ms.checked_sub(request.start_ms) else {
        return Err(invalid_provider_payload());
    };
    let maximum_range_ms = i64::try_from(TEMPEST_MAXIMUM_RANGE_SECONDS)
        .ok()
        .and_then(|seconds| seconds.checked_mul(MILLISECONDS_PER_SECOND))
        .ok_or_else(invalid_provider_payload)?;

    // require exact second-aligned resolved timestamps
    if request.start_ms % MILLISECONDS_PER_SECOND != 0
        || request.end_exclusive_ms % MILLISECONDS_PER_SECOND != 0
        || duration_ms <= 0
        || duration_ms > maximum_range_ms
    {
        return Err(invalid_provider_payload());
    }

    Ok(TempestWindow {
        end_epoch_seconds_inclusive: request.end_exclusive_ms / MILLISECONDS_PER_SECOND - 1,
        start_epoch_seconds: request.start_ms / MILLISECONDS_PER_SECOND,
    })
}

// normalize one typed obs_st row
fn normalize_tempest_observation(
    observation: &TempestObservation,
    request: &TempestObservationRequest,
    received_at_ms: i64,
) -> Result<NormalizedTempestRecord, TaskFailure> {
    let report_interval_minutes = provider_number(observation.report_interval_minutes)?;

    // reject unusable precipitation denominators
    if report_interval_minutes.is_some_and(|interval| interval <= 0.0) {
        return Err(invalid_provider_payload());
    }

    let precipitation_mm = normalize_metric(observation.precipitation_mm, 0.0, 2_000.0, false)?;
    let precipitation_rate_mm_per_hour = match (precipitation_mm, report_interval_minutes) {
        (Some(precipitation), Some(interval)) => normalize_metric(
            Some(precipitation * (60.0 / interval)),
            0.0,
            10_000.0,
            false,
        )?,
        _ => None,
    };
    let wind_direction_degrees = match provider_number(observation.wind_direction_degrees)? {
        Some(360.0) => Some(0.0),
        value => normalize_metric(value, 0.0, 360.0, true)?,
    };
    let (uv_index, uv_index_out_of_range) = normalize_tempest_uv(observation.uv_index)?;
    let valid_at_ms = observation
        .epoch_seconds
        .checked_mul(MILLISECONDS_PER_SECOND)
        .ok_or_else(invalid_provider_payload)?;

    Ok(NormalizedTempestRecord {
        device: TempestDeviceMetadata {
            model: "Tempest",
            serial: request.serial.clone(),
            vendor: "WeatherFlow",
        },
        metrics: CanonicalWeatherMetrics {
            apparent_temperature_c: None,
            black_globe_temperature_c: None,
            cloud_cover_percent: None,
            pm25_micrograms_per_cubic_meter: None,
            precipitation_mm,
            precipitation_rate_mm_per_hour,
            pressure_hpa: normalize_metric(observation.pressure_hpa, 100.0, 1_200.0, false)?,
            relative_humidity_percent: normalize_metric(
                observation.relative_humidity_percent,
                0.0,
                100.0,
                false,
            )?,
            soil_electrical_conductivity_microsiemens_per_cm: None,
            soil_moisture_percent: None,
            solar_radiation_wm2: normalize_metric(
                observation.solar_radiation_wm2,
                0.0,
                2_500.0,
                false,
            )?,
            temperature_c: normalize_metric(observation.air_temperature_c, -100.0, 70.0, false)?,
            uv_index,
            water_level_m: None,
            wind_direction_degrees,
            wind_gust_mps: normalize_metric(observation.wind_gust_mps, 0.0, 150.0, false)?,
            wind_speed_mps: normalize_metric(observation.wind_speed_mps, 0.0, 150.0, false)?,
            wet_bulb_globe_temperature_c: None,
        },
        model: None,
        product_run_at_ms: None,
        provider: TempestProviderMetadata {
            battery_volts: provider_number(observation.battery_volts)?,
            device_id: request.device_id,
            illuminance_lux: provider_number(observation.illuminance_lux)?,
            lightning_average_distance_km: provider_number(
                observation.lightning_average_distance_km,
            )?,
            lightning_strike_count: provider_number(observation.lightning_strike_count)?,
            location_id: request.location_id,
            precipitation_type: provider_number(observation.precipitation_type)?,
            rain_accumulation_nc_mm: provider_number(observation.rain_accumulation_nc_mm)?,
            report_interval_minutes,
            wind_lull_mps: provider_number(observation.wind_lull_mps)?,
            wind_sample_interval_seconds: provider_number(
                observation.wind_sample_interval_seconds,
            )?,
        },
        quality: TempestQualityMetadata {
            flags: uv_index_out_of_range.then(|| vec![TempestQualityFlag::UvIndexOutOfRange]),
            sampling: "every_distinct_provider_observation",
        },
        received_at_ms,
        source_id: request.source_id.clone(),
        source_kind: TempestSourceKind::PhysicalSensor,
        upstream_timezone: request.timezone.clone(),
        valid_at_ms,
    })
}

// normalize one nullable Tempest ultraviolet reading
fn normalize_tempest_uv(value: Option<f64>) -> Result<(Option<f64>, bool), TaskFailure> {
    let value = provider_number(value)?;
    let Some(value) = value else {
        return Ok((None, false));
    };

    // discard only impossible ultraviolet readings
    if !(0.0..=20.0).contains(&value) {
        return Ok((None, true));
    }

    Ok((Some(value), false))
}

// require one JSON-compatible provider number
fn provider_number(value: Option<f64>) -> Result<Option<f64>, TaskFailure> {
    // preserve provider nulls
    let Some(value) = value else {
        return Ok(None);
    };

    // match the bounded Tempest response parser
    if !value.is_finite() || !(-MAXIMUM_PROVIDER_NUMBER..=MAXIMUM_PROVIDER_NUMBER).contains(&value)
    {
        return Err(invalid_provider_payload());
    }

    Ok(Some(value))
}

// validate one already-parsed identity string
fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum
}

// create one bounded provider failure
fn failure_with_code(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TEMPEST_MAXIMUM_RANGE_SECONDS, TEMPEST_OBSERVATION_ADAPTER_VERSION, TempestObservation,
        TempestObservationEnvelope, TempestObservationRequest, TempestQualityFlag,
        TempestResponseType, TempestSourceKind, normalize_tempest_observations,
    };

    // build the request used by the compiled node fixture
    fn fixture_request() -> TempestObservationRequest {
        TempestObservationRequest {
            device_id: 470_937,
            end_exclusive_ms: 1_787_191_200_000,
            location_id: 203_055,
            serial: "ST-00198967".to_owned(),
            source_id: "source-tempest".to_owned(),
            start_ms: 1_787_184_000_000,
            timezone: "America/Los_Angeles".to_owned(),
        }
    }

    // build one typed row from the checked node fixture
    fn fixture_observation(epoch_seconds: i64, values: [f64; 21]) -> TempestObservation {
        TempestObservation {
            air_temperature_c: Some(values[6]),
            battery_volts: Some(values[15]),
            epoch_seconds,
            illuminance_lux: Some(values[8]),
            lightning_average_distance_km: Some(values[13]),
            lightning_strike_count: Some(values[14]),
            local_daily_rain_accumulation_final_mm: Some(values[19]),
            local_daily_rain_accumulation_mm: Some(values[17]),
            precipitation_analysis_type: Some(values[20]),
            precipitation_mm: Some(values[11]),
            precipitation_type: Some(values[12]),
            pressure_hpa: Some(values[5]),
            rain_accumulation_nc_mm: Some(values[18]),
            relative_humidity_percent: Some(values[7]),
            report_interval_minutes: Some(values[16]),
            solar_radiation_wm2: Some(values[10]),
            uv_index: Some(values[9]),
            wind_direction_degrees: Some(values[3]),
            wind_gust_mps: Some(values[2]),
            wind_lull_mps: Some(values[0]),
            wind_sample_interval_seconds: Some(values[4]),
            wind_speed_mps: Some(values[1]),
        }
    }

    // build the three obs_st rows from observations.json
    fn fixture_envelope() -> TempestObservationEnvelope {
        TempestObservationEnvelope {
            bucket_step_minutes: Some(1.0),
            device_id: 470_937,
            observations: Some(vec![
                fixture_observation(
                    1_787_184_000,
                    [
                        0.3, 1.1, 2.1, 360.0, 3.0, 1_009.6, 24.5, 60.0, 23_157.0, 1.54, 193.0, 0.2,
                        1.0, 12.0, 2.0, 2.77, 1.0, 0.2, 0.18, 0.18, 1.0,
                    ],
                ),
                fixture_observation(
                    1_787_184_060,
                    [
                        0.7, 2.1, 2.9, 137.0, 3.0, 1_009.6, 24.4, 59.0, 14_418.0, 0.95, 120.0, 0.0,
                        0.0, 0.0, 0.0, 2.77, 1.0, 0.2, 0.0, 0.0, 0.0,
                    ],
                ),
                fixture_observation(
                    1_787_187_600,
                    [
                        0.4, 1.2, 2.2, 141.0, 3.0, 1_009.4, 24.1, 61.0, 12_000.0, 0.8, 100.0, 0.1,
                        1.0, 15.0, 1.0, 2.76, 1.0, 0.3, 0.09, 0.27, 1.0,
                    ],
                ),
            ]),
            response_type: TempestResponseType::StationObservation,
            status_code: 0,
        }
    }

    // match the compiled node observations fixture
    #[test]
    fn tempest_fixture_preserves_minute_records_units_and_provenance() {
        let batch = normalize_tempest_observations(
            fixture_envelope(),
            &fixture_request(),
            1_787_432_400_000,
            2,
        )
        .expect("fixture should normalize");

        assert_eq!(batch.adapter_version, TEMPEST_OBSERVATION_ADAPTER_VERSION);
        assert_eq!(batch.attempts, 2);
        assert_eq!(batch.records.len(), 3);
        assert_eq!(batch.provider_cursor_valid_at_ms, Some(1_787_187_600_000));
        assert_eq!(batch.response_metadata.raw_observation_count, 3);
        assert_eq!(batch.response_metadata.minute_record_count, 3);
        assert_eq!(batch.response_metadata.bucket_step_minutes, Some(1.0));
        let first = &batch.records[0];
        assert_eq!(first.source_kind, TempestSourceKind::PhysicalSensor);
        assert_eq!(first.valid_at_ms, 1_787_184_000_000);
        assert_eq!(first.received_at_ms, 1_787_432_400_000);
        assert_eq!(first.metrics.temperature_c, Some(24.5));
        assert_eq!(first.metrics.precipitation_mm, Some(0.2));
        assert_eq!(first.metrics.precipitation_rate_mm_per_hour, Some(12.0));
        assert_eq!(first.metrics.wind_direction_degrees, Some(0.0));
        assert_eq!(first.metrics.solar_radiation_wm2, Some(193.0));
        assert_eq!(first.metrics.uv_index, Some(1.54));
        assert_eq!(first.device.model, "Tempest");
        assert_eq!(first.device.serial, "ST-00198967");
        assert_eq!(first.device.vendor, "WeatherFlow");
        assert_eq!(first.provider.illuminance_lux, Some(23_157.0));
        assert_eq!(first.quality.flags, None);
        assert_eq!(
            first.quality.sampling,
            "every_distinct_provider_observation"
        );
        assert_eq!(first.upstream_timezone, "America/Los_Angeles");
        assert_eq!(batch.records[1].valid_at_ms, 1_787_184_060_000);
        assert_eq!(batch.records[2].valid_at_ms, 1_787_187_600_000);
    }

    // preserve first duplicates input order nulls and quality evidence
    #[test]
    fn tempest_normalization_matches_distinct_null_and_uv_semantics() {
        let mut envelope = fixture_envelope();
        let observations = envelope
            .observations
            .as_mut()
            .expect("fixture should contain observations");
        let mut duplicate = observations[0].clone();
        duplicate.air_temperature_c = Some(50.0);
        let original_first = observations[0].clone();
        let mut modified_second = observations[1].clone();
        modified_second.report_interval_minutes = None;
        modified_second.precipitation_mm = None;
        modified_second.uv_index = Some(20.01);
        let original_third = observations[2].clone();
        let mut exclusive_end = original_third.clone();
        exclusive_end.epoch_seconds = 1_787_191_200;
        exclusive_end.air_temperature_c = Some(999.0);
        *observations = vec![
            modified_second,
            duplicate,
            original_first,
            exclusive_end,
            original_third,
        ];

        let batch =
            normalize_tempest_observations(envelope, &fixture_request(), 1_787_432_400_000, 1)
                .expect("distinct fixture should normalize");

        assert_eq!(batch.records.len(), 3);
        assert_eq!(batch.response_metadata.raw_observation_count, 5);
        assert_eq!(batch.response_metadata.minute_record_count, 3);
        assert_eq!(batch.records[0].valid_at_ms, 1_787_184_060_000);
        assert_eq!(batch.records[0].metrics.precipitation_mm, None);
        assert_eq!(
            batch.records[0].metrics.precipitation_rate_mm_per_hour,
            None
        );
        assert_eq!(batch.records[0].metrics.uv_index, None);
        assert_eq!(
            batch.records[0].quality.flags,
            Some(vec![TempestQualityFlag::UvIndexOutOfRange]),
        );
        assert_eq!(batch.records[1].metrics.temperature_c, Some(50.0));

        let empty = normalize_tempest_observations(
            TempestObservationEnvelope {
                observations: None,
                ..fixture_envelope()
            },
            &fixture_request(),
            1_787_432_400_000,
            1,
        )
        .expect("null observation gap should normalize");
        assert!(empty.records.is_empty());
        assert_eq!(empty.provider_cursor_valid_at_ms, None);
    }

    // reject identity status range and rate violations
    #[test]
    fn tempest_normalization_fails_closed_on_contract_violations() {
        let request = fixture_request();
        let rejected = normalize_tempest_observations(
            TempestObservationEnvelope {
                status_code: 4,
                ..fixture_envelope()
            },
            &request,
            1_787_432_400_000,
            1,
        )
        .expect_err("provider rejection should fail");
        assert_eq!(rejected.code, "provider_request_rejected");

        let invalid_identity = normalize_tempest_observations(
            TempestObservationEnvelope {
                device_id: 470_938,
                ..fixture_envelope()
            },
            &request,
            1_787_432_400_000,
            1,
        )
        .expect_err("device drift should fail");
        assert_eq!(invalid_identity.code, "invalid_payload");

        let invalid_range = normalize_tempest_observations(
            fixture_envelope(),
            &TempestObservationRequest {
                end_exclusive_ms: request.start_ms
                    + i64::try_from(TEMPEST_MAXIMUM_RANGE_SECONDS + 1).expect("range should fit")
                        * 1_000,
                ..request.clone()
            },
            1_787_432_400_000,
            1,
        )
        .expect_err("oversized range should fail");
        assert_eq!(invalid_range.code, "invalid_payload");

        let mut invalid_interval_envelope = fixture_envelope();
        invalid_interval_envelope
            .observations
            .as_mut()
            .expect("fixture should contain observations")[0]
            .report_interval_minutes = Some(0.0);
        let invalid_interval = normalize_tempest_observations(
            invalid_interval_envelope,
            &request,
            1_787_432_400_000,
            1,
        )
        .expect_err("zero report interval should fail");
        assert_eq!(invalid_interval.code, "invalid_payload");
    }
}
