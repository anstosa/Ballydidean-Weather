//! Schedule worker polls at the same wall-clock slots as Node.

use weather_worker::{
    WorkerProcessFailure, WorkerProcessPhase, WorkerProcessSchedule, WorkerProcessSchedulerPort,
    next_worker_scheduled_at_ms,
};

/// An event loop's cancellable one-shot timer delivering a worker poll.
pub trait WorkerProcessOneShotTimer {
    /// Deliver one future callback after the supplied delay, without polling synchronously.
    fn arm(&mut self, delay_ms: u64) -> Result<(), WorkerProcessFailure>;

    /// Cancel the current slot if it remains armed.
    fn cancel(&mut self);
}

#[derive(Clone, Copy)]
struct ArmedWorkerSchedule {
    cadence_ms: u64,
    digest_prefix: [u8; 4],
}

/// Own one timer and reuse the immutable jitter material while the scheduler runs.
pub struct AnchoredWorkerProcessScheduler<Clock, Timer, Digest, Diagnostics> {
    active: Option<ArmedWorkerSchedule>,
    clock: Clock,
    diagnostics: Diagnostics,
    digest: Digest,
    timer: Timer,
}

impl<Clock, Timer, Digest, Diagnostics>
    AnchoredWorkerProcessScheduler<Clock, Timer, Digest, Diagnostics>
{
    /// Bind wall-clock reads, the caller's SHA-256 owner, the timer, and diagnostics.
    pub const fn new(clock: Clock, timer: Timer, digest: Digest, diagnostics: Diagnostics) -> Self {
        Self {
            active: None,
            clock,
            diagnostics,
            digest,
            timer,
        }
    }
}

impl<Clock, Timer, Digest, Diagnostics>
    AnchoredWorkerProcessScheduler<Clock, Timer, Digest, Diagnostics>
where
    Clock: FnMut() -> i64,
    Timer: WorkerProcessOneShotTimer,
{
    /// Arm from two ordered clock reads, not the previous timer's deadline.
    fn arm_next(&mut self, schedule: ArmedWorkerSchedule) -> Result<(), WorkerProcessFailure> {
        let next_at = next_worker_scheduled_at_ms(
            (self.clock)(),
            schedule.cadence_ms,
            schedule.digest_prefix,
        )
        .map_err(|_| WorkerProcessFailure {
            code: "worker_scheduler_invalid_time".to_owned(),
            phase: WorkerProcessPhase::Scheduler,
        })?;
        // a clock tick can pass the slot before the timer is armed
        let delay_ms = u64::try_from(i128::from(next_at) - i128::from((self.clock)())).unwrap_or(0);
        self.timer.arm(delay_ms)
    }
}

impl<Clock, Timer, Digest, Diagnostics> WorkerProcessSchedulerPort
    for AnchoredWorkerProcessScheduler<Clock, Timer, Digest, Diagnostics>
where
    Clock: FnMut() -> i64,
    Diagnostics: FnMut(&WorkerProcessFailure),
    Digest: FnMut(&str) -> Result<[u8; 4], WorkerProcessFailure>,
    Timer: WorkerProcessOneShotTimer,
{
    /// Resolve jitter once per registration and arm the initial timer.
    fn start(&mut self, schedule: WorkerProcessSchedule) -> Result<(), WorkerProcessFailure> {
        // don't duplicate the pending timer when already active
        if self.active.is_some() {
            return Ok(());
        }
        let active = ArmedWorkerSchedule {
            cadence_ms: schedule.cadence_ms,
            digest_prefix: (self.digest)(&schedule.key)?,
        };
        self.arm_next(active)?;
        self.active = Some(active);
        Ok(())
    }

    /// Cancel future timer delivery and suppress any rearm after shutdown.
    fn stop(&mut self) {
        // clear the active flag before asking the timer to cancel
        if self.active.take().is_some() {
            self.timer.cancel();
        }
    }

    /// Recompute the next anchor once the current callback settles.
    fn rearm_after_poll(&mut self) -> Result<(), WorkerProcessFailure> {
        // a callback may finish after the process stops polling
        if let Some(schedule) = self.active {
            self.arm_next(schedule)?;
        }
        Ok(())
    }

    /// Forward exactly one diagnostic for an unhandled background iteration failure.
    fn record_iteration_error(&mut self, failure: &WorkerProcessFailure) {
        (self.diagnostics)(failure);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;

    struct Timer<'a>(&'a RefCell<Vec<String>>);

    impl WorkerProcessOneShotTimer for Timer<'_> {
        // track every armed delay without a real wait
        fn arm(&mut self, delay_ms: u64) -> Result<(), WorkerProcessFailure> {
            self.0.borrow_mut().push(format!("arm:{delay_ms}"));
            Ok(())
        }

        // track the cancellation of the one outstanding slot
        fn cancel(&mut self) {
            self.0.borrow_mut().push("cancel".to_owned());
        }
    }

    /// Match the known worker-a jitter, drift avoidance, and late second clock read.
    #[test]
    fn arms_one_anchored_timer_until_stop() {
        let trace = RefCell::new(Vec::new());
        let mut times = VecDeque::from([120_000, 120_013, 250_000, 250_004, 300_000, 304_000]);
        let mut scheduler = AnchoredWorkerProcessScheduler::new(
            || times.pop_front().expect("only admitted timer samples"),
            Timer(&trace),
            |key: &str| {
                trace.borrow_mut().push(format!("digest:{key}"));
                Ok([106, 101, 226, 55])
            },
            |failure: &WorkerProcessFailure| {
                trace
                    .borrow_mut()
                    .push(format!("diagnostic:{}", failure.code));
            },
        );
        let schedule = WorkerProcessSchedule {
            cadence_ms: 60_000,
            key: "worker-a".to_owned(),
        };
        scheduler.start(schedule.clone()).unwrap();
        scheduler.start(schedule).unwrap();
        scheduler.record_iteration_error(&WorkerProcessFailure {
            phase: WorkerProcessPhase::Iteration,
            code: "test_iteration_failed".to_owned(),
        });
        scheduler.rearm_after_poll().unwrap();
        scheduler.rearm_after_poll().unwrap();
        scheduler.stop();
        scheduler.stop();
        scheduler.rearm_after_poll().unwrap();
        assert!(times.is_empty());
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "digest:worker-a",
                "arm:2430",
                "diagnostic:test_iteration_failed",
                "arm:52439",
                "arm:0",
                "cancel",
            ]
        );
    }
}
