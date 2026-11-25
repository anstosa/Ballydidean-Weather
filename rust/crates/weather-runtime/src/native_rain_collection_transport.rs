//! Native prospective rain-capture HTTP and pacing bindings.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use hyper::Method;
use hyper::header::{HeaderMap, HeaderValue, ORIGIN, RETRY_AFTER, USER_AGENT};
use url::Url;
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_worker::{
    RAIN_COLLECTION_STATIONS, RainCaptureMetadataValue, RainCaptureOutcome, RainCaptureReceipt,
    RainCaptureRequest, RainCollectionPacer, RainCollectionProvider, TaskFailure,
    TempestObservationRequest, normalize_tempest_observations,
};

use crate::maintenance_canonical_json::{exact_keys, field};
use crate::native_outbound_http::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
    NativeOutboundHttpResponse,
};
use crate::{
    NativeWirePrimitives, decode_native_tempest_payload, system_worker_health_milliseconds,
};

const FORECAST_ENDPOINT: &str = "https://single-runs-api.open-meteo.com/v1/forecast";
const TEMPEST_ORIGIN: &str = "https://swd.weatherflow.com";
const TEMPEST_BUILD: &str = "169";
const TEMPEST_REQUEST_ORIGIN: &str = "https://tempestwx.com";
const TEMPEST_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
const CONTRACT_VERSION: &str = "rain-prospective-capture/v1";
const FORECAST_HOURS: usize = 49;
const DECISION_DELAY_MILLISECONDS: i64 = 8 * 3_600_000;
const HOUR_MILLISECONDS: i64 = 3_600_000;
const MAXIMUM_BODY_BYTES: u64 = 2_000_000;
const TIMEOUT_MILLISECONDS: u64 = 45_000;
const STATION_WINDOW_MILLISECONDS: i64 = 2 * HOUR_MILLISECONDS;
const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const EXPECTED_GRID_LATITUDE: f64 = 47.97891;
const EXPECTED_GRID_LONGITUDE: f64 = -122.44185;
const GRID_TOLERANCE: f64 = 0.00001;

const FORECAST_VARIABLES: &[&str] = &[
    "temperature_2m",
    "relative_humidity_2m",
    "wind_speed_10m",
    "precipitation",
    "surface_pressure",
    "cloud_cover",
    "wind_direction_10m",
];
const FORECAST_FIELDS: &[&str] = &[
    "time",
    "temperature_2m",
    "relative_humidity_2m",
    "wind_speed_10m",
    "precipitation",
    "surface_pressure",
    "cloud_cover",
    "wind_direction_10m",
];

/// Shared process transport for prospective forecast and fixed-gauge evidence.
pub struct NativeRainCollectionTransport {
    client: Arc<NativeOutboundHttp>,
    clock: Arc<dyn RainCollectionTransportClock>,
    forecast_endpoint: Url,
    station_api_key: Option<String>,
    tempest_origin: Url,
}

impl NativeRainCollectionTransport {
    /// Start one retained outbound client and retain the optional station credential in memory.
    pub fn new(station_api_key: Option<String>) -> Result<Self, NativeOutboundHttpError> {
        Self::with_client(Arc::new(NativeOutboundHttp::new()?), station_api_key)
    }

    /// Adopt the process outbound client without creating another runtime or connection pool.
    pub fn with_client(
        client: Arc<NativeOutboundHttp>,
        station_api_key: Option<String>,
    ) -> Result<Self, NativeOutboundHttpError> {
        let forecast_endpoint =
            Url::parse(FORECAST_ENDPOINT).map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        let tempest_origin =
            Url::parse(TEMPEST_ORIGIN).map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        Ok(Self {
            client,
            clock: Arc::new(SystemRainCollectionClock),
            forecast_endpoint,
            station_api_key,
            tempest_origin,
        })
    }

