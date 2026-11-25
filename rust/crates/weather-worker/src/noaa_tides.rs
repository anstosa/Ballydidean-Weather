//! Typed NOAA observation and prediction normalization without transport or date codecs.

use std::collections::BTreeSet;

use super::{CanonicalWeatherMetrics, ScheduledProviderRecord, TaskFailure, normalize_metric};

pub const NOAA_TIDE_OBSERVATION_ADAPTER_VERSION: &str = "noaa-water-level/v1";
pub const NOAA_TIDE_PREDICTION_ADAPTER_VERSION: &str = "noaa-tide-predictions/v1";
pub const NOAA_TIDE_CHUNK_PLAN_VERSION: &str = "noaa-calendar-range/v1";
pub const NOAA_TIDE_DATA_ENDPOINT: &str = "noaa-co-ops/datagetter";
pub const MAXIMUM_NOAA_OBSERVATION_RANGE_SECONDS: u64 = 31 * 24 * 60 * 60;
pub const MAXIMUM_NOAA_PREDICTION_RANGE_SECONDS: u64 = 366 * 24 * 60 * 60;
/// Maximum cadence accepted by the checked tide catalog.
pub const NOAA_SCHEDULED_MAXIMUM_CADENCE_SECONDS: u64 = 86_400;
/// Minimum cadence accepted by the checked tide catalog.
pub const NOAA_SCHEDULED_MINIMUM_CADENCE_SECONDS: u64 = 360;
/// Delay that keeps scheduled observations behind NOAA's verified-data clock.
pub const NOAA_SCHEDULED_OBSERVATION_LAG_MILLISECONDS: i64 = 12 * 60_000;
/// Fixed scheduled prediction horizon from the checked adapter contract.
pub const NOAA_SCHEDULED_PREDICTION_DAYS: i64 = 30;

const MILLISECONDS_PER_MINUTE: i64 = 60_000;

/// One frozen NOAA tide source kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoaaTideSourceKind {
    Observation,
    Prediction,
}

/// One frozen NOAA product identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoaaTideProduct {
    Predictions,
    WaterLevel,
}

/// One typed NOAA range after UTC timestamps were resolved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoaaTideRangeRequest {
    pub end_exclusive_ms: i64,
    pub source_id: String,
    pub source_kind: NoaaTideSourceKind,
    pub start_ms: i64,
    pub station_id: String,
    pub timezone: String,
}

/// One capability admitted from the loaded tide runtime contract.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NoaaTideCapability {
    Current,
    Forecast,
    Historical,
}

/// One scheduled NOAA database identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoaaTideScheduledSource {
    pub active: bool,
    pub cadence_seconds: u64,
    pub id: String,
    pub material_config_identity: String,
    pub provider_key: String,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub source_kind: NoaaTideSourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// One loaded NOAA station and source configuration identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoaaTideRuntimeConfiguration {
    pub adapter_contract_version: &'static str,
    pub capabilities: BTreeSet<NoaaTideCapability>,
    pub forecast_days: Option<i64>,
    pub material_config_identity: String,
    pub maximum_chunk_days: u16,
    pub provider_key: String,
    pub site_slug: String,
    pub source_active: bool,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub source_kind: NoaaTideSourceKind,
    pub station_active: bool,
    pub station_id: String,
    pub station_slug: String,
    pub timezone: String,
}

/// Inputs for one admitted scheduled NOAA execution.
#[derive(Clone, Copy, Debug)]
pub struct NoaaTideScheduledExecution<'a> {
    pub configuration: &'a NoaaTideRuntimeConfiguration,
    pub now_ms: i64,
    pub run_deadline_at_ms: i64,
    pub source: &'a NoaaTideScheduledSource,
}

/// One typed NOAA durable provider cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoaaTideProviderCursor {
    pub valid_at_ms: i64,
}

/// One typed verified water-level row.
#[derive(Clone, Debug, PartialEq)]
pub struct NoaaTideObservation {
    pub flags: String,
    pub quality_status: String,
    pub valid_at_ms: i64,
    pub water_level_m: f64,
}

/// One high or low prediction event type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoaaPredictionType {
    High,
    Low,
    Unsupported,
}

/// One typed high-low prediction row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoaaTidePrediction {
    pub prediction_type: NoaaPredictionType,
    pub valid_at_ms: i64,
    pub water_level_m: f64,
}

/// One transport-parsed NOAA payload.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoaaTidePayload {
    pub error_message: Option<String>,
    pub observations: Option<Vec<NoaaTideObservation>>,
    pub predictions: Option<Vec<NoaaTidePrediction>>,
}

