//! Native scheduled Open-Meteo current and concurrent forecast transport.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, mpsc};

use url::Url;
use weather_contracts::JsonValue;
use weather_database::{SerializedJson, Sha256Digest};
use weather_domain::validate_coordinates;
use weather_edge::ArchiveWirePrimitives;
use weather_providers::{
    ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse, ProviderError, ProviderFailure,
    ProviderFailureClassification, ProviderResponse,
};
use weather_worker::{
    OpenMeteoAirQualitySample, OpenMeteoEnvelope, OpenMeteoSourceKind, OpenMeteoWeatherSample,
    ProviderRequest, TaskFailure,
};

use crate::maintenance_canonical_json::field;
use crate::native_open_meteo_transport::{
    compatibility_endpoint, decode_provider_json, json_get, number_text,
};
use crate::native_outbound_http::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
    NativeOutboundHttpResponse, NativeProviderCodec, OwnedNativeProviderExchange,
};
use crate::{
    NativeOpenMeteoValues, NativeWirePrimitives, OpenMeteoArchiveValues, OpenMeteoAttemptPayload,
    OpenMeteoForecastAirQualityProduct, OpenMeteoForecastPair, OpenMeteoForecastWeatherProduct,
    OpenMeteoParsedProduct, OpenMeteoWorkerLocation, WorkerOpenMeteoForecastTransport,
    WorkerOpenMeteoTransport, system_worker_health_milliseconds,
};

const CURRENT_ENDPOINT: &str = "https://api.open-meteo.com/v1/forecast";
const AIR_QUALITY_ENDPOINT: &str = "https://air-quality-api.open-meteo.com/v1/air-quality";
const CURRENT_PATH: &str = "/v1/forecast";
const AIR_QUALITY_PATH: &str = "/v1/air-quality";
const CURRENT_VARIABLES: &str = "temperature_2m,apparent_temperature,relative_humidity_2m,precipitation,cloud_cover,wind_speed_10m,wind_gusts_10m,wind_direction_10m,surface_pressure";
const FORECAST_VARIABLES: &str = "temperature_2m,apparent_temperature,relative_humidity_2m,precipitation,cloud_cover,wind_speed_10m,wind_gusts_10m,wind_direction_10m,surface_pressure,uv_index";

/// Shared production transport for scheduled Open-Meteo products.
pub struct NativeOpenMeteoScheduledTransport {
    client: Arc<NativeOutboundHttp>,
    compatibility_origin: Option<String>,
}

impl NativeOpenMeteoScheduledTransport {
    /// Start one retained pooled client for current and paired forecast requests.
    pub fn new(compatibility_origin: Option<String>) -> Result<Self, NativeOutboundHttpError> {
        NativeOutboundHttp::new().map(|client| Self {
            client: Arc::new(client),
            compatibility_origin,
        })
    }

    /// Adopt an existing process client and an already-admitted compatibility origin.
    pub const fn with_client(
        client: Arc<NativeOutboundHttp>,
        compatibility_origin: Option<String>,
    ) -> Self {
        Self {
            client,
            compatibility_origin,
        }
    }

    /// Resolve the weather endpoint without changing the fixed provider path.
    fn weather_endpoint(&self) -> Result<Url, NativeOutboundHttpError> {
        // route only an explicitly admitted origin through the compatibility path
        if let Some(origin) = &self.compatibility_origin {
            return compatibility_endpoint(origin, CURRENT_PATH);
        }
        Url::parse(CURRENT_ENDPOINT).map_err(|_| NativeOutboundHttpError::InvalidRequest)
    }

    /// Resolve the independent air-quality endpoint under the same compatibility origin.
    fn air_quality_endpoint(&self) -> Result<Url, NativeOutboundHttpError> {
        // route only an explicitly admitted origin through the compatibility path
        if let Some(origin) = &self.compatibility_origin {
            return compatibility_endpoint(origin, AIR_QUALITY_PATH);
        }
        Url::parse(AIR_QUALITY_ENDPOINT).map_err(|_| NativeOutboundHttpError::InvalidRequest)
    }
}

impl WorkerOpenMeteoTransport for NativeOpenMeteoScheduledTransport {
    type Exchange<'a>
        = OwnedNativeProviderExchange<CurrentCodec>
    where
        Self: 'a;