    /// Adopt finite fixture endpoints and a deterministic clock without changing production policy.
    #[cfg(test)]
    fn with_test_dependencies(
        client: Arc<NativeOutboundHttp>,
        station_api_key: Option<String>,
        forecast_endpoint: Url,
        tempest_origin: Url,
        clock: Arc<dyn RainCollectionTransportClock>,
    ) -> Self {
        Self {
            client,
            clock,
            forecast_endpoint,
            station_api_key,
            tempest_origin,
        }
    }

    /// Sample and format the capture clock using the canonical millisecond UTC owner.
    fn now(&self) -> Result<(i64, String), TaskFailure> {
        let milliseconds = self
            .clock
            .now_milliseconds()
            .ok_or_else(|| task_failure("rain capture clock unavailable"))?;
        let instant = NativeWirePrimitives
            .format_utc_milliseconds(milliseconds)
            .ok_or_else(|| task_failure("rain capture clock unavailable"))?;
        Ok((milliseconds, instant))
    }

    /// Build one exact credential-free forecast or credential-owned station request.
    fn request(
        &self,
        request: &RainCaptureRequest,
        started_at_ms: i64,
    ) -> Result<NativeOutboundHttpRequest, TaskFailure> {
        // preserve the source-specific request identity and credential boundary
        match request {
            RainCaptureRequest::Forecast {
                attempt,
                run_initialized_at,
                slot_key,
            } => forecast_request(
                self.forecast_endpoint.clone(),
                *attempt,
                run_initialized_at,
                slot_key,
            ),
            RainCaptureRequest::Station {
                end_exclusive,
                slot_key,
                start,
                station_id,
            } => station_request(
                &self.tempest_origin,
                self.station_api_key.as_deref(),
                *station_id,
                start,
                end_exclusive,
                slot_key,
                started_at_ms,
            ),
        }
        .map_err(|_| task_failure("invalid rain capture request"))
    }
}

impl RainCollectionProvider for NativeRainCollectionTransport {
    /// Fetch and validate one complete raw receipt without retrying or exposing credentials.
    fn fetch_rain_capture(
        &mut self,
        request: &RainCaptureRequest,
    ) -> Result<RainCaptureReceipt, TaskFailure> {
        let (started_at_ms, started_at) = self.now()?;
        let outbound = self.request(request, started_at_ms)?;
        let response = self.client.execute(
            outbound,
            Duration::from_millis(TIMEOUT_MILLISECONDS),
            MAXIMUM_BODY_BYTES,
        );

        // preserve incomplete exchanges as body-free transport receipts
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let (_, completed_at) = self.now()?;
                return Ok(transport_receipt(request, started_at, completed_at, &error));
            }
        };
        let (completed_at_ms, completed_at) = self.now()?;
        Ok(response_receipt(
            request,
            started_at,
            completed_at_ms,
            completed_at,
            response,
        ))
    }
}

/// Blocking worker-lane pacer for the policy-owned delay between committed receipts.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeRainCollectionPacer;

impl RainCollectionPacer for NativeRainCollectionPacer {
    /// Sleep for the exact admitted duration without rounding or hidden retry work.
    fn sleep(&mut self, milliseconds: u64) -> Result<(), TaskFailure> {
        thread::sleep(Duration::from_millis(milliseconds));
        Ok(())
    }
}

/// Clock boundary kept injectable for finite transport tests.
trait RainCollectionTransportClock: Send + Sync {
    /// Return the current Unix epoch in whole milliseconds.
    fn now_milliseconds(&self) -> Option<i64>;
}

/// Actual process-clock implementation used by production construction.
struct SystemRainCollectionClock;

impl RainCollectionTransportClock for SystemRainCollectionClock {
    /// Sample the shared worker wall-clock owner.
    fn now_milliseconds(&self) -> Option<i64> {
        system_worker_health_milliseconds()
    }
}