/// Stable NOAA station device provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoaaTideDeviceMetadata {
    pub model: &'static str,
    pub serial: String,
    pub vendor: &'static str,
}

/// NOAA row-quality evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoaaTideQualityMetadata {
    HighLowPrediction { sampling: &'static str },
    VerifiedObservation { flags: Vec<String>, status: String },
}

/// NOAA product provenance retained on every record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoaaTideProviderMetadata {
    pub dataset: NoaaTideProduct,
    pub datum: &'static str,
    pub prediction_type: Option<NoaaPredictionType>,
    pub product: NoaaTideProduct,
    pub station_id: String,
}

/// One normalized NOAA observation or prediction record.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedNoaaTideRecord {
    pub device: NoaaTideDeviceMetadata,
    pub metrics: CanonicalWeatherMetrics,
    pub model: Option<String>,
    pub product_run_at_ms: Option<i64>,
    pub provider: NoaaTideProviderMetadata,
    pub quality: NoaaTideQualityMetadata,
    pub received_at_ms: i64,
    pub source_id: String,
    pub source_kind: NoaaTideSourceKind,
    pub upstream_timezone: String,
    pub valid_at_ms: i64,
}

/// Bounded NOAA batch diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoaaTideResponseMetadata {
    pub datum: &'static str,
    pub normalized_record_count: usize,
    pub product: NoaaTideProduct,
    pub provider: &'static str,
    pub station_id: String,
}

/// One normalized NOAA provider batch.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedNoaaTideBatch {
    pub adapter_version: &'static str,
    pub attempts: u32,
    pub provider_cursor_valid_at_ms: Option<i64>,
    pub records: Vec<NormalizedNoaaTideRecord>,
    pub response_metadata: NoaaTideResponseMetadata,
}

impl ScheduledProviderRecord for NormalizedNoaaTideRecord {
    // expose the normalized provider timestamp
    fn valid_at_ms(&self) -> i64 {
        self.valid_at_ms
    }
}

/// Select the frozen adapter for one NOAA source kind.
pub const fn noaa_tide_adapter_version(source_kind: NoaaTideSourceKind) -> &'static str {
    // map observed and predicted water levels
    match source_kind {
        NoaaTideSourceKind::Observation => NOAA_TIDE_OBSERVATION_ADAPTER_VERSION,
        NoaaTideSourceKind::Prediction => NOAA_TIDE_PREDICTION_ADAPTER_VERSION,
    }
}

/// Normalize one typed NOAA observation or prediction range.
pub fn normalize_noaa_tide_range(
    payload: &NoaaTidePayload,
    request: &NoaaTideRangeRequest,
    received_at_ms: i64,
    attempts: u32,
) -> Result<NormalizedNoaaTideBatch, TaskFailure> {
    validate_range(request)?;

    // surface provider-declared failures before reading rows
    if payload.error_message.is_some() {
        return Err(noaa_invalid_payload());
    }

    let product = noaa_product(request.source_kind);

    // dispatch the selected response family
    let records = match request.source_kind {
        NoaaTideSourceKind::Observation => normalize_observations(
            payload
                .observations
                .as_deref()
                .ok_or_else(noaa_invalid_payload)?,
            request,
            received_at_ms,
        )?,
        NoaaTideSourceKind::Prediction => normalize_predictions(
            payload
                .predictions
                .as_deref()
                .ok_or_else(noaa_invalid_payload)?,
            request,
            received_at_ms,
        )?,
    };
    let provider_cursor_valid_at_ms = records.last().map(|record| record.valid_at_ms);

    Ok(NormalizedNoaaTideBatch {
        adapter_version: noaa_tide_adapter_version(request.source_kind),
        attempts,
        provider_cursor_valid_at_ms,
        response_metadata: NoaaTideResponseMetadata {
            datum: "MLLW",
            normalized_record_count: records.len(),
            product,
            provider: "noaa-co-ops",
            station_id: request.station_id.clone(),
        },
        records,
    })
}

// normalize verified observation rows in provider order
fn normalize_observations(
    rows: &[NoaaTideObservation],
    request: &NoaaTideRangeRequest,
    received_at_ms: i64,
) -> Result<Vec<NormalizedNoaaTideRecord>, TaskFailure> {
    let mut records = Vec::with_capacity(rows.len());

    // retain every in-window provider row
    for row in rows {
        validate_row_time(row.valid_at_ms)?;

        // remove inclusive boundary padding
        if row.valid_at_ms < request.start_ms || row.valid_at_ms >= request.end_exclusive_ms {
            continue;
        }

        // require bounded NOAA quality strings
        if !bounded_text(&row.quality_status, 16) || !bounded_text(&row.flags, 128) {
            return Err(noaa_invalid_payload());
        }
        records.push(normalized_record(
            request,
            received_at_ms,
            row.valid_at_ms,
            row.water_level_m,
            None,
            NoaaTideQualityMetadata::VerifiedObservation {
                flags: row.flags.split(',').map(str::to_owned).collect(),
                status: row.quality_status.clone(),
            },
        )?);
    }

    Ok(records)
}

