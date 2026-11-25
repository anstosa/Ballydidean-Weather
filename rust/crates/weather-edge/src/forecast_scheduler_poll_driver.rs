//! Drive the native forecast scheduler from public readiness and one fixed interval.

use std::cell::Cell;
use std::time::Duration;

use crate::forecast_scheduler_due::ForecastSchedulerCalendarPort;
use crate::native_revision_spool::{
    FORECAST_SCHEDULER_POLL_INTERVAL, ForecastAllocationStatfsPort, ForecastSchedulerPorts,
    ForecastSchedulerSlotsPort, ForecastSchedulerStateCodec, ForecastSchedulerTick,
    ForecastSchedulerTickError, ForecastSchedulerTriggerPort, NativeForecastScheduler,
};
use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec,
};

/// Timer ownership needed by the process event loop without keeping the process alive by itself.
pub trait ForecastSchedulerPollTimerPort {
    type Interval;
    type Error;

    /// Schedule one detached callback whose event invokes `immediate_elapsed`.
    fn schedule_immediate_callback(&mut self);

    /// Install one unreferenced fixed interval whose callback invokes `interval_elapsed`.
    fn install_unreferenced_interval(
        &mut self,
        cadence: Duration,
    ) -> Result<Self::Interval, Self::Error>;

    /// Clear only the interval previously returned by this timer owner.
    fn clear_unreferenced_interval(&mut self, interval: Self::Interval);
}

/// Scheduler operations consumed by the interval driver.
pub trait ForecastSchedulerPollTarget {
    type Tick;
    type Error;

    /// Admit polling once after scheduler initialization.
    fn start_polling(&mut self) -> bool;

    /// Run one serialized due observation.
    fn run_due(&mut self) -> Result<Self::Tick, Self::Error>;

    /// Report whether the scheduler still owns its polling admission.
    fn is_polling(&self) -> bool;

    /// Stop only the process-local polling admission.
    fn stop_polling(&mut self);
}

/// Borrow the real native scheduler together with every port needed by one due observation.
pub struct NativeForecastSchedulerPollTarget<
    'borrow,
    'ports,
    Codec,
    Calendar,
    Clock,
    Slots,
    Statfs,
    Trigger,
> {
    scheduler: &'borrow mut NativeForecastScheduler,
    ports:
        &'borrow mut ForecastSchedulerPorts<'ports, Codec, Calendar, Clock, Slots, Statfs, Trigger>,
}

impl<'borrow, 'ports, Codec, Calendar, Clock, Slots, Statfs, Trigger>
    NativeForecastSchedulerPollTarget<
        'borrow,
        'ports,
        Codec,
        Calendar,
        Clock,
        Slots,
        Statfs,
        Trigger,
    >
{
    /// Bind one native scheduler to its already initialized process-local ports.
    pub fn new(
        scheduler: &'borrow mut NativeForecastScheduler,
        ports: &'borrow mut ForecastSchedulerPorts<
            'ports,
            Codec,
            Calendar,
            Clock,
            Slots,
            Statfs,
            Trigger,
        >,
    ) -> Self {
        Self { scheduler, ports }
    }
}

impl<Codec, Calendar, Clock, Slots, Statfs, Trigger> ForecastSchedulerPollTarget
    for NativeForecastSchedulerPollTarget<'_, '_, Codec, Calendar, Clock, Slots, Statfs, Trigger>
where
    Codec: RevisionArchiveCanonicalCodec + ForecastSchedulerStateCodec,
    Calendar: ForecastSchedulerCalendarPort,
    Clock: RevisionArchiveCanonicalClock,
    Slots: ForecastSchedulerSlotsPort,
    Statfs: ForecastAllocationStatfsPort,
    Trigger: ForecastSchedulerTriggerPort,
{
    type Tick = ForecastSchedulerTick;
    type Error = ForecastSchedulerTickError;

    /// Delegate polling admission to the native scheduler owner.
    fn start_polling(&mut self) -> bool {
        self.scheduler.start_polling()
    }

    /// Delegate one due observation through the bound native ports.
    fn run_due(&mut self) -> Result<Self::Tick, Self::Error> {
        self.scheduler.run_due(self.ports)
    }

    /// Read the native scheduler's current polling admission.
    fn is_polling(&self) -> bool {
        self.scheduler.is_polling()
    }

    /// Release the native scheduler's process-local polling admission.
    fn stop_polling(&mut self) {
        self.scheduler.stop_polling();
    }
}

/// Result of one interval callback without retaining or queueing a missed callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastSchedulerPollStep<Tick, Error> {
    NotPolling,
    SkippedAlreadyRunning,
    Completed(Result<Tick, Error>),
}

