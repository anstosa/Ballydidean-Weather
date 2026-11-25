//! Native public-edge ownership for Xweather route, usage, transport, and cache state.

use std::error::Error;
use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use serde_json::{Map, Value};
use url::Url;
use weather_edge::{
    AdmittedXweatherMapSite, AdmittedXweatherProvider, AdmittedXweatherUsageCalendar,
    AdmittedXweatherUsageLedger, AdmittedXweatherValidTime, BoundedXweatherTileTransport,
    DecodedXweatherFrameRoute, DecodedXweatherMapRoute, EdgeXweatherCredentials, ForecastMapSite,
    XweatherBudgetedTileLoadError, XweatherBudgetedTileLoader, XweatherMapProxyMethod,
    XweatherMapProxyRequest, XweatherMapProxyResponse, XweatherMapTileCache,
    XweatherMapTileCacheError, XweatherTileCacheConfigurationError, XweatherTileCacheGetError,
    XweatherTileCacheKey, XweatherTileCacheRequest, XweatherTileCacheResponse,
    XweatherTileCacheStats, XweatherTileClock, XweatherTileLoader, XweatherTileMemoryCache,
    XweatherTileTransportError, XweatherUsageBudget, XweatherUsageBudgetCreateError,
    XweatherUsageCalendarSource, XweatherUsageLedgerError, XweatherUsagePersistence,
    XweatherUsageSnapshot, handle_xweather_map_proxy,
};

use crate::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeXweatherTileDiagnostics,
    NativeXweatherTileHttp, NativeXweatherTileTargets, system_worker_health_milliseconds,
};

const XWEATHER_FORECAST_FRESHNESS_MS: i64 = 60 * 60 * 1_000;
const XWEATHER_MAP_CACHE_BYTES: u64 = 256 * 1_024 * 1_024;
const XWEATHER_PROVIDER_DAILY_MAP_UNIT_BUDGET: u64 = 300;
const XWEATHER_PROVIDER_MONTHLY_MAP_UNIT_BUDGET: u64 = 10_000;
const XWEATHER_PROVIDER_CONCURRENCY: usize = 8;
const XWEATHER_PROVIDER_MINIMUM_INTERVAL: Duration = Duration::from_millis(125);
const XWEATHER_PROVIDER_QUEUE_LIMIT: usize = 512;
const JAVASCRIPT_MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const UTC_DAY_MILLISECONDS: i64 = 24 * 60 * 60 * 1_000;
const COORDINATE_SCALE: f64 = 1_000_000.0;

type NativeTileTransport = BoundedXweatherTileTransport<
    NativeXweatherTileTargets,
    NativeXweatherTileHttp,
    NativeXweatherTileDiagnostics,
>;
type NativeTileBudget = XweatherUsageBudget<NativeEdgeTileTime, NativeXweatherUsagePersistence>;
type NativeBudgetedTileLoader = XweatherBudgetedTileLoader<
    NativeEdgeTileTime,
    NativeXweatherUsagePersistence,
    NativeEdgeProviderTileLoader,
>;
type NativeTileCache = XweatherTileMemoryCache<NativeScheduledTileLoader, NativeEdgeTileTime>;

/// Project one admitted template site into the exact six-decimal route identity.
pub fn native_edge_tile_site(site: ForecastMapSite) -> Option<AdmittedXweatherMapSite> {
    // retain the existing public template bounds
    if !(-85.0..=85.0).contains(&site.latitude) || !(-180.0..=180.0).contains(&site.longitude) {
        return None;
    }
    let latitude = decimal_coordinate(&javascript_fixed_six(site.latitude), 2)?;
    let longitude = decimal_coordinate(&javascript_fixed_six(site.longitude), 3)?;
    Some(AdmittedXweatherMapSite {
        latitude_millionths: latitude.millionths,
        longitude_millionths: longitude.millionths,
    })
}

/// Concrete Xweather public-route owner with one process cache and usage ledger.
pub struct NativeEdgeTileOwner {
    budget: Arc<NativeTileBudget>,
    cache: NativeTileCache,
    provider_available: bool,
    time: NativeEdgeTileTime,
}

