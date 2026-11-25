//! Native pooled HTTP binding for Netatmo public measurement history.

use std::sync::{Arc, mpsc};
use std::thread;

use hyper::Method;
use hyper::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue};
use url::Url;
use weather_contracts::JsonValue;
use weather_providers::{
    ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse, ProviderError, ProviderFailure,
    ProviderFailureClassification, ProviderResponse,
};
use weather_worker::{
    NetatmoMeasurementBody, NetatmoMeasurementRow, NetatmoMeasurementSeries,
    NetatmoMeasurementStatus, NetatmoMeasurementValues, PublicStationNumber, TaskFailure,
};

use crate::maintenance_canonical_json::field;
use crate::native_json::NativeJson;
use crate::native_open_meteo_transport::{decode_provider_json, json_get};
use crate::native_outbound_http::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
    NativeOutboundHttpResponse, NativeProviderCodec, NativeProviderExchange,
    OwnedNativeProviderExchange,
};
use crate::{
    NetatmoMeasurementProduct, NetatmoModulePlan, NetatmoModuleResponses, NetatmoTokenProduct,
    WorkerNetatmoTransport, system_worker_health_milliseconds,
};

const NETATMO_TOKEN_ENDPOINT: &str = "https://auth.netatmo.com/weathermap/token";
const NETATMO_MEASURE_ENDPOINT: &str = "https://app.netatmo.net/api/getmeasure";
const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const MAXIMUM_DIAGNOSTIC_UNITS: usize = 512;

/// Shared process transport for the token exchange and four concurrent module requests.
pub struct NativeNetatmoTransport {
    client: Arc<NativeOutboundHttp>,
    measure_endpoint: Url,
    now: fn() -> Option<i64>,
    token_endpoint: Url,
}

impl NativeNetatmoTransport {
    /// Start one retained pooled client for every Netatmo exchange.
    pub fn new() -> Result<Self, NativeOutboundHttpError> {
        let client = Arc::new(NativeOutboundHttp::new()?);
        Self::with_shared_client(client)
    }

    /// Adopt the process HTTP client without creating another runtime or connection pool.
    pub fn with_shared_client(
        client: Arc<NativeOutboundHttp>,
    ) -> Result<Self, NativeOutboundHttpError> {
        let token_endpoint = Url::parse(NETATMO_TOKEN_ENDPOINT)
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        let measure_endpoint = Url::parse(NETATMO_MEASURE_ENDPOINT)
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        Ok(Self {
            client,
            measure_endpoint,
            now: system_worker_health_milliseconds,
            token_endpoint,
        })
    }

    /// Adopt finite fixture endpoints and a deterministic post-response receipt clock.
    #[cfg(test)]
    pub(crate) fn with_endpoints(
        client: Arc<NativeOutboundHttp>,
        mut token_endpoint: Url,
        mut measure_endpoint: Url,
        now: fn() -> Option<i64>,
    ) -> Self {
        token_endpoint.set_query(None);
        token_endpoint.set_fragment(None);
        measure_endpoint.set_query(None);
        measure_endpoint.set_fragment(None);
        Self {
            client,
            measure_endpoint,
            now,
            token_endpoint,
        }
    }
}

impl WorkerNetatmoTransport for NativeNetatmoTransport {
    type TokenExchange<'a>
        = NativeProviderExchange<'a, NetatmoTokenCodec>
    where
        Self: 'a;
    type ModuleExchange = OwnedNativeProviderExchange<NetatmoMeasurementCodec>;

