//! Native transport, value, cache, and archive wiring for recorded forecast products.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hyper::Method;
use hyper::header::{ACCEPT, HeaderValue};
use jiff::{civil::Date, tz::TimeZone};
use url::Url;
use weather_contracts::JsonValue;
use weather_edge::{
    AdmittedForecastObservationsJson, AdmittedForecastRecordedInstant, AdmittedForecastRecordedNow,
    AdmittedForecastRecordedProducts, AdmittedPublicApiOrigin, AdmittedSolarCloudEvidenceJson,
    ArchiveWirePrimitives, DecodedForecastRecordedInput, ForecastAllocationStatfsPort,
    ForecastCycleHashPair, ForecastEvidenceMaterial, ForecastEvidenceReceiptCodec,
    ForecastRecordedBodyReadOutcome, ForecastRecordedInputHttpPort, ForecastRecordedInputMetrics,
    ForecastRecordedInputProvenance, ForecastRecordedInputRecord, ForecastRecordedInputRequest,
    ForecastRecordedInputResponse, ForecastRecordedInputResponseHead, ForecastRecordedInputSite,
    ForecastRecordedInputSourceKind, ForecastRecordedLoaderBoundary, ForecastRecordedLoaderClock,
    ForecastRecordedProductRequest, ForecastRecordedProductResponse, ForecastRecordedProductsCache,
    ForecastRecordedProductsCacheError, ForecastRecordedProductsClock,
    ForecastRecordedProductsRead, ForecastRecordedRedirectPolicy, ForecastRecordedSolarSite,
    HttpForecastRecordedProductsLoader, NativeForecastSchedulerArchive,
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, handle_forecast_recorded_product,
};

use crate::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
    NativeOutboundHttpResponse, NativeWirePrimitives, load_named_timezone,
    system_worker_health_milliseconds,
};

const FORECAST_RECORDED_MAXIMUM_BYTES: usize = 4 * 1024 * 1024;
const FORECAST_RECORDED_TIMEOUT_MS: u64 = 30_000;
const FORECAST_RECORDED_RETRY_MS: i64 = 30_000;
const RESPONSE_CHUNK_BYTES: usize = 16 * 1024;
const SITE_TIMEZONE: &str = "America/Los_Angeles";

type NativeRecordedLoader<Projection> = HttpForecastRecordedProductsLoader<
    NativeForecastRecordedHttp,
    NativeForecastRecordedClock,
    NativeForecastRecordedBoundary<Projection>,
>;
type NativeRecordedCache<Projection> =
    ForecastRecordedProductsCache<NativeForecastRecordedClock, NativeRecordedLoader<Projection>>;

/// Pure projection algorithms still owned by the original web product layer.
pub trait NativeForecastRecordedProjection {
    type Error: Clone;

    /// Build the exact recorded observation-point array from validated typed rows.
    fn forecast_observation_points(
        &self,
        records: &[ForecastRecordedInputRecord],
        projection_from: &AdmittedForecastRecordedInstant,
        through: &AdmittedForecastRecordedInstant,
    ) -> Result<JsonValue, Self::Error>;

    /// Build the optional exact solar-cloud evidence object from validated typed rows.
    fn solar_cloud_evidence(
        &self,
        records: &[ForecastRecordedInputRecord],
        site: ForecastRecordedSolarSite,
        through: &AdmittedForecastRecordedInstant,
    ) -> Result<Option<JsonValue>, Self::Error>;
}

/// Concrete public recorded-products read owner over one shared five-minute cache.
pub struct NativeEdgeRecordedOwner<Projection: NativeForecastRecordedProjection> {
    cache: NativeRecordedCache<Projection>,
}

