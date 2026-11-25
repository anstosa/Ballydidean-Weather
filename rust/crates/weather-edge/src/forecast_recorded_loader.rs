//! Bounded private-input fill for the recorded forecast products cache.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::{
    AdmittedForecastObservationsJson, AdmittedForecastRecordedProducts,
    AdmittedSolarCloudEvidenceJson, ForecastRecordedProductsLoader,
};

const ABSOLUTE_MAXIMUM_INPUT_BYTES: usize = 4 * 1024 * 1024;
const MAXIMUM_RECORDS: usize = 20_000;
const HOUR_MS: i64 = 60 * 60 * 1_000;
const QUERY_PADDING_MS: i64 = 20 * 60 * 1_000;
const SITE_SLUG: &str = "ballydidean";
const SITE_TIMEZONE: &str = "America/Los_Angeles";
const SITE_LATITUDE: f64 = 47.950_429_954_185_445;
const SITE_LONGITUDE: f64 = -122.427_970_126_081_93;

/// The one private request method used by the recorded-products fill.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastRecordedInputMethod {
    Get,
}

/// Redirect handling required for the fixed private request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastRecordedRedirectPolicy {
    Error,
}

/// The fixed private forecast-observation request admitted to an HTTP adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastRecordedInputRequest {
    pub accept: &'static str,
    pub deadline_monotonic_ms: u64,
    pub method: ForecastRecordedInputMethod,
    pub path: &'static str,
    pub redirect: ForecastRecordedRedirectPolicy,
}

/// Response metadata needed before reading the private body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForecastRecordedInputResponseHead {
    pub body_present: bool,
    pub status_code: u16,
}

/// Result of one adapter-owned streaming body read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastRecordedBodyReadOutcome {
    Complete,
    ReceiverStopped,
}

/// One private HTTP response whose body remains adapter-owned and streaming.
pub trait ForecastRecordedInputResponse {
    type Error;

    /// Return status and body availability before retaining input bytes.
    fn head(&self) -> ForecastRecordedInputResponseHead;

    /// Stream chunks until complete or until the bounded receiver refuses one.
    fn read_body(
        &mut self,
        receiver: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<ForecastRecordedBodyReadOutcome, Self::Error>;
}

/// Injected HTTP transport for the single fixed private request.
pub trait ForecastRecordedInputHttpPort {
    type Error;
    type Response: ForecastRecordedInputResponse<Error = Self::Error>;

    /// Execute the admitted request without implementing HTTP in this crate.
    fn execute(
        &self,
        request: &ForecastRecordedInputRequest,
    ) -> Result<Self::Response, Self::Error>;
}

/// Wall and monotonic clocks used by the complete fill deadline.
pub trait ForecastRecordedLoaderClock {
    /// Read the monotonic request and computation clock.
    fn monotonic_ms(&self) -> u64;

    /// Read the wall clock used for generated-at receipt validation.
    fn wall_epoch_ms(&self) -> i64;
}

/// One canonical UTC instant decoded by the admitted boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedForecastRecordedInstant {
    pub epoch_ms: i64,
    pub value: String,
}

impl AdmittedForecastRecordedInstant {
    /// Retain one canonical instant already checked by the owning codec.
    pub fn admitted(value: impl Into<String>, epoch_ms: i64) -> Self {
        Self {
            epoch_ms,
            value: value.into(),
        }
    }
}

/// The closed source-kind input accepted by the projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastRecordedInputSourceKind {
    PhysicalSensor,
    ModelCurrent,
}

impl ForecastRecordedInputSourceKind {
    /// Return the source ordering retained by the private SQL response.
    const fn order(self) -> u8 {
        // preserve physical rows before model-current rows
        match self {
            Self::PhysicalSensor => 0,
            Self::ModelCurrent => 1,
        }
    }
}