    /// Bind the exact source-owned current request to one independently retried exchange.
    fn bind_request(
        &mut self,
        location: OpenMeteoWorkerLocation,
        request: &ProviderRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure> {
        // reject cross-product use before opening provider traffic
        if request.source_kind != OpenMeteoSourceKind::ModelCurrent {
            return Err(invalid_task());
        }
        let endpoint = self.weather_endpoint().map_err(|_| invalid_task())?;
        let request =
            current_request(endpoint, location, &request.timezone).map_err(|_| invalid_task())?;
        Ok(OwnedNativeProviderExchange::new(
            Arc::clone(&self.client),
            CurrentCodec {
                attempt: 1,
                request,
            },
        ))
    }
}

impl WorkerOpenMeteoForecastTransport for NativeOpenMeteoScheduledTransport {
    type WeatherExchange = OwnedNativeProviderExchange<ForecastWeatherCodec>;
    type AirQualityExchange = OwnedNativeProviderExchange<ForecastAirQualityCodec>;

    /// Settle both independently retried products concurrently and drain both controller lanes.
    fn run_pair<Weather, AirQuality>(
        &mut self,
        location: OpenMeteoWorkerLocation,
        request: &ProviderRequest,
        weather: Weather,
        air_quality: AirQuality,
    ) -> Result<OpenMeteoForecastPair, TaskFailure>
    where
        Weather: FnOnce(
                &mut Self::WeatherExchange,
            )
                -> Result<ProviderResponse<OpenMeteoForecastWeatherProduct>, TaskFailure>
            + Send
            + 'static,
        AirQuality: FnOnce(
                &mut Self::AirQualityExchange,
            )
                -> Result<ProviderResponse<OpenMeteoForecastAirQualityProduct>, TaskFailure>
            + Send
            + 'static,
    {
        // reject cross-product use before opening either provider lane
        if request.source_kind != OpenMeteoSourceKind::Forecast {
            return Err(invalid_task());
        }
        let weather_request = forecast_weather_request(
            self.weather_endpoint().map_err(|_| invalid_task())?,
            location,
            &request.timezone,
        )
        .map_err(|_| invalid_task())?;
        let air_quality_request = forecast_air_quality_request(
            self.air_quality_endpoint().map_err(|_| invalid_task())?,
            location,
            &request.timezone,
        )
        .map_err(|_| invalid_task())?;
        let weather_exchange = OwnedNativeProviderExchange::new(
            Arc::clone(&self.client),
            ForecastWeatherCodec {
                attempt: 1,
                request: weather_request,
            },
        );
        let air_quality_exchange = OwnedNativeProviderExchange::new(
            Arc::clone(&self.client),
            ForecastAirQualityCodec {
                attempt: 1,
                request: air_quality_request,
            },
        );
        settle_pair(weather_exchange, air_quality_exchange, weather, air_quality)
    }

    /// Sample the provider receipt clock once after both forecast products succeeded.
    fn forecast_received_at_ms(&mut self) -> i64 {
        system_worker_health_milliseconds().unwrap_or(0)
    }
}

/// Request-bound codec for one scheduled current-weather exchange.
pub struct CurrentCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<OpenMeteoAttemptPayload> for CurrentCodec {
    /// Reuse the exact current request across controller-owned retry attempts.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Decode native JSON and project the successful current provider wire.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<OpenMeteoAttemptPayload>, ProviderAttemptError> {
        let decoded = decode_provider_json(response, self.attempt)?;
        let payload = if (200..300).contains(&decoded.status) {
            let received_at_ms = system_worker_health_milliseconds().unwrap_or(0);
            let (product, generation_ms) = parse_current_product(&decoded.body, self.attempt)
                .map_err(|error| with_failure_status(error, decoded.status))?;
            OpenMeteoAttemptPayload {
                product,
                received_at_ms,
                response_metadata: Some(
                    response_metadata(decoded.status, generation_ms, self.attempt)
                        .map_err(|error| with_failure_status(error, decoded.status))?,
                ),
                upstream_response_checksum: decoded.checksum.as_str().to_owned(),
            }
        } else {
            placeholder_current(decoded.checksum.as_str())
        };
        Ok(provider_response(payload, decoded))
    }
}

/// Request-bound codec for the weather half of a scheduled forecast pair.
pub struct ForecastWeatherCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<OpenMeteoForecastWeatherProduct> for ForecastWeatherCodec {
    /// Reuse the exact weather request across its independent retry controller.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Decode native JSON and project aligned hourly weather samples.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<OpenMeteoForecastWeatherProduct>, ProviderAttemptError>
    {
        let decoded = decode_provider_json(response, self.attempt)?;
        let product = if (200..300).contains(&decoded.status) {
            parse_forecast_weather_product(&decoded.body, decoded.checksum.clone(), self.attempt)
                .map_err(|error| with_failure_status(error, decoded.status))?
        } else {
            OpenMeteoForecastWeatherProduct {
                checksum: decoded.checksum.clone(),
                generation_ms: None,
                envelope: placeholder_envelope(),
                samples: Vec::new(),
            }
        };
        Ok(provider_response(product, decoded))
    }
}

