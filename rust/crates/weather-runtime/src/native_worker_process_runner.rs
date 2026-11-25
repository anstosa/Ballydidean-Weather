//! Real current-thread timer ownership and graceful retained worker execution.

use std::{
    cell::RefCell,
    error::Error,
    fmt::{self, Display, Formatter},
    future::{Future, poll_fn},
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

use tokio::time::{Instant, Sleep, sleep_until};
use weather_worker::{
    RunningWorkerProcess, WorkerProcessBackfillPort, WorkerProcessFailure, WorkerProcessIteration,
    WorkerProcessPhase, WorkerProcessPoll, WorkerProcessPool, WorkerProcessSchedulerPort,
    WorkerProcessTrigger,
};

use crate::worker_scheduler::WorkerProcessOneShotTimer;

#[derive(Default)]
struct NativeWorkerProcessTimerState {
    event_waker: Option<Waker>,
    sleep: Option<Pin<Box<Sleep>>>,
}

/// Scheduler-owned handle for one real current-thread Tokio timer slot.
pub struct NativeWorkerProcessOneShotTimer {
    state: Rc<RefCell<NativeWorkerProcessTimerState>>,
}

/// Event-loop owner that observes the scheduler's real one-shot timer.
pub struct NativeWorkerProcessTimerEvents {
    state: Rc<RefCell<NativeWorkerProcessTimerState>>,
}

impl NativeWorkerProcessOneShotTimer {
    /// Create separate scheduler and event-loop owners for one inactive slot.
    pub fn new() -> (Self, NativeWorkerProcessTimerEvents) {
        let state = Rc::new(RefCell::new(NativeWorkerProcessTimerState::default()));
        (
            Self {
                state: Rc::clone(&state),
            },
            NativeWorkerProcessTimerEvents { state },
        )
    }
}

impl WorkerProcessOneShotTimer for NativeWorkerProcessOneShotTimer {
    /// Arm exactly one deferred callback on the process-owned time driver.
    fn arm(&mut self, delay_ms: u64) -> Result<(), WorkerProcessFailure> {
        let mut state = self.state.borrow_mut();

        // never replace the scheduler's one outstanding slot
        if state.sleep.is_some() {
            return Err(worker_scheduler_failure(
                "native_worker_timer_already_armed",
            ));
        }
        // reject registration outside the selected native process runtime
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(worker_scheduler_failure(
                "native_worker_timer_runtime_not_entered",
            ));
        }
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(delay_ms))
            .ok_or_else(|| worker_scheduler_failure("native_worker_timer_deadline_overflow"))?;

        // the native backend runtime guarantees an enabled time driver
        state.sleep = Some(Box::pin(sleep_until(deadline)));
        Ok(())
    }

    /// Cancel and wake observation of only the currently armed slot.
    fn cancel(&mut self) {
        let waker = {
            let mut state = self.state.borrow_mut();
            state.sleep = None;
            state.event_waker.take()
        };

        // let a pending event owner observe that its slot became inactive
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl Drop for NativeWorkerProcessOneShotTimer {
    /// Release a retained slot when startup or process ownership ends unexpectedly.
    fn drop(&mut self) {
        self.cancel();
    }
}

impl NativeWorkerProcessTimerEvents {
    /// Poll one elapsed slot and clear it before the process rearms its successor.
    pub fn poll_elapsed(&mut self, context: &mut Context<'_>) -> Poll<Option<()>> {
        let mut state = self.state.borrow_mut();

        // an inactive scheduler has no event capable of retaining this process lane
        let Some(sleep) = state.sleep.as_mut() else {
            state.event_waker = None;
            return Poll::Ready(None);
        };
        // retain only the current observer while the real timer remains pending
        if sleep.as_mut().poll(context).is_pending() {
            state.event_waker = Some(context.waker().clone());
            return Poll::Pending;
        }
        state.sleep = None;
        state.event_waker = None;
        Poll::Ready(Some(()))
    }
}

/// Counts every real timer delivery and its contained worker disposition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeWorkerProcessReport {
    pub initial_trigger: WorkerProcessTrigger,
    pub polls: u64,
    pub busy_polls: u64,
    pub completed_polls: u64,
    pub failed_polls: u64,
}

