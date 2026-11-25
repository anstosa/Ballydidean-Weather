use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use weather_edge::{
    XweatherMapUnits, XweatherTileCacheKey, XweatherTileCacheRequest, XweatherTileCacheResponse,
    XweatherTileCacheStats, XweatherTileClock, XweatherTileLoad, XweatherTileLoader,
    XweatherTileMemoryCache, XweatherTilePhase,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureLoaderError(&'static str);

impl Display for FixtureLoaderError {
    /// render one injected provider failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureLoaderError {}

#[derive(Debug)]
struct DeferredBody {
    released: Mutex<Option<Vec<u8>>>,
    ready: Condvar,
    started: Mutex<bool>,
    started_signal: Condvar,
}

impl DeferredBody {
    /// create one blocked provider response.
    fn pending() -> Self {
        Self {
            released: Mutex::new(None),
            ready: Condvar::new(),
            started: Mutex::new(false),
            started_signal: Condvar::new(),
        }
    }

    /// mark the provider call active and wait for its admitted bytes.
    fn load(&self) -> Vec<u8> {
        {
            let mut started = self.started.lock().expect("deferred started lock");
            *started = true;
            self.started_signal.notify_all();
        }
        let mut released = self.released.lock().expect("deferred body lock");

        // retain the provider call until the fixture releases its bytes
        while released.is_none() {
            released = self.ready.wait(released).expect("deferred body lock");
        }

        released.take().expect("released deferred body")
    }

    /// wait until the provider seam has begun its blocked load.
    fn wait_until_started(&self) {
        let mut started = self.started.lock().expect("deferred started lock");

        // wait only until the provider call reports its start
        while !*started {
            started = self
                .started_signal
                .wait(started)
                .expect("deferred started lock");
        }
    }

    /// release one typed provider body to every waiting fixture thread.
    fn release(&self, body: impl Into<Vec<u8>>) {
        let mut released = self.released.lock().expect("deferred body lock");
        *released = Some(body.into());
        self.ready.notify_all();
    }
}

#[derive(Debug)]
enum FixtureLoadOutcome {
    Body(Vec<u8>),
    Error(FixtureLoaderError),
    Deferred(Arc<DeferredBody>),
}

#[derive(Debug, Default)]
struct FixtureLoaderState {
    calls: HashMap<String, usize>,
    sequences: HashMap<String, VecDeque<FixtureLoadOutcome>>,
}

#[derive(Debug, Clone, Default)]
struct FixtureLoader {
    state: Arc<Mutex<FixtureLoaderState>>,
}

impl FixtureLoader {
    /// create one deterministic provider outcome registry.
    fn from_sequences(
        sequences: impl IntoIterator<Item = (&'static str, Vec<FixtureLoadOutcome>)>,
    ) -> Self {
        let sequences = sequences
            .into_iter()
            .map(|(key, outcomes)| (key.to_owned(), outcomes.into()))
            .collect();
        Self {
            state: Arc::new(Mutex::new(FixtureLoaderState {
                calls: HashMap::new(),
                sequences,
            })),
        }
    }

    /// return the observed provider call count for one key.
    fn calls(&self, key: &str) -> usize {
        self.state
            .lock()
            .expect("fixture loader lock")
            .calls
            .get(key)
            .copied()
            .unwrap_or_default()
    }
}

impl XweatherTileLoader for FixtureLoader {
    type Error = FixtureLoaderError;

    /// return the next injected body, failure, or blocked provider response.
    fn load_tile(&self, request: &XweatherTileCacheRequest) -> Result<Vec<u8>, Self::Error> {
        let outcome = {
            let mut state = self.state.lock().expect("fixture loader lock");
            *state
                .calls
                .entry(request.key.as_str().to_owned())
                .or_default() += 1;
            state
                .sequences
                .get_mut(request.key.as_str())
                .and_then(VecDeque::pop_front)
                .expect("fixture provider outcome")
        };

        // preserve the selected provider outcome category
        match outcome {
            FixtureLoadOutcome::Body(body) => Ok(body),
            FixtureLoadOutcome::Error(error) => Err(error),
            FixtureLoadOutcome::Deferred(deferred) => Ok(deferred.load()),
        }
    }
}

#[derive(Debug, Clone)]
struct FixtureClock {
    now: Arc<AtomicI64>,
}

impl FixtureClock {
    /// create one mutable admitted epoch source.
    fn fixed(now: i64) -> Self {
        Self {
            now: Arc::new(AtomicI64::new(now)),
        }
    }

    /// move the admitted clock to one exact fixture instant.
    fn set(&self, now: i64) {
        self.now.store(now, Ordering::Relaxed);
    }
}

impl XweatherTileClock for FixtureClock {
    /// return the current fixed fixture instant.
    fn now_epoch_ms(&self) -> i64 {
        self.now.load(Ordering::Relaxed)
    }
}

/// build one parsed request after URL and geometry admission.
fn request(key: &str, phase: XweatherTilePhase) -> XweatherTileCacheRequest {
    XweatherTileCacheRequest {
        key: XweatherTileCacheKey::admitted(key),
        phase,
        provider_units: XweatherMapUnits::admitted(1).expect("fixture map unit"),
        tile: XweatherTileLoad::Opaque,
    }
}

/// build one bounded fixture cache.
fn cache(
    loader: FixtureLoader,
    clock: FixtureClock,
    maximum_bytes: u64,
    freshness_ms: i64,
) -> XweatherTileMemoryCache<FixtureLoader, FixtureClock> {
    XweatherTileMemoryCache::new(loader, clock, maximum_bytes, freshness_ms)
        .expect("valid fixture cache")
}

/// render one returned cache entry like the Node oracle.
fn response_signature(response: &XweatherTileCacheResponse) -> String {
    format!(
        "{}/{}/{}",
        response.cache_status.wire_value(),
        response.fetched_at_epoch_ms,
        String::from_utf8_lossy(response.body.as_bytes())
    )
}

/// render bounded counters in source field order.
fn stats_signature(stats: XweatherTileCacheStats) -> String {
    format!(
        "{}/{}/{}/{}",
        stats.entries, stats.inflight, stats.maximum_bytes, stats.stored_bytes
    )
}

// verifies exact source hit, expiry, eviction, bounds, error, and coalescing behavior
#[test]
fn matches_node_xweather_tile_memory_cache_policy() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/xweather-tile-cache-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run Xweather tile cache Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");

    let clock = FixtureClock::fixed(1_000);
    let basic_loader = FixtureLoader::from_sequences([
        (
            "forecast-basic",
            vec![
                FixtureLoadOutcome::Body(b"aa".to_vec()),
                FixtureLoadOutcome::Body(b"bbb".to_vec()),
            ],
        ),
        (
            "history-basic",
            vec![FixtureLoadOutcome::Body(b"hh".to_vec())],
        ),
    ]);
    let basic = cache(basic_loader.clone(), clock.clone(), 16, 100);
    let forecast_request = request("forecast-basic", XweatherTilePhase::Forecast);
    let forecast_miss = basic.get(&forecast_request).expect("forecast miss");
    clock.set(1_099);
    let forecast_hit = basic.get(&forecast_request).expect("forecast hit");
    clock.set(1_100);
    let forecast_refresh = basic.get(&forecast_request).expect("forecast refresh");
    let history_request = request("history-basic", XweatherTilePhase::History);
    let history_miss = basic.get(&history_request).expect("history miss");
    clock.set(99_999);
    let history_hit = basic.get(&history_request).expect("history hit");

    let priority_loader = FixtureLoader::from_sequences([
        ("history-1", vec![FixtureLoadOutcome::Body(b"h1".to_vec())]),
        (
            "forecast-1",
            vec![
                FixtureLoadOutcome::Body(b"f1".to_vec()),
                FixtureLoadOutcome::Body(b"F1".to_vec()),
                FixtureLoadOutcome::Body(b"R1".to_vec()),
            ],
        ),
        ("history-2", vec![FixtureLoadOutcome::Body(b"h2".to_vec())]),
    ]);
    let priority = cache(priority_loader.clone(), clock.clone(), 4, 100);
    let priority_history_one_request = request("history-1", XweatherTilePhase::History);
    let priority_forecast_one_request = request("forecast-1", XweatherTilePhase::Forecast);
    let priority_history_two_request = request("history-2", XweatherTilePhase::History);
    let priority_history_one = priority
        .get(&priority_history_one_request)
        .expect("priority history one");
    let priority_forecast_one = priority
        .get(&priority_forecast_one_request)
        .expect("priority forecast one");
    let priority_history_two = priority
        .get(&priority_history_two_request)
        .expect("priority history two");
    let priority_after_history = stats_signature(priority.stats());
    let priority_history_hit = priority
        .get(&priority_history_one_request)
        .expect("priority history hit");
    let priority_forecast_miss = priority
        .get(&priority_forecast_one_request)
        .expect("priority forecast miss");
    let priority_forecast_repeated = priority
        .get(&priority_forecast_one_request)
        .expect("priority forecast repeated miss");

    let oversize_loader = FixtureLoader::from_sequences([(
        "oversize",
        vec![
            FixtureLoadOutcome::Body(b"wide".to_vec()),
            FixtureLoadOutcome::Body(b"WIDE".to_vec()),
        ],
    )]);
    let oversize = cache(oversize_loader.clone(), clock.clone(), 3, 100);
    let oversize_request = request("oversize", XweatherTilePhase::Forecast);
    let oversize_first = oversize.get(&oversize_request).expect("oversize first");
    let oversize_second = oversize.get(&oversize_request).expect("oversize second");

    clock.set(10_000);
    let refresh_loader = FixtureLoader::from_sequences([(
        "refresh",
        vec![
            FixtureLoadOutcome::Body(b"ok".to_vec()),
            FixtureLoadOutcome::Body(b"wide".to_vec()),
        ],
    )]);
    let refresh = cache(refresh_loader, clock.clone(), 3, 10);
    let refresh_request = request("refresh", XweatherTilePhase::Forecast);
    let refresh_first = refresh.get(&refresh_request).expect("refresh first");
    clock.set(10_010);
    let refresh_oversize = refresh
        .get(&refresh_request)
        .expect("refresh oversize response");

    let invalid_loader = FixtureLoader::from_sequences([(
        "invalid",
        vec![
            FixtureLoadOutcome::Body(Vec::new()),
            FixtureLoadOutcome::Body(b"ok".to_vec()),
        ],
    )]);
    let invalid = cache(invalid_loader, clock.clone(), 4, 100);
    let invalid_request = request("invalid", XweatherTilePhase::Forecast);
    let invalid_error = invalid
        .get(&invalid_request)
        .expect_err("reject empty provider body")
        .to_string();
    let invalid_retry = invalid.get(&invalid_request).expect("invalid retry");

    let failure_loader = FixtureLoader::from_sequences([(
        "failure",
        vec![
            FixtureLoadOutcome::Error(FixtureLoaderError("provider failed")),
            FixtureLoadOutcome::Body(b"ok".to_vec()),
        ],
    )]);
    let failure = cache(failure_loader, clock.clone(), 4, 100);
    let failure_request = request("failure", XweatherTilePhase::Forecast);
    let provider_error = failure
        .get(&failure_request)
        .expect_err("preserve provider failure")
        .to_string();
    let provider_retry = failure.get(&failure_request).expect("provider retry");

    let deferred = Arc::new(DeferredBody::pending());
    let coalesced_loader = FixtureLoader::from_sequences([(
        "coalesced",
        vec![FixtureLoadOutcome::Deferred(Arc::clone(&deferred))],
    )]);
    let coalesced = Arc::new(cache(coalesced_loader.clone(), clock.clone(), 4, 100));
    let coalesced_request = request("coalesced", XweatherTilePhase::Forecast);
    let first_cache = Arc::clone(&coalesced);
    let first_request = coalesced_request.clone();
    let first = thread::spawn(move || first_cache.get(&first_request));
    deferred.wait_until_started();
    let second_cache = Arc::clone(&coalesced);
    let second_request = coalesced_request.clone();
    let second = thread::spawn(move || second_cache.get(&second_request));
    thread::sleep(Duration::from_millis(20));
    let coalesced_pending_stats = stats_signature(coalesced.stats());
    deferred.release(b"cc".to_vec());
    let coalesced_first = first
        .join()
        .expect("first coalesced thread")
        .expect("first coalesced response");
    let coalesced_second = second
        .join()
        .expect("second coalesced thread")
        .expect("second coalesced response");

    let invalid_configuration = XweatherTileMemoryCache::new(basic_loader.clone(), clock, 0, 100)
        .expect_err("reject zero byte bound")
        .to_string();

    let rust = [
        format!(
            "forecast={},{},{}:calls={}:{}",
            response_signature(&forecast_miss),
            response_signature(&forecast_hit),
            response_signature(&forecast_refresh),
            basic_loader.calls("forecast-basic"),
            stats_signature(basic.stats())
        ),
        format!(
            "history={},{}:calls={}",
            response_signature(&history_miss),
            response_signature(&history_hit),
            basic_loader.calls("history-basic")
        ),
        format!(
            "priority={},{},{}:{}:{},{},{}:calls={}:{}",
            response_signature(&priority_history_one),
            response_signature(&priority_forecast_one),
            response_signature(&priority_history_two),
            priority_after_history,
            response_signature(&priority_history_hit),
            response_signature(&priority_forecast_miss),
            response_signature(&priority_forecast_repeated),
            priority_loader.calls("forecast-1"),
            stats_signature(priority.stats())
        ),
        format!(
            "oversize={},{}:calls={}:{}",
            response_signature(&oversize_first),
            response_signature(&oversize_second),
            oversize_loader.calls("oversize"),
            stats_signature(oversize.stats())
        ),
        format!(
            "refresh-oversize={},{}:{}",
            response_signature(&refresh_first),
            response_signature(&refresh_oversize),
            stats_signature(refresh.stats())
        ),
        format!(
            "invalid={invalid_error}:{}:{}",
            response_signature(&invalid_retry),
            stats_signature(invalid.stats())
        ),
        format!(
            "failure={provider_error}:{}:{}",
            response_signature(&provider_retry),
            stats_signature(failure.stats())
        ),
        format!(
            "coalesced={coalesced_pending_stats}:calls={}:{},{}:{}",
            coalesced_loader.calls("coalesced"),
            response_signature(&coalesced_first),
            response_signature(&coalesced_second),
            stats_signature(coalesced.stats())
        ),
        format!("configuration={invalid_configuration}"),
    ]
    .join("\n");
    assert_eq!(rust, node.trim_end());
}

// verifies the cache accepts the source maximum-safe numeric boundary only
#[test]
fn rejects_unsafe_xweather_cache_bounds() {
    let loader = FixtureLoader::default();
    let clock = FixtureClock::fixed(0);
    assert!(
        XweatherTileMemoryCache::new(
            loader.clone(),
            clock.clone(),
            9_007_199_254_740_991,
            9_007_199_254_740_991,
        )
        .is_ok()
    );
    assert!(XweatherTileMemoryCache::new(loader.clone(), clock.clone(), 1, 0).is_err());
    assert!(
        XweatherTileMemoryCache::new(loader.clone(), clock.clone(), 1, 9_007_199_254_740_992,)
            .is_err()
    );
    assert!(XweatherTileMemoryCache::new(loader, clock, 9_007_199_254_740_992, 1).is_err());
}
