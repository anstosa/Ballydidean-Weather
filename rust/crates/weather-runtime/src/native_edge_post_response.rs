//! Serialized off-reactor forecast evidence and optional archive settlement.

use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::{Notify, mpsc, oneshot};
use weather_edge::{
    CompletedForecastResponseCapture, EdgeForecastTerminalObserver, ForecastPostResponseWork,
    ForecastResponseEvidenceStore, ForecastResponseTerminal, ForecastScheduleArchive,
    ForecastScheduledEvidenceArchive, RevisionArchiveCanonicalClock,
    persist_forecast_response_capture,
};

/// Preserve disabled, failed initialization, and ready archive states independently.
pub enum NativeEdgePostResponseArchive<Archive> {
    Disabled,
    Unavailable,
    Ready(Archive),
}

struct NativeEdgePostResponseOwners<Store, Archive> {
    store: Store,
    archive: NativeEdgePostResponseArchive<Archive>,
}

enum NativeEdgePostResponseMessage<Prepared> {
    Work {
        work: ForecastPostResponseWork<Prepared>,
        completion: oneshot::Sender<
            Result<CompletedForecastResponseCapture, NativeEdgePostResponseExecutionError>,
        >,
    },
    Terminal {
        observer: EdgeForecastTerminalObserver<Prepared>,
        terminal: ForecastResponseTerminal,
        canonical_terminal_at: Option<String>,
        completion: oneshot::Sender<
            Result<CompletedForecastResponseCapture, NativeEdgePostResponseExecutionError>,
        >,
    },
    Shutdown,
}

enum NativeEdgePostResponseOperation<Prepared> {
    Work(ForecastPostResponseWork<Prepared>),
    Terminal {
        observer: EdgeForecastTerminalObserver<Prepared>,
        terminal: ForecastResponseTerminal,
        canonical_terminal_at: Option<String>,
    },
}

struct NativeEdgeFixedTerminalClock(Option<String>);

impl RevisionArchiveCanonicalClock for NativeEdgeFixedTerminalClock {
    /// Return the one canonical timestamp sampled by the socket terminal callback.
    fn canonical_now(&mut self) -> Option<String> {
        self.0.take()
    }
}

#[derive(Default)]
struct NativeEdgePostResponseState {
    closed: bool,
    drained: bool,
    failed: bool,
}

#[derive(Default)]
struct NativeEdgePostResponseLifecycle {
    state: Mutex<NativeEdgePostResponseState>,
    drained: Notify,
}

impl NativeEdgePostResponseLifecycle {
    /// Recover only lifecycle flags because persistence never runs under this mutex.
    fn state(&self) -> MutexGuard<'_, NativeEdgePostResponseState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

struct NativeEdgePostResponseWorkerGuard {
    lifecycle: Arc<NativeEdgePostResponseLifecycle>,
    normal: bool,
}

impl NativeEdgePostResponseWorkerGuard {
    /// Mark one FIFO receiver exit after every preceding item completed.
    fn finish(&mut self) {
        self.normal = true;
    }
}

impl Drop for NativeEdgePostResponseWorkerGuard {
    /// Publish drain or worker failure and wake every shutdown waiter.
    fn drop(&mut self) {
        let mut state = self.lifecycle.state();
        state.closed = true;
        state.drained = true;
        state.failed |= !self.normal;
        drop(state);
        self.lifecycle.drained.notify_waiters();
    }
}

/// Unbounded source-compatible FIFO lane over one real evidence store and archive owner.
///
/// The predecessor chains commits on a Promise without a capacity rejection. This lane therefore
/// uses Tokio's unbounded channel, but consumes exactly one item at a time and moves every actual
/// filesystem operation to `spawn_blocking`.
pub struct NativeEdgePostResponseLane<Store, Archive>
where
    Store: ForecastResponseEvidenceStore,
{
    sender: mpsc::UnboundedSender<NativeEdgePostResponseMessage<Store::Prepared>>,
    lifecycle: Arc<NativeEdgePostResponseLifecycle>,
    terminal_store: Store,
    _archive: std::marker::PhantomData<fn() -> Archive>,
}

impl<Store, Archive> NativeEdgePostResponseLane<Store, Archive>
where
    Store: Clone + ForecastResponseEvidenceStore + Send + 'static,
    Store::Prepared: Send + 'static,
    Archive: ForecastScheduledEvidenceArchive<Store::Prepared> + Send + 'static,
{
    /// Start one serialized consumer retaining the actual store and optional archive state.
    pub fn new(store: Store, archive: NativeEdgePostResponseArchive<Archive>) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let lifecycle = Arc::new(NativeEdgePostResponseLifecycle::default());
        let owners = Arc::new(Mutex::new(NativeEdgePostResponseOwners {
            store: store.clone(),
            archive,
        }));
        // detach the process-owned consumer while lifecycle state retains drain observability
        drop(tokio::spawn(run_native_edge_post_response_lane(
            receiver,
            owners,
            Arc::clone(&lifecycle),
        )));
        Self {
            sender,
            lifecycle,
            terminal_store: store,
            _archive: std::marker::PhantomData,
        }
    }

