//! Execute configured Tempest scheduled sources through the retained worker pool.

use std::fmt::Display;

use weather_contracts::TempestConfiguration;
use weather_database::{
    ScheduledCheckpointReadPort, ScheduledIngestionCommandPort, ScheduledRunFailureCommandPort,
    ScheduledRunStartCommandPort,
};
use weather_providers::{
    DEFAULT_PROVIDER_MAX_ATTEMPTS, DEFAULT_PROVIDER_TIMEOUT_MS, MAX_PROVIDER_BODY_BYTES,
    ProviderControls, ProviderIo,
};
use weather_worker::{
    ScheduledProvider, SourceRunResult, TEMPEST_SOURCE_CONTRACT_VERSION, TaskFailure,
    TempestObservationRequest, TempestRuntimeConfiguration, TempestSourceKind, WorkerDueSource,
    WorkerScheduledSourceDispatcher, run_scheduled_tempest_source_after_lock,
};

use crate::{
    OpenMeteoDatabaseBoundary, RetryingTempestProvider, TempestAttemptPayload,
    TempestDatabaseRepository, TempestRuntimeValues, WorkerIterationDatabaseRepository,
    WorkerSourceMaterialIdentity, plan_worker_ingestion_deadlines,
};

/// Approved request-bound Tempest exchange created only after the durable running row exists.
pub trait WorkerTempestTransport {
    type Exchange<'a>: ProviderIo<TempestAttemptPayload>
    where
        Self: 'a;

    /// Bind the exact typed observation request to one bounded exchange.
    fn bind_request(
        &mut self,
        request: &TempestObservationRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure>;
}

/// Scheduled Tempest dispatch with one frozen generated catalog and optional transport.
pub struct WorkerTempestDispatcher<Transport, Other> {
    controls: ProviderControls,
    configurations: Vec<TempestRuntimeConfiguration>,
    other: Other,
    transport: Option<Transport>,
}

impl<Transport, Other> WorkerTempestDispatcher<Transport, Other> {
    /// Freeze generated Tempest identities with the production provider response budget.
    pub fn from_configuration(
        configuration: &TempestConfiguration,
        material: &mut impl WorkerSourceMaterialIdentity,
        transport: Option<Transport>,
        other: Other,
    ) -> Result<Self, TaskFailure> {
        Self::with_controls(
            configuration,
            material,
            transport,
            other,
            ProviderControls {
                max_attempts: DEFAULT_PROVIDER_MAX_ATTEMPTS,
                max_body_bytes: MAX_PROVIDER_BODY_BYTES,
                timeout_ms: DEFAULT_PROVIDER_TIMEOUT_MS,
            },
        )
    }

