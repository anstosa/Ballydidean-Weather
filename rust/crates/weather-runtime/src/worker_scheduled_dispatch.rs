//! Execute Open-Meteo scheduled tasks through the discovery and heartbeat pool.

use std::fmt::Display;

use weather_contracts::{SiteConfiguration, SourceCapability, SourceKind};
use weather_database::{
    ScheduledCheckpointReadPort, ScheduledIngestionCommandPort, ScheduledRunFailureCommandPort,
    ScheduledRunStartCommandPort, SerializedJson,
};
use weather_providers::{ProviderControls, ProviderIo, ProviderRequestPolicy};
use weather_worker::{
    NormalizedOpenMeteoRecord, OPEN_METEO_CURRENT_ADAPTER_VERSION,
    OPEN_METEO_FORECAST_ADAPTER_VERSION, OpenMeteoCapability, OpenMeteoCursor,
    OpenMeteoRuntimeConfiguration, OpenMeteoScheduledSource, OpenMeteoSourceKind, ProviderRequest,
    ScheduledProvider, ScheduledProviderResult, SourceRunResult, TaskFailure, WorkerDueSource,
    WorkerScheduledSourceDispatcher, run_scheduled_source_after_lock,
};

use crate::{
    OpenMeteoAttemptPayload, OpenMeteoDatabaseBoundary, OpenMeteoDatabaseRepository,
    OpenMeteoForecastEvidenceValues, OpenMeteoRuntimeValues, RetryingOpenMeteoProvider,
    WorkerIterationDatabaseRepository, WorkerOpenMeteoForecastTransport,
    WorkerSourceMaterialIdentity, fetch_worker_open_meteo_forecast,
    plan_worker_ingestion_deadlines,
};

/// An approved transport creates a request-bound retry exchange only after the run commits.
pub trait WorkerOpenMeteoTransport {
    type Exchange<'a>: ProviderIo<OpenMeteoAttemptPayload>
    where
        Self: 'a;

    /// Bind the exact current-weather request to the loaded site coordinates.
    fn bind_request(
        &mut self,
        location: OpenMeteoWorkerLocation,
        request: &ProviderRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure>;
}

/// Read both coordinates from the loaded site, not the station or database source row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpenMeteoWorkerLocation {
    pub latitude: f64,
    pub longitude: f64,
}

/// Execute scheduled Open-Meteo current weather, delegating other configured providers.
pub struct WorkerOpenMeteoDispatcher<Transport, Other> {
    controls: ProviderControls,
    other: Other,
    site: ScheduledOpenMeteoSite,
    transport: Transport,
}

impl<Transport, Other> WorkerOpenMeteoDispatcher<Transport, Other> {
    /// Load the source identities for the independently retried current-weather product.
    pub fn from_site(
        site: &SiteConfiguration,
        material: &mut impl WorkerSourceMaterialIdentity,
        transport: Transport,
        other: Other,
        controls: ProviderControls,
    ) -> Result<Self, TaskFailure> {
        Ok(Self {
            controls,
            other,
            site: ScheduledOpenMeteoSite::load(site, material, OpenMeteoSourceKind::ModelCurrent)?,
            transport,
        })
    }

    /// Add forecast dispatch without reusing the single-current-request retry controller.
    pub fn with_forecast<Forecast, Evidence>(
        self,
        site: &SiteConfiguration,
        material: &mut impl WorkerSourceMaterialIdentity,
        transport: Forecast,
        evidence: Evidence,
    ) -> Result<WorkerOpenMeteoForecastDispatcher<Forecast, Evidence, Self>, TaskFailure> {
        WorkerOpenMeteoForecastDispatcher::from_site(
            site,
            material,
            transport,
            evidence,
            self.controls,
            self,
        )
    }
}

impl<'a, Database, Values, Transport, Other>
    WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>
    for WorkerOpenMeteoDispatcher<Transport, Other>
where
    Database: OpenMeteoDatabaseBoundary,
    Values: OpenMeteoRuntimeValues,
    Transport: WorkerOpenMeteoTransport,
    Other: WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>,
    <Database::Session as ScheduledCheckpointReadPort>::Error: Display,
    <Database::Session as ScheduledRunStartCommandPort>::Error: Display,
    <Database::Session as ScheduledIngestionCommandPort>::Error: Display,
    <Database::Session as ScheduledRunFailureCommandPort>::Error: Display,
{
    /// Execute the current-weather lane with its single bounded exchange.
    fn run_source<C: FnMut() -> i64>(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'a, Database, Values>,
        source: &WorkerDueSource,
        clock: &mut C,
    ) -> Result<SourceRunResult, TaskFailure> {
        // route the other source families without reading an extra execution clock
        let Some(source) = open_meteo_task(source, OpenMeteoSourceKind::ModelCurrent) else {
            return self.other.run_source(repository, source, clock);
        };
        self.site.run(
            repository,
            source,
            clock,
            self.controls,
            // bind only after the worker has committed the running row
            |location, request, policy| {
                let io = self.transport.bind_request(location, &request)?;
                RetryingOpenMeteoProvider::new(io, policy.controls, policy.deadline_at_ms)
                    .fetch(request)
            },
        )
    }
}

