//! Dispatch Weather Underground, Netatmo, and PurpleAir through the shared worker pool.

use std::fmt::Display;

use weather_contracts::{
    JsonValue, Nullable, PublicStationAdapter, PublicStationConfiguration, SourceCapability,
};
use weather_database::{
    ScheduledCheckpointReadPort, ScheduledIngestionCommandPort, ScheduledRunFailureCommandPort,
    ScheduledRunStartCommandPort, Sha256Digest,
};
use weather_providers::{
    ProviderControls, ProviderFailureClassification, ProviderRequestPolicy, ProviderRunError,
};
use weather_worker::{
    PublicStationBackfillOperationError, PublicStationNormalizationError, ScheduledProvider,
    SourceRunResult, SupplementalPublicStationBatch, SupplementalPublicStationIdentity,
    SupplementalPublicStationRecord, SupplementalPublicStationResponseMetadata,
    SupplementalPublicStationScheduledConfiguration, SupplementalPublicStationScheduledRequest,
    TaskFailure, TideBackfillErrorClassification, WorkerDueSource, WorkerScheduledSourceDispatcher,
    run_scheduled_supplemental_public_station_source_after_lock,
};

pub use weather_worker::validate_supplemental_public_station_range as validate_worker_public_station_range;

use crate::{
    NetatmoWorkerIdentity, OpenMeteoDatabaseBoundary, OpenMeteoRuntimeValues,
    SupplementalPublicStationDatabaseRepository, WorkerIterationDatabaseRepository,
    WorkerNetatmoTransport, WorkerPurpleAirOperation, WorkerPurpleAirTransport,
    WorkerSourceMaterialIdentity, WorkerWundergroundTransport, fetch_worker_netatmo_operation,
    fetch_worker_wunderground_operation, plan_worker_ingestion_deadlines, provider_task_failure,
};

/// An approved SHA-256 boundary for a public station batch's ordered upstream digest material.
pub trait WorkerPublicStationEvidence {
    /// Hash the exact bytes supplied by the public response combiner.
    fn sha256(&mut self, material: &[u8]) -> Result<Sha256Digest, TaskFailure>;
}

/// Internal operation error that retains controller diagnoses until the owning runtime boundary.
pub(crate) enum SupplementalPublicStationOperationError {
    InvalidPayload {
        backfill_attempts: u32,
        message: String,
        scheduled_attempts: u32,
    },
    Task(TaskFailure),
    Normalization(PublicStationNormalizationError),
    Provider(ProviderRunError),
}

impl From<PublicStationNormalizationError> for SupplementalPublicStationOperationError {
    /// Keep exact decoded observation errors until the archive or scheduled consumer owns them.
    fn from(value: PublicStationNormalizationError) -> Self {
        Self::Normalization(value)
    }
}

impl From<TaskFailure> for SupplementalPublicStationOperationError {
    /// Retain pre-controller and normalization failures without manufacturing provider evidence.
    fn from(value: TaskFailure) -> Self {
        Self::Task(value)
    }
}

impl From<ProviderRunError> for SupplementalPublicStationOperationError {
    /// Retain the controller's exact classified failure until scheduled or archive ownership.
    fn from(value: ProviderRunError) -> Self {
        Self::Provider(value)
    }
}

impl SupplementalPublicStationOperationError {
    /// Retain an exact Node payload diagnosis while preserving the scheduled attempt contract.
    pub(crate) fn invalid_payload(
        scheduled_attempts: u32,
        backfill_attempts: u32,
        message: impl Into<String>,
    ) -> Self {
        Self::InvalidPayload {
            backfill_attempts,
            message: message.into(),
            scheduled_attempts,
        }
    }

    /// Preserve the existing scheduled worker's bounded attempts and code contract.
    pub(crate) fn into_task_failure(self) -> TaskFailure {
        match self {
            Self::InvalidPayload {
                scheduled_attempts, ..
            } => TaskFailure {
                attempts: scheduled_attempts,
                code: "invalid_payload".to_owned(),
            },
            Self::Task(failure) => failure,
            Self::Normalization(failure) => failure.into_task_failure(),
            Self::Provider(failure) => provider_task_failure(failure),
        }
    }

