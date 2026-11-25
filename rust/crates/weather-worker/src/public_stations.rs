//! Typed public-station normalization without transport, JSON, or date dependencies.

use std::collections::{BTreeMap, BTreeSet};

use super::{CanonicalWeatherMetrics, ScheduledProviderRecord, TaskFailure, normalize_metric};

pub const AMBIENT_WEATHER_ADAPTER_VERSION: &str = "ambient-device-data/v1";
pub const AMBIENT_WEATHER_DEVICE_DATA_ENDPOINT: &str = "device-data";
pub const AMBIENT_WEATHER_MAXIMUM_CADENCE_SECONDS: u64 = 86_400;
pub const AMBIENT_WEATHER_MINIMUM_CADENCE_SECONDS: u64 = 60;
pub const MAXIMUM_AMBIENT_WEATHER_RANGE_SECONDS: u64 = 31 * 24 * 60 * 60;

const MAXIMUM_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// One typed Ambient range after UTC timestamps were resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct AmbientWeatherRangeRequest {
    pub device_id: String,
    pub end_exclusive_ms: i64,
    pub mac_address: String,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub source_id: String,
    pub start_ms: i64,
    pub timezone: String,
}

/// One capability admitted from the checked public-station catalog.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AmbientWeatherCapability {
    Current,
    Historical,
}

/// One scheduled Ambient database identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmbientWeatherScheduledSource {
    pub active: bool,
    pub cadence_seconds: u64,
    pub id: String,
    pub material_config_identity: String,
    pub provider_key: String,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub source_kind: AmbientWeatherSourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// One checked Ambient station and source configuration identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmbientWeatherRuntimeConfiguration {
    pub adapter_contract_version: &'static str,
    pub capabilities: BTreeSet<AmbientWeatherCapability>,
    pub device_id: String,
    pub mac_address: String,
    pub material_config_identity: String,
    pub maximum_chunk_days: u16,
    pub model: Option<String>,
    pub provider_key: String,
    pub serial: Option<String>,
    pub site_slug: String,
    pub source_active: bool,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub source_kind: AmbientWeatherSourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// Inputs for one admitted scheduled Ambient execution.
#[derive(Clone, Copy, Debug)]
pub struct AmbientWeatherScheduledExecution<'a> {
    pub configuration: &'a AmbientWeatherRuntimeConfiguration,
    pub now_ms: i64,
    pub run_deadline_at_ms: i64,
    pub source: &'a AmbientWeatherScheduledSource,
}

/// One transport-parsed Ambient device observation.
#[derive(Clone, Debug, PartialEq)]
pub struct AmbientWeatherObservation {
    pub apparent_temperature_f: Option<f64>,
    pub date_utc_ms: i64,
    pub device_identity: AmbientWeatherDeviceIdentity,
    pub hourly_rain_in: Option<f64>,
    pub pressure_in_hg: Option<f64>,
    pub relative_humidity_percent: Option<f64>,
    pub solar_radiation_wm2: Option<f64>,
    pub temperature_f: Option<f64>,
    pub uv_index: Option<f64>,
    pub wind_direction_degrees: Option<f64>,
    pub wind_gust_mph: Option<f64>,
    pub wind_speed_mph: Option<f64>,
}

/// Optional Ambient device identity without collapsing invalid JSON values into absence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AmbientWeatherDeviceIdentity {
    Invalid,
    Missing,
    Value(String),
}

/// One transport-parsed Ambient response.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AmbientWeatherPayload {
    pub observations: Vec<AmbientWeatherObservation>,
}

/// Public Ambient observations retain physical-sensor identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmbientWeatherSourceKind {
    PhysicalSensor,
}

/// Stable public-station device provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmbientWeatherDeviceMetadata {
    pub model: Option<String>,
    pub serial: Option<String>,
    pub vendor: &'static str,
}

/// Ambient dataset provenance retained on every record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmbientWeatherProviderMetadata {
    pub dataset: &'static str,
    pub device_id: String,
}

/// Stable Ambient sampling evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AmbientWeatherQualityMetadata {
    pub sampling: &'static str,
}

/// One normalized Ambient public observation.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedAmbientWeatherRecord {
    pub device: AmbientWeatherDeviceMetadata,
    pub metrics: CanonicalWeatherMetrics,
    pub model: Option<String>,
    pub product_run_at_ms: Option<i64>,
    pub provider: AmbientWeatherProviderMetadata,
    pub quality: AmbientWeatherQualityMetadata,
    pub received_at_ms: i64,
    pub source_id: String,
    pub source_kind: AmbientWeatherSourceKind,
    pub upstream_timezone: String,
    pub valid_at_ms: i64,
}