    /// Clone the shared proxy used to prepare evidence during synchronous route dispatch.
    pub fn evidence_store(&self) -> Store {
        self.terminal_store.clone()
    }

    /// Enqueue one owned first-wins terminal with its socket-time clock sample.
    pub fn submit_terminal(
        &self,
        observer: EdgeForecastTerminalObserver<Store::Prepared>,
        terminal: ForecastResponseTerminal,
        canonical_terminal_at: Option<String>,
    ) -> Result<
        NativeEdgePostResponseReceipt,
        NativeEdgePostResponseTerminalSubmitError<Store::Prepared>,
    > {
        let (completion, receiver) = oneshot::channel();
        let mut state = self.lifecycle.state();
        // return the unobserved terminal when shutdown already won admission
        if state.closed {
            return Err(NativeEdgePostResponseTerminalSubmitError {
                observer,
                terminal,
                canonical_terminal_at,
            });
        }
        let message = NativeEdgePostResponseMessage::Terminal {
            observer,
            terminal,
            canonical_terminal_at,
            completion,
        };
        // send under the same lock as work and shutdown to retain one source FIFO
        if let Err(error) = self.sender.send(message) {
            state.closed = true;
            let NativeEdgePostResponseMessage::Terminal {
                observer,
                terminal,
                canonical_terminal_at,
                ..
            } = error.0
            else {
                unreachable!("terminal submission sends only terminal state");
            };
            return Err(NativeEdgePostResponseTerminalSubmitError {
                observer,
                terminal,
                canonical_terminal_at,
            });
        }
        drop(state);
        Ok(NativeEdgePostResponseReceipt { receiver })
    }

    /// Enqueue one already observed terminal action without imposing a new capacity policy.
    pub fn submit(
        &self,
        work: ForecastPostResponseWork<Store::Prepared>,
    ) -> Result<NativeEdgePostResponseReceipt, NativeEdgePostResponseSubmitError<Store::Prepared>>
    {
        let (completion, receiver) = oneshot::channel();
        let mut state = self.lifecycle.state();
        // refuse work after shutdown while returning the exact unpersisted item to the caller
        if state.closed {
            return Err(NativeEdgePostResponseSubmitError { work });
        }
        let message = NativeEdgePostResponseMessage::Work { work, completion };
        // send under the lifecycle lock so shutdown cannot overtake an admitted work item
        if let Err(error) = self.sender.send(message) {
            state.closed = true;
            let NativeEdgePostResponseMessage::Work { work, .. } = error.0 else {
                unreachable!("submit sends only work");
            };
            return Err(NativeEdgePostResponseSubmitError { work });
        }
        drop(state);
        Ok(NativeEdgePostResponseReceipt { receiver })
    }

    /// Stop admissions after all previously submitted FIFO work and await real worker exit.
    pub async fn close_and_drain(&self) -> Result<(), NativeEdgePostResponseExecutionError> {
        {
            let mut state = self.lifecycle.state();
            // enqueue exactly one shutdown marker after all lock-serialized work submissions
            if !state.closed {
                state.closed = true;
                let _result = self.sender.send(NativeEdgePostResponseMessage::Shutdown);
            }
        }
        // register before checking drain state so the final worker wake cannot be lost
        loop {
            let notified = self.lifecycle.drained.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let state = self.lifecycle.state();
                // report a worker failure only after every reachable completion sender has closed
                if state.drained {
                    return if state.failed {
                        Err(NativeEdgePostResponseExecutionError::Worker)
                    } else {
                        Ok(())
                    };
                }
            }
            notified.as_mut().await;
        }
    }
}