impl NativeEdgeTileOwner {
    /// Construct the exact deployed budget, cache, and optional provider transport graph.
    pub fn new(
        origin: Url,
        credentials: Option<EdgeXweatherCredentials>,
        client: Arc<NativeOutboundHttp>,
        usage_path: Option<PathBuf>,
    ) -> Result<Self, NativeEdgeTileOwnerCreateError> {
        let provider_available = credentials.is_some();
        let transport = credentials
            .map(|credentials| {
                NativeXweatherTileTargets::new(origin, credentials).map(|targets| {
                    BoundedXweatherTileTransport::new(
                        targets,
                        NativeXweatherTileHttp::new(client),
                        NativeXweatherTileDiagnostics,
                    )
                })
            })
            .transpose()
            .map_err(NativeEdgeTileOwnerCreateError::Transport)?;
        let time = NativeEdgeTileTime::system();
        let budget = Arc::new(
            XweatherUsageBudget::new(
                XWEATHER_PROVIDER_DAILY_MAP_UNIT_BUDGET,
                XWEATHER_PROVIDER_MONTHLY_MAP_UNIT_BUDGET,
                time,
                NativeXweatherUsagePersistence::new(usage_path),
            )
            .map_err(NativeEdgeTileOwnerCreateError::Budget)?,
        );
        let budgeted_loader = XweatherBudgetedTileLoader::new(
            Arc::clone(&budget),
            NativeEdgeProviderTileLoader { transport },
        );
        let loader = NativeScheduledTileLoader::new(budgeted_loader);
        let cache = XweatherTileMemoryCache::new(
            loader,
            time,
            XWEATHER_MAP_CACHE_BYTES,
            XWEATHER_FORECAST_FRESHNESS_MS,
        )
        .map_err(NativeEdgeTileOwnerCreateError::Cache)?;
        Ok(Self {
            budget,
            cache,
            provider_available,
            time,
        })
    }

    /// Decode one HTTP path and bind unforgeable provider and wall-clock route inputs.
    pub fn request(
        &self,
        method: XweatherMapProxyMethod,
        pathname: &str,
        site: AdmittedXweatherMapSite,
    ) -> XweatherMapProxyRequest {
        let now_epoch_ms = self.time.now_epoch_ms();
        XweatherMapProxyRequest {
            method,
            now_epoch_ms,
            provider: self
                .provider_available
                .then_some(AdmittedXweatherProvider::admitted()),
            retry_after_utc_day_seconds: seconds_until_utc_day(now_epoch_ms),
            route: decode_xweather_frame_route(pathname),
            site,
        }
    }

    /// Execute one decoded public route through the existing response policy and cache.
    pub fn handle(
        &self,
        method: XweatherMapProxyMethod,
        pathname: &str,
        site: AdmittedXweatherMapSite,
    ) -> XweatherMapProxyResponse {
        handle_xweather_map_proxy(self.request(method, pathname, site), self)
    }

    /// Return bounded cache counters without exposing tile bytes.
    pub fn cache_stats(&self) -> XweatherTileCacheStats {
        self.cache.stats()
    }

    /// Return the persisted provider usage counters after any UTC rollover.
    pub fn usage_snapshot(
        &self,
    ) -> Result<
        XweatherUsageSnapshot,
        weather_edge::XweatherUsageBudgetError<NativeXweatherUsagePersistenceError>,
    > {
        self.budget.snapshot()
    }

    /// Report whether startup admitted a complete server-side credential pair.
    pub const fn provider_available(&self) -> bool {
        self.provider_available
    }
}

impl XweatherMapTileCache for NativeEdgeTileOwner {
    type Error = NativeEdgeTileCacheError;

    /// Delegate admitted lookups to the one retained process cache.
    fn get_tile(
        &self,
        request: &XweatherTileCacheRequest,
    ) -> Result<XweatherTileCacheResponse, Self::Error> {
        self.cache.get(request)
    }
}

/// Construction failures before the public route becomes callable.
#[derive(Debug)]
pub enum NativeEdgeTileOwnerCreateError {
    Budget(XweatherUsageBudgetCreateError<NativeXweatherUsagePersistenceError>),
    Cache(XweatherTileCacheConfigurationError),
    Transport(NativeOutboundHttpError),
}

