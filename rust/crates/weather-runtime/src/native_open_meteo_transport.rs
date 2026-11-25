//! Native pooled HTTP bindings for Open-Meteo Archive and Previous Runs.

use std::sync::Arc;

use hyper::Method;
use hyper::header::{ACCEPT, HeaderValue, RETRY_AFTER};
use url::Url;
use weather_contracts::JsonValue;
use weather_database::Sha256Digest;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_providers::{
    ContentBudget, ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse, ProviderError,
    ProviderFailure, ProviderFailureClassification,
};
use weather_worker::{ForecastAnchorBackfillOperationError, TideBackfillOperationError};

use crate::maintenance_canonical_json::field;
use crate::native_outbound_http::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
    NativeOutboundHttpResponse, NativeProviderCodec, NativeProviderExchange,
};
use crate::{
    NativeWirePrimitives, OpenMeteoArchiveEndpoint, OpenMeteoArchivePayload, OpenMeteoArchiveQuery,
    OpenMeteoArchiveQueryValue, OpenMeteoArchiveTransport, OpenMeteoPreviousRunsEndpoint,
    OpenMeteoPreviousRunsPayload, OpenMeteoPreviousRunsQuery, OpenMeteoPreviousRunsTransport,
};

const MAXIMUM_RETRY_AFTER_MILLISECONDS: f64 = 30_000.0;

/// Production Open-Meteo transport sharing one retained pooled outbound client.
pub struct NativeOpenMeteoTransport {
    client: Arc<NativeOutboundHttp>,
}

impl NativeOpenMeteoTransport {
    /// Start the retained native outbound client used by every bound exchange.
    pub fn new() -> Result<Self, NativeOutboundHttpError> {
        NativeOutboundHttp::new().map(|client| Self {
            client: Arc::new(client),
        })
    }

    /// Adopt one already-started native client.
    pub fn with_client(client: NativeOutboundHttp) -> Self {
        Self {
            client: Arc::new(client),
        }
    }

    /// Share an existing process-level client without creating another runtime.
    pub const fn with_shared_client(client: Arc<NativeOutboundHttp>) -> Self {
        Self { client }
    }

    /// Borrow the retained client for another provider binding.
    pub fn client(&self) -> &NativeOutboundHttp {
        &self.client
    }
}

impl OpenMeteoArchiveTransport for NativeOpenMeteoTransport {
    type Exchange<'a>
        = NativeProviderExchange<'a, OpenMeteoArchiveCodec>
    where
        Self: 'a;

    /// Encode the exact ordered Archive query without starting provider traffic.
    fn bind_archive(
        &mut self,
        query: &OpenMeteoArchiveQuery<'_>,
    ) -> Result<Self::Exchange<'_>, TideBackfillOperationError> {
        let request = archive_request(query)
            .map_err(|error| TideBackfillOperationError::unexpected(error.to_string()))?;
        Ok(NativeProviderExchange::new(
            &self.client,
            OpenMeteoArchiveCodec {
                attempt: 1,
                request,
            },
        ))
    }
}

impl OpenMeteoPreviousRunsTransport for NativeOpenMeteoTransport {
    type Exchange<'a>
        = NativeProviderExchange<'a, OpenMeteoPreviousRunsCodec>
    where
        Self: 'a;

    /// Encode the exact ordered Previous Runs query without starting provider traffic.
    fn bind_previous_runs(
        &mut self,
        query: &OpenMeteoPreviousRunsQuery,
    ) -> Result<Self::Exchange<'_>, ForecastAnchorBackfillOperationError> {
        let request = previous_runs_request(query)
            .map_err(|error| ForecastAnchorBackfillOperationError::unexpected(error.to_string()))?;
        Ok(NativeProviderExchange::new(
            &self.client,
            OpenMeteoPreviousRunsCodec {
                attempt: 1,
                request,
            },
        ))
    }
}

/// Request-bound Archive response codec retained across retry attempts.
pub struct OpenMeteoArchiveCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<OpenMeteoArchivePayload> for OpenMeteoArchiveCodec {
    /// Reuse the exact logical request with controller-owned attempt limits.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Decode successful JSON and retain exact checksum and response policy evidence.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<OpenMeteoArchivePayload>, ProviderAttemptError> {
        let response = decode_provider_json(response, self.attempt)?;
        Ok(ProviderAttemptResponse {
            body: OpenMeteoArchivePayload {
                body: response.body,
                checksum: response.checksum,
            },
            content: response.content,
            provider_reason: response.provider_reason,
            retry_after_ms: response.retry_after_ms,
            status: response.status,
        })
    }
}

