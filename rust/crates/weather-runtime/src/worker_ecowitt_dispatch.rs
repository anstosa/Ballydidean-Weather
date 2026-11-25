//! Execute configured Ecowitt scheduled tasks through the discovery and heartbeat pool.

use std::fmt::Display;

use weather_contracts::EcowittConfiguration;
use weather_database::{
    ScheduledCheckpointReadPort, ScheduledIngestionCommandPort, ScheduledRunFailureCommandPort,
    ScheduledRunStartCommandPort,
};
use weather_providers::ProviderControls;
use weather_worker::{
    ECOWITT_LOCAL_LIVE_ADAPTER_VERSION, EcowittCanonicalMetadataAdapter, EcowittCapability,
    EcowittProviderCursor, EcowittResponseMetadata, EcowittRuntimeConfiguration,
    EcowittScheduledRequest, EcowittSourceKind, EcowittTimeAdapter, NormalizedEcowittRecord,
    ScheduledProvider, ScheduledProviderResult, SourceRunResult, TaskFailure, WorkerDueSource,
    WorkerScheduledSourceDispatcher, run_scheduled_ecowitt_source_after_lock,
};

use crate::{
    EcowittDatabaseRepository, EcowittProviderIo, EcowittRuntimeValues, OpenMeteoDatabaseBoundary,
    RetryingEcowittProvider, WorkerIterationDatabaseRepository, WorkerSourceMaterialIdentity,
    plan_worker_ingestion_deadlines,
};

/// Request-bound Ecowitt transport and admitted normalization owners.
pub struct WorkerEcowittExchange<Io, Time, Metadata> {
    pub io: Io,
    pub metadata: Metadata,
    pub time: Time,
}

/// Binding yields the three owners together or one typed worker failure.
pub type WorkerEcowittBindResult<Io, Time, Metadata> =
    Result<WorkerEcowittExchange<Io, Time, Metadata>, TaskFailure>;

/// Approved owner for one Ecowitt request's IO, civil time, and metadata evidence.
pub trait WorkerEcowittTransport {
    type Io<'a>: EcowittProviderIo
    where
        Self: 'a;
    type Time<'a>: EcowittTimeAdapter
    where
        Self: 'a;
    type Metadata<'a>: EcowittCanonicalMetadataAdapter
    where
        Self: 'a;

    /// Bind all request-specific owners only after the durable run row exists.
    fn bind_request(
        &mut self,
        request: &EcowittScheduledRequest,
    ) -> WorkerEcowittBindResult<Self::Io<'_>, Self::Time<'_>, Self::Metadata<'_>>;
}

/// Execute scheduled Ecowitt local-live work and delegate every other source family.
pub struct WorkerEcowittDispatcher<Transport, Other> {
    configurations: Vec<EcowittRuntimeConfiguration>,
    other: Other,
    transport: Transport,
}

impl<Transport, Other> WorkerEcowittDispatcher<Transport, Other> {
    /// Freeze station identities from the original generated configuration.
    pub fn from_configuration(
        configuration: &EcowittConfiguration,
        material: &mut impl WorkerSourceMaterialIdentity,
        transport: Transport,
        other: Other,
    ) -> Result<Self, TaskFailure> {
        let mut configurations = Vec::new();
        // preserve catalog order because due-row activity was admitted during boot synchronization
        for station in &configuration.stations {
            configurations.push(EcowittRuntimeConfiguration {
                adapter_contract_version: ECOWITT_LOCAL_LIVE_ADAPTER_VERSION,
                capabilities: [EcowittCapability::Current].into(),
                expected_mac: station.expectedMac.clone(),
                gateway_host: station.gatewayHost.clone(),
                material_config_identity: material.material_identity(&station.adapterConfig)?,
                model: station.model.clone(),
                provider_key: configuration.provider.key.clone(),
                site_slug: configuration.siteKey.clone(),
                source_config_fingerprint: station.fingerprint.clone(),
                source_key: station.sourceKey.clone(),
                source_kind: EcowittSourceKind::PhysicalSensor,
                station_slug: station.key.clone(),
                timezone: station.timezone.clone(),
            });
        }
        Ok(Self {
            configurations,
            other,
            transport,
        })
    }
}

impl<'a, Database, Values, Transport, Other>
    WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>
    for WorkerEcowittDispatcher<Transport, Other>
where
    Database: OpenMeteoDatabaseBoundary,
    Values: EcowittRuntimeValues,
    Transport: WorkerEcowittTransport,
    Other: WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>,
    <Database::Session as ScheduledCheckpointReadPort>::Error: Display,
    <Database::Session as ScheduledRunStartCommandPort>::Error: Display,
    <Database::Session as ScheduledIngestionCommandPort>::Error: Display,
    <Database::Session as ScheduledRunFailureCommandPort>::Error: Display,
{
    /// Execute one Ecowitt task with the retained iteration pool and locked start clock.
    fn run_source<Clock: FnMut() -> i64>(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'a, Database, Values>,
        source: &WorkerDueSource,
        clock: &mut Clock,
    ) -> Result<SourceRunResult, TaskFailure> {
        // delegate non-Ecowitt work without consuming its execution clock
        let WorkerDueSource::Ecowitt(source) = source else {
            return self.other.run_source(repository, source, clock);
        };
        let _execution_started_at_ms = clock();
        let configuration = self
            .configurations
            .iter()
            .find(|configuration| configuration.source_key == source.source_key)
            .ok_or_else(|| TaskFailure {
                attempts: 0,
                code: "ecowitt_runtime_source_missing".to_owned(),
            })?;
        let (database, values) = repository.scheduled_resources();
        let mut scheduled = EcowittDatabaseRepository::new(database, values);
        let mut provider = RequestBoundWorkerEcowittProvider {
            deadline_at_ms: None,
            transport: &mut self.transport,
        };
        run_scheduled_ecowitt_source_after_lock(
            source,
            configuration,
            &mut scheduled,
            &mut provider,
            clock,
            // use the general Node request budget while retaining Ecowitt endpoint controls
            |provider, now_ms| {
                let deadlines =
                    plan_worker_ingestion_deadlines(now_ms, ProviderControls::default())?;
                provider.deadline_at_ms = Some(deadlines.provider_deadline_at_ms);
                Ok(deadlines.run_deadline_at_ms)
            },
        )
    }
}

struct RequestBoundWorkerEcowittProvider<'a, Transport> {
    deadline_at_ms: Option<i64>,
    transport: &'a mut Transport,
}

impl<Transport: WorkerEcowittTransport> ScheduledProvider
    for RequestBoundWorkerEcowittProvider<'_, Transport>
{
    type Cursor = EcowittProviderCursor;
    type Record = NormalizedEcowittRecord;
    type Request = EcowittScheduledRequest;
    type ResponseMetadata = EcowittResponseMetadata;

    /// Open the request-bound transport and execute both provider endpoint controllers.
    fn fetch(
        &mut self,
        request: Self::Request,
    ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
        // refuse traffic until the locked clock has admitted an absolute provider deadline
        let Some(deadline_at_ms) = self.deadline_at_ms else {
            return Err(TaskFailure {
                attempts: 0,
                code: "ecowitt_provider_deadline_missing".to_owned(),
            });
        };
        let exchange = self.transport.bind_request(&request)?;
        RetryingEcowittProvider::new(
            exchange.io,
            Some(deadline_at_ms),
            exchange.time,
            exchange.metadata,
        )
        .fetch(request)
    }
}
