//! Native retained HTTP binding for scheduled and backfill Ambient Weather ranges.

use std::fmt::Write as _;
use std::sync::Arc;

use url::Url;
use weather_contracts::JsonValue;
use weather_database::Sha256Digest;
use weather_edge::RevisionArchiveCanonicalCodec;
use weather_providers::{
    DEFAULT_PROVIDER_MAX_ATTEMPTS, DEFAULT_PROVIDER_TIMEOUT_MS, MAX_PROVIDER_BODY_BYTES,
    ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse, ProviderControls,
    ProviderError, ProviderFailure, ProviderFailureClassification,
};
use weather_worker::{
    AmbientWeatherBackfillProvider, AmbientWeatherDeviceIdentity, AmbientWeatherObservation,
    AmbientWeatherPayload, AmbientWeatherProviderCursor, AmbientWeatherRangeRequest,
    AmbientWeatherResponseMetadata, NormalizedAmbientWeatherRecord, ProviderBatch,
    PublicStationBackfillOperationError, ScheduledProvider, TaskFailure,
    TideBackfillErrorClassification,
};

use crate::maintenance_canonical_json::field;
use crate::native_open_meteo_transport::{decode_provider_json, json_get};
use crate::native_outbound_http::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
    NativeOutboundHttpResponse, NativeProviderCodec, OwnedNativeProviderExchange,
};
use crate::{
    AmbientWeatherAttemptPayload, NativeWirePrimitives, RetryingAmbientWeatherProvider,
    WorkerAmbientWeatherTransport, system_worker_health_milliseconds,
};

const AMBIENT_ENDPOINT: &str = "https://lightning.ambientweather.net/device-data";
const AMBIENT_RESPONSE_LIMIT: &str = "10000";
const MAXIMUM_RANGE_MILLISECONDS: i64 = 31 * 86_400_000;
const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Shared process transport for public Ambient Weather device observations.
pub struct NativeAmbientWeatherTransport {
    client: Arc<NativeOutboundHttp>,
    endpoint: Url,
}

impl NativeAmbientWeatherTransport {
    /// Start one retained native client for scheduled and archive Ambient requests.
    pub fn new() -> Result<Self, NativeOutboundHttpError> {
        let client = Arc::new(NativeOutboundHttp::new()?);
        Self::with_client(client)
    }

    /// Adopt an existing process client without creating another runtime or pool.
    pub fn with_client(client: Arc<NativeOutboundHttp>) -> Result<Self, NativeOutboundHttpError> {
        let endpoint =
            Url::parse(AMBIENT_ENDPOINT).map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        Ok(Self { client, endpoint })
    }

    /// Adopt one finite fixture endpoint while preserving the production codec.
    #[cfg(test)]
    pub(crate) const fn with_endpoint(client: Arc<NativeOutboundHttp>, endpoint: Url) -> Self {
        Self { client, endpoint }
    }
}

impl WorkerAmbientWeatherTransport for NativeAmbientWeatherTransport {
    type Exchange<'a>
        = OwnedNativeProviderExchange<AmbientWeatherCodec>
    where
        Self: 'a;

