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
    AdmittedXweatherBudgetRetryAfter, AdmittedXweatherUsageCalendar, AdmittedXweatherUsageLedger,
    XweatherBudgetedTileLoadError, XweatherBudgetedTileLoader, XweatherMapUnits,
    XweatherTileCacheGetError, XweatherTileCacheKey, XweatherTileCacheRequest,
    XweatherTileCacheResponse, XweatherTileCacheStatus, XweatherTileClock, XweatherTileLoad,
    XweatherTileLoader, XweatherTileMemoryCache, XweatherTilePhase, XweatherUsageBudget,
    XweatherUsageBudgetError, XweatherUsageCalendarSource, XweatherUsagePersistence,
    XweatherUsageSnapshot, xweather_budgeted_tile_failure_response,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixturePersistenceError;

impl Display for FixturePersistenceError {
    /// render one injected persistence failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("fixture persistence failed")
    }
}

impl Error for FixturePersistenceError {}

#[derive(Debug, Default)]
struct FixturePersistenceState {
    fail_next: bool,
    stored: Option<AdmittedXweatherUsageLedger>,
}

#[derive(Debug, Clone, Default)]
struct FixturePersistence {
    state: Arc<Mutex<FixturePersistenceState>>,
}

impl FixturePersistence {
    /// fail the next atomic persistence attempt.
    fn fail_next(&self) {
        self.state.lock().expect("persistence lock").fail_next = true;
    }
}

impl XweatherUsagePersistence for FixturePersistence {
    type Error = FixturePersistenceError;

    /// load the current optional admitted fixture counter.
    fn load(&mut self) -> Result<Option<AdmittedXweatherUsageLedger>, Self::Error> {
        Ok(self.state.lock().expect("persistence lock").stored.clone())
    }

