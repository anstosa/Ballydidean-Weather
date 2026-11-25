//! Owned forecast scheduler ports and nonblocking execution of one due observation at a time.

use std::{
    convert::Infallible,
    future::Future,
    path::PathBuf,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::task::JoinHandle;
use weather_edge::{
    ForecastAllocationStatfsPort, ForecastSchedulerCalendarPort, ForecastSchedulerConfiguration,
    ForecastSchedulerPollDriver, ForecastSchedulerPollTarget, ForecastSchedulerPorts,
    ForecastSchedulerPublicReadyStarter, ForecastSchedulerSlotsPort, ForecastSchedulerState,
    ForecastSchedulerStateCodec, ForecastSchedulerTick, ForecastSchedulerTickError,
    ForecastSchedulerTriggerPort, NativeForecastScheduler, RevisionArchiveCanonicalClock,
    RevisionArchiveCanonicalCodec,
};

use crate::{
    NativeForecastSchedulerEvent, NativeForecastSchedulerInterval, NativeForecastSchedulerLane,
    NativeForecastSchedulerTimer, NativeForecastSchedulerTimerError,
};

/// Physical owners moved together into the one native scheduler target.
pub struct NativeOwnedForecastSchedulerPorts<Codec, Calendar, Clock, Slots, Statfs, Trigger> {
    pub codec: Codec,
    pub calendar: Calendar,
    pub clock: Clock,
    pub slots: Slots,
    pub filesystem: Statfs,
    pub trigger: Trigger,
}

/// Own all actual scheduler ports by value without retaining self-referential borrows.
pub struct NativeOwnedForecastSchedulerTarget<Codec, Calendar, Clock, Slots, Statfs, Trigger> {
    scheduler: NativeForecastScheduler,
    ports: NativeOwnedForecastSchedulerPorts<Codec, Calendar, Clock, Slots, Statfs, Trigger>,
}

impl<Codec, Calendar, Clock, Slots, Statfs, Trigger>
    NativeOwnedForecastSchedulerTarget<Codec, Calendar, Clock, Slots, Statfs, Trigger>
where
    Codec: RevisionArchiveCanonicalCodec + ForecastSchedulerStateCodec,
    Calendar: ForecastSchedulerCalendarPort,
    Clock: RevisionArchiveCanonicalClock,
    Slots: ForecastSchedulerSlotsPort,
    Statfs: ForecastAllocationStatfsPort,
    Trigger: ForecastSchedulerTriggerPort,
{
    /// Initialize the genuine recovered scheduler using the retained owning codec and ports.
    pub fn initialize(
        root: PathBuf,
        configuration: ForecastSchedulerConfiguration,
        mut ports: NativeOwnedForecastSchedulerPorts<
            Codec,
            Calendar,
            Clock,
            Slots,
            Statfs,
            Trigger,
        >,
    ) -> Self {
        let scheduler = NativeForecastScheduler::initialize(
            root,
            configuration,
            &mut ForecastSchedulerPorts {
                codec: &ports.codec,
                calendar: &ports.calendar,
                clock: &mut ports.clock,
                slots: &mut ports.slots,
                filesystem: &mut ports.filesystem,
                trigger: &mut ports.trigger,
            },
        );
        Self { scheduler, ports }
    }

    /// Borrow the exact retained scheduler status when no worker owns the target.
    pub const fn status(&self) -> &ForecastSchedulerState {
        self.scheduler.status()
    }
}

impl<Codec, Calendar, Clock, Slots, Statfs, Trigger> ForecastSchedulerPollTarget
    for NativeOwnedForecastSchedulerTarget<Codec, Calendar, Clock, Slots, Statfs, Trigger>
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

    /// Retain the source scheduler startup admission.
    fn start_polling(&mut self) -> bool {
        self.scheduler.start_polling()
    }

    /// Run exactly one due observation with operation-local borrows of real owners.
    fn run_due(&mut self) -> Result<Self::Tick, Self::Error> {
        self.scheduler.run_due(&mut ForecastSchedulerPorts {
            codec: &self.ports.codec,
            calendar: &self.ports.calendar,
            clock: &mut self.ports.clock,
            slots: &mut self.ports.slots,
            filesystem: &mut self.ports.filesystem,
            trigger: &mut self.ports.trigger,
        })
    }

    /// Read the actual retained polling admission.
    fn is_polling(&self) -> bool {
        self.scheduler.is_polling()
    }

    /// Stop future due observations through the existing scheduler policy.
    fn stop_polling(&mut self) {
        self.scheduler.stop_polling();
    }
}

