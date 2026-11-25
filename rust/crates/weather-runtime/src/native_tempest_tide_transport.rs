//! Native retained HTTP bindings for scheduled Tempest and NOAA tide products.

use std::sync::Arc;

use hyper::header::{HeaderValue, ORIGIN, USER_AGENT};
use url::Url;
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;
use weather_providers::{
    DEFAULT_PROVIDER_MAX_ATTEMPTS, DEFAULT_PROVIDER_TIMEOUT_MS, MAX_PROVIDER_BODY_BYTES,
    ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse, ProviderControls,
    ProviderError, ProviderFailure, ProviderFailureClassification, ProviderRequestPolicy,
    run_provider_request,
};
use weather_worker::{
    NoaaTideRangeRequest, ProviderBatch, ScheduledProvider, TaskFailure,
    TempestBackfillOperationError, TempestBackfillProvider, TempestObservationEnvelope,
    TempestObservationRequest, TideBackfillErrorClassification, TideBackfillOperationError,
    TideBackfillProvider, TideBackfillProviderRequest,
};

use crate::native_noaa_tide_transport::{NativeNoaaTideCodec, bind_native_noaa_request};
use crate::native_open_meteo_transport::{decode_provider_json, json_get};
use crate::native_outbound_http::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
    NativeOutboundHttpResponse, NativeProviderCodec, OwnedNativeProviderExchange,
};
use crate::{
    NativeWirePrimitives, RetryingNoaaTideProvider, RetryingTempestProvider, TempestAttemptPayload,
    WorkerNoaaTideTransport, WorkerTempestTransport, decode_native_tempest_payload,
    system_worker_health_milliseconds,
};

const TEMPEST_ORIGIN: &str = "https://swd.weatherflow.com";
const NOAA_ENDPOINT: &str = "https://api.tidesandcurrents.noaa.gov/api/prod/datagetter";
const TEMPEST_BUILD: &str = "169";
const TEMPEST_REQUEST_ORIGIN: &str = "https://tempestwx.com";
const TEMPEST_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Shared process transport for credentialed Tempest and public NOAA requests.
pub struct NativeTempestTideTransport {
    client: Arc<NativeOutboundHttp>,
    noaa_endpoint: Url,
    tempest_api_key: String,
    tempest_origin: Url,
}

impl NativeTempestTideTransport {
    /// Start one retained pooled client with the admitted Tempest credential.
    pub fn new(tempest_api_key: String) -> Result<Self, NativeOutboundHttpError> {
        let client = Arc::new(NativeOutboundHttp::new()?);
        Self::with_client(client, tempest_api_key)
    }

    /// Adopt a retained process client without creating another runtime or connection pool.
    pub fn with_client(
        client: Arc<NativeOutboundHttp>,
        tempest_api_key: String,
    ) -> Result<Self, NativeOutboundHttpError> {
        let tempest_origin =
            Url::parse(TEMPEST_ORIGIN).map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        let noaa_endpoint =
            Url::parse(NOAA_ENDPOINT).map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        Ok(Self {
            client,
            noaa_endpoint,
            tempest_api_key,
            tempest_origin,
        })
    }

    /// Resolve and compare the live ST identity before importing any historical records.
    pub fn verify_station(
        &self,
        location_id: u64,
        device_id: u64,
        serial: &str,
    ) -> Result<(), TaskFailure> {
        // reject impossible configured location and credential values before provider I/O
        if location_id == 0
            || location_id > MAXIMUM_SAFE_INTEGER
            || self.tempest_api_key.is_empty()
            || self.tempest_api_key.encode_utf16().count() > 256
            || self.tempest_api_key.chars().any(char::is_whitespace)
        {
            return Err(invalid_task("invalid_payload"));
        }
        let mut url = self
            .tempest_origin
            .join(&format!("/swd/rest/locations/{location_id}"))
            .map_err(|_| invalid_task("invalid_payload"))?;
        url.query_pairs_mut()
            .append_pair("api_key", &self.tempest_api_key)
            .append_pair("build", TEMPEST_BUILD)
            .append_pair("include_arbitrary_locations", "true");
        let request = json_get(url)
            .map_err(|_| invalid_task("invalid_payload"))?
            .header(ORIGIN, HeaderValue::from_static(TEMPEST_REQUEST_ORIGIN))
            .header(USER_AGENT, HeaderValue::from_static(TEMPEST_USER_AGENT));
        let mut exchange = OwnedNativeProviderExchange::new(
            Arc::clone(&self.client),
            TempestLocationCodec {
                request,
                attempt: 1,
            },
        );
        let response = run_provider_request(
            ProviderRequestPolicy {
                controls: ProviderControls::default(),
                deadline_at_ms: None,
            },
            &mut exchange,
        )
        .map_err(crate::provider_task_failure)?;
        let (actual_device, actual_serial) = decode_tempest_location(&response.body, location_id)?;
        // do not import records when the mounted location refers to a replacement device
        if actual_device != device_id || actual_serial != serial {
            return Err(invalid_task("tempest_station_identity_changed"));
        }
        Ok(())
    }