impl Display for NativeEdgeTileOwnerCreateError {
    /// Preserve the owning startup failure without credential or target material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the owning category
        match self {
            Self::Budget(error) => Display::fmt(error, formatter),
            Self::Cache(error) => Display::fmt(error, formatter),
            Self::Transport(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for NativeEdgeTileOwnerCreateError {
    /// Retain the exact local configuration or transport source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // expose only the owning failure
        match self {
            Self::Budget(error) => Some(error),
            Self::Cache(error) => Some(error),
            Self::Transport(error) => Some(error),
        }
    }
}

/// Complete public cache failure after budget and transport composition.
pub type NativeEdgeTileCacheError = XweatherTileCacheGetError<NativeScheduledTileFailure>;

/// Queue admission or an admitted budget/provider load failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeScheduledTileFailure {
    Load(
        XweatherBudgetedTileLoadError<
            NativeXweatherUsagePersistenceError,
            NativeEdgeProviderTileFailure,
        >,
    ),
    QueueFull,
}

impl Display for NativeScheduledTileFailure {
    /// Preserve queue and admitted provider failure categories.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the owning failure
        match self {
            Self::Load(error) => Display::fmt(error, formatter),
            Self::QueueFull => formatter.write_str("Xweather provider queue is full"),
        }
    }
}

impl Error for NativeScheduledTileFailure {
    /// Retain only an admitted budget or provider source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // queue refusal has no underlying error
        match self {
            Self::Load(error) => Some(error),
            Self::QueueFull => None,
        }
    }
}

impl XweatherMapTileCacheError for NativeScheduledTileFailure {
    /// Report only the nested budget exhaustion category.
    fn is_budget_exhausted(&self) -> bool {
        matches!(self, Self::Load(error) if error.is_budget_exhausted())
    }
}

#[derive(Debug)]
struct NativeProviderScheduleState {
    active: usize,
    last_started: Option<Instant>,
    next_ticket: u64,
    queue: std::collections::VecDeque<u64>,
}

/// Exact bounded FIFO start scheduler outside budget reservation.
struct NativeScheduledTileLoader {
    loader: NativeBudgetedTileLoader,
    ready: Condvar,
    state: Mutex<NativeProviderScheduleState>,
}

impl NativeScheduledTileLoader {
    /// Construct one empty process scheduler around the budgeted provider.
    fn new(loader: NativeBudgetedTileLoader) -> Self {
        Self {
            loader,
            ready: Condvar::new(),
            state: Mutex::new(NativeProviderScheduleState {
                active: 0,
                last_started: None,
                next_ticket: 0,
                queue: std::collections::VecDeque::new(),
            }),
        }
    }

    /// Admit one bounded FIFO waiter and block until its provider start slot.
    fn acquire(&self) -> Result<NativeProviderPermit<'_>, NativeScheduledTileFailure> {
        let mut state = self
            .state
            .lock()
            .expect("Xweather provider schedule lock poisoned");
        // reject before retaining one more queued request
        if state.queue.len() >= XWEATHER_PROVIDER_QUEUE_LIMIT {
            return Err(NativeScheduledTileFailure::QueueFull);
        }
        let ticket = state.next_ticket;
        state.next_ticket = state.next_ticket.wrapping_add(1);
        state.queue.push_back(ticket);

        // wait for FIFO ownership, active capacity, and the minimum start interval
        loop {
            let owns_front = state.queue.front().copied() == Some(ticket);
            let has_capacity = state.active < XWEATHER_PROVIDER_CONCURRENCY;
            let remaining = state.last_started.map_or(Duration::ZERO, |last_started| {
                XWEATHER_PROVIDER_MINIMUM_INTERVAL.saturating_sub(last_started.elapsed())
            });
            // start only after every admission condition holds
            if owns_front && has_capacity && remaining.is_zero() {
                let removed = state.queue.pop_front();
                debug_assert_eq!(removed, Some(ticket));
                state.active += 1;
                state.last_started = Some(Instant::now());
                self.ready.notify_all();
                return Ok(NativeProviderPermit { scheduler: self });
            }
            // wake the head on its interval or any capacity change
            if owns_front && has_capacity {
                let (next, _) = self
                    .ready
                    .wait_timeout(state, remaining)
                    .expect("Xweather provider schedule lock poisoned");
                state = next;
            } else {
                state = self
                    .ready
                    .wait(state)
                    .expect("Xweather provider schedule lock poisoned");
            }
        }
    }
}