struct BackgroundTarget<Target: ForecastSchedulerPollTarget> {
    idle: Option<Target>,
    running: Option<JoinHandle<Target>>,
    polling: bool,
    failed: bool,
}

impl<Target: ForecastSchedulerPollTarget + Send + 'static> BackgroundTarget<Target> {
    /// Return ownership after a blocking due operation without waiting on the socket reactor.
    fn poll_completion(&mut self, context: &mut Context<'_>) {
        let Some(task) = self.running.as_mut() else {
            return;
        };
        let result = match Pin::new(task).poll(context) {
            Poll::Pending => return,
            Poll::Ready(result) => result,
        };
        self.running = None;
        match result {
            Ok(mut target) => {
                // retain stop requests that arrived while a due operation owned the target
                if !self.polling {
                    target.stop_polling();
                }
                self.polling = target.is_polling();
                self.idle = Some(target);
            }
            Err(_) => {
                self.failed = true;
                self.polling = false;
            }
        }
    }
}

impl<Target: ForecastSchedulerPollTarget + Send + 'static> ForecastSchedulerPollTarget
    for BackgroundTarget<Target>
{
    type Tick = bool;
    type Error = Infallible;

    /// Start the actual idle native scheduler without starting a due operation on readiness.
    fn start_polling(&mut self) -> bool {
        self.polling = self
            .idle
            .as_mut()
            .is_some_and(ForecastSchedulerPollTarget::start_polling);
        self.polling
    }

    /// Start at most one real blocking due operation and discard overlaps rather than queue them.
    fn run_due(&mut self) -> Result<bool, Infallible> {
        let Some(mut target) = self.idle.take() else {
            return Ok(false);
        };
        // never wait for loopback HTTP or archive writes on the native listener reactor
        self.running = Some(tokio::task::spawn_blocking(move || {
            let _ = target.run_due();
            target
        }));
        Ok(true)
    }

    /// Preserve polling admission while the current operation owns the target.
    fn is_polling(&self) -> bool {
        self.polling
    }

    /// Stop a currently idle target or retain the stop until the running operation releases it.
    fn stop_polling(&mut self) {
        self.polling = false;
        if let Some(target) = self.idle.as_mut() {
            target.stop_polling();
        }
    }
}

/// Independent real timer and one blocking scheduler operation retained until shutdown.
pub struct NativeBackgroundForecastScheduler<Target: ForecastSchedulerPollTarget> {
    target: BackgroundTarget<Target>,
    driver: ForecastSchedulerPollDriver<NativeForecastSchedulerInterval>,
    timer: NativeForecastSchedulerTimer,
}

impl<Target: ForecastSchedulerPollTarget + Send + 'static>
    NativeBackgroundForecastScheduler<Target>
{
    /// Retain the admitted real scheduler target without installing a timer before readiness.
    pub fn new(target: Target) -> Self {
        Self {
            target: BackgroundTarget {
                idle: Some(target),
                running: None,
                polling: false,
                failed: false,
            },
            driver: ForecastSchedulerPollDriver::new(),
            timer: NativeForecastSchedulerTimer::new(),
        }
    }

    /// Borrow the retained target only when no due work owns it.
    pub const fn idle_target(&self) -> Option<&Target> {
        self.target.idle.as_ref()
    }

    /// Report the categorical blocking-worker failure without claiming a successful due observation.
    pub const fn worker_failed(&self) -> bool {
        self.target.failed
    }
}