/// Request-bound codec for the air-quality half of a scheduled forecast pair.
pub struct ForecastAirQualityCodec {
    attempt: u8,
    request: NativeOutboundHttpRequest,
}

impl NativeProviderCodec<OpenMeteoForecastAirQualityProduct> for ForecastAirQualityCodec {
    /// Reuse the exact air-quality request across its independent retry controller.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        self.attempt = attempt.attempt;
        Ok(self.request.clone())
    }

    /// Decode native JSON and project aligned hourly PM2.5 samples.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<OpenMeteoForecastAirQualityProduct>, ProviderAttemptError>
    {
        let decoded = decode_provider_json(response, self.attempt)?;
        let product = if (200..300).contains(&decoded.status) {
            parse_forecast_air_quality_product(
                &decoded.body,
                decoded.checksum.clone(),
                self.attempt,
            )
            .map_err(|error| with_failure_status(error, decoded.status))?
        } else {
            OpenMeteoForecastAirQualityProduct {
                checksum: decoded.checksum.clone(),
                generation_ms: None,
                samples: Vec::new(),
            }
        };
        Ok(provider_response(product, decoded))
    }
}

enum PairSettlement {
    Weather(Result<ProviderResponse<OpenMeteoForecastWeatherProduct>, TaskFailure>),
    AirQuality(Result<ProviderResponse<OpenMeteoForecastAirQualityProduct>, TaskFailure>),
}

/// Run both owned controller lanes concurrently and preserve completion-order failure selection.
fn settle_pair<Weather, AirQuality>(
    mut weather_exchange: OwnedNativeProviderExchange<ForecastWeatherCodec>,
    mut air_quality_exchange: OwnedNativeProviderExchange<ForecastAirQualityCodec>,
    weather: Weather,
    air_quality: AirQuality,
) -> Result<OpenMeteoForecastPair, TaskFailure>
where
    Weather: FnOnce(
            &mut OwnedNativeProviderExchange<ForecastWeatherCodec>,
        ) -> Result<ProviderResponse<OpenMeteoForecastWeatherProduct>, TaskFailure>
        + Send
        + 'static,
    AirQuality: FnOnce(
            &mut OwnedNativeProviderExchange<ForecastAirQualityCodec>,
        ) -> Result<ProviderResponse<OpenMeteoForecastAirQualityProduct>, TaskFailure>
        + Send
        + 'static,
{
    let (completed, settlements) = mpsc::sync_channel(2);
    std::thread::scope(|scope| {
        let weather_completed = completed.clone();
        scope.spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| weather(&mut weather_exchange)))
                .unwrap_or_else(|_| Err(transport_task()));
            let _ = weather_completed.send(PairSettlement::Weather(result));
        });
        let air_quality_completed = completed.clone();
        scope.spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| air_quality(&mut air_quality_exchange)))
                .unwrap_or_else(|_| Err(transport_task()));
            let _ = air_quality_completed.send(PairSettlement::AirQuality(result));
        });
        drop(completed);
    });

    let mut weather_product = None;
    let mut air_quality_product = None;
    let mut first_failure = None;
    // drain both siblings while retaining the first failure to settle
    for settlement in settlements {
        match settlement {
            PairSettlement::Weather(result) => match result {
                Ok(product) => weather_product = Some(product),
                Err(failure) if first_failure.is_none() => first_failure = Some(failure),
                Err(_) => {}
            },
            PairSettlement::AirQuality(result) => match result {
                Ok(product) => air_quality_product = Some(product),
                Err(failure) if first_failure.is_none() => first_failure = Some(failure),
                Err(_) => {}
            },
        }
    }
    // return the original first-settled controller failure after its sibling drains
    if let Some(failure) = first_failure {
        return Err(failure);
    }
    Ok(OpenMeteoForecastPair {
        weather: weather_product.ok_or_else(transport_task)?,
        air_quality: air_quality_product.ok_or_else(transport_task)?,
    })
}

/// Build the exact current request parameter order from the loaded site and source timezone.
fn current_request(
    mut url: Url,
    location: OpenMeteoWorkerLocation,
    timezone: &str,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    append_weather_parameters(&mut url, location, "current", CURRENT_VARIABLES, timezone)?;
    json_get(url)
}

/// Build the exact hourly weather forecast request and fixed ten-day horizon.
fn forecast_weather_request(
    mut url: Url,
    location: OpenMeteoWorkerLocation,
    timezone: &str,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    append_weather_parameters(&mut url, location, "hourly", FORECAST_VARIABLES, timezone)?;
    url.query_pairs_mut().append_pair("forecast_days", "10");
    json_get(url)
}