    /// Freeze generated Tempest identities with explicit bounded fixture controls.
    pub fn with_controls(
        configuration: &TempestConfiguration,
        material: &mut impl WorkerSourceMaterialIdentity,
        transport: Option<Transport>,
        other: Other,
        controls: ProviderControls,
    ) -> Result<Self, TaskFailure> {
        let configurations = project_tempest_runtime_configurations(configuration, material)?;
        Ok(Self {
            controls,
            configurations,
            other,
            transport,
        })
    }
}

/// Freeze the same configuration identities for scheduled and historical Tempest operations.
pub(crate) fn project_tempest_runtime_configurations(
    configuration: &TempestConfiguration,
    material: &mut impl WorkerSourceMaterialIdentity,
) -> Result<Vec<TempestRuntimeConfiguration>, TaskFailure> {
    let mut configurations = Vec::with_capacity(configuration.stations.len());
    // project every admitted station once instead of rebuilding material per due row
    for station in &configuration.stations {
        configurations.push(TempestRuntimeConfiguration {
            adapter_contract_version: TEMPEST_SOURCE_CONTRACT_VERSION,
            device_id: station.deviceId,
            location_id: station.locationId,
            material_config_identity: material.material_identity(&station.adapterConfig)?,
            provider_key: configuration.provider.key.clone(),
            serial: station.serial.clone(),
            site_slug: configuration.siteKey.clone(),
            source_config_fingerprint: station.fingerprint.clone(),
            source_key: station.sourceKey.clone(),
            source_kind: TempestSourceKind::PhysicalSensor,
            station_slug: station.key.clone(),
            timezone: station.timezone.clone(),
        });
    }
    Ok(configurations)
}

impl<'a, Database, Values, Transport, Other>
    WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>
    for WorkerTempestDispatcher<Transport, Other>
where
    Database: OpenMeteoDatabaseBoundary,
    Values: TempestRuntimeValues,
    Transport: WorkerTempestTransport,
    Other: WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>,
    <Database::Session as ScheduledCheckpointReadPort>::Error: Display,
    <Database::Session as ScheduledRunStartCommandPort>::Error: Display,
    <Database::Session as ScheduledIngestionCommandPort>::Error: Display,
    <Database::Session as ScheduledRunFailureCommandPort>::Error: Display,
{
    /// Execute Tempest through one source session and delegate every other family unchanged.
    fn run_source<C: FnMut() -> i64>(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'a, Database, Values>,
        source: &WorkerDueSource,
        clock: &mut C,
    ) -> Result<SourceRunResult, TaskFailure> {
        // preserve fallback routing without consuming an execution clock
        let WorkerDueSource::Tempest(source) = source else {
            return self.other.run_source(repository, source, clock);
        };
        let _execution_started_at_ms = clock();
        let configuration = self
            .configurations
            .iter()
            .find(|configuration| configuration.source_key == source.source_key)
            .ok_or_else(|| TaskFailure {
                attempts: 0,
                code: "tempest_runtime_source_missing".to_owned(),
            })?;
        let (database, values) = repository.scheduled_resources();
        let mut scheduled = TempestDatabaseRepository::new(database, values);
        let controls = self.controls;
        let mut provider = RequestBoundWorkerTempestProvider {
            controls,
            deadline_at_ms: None,
            transport: &mut self.transport,
        };
        run_scheduled_tempest_source_after_lock(
            source,
            configuration,
            &mut scheduled,
            &mut provider,
            clock,
            // derive the provider and durable deadlines from the locked run clock once
            |provider, now_ms| {
                let deadlines = plan_worker_ingestion_deadlines(now_ms, controls)?;
                provider.deadline_at_ms = Some(deadlines.provider_deadline_at_ms);
                Ok(deadlines.run_deadline_at_ms)
            },
        )
    }
}

struct RequestBoundWorkerTempestProvider<'a, Transport> {
    controls: ProviderControls,
    deadline_at_ms: Option<i64>,
    transport: &'a mut Option<Transport>,
}

impl<Transport: WorkerTempestTransport> ScheduledProvider
    for RequestBoundWorkerTempestProvider<'_, Transport>
{
    type Cursor = Option<weather_worker::TempestProviderCursor>;
    type Record = weather_worker::NormalizedTempestRecord;
    type Request = TempestObservationRequest;
    type ResponseMetadata = weather_worker::TempestResponseMetadata;

    /// Bind the exchange only after the durable run and its provider deadline exist.
    fn fetch(
        &mut self,
        request: Self::Request,
    ) -> weather_worker::ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor>
    {
        let Some(deadline_at_ms) = self.deadline_at_ms else {
            return Err(TaskFailure {
                attempts: 0,
                code: "tempest_provider_deadline_missing".to_owned(),
            });
        };
        let Some(transport) = self.transport.as_mut() else {
            return Err(TaskFailure {
                attempts: 0,
                code: "provider_unavailable".to_owned(),
            });
        };
        let io = transport.bind_request(&request)?;
        RetryingTempestProvider::new(io, self.controls, Some(deadline_at_ms)).fetch(request)
    }
}
