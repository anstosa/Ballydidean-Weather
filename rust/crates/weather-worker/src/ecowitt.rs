//! Typed Ecowitt local-live normalization without transport, JSON, or date dependencies.

use std::collections::BTreeSet;

use super::{CanonicalWeatherMetrics, ScheduledProviderRecord, TaskFailure, normalize_metric};

pub const ECOWITT_LOCAL_LIVE_ADAPTER_VERSION: &str = "ecowitt-local-live/v1";
pub const ECOWITT_LOCAL_LIVE_ENDPOINT: &str = "get_livedata_info";
/// Maximum cadence accepted by the versioned Ecowitt configuration.
pub const ECOWITT_MAXIMUM_CADENCE_SECONDS: u64 = 3_600;
/// Minimum cadence accepted by the versioned Ecowitt configuration.
pub const ECOWITT_MINIMUM_CADENCE_SECONDS: u64 = 60;
/// Node's canonical provider metadata boundary in JavaScript string units.
pub const MAXIMUM_ECOWITT_PROVIDER_METADATA_JS_STRING_UNITS: usize = 8_192;

/// One local calendar day admitted by a timezone-aware time adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EcowittAdmittedLocalDate {
    day: u8,
    month: u8,
    year: i32,
}

impl EcowittAdmittedLocalDate {
    /// Construct one date already admitted by the implementing time library.
    pub const fn admitted(year: i32, month: u8, day: u8) -> Self {
        Self { day, month, year }
    }

    // format the durable daily-counter key
    fn cursor_day(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Timezone-aware calendar admission required by Ecowitt normalization.
pub trait EcowittTimeAdapter {
    /// Resolve the source-local day for one exact received instant.
    fn local_date(
        &mut self,
        received_at_ms: i64,
        timezone: &str,
    ) -> Result<EcowittAdmittedLocalDate, TaskFailure>;
}

/// One canonical provider-metadata size admitted in JavaScript string units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EcowittCanonicalMetadataMeasurement {
    js_string_units: usize,
}

impl EcowittCanonicalMetadataMeasurement {
    /// Construct one measurement from an admitted canonical serializer.
    pub const fn admitted(js_string_units: usize) -> Self {
        Self { js_string_units }
    }
}

/// Canonical serialization admission required before returning Ecowitt records.
pub trait EcowittCanonicalMetadataAdapter {
    /// Measure the actual constructed provider metadata using JavaScript string units.
    fn measure_provider_metadata(
        &mut self,
        metadata: &EcowittProviderMetadata,
    ) -> Result<EcowittCanonicalMetadataMeasurement, TaskFailure>;
}

/// One provider measurement whose JSON representation was already parsed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EcowittMeasurement {
    pub unit: EcowittUnit,
    pub value: f64,
}

/// Closed units accepted by the Ecowitt local-live adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EcowittUnit {
    Celsius,
    Fahrenheit,
    Hectopascal,
    Inch,
    InchOfMercury,
    InchPerHour,
    Kilopascal,
    KilometerPerHour,
    Knot,
    MeterPerSecond,
    MicrogramsPerCubicMeter,
    MicrosiemensPerCentimeter,
    MilePerHour,
    Millimeter,
    MillimeterPerHour,
    Percent,
    Unitless,
    Unsupported,
    WattPerSquareMeter,
}

/// One parsed battery state, including unrecognized provider values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EcowittBatteryState {
    Integer(i64),
    Unrecognized,
}

/// One prior durable daily-counter cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct EcowittPreviousCursor {
    pub rain_daily_total_mm: Option<f64>,
    pub rain_day: Option<String>,
}

/// One typed current-snapshot request after host and timezone parsing.
#[derive(Clone, Debug, PartialEq)]
pub struct EcowittLiveRequest {
    pub expected_mac: String,
    pub model: String,
    pub previous_cursor: Option<EcowittPreviousCursor>,
    pub source_id: String,
    pub timezone: String,
}

/// One capability admitted from the loaded Ecowitt runtime contract.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EcowittCapability {
    Current,
}

/// One scheduled Ecowitt database identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EcowittScheduledSource {
    pub active: bool,
    pub cadence_seconds: u64,
    pub id: String,
    pub material_config_identity: String,
    pub provider_key: String,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub source_kind: EcowittSourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// One loaded Ecowitt station configuration identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EcowittRuntimeConfiguration {
    pub adapter_contract_version: &'static str,
    pub capabilities: BTreeSet<EcowittCapability>,
    pub expected_mac: String,
    pub gateway_host: String,
    pub material_config_identity: String,
    pub model: String,
    pub provider_key: String,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub source_kind: EcowittSourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// Inputs for one admitted scheduled Ecowitt execution.
#[derive(Clone, Copy, Debug)]
pub struct EcowittScheduledExecution<'a> {
    pub configuration: &'a EcowittRuntimeConfiguration,
    pub now_ms: i64,
    pub run_deadline_at_ms: i64,
    pub source: &'a EcowittScheduledSource,
}

/// One exact configured LAN request dispatched by the scheduled lifecycle.
#[derive(Clone, Debug, PartialEq)]
pub struct EcowittScheduledRequest {
    pub expected_mac: String,
    pub gateway_host: String,
    pub model: String,
    pub previous_cursor: Option<EcowittProviderCursor>,
    pub source_id: String,
    pub timezone: String,
}

/// Canonical outdoor values extracted from common_list by stable ID.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EcowittCommonReadings {
    pub apparent_temperature_fallback: Option<EcowittMeasurement>,
    pub apparent_temperature_primary: Option<EcowittMeasurement>,
    pub black_globe_battery: Option<EcowittBatteryState>,
    pub black_globe_temperature: Option<EcowittMeasurement>,
    pub outdoor_humidity: Option<EcowittMeasurement>,
    pub outdoor_temperature: Option<EcowittMeasurement>,
    pub solar_radiation: Option<EcowittMeasurement>,
    pub uv_index: Option<EcowittMeasurement>,
    pub wet_bulb_globe_temperature: Option<EcowittMeasurement>,
    pub wind_direction: Option<EcowittMeasurement>,
    pub wind_gust: Option<EcowittMeasurement>,
    pub wind_speed: Option<EcowittMeasurement>,
}

/// Gateway environmental readings extracted from wh25.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EcowittGatewayReadings {
    pub indoor_humidity: Option<EcowittMeasurement>,
    pub indoor_temperature: Option<EcowittMeasurement>,
    pub relative_pressure: Option<EcowittMeasurement>,
}

/// One populated rain hardware group.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EcowittRainReadings {
    pub battery: Option<EcowittBatteryState>,
    pub daily_total: Option<EcowittMeasurement>,
    pub rate: Option<EcowittMeasurement>,
}

/// One multi-channel temperature and humidity sensor.
#[derive(Clone, Debug, PartialEq)]
pub struct EcowittTemperatureSensor {
    pub battery: Option<EcowittBatteryState>,
    pub channel: u8,
    pub humidity: Option<EcowittMeasurement>,
    pub temperature: Option<EcowittMeasurement>,
}