/// The exact twelve optional scalar metrics in one compact input record.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastRecordedInputMetrics {
    pub apparent_temperature_c: Option<f64>,
    pub cloud_cover_percent: Option<f64>,
    pub pm25_micrograms_per_cubic_meter: Option<f64>,
    pub precipitation_rate_mm_per_hour: Option<f64>,
    pub pressure_hpa: Option<f64>,
    pub relative_humidity_percent: Option<f64>,
    pub solar_radiation_wm2: Option<f64>,
    pub temperature_c: Option<f64>,
    pub uv_index: Option<f64>,
    pub wind_direction_degrees: Option<f64>,
    pub wind_gust_mps: Option<f64>,
    pub wind_speed_mps: Option<f64>,
}

impl ForecastRecordedInputMetrics {
    /// Check every present scalar without manufacturing missing metrics.
    fn all_finite(&self) -> bool {
        [
            self.apparent_temperature_c,
            self.cloud_cover_percent,
            self.pm25_micrograms_per_cubic_meter,
            self.precipitation_rate_mm_per_hour,
            self.pressure_hpa,
            self.relative_humidity_percent,
            self.solar_radiation_wm2,
            self.temperature_c,
            self.uv_index,
            self.wind_direction_degrees,
            self.wind_gust_mps,
            self.wind_speed_mps,
        ]
        .into_iter()
        .flatten()
        .all(f64::is_finite)
    }
}

/// Minimal source identity retained for recorded projections.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastRecordedInputProvenance {
    pub provider_key: String,
    pub source_id: String,
    pub source_key: String,
    pub source_kind: ForecastRecordedInputSourceKind,
    pub station_slug: String,
}

/// One exact compact observation record decoded from the private response.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastRecordedInputRecord {
    pub id: String,
    pub metrics: ForecastRecordedInputMetrics,
    pub provenance: ForecastRecordedInputProvenance,
    pub received_at: AdmittedForecastRecordedInstant,
    pub valid_at: AdmittedForecastRecordedInstant,
}

/// The fixed private site identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastRecordedInputSite {
    pub slug: String,
    pub timezone: String,
}

/// The fixed farm geometry used only by optional solar-cloud projection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForecastRecordedSolarSite {
    pub latitude: f64,
    pub longitude: f64,
}

/// One decoded private input before geometry and row-order validation.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedForecastRecordedInput {
    pub contract_version: String,
    pub generated_at: AdmittedForecastRecordedInstant,
    pub projection_from: AdmittedForecastRecordedInstant,
    pub query_from: AdmittedForecastRecordedInstant,
    pub records: Vec<ForecastRecordedInputRecord>,
    pub site: ForecastRecordedInputSite,
    pub through: AdmittedForecastRecordedInstant,
}

/// JSON, calendar, and mathematical projection operations retained by approved owners.
pub trait ForecastRecordedLoaderBoundary {
    type Error;
    type SolarCloudEvidence;

    /// Decode exact-key UTF-8 JSON into the typed compact input.
    fn decode_input(&self, bytes: &[u8]) -> Result<DecodedForecastRecordedInput, Self::Error>;

    /// Resolve one canonical instant to the farm-local calendar day.
    fn local_day(
        &self,
        instant: &AdmittedForecastRecordedInstant,
        timezone: &str,
    ) -> Result<String, Self::Error>;

    /// Resolve farm-local midnight through the admitted calendar implementation.
    fn site_midnight(
        &self,
        local_day: &str,
        timezone: &str,
    ) -> Result<AdmittedForecastRecordedInstant, Self::Error>;

    /// Serialize one calculated epoch through the admitted canonical time owner.
    fn utc_instant(&self, epoch_ms: i64) -> Result<AdmittedForecastRecordedInstant, Self::Error>;

    /// Build and encode the recorded forecast-observation product.
    fn forecast_observations(
        &self,
        records: &[ForecastRecordedInputRecord],
        projection_from: &AdmittedForecastRecordedInstant,
        through: &AdmittedForecastRecordedInstant,
    ) -> Result<AdmittedForecastObservationsJson, Self::Error>;

    /// Build the optional solar-cloud evidence independently from encoding.
    fn build_solar_cloud_evidence(
        &self,
        records: &[ForecastRecordedInputRecord],
        site: ForecastRecordedSolarSite,
        through: &AdmittedForecastRecordedInstant,
    ) -> Result<Self::SolarCloudEvidence, Self::Error>;

