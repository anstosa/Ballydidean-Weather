//! Shared typed normalization for the remaining public station adapters.

use super::{CanonicalWeatherMetrics, ScheduledWindow, TaskFailure, normalize_metric};

use weather_contracts::PublicStationAdapter;

use super::{
    PreparedScheduledRun, PublicStationScheduledSource, ScheduledLastValidPolicy,
    ScheduledProvider, ScheduledRunRepository, ScheduledWindowHistory, SourceRunResult,
    failure_with_code, read_locked_schedule_timing, run_prepared_scheduled_source_with_timing,
    skipped_source_result,
};

use super::public_station_netatmo::{
    NetatmoMeasurementSeries, normalize_netatmo_series_with_diagnostics,
};
use super::public_station_purpleair::{
    PurpleAirDecodedHistory, normalize_purpleair_rows_with_diagnostics,
};
use super::public_station_wunderground::{
    WeatherUndergroundObservation, normalize_weather_underground_rows_with_diagnostics,
};
use super::{ProviderBatch, ScheduledProviderRecord};

const MILLISECONDS_PER_SECOND: i64 = 1_000;
const MAX_JS_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const MAX_JS_DATE_MS: i64 = 8_640_000_000_000_000;

/// A value decoded by the existing provider boundary, without collapsing bad cells to gaps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PublicStationNumber {
    #[default]
    Missing,
    Number(f64),
    Invalid,
}

/// Exact decoded-payload and domain diagnoses retained until the archive owns persistence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublicStationNormalizationError {
    InvalidPayload(String),
    Domain(String),
    Task(TaskFailure),
}

impl PublicStationNormalizationError {
    /// Retain the typed provider response diagnosis without allocating on success.
    pub(super) fn invalid_payload(message: impl Into<String>) -> Self {
        Self::InvalidPayload(message.into())
    }

    /// Access the diagnostic owned by the decoded response validator.
    pub fn message(&self) -> &str {
        match self {
            Self::InvalidPayload(message) | Self::Domain(message) => message,
            Self::Task(failure) => &failure.code,
        }
    }

    /// Keep the preexisting scheduled worker code and attempts contract.
    pub fn into_task_failure(self) -> TaskFailure {
        match self {
            Self::InvalidPayload(_) => public_station_invalid_payload(),
            Self::Domain(_) => public_station_domain_failure(),
            Self::Task(failure) => failure,
        }
    }
}

impl From<TaskFailure> for PublicStationNormalizationError {
    /// Preserve failures from external values that already use the task boundary.
    fn from(value: TaskFailure) -> Self {
        Self::Task(value)
    }
}

/// Canonical measurements and provider quality retained before record materialization.
#[derive(Clone, Debug, PartialEq)]
pub struct PublicStationNormalizedSample {
    pub valid_at_ms: i64,
    pub metrics: CanonicalWeatherMetrics,
    pub quality_status: Option<String>,
    pub quality_flags: Vec<&'static str>,
}

#[derive(Clone, Copy)]
pub(super) enum PublicStationMetric {
    Celsius,
    Fahrenheit,
    PressureHpa,
    HumidityPercent,
    SolarRadiationWm2,
    PrecipitationMm,
    PrecipitationRateMmPerHour,
    Pm25,
    UvIndex,
    WindGustKph,
    WindSpeedKph,
    WindDirection,
}

/// Return all-null canonical metrics for a source with only a partial meter set.
pub(super) const fn empty_public_station_metrics() -> CanonicalWeatherMetrics {
    CanonicalWeatherMetrics {
        apparent_temperature_c: None,
        black_globe_temperature_c: None,
        cloud_cover_percent: None,
        pm25_micrograms_per_cubic_meter: None,
        precipitation_mm: None,
        precipitation_rate_mm_per_hour: None,
        pressure_hpa: None,
        relative_humidity_percent: None,
        soil_electrical_conductivity_microsiemens_per_cm: None,
        soil_moisture_percent: None,
        solar_radiation_wm2: None,
        temperature_c: None,
        uv_index: None,
        water_level_m: None,
        wet_bulb_globe_temperature_c: None,
        wind_direction_degrees: None,
        wind_gust_mps: None,
        wind_speed_mps: None,
    }
}