impl<Store, Archive> Clone for NativeEdgePostResponseLane<Store, Archive>
where
    Store: Clone + ForecastResponseEvidenceStore,
{
    /// Share only the FIFO sender, lifecycle, and real shared evidence-store proxy.
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            lifecycle: Arc::clone(&self.lifecycle),
            terminal_store: self.terminal_store.clone(),
            _archive: std::marker::PhantomData,
        }
    }
}

/// Awaitable result retained by public callers or scheduler capture admission.
pub struct NativeEdgePostResponseReceipt {
    receiver: oneshot::Receiver<
        Result<CompletedForecastResponseCapture, NativeEdgePostResponseExecutionError>,
    >,
}

impl NativeEdgePostResponseReceipt {
    /// Await evidence plus optional archive settlement from the one serialized worker.
    pub async fn completion(
        self,
    ) -> Result<CompletedForecastResponseCapture, NativeEdgePostResponseExecutionError> {
        self.receiver
            .await
            .unwrap_or(Err(NativeEdgePostResponseExecutionError::Worker))
    }
}

/// Closed-lane refusal returning the exact work that never entered the FIFO.
pub struct NativeEdgePostResponseSubmitError<Prepared> {
    work: ForecastPostResponseWork<Prepared>,
}

impl<Prepared> NativeEdgePostResponseSubmitError<Prepared> {
    /// Recover the unpersisted terminal action for explicit caller refusal handling.
    pub fn into_work(self) -> ForecastPostResponseWork<Prepared> {
        self.work
    }
}

impl<Prepared> Debug for NativeEdgePostResponseSubmitError<Prepared> {
    /// Keep potentially sensitive prepared evidence out of diagnostics.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeEdgePostResponseSubmitError")
    }
}

impl<Prepared> Display for NativeEdgePostResponseSubmitError<Prepared> {
    /// Report only the closed FIFO condition.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("native edge post-response lane is closed")
    }
}

impl<Prepared: 'static> Error for NativeEdgePostResponseSubmitError<Prepared> {}

/// Closed-lane refusal returning one terminal that was never observed.
pub struct NativeEdgePostResponseTerminalSubmitError<Prepared> {
    observer: EdgeForecastTerminalObserver<Prepared>,
    terminal: ForecastResponseTerminal,
    canonical_terminal_at: Option<String>,
}

impl<Prepared> NativeEdgePostResponseTerminalSubmitError<Prepared> {
    /// Recover every unobserved terminal field for explicit caller failure settlement.
    pub fn into_parts(
        self,
    ) -> (
        EdgeForecastTerminalObserver<Prepared>,
        ForecastResponseTerminal,
        Option<String>,
    ) {
        (self.observer, self.terminal, self.canonical_terminal_at)
    }
}

impl<Prepared> Debug for NativeEdgePostResponseTerminalSubmitError<Prepared> {
    /// Keep prepared evidence and timestamps out of diagnostics.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeEdgePostResponseTerminalSubmitError")
    }
}

impl<Prepared> Display for NativeEdgePostResponseTerminalSubmitError<Prepared> {
    /// Report only the closed FIFO condition.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("native edge post-response lane is closed")
    }
}

impl<Prepared: 'static> Error for NativeEdgePostResponseTerminalSubmitError<Prepared> {}

/// Categorical blocking-worker or receiver failure after admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEdgePostResponseExecutionError {
    Worker,
}

impl Display for NativeEdgePostResponseExecutionError {
    /// Render the explicit post-response worker refusal.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("native edge post-response worker failed")
    }
}

impl Error for NativeEdgePostResponseExecutionError {}

