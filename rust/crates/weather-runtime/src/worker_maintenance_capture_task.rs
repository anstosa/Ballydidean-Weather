//! Trigger one due adjustment-maintenance capture through an admitted private transport.

use weather_contracts::AdjustmentMaintenanceCaptureRequest;
use weather_worker::{MaintenanceCaptureDue, TaskFailure, TemperatureCanaryTimeAdapter};

use crate::RetainedWorkerSidecarTask;

/// Exact credential-free private capture route used by the worker.
pub const WORKER_MAINTENANCE_CAPTURE_PATH: &str = weather_api::MAINTENANCE_CAPTURE_PATH;
/// Exact JSON request media type used by the worker trigger.
pub const WORKER_MAINTENANCE_CAPTURE_CONTENT_TYPE: &str = "application/json";

/// One typed JSON POST through an already admitted private origin and transport owner.
#[derive(Debug, Clone, Copy)]
pub struct WorkerMaintenanceCapturePost<'a> {
    pub body: &'a AdjustmentMaintenanceCaptureRequest,
    pub content_type: &'static str,
    pub pathname: &'static str,
}

/// Typed response metadata required from the approved private HTTP adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerMaintenanceCaptureResponse {
    pub status_code: u16,
}

/// Credential-free POST JSON owner already bound to an admitted maintenance API origin.
pub trait WorkerMaintenanceCaptureTransport {
    type Error;

    /// Execute one typed request without exposing URL parsing or HTTP framing here.
    fn post_json(
        &mut self,
        request: WorkerMaintenanceCapturePost<'_>,
    ) -> Result<WorkerMaintenanceCaptureResponse, Self::Error>;
}

/// Concrete retained sidecar task over approved transport and canonical UTC owners.
pub struct WorkerMaintenanceCaptureTask<Transport, Time> {
    time: Time,
    transport: Transport,
}

impl<Transport, Time> WorkerMaintenanceCaptureTask<Transport, Time> {
    /// Bind an origin-admitted transport and existing canonical time owner.
    pub const fn new(transport: Transport, time: Time) -> Self {
        Self { time, transport }
    }

    /// Return both process-owned adapters after the worker retires.
    pub fn into_parts(self) -> (Transport, Time) {
        (self.transport, self.time)
    }
}

impl<Database, Values, Transport, Time>
    RetainedWorkerSidecarTask<Database, Values, MaintenanceCaptureDue>
    for WorkerMaintenanceCaptureTask<Transport, Time>
where
    Transport: WorkerMaintenanceCaptureTransport,
    Time: TemperatureCanaryTimeAdapter,
{
    type Output = ();

    /// Format the admitted due clock and require one successful private POST response.
    fn run(
        &mut self,
        due: MaintenanceCaptureDue,
        _database: &mut Database,
        _values: &mut Values,
    ) -> Result<Self::Output, TaskFailure> {
        let issued_at = self.time.utc_string(due.issued_at_ms)?;
        let request = AdjustmentMaintenanceCaptureRequest {
            dueKey: format!("capture/{issued_at}"),
            issuedAt: issued_at,
        };
        let response = self
            .transport
            .post_json(WorkerMaintenanceCapturePost {
                body: &request,
                content_type: WORKER_MAINTENANCE_CAPTURE_CONTENT_TYPE,
                pathname: WORKER_MAINTENANCE_CAPTURE_PATH,
            })
            .map_err(|_| maintenance_capture_failure())?;

        // match Fetch response.ok across the complete HTTP 2xx range
        if !(200..300).contains(&response.status_code) {
            return Err(maintenance_capture_failure());
        }
        Ok(())
    }
}

/// Collapse private transport and non-success responses to the worker's stable task category.
fn maintenance_capture_failure() -> TaskFailure {
    TaskFailure {
        attempts: 1,
        code: "adjustment_maintenance_capture_failed".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    const ISSUED_AT_MS: i64 = 1_789_346_100_000;
    const ISSUED_AT: &str = "2026-09-14T00:35:00.000Z";

    #[derive(Debug, Clone, PartialEq)]
    struct ObservedPost {
        body: AdjustmentMaintenanceCaptureRequest,
        content_type: &'static str,
        pathname: &'static str,
    }

    struct Transport {
        posts: Rc<RefCell<Vec<ObservedPost>>>,
        status_code: u16,
    }

    impl WorkerMaintenanceCaptureTransport for Transport {
        type Error = ();

        /// Capture the typed POST and return the configured ordinary response.
        fn post_json(
            &mut self,
            request: WorkerMaintenanceCapturePost<'_>,
        ) -> Result<WorkerMaintenanceCaptureResponse, Self::Error> {
            self.posts.borrow_mut().push(ObservedPost {
                body: request.body.clone(),
                content_type: request.content_type,
                pathname: request.pathname,
            });
            Ok(WorkerMaintenanceCaptureResponse {
                status_code: self.status_code,
            })
        }
    }

    struct Time(Rc<RefCell<Vec<i64>>>);

    impl TemperatureCanaryTimeAdapter for Time {
        /// Format only the exact due instant supplied by the worker coordinator.
        fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
            self.0.borrow_mut().push(epoch_ms);
            // keep the fixture's canonical owner closed over one admitted timestamp
            if epoch_ms != ISSUED_AT_MS {
                return Err(TaskFailure {
                    attempts: 0,
                    code: "fixture_time".to_owned(),
                });
            }
            Ok(ISSUED_AT.to_owned())
        }
    }

    /// Send the exact due key and issued instant once for any successful 2xx response.
    #[test]
    fn posts_typed_capture_request_and_accepts_2xx() {
        let posts = Rc::new(RefCell::new(Vec::new()));
        let times = Rc::new(RefCell::new(Vec::new()));
        let mut task = WorkerMaintenanceCaptureTask::new(
            Transport {
                posts: Rc::clone(&posts),
                status_code: 204,
            },
            Time(Rc::clone(&times)),
        );

        let result = RetainedWorkerSidecarTask::<(), (), MaintenanceCaptureDue>::run(
            &mut task,
            MaintenanceCaptureDue {
                issued_at_ms: ISSUED_AT_MS,
            },
            &mut (),
            &mut (),
        );

        assert_eq!(result, Ok(()));
        assert_eq!(*times.borrow(), [ISSUED_AT_MS]);
        assert_eq!(
            *posts.borrow(),
            [ObservedPost {
                body: AdjustmentMaintenanceCaptureRequest {
                    dueKey: format!("capture/{ISSUED_AT}"),
                    issuedAt: ISSUED_AT.to_owned(),
                },
                content_type: "application/json",
                pathname: "/internal/adjustment-maintenance/shadow/capture",
            }]
        );
    }

    /// Contain a completed non-2xx response as the stable maintenance task failure.
    #[test]
    fn rejects_not_ok_response_after_one_post() {
        let posts = Rc::new(RefCell::new(Vec::new()));
        let mut task = WorkerMaintenanceCaptureTask::new(
            Transport {
                posts: Rc::clone(&posts),
                status_code: 409,
            },
            Time(Rc::new(RefCell::new(Vec::new()))),
        );

        let result = RetainedWorkerSidecarTask::<(), (), MaintenanceCaptureDue>::run(
            &mut task,
            MaintenanceCaptureDue {
                issued_at_ms: ISSUED_AT_MS,
            },
            &mut (),
            &mut (),
        );

        assert_eq!(
            result,
            Err(TaskFailure {
                attempts: 1,
                code: "adjustment_maintenance_capture_failed".to_owned(),
            })
        );
        assert_eq!(posts.borrow().len(), 1);
    }
}
