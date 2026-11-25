use crate::{
    XweatherBudgetedTileLoadError, XweatherMapUnits, XweatherTileBody, XweatherTileCacheGetError,
    XweatherTileCacheRequest, XweatherTileCacheResponse, XweatherTileCacheStatus,
    XweatherTileFrame, XweatherTileLayer, XweatherTileLoad, XweatherTileLoader,
    XweatherTileMemoryCache, XweatherTilePhase,
};

const TEN_MINUTES_MS: i128 = 10 * 60 * 1_000;
const HOUR_MS: i128 = 60 * 60 * 1_000;
const TWENTY_SIX_HOURS_MS: i128 = 26 * HOUR_MS;

/// one site coordinate identity admitted at the six-decimal source boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedXweatherMapSite {
    pub latitude_millionths: i32,
    pub longitude_millionths: i32,
}

/// one canonical Xweather valid time admitted by the owning time parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedXweatherValidTime {
    epoch_ms: i64,
    minute: u8,
    second: u8,
    wire_value: String,
}

impl AdmittedXweatherValidTime {
    /// admit canonical time facts without implementing a calendar codec here.
    pub fn admitted(wire_value: impl Into<String>, epoch_ms: i64, minute: u8, second: u8) -> Self {
        Self {
            epoch_ms,
            minute,
            second,
            wire_value: wire_value.into(),
        }
    }
}

/// one URL-matched static-frame route after numeric and calendar decoding.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedXweatherFrameRoute {
    pub cache_key: crate::XweatherTileCacheKey,
    pub height: u16,
    pub latitude: f64,
    pub latitude_millionths: i32,
    pub layer: String,
    pub longitude: f64,
    pub longitude_millionths: i32,
    pub phase: String,
    pub valid_time: Option<AdmittedXweatherValidTime>,
    pub width: u16,
    pub zoom: u8,
}

/// one decoded public map route without URL parsing in this policy.
#[derive(Debug, Clone, PartialEq)]
pub enum DecodedXweatherMapRoute {
    Missing,
    Frame(DecodedXweatherFrameRoute),
}

/// one admitted provider-availability decision from the credential boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedXweatherProvider(());

impl AdmittedXweatherProvider {
    /// admit provider availability without handling credentials here.
    pub const fn admitted() -> Self {
        Self(())
    }
}

/// the decoded read-only method selected by the request adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XweatherMapProxyMethod {
    Get,
    Head,
    Unsupported,
}

/// one application request after method, route, time, and provider admission.
#[derive(Debug, Clone, PartialEq)]
pub struct XweatherMapProxyRequest {
    pub method: XweatherMapProxyMethod,
    pub now_epoch_ms: i64,
    pub provider: Option<AdmittedXweatherProvider>,
    pub retry_after_utc_day_seconds: String,
    pub route: DecodedXweatherMapRoute,
    pub site: AdmittedXweatherMapSite,
}

/// one response body retained for the owning protocol adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XweatherMapProxyBody {
    Text(&'static [u8]),
    Png(Option<XweatherTileBody>),
}

/// exact ordinary response headers selected by the source proxy policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XweatherMapProxyHeaders {
    pub allow: Option<&'static str>,
    pub cache_control: &'static str,
    pub content_length: usize,
    pub content_type: &'static str,
    pub retry_after: Option<String>,
    pub tile_age_seconds: Option<u64>,
    pub tile_cache: Option<XweatherTileCacheStatus>,
}

/// one typed source-compatible map proxy response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XweatherMapProxyResponse {
    pub body: XweatherMapProxyBody,
    pub headers: XweatherMapProxyHeaders,
    pub status_code: u16,
}

/// cache lookup retained outside application response policy.
pub trait XweatherMapTileCache {
    type Error: XweatherMapTileCacheError;

    /// return one cached or newly loaded tile for an admitted request.
    fn get_tile(
        &self,
        request: &XweatherTileCacheRequest,
    ) -> Result<XweatherTileCacheResponse, Self::Error>;
}

/// the single cache failure category that changes the public response.
pub trait XweatherMapTileCacheError {
    /// report whether the owning budget rejected the provider attempt.
    fn is_budget_exhausted(&self) -> bool;
}

impl<LoaderError> XweatherMapTileCacheError for XweatherTileCacheGetError<LoaderError>
where
    LoaderError: XweatherMapTileCacheError,
{
    /// preserve only the budget exhaustion branch from the wrapped loader error.
    fn is_budget_exhausted(&self) -> bool {
        matches!(self, Self::Loader(error) if error.is_budget_exhausted())
    }
}

impl<Loader, Clock> XweatherMapTileCache for XweatherTileMemoryCache<Loader, Clock>
where
    Loader: XweatherTileLoader,
    Loader::Error: Clone + XweatherMapTileCacheError,
    Clock: crate::XweatherTileClock,
{
    type Error = XweatherTileCacheGetError<Loader::Error>;

    /// delegate one admitted lookup to the bounded memory cache.
    fn get_tile(
        &self,
        request: &XweatherTileCacheRequest,
    ) -> Result<XweatherTileCacheResponse, Self::Error> {
        self.get(request)
    }
}

impl<PersistenceError, LoaderError> XweatherMapTileCacheError
    for XweatherBudgetedTileLoadError<PersistenceError, LoaderError>
{
    /// report only the owning budget exhaustion category.
    fn is_budget_exhausted(&self) -> bool {
        XweatherBudgetedTileLoadError::is_budget_exhausted(self)
    }
}