    /// Bind the fixed public visitor-token endpoint before module construction.
    fn bind_token(&mut self) -> Result<Self::TokenExchange<'_>, TaskFailure> {
        let request = token_request(self.token_endpoint.clone()).map_err(|_| invalid_request())?;
        Ok(NativeProviderExchange::new(
            &self.client,
            NetatmoTokenCodec {
                attempt: 1,
                request,
            },
        ))
    }

    /// Start all module controllers independently, retain first completion error, and drain all.
    fn run_modules<Error, Outdoor, Pressure, Rain, Wind>(
        &mut self,
        token: String,
        plans: [NetatmoModulePlan; 4],
        outdoor: Outdoor,
        pressure: Pressure,
        rain: Rain,
        wind: Wind,
    ) -> Result<NetatmoModuleResponses, Error>
    where
        Error: From<TaskFailure> + Send + 'static,
        Outdoor: FnOnce(
                &mut Self::ModuleExchange,
            ) -> Result<ProviderResponse<NetatmoMeasurementProduct>, Error>
            + Send
            + 'static,
        Pressure: FnOnce(
                &mut Self::ModuleExchange,
            ) -> Result<ProviderResponse<NetatmoMeasurementProduct>, Error>
            + Send
            + 'static,
        Rain: FnOnce(
                &mut Self::ModuleExchange,
            ) -> Result<ProviderResponse<NetatmoMeasurementProduct>, Error>
            + Send
            + 'static,
        Wind: FnOnce(
                &mut Self::ModuleExchange,
            ) -> Result<ProviderResponse<NetatmoMeasurementProduct>, Error>
            + Send
            + 'static,
    {
        let [outdoor_plan, pressure_plan, rain_plan, wind_plan] = plans;
        let mut outdoor_exchange = self.module_exchange::<Error>(&token, outdoor_plan)?;
        let mut pressure_exchange = self.module_exchange::<Error>(&token, pressure_plan)?;
        let mut rain_exchange = self.module_exchange::<Error>(&token, rain_plan)?;
        let mut wind_exchange = self.module_exchange::<Error>(&token, wind_plan)?;
        let (sender, receiver) = mpsc::channel();
        let outdoor_sender = sender.clone();
        let pressure_sender = sender.clone();
        let rain_sender = sender.clone();
        let wind_sender = sender.clone();
        let handles = [
            thread::spawn(move || {
                let result = outdoor(&mut outdoor_exchange);
                let _ = outdoor_sender.send((0, result));
            }),
            thread::spawn(move || {
                let result = pressure(&mut pressure_exchange);
                let _ = pressure_sender.send((1, result));
            }),
            thread::spawn(move || {
                let result = rain(&mut rain_exchange);
                let _ = rain_sender.send((2, result));
            }),
            thread::spawn(move || {
                let result = wind(&mut wind_exchange);
                let _ = wind_sender.send((3, result));
            }),
        ];
        drop(sender);

        let mut results: [Option<ProviderResponse<NetatmoMeasurementProduct>>; 4] =
            std::array::from_fn(|_| None);
        let mut first_error = None;
        // receive every controller completion while retaining only the first reported failure
        for _ in 0..4 {
            match receiver.recv() {
                Ok((index, Ok(response))) => results[index] = Some(response),
                Ok((_, Err(error))) => {
                    // preserve first-completion Promise.all rejection identity
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
                Err(_) => {
                    // retain a bounded task failure if a controller exits without reporting
                    if first_error.is_none() {
                        first_error = Some(Error::from(invalid_request()));
                    }
                    break;
                }
            }
        }
        // join every started controller even after the first ordinary failure
        for handle in handles {
            // treat an unexpected controller panic as a bounded transport failure
            if handle.join().is_err() && first_error.is_none() {
                first_error = Some(Error::from(invalid_request()));
            }
        }
        // return the first completed failure only after all siblings have drained
        if let Some(error) = first_error {
            return Err(error);
        }
        let [outdoor, pressure, rain, wind] = results;
        Ok(NetatmoModuleResponses {
            outdoor: outdoor.ok_or_else(|| Error::from(invalid_request()))?,
            pressure: pressure.ok_or_else(|| Error::from(invalid_request()))?,
            rain: rain.ok_or_else(|| Error::from(invalid_request()))?,
            wind: wind.ok_or_else(|| Error::from(invalid_request()))?,
        })
    }

    /// Sample one receipt instant after token and all four module exchanges settled.
    fn netatmo_received_at_ms(&mut self) -> i64 {
        (self.now)().unwrap_or_default()
    }
}

impl NativeNetatmoTransport {
    /// Bind one owned module request to the shared client for independent execution.
    fn module_exchange<Error>(
        &self,
        token: &str,
        plan: NetatmoModulePlan,
    ) -> Result<OwnedNativeProviderExchange<NetatmoMeasurementCodec>, Error>
    where
        Error: From<TaskFailure>,
    {
        let request = measurement_request(self.measure_endpoint.clone(), token, &plan)
            .map_err(|_| Error::from(invalid_request()))?;
        Ok(OwnedNativeProviderExchange::new(
            Arc::clone(&self.client),
            NetatmoMeasurementCodec {
                attempt: 1,
                request,
            },
        ))
    }
}

