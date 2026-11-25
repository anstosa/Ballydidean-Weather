//! Native pooled HTTP binding for PurpleAir public-map tokens and CSV history.

use std::sync::Arc;

use hyper::Method;
use hyper::header::{ACCEPT, HeaderName, HeaderValue, ORIGIN, REFERER, USER_AGENT};
use url::Url;
use weather_database::Sha256Digest;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_providers::{
    ContentBudget, ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse,
};
use weather_worker::{
    PublicStationNumber, PurpleAirDecodedHeader, PurpleAirDecodedHistory, PurpleAirDecodedRow,
    TaskFailure,
};

use crate::maintenance_canonical_json::field;
use crate::native_outbound_http::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
    NativeOutboundHttpResponse, NativeProviderCodec, OwnedNativeProviderExchange,
};
use crate::{
    NativeWirePrimitives, WorkerPurpleAirHistoryPlan, WorkerPurpleAirHistoryProduct,
    WorkerPurpleAirTokenPlan, WorkerPurpleAirTokenProduct, WorkerPurpleAirTransport,
    system_worker_health_milliseconds,
};

const PURPLEAIR_MAP_ORIGIN: &str = "https://map.purpleair.com";
const PURPLEAIR_REFERER: &str = "https://map.purpleair.com/";
const PURPLEAIR_USER_AGENT: &str = "Mozilla/5.0 weather.ballydidean.farm ingestion";
const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const MAXIMUM_RETRY_AFTER_MILLISECONDS: f64 = 30_000.0;

/// Shared process transport for PurpleAir's public token and history requests.
pub struct NativePurpleAirTransport {
    client: Arc<NativeOutboundHttp>,
    endpoint: Url,
    now: fn() -> Option<i64>,
    production_cache_enabled: bool,
}

impl NativePurpleAirTransport {
    /// Start one retained native client for production PurpleAir requests.
    pub fn new() -> Result<Self, NativeOutboundHttpError> {
        let client = Arc::new(NativeOutboundHttp::new()?);
        Self::with_client(client)
    }

    /// Adopt the shared process client without creating another runtime or connection pool.
    pub fn with_client(client: Arc<NativeOutboundHttp>) -> Result<Self, NativeOutboundHttpError> {
        let endpoint = Url::parse(PURPLEAIR_MAP_ORIGIN)
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        Ok(Self {
            client,
            endpoint,
            now: system_worker_health_milliseconds,
            production_cache_enabled: true,
        })
    }

    /// Adopt one finite fixture origin while disabling production-only token reuse.
    #[cfg(test)]
    pub(crate) const fn with_endpoint(
        client: Arc<NativeOutboundHttp>,
        endpoint: Url,
        now: fn() -> Option<i64>,
    ) -> Self {
        Self {
            client,
            endpoint,
            now,
            production_cache_enabled: false,
        }
    }
}

impl WorkerPurpleAirTransport for NativePurpleAirTransport {
    type TokenExchange<'a>
        = OwnedNativeProviderExchange<PurpleAirTokenCodec>
    where
        Self: 'a;
    type HistoryExchange<'a>
        = OwnedNativeProviderExchange<PurpleAirHistoryCodec>
    where
        Self: 'a;

    /// Bind the exact versioned public-map token request and browser-compatible public headers.
    fn bind_token(
        &mut self,
        plan: WorkerPurpleAirTokenPlan,
    ) -> Result<Self::TokenExchange<'_>, TaskFailure> {
        let request = token_request(self.endpoint.clone(), &plan).map_err(|_| invalid_task())?;
        Ok(OwnedNativeProviderExchange::new(
            Arc::clone(&self.client),
            PurpleAirTokenCodec { request },
        ))
    }

    /// Bind the admitted token only as a request header on the exact history request.
    fn bind_history(
        &mut self,
        token: String,
        plan: WorkerPurpleAirHistoryPlan,
    ) -> Result<Self::HistoryExchange<'_>, TaskFailure> {
        let request =
            history_request(self.endpoint.clone(), &token, &plan).map_err(|_| invalid_task())?;
        Ok(OwnedNativeProviderExchange::new(
            Arc::clone(&self.client),
            PurpleAirHistoryCodec { request },
        ))
    }

    /// Enable the operation cache only for the fixed production provider origin.
    fn purpleair_production_cache_enabled(&self) -> bool {
        self.production_cache_enabled
    }

    /// Sample the independent token-cache epoch through the retained process clock.
    fn purpleair_cache_epoch_ms(&mut self) -> i64 {
        (self.now)().unwrap_or_default()
    }

    /// Sample response receipt time only after the history exchange succeeds.
    fn purpleair_received_at_ms(&mut self) -> i64 {
        (self.now)().unwrap_or_default()
    }
}

