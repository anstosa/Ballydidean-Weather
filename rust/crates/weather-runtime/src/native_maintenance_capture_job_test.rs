use std::{
    convert::Infallible,
    ffi::OsString,
    future::{self, Future},
    pin::Pin,
    time::Duration,
};

use tokio::sync::oneshot;

use super::{
    NativeMaintenanceCaptureCycleLockPort, NativeMaintenanceCaptureJobCommand,
    NativeMaintenanceCaptureJobError, parse_native_maintenance_capture_job_arguments,
    run_capture_loop_fixture,
};
use crate::NativeBackendRuntime;

/// Count only complete lock-owned fixture cycles.
struct CountingCyclePort {
    iterations: u64,
    first_complete: Option<oneshot::Sender<()>>,
}

impl NativeMaintenanceCaptureCycleLockPort for CountingCyclePort {
    type Error = Infallible;

    /// Finish one real asynchronous boundary before reporting the cycle as admitted.
    fn run_complete_capture_cycle_with_process_lock(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>> {
        Box::pin(async move {
            self.iterations += 1;
            // notify only after the complete first locked cycle
            if self.iterations == 1 {
                // consume the one-shot completion witness at most once
                if let Some(complete) = self.first_complete.take() {
                    let _ = complete.send(());
                }
            }
            Ok(())
        })
    }
}

/// Reject one fixture cycle through the exact typed port error path.
struct FailingCyclePort;

impl NativeMaintenanceCaptureCycleLockPort for FailingCyclePort {
    type Error = &'static str;

    /// Return the actual cycle owner failure without advancing the host loop.
    fn run_complete_capture_cycle_with_process_lock(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>> {
        Box::pin(future::ready(Err("fixture cycle failed")))
    }
}

/// Hold one admitted cycle open until an external stop has already been queued.
struct HeldCyclePort {
    iterations: u64,
    started: Option<oneshot::Sender<()>>,
    release: Option<oneshot::Receiver<()>>,
}

impl NativeMaintenanceCaptureCycleLockPort for HeldCyclePort {
    type Error = Infallible;

    /// Complete the lock-owned operation even when shutdown arrives while it is pending.
    fn run_complete_capture_cycle_with_process_lock(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>> {
        Box::pin(async move {
            // announce admission before yielding the in-flight cycle
            if let Some(started) = self.started.take() {
                let _ = started.send(());
            }
            // wait for the fixture to queue shutdown before completing this cycle
            if let Some(release) = self.release.take() {
                let _ = release.await;
            }
            self.iterations += 1;
            Ok(())
        })
    }
}

/// Preserve the absent and sole-readiness modes while rejecting every extension.
#[test]
fn arguments_accept_only_run_or_readiness() {
    assert_eq!(
        parse_native_maintenance_capture_job_arguments::<OsString>(&[]),
        Ok(NativeMaintenanceCaptureJobCommand::Run)
    );
    assert_eq!(
        parse_native_maintenance_capture_job_arguments(&[OsString::from("--readiness")]),
        Ok(NativeMaintenanceCaptureJobCommand::Readiness)
    );
    // reject duplicates, paths, and unrelated activation switches
    for arguments in [
        vec![OsString::from("--readiness"), OsString::from("--readiness")],
        vec![OsString::from("/tmp/state")],
        vec![OsString::from("--once")],
    ] {
        assert!(parse_native_maintenance_capture_job_arguments(&arguments).is_err());
    }
}

/// Stop exactly at the fixture ceiling without manufacturing a production stop signal.
#[test]
fn fixture_iteration_limit_returns_success() {
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let mut port = CountingCyclePort {
            iterations: 0,
            first_complete: None,
        };
        let result = run_capture_loop_fixture(&mut port, future::pending(), 2, Duration::ZERO)
            .await
            .expect("finite fixture");
        assert_eq!(result.iterations, 2);
        assert!(!result.stopped);
        assert_eq!(port.iterations, 2);
    });
}

/// Propagate the concrete locked-cycle failure without reporting a successful iteration.
#[test]
fn cycle_failure_is_not_converted_to_success() {
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let mut port = FailingCyclePort;
        let error = run_capture_loop_fixture(&mut port, future::pending(), 1, Duration::ZERO)
            .await
            .expect_err("cycle failure");
        assert!(matches!(
            error,
            NativeMaintenanceCaptureJobError::Cycle("fixture cycle failed")
        ));
    });
}

/// Finish an admitted lock-owned operation after shutdown instead of cancelling its future.
#[test]
fn shutdown_during_cycle_drains_that_complete_cycle() {
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let (stop_tx, stop_rx) = oneshot::channel();
        let controller = tokio::spawn(async move {
            let _ = started_rx.await;
            let _ = stop_tx.send(());
            let _ = release_tx.send(());
        });
        let mut port = HeldCyclePort {
            iterations: 0,
            started: Some(started_tx),
            release: Some(release_rx),
        };
        let result = run_capture_loop_fixture(
            &mut port,
            async move {
                let _ = stop_rx.await;
            },
            2,
            Duration::from_secs(30),
        )
        .await
        .expect("drained capture loop");
        controller.await.expect("shutdown controller");
        assert_eq!(result.iterations, 1);
        assert!(result.stopped);
        assert_eq!(port.iterations, 1);
    });
}

/// Wake the long poll promptly only after the already admitted cycle finishes.
#[test]
fn shutdown_after_cycle_wakes_poll_without_starting_another_cycle() {
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let (complete_tx, complete_rx) = oneshot::channel();
        let (stop_tx, stop_rx) = oneshot::channel();
        let stop_task = tokio::spawn(async move {
            let _ = complete_rx.await;
            tokio::task::yield_now().await;
            let _ = stop_tx.send(());
        });
        let mut port = CountingCyclePort {
            iterations: 0,
            first_complete: Some(complete_tx),
        };
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            run_capture_loop_fixture(
                &mut port,
                async move {
                    let _ = stop_rx.await;
                },
                2,
                Duration::from_secs(30),
            ),
        )
        .await
        .expect("shutdown wakes finite poll")
        .expect("capture loop");
        stop_task.await.expect("stop task");
        assert_eq!(result.iterations, 1);
        assert!(result.stopped);
        assert_eq!(port.iterations, 1);
    });
}