    /// Preserve classified controller evidence for the archive run's durable failure row.
    pub(crate) fn into_backfill_failure(self) -> PublicStationBackfillOperationError {
        match self {
            Self::InvalidPayload {
                backfill_attempts,
                message,
                ..
            } => PublicStationBackfillOperationError::provider(
                backfill_attempts.max(1),
                TideBackfillErrorClassification::InvalidPayload,
                "invalid_payload",
                message,
            ),
            Self::Task(failure) => backfill_task_failure(failure),
            Self::Normalization(PublicStationNormalizationError::InvalidPayload(message)) => {
                // typed Node invalidPayload uses its default one attempt after decoding
                PublicStationBackfillOperationError::provider(
                    1,
                    TideBackfillErrorClassification::InvalidPayload,
                    "invalid_payload",
                    message,
                )
            }
            Self::Normalization(PublicStationNormalizationError::Domain(message)) => {
                PublicStationBackfillOperationError::unexpected(message)
            }
            Self::Normalization(PublicStationNormalizationError::Task(failure)) => {
                backfill_task_failure(failure)
            }
            Self::Provider(ProviderRunError::InvalidControls(_)) => {
                PublicStationBackfillOperationError::unexpected("provider_invalid_controls")
            }
            Self::Provider(ProviderRunError::Provider(failure)) => {
                PublicStationBackfillOperationError::provider(
                    u32::from(failure.attempts.max(1)),
                    backfill_classification(failure.error.classification),
                    failure.error.code,
                    failure.error.message,
                )
            }
        }
    }
}

/// Frame each included raw response checksum exactly as Node, then hash through the admitted owner.
pub fn worker_public_station_digest(
    evidence: &mut impl WorkerPublicStationEvidence,
    responses: &[&Sha256Digest],
) -> Result<String, TaskFailure> {
    let mut material = Vec::with_capacity(responses.len().saturating_mul(65));
    // retain response order and the trailing newline on every included digest
    for response in responses {
        material.extend_from_slice(response.as_str().as_bytes());
        material.push(b'\n');
    }
    Ok(evidence.sha256(&material)?.as_str().to_owned())
}

/// Retain request owners across worker iterations, including PurpleAir's public map token cache.
pub struct WorkerSupplementalPublicStationOperations<
    WeatherUnderground,
    Netatmo,
    PurpleAir,
    Evidence,
> {
    weather_underground: WeatherUnderground,
    netatmo: Netatmo,
    purpleair: WorkerPurpleAirOperation<PurpleAir>,
    evidence: Evidence,
}

impl<WeatherUnderground, Netatmo, PurpleAir, Evidence>
    WorkerSupplementalPublicStationOperations<WeatherUnderground, Netatmo, PurpleAir, Evidence>
where
    PurpleAir: WorkerPurpleAirTransport,
{
    /// Start with fresh process state without binding or sending any provider requests.
    pub fn new(
        weather_underground: WeatherUnderground,
        netatmo: Netatmo,
        purpleair: PurpleAir,
        evidence: Evidence,
    ) -> Self {
        Self {
            weather_underground,
            netatmo,
            purpleair: WorkerPurpleAirOperation::new(purpleair),
            evidence,
        }
    }
}

/// Provider operation used by the one-shot public-station archive owner.
pub trait SupplementalPublicStationBackfillProvider {
    /// Fetch one admitted range with the already-planned absolute provider deadline.
    fn fetch(
        &mut self,
        request: SupplementalPublicStationScheduledRequest,
        deadline_at_ms: i64,
    ) -> Result<SupplementalPublicStationBatch, PublicStationBackfillOperationError>;
}

impl<WeatherUnderground, Netatmo, PurpleAir, Evidence> SupplementalPublicStationBackfillProvider
    for WorkerSupplementalPublicStationOperations<WeatherUnderground, Netatmo, PurpleAir, Evidence>
where
    WeatherUnderground: WorkerWundergroundTransport,
    Netatmo: WorkerNetatmoTransport,
    PurpleAir: WorkerPurpleAirTransport,
    Evidence: WorkerPublicStationEvidence,
{
    /// Reuse the scheduled provider operations without creating a second transport path.
    fn fetch(
        &mut self,
        request: SupplementalPublicStationScheduledRequest,
        deadline_at_ms: i64,
    ) -> Result<SupplementalPublicStationBatch, PublicStationBackfillOperationError> {
        dispatch_supplemental_public_station(
            self,
            request,
            ProviderRequestPolicy {
                controls: ProviderControls::default(),
                deadline_at_ms: Some(deadline_at_ms),
            },
        )
        .map_err(SupplementalPublicStationOperationError::into_backfill_failure)
    }
}

