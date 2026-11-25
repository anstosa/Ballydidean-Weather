//! One real native timer lane for the existing serialized forecast scheduler.

use std::{
    fmt,
    future::poll_fn,
    task::{Context, Poll},
    time::Duration,
};

use tokio::time::{Instant, Interval, MissedTickBehavior, interval_at};
use weather_edge::{
    ForecastSchedulerPollDriver, ForecastSchedulerPollStep, ForecastSchedulerPollTarget,
    ForecastSchedulerPollTimerPort, ForecastSchedulerPublicReadyStarter,
};

/// Closed native timer installation failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeForecastSchedulerTimerError {
    RuntimeNotEntered,
    InvalidCadence,
    AlreadyInstalled,
}

impl fmt::Display for NativeForecastSchedulerTimerError {
    /// Report a closed scheduler setup failure without runtime internals.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeNotEntered => formatter.write_str("native scheduler runtime not entered"),
            Self::InvalidCadence => formatter.write_str("native scheduler cadence invalid"),
            Self::AlreadyInstalled => {
                formatter.write_str("native scheduler interval already installed")
            }
        }
    }
}

impl std::error::Error for NativeForecastSchedulerTimerError {}

/// One admitted callback ready to yield to the original poll driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeForecastSchedulerEvent {
    Immediate,
    Interval,
}

/// Process-local token identifying one owned interval installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeForecastSchedulerInterval(u64);

/// The real Tokio clock retains one interval and one pending immediate event.
///
/// Installation requires the time-enabled runtime owned by `NativeBackendRuntime`.
/// Entering an arbitrary Tokio runtime without its time driver is a caller programming
/// error, not a recoverable scheduler setup refusal.
#[derive(Default)]
pub struct NativeForecastSchedulerTimer {
    next_generation: u64,
    immediate: bool,
    interval: Option<(NativeForecastSchedulerInterval, Interval)>,
}

impl NativeForecastSchedulerTimer {
    /// Create an inactive timer without registering any runtime work.
    pub fn new() -> Self {
        Self::default()
    }

    /// Poll one real callback without borrowing the scheduler owner across an await.
    pub fn poll_next_event(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Option<NativeForecastSchedulerEvent>> {
        // consume the deferred first observation only after readiness has returned
        if std::mem::take(&mut self.immediate) {
            return Poll::Ready(Some(NativeForecastSchedulerEvent::Immediate));
        }
        // no timer may keep an inactive process lane alive
        let Some((_, interval)) = self.interval.as_mut() else {
            return Poll::Ready(None);
        };
        interval
            .poll_tick(context)
            .map(|_| Some(NativeForecastSchedulerEvent::Interval))
    }

    /// Wait for one real callback when the caller uniquely owns the timer.
    pub async fn next_event(&mut self) -> Option<NativeForecastSchedulerEvent> {
        poll_fn(|context| self.poll_next_event(context)).await
    }
}

impl ForecastSchedulerPollTimerPort for NativeForecastSchedulerTimer {
    type Interval = NativeForecastSchedulerInterval;
    type Error = NativeForecastSchedulerTimerError;

    /// Retain a single immediate callback for the caller-owned scheduler lane.
    fn schedule_immediate_callback(&mut self) {
        self.immediate = true;
    }

    /// Register the original fixed cadence on the selected process Tokio runtime.
    fn install_unreferenced_interval(
        &mut self,
        cadence: Duration,
    ) -> Result<Self::Interval, Self::Error> {
        // do not replace an interval that the original driver still owns
        if self.interval.is_some() {
            return Err(Self::Error::AlreadyInstalled);
        }
        // reject setup outside the caller-owned runtime before creating any timer
        if tokio::runtime::Handle::try_current().is_err() {
            self.immediate = false;
            return Err(Self::Error::RuntimeNotEntered);
        }
        // start the regular cadence after the separately deferred immediate callback
        let Some(first) = Instant::now()
            .checked_add(cadence)
            .filter(|_| !cadence.is_zero())
        else {
            self.immediate = false;
            return Err(Self::Error::InvalidCadence);
        };
        // the process runtime owns the enabled time driver; unexpected panic stays visible
        let mut interval = interval_at(first, cadence);
        self.next_generation = self.next_generation.wrapping_add(1);
        let token = NativeForecastSchedulerInterval(self.next_generation);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        self.interval = Some((token, interval));
        Ok(token)
    }