    /// Adopt finite fixture endpoints while preserving the production codecs and shared client.
    #[cfg(test)]
    pub(crate) const fn with_endpoints(
        client: Arc<NativeOutboundHttp>,
        tempest_api_key: String,
        tempest_origin: Url,
        noaa_endpoint: Url,
    ) -> Self {
        Self {
            client,
            noaa_endpoint,
            tempest_api_key,
            tempest_origin,
        }
    }
}

impl WorkerTempestTransport for NativeTempestTideTransport {
    type Exchange<'a>
        = OwnedNativeProviderExchange<TempestCodec>
    where
        Self: 'a;

    /// Bind the exact credentialed observation request and public-web request headers.
    fn bind_request(
        &mut self,
        request: &TempestObservationRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure> {
        let request = tempest_request(&self.tempest_origin, &self.tempest_api_key, request)
            .map_err(|_| invalid_task("invalid_payload"))?;
        Ok(OwnedNativeProviderExchange::new(
            Arc::clone(&self.client),
            TempestCodec {
                attempt: 1,
                request,
            },
        ))
    }
}

impl WorkerNoaaTideTransport for NativeTempestTideTransport {
    type Exchange<'a>
        = OwnedNativeProviderExchange<NativeNoaaTideCodec>
    where
        Self: 'a;

    /// Bind one exact observation or prediction range to the public NOAA endpoint.
    fn bind_request(
        &mut self,
        request: &NoaaTideRangeRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure> {
        bind_native_noaa_request(
            Arc::clone(&self.client),
            self.noaa_endpoint.clone(),
            request,
        )
    }
}

impl TempestBackfillProvider for NativeTempestTideTransport {
    /// Reuse the scheduled Tempest request and retry controller for one backfill chunk.
    fn fetch(
        &mut self,
        request: TempestObservationRequest,
        deadline_at_ms: i64,
    ) -> Result<
        ProviderBatch<
            weather_worker::NormalizedTempestRecord,
            weather_worker::TempestResponseMetadata,
            Option<i64>,
        >,
        TempestBackfillOperationError,
    > {
        let exchange =
            WorkerTempestTransport::bind_request(self, &request).map_err(backfill_task_failure)?;
        let mut provider =
            RetryingTempestProvider::new(exchange, production_controls(), Some(deadline_at_ms));
        let batch =
            ScheduledProvider::fetch(&mut provider, request).map_err(backfill_task_failure)?;
        Ok(ProviderBatch {
            attempts: batch.attempts,
            provider_cursor: batch.provider_cursor.map(|cursor| cursor.valid_at_ms),
            records: batch.records,
            response_metadata: batch.response_metadata,
            upstream_response_checksum: batch.upstream_response_checksum,
        })
    }
}

impl TideBackfillProvider for NativeTempestTideTransport {
    /// Reuse the scheduled NOAA request and retry controller for one backfill chunk.
    fn fetch(
        &mut self,
        request: TideBackfillProviderRequest,
    ) -> Result<
        ProviderBatch<
            weather_worker::NormalizedNoaaTideRecord,
            weather_worker::NoaaTideResponseMetadata,
            Option<weather_worker::NoaaTideProviderCursor>,
        >,
        TideBackfillOperationError,
    > {
        let exchange = WorkerNoaaTideTransport::bind_request(self, &request.range)
            .map_err(backfill_task_failure)?;
        let mut provider = RetryingNoaaTideProvider::with_controls(
            exchange,
            production_controls(),
            Some(request.deadline_at_ms),
        );
        ScheduledProvider::fetch(&mut provider, request.range).map_err(backfill_task_failure)
    }
}

/// Request-bound codec for the Tempest observation product.
pub struct TempestCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<TempestAttemptPayload> for TempestCodec {
    /// Reuse the exact request while retaining controller-owned attempt evidence.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Decode the successful JSON through the retained native Tempest projection.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<TempestAttemptPayload>, ProviderAttemptError> {
        let decoded = decode_provider_json(response, self.attempt)?;
        let payload = if (200..300).contains(&decoded.status) {
            let received_at_ms = system_worker_health_milliseconds().unwrap_or(0);
            let envelope = decode_tempest_value(&decoded.body)
                .map_err(|failure| task_attempt_failure(failure, self.attempt, decoded.status))?;
            TempestAttemptPayload {
                envelope,
                received_at_ms,
                upstream_response_checksum: decoded.checksum.as_str().to_owned(),
            }
        } else {
            TempestAttemptPayload {
                envelope: placeholder_tempest(),
                received_at_ms: 0,
                upstream_response_checksum: decoded.checksum.as_str().to_owned(),
            }
        };
        Ok(ProviderAttemptResponse {
            body: payload,
            content: decoded.content,
            provider_reason: decoded.provider_reason,
            retry_after_ms: decoded.retry_after_ms,
            status: decoded.status,
        })
    }
}

/// Retain raw location JSON under the original generic provider retry policy.
struct TempestLocationCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<JsonValue> for TempestLocationCodec {
    /// Retain one immutable location request through each retry.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Decode only the transport layer before interpreting the location contract.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<JsonValue>, ProviderAttemptError> {
        let decoded = decode_provider_json(response, self.attempt)?;
        Ok(ProviderAttemptResponse {
            body: decoded.body,
            content: decoded.content,
            provider_reason: decoded.provider_reason,
            retry_after_ms: decoded.retry_after_ms,
            status: decoded.status,
        })
    }
}

