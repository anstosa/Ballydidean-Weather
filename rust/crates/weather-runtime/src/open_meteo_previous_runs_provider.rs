//! Decoded Open-Meteo Previous Runs provider adapter without URL, JSON, or clock codecs.

use std::collections::BTreeSet;

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;
use weather_domain::validate_coordinates;
use weather_providers::{
    ProviderControls, ProviderFailureClassification, ProviderIo, ProviderRequestPolicy,
    ProviderRunError, run_provider_request, validate_provider_controls,
};
use weather_worker::{
    FORECAST_ANCHOR_CONTRACT_EPOCH, ForecastAnchorAttemptPayload,
    ForecastAnchorBackfillOperationError, ForecastAnchorBackfillProvider, ForecastAnchorEnvelope,
    ForecastAnchorMetricCells, ForecastAnchorParsedHour, ForecastAnchorParsedLocation,
    ForecastAnchorProviderRequest, ForecastAnchorTimezone, TideBackfillErrorClassification,
};

const PREVIOUS_RUNS_ENDPOINT: &str = "https://previous-runs-api.open-meteo.com/v1/forecast";
const PREVIOUS_RUNS_PATH: &str = "/v1/forecast";
const PREVIOUS_RUNS_MODEL: &str = "best_match";
const MILLISECONDS_PER_HOUR: i64 = 3_600_000;
const MILLISECONDS_PER_DAY: i64 = 86_400_000;
const MAXIMUM_LOCATIONS: usize = 100;
const BASE_FIELDS: [&str; 9] = [
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
const EXPECTED_UNITS: [&str; 9] = ["°C", "°C", "%", "mm", "%", "m/s", "m/s", "°", "hPa"];

/// The official endpoint or one already-admitted compatibility origin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenMeteoPreviousRunsEndpoint {
    Official(&'static str),
    Compatibility { origin: String, path: &'static str },
}

/// One owned ordered parameter passed to the approved URL owner.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoPreviousRunsQueryParameter {
    pub name: &'static str,
    pub value: String,
}

/// Exact ordered Previous Runs query before standard URL encoding.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoPreviousRunsQuery {
    pub endpoint: OpenMeteoPreviousRunsEndpoint,
    pub parameters: Vec<OpenMeteoPreviousRunsQueryParameter>,
}

/// One decoded upstream body with exact response checksum evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoPreviousRunsPayload {
    pub body: JsonValue,
    pub checksum: Sha256Digest,
}

/// Approved number, UTC-hour, and receipt-clock primitives.
pub trait OpenMeteoPreviousRunsValues {
    /// Render the exact JavaScript number spelling used in coordinate lists.
    fn number_text(&mut self, value: f64) -> String;

    /// Parse one provider UTC wall hour and return canonical ISO plus epoch milliseconds.
    fn utc_hour(
        &mut self,
        value: &str,
    ) -> Result<(String, i64), ForecastAnchorBackfillOperationError>;

    /// Capture the successful response receipt instant once.
    fn received_at(&mut self) -> Result<String, ForecastAnchorBackfillOperationError>;
}

/// Transport owner that encodes the query and supplies bounded retry attempts.
pub trait OpenMeteoPreviousRunsTransport {
    type Exchange<'a>: ProviderIo<OpenMeteoPreviousRunsPayload>
    where
        Self: 'a;

    /// Bind one decoded exchange without starting traffic outside the retry controller.
    fn bind_previous_runs(
        &mut self,
        query: &OpenMeteoPreviousRunsQuery,
    ) -> Result<Self::Exchange<'_>, ForecastAnchorBackfillOperationError>;
}

/// Previous Runs operation composed from admitted transport and value owners.
pub struct OpenMeteoPreviousRunsOperation<Transport, Values> {
    transport: Transport,
    values: Values,
    compatibility_origin: Option<String>,
    controls: ProviderControls,
}