impl XweatherTileLoader for NativeScheduledTileLoader {
    type Error = NativeScheduledTileFailure;

    /// Queue before reserving budget and release capacity after every provider outcome.
    fn load_tile(&self, request: &XweatherTileCacheRequest) -> Result<Vec<u8>, Self::Error> {
        let _permit = self.acquire()?;
        self.loader
            .load_tile(request)
            .map_err(NativeScheduledTileFailure::Load)
    }
}

struct NativeProviderPermit<'a> {
    scheduler: &'a NativeScheduledTileLoader,
}

impl Drop for NativeProviderPermit<'_> {
    /// Release one active provider slot and wake the FIFO head.
    fn drop(&mut self) {
        let mut state = self
            .scheduler
            .state
            .lock()
            .expect("Xweather provider schedule lock poisoned");
        state.active = state
            .active
            .checked_sub(1)
            .expect("Xweather provider active count underflow");
        self.scheduler.ready.notify_all();
    }
}

/// Clone-safe provider category required by same-key cache waiters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEdgeProviderTileFailure {
    Disabled,
    Http,
    InvalidResponse,
    ResponseLimitExceeded,
    Target,
    UnsupportedLoad,
}

impl Display for NativeEdgeProviderTileFailure {
    /// Render only a bounded provider category without target details.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve externally useful failure classes
        match self {
            Self::Disabled => formatter.write_str("Xweather credentials are unavailable"),
            Self::Http => formatter.write_str("Xweather tile transport failed"),
            Self::InvalidResponse => {
                formatter.write_str("Xweather tile provider returned an invalid response")
            }
            Self::ResponseLimitExceeded => {
                formatter.write_str("weather map response exceeded the edge limit")
            }
            Self::Target => formatter.write_str("Xweather tile target is invalid"),
            Self::UnsupportedLoad => {
                formatter.write_str("Xweather tile loader requires a parsed frame")
            }
        }
    }
}

impl Error for NativeEdgeProviderTileFailure {}

/// Optional credential-bound provider behind one cache-compatible failure type.
struct NativeEdgeProviderTileLoader {
    transport: Option<NativeTileTransport>,
}

impl XweatherTileLoader for NativeEdgeProviderTileLoader {
    type Error = NativeEdgeProviderTileFailure;

    /// Execute one provider load or retain the disabled-route invariant.
    fn load_tile(&self, request: &XweatherTileCacheRequest) -> Result<Vec<u8>, Self::Error> {
        let transport = self
            .transport
            .as_ref()
            .ok_or(NativeEdgeProviderTileFailure::Disabled)?;
        transport.load_tile(request).map_err(|error| {
            // reduce non-clone native failures after safe diagnostics run
            match error {
                XweatherTileTransportError::Http(_) => NativeEdgeProviderTileFailure::Http,
                XweatherTileTransportError::InvalidResponse { .. } => {
                    NativeEdgeProviderTileFailure::InvalidResponse
                }
                XweatherTileTransportError::ResponseLimitExceeded => {
                    NativeEdgeProviderTileFailure::ResponseLimitExceeded
                }
                XweatherTileTransportError::Target(_) => NativeEdgeProviderTileFailure::Target,
                XweatherTileTransportError::UnsupportedLoad => {
                    NativeEdgeProviderTileFailure::UnsupportedLoad
                }
            }
        })
    }
}

/// Native UTC time shared by cache freshness and provider calendar budgets.
#[derive(Clone, Copy)]
struct NativeEdgeTileTime {
    now: fn() -> Option<i64>,
}

