//! Bounded blocking route execution and shared forecast-evidence store ownership.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};
use weather_edge::{
    ForecastCaptureSource, ForecastCaptureVersion, ForecastEvidenceOutcome, ForecastEvidenceWindow,
    ForecastResponseEvidenceStore,
};

use crate::NativeEdgePublicRouteGraph;

/// Cloneable proxy around one genuine process forecast-evidence store.
///
/// Every trait call releases the mutex before returning, so route dispatch and provider network
/// work never retain the evidence-store lock.
pub struct NativeSharedForecastResponseEvidenceStore<Store> {
    inner: Arc<Mutex<Store>>,
}

impl<Store> NativeSharedForecastResponseEvidenceStore<Store> {
    /// Own one initialized real store behind a process-shared mutex.
    pub fn new(store: Store) -> Self {
        Self {
            inner: Arc::new(Mutex::new(store)),
        }
    }

    /// Bind an already shared real store without adding another synchronization layer.
    pub fn from_shared(inner: Arc<Mutex<Store>>) -> Self {
        Self { inner }
    }

    /// Clone the exact shared store handle for integration or terminal settlement ownership.
    pub fn shared(&self) -> Arc<Mutex<Store>> {
        Arc::clone(&self.inner)
    }
}

impl<Store> Clone for NativeSharedForecastResponseEvidenceStore<Store> {
    /// Share only the existing store, never clone its mutable evidence state.
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<Store> ForecastResponseEvidenceStore for NativeSharedForecastResponseEvidenceStore<Store>
where
    Store: ForecastResponseEvidenceStore,
{
    type Prepared = Store::Prepared;
    type Error = NativeSharedForecastResponseEvidenceStoreError<Store::Error>;

    /// Lock only while the real store prepares one immutable evidence value.
    fn prepare(
        &mut self,
        source: ForecastCaptureSource<'_>,
        window: Option<ForecastEvidenceWindow>,
        version: ForecastCaptureVersion,
    ) -> Option<Self::Prepared> {
        // a poisoned store becomes the existing bounded preparation refusal
        match self.inner.lock() {
            Ok(mut store) => store.prepare(source, window, version),
            Err(_) => None,
        }
    }

    /// Lock only for the real store's serialized durable commit.
    fn commit(
        &mut self,
        prepared: &Self::Prepared,
        first_edge_committed_at: &str,
    ) -> Result<ForecastEvidenceOutcome, Self::Error> {
        self.inner
            .lock()
            .map_err(|_| NativeSharedForecastResponseEvidenceStoreError::Unavailable)?
            .commit(prepared, first_edge_committed_at)
            .map_err(NativeSharedForecastResponseEvidenceStoreError::Store)
    }

    /// Lock only while recording the real store's early-close observation.
    fn note_close_without_finish(&mut self) {
        // a poisoned store cannot safely accept another mutable observation
        if let Ok(mut store) = self.inner.lock() {
            store.note_close_without_finish();
        }
    }

    /// Lock only while recording the real store's bounded asynchronous failure.
    fn note_write_failure(&mut self) {
        // a poisoned store cannot safely accept another mutable observation
        if let Ok(mut store) = self.inner.lock() {
            store.note_write_failure();
        }
    }
}

/// Shared evidence-store failures not already contained by the capture lifecycle.
#[derive(Debug)]
pub enum NativeSharedForecastResponseEvidenceStoreError<StoreError> {
    Unavailable,
    Store(StoreError),
}

impl<StoreError: Display> Display for NativeSharedForecastResponseEvidenceStoreError<StoreError> {
    /// Preserve a real store error while keeping mutex refusal categorical.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // distinguish synchronization refusal from the underlying store operation
        match self {
            Self::Unavailable => formatter.write_str("forecast evidence store is unavailable"),
            Self::Store(error) => write!(formatter, "forecast evidence store: {error}"),
        }
    }
}