impl<Transport, Values> OpenMeteoPreviousRunsOperation<Transport, Values> {
    /// Bind configured compatibility, retry, and decoded-value owners.
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

impl<Transport, Values> ForecastAnchorBackfillProvider
    for OpenMeteoPreviousRunsOperation<Transport, Values>
where
    Transport: OpenMeteoPreviousRunsTransport,
    Values: OpenMeteoPreviousRunsValues,
{
    /// Execute one shared retry policy and return typed positional response material.
    fn fetch(
        &mut self,
        request: ForecastAnchorProviderRequest,
    ) -> Result<ForecastAnchorAttemptPayload, ForecastAnchorBackfillOperationError> {
        validate_provider_controls(self.controls)
            .map_err(|_| unexpected("invalid provider controls"))?;
        let query = build_open_meteo_previous_runs_query(
            &request,
            self.compatibility_origin.as_deref(),
            &mut self.values,
        )?;
        let mut exchange = self.transport.bind_previous_runs(&query)?;
        let response = run_provider_request(
            ProviderRequestPolicy {
                controls: self.controls,
                deadline_at_ms: Some(request.deadline_at_ms),
            },
            &mut exchange,
        )
        .map_err(provider_failure)?;
        let received_at = self.values.received_at()?;
        let locations = project_open_meteo_previous_runs_payload(
            &response.body.body,
            &request,
            &mut self.values,
        )?;
        Ok(ForecastAnchorAttemptPayload {
            attempts: u32::from(response.attempts),
            checksum: response.body.checksum.as_str().to_owned(),
            http_status: response.status,
            locations,
            received_at,
        })
    }
}

/// Build the exact ordered source query without implementing URL encoding.
pub fn build_open_meteo_previous_runs_query(
    request: &ForecastAnchorProviderRequest,
    compatibility_origin: Option<&str>,
    values: &mut impl OpenMeteoPreviousRunsValues,
) -> Result<OpenMeteoPreviousRunsQuery, ForecastAnchorBackfillOperationError> {
    validate_request(request)?;
    let latitude = request
        .locations
        .iter()
        .map(|location| values.number_text(location.latitude))
        .collect::<Vec<_>>()
        .join(",");
    let longitude = request
        .locations
        .iter()
        .map(|location| values.number_text(location.longitude))
        .collect::<Vec<_>>()
        .join(",");
    let endpoint = match compatibility_origin {
        None => OpenMeteoPreviousRunsEndpoint::Official(PREVIOUS_RUNS_ENDPOINT),
        Some(origin) => OpenMeteoPreviousRunsEndpoint::Compatibility {
            origin: origin.to_owned(),
            path: PREVIOUS_RUNS_PATH,
        },
    };
    Ok(OpenMeteoPreviousRunsQuery {
        endpoint,
        parameters: vec![
            parameter("latitude", latitude),
            parameter("longitude", longitude),
            parameter("hourly", previous_runs_hourly_variables().join(",")),
            parameter("models", PREVIOUS_RUNS_MODEL),
            parameter("temperature_unit", "celsius"),
            parameter("wind_speed_unit", "ms"),
            parameter("precipitation_unit", "mm"),
            parameter("timezone", "UTC"),
            parameter("start_date", &request.start_date),
            parameter("end_date", &request.end_date),
        ],
    })
}

/// Decode and validate every positional root, fixed field, unit, and UTC hour.
pub fn project_open_meteo_previous_runs_payload(
    body: &JsonValue,
    request: &ForecastAnchorProviderRequest,
    values: &mut impl OpenMeteoPreviousRunsValues,
) -> Result<Vec<ForecastAnchorParsedLocation>, ForecastAnchorBackfillOperationError> {
    validate_request(request)?;
    let roots = response_roots(body, request.locations.len())?;
    let expected_hours =
        usize::try_from((request.end_exclusive_ms - request.start_ms) / MILLISECONDS_PER_HOUR)
            .map_err(|_| invalid("Previous Runs hourly span is invalid"))?;
    let mut locations = Vec::with_capacity(roots.len());

    // retain request-position binding without reordering provider roots
    for (location_index, root) in roots.into_iter().enumerate() {
        require_location_id(root, location_index)?;
        let hourly = object(required(root, "hourly"), "hourly")?;
        let units = object(required(root, "hourly_units"), "hourly_units")?;
        let times = array(required(hourly, "time"), "hourly.time")?;

        // require every requested UTC hour including all-null anchor rows
        if times.len() != expected_hours {
            return Err(invalid(
                "Previous Runs hourly.time does not cover every requested hour",
            ));
        }
        let arrays = metric_arrays(hourly, units, expected_hours)?;
        let envelope = envelope(root)?;
        let mut hours = Vec::with_capacity(expected_hours);

        // preserve the exact expected UTC axis and all seven lead positions
        for (time_index, time) in times.iter().enumerate() {
            let wall_time = text(time, "hourly.time")?;
            let (valid_at, valid_at_ms) = values.utc_hour(&wall_time)?;
            let offset = i64::try_from(time_index)
                .map_err(|_| invalid("Previous Runs time index overflow"))?
                .checked_mul(MILLISECONDS_PER_HOUR)
                .ok_or_else(|| invalid("Previous Runs time index overflow"))?;
            let expected_ms = request
                .start_ms
                .checked_add(offset)
                .ok_or_else(|| invalid("Previous Runs time index overflow"))?;

            // reject gaps, duplicates, shifted axes, or noncanonical UTC strings
            if valid_at_ms != expected_ms
                || valid_at_ms < request.start_ms
                || valid_at_ms >= request.end_exclusive_ms
                || hours
                    .last()
                    .is_some_and(|prior: &ForecastAnchorParsedHour| prior.valid_at >= valid_at)
            {
                return Err(invalid(
                    "Previous Runs hourly.time does not match the requested hourly axis",
                ));
            }
            let mut anchors = [empty_metrics(); 7];

            // project each fixed lead while retaining base-field positions
            for (lead_index, anchor) in anchors.iter_mut().enumerate() {
                let metric = |base: usize| {
                    nullable_number(
                        &arrays[base * 7 + lead_index][time_index],
                        BASE_FIELDS[base],
                    )
                };
                *anchor = ForecastAnchorMetricCells {
                    temperature_c: metric(0)?,
                    apparent_temperature_c: metric(1)?,
                    relative_humidity_percent: metric(2)?,
                    precipitation_mm: metric(3)?,
                    cloud_cover_percent: metric(4)?,
                    wind_speed_mps: metric(5)?,
                    wind_gust_mps: metric(6)?,
                    wind_direction_degrees: metric(7)?,
                    pressure_hpa: metric(8)?,
                };
            }
            hours.push(ForecastAnchorParsedHour {
                anchors,
                valid_at,
                valid_at_ms,
            });
        }
        locations.push(ForecastAnchorParsedLocation {
            envelope,
            generation_ms: generation(root),
            hours,
        });
    }
    Ok(locations)
}

type Fields = [(Vec<u16>, JsonValue)];

/// Require an already-decoded object.
fn object<'a>(
    value: &'a JsonValue,
    label: &str,
) -> Result<&'a Fields, ForecastAnchorBackfillOperationError> {
    match value {
        JsonValue::Object(fields) => Ok(fields),
        _ => Err(invalid(format!("{label} must be an object"))),
    }
}

