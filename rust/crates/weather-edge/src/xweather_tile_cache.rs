use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::{Arc, Condvar, Mutex};

use crate::XweatherMapUnits;

const JAVASCRIPT_MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const JAVASCRIPT_MAXIMUM_SAFE_INTEGER_I64: i64 = 9_007_199_254_740_991;

/// one already parsed canonical Xweather tile cache identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct XweatherTileCacheKey(String);

impl XweatherTileCacheKey {
    /// admit one canonical key from the owning tile parser.
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// borrow the admitted canonical identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// the phase-specific freshness class carried by one parsed tile request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XweatherTilePhase {
    History,
    Forecast,
}

/// one reviewed Xweather map product layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XweatherTileLayer {
    Clouds,
    Precipitation,
    Radar,
    Wind,
}

/// one parsed static-frame load descriptor retained outside URL handling.
#[derive(Debug, Clone, PartialEq)]
pub struct XweatherTileFrame {
    pub height: u16,
    pub latitude: f64,
    pub layer: XweatherTileLayer,
    pub longitude: f64,
    pub valid_time: String,
    pub width: u16,
    pub zoom: u8,
}

/// typed provider input paired with one canonical cache identity.
#[derive(Debug, Clone, PartialEq)]
pub enum XweatherTileLoad {
    Opaque,
    Frame(XweatherTileFrame),
}

/// one parsed tile lookup after URL and geometry admission.
#[derive(Debug, Clone, PartialEq)]
pub struct XweatherTileCacheRequest {
    pub key: XweatherTileCacheKey,
    pub phase: XweatherTilePhase,
    pub provider_units: XweatherMapUnits,
    pub tile: XweatherTileLoad,
}

/// immutable non-empty tile bytes admitted from the provider seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XweatherTileBody(Arc<[u8]>);

impl XweatherTileBody {
    /// borrow the cached provider bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// return the stored provider byte length.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// report the non-empty invariant held by every admitted body.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// admit one non-empty provider response.
    fn from_loaded(bytes: Vec<u8>) -> Result<Self, XweatherTileBodyError> {
        // reject the source cache's empty provider response
        if bytes.is_empty() {
            return Err(XweatherTileBodyError);
        }

        Ok(Self(Arc::from(bytes)))
    }
}

/// one invalid provider body returned by the injected loader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XweatherTileBodyError;

impl Display for XweatherTileBodyError {
    /// render the source cache's exact provider-body error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Xweather tile loader returned an invalid body")
    }
}

impl Error for XweatherTileBodyError {}

/// the source-visible cache outcome for one returned tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XweatherTileCacheStatus {
    Hit,
    Miss,
}

impl XweatherTileCacheStatus {
    /// return the exact source status value.
    pub const fn wire_value(self) -> &'static str {
        // preserve the two source cache result categories
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
        }
    }
}

/// one immutable cached or freshly loaded tile result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XweatherTileCacheResponse {
    pub body: XweatherTileBody,
    pub cache_status: XweatherTileCacheStatus,
    pub fetched_at_epoch_ms: i64,
    pub phase: XweatherTilePhase,
}

/// provider loading retained outside cache policy and transport details.
pub trait XweatherTileLoader {
    type Error;

    /// load already parsed tile bytes without URL or HTTP behavior here.
    fn load_tile(&self, request: &XweatherTileCacheRequest) -> Result<Vec<u8>, Self::Error>;
}

/// epoch observation retained outside cache policy and time serialization.
pub trait XweatherTileClock {
    /// return the current admitted epoch millisecond instant.
    fn now_epoch_ms(&self) -> i64;
}

/// one invalid bounded cache configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XweatherTileCacheConfigurationError;

impl Display for XweatherTileCacheConfigurationError {
    /// render the source cache's exact configuration error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid Xweather tile cache configuration")
    }
}

impl Error for XweatherTileCacheConfigurationError {}

/// one provider or provider-body failure from a cache lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XweatherTileCacheGetError<LoaderError> {
    Loader(LoaderError),
    InvalidBody(XweatherTileBodyError),
}

impl<LoaderError: Display> Display for XweatherTileCacheGetError<LoaderError> {
    /// render the unchanged underlying lookup failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve provider and body validation categories
        match self {
            Self::Loader(error) => Display::fmt(error, formatter),
            Self::InvalidBody(error) => Display::fmt(error, formatter),
        }
    }
}

impl<LoaderError> Error for XweatherTileCacheGetError<LoaderError>
where
    LoaderError: Error + 'static,
{
    /// retain the exact underlying lookup source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // expose only the owning provider or validation failure
        match self {
            Self::Loader(error) => Some(error),
            Self::InvalidBody(error) => Some(error),
        }
    }
}

