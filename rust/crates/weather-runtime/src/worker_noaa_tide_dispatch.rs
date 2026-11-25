//! Execute configured NOAA tide sources through the retained worker database pool.

use std::collections::BTreeSet;
use std::fmt::Display;

use weather_contracts::{
    JsonValue, SourceCapability, TideConfiguration, TideSourceConfigurationSourceKindEnum,
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
    NOAA_SCHEDULED_PREDICTION_DAYS, NOAA_TIDE_OBSERVATION_ADAPTER_VERSION,
    NOAA_TIDE_PREDICTION_ADAPTER_VERSION, NoaaTideCapability, NoaaTideRangeRequest,
    NoaaTideRuntimeConfiguration, NoaaTideSourceKind, ScheduledProvider, SourceRunResult,
    TaskFailure, WorkerDueSource, WorkerScheduledSourceDispatcher,
    run_scheduled_noaa_tide_source_after_lock,
};

use crate::{
    NoaaTideAttemptPayload, NoaaTideDatabaseRepository, NoaaTideRuntimeValues,
    OpenMeteoDatabaseBoundary, RetryingNoaaTideProvider, WorkerIterationDatabaseRepository,
    WorkerSourceMaterialIdentity, plan_worker_ingestion_deadlines,
};

const INVALID_TIDE_RUNTIME_CONFIGURATION: &str = "invalid_tide_runtime_configuration";

/// Approved NOAA exchange created only for a committed typed range request.
pub trait WorkerNoaaTideTransport {
    type Exchange<'a>: ProviderIo<NoaaTideAttemptPayload>
    where
        Self: 'a;