/// Execute scheduled Open-Meteo forecasts with a paired transport and one source session.
pub struct WorkerOpenMeteoForecastDispatcher<Transport, Evidence, Other> {
    controls: ProviderControls,
    evidence: Evidence,
    other: Other,
    site: ScheduledOpenMeteoSite,
    transport: Transport,
}

impl<Transport, Evidence, Other> WorkerOpenMeteoForecastDispatcher<Transport, Evidence, Other> {
    /// Load the forecast identities while keeping the fallback's own transport policy.
    pub fn from_site(
        site: &SiteConfiguration,
        material: &mut impl WorkerSourceMaterialIdentity,
        transport: Transport,
        evidence: Evidence,
        controls: ProviderControls,
        other: Other,
    ) -> Result<Self, TaskFailure> {
        Ok(Self {
            controls,
            evidence,
            other,
            site: ScheduledOpenMeteoSite::load(site, material, OpenMeteoSourceKind::Forecast)?,
            transport,
        })
    }
}

impl<'a, Database, Values, Transport, Evidence, Other>
    WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>
    for WorkerOpenMeteoForecastDispatcher<Transport, Evidence, Other>
where
    Database: OpenMeteoDatabaseBoundary,
    Values: OpenMeteoRuntimeValues,
    Transport: WorkerOpenMeteoForecastTransport,
    Evidence: OpenMeteoForecastEvidenceValues,
    Other: WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>,
    <Database::Session as ScheduledCheckpointReadPort>::Error: Display,
    <Database::Session as ScheduledRunStartCommandPort>::Error: Display,
    <Database::Session as ScheduledIngestionCommandPort>::Error: Display,
    <Database::Session as ScheduledRunFailureCommandPort>::Error: Display,
{
    /// Execute the committed forecast lane using separately bounded weather and air quality.
    fn run_source<C: FnMut() -> i64>(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'a, Database, Values>,
        source: &WorkerDueSource,
        clock: &mut C,
    ) -> Result<SourceRunResult, TaskFailure> {
        // route current weather and other families to the configured fallback
        let Some(source) = open_meteo_task(source, OpenMeteoSourceKind::Forecast) else {
            return self.other.run_source(repository, source, clock);
        };
        self.site.run(
            repository,
            source,
            clock,
            self.controls,
            // bind both requests once the single durable forecast run has started
            |location, request, policy| {
                fetch_worker_open_meteo_forecast(
                    &mut self.transport,
                    &mut self.evidence,
                    location,
                    request,
                    policy,
                )
            },
        )
    }
}

/// Select only the exact product owned by this dispatch lane.
fn open_meteo_task(
    source: &WorkerDueSource,
    kind: OpenMeteoSourceKind,
) -> Option<&OpenMeteoScheduledSource> {
    match source {
        WorkerDueSource::OpenMeteo(source) if source.source_kind == kind => Some(source),
        _ => None,
    }
}

struct ScheduledOpenMeteoSite {
    location: OpenMeteoWorkerLocation,
    sources: Vec<OpenMeteoRuntimeConfiguration>,
}

