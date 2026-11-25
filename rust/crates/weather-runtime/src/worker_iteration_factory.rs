//! Turn the loaded runtime into the retained, sidecar-aware worker iteration.

use weather_worker::{
    WorkerProcessFailure, WorkerProcessIterationFactory, WorkerScheduledSourceDispatcher,
    WorkerSidecarPhase,
};

use crate::{
    DatabaseWorkerProcessIteration, OpenMeteoRuntimeValues, WorkerIterationDatabasePort,
    WorkerIterationDatabaseRepository, WorkerIterationDrainPort,
};

/// The loaded work owners transferred after readiness and durable heartbeat admission.
pub struct AdmittedDatabaseWorkerIteration<Dispatcher, Phase> {
    pub dispatcher: Dispatcher,
    pub sidecar_phase: Phase,
}

/// Retain the process clock, canonical value owner, and drain across all worker iterations.
pub struct DatabaseWorkerProcessIterationFactory<Values, Clock, Drain> {
    clock: Clock,
    drain: Drain,
    values: Values,
}

impl<Values, Clock, Drain> DatabaseWorkerProcessIterationFactory<Values, Clock, Drain> {
    /// Configure iteration boundaries without opening a worker pool or loading provider state.
    pub const fn new(values: Values, clock: Clock, drain: Drain) -> Self {
        Self {
            clock,
            drain,
            values,
        }
    }
}

impl<Configuration, Database, Values, Clock, Drain, Dispatcher, Phase>
    WorkerProcessIterationFactory<
        Configuration,
        Database,
        AdmittedDatabaseWorkerIteration<Dispatcher, Phase>,
    > for DatabaseWorkerProcessIterationFactory<Values, Clock, Drain>
where
    Database: WorkerIterationDatabasePort,
    Values: OpenMeteoRuntimeValues,
    Clock: FnMut() -> i64,
    Drain: WorkerIterationDrainPort<Database>,
    for<'a> Dispatcher:
        WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>,
    for<'a> Phase: WorkerSidecarPhase<WorkerIterationDatabaseRepository<'a, Database, Values>>,
{
    type Iteration = DatabaseWorkerProcessIteration<Values, Dispatcher, Clock, Drain, Phase>;

    /// Keep the configured work owners instead of falling back to the inactive constructor.
    fn create_iteration(
        self,
        _configuration: &Configuration,
        _pool: &mut Database,
        runtime: AdmittedDatabaseWorkerIteration<Dispatcher, Phase>,
    ) -> Result<Self::Iteration, WorkerProcessFailure> {
        Ok(DatabaseWorkerProcessIteration::new(
            self.values,
            runtime.dispatcher,
            self.clock,
            self.drain,
        )
        .with_sidecar_phase(runtime.sidecar_phase))
    }
}
