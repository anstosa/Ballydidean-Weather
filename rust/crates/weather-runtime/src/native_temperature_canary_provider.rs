//! Native single-run ECMWF canary provider over the process-shared HTTP and JSON owner.

use std::sync::Arc;

use url::Url;
use weather_contracts::JsonValue;
use weather_domain::validate_coordinates;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_providers::{
    ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse, ProviderControls,
    ProviderRequestPolicy, ProviderRunError, run_provider_request,
};
use weather_worker::{
    OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION, OPEN_METEO_ECMWF_SINGLE_RUN_MODEL, TaskFailure,
    TemperatureCanaryBatch, TemperatureCanaryFetchControls, TemperatureCanaryHour,
    TemperatureCanaryProvider, TemperatureCanaryRequest,
};

use crate::{
    NativeOutboundHttp, NativeWirePrimitives,
    maintenance_canonical_json::{field, text},
    native_open_meteo_transport::{
        DecodedProviderJson, compatibility_endpoint, decode_provider_json, json_get, number_text,
    },
    native_outbound_http::{
        NativeOutboundHttpRequest, NativeOutboundHttpResponse, NativeProviderCodec,
        NativeProviderExchange,
    },
    system_worker_health_milliseconds,
};

const SINGLE_RUN_ENDPOINT: &str = "https://single-runs-api.open-meteo.com/v1/forecast";
const SINGLE_RUN_PATH: &str = "/v1/forecast";
const HOUR_MS: i64 = 3_600_000;
const FOUR_RUN_HOURS: i64 = 6 * HOUR_MS;
const MODEL_50R1_CUTOFF_MS: i64 = 1_778_565_600_000;
const HOURS: usize = 19;

/// One retained provider; the original configured compatibility origin is optional.
pub struct NativeTemperatureCanaryProvider {
    client: Arc<NativeOutboundHttp>,
    compatibility_origin: Option<String>,
}

impl NativeTemperatureCanaryProvider {
    /// Share the worker's admitted outbound client without another network runtime.
    pub const fn new(
        client: Arc<NativeOutboundHttp>,
        compatibility_origin: Option<String>,
    ) -> Self {
        Self {
            client,
            compatibility_origin,
        }
    }

    /// Resolve the official provider path or the existing admitted compatibility origin.
    fn endpoint(&self) -> Result<Url, TaskFailure> {
        // preserve the configured origin only with the fixed predecessor path
        match self.compatibility_origin.as_deref() {
            Some(origin) => compatibility_endpoint(origin, SINGLE_RUN_PATH)
                .map_err(|_| failure("invalid_request", 0)),
            None => Url::parse(SINGLE_RUN_ENDPOINT).map_err(|_| failure("invalid_request", 0)),
        }
    }
}

impl TemperatureCanaryProvider for NativeTemperatureCanaryProvider {
    /// Fetch and normalize one real bounded single-run response with its retained receipt identity.
    fn fetch_single_run(
        &mut self,
        request: &TemperatureCanaryRequest,
        controls: TemperatureCanaryFetchControls,
    ) -> Result<TemperatureCanaryBatch, TaskFailure> {
        let initialized = request_epoch(request)?;
        let url = build_url(self.endpoint()?, request)?;
        let request_wire = json_get(url).map_err(|_| failure("invalid_request", 0))?;
        let mut exchange = NativeProviderExchange::new(
            &self.client,
            SingleRunCodec {
                attempt: 1,
                request: request_wire,
            },
        );
        let response = run_provider_request(
            ProviderRequestPolicy {
                controls: ProviderControls {
                    max_attempts: controls.max_attempts,
                    max_body_bytes: controls
                        .max_body_bytes
                        .try_into()
                        .map_err(|_| failure("invalid_request", 0))?,
                    timeout_ms: controls.timeout_ms,
                },
                deadline_at_ms: None,
            },
            &mut exchange,
        )
        .map_err(provider_failure)?;
        // sample the real receipt clock after successful body decoding and before normalization
        let received_at = system_worker_health_milliseconds()
            .and_then(|value| NativeWirePrimitives.format_utc_milliseconds(value))
            .ok_or_else(|| failure("invalid_payload", response.attempts))?;
        let hours = normalize(&response.body.body, initialized)
            .ok_or_else(|| failure("invalid_payload", response.attempts))?;
        Ok(TemperatureCanaryBatch {
            adapter_version: OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION.to_owned(),
            attempts: response.attempts,
            hours,
            model_cycle: if initialized < MODEL_50R1_CUTOFF_MS {
                "49r1"
            } else {
                "50r1"
            }
            .to_owned(),
            provider_response_sha256: response.body.checksum.as_str().to_owned(),
            received_at,
            run_initialized_at: request.run_initialized_at.clone(),
            upstream_model: OPEN_METEO_ECMWF_SINGLE_RUN_MODEL.to_owned(),
        })
    }
}