/// Request-bound decoder for one public-map token response.
pub struct PurpleAirTokenCodec {
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<WorkerPurpleAirTokenProduct> for PurpleAirTokenCodec {
    /// Reuse the exact request while retaining controller-owned attempt evidence.
    fn request(
        &mut self,
        _attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        Ok(self.request.clone())
    }

    /// Decode Fetch text semantics without admitting the token into diagnostics or checksums.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<WorkerPurpleAirTokenProduct>, ProviderAttemptError> {
        let decoded = decode_provider_text(response);
        Ok(ProviderAttemptResponse {
            body: WorkerPurpleAirTokenProduct { body: decoded.body },
            content: decoded.content,
            provider_reason: decoded.provider_reason,
            retry_after_ms: decoded.retry_after_ms,
            status: decoded.status,
        })
    }
}

/// Request-bound decoder for one PurpleAir CSV history response.
pub struct PurpleAirHistoryCodec {
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<WorkerPurpleAirHistoryProduct> for PurpleAirHistoryCodec {
    /// Reuse the exact request while retaining controller-owned attempt evidence.
    fn request(
        &mut self,
        _attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        Ok(self.request.clone())
    }

    /// Decode successful CSV lazily and retain the checksum of Fetch-decoded response text.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<WorkerPurpleAirHistoryProduct>, ProviderAttemptError> {
        let decoded = decode_provider_text(response);
        let checksum = Sha256Digest::admitted(lower_hex(RevisionArchiveCanonicalCodec::sha256(
            &NativeWirePrimitives,
            decoded.body.as_bytes(),
        )));
        let history = if (200..300).contains(&decoded.status) {
            decode_history_csv(&decoded.body)
        } else {
            // non-success bodies are discarded after shared HTTP classification
            PurpleAirDecodedHistory {
                header: PurpleAirDecodedHeader::Missing,
                rows: Vec::new(),
            }
        };
        Ok(ProviderAttemptResponse {
            body: WorkerPurpleAirHistoryProduct { checksum, history },
            content: decoded.content,
            provider_reason: decoded.provider_reason,
            retry_after_ms: decoded.retry_after_ms,
            status: decoded.status,
        })
    }
}

/// One Fetch-decoded text response with shared controller evidence.
struct DecodedProviderText {
    body: String,
    content: ContentBudget,
    provider_reason: Option<String>,
    retry_after_ms: Option<f64>,
    status: u16,
}

/// Build the original `/v1/token` request and ordered version query.
fn token_request(
    endpoint: Url,
    plan: &WorkerPurpleAirTokenPlan,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    validate_map_version(&plan.map_version)?;
    let mut url = provider_url(endpoint, "/v1/token")?;
    url.query_pairs_mut()
        .append_pair("version", &plan.map_version);
    purpleair_get(url, None)
}

/// Build the original map-history path and URLSearchParams insertion order.
fn history_request(
    endpoint: Url,
    token: &str,
    plan: &WorkerPurpleAirHistoryPlan,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    validate_history_plan(plan)?;
    let mut url = provider_url(
        endpoint,
        &format!("/v1/sensors/{}/history/csv", plan.sensor_index),
    )?;
    let fields = plan.fields.join(",");
    let start = NativeWirePrimitives
        .format_utc_milliseconds(plan.window.start_ms)
        .ok_or(NativeOutboundHttpError::InvalidRequest)?;
    let end = NativeWirePrimitives
        .format_utc_milliseconds(plan.window.end_exclusive_ms)
        .ok_or(NativeOutboundHttpError::InvalidRequest)?;
    url.query_pairs_mut()
        .append_pair("fields", &fields)
        .append_pair("start_timestamp", &start)
        .append_pair("end_timestamp", &end)
        .append_pair("average", &plan.average.to_string());
    purpleair_get(url, Some(token))
}

