//! Run admitted worker sidecar jobs with a scoped borrow of the retained database.

use weather_worker::{
    FixedGaugeTargetResult, MaintenanceCaptureDue, RainCaptureResult, TaskFailure,
    TemperatureCanaryCollectionResult, WorkerSidecarTasks,
};

use crate::WorkerIterationDatabaseRepository;

/// One configured operation using the current worker database and canonical owners.
pub trait RetainedWorkerSidecarTask<Database, Values, Input> {
    type Output;

    /// Run one typed task without opening a second worker pool.
    fn run(
        &mut self,
        input: Input,
        database: &mut Database,
        values: &mut Values,
    ) -> Result<Self::Output, TaskFailure>;
}

impl<Database, Values, Input, Output, Operation> RetainedWorkerSidecarTask<Database, Values, Input>
    for Operation
where
    Operation: FnMut(Input, &mut Database, &mut Values) -> Result<Output, TaskFailure>,
{
    type Output = Output;

    /// Reuse an already configured operation through the same scoped owners.
    fn run(
        &mut self,
        input: Input,
        database: &mut Database,
        values: &mut Values,
    ) -> Result<Self::Output, TaskFailure> {
        self(input, database, values)
    }
}

/// The five optional job owners evaluated by the existing sidecar admission phase.
pub struct RetainedWorkerSidecarTasks<Temperature, Rain, Target, Inference, Maintenance> {
    pub temperature: Temperature,
    pub rain: Rain,
    pub target: Target,
    pub inference: Inference,
    pub maintenance: Maintenance,
}

impl<Database, Values, Temperature, Rain, Target, Inference, Maintenance>
    WorkerSidecarTasks<WorkerIterationDatabaseRepository<'_, Database, Values>>
    for RetainedWorkerSidecarTasks<Temperature, Rain, Target, Inference, Maintenance>
where
    Temperature:
        RetainedWorkerSidecarTask<Database, Values, (), Output = TemperatureCanaryCollectionResult>,
    Rain: RetainedWorkerSidecarTask<Database, Values, (), Output = RainCaptureResult>,
    Target: RetainedWorkerSidecarTask<Database, Values, (), Output = FixedGaugeTargetResult>,
    Inference: RetainedWorkerSidecarTask<Database, Values, i64, Output = bool>,
    Maintenance: RetainedWorkerSidecarTask<Database, Values, MaintenanceCaptureDue, Output = ()>,
{
    /// Connect the admitted temperature collector to this iteration's retained owners.
    fn collect_temperature_canary(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'_, Database, Values>,
    ) -> Result<TemperatureCanaryCollectionResult, TaskFailure> {
        repository
            .with_retained_resources(|database, values| self.temperature.run((), database, values))
    }

    /// Connect the prospective raw collector to the same retained database.
    fn collect_rain_capture(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'_, Database, Values>,
    ) -> Result<RainCaptureResult, TaskFailure> {
        repository.with_retained_resources(|database, values| self.rain.run((), database, values))
    }

    /// Publish a fixed-gauge target only after the coordinator admits it.
    fn publish_fixed_gauge_target(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'_, Database, Values>,
    ) -> Result<FixedGaugeTargetResult, TaskFailure> {
        repository.with_retained_resources(|database, values| self.target.run((), database, values))
    }

    /// Forward the separately captured inference instant unchanged.
    fn publish_rain_inference(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'_, Database, Values>,
        now_ms: i64,
    ) -> Result<bool, TaskFailure> {
        repository.with_retained_resources(|database, values| {
            self.inference.run(now_ms, database, values)
        })
    }

    /// Forward the stable due key without passing request credentials.
    fn capture_adjustment_maintenance(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'_, Database, Values>,
        due: MaintenanceCaptureDue,
    ) -> Result<(), TaskFailure> {
        repository
            .with_retained_resources(|database, values| self.maintenance.run(due, database, values))
    }
}