impl NativeEdgeTileTime {
    /// Bind the process wall clock.
    const fn system() -> Self {
        Self {
            now: system_worker_health_milliseconds,
        }
    }

    /// Sample one representable Unix millisecond instant.
    fn now_epoch_ms(self) -> i64 {
        (self.now)().expect("system time is outside the native millisecond range")
    }
}

impl XweatherTileClock for NativeEdgeTileTime {
    /// Sample the process clock for one freshness decision.
    fn now_epoch_ms(&self) -> i64 {
        (*self).now_epoch_ms()
    }
}

impl XweatherUsageCalendarSource for NativeEdgeTileTime {
    /// Project the process instant into the exact UTC day and month identities.
    fn current_calendar(&self) -> AdmittedXweatherUsageCalendar {
        let timestamp = Timestamp::from_millisecond((*self).now_epoch_ms())
            .expect("system time is outside Jiff's timestamp range");
        let date = jiff::tz::TimeZone::UTC.to_datetime(timestamp).date();
        AdmittedXweatherUsageCalendar::admitted(
            format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day()),
            format!("{:04}-{:02}", date.year(), date.month()),
        )
    }
}

/// Atomic optional-file persistence for the source usage-ledger shape.
#[derive(Debug)]
struct NativeXweatherUsagePersistence {
    path: Option<PathBuf>,
}

impl NativeXweatherUsagePersistence {
    /// Retain the explicitly configured optional persistence path.
    fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }
}

impl XweatherUsagePersistence for NativeXweatherUsagePersistence {
    type Error = NativeXweatherUsagePersistenceError;

    /// Load one exact source-compatible ledger or the first-run empty state.
    fn load(&mut self) -> Result<Option<AdmittedXweatherUsageLedger>, Self::Error> {
        let Some(path) = &self.path else {
            return Ok(None);
        };
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            // accept a missing first-run ledger
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        decode_usage_ledger(&bytes).map(Some)
    }

    /// Replace one complete source-compatible ledger atomically.
    fn persist(&mut self, state: &AdmittedXweatherUsageLedger) -> Result<(), Self::Error> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let directory = nonempty_parent(path);
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o750)
            .create(directory)
            .map_err(NativeXweatherUsagePersistenceError::from)?;
        let temporary = temporary_usage_path(path);
        let bytes = format!(
            "{{\"day\":\"{}\",\"dayUnits\":{},\"month\":\"{}\",\"monthUnits\":{}}}\n",
            state.calendar().day(),
            state.day_units(),
            state.calendar().month(),
            state.month_units(),
        );
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(NativeXweatherUsagePersistenceError::from)?;
        file.write_all(bytes.as_bytes())
            .map_err(NativeXweatherUsagePersistenceError::from)?;
        drop(file);
        fs::rename(temporary, path).map_err(NativeXweatherUsagePersistenceError::from)
    }
}

/// Bounded usage-ledger failure suitable for cache waiter cloning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeXweatherUsagePersistenceError {
    Invalid,
    Io(io::ErrorKind),
}

impl From<io::Error> for NativeXweatherUsagePersistenceError {
    /// Retain only the portable I/O category.
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

impl Display for NativeXweatherUsagePersistenceError {
    /// Render a bounded startup or reservation diagnosis.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // distinguish corrupt material from host I/O
        match self {
            Self::Invalid => formatter.write_str("invalid persisted Xweather usage budget"),
            Self::Io(kind) => write!(formatter, "Xweather usage persistence failed: {kind:?}"),
        }
    }
}

impl Error for NativeXweatherUsagePersistenceError {}

/// Decode the permissive object shape while requiring each source-owned field.
fn decode_usage_ledger(
    bytes: &[u8],
) -> Result<AdmittedXweatherUsageLedger, NativeXweatherUsagePersistenceError> {
    let Value::Object(fields) =
        serde_json::from_slice(bytes).map_err(|_| NativeXweatherUsagePersistenceError::Invalid)?
    else {
        return Err(NativeXweatherUsagePersistenceError::Invalid);
    };
    let day = json_string(&fields, "day")?;
    let month = json_string(&fields, "month")?;
    // require canonical calendar identities
    if !canonical_day(day) || !canonical_month(month) {
        return Err(NativeXweatherUsagePersistenceError::Invalid);
    }
    let day_units = json_safe_integer(&fields, "dayUnits")?;
    let month_units = json_safe_integer(&fields, "monthUnits")?;
    AdmittedXweatherUsageLedger::admitted(
        AdmittedXweatherUsageCalendar::admitted(day, month),
        day_units,
        month_units,
    )
    .map_err(|XweatherUsageLedgerError| NativeXweatherUsagePersistenceError::Invalid)
}