/// One multi-channel soil sensor.
#[derive(Clone, Debug, PartialEq)]
pub struct EcowittSoilSensor {
    pub battery: Option<EcowittBatteryState>,
    pub channel: u8,
    pub conductivity: Option<EcowittMeasurement>,
    pub moisture: Option<EcowittMeasurement>,
    pub temperature: Option<EcowittMeasurement>,
}

/// One multi-channel particulate sensor.
#[derive(Clone, Debug, PartialEq)]
pub struct EcowittParticulateSensor {
    pub battery: Option<EcowittBatteryState>,
    pub channel: u8,
    pub pm25: Option<EcowittMeasurement>,
}

/// One transport-parsed Ecowitt live response.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EcowittLivePayload {
    pub common: EcowittCommonReadings,
    pub gateway: Option<EcowittGatewayReadings>,
    pub group_count: usize,
    pub particulate_sensors: Vec<EcowittParticulateSensor>,
    pub piezo_rain: Option<EcowittRainReadings>,
    pub soil_sensors: Vec<EcowittSoilSensor>,
    pub temperature_sensors: Vec<EcowittTemperatureSensor>,
    pub traditional_rain: Option<EcowittRainReadings>,
}

/// Ecowitt's current physical-sensor source identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EcowittSourceKind {
    PhysicalSensor,
}

/// Stable gateway provenance on one normalized record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EcowittDeviceMetadata {
    pub model: String,
    pub serial: String,
    pub vendor: &'static str,
}

/// Sparse normalized readings owned by one property sensor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EcowittSensorReadings {
    pub battery_external_power: Option<f64>,
    pub battery_level: Option<f64>,
    pub battery_low: Option<f64>,
    pub black_globe_temperature_c: Option<f64>,
    pub daily_precipitation_mm: Option<f64>,
    pub pm25_micrograms_per_cubic_meter: Option<f64>,
    pub precipitation_rate_mm_per_hour: Option<f64>,
    pub pressure_hpa: Option<f64>,
    pub relative_humidity_percent: Option<f64>,
    pub soil_electrical_conductivity_microsiemens_per_cm: Option<f64>,
    pub soil_moisture_percent: Option<f64>,
    pub solar_radiation_wm2: Option<f64>,
    pub temperature_c: Option<f64>,
    pub uv_index: Option<f64>,
    pub wet_bulb_globe_temperature_c: Option<f64>,
    pub wind_direction_degrees: Option<f64>,
    pub wind_gust_mps: Option<f64>,
    pub wind_speed_mps: Option<f64>,
}

/// One independently scoped Ecowitt property sensor.
#[derive(Clone, Debug, PartialEq)]
pub struct EcowittSensorSnapshot {
    pub channel: Option<u8>,
    pub key: String,
    pub model: &'static str,
    pub readings: EcowittSensorReadings,
}

/// One selected rain hardware kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EcowittRainGaugeKind {
    Piezo,
    Traditional,
}

/// Provider provenance retained with the canonical record.
#[derive(Clone, Debug, PartialEq)]
pub struct EcowittProviderMetadata {
    pub dataset: &'static str,
    pub device_id: String,
    pub property_sensors: Vec<EcowittSensorSnapshot>,
}

/// Stable local-poll quality evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EcowittQualityMetadata {
    pub sampling: &'static str,
}

/// One canonical current record from the Ecowitt gateway.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedEcowittRecord {
    pub device: EcowittDeviceMetadata,
    pub metrics: CanonicalWeatherMetrics,
    pub model: Option<String>,
    pub product_run_at_ms: Option<i64>,
    pub provider: EcowittProviderMetadata,
    pub quality: EcowittQualityMetadata,
    pub received_at_ms: i64,
    pub source_id: String,
    pub source_kind: EcowittSourceKind,
    pub upstream_timezone: String,
    pub valid_at_ms: i64,
}

/// One durable current-snapshot cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct EcowittProviderCursor {
    pub rain_daily_total_mm: Option<f64>,
    pub rain_day: String,
    pub valid_at_ms: i64,
}

/// Bounded Ecowitt gateway response diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EcowittResponseMetadata {
    pub gateway_mac: String,
    pub group_count: usize,
    pub rain_gauge: Option<EcowittRainGaugeKind>,
    pub soil_channel_count: usize,
    pub temperature_humidity_channel_count: usize,
}

/// One normalized Ecowitt current provider batch.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedEcowittBatch {
    pub adapter_version: &'static str,
    pub attempts: u32,
    pub provider_cursor: EcowittProviderCursor,
    pub records: Vec<NormalizedEcowittRecord>,
    pub response_metadata: EcowittResponseMetadata,
}

impl ScheduledProviderRecord for NormalizedEcowittRecord {
    // expose the normalized provider timestamp
    fn valid_at_ms(&self) -> i64 {
        self.valid_at_ms
    }
}

/// Confirm the configured station is answering before the scheduled live request.
pub fn require_ecowitt_gateway_identity(
    gateway_mac: &str,
    expected_mac: &str,
) -> Result<(), TaskFailure> {
    require_normalized_gateway_identity(gateway_mac, &normalized_mac(expected_mac)?)
}

/// Match the decoded gateway against an already normalized configured MAC.
fn require_normalized_gateway_identity(
    gateway_mac: &str,
    expected_mac: &str,
) -> Result<(), TaskFailure> {
    // do not attribute the sample to a different configured station
    if normalized_mac(gateway_mac)? != expected_mac {
        return Err(ecowitt_invalid_payload());
    }
    Ok(())
}