/// Build the exact paired air-quality request and provider-limited seven-day horizon.
fn forecast_air_quality_request(
    mut url: Url,
    location: OpenMeteoWorkerLocation,
    timezone: &str,
) -> Result<NativeOutboundHttpRequest, NativeOutboundHttpError> {
    validate_coordinates(location.latitude, location.longitude)
        .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
    url.query_pairs_mut()
        .append_pair("latitude", &number_text(location.latitude)?)
        .append_pair("longitude", &number_text(location.longitude)?)
        .append_pair("hourly", "pm2_5")
        .append_pair("forecast_days", "7")
        .append_pair("timezone", timezone);
    json_get(url)
}

/// Append the source's common weather parameters in URLSearchParams insertion order.
fn append_weather_parameters(
    url: &mut Url,
    location: OpenMeteoWorkerLocation,
    collection: &str,
    variables: &str,
    timezone: &str,
) -> Result<(), NativeOutboundHttpError> {
    validate_coordinates(location.latitude, location.longitude)
        .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
    url.query_pairs_mut()
        .append_pair("latitude", &number_text(location.latitude)?)
        .append_pair("longitude", &number_text(location.longitude)?)
        .append_pair(collection, variables)
        .append_pair("temperature_unit", "celsius")
        .append_pair("wind_speed_unit", "ms")
        .append_pair("precipitation_unit", "mm")
        .append_pair("timezone", timezone);
    Ok(())
}

struct ParsedEnvelope {
    envelope: OpenMeteoEnvelope,
    offset_seconds: i32,
}

/// Parse and validate one current provider product after successful JSON decoding.
fn parse_current_product(
    root: &JsonValue,
    attempt: u8,
) -> Result<(OpenMeteoParsedProduct, Option<f64>), ProviderAttemptError> {
    let envelope = parse_envelope(root, attempt)?;
    let current = require_object(required(root, "current", attempt)?, "current", attempt)?;
    let units = require_object(
        required(root, "current_units", attempt)?,
        "current_units",
        attempt,
    )?;
    validate_weather_units(units, false, attempt)?;
    let time = require_text(required(current, "time", attempt)?, "current.time", attempt)?;
    let valid_at_ms = resolve_current_time(
        &time,
        &envelope.envelope.upstream_timezone,
        envelope.offset_seconds,
        attempt,
    )?;
    let sample = OpenMeteoWeatherSample {
        apparent_temperature_c: nullable_number(current, "apparent_temperature", attempt)?,
        cloud_cover_percent: nullable_number(current, "cloud_cover", attempt)?,
        precipitation_mm: nullable_number(current, "precipitation", attempt)?,
        pressure_hpa: nullable_number(current, "surface_pressure", attempt)?,
        relative_humidity_percent: nullable_number(current, "relative_humidity_2m", attempt)?,
        temperature_c: nullable_number(current, "temperature_2m", attempt)?,
        uv_index: None,
        valid_at_ms,
        wind_direction_degrees: normalized_direction(current, attempt)?,
        wind_gust_mps: nullable_number(current, "wind_gusts_10m", attempt)?,
        wind_speed_mps: nullable_number(current, "wind_speed_10m", attempt)?,
    };
    Ok((
        OpenMeteoParsedProduct {
            envelope: envelope.envelope,
            sample,
        },
        generation_milliseconds(root),
    ))
}