/// Resolve one fixed provider path without retaining a query or fragment from the origin.
fn provider_url(mut endpoint: Url, path: &str) -> Result<Url, NativeOutboundHttpError> {
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    endpoint
        .join(path)
        .map_err(|_| NativeOutboundHttpError::InvalidRequest)
}

/// Apply the source's exact public origin, referrer, user agent, accept, and optional token headers.
fn purpleair_get(
    url: Url,
    token: Option<&str>,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    let mut request = NativeOutboundHttpRequest::new(Method::GET, url)?
        .header(ACCEPT, HeaderValue::from_static("text/plain"))
        .header(ORIGIN, HeaderValue::from_static(PURPLEAIR_MAP_ORIGIN))
        .header(REFERER, HeaderValue::from_static(PURPLEAIR_REFERER))
        .header(USER_AGENT, HeaderValue::from_static(PURPLEAIR_USER_AGENT));
    // expose the public token only on the history request
    if let Some(token) = token {
        let value =
            HeaderValue::from_str(token).map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        request = request.header(HeaderName::from_static("x-api-token"), value);
    }
    Ok(request)
}

/// Recheck the exact dotted numeric map version before provider traffic.
fn validate_map_version(value: &str) -> Result<(), NativeOutboundHttpError> {
    let mut parts = value.split('.');
    // require exactly three nonempty ASCII-numeric components
    for _ in 0..3 {
        let Some(part) = parts.next() else {
            return Err(NativeOutboundHttpError::InvalidRequest);
        };
        // reject empty and nonnumeric components
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(NativeOutboundHttpError::InvalidRequest);
        }
    }
    // reject a fourth component
    if parts.next().is_some() {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(())
}

/// Recheck fixed history controls and the positive JavaScript-safe sensor identity.
fn validate_history_plan(plan: &WorkerPurpleAirHistoryPlan) -> Result<(), NativeOutboundHttpError> {
    // reject mutated controls even when a caller bypasses the operation owner
    if plan.average != 0
        || plan.fields != crate::WORKER_PURPLEAIR_HISTORY_FIELDS
        || plan.sensor_index == 0
        || plan.sensor_index > MAXIMUM_SAFE_INTEGER as u64
        || plan.window.start_ms % 1_000 != 0
        || plan.window.end_exclusive_ms % 1_000 != 0
        || plan.window.start_ms >= plan.window.end_exclusive_ms
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(())
}

/// Apply Fetch UTF-8 replacement, HTTP reason extraction, and retry-after parsing.
fn decode_provider_text(response: NativeOutboundHttpResponse) -> DecodedProviderText {
    let body = String::from_utf8_lossy(&response.body).into_owned();
    let provider_reason = provider_reason(&body);
    let retry_after_ms = retry_after_milliseconds(&response);
    DecodedProviderText {
        body,
        content: ContentBudget {
            declared_body_bytes: response.declared_body_bytes,
            streamed_body_bytes: response.streamed_body_bytes,
        },
        provider_reason,
        retry_after_ms,
        status: response.status,
    }
}

/// Extract one nonempty JSON reason string from a non-success text body.
fn provider_reason(body: &str) -> Option<String> {
    let value = ArchiveWirePrimitives::decode_json(&NativeWirePrimitives, body.as_bytes())?;
    let weather_contracts::JsonValue::String(reason) = field(&value, "reason")? else {
        return None;
    };
    let reason = String::from_utf16(reason).ok()?;
    (!reason.trim().is_empty()).then_some(reason)
}