/// Request-bound token decoder retained across controller retries.
pub struct NetatmoTokenCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<NetatmoTokenProduct> for NetatmoTokenCodec {
    /// Reuse the exact token request while retaining controller attempt evidence.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Decode the bounded visitor token and preserve the raw response checksum.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<NetatmoTokenProduct>, ProviderAttemptError> {
        let decoded = decode_provider_json(response, self.attempt)?;
        let token = if (200..300).contains(&decoded.status) {
            decode_token(&decoded.body).map_err(|message| {
                invalid_payload_failure(self.attempt, decoded.status, &message)
            })?
        } else {
            // non-success bodies are ignored by the shared provider controller
            String::new()
        };
        Ok(ProviderAttemptResponse {
            body: NetatmoTokenProduct {
                checksum: decoded.checksum,
                token,
            },
            content: decoded.content,
            provider_reason: decoded.provider_reason,
            retry_after_ms: decoded.retry_after_ms,
            status: decoded.status,
        })
    }
}

/// Request-bound module decoder retained across controller retries.
pub struct NetatmoMeasurementCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<NetatmoMeasurementProduct> for NetatmoMeasurementCodec {
    /// Reuse the exact module request while retaining controller attempt evidence.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Decode typed lazy module rows and preserve the raw response checksum.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<NetatmoMeasurementProduct>, ProviderAttemptError> {
        let decoded = decode_provider_json(response, self.attempt)?;
        let series = if (200..300).contains(&decoded.status) {
            decode_measurement_series(&decoded.body)
        } else {
            // non-success bodies are ignored by the shared provider controller
            NetatmoMeasurementSeries {
                status: NetatmoMeasurementStatus::InvalidObject,
                body: NetatmoMeasurementBody::Invalid,
            }
        };
        Ok(ProviderAttemptResponse {
            body: NetatmoMeasurementProduct {
                checksum: decoded.checksum,
                series,
            },
            content: decoded.content,
            provider_reason: decoded.provider_reason,
            retry_after_ms: decoded.retry_after_ms,
            status: decoded.status,
        })
    }
}

/// Build the fixed visitor-token GET request.
fn token_request(mut endpoint: Url) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    json_get(endpoint)
}

/// Build one exact ordered JSON POST with the credential only in its authorization header.
fn measurement_request(
    mut endpoint: Url,
    token: &str,
    plan: &NetatmoModulePlan,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    validate_module_plan(plan)?;
    let authorization = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
    let body = encode_module_plan(plan)?;
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    NativeOutboundHttpRequest::new(Method::POST, endpoint).map(|request| {
        request
            .header(ACCEPT, HeaderValue::from_static("application/json"))
            .header(AUTHORIZATION, authorization)
            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .body(body)
    })
}

/// Recheck fixed request fields and bounded catalog identifiers before traffic.
fn validate_module_plan(plan: &NetatmoModulePlan) -> Result<(), NativeOutboundHttpError> {
    // reject drift from the closed getmeasure request grammar
    if !netatmo_identifier(&plan.device_id)
        || !netatmo_identifier(&plan.module_id)
        || plan
            .date_begin
            .parse::<i64>()
            .ok()
            .is_none_or(|value| value < 1)
        || plan
            .date_end
            .parse::<i64>()
            .ok()
            .is_none_or(|value| value < 1)
        || plan.optimize
        || plan.scale != "max"
        || !matches!(
            plan.measurement_types,
            "Temperature,Humidity"
                | "Pressure"
                | "Rain"
                | "WindStrength,WindAngle,GustStrength,GustAngle"
        )
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(())
}

/// Recognize the catalog's exact lowercase colon-delimited hardware identity.
fn netatmo_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 17
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            2 | 5 | 8 | 11 | 14 => *byte == b':',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(byte),
        })
}