impl ScheduledOpenMeteoSite {
    /// Freeze typed source identities from the loaded TS catalog once per process.
    fn load(
        site: &SiteConfiguration,
        material: &mut impl WorkerSourceMaterialIdentity,
        source_kind: OpenMeteoSourceKind,
    ) -> Result<Self, TaskFailure> {
        let (contract_kind, required_capability, capability, adapter_version) = match source_kind {
            OpenMeteoSourceKind::ModelCurrent => (
                SourceKind::ModelCurrent,
                SourceCapability::Current,
                OpenMeteoCapability::Current,
                OPEN_METEO_CURRENT_ADAPTER_VERSION,
            ),
            OpenMeteoSourceKind::Forecast => (
                SourceKind::Forecast,
                SourceCapability::Forecast,
                OpenMeteoCapability::Forecast,
                OPEN_METEO_FORECAST_ADAPTER_VERSION,
            ),
        };
        let mut sources = Vec::new();
        // preserve catalog order and never rebuild runtime configuration from a due row
        for candidate in &site.sources {
            // the ordered due-source classifier checks the database active flag
            if candidate.sourceKind != contract_kind
                || !candidate.capabilities.contains(&required_capability)
            {
                continue;
            }
            sources.push(OpenMeteoRuntimeConfiguration {
                active: true,
                adapter_version,
                capabilities: [capability].into(),
                material_config_identity: material.material_identity(&candidate.adapterConfig)?,
                provider_key: site.provider.key.clone(),
                site_slug: site.site.key.clone(),
                source_config_fingerprint: candidate.fingerprint.clone(),
                source_key: candidate.key.clone(),
                source_kind,
                station_slug: site.station.key.clone(),
                timezone: site.site.timezone.clone(),
            });
        }
        Ok(Self {
            location: OpenMeteoWorkerLocation {
                latitude: site.site.latitude,
                longitude: site.site.longitude,
            },
            sources,
        })
    }

    /// Reborrow the same pool, and plan both absolute deadlines from the locked start clock.
    fn run<Database, Values, Clock, Fetch>(
        &self,
        repository: &mut WorkerIterationDatabaseRepository<'_, Database, Values>,
        source: &OpenMeteoScheduledSource,
        clock: &mut Clock,
        controls: ProviderControls,
        fetch: Fetch,
    ) -> Result<SourceRunResult, TaskFailure>
    where
        Clock: FnMut() -> i64,
        Database: OpenMeteoDatabaseBoundary,
        Values: OpenMeteoRuntimeValues,
        Fetch: FnMut(
            OpenMeteoWorkerLocation,
            ProviderRequest,
            ProviderRequestPolicy,
        ) -> ScheduledOpenMeteoBatch,
        <Database::Session as ScheduledCheckpointReadPort>::Error: Display,
        <Database::Session as ScheduledRunStartCommandPort>::Error: Display,
        <Database::Session as ScheduledIngestionCommandPort>::Error: Display,
        <Database::Session as ScheduledRunFailureCommandPort>::Error: Display,
    {
        let _execution_started_at = clock();
        let configuration = self
            .sources
            .iter()
            .find(|configuration| configuration.source_key == source.source_key)
            .ok_or_else(|| TaskFailure {
                attempts: 0,
                code: "open_meteo_runtime_source_missing".to_owned(),
            })?;
        let (database, values) = repository.scheduled_resources();
        let mut scheduled = OpenMeteoDatabaseRepository::new(database, values);
        let mut provider = RequestBoundWorkerOpenMeteoProvider {
            location: self.location,
            policy: None,
            fetch,
        };
        run_scheduled_source_after_lock(
            source,
            configuration,
            &mut scheduled,
            &mut provider,
            clock,
            // the shared provider deadline does not include the durable finalization grace
            |provider, now_ms| {
                let deadlines = plan_worker_ingestion_deadlines(now_ms, controls)?;
                provider.policy = Some(ProviderRequestPolicy {
                    controls,
                    deadline_at_ms: Some(deadlines.provider_deadline_at_ms),
                });
                Ok(deadlines.run_deadline_at_ms)
            },
        )
    }
}

type ScheduledOpenMeteoBatch = ScheduledProviderResult<
    NormalizedOpenMeteoRecord,
    Option<SerializedJson>,
    Option<OpenMeteoCursor>,
>;

struct RequestBoundWorkerOpenMeteoProvider<Fetch> {
    fetch: Fetch,
    location: OpenMeteoWorkerLocation,
    policy: Option<ProviderRequestPolicy>,
}

impl<Fetch> ScheduledProvider for RequestBoundWorkerOpenMeteoProvider<Fetch>
where
    Fetch: FnMut(
        OpenMeteoWorkerLocation,
        ProviderRequest,
        ProviderRequestPolicy,
    ) -> ScheduledOpenMeteoBatch,
{
    type Cursor = Option<OpenMeteoCursor>;
    type Record = NormalizedOpenMeteoRecord;
    type Request = ProviderRequest;
    type ResponseMetadata = Option<SerializedJson>;

    /// Keep the requested product under the locked run's provider deadline.
    fn fetch(&mut self, request: ProviderRequest) -> ScheduledOpenMeteoBatch {
        // refuse to open an exchange before the source-run clock admits its budget
        let Some(policy) = self.policy else {
            return Err(TaskFailure {
                attempts: 0,
                code: "open_meteo_provider_deadline_missing".to_owned(),
            });
        };
        (self.fetch)(self.location, request, policy)
    }
}