/// Normalize one identity-bound Ecowitt local-live current snapshot.
pub fn normalize_ecowitt_live<T, M>(
    gateway_mac: &str,
    payload: &EcowittLivePayload,
    request: &EcowittLiveRequest,
    received_at_ms: i64,
    attempts: u32,
    time_adapter: &mut T,
    metadata_adapter: &mut M,
) -> Result<NormalizedEcowittBatch, TaskFailure>
where
    M: EcowittCanonicalMetadataAdapter,
    T: EcowittTimeAdapter,
{
    let expected_mac = validate_request(request)?;
    require_normalized_gateway_identity(gateway_mac, &expected_mac)?;

    // bind the daily cursor to the exact received instant and source timezone
    let received_local_day = time_adapter
        .local_date(received_at_ms, &request.timezone)?
        .cursor_day();

    let rain = select_rain(payload);
    let daily_total_mm = rain
        .map(|(_, readings)| rain_accumulation(readings.daily_total))
        .transpose()?
        .flatten();
    let precipitation_mm = rain_delta(
        daily_total_mm,
        &received_local_day,
        request.previous_cursor.as_ref(),
    )?;
    let primary_soil = payload
        .soil_sensors
        .iter()
        .find(|sensor| sensor.channel == 1);
    let primary_particulate = payload
        .particulate_sensors
        .iter()
        .find(|sensor| sensor.channel == 1);
    let relative_pressure = payload
        .gateway
        .as_ref()
        .and_then(|gateway| gateway.relative_pressure);
    let property_sensors = normalize_sensor_snapshots(payload, rain)?;
    let rain_rate_mm_per_hour = rain
        .map(|(_, readings)| rain_rate(readings.rate))
        .transpose()?
        .flatten();
    let provider_metadata = EcowittProviderMetadata {
        dataset: "get_livedata_info",
        device_id: expected_mac.clone(),
        property_sensors,
    };
    let metadata_measurement = metadata_adapter.measure_provider_metadata(&provider_metadata)?;

    // reject records whose actual canonical provider metadata exceeds Node's boundary
    if metadata_measurement.js_string_units > MAXIMUM_ECOWITT_PROVIDER_METADATA_JS_STRING_UNITS {
        return Err(ecowitt_invalid_payload());
    }
    let record = NormalizedEcowittRecord {
        device: EcowittDeviceMetadata {
            model: request.model.clone(),
            serial: expected_mac.clone(),
            vendor: "Ecowitt",
        },
        metrics: CanonicalWeatherMetrics {
            apparent_temperature_c: temperature_metric(
                request_measurement(
                    payload.common.apparent_temperature_primary,
                    payload.common.apparent_temperature_fallback,
                ),
                TemperatureMetric::Apparent,
            )?,
            black_globe_temperature_c: temperature_metric(
                payload.common.black_globe_temperature,
                TemperatureMetric::BlackGlobe,
            )?,
            cloud_cover_percent: None,
            pm25_micrograms_per_cubic_meter: particulate_metric(
                primary_particulate.and_then(|sensor| sensor.pm25),
            )?,
            precipitation_mm,
            precipitation_rate_mm_per_hour: rain_rate_mm_per_hour,
            pressure_hpa: pressure_metric(relative_pressure)?,
            relative_humidity_percent: percent_metric(payload.common.outdoor_humidity)?,
            soil_electrical_conductivity_microsiemens_per_cm: soil_ec_metric(
                primary_soil.and_then(|sensor| sensor.conductivity),
            )?,
            soil_moisture_percent: percent_metric(primary_soil.and_then(|sensor| sensor.moisture))?,
            solar_radiation_wm2: solar_metric(payload.common.solar_radiation)?,
            temperature_c: temperature_metric(
                payload.common.outdoor_temperature,
                TemperatureMetric::Temperature,
            )?,
            uv_index: unitless_uv_metric(payload.common.uv_index)?,
            water_level_m: None,
            wet_bulb_globe_temperature_c: temperature_metric(
                payload.common.wet_bulb_globe_temperature,
                TemperatureMetric::WetBulbGlobe,
            )?,
            wind_direction_degrees: direction_metric(payload.common.wind_direction)?,
            wind_gust_mps: wind_metric(payload.common.wind_gust)?,
            wind_speed_mps: wind_metric(payload.common.wind_speed)?,
        },
        model: None,
        product_run_at_ms: None,
        provider: provider_metadata,
        quality: EcowittQualityMetadata {
            sampling: "local_gateway_poll",
        },
        received_at_ms,
        source_id: request.source_id.clone(),
        source_kind: EcowittSourceKind::PhysicalSensor,
        upstream_timezone: request.timezone.clone(),
        valid_at_ms: received_at_ms,
    };

    Ok(NormalizedEcowittBatch {
        adapter_version: ECOWITT_LOCAL_LIVE_ADAPTER_VERSION,
        attempts,
        provider_cursor: EcowittProviderCursor {
            rain_daily_total_mm: daily_total_mm,
            rain_day: received_local_day,
            valid_at_ms: record.valid_at_ms,
        },
        records: vec![record],
        response_metadata: EcowittResponseMetadata {
            gateway_mac: expected_mac,
            group_count: payload.group_count,
            rain_gauge: rain.map(|(kind, _)| kind),
            soil_channel_count: payload.soil_sensors.len(),
            temperature_humidity_channel_count: payload.temperature_sensors.len(),
        },
    })
}

// select the primary rain hardware scope
fn select_rain(
    payload: &EcowittLivePayload,
) -> Option<(EcowittRainGaugeKind, &EcowittRainReadings)> {
    // prefer the dedicated tipping-bucket gauge
    if let Some(rain) = payload.traditional_rain.as_ref() {
        return Some((EcowittRainGaugeKind::Traditional, rain));
    }

    payload
        .piezo_rain
        .as_ref()
        .map(|rain| (EcowittRainGaugeKind::Piezo, rain))
}