    /// Encode either projected evidence or the valid null solar fallback.
    fn encode_solar_cloud_evidence(
        &self,
        through: &AdmittedForecastRecordedInstant,
        evidence: Option<&Self::SolarCloudEvidence>,
    ) -> Result<AdmittedSolarCloudEvidenceJson, Self::Error>;
}

/// One bounded fill failure category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForecastRecordedLoaderError<HttpError, BoundaryError> {
    Http(HttpError),
    Boundary(BoundaryError),
    InputUnavailable,
    InputLimitExceeded,
    InputEnvelopeInvalid,
    InputGeometryInvalid,
    InputRecordInvalid,
    InputOrderInvalid,
    DeadlineExceeded,
}

impl<HttpError: Display, BoundaryError: Display> Display
    for ForecastRecordedLoaderError<HttpError, BoundaryError>
{
    /// Render the source-compatible bounded fill failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve adapter failures and exact source refusal categories
        match self {
            Self::Http(error) => Display::fmt(error, formatter),
            Self::Boundary(error) => Display::fmt(error, formatter),
            Self::InputUnavailable => {
                formatter.write_str("Forecast observation input is unavailable")
            }
            Self::InputLimitExceeded => {
                formatter.write_str("Forecast observation input limit exceeded")
            }
            Self::InputEnvelopeInvalid => {
                formatter.write_str("Forecast observation input envelope is invalid")
            }
            Self::InputGeometryInvalid => {
                formatter.write_str("Forecast observation input geometry is invalid")
            }
            Self::InputRecordInvalid => {
                formatter.write_str("Forecast observation input record is invalid")
            }
            Self::InputOrderInvalid => {
                formatter.write_str("Forecast observation input order is invalid")
            }
            Self::DeadlineExceeded => formatter.write_str("Forecast observation deadline exceeded"),
        }
    }
}

impl<HttpError, BoundaryError> Error for ForecastRecordedLoaderError<HttpError, BoundaryError>
where
    HttpError: Error + 'static,
    BoundaryError: Error + 'static,
{
    /// Return only the injected adapter error as an underlying source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // avoid inventing sources for bounded policy refusals
        match self {
            Self::Http(error) => Some(error),
            Self::Boundary(error) => Some(error),
            Self::InputUnavailable
            | Self::InputLimitExceeded
            | Self::InputEnvelopeInvalid
            | Self::InputGeometryInvalid
            | Self::InputRecordInvalid
            | Self::InputOrderInvalid
            | Self::DeadlineExceeded => None,
        }
    }
}

/// Concrete bounded loader for the existing recorded-products cache.
pub struct HttpForecastRecordedProductsLoader<Http, Clock, Boundary> {
    boundary: Boundary,
    clock: Clock,
    http: Http,
    max_bytes: usize,
    timeout_ms: u64,
}

impl<Http, Clock, Boundary> HttpForecastRecordedProductsLoader<Http, Clock, Boundary> {
    /// Bind injected transport and approved value owners without performing I/O.
    pub const fn new(
        http: Http,
        clock: Clock,
        boundary: Boundary,
        max_bytes: usize,
        timeout_ms: u64,
    ) -> Self {
        Self {
            boundary,
            clock,
            http,
            max_bytes,
            timeout_ms,
        }
    }

    /// Recover all bound owners after the cache lifecycle ends.
    pub fn into_parts(self) -> (Http, Clock, Boundary) {
        (self.http, self.clock, self.boundary)
    }
}

impl<Http, Clock, Boundary> ForecastRecordedProductsLoader
    for HttpForecastRecordedProductsLoader<Http, Clock, Boundary>