/// Build the original ordered single-run forecast request.
fn forecast_request(
    mut endpoint: Url,
    attempt: u8,
    run_initialized_at: &str,
    slot_key: &str,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    let initialized_ms =
        canonical_utc(run_initialized_at).ok_or(NativeOutboundHttpError::InvalidRequest)?;
    let hour = initialized_ms.div_euclid(HOUR_MILLISECONDS).rem_euclid(24);
    let expected_slot = format!("forecast:{run_initialized_at}:{attempt}");
    // retain only canonical model cycles, declared attempts, and their durable slot identity
    if initialized_ms.rem_euclid(HOUR_MILLISECONDS) != 0
        || !matches!(hour, 0 | 6 | 12 | 18)
        || !matches!(attempt, 1 | 2)
        || slot_key != expected_slot
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    let run = run_initialized_at
        .get(..16)
        .ok_or(NativeOutboundHttpError::InvalidRequest)?;
    endpoint
        .query_pairs_mut()
        .append_pair("latitude", "47.950429954185445")
        .append_pair("longitude", "-122.42797012608193")
        .append_pair("run", run)
        .append_pair("models", "ecmwf_ifs")
        .append_pair("hourly", &FORECAST_VARIABLES.join(","))
        .append_pair("forecast_hours", "49")
        .append_pair("timezone", "GMT")
        .append_pair("temperature_unit", "celsius")
        .append_pair("wind_speed_unit", "ms")
        .append_pair("precipitation_unit", "mm")
        .append_pair("timeformat", "iso8601");
    NativeOutboundHttpRequest::new(Method::GET, endpoint).map(|request| request.redirect_limit(0))
}

/// Build the original credentialed Tempest public-web observation request.
fn station_request(
    origin: &Url,
    api_key: Option<&str>,
    station_id: u64,
    start: &str,
    end_exclusive: &str,
    slot_key: &str,
    started_at_ms: i64,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    let station = RAIN_COLLECTION_STATIONS
        .iter()
        .find(|station| station.location_id == station_id)
        .ok_or(NativeOutboundHttpError::InvalidRequest)?;
    let api_key = api_key.ok_or(NativeOutboundHttpError::InvalidRequest)?;
    let start_ms = canonical_utc(start).ok_or(NativeOutboundHttpError::InvalidRequest)?;
    let end_exclusive_ms =
        canonical_utc(end_exclusive).ok_or(NativeOutboundHttpError::InvalidRequest)?;
    let duration_ms = end_exclusive_ms
        .checked_sub(start_ms)
        .ok_or(NativeOutboundHttpError::InvalidRequest)?;
    // retain the fixed catalog, exact durable identity, and prospective two-hour window
    if api_key.is_empty()
        || api_key.encode_utf16().count() > 256
        || api_key.chars().any(char::is_whitespace)
        || station.device_id == 0
        || station.device_id > MAXIMUM_SAFE_INTEGER
        || slot_key != format!("station:{station_id}:{end_exclusive}")
        || duration_ms <= 0
        || duration_ms > STATION_WINDOW_MILLISECONDS
        || end_exclusive_ms > started_at_ms
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    let mut url = origin
        .join(&format!(
            "/swd/rest/observations/device/{}",
            station.device_id
        ))
        .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
    url.query_pairs_mut()
        .append_pair("api_key", api_key)
        .append_pair("build", TEMPEST_BUILD)
        .append_pair("time_start", &(start_ms / 1_000).to_string())
        .append_pair("time_end", &(end_exclusive_ms / 1_000 - 1).to_string());
    Ok(NativeOutboundHttpRequest::new(Method::GET, url)?
        .header(ORIGIN, HeaderValue::from_static(TEMPEST_REQUEST_ORIGIN))
        .header(USER_AGENT, HeaderValue::from_static(TEMPEST_USER_AGENT))
        .redirect_limit(0))
}