/// A configured dispatcher for the three remaining public station adapters.
pub struct WorkerSupplementalPublicStationDispatcher<
    WeatherUnderground,
    Netatmo,
    PurpleAir,
    Evidence,
    Other,
> {
    configurations: Vec<SupplementalPublicStationScheduledConfiguration>,
    operations:
        WorkerSupplementalPublicStationOperations<WeatherUnderground, Netatmo, PurpleAir, Evidence>,
    other: Other,
}

impl<WeatherUnderground, Netatmo, PurpleAir, Evidence, Other>
    WorkerSupplementalPublicStationDispatcher<
        WeatherUnderground,
        Netatmo,
        PurpleAir,
        Evidence,
        Other,
    >
{
    /// Freeze exact generated station and source identities once per loaded catalog.
    pub fn from_configuration(
        catalog: &PublicStationConfiguration,
        material: &mut impl WorkerSourceMaterialIdentity,
        operations: WorkerSupplementalPublicStationOperations<
            WeatherUnderground,
            Netatmo,
            PurpleAir,
            Evidence,
        >,
        other: Other,
    ) -> Result<Self, TaskFailure> {
        let mut configurations = Vec::new();
        // preserve the validated catalog's first-source order
        for station in &catalog.stations {
            for source in &station.sources {
                // Ambient is owned by its existing scheduled lane
                let Some(identity) = material_identity(source.adapter, &source.adapterConfig)?
                else {
                    continue;
                };
                let object = JsonValue::Object(source.adapterConfig.clone());
                configurations.push(SupplementalPublicStationScheduledConfiguration {
                    identity,
                    active: source.active,
                    supports_current: source.capabilities.contains(&SourceCapability::Current),
                    material_config_identity: material.material_identity(&object)?,
                    maximum_chunk_days: source.maximumChunkDays,
                    model: nullable_text(&station.model),
                    provider_key: source.providerKey.clone(),
                    serial: nullable_text(&station.serial),
                    site_slug: catalog.siteKey.clone(),
                    source_config_fingerprint: source.fingerprint.clone(),
                    source_key: source.key.clone(),
                    station_slug: station.key.clone(),
                    timezone: station.timezone.clone(),
                });
            }
        }
        Ok(Self {
            configurations,
            operations,
            other,
        })
    }
}

impl<'a, Database, Values, WeatherUnderground, Netatmo, PurpleAir, Evidence, Other>
    WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>
    for WorkerSupplementalPublicStationDispatcher<
        WeatherUnderground,
        Netatmo,
        PurpleAir,
        Evidence,
        Other,
    >