/// Borrow one named object member.
fn member<'a>(fields: &'a Fields, key: &str) -> Option<&'a JsonValue> {
    fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
}

/// Let a missing member follow the same invalid-shape path as null.
fn required<'a>(fields: &'a Fields, key: &str) -> &'a JsonValue {
    member(fields, key).unwrap_or(&JsonValue::Null)
}

/// Require one decoded array.
fn array<'a>(
    value: &'a JsonValue,
    label: &str,
) -> Result<&'a [JsonValue], ForecastAnchorBackfillOperationError> {
    match value {
        JsonValue::Array(values) => Ok(values),
        _ => Err(invalid(format!("{label} must be an array"))),
    }
}

/// Require one nonempty decoded string.
fn text(value: &JsonValue, label: &str) -> Result<String, ForecastAnchorBackfillOperationError> {
    let JsonValue::String(value) = value else {
        return Err(invalid(format!("{label} must be a non-empty string")));
    };
    let value = String::from_utf16(value)
        .map_err(|_| invalid(format!("{label} must be a non-empty string")))?;
    // preserve exact provider text after rejecting its blank form
    if value.trim().is_empty() {
        return Err(invalid(format!("{label} must be a non-empty string")));
    }
    Ok(value)
}

/// Require one finite provider number.
fn number(value: &JsonValue, label: &str) -> Result<f64, ForecastAnchorBackfillOperationError> {
    match value {
        JsonValue::Number(value) if value.is_finite() => Ok(*value),
        _ => Err(invalid(format!("{label} must be a finite number"))),
    }
}

/// Retain provider null or one finite metric value.
fn nullable_number(
    value: &JsonValue,
    label: &str,
) -> Result<Option<f64>, ForecastAnchorBackfillOperationError> {
    match value {
        JsonValue::Null => Ok(None),
        value => number(value, label).map(Some),
    }
}