/// Parse the source's seconds and absolute-date Retry-After forms with its fixed ceiling.
fn retry_after_milliseconds(response: &NativeOutboundHttpResponse) -> Option<f64> {
    let value = response
        .headers
        .get(hyper::header::RETRY_AFTER)?
        .to_str()
        .ok()?;
    let value = value.trim();
    // retain seconds-form hints without consulting the process clock
    if let Ok(seconds) = value.parse::<f64>()
        && seconds.is_finite()
        && seconds >= 0.0
    {
        return Some((seconds * 1_000.0).min(MAXIMUM_RETRY_AFTER_MILLISECONDS));
    }
    static HTTP_DATE_PARSER: jiff::fmt::rfc2822::DateTimeParser =
        jiff::fmt::rfc2822::DateTimeParser::new();
    let absolute = HTTP_DATE_PARSER
        .parse_timestamp(value)
        .ok()
        .map(|timestamp| timestamp.as_millisecond())
        .or_else(|| NativeWirePrimitives.parse_explicit_utc_milliseconds(value))?;
    let delay = absolute
        .saturating_sub(system_worker_health_milliseconds()?)
        .max(0) as f64;
    Some(delay.min(MAXIMUM_RETRY_AFTER_MILLISECONDS))
}

/// Decode the source's deliberately simple unquoted CSV into lazy typed row cells.
fn decode_history_csv(csv: &str) -> PurpleAirDecodedHistory {
    let csv = csv.trim();
    // retain the missing-schema distinction for an empty decoded body
    if csv.is_empty() {
        return PurpleAirDecodedHistory {
            header: PurpleAirDecodedHeader::Missing,
            rows: Vec::new(),
        };
    }
    let mut lines = csv
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line));
    let header = lines
        .next()
        .unwrap_or_default()
        .split(',')
        .collect::<Vec<_>>();
    // reject a malformed schema before interpreting row positions
    let Some(indices) = PurpleAirHeaderIndices::new(&header) else {
        return PurpleAirDecodedHistory {
            header: PurpleAirDecodedHeader::Invalid,
            rows: Vec::new(),
        };
    };
    let mut rows = Vec::new();
    // preserve provider order and defer row diagnoses to the established normalizer
    for line in lines {
        rows.push(decode_history_row(line, header.len(), indices));
    }
    PurpleAirDecodedHistory {
        header: PurpleAirDecodedHeader::Valid,
        rows,
    }
}

/// Field positions for one exact but provider-order-independent PurpleAir schema.
#[derive(Clone, Copy)]
struct PurpleAirHeaderIndices {
    time_stamp: usize,
    sensor_index: usize,
    humidity_a: usize,
    humidity_b: usize,
    pm25_atm_a: usize,
    pm25_atm_b: usize,
    pressure_a: usize,
    pressure_b: usize,
    temperature_a: usize,
    temperature_b: usize,
}

impl PurpleAirHeaderIndices {
    /// Admit every expected field exactly once while allowing provider ordering.
    fn new(header: &[&str]) -> Option<Self> {
        const EXPECTED: [&str; 10] = [
            "time_stamp",
            "sensor_index",
            "humidity_a",
            "humidity_b",
            "pm2.5_atm_a",
            "pm2.5_atm_b",
            "pressure_a",
            "pressure_b",
            "temperature_a",
            "temperature_b",
        ];
        // reject missing, extra, and duplicate fields before resolving positions
        if header.len() != EXPECTED.len()
            || EXPECTED
                .iter()
                .any(|expected| header.iter().filter(|field| *field == expected).count() != 1)
        {
            return None;
        }
        let index = |name: &str| header.iter().position(|field| *field == name);
        Some(Self {
            time_stamp: index("time_stamp")?,
            sensor_index: index("sensor_index")?,
            humidity_a: index("humidity_a")?,
            humidity_b: index("humidity_b")?,
            pm25_atm_a: index("pm2.5_atm_a")?,
            pm25_atm_b: index("pm2.5_atm_b")?,
            pressure_a: index("pressure_a")?,
            pressure_b: index("pressure_b")?,
            temperature_a: index("temperature_a")?,
            temperature_b: index("temperature_b")?,
        })
    }
}