/// Project one fully received response into the durable receipt contract.
fn response_receipt(
    request: &RainCaptureRequest,
    started_at: String,
    completed_at_ms: i64,
    completed_at: String,
    response: NativeOutboundHttpResponse,
) -> RainCaptureReceipt {
    let status = response.status;
    let digest = sha256_hex(&response.body);
    // retain all complete non-success bodies without attempting provider parsing
    if !(200..300).contains(&status) {
        // classify only the two provider pause families specially
        let outcome = match status {
            429 => RainCaptureOutcome::RateLimited,
            401 | 403 => RainCaptureOutcome::Unauthorized,
            _ => RainCaptureOutcome::Invalid,
        };
        // retain Retry-After policy only for an explicit rate limit
        let metadata = if outcome == RainCaptureOutcome::RateLimited {
            retry_after_metadata(&response.headers)
        } else {
            BTreeMap::new()
        };
        return RainCaptureReceipt {
            available_by_decision: forecast_request_kind(request).then_some(false),
            body: Some(response.body),
            body_sha256: Some(digest),
            completed_at,
            error_code: Some(format!("http_{status}")),
            http_status: Some(status),
            metadata,
            outcome,
            parser_version: CONTRACT_VERSION,
            row_count: 0,
            started_at,
        };
    }

    // apply the request-family validator to the exact retained bytes
    let parsed = match request {
        RainCaptureRequest::Forecast {
            run_initialized_at, ..
        } => forecast_metadata(&response.body, run_initialized_at, completed_at_ms),
        RainCaptureRequest::Station {
            end_exclusive,
            start,
            station_id,
            ..
        } => station_metadata(
            &response.body,
            *station_id,
            start,
            end_exclusive,
            completed_at_ms,
        ),
    };
    // retain invalid complete bodies and their source-specific stable category
    // project validation success or failure without discarding the body
    let (available_by_decision, error_code, metadata, outcome, row_count) = match parsed {
        Ok(parsed) => (
            parsed.available_by_decision,
            None,
            parsed.metadata,
            RainCaptureOutcome::Valid,
            parsed.row_count,
        ),
        Err(code) => (
            forecast_request_kind(request).then_some(false),
            Some(code.to_owned()),
            BTreeMap::new(),
            RainCaptureOutcome::Invalid,
            0,
        ),
    };
    RainCaptureReceipt {
        available_by_decision,
        body: Some(response.body),
        body_sha256: Some(digest),
        completed_at,
        error_code,
        http_status: Some(status),
        metadata,
        outcome,
        parser_version: CONTRACT_VERSION,
        row_count,
        started_at,
    }
}

/// Project one incomplete exchange into the original body-free failure shape.
fn transport_receipt(
    request: &RainCaptureRequest,
    started_at: String,
    completed_at: String,
    error: &NativeOutboundHttpError,
) -> RainCaptureReceipt {
    // expose only the three predecessor transport categories
    let error_code = match error {
        NativeOutboundHttpError::Deadline => "timeout",
        NativeOutboundHttpError::BodyTooLarge { .. } => "body_too_large",
        _ => "transport_error",
    };
    RainCaptureReceipt {
        available_by_decision: forecast_request_kind(request).then_some(false),
        body: None,
        body_sha256: None,
        completed_at,
        error_code: Some(error_code.to_owned()),
        http_status: None,
        metadata: BTreeMap::new(),
        outcome: RainCaptureOutcome::TransportError,
        parser_version: CONTRACT_VERSION,
        row_count: 0,
        started_at,
    }
}

/// One successfully validated provider body projection.
struct ParsedCapture {
    available_by_decision: Option<bool>,
    metadata: BTreeMap<String, RainCaptureMetadataValue>,
    row_count: usize,
}

