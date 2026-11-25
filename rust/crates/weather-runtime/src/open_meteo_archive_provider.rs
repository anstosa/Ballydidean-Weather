//! Original Open-Meteo Archive query, bounded exchange, and decoded payload normalization.

use weather_contracts::{
    CanonicalWeatherMetrics, JsonValue, NormalizedWeatherRecord, Nullable, SourceKind,
    WeatherRecordMetadata,
};
use weather_database::Sha256Digest;
use weather_domain::{MetricName, Unit, normalize_metric_value, validate_coordinates};
use weather_providers::{
    ProviderControls, ProviderFailureClassification, ProviderIo, ProviderRequestPolicy,
    ProviderRunError, run_provider_request, validate_provider_controls,
};
use weather_worker::{
    OpenMeteoArchiveInstant, OpenMeteoArchiveProvider, OpenMeteoArchiveRequest, ProviderBatch,
    TideBackfillErrorClassification, TideBackfillOperationError,
};

const ARCHIVE_ENDPOINT: &str = "https://archive-api.open-meteo.com/v1/archive";
const ARCHIVE_PATH: &str = "/v1/archive";
const ARCHIVE_VARIABLES: &str = "temperature_2m,apparent_temperature,relative_humidity_2m,precipitation,cloud_cover,wind_speed_10m,wind_gusts_10m,wind_direction_10m,surface_pressure";
const ARCHIVE_FIELDS: [&str; 9] = [
    "temperature_2m",
    "apparent_temperature",
    "relative_humidity_2m",
    "precipitation",
    "cloud_cover",
    "wind_speed_10m",
    "wind_gusts_10m",
    "wind_direction_10m",
    "surface_pressure",
];
const ARCHIVE_UNITS: [&str; 9] = ["°C", "°C", "%", "mm", "%", "m/s", "m/s", "°", "hPa"];

/// The official endpoint or the already admitted compatibility origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpenMeteoArchiveEndpoint<'a> {
    Official(&'static str),
    Compatibility { origin: &'a str, path: &'static str },
}

/// Borrowed text or a finite coordinate for the admitted URL builder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpenMeteoArchiveQueryValue<'a> {
    Number(f64),
    Text(&'a str),
}

/// Ordered archive query fields before standard URL encoding.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoArchiveQuery<'a> {
    pub endpoint: OpenMeteoArchiveEndpoint<'a>,
    pub parameters: [(&'static str, OpenMeteoArchiveQueryValue<'a>); 9],
}

/// One decoded upstream body after the approved bounded response/JSON owner.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoArchivePayload {
    pub body: JsonValue,
    pub checksum: Sha256Digest,
}

/// Selected number and civil-time primitives used by the original Archive provider.
pub trait OpenMeteoArchiveValues {
    /// Verify the original timezone string with the admitted IANA implementation.
    fn validate_timezone(&mut self, timezone: &str) -> Result<(), TideBackfillOperationError>;

    /// Resolve a provider timestamp using the previous instant to disambiguate a fold.
    fn resolve_provider_time(
        &mut self,
        wall_time: &str,
        timezone: &str,
        fallback_offset_seconds: i32,
        previous_valid_at: Option<&OpenMeteoArchiveInstant>,
    ) -> Result<OpenMeteoArchiveInstant, TideBackfillOperationError>;

    /// Read the canonical UTC clock once after the successful response.
    fn received_at(&mut self) -> Result<String, TideBackfillOperationError>;

    /// Render one finite provider number with the original JavaScript number spelling.
    fn number_text(&mut self, value: f64) -> String;
}

/// The approved transport owner encodes the query and opens attempts through shared policy.
pub trait OpenMeteoArchiveTransport {
    type Exchange<'a>: ProviderIo<OpenMeteoArchivePayload>
    where
        Self: 'a;

    /// Bind a decoded-result exchange without starting a request outside the retry controller.
    fn bind_archive(
        &mut self,
        query: &OpenMeteoArchiveQuery<'_>,
    ) -> Result<Self::Exchange<'_>, TideBackfillOperationError>;
}

/// Backfill provider using the original request and shared bounded retry controller.
pub struct OpenMeteoArchiveOperation<Transport, Values> {
    transport: Transport,
    values: Values,
    compatibility_origin: Option<String>,
    controls: ProviderControls,
}

impl<Transport, Values> OpenMeteoArchiveOperation<Transport, Values> {
    /// Bind the loaded compatibility origin and admitted transport/values owners.
    pub const fn new(
        transport: Transport,
        values: Values,
        compatibility_origin: Option<String>,
        controls: ProviderControls,
    ) -> Self {
        Self {
            transport,
            values,
            compatibility_origin,
            controls,
        }
    }
}