/// Admit all original location metadata and return the unique ST device identity.
fn decode_tempest_location(
    value: &JsonValue,
    location_id: u64,
) -> Result<(u64, String), TaskFailure> {
    use crate::maintenance_canonical_json::{field, text};
    let invalid = || invalid_task("invalid_payload");
    let status = field(value, "status").ok_or_else(invalid)?;
    // retain the source's classification for provider-level rejection
    if field(status, "status_code") != Some(&JsonValue::Number(0.0)) {
        return Err(invalid_task("provider_request_rejected"));
    }
    let Some(JsonValue::Array(locations)) = field(value, "locations") else {
        return Err(invalid());
    };
    // validate all entries before choosing the first requested location
    if locations
        .iter()
        .any(|location| !matches!(location, JsonValue::Object(_)))
    {
        return Err(invalid());
    }
    let location = locations
        .iter()
        .find(|location| {
            field(location, "location_id") == Some(&JsonValue::Number(location_id as f64))
        })
        .ok_or_else(invalid)?;
    let Some(JsonValue::Array(devices)) = field(location, "devices") else {
        return Err(invalid());
    };
    // require every device to be an object and exactly one to be the ST device
    if devices
        .iter()
        .any(|device| !matches!(device, JsonValue::Object(_)))
    {
        return Err(invalid());
    }
    let mut weather_devices = devices
        .iter()
        .filter(|device| text(device, "device_type").as_deref() == Some("ST"));
    let device = weather_devices.next().ok_or_else(invalid)?;
    if weather_devices.next().is_some() {
        return Err(invalid());
    }
    let Some(JsonValue::Number(id)) = field(device, "device_id") else {
        return Err(invalid());
    };
    // require a positive JS-safe integral device identifier and complete original metadata
    if !(1.0..=MAXIMUM_SAFE_INTEGER as f64).contains(id) || id.fract() != 0.0 {
        return Err(invalid());
    }
    let bounded_text = |object: &JsonValue, key: &str, maximum: usize| {
        text(object, key)
            .filter(|value| !value.trim().is_empty() && value.encode_utf16().count() <= maximum)
            .ok_or_else(invalid)
    };
    let bounded_number = |key: &str, lower: f64, upper: f64| matches!(field(location, key), Some(JsonValue::Number(number)) if (lower..=upper).contains(number));
    bounded_text(location, "name", 160)?;
    let timezone = bounded_text(location, "timezone", 64)?;
    // use the retained native IANA calendar and ensure the original range bounds
    if !bounded_number("latitude", -90.0, 90.0)
        || !bounded_number("longitude", -180.0, 180.0)
        || jiff::tz::TimeZone::get(&timezone).is_err()
    {
        return Err(invalid());
    }
    Ok((*id as u64, bounded_text(device, "serial_number", 128)?))
}