// normalize every independently scoped property sensor
fn normalize_sensor_snapshots(
    payload: &EcowittLivePayload,
    rain: Option<(EcowittRainGaugeKind, &EcowittRainReadings)>,
) -> Result<Vec<EcowittSensorSnapshot>, TaskFailure> {
    let mut sensors = Vec::new();

    // retain gateway environmental readings
    if let Some(gateway) = payload.gateway.as_ref() {
        sensors.push(EcowittSensorSnapshot {
            channel: None,
            key: "gateway".to_owned(),
            model: "GW3000",
            readings: EcowittSensorReadings {
                pressure_hpa: pressure_metric(gateway.relative_pressure)?,
                relative_humidity_percent: percent_metric(gateway.indoor_humidity)?,
                temperature_c: temperature_metric(
                    gateway.indoor_temperature,
                    TemperatureMetric::Temperature,
                )?,
                ..EcowittSensorReadings::default()
            },
        });
    }

    let piezo_rain = rain.filter(|(kind, _)| *kind == EcowittRainGaugeKind::Piezo);
    sensors.push(EcowittSensorSnapshot {
        channel: None,
        key: "weather-array".to_owned(),
        model: "WS90",
        readings: EcowittSensorReadings {
            battery_level: battery_value(
                payload.piezo_rain.as_ref().and_then(|group| group.battery),
                5,
            ),
            daily_precipitation_mm: piezo_rain
                .map(|(_, readings)| rain_accumulation(readings.daily_total))
                .transpose()?
                .flatten(),
            precipitation_rate_mm_per_hour: piezo_rain
                .map(|(_, readings)| rain_rate(readings.rate))
                .transpose()?
                .flatten(),
            relative_humidity_percent: percent_metric(payload.common.outdoor_humidity)?,
            solar_radiation_wm2: solar_metric(payload.common.solar_radiation)?,
            temperature_c: temperature_metric(
                payload.common.outdoor_temperature,
                TemperatureMetric::Temperature,
            )?,
            uv_index: unitless_uv_metric(payload.common.uv_index)?,
            wind_direction_degrees: direction_metric(payload.common.wind_direction)?,
            wind_gust_mps: wind_metric(payload.common.wind_gust)?,
            wind_speed_mps: wind_metric(payload.common.wind_speed)?,
            ..EcowittSensorReadings::default()
        },
    });

    let globe_readings = EcowittSensorReadings {
        battery_level: battery_value(payload.common.black_globe_battery, 5),
        black_globe_temperature_c: temperature_metric(
            payload.common.black_globe_temperature,
            TemperatureMetric::BlackGlobe,
        )?,
        wet_bulb_globe_temperature_c: temperature_metric(
            payload.common.wet_bulb_globe_temperature,
            TemperatureMetric::WetBulbGlobe,
        )?,
        ..EcowittSensorReadings::default()
    };

    // retain an installed heat-stress sensor only when reporting
    if sensor_has_readings(&globe_readings) {
        sensors.push(EcowittSensorSnapshot {
            channel: None,
            key: "black-globe".to_owned(),
            model: "WN38",
            readings: globe_readings,
        });
    }

    // retain a dedicated traditional rain gauge separately
    if let Some((EcowittRainGaugeKind::Traditional, readings)) = rain {
        sensors.push(EcowittSensorSnapshot {
            channel: None,
            key: "rain-gauge".to_owned(),
            model: "WH40H",
            readings: EcowittSensorReadings {
                battery_level: battery_value(readings.battery, 5),
                daily_precipitation_mm: rain_accumulation(readings.daily_total)?,
                precipitation_rate_mm_per_hour: rain_rate(readings.rate)?,
                ..EcowittSensorReadings::default()
            },
        });
    }

    // retain every temperature channel in provider order
    for sensor in &payload.temperature_sensors {
        validate_channel(sensor.channel)?;
        sensors.push(EcowittSensorSnapshot {
            channel: Some(sensor.channel),
            key: format!("temperature-{}", sensor.channel),
            model: temperature_sensor_model(sensor.channel),
            readings: EcowittSensorReadings {
                battery_low: battery_value(sensor.battery, 1),
                relative_humidity_percent: percent_metric(sensor.humidity)?,
                temperature_c: temperature_metric(
                    sensor.temperature,
                    TemperatureMetric::Temperature,
                )?,
                ..EcowittSensorReadings::default()
            },
        });
    }

    // retain every soil channel in provider order
    for sensor in &payload.soil_sensors {
        validate_channel(sensor.channel)?;
        sensors.push(EcowittSensorSnapshot {
            channel: Some(sensor.channel),
            key: format!("soil-{}", sensor.channel),
            model: "WH52",
            readings: EcowittSensorReadings {
                battery_level: battery_value(sensor.battery, 5),
                soil_electrical_conductivity_microsiemens_per_cm: soil_ec_metric(
                    sensor.conductivity,
                )?,
                soil_moisture_percent: percent_metric(sensor.moisture)?,
                temperature_c: temperature_metric(
                    sensor.temperature,
                    TemperatureMetric::Temperature,
                )?,
                ..EcowittSensorReadings::default()
            },
        });
    }

    // retain every particulate channel in provider order
    for sensor in &payload.particulate_sensors {
        validate_channel(sensor.channel)?;
        sensors.push(EcowittSensorSnapshot {
            channel: Some(sensor.channel),
            key: format!("air-quality-{}", sensor.channel),
            model: "WH41",
            readings: EcowittSensorReadings {
                battery_external_power: battery_external_power(sensor.battery),
                battery_level: battery_value(sensor.battery, 5),
                pm25_micrograms_per_cubic_meter: particulate_metric(sensor.pm25)?,
                ..EcowittSensorReadings::default()
            },
        });
    }

    Ok(sensors)
}

// require immutable request identity
fn validate_request(request: &EcowittLiveRequest) -> Result<String, TaskFailure> {
    let expected_mac = normalized_mac(&request.expected_mac)?;

    // require bounded parsed identity fields
    if !bounded_text(&request.model, 128)
        || !bounded_text(&request.source_id, 128)
        || !bounded_text(&request.timezone, 64)
    {
        return Err(ecowitt_invalid_payload());
    }

    Ok(expected_mac)
}

// normalize one canonical hardware address
fn normalized_mac(value: &str) -> Result<String, TaskFailure> {
    let upper = value.to_ascii_uppercase();
    let bytes = upper.as_bytes();

    // require six hexadecimal octets
    if bytes.len() != 17
        || bytes.iter().enumerate().any(|(index, byte)| {
            // distinguish separators from hexadecimal bytes
            if matches!(index, 2 | 5 | 8 | 11 | 14) {
                *byte != b':'
            } else {
                !byte.is_ascii_hexdigit()
            }
        })
    {
        return Err(ecowitt_invalid_payload());
    }

    Ok(upper)
}

// select a primary value with one documented fallback
fn request_measurement(
    primary: Option<EcowittMeasurement>,
    fallback: Option<EcowittMeasurement>,
) -> Option<EcowittMeasurement> {
    primary.or(fallback)
}

// derive an interval accumulation from one durable daily total
fn rain_delta(
    current_total_mm: Option<f64>,
    local_day: &str,
    previous_cursor: Option<&EcowittPreviousCursor>,
) -> Result<Option<f64>, TaskFailure> {
    let Some(current_total_mm) = current_total_mm else {
        return Ok(None);
    };
    let previous_total = previous_cursor.and_then(|cursor| cursor.rain_daily_total_mm);
    let previous_day = previous_cursor.and_then(|cursor| cursor.rain_day.as_deref());

    // seed a new local day from the current daily total
    if previous_day != Some(local_day) {
        return canonical_metric(Some(current_total_mm), 0.0, 2_000.0, false);
    }

    // seed a cursor without one numeric daily total
    let Some(previous_total) = previous_total else {
        return canonical_metric(Some(current_total_mm), 0.0, 2_000.0, false);
    };

    // avoid double counting a same-day gateway reset
    if current_total_mm < previous_total {
        return Ok(None);
    }

    canonical_metric(Some(current_total_mm - previous_total), 0.0, 2_000.0, false)
}

// normalize one rain accumulation
fn rain_accumulation(measurement: Option<EcowittMeasurement>) -> Result<Option<f64>, TaskFailure> {
    let Some(measurement) = measurement else {
        return Ok(None);
    };

    // convert the supported rain accumulation units
    let value = match measurement.unit {
        EcowittUnit::Millimeter => measurement.value,
        EcowittUnit::Inch => measurement.value * 25.4,
        _ => return Err(ecowitt_invalid_payload()),
    };
    canonical_metric(Some(value), 0.0, 2_000.0, false)
}

