//! Bind the worker's optional maintenance capture task to admitted native owners.

use std::{sync::Arc, time::Duration};

use weather_api::AdmittedAdjustmentRevisionCaptureWitness;
use weather_worker::{
    MaintenanceCaptureDue, TaskFailure, WorkerProcessFailure, WorkerProcessPhase,
};

use crate::{
    NativeOutboundHttp, NativePrivatePostTransport, NativeWirePrimitives,
    NativeWorkerMaintenanceCaptureTransport, RetainedWorkerSidecarTask, WorkerConfiguration,
    WorkerMaintenanceCaptureTask,
};

/// Actual worker maintenance task over the shared process HTTP runtime and native JSON/time owner.
pub type NativeWorkerMaintenanceCaptureTask =
    WorkerMaintenanceCaptureTask<NativeWorkerMaintenanceCaptureTransport, NativeWirePrimitives>;

/// Explicit optional task retained after configuration and capture-witness admission.
pub enum NativeWorkerMaintenanceTaskBinding {
    /// The source omits the task when either private configuration or the witness is absent.
    Absent,
    /// Both authorities were present and the genuine native task was constructed.
    Loaded(NativeWorkerMaintenanceCaptureTask),
}

impl NativeWorkerMaintenanceTaskBinding {
    /// Report whether startup loaded the genuine maintenance client and task.
    pub const fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded(_))
    }
}

impl<Database, Values> RetainedWorkerSidecarTask<Database, Values, MaintenanceCaptureDue>
    for NativeWorkerMaintenanceTaskBinding
{
    type Output = ();

    /// Delegate admitted work and refuse an inconsistent call to an absent optional task.
    fn run(
        &mut self,
        due: MaintenanceCaptureDue,
        database: &mut Database,
        values: &mut Values,
    ) -> Result<Self::Output, TaskFailure> {
        // the sidecar plan must never dispatch the source-absent variant
        match self {
            Self::Absent => Err(TaskFailure {
                attempts: 0,
                code: "adjustment_maintenance_capture_unavailable".to_owned(),
            }),
            Self::Loaded(task) => task.run(due, database, values),
        }
    }
}

/// Construct the exact optional maintenance task after private origin and witness admission.
pub fn native_worker_maintenance_task_binding(
    configuration: &WorkerConfiguration,
    capture_witness: Option<&AdmittedAdjustmentRevisionCaptureWitness>,
    client: Arc<NativeOutboundHttp>,
    timeout: Duration,
    maximum_response_body_bytes: usize,
) -> Result<NativeWorkerMaintenanceTaskBinding, WorkerProcessFailure> {
    let Some(origin) = configuration.adjustment_maintenance_api_origin.as_deref() else {
        // mirror the source's explicit undefined task when no private origin is configured
        return Ok(NativeWorkerMaintenanceTaskBinding::Absent);
    };
    let Some(_capture_witness) = capture_witness else {
        // mirror the source's future-only gate when the verified root witness is unavailable
        return Ok(NativeWorkerMaintenanceTaskBinding::Absent);
    };
    // refuse unusable native transport controls instead of installing a configured dummy task
    if timeout.is_zero() || maximum_response_body_bytes == 0 {
        return Err(binding_failure(
            "native_worker_maintenance_transport_controls_invalid",
        ));
    }
    let transport = NativeWorkerMaintenanceCaptureTransport::new(
        origin,
        NativePrivatePostTransport::new(client, timeout),
        maximum_response_body_bytes,
    )
    .map_err(|_| binding_failure("native_worker_maintenance_origin_invalid"))?;
    Ok(NativeWorkerMaintenanceTaskBinding::Loaded(
        WorkerMaintenanceCaptureTask::new(transport, NativeWirePrimitives),
    ))
}

/// Produce one bounded runtime-loading failure without origin or witness material.
fn binding_failure(code: &str) -> WorkerProcessFailure {
    WorkerProcessFailure {
        code: code.to_owned(),
        phase: WorkerProcessPhase::Runtime,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Refuse an impossible dispatch through the explicitly source-absent binding.
    #[test]
    fn absent_binding_refuses_dispatch_without_provider_attempts() {
        let mut binding = NativeWorkerMaintenanceTaskBinding::Absent;

        let result = RetainedWorkerSidecarTask::<(), (), MaintenanceCaptureDue>::run(
            &mut binding,
            MaintenanceCaptureDue { issued_at_ms: 0 },
            &mut (),
            &mut (),
        );

        assert_eq!(
            result,
            Err(TaskFailure {
                attempts: 0,
                code: "adjustment_maintenance_capture_unavailable".to_owned(),
            })
        );
        assert!(!binding.is_loaded());
    }
}