impl NativeWorkerProcessReport {
    /// Begin reporting from the startup iteration already owned by the process.
    const fn new(initial_trigger: WorkerProcessTrigger) -> Self {
        Self {
            initial_trigger,
            polls: 0,
            busy_polls: 0,
            completed_polls: 0,
            failed_polls: 0,
        }
    }

    /// Record exactly one disposition returned by one elapsed timer poll.
    fn record(&mut self, outcome: WorkerProcessPoll) {
        self.polls = self.polls.saturating_add(1);

        // retain bounded counts instead of per-tick allocations
        match outcome {
            WorkerProcessPoll::Busy => self.busy_polls = self.busy_polls.saturating_add(1),
            WorkerProcessPoll::Completed => {
                self.completed_polls = self.completed_polls.saturating_add(1);
            }
            WorkerProcessPoll::Failed => {
                self.failed_polls = self.failed_polls.saturating_add(1);
            }
        }
    }
}

/// Terminal retained-worker failure with partial counts and shutdown outcome preserved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeWorkerProcessRunError {
    pub report: NativeWorkerProcessReport,
    pub process_failure: Option<WorkerProcessFailure>,
    pub shutdown_failure: Option<WorkerProcessFailure>,
}

impl Display for NativeWorkerProcessRunError {
    /// Report both independent terminal phases when shutdown also fails.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // preserve both failures in the bounded process diagnostic
        match (&self.process_failure, &self.shutdown_failure) {
            (Some(process), Some(shutdown)) => {
                write!(
                    formatter,
                    "native worker failed: {process}; shutdown failed: {shutdown}"
                )
            }
            (Some(process), None) => write!(formatter, "native worker failed: {process}"),
            (None, Some(shutdown)) => {
                write!(formatter, "native worker shutdown failed: {shutdown}")
            }
            (None, None) => formatter.write_str("native worker failed without a retained cause"),
        }
    }
}