impl<Transport: OpenMeteoArchiveTransport, Values: OpenMeteoArchiveValues> OpenMeteoArchiveProvider
    for OpenMeteoArchiveOperation<Transport, Values>
{
    /// Fetch one archive response, normalize every row, and keep the exact local date interval.
    fn fetch(
        &mut self,
        request: OpenMeteoArchiveRequest,
    ) -> Result<ProviderBatch<NormalizedWeatherRecord, JsonValue, ()>, TideBackfillOperationError>
    {
        validate_provider_controls(self.controls)
            .map_err(|_| unexpected("invalid provider controls"))?;
        let query = build_open_meteo_archive_query(
            &request,
            self.compatibility_origin.as_deref(),
            &mut self.values,
        )?;
        let mut exchange = self.transport.bind_archive(&query)?;
        let response = run_provider_request(
            ProviderRequestPolicy {
                controls: self.controls,
                deadline_at_ms: Some(request.deadline_at_ms),
            },
            &mut exchange,
        )
        .map_err(provider_failure)?;
        let received_at = self.values.received_at()?;
        let body = response.body.body;
        let records =
            normalize_open_meteo_archive_rows(&body, &request, &received_at, &mut self.values)?;
        Ok(ProviderBatch {
            attempts: u32::from(response.attempts),
            provider_cursor: (),
            records,
            response_metadata: response_metadata(&body, response.status),
            upstream_response_checksum: response.body.checksum.as_str().to_owned(),
        })
    }
}

/// Build the source-owned request without adding a URL or date implementation.
pub fn build_open_meteo_archive_query<'a>(
    request: &'a OpenMeteoArchiveRequest,
    compatibility_origin: Option<&'a str>,
    values: &mut impl OpenMeteoArchiveValues,
) -> Result<OpenMeteoArchiveQuery<'a>, TideBackfillOperationError> {
    use OpenMeteoArchiveQueryValue::{Number, Text};

    validate_coordinates(request.latitude, request.longitude)
        .map_err(|error| unexpected(error.to_string()))?;
    // keep the source identifier bound to the original string limits
    if request.source_id.trim().is_empty() || request.source_id.encode_utf16().count() > 128 {
        return Err(unexpected("sourceId must be non-empty and bounded"));
    }
    values.validate_timezone(&request.timezone)?;
    // reject inconsistent local-date or admitted UTC boundaries before I/O
    if request.start_date > request.end_date
        || request.end_date >= request.end_exclusive_date
        || request.interval_start_ms >= request.interval_end_exclusive_ms
    {
        return Err(unexpected("archive startDate must not follow endDate"));
    }
    let endpoint = match compatibility_origin {
        None => OpenMeteoArchiveEndpoint::Official(ARCHIVE_ENDPOINT),
        Some(origin) => OpenMeteoArchiveEndpoint::Compatibility {
            origin,
            path: ARCHIVE_PATH,
        },
    };
    Ok(OpenMeteoArchiveQuery {
        endpoint,
        parameters: [
            ("latitude", Number(request.latitude)),
            ("longitude", Number(request.longitude)),
            ("hourly", Text(ARCHIVE_VARIABLES)),
            ("temperature_unit", Text("celsius")),
            ("wind_speed_unit", Text("ms")),
            ("precipitation_unit", Text("mm")),
            ("timezone", Text("UTC")),
            ("start_date", Text(&request.start_date)),
            ("end_date", Text(&request.end_exclusive_date)),
        ],
    })
}

/// Normalize the admitted JsonValue and preserve the held local-window geometry.
pub fn normalize_open_meteo_archive_rows(
    body: &JsonValue,
    request: &OpenMeteoArchiveRequest,
    received_at: &str,
    values: &mut impl OpenMeteoArchiveValues,
) -> Result<Vec<NormalizedWeatherRecord>, TideBackfillOperationError> {
    let root = object(body, "Open-Meteo response")?;
    let hourly = object(required(root, "hourly"), "hourly")?;
    let units = object(required(root, "hourly_units"), "hourly_units")?;
    let (metadata, offset_seconds) = normalize_envelope(root, values)?;
    let times = array(required(hourly, "time"), "hourly.time")?;
    // reject a successful but empty provider body
    if times.is_empty() {
        return Err(invalid("hourly.time must not be empty"));
    }
    let arrays = metric_arrays(hourly, times.len())?;
    let mut records = Vec::with_capacity(times.len());
    let mut previous = None;
    // validate each provider row in the original order even when outside the retained window
    for (index, time) in times.iter().enumerate() {
        let wall_time = text(time, &format!("hourly.time[{index}]"))?;
        let instant = values.resolve_provider_time(
            &wall_time,
            &metadata.upstreamTimezone,
            offset_seconds,
            previous.as_ref(),
        )?;
        let metrics = normalize_metrics(units, &arrays, index)?;
        // exclude the provider padding using the admitted half-open local interval
        if (request.interval_start_ms..request.interval_end_exclusive_ms)
            .contains(&instant.epoch_milliseconds)
        {
            records.push(NormalizedWeatherRecord {
                metadata: metadata.clone(),
                metrics,
                productRunAt: Nullable::Null,
                receivedAt: received_at.to_owned(),
                sourceId: request.source_id.clone(),
                sourceKind: SourceKind::Reanalysis,
                validAt: instant.utc.clone(),
            });
        }
        previous = Some(instant);
    }
    // reject a successful response that did not contain any admitted local hours
    if records.is_empty() {
        return Err(invalid("archive payload contained no requested records"));
    }
    Ok(records)
}