where
    Database: OpenMeteoDatabaseBoundary,
    Values: OpenMeteoRuntimeValues,
    WeatherUnderground: WorkerWundergroundTransport,
    Netatmo: WorkerNetatmoTransport,
    PurpleAir: WorkerPurpleAirTransport,
    Evidence: WorkerPublicStationEvidence,
    Other: WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>,
    <Database::Session as ScheduledCheckpointReadPort>::Error: Display,
    <Database::Session as ScheduledRunStartCommandPort>::Error: Display,
    <Database::Session as ScheduledIngestionCommandPort>::Error: Display,
    <Database::Session as ScheduledRunFailureCommandPort>::Error: Display,
{
    /// Retain one pool from discovery through provider operation, commit, and heartbeat.
    fn run_source<Clock: FnMut() -> i64>(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'a, Database, Values>,
        source: &WorkerDueSource,
        clock: &mut Clock,
    ) -> Result<SourceRunResult, TaskFailure> {
        // let other configured providers keep their own start clock
        let WorkerDueSource::PublicStation(source) = source else {
            return self.other.run_source(repository, source, clock);
        };
        let _execution_started_at = clock();
        let configuration = self
            .configurations
            .iter()
            .find(|configured| configured.source_key == source.source_key)
            .ok_or_else(|| TaskFailure {
                attempts: 0,
                code: "public_station_runtime_source_missing".to_owned(),
            })?;
        let (database, values) = repository.scheduled_resources();
        let mut scheduled = SupplementalPublicStationDatabaseRepository::new(database, values);
        let mut provider = BoundPublicStationProvider {
            operations: &mut self.operations,
            policy: None,
        };
        run_scheduled_supplemental_public_station_source_after_lock(
            source,
            configuration,
            &mut scheduled,
            &mut provider,
            clock,
            // use the general deadline plan rather than adding separate provider grace periods
            |provider, now_ms| {
                let controls = ProviderControls::default();
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

struct BoundPublicStationProvider<'a, W, N, P, E> {
    operations: &'a mut WorkerSupplementalPublicStationOperations<W, N, P, E>,
    policy: Option<ProviderRequestPolicy>,
}

impl<W, N, P, E> ScheduledProvider for BoundPublicStationProvider<'_, W, N, P, E>
where
    W: WorkerWundergroundTransport,
    N: WorkerNetatmoTransport,
    P: WorkerPurpleAirTransport,
    E: WorkerPublicStationEvidence,
{
    type Cursor = Option<i64>;
    type Record = SupplementalPublicStationRecord;
    type Request = SupplementalPublicStationScheduledRequest;
    type ResponseMetadata = SupplementalPublicStationResponseMetadata;

    /// Start only the provider operation matched by the locked, committed source.
    fn fetch(
        &mut self,
        request: Self::Request,
    ) -> Result<SupplementalPublicStationBatch, TaskFailure> {
        // do not bind or send requests without the locked absolute deadline
        let Some(policy) = self.policy else {
            return Err(TaskFailure {
                attempts: 0,
                code: "public_station_provider_deadline_missing".to_owned(),
            });
        };
        dispatch_supplemental_public_station(self.operations, request, policy)
            .map_err(SupplementalPublicStationOperationError::into_task_failure)
    }
}

// dispatch one checked identity through the same provider owner for scheduled and archive work
fn dispatch_supplemental_public_station<W, N, P, E>(
    operations: &mut WorkerSupplementalPublicStationOperations<W, N, P, E>,
    request: SupplementalPublicStationScheduledRequest,
    policy: ProviderRequestPolicy,
) -> Result<SupplementalPublicStationBatch, SupplementalPublicStationOperationError>
where
    W: WorkerWundergroundTransport,
    N: WorkerNetatmoTransport,
    P: WorkerPurpleAirTransport,
    E: WorkerPublicStationEvidence,
{
    match &request.identity {
        SupplementalPublicStationIdentity::WeatherUnderground {
            station_id,
            public_api_key,
        } => fetch_worker_wunderground_operation(
            &mut operations.weather_underground,
            &mut operations.evidence,
            &request,
            station_id,
            public_api_key,
            policy,
        ),
        SupplementalPublicStationIdentity::Netatmo {
            device_id,
            outdoor_module_id,
            rain_module_id,
            wind_module_id,
        } => fetch_worker_netatmo_operation(
            &mut operations.netatmo,
            &mut operations.evidence,
            &request,
            NetatmoWorkerIdentity {
                device_id,
                outdoor_module_id,
                rain_module_id,
                wind_module_id,
            },
            policy,
        ),
        SupplementalPublicStationIdentity::PurpleAir {
            map_version,
            sensor_index,
        } => operations.purpleair.fetch_operation(
            &mut operations.evidence,
            &request,
            map_version,
            *sensor_index,
            policy,
        ),
    }
}

// retain the worker failure's attempts and code through the archive catch boundary
fn backfill_task_failure(failure: TaskFailure) -> PublicStationBackfillOperationError {
    // retain the one canonical payload classification still present in TaskFailure
    let classification = if failure.code == "invalid_payload" {
        TideBackfillErrorClassification::InvalidPayload
    } else {
        // TaskFailure has already discarded the transport classification
        TideBackfillErrorClassification::Retryable
    };
    PublicStationBackfillOperationError::provider(
        failure.attempts.max(1),
        classification,
        failure.code.clone(),
        failure.code,
    )
}

// project the controller's exact classification into the durable ingestion contract
fn backfill_classification(
    classification: ProviderFailureClassification,
) -> TideBackfillErrorClassification {
    match classification {
        ProviderFailureClassification::InvalidPayload => {
            TideBackfillErrorClassification::InvalidPayload
        }
        ProviderFailureClassification::Permanent => TideBackfillErrorClassification::Permanent,
        ProviderFailureClassification::RateLimited => TideBackfillErrorClassification::RateLimited,
        ProviderFailureClassification::Retryable => TideBackfillErrorClassification::Retryable,
    }
}

/// Extract only already-decoded typed request fields from the approved catalog.
pub(crate) fn material_identity(
    adapter: PublicStationAdapter,
    fields: &[(Vec<u16>, JsonValue)],
) -> Result<Option<SupplementalPublicStationIdentity>, TaskFailure> {
    match adapter {
        PublicStationAdapter::AmbientWeather => Ok(None),
        PublicStationAdapter::WeatherUnderground => Ok(Some(
            SupplementalPublicStationIdentity::WeatherUnderground {
                station_id: required_text(fields, "stationId")?,
                public_api_key: required_text(fields, "publicApiKey")?,
            },
        )),
        PublicStationAdapter::Netatmo => Ok(Some(SupplementalPublicStationIdentity::Netatmo {
            device_id: required_text(fields, "deviceId")?,
            outdoor_module_id: required_text(fields, "outdoorModuleId")?,
            rain_module_id: required_text(fields, "rainModuleId")?,
            wind_module_id: required_text(fields, "windModuleId")?,
        })),
        PublicStationAdapter::Purpleair => Ok(Some(SupplementalPublicStationIdentity::PurpleAir {
            map_version: required_text(fields, "mapVersion")?,
            sensor_index: required_sensor_index(fields)?,
        })),
    }
}

/// Read one exact ASCII field name from the ordered generated adapter object.
fn field<'a>(fields: &'a [(Vec<u16>, JsonValue)], name: &str) -> Option<&'a JsonValue> {
    fields
        .iter()
        .find(|(key, _)| key.iter().copied().eq(name.encode_utf16()))
        .map(|(_, value)| value)
}

/// Require one nonempty string without introducing a new JSON codec.
pub(crate) fn required_text(
    fields: &[(Vec<u16>, JsonValue)],
    name: &str,
) -> Result<String, TaskFailure> {
    // reject impossible configuration drift before constructing request owners
    let Some(JsonValue::String(text)) = field(fields, name) else {
        return Err(invalid_catalog());
    };
    String::from_utf16(text)
        .ok()
        .filter(|text| !text.is_empty())
        .ok_or_else(invalid_catalog)
}

/// Read an exact positive JavaScript-safe PurpleAir sensor identifier.
fn required_sensor_index(fields: &[(Vec<u16>, JsonValue)]) -> Result<u64, TaskFailure> {
    // do not coerce text or fractional values to valid sensor identities
    match field(fields, "sensorIndex") {
        Some(JsonValue::Number(index))
            if index.fract() == 0.0 && (1.0..=9_007_199_254_740_991.0).contains(index) =>
        {
            Ok(*index as u64)
        }
        _ => Err(invalid_catalog()),
    }
}

/// Match the optional devices supplied by the TypeScript catalog.
pub(crate) fn nullable_text(value: &Nullable<String>) -> Option<String> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(value.clone()),
    }
}

