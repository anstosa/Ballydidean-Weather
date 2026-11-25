//! Select the real temperature collection and optional retained archive task at worker startup.

use std::{sync::Arc, time::Duration};

use weather_api::AdmittedAdjustmentRevisionCaptureWitness;
use weather_worker::{
    TaskFailure, TemperatureCanaryCollectionResult, TemperatureCanaryFetchLimits,
};

use crate::{
    NativeOutboundHttp, NativePostgresPool, NativeTemperatureCanaryServingDatabaseFactory,
    NativeTemperatureRevisionArchiveDatabaseFactory, NativeWorkerTemperatureBindings,
    RetainedWorkerSidecarTask, TemperatureCanaryRevisionArchiveMode, WorkerConfiguration,
    WorkerTemperatureCanaryTask, native_worker_temperature_task, system_worker_health_milliseconds,
};

type WorkerClock = fn() -> i64;
type ServingTask = WorkerTemperatureCanaryTask<
    NativeWorkerTemperatureBindings<NativeTemperatureCanaryServingDatabaseFactory>,
    WorkerClock,
>;
type ArchiveTask = WorkerTemperatureCanaryTask<
    NativeWorkerTemperatureBindings<NativeTemperatureRevisionArchiveDatabaseFactory>,
    WorkerClock,
>;

/// Both real worker temperature paths retain the same provider, calendar, and PostgreSQL pool.
pub enum NativeWorkerTemperatureTaskOwner {
    Serving(Box<ServingTask>),
    Archive(Box<ArchiveTask>),
}

impl<Database, Values> RetainedWorkerSidecarTask<Database, Values, ()>
    for NativeWorkerTemperatureTaskOwner
{
    type Output = TemperatureCanaryCollectionResult;

    /// Delegate only to the actual selected serving or archive-composed repository.
    fn run(
        &mut self,
        (): (),
        database: &mut Database,
        values: &mut Values,
    ) -> Result<Self::Output, TaskFailure> {
        match self {
            Self::Serving(task) => task.run((), database, values),
            Self::Archive(task) => task.run((), database, values),
        }
    }
}

/// Sample the real collection clock; an unavailable OS clock stays outside admitted Date bounds.
fn worker_clock() -> i64 {
    system_worker_health_milliseconds().unwrap_or(i64::MAX)
}

/// Preserve the predecessor's origin-and-witness choice without requiring either when absent.
pub fn native_worker_temperature_task_owner(
    configuration: &WorkerConfiguration,
    pool: NativePostgresPool,
    client: Arc<NativeOutboundHttp>,
    witness: Option<&AdmittedAdjustmentRevisionCaptureWitness>,
    archive_timeout: Duration,
) -> NativeWorkerTemperatureTaskOwner {
    let limits = TemperatureCanaryFetchLimits::default();
    let clock = worker_clock as WorkerClock;
    let compatibility_origin = configuration.open_meteo_compatibility_origin.clone();
    // the original worker adds revision archive effects only when both independent authorities exist
    if let (Some(origin), Some(witness)) = (
        configuration.adjustment_maintenance_api_origin.as_deref(),
        witness,
    ) {
        let factory = NativeTemperatureRevisionArchiveDatabaseFactory::new(
            witness.clone(),
            Arc::clone(&client),
            origin,
            archive_timeout,
        );
        return NativeWorkerTemperatureTaskOwner::Archive(Box::new(
            native_worker_temperature_task(
                &configuration.site,
                NativeWorkerTemperatureBindings::new(
                    pool,
                    client,
                    compatibility_origin,
                    TemperatureCanaryRevisionArchiveMode::Enabled,
                    factory,
                ),
                clock,
                limits,
            ),
        ));
    }
    NativeWorkerTemperatureTaskOwner::Serving(Box::new(native_worker_temperature_task(
        &configuration.site,
        NativeWorkerTemperatureBindings::new(
            pool,
            client,
            compatibility_origin,
            TemperatureCanaryRevisionArchiveMode::Disabled,
            NativeTemperatureCanaryServingDatabaseFactory,
        ),
        clock,
        limits,
    )))
}