/// Read one finite cell without interpreting a provider gap as zero.
pub(super) fn public_station_number(
    value: PublicStationNumber,
    field: &str,
) -> Result<Option<f64>, PublicStationNormalizationError> {
    match value {
        PublicStationNumber::Missing => Ok(None),
        PublicStationNumber::Number(value) if value.is_finite() => Ok(Some(value)),
        _ => Err(PublicStationNormalizationError::invalid_payload(format!(
            "{field} must be a finite number or null"
        ))),
    }
}

/// Check the configured provider's exact-second half-open window before using rows.
pub(super) fn public_station_range(
    window: ScheduledWindow,
    max_ms: i64,
) -> Result<(), PublicStationNormalizationError> {
    // ensure the original endpoints can be represented by the Node date boundary
    let supported = -MAX_JS_DATE_MS..=MAX_JS_DATE_MS;
    if !supported.contains(&window.start_ms)
        || !supported.contains(&window.end_exclusive_ms)
        || window.start_ms % MILLISECONDS_PER_SECOND != 0
        || window.end_exclusive_ms % MILLISECONDS_PER_SECOND != 0
        || !window
            .end_exclusive_ms
            .checked_sub(window.start_ms)
            .is_some_and(|duration| (1..=max_ms).contains(&duration))
    {
        return Err(PublicStationNormalizationError::Domain(
            "public-station range is invalid or too large".to_owned(),
        ));
    }
    Ok(())
}

/// Convert the provider's positive safe epoch seconds for in-window comparison.
pub(super) fn public_station_epoch_ms(epoch_seconds: i64) -> Result<i64, TaskFailure> {
    // Node rejects malformed numeric timestamps even for rows outside the time window
    if !(1..=MAX_JS_SAFE_INTEGER).contains(&epoch_seconds) {
        return Err(public_station_invalid_payload());
    }
    epoch_seconds
        .checked_mul(MILLISECONDS_PER_SECOND)
        .ok_or_else(public_station_invalid_payload)
}

/// Apply the canonical unit and bounds used for one public station metric.
pub(super) fn public_station_metric(
    metric: PublicStationMetric,
    value: Option<f64>,
) -> Result<Option<f64>, PublicStationNormalizationError> {
    let (name, min, max, exclude_max) = match metric {
        PublicStationMetric::Celsius | PublicStationMetric::Fahrenheit => {
            ("temperatureC", -100.0, 70.0, false)
        }
        PublicStationMetric::PressureHpa => ("pressureHpa", 100.0, 1_200.0, false),
        PublicStationMetric::HumidityPercent => ("relativeHumidityPercent", 0.0, 100.0, false),
        PublicStationMetric::SolarRadiationWm2 => ("solarRadiationWm2", 0.0, 2_500.0, false),
        PublicStationMetric::PrecipitationMm => ("precipitationMm", 0.0, 2_000.0, false),
        PublicStationMetric::PrecipitationRateMmPerHour => {
            ("precipitationRateMmPerHour", 0.0, 10_000.0, false)
        }
        PublicStationMetric::Pm25 => ("pm25MicrogramsPerCubicMeter", 0.0, 999.0, false),
        PublicStationMetric::UvIndex => ("uvIndex", 0.0, 20.0, false),
        PublicStationMetric::WindGustKph => ("windGustMps", 0.0, 150.0, false),
        PublicStationMetric::WindSpeedKph => ("windSpeedMps", 0.0, 150.0, false),
        PublicStationMetric::WindDirection => ("windDirectionDegrees", 0.0, 360.0, true),
    };
    // classify nonfinite derived values before converting units as the domain does
    if value.is_some_and(|value| !value.is_finite()) {
        return Err(PublicStationNormalizationError::Domain(format!(
            "{name} must be finite"
        )));
    }
    // convert only provider metrics reported in noncanonical units
    let converted = value.map(|value| match metric {
        PublicStationMetric::Fahrenheit => (value - 32.0) * (5.0 / 9.0),
        PublicStationMetric::WindGustKph | PublicStationMetric::WindSpeedKph => value / 3.6,
        _ => value,
    });
    normalize_metric(converted, min, max, exclude_max).map_err(|_| {
        // distinguish the original exclusive maximum from an ordinary metric bounds error
        let message = if exclude_max && converted == Some(max) {
            format!("{name} must be less than {max}")
        } else {
            format!("{name} must be between {min} and {max}")
        };
        PublicStationNormalizationError::Domain(message)
    })
}

