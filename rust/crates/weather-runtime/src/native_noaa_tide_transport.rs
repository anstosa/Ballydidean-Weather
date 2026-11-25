//! Credential-free native HTTP binding for scheduled NOAA tide products.

use std::sync::Arc;

use url::Url;
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;
use weather_providers::{
    DEFAULT_PROVIDER_MAX_ATTEMPTS, DEFAULT_PROVIDER_TIMEOUT_MS, MAX_PROVIDER_BODY_BYTES,
    ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse, ProviderControls,
    ProviderError, ProviderFailure, ProviderFailureClassification,
};
use weather_worker::{
    NoaaPredictionType, NoaaTideObservation, NoaaTidePayload, NoaaTidePrediction,
    NoaaTideRangeRequest, NoaaTideSourceKind, ScheduledProvider, TaskFailure,
    TideBackfillErrorClassification, TideBackfillOperationError, TideBackfillProvider,
    TideBackfillProviderRequest,
};

use crate::maintenance_canonical_json::{field, text};
use crate::native_open_meteo_transport::{decode_provider_json, json_get};
use crate::native_outbound_http::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
    NativeOutboundHttpResponse, NativeProviderCodec, OwnedNativeProviderExchange,
};
use crate::{
    NativeWirePrimitives, NoaaTideAttemptPayload, WorkerNoaaTideTransport,
    system_worker_health_milliseconds,
};

const NOAA_ENDPOINT: &str = "https://api.tidesandcurrents.noaa.gov/api/prod/datagetter";
const MILLISECONDS_PER_MINUTE: i64 = 60_000;

/// Shared credential-free transport for NOAA observations and predictions.
pub struct NativeNoaaTideTransport {
    client: Arc<NativeOutboundHttp>,
    endpoint: Url,
}

impl NativeNoaaTideTransport {
    /// Start one retained outbound runtime for standalone NOAA scheduling.
    pub fn new() -> Result<Self, NativeOutboundHttpError> {
        Self::with_client(Arc::new(NativeOutboundHttp::new()?))
    }

    /// Adopt the process-wide outbound runtime without requiring a Tempest credential.
    pub fn with_client(client: Arc<NativeOutboundHttp>) -> Result<Self, NativeOutboundHttpError> {
        let endpoint =
            Url::parse(NOAA_ENDPOINT).map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        Ok(Self { client, endpoint })
    }

    /// Adopt one finite fixture endpoint without changing request or codec behavior.
    #[cfg(test)]
    pub(crate) const fn with_endpoint(client: Arc<NativeOutboundHttp>, endpoint: Url) -> Self {
        Self { client, endpoint }
    }
}

impl WorkerNoaaTideTransport for NativeNoaaTideTransport {
    type Exchange<'a>
        = OwnedNativeProviderExchange<NativeNoaaTideCodec>
    where
        Self: 'a;

    /// Bind one exact observation or prediction range without provider credentials.
    fn bind_request(
        &mut self,
        request: &NoaaTideRangeRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure> {
        bind_native_noaa_request(Arc::clone(&self.client), self.endpoint.clone(), request)
    }
}

impl TideBackfillProvider for NativeNoaaTideTransport {
    /// Reuse the scheduled NOAA request, codec, and retry controller for one backfill chunk.
    fn fetch(
        &mut self,
        request: TideBackfillProviderRequest,
    ) -> Result<
        weather_worker::ProviderBatch<
            weather_worker::NormalizedNoaaTideRecord,
            weather_worker::NoaaTideResponseMetadata,
            Option<weather_worker::NoaaTideProviderCursor>,
        >,
        TideBackfillOperationError,
    > {
        let exchange = WorkerNoaaTideTransport::bind_request(self, &request.range)
            .map_err(backfill_task_failure)?;
        let mut provider = crate::RetryingNoaaTideProvider::with_controls(
            exchange,
            production_controls(),
            Some(request.deadline_at_ms),
        );
        ScheduledProvider::fetch(&mut provider, request.range).map_err(backfill_task_failure)
    }
}

/// Bind the shared request and codec for standalone and combined NOAA owners.
pub(crate) fn bind_native_noaa_request(
    client: Arc<NativeOutboundHttp>,
    endpoint: Url,
    request: &NoaaTideRangeRequest,
) -> Result<OwnedNativeProviderExchange<NativeNoaaTideCodec>, TaskFailure> {
    let outbound =
        noaa_request(endpoint, request).map_err(|_| invalid_task("noaa_invalid_payload"))?;
    Ok(OwnedNativeProviderExchange::new(
        client,
        NativeNoaaTideCodec {
            attempt: 1,
            request: outbound,
            source_kind: request.source_kind,
        },
    ))
}

/// Request-bound codec for one NOAA observation or prediction product.
pub struct NativeNoaaTideCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
    source_kind: NoaaTideSourceKind,
}