/// Select the original single-root or positional multi-root response shape.
fn response_roots(
    body: &JsonValue,
    location_count: usize,
) -> Result<Vec<&Fields>, ForecastAnchorBackfillOperationError> {
    // preserve the provider's distinct single-coordinate response contract
    if location_count == 1 {
        return object(body, "Open-Meteo Previous Runs response").map(|root| vec![root]);
    }
    let responses = array(body, "Open-Meteo Previous Runs multi-location response")?;

    // reject missing or extra positional roots
    if responses.len() != location_count {
        return Err(invalid(
            "Previous Runs response count does not match request positions",
        ));
    }
    responses
        .iter()
        .enumerate()
        .map(|(index, value)| object(value, &format!("responses[{index}]")))
        .collect()
}

/// Verify an optional provider position marker without reordering roots.
fn require_location_id(
    root: &Fields,
    index: usize,
) -> Result<(), ForecastAnchorBackfillOperationError> {
    let location_id = member(root, "location_id");

    // allow only the first root to omit its zero marker
    if index == 0 && location_id.is_none() {
        return Ok(());
    }
    let value = number(location_id.unwrap_or(&JsonValue::Null), "location_id")?;

    // require an exact zero-based safe integer position
    if value.fract() != 0.0 || value != index as f64 {
        return Err(invalid(
            "Previous Runs location_id does not match request position",
        ));
    }
    Ok(())
}

/// Validate the zero-offset response grid envelope.
fn envelope(root: &Fields) -> Result<ForecastAnchorEnvelope, ForecastAnchorBackfillOperationError> {
    let latitude = number(required(root, "latitude"), "latitude")?;
    let longitude = number(required(root, "longitude"), "longitude")?;
    validate_coordinates(latitude, longitude)
        .map_err(|_| invalid("Previous Runs envelope has invalid location metadata"))?;
    let timezone = match text(required(root, "timezone"), "timezone")?.as_str() {
        "UTC" => ForecastAnchorTimezone::Utc,
        "GMT" => ForecastAnchorTimezone::Gmt,
        _ => {
            return Err(invalid(
                "Previous Runs response must use a zero-offset UTC or GMT envelope",
            ));
        }
    };
    let utc_offset = number(required(root, "utc_offset_seconds"), "utc_offset_seconds")?;

    // require the exact zero-offset contract without a lossy numeric cast
    if utc_offset != 0.0 {
        return Err(invalid(
            "Previous Runs response must use a zero-offset UTC or GMT envelope",
        ));
    }
    let elevation_m = match member(root, "elevation") {
        None | Some(JsonValue::Null) => None,
        Some(value) => Some(number(value, "elevation")?),
    };
    Ok(ForecastAnchorEnvelope {
        elevation_m,
        latitude,
        longitude,
        timezone,
        utc_offset_seconds: 0,
    })
}

/// Collect and validate all 63 lead-suffixed arrays in base-major order.
fn metric_arrays<'a>(
    hourly: &'a Fields,
    units: &Fields,
    expected_length: usize,
) -> Result<Vec<&'a [JsonValue]>, ForecastAnchorBackfillOperationError> {
    // reject unknown or duplicated hourly and unit fields before projecting values
    if hourly.len() != 64
        || units.len() != 64
        || hourly.iter().any(|(name, _)| !known_hourly_field(name))
        || units.iter().any(|(name, _)| !known_hourly_field(name))
    {
        return Err(invalid(
            "Previous Runs response contains an unexpected hourly field",
        ));
    }
    require_unit(required(units, "time"), "iso8601", "hourly_units.time")?;
    let mut arrays = Vec::with_capacity(63);

    // preserve the provider's base-variable order before lead order
    for (base_index, base) in BASE_FIELDS.iter().enumerate() {
        // admit every fixed lead array and its literal unit
        for day_offset in 1..=7 {
            let field = format!("{base}_previous_day{day_offset}");
            let cells = array(required(hourly, &field), &format!("hourly.{field}"))?;

            // reject positional drift from the UTC time axis
            if cells.len() != expected_length {
                return Err(invalid(format!(
                    "hourly.{field} length does not match hourly.time"
                )));
            }
            require_unit(
                required(units, &field),
                EXPECTED_UNITS[base_index],
                &format!("hourly_units.{field}"),
            )?;
            arrays.push(cells);
        }
    }
    Ok(arrays)
}

/// Recognize only time or one of the 63 exact lead-suffixed names.
fn known_hourly_field(name: &[u16]) -> bool {
    let Ok(name) = String::from_utf16(name) else {
        return false;
    };
    name == "time"
        || BASE_FIELDS.iter().any(|base| {
            name.strip_prefix(base)
                .and_then(|suffix| suffix.strip_prefix("_previous_day"))
                .is_some_and(|day| matches!(day, "1" | "2" | "3" | "4" | "5" | "6" | "7"))
        })
}