/// Parse aligned forecast weather arrays and monotonic provider-local timestamps.
fn parse_forecast_weather_product(
    root: &JsonValue,
    checksum: Sha256Digest,
    attempt: u8,
) -> Result<OpenMeteoForecastWeatherProduct, ProviderAttemptError> {
    let envelope = parse_envelope(root, attempt)?;
    let hourly = require_object(required(root, "hourly", attempt)?, "hourly", attempt)?;
    let units = require_object(
        required(root, "hourly_units", attempt)?,
        "hourly_units",
        attempt,
    )?;
    validate_weather_units(units, true, attempt)?;
    let times = require_array(required(hourly, "time", attempt)?, "hourly.time", attempt)?;
    // reject successful empty weather products
    if times.is_empty() {
        return Err(payload_failure(attempt, "hourly.time must not be empty"));
    }
    let temperature = aligned_array(hourly, "temperature_2m", times.len(), attempt)?;
    let apparent = aligned_array(hourly, "apparent_temperature", times.len(), attempt)?;
    let humidity = aligned_array(hourly, "relative_humidity_2m", times.len(), attempt)?;
    let precipitation = aligned_array(hourly, "precipitation", times.len(), attempt)?;
    let clouds = aligned_array(hourly, "cloud_cover", times.len(), attempt)?;
    let wind_speed = aligned_array(hourly, "wind_speed_10m", times.len(), attempt)?;
    let wind_gust = aligned_array(hourly, "wind_gusts_10m", times.len(), attempt)?;
    let wind_direction = aligned_array(hourly, "wind_direction_10m", times.len(), attempt)?;
    let pressure = aligned_array(hourly, "surface_pressure", times.len(), attempt)?;
    let uv_index = aligned_array(hourly, "uv_index", times.len(), attempt)?;
    let mut values = NativeOpenMeteoValues::new();
    let mut previous = None;
    let mut samples = Vec::with_capacity(times.len());
    // preserve provider order while selecting successive timezone-fold instants
    for (index, time) in times.iter().enumerate() {
        let time = require_text(time, "hourly.time", attempt)?;
        let instant = values
            .resolve_provider_time(
                &time,
                &envelope.envelope.upstream_timezone,
                envelope.offset_seconds,
                previous.as_ref(),
            )
            .map_err(|_| payload_failure(attempt, "provider time is invalid"))?;
        samples.push(OpenMeteoWeatherSample {
            apparent_temperature_c: nullable_value(
                &apparent[index],
                "apparent_temperature",
                attempt,
            )?,
            cloud_cover_percent: nullable_value(&clouds[index], "cloud_cover", attempt)?,
            precipitation_mm: nullable_value(&precipitation[index], "precipitation", attempt)?,
            pressure_hpa: nullable_value(&pressure[index], "surface_pressure", attempt)?,
            relative_humidity_percent: nullable_value(
                &humidity[index],
                "relative_humidity_2m",
                attempt,
            )?,
            temperature_c: nullable_value(&temperature[index], "temperature_2m", attempt)?,
            uv_index: nullable_value(&uv_index[index], "uv_index", attempt)?,
            valid_at_ms: instant.epoch_milliseconds,
            wind_direction_degrees: normalized_direction_value(&wind_direction[index], attempt)?,
            wind_gust_mps: nullable_value(&wind_gust[index], "wind_gusts_10m", attempt)?,
            wind_speed_mps: nullable_value(&wind_speed[index], "wind_speed_10m", attempt)?,
        });
        previous = Some(instant);
    }
    Ok(OpenMeteoForecastWeatherProduct {
        checksum,
        generation_ms: generation_milliseconds(root),
        envelope: envelope.envelope,
        samples,
    })
}

/// Parse aligned air-quality arrays with the same provider-local time rules.
fn parse_forecast_air_quality_product(
    root: &JsonValue,
    checksum: Sha256Digest,
    attempt: u8,
) -> Result<OpenMeteoForecastAirQualityProduct, ProviderAttemptError> {
    let envelope = parse_envelope(root, attempt)?;
    let hourly = require_object(
        required(root, "hourly", attempt)?,
        "air-quality hourly",
        attempt,
    )?;
    let units = require_object(
        required(root, "hourly_units", attempt)?,
        "air-quality hourly_units",
        attempt,
    )?;
    require_unit(units, "pm2_5", "μg/m³", attempt)?;
    let times = require_array(
        required(hourly, "time", attempt)?,
        "air-quality hourly.time",
        attempt,
    )?;
    let pm25 = aligned_array(hourly, "pm2_5", times.len(), attempt)?;
    // reject empty air-quality products before timestamp projection
    if times.is_empty() {
        return Err(payload_failure(
            attempt,
            "air-quality hourly arrays must be non-empty and aligned",
        ));
    }
    let mut values = NativeOpenMeteoValues::new();
    let mut previous = None;
    let mut samples = Vec::with_capacity(times.len());
    // preserve provider order while selecting successive timezone-fold instants
    for (index, time) in times.iter().enumerate() {
        let time = require_text(time, "air-quality hourly.time", attempt)?;
        let instant = values
            .resolve_provider_time(
                &time,
                &envelope.envelope.upstream_timezone,
                envelope.offset_seconds,
                previous.as_ref(),
            )
            .map_err(|_| payload_failure(attempt, "provider time is invalid"))?;
        samples.push(OpenMeteoAirQualitySample {
            pm25_micrograms_per_cubic_meter: nullable_value(
                &pm25[index],
                "air-quality hourly.pm2_5",
                attempt,
            )?,
            valid_at_ms: instant.epoch_milliseconds,
        });
        previous = Some(instant);
    }
    Ok(OpenMeteoForecastAirQualityProduct {
        checksum,
        generation_ms: generation_milliseconds(root),
        samples,
    })
}