/// Decode one row without collapsing invalid cells into provider gaps.
fn decode_history_row(
    line: &str,
    expected_width: usize,
    indices: PurpleAirHeaderIndices,
) -> PurpleAirDecodedRow {
    let values = line.split(',').collect::<Vec<_>>();
    // return a width sentinel before indexing a malformed row
    if values.len() != expected_width {
        return PurpleAirDecodedRow {
            row_width_valid: false,
            sensor_index: None,
            timestamp_ms: None,
            humidity_a: PublicStationNumber::Missing,
            humidity_b: PublicStationNumber::Missing,
            pm25_atm_a: PublicStationNumber::Missing,
            pm25_atm_b: PublicStationNumber::Missing,
            pressure_a: PublicStationNumber::Missing,
            pressure_b: PublicStationNumber::Missing,
            temperature_a: PublicStationNumber::Missing,
            temperature_b: PublicStationNumber::Missing,
        };
    }
    PurpleAirDecodedRow {
        row_width_valid: true,
        sensor_index: decode_safe_integer(values[indices.sensor_index]),
        timestamp_ms: NativeWirePrimitives
            .parse_explicit_utc_milliseconds(values[indices.time_stamp]),
        humidity_a: decode_number(values[indices.humidity_a]),
        humidity_b: decode_number(values[indices.humidity_b]),
        pm25_atm_a: decode_number(values[indices.pm25_atm_a]),
        pm25_atm_b: decode_number(values[indices.pm25_atm_b]),
        pressure_a: decode_number(values[indices.pressure_a]),
        pressure_b: decode_number(values[indices.pressure_b]),
        temperature_a: decode_number(values[indices.temperature_a]),
        temperature_b: decode_number(values[indices.temperature_b]),
    }
}

/// Decode one Number.isSafeInteger-compatible nonempty CSV identity.
fn decode_safe_integer(value: &str) -> Option<i64> {
    // preserve the source's explicit rejection of an empty identifier cell
    if value.is_empty() {
        return None;
    }
    let number = parse_javascript_number(value)?;
    // retain finite integral values only inside the JavaScript safe range
    if !number.is_finite()
        || number.fract() != 0.0
        || !(-MAXIMUM_SAFE_INTEGER..=MAXIMUM_SAFE_INTEGER).contains(&number)
    {
        return None;
    }
    Some(number as i64)
}

/// Decode one nullable CSV metric while retaining malformed values explicitly.
fn decode_number(value: &str) -> PublicStationNumber {
    // preserve only the two source gap spellings
    if value.is_empty() || value == "null" {
        return PublicStationNumber::Missing;
    }
    parse_javascript_number(value)
        .filter(|number| number.is_finite())
        .map_or(PublicStationNumber::Invalid, PublicStationNumber::Number)
}

/// Parse the finite Number string forms used by provider CSV, including radix prefixes.
fn parse_javascript_number(value: &str) -> Option<f64> {
    let value = value.trim();
    // Number maps whitespace-only strings to zero
    if value.is_empty() {
        return Some(0.0);
    }
    // admit unsigned integer radix prefixes handled by Number
    if let Some(digits) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        return parse_radix_number(digits, 16);
    }
    // preserve Number's binary prefix form
    if let Some(digits) = value
        .strip_prefix("0b")
        .or_else(|| value.strip_prefix("0B"))
    {
        return parse_radix_number(digits, 2);
    }
    // preserve Number's octal prefix form
    if let Some(digits) = value
        .strip_prefix("0o")
        .or_else(|| value.strip_prefix("0O"))
    {
        return parse_radix_number(digits, 8);
    }
    value.parse::<f64>().ok()
}

/// Fold one unsigned JavaScript radix literal directly into binary64 precision.
fn parse_radix_number(digits: &str, radix: u32) -> Option<f64> {
    // reject a prefix without digits
    if digits.is_empty() {
        return None;
    }
    let mut number = 0.0;
    // preserve Number's rounding behavior beyond native integer width
    for byte in digits.bytes() {
        let digit = char::from(byte).to_digit(radix)?;
        number = number * f64::from(radix) + f64::from(digit);
    }
    Some(number)
}

/// Render one SHA-256 digest in lowercase hexadecimal.
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

/// Preserve the established no-traffic failure category.
fn invalid_task() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "provider_unavailable".to_owned(),
    }
}

#[cfg(test)]
#[path = "native_purpleair_transport_test.rs"]
mod tests;