/// Borrow one required JSON string field.
fn json_string<'a>(
    fields: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, NativeXweatherUsagePersistenceError> {
    fields
        .get(name)
        .and_then(Value::as_str)
        .ok_or(NativeXweatherUsagePersistenceError::Invalid)
}

/// Decode one nonnegative JavaScript-safe integer, including an integral decimal spelling.
fn json_safe_integer(
    fields: &Map<String, Value>,
    name: &str,
) -> Result<u64, NativeXweatherUsagePersistenceError> {
    let value = fields
        .get(name)
        .and_then(Value::as_f64)
        .ok_or(NativeXweatherUsagePersistenceError::Invalid)?;
    // preserve Number.isSafeInteger admission
    if !value.is_finite()
        || !(0.0..=JAVASCRIPT_MAXIMUM_SAFE_INTEGER).contains(&value)
        || value.fract() != 0.0
    {
        return Err(NativeXweatherUsagePersistenceError::Invalid);
    }
    Ok(value as u64)
}

/// Require the exact persisted UTC day spelling.
fn canonical_day(value: &str) -> bool {
    value.len() == 10
        && value.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7) && byte == b'-'
                || !matches!(index, 4 | 7) && byte.is_ascii_digit()
        })
}

/// Require the exact persisted UTC month spelling.
fn canonical_month(value: &str) -> bool {
    value.len() == 7
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| index == 4 && byte == b'-' || index != 4 && byte.is_ascii_digit())
}

/// Select dot for a relative filename without a lexical parent.
fn nonempty_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Match the predecessor's process-specific sibling temporary filename.
fn temporary_usage_path(path: &Path) -> PathBuf {
    let mut temporary: OsString = path.as_os_str().to_owned();
    temporary.push(format!(".{}.tmp", std::process::id()));
    temporary.into()
}

/// Decode the exact static-frame route grammar into the existing application owner.
fn decode_xweather_frame_route(pathname: &str) -> DecodedXweatherMapRoute {
    let Some(frame) = pathname
        .strip_prefix("/maps/xweather/")
        .and_then(|value| value.strip_suffix(".png"))
    else {
        return DecodedXweatherMapRoute::Missing;
    };
    let segments = frame.split('/').collect::<Vec<_>>();
    // require the six exact frame segments
    if segments.len() != 6 {
        return DecodedXweatherMapRoute::Missing;
    }
    let [phase, layer, valid_time, zoom, dimensions, coordinates] = segments.as_slice() else {
        return DecodedXweatherMapRoute::Missing;
    };
    // retain only the public phase and layer grammar
    if !matches!(*phase, "history" | "forecast")
        || !matches!(*layer, "radar" | "clouds" | "precipitation" | "wind")
    {
        return DecodedXweatherMapRoute::Missing;
    }
    let Some(valid_time) = decode_valid_time(valid_time) else {
        return DecodedXweatherMapRoute::Missing;
    };
    let Some(zoom) = decimal_integer(zoom, 1, 2).and_then(|value| u8::try_from(value).ok()) else {
        return DecodedXweatherMapRoute::Missing;
    };
    let Some((width, height)) = dimensions.split_once('x').and_then(|(width, height)| {
        Some((
            u16::try_from(decimal_integer(width, 3, 4)?).ok()?,
            u16::try_from(decimal_integer(height, 3, 4)?).ok()?,
        ))
    }) else {
        return DecodedXweatherMapRoute::Missing;
    };
    let Some((latitude, longitude)) = coordinates.split_once(',') else {
        return DecodedXweatherMapRoute::Missing;
    };
    let Some(latitude) = decimal_coordinate(latitude, 2) else {
        return DecodedXweatherMapRoute::Missing;
    };
    let Some(longitude) = decimal_coordinate(longitude, 3) else {
        return DecodedXweatherMapRoute::Missing;
    };
    let cache_key = format!(
        "frame/{phase}/{layer}/{}/{zoom}/{width}/{height}/{}/{}",
        valid_time.wire_value,
        javascript_number(latitude.value),
        javascript_number(longitude.value),
    );
    DecodedXweatherMapRoute::Frame(DecodedXweatherFrameRoute {
        cache_key: XweatherTileCacheKey::admitted(cache_key),
        height,
        latitude: latitude.value,
        latitude_millionths: latitude.millionths,
        layer: (*layer).to_owned(),
        longitude: longitude.value,
        longitude_millionths: longitude.millionths,
        phase: (*phase).to_owned(),
        valid_time: Some(AdmittedXweatherValidTime::admitted(
            valid_time.wire_value,
            valid_time.epoch_ms,
            valid_time.minute,
            valid_time.second,
        )),
        width,
        zoom,
    })
}