// normalize one rain rate
fn rain_rate(measurement: Option<EcowittMeasurement>) -> Result<Option<f64>, TaskFailure> {
    let Some(measurement) = measurement else {
        return Ok(None);
    };

    // convert the supported rain rate units
    let value = match measurement.unit {
        EcowittUnit::MillimeterPerHour => measurement.value,
        EcowittUnit::InchPerHour => measurement.value * 25.4,
        _ => return Err(ecowitt_invalid_payload()),
    };
    canonical_metric(Some(value), 0.0, 10_000.0, false)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TemperatureMetric {
    Apparent,
    BlackGlobe,
    Temperature,
    WetBulbGlobe,
}

// normalize one temperature measurement
fn temperature_metric(
    measurement: Option<EcowittMeasurement>,
    metric: TemperatureMetric,
) -> Result<Option<f64>, TaskFailure> {
    let Some(measurement) = measurement else {
        return Ok(None);
    };

    // convert the supported temperature units
    let value = match measurement.unit {
        EcowittUnit::Celsius => measurement.value,
        EcowittUnit::Fahrenheit => (measurement.value - 32.0) * (5.0 / 9.0),
        _ => return Err(ecowitt_invalid_payload()),
    };
    // select the canonical temperature bounds
    let (minimum, maximum) = match metric {
        TemperatureMetric::Apparent | TemperatureMetric::Temperature => (-100.0, 70.0),
        TemperatureMetric::BlackGlobe | TemperatureMetric::WetBulbGlobe => (-100.0, 125.0),
    };
    canonical_metric(Some(value), minimum, maximum, false)
}

// normalize one relative pressure measurement
fn pressure_metric(measurement: Option<EcowittMeasurement>) -> Result<Option<f64>, TaskFailure> {
    let Some(measurement) = measurement else {
        return Ok(None);
    };

    // convert the supported pressure units
    let value = match measurement.unit {
        EcowittUnit::Hectopascal => measurement.value,
        EcowittUnit::Kilopascal => measurement.value * 1_000.0 / 100.0,
        EcowittUnit::InchOfMercury => measurement.value * 33.863_886_666_7,
        _ => return Err(ecowitt_invalid_payload()),
    };
    canonical_metric(Some(value), 100.0, 1_200.0, false)
}

// normalize one percentage measurement
fn percent_metric(measurement: Option<EcowittMeasurement>) -> Result<Option<f64>, TaskFailure> {
    let Some(measurement) = measurement else {
        return Ok(None);
    };

    // require an explicit percentage unit
    if measurement.unit != EcowittUnit::Percent {
        return Err(ecowitt_invalid_payload());
    }
    canonical_metric(Some(measurement.value), 0.0, 100.0, false)
}

// normalize one wind measurement
fn wind_metric(measurement: Option<EcowittMeasurement>) -> Result<Option<f64>, TaskFailure> {
    let Some(measurement) = measurement else {
        return Ok(None);
    };

    // convert the supported wind units
    let value = match measurement.unit {
        EcowittUnit::MeterPerSecond => measurement.value,
        EcowittUnit::MilePerHour => measurement.value * 0.447_04,
        EcowittUnit::KilometerPerHour => measurement.value / 3.6,
        EcowittUnit::Knot => measurement.value * 0.514_444,
        _ => return Err(ecowitt_invalid_payload()),
    };
    canonical_metric(Some(value), 0.0, 150.0, false)
}

// normalize one direction measurement
fn direction_metric(measurement: Option<EcowittMeasurement>) -> Result<Option<f64>, TaskFailure> {
    let Some(measurement) = measurement else {
        return Ok(None);
    };

    // require the gateway's unitless direction representation
    if measurement.unit != EcowittUnit::Unitless {
        return Err(ecowitt_invalid_payload());
    }
    // collapse the duplicate circular north value
    let value = if measurement.value == 360.0 {
        0.0
    } else {
        measurement.value
    };
    canonical_metric(Some(value), 0.0, 360.0, true)
}

// normalize one solar-radiation measurement
fn solar_metric(measurement: Option<EcowittMeasurement>) -> Result<Option<f64>, TaskFailure> {
    let Some(measurement) = measurement else {
        return Ok(None);
    };

    // require watts per square meter
    if measurement.unit != EcowittUnit::WattPerSquareMeter {
        return Err(ecowitt_invalid_payload());
    }
    canonical_metric(Some(measurement.value), 0.0, 2_500.0, false)
}

// normalize one unitless ultraviolet measurement
fn unitless_uv_metric(measurement: Option<EcowittMeasurement>) -> Result<Option<f64>, TaskFailure> {
    let Some(measurement) = measurement else {
        return Ok(None);
    };

    // require an actually unitless value
    if measurement.unit != EcowittUnit::Unitless {
        return Err(ecowitt_invalid_payload());
    }
    canonical_metric(Some(measurement.value), 0.0, 20.0, false)
}

// normalize one soil-conductivity measurement
fn soil_ec_metric(measurement: Option<EcowittMeasurement>) -> Result<Option<f64>, TaskFailure> {
    let Some(measurement) = measurement else {
        return Ok(None);
    };

    // require microsiemens per centimeter
    if measurement.unit != EcowittUnit::MicrosiemensPerCentimeter {
        return Err(ecowitt_invalid_payload());
    }
    canonical_metric(Some(measurement.value), 0.0, 10_000.0, false)
}

// normalize one particulate measurement
fn particulate_metric(measurement: Option<EcowittMeasurement>) -> Result<Option<f64>, TaskFailure> {
    let Some(measurement) = measurement else {
        return Ok(None);
    };

    // accept the gateway's implicit concentration unit
    if measurement.unit != EcowittUnit::Unitless
        && measurement.unit != EcowittUnit::MicrogramsPerCubicMeter
    {
        return Err(ecowitt_invalid_payload());
    }
    canonical_metric(Some(measurement.value), 0.0, 999.0, false)
}

// retain one bounded battery value
fn battery_value(state: Option<EcowittBatteryState>, maximum: i64) -> Option<f64> {
    let Some(EcowittBatteryState::Integer(value)) = state else {
        return None;
    };

    // omit out-of-family provider states
    if !(0..=maximum).contains(&value) {
        return None;
    }

    Some(value as f64)
}

// retain only the WH41 external-power sentinel
fn battery_external_power(state: Option<EcowittBatteryState>) -> Option<f64> {
    matches!(state, Some(EcowittBatteryState::Integer(6))).then_some(1.0)
}

// select the documented channel-specific temperature model
fn temperature_sensor_model(channel: u8) -> &'static str {
    // reserve channel five for the installed probe model
    if channel == 5 { "WN30" } else { "WN31" }
}

// validate one property sensor channel
fn validate_channel(channel: u8) -> Result<(), TaskFailure> {
    // require the documented channel range
    if !(1..=32).contains(&channel) {
        return Err(ecowitt_invalid_payload());
    }

    Ok(())
}

// report whether one sparse snapshot has any live reading
fn sensor_has_readings(readings: &EcowittSensorReadings) -> bool {
    readings.battery_external_power.is_some()
        || readings.battery_level.is_some()
        || readings.battery_low.is_some()
        || readings.black_globe_temperature_c.is_some()
        || readings.daily_precipitation_mm.is_some()
        || readings.pm25_micrograms_per_cubic_meter.is_some()
        || readings.precipitation_rate_mm_per_hour.is_some()
        || readings.pressure_hpa.is_some()
        || readings.relative_humidity_percent.is_some()
        || readings
            .soil_electrical_conductivity_microsiemens_per_cm
            .is_some()
        || readings.soil_moisture_percent.is_some()
        || readings.solar_radiation_wm2.is_some()
        || readings.temperature_c.is_some()
        || readings.uv_index.is_some()
        || readings.wet_bulb_globe_temperature_c.is_some()
        || readings.wind_direction_degrees.is_some()
        || readings.wind_gust_mps.is_some()
        || readings.wind_speed_mps.is_some()
}

// validate one canonical output metric
fn canonical_metric(
    value: Option<f64>,
    minimum: f64,
    maximum: f64,
    maximum_exclusive: bool,
) -> Result<Option<f64>, TaskFailure> {
    normalize_metric(value, minimum, maximum, maximum_exclusive)
        .map_err(|_| ecowitt_invalid_payload())
}

// validate one already-parsed text identity
fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum
}