/// Match the providers' unavailable-negative and 360-degree-north conventions.
pub(super) fn public_station_direction(
    value: Option<f64>,
) -> Result<Option<f64>, PublicStationNormalizationError> {
    // a negative reading represents unavailable direction rather than an angle
    if value.is_some_and(|value| value < 0.0) {
        return Ok(None);
    }
    // normalize exactly 360 but do not wrap readings above it
    let value = value.map(|value| if value == 360.0 { 0.0 } else { value });
    public_station_metric(PublicStationMetric::WindDirection, value)
}

/// Average two admitted values even when only one channel is available.
pub(super) fn public_station_average(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some((left + right) / 2.0),
        (left, right) => left.or(right),
    }
}

/// Preserve a Node provider payload classification.
pub(super) fn public_station_invalid_payload() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "invalid_payload".to_owned(),
    }
}

/// Match the coordinator's conversion of a plain TS range or metric error.
fn public_station_domain_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "provider_unavailable".to_owned(),
    }
}

/// One product and its configured identity after decoding at the transport boundary.
pub enum SupplementalPublicStationProduct<'a> {
    WeatherUnderground {
        station_id: &'a str,
        days: &'a [Vec<WeatherUndergroundObservation>],
    },
    Netatmo {
        device_id: &'a str,
        modules: &'a [NetatmoMeasurementSeries; 4],
    },
    PurpleAir {
        sensor_index: u64,
        history: &'a PurpleAirDecodedHistory,
    },
}

/// Shared request material already admitted by the configured station catalog.
pub struct SupplementalPublicStationInputs<'a> {
    pub model: Option<&'a str>,
    pub serial: Option<&'a str>,
    pub source_id: &'a str,
    pub timezone: &'a str,
    pub window: ScheduledWindow,
}

/// Provider-specific provenance and quality attached to a canonical observation.
#[derive(Clone, Debug, PartialEq)]
pub struct SupplementalPublicStationRecord {
    pub dataset: &'static str,
    pub device_id: String,
    pub model: Option<String>,
    pub report_interval_minutes: u8,
    pub sampling: &'static str,
    pub serial: Option<String>,
    pub source_id: String,
    pub received_at_ms: i64,
    pub sample: PublicStationNormalizedSample,
    pub timezone: String,
    pub vendor: &'static str,
}

impl ScheduledProviderRecord for SupplementalPublicStationRecord {
    /// Surface the canonical observation time used by the shared checkpoint.
    fn valid_at_ms(&self) -> i64 {
        self.sample.valid_at_ms
    }
}

/// One retained public station provider result, without transport header material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupplementalPublicStationResponseMetadata {
    pub measurement_request_count: Option<usize>,
    pub normalized_record_count: usize,
    pub provider: &'static str,
    pub raw_observation_count: Option<usize>,
    pub resolution_seconds: Option<u16>,
}

struct PublicStationProvenance {
    dataset: &'static str,
    device_id: String,
    provider: &'static str,
    report_interval_minutes: u8,
    sampling: &'static str,
    vendor: &'static str,
}

/// One normalized result ready for the shared scheduled-ingestion lifecycle.
pub type SupplementalPublicStationBatch = ProviderBatch<
    SupplementalPublicStationRecord,
    SupplementalPublicStationResponseMetadata,
    Option<i64>,
>;

/// Normalize decoded responses into the common ingestion batch; the transport owner supplies the
/// admitted SHA-256 of the ordered response digest sequence and physical attempt count.
pub fn normalize_supplemental_public_station_batch(
    input: SupplementalPublicStationInputs<'_>,
    product: SupplementalPublicStationProduct<'_>,
    received_at_ms: i64,
    attempts: u32,
    upstream_response_checksum: String,
) -> Result<SupplementalPublicStationBatch, TaskFailure> {
    normalize_supplemental_public_station_batch_with_diagnostics(
        input,
        product,
        received_at_ms,
        attempts,
        upstream_response_checksum,
    )
    .map_err(PublicStationNormalizationError::into_task_failure)
}