/// Build the exact Tempest path, ordered query, and browser-identity headers.
fn tempest_request(
    origin: &Url,
    api_key: &str,
    request: &TempestObservationRequest,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    validate_tempest_request(api_key, request)?;
    let mut url = origin
        .join(&format!(
            "/swd/rest/observations/device/{}",
            request.device_id
        ))
        .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
    url.query_pairs_mut()
        .append_pair("api_key", api_key)
        .append_pair("build", TEMPEST_BUILD)
        .append_pair("time_start", &(request.start_ms / 1_000).to_string())
        .append_pair(
            "time_end",
            &(request.end_exclusive_ms / 1_000 - 1).to_string(),
        );
    Ok(json_get(url)?
        .header(ORIGIN, HeaderValue::from_static(TEMPEST_REQUEST_ORIGIN))
        .header(USER_AGENT, HeaderValue::from_static(TEMPEST_USER_AGENT)))
}

/// Validate the request fields consumed by the Tempest URL owner.
fn validate_tempest_request(
    api_key: &str,
    request: &TempestObservationRequest,
) -> Result<(), NativeOutboundHttpError> {
    let duration = request
        .end_exclusive_ms
        .checked_sub(request.start_ms)
        .ok_or(NativeOutboundHttpError::InvalidRequest)?;
    // retain the exact one-line credential and request limits
    if api_key.is_empty()
        || api_key.encode_utf16().count() > 256
        || api_key.chars().any(char::is_whitespace)
        || request.device_id == 0
        || request.device_id > MAXIMUM_SAFE_INTEGER
        || request.start_ms % 1_000 != 0
        || request.end_exclusive_ms % 1_000 != 0
        || duration <= 0
        || duration > 86_400_000
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(())
}

/// Reuse the retained native Tempest parser after native Fetch-text JSON admission.
fn decode_tempest_value(value: &JsonValue) -> Result<TempestObservationEnvelope, TaskFailure> {
    let mut bytes = NativeWirePrimitives
        .encode_ordered_json(value)
        .ok_or_else(|| invalid_task("invalid_payload"))?;
    // remove archive framing before passing the provider JSON document to the retained decoder
    if bytes.pop() != Some(b'\n') {
        return Err(invalid_task("invalid_payload"));
    }
    decode_native_tempest_payload(&bytes)
}

/// Convert a typed decoder failure into one controller-classified provider attempt.
fn task_attempt_failure(failure: TaskFailure, attempt: u8, status: u16) -> ProviderAttemptError {
    let classification = if failure.code == "provider_request_rejected" {
        ProviderFailureClassification::Permanent
    } else {
        ProviderFailureClassification::InvalidPayload
    };
    ProviderAttemptError::Classified(ProviderFailure {
        attempts: attempt,
        content_budget_role: None,
        error: ProviderError {
            classification,
            code: failure.code,
            message: "Tempest provider payload is invalid".to_owned(),
            retry_after_ms: None,
        },
        status: Some(status),
    })
}

/// Construct a discarded Tempest body for non-success HTTP classification.
fn placeholder_tempest() -> TempestObservationEnvelope {
    TempestObservationEnvelope {
        bucket_step_minutes: None,
        device_id: 0,
        observations: None,
        response_type: weather_worker::TempestResponseType::Other,
        status_code: 0,
    }
}

/// Return the provider's fixed ten-megabyte bounded retry controls.
const fn production_controls() -> ProviderControls {
    ProviderControls {
        max_attempts: DEFAULT_PROVIDER_MAX_ATTEMPTS,
        max_body_bytes: MAX_PROVIDER_BODY_BYTES,
        timeout_ms: DEFAULT_PROVIDER_TIMEOUT_MS,
    }
}

/// Construct one local pre-I/O task failure.
fn invalid_task(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

/// Map one scheduled task code into the backfill domain classification.
fn backfill_task_failure(failure: TaskFailure) -> TideBackfillOperationError {
    let classification = match failure.code.as_str() {
        "provider_rate_limited" => TideBackfillErrorClassification::RateLimited,
        "provider_request_rejected" => TideBackfillErrorClassification::Permanent,
        "invalid_json"
        | "invalid_payload"
        | "noaa_invalid_payload"
        | "provider_response_too_large" => TideBackfillErrorClassification::InvalidPayload,
        _ => TideBackfillErrorClassification::Retryable,
    };
    TideBackfillOperationError::provider(
        failure.attempts.max(1),
        classification,
        failure.code,
        "provider request failed",
    )
}

#[cfg(test)]
#[path = "native_tempest_tide_transport_test.rs"]
mod tests;