// normalize high-low prediction rows in provider order
fn normalize_predictions(
    rows: &[NoaaTidePrediction],
    request: &NoaaTideRangeRequest,
    received_at_ms: i64,
) -> Result<Vec<NormalizedNoaaTideRecord>, TaskFailure> {
    let mut records = Vec::with_capacity(rows.len());

    // retain every in-window provider row
    for row in rows {
        validate_row_time(row.valid_at_ms)?;

        // remove inclusive boundary padding
        if row.valid_at_ms < request.start_ms || row.valid_at_ms >= request.end_exclusive_ms {
            continue;
        }

        // require one documented event type
        if row.prediction_type == NoaaPredictionType::Unsupported {
            return Err(noaa_invalid_payload());
        }
        records.push(normalized_record(
            request,
            received_at_ms,
            row.valid_at_ms,
            row.water_level_m,
            Some(row.prediction_type),
            NoaaTideQualityMetadata::HighLowPrediction {
                sampling: "high_low_prediction",
            },
        )?);
    }

    Ok(records)
}

// build one canonical NOAA record
fn normalized_record(
    request: &NoaaTideRangeRequest,
    received_at_ms: i64,
    valid_at_ms: i64,
    water_level_m: f64,
    prediction_type: Option<NoaaPredictionType>,
    quality: NoaaTideQualityMetadata,
) -> Result<NormalizedNoaaTideRecord, TaskFailure> {
    let product = noaa_product(request.source_kind);

    // select the exact station model
    let model = match request.source_kind {
        NoaaTideSourceKind::Observation => "Water Level Station",
        NoaaTideSourceKind::Prediction => "Tide Prediction Station",
    };
    let water_level_m = canonical_water_level(water_level_m)?;

    Ok(NormalizedNoaaTideRecord {
        device: NoaaTideDeviceMetadata {
            model,
            serial: request.station_id.clone(),
            vendor: "NOAA CO-OPS",
        },
        metrics: CanonicalWeatherMetrics {
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
            water_level_m: Some(water_level_m),
            wind_direction_degrees: None,
            wind_gust_mps: None,
            wind_speed_mps: None,
            wet_bulb_globe_temperature_c: None,
        },
        model: None,
        product_run_at_ms: None,
        provider: NoaaTideProviderMetadata {
            dataset: product,
            datum: "MLLW",
            prediction_type,
            product,
            station_id: request.station_id.clone(),
        },
        quality,
        received_at_ms,
        source_id: request.source_id.clone(),
        source_kind: request.source_kind,
        upstream_timezone: request.timezone.clone(),
        valid_at_ms,
    })
}

// validate one resolved half-open range
fn validate_range(request: &NoaaTideRangeRequest) -> Result<(), TaskFailure> {
    // require bounded source and parsed timezone identity
    if !bounded_text(&request.source_id, 128)
        || !bounded_text(&request.timezone, 64)
        || !station_id_is_valid(&request.station_id)
    {
        return Err(noaa_invalid_payload());
    }
    let Some(duration_ms) = request.end_exclusive_ms.checked_sub(request.start_ms) else {
        return Err(noaa_invalid_payload());
    };

    // select the admitted range limit
    let maximum_range_seconds = match request.source_kind {
        NoaaTideSourceKind::Observation => MAXIMUM_NOAA_OBSERVATION_RANGE_SECONDS,
        NoaaTideSourceKind::Prediction => MAXIMUM_NOAA_PREDICTION_RANGE_SECONDS,
    };
    let maximum_range_ms = i64::try_from(maximum_range_seconds)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000))
        .ok_or_else(noaa_invalid_payload)?;

    // require an increasing minute-aligned provider range
    if request.start_ms % MILLISECONDS_PER_MINUTE != 0
        || request.end_exclusive_ms % MILLISECONDS_PER_MINUTE != 0
        || duration_ms <= 0
        || duration_ms > maximum_range_ms
    {
        return Err(noaa_invalid_payload());
    }

    Ok(())
}