impl NativeProviderCodec<NoaaTideAttemptPayload> for NativeNoaaTideCodec {
    /// Reuse the exact request while retaining controller-owned attempt evidence.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Decode successful NOAA JSON through the established typed normalization boundary.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<NoaaTideAttemptPayload>, ProviderAttemptError> {
        let decoded = decode_provider_json(response, self.attempt)?;
        // parse provider payloads only for successful HTTP statuses
        let payload = if (200..300).contains(&decoded.status) {
            let received_at_ms = system_worker_health_milliseconds().unwrap_or(0);
            let payload = decode_noaa_payload(&decoded.body, self.source_kind)
                .map_err(|message| noaa_attempt_failure(self.attempt, decoded.status, &message))?;
            NoaaTideAttemptPayload {
                payload,
                received_at_ms,
                upstream_response_checksum: decoded.checksum.as_str().to_owned(),
            }
        } else {
            NoaaTideAttemptPayload {
                payload: NoaaTidePayload::default(),
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

/// Build the exact NOAA URLSearchParams insertion order.
fn noaa_request(
    mut url: Url,
    request: &NoaaTideRangeRequest,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    validate_noaa_request(request)?;
    let end_inclusive = request
        .end_exclusive_ms
        .checked_sub(MILLISECONDS_PER_MINUTE)
        .ok_or(NativeOutboundHttpError::InvalidRequest)?;
    // select only the two generated NOAA products and their interval policy
    let (product, interval) = match request.source_kind {
        NoaaTideSourceKind::Observation => ("water_level", None),
        NoaaTideSourceKind::Prediction => ("predictions", Some("hilo")),
    };
    url.query_pairs_mut()
        .append_pair("application", "weather.ballydidean.farm")
        .append_pair("begin_date", &format_noaa_minute(request.start_ms)?)
        .append_pair("end_date", &format_noaa_minute(end_inclusive)?)
        .append_pair("station", &request.station_id)
        .append_pair("product", product)
        .append_pair("datum", "MLLW")
        .append_pair("time_zone", "gmt")
        .append_pair("units", "metric")
        .append_pair("format", "json");
    // append the prediction-only high-low interval last
    if let Some(interval) = interval {
        url.query_pairs_mut().append_pair("interval", interval);
    }
    json_get(url)
}

/// Validate the range and identity fields consumed by the NOAA URL owner.
fn validate_noaa_request(request: &NoaaTideRangeRequest) -> Result<(), NativeOutboundHttpError> {
    let duration = request
        .end_exclusive_ms
        .checked_sub(request.start_ms)
        .ok_or(NativeOutboundHttpError::InvalidRequest)?;
    // apply the source-specific maximum provider range
    let maximum = match request.source_kind {
        NoaaTideSourceKind::Observation => 31_i64 * 86_400_000,
        NoaaTideSourceKind::Prediction => 366_i64 * 86_400_000,
    };
    // retain minute alignment, the seven-digit station, and product range bounds
    if request.station_id.len() != 7
        || !request.station_id.bytes().all(|byte| byte.is_ascii_digit())
        || request.start_ms % MILLISECONDS_PER_MINUTE != 0
        || request.end_exclusive_ms % MILLISECONDS_PER_MINUTE != 0
        || duration <= 0
        || duration > maximum
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(())
}

/// Render one exact UTC minute in NOAA's zone-less request spelling.
fn format_noaa_minute(milliseconds: i64) -> Result<String, NativeOutboundHttpError> {
    let instant = NativeWirePrimitives
        .format_utc_milliseconds(milliseconds)
        .ok_or(NativeOutboundHttpError::InvalidRequest)?;
    Ok(instant[..16].replace('T', " "))
}

/// Decode one successful NOAA response into observations or predictions.
fn decode_noaa_payload(
    root: &JsonValue,
    source_kind: NoaaTideSourceKind,
) -> Result<NoaaTidePayload, String> {
    require_object(root, "NOAA response")?;
    // retain provider-declared JSON errors for the established normalizer
    if let Some(error) = field(root, "error").filter(|value| !matches!(value, JsonValue::Null)) {
        require_object(error, "NOAA error")?;
        let message = required_text(error, "message", 512)?;
        return Ok(NoaaTidePayload {
            error_message: Some(message),
            ..NoaaTidePayload::default()
        });
    }
    // decode only the row family selected by the durable scheduled source
    match source_kind {
        NoaaTideSourceKind::Observation => Ok(NoaaTidePayload {
            observations: Some(decode_noaa_observations(root)?),
            ..NoaaTidePayload::default()
        }),
        NoaaTideSourceKind::Prediction => Ok(NoaaTidePayload {
            predictions: Some(decode_noaa_predictions(root)?),
            ..NoaaTidePayload::default()
        }),
    }
}

/// Decode verified NOAA water-level rows without filtering the requested window.
fn decode_noaa_observations(root: &JsonValue) -> Result<Vec<NoaaTideObservation>, String> {
    let rows = required_array(root, "data")?;
    let mut output = Vec::with_capacity(rows.len());
    // preserve provider row order for downstream window filtering
    for row in rows {
        require_object(row, "NOAA data row")?;
        output.push(NoaaTideObservation {
            flags: required_text(row, "f", 128)?,
            quality_status: required_text(row, "q", 16)?,
            valid_at_ms: noaa_minute(required_text(row, "t", 16)?.as_str())?,
            water_level_m: numeric_text(row, "v")?,
        });
    }
    Ok(output)
}

/// Decode NOAA high-low prediction rows without filtering the requested window.
fn decode_noaa_predictions(root: &JsonValue) -> Result<Vec<NoaaTidePrediction>, String> {
    let rows = required_array(root, "predictions")?;
    let mut output = Vec::with_capacity(rows.len());
    // preserve provider row order for downstream window filtering
    for row in rows {
        require_object(row, "NOAA prediction row")?;
        // retain unsupported provider types for downstream contract validation
        let prediction_type = match required_text(row, "type", 1)?.as_str() {
            "H" => NoaaPredictionType::High,
            "L" => NoaaPredictionType::Low,
            _ => NoaaPredictionType::Unsupported,
        };
        output.push(NoaaTidePrediction {
            prediction_type,
            valid_at_ms: noaa_minute(required_text(row, "t", 16)?.as_str())?,
            water_level_m: numeric_text(row, "v")?,
        });
    }
    Ok(output)
}

/// Parse and round-trip one exact NOAA GMT minute.
fn noaa_minute(value: &str) -> Result<i64, String> {
    let bytes = value.as_bytes();
    let shape = bytes.len() == 16
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes.get(10) == Some(&b' ')
        && bytes.get(13) == Some(&b':')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7 | 10 | 13) || byte.is_ascii_digit());
    // reject timestamps outside the documented fixed-width shape
    if !shape {
        return Err("NOAA timestamp is invalid".to_owned());
    }
    let explicit = format!("{}T{}:00.000Z", &value[..10], &value[11..]);
    let milliseconds = NativeWirePrimitives
        .parse_explicit_utc_milliseconds(&explicit)
        .ok_or_else(|| "NOAA timestamp is invalid".to_owned())?;
    // reject calendar rollovers by requiring the same source spelling after formatting
    if format_noaa_minute(milliseconds).ok().as_deref() != Some(value) {
        return Err("NOAA timestamp is invalid".to_owned());
    }
    Ok(milliseconds)
}

/// Decode one numeric NOAA string with the provider's finite-number rule.
fn numeric_text(object: &JsonValue, key: &str) -> Result<f64, String> {
    let value = required_text(object, key, usize::MAX)?;
    let value = value.trim();
    // match Number's empty-string zero while rejecting other malformed values
    let number = if value.is_empty() {
        0.0
    } else {
        value
            .parse::<f64>()
            .map_err(|_| format!("{key} must be numeric"))?
    };
    // reject infinities and NaN like Number.isFinite
    if !number.is_finite() {
        return Err(format!("{key} must be numeric"));
    }
    Ok(number)
}

/// Require one JSON object.
fn require_object(value: &JsonValue, label: &str) -> Result<(), String> {
    // reject arrays and nulls at the provider boundary
    if !matches!(value, JsonValue::Object(_)) {
        return Err(format!("{label} must be an object"));
    }
    Ok(())
}

/// Require one named JSON array.
fn required_array<'a>(object: &'a JsonValue, key: &str) -> Result<&'a [JsonValue], String> {
    let JsonValue::Array(values) =
        field(object, key).ok_or_else(|| format!("{key} is required"))?
    else {
        return Err(format!("{key} must be an array"));
    };
    Ok(values)
}