impl Error for NativeWorkerProcessRunError {
    /// Expose the first retained typed failure as the error source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.process_failure
            .as_ref()
            .or(self.shutdown_failure.as_ref())
            .map(|failure| failure as &(dyn Error + 'static))
    }
}

enum NativeWorkerProcessEvent {
    Shutdown,
    TimerElapsed,
    TimerInactive,
}

struct NativeWorkerProcessShutdownGuard<
    'a,
    Configuration,
    Pool,
    Scheduler,
    Backfill,
    Iteration,
    Handle,
> where
    Backfill: WorkerProcessBackfillPort<Configuration, Pool, Handle = Handle>,
    Iteration: WorkerProcessIteration<Configuration, Pool>,
    Pool: WorkerProcessPool<Configuration>,
    Scheduler: WorkerProcessSchedulerPort,
{
    attempted: bool,
    process: &'a RunningWorkerProcess<Configuration, Pool, Scheduler, Backfill, Iteration, Handle>,
}

impl<'a, Configuration, Pool, Scheduler, Backfill, Iteration, Handle>
    NativeWorkerProcessShutdownGuard<
        'a,
        Configuration,
        Pool,
        Scheduler,
        Backfill,
        Iteration,
        Handle,
    >
where
    Backfill: WorkerProcessBackfillPort<Configuration, Pool, Handle = Handle>,
    Iteration: WorkerProcessIteration<Configuration, Pool>,
    Pool: WorkerProcessPool<Configuration>,
    Scheduler: WorkerProcessSchedulerPort,
{
    /// Retain cancellation-safe shutdown ownership without moving process state.
    const fn new(
        process: &'a RunningWorkerProcess<
            Configuration,
            Pool,
            Scheduler,
            Backfill,
            Iteration,
            Handle,
        >,
    ) -> Self {
        Self {
            attempted: false,
            process,
        }
    }

    /// Attempt ordered shutdown once and preserve its typed result.
    fn shutdown(&mut self) -> Result<(), WorkerProcessFailure> {
        self.attempted = true;
        self.process.shutdown()
    }
}

impl<Configuration, Pool, Scheduler, Backfill, Iteration, Handle> Drop
    for NativeWorkerProcessShutdownGuard<
        '_,
        Configuration,
        Pool,
        Scheduler,
        Backfill,
        Iteration,
        Handle,
    >
where
    Backfill: WorkerProcessBackfillPort<Configuration, Pool, Handle = Handle>,
    Iteration: WorkerProcessIteration<Configuration, Pool>,
    Pool: WorkerProcessPool<Configuration>,
    Scheduler: WorkerProcessSchedulerPort,
{
    /// Shut down retained resources when the runner future itself is cancelled.
    fn drop(&mut self) {
        // never retry a pool close that already returned a terminal failure
        if !self.attempted {
            let _ = self.shutdown();
        }
    }
}

/// Drive one retained normal-mode worker until shutdown while giving shutdown priority.
///
/// `WorkerProcessStartResult::Once` remains a separately closed compatibility result and must not
/// enter this timer loop.
pub async fn run_native_worker_process<
    Configuration,
    Pool,
    Scheduler,
    Backfill,
    Iteration,
    Handle,
    Shutdown,
>(
    process: RunningWorkerProcess<Configuration, Pool, Scheduler, Backfill, Iteration, Handle>,
    mut timer_events: NativeWorkerProcessTimerEvents,
    shutdown: Shutdown,
) -> Result<NativeWorkerProcessReport, NativeWorkerProcessRunError>
where
    Backfill: WorkerProcessBackfillPort<Configuration, Pool, Handle = Handle>,
    Iteration: WorkerProcessIteration<Configuration, Pool>,
    Pool: WorkerProcessPool<Configuration>,
    Scheduler: WorkerProcessSchedulerPort,
    Shutdown: Future<Output = ()>,
{
    let mut guard = NativeWorkerProcessShutdownGuard::new(&process);
    let mut shutdown = Box::pin(shutdown);
    let mut report = NativeWorkerProcessReport::new(process.initial_trigger());

    // drive only one callback at a time and stop on the first terminal lane event
    let process_failure = loop {
        let event = poll_fn(|context| {
            // a simultaneously ready shutdown always wins over a timer callback
            if shutdown.as_mut().poll(context).is_ready() {
                return Poll::Ready(NativeWorkerProcessEvent::Shutdown);
            }
            // distinguish a consumed timer from an unexpectedly inactive scheduler
            match timer_events.poll_elapsed(context) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Some(())) => Poll::Ready(NativeWorkerProcessEvent::TimerElapsed),
                Poll::Ready(None) => Poll::Ready(NativeWorkerProcessEvent::TimerInactive),
            }
        })
        .await;

        // execute exactly one retained process poll for each elapsed timer
        match event {
            NativeWorkerProcessEvent::Shutdown => break None,
            NativeWorkerProcessEvent::TimerInactive => {
                break Some(worker_scheduler_failure("native_worker_timer_inactive"));
            }
            NativeWorkerProcessEvent::TimerElapsed => match process.poll() {
                Ok(outcome) => report.record(outcome),
                Err(failure) => break Some(failure),
            },
        }
    };
    let shutdown_failure = guard.shutdown().err();

    // return counts only after the scheduler, work, and pool have been drained
    if process_failure.is_none() && shutdown_failure.is_none() {
        Ok(report)
    } else {
        Err(NativeWorkerProcessRunError {
            report,
            process_failure,
            shutdown_failure,
        })
    }
}

/// Construct one bounded failure owned by the scheduler phase.
fn worker_scheduler_failure(code: &str) -> WorkerProcessFailure {
    WorkerProcessFailure {
        code: code.to_owned(),
        phase: WorkerProcessPhase::Scheduler,
    }
}