// validate one resolved NOAA GMT minute
fn validate_row_time(valid_at_ms: i64) -> Result<(), TaskFailure> {
    // retain only exact provider minutes
    if valid_at_ms % MILLISECONDS_PER_MINUTE != 0 {
        return Err(noaa_invalid_payload());
    }

    Ok(())
}

// select one product identity from the source kind
const fn noaa_product(source_kind: NoaaTideSourceKind) -> NoaaTideProduct {
    // map the exact admitted request variant
    match source_kind {
        NoaaTideSourceKind::Observation => NoaaTideProduct::WaterLevel,
        NoaaTideSourceKind::Prediction => NoaaTideProduct::Predictions,
    }
}

// normalize one canonical water level
fn canonical_water_level(value: f64) -> Result<f64, TaskFailure> {
    normalize_metric(Some(value), -20.0, 30.0, false)
        .map_err(|_| noaa_invalid_payload())?
        .ok_or_else(noaa_invalid_payload)
}

// require the documented seven-digit station key
fn station_id_is_valid(value: &str) -> bool {
    value.len() == 7 && value.bytes().all(|byte| byte.is_ascii_digit())
}

// validate one already-parsed bounded string
fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum
}

// create one NOAA schema failure
fn noaa_invalid_payload() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "noaa_invalid_payload".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NOAA_TIDE_OBSERVATION_ADAPTER_VERSION, NOAA_TIDE_PREDICTION_ADAPTER_VERSION,
        NoaaPredictionType, NoaaTideObservation, NoaaTidePayload, NoaaTidePrediction,
        NoaaTideProduct, NoaaTideQualityMetadata, NoaaTideRangeRequest, NoaaTideSourceKind,
        normalize_noaa_tide_range,
    };

    // build the compiled node observation request fixture
    fn observation_request() -> NoaaTideRangeRequest {
        NoaaTideRangeRequest {
            end_exclusive_ms: 1_546_304_400_000,
            source_id: "41".to_owned(),
            source_kind: NoaaTideSourceKind::Observation,
            start_ms: 1_546_300_800_000,
            station_id: "9444900".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }
    }

    // build the compiled node prediction request fixture
    fn prediction_request() -> NoaaTideRangeRequest {
        NoaaTideRangeRequest {
            end_exclusive_ms: 1_546_473_600_000,
            source_id: "42".to_owned(),
            source_kind: NoaaTideSourceKind::Prediction,
            start_ms: 1_546_300_800_000,
            station_id: "9447814".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }
    }

    // match the compiled node verified-observation fixture
    #[test]
    fn noaa_observations_preserve_six_minute_levels_and_quality() {
        let batch = normalize_noaa_tide_range(
            &NoaaTidePayload {
                observations: Some(vec![
                    NoaaTideObservation {
                        flags: "0,0,0,0".to_owned(),
                        quality_status: "v".to_owned(),
                        valid_at_ms: 1_546_300_800_000,
                        water_level_m: 0.770,
                    },
                    NoaaTideObservation {
                        flags: "0,0,0,0".to_owned(),
                        quality_status: "v".to_owned(),
                        valid_at_ms: 1_546_301_160_000,
                        water_level_m: 0.731,
                    },
                ]),
                ..NoaaTidePayload::default()
            },
            &observation_request(),
            1_787_616_000_000,
            2,
        )
        .expect("observation fixture should normalize");

        assert_eq!(batch.adapter_version, NOAA_TIDE_OBSERVATION_ADAPTER_VERSION);
        assert_eq!(batch.attempts, 2);
        assert_eq!(batch.records.len(), 2);
        assert_eq!(batch.records[0].metrics.water_level_m, Some(0.77));
        assert_eq!(batch.records[1].valid_at_ms, 1_546_301_160_000);
        assert_eq!(
            batch.records[0].source_kind,
            NoaaTideSourceKind::Observation
        );
        assert_eq!(batch.records[0].device.model, "Water Level Station");
        assert_eq!(batch.records[0].device.vendor, "NOAA CO-OPS");
        assert_eq!(batch.records[0].provider.datum, "MLLW");
        assert_eq!(
            batch.records[0].provider.product,
            NoaaTideProduct::WaterLevel
        );
        assert_eq!(
            batch.records[0].quality,
            NoaaTideQualityMetadata::VerifiedObservation {
                flags: vec![
                    "0".to_owned(),
                    "0".to_owned(),
                    "0".to_owned(),
                    "0".to_owned()
                ],
                status: "v".to_owned(),
            },
        );
        assert_eq!(batch.provider_cursor_valid_at_ms, Some(1_546_301_160_000));
        assert_eq!(batch.response_metadata.normalized_record_count, 2);
        assert_eq!(batch.response_metadata.provider, "noaa-co-ops");
    }

    // match the compiled node high-low prediction fixture
    #[test]
    fn noaa_predictions_preserve_event_type_and_station_provenance() {
        let batch = normalize_noaa_tide_range(
            &NoaaTidePayload {
                predictions: Some(vec![
                    NoaaTidePrediction {
                        prediction_type: NoaaPredictionType::Low,
                        valid_at_ms: 1_546_312_680_000,
                        water_level_m: 0.317,
                    },
                    NoaaTidePrediction {
                        prediction_type: NoaaPredictionType::High,
                        valid_at_ms: 1_546_342_020_000,
                        water_level_m: 3.271,
                    },
                ]),
                ..NoaaTidePayload::default()
            },
            &prediction_request(),
            1_787_616_000_000,
            1,
        )
        .expect("prediction fixture should normalize");

        assert_eq!(batch.adapter_version, NOAA_TIDE_PREDICTION_ADAPTER_VERSION);
        assert_eq!(batch.records.len(), 2);
        assert_eq!(
            batch.records[0].provider.prediction_type,
            Some(NoaaPredictionType::Low)
        );
        assert_eq!(batch.records[1].metrics.water_level_m, Some(3.271));
        assert_eq!(batch.records[1].source_kind, NoaaTideSourceKind::Prediction);
        assert_eq!(batch.records[1].device.model, "Tide Prediction Station");
        assert_eq!(
            batch.response_metadata.product,
            NoaaTideProduct::Predictions
        );
        assert_eq!(
            batch.records[0].quality,
            NoaaTideQualityMetadata::HighLowPrediction {
                sampling: "high_low_prediction",
            },
        );
    }

    // preserve provider order while removing padded boundary rows
    #[test]
    fn noaa_range_filters_half_open_padding_without_sorting() {
        let batch = normalize_noaa_tide_range(
            &NoaaTidePayload {
                observations: Some(vec![
                    NoaaTideObservation {
                        flags: "0".to_owned(),
                        quality_status: "v".to_owned(),
                        valid_at_ms: 1_546_301_160_000,
                        water_level_m: 0.731,
                    },
                    NoaaTideObservation {
                        flags: "0".to_owned(),
                        quality_status: "v".to_owned(),
                        valid_at_ms: 1_546_300_800_000,
                        water_level_m: 0.770,
                    },
                    NoaaTideObservation {
                        flags: "0".to_owned(),
                        quality_status: "v".to_owned(),
                        valid_at_ms: 1_546_304_400_000,
                        water_level_m: 9.0,
                    },
                ]),
                ..NoaaTidePayload::default()
            },
            &observation_request(),
            1_787_616_000_000,
            1,
        )
        .expect("padded range should normalize");

        assert_eq!(batch.records.len(), 2);
        assert_eq!(batch.records[0].valid_at_ms, 1_546_301_160_000);
        assert_eq!(batch.records[1].valid_at_ms, 1_546_300_800_000);
        assert_eq!(batch.provider_cursor_valid_at_ms, Some(1_546_300_800_000));
    }

    // fail closed on provider range and event contract drift
    #[test]
    fn noaa_rejects_provider_errors_invalid_ranges_and_event_types() {
        let provider_error = normalize_noaa_tide_range(
            &NoaaTidePayload {
                error_message: Some("station unavailable".to_owned()),
                ..NoaaTidePayload::default()
            },
            &observation_request(),
            1_787_616_000_000,
            1,
        )
        .expect_err("provider error should fail");
        assert_eq!(provider_error.code, "noaa_invalid_payload");

        let invalid_range = normalize_noaa_tide_range(
            &NoaaTidePayload {
                observations: Some(Vec::new()),
                ..NoaaTidePayload::default()
            },
            &NoaaTideRangeRequest {
                end_exclusive_ms: 1_546_300_830_000,
                ..observation_request()
            },
            1_787_616_000_000,
            1,
        )
        .expect_err("unaligned range should fail");
        assert_eq!(invalid_range.code, "noaa_invalid_payload");

        let unsupported_prediction = normalize_noaa_tide_range(
            &NoaaTidePayload {
                predictions: Some(vec![NoaaTidePrediction {
                    prediction_type: NoaaPredictionType::Unsupported,
                    valid_at_ms: 1_546_312_680_000,
                    water_level_m: 0.317,
                }]),
                ..NoaaTidePayload::default()
            },
            &prediction_request(),
            1_787_616_000_000,
            1,
        )
        .expect_err("unsupported event should fail");
        assert_eq!(unsupported_prediction.code, "noaa_invalid_payload");
    }
}
