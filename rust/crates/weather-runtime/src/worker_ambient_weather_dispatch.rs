//! Execute configured Ambient Weather sources through the retained worker pool.

use std::fmt::Display;

use weather_contracts::{
    JsonValue, Nullable, PublicStationAdapter, PublicStationConfiguration, SourceCapability,
};
use weather_database::{
    ScheduledCheckpointReadPort, ScheduledIngestionCommandPort, ScheduledRunFailureCommandPort,
    ScheduledRunStartCommandPort,
};
use weather_providers::{
    DEFAULT_PROVIDER_MAX_ATTEMPTS, DEFAULT_PROVIDER_TIMEOUT_MS, MAX_PROVIDER_BODY_BYTES,
    ProviderControls, ProviderIo,
};
use weather_worker::{
    AMBIENT_WEATHER_ADAPTER_VERSION, AmbientWeatherCapability, AmbientWeatherProviderCursor,
    AmbientWeatherRangeRequest, AmbientWeatherResponseMetadata, AmbientWeatherRuntimeConfiguration,
    AmbientWeatherSourceKind, NormalizedAmbientWeatherRecord, ScheduledProvider,
    ScheduledProviderResult, SourceRunResult, TaskFailure, WorkerDueSource,
    WorkerScheduledSourceDispatcher, run_scheduled_ambient_weather_source_after_lock,
};

use crate::{
    AmbientWeatherAttemptPayload, AmbientWeatherDatabaseRepository, AmbientWeatherRuntimeValues,
    OpenMeteoDatabaseBoundary, RetryingAmbientWeatherProvider, WorkerIterationDatabaseRepository,
    WorkerSourceMaterialIdentity, plan_worker_ingestion_deadlines,
};

/// Approved request-bound Ambient exchange created only after the running row exists.
pub trait WorkerAmbientWeatherTransport {
    type Exchange<'a>: ProviderIo<AmbientWeatherAttemptPayload>
    where
        Self: 'a;

