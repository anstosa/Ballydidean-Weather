use std::convert::Infallible;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use weather_edge::{
    AdmittedXweatherMapSite, AdmittedXweatherProvider, AdmittedXweatherUsageCalendar,
    AdmittedXweatherUsageLedger, AdmittedXweatherValidTime, DecodedXweatherFrameRoute,
    DecodedXweatherMapRoute, XweatherBudgetedTileLoader, XweatherMapProxyBody,
    XweatherMapProxyMethod, XweatherMapProxyRequest, XweatherMapProxyResponse,
    XweatherTileCacheKey, XweatherTileCacheRequest, XweatherTileCacheStatus, XweatherTileClock,
    XweatherTileLayer, XweatherTileLoad, XweatherTileLoader, XweatherTileMemoryCache,
    XweatherUsageBudget, XweatherUsageCalendarSource, XweatherUsagePersistence,
    handle_xweather_map_proxy,
};

const NOW: i64 = 1_787_918_400_000;
const FORECAST_EPOCH: i64 = 1_787_922_000_000;
const HISTORY_EPOCH: i64 = 1_787_917_800_000;
const INVALID_TIME_EPOCH: i64 = 1_788_102_000_000;
const SITE: AdmittedXweatherMapSite = AdmittedXweatherMapSite {
    latitude_millionths: 47_950_000,
    longitude_millionths: -122_430_000,
};

#[derive(Debug, Clone)]
struct FixtureCalendar;

impl XweatherUsageCalendarSource for FixtureCalendar {
    /// return the fixed admitted request calendar.
    fn current_calendar(&self) -> AdmittedXweatherUsageCalendar {
        AdmittedXweatherUsageCalendar::admitted("2026-08-28", "2026-08")
    }
}

#[derive(Debug, Clone, Default)]
struct FixturePersistence {
    state: Arc<Mutex<Option<AdmittedXweatherUsageLedger>>>,
}

impl XweatherUsagePersistence for FixturePersistence {
    type Error = Infallible;

    /// load the current optional admitted fixture ledger.
    fn load(&mut self) -> Result<Option<AdmittedXweatherUsageLedger>, Self::Error> {
        Ok(self.state.lock().expect("persistence lock").clone())
    }

    /// atomically retain one complete admitted fixture ledger.
    fn persist(&mut self, state: &AdmittedXweatherUsageLedger) -> Result<(), Self::Error> {
        *self.state.lock().expect("persistence lock") = Some(state.clone());
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureProviderError;

impl Display for FixtureProviderError {
    /// render one injected upstream failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("provider failed")
    }
}

impl Error for FixtureProviderError {}

#[derive(Debug, Clone)]
struct FixtureProvider {
    calls: Arc<AtomicUsize>,
}

impl FixtureProvider {
    /// return the observed fake provider call count.
    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl XweatherTileLoader for FixtureProvider {
    type Error = FixtureProviderError;

    /// return one typed fake image or the source-derived upstream failure.
    fn load_tile(&self, request: &XweatherTileCacheRequest) -> Result<Vec<u8>, Self::Error> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let XweatherTileLoad::Frame(frame) = &request.tile else {
            panic!("expected frame descriptor");
        };

        // inject the source oracle's ordinary failed wind request
        if frame.layer == XweatherTileLayer::Wind {
            return Err(FixtureProviderError);
        }
        let phase = match request.phase {
            weather_edge::XweatherTilePhase::History => "history",
            weather_edge::XweatherTilePhase::Forecast => "forecast",
        };
        let layer = match frame.layer {
            XweatherTileLayer::Clouds => "clouds",
            XweatherTileLayer::Precipitation => "precipitation",
            XweatherTileLayer::Radar => "radar",
            XweatherTileLayer::Wind => "wind",
        };
        Ok(format!("{phase}-{layer}").into_bytes())
    }
}

#[derive(Debug, Clone)]
struct FixtureClock {
    now: Arc<AtomicI64>,
}

impl FixtureClock {
    /// create one fixed application clock.
    fn fixed(now: i64) -> Self {
        Self {
            now: Arc::new(AtomicI64::new(now)),
        }
    }