impl<Projection: NativeForecastRecordedProjection> NativeEdgeRecordedOwner<Projection> {
    /// Bind one admitted API origin, process client, native clocks, and projection owner.
    pub fn new(
        origin: AdmittedPublicApiOrigin,
        client: Arc<NativeOutboundHttp>,
        projection: Projection,
    ) -> Result<Self, NativeEdgeRecordedOwnerCreateError> {
        let timezone = load_named_timezone(SITE_TIMEZONE)
            .ok_or(NativeEdgeRecordedOwnerCreateError::Timezone)?;
        let clock = NativeForecastRecordedClock::new(timezone.clone());
        let http = NativeForecastRecordedHttp::new(origin, client, clock.clone())?;
        let boundary = NativeForecastRecordedBoundary {
            projection,
            timezone,
            wire: NativeWirePrimitives,
        };
        let loader = HttpForecastRecordedProductsLoader::new(
            http,
            clock.clone(),
            boundary,
            FORECAST_RECORDED_MAXIMUM_BYTES,
            FORECAST_RECORDED_TIMEOUT_MS,
        );
        Ok(Self {
            cache: ForecastRecordedProductsCache::new(clock, loader, FORECAST_RECORDED_RETRY_MS),
        })
    }

    /// Execute one existing queryless public response policy over the native cache.
    pub fn handle(
        &self,
        request: ForecastRecordedProductRequest,
    ) -> ForecastRecordedProductResponse {
        handle_forecast_recorded_product(request, self)
    }
}

impl<Projection> ForecastRecordedProductsRead for NativeEdgeRecordedOwner<Projection>
where
    Projection: NativeForecastRecordedProjection,
{
    type Error = ForecastRecordedProductsCacheError<
        weather_edge::ForecastRecordedLoaderError<
            NativeForecastRecordedHttpError,
            NativeForecastRecordedBoundaryError<Projection::Error>,
        >,
    >;

    /// Delegate one read to the retained same-day cache.
    fn get_recorded_products(&self) -> Result<AdmittedForecastRecordedProducts, Self::Error> {
        self.cache.get()
    }
}

/// Startup failures before any private input request is sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEdgeRecordedOwnerCreateError {
    Origin,
    Timezone,
}

impl Display for NativeEdgeRecordedOwnerCreateError {
    /// Render the bounded startup refusal.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // distinguish origin admission from required civil time
        match self {
            Self::Origin => formatter.write_str("forecast recorded API origin is invalid"),
            Self::Timezone => formatter.write_str("forecast recorded timezone is unavailable"),
        }
    }
}

impl Error for NativeEdgeRecordedOwnerCreateError {}

/// Initialize the existing validated online archive owners below one admitted root.
pub fn initialize_native_edge_online_archive<'a, Codec, Calendar, Clock, Statfs>(
    root: PathBuf,
    material: &'a ForecastEvidenceMaterial<Codec>,
    calendar: &'a Calendar,
    clock: Clock,
    statfs: Statfs,
) -> io::Result<NativeForecastSchedulerArchive<'a, Codec, Calendar, Clock, Statfs>>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Calendar: weather_edge::ForecastSchedulerCalendarPort,
    Clock: RevisionArchiveCanonicalClock,
    Statfs: ForecastAllocationStatfsPort,
{
    NativeForecastSchedulerArchive::initialize(root, material, calendar, clock, statfs)
}

/// Shared monotonic, wall-clock, and farm-calendar source for one cache graph.
#[derive(Clone)]
struct NativeForecastRecordedClock {
    monotonic_origin: Instant,
    timezone: TimeZone,
}

impl NativeForecastRecordedClock {
    /// Retain one monotonic origin and admitted farm timezone.
    fn new(timezone: TimeZone) -> Self {
        Self {
            monotonic_origin: Instant::now(),
            timezone,
        }
    }

    /// Sample one representable process wall-clock instant.
    fn wall_epoch_ms(&self) -> i64 {
        system_worker_health_milliseconds()
            .expect("system time is outside the native millisecond range")
    }
}

impl ForecastRecordedProductsClock for NativeForecastRecordedClock {
    /// Observe one cache instant and its farm-local calendar day.
    fn now(&self) -> AdmittedForecastRecordedNow {
        let epoch_ms = self.wall_epoch_ms();
        let timestamp = jiff::Timestamp::from_millisecond(epoch_ms)
            .expect("system time is outside Jiff's timestamp range");
        AdmittedForecastRecordedNow {
            epoch_ms,
            local_day: self.timezone.to_datetime(timestamp).date().to_string(),
        }
    }
}