where
    Http: ForecastRecordedInputHttpPort,
    Clock: ForecastRecordedLoaderClock,
    Boundary: ForecastRecordedLoaderBoundary,
{
    type Error = ForecastRecordedLoaderError<Http::Error, Boundary::Error>;

    /// Execute one complete private input fill under a cumulative byte and time bound.
    fn fill(
        &self,
        request_started_at_epoch_ms: i64,
    ) -> Result<AdmittedForecastRecordedProducts, Self::Error> {
        let deadline = self.clock.monotonic_ms().saturating_add(self.timeout_ms);
        let request = ForecastRecordedInputRequest {
            accept: "application/json",
            deadline_monotonic_ms: deadline,
            method: ForecastRecordedInputMethod::Get,
            path: "/internal/v1/forecast-observation-input",
            redirect: ForecastRecordedRedirectPolicy::Error,
        };
        let mut response = self
            .http
            .execute(&request)
            .map_err(ForecastRecordedLoaderError::Http)?;
        let head = response.head();

        // reject failed or bodyless private envelopes before retaining chunks
        if !(200..300).contains(&head.status_code) || !head.body_present {
            return Err(ForecastRecordedLoaderError::InputUnavailable);
        }
        let maximum_bytes = self.max_bytes.min(ABSOLUTE_MAXIMUM_INPUT_BYTES);
        let mut body = Vec::new();
        let mut receiver_refused = false;
        let outcome = response
            .read_body(&mut |chunk| {
                // reject an accumulated length that cannot be represented locally
                let Some(next_bytes) = body.len().checked_add(chunk.len()) else {
                    receiver_refused = true;
                    return false;
                };

                // refuse the chunk before retaining bytes at or beyond either bound
                if next_bytes > maximum_bytes || self.clock.monotonic_ms() >= deadline {
                    receiver_refused = true;
                    return false;
                }
                body.extend_from_slice(chunk);
                true
            })
            .map_err(ForecastRecordedLoaderError::Http)?;

        // the adapter must stop immediately when the bounded receiver refuses a chunk
        if receiver_refused || outcome == ForecastRecordedBodyReadOutcome::ReceiverStopped {
            return Err(ForecastRecordedLoaderError::InputLimitExceeded);
        }
        let received_at_epoch_ms = self.clock.wall_epoch_ms();
        let input = self
            .boundary
            .decode_input(&body)
            .map_err(ForecastRecordedLoaderError::Boundary)?;
        validate_input(
            &input,
            request_started_at_epoch_ms,
            received_at_epoch_ms,
            &self.boundary,
        )?;
        let forecast_observations = self
            .boundary
            .forecast_observations(&input.records, &input.projection_from, &input.through)
            .map_err(ForecastRecordedLoaderError::Boundary)?;
        // isolate optional solar projection refusal from the required forecast product
        let solar_cloud_evidence = self
            .boundary
            .build_solar_cloud_evidence(
                &input.records,
                ForecastRecordedSolarSite {
                    latitude: SITE_LATITUDE,
                    longitude: SITE_LONGITUDE,
                },
                &input.through,
            )
            .ok();
        let solar_cloud_evidence = self
            .boundary
            .encode_solar_cloud_evidence(&input.through, solar_cloud_evidence.as_ref())
            .map_err(ForecastRecordedLoaderError::Boundary)?;

        // keep decoding and both projections inside the complete-fill deadline
        if self.clock.monotonic_ms() >= deadline {
            return Err(ForecastRecordedLoaderError::DeadlineExceeded);
        }
        let day = self
            .boundary
            .local_day(&input.projection_from, &input.site.timezone)
            .map_err(ForecastRecordedLoaderError::Boundary)?;
        Ok(AdmittedForecastRecordedProducts {
            day,
            forecast_observations,
            generated_at: input.through.value,
            solar_cloud_evidence,
        })
    }
}