/// Own exactly one interval and serialize every immediate or recurring scheduler tick.
pub struct ForecastSchedulerPollDriver<Interval> {
    immediate_pending: bool,
    interval: Option<Interval>,
    tick_running: Cell<bool>,
}

impl<Interval> Default for ForecastSchedulerPollDriver<Interval> {
    /// Create an inactive driver without allocating a timer or process resource.
    fn default() -> Self {
        Self {
            immediate_pending: false,
            interval: None,
            tick_running: Cell::new(false),
        }
    }
}

impl<Interval> ForecastSchedulerPollDriver<Interval> {
    /// Create an inactive driver for one scheduler owner.
    pub fn new() -> Self {
        Self::default()
    }

    /// Submit a detached immediate callback, then install the exact fixed cadence.
    pub fn start_on_public_ready<Target, Timer>(
        &mut self,
        target: &mut Target,
        timer: &mut Timer,
    ) -> Result<bool, Timer::Error>
    where
        Target: ForecastSchedulerPollTarget,
        Timer: ForecastSchedulerPollTimerPort<Interval = Interval>,
    {
        // preserve false parity for an installed timer or an inactive native scheduler
        if self.interval.is_some() || !target.start_polling() {
            return Ok(false);
        }

        // enqueue without running or awaiting scheduler work inside listener readiness
        timer.schedule_immediate_callback();
        self.immediate_pending = true;

        // roll native admission back when the process timer cannot be installed
        match timer.install_unreferenced_interval(FORECAST_SCHEDULER_POLL_INTERVAL) {
            Ok(interval) => {
                self.interval = Some(interval);
                Ok(true)
            }
            Err(error) => {
                self.immediate_pending = false;
                target.stop_polling();
                Err(error)
            }
        }
    }

    /// Run the one detached immediate callback after public readiness has returned.
    pub fn immediate_elapsed<Target, Timer>(
        &mut self,
        target: &mut Target,
        timer: &mut Timer,
    ) -> ForecastSchedulerPollStep<Target::Tick, Target::Error>
    where
        Target: ForecastSchedulerPollTarget,
        Timer: ForecastSchedulerPollTimerPort<Interval = Interval>,
    {
        // ignore stale executor callbacks after stop or failed timer installation
        if !self.immediate_pending || !target.is_polling() {
            self.immediate_pending = false;
            return ForecastSchedulerPollStep::NotPolling;
        }
        self.immediate_pending = false;

        let Some(result) = self.run_serialized(target) else {
            return ForecastSchedulerPollStep::SkippedAlreadyRunning;
        };
        // clear the installed interval after an immediate terminal observation
        if !target.is_polling() {
            self.clear_interval(timer);
        }
        ForecastSchedulerPollStep::Completed(result)
    }

    /// Run one interval callback, skipping overlap instead of retaining a callback queue.
    pub fn interval_elapsed<Target, Timer>(
        &mut self,
        target: &mut Target,
        timer: &mut Timer,
    ) -> ForecastSchedulerPollStep<Target::Tick, Target::Error>
    where
        Target: ForecastSchedulerPollTarget,
        Timer: ForecastSchedulerPollTimerPort<Interval = Interval>,
    {
        // synchronize a stale timer handle with a terminal native scheduler
        if self.interval.is_none() || !target.is_polling() {
            self.clear_interval(timer);
            return ForecastSchedulerPollStep::NotPolling;
        }

        let result = {
            // discard this callback while the preceding due observation still owns the lane
            let Some(_running) = PollTickGuard::enter(&self.tick_running) else {
                return ForecastSchedulerPollStep::SkippedAlreadyRunning;
            };
            target.run_due()
        };

        // clear the process interval when the native terminal tick relinquishes polling
        if !target.is_polling() {
            self.clear_interval(timer);
        }
        ForecastSchedulerPollStep::Completed(result)
    }

    /// Stop owner polling and clear the one process-local interval if installed.
    pub fn stop<Target, Timer>(&mut self, target: &mut Target, timer: &mut Timer)
    where
        Target: ForecastSchedulerPollTarget,
        Timer: ForecastSchedulerPollTimerPort<Interval = Interval>,
    {
        target.stop_polling();
        self.immediate_pending = false;
        self.clear_interval(timer);
    }

    /// Report whether this runtime driver currently retains an interval handle.
    pub const fn has_interval(&self) -> bool {
        self.interval.is_some()
    }

    /// Report whether readiness submitted an immediate callback not yet consumed.
    pub const fn has_pending_immediate(&self) -> bool {
        self.immediate_pending
    }

    /// Run one tick under the non-queueing process-local admission.
    fn run_serialized<Target>(
        &self,
        target: &mut Target,
    ) -> Option<Result<Target::Tick, Target::Error>>
    where
        Target: ForecastSchedulerPollTarget,
    {
        // skip rather than queue a callback behind an existing tick
        let _running = PollTickGuard::enter(&self.tick_running)?;
        Some(target.run_due())
    }