/// bounded operational counters without cached tile material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XweatherTileCacheStats {
    pub entries: usize,
    pub inflight: usize,
    pub maximum_bytes: usize,
    pub stored_bytes: usize,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    body: XweatherTileBody,
    fetched_at_epoch_ms: i64,
    phase: XweatherTilePhase,
}

impl CacheEntry {
    /// return one public response with the selected lookup status.
    fn response(&self, cache_status: XweatherTileCacheStatus) -> XweatherTileCacheResponse {
        XweatherTileCacheResponse {
            body: self.body.clone(),
            cache_status,
            fetched_at_epoch_ms: self.fetched_at_epoch_ms,
            phase: self.phase,
        }
    }
}

#[derive(Debug)]
struct PendingLoad<LoaderError> {
    completed:
        Mutex<Option<Result<XweatherTileCacheResponse, XweatherTileCacheGetError<LoaderError>>>>,
    ready: Condvar,
}

impl<LoaderError: Clone> PendingLoad<LoaderError> {
    /// create one unresolved same-key provider load.
    fn new() -> Self {
        Self {
            completed: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    /// publish one completed provider result to every same-key waiter.
    fn complete(
        &self,
        result: Result<XweatherTileCacheResponse, XweatherTileCacheGetError<LoaderError>>,
    ) {
        let mut completed = self
            .completed
            .lock()
            .expect("Xweather tile pending-load lock poisoned");
        *completed = Some(result);
        self.ready.notify_all();
    }

    /// wait for and clone the shared same-key provider result.
    fn wait(&self) -> Result<XweatherTileCacheResponse, XweatherTileCacheGetError<LoaderError>> {
        let mut completed = self
            .completed
            .lock()
            .expect("Xweather tile pending-load lock poisoned");

        // wait only while the provider result remains unresolved
        while completed.is_none() {
            completed = self
                .ready
                .wait(completed)
                .expect("Xweather tile pending-load lock poisoned");
        }

        completed
            .as_ref()
            .expect("completed Xweather tile load")
            .clone()
    }
}

#[derive(Debug)]
struct CacheState<LoaderError> {
    entries: HashMap<XweatherTileCacheKey, CacheEntry>,
    inflight: HashMap<XweatherTileCacheKey, Arc<PendingLoad<LoaderError>>>,
    lru: VecDeque<XweatherTileCacheKey>,
    stored_bytes: usize,
}

impl<LoaderError> CacheState<LoaderError> {
    /// create one empty bounded cache state.
    fn empty() -> Self {
        Self {
            entries: HashMap::new(),
            inflight: HashMap::new(),
            lru: VecDeque::new(),
            stored_bytes: 0,
        }
    }

    /// promote one cache hit to the most-recent boundary.
    fn touch(&mut self, key: &XweatherTileCacheKey) {
        self.remove_lru_key(key);
        self.lru.push_back(key.clone());
    }

    /// store one provider response within the memory budget.
    fn store(&mut self, key: XweatherTileCacheKey, entry: CacheEntry, maximum_bytes: usize) {
        // replace one prior body without double-counting bytes
        if let Some(prior) = self.entries.remove(&key) {
            self.stored_bytes -= prior.body.len();
            self.remove_lru_key(&key);
        }

        // skip bodies that cannot fit by themselves
        if entry.body.len() > maximum_bytes {
            return;
        }

        self.stored_bytes += entry.body.len();
        self.entries.insert(key.clone(), entry);
        self.lru.push_back(key);

        // restore the configured byte bound after every insertion
        while self.stored_bytes > maximum_bytes {
            let Some(oldest_key) = self.eviction_candidate() else {
                self.stored_bytes = 0;
                break;
            };
            self.remove_entry(&oldest_key);
        }
    }

    /// select the least-recent forecast entry before immutable history.
    fn eviction_candidate(&self) -> Option<XweatherTileCacheKey> {
        // preserve insertion order while preferring mutable forecast data
        for key in &self.lru {
            // return the first live forecast candidate
            if self.entries.get(key).map(|entry| entry.phase) == Some(XweatherTilePhase::Forecast) {
                return Some(key.clone());
            }
        }

        self.lru.front().cloned()
    }

    /// remove one cached entry and its exact byte contribution.
    fn remove_entry(&mut self, key: &XweatherTileCacheKey) {
        // subtract only a live cached body
        if let Some(entry) = self.entries.remove(key) {
            self.stored_bytes -= entry.body.len();
        }
        self.remove_lru_key(key);
    }

    /// remove one key from the explicit LRU order.
    fn remove_lru_key(&mut self, key: &XweatherTileCacheKey) {
        // remove the single live ordering position when present
        if let Some(position) = self.lru.iter().position(|candidate| candidate == key) {
            self.lru.remove(position);
        }
    }
}

/// one transport-independent bounded in-process Xweather tile cache.
#[derive(Debug)]
pub struct XweatherTileMemoryCache<Loader, Clock>
where
    Loader: XweatherTileLoader,
{
    clock: Clock,
    forecast_freshness_ms: i64,
    loader: Loader,
    maximum_bytes: usize,
    state: Mutex<CacheState<Loader::Error>>,
}

impl<Loader, Clock> XweatherTileMemoryCache<Loader, Clock>
where
    Loader: XweatherTileLoader,
    Loader::Error: Clone,
    Clock: XweatherTileClock,
{
    /// configure one bounded cache with injected provider and clock seams.
    pub fn new(
        loader: Loader,
        clock: Clock,
        maximum_bytes: u64,
        forecast_freshness_ms: i64,
    ) -> Result<Self, XweatherTileCacheConfigurationError> {
        // reject the source cache's unsafe numeric configuration
        if !(1..=JAVASCRIPT_MAXIMUM_SAFE_INTEGER).contains(&maximum_bytes)
            || !(1..=JAVASCRIPT_MAXIMUM_SAFE_INTEGER_I64).contains(&forecast_freshness_ms)
        {
            return Err(XweatherTileCacheConfigurationError);
        }
        let maximum_bytes =
            usize::try_from(maximum_bytes).map_err(|_| XweatherTileCacheConfigurationError)?;
        Ok(Self {
            clock,
            forecast_freshness_ms,
            loader,
            maximum_bytes,
            state: Mutex::new(CacheState::empty()),
        })
    }

    /// load one tile from memory or one coalesced provider request.
    pub fn get(
        &self,
        request: &XweatherTileCacheRequest,
    ) -> Result<XweatherTileCacheResponse, XweatherTileCacheGetError<Loader::Error>> {
        let pending = {
            let mut state = self
                .state
                .lock()
                .expect("Xweather tile cache state lock poisoned");
            let cached = state.entries.get(&request.key).cloned();

            // return immutable history or one fresh forecast entry
            if let Some(cached) = cached {
                // promote only a phase-fresh cached entry
                if self.is_fresh(request.phase, &cached) {
                    state.touch(&request.key);
                    return Ok(cached.response(XweatherTileCacheStatus::Hit));
                }
            }

            // coalesce every concurrent same-key miss or refresh
            if let Some(pending) = state.inflight.get(&request.key) {
                Some(Arc::clone(pending))
            } else {
                let pending = Arc::new(PendingLoad::new());
                state
                    .inflight
                    .insert(request.key.clone(), Arc::clone(&pending));
                None
            }
        };

        // wait on the already-running same-key provider request
        if let Some(pending) = pending {
            return pending.wait();
        }

        let result = self.load_and_store(request);
        let pending = {
            let mut state = self
                .state
                .lock()
                .expect("Xweather tile cache state lock poisoned");
            state
                .inflight
                .remove(&request.key)
                .expect("owned Xweather tile provider load")
        };
        pending.complete(result.clone());
        result
    }

    /// expose bounded operational counters without tile material.
    pub fn stats(&self) -> XweatherTileCacheStats {
        let state = self
            .state
            .lock()
            .expect("Xweather tile cache state lock poisoned");
        XweatherTileCacheStats {
            entries: state.entries.len(),
            inflight: state.inflight.len(),
            maximum_bytes: self.maximum_bytes,
            stored_bytes: state.stored_bytes,
        }
    }

    /// apply phase-specific freshness without observing history time.
    fn is_fresh(&self, phase: XweatherTilePhase, entry: &CacheEntry) -> bool {
        // preserve immutable historical frames for the process lifetime
        if phase == XweatherTilePhase::History {
            return true;
        }

        i128::from(self.clock.now_epoch_ms()) - i128::from(entry.fetched_at_epoch_ms)
            < i128::from(self.forecast_freshness_ms)
    }

    /// validate, timestamp, and retain one successful provider response.
    fn load_and_store(
        &self,
        request: &XweatherTileCacheRequest,
    ) -> Result<XweatherTileCacheResponse, XweatherTileCacheGetError<Loader::Error>> {
        let loaded = self
            .loader
            .load_tile(request)
            .map_err(XweatherTileCacheGetError::Loader)?;
        let body = XweatherTileBody::from_loaded(loaded)
            .map_err(XweatherTileCacheGetError::InvalidBody)?;
        let entry = CacheEntry {
            body,
            fetched_at_epoch_ms: self.clock.now_epoch_ms(),
            phase: request.phase,
        };
        let response = entry.response(XweatherTileCacheStatus::Miss);
        let mut state = self
            .state
            .lock()
            .expect("Xweather tile cache state lock poisoned");
        state.store(request.key.clone(), entry, self.maximum_bytes);
        Ok(response)
    }
}