/// Validate one decoded envelope, geometry, record set, and frozen row order.
fn validate_input<HttpError, Boundary>(
    input: &DecodedForecastRecordedInput,
    request_started_at_epoch_ms: i64,
    received_at_epoch_ms: i64,
    boundary: &Boundary,
) -> Result<(), ForecastRecordedLoaderError<HttpError, Boundary::Error>>
where
    Boundary: ForecastRecordedLoaderBoundary,
{
    // require the exact contract, site, receipt interval, and bounded row count
    if input.contract_version != "forecast-observation-input/v1"
        || input.site.slug != SITE_SLUG
        || input.site.timezone != SITE_TIMEZONE
        || input.through != input.generated_at
        || input.generated_at.epoch_ms < request_started_at_epoch_ms
        || input.generated_at.epoch_ms > received_at_epoch_ms
        || input.records.len() > MAXIMUM_RECORDS
    {
        return Err(ForecastRecordedLoaderError::InputEnvelopeInvalid);
    }
    let local_day = boundary
        .local_day(&input.generated_at, &input.site.timezone)
        .map_err(ForecastRecordedLoaderError::Boundary)?;
    let expected_projection = boundary
        .site_midnight(&local_day, &input.site.timezone)
        .map_err(ForecastRecordedLoaderError::Boundary)?;
    // reject an impossible admitted projection lower-bound calculation
    let Some(projection_query_epoch) = expected_projection.epoch_ms.checked_sub(3 * HOUR_MS) else {
        return Err(ForecastRecordedLoaderError::InputGeometryInvalid);
    };
    // reject an impossible admitted receipt lower-bound calculation
    let Some(receipt_query_epoch) = input
        .generated_at
        .epoch_ms
        .checked_sub(24 * HOUR_MS + QUERY_PADDING_MS)
    else {
        return Err(ForecastRecordedLoaderError::InputGeometryInvalid);
    };
    let expected_query_epoch = projection_query_epoch.min(receipt_query_epoch);
    let expected_query = boundary
        .utc_instant(expected_query_epoch)
        .map_err(ForecastRecordedLoaderError::Boundary)?;

    // require exact canonical geometry rather than merely equivalent epochs
    if input.projection_from != expected_projection || input.query_from != expected_query {
        return Err(ForecastRecordedLoaderError::InputGeometryInvalid);
    }
    let mut previous_kind = ForecastRecordedInputSourceKind::PhysicalSensor;
    let mut previous_valid_at = i64::MAX;
    let mut previous_id: Option<&str> = None;

    // validate every compact record and the frozen physical-then-model ordering
    for record in &input.records {
        // reject invalid identity, range, source fields, or scalar values
        if !positive_decimal_id(&record.id)
            || record.valid_at.epoch_ms < input.query_from.epoch_ms
            || record.valid_at.epoch_ms >= input.through.epoch_ms
            || !bounded_text(&record.provenance.provider_key)
            || !bounded_text(&record.provenance.source_id)
            || !bounded_text(&record.provenance.source_key)
            || !bounded_text(&record.provenance.station_slug)
            || !record.metrics.all_finite()
        {
            return Err(ForecastRecordedLoaderError::InputRecordInvalid);
        }
        let kind = record.provenance.source_kind;
        let unordered_id = record.valid_at.epoch_ms == previous_valid_at
            && previous_id.is_some_and(|previous| decimal_id_greater(&record.id, previous));

        // retain source-kind order then descending valid-at and record-id order
        if kind.order() < previous_kind.order()
            || (kind == previous_kind
                && (record.valid_at.epoch_ms > previous_valid_at || unordered_id))
        {
            return Err(ForecastRecordedLoaderError::InputOrderInvalid);
        }
        previous_kind = kind;
        previous_valid_at = record.valid_at.epoch_ms;
        previous_id = Some(&record.id);
    }

    Ok(())
}

/// Match the source's nonzero canonical decimal record identity.
fn positive_decimal_id(value: &str) -> bool {
    let bytes = value.as_bytes();

    // require one nonzero leading digit followed only by ASCII digits
    !bytes.is_empty()
        && matches!(bytes[0], b'1'..=b'9')
        && bytes[1..].iter().all(u8::is_ascii_digit)
}

/// Compare arbitrary-length canonical positive decimal identities.
fn decimal_id_greater(left: &str, right: &str) -> bool {
    // canonical decimals compare by length before lexical digits
    left.len() > right.len() || (left.len() == right.len() && left.as_bytes() > right.as_bytes())
}

/// Match the source's nonempty 512-JavaScript-code-unit identity bound.
fn bounded_text(value: &str) -> bool {
    let length = value.encode_utf16().count();
    !value.is_empty() && length <= 512
}
