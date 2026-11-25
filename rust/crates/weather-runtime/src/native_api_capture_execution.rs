//! Genuine detached execution for the native private maintenance capture callback.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex, mpsc},
    thread::{self, JoinHandle},
};

use weather_api::{AdjustmentMaintenanceCaptureCallback, AdjustmentMaintenanceOperationError};
use weather_contracts::CaptureApiMaintenanceShadowResult;

use crate::{
    ApiMaintenanceCaptureExecutor, ApiMaintenanceCaptureJob, NativeApiCaptureDetachedWork,
    NativeApiCapturePredictionOwner, NativeApiModelAdmission, NativePostgresPool,
    NativePostgresPrivateCaptureOwner, PreparedApiMaintenanceCaptureOperations,
    PreparedPrivateMaintenanceApi, RetainedApiMaintenanceCapture, run_detached_native_work,
};

type OperationError = AdjustmentMaintenanceOperationError;

/// Process-retained owner that lends each private request a genuine capture callback.
#[derive(Clone)]
pub struct NativeApiCaptureExecution {
    prediction: NativeApiCapturePredictionOwner,
    active: Arc<NativeCaptureActive>,
}

impl NativeApiCaptureExecution {
    /// Retain the actual database and archive owner used by every detached capture job.
    pub fn new(prediction: NativeApiCapturePredictionOwner) -> Self {
        Self {
            prediction,
            active: Arc::new(NativeCaptureActive::default()),
        }
    }

    /// Wait for all independently started captures after private request admission ends.
    pub fn drain(&self) -> Result<(), OperationError> {
        let mut count = self
            .active
            .count
            .lock()
            .map_err(|_| OperationError::Rejected)?;
        // include detached work when an earlier request failed after starting a capture
        while *count != 0 {
            count = self
                .active
                .drained
                .wait(count)
                .map_err(|_| OperationError::Rejected)?;
        }
        Ok(())
    }
}

impl NativePostgresPrivateCaptureOwner for NativeApiCaptureExecution {
    /// Bind actual reads, installed candidates, gap relay, and detached execution for one request.
    fn with_capture<Result>(
        &mut self,
        application: &PreparedPrivateMaintenanceApi<NativePostgresPool>,
        model: &NativeApiModelAdmission,
        operation: impl FnOnce(&mut dyn AdjustmentMaintenanceCaptureCallback) -> Result,
    ) -> Result {
        let (preparation, snapshots, candidates) = application.capture_material();
        let executor = NativeApiCaptureExecutor::new(
            application.shared_worker(),
            model.clone(),
            self.prediction.clone(),
            Arc::clone(&self.active),
        );
        let operations = PreparedApiMaintenanceCaptureOperations {
            temperature: candidates.temperature_capture(snapshots, None, model),
            wind: candidates.wind_capture(snapshots, model),
            witness: &preparation.capture_witness,
            time: model,
            executor,
            gap: self.prediction.relay(),
        };
        let mut capture = RetainedApiMaintenanceCapture::new(
            application.capture_reads(),
            model.clone(),
            operations,
        );
        operation(&mut capture)
    }
}

/// One detached native capture result and its non-cancelling thread handle.
pub struct NativeApiCapturePending {
    handle: Option<JoinHandle<()>>,
}

/// Request-local executor that starts owned jobs and observes all started completions together.
pub struct NativeApiCaptureExecutor {
    application: PreparedPrivateMaintenanceApi<NativePostgresPool>,
    model: NativeApiModelAdmission,
    prediction: NativeApiCapturePredictionOwner,
    active: Arc<NativeCaptureActive>,
    sender: mpsc::Sender<Result<(), OperationError>>,
    receiver: mpsc::Receiver<Result<(), OperationError>>,
}