/// One typed Ambient durable provider cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AmbientWeatherProviderCursor {
    pub valid_at_ms: i64,
}

/// Bounded Ambient batch diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AmbientWeatherResponseMetadata {
    pub normalized_record_count: usize,
    pub provider: &'static str,
    pub raw_observation_count: usize,
}

/// One normalized Ambient provider batch.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedAmbientWeatherBatch {
    pub adapter_version: &'static str,
    pub attempts: u32,
    pub provider_cursor: Option<AmbientWeatherProviderCursor>,
    pub records: Vec<NormalizedAmbientWeatherRecord>,
    pub response_metadata: AmbientWeatherResponseMetadata,
}

impl ScheduledProviderRecord for NormalizedAmbientWeatherRecord {
    // expose the normalized provider timestamp
    fn valid_at_ms(&self) -> i64 {
        self.valid_at_ms
    }
}

/// Normalize every first distinct in-window Ambient observation.
pub fn normalize_ambient_weather_range(
    payload: &AmbientWeatherPayload,
    request: &AmbientWeatherRangeRequest,
    received_at_ms: i64,
    attempts: u32,
) -> Result<NormalizedAmbientWeatherBatch, TaskFailure> {
    validate_request(request)?;
    let mut distinct = BTreeMap::new();

    // retain the first row at every exact in-window timestamp
    for row in &payload.observations {
        // require the provider's positive integer epoch
        if !(1..=MAXIMUM_SAFE_INTEGER).contains(&row.date_utc_ms) {
            return Err(ambient_invalid_payload());
        }

        // exclude inclusive-boundary and unrelated rows
        if row.date_utc_ms < request.start_ms || row.date_utc_ms >= request.end_exclusive_ms {
            continue;
        }

        // reject a conflicting optional device identity
        match &row.device_identity {
            AmbientWeatherDeviceIdentity::Missing => {}
            AmbientWeatherDeviceIdentity::Value(device_id) if device_id == &request.device_id => {}
            AmbientWeatherDeviceIdentity::Invalid | AmbientWeatherDeviceIdentity::Value(_) => {
                return Err(ambient_invalid_payload());
            }
        }

        distinct.entry(row.date_utc_ms).or_insert(row);
    }

    let mut records = Vec::with_capacity(distinct.len());

    // normalize records in ascending provider time
    for (valid_at_ms, row) in distinct {
        records.push(normalized_record(
            request,
            row,
            valid_at_ms,
            received_at_ms,
        )?);
    }
    let provider_cursor = records.last().map(|record| AmbientWeatherProviderCursor {
        valid_at_ms: record.valid_at_ms,
    });

    Ok(NormalizedAmbientWeatherBatch {
        adapter_version: AMBIENT_WEATHER_ADAPTER_VERSION,
        attempts,
        provider_cursor,
        response_metadata: AmbientWeatherResponseMetadata {
            normalized_record_count: records.len(),
            provider: "ambient-weather-network",
            raw_observation_count: payload.observations.len(),
        },
        records,
    })
}

// normalize one Ambient observation
fn normalized_record(
    request: &AmbientWeatherRangeRequest,
    row: &AmbientWeatherObservation,
    valid_at_ms: i64,
    received_at_ms: i64,
) -> Result<NormalizedAmbientWeatherRecord, TaskFailure> {
    Ok(NormalizedAmbientWeatherRecord {
        device: AmbientWeatherDeviceMetadata {
            model: request.model.clone(),
            serial: request.serial.clone(),
            vendor: "Ambient Weather",
        },
        metrics: CanonicalWeatherMetrics {
            apparent_temperature_c: ambient_apparent_temperature(row.apparent_temperature_f)?,
            black_globe_temperature_c: None,
            cloud_cover_percent: None,
            pm25_micrograms_per_cubic_meter: None,
            precipitation_mm: None,
            precipitation_rate_mm_per_hour: converted_metric(
                row.hourly_rain_in,
                25.4,
                0.0,
                10_000.0,
                false,
            )?,
            pressure_hpa: converted_metric(
                row.pressure_in_hg,
                33.863_886_666_7,
                100.0,
                1_200.0,
                false,
            )?,
            relative_humidity_percent: canonical_metric(
                row.relative_humidity_percent,
                0.0,
                100.0,
                false,
            )?,
            soil_electrical_conductivity_microsiemens_per_cm: None,
            soil_moisture_percent: None,
            solar_radiation_wm2: canonical_metric(row.solar_radiation_wm2, 0.0, 2_500.0, false)?,
            temperature_c: fahrenheit_temperature(row.temperature_f)?,
            uv_index: ambient_uv(row.uv_index)?,
            water_level_m: None,
            wet_bulb_globe_temperature_c: None,
            wind_direction_degrees: ambient_direction(row.wind_direction_degrees)?,
            wind_gust_mps: converted_metric(row.wind_gust_mph, 0.447_04, 0.0, 150.0, false)?,
            wind_speed_mps: converted_metric(row.wind_speed_mph, 0.447_04, 0.0, 150.0, false)?,
        },
        model: None,
        product_run_at_ms: None,
        provider: AmbientWeatherProviderMetadata {
            dataset: "public-device-data",
            device_id: request.device_id.clone(),
        },
        quality: AmbientWeatherQualityMetadata {
            sampling: "every_distinct_provider_observation",
        },
        received_at_ms,
        source_id: request.source_id.clone(),
        source_kind: AmbientWeatherSourceKind::PhysicalSensor,
        upstream_timezone: request.timezone.clone(),
        valid_at_ms,
    })
}