/// Retain decoded response diagnostics for the archive's durable failure boundary.
pub fn normalize_supplemental_public_station_batch_with_diagnostics(
    input: SupplementalPublicStationInputs<'_>,
    product: SupplementalPublicStationProduct<'_>,
    received_at_ms: i64,
    attempts: u32,
    upstream_response_checksum: String,
) -> Result<SupplementalPublicStationBatch, PublicStationNormalizationError> {
    // preserve the admitted source/time contracts before materializing response rows
    if input.source_id.is_empty()
        || input.source_id.encode_utf16().count() > 128
        || input.timezone.is_empty()
        || !(-MAX_JS_DATE_MS..=MAX_JS_DATE_MS).contains(&received_at_ms)
    {
        return Err(public_station_domain_failure().into());
    }
    let (provenance, samples, raw_observation_count, measurement_request_count, resolution_seconds) =
        match product {
            SupplementalPublicStationProduct::WeatherUnderground { station_id, days } => (
                PublicStationProvenance {
                    dataset: "pws-history-all",
                    device_id: station_id.to_owned(),
                    provider: "weather-underground",
                    report_interval_minutes: 5,
                    sampling: "five_minute_provider_observations",
                    vendor: "Ambient Weather",
                },
                normalize_weather_underground_rows_with_diagnostics(
                    days,
                    station_id,
                    input.window,
                )?,
                Some(days.iter().map(Vec::len).sum()),
                None,
                None,
            ),
            SupplementalPublicStationProduct::Netatmo { device_id, modules } => (
                PublicStationProvenance {
                    dataset: "public-getmeasure",
                    device_id: device_id.to_owned(),
                    provider: "netatmo-weathermap",
                    report_interval_minutes: 5,
                    sampling: "five_minute_provider_buckets",
                    vendor: "Netatmo",
                },
                normalize_netatmo_series_with_diagnostics(modules, input.window)?,
                None,
                Some(modules.len()),
                None,
            ),
            SupplementalPublicStationProduct::PurpleAir {
                sensor_index,
                history,
            } => (
                PublicStationProvenance {
                    dataset: "public-map-history",
                    device_id: sensor_index.to_string(),
                    provider: "purpleair",
                    report_interval_minutes: 2,
                    sampling: "every_two_minute_provider_observation",
                    vendor: "PurpleAir",
                },
                normalize_purpleair_rows_with_diagnostics(history, sensor_index, input.window)?,
                Some(history.rows.len()),
                None,
                Some(120),
            ),
        };
    let provider_cursor = samples.last().map(|sample| sample.valid_at_ms);
    let response_metadata = SupplementalPublicStationResponseMetadata {
        measurement_request_count,
        normalized_record_count: samples.len(),
        provider: provenance.provider,
        raw_observation_count,
        resolution_seconds,
    };
    let records = samples
        .into_iter()
        // attach the immutable configured station identity to each measurement
        .map(|sample| SupplementalPublicStationRecord {
            dataset: provenance.dataset,
            device_id: provenance.device_id.clone(),
            model: input.model.map(str::to_owned),
            report_interval_minutes: provenance.report_interval_minutes,
            sampling: provenance.sampling,
            serial: input.serial.map(str::to_owned),
            source_id: input.source_id.to_owned(),
            received_at_ms,
            sample,
            timezone: input.timezone.to_owned(),
            vendor: provenance.vendor,
        })
        .collect();
    Ok(ProviderBatch {
        attempts,
        provider_cursor,
        records,
        response_metadata,
        upstream_response_checksum,
    })
}

/// Provider identity and request fields admitted from a generated public-station catalog.
#[derive(Clone, Eq, PartialEq)]
pub enum SupplementalPublicStationIdentity {
    WeatherUnderground {
        station_id: String,
        public_api_key: String,
    },
    Netatmo {
        device_id: String,
        outdoor_module_id: String,
        rain_module_id: String,
        wind_module_id: String,
    },
    PurpleAir {
        map_version: String,
        sensor_index: u64,
    },
}