impl<Target: ForecastSchedulerPollTarget + Send + 'static> ForecastSchedulerPublicReadyStarter
    for NativeBackgroundForecastScheduler<Target>
{
    type Error = NativeForecastSchedulerTimerError;

    /// Start original immediate and periodic callback admission after real listener readiness.
    fn start_on_public_ready(&mut self) -> Result<bool, Self::Error> {
        self.driver
            .start_on_public_ready(&mut self.target, &mut self.timer)
    }
}

impl<Target: ForecastSchedulerPollTarget + Send + 'static> NativeForecastSchedulerLane
    for NativeBackgroundForecastScheduler<Target>
{
    /// Drive the timer and completion separately without advancing a blocked due operation inline.
    fn poll_scheduler(&mut self, context: &mut Context<'_>) -> Poll<()> {
        self.target.poll_completion(context);
        // a terminal target relinquishes its interval as in the original driver
        if !self.target.polling {
            self.driver.stop(&mut self.target, &mut self.timer);
            return Poll::Ready(());
        }
        let event = match self.timer.poll_next_event(context) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(None) => return Poll::Ready(()),
            Poll::Ready(Some(event)) => event,
        };
        match event {
            NativeForecastSchedulerEvent::Immediate => {
                let _ = self
                    .driver
                    .immediate_elapsed(&mut self.target, &mut self.timer);
            }
            NativeForecastSchedulerEvent::Interval => {
                let _ = self
                    .driver
                    .interval_elapsed(&mut self.target, &mut self.timer);
            }
        }
        // poll the new join handle immediately to register its wakeup with the listener lane
        self.target.poll_completion(context);
        // arrange one timer poll after consuming the immediate event
        context.waker().wake_by_ref();
        Poll::Pending
    }

    /// Stop future callbacks without blocking the reactor on the current due request.
    fn stop_scheduler(&mut self) {
        self.driver.stop(&mut self.target, &mut self.timer);
    }

    /// Wait for already admitted due work to settle before releasing its real owners.
    fn poll_drained(&mut self, context: &mut Context<'_>) -> Poll<()> {
        self.target.poll_completion(context);
        if self.target.running.is_none() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::poll_fn, sync::mpsc, time::Duration};

    struct FixtureTarget {
        polling: bool,
        started: mpsc::SyncSender<()>,
        finish: mpsc::Receiver<()>,
    }

    impl ForecastSchedulerPollTarget for FixtureTarget {
        type Tick = ();
        type Error = Infallible;

        /// Retain ordinary fixture polling state.
        fn start_polling(&mut self) -> bool {
            self.polling = true;
            true
        }
        /// Run one finite operation whose completion is independently controlled by the test.
        fn run_due(&mut self) -> Result<(), Infallible> {
            self.started.send(()).unwrap();
            self.finish.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(())
        }
        /// Observe fixture polling state.
        fn is_polling(&self) -> bool {
            self.polling
        }
        /// Stop only the fixture's polling admission.
        fn stop_polling(&mut self) {
            self.polling = false;
        }
    }

    /// The real timer lane can return while due work blocks and then drain that owner at shutdown.
    #[test]
    fn native_background_forecast_scheduler_drains_admitted_blocking_due_work() {
        let (started, started_rx) = mpsc::sync_channel(1);
        let (finish, finish_rx) = mpsc::channel();
        let target = FixtureTarget {
            polling: false,
            started,
            finish: finish_rx,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let mut scheduler = NativeBackgroundForecastScheduler::new(target);
            assert!(scheduler.start_on_public_ready().unwrap());
            poll_fn(|context| {
                assert!(scheduler.poll_scheduler(context).is_pending());
                Poll::Ready(())
            })
            .await;
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(scheduler.idle_target().is_none());
            scheduler.stop_scheduler();
            finish.send(()).unwrap();
            poll_fn(|context| scheduler.poll_drained(context)).await;
            assert!(!scheduler.worker_failed());
            assert!(!scheduler.idle_target().unwrap().is_polling());
        });
    }
}