    /// Drop only the timer installed by the selected poll driver.
    fn clear_unreferenced_interval(&mut self, interval: Self::Interval) {
        // a stale clear cannot cancel another generation
        if self
            .interval
            .as_ref()
            .is_some_and(|(held, _)| *held == interval)
        {
            self.interval = None;
            self.immediate = false;
        }
    }
}

/// Process-owned poll lane for an edge's independent scheduler timer.
///
/// Implementations must not retain an application borrow across pending work, and must use
/// bounded execution for due work. The HTTP process does not detach or queue timer callbacks.
pub trait NativeForecastSchedulerLane {
    /// Poll the next event and report completion only when this lane is inactive.
    fn poll_scheduler(&mut self, context: &mut Context<'_>) -> Poll<()>;

    /// Release any remaining process-local timer admission.
    fn stop_scheduler(&mut self);

    /// Drain any already admitted async due operation after polling has been stopped.
    fn poll_drained(&mut self, _: &mut Context<'_>) -> Poll<()> {
        Poll::Ready(())
    }
}

/// Retain the real interval and original serialized driver with the selected scheduler target.
///
/// No task is detached here. The process must drive this owner in its admitted scheduler lane,
/// which also owns any potentially blocking due work.
pub struct NativeForecastSchedulerRuntime<Target: ForecastSchedulerPollTarget> {
    driver: ForecastSchedulerPollDriver<NativeForecastSchedulerInterval>,
    timer: NativeForecastSchedulerTimer,
    target: Target,
}

impl<Target: ForecastSchedulerPollTarget> NativeForecastSchedulerRuntime<Target> {
    /// Attach the initialized scheduler and keep the timer inactive until listener readiness.
    pub fn new(target: Target) -> Self {
        Self {
            driver: ForecastSchedulerPollDriver::new(),
            timer: NativeForecastSchedulerTimer::new(),
            target,
        }
    }

    /// Poll the next event and run due work only when its real timer is ready.
    pub fn poll_next_tick(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Option<ForecastSchedulerPollStep<Target::Tick, Target::Error>>> {
        let event = match self.timer.poll_next_event(context) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Ready(Some(event)) => event,
        };
        // keep the deferred and recurring callbacks in the original driver
        Poll::Ready(Some(match event {
            NativeForecastSchedulerEvent::Immediate => self
                .driver
                .immediate_elapsed(&mut self.target, &mut self.timer),
            NativeForecastSchedulerEvent::Interval => self
                .driver
                .interval_elapsed(&mut self.target, &mut self.timer),
        }))
    }

    /// Dispatch one scheduled event when the caller uniquely owns the scheduler lane.
    pub async fn next_tick(
        &mut self,
    ) -> Option<ForecastSchedulerPollStep<Target::Tick, Target::Error>> {
        poll_fn(|context| self.poll_next_tick(context)).await
    }

    /// Observe the retained scheduler target without exposing timer state.
    pub const fn target(&self) -> &Target {
        &self.target
    }