    /// advance to one admitted request instant.
    fn set(&self, now: i64) {
        self.now.store(now, Ordering::Relaxed);
    }
}

impl XweatherTileClock for FixtureClock {
    /// return the current cache fixture instant.
    fn now_epoch_ms(&self) -> i64 {
        self.now.load(Ordering::Relaxed)
    }
}

/// build one URL-matched source fixture route.
fn frame_route(
    cache_key: &str,
    phase: &str,
    layer: &str,
    wire_time: &str,
    epoch_ms: i64,
    minute: u8,
) -> DecodedXweatherFrameRoute {
    DecodedXweatherFrameRoute {
        cache_key: XweatherTileCacheKey::admitted(cache_key),
        height: 168,
        latitude: 47.95,
        latitude_millionths: 47_950_000,
        layer: layer.to_owned(),
        longitude: -122.43,
        longitude_millionths: -122_430_000,
        phase: phase.to_owned(),
        valid_time: Some(AdmittedXweatherValidTime::admitted(
            wire_time, epoch_ms, minute, 0,
        )),
        width: 256,
        zoom: 10,
    }
}

/// build one decoded application request.
fn request(
    method: XweatherMapProxyMethod,
    route: DecodedXweatherMapRoute,
    provider: bool,
    now_epoch_ms: i64,
    retry_after: &str,
) -> XweatherMapProxyRequest {
    XweatherMapProxyRequest {
        method,
        now_epoch_ms,
        provider: provider.then_some(AdmittedXweatherProvider::admitted()),
        retry_after_utc_day_seconds: retry_after.to_owned(),
        route,
        site: SITE,
    }
}

/// render one response like the compiled Node protocol adapter oracle.
fn response_signature(response: &XweatherMapProxyResponse) -> String {
    let body = match &response.body {
        XweatherMapProxyBody::Text(body) => String::from_utf8_lossy(body).trim_end().to_owned(),
        XweatherMapProxyBody::Png(None) => "-".to_owned(),
        XweatherMapProxyBody::Png(Some(body)) => {
            String::from_utf8_lossy(body.as_bytes()).to_string()
        }
    };
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        response.status_code,
        response.headers.cache_control,
        response.headers.content_type,
        response.headers.content_length,
        response.headers.allow.unwrap_or("-"),
        response.headers.retry_after.as_deref().unwrap_or("-"),
        response
            .headers
            .tile_age_seconds
            .map(|age| age.to_string())
            .unwrap_or_else(|| "-".to_owned()),
        response
            .headers
            .tile_cache
            .map(XweatherTileCacheStatus::wire_value)
            .unwrap_or("-"),
        body
    )
}