impl<StoreError: Error + 'static> Error
    for NativeSharedForecastResponseEvidenceStoreError<StoreError>
{
    /// Preserve only a genuine nested store failure.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // synchronization refusal has no nested source
        match self {
            Self::Unavailable => None,
            Self::Store(error) => Some(error),
        }
    }
}

/// Process-shared public route executor with a fixed blocking-work concurrency ceiling.
#[derive(Clone)]
pub struct NativeEdgeRouteExecution {
    graph: Arc<NativeEdgePublicRouteGraph>,
    permits: Arc<Semaphore>,
    lifecycle: Arc<NativeEdgeRouteExecutionLifecycle>,
    maximum_workers: NonZeroUsize,
}

#[derive(Default)]
struct NativeEdgeRouteExecutionState {
    active: usize,
    closed: bool,
}

#[derive(Default)]
struct NativeEdgeRouteExecutionLifecycle {
    state: Mutex<NativeEdgeRouteExecutionState>,
    drained: Notify,
}

impl NativeEdgeRouteExecutionLifecycle {
    /// Recover the small counter state because user route work never runs under this mutex.
    fn state(&self) -> MutexGuard<'_, NativeEdgeRouteExecutionState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

struct NativeEdgeRouteExecutionGuard {
    lifecycle: Arc<NativeEdgeRouteExecutionLifecycle>,
    permit: Option<OwnedSemaphorePermit>,
}

impl Drop for NativeEdgeRouteExecutionGuard {
    /// Release one admitted worker and wake shutdown after the last closure has really exited.
    fn drop(&mut self) {
        self.permit.take();
        let mut state = self.lifecycle.state();
        state.active = state.active.saturating_sub(1);
        let drained = state.closed && state.active == 0;
        drop(state);
        // notify only after publishing the zero-active state
        if drained {
            self.lifecycle.drained.notify_waiters();
        }
    }
}

impl NativeEdgeRouteExecution {
    /// Bind an already initialized route graph to one explicit nonzero worker limit.
    pub fn new(graph: Arc<NativeEdgePublicRouteGraph>, maximum_workers: NonZeroUsize) -> Self {
        Self {
            graph,
            permits: Arc::new(Semaphore::new(maximum_workers.get())),
            lifecycle: Arc::new(NativeEdgeRouteExecutionLifecycle::default()),
            maximum_workers,
        }
    }

    /// Return the immutable configured worker ceiling rather than transient available permits.
    pub const fn maximum_workers(&self) -> NonZeroUsize {
        self.maximum_workers
    }

    /// Borrow the retained route graph for process integration and identity checks.
    pub const fn graph(&self) -> &Arc<NativeEdgePublicRouteGraph> {
        &self.graph
    }

    /// Refuse later admissions while allowing already admitted blocking operations to finish.
    pub fn close(&self) {
        // publish closure before waking queued semaphore acquisitions
        self.lifecycle.state().closed = true;
        self.permits.close();
    }

    /// Refuse new work and await every already admitted blocking route closure.
    pub async fn close_and_drain(&self) {
        self.close();
        // register each notification before observing active work to avoid a lost final wakeup
        loop {
            let notified = self.lifecycle.drained.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            // return only after the final RAII worker guard published completion
            if self.lifecycle.state().active == 0 {
                return;
            }
            notified.as_mut().await;
        }
    }

    /// Run one actual synchronous route closure outside Tokio's connection reactor.
    pub async fn execute<Operation, Output>(
        &self,
        operation: Operation,
    ) -> Result<Output, NativeEdgeRouteExecutionError>
    where
        Operation: FnOnce(&NativeEdgePublicRouteGraph) -> Output + Send + 'static,
        Output: Send + 'static,
    {
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| NativeEdgeRouteExecutionError::Closed)?;
        let guard = {
            let mut state = self.lifecycle.state();
            // reject a permit acquired concurrently with process closure before admitting work
            if state.closed {
                return Err(NativeEdgeRouteExecutionError::Closed);
            }
            state.active += 1;
            NativeEdgeRouteExecutionGuard {
                lifecycle: Arc::clone(&self.lifecycle),
                permit: Some(permit),
            }
        };
        let graph = Arc::clone(&self.graph);
        // retain the permit inside the blocking job for the complete synchronous route closure
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            operation(&graph)
        })
        .await
        .map_err(|_| NativeEdgeRouteExecutionError::Worker)
    }
}