impl ForecastRecordedLoaderClock for NativeForecastRecordedClock {
    /// Return milliseconds from the graph's one monotonic origin.
    fn monotonic_ms(&self) -> u64 {
        u64::try_from(self.monotonic_origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Sample the process wall clock after response consumption.
    fn wall_epoch_ms(&self) -> i64 {
        NativeForecastRecordedClock::wall_epoch_ms(self)
    }
}

/// Native fixed-origin HTTP adapter for the private compact input.
struct NativeForecastRecordedHttp {
    client: Arc<NativeOutboundHttp>,
    clock: NativeForecastRecordedClock,
    origin: Url,
}

impl NativeForecastRecordedHttp {
    /// Revalidate one admitted root origin and retain the shared process client.
    fn new(
        origin: AdmittedPublicApiOrigin,
        client: Arc<NativeOutboundHttp>,
        clock: NativeForecastRecordedClock,
    ) -> Result<Self, NativeEdgeRecordedOwnerCreateError> {
        let origin =
            Url::parse(origin.as_str()).map_err(|_| NativeEdgeRecordedOwnerCreateError::Origin)?;
        // require one credential-free HTTP root origin
        if !matches!(origin.scheme(), "http" | "https")
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
        {
            return Err(NativeEdgeRecordedOwnerCreateError::Origin);
        }
        Ok(Self {
            client,
            clock,
            origin,
        })
    }
}

impl ForecastRecordedInputHttpPort for NativeForecastRecordedHttp {
    type Error = NativeForecastRecordedHttpError;
    type Response = NativeForecastRecordedResponse;

    /// Execute only the fixed private GET with redirect refusal and its remaining deadline.
    fn execute(
        &self,
        request: &ForecastRecordedInputRequest,
    ) -> Result<Self::Response, Self::Error> {
        // reject alternate transport geometry
        if request.method != weather_edge::ForecastRecordedInputMethod::Get
            || request.path != "/internal/v1/forecast-observation-input"
            || request.accept != "application/json"
            || request.redirect != ForecastRecordedRedirectPolicy::Error
        {
            return Err(NativeForecastRecordedHttpError::InvalidRequest);
        }
        let target = self
            .origin
            .join(request.path)
            .map_err(|_| NativeForecastRecordedHttpError::InvalidRequest)?;
        // preserve the admitted origin after relative resolution
        if target.origin() != self.origin.origin() {
            return Err(NativeForecastRecordedHttpError::InvalidRequest);
        }
        let accept = HeaderValue::from_str(request.accept)
            .map_err(|_| NativeForecastRecordedHttpError::InvalidRequest)?;
        let timeout_ms = request
            .deadline_monotonic_ms
            .saturating_sub(self.clock.monotonic_ms());
        let response = self
            .client
            .execute(
                NativeOutboundHttpRequest::new(Method::GET, target)
                    .map_err(NativeForecastRecordedHttpError::from)?
                    .header(ACCEPT, accept)
                    .redirect_limit(0)
                    .stream_body_limit_only(),
                Duration::from_millis(timeout_ms),
                u64::try_from(FORECAST_RECORDED_MAXIMUM_BYTES)
                    .expect("recorded response limit fits u64")
                    .saturating_add(1),
            )
            .map_err(NativeForecastRecordedHttpError::from)?;
        Ok(NativeForecastRecordedResponse { response })
    }
}

/// Clone-safe native HTTP categories required by shared failed-fill waiters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeForecastRecordedHttpError {
    Body,
    Deadline,
    InvalidRequest,
    Queue,
    Transport,
}

impl From<NativeOutboundHttpError> for NativeForecastRecordedHttpError {
    /// Reduce non-clone transport details after the owning adapter handles them.
    fn from(error: NativeOutboundHttpError) -> Self {
        // preserve public operational categories only
        match error {
            NativeOutboundHttpError::BodyDecode
            | NativeOutboundHttpError::BodyTooLarge { .. }
            | NativeOutboundHttpError::UnsupportedContentEncoding => Self::Body,
            NativeOutboundHttpError::Deadline => Self::Deadline,
            NativeOutboundHttpError::InvalidRequest
            | NativeOutboundHttpError::InvalidRedirect
            | NativeOutboundHttpError::TooManyRedirects => Self::InvalidRequest,
            NativeOutboundHttpError::QueueSaturated => Self::Queue,
            NativeOutboundHttpError::RuntimeStart(_)
            | NativeOutboundHttpError::RuntimeStopped
            | NativeOutboundHttpError::ThreadStart(_)
            | NativeOutboundHttpError::TlsConfiguration(_)
            | NativeOutboundHttpError::Transport(_) => Self::Transport,
        }
    }
}

impl Display for NativeForecastRecordedHttpError {
    /// Render only the bounded private-input category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // avoid retaining origins or transport messages
        match self {
            Self::Body => formatter.write_str("forecast recorded response body failed"),
            Self::Deadline => formatter.write_str("forecast recorded request timed out"),
            Self::InvalidRequest => formatter.write_str("forecast recorded request is invalid"),
            Self::Queue => formatter.write_str("forecast recorded HTTP queue is full"),
            Self::Transport => formatter.write_str("forecast recorded transport failed"),
        }
    }
}

