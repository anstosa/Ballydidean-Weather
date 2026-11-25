//! Compose all five actual retained worker sidecar tasks over the same process resources.

use std::{sync::Arc, time::Duration};

use weather_edge::ForecastEvidenceMaterialCodec;
use weather_worker::{TaskFailure, WorkerProcessFailure, WorkerProcessPhase};

use crate::{
    LoadedWorkerRainServing, NativeApiMaintenanceCandidateFiles, NativeModelEvidenceValues,
    NativeOutboundHttp, NativePostgresWorkerPool, NativeWirePrimitives,
    NativeWorkerMaintenanceTaskBinding, NativeWorkerProcessTimerEvents,
    NativeWorkerRainCaptureTaskOwner, NativeWorkerRainFixedGaugeTask,
    NativeWorkerRainInferenceTask, NativeWorkerRequiredSidecars, NativeWorkerRuntimePorts,
    NativeWorkerSidecarOwner, NativeWorkerSidecarTaskLoader, NativeWorkerTemperatureTaskOwner,
    RetainedCaptureEpochWitnessLoader, RetainedWorkerSidecarTasks, WorkerConfiguration,
    WorkerLoadedSidecarAdmission, native_worker_maintenance_task_binding,
    native_worker_rain_capture_task, native_worker_rain_fixed_gauge_task,
    native_worker_rain_inference_task, native_worker_runtime_ports_with_client,
    native_worker_shared_pool, native_worker_temperature_task_owner,
};

/// Explicit already admitted internal transport limits shared by every constructed sidecar.
#[derive(Clone, Copy)]
pub struct NativeWorkerTaskLimits {
    pub archive_timeout: Duration,
    pub maintenance_response_body_bytes: usize,
}

/// All five real task owners, including the precise predecessor compression effect.
pub type NativeWorkerTaskSet<Codec> = RetainedWorkerSidecarTasks<
    NativeWorkerTemperatureTaskOwner,
    NativeWorkerRainCaptureTaskOwner<Codec>,
    NativeWorkerRainFixedGaugeTask,
    NativeWorkerRainInferenceTask,
    NativeWorkerMaintenanceTaskBinding,
>;

/// One process startup owner; compression is supplied only by the independently admitted codec.
pub struct NativeWorkerTaskAssembly<Codec> {
    client: Arc<NativeOutboundHttp>,
    compression: Option<Codec>,
    limits: NativeWorkerTaskLimits,
}

impl<Codec> NativeWorkerTaskAssembly<Codec> {
    /// Retain actual shared runtime effects without initiating a provider or private request.
    pub fn new(
        client: Arc<NativeOutboundHttp>,
        compression: Codec,
        limits: NativeWorkerTaskLimits,
    ) -> Result<Self, WorkerProcessFailure> {
        // do not substitute a timeout or body limit that startup did not admit
        if limits.archive_timeout.is_zero() || limits.maintenance_response_body_bytes == 0 {
            return Err(runtime_failure("native_worker_sidecar_limits_invalid"));
        }
        Ok(Self {
            client,
            compression: Some(compression),
            limits,
        })
    }
}

impl<Codec: ForecastEvidenceMaterialCodec> NativeWorkerSidecarTaskLoader
    for NativeWorkerTaskAssembly<Codec>
{
    type Tasks = NativeWorkerTaskSet<Codec>;

    /// Bind every task to the already admitted model, one pool, and the single root witness.
    fn load_required_tasks(
        &mut self,
        configuration: &WorkerConfiguration,
        pool: &mut NativePostgresWorkerPool,
        files: &mut NativeApiMaintenanceCandidateFiles,
        mut values: NativeModelEvidenceValues,
        rain: LoadedWorkerRainServing,
    ) -> Result<NativeWorkerRequiredSidecars<Self::Tasks>, WorkerProcessFailure> {
        let compression = self
            .compression
            .take()
            .ok_or_else(|| runtime_failure("native_worker_sidecar_already_loaded"))?;
        let mut loaded = WorkerLoadedSidecarAdmission::default();
        let mut witness_loader =
            RetainedCaptureEpochWitnessLoader::native(NativeWirePrimitives, NativeWirePrimitives);
        loaded.load_capture_epoch(&mut witness_loader);
        let shared_pool = native_worker_shared_pool(pool).map_err(task_failure)?;
        let temperature = native_worker_temperature_task_owner(
            configuration,
            shared_pool,
            Arc::clone(&self.client),
            loaded.capture_epoch_witness(),
            self.limits.archive_timeout,
        );
        let capture =
            native_worker_rain_capture_task(Arc::clone(&self.client), configuration, &mut values)?;
        let target = native_worker_rain_fixed_gauge_task(
            pool,
            Arc::clone(&self.client),
            configuration,
            loaded.adjustment_capture_epoch.clone(),
            self.limits.archive_timeout,
        )
        .map_err(task_failure)?;
        let inference = native_worker_rain_inference_task(
            pool,
            Arc::clone(&self.client),
            configuration,
            &mut loaded,
            crate::NativeWorkerRainInferenceMaterial {
                files,
                values,
                model: rain,
            },
            self.limits.archive_timeout,
        )
        .map_err(task_failure)?;
        let maintenance = native_worker_maintenance_task_binding(
            configuration,
            loaded.capture_epoch_witness(),
            Arc::clone(&self.client),
            self.limits.archive_timeout,
            self.limits.maintenance_response_body_bytes,
        )?;
        loaded.adjustment_maintenance_operation_loaded = maintenance.is_loaded();
        Ok(NativeWorkerRequiredSidecars {
            loaded,
            tasks: RetainedWorkerSidecarTasks {
                temperature,
                rain: NativeWorkerRainCaptureTaskOwner::new(capture, compression),
                target,
                inference,
                maintenance,
            },
        })
    }
}

