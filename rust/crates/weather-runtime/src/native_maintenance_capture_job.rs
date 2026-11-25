//! Native host loop for complete lock-serialized maintenance capture cycles.

use std::{
    error::Error,
    ffi::OsStr,
    fmt::{self, Display, Formatter},
    future::{Future, poll_fn},
    io,
    pin::Pin,
    time::Duration,
};

use crate::NativeProcessShutdown;

const CAPTURE_POLL_INTERVAL: Duration = Duration::from_millis(15_000);

/// One exact installed process mode admitted by the predecessor CLI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceCaptureJobCommand {
    Run,
    Readiness,
}

/// Closed CLI rejection for every path, activation switch, or extra argument.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCaptureJobArgumentsError;

impl Display for NativeMaintenanceCaptureJobArgumentsError {
    /// Preserve the predecessor's single closed argument diagnostic.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("maintenance capture job accepts only --readiness")
    }
}

impl Error for NativeMaintenanceCaptureJobArgumentsError {}

/// Parse only ordinary service execution or the one exact readiness inspection flag.
pub fn parse_native_maintenance_capture_job_arguments<Argument: AsRef<OsStr>>(
    arguments: &[Argument],
) -> Result<NativeMaintenanceCaptureJobCommand, NativeMaintenanceCaptureJobArgumentsError> {
    match arguments {
        [] => Ok(NativeMaintenanceCaptureJobCommand::Run),
        [argument] if argument.as_ref() == OsStr::new("--readiness") => {
            Ok(NativeMaintenanceCaptureJobCommand::Readiness)
        }
        _ => Err(NativeMaintenanceCaptureJobArgumentsError),
    }
}

/// Actual complete capture-cycle boundary that must retain the controller process lock.
pub trait NativeMaintenanceCaptureCycleLockPort {
    type Error;

    /// Run one complete admitted cycle inside the real shared controller process lock.
    fn run_complete_capture_cycle_with_process_lock(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>>;
}

/// Successful terminal state from a stopped long-running service process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCaptureJobResult {
    pub iterations: u64,
    pub stopped: bool,
}

/// Signal registration, actual capture-cycle, or unreachable counter failure.
#[derive(Debug)]
pub enum NativeMaintenanceCaptureJobError<CycleError> {
    SignalRegistration(io::Error),
    Cycle(CycleError),
    IterationOverflow,
}

impl<CycleError: Display> Display for NativeMaintenanceCaptureJobError<CycleError> {
    /// Render the actual cycle diagnostic or the bounded owning process phase.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignalRegistration(_) => {
                formatter.write_str("maintenance capture shutdown signal registration failed")
            }
            Self::Cycle(error) => Display::fmt(error, formatter),
            Self::IterationOverflow => {
                formatter.write_str("maintenance capture iteration count overflowed")
            }
        }
    }
}

impl<CycleError: Error + 'static> Error for NativeMaintenanceCaptureJobError<CycleError> {
    /// Retain typed signal and cycle failures without manufacturing a process result.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SignalRegistration(error) => Some(error),
            Self::Cycle(error) => Some(error),
            Self::IterationOverflow => None,
        }
    }
}

/// Register the real process signals and run complete capture cycles until shutdown.
pub async fn run_native_maintenance_capture_job<Port>(
    port: &mut Port,
) -> Result<NativeMaintenanceCaptureJobResult, NativeMaintenanceCaptureJobError<Port::Error>>
where
    Port: NativeMaintenanceCaptureCycleLockPort,
{
    let shutdown = NativeProcessShutdown::register()
        .map_err(NativeMaintenanceCaptureJobError::SignalRegistration)?;
    run_capture_loop(port, shutdown.wait(), RunSettings::production()).await
}

/// Internal fixed production settings with a fixture-only finite iteration ceiling.
#[derive(Clone, Copy)]
struct RunSettings {
    poll_interval: Duration,
    #[cfg(test)]
    maximum_iterations: Option<u64>,
}

impl RunSettings {
    /// Select the predecessor's fixed fifteen-second production poll interval.
    const fn production() -> Self {
        Self {
            poll_interval: CAPTURE_POLL_INTERVAL,
            #[cfg(test)]
            maximum_iterations: None,
        }
    }

