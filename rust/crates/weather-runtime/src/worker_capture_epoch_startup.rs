//! Bind the database worker lifecycle to installed capture and archive owners.

use std::sync::Arc;

use weather_worker::{
    AmbientWeatherBackfillProvider, WorkerProcessEnvironmentPort, WorkerProcessIterationFactory,
    WorkerProcessOptions, WorkerProcessPoolFactory, WorkerProcessPorts, WorkerProcessRuntimeLoader,
    WorkerProcessSchedulerPort, WorkerProcessStartupResult, start_worker_process,
};

use crate::{
    DatabaseWorkerRuntimeWithCaptureEpoch, NativeCaptureEpochWitnessFile,
    PublicStationStartupBackfill, PublicStationStartupDiagnostics, PublicStationStartupPool,
    PublicStationStartupTime, RetainedCaptureEpochWitnessLoader,
    SupplementalPublicStationBackfillProvider, WorkerConfiguration,
};

/// In-process worker startup runtime with the witnessed maintenance phase.
pub type NativeWorkerCaptureEpochRuntime<Runtime, Codec, Time> =
    DatabaseWorkerRuntimeWithCaptureEpoch<
        Runtime,
        RetainedCaptureEpochWitnessLoader<NativeCaptureEpochWitnessFile, Codec, Time>,
    >;

/// Provider and clock owners bound to the normal worker's one background archive.
pub struct WorkerPublicStationArchiveOwners<Ambient, Supplemental, Time, Diagnostics> {
    pub ambient: Ambient,
    pub supplemental: Supplemental,
    pub time: Time,
    pub diagnostics: Arc<Diagnostics>,
}

/// Startup result retaining the sole catalog-bound background owner.
pub type DatabaseWorkerStartupResult<
    Pool,
    Scheduler,
    Ambient,
    Supplemental,
    ArchiveTime,
    Diagnostics,
    Iteration,
    Runtime,
> = WorkerProcessStartupResult<
    WorkerConfiguration,
    Pool,
    Scheduler,
    PublicStationStartupBackfill<Ambient, Supplemental, ArchiveTime, Diagnostics>,
    Iteration,
    Runtime,
>;

/// Start the worker with the installed witness and the same-pool public-station archive.
pub fn start_database_worker_process<
    Environment,
    Factory,
    Runtime,
    Scheduler,
    Ambient,
    Supplemental,
    ArchiveTime,
    Diagnostics,
    Iteration,
    Codec,
    Time,
>(
    options: WorkerProcessOptions,
    ports: WorkerProcessPorts<
        Environment,
        Factory,
        Runtime,
        Scheduler,
        WorkerPublicStationArchiveOwners<Ambient, Supplemental, ArchiveTime, Diagnostics>,
        Iteration,
    >,
    codec: Codec,
    time: Time,
) -> DatabaseWorkerStartupResult<
    Factory::Pool,
    Scheduler,
    Ambient,
    Supplemental,
    ArchiveTime,
    Diagnostics,
    Iteration,
    Runtime::Runtime,
>
where
    Environment: WorkerProcessEnvironmentPort<Configuration = WorkerConfiguration>,
    Factory: WorkerProcessPoolFactory<WorkerConfiguration>,
    Factory::Pool: PublicStationStartupPool,
    Runtime: WorkerProcessRuntimeLoader<WorkerConfiguration, Factory::Pool>,
    NativeWorkerCaptureEpochRuntime<Runtime, Codec, Time>:
        WorkerProcessRuntimeLoader<WorkerConfiguration, Factory::Pool, Runtime = Runtime::Runtime>,
    Iteration: WorkerProcessIterationFactory<WorkerConfiguration, Factory::Pool, Runtime::Runtime>,
    Ambient: AmbientWeatherBackfillProvider + Send + 'static,
    Supplemental: SupplementalPublicStationBackfillProvider + Send + 'static,
    ArchiveTime: PublicStationStartupTime + Send + 'static,
    Diagnostics: PublicStationStartupDiagnostics + Send + Sync + 'static,
    Scheduler: WorkerProcessSchedulerPort,
{
    let WorkerProcessPorts {
        backfill,
        environment,
        iteration_factory,
        loaders,
        pool_factory,
        scheduler,
    } = ports;
    let WorkerPublicStationArchiveOwners {
        ambient,
        supplemental,
        time: archive_time,
        diagnostics,
    } = backfill;
    start_worker_process(
        options,
        WorkerProcessPorts {
            backfill: PublicStationStartupBackfill::new(
                ambient,
                supplemental,
                archive_time,
                diagnostics,
            ),
            environment,
            iteration_factory,
            loaders: DatabaseWorkerRuntimeWithCaptureEpoch::native(loaders, codec, time),
            pool_factory,
            scheduler,
        },
    )
}