/// Require one literal provider unit.
fn require_unit(
    value: &JsonValue,
    expected: &str,
    label: &str,
) -> Result<(), ForecastAnchorBackfillOperationError> {
    let JsonValue::String(actual) = value else {
        return Err(invalid(format!("{label} unit must be {expected}")));
    };

    // reject any unit spelling or string-unit drift
    if !actual.iter().copied().eq(expected.encode_utf16()) {
        return Err(invalid(format!("{label} unit must be {expected}")));
    }
    Ok(())
}

/// Retain only finite nonnegative generation evidence.
fn generation(root: &Fields) -> Option<f64> {
    match member(root, "generationtime_ms") {
        Some(JsonValue::Number(value)) if value.is_finite() && *value >= 0.0 => Some(*value),
        _ => None,
    }
}

/// Validate the frozen request identity, geometry, and storage locations.
fn validate_request(
    request: &ForecastAnchorProviderRequest,
) -> Result<(), ForecastAnchorBackfillOperationError> {
    let span = request
        .end_exclusive_ms
        .checked_sub(request.start_ms)
        .ok_or_else(|| unexpected("Previous Runs dates are invalid"))?;

    // require the exact contract, deadline, inclusive 1-14-day range, and labels
    if request.contract_epoch != FORECAST_ANCHOR_CONTRACT_EPOCH
        || request.deadline_at_ms <= 0
        || request.start_date.is_empty()
        || request.end_date.is_empty()
        || request.start_date > request.end_date
        || span <= 0
        || span % MILLISECONDS_PER_DAY != 0
        || span > 14 * MILLISECONDS_PER_DAY
        || request.locations.is_empty()
        || request.locations.len() > MAXIMUM_LOCATIONS
    {
        return Err(unexpected("Previous Runs request is invalid"));
    }
    let mut source_ids = BTreeSet::new();

    // validate every positional location before constructing query text
    for location in &request.locations {
        validate_coordinates(location.latitude, location.longitude)
            .map_err(|_| unexpected("Previous Runs location is invalid"))?;
        if location.source_id.trim().is_empty()
            || location.source_id.encode_utf16().count() > 128
            || !fingerprint_is_valid(&location.source_config_fingerprint)
            || !source_ids.insert(location.source_id.as_str())
        {
            return Err(unexpected("Previous Runs location is invalid"));
        }
    }
    Ok(())
}

/// Construct all 63 query fields in base-major then lead order.
fn previous_runs_hourly_variables() -> Vec<String> {
    let mut fields = Vec::with_capacity(63);

    // preserve provider variable order before fixed lead order
    for base in BASE_FIELDS {
        // append each lead exactly once
        for day_offset in 1..=7 {
            fields.push(format!("{base}_previous_day{day_offset}"));
        }
    }
    fields
}

/// Construct one owned ordered query parameter.
fn parameter(name: &'static str, value: impl Into<String>) -> OpenMeteoPreviousRunsQueryParameter {
    OpenMeteoPreviousRunsQueryParameter {
        name,
        value: value.into(),
    }
}

/// Construct an all-null fixed-lead metric cell.
const fn empty_metrics() -> ForecastAnchorMetricCells {
    ForecastAnchorMetricCells {
        apparent_temperature_c: None,
        cloud_cover_percent: None,
        precipitation_mm: None,
        pressure_hpa: None,
        relative_humidity_percent: None,
        temperature_c: None,
        wind_direction_degrees: None,
        wind_gust_mps: None,
        wind_speed_mps: None,
    }
}

/// Require one lowercase SHA-256 storage fingerprint.
fn fingerprint_is_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Construct one classified invalid-payload response.
fn invalid(message: impl Into<String>) -> ForecastAnchorBackfillOperationError {
    ForecastAnchorBackfillOperationError::provider(
        1,
        TideBackfillErrorClassification::InvalidPayload,
        "invalid_payload",
        message,
    )
}

/// Construct an outer request/domain failure for normal catch-boundary coercion.
fn unexpected(message: impl Into<String>) -> ForecastAnchorBackfillOperationError {
    ForecastAnchorBackfillOperationError::unexpected(message)
}

/// Preserve the shared retry controller's classification and exact attempts.
fn provider_failure(error: ProviderRunError) -> ForecastAnchorBackfillOperationError {
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
    ForecastAnchorBackfillOperationError::provider(
        u32::from(failure.attempts),
        classification,
        failure.error.code,
        failure.error.message,
    )
}