    /// Release the original polling admission and drop the process-local interval.
    pub fn stop(&mut self) {
        self.driver.stop(&mut self.target, &mut self.timer);
    }
}

impl<Target: ForecastSchedulerPollTarget> ForecastSchedulerPublicReadyStarter
    for NativeForecastSchedulerRuntime<Target>
{
    type Error = NativeForecastSchedulerTimerError;

    /// Start the original driver from the real public socket readiness event.
    fn start_on_public_ready(&mut self) -> Result<bool, Self::Error> {
        self.driver
            .start_on_public_ready(&mut self.target, &mut self.timer)
    }
}

impl<Target: ForecastSchedulerPollTarget> NativeForecastSchedulerLane
    for NativeForecastSchedulerRuntime<Target>
{
    /// Retain one timer callback at a time in the process's scheduler lane.
    fn poll_scheduler(&mut self, context: &mut Context<'_>) -> Poll<()> {
        match self.poll_next_tick(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(()),
            Poll::Ready(Some(_step)) => {
                // contain one due result like the original scheduler callback and poll the next timer later
                context.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    /// Stop the process timer and prevent any new due work.
    fn stop_scheduler(&mut self) {
        self.stop();
    }
}

impl<Target: ForecastSchedulerPollTarget> Drop for NativeForecastSchedulerRuntime<Target> {
    /// Release only this driver's polling ownership when its entire process lane is dropped.
    fn drop(&mut self) {
        // an inactive driver must not stop a previously owned external target
        if self.driver.has_interval() || self.driver.has_pending_immediate() {
            self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Target {
        polling: bool,
        ticks: usize,
    }

    impl ForecastSchedulerPollTarget for Target {
        type Tick = usize;
        type Error = ();

        /// Admit the fixture owner only once.
        fn start_polling(&mut self) -> bool {
            !std::mem::replace(&mut self.polling, true)
        }

        /// Count dispatches rather than timer registrations.
        fn run_due(&mut self) -> Result<usize, ()> {
            self.ticks += 1;
            Ok(self.ticks)
        }

        /// Report the current fixture's polling state.
        fn is_polling(&self) -> bool {
            self.polling
        }

        /// Release the fixture's original admission.
        fn stop_polling(&mut self) {
            self.polling = false;
        }
    }

    /// Prove readiness only queues work and stop clears the fixed real interval.
    #[test]
    fn native_scheduler_runtime_defers_immediate_work_until_after_ready() {
        let runtime = crate::NativeBackendRuntime::new().unwrap();
        runtime.block_on(async {
            let mut scheduler = NativeForecastSchedulerRuntime::new(Target::default());
            assert_eq!(scheduler.start_on_public_ready(), Ok(true));
            assert_eq!(scheduler.start_on_public_ready(), Ok(false));
            assert_eq!(scheduler.target().ticks, 0);
            assert_eq!(
                scheduler.next_tick().await,
                Some(ForecastSchedulerPollStep::Completed(Ok(1)))
            );
            scheduler.stop();
            assert_eq!(scheduler.next_tick().await, None);
            assert!(!scheduler.target().polling);
        });
    }

    /// Roll back ordinary scheduler admission when no process runtime has been entered.
    #[test]
    fn native_scheduler_runtime_rolls_back_outside_runtime() {
        let mut scheduler = NativeForecastSchedulerRuntime::new(Target::default());
        assert_eq!(
            scheduler.start_on_public_ready(),
            Err(NativeForecastSchedulerTimerError::RuntimeNotEntered)
        );
        assert!(!scheduler.target().polling);
    }

    /// Drive one actual elapsed interval and clear it using its exact generation token.
    #[test]
    fn native_scheduler_timer_emits_real_elapsed_interval() {
        let runtime = crate::NativeBackendRuntime::new().unwrap();
        runtime.block_on(async {
            let mut timer = NativeForecastSchedulerTimer::new();
            let start = Instant::now();
            let token = timer
                .install_unreferenced_interval(Duration::from_millis(10))
                .unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), timer.next_event())
                    .await
                    .unwrap(),
                Some(NativeForecastSchedulerEvent::Interval)
            );
            assert!(start.elapsed() >= Duration::from_millis(5));
            timer.clear_unreferenced_interval(token);
            assert_eq!(timer.next_event().await, None);
        });
    }
}