type Fields = [(Vec<u16>, JsonValue)];

/// Require the original decoded object shape without parsing wire bytes.
fn object<'a>(value: &'a JsonValue, label: &str) -> Result<&'a Fields, TideBackfillOperationError> {
    match value {
        JsonValue::Object(fields) => Ok(fields),
        _ => Err(invalid(format!("{label} must be an object"))),
    }
}

/// Find exactly one original field in the admitted object.
fn member<'a>(fields: &'a Fields, key: &str) -> Option<&'a JsonValue> {
    fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
}

/// Let a missing required member fail the exact same shape check as a JSON null.
fn required<'a>(fields: &'a Fields, key: &str) -> &'a JsonValue {
    member(fields, key).unwrap_or(&JsonValue::Null)
}

/// Borrow one original JSON array.
fn array<'a>(
    value: &'a JsonValue,
    label: &str,
) -> Result<&'a [JsonValue], TideBackfillOperationError> {
    match value {
        JsonValue::Array(values) => Ok(values),
        _ => Err(invalid(format!("{label} must be an array"))),
    }
}

/// Admit a provider string before its semantic owner.
fn text(value: &JsonValue, label: &str) -> Result<String, TideBackfillOperationError> {
    let message = || invalid(format!("{label} must be a non-empty string"));
    let JsonValue::String(value) = value else {
        return Err(message());
    };
    let value = String::from_utf16(value).map_err(|_| message())?;
    // reject blank decoded strings while preserving the original text
    if value.trim().is_empty() {
        return Err(message());
    }
    Ok(value)
}

/// Borrow a finite scalar from the decoded provider object.
fn number(value: &JsonValue, label: &str) -> Result<f64, TideBackfillOperationError> {
    match value {
        JsonValue::Number(value) if value.is_finite() => Ok(*value),
        _ => Err(invalid(format!("{label} must be a finite number"))),
    }
}

/// Build exact archive provenance once per provider response.
fn normalize_envelope(
    root: &Fields,
    values: &mut impl OpenMeteoArchiveValues,
) -> Result<(WeatherRecordMetadata, i32), TideBackfillOperationError> {
    let latitude = number(required(root, "latitude"), "latitude")?;
    let longitude = number(required(root, "longitude"), "longitude")?;
    validate_coordinates(latitude, longitude).map_err(|error| unexpected(error.to_string()))?;
    let timezone = text(required(root, "timezone"), "timezone")?;
    values.validate_timezone(&timezone)?;
    let offset_seconds = number(required(root, "utc_offset_seconds"), "utc_offset_seconds")?;
    // retain the original realistic fallback offset without rounding it here
    if offset_seconds.fract() != 0.0 || offset_seconds.abs() > 50_400.0 {
        return Err(invalid("utc_offset_seconds is invalid"));
    }
    let mut provider = vec![(units("dataset"), string("archive"))];
    // omit elevation only when absent or explicitly null
    if let Some(elevation) =
        member(root, "elevation").filter(|value| !matches!(value, JsonValue::Null))
    {
        provider.push((
            units("elevation_m"),
            JsonValue::Number(number(elevation, "elevation")?),
        ));
    }
    let grid = format!(
        "{},{}",
        values.number_text(latitude),
        values.number_text(longitude)
    );
    provider.push((units("grid_cell"), string(&grid)));
    Ok((
        WeatherRecordMetadata {
            device: Nullable::Null,
            model: Nullable::Value("reanalysis".to_owned()),
            provider: Nullable::Value(provider),
            quality: Nullable::Null,
            upstreamTimezone: timezone,
        },
        offset_seconds as i32,
    ))
}