/// Validate and summarize the exact forty-nine-hour forecast schema.
fn forecast_metadata(
    body: &[u8],
    run_initialized_at: &str,
    completed_at_ms: i64,
) -> Result<ParsedCapture, &'static str> {
    std::str::from_utf8(body).map_err(|_| "invalid_json_or_payload")?;
    let root = NativeWirePrimitives
        .decode_json(body)
        .ok_or("invalid_json_or_payload")?;
    let units = field(&root, "hourly_units").ok_or("invalid_json_or_payload")?;
    let hourly = field(&root, "hourly").ok_or("invalid_json_or_payload")?;
    // match the source's generic payload error for non-object nested members
    if !matches!(units, JsonValue::Object(_)) || !matches!(hourly, JsonValue::Object(_)) {
        return Err("invalid_json_or_payload");
    }
    let offset = number(field(&root, "utc_offset_seconds"));
    let timezone = string(field(&root, "timezone"));
    let location_id = field(&root, "location_id");
    // reject unit and field drift before consuming any hourly cell
    if !exact_keys(units, FORECAST_FIELDS)
        || !exact_keys(hourly, FORECAST_FIELDS)
        || offset != Some(0.0)
        || timezone.as_deref() != Some("GMT")
        || location_id.is_some_and(|value| number(Some(value)) != Some(0.0))
    {
        return Err("invalid_forecast_schema");
    }
    let expected_units = [
        ("time", "iso8601"),
        ("temperature_2m", "°C"),
        ("relative_humidity_2m", "%"),
        ("wind_speed_10m", "m/s"),
        ("precipitation", "mm"),
        ("surface_pressure", "hPa"),
        ("cloud_cover", "%"),
        ("wind_direction_10m", "°"),
    ];
    // require the exact unit and cardinality for every requested member
    for (name, unit) in expected_units {
        if string(field(units, name)).as_deref() != Some(unit)
            || array(field(hourly, name)).is_none_or(|values| values.len() != FORECAST_HOURS)
        {
            return Err("invalid_forecast_schema");
        }
    }
    let latitude = number(field(&root, "latitude")).ok_or("invalid_forecast_grid")?;
    let longitude = number(field(&root, "longitude")).ok_or("invalid_forecast_grid")?;
    let elevation = number(field(&root, "elevation")).ok_or("invalid_forecast_grid")?;
    // preserve only the fixed provider grid and a physically bounded elevation
    if !latitude.is_finite()
        || (latitude - EXPECTED_GRID_LATITUDE).abs() > GRID_TOLERANCE
        || !longitude.is_finite()
        || (longitude - EXPECTED_GRID_LONGITUDE).abs() > GRID_TOLERANCE
        || !elevation.is_finite()
        || !(-1_000.0..=10_000.0).contains(&elevation)
    {
        return Err("invalid_forecast_grid");
    }
    let initialized_ms = canonical_utc(run_initialized_at).ok_or("invalid_json_or_payload")?;
    let times = array(field(hourly, "time")).ok_or("invalid_forecast_schema")?;
    // bind each returned provider hour to the requested model run
    for (lead, value) in times.iter().enumerate() {
        let lead_ms = i64::try_from(lead)
            .ok()
            .and_then(|lead| lead.checked_mul(HOUR_MILLISECONDS))
            .and_then(|lead| initialized_ms.checked_add(lead))
            .ok_or("invalid_json_or_payload")?;
        let expected = NativeWirePrimitives
            .format_utc_milliseconds(lead_ms)
            .and_then(|value| value.get(..16).map(str::to_owned))
            .ok_or("invalid_json_or_payload")?;
        if string(Some(value)).as_deref() != Some(expected.as_str()) {
            return Err("invalid_forecast_grid");
        }
    }
    let bounds = [
        ("temperature_2m", -100.0, 70.0),
        ("relative_humidity_2m", 0.0, 100.0),
        ("wind_speed_10m", 0.0, 150.0),
        ("precipitation", 0.0, 2_000.0),
        ("surface_pressure", 100.0, 1_200.0),
        ("cloud_cover", 0.0, 100.0),
        ("wind_direction_10m", 0.0, 360.0),
    ];
    let mut null_count = 0_usize;
    // validate every numeric cell without replacing provider nulls
    for (name, minimum, maximum) in bounds {
        let values = array(field(hourly, name)).ok_or("invalid_forecast_schema")?;
        // count missing cells and reject nonphysical values in place
        for value in values {
            match value {
                JsonValue::Null => null_count += 1,
                JsonValue::Number(value)
                    if value.is_finite() && (minimum..=maximum).contains(value) => {}
                _ => return Err("invalid_forecast_cell"),
            }
        }
    }
    let decision_at_ms = initialized_ms
        .checked_add(DECISION_DELAY_MILLISECONDS)
        .ok_or("invalid_json_or_payload")?;
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "runInitializedAt".to_owned(),
        RainCaptureMetadataValue::String(run_initialized_at.to_owned()),
    );
    metadata.insert(
        "returnedLatitude".to_owned(),
        RainCaptureMetadataValue::Number(latitude),
    );
    metadata.insert(
        "returnedLongitude".to_owned(),
        RainCaptureMetadataValue::Number(longitude),
    );
    metadata.insert(
        "forecastElevationM".to_owned(),
        RainCaptureMetadataValue::Number(elevation),
    );
    metadata.insert(
        "nullCount".to_owned(),
        RainCaptureMetadataValue::Number(null_count as f64),
    );
    metadata.insert(
        "leadCount".to_owned(),
        RainCaptureMetadataValue::Number((FORECAST_HOURS - 1) as f64),
    );
    Ok(ParsedCapture {
        available_by_decision: Some(completed_at_ms <= decision_at_ms),
        metadata,
        row_count: FORECAST_HOURS - 1,
    })
}