    /// Clear and forget only the interval installed by this driver.
    fn clear_interval<Timer>(&mut self, timer: &mut Timer)
    where
        Timer: ForecastSchedulerPollTimerPort<Interval = Interval>,
    {
        // avoid manufacturing a clear call before readiness or after terminal cleanup
        if let Some(interval) = self.interval.take() {
            timer.clear_unreferenced_interval(interval);
        }
    }
}

/// No-argument readiness action used by the edge startup lifecycle callback.
pub trait ForecastSchedulerPublicReadyStarter {
    type Error;

    /// Start the scheduler only after the public listener readiness event.
    fn start_on_public_ready(&mut self) -> Result<bool, Self::Error>;
}

/// Borrow one driver, target, and timer as a typed public-readiness action.
pub struct BorrowedForecastSchedulerPublicReadyStarter<'a, Interval, Target, Timer> {
    driver: &'a mut ForecastSchedulerPollDriver<Interval>,
    target: &'a mut Target,
    timer: &'a mut Timer,
}

impl<'a, Interval, Target, Timer>
    BorrowedForecastSchedulerPublicReadyStarter<'a, Interval, Target, Timer>
{
    /// Bind the exact process owners needed by one public readiness callback.
    pub fn new(
        driver: &'a mut ForecastSchedulerPollDriver<Interval>,
        target: &'a mut Target,
        timer: &'a mut Timer,
    ) -> Self {
        Self {
            driver,
            target,
            timer,
        }
    }
}

impl<Interval, Target, Timer> ForecastSchedulerPublicReadyStarter
    for BorrowedForecastSchedulerPublicReadyStarter<'_, Interval, Target, Timer>
where
    Target: ForecastSchedulerPollTarget,
    Timer: ForecastSchedulerPollTimerPort<Interval = Interval>,
{
    type Error = Timer::Error;

    /// Delegate the readiness event without exposing timer internals to startup.
    fn start_on_public_ready(&mut self) -> Result<bool, Self::Error> {
        self.driver.start_on_public_ready(self.target, self.timer)
    }
}

/// Reset one in-progress marker even when a due observation unwinds.
struct PollTickGuard<'a>(&'a Cell<bool>);

impl<'a> PollTickGuard<'a> {
    /// Enter only an idle tick lane without retaining a pending callback.
    fn enter(running: &'a Cell<bool>) -> Option<Self> {
        // refuse overlap while leaving no queued work behind
        if running.replace(true) {
            return None;
        }
        Some(Self(running))
    }
}