impl SupplementalPublicStationIdentity {
    /// Recover the existing TypeScript adapter discriminator.
    pub const fn adapter(&self) -> PublicStationAdapter {
        match self {
            Self::WeatherUnderground { .. } => PublicStationAdapter::WeatherUnderground,
            Self::Netatmo { .. } => PublicStationAdapter::Netatmo,
            Self::PurpleAir { .. } => PublicStationAdapter::Purpleair,
        }
    }
}

/// Return the retained ingestion version and endpoint label for a supplemental provider.
pub const fn supplemental_public_station_contract(
    adapter: PublicStationAdapter,
) -> Option<(&'static str, &'static str)> {
    match adapter {
        PublicStationAdapter::WeatherUnderground => {
            Some(("wunderground-pws-history/v1", "pws/history/all"))
        }
        PublicStationAdapter::Netatmo => Some(("netatmo-public-measures/v1", "getmeasure")),
        PublicStationAdapter::Purpleair => {
            Some(("purpleair-map-history/v1", "sensors/history/csv"))
        }
        PublicStationAdapter::AmbientWeather => None,
    }
}

/// Configured identity retained from the same catalog that admitted the due source.
pub struct SupplementalPublicStationScheduledConfiguration {
    pub active: bool,
    pub supports_current: bool,
    pub identity: SupplementalPublicStationIdentity,
    pub material_config_identity: String,
    pub maximum_chunk_days: u64,
    pub model: Option<String>,
    pub provider_key: String,
    pub serial: Option<String>,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub station_slug: String,
    pub timezone: String,
}

/// A public provider request admitted only after the scheduled run has committed.
pub struct SupplementalPublicStationScheduledRequest {
    pub identity: SupplementalPublicStationIdentity,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub source_id: String,
    pub timezone: String,
    pub window: ScheduledWindow,
}

impl SupplementalPublicStationScheduledRequest {
    /// Borrow the common normalization inputs while retaining request-specific fields.
    pub fn normalization_inputs(&self) -> SupplementalPublicStationInputs<'_> {
        SupplementalPublicStationInputs {
            model: self.model.as_deref(),
            serial: self.serial.as_deref(),
            source_id: &self.source_id,
            timezone: &self.timezone,
            window: self.window,
        }
    }
}

/// Admit a provider's bounded UTC range before any request or token exchange.
pub fn validate_supplemental_public_station_range(
    window: ScheduledWindow,
    maximum_ms: i64,
) -> Result<(), TaskFailure> {
    public_station_range(window, maximum_ms)
        .map_err(PublicStationNormalizationError::into_task_failure)
}