/// Consume one source-ordered FIFO until explicit shutdown or a blocking worker failure.
async fn run_native_edge_post_response_lane<Store, Archive>(
    mut receiver: mpsc::UnboundedReceiver<NativeEdgePostResponseMessage<Store::Prepared>>,
    owners: Arc<Mutex<NativeEdgePostResponseOwners<Store, Archive>>>,
    lifecycle: Arc<NativeEdgePostResponseLifecycle>,
) where
    Store: ForecastResponseEvidenceStore + Send + 'static,
    Store::Prepared: Send + 'static,
    Archive: ForecastScheduledEvidenceArchive<Store::Prepared> + Send + 'static,
{
    let mut guard = NativeEdgePostResponseWorkerGuard {
        lifecycle,
        normal: false,
    };
    // await and settle exactly one item before advancing the source FIFO
    while let Some(message) = receiver.recv().await {
        let (operation, completion) = match message {
            NativeEdgePostResponseMessage::Work { work, completion } => {
                (NativeEdgePostResponseOperation::Work(work), completion)
            }
            NativeEdgePostResponseMessage::Terminal {
                observer,
                terminal,
                canonical_terminal_at,
                completion,
            } => (
                NativeEdgePostResponseOperation::Terminal {
                    observer,
                    terminal,
                    canonical_terminal_at,
                },
                completion,
            ),
            NativeEdgePostResponseMessage::Shutdown => {
                guard.finish();
                return;
            }
        };
        let owners = Arc::clone(&owners);
        let completed = tokio::task::spawn_blocking(move || {
            let mut owners = owners.lock().map_err(|_| ())?;
            let NativeEdgePostResponseOwners { store, archive } = &mut *owners;
            let work = match operation {
                NativeEdgePostResponseOperation::Work(work) => work,
                NativeEdgePostResponseOperation::Terminal {
                    mut observer,
                    terminal,
                    canonical_terminal_at,
                } => {
                    let mut clock = NativeEdgeFixedTerminalClock(canonical_terminal_at);
                    // observe first-wins terminal state only while off the reactor
                    let Some(work) = observer.observe(terminal, &mut clock, store) else {
                        return Ok(None);
                    };
                    work
                }
            };
            // preserve disabled, unavailable, and ready scheduler settlement distinctions
            let archive = match archive {
                NativeEdgePostResponseArchive::Disabled => ForecastScheduleArchive::Disabled,
                NativeEdgePostResponseArchive::Unavailable => ForecastScheduleArchive::Unavailable,
                NativeEdgePostResponseArchive::Ready(archive) => {
                    ForecastScheduleArchive::Ready(archive)
                }
            };
            Ok(Some(persist_forecast_response_capture(
                work, store, archive,
            )))
        })
        .await;
        // fail the lane explicitly if the blocking task panicked or owner mutex became unavailable
        match completed {
            Ok(Ok(Some(completed))) => {
                let _result = completion.send(Ok(completed));
            }
            Ok(Ok(None)) => {
                let _result = completion.send(Err(NativeEdgePostResponseExecutionError::Worker));
            }
            Ok(Err(())) | Err(_) => {
                let _result = completion.send(Err(NativeEdgePostResponseExecutionError::Worker));
                return;
            }
        }
    }
    guard.finish();
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;

    #[derive(Clone, Default)]
    struct FixtureStore;

    impl ForecastResponseEvidenceStore for FixtureStore {
        type Prepared = ();
        type Error = Infallible;

        /// Prepare one inert owned fixture value.
        fn prepare(
            &mut self,
            _source: weather_edge::ForecastCaptureSource<'_>,
            _window: Option<weather_edge::ForecastEvidenceWindow>,
            _version: weather_edge::ForecastCaptureVersion,
        ) -> Option<Self::Prepared> {
            Some(())
        }

        /// Return one ordinary committed fixture outcome.
        fn commit(
            &mut self,
            _prepared: &Self::Prepared,
            _first_edge_committed_at: &str,
        ) -> Result<weather_edge::ForecastEvidenceOutcome, Self::Error> {
            Ok(weather_edge::ForecastEvidenceOutcome::Created)
        }

        /// Observe an unused fixture close.
        fn note_close_without_finish(&mut self) {}

        /// Observe an unused fixture failure.
        fn note_write_failure(&mut self) {}
    }

    struct FixtureArchive;

    impl ForecastScheduledEvidenceArchive<()> for FixtureArchive {
        type Error = Infallible;

        /// Return one ordinary settled archive fixture outcome.
        fn append_committed(
            &mut self,
            _due_key: &str,
            _prepared: &(),
        ) -> Result<weather_edge::ForecastArchiveAppendOutcome, Self::Error> {
            Ok(weather_edge::ForecastArchiveAppendOutcome::Settled)
        }
    }

    /// Verify an empty lane closes and drains without inventing a queue capacity.
    #[test]
    fn empty_post_response_lane_drains() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            let lane = NativeEdgePostResponseLane::<FixtureStore, FixtureArchive>::new(
                FixtureStore,
                NativeEdgePostResponseArchive::Disabled,
            );
            assert_eq!(lane.close_and_drain().await, Ok(()));
        });
    }
}