    /// atomically retain one complete admitted fixture counter.
    fn persist(&mut self, state: &AdmittedXweatherUsageLedger) -> Result<(), Self::Error> {
        let mut fixture = self.state.lock().expect("persistence lock");

        // inject one ordinary atomic persistence failure
        if fixture.fail_next {
            fixture.fail_next = false;
            return Err(FixturePersistenceError);
        }

        fixture.stored = Some(state.clone());
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct FixtureCalendar {
    current: Arc<Mutex<AdmittedXweatherUsageCalendar>>,
}

impl FixtureCalendar {
    /// create one fixed admitted UTC calendar identity.
    fn fixed(day: &str, month: &str) -> Self {
        Self {
            current: Arc::new(Mutex::new(AdmittedXweatherUsageCalendar::admitted(
                day, month,
            ))),
        }
    }

    /// move to one new admitted UTC calendar identity.
    fn set(&self, day: &str, month: &str) {
        *self.current.lock().expect("calendar lock") =
            AdmittedXweatherUsageCalendar::admitted(day, month);
    }
}

impl XweatherUsageCalendarSource for FixtureCalendar {
    /// return the current admitted fixture calendar.
    fn current_calendar(&self) -> AdmittedXweatherUsageCalendar {
        self.current.lock().expect("calendar lock").clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureProviderError(&'static str);

impl Display for FixtureProviderError {
    /// render one injected provider failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureProviderError {}

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

    /// mark the provider active and wait for its fixture bytes.
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

    /// wait until the provider seam begins its blocked load.
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

    /// release one provider body to the waiting fixture call.
    fn release(&self, body: impl Into<Vec<u8>>) {
        let mut released = self.released.lock().expect("deferred body lock");
        *released = Some(body.into());
        self.ready.notify_all();
    }
}

#[derive(Debug)]
enum FixtureProviderOutcome {
    Body(Vec<u8>),
    Error(FixtureProviderError),
    Deferred(Arc<DeferredBody>),
}

#[derive(Debug, Default)]
struct FixtureProviderState {
    calls: usize,
    sequences: HashMap<String, VecDeque<FixtureProviderOutcome>>,
}

#[derive(Debug, Clone, Default)]
struct FixtureProviderLoader {
    state: Arc<Mutex<FixtureProviderState>>,
}

impl FixtureProviderLoader {
    /// create one deterministic provider outcome registry.
    fn from_sequences(
        sequences: impl IntoIterator<Item = (&'static str, Vec<FixtureProviderOutcome>)>,
    ) -> Self {
        let sequences = sequences
            .into_iter()
            .map(|(key, outcomes)| (key.to_owned(), outcomes.into()))
            .collect();
        Self {
            state: Arc::new(Mutex::new(FixtureProviderState {
                calls: 0,
                sequences,
            })),
        }
    }

    /// return the total observed provider call count.
    fn calls(&self) -> usize {
        self.state.lock().expect("provider lock").calls
    }
}

impl XweatherTileLoader for FixtureProviderLoader {
    type Error = FixtureProviderError;

    /// return the next injected provider outcome.
    fn load_tile(&self, request: &XweatherTileCacheRequest) -> Result<Vec<u8>, Self::Error> {
        let outcome = {
            let mut state = self.state.lock().expect("provider lock");
            state.calls += 1;
            state
                .sequences
                .get_mut(request.key.as_str())
                .and_then(VecDeque::pop_front)
                .expect("fixture provider outcome")
        };

        // preserve the selected provider outcome category
        match outcome {
            FixtureProviderOutcome::Body(body) => Ok(body),
            FixtureProviderOutcome::Error(error) => Err(error),
            FixtureProviderOutcome::Deferred(deferred) => Ok(deferred.load()),
        }
    }
}

#[derive(Debug, Clone)]
struct FixtureCacheClock {
    now: Arc<AtomicI64>,
}

impl FixtureCacheClock {
    /// create one fixed cache freshness instant.
    fn fixed(now: i64) -> Self {
        Self {
            now: Arc::new(AtomicI64::new(now)),
        }
    }

    /// move the cache freshness clock.
    fn set(&self, now: i64) {
        self.now.store(now, Ordering::Relaxed);
    }
}

impl XweatherTileClock for FixtureCacheClock {
    /// return the current cache fixture instant.
    fn now_epoch_ms(&self) -> i64 {
        self.now.load(Ordering::Relaxed)
    }
}

/// create one budget over a shared fixture persistence port.
fn budget(
    daily_limit: u64,
    monthly_limit: u64,
    calendar: FixtureCalendar,
    persistence: FixturePersistence,
) -> XweatherUsageBudget<FixtureCalendar, FixturePersistence> {
    XweatherUsageBudget::new(daily_limit, monthly_limit, calendar, persistence)
        .expect("valid fixture budget")
}

/// admit one exact fixture map-unit count.
fn units(value: u64) -> XweatherMapUnits {
    XweatherMapUnits::admitted(value).expect("valid fixture map units")
}

/// build one parsed cache request with admitted billable units.
fn request(key: &str, unit_count: u64) -> XweatherTileCacheRequest {
    XweatherTileCacheRequest {
        key: XweatherTileCacheKey::admitted(key),
        phase: XweatherTilePhase::Forecast,
        provider_units: units(unit_count),
        tile: XweatherTileLoad::Opaque,
    }
}

/// render one redacted usage snapshot like the Node oracle.
fn snapshot_signature(snapshot: &XweatherUsageSnapshot) -> String {
    format!(
        "{}/{}/{}/{}/{}/{}",
        snapshot.day,
        snapshot.day_units,
        snapshot.month,
        snapshot.month_units,
        snapshot.daily_limit,
        snapshot.monthly_limit
    )
}

/// render one budget failure and its optional source code.
fn budget_error_signature(error: &XweatherUsageBudgetError<FixturePersistenceError>) -> String {
    format!("{}:{}", error, error.code().unwrap_or("-"))
}

/// render one cache failure and its optional source budget code.
fn cache_error_signature(
    error: &XweatherTileCacheGetError<
        XweatherBudgetedTileLoadError<FixturePersistenceError, FixtureProviderError>,
    >,
) -> String {
    let code = match error {
        XweatherTileCacheGetError::Loader(error) => error.code(),
        XweatherTileCacheGetError::InvalidBody(_) => None,
    };
    format!("{}:{}", error, code.unwrap_or("-"))
}

/// render only the source-visible cache status.
fn status_signature(response: &XweatherTileCacheResponse) -> &'static str {
    // preserve exact hit and miss values
    match response.cache_status {
        XweatherTileCacheStatus::Hit => "hit",
        XweatherTileCacheStatus::Miss => "miss",
    }
}

// verifies source persistence, reset, exhaustion, retry, and cache integration parity
#[test]
fn matches_node_xweather_usage_budget_policy() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/xweather-usage-budget-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run Xweather usage budget Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");

    let persisted_calendar = FixtureCalendar::fixed("2026-08-28", "2026-08");
    let persisted_port = FixturePersistence::default();
    let persisted = budget(3, 5, persisted_calendar.clone(), persisted_port.clone());
    persisted
        .reserve(units(2))
        .expect("reserve persisted units");
    let persisted_initial = persisted.snapshot().expect("persisted initial snapshot");
    let recreated = budget(3, 5, persisted_calendar.clone(), persisted_port.clone());
    let persisted_recreated = recreated.snapshot().expect("recreated snapshot");
    persisted_calendar.set("2026-08-29", "2026-08");
    let daily_reset = recreated.snapshot().expect("daily reset");
    recreated.reserve(units(1)).expect("daily reserve");
    let daily_reserved = recreated.snapshot().expect("daily reserved");
    persisted_calendar.set("2026-09-01", "2026-09");
    let monthly_reset = recreated.snapshot().expect("monthly reset");
    recreated.reserve(units(1)).expect("monthly reserve");
    let monthly_reserved = recreated.snapshot().expect("monthly reserved");

    let ceiling_calendar = FixtureCalendar::fixed("2026-08-28", "2026-08");
    let exhausted = budget(
        2,
        3,
        ceiling_calendar.clone(),
        FixturePersistence::default(),
    );
    exhausted.reserve(units(2)).expect("fill daily budget");
    let daily_exhausted = exhausted
        .reserve(units(1))
        .expect_err("reject daily exhaustion");
    ceiling_calendar.set("2026-08-29", "2026-08");
    exhausted.reserve(units(1)).expect("fill monthly budget");
    let monthly_exhausted = exhausted
        .reserve(units(1))
        .expect_err("reject monthly exhaustion");
    let exhausted_snapshot = exhausted.snapshot().expect("exhausted snapshot");

    let reserve_calendar = FixtureCalendar::fixed("2026-08-29", "2026-08");
    let reserve_port = FixturePersistence::default();
    let blocked = budget(3, 5, reserve_calendar, reserve_port.clone());
    reserve_port.fail_next();
    let reserve_persistence = blocked
        .reserve(units(1))
        .expect_err("inject reserve persistence failure");
    let after_reserve_persistence = blocked.snapshot().expect("reserve rollback snapshot");

    let roll_calendar = FixtureCalendar::fixed("2026-08-29", "2026-08");
    let roll_port = FixturePersistence::default();
    let rolling = budget(3, 5, roll_calendar.clone(), roll_port.clone());
    rolling.reserve(units(1)).expect("seed roll fixture");
    roll_calendar.set("2026-08-30", "2026-08");
    roll_port.fail_next();
    let roll_persistence = rolling
        .snapshot()
        .expect_err("inject calendar persistence failure");
    roll_calendar.set("2026-08-29", "2026-08");
    let after_roll_persistence = rolling.snapshot().expect("roll rollback snapshot");

    let cache_calendar = FixtureCalendar::fixed("2026-08-28", "2026-08");
    let cache_budget = Arc::new(budget(
        10,
        10,
        cache_calendar,
        FixturePersistence::default(),
    ));
    let deferred = Arc::new(DeferredBody::pending());
    let provider = FixtureProviderLoader::from_sequences([
        (
            "cached",
            vec![
                FixtureProviderOutcome::Body(b"aa".to_vec()),
                FixtureProviderOutcome::Error(FixtureProviderError("provider failed")),
                FixtureProviderOutcome::Body(b"bb".to_vec()),
            ],
        ),
        (
            "coalesced",
            vec![FixtureProviderOutcome::Deferred(Arc::clone(&deferred))],
        ),
    ]);
    let cache_clock = FixtureCacheClock::fixed(1_000);
    let cache = Arc::new(
        XweatherTileMemoryCache::new(
            XweatherBudgetedTileLoader::new(Arc::clone(&cache_budget), provider.clone()),
            cache_clock.clone(),
            32,
            100,
        )
        .expect("valid budgeted cache"),
    );
    let cached_request = request("cached", 1);
    let cache_miss = cache.get(&cached_request).expect("cache miss");
    let cache_hit = cache.get(&cached_request).expect("cache hit");
    let after_hit = cache_budget.snapshot().expect("after hit snapshot");
    cache_clock.set(1_100);
    let provider_failure = cache
        .get(&cached_request)
        .expect_err("inject provider failure");
    let after_failure = cache_budget.snapshot().expect("after failure snapshot");
    let cache_retry = cache.get(&cached_request).expect("cache retry");
    let after_retry = cache_budget.snapshot().expect("after retry snapshot");
    let coalesced_request = request("coalesced", 2);
    let first_cache = Arc::clone(&cache);
    let first_request = coalesced_request.clone();
    let first = thread::spawn(move || first_cache.get(&first_request));
    deferred.wait_until_started();
    let second_cache = Arc::clone(&cache);
    let second_request = coalesced_request.clone();
    let second = thread::spawn(move || second_cache.get(&second_request));
    thread::sleep(Duration::from_millis(20));
    let pending_snapshot = cache_budget.snapshot().expect("pending snapshot");
    deferred.release(b"cc".to_vec());
    let coalesced_first = first
        .join()
        .expect("first pending thread")
        .expect("first pending response");
    let coalesced_second = second
        .join()
        .expect("second pending thread")
        .expect("second pending response");
    let after_pending = cache_budget.snapshot().expect("after pending snapshot");

    let exhaustion_calendar = FixtureCalendar::fixed("2026-08-28", "2026-08");
    let exhaustion_budget = Arc::new(budget(
        2,
        5,
        exhaustion_calendar,
        FixturePersistence::default(),
    ));
    let exhaustion_provider = FixtureProviderLoader::from_sequences([(
        "full",
        vec![FixtureProviderOutcome::Body(b"ok".to_vec())],
    )]);
    let exhausted_cache = XweatherTileMemoryCache::new(
        XweatherBudgetedTileLoader::new(
            Arc::clone(&exhaustion_budget),
            exhaustion_provider.clone(),
        ),
        cache_clock,
        32,
        100,
    )
    .expect("valid exhausted cache");
    exhausted_cache
        .get(&request("full", 2))
        .expect("fill cache budget");
    let exhausted_error = exhausted_cache
        .get(&request("rejected", 1))
        .expect_err("reject exhausted cache miss");
    let exhausted_retry = exhausted_cache
        .get(&request("rejected", 1))
        .expect_err("reject exhausted cache retry");
    let decision = xweather_budgeted_tile_failure_response(
        &exhausted_error,
        AdmittedXweatherBudgetRetryAfter::admitted("43200"),
    );
    let fallback = xweather_budgeted_tile_failure_response(
        &provider_failure,
        AdmittedXweatherBudgetRetryAfter::admitted("43200"),
    );
    let exhaustion_snapshot = exhaustion_budget
        .snapshot()
        .expect("cache exhaustion snapshot");

    let rust = [
        format!(
            "persisted={}:{}",
            snapshot_signature(&persisted_initial),
            snapshot_signature(&persisted_recreated)
        ),
        format!(
            "windows={}:{}:{}:{}",
            snapshot_signature(&daily_reset),
            snapshot_signature(&daily_reserved),
            snapshot_signature(&monthly_reset),
            snapshot_signature(&monthly_reserved)
        ),
        format!(
            "ceilings={}:{}:{}",
            budget_error_signature(&daily_exhausted),
            budget_error_signature(&monthly_exhausted),
            snapshot_signature(&exhausted_snapshot)
        ),
        format!(
            "reserve-persistence={}:{}",
            budget_error_signature(&reserve_persistence),
            snapshot_signature(&after_reserve_persistence)
        ),
        format!(
            "roll-persistence={}:{}",
            budget_error_signature(&roll_persistence),
            snapshot_signature(&after_roll_persistence)
        ),
        format!(
            "cache={}/{}:{}:{}:{}:{}:{}",
            status_signature(&cache_miss),
            status_signature(&cache_hit),
            snapshot_signature(&after_hit),
            cache_error_signature(&provider_failure),
            snapshot_signature(&after_failure),
            status_signature(&cache_retry),
            snapshot_signature(&after_retry)
        ),
        format!(
            "pending={}:{}/{}:{}:calls={}",
            pending_snapshot.day_units,
            status_signature(&coalesced_first),
            status_signature(&coalesced_second),
            after_pending.day_units,
            provider.calls()
        ),
        format!(
            "exhaustion={}:{}:calls={}:{}",
            cache_error_signature(&exhausted_error),
            cache_error_signature(&exhausted_retry),
            exhaustion_provider.calls(),
            snapshot_signature(&exhaustion_snapshot)
        ),
        format!(
            "decision={}:{}:{}={}",
            decision.status_code(),
            decision.body().trim_end(),
            decision.retry_after_header().expect("retry header").0,
            decision.retry_after_header().expect("retry header").1
        ),
        format!(
            "fallback={}:{}:{}",
            fallback.status_code(),
            fallback.body().trim_end(),
            fallback
                .retry_after_header()
                .map(|(name, value)| format!("{name}={value}"))
                .unwrap_or_else(|| "-".to_owned())
        ),
    ]
    .join("\n");
    assert_eq!(rust, node.trim_end());
}

// verifies source safe-integer, ledger, and map-unit admission boundaries
#[test]
fn rejects_invalid_xweather_budget_inputs() {
    let calendar = FixtureCalendar::fixed("2026-08-28", "2026-08");
    let persistence = FixturePersistence::default();
    assert!(XweatherUsageBudget::new(0, 5, calendar.clone(), persistence.clone()).is_err());
    assert!(XweatherUsageBudget::new(5, 4, calendar.clone(), persistence.clone()).is_err());
    assert!(XweatherUsageBudget::new(1, 9_007_199_254_740_992, calendar, persistence,).is_err());
    assert!(XweatherMapUnits::admitted(0).is_err());
    assert!(XweatherMapUnits::admitted(9_007_199_254_740_992).is_err());
    assert!(
        AdmittedXweatherUsageLedger::admitted(
            AdmittedXweatherUsageCalendar::admitted("2026-08-28", "2026-08"),
            9_007_199_254_740_992,
            0,
        )
        .is_err()
    );
    let direct = budget(
        2,
        5,
        FixtureCalendar::fixed("2026-08-28", "2026-08"),
        FixturePersistence::default(),
    );
    assert!(matches!(
        direct.reserve(units(3)),
        Err(XweatherUsageBudgetError::Exhausted)
    ));
    assert_eq!(direct.snapshot().expect("direct snapshot").day_units, 0);
}