    /// Report the fixture-only terminal count without adding a production control surface.
    fn iteration_limit_reached(self, iterations: u64) -> bool {
        #[cfg(test)]
        {
            self.maximum_iterations == Some(iterations)
        }
        #[cfg(not(test))]
        {
            let _ = iterations;
            false
        }
    }
}

/// Run the common loop while retaining the shutdown future across cycle and timer boundaries.
async fn run_capture_loop<Port, Shutdown>(
    port: &mut Port,
    shutdown: Shutdown,
    settings: RunSettings,
) -> Result<NativeMaintenanceCaptureJobResult, NativeMaintenanceCaptureJobError<Port::Error>>
where
    Port: NativeMaintenanceCaptureCycleLockPort,
    Shutdown: Future<Output = ()>,
{
    let mut shutdown = Box::pin(shutdown);
    let mut iterations = 0_u64;
    // check stop before each new operation while never cancelling an admitted in-flight cycle
    loop {
        if shutdown_observed(shutdown.as_mut()).await {
            return Ok(NativeMaintenanceCaptureJobResult {
                iterations,
                stopped: true,
            });
        }
        // keep the iteration ceiling confined to the compiled test fixture
        if settings.iteration_limit_reached(iterations) {
            return Ok(NativeMaintenanceCaptureJobResult {
                iterations,
                stopped: false,
            });
        }
        port.run_complete_capture_cycle_with_process_lock()
            .await
            .map_err(NativeMaintenanceCaptureJobError::Cycle)?;
        iterations = iterations
            .checked_add(1)
            .ok_or(NativeMaintenanceCaptureJobError::IterationOverflow)?;
        // observe a signal queued during the complete cycle before starting its poll delay
        if shutdown_observed(shutdown.as_mut()).await {
            return Ok(NativeMaintenanceCaptureJobResult {
                iterations,
                stopped: true,
            });
        }
        // do not delay explicit fixture completion after its last admitted cycle
        if settings.iteration_limit_reached(iterations) {
            return Ok(NativeMaintenanceCaptureJobResult {
                iterations,
                stopped: false,
            });
        }
        // wake the finite poll promptly without allowing shutdown to cancel a capture operation
        if wait_for_poll_or_shutdown(shutdown.as_mut(), settings.poll_interval).await {
            return Ok(NativeMaintenanceCaptureJobResult {
                iterations,
                stopped: true,
            });
        }
    }
}

/// Poll shutdown once without waiting or consuming a pending capture-loop wakeup.
async fn shutdown_observed<Shutdown>(mut shutdown: Pin<&mut Shutdown>) -> bool
where
    Shutdown: Future<Output = ()>,
{
    poll_fn(|context| std::task::Poll::Ready(shutdown.as_mut().poll(context).is_ready())).await
}

/// Race the retained shutdown receiver against one finite poll timer without macro features.
async fn wait_for_poll_or_shutdown<Shutdown>(
    mut shutdown: Pin<&mut Shutdown>,
    poll_interval: Duration,
) -> bool
where
    Shutdown: Future<Output = ()>,
{
    let mut timer = Box::pin(tokio::time::sleep(poll_interval));
    poll_fn(|context| {
        // prefer a stop already visible at the same wakeup as the elapsed timer
        if shutdown.as_mut().poll(context).is_ready() {
            return std::task::Poll::Ready(true);
        }
        // finish normally only after the fixed finite delay has actually elapsed
        if timer.as_mut().poll(context).is_ready() {
            std::task::Poll::Ready(false)
        } else {
            std::task::Poll::Pending
        }
    })
    .await
}

/// Run an isolated finite fixture without exposing an iteration limit to production callers.
#[cfg(test)]
async fn run_capture_loop_fixture<Port, Shutdown>(
    port: &mut Port,
    shutdown: Shutdown,
    maximum_iterations: u64,
    poll_interval: Duration,
) -> Result<NativeMaintenanceCaptureJobResult, NativeMaintenanceCaptureJobError<Port::Error>>
where
    Port: NativeMaintenanceCaptureCycleLockPort,
    Shutdown: Future<Output = ()>,
{
    assert!(maximum_iterations > 0, "fixture iteration limit");
    run_capture_loop(
        port,
        shutdown,
        RunSettings {
            poll_interval,
            maximum_iterations: Some(maximum_iterations),
        },
    )
    .await
}

#[cfg(test)]
#[path = "native_maintenance_capture_job_test.rs"]
mod tests;