    /// Bind the exact ordered range request without starting provider traffic.
    fn bind_request(
        &mut self,
        request: &AmbientWeatherRangeRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure> {
        let outbound = ambient_request(self.endpoint.clone(), request)
            .map_err(|_| invalid_task("invalid_payload"))?;
        Ok(OwnedNativeProviderExchange::new(
            Arc::clone(&self.client),
            AmbientWeatherCodec {
                attempt: 1,
                request: outbound,
            },
        ))
    }
}

impl AmbientWeatherBackfillProvider for NativeAmbientWeatherTransport {
    /// Reuse the scheduled request codec and retry policy for one archive chunk.
    fn fetch(
        &mut self,
        request: AmbientWeatherRangeRequest,
        deadline_at_ms: i64,
    ) -> Result<
        ProviderBatch<
            NormalizedAmbientWeatherRecord,
            AmbientWeatherResponseMetadata,
            Option<AmbientWeatherProviderCursor>,
        >,
        PublicStationBackfillOperationError,
    > {
        let exchange = WorkerAmbientWeatherTransport::bind_request(self, &request)
            .map_err(backfill_task_failure)?;
        let mut provider = RetryingAmbientWeatherProvider::with_controls(
            exchange,
            production_controls(),
            Some(deadline_at_ms),
        );
        ScheduledProvider::fetch(&mut provider, request).map_err(backfill_task_failure)
    }
}

/// Request-bound decoder for one Ambient Weather device-data response.
pub struct AmbientWeatherCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<AmbientWeatherAttemptPayload> for AmbientWeatherCodec {
    /// Reuse the exact logical range while retaining controller-owned attempt evidence.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Decode the bounded response into the established typed Ambient payload.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<AmbientWeatherAttemptPayload>, ProviderAttemptError> {
        let decoded = decode_provider_json(response, self.attempt)?;
        let upstream_response_checksum =
            ordered_response_checksum(std::slice::from_ref(&decoded.checksum));
        // parse only successful bodies because the controller owns HTTP failure classification
        let payload = if (200..300).contains(&decoded.status) {
            AmbientWeatherAttemptPayload {
                payload: decode_ambient_payload(&decoded.body).map_err(|message| {
                    ambient_payload_failure(self.attempt, decoded.status, message)
                })?,
                received_at_ms: system_worker_health_milliseconds().unwrap_or(0),
                upstream_response_checksum,
            }
        } else {
            AmbientWeatherAttemptPayload {
                payload: AmbientWeatherPayload::default(),
                received_at_ms: 0,
                upstream_response_checksum,
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

/// Build the exact original endpoint and URLSearchParams insertion order.
fn ambient_request(
    mut endpoint: Url,
    request: &AmbientWeatherRangeRequest,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    validate_request(request)?;
    let end_inclusive = request
        .end_exclusive_ms
        .checked_sub(1)
        .ok_or(NativeOutboundHttpError::InvalidRequest)?;
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    endpoint
        .query_pairs_mut()
        .append_pair("macAddress", &request.mac_address)
        .append_pair("start", &request.start_ms.to_string())
        .append_pair("end", &end_inclusive.to_string())
        .append_pair("limit", AMBIENT_RESPONSE_LIMIT);
    json_get(endpoint)
}

/// Validate every request field consumed by the original Ambient range owner.
fn validate_request(request: &AmbientWeatherRangeRequest) -> Result<(), NativeOutboundHttpError> {
    let duration = request
        .end_exclusive_ms
        .checked_sub(request.start_ms)
        .ok_or(NativeOutboundHttpError::InvalidRequest)?;
    // retain the original bounded range, source, timezone, and public identity contract
    if request.source_id.is_empty()
        || request.source_id.encode_utf16().count() > 128
        || request.timezone.is_empty()
        || request.timezone.encode_utf16().count() > 64
        || !valid_device_id(&request.device_id)
        || !valid_mac_address(&request.mac_address)
        || request.start_ms % 1_000 != 0
        || request.end_exclusive_ms % 1_000 != 0
        || duration <= 0
        || duration > MAXIMUM_RANGE_MILLISECONDS
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(())
}

/// Recognize the exact lowercase 24-digit Ambient device identifier.
fn valid_device_id(value: &str) -> bool {
    value.len() == 24
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Recognize the exact uppercase colon-delimited Ambient hardware address.
fn valid_mac_address(value: &str) -> bool {
    let bytes = value.as_bytes();
    // require six uppercase hexadecimal octets with no alternate separators
    bytes.len() == 17
        && bytes.iter().enumerate().all(|(index, byte)| {
            // distinguish the five fixed separators from hexadecimal bytes
            if matches!(index, 2 | 5 | 8 | 11 | 14) {
                *byte == b':'
            } else {
                byte.is_ascii_digit() || (b'A'..=b'F').contains(byte)
            }
        })
}

/// Project the provider JSON object into the transport-neutral Ambient payload.
fn decode_ambient_payload(value: &JsonValue) -> Result<AmbientWeatherPayload, String> {
    // require the original top-level data array without coercion
    let data = match field(value, "data") {
        Some(JsonValue::Array(data)) => data,
        _ => return Err("Ambient data must be an array".to_owned()),
    };
    let mut observations = Vec::with_capacity(data.len());
    // retain provider order because normalization owns timestamp sorting and first-row deduplication
    for (index, value) in data.iter().enumerate() {
        let JsonValue::Object(_) = value else {
            return Err(format!("data[{index}] must be an object"));
        };
        observations.push(AmbientWeatherObservation {
            apparent_temperature_f: nullable_number(value, "feelsLike", index)?,
            date_utc_ms: required_integer(value, "dateutc", index)?,
            device_identity: device_identity(value),
            hourly_rain_in: nullable_number(value, "hourlyrainin", index)?,
            pressure_in_hg: nullable_number(value, "baromrelin", index)?,
            relative_humidity_percent: nullable_number(value, "humidity", index)?,
            solar_radiation_wm2: nullable_number(value, "solarradiation", index)?,
            temperature_f: nullable_number(value, "tempf", index)?,
            uv_index: nullable_number(value, "uv", index)?,
            wind_direction_degrees: nullable_number(value, "winddir", index)?,
            wind_gust_mph: nullable_number(value, "windgustmph", index)?,
            wind_speed_mph: nullable_number(value, "windspeedmph", index)?,
        });
    }
    Ok(AmbientWeatherPayload { observations })
}

/// Decode one required positive JavaScript-safe integer observation timestamp.
fn required_integer(object: &JsonValue, name: &str, index: usize) -> Result<i64, String> {
    let Some(JsonValue::Number(value)) = field(object, name) else {
        return Err(format!("data[{index}].{name} must be a positive integer"));
    };
    // preserve Number.isSafeInteger and the provider's positive timestamp rule
    if !value.is_finite() || value.fract() != 0.0 || !(1.0..=MAXIMUM_SAFE_INTEGER).contains(value) {
        return Err(format!("data[{index}].{name} must be a positive integer"));
    }
    Ok(*value as i64)
}

/// Decode one provider number while preserving missing and explicit null values.
fn nullable_number(object: &JsonValue, name: &str, index: usize) -> Result<Option<f64>, String> {
    // preserve only the original missing null or finite-number alternatives
    match field(object, name) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::Number(value)) if value.is_finite() => Ok(Some(*value)),
        _ => Err(format!(
            "data[{index}].{name} must be a finite number or null"
        )),
    }
}

/// Decode optional identity without collapsing invalid values into absence.
fn device_identity(object: &JsonValue) -> AmbientWeatherDeviceIdentity {
    // retain malformed identity evidence for the existing normalizer to reject
    match field(object, "deviceId") {
        None => AmbientWeatherDeviceIdentity::Missing,
        Some(JsonValue::String(value)) => String::from_utf16(value).map_or(
            AmbientWeatherDeviceIdentity::Invalid,
            AmbientWeatherDeviceIdentity::Value,
        ),
        _ => AmbientWeatherDeviceIdentity::Invalid,
    }
}

/// Hash each lowercase raw response checksum plus a newline in response order.
fn ordered_response_checksum(responses: &[Sha256Digest]) -> String {
    let mut material = Vec::with_capacity(responses.len().saturating_mul(65));
    // preserve response order and the trailing newline on every checksum boundary
    for response in responses {
        material.extend_from_slice(response.as_str().as_bytes());
        material.push(b'\n');
    }
    lower_hex(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &material,
    ))
}

/// Render one fixed SHA-256 output using lowercase hexadecimal.
fn lower_hex(digest: [u8; 32]) -> String {
    let mut encoded = String::with_capacity(64);
    // write every digest byte without another formatting dependency
    for byte in digest {
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

/// Classify one successful-HTTP Ambient schema failure for the retry controller.
fn ambient_payload_failure(attempt: u8, status: u16, message: String) -> ProviderAttemptError {
    ProviderAttemptError::Classified(ProviderFailure {
        attempts: attempt,
        content_budget_role: None,
        error: ProviderError {
            classification: ProviderFailureClassification::InvalidPayload,
            code: "invalid_payload".to_owned(),
            message,
            retry_after_ms: None,
        },
        status: Some(status),
    })
}

/// Return the original ten-megabyte bounded retry controls.
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

/// Map one scheduled failure into the archive run's durable classification.
fn backfill_task_failure(failure: TaskFailure) -> PublicStationBackfillOperationError {
    // map each stable scheduled code to the matching durable archive classification
    let classification = match failure.code.as_str() {
        "provider_rate_limited" => TideBackfillErrorClassification::RateLimited,
        "provider_request_rejected" => TideBackfillErrorClassification::Permanent,
        "invalid_json" | "invalid_payload" | "provider_response_too_large" => {
            TideBackfillErrorClassification::InvalidPayload
        }
        _ => TideBackfillErrorClassification::Retryable,
    };
    PublicStationBackfillOperationError::provider(
        failure.attempts.max(1),
        classification,
        failure.code,
        "provider request failed",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    /// Build one exact Ambient range accepted by both scheduled and backfill owners.
    fn request() -> AmbientWeatherRangeRequest {
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

    /// Exercise the real pooled client and typed Ambient projection against loopback only.
    #[test]
    fn scheduled_exchange_preserves_request_projection_and_checksum() {
        let body = concat!(
            "{\"data\":[{\"baromrelin\":30,\"dateutc\":1787576400000,",
            "\"deviceId\":\"5fe7d16a25083b000171f8e5\",\"feelsLike\":54,",
            "\"hourlyrainin\":0.1,\"humidity\":90,\"solarradiation\":100,",
            "\"tempf\":53.6,\"uv\":1,\"winddir\":360,\"windgustmph\":10,",
            "\"windspeedmph\":5}]}"
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            // consume the complete request headers before writing the finite response
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                // reject an incomplete request instead of waiting forever after EOF
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body,
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(request).unwrap()
        });
        let client = Arc::new(NativeOutboundHttp::new().unwrap());
        let endpoint = Url::parse(&format!("http://{address}/device-data")).unwrap();
        let mut transport = NativeAmbientWeatherTransport::with_endpoint(client, endpoint);
        let range = request();
        let exchange = transport.bind_request(&range).unwrap();
        let mut provider = RetryingAmbientWeatherProvider::with_controls(
            exchange,
            ProviderControls {
                max_attempts: 1,
                max_body_bytes: 10_000_000,
                timeout_ms: 5_000,
            },
            None,
        );
        let batch = ScheduledProvider::fetch(&mut provider, range).unwrap();
        let observed = server.join().unwrap();

        assert!(observed.starts_with(concat!(
            "GET /device-data?macAddress=F0%3A08%3AD1%3A07%3AB3%3A9E",
            "&start=1787576400000&end=1787579999999&limit=10000 HTTP/1.1\r\n"
        )));
        assert!(
            observed
                .to_ascii_lowercase()
                .contains("accept: application/json\r\n")
        );
        assert_eq!(batch.attempts, 1);
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].valid_at_ms, 1_787_576_400_000);
        assert_eq!(
            batch.records[0].metrics.temperature_c,
            Some(12.000000000000002)
        );
        assert_eq!(
            batch.upstream_response_checksum,
            "96ca5e2a2e8dd2027e6071c89e0c4b89eb73d76ecb21658ef9e6791b1ddff010"
        );
    }

    /// Prove that the batch checksum retains response order and every newline boundary.
    #[test]
    fn ordered_checksum_changes_with_response_order() {
        let first = Sha256Digest::admitted("0".repeat(64));
        let second = Sha256Digest::admitted("1".repeat(64));
        assert_ne!(
            ordered_response_checksum(&[first.clone(), second.clone()]),
            ordered_response_checksum(&[second, first])
        );
    }

    /// Reject malformed numeric fields without collapsing them into missing data.
    #[test]
    fn typed_projection_rejects_numeric_strings() {
        let payload = JsonValue::Object(vec![(
            "data".encode_utf16().collect(),
            JsonValue::Array(vec![JsonValue::Object(vec![
                (
                    "dateutc".encode_utf16().collect(),
                    JsonValue::Number(1_787_576_400_000.0),
                ),
                (
                    "tempf".encode_utf16().collect(),
                    JsonValue::String("53.6".encode_utf16().collect()),
                ),
            ])]),
        )]);
        assert_eq!(
            decode_ambient_payload(&payload).unwrap_err(),
            "data[0].tempf must be a finite number or null"
        );
    }
}