impl NativeApiCaptureExecutor {
    /// Retain independent shared owners and one completion channel for the request's two jobs.
    fn new(
        application: PreparedPrivateMaintenanceApi<NativePostgresPool>,
        model: NativeApiModelAdmission,
        prediction: NativeApiCapturePredictionOwner,
        active: Arc<NativeCaptureActive>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            application,
            model,
            prediction,
            active,
            sender,
            receiver,
        }
    }

    /// Start one owned background job without retaining request-borrowed model material.
    fn start_detached(&mut self, work: NativeApiCaptureDetachedWork) -> NativeApiCapturePending {
        let application = self.application.shared_worker();
        let model = self.model.clone();
        let prediction = self.prediction.clone();
        let sender = self.sender.clone();
        let guard = match NativeCaptureGuard::new(Arc::clone(&self.active)) {
            Ok(guard) => guard,
            Err(error) => {
                let _ = self.sender.send(Err(error));
                return NativeApiCapturePending { handle: None };
            }
        };
        let spawned = thread::Builder::new()
            .name("weather-api-maintenance-capture".to_owned())
            .spawn(move || {
                let _guard = guard;
                // always settle the completion channel even when a detached evaluator panics
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run_detached_native_work(work, &application, &model, &prediction)
                        .map(|_: CaptureApiMaintenanceShadowResult| ())
                }))
                .unwrap_or(Err(OperationError::Rejected));
                let _ = sender.send(result);
            });
        // report local thread admission through the same completion path
        match spawned {
            Ok(handle) => NativeApiCapturePending {
                handle: Some(handle),
            },
            Err(_) => {
                let _ = self.sender.send(Err(OperationError::Rejected));
                NativeApiCapturePending { handle: None }
            }
        }
    }

    /// Join every present thread after its completion has been observed.
    fn join_pending(
        pending: Option<NativeApiCapturePending>,
        first_error: &mut Option<OperationError>,
    ) {
        // a missing family owns no thread or completion
        if let Some(mut pending) = pending {
            // classify a worker panic only when no reported operation error already won
            if pending
                .handle
                .take()
                .is_some_and(|handle| handle.join().is_err())
                && first_error.is_none()
            {
                *first_error = Some(OperationError::Rejected);
            }
        }
    }
}

impl<'a> ApiMaintenanceCaptureExecutor<'a> for NativeApiCaptureExecutor {
    type Pending = NativeApiCapturePending;

    /// Move the admitted job into owned work and start it before returning to source reads.
    fn start(&mut self, job: ApiMaintenanceCaptureJob<'a>) -> Self::Pending {
        self.start_detached(job.into_detached_native_work())
    }

    /// Drain both completion reports in arrival order, then join every started native thread.
    fn finish(
        &mut self,
        temperature: Option<Self::Pending>,
        wind: Option<Self::Pending>,
    ) -> Result<(), OperationError> {
        let completion_count = usize::from(temperature.is_some()) + usize::from(wind.is_some());
        let mut first_error = None;
        // observe every started result while retaining the first completed failure
        for _ in 0..completion_count {
            match self.receiver.recv() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    // retain only the first reported operation failure
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
                Err(_) => {
                    // represent a disconnected worker only when no earlier failure won
                    if first_error.is_none() {
                        first_error = Some(OperationError::Rejected);
                    }
                    break;
                }
            }
        }
        Self::join_pending(temperature, &mut first_error);
        Self::join_pending(wind, &mut first_error);
        // a categorical gap is a successful capture outcome; only operation errors propagate
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// Count active owned captures independently of request-local completion handles.
#[derive(Default)]
struct NativeCaptureActive {
    count: Mutex<usize>,
    drained: Condvar,
}

/// Release a capture's process drain claim on success, failure, panic or failed spawn.
struct NativeCaptureGuard(Arc<NativeCaptureActive>);

impl NativeCaptureGuard {
    /// Acquire an active claim before moving the job into its independent thread.
    fn new(active: Arc<NativeCaptureActive>) -> Result<Self, OperationError> {
        {
            let mut count = active.count.lock().map_err(|_| OperationError::Rejected)?;
            *count = count.checked_add(1).ok_or(OperationError::Rejected)?;
        }
        Ok(Self(active))
    }
}

impl Drop for NativeCaptureGuard {
    /// Signal process drain only after the last owned job settles.
    fn drop(&mut self) {
        // recover the count for shutdown even if an unrelated owner poisoned its mutex
        let mut count = self
            .0
            .count
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.0.drained.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independently retained capture work keeps its process drain claim until completion.
    #[test]
    fn native_detached_capture_guard_survives_request_handle_drop() {
        let active = Arc::new(NativeCaptureActive::default());
        let first = NativeCaptureGuard::new(Arc::clone(&active)).unwrap();
        let second = NativeCaptureGuard::new(Arc::clone(&active)).unwrap();
        assert_eq!(*active.count.lock().unwrap(), 2);
        let (settle, wait) = mpsc::channel();
        let handle = thread::spawn(move || {
            let _first = first;
            wait.recv().unwrap();
        });
        // dropping the observer must not end the underlying capture
        drop(handle);
        drop(second);
        assert_eq!(*active.count.lock().unwrap(), 1);
        settle.send(()).unwrap();
        let mut count = active.count.lock().unwrap();
        // await the actual worker's drain notification with a bounded test timeout
        while *count != 0 {
            let (next, result) = active
                .drained
                .wait_timeout(count, std::time::Duration::from_secs(2))
                .unwrap();
            assert!(!result.timed_out());
            count = next;
        }
        assert_eq!(*count, 0);
    }
}