/// Request-bound Previous Runs response codec retained across retry attempts.
pub struct OpenMeteoPreviousRunsCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<OpenMeteoPreviousRunsPayload> for OpenMeteoPreviousRunsCodec {
    /// Reuse the exact logical request with controller-owned attempt limits.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Decode successful JSON and retain exact checksum and response policy evidence.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<OpenMeteoPreviousRunsPayload>, ProviderAttemptError> {
        let response = decode_provider_json(response, self.attempt)?;
        Ok(ProviderAttemptResponse {
            body: OpenMeteoPreviousRunsPayload {
                body: response.body,
                checksum: response.checksum,
            },
            content: response.content,
            provider_reason: response.provider_reason,
            retry_after_ms: response.retry_after_ms,
            status: response.status,
        })
    }
}

pub(crate) struct DecodedProviderJson {
    pub(crate) body: JsonValue,
    pub(crate) checksum: Sha256Digest,
    pub(crate) content: ContentBudget,
    pub(crate) provider_reason: Option<String>,
    pub(crate) retry_after_ms: Option<f64>,
    pub(crate) status: u16,
}

/// Build the Archive URL with URLSearchParams-compatible ordered encoding.
fn archive_request(
    query: &OpenMeteoArchiveQuery<'_>,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    let mut url = match query.endpoint {
        // preserve the fixed official endpoint
        OpenMeteoArchiveEndpoint::Official(endpoint) => parse_endpoint(endpoint)?,
        // replace any compatibility path with the admitted Archive path
        OpenMeteoArchiveEndpoint::Compatibility { origin, path } => {
            compatibility_endpoint(origin, path)?
        }
    };
    // append each field in the original insertion order
    for (name, value) in &query.parameters {
        let value = match value {
            OpenMeteoArchiveQueryValue::Number(value) => number_text(*value)?,
            OpenMeteoArchiveQueryValue::Text(value) => (*value).to_owned(),
        };
        url.query_pairs_mut().append_pair(name, &value);
    }
    json_get(url)
}

/// Build the Previous Runs URL with URLSearchParams-compatible ordered encoding.
fn previous_runs_request(
    query: &OpenMeteoPreviousRunsQuery,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    let mut url = match &query.endpoint {
        // preserve the fixed official endpoint
        OpenMeteoPreviousRunsEndpoint::Official(endpoint) => parse_endpoint(endpoint)?,
        // replace any compatibility path with the admitted Previous Runs path
        OpenMeteoPreviousRunsEndpoint::Compatibility { origin, path } => {
            compatibility_endpoint(origin, path)?
        }
    };
    // append each owned field in the original insertion order
    for parameter in &query.parameters {
        url.query_pairs_mut()
            .append_pair(parameter.name, &parameter.value);
    }
    json_get(url)
}

/// Construct one GET request with the original JSON accept header.
pub(crate) fn json_get(url: Url) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    NativeOutboundHttpRequest::new(Method::GET, url)
        .map(|request| request.header(ACCEPT, HeaderValue::from_static("application/json")))
}