/// Explicit route admission or blocking-worker failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEdgeRouteExecutionError {
    Closed,
    Worker,
}

impl Display for NativeEdgeRouteExecutionError {
    /// Render the exact bounded execution refusal category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // distinguish deliberate closure from a failed blocking task
        match self {
            Self::Closed => formatter.write_str("native edge route execution is closed"),
            Self::Worker => formatter.write_str("native edge route worker failed"),
        }
    }
}

impl Error for NativeEdgeRouteExecutionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_contracts::JsonValue;

    #[derive(Default)]
    struct FixtureStore {
        closed: usize,
        commits: usize,
        failures: usize,
        prepares: usize,
    }

    impl ForecastResponseEvidenceStore for FixtureStore {
        type Prepared = usize;
        type Error = std::convert::Infallible;

        /// Record one real proxy preparation call.
        fn prepare(
            &mut self,
            _source: ForecastCaptureSource<'_>,
            _window: Option<ForecastEvidenceWindow>,
            _version: ForecastCaptureVersion,
        ) -> Option<Self::Prepared> {
            self.prepares += 1;
            Some(self.prepares)
        }

        /// Record one real proxy commit call.
        fn commit(
            &mut self,
            _prepared: &Self::Prepared,
            _first_edge_committed_at: &str,
        ) -> Result<ForecastEvidenceOutcome, Self::Error> {
            self.commits += 1;
            Ok(ForecastEvidenceOutcome::Created)
        }

        /// Record one early-close call.
        fn note_close_without_finish(&mut self) {
            self.closed += 1;
        }

        /// Record one asynchronous write-failure call.
        fn note_write_failure(&mut self) {
            self.failures += 1;
        }
    }

    /// Verify cloned proxies share only short trait-call critical sections and real state.
    #[test]
    fn forecast_evidence_proxy_shares_real_store() {
        let mut first = NativeSharedForecastResponseEvidenceStore::new(FixtureStore::default());
        let mut second = first.clone();
        let value = JsonValue::Array(Vec::new());
        let prepared = first
            .prepare(
                ForecastCaptureSource::Projected(&value),
                Some(ForecastEvidenceWindow::TenDays),
                ForecastCaptureVersion::Legacy,
            )
            .unwrap();
        assert!(matches!(
            second.commit(&prepared, "2026-11-24T16:00:00.000Z"),
            Ok(ForecastEvidenceOutcome::Created)
        ));
        first.note_close_without_finish();
        second.note_write_failure();
        let shared = first.shared();
        let store = shared.lock().unwrap();
        assert_eq!(store.prepares, 1);
        assert_eq!(store.commits, 1);
        assert_eq!(store.closed, 1);
        assert_eq!(store.failures, 1);
    }

    /// Verify guard destruction publishes completion and returns its owned semaphore permit.
    #[test]
    fn route_execution_guard_releases_active_worker() {
        let permits = Arc::new(Semaphore::new(1));
        let permit = Arc::clone(&permits).try_acquire_owned().unwrap();
        let lifecycle = Arc::new(NativeEdgeRouteExecutionLifecycle::default());
        {
            let mut state = lifecycle.state();
            state.active = 1;
            state.closed = true;
        }
        let guard = NativeEdgeRouteExecutionGuard {
            lifecycle: Arc::clone(&lifecycle),
            permit: Some(permit),
        };
        assert_eq!(permits.available_permits(), 0);
        drop(guard);
        assert_eq!(lifecycle.state().active, 0);
        assert_eq!(permits.available_permits(), 1);
    }
}