/// The fully composed worker process ports with required concrete sidecar ownership.
pub type NativeWorkerComposedPorts<Codec> =
    NativeWorkerRuntimePorts<NativeWorkerSidecarOwner<NativeWorkerTaskAssembly<Codec>>>;

/// Prepare the actual native worker around one shared outbound client and exact compression owner.
pub fn native_worker_composed_ports<Codec: ForecastEvidenceMaterialCodec>(
    release: String,
    compression: Codec,
    limits: NativeWorkerTaskLimits,
) -> Result<
    (
        NativeWorkerComposedPorts<Codec>,
        NativeWorkerProcessTimerEvents,
    ),
    WorkerProcessFailure,
> {
    let client = Arc::new(
        NativeOutboundHttp::new()
            .map_err(|_| runtime_failure("native_worker_http_initialization_failed"))?,
    );
    let loader = NativeWorkerTaskAssembly::new(Arc::clone(&client), compression, limits)?;
    let sidecars = NativeWorkerSidecarOwner::installed(loader)
        .map_err(|_| runtime_failure("native_worker_model_root_invalid"))?;
    native_worker_runtime_ports_with_client(release, client, sidecars)
        .map_err(|_| runtime_failure("native_worker_provider_initialization_failed"))
}

/// Retain a bounded runtime failure at the original worker startup phase.
fn runtime_failure(code: &str) -> WorkerProcessFailure {
    WorkerProcessFailure {
        code: code.to_owned(),
        phase: WorkerProcessPhase::Runtime,
    }
}

/// Preserve real sidecar preparation failure without exporting provider content.
fn task_failure(failure: TaskFailure) -> WorkerProcessFailure {
    runtime_failure(&failure.code)
}

/// Normal-mode and closed one-shot results from the existing worker process lifecycle.
#[derive(Debug)]
pub enum NativeComposedWorkerResult {
    Once(weather_worker::WorkerProcessTrigger),
    Running(crate::NativeWorkerProcessReport),
}

/// Original worker startup and retained native timer-loop failure categories.
#[derive(Debug)]
pub enum NativeComposedWorkerError {
    Startup(WorkerProcessFailure),
    Running(crate::NativeWorkerProcessRunError),
}

/// Run the complete native worker through its source-owned startup and shutdown sequence.
///
/// The caller must supply the proved deterministic compression owner and real shutdown future.
pub async fn run_composed_native_worker<Codec: ForecastEvidenceMaterialCodec>(
    release: String,
    compression: Codec,
    limits: NativeWorkerTaskLimits,
    options: weather_worker::WorkerProcessOptions,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<NativeComposedWorkerResult, NativeComposedWorkerError> {
    let (ports, timer_events) = native_worker_composed_ports(release, compression, limits)
        .map_err(NativeComposedWorkerError::Startup)?;
    let process = weather_worker::start_worker_process(options, ports)
        .map_err(NativeComposedWorkerError::Startup)?;
    // the compatibility one-shot result is already closed and never enters the long-running timer loop
    match process {
        weather_worker::WorkerProcessStartResult::Once(trigger) => {
            Ok(NativeComposedWorkerResult::Once(trigger))
        }
        weather_worker::WorkerProcessStartResult::Running(process) => {
            crate::run_native_worker_process(process, timer_events, shutdown)
                .await
                .map(NativeComposedWorkerResult::Running)
                .map_err(NativeComposedWorkerError::Running)
        }
    }
}