/// Validate and summarize one exact fixed-gauge Tempest response.
fn station_metadata(
    body: &[u8],
    station_id: u64,
    start: &str,
    end_exclusive: &str,
    completed_at_ms: i64,
) -> Result<ParsedCapture, &'static str> {
    std::str::from_utf8(body).map_err(|_| "invalid_json_or_payload")?;
    let station = RAIN_COLLECTION_STATIONS
        .iter()
        .find(|station| station.location_id == station_id)
        .ok_or("invalid_json_or_payload")?;
    let start_ms = canonical_utc(start).ok_or("invalid_json_or_payload")?;
    let end_exclusive_ms = canonical_utc(end_exclusive).ok_or("invalid_json_or_payload")?;
    let value = NativeWirePrimitives
        .decode_json(body)
        .ok_or("invalid_json_or_payload")?;
    reject_duplicate_station_intervals(&value)?;
    let envelope = decode_native_tempest_payload(body).map_err(|_| "invalid_json_or_payload")?;
    let request = TempestObservationRequest {
        device_id: station.device_id,
        end_exclusive_ms,
        location_id: station.location_id,
        serial: station.serial.to_owned(),
        source_id: format!("rain-prospective-tempest-{}", station.location_id),
        start_ms,
        timezone: "America/Los_Angeles".to_owned(),
    };
    let normalized = normalize_tempest_observations(envelope, &request, completed_at_ms, 1)
        .map_err(|_| "invalid_json_or_payload")?;
    // enforce the prospective interval policy after the shared exact normalization
    for record in &normalized.records {
        let interval = record.provider.report_interval_minutes;
        let precipitation = record.metrics.precipitation_mm;
        if record.valid_at_ms > completed_at_ms
            || record.valid_at_ms.rem_euclid(60_000) != 0
            || interval.is_none_or(|value| value.fract() != 0.0 || !(1.0..=5.0).contains(&value))
            || precipitation.is_some_and(|value| !(0.0..=500.0).contains(&value))
        {
            return Err("invalid_station_interval");
        }
    }
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "stationId".to_owned(),
        RainCaptureMetadataValue::Number(station.location_id as f64),
    );
    metadata.insert(
        "deviceId".to_owned(),
        RainCaptureMetadataValue::Number(station.device_id as f64),
    );
    metadata.insert(
        "intervalCount".to_owned(),
        RainCaptureMetadataValue::Number(normalized.records.len() as f64),
    );
    metadata.insert(
        "firstIntervalAt".to_owned(),
        normalized
            .records
            .first()
            .map_or(RainCaptureMetadataValue::Null, |record| {
                station_interval_metadata(record.valid_at_ms)
            }),
    );
    metadata.insert(
        "lastIntervalAt".to_owned(),
        normalized
            .records
            .last()
            .map_or(RainCaptureMetadataValue::Null, |record| {
                station_interval_metadata(record.valid_at_ms)
            }),
    );
    Ok(ParsedCapture {
        available_by_decision: None,
        metadata,
        row_count: normalized.records.len(),
    })
}