impl Error for NativeForecastRecordedHttpError {}

/// One fully consumed native response replayed through the bounded loader contract.
struct NativeForecastRecordedResponse {
    response: NativeOutboundHttpResponse,
}

impl ForecastRecordedInputResponse for NativeForecastRecordedResponse {
    type Error = NativeForecastRecordedHttpError;

    /// Report status and Fetch-compatible null-body status classes.
    fn head(&self) -> ForecastRecordedInputResponseHead {
        ForecastRecordedInputResponseHead {
            body_present: !matches!(self.response.status, 101 | 204 | 205 | 304),
            status_code: self.response.status,
        }
    }

    /// Replay native chunks until complete or bounded receiver refusal.
    fn read_body(
        &mut self,
        receiver: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<ForecastRecordedBodyReadOutcome, Self::Error> {
        // stop at the first refused chunk
        for chunk in self.response.body.chunks(RESPONSE_CHUNK_BYTES) {
            // preserve immediate receiver cancellation
            if !receiver(chunk) {
                return Ok(ForecastRecordedBodyReadOutcome::ReceiverStopped);
            }
        }
        Ok(ForecastRecordedBodyReadOutcome::Complete)
    }
}

/// Native JSON, timezone, UTC, and output-envelope owner around pure projections.
struct NativeForecastRecordedBoundary<Projection> {
    projection: Projection,
    timezone: TimeZone,
    wire: NativeWirePrimitives,
}

impl<Projection> ForecastRecordedLoaderBoundary for NativeForecastRecordedBoundary<Projection>
where
    Projection: NativeForecastRecordedProjection,
{
    type Error = NativeForecastRecordedBoundaryError<Projection::Error>;
    type SolarCloudEvidence = JsonValue;

    /// Decode the exact-key private input into typed records.
    fn decode_input(&self, bytes: &[u8]) -> Result<DecodedForecastRecordedInput, Self::Error> {
        decode_recorded_input(bytes, &self.wire).ok_or(NativeForecastRecordedBoundaryError::Json)
    }

    /// Project one canonical instant into the fixed farm timezone day.
    fn local_day(
        &self,
        instant: &AdmittedForecastRecordedInstant,
        timezone: &str,
    ) -> Result<String, Self::Error> {
        // require the single reviewed site timezone
        if timezone != SITE_TIMEZONE {
            return Err(NativeForecastRecordedBoundaryError::Time);
        }
        let timestamp = jiff::Timestamp::from_millisecond(instant.epoch_ms)
            .map_err(|_| NativeForecastRecordedBoundaryError::Time)?;
        Ok(self.timezone.to_datetime(timestamp).date().to_string())
    }

    /// Resolve one unambiguous farm-local midnight.
    fn site_midnight(
        &self,
        local_day: &str,
        timezone: &str,
    ) -> Result<AdmittedForecastRecordedInstant, Self::Error> {
        // require the single reviewed site timezone
        if timezone != SITE_TIMEZONE {
            return Err(NativeForecastRecordedBoundaryError::Time);
        }
        let date = local_day
            .parse::<Date>()
            .map_err(|_| NativeForecastRecordedBoundaryError::Time)?;
        // reject normalized or expanded calendar spellings
        if date.to_string() != local_day {
            return Err(NativeForecastRecordedBoundaryError::Time);
        }
        let timestamp = self
            .timezone
            .to_timestamp(date.at(0, 0, 0, 0))
            .map_err(|_| NativeForecastRecordedBoundaryError::Time)?;
        self.utc_instant(timestamp.as_millisecond())
    }

    /// Format one epoch through the shared canonical millisecond owner.
    fn utc_instant(&self, epoch_ms: i64) -> Result<AdmittedForecastRecordedInstant, Self::Error> {
        self.wire
            .format_utc_milliseconds(epoch_ms)
            .map(|value| AdmittedForecastRecordedInstant::admitted(value, epoch_ms))
            .ok_or(NativeForecastRecordedBoundaryError::Time)
    }

    /// Build and encode the exact public recorded-observation envelope.
    fn forecast_observations(
        &self,
        records: &[ForecastRecordedInputRecord],
        projection_from: &AdmittedForecastRecordedInstant,
        through: &AdmittedForecastRecordedInstant,
    ) -> Result<AdmittedForecastObservationsJson, Self::Error> {
        let points = self
            .projection
            .forecast_observation_points(records, projection_from, through)
            .map_err(NativeForecastRecordedBoundaryError::Projection)?;
        // require the source algorithm's array result
        if !matches!(points, JsonValue::Array(_)) {
            return Err(NativeForecastRecordedBoundaryError::Json);
        }
        compact_json(
            &self.wire,
            &object(vec![
                ("generatedAt", text(&through.value)),
                ("data", points),
            ]),
        )
        .map(AdmittedForecastObservationsJson::admitted)
        .ok_or(NativeForecastRecordedBoundaryError::Json)
    }

    /// Build the optional solar-cloud evidence independently from envelope encoding.
    fn build_solar_cloud_evidence(
        &self,
        records: &[ForecastRecordedInputRecord],
        site: ForecastRecordedSolarSite,
        through: &AdmittedForecastRecordedInstant,
    ) -> Result<Self::SolarCloudEvidence, Self::Error> {
        self.projection
            .solar_cloud_evidence(records, site, through)
            .map_err(NativeForecastRecordedBoundaryError::Projection)?
            .ok_or(NativeForecastRecordedBoundaryError::OptionalEvidence)
    }

    /// Encode projected evidence or the source's valid null fallback.
    fn encode_solar_cloud_evidence(
        &self,
        through: &AdmittedForecastRecordedInstant,
        evidence: Option<&Self::SolarCloudEvidence>,
    ) -> Result<AdmittedSolarCloudEvidenceJson, Self::Error> {
        // accept only an object when optional evidence exists
        if evidence.is_some_and(|value| !matches!(value, JsonValue::Object(_))) {
            return Err(NativeForecastRecordedBoundaryError::Json);
        }
        compact_json(
            &self.wire,
            &object(vec![
                ("contractVersion", text("solar-cloud-evidence/v1")),
                ("generatedAt", text(&through.value)),
                ("evidence", evidence.cloned().unwrap_or(JsonValue::Null)),
            ]),
        )
        .map(AdmittedSolarCloudEvidenceJson::admitted)
        .ok_or(NativeForecastRecordedBoundaryError::Json)
    }
}

/// Clone-safe native value or injected projection failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeForecastRecordedBoundaryError<ProjectionError> {
    Json,
    OptionalEvidence,
    Projection(ProjectionError),
    Time,
}