/// execute one decoded map request over the real bounded cache path.
pub fn handle_xweather_map_proxy<Cache>(
    request: XweatherMapProxyRequest,
    cache: &Cache,
) -> XweatherMapProxyResponse
where
    Cache: XweatherMapTileCache,
{
    // keep the source map edge read-only before route or provider work
    if request.method == XweatherMapProxyMethod::Unsupported {
        return text_response(405, b"method not allowed\n", Some("GET, HEAD"), None);
    }
    let frame = match request.route {
        DecodedXweatherMapRoute::Missing => {
            return text_response(404, b"not found\n", None, None);
        }
        DecodedXweatherMapRoute::Frame(frame) => frame,
    };
    let Some(tile_request) =
        validate_xweather_frame_route(request.site, request.now_epoch_ms, frame)
    else {
        return text_response(404, b"not found\n", None, None);
    };

    // stop before cache or provider work without admitted availability
    if request.provider.is_none() {
        return text_response(503, b"weather map unavailable\n", None, Some("300"));
    }
    let phase = tile_request.phase;

    // map cache, budget, and provider outcomes into the source response contract
    match cache.get_tile(&tile_request) {
        Ok(cached) => image_response(request.method, phase, request.now_epoch_ms, cached),
        Err(error) if error.is_budget_exhausted() => text_response(
            429,
            b"weather map budget exhausted\n",
            None,
            Some(&request.retry_after_utc_day_seconds),
        ),
        Err(_) => text_response(502, b"weather tile unavailable\n", None, None),
    }
}

/// validate one URL-matched frame against the exact source bounds.
fn validate_xweather_frame_route(
    site: AdmittedXweatherMapSite,
    now_epoch_ms: i64,
    frame: DecodedXweatherFrameRoute,
) -> Option<XweatherTileCacheRequest> {
    let phase = match frame.phase.as_str() {
        "history" => XweatherTilePhase::History,
        "forecast" => XweatherTilePhase::Forecast,
        _ => return None,
    };
    let layer = match frame.layer.as_str() {
        "radar" => XweatherTileLayer::Radar,
        "clouds" => XweatherTileLayer::Clouds,
        "precipitation" => XweatherTileLayer::Precipitation,
        "wind" => XweatherTileLayer::Wind,
        _ => return None,
    };
    let valid_time = frame.valid_time?;

    // constrain the single reviewed static-map geometry
    if frame.zoom != 10
        || frame.width != 256
        || frame.height != 168
        || frame.latitude_millionths != site.latitude_millionths
        || frame.longitude_millionths != site.longitude_millionths
        || valid_time.second != 0
    {
        return None;
    }
    let now_epoch_ms = i128::from(now_epoch_ms);
    let valid_epoch_ms = i128::from(valid_time.epoch_ms);

    // preserve phase-specific minute grids and rolling time windows
    match phase {
        XweatherTilePhase::History => {
            let age_ms = now_epoch_ms - valid_epoch_ms;
            if valid_time.minute % 10 != 0
                || !(-TEN_MINUTES_MS..=TWENTY_SIX_HOURS_MS).contains(&age_ms)
            {
                return None;
            }
        }
        XweatherTilePhase::Forecast => {
            let future_ms = valid_epoch_ms - now_epoch_ms;
            if valid_time.minute != 0 || !(-HOUR_MS..=TWENTY_SIX_HOURS_MS).contains(&future_ms) {
                return None;
            }
        }
    }
    let provider_units = XweatherMapUnits::admitted(1).expect("static frame map unit invariant");
    Some(XweatherTileCacheRequest {
        key: frame.cache_key,
        phase,
        provider_units,
        tile: XweatherTileLoad::Frame(XweatherTileFrame {
            height: frame.height,
            latitude: frame.latitude,
            layer,
            longitude: frame.longitude,
            valid_time: valid_time.wire_value,
            width: frame.width,
            zoom: frame.zoom,
        }),
    })
}

/// build one exact source text response without protocol I/O.
fn text_response(
    status_code: u16,
    body: &'static [u8],
    allow: Option<&'static str>,
    retry_after: Option<&str>,
) -> XweatherMapProxyResponse {
    XweatherMapProxyResponse {
        body: XweatherMapProxyBody::Text(body),
        headers: XweatherMapProxyHeaders {
            allow,
            cache_control: "no-store",
            content_length: body.len(),
            content_type: "text/plain; charset=utf-8",
            retry_after: retry_after.map(str::to_owned),
            tile_age_seconds: None,
            tile_cache: None,
        },
        status_code,
    }
}

/// build one exact successful image response without protocol I/O.
fn image_response(
    method: XweatherMapProxyMethod,
    phase: XweatherTilePhase,
    now_epoch_ms: i64,
    cached: XweatherTileCacheResponse,
) -> XweatherMapProxyResponse {
    let elapsed_ms = i128::from(now_epoch_ms) - i128::from(cached.fetched_at_epoch_ms);
    let tile_age_seconds =
        u64::try_from(elapsed_ms.max(0) / 1_000).expect("bounded epoch difference in seconds");
    let content_length = cached.body.len();
    let body = if method == XweatherMapProxyMethod::Head {
        None
    } else {
        Some(cached.body)
    };
    XweatherMapProxyResponse {
        body: XweatherMapProxyBody::Png(body),
        headers: XweatherMapProxyHeaders {
            allow: None,
            cache_control: match phase {
                XweatherTilePhase::History => "public, max-age=31536000, immutable",
                XweatherTilePhase::Forecast => "no-store",
            },
            content_length,
            content_type: "image/png",
            retry_after: None,
            tile_age_seconds: Some(tile_age_seconds),
            tile_cache: Some(cached.cache_status),
        },
        status_code: 200,
    }
}