/// Execute a scheduled public station provider through the shared locked ingestion lifecycle.
pub fn run_scheduled_supplemental_public_station_source_after_lock<R, P, C, T>(
    source: &PublicStationScheduledSource,
    configuration: &SupplementalPublicStationScheduledConfiguration,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
    timing: T,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = Option<i64>,
            Record = SupplementalPublicStationRecord,
            Request = SupplementalPublicStationScheduledRequest,
            ResponseMetadata = SupplementalPublicStationResponseMetadata,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
    T: FnOnce(&mut P, i64) -> Result<i64, TaskFailure>,
{
    // retain the loaded catalog identity instead of trusting fields on a due row alone
    if !source.active
        || !configuration.active
        || !configuration.supports_current
        || source.adapter != configuration.identity.adapter()
        || source.material_config_identity != configuration.material_config_identity
        || source.provider_key != configuration.provider_key
        || source.site_slug != configuration.site_slug
        || source.source_config_fingerprint != configuration.source_config_fingerprint
        || source.source_key != configuration.source_key
        || source.station_slug != configuration.station_slug
        || source.timezone != configuration.timezone
    {
        let _source_completed_at_ms = clock();
        return Ok(skipped_source_result(
            &source.id,
            "source is not an active configured scheduled source",
        ));
    }
    let (adapter_version, endpoint) = supplemental_public_station_contract(source.adapter)
        .ok_or_else(|| failure_with_code("source adapter contract must match runtime"))?;
    let maximum_range_seconds = configuration
        .maximum_chunk_days
        .checked_mul(86_400)
        .ok_or_else(|| failure_with_code("public station range overflow"))?;
    run_prepared_scheduled_source_with_timing(
        PreparedScheduledRun {
            adapter_version,
            cadence_seconds: source.cadence_seconds,
            endpoint,
            last_valid_policy: ScheduledLastValidPolicy::FinalRecord,
            maximum_range_seconds: Some(maximum_range_seconds),
            precomputed_window: None,
            source_config_fingerprint: &source.source_config_fingerprint,
            source_id: &source.id,
            window_history: ScheduledWindowHistory::Checkpoint,
        },
        repository,
        provider,
        clock,
        // use one locked start for both absolute budgets and the checkpoint window
        |provider, clock| read_locked_schedule_timing(provider, clock, timing),
        true,
        // these range requests use the checkpoint window rather than its provider cursor
        |_checkpoint, window| SupplementalPublicStationScheduledRequest {
            identity: configuration.identity.clone(),
            model: configuration.model.clone(),
            serial: configuration.serial.clone(),
            source_id: source.id.clone(),
            timezone: configuration.timezone.clone(),
            window,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PurpleAirDecodedHeader, PurpleAirDecodedRow};

    /// Match the archived Node fixture including an excluded end-boundary observation.
    #[test]
    fn preserves_public_station_batch_cursor_and_response_provenance() {
        let window = ScheduledWindow {
            start_ms: 1_787_576_400_000,
            end_exclusive_ms: 1_787_580_000_000,
        };
        let first = PurpleAirDecodedRow {
            row_width_valid: true,
            sensor_index: Some(32_489),
            timestamp_ms: Some(window.start_ms),
            humidity_a: PublicStationNumber::Number(50.0),
            humidity_b: PublicStationNumber::Number(52.0),
            pm25_atm_a: PublicStationNumber::Number(10.0),
            pm25_atm_b: PublicStationNumber::Number(12.0),
            pressure_a: PublicStationNumber::Number(1_000.0),
            pressure_b: PublicStationNumber::Number(1_002.0),
            temperature_a: PublicStationNumber::Number(68.0),
            temperature_b: PublicStationNumber::Missing,
        };
        let end = PurpleAirDecodedRow {
            timestamp_ms: Some(window.end_exclusive_ms),
            ..first.clone()
        };
        let history = PurpleAirDecodedHistory {
            header: PurpleAirDecodedHeader::Valid,
            rows: vec![first, end],
        };
        let batch = normalize_supplemental_public_station_batch(
            SupplementalPublicStationInputs {
                source_id: "source-public",
                model: Some("test-model"),
                serial: Some("test-serial"),
                timezone: "America/Los_Angeles",
                window,
            },
            SupplementalPublicStationProduct::PurpleAir {
                sensor_index: 32_489,
                history: &history,
            },
            1_787_583_600_000,
            2,
            "fixture-ordered-digest".to_owned(),
        )
        .unwrap();

        assert_eq!(batch.attempts, 2);
        assert_eq!(batch.provider_cursor, Some(window.start_ms));
        assert_eq!(batch.upstream_response_checksum, "fixture-ordered-digest");
        assert_eq!(
            batch.response_metadata,
            SupplementalPublicStationResponseMetadata {
                measurement_request_count: None,
                normalized_record_count: 1,
                provider: "purpleair",
                raw_observation_count: Some(2),
                resolution_seconds: Some(120),
            }
        );
        let [record] = batch.records.as_slice() else {
            panic!("the provider end boundary must not become a stored sample");
        };
        assert_eq!(record.model.as_deref(), Some("test-model"));
        assert_eq!(record.serial.as_deref(), Some("test-serial"));
        assert_eq!(record.device_id, "32489");
        assert_eq!(record.dataset, "public-map-history");
        assert_eq!(record.vendor, "PurpleAir");
        assert_eq!(record.report_interval_minutes, 2);
        assert_eq!(
            record.sample.metrics.pm25_micrograms_per_cubic_meter,
            Some(11.0)
        );
        assert_eq!(record.sample.metrics.temperature_c, Some(20.0));
        assert_eq!(
            record.sample.quality_flags,
            [
                "dual_channel_average",
                "uncorrected_sensor_enclosure_temperature",
            ]
        );
    }

    struct ScheduleFixture<'a>(&'a std::cell::RefCell<Vec<&'static str>>);

    type Completion = crate::ScheduledCompletion<
        SupplementalPublicStationRecord,
        SupplementalPublicStationResponseMetadata,
        Option<i64>,
    >;

    impl
        ScheduledRunRepository<
            SupplementalPublicStationRecord,
            SupplementalPublicStationResponseMetadata,
            Option<i64>,
        > for ScheduleFixture<'_>
    {
        type Session = ();

        /// Retain a source session before sampling the locked start.
        fn acquire_source_session(&mut self, source_id: &str) -> Result<Option<()>, TaskFailure> {
            assert_eq!(source_id, "source-public");
            self.0.borrow_mut().push("lock");
            Ok(Some(()))
        }

        /// Use the captured start for expired-row admission.
        fn abandon_expired_runs(&mut self, _: &mut (), now_ms: i64) -> Result<(), TaskFailure> {
            assert_eq!(now_ms, 180_000);
            self.0.borrow_mut().push("abandon");
            Ok(())
        }

        /// Resume from a prior checkpoint without consuming its optional cursor.
        fn scheduled_checkpoint(
            &mut self,
            _: &mut (),
        ) -> Result<Option<crate::ScheduledCheckpoint<Option<i64>>>, TaskFailure> {
            self.0.borrow_mut().push("checkpoint");
            Ok(Some(crate::ScheduledCheckpoint {
                version: 8,
                window_end_exclusive_ms: 120_000,
                provider_cursor: Some(60_000),
            }))
        }

        /// Make the running row visible before a provider request can start.
        fn start_ingestion_run(
            &mut self,
            _: &mut (),
            start: crate::ScheduledRunStart,
        ) -> Result<String, TaskFailure> {
            assert_eq!(start.adapter_version, "purpleair-map-history/v1");
            assert_eq!(start.endpoint, "sensors/history/csv");
            assert_eq!(start.deadline_at_ms, 300_000);
            assert_eq!(
                start.requested_window,
                ScheduledWindow {
                    start_ms: 120_000,
                    end_exclusive_ms: 180_000,
                }
            );
            self.0.borrow_mut().push("start-commit");
            Ok("run-public".to_owned())
        }

        /// Verify the expected version, last observation, and committed range.
        fn complete_scheduled_ingestion(
            &mut self,
            _: &mut (),
            completion: Completion,
        ) -> Result<(), TaskFailure> {
            assert_eq!(completion.run_id, "run-public");
            assert_eq!(completion.expected_checkpoint_version, Some(8));
            assert_eq!(completion.provider_cursor, Some(120_000));
            assert_eq!(completion.last_valid_at_ms, 120_000);
            assert_eq!(completion.window.end_exclusive_ms, 180_000);
            assert_eq!(completion.attempts, 2);
            assert_eq!(completion.record_count, 1);
            self.0.borrow_mut().push("complete-commit");
            Ok(())
        }

        /// Successful fixture work cannot write a failure row.
        fn fail_ingestion_run(
            &mut self,
            _: &mut (),
            _: &str,
            failure: &TaskFailure,
        ) -> Result<(), TaskFailure> {
            panic!("unexpected public station failure: {:?}", failure.code)
        }

        /// Return the source session before taking either completion clock.
        fn release_source_session(&mut self, _: ()) -> Result<(), TaskFailure> {
            self.0.borrow_mut().push("release");
            Ok(())
        }
    }

    impl ScheduledProvider for ScheduleFixture<'_> {
        type Cursor = Option<i64>;
        type Record = SupplementalPublicStationRecord;
        type Request = SupplementalPublicStationScheduledRequest;
        type ResponseMetadata = SupplementalPublicStationResponseMetadata;

        /// Decode a single provider observation using the retained request.
        fn fetch(
            &mut self,
            request: Self::Request,
        ) -> Result<SupplementalPublicStationBatch, TaskFailure> {
            assert_eq!(request.window.start_ms, 120_000);
            assert_eq!(request.window.end_exclusive_ms, 180_000);
            assert_eq!(request.timezone, "UTC");
            // retain the exact configured request even though the transport is a fixture
            let SupplementalPublicStationIdentity::PurpleAir {
                map_version,
                sensor_index,
            } = &request.identity
            else {
                panic!("expected the configured PurpleAir provider");
            };
            assert_eq!(map_version, "3.2.3");
            assert_eq!(*sensor_index, 42);
            self.0.borrow_mut().push("provider-request");
            let history = PurpleAirDecodedHistory {
                header: PurpleAirDecodedHeader::Valid,
                rows: vec![PurpleAirDecodedRow {
                    row_width_valid: true,
                    sensor_index: Some(i64::try_from(*sensor_index).expect("fixture sensor id")),
                    timestamp_ms: Some(120_000),
                    humidity_a: PublicStationNumber::Missing,
                    humidity_b: PublicStationNumber::Missing,
                    pm25_atm_a: PublicStationNumber::Number(10.0),
                    pm25_atm_b: PublicStationNumber::Missing,
                    pressure_a: PublicStationNumber::Missing,
                    pressure_b: PublicStationNumber::Missing,
                    temperature_a: PublicStationNumber::Missing,
                    temperature_b: PublicStationNumber::Missing,
                }],
            };
            normalize_supplemental_public_station_batch(
                request.normalization_inputs(),
                SupplementalPublicStationProduct::PurpleAir {
                    sensor_index: *sensor_index,
                    history: &history,
                },
                180_000,
                2,
                "fixture-response".to_owned(),
            )
        }
    }

    /// Exercise the new scheduled lifecycle through locked request and checkpoint commit.
    #[test]
    fn supplemental_source_commits_before_provider_and_releases_before_success_clock() {
        let source = PublicStationScheduledSource {
            adapter: PublicStationAdapter::Purpleair,
            active: true,
            cadence_seconds: 60,
            id: "source-public".to_owned(),
            material_config_identity: "material".to_owned(),
            provider_key: "purpleair".to_owned(),
            site_slug: "fixture".to_owned(),
            source_config_fingerprint: "fingerprint".to_owned(),
            source_key: "public".to_owned(),
            station_slug: "station".to_owned(),
            timezone: "UTC".to_owned(),
        };
        let configuration = SupplementalPublicStationScheduledConfiguration {
            active: true,
            supports_current: true,
            identity: SupplementalPublicStationIdentity::PurpleAir {
                map_version: "3.2.3".to_owned(),
                sensor_index: 42,
            },
            material_config_identity: source.material_config_identity.clone(),
            maximum_chunk_days: 2,
            model: None,
            serial: None,
            provider_key: source.provider_key.clone(),
            site_slug: source.site_slug.clone(),
            source_config_fingerprint: source.source_config_fingerprint.clone(),
            source_key: source.source_key.clone(),
            station_slug: source.station_slug.clone(),
            timezone: source.timezone.clone(),
        };
        let trace = std::cell::RefCell::new(Vec::new());
        let mut samples = [180_000, 181_000, 182_000].into_iter();
        let result = run_scheduled_supplemental_public_station_source_after_lock(
            &source,
            &configuration,
            &mut ScheduleFixture(&trace),
            &mut ScheduleFixture(&trace),
            &mut || {
                trace.borrow_mut().push("clock");
                samples.next().expect("fixture clock exhausted")
            },
            |_, start| Ok(start + 120_000),
        )
        .unwrap();
        assert_eq!(result.status, crate::SourceRunStatus::Succeeded);
        assert_eq!(result.successful_at_ms, Some(182_000));
        assert_eq!(
            trace.into_inner(),
            [
                "lock",
                "clock",
                "abandon",
                "checkpoint",
                "start-commit",
                "provider-request",
                "complete-commit",
                "release",
                "clock",
                "clock",
            ]
        );
    }
}