/// Retain the validated provider request across the controller-owned attempt(s).
struct SingleRunCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<DecodedProviderJson> for SingleRunCodec {
    /// Use the exact URL with the controller's actual attempt number.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Reuse the common HTTP classification and exact provider checksum decoder.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<DecodedProviderJson>, ProviderAttemptError> {
        let body = decode_provider_json(response, self.attempt)?;
        Ok(ProviderAttemptResponse {
            content: body.content,
            provider_reason: body.provider_reason.clone(),
            retry_after_ms: body.retry_after_ms,
            status: body.status,
            body,
        })
    }
}

/// Admit the genuine frozen six-hour initialization and the original coordinate domains.
fn request_epoch(request: &TemperatureCanaryRequest) -> Result<i64, TaskFailure> {
    validate_coordinates(request.latitude, request.longitude)
        .map_err(|_| failure("invalid_request", 0))?;
    NativeWirePrimitives
        .canonical_utc_milliseconds(&request.run_initialized_at)
        .filter(|value| {
            value.rem_euclid(FOUR_RUN_HOURS) == 0 && request.run_initialized_at.len() == 24
        })
        .ok_or_else(|| failure("invalid_request", 0))
}

/// Preserve every original ordered provider parameter without changing the source URL.
fn build_url(mut endpoint: Url, request: &TemperatureCanaryRequest) -> Result<Url, TaskFailure> {
    request_epoch(request)?;
    let latitude = number_text(request.latitude).map_err(|_| failure("invalid_request", 0))?;
    let longitude = number_text(request.longitude).map_err(|_| failure("invalid_request", 0))?;
    endpoint
        .query_pairs_mut()
        .append_pair("latitude", &latitude)
        .append_pair("longitude", &longitude)
        .append_pair(
            "hourly",
            "temperature_2m,relative_humidity_2m,wind_speed_10m",
        )
        .append_pair("models", OPEN_METEO_ECMWF_SINGLE_RUN_MODEL)
        .append_pair("run", &request.run_initialized_at[..16])
        .append_pair("forecast_hours", "19")
        .append_pair("temperature_unit", "celsius")
        .append_pair("wind_speed_unit", "ms")
        .append_pair("timezone", "UTC");
    Ok(endpoint)
}

/// Normalize the complete 19-hour grid and omit only the initialization analysis value.
fn normalize(payload: &JsonValue, initialized: i64) -> Option<Vec<TemperatureCanaryHour>> {
    // the original envelope requires UTC or GMT at exact zero offset
    if !matches!(field(payload, "utc_offset_seconds"), Some(JsonValue::Number(value)) if *value == 0.0)
        || !matches!(text(payload, "timezone").as_deref(), Some("UTC" | "GMT"))
    {
        return None;
    }
    let units = field(payload, "hourly_units")?;
    // require all four source units before consuming the parallel arrays
    for (field, expected) in [
        ("time", "iso8601"),
        ("temperature_2m", "°C"),
        ("relative_humidity_2m", "%"),
        ("wind_speed_10m", "m/s"),
    ] {
        if text(units, field).as_deref() != Some(expected) {
            return None;
        }
    }
    let hourly = field(payload, "hourly")?;
    let times = hourly_array(hourly, "time")?;
    let temperatures = hourly_array(hourly, "temperature_2m")?;
    let humidity = hourly_array(hourly, "relative_humidity_2m")?;
    let winds = hourly_array(hourly, "wind_speed_10m")?;
    let mut hours = Vec::with_capacity(HOURS - 1);
    // validate every provider timestamp including lead zero before retaining model inputs
    for lead in 0..HOURS {
        let JsonValue::String(units) = &times[lead] else {
            return None;
        };
        let time = String::from_utf16(units).ok()?;
        let expected_ms =
            initialized.checked_add(i64::try_from(lead).ok()?.checked_mul(HOUR_MS)?)?;
        let expected = NativeWirePrimitives.format_utc_milliseconds(expected_ms)?;
        // require the original exact minute-resolution UTC provider timestamp
        if time.len() != 16 || expected.get(..16)? != time || !time.ends_with(":00") {
            return None;
        }
        // initialization analysis metrics are deliberately not read by the predecessor
        if lead == 0 {
            continue;
        }
        hours.push(TemperatureCanaryHour {
            model_lead_hours: u8::try_from(lead).ok()?,
            raw_relative_humidity_percent: metric(&humidity[lead], 0.0, 100.0)?,
            raw_temperature_c: metric(&temperatures[lead], -100.0, 70.0)??,
            raw_wind_speed_mps: metric(&winds[lead], 0.0, 150.0)?,
            valid_at: expected,
            valid_at_ms: expected_ms,
        });
    }
    Some(hours)
}