/// Reject duplicate integer timestamps before strict Tempest row decoding.
fn reject_duplicate_station_intervals(value: &JsonValue) -> Result<(), &'static str> {
    let Some(JsonValue::Array(observations)) = field(value, "obs") else {
        return Ok(());
    };
    let mut seen = BTreeSet::new();
    // match the source preflight by skipping malformed rows until the strict decoder runs
    for observation in observations {
        let JsonValue::Array(cells) = observation else {
            continue;
        };
        let Some(JsonValue::Number(timestamp)) = cells.first() else {
            continue;
        };
        // compare every finite integer using JavaScript Set's normalized zero identity
        if timestamp.is_finite() && timestamp.fract() == 0.0 {
            // treat positive and negative zero as one JavaScript numeric key
            let identity = if *timestamp == 0.0 {
                0
            } else {
                timestamp.to_bits()
            };
            // stop on the first duplicate before strict payload validation
            if !seen.insert(identity) {
                return Err("duplicate_station_interval");
            }
        }
    }
    Ok(())
}

/// Format one already-admitted station interval for receipt metadata.
fn station_interval_metadata(milliseconds: i64) -> RainCaptureMetadataValue {
    NativeWirePrimitives
        .format_utc_milliseconds(milliseconds)
        .map_or(
            RainCaptureMetadataValue::Null,
            RainCaptureMetadataValue::String,
        )
}

/// Retain only bounded Retry-After seconds or require an operator resume decision.
fn retry_after_metadata(headers: &HeaderMap) -> BTreeMap<String, RainCaptureMetadataValue> {
    let mut metadata = BTreeMap::new();
    let mut values = headers.get_all(RETRY_AFTER).iter();
    let value = values.next();
    let duplicate = values.next().is_some();
    // preserve Fetch's comma-joined rejection when more than one hint was supplied
    let text = (!duplicate)
        .then(|| value.and_then(|value| value.to_str().ok()))
        .flatten();
    let seconds = text
        .filter(|value| {
            (1..=6).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
        })
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value <= 604_800);
    metadata.insert(
        "retryAfterSeconds".to_owned(),
        seconds.map_or(RainCaptureMetadataValue::Null, |value| {
            RainCaptureMetadataValue::Number(value as f64)
        }),
    );
    metadata.insert(
        "retryAfterRequiresManualResume".to_owned(),
        RainCaptureMetadataValue::Boolean(value.is_some() && seconds.is_none()),
    );
    metadata
}

/// Hash exact decoded response bytes using the shared native SHA-256 owner.
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, bytes);
    let mut text = String::with_capacity(64);
    // append two lowercase hexadecimal digits for every digest byte
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Parse only the canonical millisecond UTC identity used by durable request slots.
fn canonical_utc(value: &str) -> Option<i64> {
    RevisionArchiveCanonicalCodec::canonical_utc_milliseconds(&NativeWirePrimitives, value)
}

/// Read one finite JSON number without coercion.
fn number(value: Option<&JsonValue>) -> Option<f64> {
    // preserve only JSON-compatible finite numeric values
    match value? {
        JsonValue::Number(value) if value.is_finite() => Some(*value),
        _ => None,
    }
}

/// Read one JSON string without replacing invalid UTF-16 code units.
fn string(value: Option<&JsonValue>) -> Option<String> {
    let JsonValue::String(value) = value? else {
        return None;
    };
    String::from_utf16(value).ok()
}

/// Read one JSON array without coercing objects or missing members.
fn array(value: Option<&JsonValue>) -> Option<&[JsonValue]> {
    let JsonValue::Array(value) = value? else {
        return None;
    };
    Some(value)
}

/// Identify whether a receipt uses the forecast-only availability decision.
const fn forecast_request_kind(request: &RainCaptureRequest) -> bool {
    matches!(request, RainCaptureRequest::Forecast { .. })
}

