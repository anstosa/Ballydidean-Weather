//! Native pooled HTTP binding for Weather Underground public-station history.

use std::sync::Arc;

use jiff::{Timestamp, tz::TimeZone};
use url::Url;
use weather_contracts::JsonValue;
use weather_providers::{
    ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse, ProviderError, ProviderFailure,
    ProviderFailureClassification,
};
use weather_worker::{
    PublicStationNumber, TaskFailure, WeatherUndergroundMetricObservation,
    WeatherUndergroundObservation, WeatherUndergroundQcStatus,
};

use crate::maintenance_canonical_json::field;
use crate::native_open_meteo_transport::{decode_provider_json, json_get, number_text};
use crate::native_open_meteo_values::load_named_timezone;
use crate::native_outbound_http::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
    NativeOutboundHttpResponse, NativeProviderCodec, NativeProviderExchange,
};
use crate::{
    WorkerWundergroundTransport, WundergroundDayRequest, WundergroundDayResponse,
    WundergroundLocalDate, system_worker_health_milliseconds,
};

const WUNDERGROUND_HISTORY_ENDPOINT: &str = "https://api.weather.com/v2/pws/history/all";
const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const MAXIMUM_DIAGNOSTIC_UNITS: usize = 512;

/// Shared native Weather Underground transport with one cached source-local timezone.
pub struct NativeWundergroundTransport {
    client: Arc<NativeOutboundHttp>,
    endpoint: Url,
    now: fn() -> Option<i64>,
    timezone: Option<CachedTimeZone>,
}

/// One retained host timezone selected by the current scheduled source.
struct CachedTimeZone {
    name: String,
    value: TimeZone,
}

impl NativeWundergroundTransport {
    /// Start one retained pooled client for Weather Underground scheduled requests.
    pub fn new() -> Result<Self, NativeOutboundHttpError> {
        let client = Arc::new(NativeOutboundHttp::new()?);
        Self::with_shared_client(client)
    }

    /// Adopt the process HTTP client without creating another runtime or connection pool.
    pub fn with_shared_client(
        client: Arc<NativeOutboundHttp>,
    ) -> Result<Self, NativeOutboundHttpError> {
        let endpoint = Url::parse(WUNDERGROUND_HISTORY_ENDPOINT)
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        Ok(Self {
            client,
            endpoint,
            now: system_worker_health_milliseconds,
            timezone: None,
        })
    }

    /// Adopt one finite fixture endpoint and deterministic receipt clock.
    #[cfg(test)]
    pub(crate) fn with_endpoint(
        client: Arc<NativeOutboundHttp>,
        mut endpoint: Url,
        now: fn() -> Option<i64>,
    ) -> Self {
        endpoint.set_query(None);
        endpoint.set_fragment(None);
        Self {
            client,
            endpoint,
            now,
            timezone: None,
        }
    }

    /// Load one named zone once for consecutive requests from the same station.
    fn timezone(&mut self, name: &str) -> Option<&TimeZone> {
        // reload only when the scheduled source selects a different timezone
        if self
            .timezone
            .as_ref()
            .is_none_or(|cached| cached.name != name)
        {
            self.timezone = Some(CachedTimeZone {
                name: name.to_owned(),
                value: load_named_timezone(name)?,
            });
        }
        self.timezone.as_ref().map(|cached| &cached.value)
    }
}

impl WorkerWundergroundTransport for NativeWundergroundTransport {
    type Day<'a>
        = NativeProviderExchange<'a, WundergroundCodec>
    where
        Self: 'a;

    /// Project one UTC instant into the source's exact compact local calendar date.
    fn local_date(
        &mut self,
        epoch_ms: i64,
        timezone: &str,
    ) -> Result<WundergroundLocalDate, TaskFailure> {
        let timestamp = Timestamp::from_millisecond(epoch_ms).map_err(|_| invalid_request())?;
        let timezone = self.timezone(timezone).ok_or_else(invalid_request)?;
        let date = timezone.to_datetime(timestamp).date();
        WundergroundLocalDate::admitted_yyyymmdd(format!(
            "{:04}{:02}{:02}",
            date.year(),
            date.month(),
            date.day()
        ))
    }