// validate one already-resolved Ambient request
fn validate_request(request: &AmbientWeatherRangeRequest) -> Result<(), TaskFailure> {
    // require bounded identity admitted by the configuration parser
    if !bounded_text(&request.source_id, 128)
        || !bounded_text(&request.timezone, 64)
        || !bounded_text(&request.device_id, 128)
        || !bounded_text(&request.mac_address, 64)
        || !ambient_device_id_is_valid(&request.device_id)
        || !ambient_mac_is_valid(&request.mac_address)
        || !optional_bounded_text(request.model.as_deref(), 128)
        || !optional_bounded_text(request.serial.as_deref(), 128)
    {
        return Err(ambient_invalid_payload());
    }
    let Some(duration_ms) = request.end_exclusive_ms.checked_sub(request.start_ms) else {
        return Err(ambient_invalid_payload());
    };
    let maximum_range_ms = i64::try_from(MAXIMUM_AMBIENT_WEATHER_RANGE_SECONDS)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000))
        .ok_or_else(ambient_invalid_payload)?;

    // require one bounded exact-second half-open range
    if request.start_ms % 1_000 != 0
        || request.end_exclusive_ms % 1_000 != 0
        || duration_ms <= 0
        || duration_ms > maximum_range_ms
    {
        return Err(ambient_invalid_payload());
    }

    Ok(())
}

// compare one durable source to the checked Ambient configuration
pub(crate) fn ambient_weather_source_matches_configuration(
    source: &AmbientWeatherScheduledSource,
    configuration: &AmbientWeatherRuntimeConfiguration,
) -> bool {
    ambient_weather_source_identity_matches_configuration(source, configuration)
        && configuration
            .capabilities
            .contains(&AmbientWeatherCapability::Current)
}

// compare one durable source to the historical Ambient configuration
pub(crate) fn ambient_weather_backfill_source_matches_configuration(
    source: &AmbientWeatherScheduledSource,
    configuration: &AmbientWeatherRuntimeConfiguration,
) -> bool {
    ambient_weather_source_identity_matches_configuration(source, configuration)
        && configuration
            .capabilities
            .contains(&AmbientWeatherCapability::Historical)
}

// compare the capability-independent Ambient source identity
fn ambient_weather_source_identity_matches_configuration(
    source: &AmbientWeatherScheduledSource,
    configuration: &AmbientWeatherRuntimeConfiguration,
) -> bool {
    source.active
        && configuration.source_active
        && !source.id.trim().is_empty()
        && source.id.len() <= 128
        && !source.provider_key.trim().is_empty()
        && !source.site_slug.trim().is_empty()
        && !source.station_slug.trim().is_empty()
        && !source.source_key.trim().is_empty()
        && !source.source_config_fingerprint.trim().is_empty()
        && !source.material_config_identity.trim().is_empty()
        && !source.timezone.trim().is_empty()
        && ambient_device_id_is_valid(&configuration.device_id)
        && ambient_mac_is_valid(&configuration.mac_address)
        && (1..=31).contains(&configuration.maximum_chunk_days)
        && optional_config_text_is_valid(configuration.model.as_deref(), 128)
        && optional_config_text_is_valid(configuration.serial.as_deref(), 128)
        && source.provider_key == configuration.provider_key
        && source.site_slug == configuration.site_slug
        && source.station_slug == configuration.station_slug
        && source.source_key == configuration.source_key
        && source.source_kind == AmbientWeatherSourceKind::PhysicalSensor
        && source.source_kind == configuration.source_kind
        && source.source_config_fingerprint == configuration.source_config_fingerprint
        && source.timezone == configuration.timezone
        && source.material_config_identity == configuration.material_config_identity
}