struct DecodedValidTime {
    epoch_ms: i64,
    minute: u8,
    second: u8,
    wire_value: String,
}

/// Parse one canonical fourteen-digit UTC instant without normalization.
fn decode_valid_time(value: &str) -> Option<DecodedValidTime> {
    // require the source route's fixed digits
    if value.len() != 14 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let year = value[0..4].parse::<u16>().ok()?;
    // reject javascript's remapped zero-through-ninety-nine years
    if year < 100 {
        return None;
    }
    let iso = format!(
        "{}-{}-{}T{}:{}:{}Z",
        &value[0..4],
        &value[4..6],
        &value[6..8],
        &value[8..10],
        &value[10..12],
        &value[12..14],
    );
    let timestamp = iso.parse::<Timestamp>().ok()?;
    let datetime = jiff::tz::TimeZone::UTC.to_datetime(timestamp);
    let canonical = format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}",
        datetime.year(),
        datetime.month(),
        datetime.day(),
        datetime.hour(),
        datetime.minute(),
        datetime.second(),
    );
    // reject calendar normalization
    if canonical != value {
        return None;
    }
    Some(DecodedValidTime {
        epoch_ms: timestamp.as_millisecond(),
        minute: u8::try_from(datetime.minute()).ok()?,
        second: u8::try_from(datetime.second()).ok()?,
        wire_value: canonical,
    })
}