/// Parse shared provider envelope metadata and validate its timezone identity.
fn parse_envelope(root: &JsonValue, attempt: u8) -> Result<ParsedEnvelope, ProviderAttemptError> {
    require_object(root, "Open-Meteo response", attempt)?;
    let latitude = require_number(required(root, "latitude", attempt)?, "latitude", attempt)?;
    let longitude = require_number(required(root, "longitude", attempt)?, "longitude", attempt)?;
    validate_coordinates(latitude, longitude)
        .map_err(|_| payload_failure(attempt, "provider coordinates are invalid"))?;
    let timezone = require_text(required(root, "timezone", attempt)?, "timezone", attempt)?;
    // retain the shared domain timezone identity bound before loading host rules
    if timezone.encode_utf16().count() > 64 {
        return Err(payload_failure(
            attempt,
            "timezone must be non-empty and bounded",
        ));
    }
    let offset = require_number(
        required(root, "utc_offset_seconds", attempt)?,
        "utc_offset_seconds",
        attempt,
    )?;
    // admit only the original safe integral fourteen-hour offset range
    if offset.fract() != 0.0 || offset.abs() > 50_400.0 {
        return Err(payload_failure(attempt, "utc_offset_seconds is invalid"));
    }
    let mut values = NativeOpenMeteoValues::new();
    values
        .validate_timezone(&timezone)
        .map_err(|_| payload_failure(attempt, "provider timezone is invalid"))?;
    let elevation_m = match field(root, "elevation") {
        None | Some(JsonValue::Null) => None,
        Some(value) => Some(require_number(value, "elevation", attempt)?),
    };
    Ok(ParsedEnvelope {
        envelope: OpenMeteoEnvelope {
            elevation_m,
            latitude,
            longitude,
            upstream_timezone: timezone,
        },
        offset_seconds: offset as i32,
    })
}

/// Resolve current time only when the exact provider offset agrees with the named zone.
fn resolve_current_time(
    time: &str,
    timezone: &str,
    offset_seconds: i32,
    attempt: u8,
) -> Result<i64, ProviderAttemptError> {
    let mut values = NativeOpenMeteoValues::new();
    let first = values
        .resolve_provider_time(time, timezone, offset_seconds, None)
        .map_err(|_| payload_failure(attempt, "provider time is invalid"))?;
    // explicit instants bypass provider-offset selection in the original operation
    if has_explicit_offset(time) {
        return Ok(first.epoch_milliseconds);
    }
    let naive = explicit_utc_for_wall_time(time)
        .and_then(|instant| NativeWirePrimitives.parse_explicit_utc_milliseconds(&instant))
        .ok_or_else(|| payload_failure(attempt, "provider time is invalid"))?;
    let preferred = naive
        .checked_sub(i64::from(offset_seconds) * 1_000)
        .ok_or_else(|| payload_failure(attempt, "provider time is invalid"))?;
    // accept the earlier candidate when it matches the provider's explicit offset
    if first.epoch_milliseconds == preferred {
        return Ok(preferred);
    }
    let later = values
        .resolve_provider_time(time, timezone, offset_seconds, Some(&first))
        .map_err(|_| payload_failure(attempt, "provider time does not match its UTC offset"))?;
    // admit a fold's later candidate only when it matches the provider offset
    if later.epoch_milliseconds == preferred {
        return Ok(preferred);
    }
    Err(payload_failure(
        attempt,
        "provider time does not match its UTC offset",
    ))
}

/// Convert an admitted minute-or-second local spelling into an explicit UTC parse input.
fn explicit_utc_for_wall_time(time: &str) -> Option<String> {
    match time.len() {
        16 => Some(format!("{time}:00Z")),
        19 => Some(format!("{time}Z")),
        _ => None,
    }
}

/// Recognize the explicit offset suffix accepted by the provider conversion.
fn has_explicit_offset(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.last() == Some(&b'Z')
        || matches!(
            bytes.get(bytes.len().saturating_sub(6)..),
            Some([b'+' | b'-', a, b, b':', c, d])
                if [a, b, c, d].iter().all(|byte| byte.is_ascii_digit())
        )
}

/// Validate the exact provider weather units once before projecting samples.
fn validate_weather_units(
    units: &JsonValue,
    forecast: bool,
    attempt: u8,
) -> Result<(), ProviderAttemptError> {
    require_unit(units, "temperature_2m", "°C", attempt)?;
    require_unit(units, "apparent_temperature", "°C", attempt)?;
    require_unit(units, "relative_humidity_2m", "%", attempt)?;
    require_unit(units, "precipitation", "mm", attempt)?;
    require_unit(units, "cloud_cover", "%", attempt)?;
    require_unit(units, "wind_speed_10m", "m/s", attempt)?;
    require_unit(units, "wind_gusts_10m", "m/s", attempt)?;
    require_unit(units, "wind_direction_10m", "°", attempt)?;
    require_unit(units, "surface_pressure", "hPa", attempt)?;
    // require the dimensionless UV unit only for the forecast product
    if forecast {
        require_unit(units, "uv_index", "", attempt)?;
    }
    Ok(())
}