// require one lowercase Ambient device identifier
fn ambient_device_id_is_valid(value: &str) -> bool {
    value.len() == 24
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// require one uppercase canonical Ambient hardware address
fn ambient_mac_is_valid(value: &str) -> bool {
    let bytes = value.as_bytes();

    // require six uppercase hexadecimal octets
    bytes.len() == 17
        && bytes.iter().enumerate().all(|(index, byte)| {
            // distinguish separators from uppercase hexadecimal bytes
            if matches!(index, 2 | 5 | 8 | 11 | 14) {
                *byte == b':'
            } else {
                byte.is_ascii_digit() || (b'A'..=b'F').contains(byte)
            }
        })
}

// normalize one ordinary canonical metric
fn canonical_metric(
    value: Option<f64>,
    minimum: f64,
    maximum: f64,
    maximum_exclusive: bool,
) -> Result<Option<f64>, TaskFailure> {
    normalize_metric(value, minimum, maximum, maximum_exclusive)
        .map_err(|_| ambient_invalid_payload())
}

// convert and normalize one canonical metric
fn converted_metric(
    value: Option<f64>,
    multiplier: f64,
    minimum: f64,
    maximum: f64,
    maximum_exclusive: bool,
) -> Result<Option<f64>, TaskFailure> {
    canonical_metric(
        value.map(|measurement| measurement * multiplier),
        minimum,
        maximum,
        maximum_exclusive,
    )
}

// convert one Fahrenheit temperature
fn fahrenheit_temperature(value: Option<f64>) -> Result<Option<f64>, TaskFailure> {
    canonical_metric(
        value.map(|temperature| (temperature - 32.0) * (5.0 / 9.0)),
        -100.0,
        70.0,
        false,
    )
}

// discard impossible Ambient apparent-temperature values
fn ambient_apparent_temperature(value: Option<f64>) -> Result<Option<f64>, TaskFailure> {
    // reject non-finite provider values before applying the discard rule
    if value.is_some_and(|temperature| !temperature.is_finite()) {
        return Err(ambient_invalid_payload());
    }

    // preserve the Node adapter's pre-conversion discard boundary
    if value.is_some_and(|temperature| !(-148.0..=158.0).contains(&temperature)) {
        return Ok(None);
    }

    fahrenheit_temperature(value)
}

// normalize one optional circular direction
fn ambient_direction(value: Option<f64>) -> Result<Option<f64>, TaskFailure> {
    // reject non-finite provider values before applying missing-direction semantics
    if value.is_some_and(|direction| !direction.is_finite()) {
        return Err(ambient_invalid_payload());
    }

    // preserve unavailable negative provider directions
    if value.is_some_and(|direction| direction < 0.0) {
        return Ok(None);
    }
    let normalized = value.map(|direction| if direction == 360.0 { 0.0 } else { direction });
    canonical_metric(normalized, 0.0, 360.0, true)
}

// retain only canonical Ambient ultraviolet readings
fn ambient_uv(value: Option<f64>) -> Result<Option<f64>, TaskFailure> {
    // reject non-finite provider values before applying the discard rule
    if value.is_some_and(|uv| !uv.is_finite()) {
        return Err(ambient_invalid_payload());
    }

    // discard impossible public sensor values
    if value.is_some_and(|uv| !(0.0..=20.0).contains(&uv)) {
        return Ok(None);
    }

    canonical_metric(value, 0.0, 20.0, false)
}

// validate one already-parsed bounded string
fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum
}

// validate one optional already-parsed bounded string
fn optional_bounded_text(value: Option<&str>, maximum: usize) -> bool {
    value.is_none_or(|text| bounded_text(text, maximum))
}

// validate one optional string admitted from checked configuration
fn optional_config_text_is_valid(value: Option<&str>, maximum: usize) -> bool {
    value.is_none_or(|text| !text.trim().is_empty() && text.len() <= maximum)
}