/// Require one bounded string field without replacing UTF-16 code units.
fn required_text(object: &JsonValue, key: &str, maximum: usize) -> Result<String, String> {
    let value = text(object, key).ok_or_else(|| format!("{key} must be a bounded string"))?;
    // retain the source's non-empty UTF-16 length bound
    if value.is_empty() || value.encode_utf16().count() > maximum {
        return Err(format!("{key} must be a bounded string"));
    }
    Ok(value)
}

/// Convert one NOAA schema failure into its stable controller classification.
fn noaa_attempt_failure(attempt: u8, status: u16, message: &str) -> ProviderAttemptError {
    let bounded = message.encode_utf16().take(512).collect::<Vec<_>>();
    ProviderAttemptError::Classified(ProviderFailure {
        attempts: attempt,
        content_budget_role: None,
        error: ProviderError {
            classification: ProviderFailureClassification::InvalidPayload,
            code: "noaa_invalid_payload".to_owned(),
            message: String::from_utf16_lossy(&bounded),
            retry_after_ms: None,
        },
        status: Some(status),
    })
}

/// Return the fixed ten-megabyte bounded retry controls shared by NOAA scheduling and backfill.
const fn production_controls() -> ProviderControls {
    ProviderControls {
        max_attempts: DEFAULT_PROVIDER_MAX_ATTEMPTS,
        max_body_bytes: MAX_PROVIDER_BODY_BYTES,
        timeout_ms: DEFAULT_PROVIDER_TIMEOUT_MS,
    }
}