/// Require one exact unit spelling without accepting provider drift.
fn require_unit(
    units: &JsonValue,
    key: &str,
    expected: &str,
    attempt: u8,
) -> Result<(), ProviderAttemptError> {
    let value = required(units, key, attempt)?;
    let JsonValue::String(value) = value else {
        return Err(payload_failure(attempt, &format!("{key} unit is invalid")));
    };
    // compare raw UTF-16 units to avoid replacement or normalization
    if !value.iter().copied().eq(expected.encode_utf16()) {
        return Err(payload_failure(
            attempt,
            &format!("{key} unit must be {expected}"),
        ));
    }
    Ok(())
}

/// Require one aligned provider array for an hourly metric.
fn aligned_array<'a>(
    object: &'a JsonValue,
    key: &str,
    expected: usize,
    attempt: u8,
) -> Result<&'a [JsonValue], ProviderAttemptError> {
    let values = require_array(required(object, key, attempt)?, key, attempt)?;
    // reject positional metric drift before indexing the array
    if values.len() != expected {
        return Err(payload_failure(
            attempt,
            &format!("hourly.{key} length does not match hourly.time"),
        ));
    }
    Ok(values)
}

/// Read a required object member.
fn required<'a>(
    object: &'a JsonValue,
    key: &str,
    attempt: u8,
) -> Result<&'a JsonValue, ProviderAttemptError> {
    field(object, key).ok_or_else(|| payload_failure(attempt, &format!("{key} is required")))
}

/// Require one JSON object.
fn require_object<'a>(
    value: &'a JsonValue,
    label: &str,
    attempt: u8,
) -> Result<&'a JsonValue, ProviderAttemptError> {
    // reject nulls and arrays like the original record guard
    if !matches!(value, JsonValue::Object(_)) {
        return Err(payload_failure(
            attempt,
            &format!("{label} must be an object"),
        ));
    }
    Ok(value)
}

/// Require one JSON array.
fn require_array<'a>(
    value: &'a JsonValue,
    label: &str,
    attempt: u8,
) -> Result<&'a [JsonValue], ProviderAttemptError> {
    let JsonValue::Array(values) = value else {
        return Err(payload_failure(
            attempt,
            &format!("{label} must be an array"),
        ));
    };
    Ok(values)
}

/// Require one non-empty decoded provider string.
fn require_text(
    value: &JsonValue,
    label: &str,
    attempt: u8,
) -> Result<String, ProviderAttemptError> {
    let JsonValue::String(value) = value else {
        return Err(payload_failure(
            attempt,
            &format!("{label} must be a non-empty string"),
        ));
    };
    let value = String::from_utf16(value)
        .map_err(|_| payload_failure(attempt, &format!("{label} is invalid")))?;
    // reject strings which are empty after the source trim policy
    if value.trim().is_empty() {
        return Err(payload_failure(
            attempt,
            &format!("{label} must be a non-empty string"),
        ));
    }
    Ok(value)
}

/// Require one finite provider number.
fn require_number(
    value: &JsonValue,
    label: &str,
    attempt: u8,
) -> Result<f64, ProviderAttemptError> {
    let JsonValue::Number(value) = value else {
        return Err(payload_failure(
            attempt,
            &format!("{label} must be a finite number"),
        ));
    };
    // retain the source's explicit finite-number guard
    if !value.is_finite() {
        return Err(payload_failure(
            attempt,
            &format!("{label} must be a finite number"),
        ));
    }
    Ok(*value)
}

/// Read one required nullable numeric object member.
fn nullable_number(
    object: &JsonValue,
    key: &str,
    attempt: u8,
) -> Result<Option<f64>, ProviderAttemptError> {
    nullable_value(required(object, key, attempt)?, key, attempt)
}

/// Read one nullable provider metric value.
fn nullable_value(
    value: &JsonValue,
    label: &str,
    attempt: u8,
) -> Result<Option<f64>, ProviderAttemptError> {
    // retain documented null metric cells
    if matches!(value, JsonValue::Null) {
        return Ok(None);
    }
    require_number(value, label, attempt).map(Some)
}

/// Normalize the provider's circular north duplicate.
fn normalized_direction(
    object: &JsonValue,
    attempt: u8,
) -> Result<Option<f64>, ProviderAttemptError> {
    normalized_direction_value(required(object, "wind_direction_10m", attempt)?, attempt)
}