/// Borrow the required nine positional arrays, each aligned with the time axis.
fn metric_arrays(
    hourly: &Fields,
    length: usize,
) -> Result<[&[JsonValue]; 9], TideBackfillOperationError> {
    let mut arrays = [&[][..]; 9];
    // admit complete geometry before any calendar conversion
    for (index, field) in ARCHIVE_FIELDS.iter().enumerate() {
        let label = format!("hourly.{field}");
        let cells = array(required(hourly, field), &label)?;
        // match the original row-alignment diagnostic
        if cells.len() != length {
            return Err(invalid(format!(
                "{label} length does not match hourly.time"
            )));
        }
        arrays[index] = cells;
    }
    Ok(arrays)
}

/// Reuse the original canonical metric rules and keep forecast-only metrics null.
fn normalize_metrics(
    units: &Fields,
    arrays: &[&[JsonValue]; 9],
    index: usize,
) -> Result<CanonicalWeatherMetrics, TideBackfillOperationError> {
    let metric = |column: usize, name: MetricName, unit: Unit| {
        let field = ARCHIVE_FIELDS[column];
        let value = match &arrays[column][index] {
            JsonValue::Null => None,
            cell => Some(number(cell, field)?),
        };
        let expected = ARCHIVE_UNITS[column];
        // require explicit exact units even when that row has a null value
        if !matches!(member(units, field), Some(JsonValue::String(actual)) if actual.iter().copied().eq(expected.encode_utf16()))
        {
            return Err(invalid(format!("{field} unit must be {expected}")));
        }
        let value = match (name, value) {
            (MetricName::WindDirectionDegrees, Some(360.0)) => Some(0.0),
            (_, value) => value,
        };
        normalize_metric_value(name, value, unit)
            .map(|value| value.map_or(Nullable::Null, Nullable::Value))
            .map_err(|error| unexpected(error.to_string()))
    };
    Ok(CanonicalWeatherMetrics {
        apparentTemperatureC: metric(1, MetricName::ApparentTemperatureC, Unit::Celsius)?,
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: metric(4, MetricName::CloudCoverPercent, Unit::Percent)?,
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: metric(3, MetricName::PrecipitationMm, Unit::Millimeter)?,
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: metric(8, MetricName::PressureHpa, Unit::Hectopascal)?,
        relativeHumidityPercent: metric(2, MetricName::RelativeHumidityPercent, Unit::Percent)?,
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: metric(0, MetricName::TemperatureC, Unit::Celsius)?,
        uvIndex: Nullable::Null,
        waterLevelM: Nullable::Null,
        windDirectionDegrees: metric(7, MetricName::WindDirectionDegrees, Unit::Degree)?,
        windGustMps: metric(6, MetricName::WindGustMps, Unit::MeterPerSecond)?,
        windSpeedMps: metric(5, MetricName::WindSpeedMps, Unit::MeterPerSecond)?,
        wetBulbGlobeTemperatureC: Nullable::Null,
    })
}

/// Project the exact optional evidence member without owning JSON formatting.
fn response_metadata(body: &JsonValue, status: u16) -> JsonValue {
    let mut metadata = vec![(units("http_status"), JsonValue::Number(f64::from(status)))];
    // preserve finite provider generation metadata including finite negative values
    if let JsonValue::Object(fields) = body
        && let Some(JsonValue::Number(generation)) = member(fields, "generationtime_ms")
        && generation.is_finite()
    {
        metadata.push((units("generation_ms"), JsonValue::Number(*generation)));
    }
    JsonValue::Object(metadata)
}

/// Project a generated JS string without parsing or escaping wire bytes.
fn units(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// Create one decoded JSON string value.
fn string(value: &str) -> JsonValue {
    JsonValue::String(units(value))
}

/// Retain the original classified payload failure and its original one attempt.
fn invalid(message: impl Into<String>) -> TideBackfillOperationError {
    TideBackfillOperationError::provider(
        1,
        TideBackfillErrorClassification::InvalidPayload,
        "invalid_payload",
        message,
    )
}

/// Leave ordinary argument/domain errors to the original outer catch mapping.
fn unexpected(message: impl Into<String>) -> TideBackfillOperationError {
    TideBackfillOperationError::unexpected(message)
}

/// Preserve the controller’s classified failure and exact attempt count.
fn provider_failure(error: ProviderRunError) -> TideBackfillOperationError {
    let ProviderRunError::Provider(failure) = error else {
        return unexpected("invalid provider controls");
    };
    let classification = match failure.error.classification {
        ProviderFailureClassification::InvalidPayload => {
            TideBackfillErrorClassification::InvalidPayload
        }
        ProviderFailureClassification::Permanent => TideBackfillErrorClassification::Permanent,
        ProviderFailureClassification::RateLimited => TideBackfillErrorClassification::RateLimited,
        ProviderFailureClassification::Retryable => TideBackfillErrorClassification::Retryable,
    };
    TideBackfillOperationError::provider(
        u32::from(failure.attempts),
        classification,
        failure.error.code,
        failure.error.message,
    )
}