/// Map one scheduled NOAA failure into the backfill domain classification.
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

/// Construct one local pre-I/O task failure.
fn invalid_task(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };
    use weather_providers::{ProviderRequestPolicy, run_provider_request};

    /// Exercise the real credential-free prediction transport and source query.
    #[test]
    fn native_noaa_tide_transport_projects_loopback_prediction() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = Url::parse(&format!(
            "http://{}/api/prod/datagetter",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let body = br#"{"predictions":[{"t":"2023-11-14 22:14","v":"1.26","type":"H"}]}"#;
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            // await one fixture exchange with a finite deadline
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            // handle fragmented HTTP request metadata
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 8192);
            }
            assert!(request.starts_with(b"GET /api/prod/datagetter?application=weather.ballydidean.farm&begin_date=2023-11-14+22%3A14&end_date=2023-11-14+22%3A14&station=9447130&product=predictions&datum=MLLW&time_zone=gmt&units=metric&format=json&interval=hilo HTTP/1.1\r\n"));
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();
            socket.write_all(body).unwrap();
        });
        let mut transport = NativeNoaaTideTransport::with_endpoint(
            Arc::new(NativeOutboundHttp::new().unwrap()),
            endpoint,
        );
        let start_ms = 1_700_000_040_000;
        let range = NoaaTideRangeRequest {
            start_ms,
            end_exclusive_ms: start_ms + MILLISECONDS_PER_MINUTE,
            source_id: "fixture-noaa".to_owned(),
            source_kind: NoaaTideSourceKind::Prediction,
            station_id: "9447130".to_owned(),
            timezone: "UTC".to_owned(),
        };
        let mut exchange = transport.bind_request(&range).unwrap();
        let response = run_provider_request(
            ProviderRequestPolicy {
                controls: ProviderControls {
                    max_attempts: 1,
                    max_body_bytes: 4096,
                    timeout_ms: 5000,
                },
                deadline_at_ms: None,
            },
            &mut exchange,
        )
        .unwrap();
        assert_eq!(
            response.body.payload.predictions,
            Some(vec![NoaaTidePrediction {
                prediction_type: NoaaPredictionType::High,
                valid_at_ms: start_ms,
                water_level_m: 1.26,
            }])
        );
        assert_eq!(response.body.upstream_response_checksum.len(), 64);
        server.join().unwrap();
    }
}