impl<ProjectionError: Display> Display for NativeForecastRecordedBoundaryError<ProjectionError> {
    /// Render the owning bounded projection category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the injected algorithm failure only through its owner
        match self {
            Self::Json => formatter.write_str("forecast recorded JSON is invalid"),
            Self::OptionalEvidence => formatter.write_str("solar cloud evidence is unavailable"),
            Self::Projection(error) => Display::fmt(error, formatter),
            Self::Time => formatter.write_str("forecast recorded time is invalid"),
        }
    }
}

impl<ProjectionError> Error for NativeForecastRecordedBoundaryError<ProjectionError>
where
    ProjectionError: Error + 'static,
{
    /// Retain only the injected projection source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // native refusal categories have no underlying source
        match self {
            Self::Projection(error) => Some(error),
            Self::Json | Self::OptionalEvidence | Self::Time => None,
        }
    }
}

/// Decode one complete exact-key private input document.
fn decode_recorded_input(
    bytes: &[u8],
    wire: &NativeWirePrimitives,
) -> Option<DecodedForecastRecordedInput> {
    let value = wire.decode_json(bytes)?;
    let fields = exact_object(
        &value,
        &[
            "contractVersion",
            "generatedAt",
            "projectionFrom",
            "queryFrom",
            "records",
            "site",
            "through",
        ],
    )?;
    let records = array(field(fields, "records")?)?
        .iter()
        .map(|record| decode_record(record, wire))
        .collect::<Option<Vec<_>>>()?;
    let site = exact_object(field(fields, "site")?, &["slug", "timezone"])?;
    Some(DecodedForecastRecordedInput {
        contract_version: string(field(fields, "contractVersion")?)?,
        generated_at: instant(field(fields, "generatedAt")?, wire)?,
        projection_from: instant(field(fields, "projectionFrom")?, wire)?,
        query_from: instant(field(fields, "queryFrom")?, wire)?,
        records,
        site: ForecastRecordedInputSite {
            slug: string(field(site, "slug")?)?,
            timezone: string(field(site, "timezone")?)?,
        },
        through: instant(field(fields, "through")?, wire)?,
    })
}

