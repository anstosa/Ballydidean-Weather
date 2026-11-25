//! Connect the prospective rain collector to the retained worker repository.

use weather_worker::{
    RAIN_COLLECTION_POLICY, RAIN_COLLECTION_STATIONS, RainCaptureResult, RainCollectionClock,
    RainCollectionExecution, RainCollectionPacer, RainCollectionProvider, RainCollectionRepository,
    RainCollectionTimeAdapter, TaskFailure, collect_rain_evidence,
};

use crate::RetainedWorkerSidecarTask;

/// Borrow the configured durable claim/receipt repository only for an admitted capture.
pub trait WorkerRainCaptureRepositoryBinding<Database, Values> {
    type Repository<'a>: RainCollectionRepository
    where
        Self: 'a,
        Database: 'a,
        Values: 'a;

    /// Use the retained worker pool and approved value owner without opening a second pool.
    fn bind<'a>(
        &'a mut self,
        database: &'a mut Database,
        values: &'a mut Values,
    ) -> Result<Self::Repository<'a>, TaskFailure>;
}

/// One concrete rain-evidence job using the source-owned fixed policy and catalog.
pub struct WorkerRainCaptureTask<Binding, Clock, Time, Provider, Pacer> {
    binding: Binding,
    clock: Clock,
    pacer: Pacer,
    provider: Provider,
    release: String,
    time: Time,
}

impl<Binding, Clock, Time, Provider, Pacer>
    WorkerRainCaptureTask<Binding, Clock, Time, Provider, Pacer>
{
    /// Retain configured ports without making provider or database requests.
    pub fn new(
        binding: Binding,
        clock: Clock,
        time: Time,
        provider: Provider,
        pacer: Pacer,
        release: String,
    ) -> Self {
        Self {
            binding,
            clock,
            pacer,
            provider,
            release,
            time,
        }
    }
}

impl<Database, Values, Binding, Clock, Time, Provider, Pacer>
    RetainedWorkerSidecarTask<Database, Values, ()>
    for WorkerRainCaptureTask<Binding, Clock, Time, Provider, Pacer>
where
    Binding: WorkerRainCaptureRepositoryBinding<Database, Values>,
    Clock: RainCollectionClock,
    Time: RainCollectionTimeAdapter,
    Provider: RainCollectionProvider,
    Pacer: RainCollectionPacer,
{
    type Output = RainCaptureResult;

    /// Execute the existing bounded planner, durable claim, provider and append sequence.
    fn run(
        &mut self,
        _input: (),
        database: &mut Database,
        values: &mut Values,
    ) -> Result<Self::Output, TaskFailure> {
        let mut repository = self.binding.bind(database, values)?;
        let result = collect_rain_evidence(&mut RainCollectionExecution {
            catalog: RAIN_COLLECTION_STATIONS,
            clock: &mut self.clock,
            pacer: &mut self.pacer,
            policy: &RAIN_COLLECTION_POLICY,
            provider: &mut self.provider,
            release: &self.release,
            repository: &mut repository,
            stations_authorized: RAIN_COLLECTION_POLICY.station_access_authorized,
            time_adapter: &mut self.time,
        })?;
        Ok(RainCaptureResult {
            failed: result.failed,
            valid: result.valid,
        })
    }
}