    /// Bind the credential to the exact ordered provider URL for one local day.
    fn bind_day<'a>(
        &'a mut self,
        request: &WundergroundDayRequest<'_>,
    ) -> Result<Self::Day<'a>, TaskFailure> {
        let outbound =
            wunderground_request(self.endpoint.clone(), request).map_err(|_| invalid_request())?;
        Ok(NativeProviderExchange::new(
            &self.client,
            WundergroundCodec {
                attempt: 1,
                request: outbound,
            },
        ))
    }

    /// Sample the process receipt clock only after all provider days succeeded.
    fn received_at_ms(&mut self) -> i64 {
        (self.now)().unwrap_or_default()
    }
}

/// Request-bound codec for one Weather Underground local-day response.
pub struct WundergroundCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<WundergroundDayResponse> for WundergroundCodec {
    /// Reuse the exact credentialed request while retaining controller attempt evidence.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Decode successful JSON into lazy typed cells and retain raw response evidence.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<WundergroundDayResponse>, ProviderAttemptError> {
        let decoded = decode_provider_json(response, self.attempt)?;
        let observations = if (200..300).contains(&decoded.status) {
            decode_wunderground_payload(&decoded.body).map_err(|message| {
                invalid_payload_failure(self.attempt, decoded.status, &message)
            })?
        } else {
            // non-success bodies are classified by the shared provider controller
            Vec::new()
        };
        Ok(ProviderAttemptResponse {
            body: WundergroundDayResponse {
                checksum: decoded.checksum,
                observations,
            },
            content: decoded.content,
            provider_reason: decoded.provider_reason,
            retry_after_ms: decoded.retry_after_ms,
            status: decoded.status,
        })
    }
}

/// Build the original endpoint and URLSearchParams insertion order.
fn wunderground_request(
    mut endpoint: Url,
    request: &WundergroundDayRequest<'_>,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    validate_request(request)?;
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    endpoint
        .query_pairs_mut()
        .append_pair("stationId", request.station_id)
        .append_pair("format", request.format)
        .append_pair("units", request.units)
        .append_pair("numericPrecision", request.numeric_precision)
        .append_pair("date", request.date.as_str())
        .append_pair("apiKey", request.public_api_key);
    json_get(endpoint)
}

/// Recheck every credential and fixed query value before provider traffic.
fn validate_request(request: &WundergroundDayRequest<'_>) -> Result<(), NativeOutboundHttpError> {
    let station = request.station_id.as_bytes();
    let key = request.public_api_key.as_bytes();
    let date = request.date.as_str().as_bytes();
    // reject aliases even if a caller constructs the public request directly
    if !(3..=32).contains(&station.len())
        || !station
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        || key.len() != 32
        || !key
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || date.len() != 8
        || !date.iter().all(u8::is_ascii_digit)
        || request.format != "json"
        || request.units != "m"
        || request.numeric_precision != "decimal"
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(())
}

/// Decode a successful archive document without eagerly validating unused metric cells.
fn decode_wunderground_payload(
    root: &JsonValue,
) -> Result<Vec<WeatherUndergroundObservation>, String> {
    // require the same top-level object admitted by the TypeScript provider
    if !matches!(root, JsonValue::Object(_)) {
        return Err("Weather Underground response must be an object".to_owned());
    }
    let rows = match field(root, "observations") {
        None | Some(JsonValue::Null) => return Ok(Vec::new()),
        Some(JsonValue::Array(rows)) => rows,
        Some(_) => {
            return Err("Weather Underground observations must be an array".to_owned());
        }
    };
    let mut observations = Vec::with_capacity(rows.len());
    // preserve provider order while validating every row object and epoch before filtering
    for (index, row) in rows.iter().enumerate() {
        // reject a primitive row before reading its timestamp
        if !matches!(row, JsonValue::Object(_)) {
            return Err(format!("observations[{index}] must be an object"));
        }
        observations.push(WeatherUndergroundObservation {
            epoch_seconds: positive_integer(row, "epoch")
                .ok_or_else(|| format!("observations[{index}].epoch must be a positive integer"))?,
            humidity_avg_percent: decoded_number(row, "humidityAvg"),
            metric: decoded_metric(row),
            qc_status: decoded_qc_status(row),
            solar_radiation_high_wm2: decoded_number(row, "solarRadiationHigh"),
            station_id: decoded_station_id(row),
            uv_high: decoded_number(row, "uvHigh"),
            wind_direction_avg_degrees: decoded_number(row, "winddirAvg"),
        });
    }
    Ok(observations)
}