/// Require an exact full-length hourly array, including analysis at index zero.
fn hourly_array<'a>(document: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(document, key)? else {
        return None;
    };
    (values.len() == HOURS).then_some(values.as_slice())
}

/// Preserve nullable finite metrics within the actual original physical domain.
fn metric(value: &JsonValue, minimum: f64, maximum: f64) -> Option<Option<f64>> {
    match value {
        JsonValue::Null => Some(None),
        JsonValue::Number(value) if value.is_finite() && (minimum..=maximum).contains(value) => {
            Some(Some(*value))
        }
        _ => None,
    }
}

/// Preserve bounded controller categories and attempt count across the collector boundary.
fn provider_failure(error: ProviderRunError) -> TaskFailure {
    match error {
        ProviderRunError::Provider(value) => TaskFailure {
            attempts: u32::from(value.attempts),
            code: value.error.code,
        },
        ProviderRunError::InvalidControls(_) => failure("invalid_request", 0),
    }
}

/// Retain the collector's stable task failure category without provider body content.
fn failure(code: &str, attempts: u8) -> TaskFailure {
    TaskFailure {
        attempts: u32::from(attempts),
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify exact provider grid and request parameter order without network traffic.
    #[test]
    fn native_temperature_canary_provider_uses_original_single_run_contract() {
        let request = TemperatureCanaryRequest {
            latitude: 47.95,
            longitude: -122.42,
            run_initialized_at: "2026-11-24T00:00:00.000Z".to_owned(),
        };
        let epoch = request_epoch(&request).unwrap();
        let url = build_url(Url::parse(SINGLE_RUN_ENDPOINT).unwrap(), &request).unwrap();
        assert_eq!(
            url.as_str(),
            "https://single-runs-api.open-meteo.com/v1/forecast?latitude=47.95&longitude=-122.42&hourly=temperature_2m%2Crelative_humidity_2m%2Cwind_speed_10m&models=ecmwf_ifs&run=2026-11-24T00%3A00&forecast_hours=19&temperature_unit=celsius&wind_speed_unit=ms&timezone=UTC"
        );
        let times: Vec<_> = (0..HOURS)
            .map(|lead| {
                NativeWirePrimitives
                    .format_utc_milliseconds(epoch + i64::try_from(lead).unwrap() * HOUR_MS)
                    .unwrap()[..16]
                    .to_owned()
            })
            .collect();
        let mut temperatures = vec![Some(5.0); HOURS];
        temperatures[0] = None;
        let payload = serde_json::json!({
            "utc_offset_seconds": 0, "timezone": "UTC",
            "hourly_units": {"time":"iso8601","temperature_2m":"°C","relative_humidity_2m":"%","wind_speed_10m":"m/s"},
            "hourly": {"time":times,"temperature_2m":temperatures,"relative_humidity_2m":vec![None::<f64>; HOURS],"wind_speed_10m":vec![2.0; HOURS]}
        });
        let payload = NativeWirePrimitives
            .decode_json(&serde_json::to_vec(&payload).unwrap())
            .unwrap();
        let rows = normalize(&payload, epoch).unwrap();
        assert_eq!(rows.len(), 18);
        assert_eq!(rows[0].model_lead_hours, 1);
        assert_eq!(rows[0].raw_temperature_c, 5.0);
        assert_eq!(rows[0].raw_relative_humidity_percent, None);
        assert_eq!(rows[17].valid_at, "2026-11-24T18:00:00.000Z");
    }
}