/// Encode the exact JSON.stringify property order used by the TypeScript request.
fn encode_module_plan(plan: &NetatmoModulePlan) -> Result<Vec<u8>, NativeOutboundHttpError> {
    let value = JsonValue::Object(vec![
        member("date_begin", string(&plan.date_begin)),
        member("date_end", string(&plan.date_end)),
        member("device_id", string(&plan.device_id)),
        member("module_id", string(&plan.module_id)),
        member("optimize", JsonValue::Boolean(plan.optimize)),
        member("scale", string(plan.scale)),
        member("type", string(plan.measurement_types)),
    ]);
    let mut body =
        NativeJson::encode_ordered(&value).ok_or(NativeOutboundHttpError::InvalidRequest)?;
    // remove only the native archive serializer's framing line feed
    if body.pop() != Some(b'\n') {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(body)
}

/// Decode the successful visitor-token document through its bounded string contract.
fn decode_token(root: &JsonValue) -> Result<String, String> {
    // reject arrays and scalars before reading the token body
    if !matches!(root, JsonValue::Object(_)) {
        return Err("Netatmo token response must be an object".to_owned());
    }
    let Some(JsonValue::String(value)) = field(root, "body") else {
        return Err("token body must be a bounded non-empty string".to_owned());
    };
    let token = String::from_utf16(value)
        .map_err(|_| "token body must be a bounded non-empty string".to_owned())?;
    // match the source's nonempty four-kibicode-unit credential bound
    if token.is_empty() || value.len() > 4_096 {
        return Err("token body must be a bounded non-empty string".to_owned());
    }
    Ok(token)
}

/// Decode one module response while leaving row width and cells lazy for normalization.
fn decode_measurement_series(root: &JsonValue) -> NetatmoMeasurementSeries {
    let JsonValue::Object(_) = root else {
        return NetatmoMeasurementSeries {
            status: NetatmoMeasurementStatus::InvalidObject,
            body: NetatmoMeasurementBody::Invalid,
        };
    };
    let status = match field(root, "status") {
        Some(JsonValue::String(value)) if value.iter().copied().eq("ok".encode_utf16()) => {
            NetatmoMeasurementStatus::Ok
        }
        _ => NetatmoMeasurementStatus::Invalid,
    };
    let body = match field(root, "body") {
        Some(JsonValue::Null) => NetatmoMeasurementBody::Null,
        Some(JsonValue::Array(values)) if values.is_empty() => NetatmoMeasurementBody::EmptyArray,
        Some(JsonValue::Object(fields)) => NetatmoMeasurementBody::Rows(
            fields
                .iter()
                .map(|(epoch, values)| decoded_measurement_row(epoch, values))
                .collect(),
        ),
        _ => NetatmoMeasurementBody::Invalid,
    };
    NetatmoMeasurementSeries { status, body }
}

/// Decode one Object.entries row after NativeJson has applied JavaScript key ordering.
fn decoded_measurement_row(epoch: &[u16], values: &JsonValue) -> NetatmoMeasurementRow {
    let epoch_text = String::from_utf16(epoch).unwrap_or_else(|_| String::from_utf16_lossy(epoch));
    let epoch_seconds = javascript_number(&epoch_text)
        .filter(|value| value.is_finite() && value.fract() == 0.0)
        .filter(|value| (1.0..=MAXIMUM_SAFE_INTEGER).contains(value))
        .map(|value| value as i64)
        .unwrap_or_default();
    let values = match values {
        JsonValue::Array(values) => {
            NetatmoMeasurementValues::Numbers(values.iter().map(decoded_public_number).collect())
        }
        _ => NetatmoMeasurementValues::Invalid { epoch_text },
    };
    NetatmoMeasurementRow {
        epoch_seconds,
        values,
    }
}

/// Preserve one module value without collapsing a malformed JSON cell to null.
fn decoded_public_number(value: &JsonValue) -> PublicStationNumber {
    match value {
        JsonValue::Null => PublicStationNumber::Missing,
        JsonValue::Number(value) if value.is_finite() => PublicStationNumber::Number(*value),
        _ => PublicStationNumber::Invalid,
    }
}

/// Reproduce JavaScript Number for provider-owned object keys.
fn javascript_number(value: &str) -> Option<f64> {
    let value = value.trim();
    // JavaScript converts an empty key string to zero
    if value.is_empty() {
        return Some(0.0);
    }
    let radix = [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ]
    .into_iter()
    .find_map(|(prefix, radix)| value.strip_prefix(prefix).map(|digits| (digits, radix)));
    // admit unsigned explicit-radix keys before decimal and exponent syntax
    if let Some((digits, radix)) = radix {
        // reject an empty or malformed explicit-radix key
        if digits.is_empty() || !digits.chars().all(|digit| digit.is_digit(radix)) {
            return None;
        }
        return u64::from_str_radix(digits, radix)
            .ok()
            .map(|number| number as f64);
    }
    value.parse::<f64>().ok()
}

/// Convert one token schema failure into the controller's invalid-payload category.
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

/// Build one generated JSON member without changing property order.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Build one generated JSON string from valid process material.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Preserve the established zero-attempt request construction failure.
fn invalid_request() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "provider_unavailable".to_owned(),
    }
}

#[cfg(test)]
#[path = "native_netatmo_transport_test.rs"]
mod tests;