    /// Bind one typed range request to an exchange whose payload checksum is the admitted
    /// SHA-256 of each raw response checksum plus a newline, in response order.
    fn bind_request(
        &mut self,
        request: &AmbientWeatherRangeRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure>;
}

/// Scheduled Ambient dispatch with future public-station variants delegated unchanged.
pub struct WorkerAmbientWeatherDispatcher<Transport, Other> {
    configurations: Vec<AmbientWeatherRuntimeConfiguration>,
    controls: ProviderControls,
    other: Other,
    transport: Option<Transport>,
}

impl<Transport, Other> WorkerAmbientWeatherDispatcher<Transport, Other> {
    /// Freeze generated Ambient identities with the production response budget.
    pub fn from_configuration(
        configuration: &PublicStationConfiguration,
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

    /// Freeze generated Ambient identities with explicit bounded fixture controls.
    pub fn with_controls(
        configuration: &PublicStationConfiguration,
        material: &mut impl WorkerSourceMaterialIdentity,
        transport: Option<Transport>,
        other: Other,
        controls: ProviderControls,
    ) -> Result<Self, TaskFailure> {
        let mut configurations = Vec::new();
        // preserve station and source order so the first matching key remains authoritative
        for station in &configuration.stations {
            for source in &station.sources {
                // leave other public-station variants to the downstream dispatcher
                if source.adapter != PublicStationAdapter::AmbientWeather {
                    continue;
                }
                let maximum_chunk_days =
                    u16::try_from(source.maximumChunkDays).map_err(|_| TaskFailure {
                        attempts: 0,
                        code: "ambient_maximum_chunk_days_invalid".to_owned(),
                    })?;
                let material_config_identity =
                    material.material_identity(&JsonValue::Object(source.adapterConfig.clone()))?;
                let device_id = adapter_string(&source.adapterConfig, "deviceId")?;
                let mac_address = adapter_string(&source.adapterConfig, "macAddress")?;
                configurations.push(AmbientWeatherRuntimeConfiguration {
                    adapter_contract_version: AMBIENT_WEATHER_ADAPTER_VERSION,
                    capabilities: ambient_capabilities(&source.capabilities),
                    device_id,
                    mac_address,
                    material_config_identity,
                    maximum_chunk_days,
                    model: nullable_text(&station.model),
                    provider_key: source.providerKey.clone(),
                    serial: nullable_text(&station.serial),
                    site_slug: configuration.siteKey.clone(),
                    source_active: source.active,
                    source_config_fingerprint: source.fingerprint.clone(),
                    source_key: source.key.clone(),
                    source_kind: AmbientWeatherSourceKind::PhysicalSensor,
                    station_slug: station.key.clone(),
                    timezone: station.timezone.clone(),
                });
            }
        }
        Ok(Self {
            configurations,
            controls,
            other,
            transport,
        })
    }
}

impl<'a, Database, Values, Transport, Other>
    WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>
    for WorkerAmbientWeatherDispatcher<Transport, Other>
where
    Database: OpenMeteoDatabaseBoundary,
    Values: AmbientWeatherRuntimeValues,
    Transport: WorkerAmbientWeatherTransport,
    Other: WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>,
    <Database::Session as ScheduledCheckpointReadPort>::Error: Display,
    <Database::Session as ScheduledRunStartCommandPort>::Error: Display,
    <Database::Session as ScheduledIngestionCommandPort>::Error: Display,
    <Database::Session as ScheduledRunFailureCommandPort>::Error: Display,
{
    /// Execute Ambient through one retained source session and delegate other families.
    fn run_source<Clock: FnMut() -> i64>(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'a, Database, Values>,
        source: &WorkerDueSource,
        clock: &mut Clock,
    ) -> Result<SourceRunResult, TaskFailure> {
        // preserve fallback routing without consuming an execution clock
        let WorkerDueSource::Ambient(source) = source else {
            return self.other.run_source(repository, source, clock);
        };
        let _execution_started_at_ms = clock();
        let configuration = self
            .configurations
            .iter()
            .find(|configuration| configuration.source_key == source.source_key)
            .ok_or_else(|| TaskFailure {
                attempts: 0,
                code: "ambient_weather_runtime_source_missing".to_owned(),
            })?;
        let (database, values) = repository.scheduled_resources();
        let mut scheduled = AmbientWeatherDatabaseRepository::new(database, values);
        let controls = self.controls;
        let mut provider = RequestBoundWorkerAmbientProvider {
            controls,
            deadline_at_ms: None,
            transport: &mut self.transport,
        };
        run_scheduled_ambient_weather_source_after_lock(
            source,
            configuration,
            &mut scheduled,
            &mut provider,
            clock,
            // derive provider and durable deadlines from the locked start clock once
            |provider, now_ms| {
                let deadlines = plan_worker_ingestion_deadlines(now_ms, controls)?;
                provider.deadline_at_ms = Some(deadlines.provider_deadline_at_ms);
                Ok(deadlines.run_deadline_at_ms)
            },
        )
    }
}

struct RequestBoundWorkerAmbientProvider<'a, Transport> {
    controls: ProviderControls,
    deadline_at_ms: Option<i64>,
    transport: &'a mut Option<Transport>,
}

impl<Transport: WorkerAmbientWeatherTransport> ScheduledProvider
    for RequestBoundWorkerAmbientProvider<'_, Transport>
{
    type Cursor = Option<AmbientWeatherProviderCursor>;
    type Record = NormalizedAmbientWeatherRecord;
    type Request = AmbientWeatherRangeRequest;
    type ResponseMetadata = AmbientWeatherResponseMetadata;

    /// Bind one physical exchange only after the durable run and deadline exist.
    fn fetch(
        &mut self,
        request: Self::Request,
    ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
        // reject traffic until the locked clock admits its absolute provider deadline
        let Some(deadline_at_ms) = self.deadline_at_ms else {
            return Err(TaskFailure {
                attempts: 0,
                code: "ambient_weather_provider_deadline_missing".to_owned(),
            });
        };
        // preserve an ordinary unavailable-provider failure after durable start
        let Some(transport) = self.transport.as_mut() else {
            return Err(TaskFailure {
                attempts: 0,
                code: "provider_unavailable".to_owned(),
            });
        };
        let io = transport.bind_request(&request)?;
        RetryingAmbientWeatherProvider::with_controls(io, self.controls, Some(deadline_at_ms))
            .fetch(request)
    }
}

/// Select only Ambient capabilities understood by its runtime configuration.
fn ambient_capabilities(
    capabilities: &[SourceCapability],
) -> std::collections::BTreeSet<AmbientWeatherCapability> {
    let mut admitted = std::collections::BTreeSet::new();
    // retain catalog order while projecting into the closed runtime capability set
    for capability in capabilities {
        match capability {
            SourceCapability::Current => {
                admitted.insert(AmbientWeatherCapability::Current);
            }
            SourceCapability::Historical => {
                admitted.insert(AmbientWeatherCapability::Historical);
            }
            SourceCapability::Forecast | SourceCapability::Stream => {}
        }
    }
    admitted
}

/// Preserve generated nullable station text without adding defaults.
fn nullable_text(value: &Nullable<String>) -> Option<String> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(value.clone()),
    }
}

/// Read one validated UTF-16 string field from generated ordered adapter material.
fn adapter_string(entries: &[(Vec<u16>, JsonValue)], field: &str) -> Result<String, TaskFailure> {
    let field_utf16: Vec<u16> = field.encode_utf16().collect();
    let value = entries
        .iter()
        .find_map(|(key, value)| (key == &field_utf16).then_some(value));
    // require the generated validated material to retain the declared string field
    let Some(JsonValue::String(value)) = value else {
        return Err(TaskFailure {
            attempts: 0,
            code: "ambient_adapter_material_invalid".to_owned(),
        });
    };
    String::from_utf16(value).map_err(|_| TaskFailure {
        attempts: 0,
        code: "ambient_adapter_material_invalid".to_owned(),
    })
}