/// Decode one exact-key compact observation record.
fn decode_record(
    value: &JsonValue,
    wire: &NativeWirePrimitives,
) -> Option<ForecastRecordedInputRecord> {
    let fields = exact_object(
        value,
        &["id", "metrics", "provenance", "receivedAt", "validAt"],
    )?;
    let metrics = exact_object(
        field(fields, "metrics")?,
        &[
            "apparentTemperatureC",
            "cloudCoverPercent",
            "pm25MicrogramsPerCubicMeter",
            "precipitationRateMmPerHour",
            "pressureHpa",
            "relativeHumidityPercent",
            "solarRadiationWm2",
            "temperatureC",
            "uvIndex",
            "windDirectionDegrees",
            "windGustMps",
            "windSpeedMps",
        ],
    )?;
    let provenance = exact_object(
        field(fields, "provenance")?,
        &[
            "providerKey",
            "sourceId",
            "sourceKey",
            "sourceKind",
            "stationSlug",
        ],
    )?;
    let source_kind = match string(field(provenance, "sourceKind")?)?.as_str() {
        "physical_sensor" => ForecastRecordedInputSourceKind::PhysicalSensor,
        "model_current" => ForecastRecordedInputSourceKind::ModelCurrent,
        _ => return None,
    };
    Some(ForecastRecordedInputRecord {
        id: string(field(fields, "id")?)?,
        metrics: ForecastRecordedInputMetrics {
            apparent_temperature_c: nullable_number(field(metrics, "apparentTemperatureC")?)?,
            cloud_cover_percent: nullable_number(field(metrics, "cloudCoverPercent")?)?,
            pm25_micrograms_per_cubic_meter: nullable_number(field(
                metrics,
                "pm25MicrogramsPerCubicMeter",
            )?)?,
            precipitation_rate_mm_per_hour: nullable_number(field(
                metrics,
                "precipitationRateMmPerHour",
            )?)?,
            pressure_hpa: nullable_number(field(metrics, "pressureHpa")?)?,
            relative_humidity_percent: nullable_number(field(metrics, "relativeHumidityPercent")?)?,
            solar_radiation_wm2: nullable_number(field(metrics, "solarRadiationWm2")?)?,
            temperature_c: nullable_number(field(metrics, "temperatureC")?)?,
            uv_index: nullable_number(field(metrics, "uvIndex")?)?,
            wind_direction_degrees: nullable_number(field(metrics, "windDirectionDegrees")?)?,
            wind_gust_mps: nullable_number(field(metrics, "windGustMps")?)?,
            wind_speed_mps: nullable_number(field(metrics, "windSpeedMps")?)?,
        },
        provenance: ForecastRecordedInputProvenance {
            provider_key: string(field(provenance, "providerKey")?)?,
            source_id: string(field(provenance, "sourceId")?)?,
            source_key: string(field(provenance, "sourceKey")?)?,
            source_kind,
            station_slug: string(field(provenance, "stationSlug")?)?,
        },
        received_at: instant(field(fields, "receivedAt")?, wire)?,
        valid_at: instant(field(fields, "validAt")?, wire)?,
    })
}