/// Decode the metric object while retaining invalid and missing cells for lazy normalization.
fn decoded_metric(row: &JsonValue) -> Option<WeatherUndergroundMetricObservation> {
    let metric = field(row, "metric")?;
    // preserve non-object metric values as the normalizer's missing-object diagnosis
    if !matches!(metric, JsonValue::Object(_)) {
        return None;
    }
    Some(WeatherUndergroundMetricObservation {
        precip_rate_mm_per_hour: decoded_number(metric, "precipRate"),
        pressure_max_hpa: decoded_number(metric, "pressureMax"),
        pressure_min_hpa: decoded_number(metric, "pressureMin"),
        temperature_avg_c: decoded_number(metric, "tempAvg"),
        wind_gust_high_kph: decoded_number(metric, "windgustHigh"),
        wind_speed_avg_kph: decoded_number(metric, "windspeedAvg"),
    })
}

/// Preserve one JSON metric cell without collapsing a malformed value to a provider gap.
fn decoded_number(object: &JsonValue, key: &str) -> PublicStationNumber {
    match field(object, key) {
        None | Some(JsonValue::Null) => PublicStationNumber::Missing,
        Some(JsonValue::Number(value)) if value.is_finite() => PublicStationNumber::Number(*value),
        Some(_) => PublicStationNumber::Invalid,
    }
}

/// Preserve the nullable QC number with JavaScript's exact string spelling.
fn decoded_qc_status(row: &JsonValue) -> WeatherUndergroundQcStatus {
    match field(row, "qcStatus") {
        None | Some(JsonValue::Null) => WeatherUndergroundQcStatus::Missing,
        Some(JsonValue::Number(value)) if value.is_finite() => number_text(*value)
            .map(WeatherUndergroundQcStatus::AdmittedNumber)
            .unwrap_or(WeatherUndergroundQcStatus::Invalid),
        Some(_) => WeatherUndergroundQcStatus::Invalid,
    }
}

/// Decode the station string or retain a value guaranteed to fail station binding.
fn decoded_station_id(row: &JsonValue) -> String {
    let Some(JsonValue::String(value)) = field(row, "stationID") else {
        return String::new();
    };
    String::from_utf16(value).unwrap_or_default()
}

/// Decode one JavaScript-safe positive integer from a JSON number.
fn positive_integer(object: &JsonValue, key: &str) -> Option<i64> {
    let JsonValue::Number(value) = field(object, key)? else {
        return None;
    };
    // match Number.isSafeInteger and the source's positive epoch floor
    if !value.is_finite() || value.fract() != 0.0 || !(1.0..=MAXIMUM_SAFE_INTEGER).contains(value) {
        return None;
    }
    Some(*value as i64)
}

/// Convert one typed response schema failure into the controller's invalid-payload category.
fn invalid_payload_failure(attempt: u8, status: u16, message: &str) -> ProviderAttemptError {
    let bounded = message
        .encode_utf16()
        .take(MAXIMUM_DIAGNOSTIC_UNITS)
        .collect::<Vec<_>>();
    ProviderAttemptError::Classified(ProviderFailure {
        attempts: attempt,
        content_budget_role: None,
        error: ProviderError {
            classification: ProviderFailureClassification::InvalidPayload,
            code: "invalid_payload".to_owned(),
            message: String::from_utf16_lossy(&bounded),
            retry_after_ms: None,
        },
        status: Some(status),
    })
}

/// Preserve the established no-traffic scheduled failure category.
fn invalid_request() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "provider_unavailable".to_owned(),
    }
}

#[cfg(test)]
#[path = "native_wunderground_transport_test.rs"]
mod tests;