/// Parse one ASCII decimal integer with exact digit-count bounds.
fn decimal_integer(value: &str, minimum_digits: usize, maximum_digits: usize) -> Option<u64> {
    // reject signs, whitespace, and alternate numeric grammar
    if !(minimum_digits..=maximum_digits).contains(&value.len())
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

struct DecodedCoordinate {
    millionths: i32,
    value: f64,
}

/// Parse one route coordinate with the original bounded fractional grammar.
fn decimal_coordinate(value: &str, maximum_integer_digits: usize) -> Option<DecodedCoordinate> {
    let (negative, magnitude) = value
        .strip_prefix('-')
        .map_or((false, value), |magnitude| (true, magnitude));
    let (integer, fraction) = magnitude
        .split_once('.')
        .map_or((magnitude, None), |(integer, fraction)| {
            (integer, Some(fraction))
        });
    // require one bounded integer and an optional nonempty six-digit fraction
    if integer.is_empty()
        || integer.len() > maximum_integer_digits
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|fraction| {
            fraction.is_empty()
                || fraction.len() > 6
                || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return None;
    }
    let whole = integer.parse::<i32>().ok()?;
    let fraction = fraction.unwrap_or("");
    let fractional = fraction.parse::<i32>().unwrap_or(0) * 10_i32.pow(6 - fraction.len() as u32);
    let magnitude_millionths = whole.checked_mul(1_000_000)?.checked_add(fractional)?;
    // apply the lexical sign after exact fixed-point parsing
    let millionths = if negative {
        magnitude_millionths.checked_neg()?
    } else {
        magnitude_millionths
    };
    Some(DecodedCoordinate {
        millionths,
        value: value.parse().ok()?,
    })
}

/// Match JavaScript toFixed at the configured six-decimal map boundary.
fn javascript_fixed_six(value: f64) -> String {
    // normalize javascript's exact negative zero
    if value == 0.0 {
        return "0.000000".to_owned();
    }
    let magnitude = value.abs();
    let scaled = magnitude * COORDINATE_SCALE;
    // preserve exact midpoint rounding away from zero
    if scaled.fract() == 0.5 && magnitude.mul_add(COORDINATE_SCALE, -scaled) == 0.0 {
        let rounded = scaled.ceil() as u32;
        let sign = if value < 0.0 { "-" } else { "" };
        return format!("{sign}{}.{:06}", rounded / 1_000_000, rounded % 1_000_000);
    }
    format!("{value:.6}")
}

/// Match JavaScript String for the bounded finite coordinate population.
fn javascript_number(value: f64) -> String {
    // normalize JavaScript's negative-zero spelling
    if value == 0.0 {
        "0".to_owned()
    } else {
        value.to_string()
    }
}

/// Calculate the original positive ceil delay to the next UTC day.
fn seconds_until_utc_day(now_epoch_ms: i64) -> String {
    let timestamp = Timestamp::from_millisecond(now_epoch_ms)
        .expect("system time is outside Jiff's timestamp range");
    let date = jiff::tz::TimeZone::UTC.to_datetime(timestamp).date();
    let start = jiff::tz::TimeZone::UTC
        .to_timestamp(date.at(0, 0, 0, 0))
        .expect("UTC calendar day is representable")
        .as_millisecond();
    let remaining = start
        .saturating_add(UTC_DAY_MILLISECONDS)
        .saturating_sub(now_epoch_ms);
    remaining
        .saturating_add(999)
        .div_euclid(1_000)
        .max(1)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify exact frame grammar, canonical time, and JavaScript cache identity.
    #[test]
    fn decodes_one_exact_public_frame() {
        let DecodedXweatherMapRoute::Frame(frame) = decode_xweather_frame_route(
            "/maps/xweather/history/radar/20261124153000/10/256x168/54.321000,-5.678000.png",
        ) else {
            panic!("expected admitted frame");
        };
        assert_eq!(
            frame.cache_key.as_str(),
            "frame/history/radar/20261124153000/10/256/168/54.321/-5.678"
        );
        assert_eq!(frame.latitude_millionths, 54_321_000);
        assert_eq!(frame.longitude_millionths, -5_678_000);
        assert!(matches!(
            decode_xweather_frame_route(
                "/maps/xweather/history/radar/20260230153000/10/256x168/54.321,-5.678.png"
            ),
            DecodedXweatherMapRoute::Missing
        ));
    }

    /// Verify startup site projection uses the same six-decimal route identity.
    #[test]
    fn projects_template_site_identity() {
        assert_eq!(
            native_edge_tile_site(ForecastMapSite {
                latitude: 47.950429954185445,
                longitude: -122.42797012608193,
            }),
            Some(AdmittedXweatherMapSite {
                latitude_millionths: 47_950_430,
                longitude_millionths: -122_427_970,
            })
        );
    }

    /// Verify source-compatible persisted fields and integral JSON numbers.
    #[test]
    fn decodes_usage_ledger_shape() {
        let ledger = decode_usage_ledger(
            br#"{"day":"2026-11-24","dayUnits":2.0,"month":"2026-11","monthUnits":9,"extra":true}"#,
        )
        .unwrap();
        assert_eq!(ledger.calendar().day(), "2026-11-24");
        assert_eq!(ledger.calendar().month(), "2026-11");
        assert_eq!(ledger.day_units(), 2);
        assert_eq!(ledger.month_units(), 9);
    }

    /// Verify exact ceil behavior at a UTC day boundary.
    #[test]
    fn reports_seconds_until_next_utc_day() {
        let timestamp = "2026-11-24T23:59:59.001Z".parse::<Timestamp>().unwrap();
        assert_eq!(seconds_until_utc_day(timestamp.as_millisecond()), "1");
    }
}