/// Return a bounded failure that never exposes configured provider values.
fn invalid_catalog() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "invalid_public_station_runtime_configuration".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use weather_providers::{ProviderFailure, ProviderFailureClassification, ProviderRunError};
    use weather_worker::{
        NetatmoMeasurementBody, NetatmoMeasurementRow, NetatmoMeasurementSeries,
        NetatmoMeasurementStatus, NetatmoMeasurementValues, PublicStationBackfillOperationError,
        PublicStationNormalizationError, PublicStationNumber, PurpleAirDecodedHeader,
        PurpleAirDecodedHistory, PurpleAirDecodedRow, ScheduledWindow,
        SupplementalPublicStationInputs, SupplementalPublicStationProduct,
        TideBackfillErrorClassification, WeatherUndergroundMetricObservation,
        WeatherUndergroundObservation, WeatherUndergroundQcStatus,
        normalize_supplemental_public_station_batch_with_diagnostics,
    };

    use super::SupplementalPublicStationOperationError;

    /// Preserve the exact provider diagnosis used by a failed archive ingestion row.
    #[test]
    fn archive_failure_retains_controller_classification_code_message_and_attempts() {
        let failure = ProviderFailure::classified(
            2,
            ProviderFailureClassification::RateLimited,
            "provider_rate_limited",
            "Weather Underground throttled this source",
            Some(429),
        );
        let actual =
            SupplementalPublicStationOperationError::Provider(ProviderRunError::Provider(failure))
                .into_backfill_failure();

        assert_eq!(
            actual,
            PublicStationBackfillOperationError::provider(
                2,
                TideBackfillErrorClassification::RateLimited,
                "provider_rate_limited",
                "Weather Underground throttled this source",
            )
        );
    }

    const START_MS: i64 = 1_787_576_400_000;
    const EPOCH: i64 = START_MS / 1_000;

    // use the runtime's actual decoded batch entrypoint with a fixed admitted window
    fn decoded_batch(
        product: SupplementalPublicStationProduct<'_>,
    ) -> Result<weather_worker::SupplementalPublicStationBatch, PublicStationNormalizationError>
    {
        normalize_supplemental_public_station_batch_with_diagnostics(
            SupplementalPublicStationInputs {
                source_id: "source-fixture",
                model: None,
                serial: None,
                timezone: "UTC",
                window: ScheduledWindow {
                    start_ms: START_MS,
                    end_exclusive_ms: START_MS + 3_600_000,
                },
            },
            product,
            START_MS + 7_200_000,
            3,
            "fixture".to_owned(),
        )
    }

    // capture a deliberately malformed decoded response
    fn decoded_error(
        product: SupplementalPublicStationProduct<'_>,
    ) -> PublicStationNormalizationError {
        decoded_batch(product).expect_err("malformed fixture must fail")
    }

    // generate one valid decoded WU row before changing the targeted evidence
    fn wunderground_fixture() -> WeatherUndergroundObservation {
        let missing = PublicStationNumber::Missing;
        WeatherUndergroundObservation {
            epoch_seconds: EPOCH,
            humidity_avg_percent: missing,
            metric: Some(WeatherUndergroundMetricObservation {
                precip_rate_mm_per_hour: missing,
                pressure_max_hpa: missing,
                pressure_min_hpa: missing,
                temperature_avg_c: missing,
                wind_gust_high_kph: missing,
                wind_speed_avg_kph: missing,
            }),
            qc_status: WeatherUndergroundQcStatus::Missing,
            solar_radiation_high_wm2: missing,
            station_id: "KWAREHOUSE1".to_owned(),
            uv_high: missing,
            wind_direction_avg_degrees: missing,
        }
    }

    // normalize one malformed WU row through the retained archive error owner
    fn wunderground_error(row: WeatherUndergroundObservation) -> PublicStationNormalizationError {
        decoded_error(SupplementalPublicStationProduct::WeatherUnderground {
            station_id: "KWAREHOUSE1",
            days: &[vec![row]],
        })
    }

    // normalize one malformed Netatmo module alongside the other empty successful modules
    fn netatmo_error(
        status: NetatmoMeasurementStatus,
        body: NetatmoMeasurementBody,
    ) -> PublicStationNormalizationError {
        let mut modules: [NetatmoMeasurementSeries; 4] =
            std::array::from_fn(|_| NetatmoMeasurementSeries {
                status: NetatmoMeasurementStatus::Ok,
                body: NetatmoMeasurementBody::Null,
            });
        modules[0] = NetatmoMeasurementSeries { status, body };
        decoded_error(SupplementalPublicStationProduct::Netatmo {
            device_id: "aa:bb:cc:dd:ee:ff",
            modules: &modules,
        })
    }

    // normalize one malformed Netatmo value at the accepted provider epoch
    fn netatmo_values_error(values: NetatmoMeasurementValues) -> PublicStationNormalizationError {
        netatmo_error(
            NetatmoMeasurementStatus::Ok,
            NetatmoMeasurementBody::Rows(vec![NetatmoMeasurementRow {
                epoch_seconds: EPOCH,
                values,
            }]),
        )
    }

    // build one decoded PurpleAir row with an admitted schema and identity
    fn purpleair_fixture() -> PurpleAirDecodedRow {
        let missing = PublicStationNumber::Missing;
        PurpleAirDecodedRow {
            row_width_valid: true,
            sensor_index: Some(42),
            timestamp_ms: Some(START_MS),
            humidity_a: missing,
            humidity_b: missing,
            pm25_atm_a: missing,
            pm25_atm_b: missing,
            pressure_a: missing,
            pressure_b: missing,
            temperature_a: missing,
            temperature_b: missing,
        }
    }

    // normalize one malformed CSV response while retaining its exact decoded header state
    fn purpleair_error(
        header: PurpleAirDecodedHeader,
        rows: Vec<PurpleAirDecodedRow>,
    ) -> PublicStationNormalizationError {
        decoded_error(SupplementalPublicStationProduct::PurpleAir {
            sensor_index: 42,
            history: &PurpleAirDecodedHistory { header, rows },
        })
    }

    /// Compare all original malformed row diagnoses at the actual archive projection boundary.
    #[test]
    fn malformed_decoded_rows_keep_node_archive_diagnoses() {
        let oracle = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/public-station-row-errors-node-oracle.mjs");
        let output = Command::new("node")
            .arg(oracle)
            .output()
            .expect("Node original providers");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = String::from_utf8(output.stdout).expect("oracle TSV");
        let valid_header = PurpleAirDecodedHeader::Valid;
        let mut wu_bounds = wunderground_fixture();
        wu_bounds
            .metric
            .as_mut()
            .expect("metric object")
            .temperature_avg_c = PublicStationNumber::Number(100.0);
        let mut wu_overflow = wunderground_fixture();
        let metric = wu_overflow.metric.as_mut().expect("metric object");
        metric.pressure_max_hpa = PublicStationNumber::Number(1e308);
        metric.pressure_min_hpa = PublicStationNumber::Number(1e308);
        let cases = [
            (
                "wu-epoch",
                wunderground_error(WeatherUndergroundObservation {
                    epoch_seconds: 0,
                    ..wunderground_fixture()
                }),
            ),
            (
                "wu-identity",
                wunderground_error(WeatherUndergroundObservation {
                    station_id: "other".to_owned(),
                    ..wunderground_fixture()
                }),
            ),
            (
                "wu-object",
                wunderground_error(WeatherUndergroundObservation {
                    metric: None,
                    ..wunderground_fixture()
                }),
            ),
            (
                "wu-qc",
                wunderground_error(WeatherUndergroundObservation {
                    qc_status: WeatherUndergroundQcStatus::Invalid,
                    ..wunderground_fixture()
                }),
            ),
            ("wu-bounds", wunderground_error(wu_bounds)),
            ("wu-overflow", wunderground_error(wu_overflow)),
            (
                "net-root",
                netatmo_error(
                    NetatmoMeasurementStatus::InvalidObject,
                    NetatmoMeasurementBody::Null,
                ),
            ),
            (
                "net-status",
                netatmo_error(
                    NetatmoMeasurementStatus::Invalid,
                    NetatmoMeasurementBody::Null,
                ),
            ),
            (
                "net-body",
                netatmo_error(
                    NetatmoMeasurementStatus::Ok,
                    NetatmoMeasurementBody::Invalid,
                ),
            ),
            (
                "net-key",
                netatmo_values_error(NetatmoMeasurementValues::Invalid {
                    epoch_text: format!("0{EPOCH}"),
                }),
            ),
            (
                "net-short",
                netatmo_values_error(NetatmoMeasurementValues::Numbers(vec![
                    PublicStationNumber::Number(12.0),
                ])),
            ),
            (
                "net-cell",
                netatmo_values_error(NetatmoMeasurementValues::Numbers(vec![
                    PublicStationNumber::Invalid,
                    PublicStationNumber::Number(50.0),
                ])),
            ),
            (
                "pa-empty",
                purpleair_error(PurpleAirDecodedHeader::Missing, Vec::new()),
            ),
            (
                "pa-header",
                purpleair_error(PurpleAirDecodedHeader::Invalid, Vec::new()),
            ),
            (
                "pa-width",
                purpleair_error(
                    valid_header,
                    vec![PurpleAirDecodedRow {
                        row_width_valid: false,
                        ..purpleair_fixture()
                    }],
                ),
            ),
            (
                "pa-sensor",
                purpleair_error(
                    valid_header,
                    vec![PurpleAirDecodedRow {
                        sensor_index: None,
                        ..purpleair_fixture()
                    }],
                ),
            ),
            (
                "pa-identity",
                purpleair_error(
                    valid_header,
                    vec![PurpleAirDecodedRow {
                        sensor_index: Some(-1),
                        ..purpleair_fixture()
                    }],
                ),
            ),
            (
                "pa-timestamp",
                purpleair_error(
                    valid_header,
                    vec![PurpleAirDecodedRow {
                        timestamp_ms: None,
                        ..purpleair_fixture()
                    }],
                ),
            ),
            (
                "pa-cell",
                purpleair_error(
                    valid_header,
                    vec![PurpleAirDecodedRow {
                        pm25_atm_a: PublicStationNumber::Invalid,
                        ..purpleair_fixture()
                    }],
                ),
            ),
            (
                "pa-overflow",
                purpleair_error(
                    valid_header,
                    vec![PurpleAirDecodedRow {
                        pressure_a: PublicStationNumber::Number(1e308),
                        pressure_b: PublicStationNumber::Number(1e308),
                        ..purpleair_fixture()
                    }],
                ),
            ),
        ];
        assert_eq!(output.lines().count(), cases.len());

        // compare each exact failure against Node and retain the scheduled reduction separately
        for ((name, failure), line) in cases.into_iter().zip(output.lines()) {
            let fields = line.split('\t').collect::<Vec<_>>();
            let [fixture, attempts, classification, code, message] = fields.as_slice() else {
                panic!("unexpected oracle diagnostic")
            };
            assert_eq!(name, *fixture);
            let scheduled = failure.clone().into_task_failure();
            assert_eq!(scheduled.attempts, 0);
            assert_eq!(scheduled.code, *code);
            let actual =
                SupplementalPublicStationOperationError::from(failure).into_backfill_failure();
            // the archive catch preserves typed provider failures and coerces plain metric errors
            if *classification == "invalid_payload" {
                assert_eq!(
                    actual,
                    PublicStationBackfillOperationError::provider(
                        attempts.parse().expect("attempt count"),
                        TideBackfillErrorClassification::InvalidPayload,
                        *code,
                        *message,
                    ),
                    "{name}"
                );
            } else {
                assert_eq!(
                    (*attempts, *classification, *code),
                    ("1", "retryable", "provider_unavailable")
                );
                assert_eq!(
                    actual,
                    PublicStationBackfillOperationError::unexpected(*message),
                    "{name}"
                );
            }
        }
    }

    /// Preserve out-of-window filtering even at the maximum JavaScript safe integer.
    #[test]
    fn extreme_safe_epoch_padding_matches_node_without_overflow() {
        const MAX_JS_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
        let oracle = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/public-station-row-errors-node-oracle.mjs");
        let output = Command::new("node")
            .arg(oracle)
            .arg("--padding")
            .output()
            .expect("Node original providers");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let source = String::from_utf8(output.stdout).expect("oracle TSV");
        // all JS-safe seconds times 1000 fit into i64, including values beyond the JS Date range
        let wu = decoded_batch(SupplementalPublicStationProduct::WeatherUnderground {
            station_id: "KWAREHOUSE1",
            days: &[vec![WeatherUndergroundObservation {
                epoch_seconds: MAX_JS_SAFE_INTEGER,
                station_id: "malformed padding".to_owned(),
                metric: None,
                ..wunderground_fixture()
            }]],
        })
        .expect("safe padded WU epoch");
        let mut modules: [NetatmoMeasurementSeries; 4] =
            std::array::from_fn(|_| NetatmoMeasurementSeries {
                status: NetatmoMeasurementStatus::Ok,
                body: NetatmoMeasurementBody::Null,
            });
        modules[0].body = NetatmoMeasurementBody::Rows(vec![NetatmoMeasurementRow {
            epoch_seconds: MAX_JS_SAFE_INTEGER,
            values: NetatmoMeasurementValues::Invalid {
                epoch_text: MAX_JS_SAFE_INTEGER.to_string(),
            },
        }]);
        let net = decoded_batch(SupplementalPublicStationProduct::Netatmo {
            device_id: "aa:bb:cc:dd:ee:ff",
            modules: &modules,
        })
        .expect("safe padded Netatmo epoch");
        assert_eq!(
            source.lines().find(|line| line.starts_with("padding\t")),
            Some(format!("padding\t{}\t{}", wu.records.len(), net.records.len()).as_str())
        );
    }
}