/// Parse one fixed endpoint without admitting preexisting query state.
fn parse_endpoint(endpoint: &str) -> Result<Url, NativeOutboundHttpError> {
    let mut url = Url::parse(endpoint).map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

/// Resolve one fixed provider path against an admitted compatibility origin.
pub(crate) fn compatibility_endpoint(
    origin: &str,
    path: &str,
) -> Result<Url, NativeOutboundHttpError> {
    let origin = Url::parse(origin).map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
    let mut url = origin
        .join(path)
        .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

/// Render one finite f64 with JSON.stringify's number spelling.
pub(crate) fn number_text(value: f64) -> Result<String, NativeOutboundHttpError> {
    // reject non-finite coordinates instead of serializing them as null
    if !value.is_finite() {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    let mut encoded = ArchiveWirePrimitives::encode_ordered_json(
        &NativeWirePrimitives,
        &JsonValue::Number(value),
    )
    .ok_or(NativeOutboundHttpError::InvalidRequest)?;

    // remove native archive framing from the URL parameter value
    if encoded.pop() != Some(b'\n') {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    String::from_utf8(encoded).map_err(|_| NativeOutboundHttpError::InvalidRequest)
}

/// Decode Fetch text semantics, checksum bytes, and successful JSON.
pub(crate) fn decode_provider_json(
    response: NativeOutboundHttpResponse,
    attempts: u8,
) -> Result<DecodedProviderJson, ProviderAttemptError> {
    let text = String::from_utf8_lossy(&response.body);
    let checksum = Sha256Digest::admitted(lower_hex(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        text.as_bytes(),
    )));
    let decoded = ArchiveWirePrimitives::decode_json(&NativeWirePrimitives, text.as_bytes());
    let success = (200..300).contains(&response.status);

    // preserve HTTP classification ahead of JSON parsing for non-success responses
    if success && decoded.is_none() {
        return Err(invalid_json_failure(attempts, response.status));
    }
    let provider_reason = decoded.as_ref().and_then(provider_reason);
    let retry_after_ms = retry_after_milliseconds(&response);
    Ok(DecodedProviderJson {
        body: decoded.unwrap_or(JsonValue::Null),
        checksum,
        content: ContentBudget {
            declared_body_bytes: response.declared_body_bytes,
            streamed_body_bytes: response.streamed_body_bytes,
        },
        provider_reason,
        retry_after_ms,
        status: response.status,
    })
}

/// Extract one provider reason string without coercion.
fn provider_reason(value: &JsonValue) -> Option<String> {
    let JsonValue::String(reason) = field(value, "reason")? else {
        return None;
    };
    let reason = String::from_utf16(reason).ok()?;
    (!reason.trim().is_empty()).then_some(reason)
}

/// Parse the source's two Retry-After forms using the native calendar owner.
fn retry_after_milliseconds(response: &NativeOutboundHttpResponse) -> Option<f64> {
    let value = response.headers.get(RETRY_AFTER)?.to_str().ok()?;
    retry_after_hint(value, crate::system_worker_health_milliseconds)
}

/// Preserve relative seconds and absolute HTTP-date delays with the source ceiling.
fn retry_after_hint(value: &str, now: impl FnOnce() -> Option<i64>) -> Option<f64> {
    let value = value.trim();
    // the source samples its clock only for date-form retry hints
    let seconds = if value.is_empty() {
        Some(0.0)
    } else {
        value.parse::<f64>().ok()
    };
    if let Some(seconds) = seconds.filter(|seconds| seconds.is_finite() && *seconds >= 0.0) {
        return Some((seconds * 1_000.0).min(MAXIMUM_RETRY_AFTER_MILLISECONDS));
    }
    static HTTP_DATE_PARSER: jiff::fmt::rfc2822::DateTimeParser =
        jiff::fmt::rfc2822::DateTimeParser::new();
    let absolute = HTTP_DATE_PARSER
        .parse_timestamp(value)
        .ok()
        .map(|timestamp| timestamp.as_millisecond())
        .or_else(|| NativeWirePrimitives.parse_explicit_utc_milliseconds(value))?;
    let delay = absolute.saturating_sub(now()?).max(0) as f64;
    Some(delay.min(MAXIMUM_RETRY_AFTER_MILLISECONDS))
}

#[cfg(test)]
mod retry_tests {
    use super::*;

    /// Preserve the original retry ceiling for both relative and absolute valid headers.
    #[test]
    fn retry_after_supports_http_date_without_sampling_for_seconds() {
        assert_eq!(
            retry_after_hint("0.25", || panic!("relative delay")),
            Some(250.0)
        );
        assert_eq!(retry_after_hint("120", || None), Some(30_000.0));
        let now = NativeWirePrimitives
            .parse_explicit_utc_milliseconds("2026-11-24T01:02:03Z")
            .unwrap();
        assert_eq!(
            retry_after_hint("Tue, 24 Nov 2026 01:02:08 GMT", || Some(now)),
            Some(5_000.0)
        );
        assert_eq!(
            retry_after_hint("Tue, 24 Nov 2026 01:00:00 GMT", || Some(now)),
            Some(0.0)
        );
        assert_eq!(retry_after_hint("not a date", || Some(now)), None);
    }
}

/// Construct the original successful-HTTP invalid JSON classification.
fn invalid_json_failure(attempts: u8, status: u16) -> ProviderAttemptError {
    ProviderAttemptError::Classified(ProviderFailure {
        attempts,
        content_budget_role: None,
        error: ProviderError {
            classification: ProviderFailureClassification::InvalidPayload,
            code: "invalid_json".to_owned(),
            message: "provider returned invalid JSON".to_owned(),
            retry_after_ms: None,
        },
        status: Some(status),
    })
}

/// Render one digest in lowercase hexadecimal.
fn lower_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);

    // emit exactly two lowercase nibbles for every byte
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
#[path = "native_open_meteo_transport_test.rs"]
mod tests;