/// Normalize one nullable direction cell before domain validation.
fn normalized_direction_value(
    value: &JsonValue,
    attempt: u8,
) -> Result<Option<f64>, ProviderAttemptError> {
    nullable_value(value, "wind_direction_10m", attempt)
        .map(|value| value.map(|direction| if direction == 360.0 { 0.0 } else { direction }))
}

/// Read optional finite provider generation timing metadata.
fn generation_milliseconds(root: &JsonValue) -> Option<f64> {
    match field(root, "generationtime_ms") {
        Some(JsonValue::Number(value)) if value.is_finite() => Some(*value),
        _ => None,
    }
}

/// Serialize current response evidence in the original producer key order.
fn response_metadata(
    status: u16,
    generation_ms: Option<f64>,
    attempt: u8,
) -> Result<SerializedJson, ProviderAttemptError> {
    let mut fields = vec![(
        "http_status".encode_utf16().collect(),
        JsonValue::Number(f64::from(status)),
    )];
    // append generation evidence only when the provider supplied one finite number
    if let Some(generation_ms) = generation_ms {
        fields.push((
            "generation_ms".encode_utf16().collect(),
            JsonValue::Number(generation_ms),
        ));
    }
    let mut bytes = ArchiveWirePrimitives::encode_ordered_json(
        &NativeWirePrimitives,
        &JsonValue::Object(fields),
    )
    .ok_or_else(|| payload_failure(attempt, "response metadata is invalid"))?;
    // remove archive framing before admitting database JSON text
    if bytes.pop() != Some(b'\n') {
        return Err(payload_failure(attempt, "response metadata is invalid"));
    }
    String::from_utf8(bytes)
        .map(SerializedJson::admitted)
        .map_err(|_| payload_failure(attempt, "response metadata is invalid"))
}

/// Rebuild one typed response while retaining transport evidence unchanged.
fn provider_response<Product>(
    body: Product,
    decoded: crate::native_open_meteo_transport::DecodedProviderJson,
) -> ProviderAttemptResponse<Product> {
    ProviderAttemptResponse {
        body,
        content: decoded.content,
        provider_reason: decoded.provider_reason,
        retry_after_ms: decoded.retry_after_ms,
        status: decoded.status,
    }
}

/// Construct the discarded body used only for non-success HTTP classification.
fn placeholder_current(checksum: &str) -> OpenMeteoAttemptPayload {
    OpenMeteoAttemptPayload {
        product: OpenMeteoParsedProduct {
            envelope: placeholder_envelope(),
            sample: OpenMeteoWeatherSample {
                apparent_temperature_c: None,
                cloud_cover_percent: None,
                precipitation_mm: None,
                pressure_hpa: None,
                relative_humidity_percent: None,
                temperature_c: None,
                uv_index: None,
                valid_at_ms: 0,
                wind_direction_degrees: None,
                wind_gust_mps: None,
                wind_speed_mps: None,
            },
        },
        received_at_ms: 0,
        response_metadata: None,
        upstream_response_checksum: checksum.to_owned(),
    }
}

/// Construct the discarded envelope used only for non-success HTTP classification.
fn placeholder_envelope() -> OpenMeteoEnvelope {
    OpenMeteoEnvelope {
        elevation_m: None,
        latitude: 0.0,
        longitude: 0.0,
        upstream_timezone: String::new(),
    }
}

/// Construct one exact invalid-payload attempt failure.
fn payload_failure(attempt: u8, message: &str) -> ProviderAttemptError {
    let bounded = message.encode_utf16().take(512).collect::<Vec<_>>();
    ProviderAttemptError::Classified(ProviderFailure {
        attempts: attempt,
        content_budget_role: None,
        error: ProviderError {
            classification: ProviderFailureClassification::InvalidPayload,
            code: "invalid_payload".to_owned(),
            message: String::from_utf16_lossy(&bounded),
            retry_after_ms: None,
        },
        status: Some(200),
    })
}

/// Replace the placeholder successful status on one typed payload failure.
fn with_failure_status(error: ProviderAttemptError, status: u16) -> ProviderAttemptError {
    match error {
        ProviderAttemptError::Classified(mut failure) => {
            failure.status = Some(status);
            ProviderAttemptError::Classified(failure)
        }
        error => error,
    }
}

/// Construct a pre-I/O scheduled payload failure.
fn invalid_task() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "invalid_payload".to_owned(),
    }
}

/// Construct one bounded controller-thread failure.
fn transport_task() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "provider_transport_failure".to_owned(),
    }
}

#[cfg(test)]
#[path = "native_open_meteo_scheduled_transport_test.rs"]
mod tests;