// verifies actual source parse and proxy branches over the Rust cache and budget path
#[test]
fn matches_compiled_node_xweather_map_proxy_application_path() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/xweather-map-proxy-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run Xweather map proxy Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");

    let budget = Arc::new(
        XweatherUsageBudget::new(3, 5, FixtureCalendar, FixturePersistence::default())
            .expect("valid fixture budget"),
    );
    let provider = FixtureProvider {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let clock = FixtureClock::fixed(NOW);
    let cache = XweatherTileMemoryCache::new(
        XweatherBudgetedTileLoader::new(Arc::clone(&budget), provider.clone()),
        clock.clone(),
        1_024,
        60 * 60 * 1_000,
    )
    .expect("valid fixture cache");
    let forecast_radar = frame_route(
        "forecast-radar",
        "forecast",
        "radar",
        "20260828130000",
        FORECAST_EPOCH,
        0,
    );
    let history_radar = frame_route(
        "history-radar",
        "history",
        "radar",
        "20260828115000",
        HISTORY_EPOCH,
        50,
    );
    let forecast_wind = frame_route(
        "forecast-wind",
        "forecast",
        "wind",
        "20260828130000",
        FORECAST_EPOCH,
        0,
    );
    let forecast_clouds = frame_route(
        "forecast-clouds",
        "forecast",
        "clouds",
        "20260828130000",
        FORECAST_EPOCH,
        0,
    );
    let unsupported = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Unsupported,
            DecodedXweatherMapRoute::Frame(forecast_radar.clone()),
            false,
            NOW,
            "43200",
        ),
        &cache,
    );
    let missing = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Get,
            DecodedXweatherMapRoute::Missing,
            false,
            NOW,
            "43200",
        ),
        &cache,
    );
    let mut invalid_geometry_route = forecast_radar.clone();
    invalid_geometry_route.width = 257;
    let invalid_geometry = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Get,
            DecodedXweatherMapRoute::Frame(invalid_geometry_route),
            false,
            NOW,
            "43200",
        ),
        &cache,
    );
    let mut invalid_center_route = forecast_radar.clone();
    invalid_center_route.latitude = 47.96;
    invalid_center_route.latitude_millionths = 47_960_000;
    let invalid_center = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Get,
            DecodedXweatherMapRoute::Frame(invalid_center_route),
            false,
            NOW,
            "43200",
        ),
        &cache,
    );
    let invalid_time_route = frame_route(
        "invalid-time",
        "forecast",
        "radar",
        "20260830150000",
        INVALID_TIME_EPOCH,
        0,
    );
    let invalid_time = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Get,
            DecodedXweatherMapRoute::Frame(invalid_time_route),
            false,
            NOW,
            "43200",
        ),
        &cache,
    );
    let unavailable = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Get,
            DecodedXweatherMapRoute::Frame(forecast_radar.clone()),
            false,
            NOW,
            "43200",
        ),
        &cache,
    );
    let forecast_miss = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Get,
            DecodedXweatherMapRoute::Frame(forecast_radar.clone()),
            true,
            NOW,
            "43200",
        ),
        &cache,
    );
    clock.set(NOW + 2_000);
    let forecast_head = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Head,
            DecodedXweatherMapRoute::Frame(forecast_radar.clone()),
            true,
            NOW + 2_000,
            "43198",
        ),
        &cache,
    );
    let history_miss = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Get,
            DecodedXweatherMapRoute::Frame(history_radar.clone()),
            true,
            NOW + 2_000,
            "43198",
        ),
        &cache,
    );
    clock.set(NOW + 5_000);
    let history_hit = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Get,
            DecodedXweatherMapRoute::Frame(history_radar),
            true,
            NOW + 5_000,
            "43195",
        ),
        &cache,
    );
    let upstream_failure = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Get,
            DecodedXweatherMapRoute::Frame(forecast_wind),
            true,
            NOW + 5_000,
            "43195",
        ),
        &cache,
    );
    let exhausted = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Get,
            DecodedXweatherMapRoute::Frame(forecast_clouds),
            true,
            NOW + 5_000,
            "43195",
        ),
        &cache,
    );
    clock.set(NOW + 6_000);
    let cached_after_exhaustion = handle_xweather_map_proxy(
        request(
            XweatherMapProxyMethod::Get,
            DecodedXweatherMapRoute::Frame(forecast_radar),
            true,
            NOW + 6_000,
            "43194",
        ),
        &cache,
    );
    let snapshot = budget.snapshot().expect("budget snapshot");
    let rust = [
        format!("unsupported={}", response_signature(&unsupported)),
        format!("missing={}", response_signature(&missing)),
        format!("invalid-geometry={}", response_signature(&invalid_geometry)),
        format!("invalid-center={}", response_signature(&invalid_center)),
        format!("invalid-time={}", response_signature(&invalid_time)),
        format!("unavailable={}", response_signature(&unavailable)),
        format!("forecast-miss={}", response_signature(&forecast_miss)),
        format!("forecast-head={}", response_signature(&forecast_head)),
        format!("history-miss={}", response_signature(&history_miss)),
        format!("history-hit={}", response_signature(&history_hit)),
        format!("upstream-failure={}", response_signature(&upstream_failure)),
        format!("exhausted={}", response_signature(&exhausted)),
        format!(
            "cached-after-exhaustion={}",
            response_signature(&cached_after_exhaustion)
        ),
        format!(
            "usage={}/{}:provider-calls={}",
            snapshot.day_units,
            snapshot.month_units,
            provider.calls()
        ),
    ]
    .join("\n");
    assert_eq!(rust, node.trim_end());
}