type JsonFields = [(Vec<u16>, JsonValue)];

/// Require one object with exactly the supplied key set.
fn exact_object<'a>(value: &'a JsonValue, names: &[&str]) -> Option<&'a JsonFields> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    // reject missing, extra, or duplicate-key-normalized fields
    if fields.len() != names.len()
        || !names
            .iter()
            .all(|name| fields.iter().any(|(key, _)| utf16_equals(key, name)))
    {
        return None;
    }
    Some(fields)
}

/// Borrow one required field by its exact UTF-16 key.
fn field<'a>(fields: &'a JsonFields, name: &str) -> Option<&'a JsonValue> {
    fields
        .iter()
        .find(|(key, _)| utf16_equals(key, name))
        .map(|(_, value)| value)
}

/// Compare an owned generated key without allocating another UTF-16 vector.
fn utf16_equals(units: &[u16], value: &str) -> bool {
    units.iter().copied().eq(value.encode_utf16())
}

/// Decode one generated JSON string without lossy replacement.
fn string(value: &JsonValue) -> Option<String> {
    let JsonValue::String(units) = value else {
        return None;
    };
    String::from_utf16(units).ok()
}

/// Borrow one generated JSON array.
fn array(value: &JsonValue) -> Option<&[JsonValue]> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    Some(values)
}

/// Decode one finite optional scalar without manufacturing missing values.
fn nullable_number(value: &JsonValue) -> Option<Option<f64>> {
    // preserve only null or finite number alternatives
    match value {
        JsonValue::Null => Some(None),
        JsonValue::Number(value) if value.is_finite() => Some(Some(*value)),
        _ => None,
    }
}

/// Decode one exact canonical millisecond UTC instant.
fn instant(
    value: &JsonValue,
    wire: &NativeWirePrimitives,
) -> Option<AdmittedForecastRecordedInstant> {
    let value = string(value)?;
    let epoch_ms = wire.canonical_utc_milliseconds(&value)?;
    Some(AdmittedForecastRecordedInstant::admitted(value, epoch_ms))
}

/// Build one ordered generated JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one generated JSON string.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Encode compact JSON by removing only the native encoder's required final line feed.
fn compact_json(wire: &NativeWirePrimitives, value: &JsonValue) -> Option<Vec<u8>> {
    let mut bytes = wire.encode_ordered_json(value)?;
    (bytes.pop() == Some(b'\n')).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify exact-key input decoding through native JSON and canonical time.
    #[test]
    fn decodes_exact_private_input() {
        let bytes = br#"{"contractVersion":"forecast-observation-input/v1","generatedAt":"2026-11-24T16:00:00.000Z","projectionFrom":"2026-11-24T08:00:00.000Z","queryFrom":"2026-11-24T05:00:00.000Z","records":[],"site":{"slug":"ballydidean","timezone":"America/Los_Angeles"},"through":"2026-11-24T16:00:00.000Z"}"#;
        let decoded = decode_recorded_input(bytes, &NativeWirePrimitives).unwrap();
        assert_eq!(decoded.contract_version, "forecast-observation-input/v1");
        assert_eq!(decoded.generated_at.epoch_ms, decoded.through.epoch_ms);
        assert!(decoded.records.is_empty());
    }

    /// Verify compact envelopes preserve source field order without a final line feed.
    #[test]
    fn encodes_compact_recorded_envelope() {
        let bytes = compact_json(
            &NativeWirePrimitives,
            &object(vec![
                ("generatedAt", text("2026-11-24T16:00:00.000Z")),
                ("data", JsonValue::Array(Vec::new())),
            ]),
        )
        .unwrap();
        assert_eq!(
            bytes,
            br#"{"generatedAt":"2026-11-24T16:00:00.000Z","data":[]}"#
        );
    }
}