impl Drop for PollTickGuard<'_> {
    /// Release the tick lane after every ordinary completion or unwind.
    fn drop(&mut self) {
        self.0.set(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Failure {
        Tick,
        Timer,
    }

    #[derive(Default)]
    struct Target {
        active: bool,
        fail_next: bool,
        polling: bool,
        run_count: usize,
        start_count: usize,
        stop_after_next: bool,
        stop_count: usize,
    }

    impl ForecastSchedulerPollTarget for Target {
        type Tick = usize;
        type Error = Failure;

        /// Admit polling once for an active fixture.
        fn start_polling(&mut self) -> bool {
            self.start_count += 1;
            // preserve false for inactive or already polling fixtures
            if !self.active || self.polling {
                return false;
            }
            self.polling = true;
            true
        }

        /// Record one deterministic due observation and its injected outcome.
        fn run_due(&mut self) -> Result<Self::Tick, Self::Error> {
            self.run_count += 1;
            // relinquish polling only after the admitted terminal observation
            if self.stop_after_next {
                self.stop_after_next = false;
                self.polling = false;
            }
            // return one contained operational tick failure when requested
            if self.fail_next {
                self.fail_next = false;
                return Err(Failure::Tick);
            }
            Ok(self.run_count)
        }

        /// Report the fixture's native polling admission.
        fn is_polling(&self) -> bool {
            self.polling
        }

        /// Release only the fixture's process-local polling admission.
        fn stop_polling(&mut self) {
            self.stop_count += 1;
            self.polling = false;
        }
    }

    #[derive(Default)]
    struct Timer {
        cleared: Vec<usize>,
        events: Vec<&'static str>,
        fail_install: bool,
        installed: Vec<Duration>,
    }

    impl ForecastSchedulerPollTimerPort for Timer {
        type Interval = usize;
        type Error = Failure;

        /// Retain one detached immediate callback submission without invoking it.
        fn schedule_immediate_callback(&mut self) {
            self.events.push("immediate");
        }

        /// Retain the exact installed cadence and return its stable test handle.
        fn install_unreferenced_interval(
            &mut self,
            cadence: Duration,
        ) -> Result<Self::Interval, Self::Error> {
            // exercise timer setup failure without manufacturing an interval handle
            if self.fail_install {
                return Err(Failure::Timer);
            }
            self.events.push("interval");
            self.installed.push(cadence);
            Ok(self.installed.len())
        }

        /// Retain one exact cleared test handle.
        fn clear_unreferenced_interval(&mut self, interval: Self::Interval) {
            self.cleared.push(interval);
        }
    }

    /// Submit immediate work without running it, then retain one exact interval.
    #[test]
    fn readiness_starts_immediate_tick_then_exact_cadence_once() {
        let mut driver = ForecastSchedulerPollDriver::new();
        let mut target = Target {
            active: true,
            ..Target::default()
        };
        let mut timer = Timer::default();
        {
            let mut starter = BorrowedForecastSchedulerPublicReadyStarter::new(
                &mut driver,
                &mut target,
                &mut timer,
            );
            assert!(starter.start_on_public_ready().unwrap());
        }
        assert_eq!(target.run_count, 0);
        assert_eq!(timer.events, ["immediate", "interval"]);
        assert_eq!(timer.installed, [Duration::from_secs(30)]);
        assert!(driver.has_interval());
        assert!(driver.has_pending_immediate());

        assert_eq!(
            driver.immediate_elapsed(&mut target, &mut timer),
            ForecastSchedulerPollStep::Completed(Ok(1))
        );
        assert!(!driver.has_pending_immediate());

        let mut starter =
            BorrowedForecastSchedulerPublicReadyStarter::new(&mut driver, &mut target, &mut timer);
        assert!(!starter.start_on_public_ready().unwrap());
        assert_eq!(target.run_count, 1);
        assert_eq!(timer.installed.len(), 1);
    }

    /// Contain ordinary tick failure and clear the interval on a later terminal tick.
    #[test]
    fn failure_keeps_polling_until_terminal_tick_clears_interval() {
        let mut driver = ForecastSchedulerPollDriver::new();
        let mut target = Target {
            active: true,
            fail_next: true,
            ..Target::default()
        };
        let mut timer = Timer::default();
        assert!(
            driver
                .start_on_public_ready(&mut target, &mut timer)
                .unwrap()
        );
        assert!(driver.has_interval());
        assert_eq!(target.run_count, 0);
        assert_eq!(
            driver.immediate_elapsed(&mut target, &mut timer),
            ForecastSchedulerPollStep::Completed(Err(Failure::Tick))
        );
        assert_eq!(target.run_count, 1);

        target.fail_next = true;
        assert_eq!(
            driver.interval_elapsed(&mut target, &mut timer),
            ForecastSchedulerPollStep::Completed(Err(Failure::Tick))
        );
        assert!(driver.has_interval());

        target.stop_after_next = true;
        assert_eq!(
            driver.interval_elapsed(&mut target, &mut timer),
            ForecastSchedulerPollStep::Completed(Ok(3))
        );
        assert!(!driver.has_interval());
        assert_eq!(timer.cleared, [1]);
    }

    /// Preserve inactive false and roll polling back after timer installation failure.
    #[test]
    fn inactive_and_timer_failure_preserve_start_parity() {
        let mut driver = ForecastSchedulerPollDriver::new();
        let mut target = Target::default();
        let mut timer = Timer::default();
        assert!(
            !driver
                .start_on_public_ready(&mut target, &mut timer)
                .unwrap()
        );
        assert_eq!(target.run_count, 0);
        assert!(timer.events.is_empty());
        assert!(timer.installed.is_empty());

        target.active = true;
        timer.fail_install = true;
        assert_eq!(
            driver.start_on_public_ready(&mut target, &mut timer),
            Err(Failure::Timer)
        );
        assert!(!target.polling);
        assert_eq!(target.stop_count, 1);
        assert!(!driver.has_interval());
        assert!(!driver.has_pending_immediate());
    }

    /// Skip overlapping callbacks without queueing and honor explicit owner stop.
    #[test]
    fn overlapping_callback_skips_and_owner_stop_clears_interval() {
        let mut driver = ForecastSchedulerPollDriver::new();
        let mut target = Target {
            active: true,
            ..Target::default()
        };
        let mut timer = Timer::default();
        assert!(
            driver
                .start_on_public_ready(&mut target, &mut timer)
                .unwrap()
        );

        driver.tick_running.set(true);
        assert_eq!(
            driver.immediate_elapsed(&mut target, &mut timer),
            ForecastSchedulerPollStep::SkippedAlreadyRunning
        );
        driver.tick_running.set(false);
        assert_eq!(target.run_count, 0);

        driver.stop(&mut target, &mut timer);
        assert!(!target.polling);
        assert_eq!(timer.cleared, [1]);
        assert_eq!(
            driver.interval_elapsed(&mut target, &mut timer),
            ForecastSchedulerPollStep::NotPolling
        );
    }
}