// create one Ambient payload failure
fn ambient_invalid_payload() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "invalid_payload".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AMBIENT_WEATHER_ADAPTER_VERSION, AmbientWeatherDeviceIdentity, AmbientWeatherObservation,
        AmbientWeatherPayload, AmbientWeatherProviderCursor, AmbientWeatherRangeRequest,
        AmbientWeatherSourceKind, normalize_ambient_weather_range,
    };

    // build the compiled Node Ambient request fixture
    fn fixture_request() -> AmbientWeatherRangeRequest {
        AmbientWeatherRangeRequest {
            device_id: "5fe7d16a25083b000171f8e5".to_owned(),
            end_exclusive_ms: 1_787_580_000_000,
            mac_address: "F0:08:D1:07:B3:9E".to_owned(),
            model: Some("test-model".to_owned()),
            serial: Some("test-serial".to_owned()),
            source_id: "source-public".to_owned(),
            start_ms: 1_787_576_400_000,
            timezone: "America/Los_Angeles".to_owned(),
        }
    }

    // build one complete compiled Node observation fixture
    fn fixture_observation(date_utc_ms: i64) -> AmbientWeatherObservation {
        AmbientWeatherObservation {
            apparent_temperature_f: Some(54.0),
            date_utc_ms,
            device_identity: AmbientWeatherDeviceIdentity::Value(
                "5fe7d16a25083b000171f8e5".to_owned(),
            ),
            hourly_rain_in: Some(0.1),
            pressure_in_hg: Some(30.0),
            relative_humidity_percent: Some(90.0),
            solar_radiation_wm2: Some(100.0),
            temperature_f: Some(53.6),
            uv_index: Some(1.0),
            wind_direction_degrees: Some(360.0),
            wind_gust_mph: Some(10.0),
            wind_speed_mph: Some(5.0),
        }
    }

    // match the compiled Node Ambient normalization fixture
    #[test]
    fn ambient_fixture_preserves_range_units_cursor_and_provenance() {
        let batch = normalize_ambient_weather_range(
            &AmbientWeatherPayload {
                observations: vec![
                    fixture_observation(1_787_576_400_000),
                    fixture_observation(1_787_580_000_000),
                ],
            },
            &fixture_request(),
            1_787_583_600_000,
            2,
        )
        .expect("Ambient fixture should normalize");

        assert_eq!(batch.adapter_version, AMBIENT_WEATHER_ADAPTER_VERSION);
        assert_eq!(batch.attempts, 2);
        assert_eq!(batch.records.len(), 1);
        assert_eq!(
            batch.provider_cursor,
            Some(AmbientWeatherProviderCursor {
                valid_at_ms: 1_787_576_400_000,
            }),
        );
        assert_eq!(batch.response_metadata.raw_observation_count, 2);
        assert_eq!(batch.response_metadata.normalized_record_count, 1);
        assert_eq!(batch.response_metadata.provider, "ambient-weather-network");
        let record = &batch.records[0];
        assert_eq!(record.valid_at_ms, 1_787_576_400_000);
        assert_eq!(record.received_at_ms, 1_787_583_600_000);
        assert_eq!(record.source_kind, AmbientWeatherSourceKind::PhysicalSensor);
        assert!((record.metrics.temperature_c.expect("temperature") - 12.0).abs() < 1e-9);
        assert!(
            (record
                .metrics
                .apparent_temperature_c
                .expect("apparent temperature")
                - 12.222_222_222_222_221)
                .abs()
                < 1e-9
        );
        assert_eq!(record.metrics.precipitation_rate_mm_per_hour, Some(2.54));
        assert!((record.metrics.pressure_hpa.expect("pressure") - 1_015.916_600_001).abs() < 1e-9);
        assert_eq!(record.metrics.wind_direction_degrees, Some(0.0));
        assert_eq!(record.metrics.wind_speed_mps, Some(2.2352));
        assert_eq!(record.metrics.relative_humidity_percent, Some(90.0));
        assert_eq!(record.metrics.solar_radiation_wm2, Some(100.0));
        assert_eq!(record.metrics.uv_index, Some(1.0));
        assert_eq!(record.provider.dataset, "public-device-data");
        assert_eq!(record.provider.device_id, "5fe7d16a25083b000171f8e5");
        assert_eq!(record.device.vendor, "Ambient Weather");
        assert_eq!(record.device.model.as_deref(), Some("test-model"));
        assert_eq!(record.device.serial.as_deref(), Some("test-serial"));
        assert_eq!(
            record.quality.sampling,
            "every_distinct_provider_observation"
        );
    }

    // retain legacy missing identity and discard impossible derived values
    #[test]
    fn ambient_legacy_rows_preserve_null_and_discard_semantics() {
        let mut observation = fixture_observation(1_787_576_400_000);
        observation.apparent_temperature_f = Some(180.0);
        observation.device_identity = AmbientWeatherDeviceIdentity::Missing;
        observation.uv_index = Some(21.0);
        observation.wind_direction_degrees = Some(-1.0);
        let batch = normalize_ambient_weather_range(
            &AmbientWeatherPayload {
                observations: vec![observation],
            },
            &fixture_request(),
            1_787_583_600_000,
            1,
        )
        .expect("legacy row should normalize");

        let record = &batch.records[0];
        assert!((record.metrics.temperature_c.expect("temperature") - 12.0).abs() < 1e-9);
        assert_eq!(record.metrics.apparent_temperature_c, None);
        assert_eq!(record.metrics.uv_index, None);
        assert_eq!(record.metrics.wind_direction_degrees, None);
    }

    // preserve first exact observations and sort output by provider time
    #[test]
    fn ambient_distinct_rows_use_first_value_in_timestamp_order() {
        let mut later = fixture_observation(1_787_578_200_000);
        later.temperature_f = Some(68.0);
        let mut first = fixture_observation(1_787_576_400_000);
        first.temperature_f = Some(50.0);
        let mut duplicate = first.clone();
        duplicate.temperature_f = Some(86.0);
        let batch = normalize_ambient_weather_range(
            &AmbientWeatherPayload {
                observations: vec![later, first, duplicate],
            },
            &fixture_request(),
            1_787_583_600_000,
            1,
        )
        .expect("distinct rows should normalize");

        assert_eq!(batch.records.len(), 2);
        assert_eq!(batch.records[0].valid_at_ms, 1_787_576_400_000);
        assert_eq!(batch.records[1].valid_at_ms, 1_787_578_200_000);
        assert_eq!(batch.records[0].metrics.temperature_c, Some(10.0));
        assert_eq!(batch.records[1].metrics.temperature_c, Some(20.0));
        assert_eq!(
            batch.provider_cursor,
            Some(AmbientWeatherProviderCursor {
                valid_at_ms: 1_787_578_200_000,
            }),
        );
    }

    // reject conflicting device identity and invalid provider numbers
    #[test]
    fn ambient_rejects_identity_range_and_metric_drift() {
        let mut conflicting = fixture_observation(1_787_576_400_000);
        conflicting.device_identity =
            AmbientWeatherDeviceIdentity::Value("another-device".to_owned());
        let identity = normalize_ambient_weather_range(
            &AmbientWeatherPayload {
                observations: vec![conflicting],
            },
            &fixture_request(),
            1_787_583_600_000,
            1,
        )
        .expect_err("conflicting identity should fail");
        assert_eq!(identity.code, "invalid_payload");

        let mut invalid_identity = fixture_observation(1_787_576_400_000);
        invalid_identity.device_identity = AmbientWeatherDeviceIdentity::Invalid;
        let identity = normalize_ambient_weather_range(
            &AmbientWeatherPayload {
                observations: vec![invalid_identity],
            },
            &fixture_request(),
            1_787_583_600_000,
            1,
        )
        .expect_err("invalid typed identity should fail");
        assert_eq!(identity.code, "invalid_payload");

        let mut invalid_metric = fixture_observation(1_787_576_400_000);
        invalid_metric.temperature_f = Some(180.0);
        let metric = normalize_ambient_weather_range(
            &AmbientWeatherPayload {
                observations: vec![invalid_metric],
            },
            &fixture_request(),
            1_787_583_600_000,
            1,
        )
        .expect_err("canonical temperature drift should fail");
        assert_eq!(metric.code, "invalid_payload");

        let mut invalid_range = fixture_request();
        invalid_range.end_exclusive_ms = invalid_range.start_ms + 31 * 86_400_000 + 1_000;
        let range = normalize_ambient_weather_range(
            &AmbientWeatherPayload::default(),
            &invalid_range,
            1_787_583_600_000,
            1,
        )
        .expect_err("oversized range should fail");
        assert_eq!(range.code, "invalid_payload");

        let mut maximum_range = fixture_request();
        maximum_range.end_exclusive_ms = maximum_range.start_ms + 31 * 86_400_000;
        let batch = normalize_ambient_weather_range(
            &AmbientWeatherPayload::default(),
            &maximum_range,
            1_787_583_600_000,
            1,
        )
        .expect("maximum range should normalize");
        assert!(batch.records.is_empty());
        assert_eq!(batch.provider_cursor, None);
    }
}