/// Construct one stable task-boundary failure without transport or credential details.
fn task_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use url::Url;
    use weather_worker::{
        RainCaptureMetadataValue, RainCaptureOutcome, RainCaptureRequest, RainCollectionProvider,
    };

    use super::{
        NativeRainCollectionTransport, RainCollectionTransportClock, retry_after_metadata,
        sha256_hex,
    };
    use crate::native_outbound_http::NativeOutboundHttp;

    /// Deterministic finite clock for one transport exchange.
    struct FixtureClock(Mutex<VecDeque<i64>>);

    impl RainCollectionTransportClock for FixtureClock {
        /// Return each fixture instant in declared sampling order.
        fn now_milliseconds(&self) -> Option<i64> {
            self.0.lock().ok()?.pop_front()
        }
    }

    /// A complete 429 response retains exact bytes, status, hash, and bounded retry metadata.
    #[test]
    fn forecast_rate_limit_receipt_preserves_transport_evidence() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let count = stream.read(&mut request).unwrap();
            let request = std::str::from_utf8(&request[..count]).unwrap();
            assert!(request.starts_with(
                "GET /v1/forecast?latitude=47.950429954185445&longitude=-122.42797012608193&run=2026-11-24T00%3A00&models=ecmwf_ifs&hourly=temperature_2m%2Crelative_humidity_2m%2Cwind_speed_10m%2Cprecipitation%2Csurface_pressure%2Ccloud_cover%2Cwind_direction_10m&forecast_hours=49&timezone=GMT&temperature_unit=celsius&wind_speed_unit=ms&precipitation_unit=mm&timeformat=iso8601 HTTP/1.1"
            ));
            let body = b"rate limited\n";
            write!(
                stream,
                "HTTP/1.1 429 Too Many Requests\r\nContent-Length: {}\r\nRetry-After: 120\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
        });
        let endpoint = Url::parse(&format!("http://{address}/v1/forecast")).unwrap();
        let mut transport = NativeRainCollectionTransport::with_test_dependencies(
            Arc::new(NativeOutboundHttp::new().unwrap()),
            None,
            endpoint,
            Url::parse(&format!("http://{address}/")).unwrap(),
            Arc::new(FixtureClock(Mutex::new(VecDeque::from([
                1_795_478_400_000,
                1_795_478_400_250,
            ])))),
        );
        let receipt = transport
            .fetch_rain_capture(&RainCaptureRequest::Forecast {
                attempt: 1,
                run_initialized_at: "2026-11-24T00:00:00.000Z".to_owned(),
                slot_key: "forecast:2026-11-24T00:00:00.000Z:1".to_owned(),
            })
            .unwrap();
        server.join().unwrap();

        assert_eq!(receipt.outcome, RainCaptureOutcome::RateLimited);
        assert_eq!(receipt.http_status, Some(429));
        assert_eq!(receipt.body.as_deref(), Some(b"rate limited\n".as_slice()));
        assert_eq!(
            receipt.body_sha256.as_deref(),
            Some(sha256_hex(b"rate limited\n").as_str())
        );
        assert_eq!(receipt.error_code.as_deref(), Some("http_429"));
        assert_eq!(receipt.available_by_decision, Some(false));
        assert_eq!(
            receipt.metadata.get("retryAfterSeconds"),
            Some(&RainCaptureMetadataValue::Number(120.0))
        );
        assert_eq!(
            receipt.metadata.get("retryAfterRequiresManualResume"),
            Some(&RainCaptureMetadataValue::Boolean(false))
        );
    }

    /// Unsupported Retry-After syntax never silently shortens the provider pause.
    #[test]
    fn retry_after_requires_manual_resume_for_unbounded_hints() {
        let value = hyper::header::HeaderValue::from_static("604801");
        let mut headers = hyper::header::HeaderMap::new();
        headers.insert(hyper::header::RETRY_AFTER, value);
        let metadata = retry_after_metadata(&headers);
        assert_eq!(
            metadata.get("retryAfterSeconds"),
            Some(&RainCaptureMetadataValue::Null)
        );
        assert_eq!(
            metadata.get("retryAfterRequiresManualResume"),
            Some(&RainCaptureMetadataValue::Boolean(true))
        );
    }
}