    /// Bind one exact observation or prediction request to its bounded exchange.
    fn bind_request(
        &mut self,
        request: &NoaaTideRangeRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure>;
}

/// Scheduled NOAA dispatch with frozen generated station/source configurations.
pub struct WorkerNoaaTideDispatcher<Transport, Other> {
    configurations: Vec<NoaaTideRuntimeConfiguration>,
    controls: ProviderControls,
    other: Other,
    transport: Transport,
}

impl<Transport, Other> WorkerNoaaTideDispatcher<Transport, Other> {
    /// Freeze the generated tide catalog with the production ten-megabyte provider budget.
    pub fn from_configuration(
        configuration: &TideConfiguration,
        material: &mut impl WorkerSourceMaterialIdentity,
        transport: Transport,
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

    /// Freeze the generated tide catalog with explicit bounded fixture controls.
    pub fn with_controls(
        configuration: &TideConfiguration,
        material: &mut impl WorkerSourceMaterialIdentity,
        transport: Transport,
        other: Other,
        controls: ProviderControls,
    ) -> Result<Self, TaskFailure> {
        let configurations = project_noaa_tide_runtime_configurations(configuration, material)?;
        Ok(Self {
            configurations,
            controls,
            other,
            transport,
        })
    }
}

/// Reuse the exact material and catalog projection for scheduled and historical tide execution.
pub(crate) fn project_noaa_tide_runtime_configurations(
    configuration: &TideConfiguration,
    material: &mut impl WorkerSourceMaterialIdentity,
) -> Result<Vec<NoaaTideRuntimeConfiguration>, TaskFailure> {
    let mut configurations = Vec::with_capacity(configuration.stations.len());
    // retain station/source order and active flags from the generated catalog
    for station in &configuration.stations {
        let source_kind = match station.source.sourceKind {
            TideSourceConfigurationSourceKindEnum::TideObservation => {
                NoaaTideSourceKind::Observation
            }
            TideSourceConfigurationSourceKindEnum::TidePrediction => NoaaTideSourceKind::Prediction,
        };
        let (adapter_contract_version, forecast_days) =
            adapter_contract(&station.source.adapterConfig, source_kind)?;
        let maximum_chunk_days =
            u16::try_from(station.source.maximumChunkDays).map_err(|_| invalid_configuration())?;
        let capabilities = tide_capabilities(&station.source.capabilities)?;
        let material_value = JsonValue::Object(station.source.adapterConfig.clone());
        configurations.push(NoaaTideRuntimeConfiguration {
            adapter_contract_version,
            capabilities,
            forecast_days,
            material_config_identity: material.material_identity(&material_value)?,
            maximum_chunk_days,
            provider_key: configuration.provider.key.clone(),
            site_slug: configuration.siteKey.clone(),
            source_active: station.source.active,
            source_config_fingerprint: station.source.fingerprint.clone(),
            source_key: station.source.key.clone(),
            source_kind,
            station_active: station.active,
            station_id: station.serial.clone(),
            station_slug: station.key.clone(),
            timezone: station.timezone.clone(),
        });
    }
    Ok(configurations)
}

impl<'a, Database, Values, Transport, Other>
    WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>
    for WorkerNoaaTideDispatcher<Transport, Other>
where
    Database: OpenMeteoDatabaseBoundary,
    Values: NoaaTideRuntimeValues,
    Transport: WorkerNoaaTideTransport,
    Other: WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>,
    <Database::Session as ScheduledCheckpointReadPort>::Error: Display,
    <Database::Session as ScheduledRunStartCommandPort>::Error: Display,
    <Database::Session as ScheduledIngestionCommandPort>::Error: Display,
    <Database::Session as ScheduledRunFailureCommandPort>::Error: Display,
{
    /// Execute NOAA through the shared source session and delegate every other family unchanged.
    fn run_source<C: FnMut() -> i64>(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'a, Database, Values>,
        source: &WorkerDueSource,
        clock: &mut C,
    ) -> Result<SourceRunResult, TaskFailure> {
        // preserve fallback routing without consuming an execution clock
        let WorkerDueSource::NoaaTide(source) = source else {
            return self.other.run_source(repository, source, clock);
        };
        let _execution_started_at_ms = clock();
        let configuration = self
            .configurations
            .iter()
            .find(|configuration| configuration.source_key == source.source_key)
            .ok_or_else(|| TaskFailure {
                attempts: 0,
                code: "noaa_tide_runtime_source_missing".to_owned(),
            })?;
        let (database, values) = repository.scheduled_resources();
        let mut scheduled = NoaaTideDatabaseRepository::new(database, values);
        let controls = self.controls;
        let mut provider = RequestBoundWorkerNoaaTideProvider {
            controls,
            deadline_at_ms: None,
            transport: &mut self.transport,
        };
        run_scheduled_noaa_tide_source_after_lock(
            source,
            configuration,
            &mut scheduled,
            &mut provider,
            clock,
            // derive both absolute budgets from the locked source clock once
            |provider, now_ms| {
                let deadlines = plan_worker_ingestion_deadlines(now_ms, controls)?;
                provider.deadline_at_ms = Some(deadlines.provider_deadline_at_ms);
                Ok(deadlines.run_deadline_at_ms)
            },
        )
    }
}

struct RequestBoundWorkerNoaaTideProvider<'a, Transport> {
    controls: ProviderControls,
    deadline_at_ms: Option<i64>,
    transport: &'a mut Transport,
}

impl<Transport: WorkerNoaaTideTransport> ScheduledProvider
    for RequestBoundWorkerNoaaTideProvider<'_, Transport>
{
    type Cursor = Option<weather_worker::NoaaTideProviderCursor>;
    type Record = weather_worker::NormalizedNoaaTideRecord;
    type Request = NoaaTideRangeRequest;
    type ResponseMetadata = weather_worker::NoaaTideResponseMetadata;

    /// Bind the NOAA exchange only after the durable run and deadline exist.
    fn fetch(
        &mut self,
        request: Self::Request,
    ) -> weather_worker::ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor>
    {
        let Some(deadline_at_ms) = self.deadline_at_ms else {
            return Err(TaskFailure {
                attempts: 0,
                code: "noaa_tide_provider_deadline_missing".to_owned(),
            });
        };
        let io = self.transport.bind_request(&request)?;
        RetryingNoaaTideProvider::with_controls(io, self.controls, Some(deadline_at_ms))
            .fetch(request)
    }
}

/// Project fixed source capabilities from the already validated generated contract.
fn tide_capabilities(
    values: &[SourceCapability],
) -> Result<BTreeSet<NoaaTideCapability>, TaskFailure> {
    let mut capabilities = BTreeSet::new();
    // preserve the generated capability set without accepting an unrelated stream product
    for value in values {
        capabilities.insert(match value {
            SourceCapability::Current => NoaaTideCapability::Current,
            SourceCapability::Forecast => NoaaTideCapability::Forecast,
            SourceCapability::Historical => NoaaTideCapability::Historical,
            SourceCapability::Stream => return Err(invalid_configuration()),
        });
    }
    Ok(capabilities)
}

/// Admit the fixed adapter contract and optional prediction horizon from its UTF-16 object.
fn adapter_contract(
    entries: &[(Vec<u16>, JsonValue)],
    source_kind: NoaaTideSourceKind,
) -> Result<(&'static str, Option<i64>), TaskFailure> {
    let expected_contract = match source_kind {
        NoaaTideSourceKind::Observation => NOAA_TIDE_OBSERVATION_ADAPTER_VERSION,
        NoaaTideSourceKind::Prediction => NOAA_TIDE_PREDICTION_ADAPTER_VERSION,
    };
    let contract = object_value(entries, "contractVersion")
        .and_then(json_string)
        .ok_or_else(invalid_configuration)?;
    // keep the static adapter identity only after the decoded value agrees
    if contract != expected_contract {
        return Err(invalid_configuration());
    }
    let forecast_days = match source_kind {
        NoaaTideSourceKind::Observation => {
            // observation material must not carry a prediction horizon
            if object_value(entries, "forecastDays").is_some() {
                return Err(invalid_configuration());
            }
            None
        }
        NoaaTideSourceKind::Prediction => {
            let days = object_value(entries, "forecastDays")
                .and_then(json_safe_integer)
                .ok_or_else(invalid_configuration)?;
            // retain the checked adapter's fixed scheduled prediction horizon
            if days != NOAA_SCHEDULED_PREDICTION_DAYS {
                return Err(invalid_configuration());
            }
            Some(days)
        }
    };
    Ok((expected_contract, forecast_days))
}

/// Find one exact UTF-16 object field without changing source order.
fn object_value<'a>(entries: &'a [(Vec<u16>, JsonValue)], key: &str) -> Option<&'a JsonValue> {
    entries
        .iter()
        .find(|(candidate, _)| candidate.iter().copied().eq(key.encode_utf16()))
        .map(|(_, value)| value)
}

/// Decode one validated UTF-16 string field.
fn json_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(value) => String::from_utf16(value).ok(),
        _ => None,
    }
}

/// Convert one generated finite safe integer without rounding.
fn json_safe_integer(value: &JsonValue) -> Option<i64> {
    match value {
        JsonValue::Number(value)
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= -(9_007_199_254_740_991_f64)
                && *value <= 9_007_199_254_740_991_f64 =>
        {
            Some(*value as i64)
        }
        _ => None,
    }
}

/// Return one zero-attempt generated-catalog projection failure.
fn invalid_configuration() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: INVALID_TIDE_RUNTIME_CONFIGURATION.to_owned(),
    }
}