// create one Ecowitt schema failure
fn ecowitt_invalid_payload() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "ecowitt_invalid_payload".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ECOWITT_LOCAL_LIVE_ADAPTER_VERSION, EcowittAdmittedLocalDate, EcowittBatteryState,
        EcowittCanonicalMetadataAdapter, EcowittCanonicalMetadataMeasurement,
        EcowittCommonReadings, EcowittGatewayReadings, EcowittLivePayload, EcowittLiveRequest,
        EcowittMeasurement, EcowittParticulateSensor, EcowittPreviousCursor,
        EcowittProviderMetadata, EcowittRainGaugeKind, EcowittRainReadings, EcowittSoilSensor,
        EcowittSourceKind, EcowittTemperatureSensor, EcowittTimeAdapter, EcowittUnit,
        MAXIMUM_ECOWITT_PROVIDER_METADATA_JS_STRING_UNITS, NormalizedEcowittBatch,
        normalize_ecowitt_live,
    };
    use crate::TaskFailure;

    #[derive(Debug)]
    struct ExactTimeAdapter {
        calls: usize,
        expected_received_at_ms: i64,
        expected_timezone: String,
        local_date: EcowittAdmittedLocalDate,
    }

    impl EcowittTimeAdapter for ExactTimeAdapter {
        // resolve only the exact admitted fixture input
        fn local_date(
            &mut self,
            received_at_ms: i64,
            timezone: &str,
        ) -> Result<EcowittAdmittedLocalDate, TaskFailure> {
            self.calls += 1;

            // fail instead of silently accepting a different time binding
            if received_at_ms != self.expected_received_at_ms || timezone != self.expected_timezone
            {
                return Err(test_adapter_failure("time_adapter_input_mismatch"));
            }

            Ok(self.local_date)
        }
    }

    #[derive(Debug)]
    struct RecordingMetadataAdapter {
        calls: usize,
        js_string_units: usize,
        observed: Vec<EcowittProviderMetadata>,
    }

    impl EcowittCanonicalMetadataAdapter for RecordingMetadataAdapter {
        // measure the exact constructed metadata through the admitted seam
        fn measure_provider_metadata(
            &mut self,
            metadata: &EcowittProviderMetadata,
        ) -> Result<EcowittCanonicalMetadataMeasurement, TaskFailure> {
            self.calls += 1;
            self.observed.push(metadata.clone());
            Ok(EcowittCanonicalMetadataMeasurement::admitted(
                self.js_string_units,
            ))
        }
    }

    // create one deterministic exact time adapter
    fn exact_time_adapter(
        received_at_ms: i64,
        local_date: EcowittAdmittedLocalDate,
    ) -> ExactTimeAdapter {
        ExactTimeAdapter {
            calls: 0,
            expected_received_at_ms: received_at_ms,
            expected_timezone: "America/Los_Angeles".to_owned(),
            local_date,
        }
    }

    // create one recording canonical metadata adapter
    fn metadata_adapter(js_string_units: usize) -> RecordingMetadataAdapter {
        RecordingMetadataAdapter {
            calls: 0,
            js_string_units,
            observed: Vec::new(),
        }
    }

    // run one fixture through both explicit admission seams
    fn normalize_on_date(
        gateway_mac: &str,
        payload: &EcowittLivePayload,
        request: &EcowittLiveRequest,
        received_at_ms: i64,
        local_date: EcowittAdmittedLocalDate,
        attempts: u32,
    ) -> Result<NormalizedEcowittBatch, TaskFailure> {
        let mut time = exact_time_adapter(received_at_ms, local_date);
        let mut metadata = metadata_adapter(1_163);
        normalize_ecowitt_live(
            gateway_mac,
            payload,
            request,
            received_at_ms,
            attempts,
            &mut time,
            &mut metadata,
        )
    }

    // build one deterministic test-adapter failure
    fn test_adapter_failure(code: &str) -> TaskFailure {
        TaskFailure {
            attempts: 0,
            code: code.to_owned(),
        }
    }

    // build one typed measurement
    const fn measurement(value: f64, unit: EcowittUnit) -> Option<EcowittMeasurement> {
        Some(EcowittMeasurement { unit, value })
    }

    // build the checked node request fixture
    fn fixture_request() -> EcowittLiveRequest {
        EcowittLiveRequest {
            expected_mac: "88:F1:55:05:D8:9F".to_owned(),
            model: "GW3000".to_owned(),
            previous_cursor: None,
            source_id: "source-ecowitt".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }
    }

    // build the checked node local-live fixture
    fn fixture_payload() -> EcowittLivePayload {
        EcowittLivePayload {
            common: EcowittCommonReadings {
                apparent_temperature_fallback: measurement(53.6, EcowittUnit::Fahrenheit),
                apparent_temperature_primary: None,
                black_globe_battery: Some(EcowittBatteryState::Integer(4)),
                black_globe_temperature: measurement(54.3, EcowittUnit::Fahrenheit),
                outdoor_humidity: measurement(96.0, EcowittUnit::Percent),
                outdoor_temperature: measurement(54.3, EcowittUnit::Fahrenheit),
                solar_radiation: measurement(21.94, EcowittUnit::WattPerSquareMeter),
                uv_index: measurement(2.0, EcowittUnit::Unitless),
                wet_bulb_globe_temperature: measurement(53.6, EcowittUnit::Fahrenheit),
                wind_direction: measurement(47.0, EcowittUnit::Unitless),
                wind_gust: measurement(10.0, EcowittUnit::MilePerHour),
                wind_speed: measurement(5.0, EcowittUnit::MilePerHour),
            },
            gateway: Some(EcowittGatewayReadings {
                indoor_humidity: measurement(70.0, EcowittUnit::Percent),
                indoor_temperature: measurement(65.3, EcowittUnit::Fahrenheit),
                relative_pressure: measurement(29.88, EcowittUnit::InchOfMercury),
            }),
            group_count: 6,
            particulate_sensors: vec![EcowittParticulateSensor {
                battery: Some(EcowittBatteryState::Integer(6)),
                channel: 1,
                pm25: measurement(12.4, EcowittUnit::Unitless),
            }],
            piezo_rain: Some(EcowittRainReadings {
                battery: Some(EcowittBatteryState::Integer(3)),
                daily_total: measurement(0.15, EcowittUnit::Inch),
                rate: measurement(0.02, EcowittUnit::InchPerHour),
            }),
            soil_sensors: vec![EcowittSoilSensor {
                battery: Some(EcowittBatteryState::Integer(5)),
                channel: 1,
                conductivity: measurement(10.0, EcowittUnit::MicrosiemensPerCentimeter),
                moisture: measurement(42.0, EcowittUnit::Percent),
                temperature: measurement(63.9, EcowittUnit::Fahrenheit),
            }],
            temperature_sensors: vec![EcowittTemperatureSensor {
                battery: Some(EcowittBatteryState::Integer(0)),
                channel: 1,
                humidity: measurement(65.0, EcowittUnit::Percent),
                temperature: measurement(68.0, EcowittUnit::Fahrenheit),
            }],
            traditional_rain: None,
        }
    }

    // locate one independently scoped property sensor
    fn sensor<'a>(
        record: &'a super::NormalizedEcowittRecord,
        key: &str,
    ) -> &'a super::EcowittSensorSnapshot {
        record
            .provider
            .property_sensors
            .iter()
            .find(|sensor| sensor.key == key)
            .expect("sensor should exist")
    }

    // match the compiled node core and property-sensor fixture
    #[test]
    fn ecowitt_fixture_preserves_units_provenance_and_operator_scopes() {
        let batch = normalize_on_date(
            "88:f1:55:05:d8:9f",
            &fixture_payload(),
            &fixture_request(),
            1_788_012_000_000,
            EcowittAdmittedLocalDate::admitted(2026, 8, 29),
            2,
        )
        .expect("fixture should normalize");

        assert_eq!(batch.adapter_version, ECOWITT_LOCAL_LIVE_ADAPTER_VERSION);
        assert_eq!(batch.attempts, 2);
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.provider_cursor.rain_day, "2026-08-29");
        assert!((batch.provider_cursor.rain_daily_total_mm.expect("rain") - 3.81).abs() < 1e-12);
        assert_eq!(
            batch.response_metadata.rain_gauge,
            Some(EcowittRainGaugeKind::Piezo)
        );
        assert_eq!(batch.response_metadata.soil_channel_count, 1);
        let record = &batch.records[0];
        assert_eq!(record.valid_at_ms, 1_788_012_000_000);
        assert_eq!(record.source_kind, EcowittSourceKind::PhysicalSensor);
        assert!(
            (record.metrics.temperature_c.expect("temperature") - 12.388_888_888_888_89).abs()
                < 1e-12
        );
        assert!((record.metrics.apparent_temperature_c.expect("apparent") - 12.0).abs() < 1e-12);
        assert_eq!(record.metrics.relative_humidity_percent, Some(96.0));
        assert!(
            (record.metrics.pressure_hpa.expect("pressure") - 1_011.852_933_600_996).abs() < 1e-9
        );
        assert!((record.metrics.wind_speed_mps.expect("wind") - 2.2352).abs() < 1e-12);
        assert!((record.metrics.wind_gust_mps.expect("gust") - 4.4704).abs() < 1e-12);
        assert_eq!(record.metrics.wind_direction_degrees, Some(47.0));
        assert!((record.metrics.precipitation_mm.expect("rain") - 3.81).abs() < 1e-12);
        assert!(
            (record.metrics.precipitation_rate_mm_per_hour.expect("rate") - 0.508).abs() < 1e-12
        );
        assert_eq!(record.metrics.solar_radiation_wm2, Some(21.94));
        assert_eq!(record.metrics.uv_index, Some(2.0));
        assert_eq!(record.metrics.pm25_micrograms_per_cubic_meter, Some(12.4));
        assert_eq!(record.metrics.soil_moisture_percent, Some(42.0));
        assert_eq!(
            record
                .metrics
                .soil_electrical_conductivity_microsiemens_per_cm,
            Some(10.0)
        );
        assert_eq!(record.device.model, "GW3000");
        assert_eq!(record.device.serial, "88:F1:55:05:D8:9F");
        assert_eq!(record.device.vendor, "Ecowitt");
        assert_eq!(record.quality.sampling, "local_gateway_poll");
        assert_eq!(record.upstream_timezone, "America/Los_Angeles");
        assert_eq!(
            record
                .provider
                .property_sensors
                .iter()
                .map(|sensor| sensor.key.as_str())
                .collect::<Vec<_>>(),
            [
                "gateway",
                "weather-array",
                "black-globe",
                "temperature-1",
                "soil-1",
                "air-quality-1",
            ],
        );
        assert_eq!(sensor(record, "gateway").readings.temperature_c, Some(18.5));
        assert_eq!(
            sensor(record, "temperature-1").readings.temperature_c,
            Some(20.0)
        );
        assert_eq!(sensor(record, "soil-1").readings.battery_level, Some(5.0));
        assert_eq!(
            sensor(record, "air-quality-1")
                .readings
                .battery_external_power,
            Some(1.0),
        );
    }

    // retain daily-counter delta reset and new-day semantics
    #[test]
    fn ecowitt_cursor_preserves_interval_rain_semantics() {
        let payload = fixture_payload();
        let mut request = fixture_request();
        request.previous_cursor = Some(EcowittPreviousCursor {
            rain_daily_total_mm: Some(2.54),
            rain_day: Some("2026-08-29".to_owned()),
        });
        let delta = normalize_on_date(
            "88:F1:55:05:D8:9F",
            &payload,
            &request,
            1_788_012_000_000,
            EcowittAdmittedLocalDate::admitted(2026, 8, 29),
            1,
        )
        .expect("same-day cursor should normalize");
        assert!((delta.records[0].metrics.precipitation_mm.expect("delta") - 1.27).abs() < 1e-9);

        request.previous_cursor = Some(EcowittPreviousCursor {
            rain_daily_total_mm: Some(5.0),
            rain_day: Some("2026-08-29".to_owned()),
        });
        let reset = normalize_on_date(
            "88:F1:55:05:D8:9F",
            &payload,
            &request,
            1_788_012_000_000,
            EcowittAdmittedLocalDate::admitted(2026, 8, 29),
            1,
        )
        .expect("counter reset should normalize");
        assert_eq!(reset.records[0].metrics.precipitation_mm, None);

        request.previous_cursor = Some(EcowittPreviousCursor {
            rain_daily_total_mm: Some(5.0),
            rain_day: Some("2026-08-28".to_owned()),
        });
        let new_day = normalize_on_date(
            "88:F1:55:05:D8:9F",
            &payload,
            &request,
            1_788_012_000_000,
            EcowittAdmittedLocalDate::admitted(2026, 8, 29),
            1,
        )
        .expect("new day should reseed");
        assert!((new_day.records[0].metrics.precipitation_mm.expect("seed") - 3.81).abs() < 1e-12);
    }

    // bind the daily rain cursor to the admitted local-midnight transition
    #[test]
    fn ecowitt_rain_day_uses_exact_received_instant_and_timezone() {
        let payload = fixture_payload();
        let mut request = fixture_request();
        request.previous_cursor = Some(EcowittPreviousCursor {
            rain_daily_total_mm: Some(2.54),
            rain_day: Some("2026-08-28".to_owned()),
        });

        let mut before_time = exact_time_adapter(
            1_787_986_799_000,
            EcowittAdmittedLocalDate::admitted(2026, 8, 28),
        );
        let mut before_metadata = metadata_adapter(1_163);
        let before = normalize_ecowitt_live(
            "88:F1:55:05:D8:9F",
            &payload,
            &request,
            1_787_986_799_000,
            1,
            &mut before_time,
            &mut before_metadata,
        )
        .expect("instant before local midnight should retain the prior day");
        assert_eq!(before_time.calls, 1);
        assert_eq!(before.provider_cursor.rain_day, "2026-08-28");
        assert!(
            (before.records[0]
                .metrics
                .precipitation_mm
                .expect("same-day delta")
                - 1.27)
                .abs()
                < 1e-9
        );

        let mut after_time = exact_time_adapter(
            1_787_986_800_000,
            EcowittAdmittedLocalDate::admitted(2026, 8, 29),
        );
        let mut after_metadata = metadata_adapter(1_163);
        let after = normalize_ecowitt_live(
            "88:F1:55:05:D8:9F",
            &payload,
            &request,
            1_787_986_800_000,
            1,
            &mut after_time,
            &mut after_metadata,
        )
        .expect("instant at local midnight should start the admitted new day");
        assert_eq!(after_time.calls, 1);
        assert_eq!(after.provider_cursor.rain_day, "2026-08-29");
        assert!(
            (after.records[0]
                .metrics
                .precipitation_mm
                .expect("new-day seed")
                - 3.81)
                .abs()
                < 1e-12
        );
    }

    // admit the exact Node metadata boundary and reject the next string unit
    #[test]
    fn ecowitt_requires_canonical_measurement_of_actual_metadata() {
        let payload = fixture_payload();
        let request = fixture_request();
        let mut admitted_time = exact_time_adapter(
            1_788_012_000_000,
            EcowittAdmittedLocalDate::admitted(2026, 8, 29),
        );
        let mut admitted_metadata =
            metadata_adapter(MAXIMUM_ECOWITT_PROVIDER_METADATA_JS_STRING_UNITS);
        let admitted = normalize_ecowitt_live(
            "88:F1:55:05:D8:9F",
            &payload,
            &request,
            1_788_012_000_000,
            1,
            &mut admitted_time,
            &mut admitted_metadata,
        )
        .expect("the exact canonical metadata boundary should be admitted");

        assert_eq!(admitted.records.len(), 1);
        assert_eq!(admitted_metadata.calls, 1);
        assert_eq!(admitted_metadata.observed.len(), 1);
        assert_eq!(
            admitted_metadata.observed[0], admitted.records[0].provider,
            "the adapter must measure the actual complete provider metadata",
        );
        assert_eq!(admitted_metadata.observed[0].dataset, "get_livedata_info");
        assert_eq!(admitted_metadata.observed[0].device_id, "88:F1:55:05:D8:9F");
        assert_eq!(admitted_metadata.observed[0].property_sensors.len(), 6);

        let mut rejected_time = exact_time_adapter(
            1_788_012_000_000,
            EcowittAdmittedLocalDate::admitted(2026, 8, 29),
        );
        let mut rejected_metadata =
            metadata_adapter(MAXIMUM_ECOWITT_PROVIDER_METADATA_JS_STRING_UNITS + 1);
        let rejected = normalize_ecowitt_live(
            "88:F1:55:05:D8:9F",
            &payload,
            &request,
            1_788_012_000_000,
            1,
            &mut rejected_time,
            &mut rejected_metadata,
        )
        .expect_err("metadata beyond the canonical boundary must not return a record");

        assert_eq!(rejected.code, "ecowitt_invalid_payload");
        assert_eq!(rejected_metadata.calls, 1);
        assert_eq!(rejected_metadata.observed.len(), 1);
        assert_eq!(
            rejected_metadata.observed[0].property_sensors.len(),
            admitted.records[0].provider.property_sensors.len(),
        );
    }

    // preserve hardware scope and battery family boundaries
    #[test]
    fn ecowitt_traditional_rain_and_batteries_remain_independent() {
        let mut payload = fixture_payload();
        payload.traditional_rain = Some(EcowittRainReadings {
            battery: Some(EcowittBatteryState::Integer(2)),
            daily_total: measurement(0.10, EcowittUnit::Inch),
            rate: measurement(0.01, EcowittUnit::InchPerHour),
        });
        payload.temperature_sensors[0].battery = Some(EcowittBatteryState::Integer(6));
        payload.soil_sensors[0].battery = Some(EcowittBatteryState::Unrecognized);
        let batch = normalize_on_date(
            "88:F1:55:05:D8:9F",
            &payload,
            &fixture_request(),
            1_788_012_000_000,
            EcowittAdmittedLocalDate::admitted(2026, 8, 29),
            1,
        )
        .expect("traditional rain should normalize");
        let record = &batch.records[0];
        assert_eq!(
            batch.response_metadata.rain_gauge,
            Some(EcowittRainGaugeKind::Traditional)
        );
        assert_eq!(
            sensor(record, "weather-array")
                .readings
                .daily_precipitation_mm,
            None
        );
        assert_eq!(
            sensor(record, "rain-gauge").readings.daily_precipitation_mm,
            Some(2.54)
        );
        assert_eq!(
            sensor(record, "rain-gauge").readings.battery_level,
            Some(2.0)
        );
        assert_eq!(sensor(record, "temperature-1").readings.battery_low, None);
        assert_eq!(sensor(record, "soil-1").readings.battery_level, None);
        assert_eq!(
            sensor(record, "air-quality-1")
                .readings
                .battery_external_power,
            Some(1.0),
        );
    }

    // preserve a valid gateway snapshot with missing optional sensors
    #[test]
    fn ecowitt_missing_values_remain_null_without_losing_the_record() {
        let batch = normalize_on_date(
            "88:F1:55:05:D8:9F",
            &EcowittLivePayload {
                group_count: 1,
                ..EcowittLivePayload::default()
            },
            &fixture_request(),
            1_788_012_000_000,
            EcowittAdmittedLocalDate::admitted(2026, 8, 29),
            1,
        )
        .expect("missing optional values should normalize");

        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.provider_cursor.rain_daily_total_mm, None);
        assert_eq!(batch.response_metadata.rain_gauge, None);
        let record = &batch.records[0];
        assert_eq!(record.metrics.temperature_c, None);
        assert_eq!(record.metrics.precipitation_mm, None);
        assert_eq!(record.metrics.pressure_hpa, None);
        assert_eq!(record.metrics.wind_speed_mps, None);
        assert_eq!(record.provider.property_sensors.len(), 1);
        assert_eq!(record.provider.property_sensors[0].key, "weather-array");
        assert_eq!(
            record.provider.property_sensors[0].readings,
            super::EcowittSensorReadings::default(),
        );
    }

    // fail closed on provider source and unit drift
    #[test]
    fn ecowitt_rejects_gateway_identity_and_unit_drift() {
        let wrong_gateway = normalize_on_date(
            "00:11:22:33:44:55",
            &fixture_payload(),
            &fixture_request(),
            1_788_012_000_000,
            EcowittAdmittedLocalDate::admitted(2026, 8, 29),
            1,
        )
        .expect_err("gateway drift should fail");
        assert_eq!(wrong_gateway.code, "ecowitt_invalid_payload");

        let mut invalid_unit = fixture_payload();
        invalid_unit.common.wind_speed = measurement(5.0, EcowittUnit::Unsupported);
        let unit_failure = normalize_on_date(
            "88:F1:55:05:D8:9F",
            &invalid_unit,
            &fixture_request(),
            1_788_012_000_000,
            EcowittAdmittedLocalDate::admitted(2026, 8, 29),
            1,
        )
        .expect_err("unit drift should fail");
        assert_eq!(unit_failure.code, "ecowitt_invalid_payload");
    }
}
