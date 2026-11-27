//! One-shot application binding for the typed Open-Meteo Previous Runs backfill.

use std::collections::BTreeSet;
use std::fmt::Display;

use weather_contracts::{
    JsonValue, NormalizedForecastAnchorRecord, Nullable, SiteConfiguration, SourceCapability,
    SourceKind,
};
use weather_database::{
    AdmittedForecastAnchorBackfillIdentity, AdmittedIngestionError,
    CompleteForecastAnchorBackfill as DatabaseForecastAnchorCompletion,
    FailForecastAnchorBackfill as DatabaseForecastAnchorFailure, ForecastAnchorBackfillCommandPort,
    ForecastAnchorBackfillPoolPort, ForecastAnchorRecordStorageEvidence,
    OpenMeteoBackfillSourceReadError, OpenMeteoBackfillSourceReadPort,
    PreparedForecastAnchorRecord, RetainedSourceSession, SerializedJson, Sha256Digest,
    StartForecastAnchorBackfillRun, complete_forecast_anchor_backfill,
    fail_forecast_anchor_backfill, forecast_anchor_abandon_expired_query,
    forecast_anchor_successful_chunk_query, resolve_open_meteo_backfill_source,
    start_forecast_anchor_backfill_run,
};
use weather_worker::{
    ForecastAnchorAdapterConfiguration, ForecastAnchorBackfillArguments,
    ForecastAnchorBackfillChunkIdentity, ForecastAnchorBackfillCompletion,
    ForecastAnchorBackfillOperationError, ForecastAnchorBackfillProvider,
    ForecastAnchorBackfillProviderFailure, ForecastAnchorBackfillReport,
    ForecastAnchorBackfillRepository, ForecastAnchorBackfillRunStart, ForecastAnchorBackfillSource,
    ForecastAnchorBackfillTimeAdapter, ForecastAnchorCapability,
    ForecastAnchorRuntimeConfiguration, ForecastAnchorSourceKind,
    HistoricalBackfillCommandTimeAdapter, OpenMeteoArchiveCleanupFailure,
    OpenMeteoBackfillArgumentError, TaskFailure, execute_forecast_anchor_backfill,
    parse_open_meteo_backfill_arguments, validate_forecast_anchor_backfill_source,
};

use crate::WorkerSourceMaterialIdentity;

const PREVIOUS_RUNS_SOURCE_KEY: &str = "open-meteo-previous-runs-v1";
const ARCHIVE_SOURCE_KEY: &str = "open-meteo-reanalysis-v1";

/// One persistence boundary whose failure retains worker ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastAnchorPersistenceOperation {
    ResumeRead,
    Acquire,
    Abandon,
    Start,
    Complete,
    FailureFinalize,
    Release,
}

/// One forecast-anchor epoch conversion role at the admitted time boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastAnchorTimeValue {
    AbandonAsOf,
    Deadline,
    IntervalEndExclusive,
    IntervalStart,
}

/// Canonical time, JSON, digest, and failure owners used before database commands.
pub trait ForecastAnchorBackfillRuntimeValues {
    /// Format one resolved epoch without adding a local date codec.
    fn utc_instant(
        &mut self,
        epoch_milliseconds: i64,
        value: ForecastAnchorTimeValue,
    ) -> Result<String, TaskFailure>;

    /// Admit one externally calculated SHA-256 value.
    fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure>;

    /// Serialize the already-projected request evidence without owning its shape.
    fn request_metadata_json(
        &mut self,
        metadata: &JsonValue,
    ) -> Result<Option<SerializedJson>, TaskFailure>;

    /// Prepare canonical JSON and content-hash evidence for one generated anchor.
    fn record_storage_evidence(
        &mut self,
        record: &NormalizedForecastAnchorRecord,
    ) -> Result<ForecastAnchorRecordStorageEvidence, TaskFailure>;

    /// Serialize the typed bounded response evidence.
    fn response_metadata_json(
        &mut self,
        metadata: &JsonValue,
    ) -> Result<Option<SerializedJson>, TaskFailure>;

    /// Admit the provider failure through the shared ingestion-error validator.
    fn ingestion_error(
        &mut self,
        failure: &ForecastAnchorBackfillProviderFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure>;

    /// Classify one persistence error for a task-level repository boundary.
    fn task_failure(
        &mut self,
        operation: ForecastAnchorPersistenceOperation,
        message: &str,
    ) -> TaskFailure;

    /// Classify one persistence error inside the committed chunk catch boundary.
    fn operation_failure(
        &mut self,
        operation: ForecastAnchorPersistenceOperation,
        message: &str,
    ) -> ForecastAnchorBackfillOperationError;

    /// Admit one already-redacted cleanup diagnostic without replacing the primary outcome.
    fn cleanup_failure(
        &mut self,
        operation: ForecastAnchorPersistenceOperation,
        message: &str,
    ) -> OpenMeteoArchiveCleanupFailure;
}

/// Worker repository backed by typed forecast-anchor SQL commands.
pub struct DatabaseForecastAnchorBackfillRepository<Database, Values> {
    database: Database,
    values: Values,
}

impl<Database, Values> DatabaseForecastAnchorBackfillRepository<Database, Values> {
    /// Bind the database and canonical value owners without performing I/O.
    pub const fn new(database: Database, values: Values) -> Self {
        Self { database, values }
    }

    /// Recover the bound owners after the one-shot lifecycle closes.
    pub fn into_parts(self) -> (Database, Values) {
        (self.database, self.values)
    }
}

impl<Database, Values> ForecastAnchorBackfillRepository
    for DatabaseForecastAnchorBackfillRepository<Database, Values>
where
    Database: ForecastAnchorBackfillPoolPort,
    Database::Error: Display,
    Values: ForecastAnchorBackfillRuntimeValues,
{
    type Session = Database::Session;

    /// Execute the exact six-bind successful chunk lookup before locking.
    fn has_successful_chunk(
        &mut self,
        identity: &ForecastAnchorBackfillChunkIdentity,
    ) -> Result<bool, TaskFailure> {
        let identity = admit_identity(identity, &mut self.values)?;
        self.database
            .has_successful_chunk(&forecast_anchor_successful_chunk_query(&identity))
            .map(|row_count| row_count == 1)
            .map_err(|error| {
                self.values.task_failure(
                    ForecastAnchorPersistenceOperation::ResumeRead,
                    &error.to_string(),
                )
            })
    }

    /// Acquire one exact retained source session and reject mismatched authority.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure> {
        let session = self
            .database
            .acquire_source_session(source_id)
            .map_err(|error| {
                self.values.task_failure(
                    ForecastAnchorPersistenceOperation::Acquire,
                    &error.to_string(),
                )
            })?;

        // release a session that does not carry the requested source authority
        match session {
            Some(session) if session.source_id() != source_id => {
                let _release = session.release();
                Err(TaskFailure {
                    attempts: 0,
                    code: "source_session_mismatch".to_owned(),
                })
            }
            session => Ok(session),
        }
    }

    /// Recover expired running rows through the exact retained-session query.
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        started_at_ms: i64,
    ) -> Result<(), ForecastAnchorBackfillOperationError> {
        let as_of = self
            .values
            .utc_instant(started_at_ms, ForecastAnchorTimeValue::AbandonAsOf)
            .map_err(operation_from_task_failure)?;
        session
            .abandon_expired_runs(&forecast_anchor_abandon_expired_query(
                session.source_id(),
                &as_of,
            ))
            .map(|_| ())
            .map_err(|error| {
                self.values.operation_failure(
                    ForecastAnchorPersistenceOperation::Abandon,
                    &error.to_string(),
                )
            })
    }

    /// Insert one exact committed backfill run before provider work.
    fn start_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        start: ForecastAnchorBackfillRunStart,
    ) -> Result<String, ForecastAnchorBackfillOperationError> {
        let input = StartForecastAnchorBackfillRun {
            adapter_version: start.adapter_version.to_owned(),
            attempts: 0,
            chunk_plan_version: start.chunk_plan_version.to_owned(),
            deadline_at: self
                .values
                .utc_instant(start.deadline_at_ms, ForecastAnchorTimeValue::Deadline)
                .map_err(operation_from_task_failure)?,
            interval_end_exclusive: self
                .values
                .utc_instant(
                    start.requested_end_exclusive_ms,
                    ForecastAnchorTimeValue::IntervalEndExclusive,
                )
                .map_err(operation_from_task_failure)?,
            interval_start: self
                .values
                .utc_instant(
                    start.requested_start_ms,
                    ForecastAnchorTimeValue::IntervalStart,
                )
                .map_err(operation_from_task_failure)?,
            request_metadata_json: self
                .values
                .request_metadata_json(&forecast_anchor_request_metadata(&start))
                .map_err(operation_from_task_failure)?,
            source_config_fingerprint: self
                .values
                .sha256_digest(&start.source_config_fingerprint)
                .map_err(operation_from_task_failure)?,
        };
        start_forecast_anchor_backfill_run(&input, session)
            .map(|run| run.id)
            .map_err(|error| {
                self.values.operation_failure(
                    ForecastAnchorPersistenceOperation::Start,
                    &error.to_string(),
                )
            })
    }

    /// Persist actual generated records and response evidence atomically.
    fn complete_forecast_anchor_backfill_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: ForecastAnchorBackfillCompletion,
    ) -> Result<(), ForecastAnchorBackfillOperationError> {
        let identity = admit_identity(&completion.identity, &mut self.values)
            .map_err(operation_from_task_failure)?;
        let mut records = Vec::with_capacity(completion.records.len());

        // preserve normalized provider order through the exact upsert batches
        for record in completion.records {
            let storage = self
                .values
                .record_storage_evidence(&record)
                .map_err(operation_from_task_failure)?;
            records.push(PreparedForecastAnchorRecord { record, storage });
        }
        let input = DatabaseForecastAnchorCompletion {
            attempts: u64::from(completion.attempts),
            identity,
            records,
            response_metadata_json: self
                .values
                .response_metadata_json(&completion.response_metadata.to_json_value())
                .map_err(operation_from_task_failure)?,
            run_id: completion.run_id,
            upstream_response_checksum: Some(
                self.values
                    .sha256_digest(&completion.upstream_response_checksum)
                    .map_err(operation_from_task_failure)?,
            ),
        };
        complete_forecast_anchor_backfill(&input, session).map_err(|error| {
            self.values.operation_failure(
                ForecastAnchorPersistenceOperation::Complete,
                &error.to_string(),
            )
        })
    }

    /// Atomically retain the exact provider failure and durable chunk outcome.
    fn fail_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        run_id: &str,
        identity: &ForecastAnchorBackfillChunkIdentity,
        failure: &ForecastAnchorBackfillProviderFailure,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
        // keep admission failures inside the secondary finalization diagnostic
        let error = match self.values.ingestion_error(failure) {
            Ok(error) => error,
            Err(error) => {
                return Err(self.values.cleanup_failure(
                    ForecastAnchorPersistenceOperation::FailureFinalize,
                    &error.code,
                ));
            }
        };
        // bind the exact failed chunk identity before entering the failure transaction
        let identity = match admit_identity(identity, &mut self.values) {
            Ok(identity) => identity,
            Err(error) => {
                return Err(self.values.cleanup_failure(
                    ForecastAnchorPersistenceOperation::FailureFinalize,
                    &error.code,
                ));
            }
        };
        let input = DatabaseForecastAnchorFailure {
            attempts: u64::from(failure.attempts),
            error,
            identity,
            run_id: run_id.to_owned(),
        };
        fail_forecast_anchor_backfill(&input, session).map_err(|error| {
            self.values.cleanup_failure(
                ForecastAnchorPersistenceOperation::FailureFinalize,
                &error.to_string(),
            )
        })
    }

    /// Release the retained database session last.
    fn release_source_session(
        &mut self,
        session: Self::Session,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
        session.release().map_err(|error| {
            self.values.cleanup_failure(
                ForecastAnchorPersistenceOperation::Release,
                &error.to_string(),
            )
        })
    }
}

// retain the original Previous Runs request evidence in its exact field order
fn forecast_anchor_request_metadata(start: &ForecastAnchorBackfillRunStart) -> JsonValue {
    JsonValue::Object(
        [
            ("end_date", start.end_date.as_str()),
            ("endpoint", start.endpoint),
            ("start_date", start.start_date.as_str()),
        ]
        .into_iter()
        // preserve names and string values without delegating object shape to the codec
        .map(|(key, value)| {
            (
                key.encode_utf16().collect(),
                JsonValue::String(value.encode_utf16().collect()),
            )
        })
        .collect(),
    )
}

// admit one worker chunk identity through shared time and digest owners
fn admit_identity<Values: ForecastAnchorBackfillRuntimeValues>(
    identity: &ForecastAnchorBackfillChunkIdentity,
    values: &mut Values,
) -> Result<AdmittedForecastAnchorBackfillIdentity, TaskFailure> {
    Ok(AdmittedForecastAnchorBackfillIdentity {
        adapter_version: identity.adapter_version.to_owned(),
        chunk_plan_version: identity.chunk_plan_version.to_owned(),
        interval_end_exclusive: values.utc_instant(
            identity.interval_end_exclusive.start_ms(),
            ForecastAnchorTimeValue::IntervalEndExclusive,
        )?,
        interval_start: values.utc_instant(
            identity.interval_start.start_ms(),
            ForecastAnchorTimeValue::IntervalStart,
        )?,
        requested_from_date: identity.requested_from_date.clone(),
        requested_to_date: identity.requested_to_date.clone(),
        source_config_fingerprint: values.sha256_digest(&identity.source_config_fingerprint)?,
        source_id: identity.source_id.clone(),
    })
}

// retain local preparation failures inside the chunk catch boundary
fn operation_from_task_failure(failure: TaskFailure) -> ForecastAnchorBackfillOperationError {
    ForecastAnchorBackfillOperationError::unexpected(failure.code)
}

/// One refusal while binding original general-backfill argv to the Previous Runs backend.
#[derive(Clone, Debug, PartialEq)]
pub enum ForecastAnchorBackfillInvocationError {
    Arguments(OpenMeteoBackfillArgumentError),
    ArchiveBackendRequired,
    SiteMismatch,
    SourceUnavailable,
    SourceContractMismatch,
    Material(TaskFailure),
}

impl From<OpenMeteoBackfillArgumentError> for ForecastAnchorBackfillInvocationError {
    /// Preserve argument admission before any loaded configuration projection.
    fn from(value: OpenMeteoBackfillArgumentError) -> Self {
        Self::Arguments(value)
    }
}

/// Prepare the exact Previous Runs invocation from the original general-backfill argv.
pub fn prepare_forecast_anchor_backfill_invocation<Time, Material>(
    arguments: &[String],
    site: &SiteConfiguration,
    time: &mut Time,
    material: &mut Material,
) -> Result<ForecastAnchorBackfillInvocation, ForecastAnchorBackfillInvocationError>
where
    Time: HistoricalBackfillCommandTimeAdapter,
    Material: WorkerSourceMaterialIdentity,
{
    let (arguments, from, to) = parse_open_meteo_backfill_arguments(arguments, time)?.into_parts();

    // preserve the original explicit site equality before selecting a configured source
    if arguments.site != site.site.key {
        return Err(ForecastAnchorBackfillInvocationError::SiteMismatch);
    }
    let source_key = match &arguments.source {
        // the original omitted source selects Archive and must not enter the anchor backend
        Nullable::Null => {
            return Err(ForecastAnchorBackfillInvocationError::ArchiveBackendRequired);
        }
        Nullable::Value(source_key) => source_key.as_str(),
    };

    // keep the explicit legacy Archive source on its distinct storage contract
    if source_key == ARCHIVE_SOURCE_KEY {
        return Err(ForecastAnchorBackfillInvocationError::ArchiveBackendRequired);
    }
    let configured = site
        .sources
        .iter()
        .find(|source| {
            source.key == source_key && source.capabilities.contains(&SourceCapability::Historical)
        })
        .ok_or(ForecastAnchorBackfillInvocationError::SourceUnavailable)?;

    // retain the selected key while leaving active-row authority to the original database query
    if source_key != PREVIOUS_RUNS_SOURCE_KEY {
        return Err(ForecastAnchorBackfillInvocationError::SourceUnavailable);
    }
    let adapter = forecast_anchor_adapter_configuration(&configured.adapterConfig)
        .ok_or(ForecastAnchorBackfillInvocationError::SourceContractMismatch)?;

    // reject a changed source contract before opening the process-owned pool
    if configured.sourceKind != SourceKind::Forecast
        || configured.capabilities != [SourceCapability::Historical]
        || configured.cadenceSeconds != Nullable::Null
    {
        return Err(ForecastAnchorBackfillInvocationError::SourceContractMismatch);
    }
    let material_config_identity = material
        .material_identity(&configured.adapterConfig)
        .map_err(ForecastAnchorBackfillInvocationError::Material)?;
    let report_path = match arguments.reportPath {
        Nullable::Null => None,
        Nullable::Value(path) => Some(path),
    };
    let chunk_days = u16::try_from(arguments.chunkDays)
        .map_err(|_| ForecastAnchorBackfillInvocationError::SourceContractMismatch)?;
    let configuration = ForecastAnchorRuntimeConfiguration {
        active: true,
        adapter,
        cadence_seconds: match &configured.cadenceSeconds {
            Nullable::Null => None,
            Nullable::Value(cadence) => Some(*cadence),
        },
        capabilities: BTreeSet::from([ForecastAnchorCapability::Historical]),
        fingerprint: configured.fingerprint.clone(),
        latitude: site.site.latitude,
        longitude: site.site.longitude,
        material_config_identity,
        provider_key: site.provider.key.clone(),
        site_slug: site.site.key.clone(),
        source_key: configured.key.clone(),
        source_kind: ForecastAnchorSourceKind::Forecast,
        station_slug: site.station.key.clone(),
        timezone: site.site.timezone.clone(),
    };
    Ok(ForecastAnchorBackfillInvocation {
        arguments: ForecastAnchorBackfillArguments {
            chunk_days,
            dry_run: arguments.dryRun,
            from,
            resume: arguments.resume,
            site: arguments.site,
            source: configured.key.clone(),
            to,
        },
        configuration,
        report_path,
    })
}

/// One typed source-read or exact configuration-identity refusal.
#[derive(Clone, Debug, PartialEq)]
pub enum ForecastAnchorBackfillSourceReadError<DriverError> {
    Read(OpenMeteoBackfillSourceReadError<DriverError>),
    Material(TaskFailure),
    Identity(TaskFailure),
}

/// Resolve and bind the original active source row before resume or provider work.
pub fn resolve_forecast_anchor_backfill_source<Port, Material>(
    port: &mut Port,
    invocation: &ForecastAnchorBackfillInvocation,
    material: &mut Material,
) -> Result<ForecastAnchorBackfillSource, ForecastAnchorBackfillSourceReadError<Port::Error>>
where
    Port: OpenMeteoBackfillSourceReadPort,
    Material: WorkerSourceMaterialIdentity,
{
    let row = resolve_open_meteo_backfill_source(
        port,
        &invocation.configuration.site_slug,
        &invocation.configuration.source_key,
    )
    .map_err(ForecastAnchorBackfillSourceReadError::Read)?;
    let material_config_identity = material
        .material_identity(&row.materialProviderConfig)
        .map_err(ForecastAnchorBackfillSourceReadError::Material)?;
    // refuse a changed database kind rather than projecting it into an anchor identity
    if row.sourceKind != SourceKind::Forecast {
        return Err(ForecastAnchorBackfillSourceReadError::Identity(
            TaskFailure {
                attempts: 0,
                code: "forecast_anchor_source_identity_mismatch".to_owned(),
            },
        ));
    }
    let source = ForecastAnchorBackfillSource {
        active: true,
        id: row.id,
        key: row.key,
        latitude: row.latitude,
        longitude: row.longitude,
        material_config_identity,
        provider_key: row.providerKey,
        site_slug: row.siteSlug,
        source_config_fingerprint: row.sourceConfigFingerprint,
        source_key: row.sourceKey,
        source_kind: ForecastAnchorSourceKind::Forecast,
        station_slug: row.stationSlug,
        timezone: row.timezone,
    };
    validate_forecast_anchor_backfill_source(
        &invocation.arguments,
        &invocation.configuration,
        &source,
    )
    .map_err(ForecastAnchorBackfillSourceReadError::Identity)?;
    Ok(source)
}

/// Project the already-decoded adapter object without introducing a JSON parser.
pub(crate) fn forecast_anchor_adapter_configuration(
    value: &JsonValue,
) -> Option<ForecastAnchorAdapterConfiguration> {
    let JsonValue::Object(entries) = value else {
        return None;
    };
    let configuration = ForecastAnchorAdapterConfiguration {
        contract_epoch: json_text(entries, "contractEpoch")?,
        contract_version: json_text(entries, "contractVersion")?,
        lead_hours: json_u16_array(entries, "leadHours")?,
        maximum_chunk_days: json_u16(entries, "maximumChunkDays")?,
        model: json_text(entries, "model")?,
        variables: json_text_array(entries, "variables")?,
    };

    // bind all fixed Previous Runs controls before returning runtime configuration
    configuration
        .matches_retained_contract()
        .then_some(configuration)
}

/// Borrow one field from an already-decoded ordered object.
fn json_field<'a>(entries: &'a [(Vec<u16>, JsonValue)], key: &str) -> Option<&'a JsonValue> {
    entries
        .iter()
        .find(|(field, _)| field.iter().copied().eq(key.encode_utf16()))
        .map(|(_, value)| value)
}

/// Project one UTF-16 JSON string through the standard library decoder.
fn json_text(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<String> {
    let JsonValue::String(value) = json_field(entries, key)? else {
        return None;
    };
    String::from_utf16(value).ok()
}

/// Project one exact finite unsigned 16-bit integer.
fn json_u16(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<u16> {
    let JsonValue::Number(value) = json_field(entries, key)? else {
        return None;
    };
    // reject fractional, negative, nonfinite, or out-of-range cells
    if !value.is_finite() || value.fract() != 0.0 || !(0.0..=f64::from(u16::MAX)).contains(value) {
        return None;
    }
    Some(*value as u16)
}

/// Project one ordered finite integer array.
fn json_u16_array(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<Vec<u16>> {
    let JsonValue::Array(values) = json_field(entries, key)? else {
        return None;
    };
    values
        .iter()
        .map(|value| {
            // reuse the scalar admission without copying or reparsing the parent object
            let JsonValue::Number(value) = value else {
                return None;
            };
            if !value.is_finite()
                || value.fract() != 0.0
                || !(0.0..=f64::from(u16::MAX)).contains(value)
            {
                return None;
            }
            Some(*value as u16)
        })
        .collect()
}

/// Project one ordered UTF-16 string array.
fn json_text_array(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<Vec<String>> {
    let JsonValue::Array(values) = json_field(entries, key)? else {
        return None;
    };
    values
        .iter()
        .map(|value| {
            let JsonValue::String(value) = value else {
                return None;
            };
            String::from_utf16(value).ok()
        })
        .collect()
}

/// Fully parsed CLI arguments and checked local configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastAnchorBackfillInvocation {
    pub arguments: ForecastAnchorBackfillArguments,
    pub configuration: ForecastAnchorRuntimeConfiguration,
    pub report_path: Option<String>,
}

/// Typed dependencies owned by the explicit one-shot forecast-anchor application.
pub trait ForecastAnchorBackfillApplicationPort {
    type Pool: ForecastAnchorBackfillRepository;
    type Provider: ForecastAnchorBackfillProvider;
    type Time: ForecastAnchorBackfillTimeAdapter;

    /// Parse arguments and load checked local configuration before opening the pool.
    fn load_invocation(&mut self) -> Result<ForecastAnchorBackfillInvocation, TaskFailure>;

    /// Open the one-shot database pool after local admission.
    fn open_pool(
        &mut self,
        invocation: &ForecastAnchorBackfillInvocation,
    ) -> Result<Self::Pool, TaskFailure>;

    /// Create the typed Previous Runs provider boundary before database readiness.
    fn create_provider(&mut self, invocation: &ForecastAnchorBackfillInvocation) -> Self::Provider;

    /// Create the admitted date and deadline boundary.
    fn create_time(&mut self, invocation: &ForecastAnchorBackfillInvocation) -> Self::Time;

    /// Verify the supported database contract inside the close-protected lifecycle.
    fn assert_supported_database(&mut self, pool: &mut Self::Pool) -> Result<(), TaskFailure>;

    /// Resolve one active exact source through the admitted repository query.
    fn resolve_source(
        &mut self,
        pool: &mut Self::Pool,
        invocation: &ForecastAnchorBackfillInvocation,
    ) -> Result<ForecastAnchorBackfillSource, TaskFailure>;

    /// Persist the optional machine report through an admitted serializer and file owner.
    fn write_report_file(
        &mut self,
        path: &str,
        report: &ForecastAnchorBackfillReport,
    ) -> Result<(), TaskFailure>;

    /// Write the machine report to the one-shot process output.
    fn write_stdout(&mut self, report: &ForecastAnchorBackfillReport) -> Result<(), TaskFailure>;

    /// Close the one-shot pool in the application finally boundary.
    fn close_pool(&mut self, pool: Self::Pool) -> Result<(), TaskFailure>;
}

/// Run the explicit forecast-anchor CLI-equivalent dependency lifecycle once.
pub fn run_forecast_anchor_backfill_application<Port>(port: &mut Port) -> Result<u8, TaskFailure>
where
    Port: ForecastAnchorBackfillApplicationPort,
{
    let invocation = port.load_invocation()?;
    run_prepared_forecast_anchor_backfill_application(invocation, port)
}

/// Run one invocation already admitted from original argv and loaded site configuration.
pub fn run_prepared_forecast_anchor_backfill_application<Port>(
    invocation: ForecastAnchorBackfillInvocation,
    port: &mut Port,
) -> Result<u8, TaskFailure>
where
    Port: ForecastAnchorBackfillApplicationPort,
{
    let mut pool = port.open_pool(&invocation)?;
    let mut provider = port.create_provider(&invocation);
    let mut time = port.create_time(&invocation);
    let execution = (|| {
        port.assert_supported_database(&mut pool)?;
        let source = port.resolve_source(&mut pool, &invocation)?;
        let report = execute_forecast_anchor_backfill(
            &invocation.arguments,
            &invocation.configuration,
            &source,
            &mut pool,
            &mut provider,
            &mut time,
        )?;

        // persist the optional file before writing the ordinary stdout report
        if let Some(path) = invocation.report_path.as_deref() {
            port.write_report_file(path, &report)?;
        }
        port.write_stdout(&report)?;
        Ok(report.exit_code)
    })();
    // match the one-shot finally boundary where close failure is terminal
    port.close_pool(pool)?;
    execution
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::fmt::{Display, Formatter};
    use std::rc::Rc;

    use weather_contracts::{
        SiteConfigurationProvider, SiteConfigurationSite, SiteConfigurationSource,
        SiteConfigurationStation, StationKind,
    };
    use weather_database::{
        ForecastAnchorAbandonExpiredQuery, ForecastAnchorFailureQuery,
        ForecastAnchorFindRunningQuery, ForecastAnchorOutcomeQuery,
        ForecastAnchorRecordUpsertQuery, ForecastAnchorRunIdentityQuery,
        ForecastAnchorRunStartBind, ForecastAnchorRunStartQuery, ForecastAnchorRunningLookup,
        ForecastAnchorSuccessQuery, ForecastAnchorSuccessfulChunkQuery,
        OpenMeteoBackfillSourceQuery, OpenMeteoBackfillSourceRow, StartedForecastAnchorBackfillRun,
    };
    use weather_worker::{
        FORECAST_ANCHOR_CONTRACT_EPOCH, FORECAST_ANCHOR_CONTRACT_VERSION,
        FORECAST_ANCHOR_MAXIMUM_CHUNK_DAYS, FORECAST_ANCHOR_MODEL,
        ForecastAnchorAdapterConfiguration, ForecastAnchorAttemptPayload,
        ForecastAnchorBackfillChunkIdentity, ForecastAnchorBackfillCompletion,
        ForecastAnchorBackfillDate, ForecastAnchorBackfillDeadlines,
        ForecastAnchorBackfillOperationError, ForecastAnchorBackfillProviderFailure,
        ForecastAnchorBackfillRunStart, ForecastAnchorCapability, ForecastAnchorEnvelope,
        ForecastAnchorMetricCells, ForecastAnchorParsedHour, ForecastAnchorParsedLocation,
        ForecastAnchorProviderRequest, ForecastAnchorSourceKind, ForecastAnchorTimezone,
        ScheduledSourceMaterialComparison,
    };

    use super::*;

    const DAY_MS: i64 = 86_400_000;
    const START_MS: i64 = 1_772_323_200_000;
    const PREVIOUS_RUNS_LEAD_HOURS: [u16; 7] = [24, 48, 72, 96, 120, 144, 168];
    const PREVIOUS_RUNS_VARIABLES: [&str; 9] = [
        "temperature",
        "apparent_temperature",
        "relative_humidity",
        "precipitation",
        "cloud_cover",
        "wind_speed",
        "wind_gust",
        "wind_direction",
        "surface_pressure",
    ];
    type Trace = Rc<RefCell<Vec<String>>>;

    /// Encode one fixture string without adding a JSON codec.
    fn json_string(value: &str) -> JsonValue {
        JsonValue::String(value.encode_utf16().collect())
    }

    /// Encode the frozen Previous Runs adapter in its loaded typed form.
    fn previous_runs_adapter() -> JsonValue {
        JsonValue::Object(vec![
            (
                "contractVersion".encode_utf16().collect(),
                json_string(FORECAST_ANCHOR_CONTRACT_VERSION),
            ),
            (
                "model".encode_utf16().collect(),
                json_string(FORECAST_ANCHOR_MODEL),
            ),
            (
                "contractEpoch".encode_utf16().collect(),
                json_string(FORECAST_ANCHOR_CONTRACT_EPOCH),
            ),
            (
                "leadHours".encode_utf16().collect(),
                JsonValue::Array(
                    PREVIOUS_RUNS_LEAD_HOURS
                        .map(|value| JsonValue::Number(f64::from(value)))
                        .to_vec(),
                ),
            ),
            (
                "maximumChunkDays".encode_utf16().collect(),
                JsonValue::Number(f64::from(FORECAST_ANCHOR_MAXIMUM_CHUNK_DAYS)),
            ),
            (
                "variables".encode_utf16().collect(),
                JsonValue::Array(PREVIOUS_RUNS_VARIABLES.map(json_string).to_vec()),
            ),
        ])
    }

    /// Construct the loaded site catalog used by the original one-shot command.
    fn site_configuration() -> SiteConfiguration {
        SiteConfiguration {
            provider: SiteConfigurationProvider {
                active: true,
                attributionLabel: "Open-Meteo".to_owned(),
                attributionUrl: "https://open-meteo.com".to_owned(),
                displayName: "Open-Meteo".to_owned(),
                key: "open-meteo".to_owned(),
            },
            site: SiteConfigurationSite {
                active: true,
                displayName: "Ballydidean".to_owned(),
                key: "ballydidean".to_owned(),
                latitude: 48.1,
                longitude: -122.7,
                timezone: "America/Los_Angeles".to_owned(),
            },
            sources: vec![SiteConfigurationSource {
                active: true,
                adapterConfig: previous_runs_adapter(),
                cadenceSeconds: Nullable::Null,
                capabilities: vec![SourceCapability::Historical],
                fingerprint: "a".repeat(64),
                key: PREVIOUS_RUNS_SOURCE_KEY.to_owned(),
                sourceKind: SourceKind::Forecast,
            }],
            station: SiteConfigurationStation {
                active: true,
                displayName: "Primary".to_owned(),
                key: "primary".to_owned(),
                kind: StationKind::Virtual,
                latitude: 48.1,
                longitude: -122.7,
                model: Nullable::Null,
                serial: Nullable::Null,
                vendor: Nullable::Value("Open-Meteo".to_owned()),
            },
            version: 1,
        }
    }

    struct InvocationMaterial;

    impl ScheduledSourceMaterialComparison for InvocationMaterial {
        type Error = ();

        /// Compare fixture values without serializing them.
        fn values_equal(
            &mut self,
            left: &JsonValue,
            right: &JsonValue,
        ) -> Result<bool, Self::Error> {
            Ok(left == right)
        }

        /// Compare one fixture value with an admitted object projection.
        fn value_and_object_equal(
            &mut self,
            value: &JsonValue,
            object: &[(Vec<u16>, JsonValue)],
        ) -> Result<bool, Self::Error> {
            Ok(matches!(value, JsonValue::Object(entries) if entries == object))
        }
    }

    impl WorkerSourceMaterialIdentity for InvocationMaterial {
        /// Bind only the frozen Previous Runs adapter used by the fixture.
        fn material_identity(&mut self, value: &JsonValue) -> Result<String, TaskFailure> {
            if value != &previous_runs_adapter() {
                return Err(TaskFailure {
                    attempts: 0,
                    code: "unexpected_material".to_owned(),
                });
            }
            Ok("admitted-material".to_owned())
        }
    }

    struct SourceReadFixture {
        query: Option<OpenMeteoBackfillSourceQuery>,
    }

    impl OpenMeteoBackfillSourceReadPort for SourceReadFixture {
        type Error = ();

        /// Retain the exact query and return one matching active database row.
        fn read_open_meteo_backfill_source(
            &mut self,
            query: &OpenMeteoBackfillSourceQuery,
        ) -> Result<Option<OpenMeteoBackfillSourceRow>, Self::Error> {
            self.query = Some(query.clone());
            Ok(Some(OpenMeteoBackfillSourceRow {
                latitude: 48.1,
                longitude: -122.7,
                material_provider_config: previous_runs_adapter(),
                provider_key: "open-meteo".to_owned(),
                site_slug: "ballydidean".to_owned(),
                source_config_fingerprint: "a".repeat(64),
                source_id: "source-previous-runs".to_owned(),
                source_key: PREVIOUS_RUNS_SOURCE_KEY.to_owned(),
                source_kind: SourceKind::Forecast,
                station_slug: "primary".to_owned(),
                timezone: "America/Los_Angeles".to_owned(),
            }))
        }
    }

    // construct the exact checked Previous Runs invocation
    fn invocation() -> ForecastAnchorBackfillInvocation {
        let configuration = ForecastAnchorRuntimeConfiguration {
            active: true,
            adapter: ForecastAnchorAdapterConfiguration {
                contract_epoch: "open-meteo-previous-runs-best-match/2026-09".to_owned(),
                contract_version: "previous-runs-hourly/v1".to_owned(),
                lead_hours: vec![24, 48, 72, 96, 120, 144, 168],
                maximum_chunk_days: 14,
                model: "best_match".to_owned(),
                variables: [
                    "temperature",
                    "apparent_temperature",
                    "relative_humidity",
                    "precipitation",
                    "cloud_cover",
                    "wind_speed",
                    "wind_gust",
                    "wind_direction",
                    "surface_pressure",
                ]
                .map(str::to_owned)
                .to_vec(),
            },
            cadence_seconds: None,
            capabilities: BTreeSet::from([ForecastAnchorCapability::Historical]),
            fingerprint: "a".repeat(64),
            latitude: 48.1,
            longitude: -122.7,
            material_config_identity: "admitted-material".to_owned(),
            provider_key: "open-meteo".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_key: "open-meteo-previous-runs-v1".to_owned(),
            source_kind: ForecastAnchorSourceKind::Forecast,
            station_slug: "primary".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        };
        ForecastAnchorBackfillInvocation {
            arguments: ForecastAnchorBackfillArguments {
                chunk_days: 14,
                dry_run: false,
                from: ForecastAnchorBackfillDate::admitted("2026-03-01", START_MS),
                resume: true,
                site: configuration.site_slug.clone(),
                source: configuration.source_key.clone(),
                to: ForecastAnchorBackfillDate::admitted("2026-03-01", START_MS),
            },
            configuration,
            report_path: Some("report.json".to_owned()),
        }
    }

    // construct the exact active source returned by the admitted query
    fn source(configuration: &ForecastAnchorRuntimeConfiguration) -> ForecastAnchorBackfillSource {
        ForecastAnchorBackfillSource {
            active: true,
            id: "source-previous-runs".to_owned(),
            key: configuration.source_key.clone(),
            latitude: configuration.latitude,
            longitude: configuration.longitude,
            material_config_identity: configuration.material_config_identity.clone(),
            provider_key: configuration.provider_key.clone(),
            site_slug: configuration.site_slug.clone(),
            source_config_fingerprint: configuration.fingerprint.clone(),
            source_key: configuration.source_key.clone(),
            source_kind: ForecastAnchorSourceKind::Forecast,
            station_slug: configuration.station_slug.clone(),
            timezone: configuration.timezone.clone(),
        }
    }

    struct FakePool {
        completions: usize,
        trace: Trace,
    }

    impl ForecastAnchorBackfillRepository for FakePool {
        type Session = String;

        // perform the exact resume read before the source lock
        fn has_successful_chunk(
            &mut self,
            _identity: &ForecastAnchorBackfillChunkIdentity,
        ) -> Result<bool, TaskFailure> {
            self.trace.borrow_mut().push("resume".to_owned());
            Ok(false)
        }

        // acquire one source-scoped transaction session
        fn acquire_source_session(
            &mut self,
            source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            self.trace.borrow_mut().push("lock".to_owned());
            Ok(Some(source_id.to_owned()))
        }

        // abandon expired runs before committing a new run
        fn abandon_expired_runs(
            &mut self,
            _session: &mut Self::Session,
            _started_at_ms: i64,
        ) -> Result<(), ForecastAnchorBackfillOperationError> {
            self.trace.borrow_mut().push("abandon".to_owned());
            Ok(())
        }

        // commit one running row before provider work
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _start: ForecastAnchorBackfillRunStart,
        ) -> Result<String, ForecastAnchorBackfillOperationError> {
            self.trace.borrow_mut().push("start".to_owned());
            Ok("run-1".to_owned())
        }

        // atomically retain actual normalized records and response evidence
        fn complete_forecast_anchor_backfill_ingestion(
            &mut self,
            _session: &mut Self::Session,
            completion: ForecastAnchorBackfillCompletion,
        ) -> Result<(), ForecastAnchorBackfillOperationError> {
            self.trace
                .borrow_mut()
                .push(format!("complete:{}", completion.records.len()));
            self.completions += 1;
            Ok(())
        }

        // reject the successful fixture's unused failure boundary
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _run_id: &str,
            _identity: &ForecastAnchorBackfillChunkIdentity,
            _failure: &ForecastAnchorBackfillProviderFailure,
        ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
            unreachable!("successful fixture does not finalize failure")
        }

        // release the source-scoped transaction last
        fn release_source_session(
            &mut self,
            _session: Self::Session,
        ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
            self.trace.borrow_mut().push("release".to_owned());
            Ok(())
        }
    }

    struct FakeTime;

    impl ForecastAnchorBackfillTimeAdapter for FakeTime {
        // return a current date after the historical request
        fn today_utc(&mut self) -> Result<ForecastAnchorBackfillDate, TaskFailure> {
            Ok(ForecastAnchorBackfillDate::admitted(
                "2026-08-22",
                1_787_356_800_000,
            ))
        }

        // add exact fixture UTC days
        fn add_utc_days(
            &mut self,
            date: &ForecastAnchorBackfillDate,
            days: i64,
        ) -> Result<ForecastAnchorBackfillDate, TaskFailure> {
            let start_ms = date.start_ms() + days * DAY_MS;
            let label = match start_ms {
                START_MS => "2026-03-01",
                value if value == START_MS + DAY_MS => "2026-03-02",
                value if value == START_MS + 13 * DAY_MS => "2026-03-14",
                _ => "2026-03-15",
            };
            Ok(ForecastAnchorBackfillDate::admitted(label, start_ms))
        }

        // preserve the production provider and run deadline budgets
        fn deadlines(
            &mut self,
        ) -> Result<ForecastAnchorBackfillDeadlines, ForecastAnchorBackfillOperationError> {
            Ok(ForecastAnchorBackfillDeadlines {
                provider_deadline_at_ms: 1_787_375_290_000,
                run_deadline_at_ms: 1_787_375_320_000,
                started_at_ms: 1_787_375_200_000,
            })
        }
    }

    impl HistoricalBackfillCommandTimeAdapter for FakeTime {
        /// Return the same fixed current day used by the executor fixture.
        fn today_utc(
            &mut self,
        ) -> Result<weather_worker::HistoricalBackfillCommandDate, TaskFailure> {
            Ok(weather_worker::HistoricalBackfillCommandDate::admitted(
                "2026-08-22",
                1_787_356_800_000,
            ))
        }

        /// Admit the one original request date without implementing a calendar parser.
        fn admit_utc_date(
            &mut self,
            label: &str,
        ) -> Result<weather_worker::HistoricalBackfillCommandDate, TaskFailure> {
            if label != "2026-03-01" {
                return Err(TaskFailure {
                    attempts: 0,
                    code: "fixture_date_unavailable".to_owned(),
                });
            }
            Ok(weather_worker::HistoricalBackfillCommandDate::admitted(
                label, START_MS,
            ))
        }

        /// Delegate exact day arithmetic to the existing executor fixture owner.
        fn add_utc_days(
            &mut self,
            date: &weather_worker::HistoricalBackfillCommandDate,
            days: i64,
        ) -> Result<weather_worker::HistoricalBackfillCommandDate, TaskFailure> {
            Ok(weather_worker::HistoricalBackfillCommandDate::admitted(
                date.label(),
                date.start_ms() + days * DAY_MS,
            ))
        }
    }

    struct FakeProvider {
        trace: Trace,
    }

    impl ForecastAnchorBackfillProvider for FakeProvider {
        // return one populated fixed lead inside a complete hourly response
        fn fetch(
            &mut self,
            request: ForecastAnchorProviderRequest,
        ) -> Result<ForecastAnchorAttemptPayload, ForecastAnchorBackfillOperationError> {
            self.trace.borrow_mut().push("fetch".to_owned());
            let empty = ForecastAnchorMetricCells {
                apparent_temperature_c: None,
                cloud_cover_percent: None,
                precipitation_mm: None,
                pressure_hpa: None,
                relative_humidity_percent: None,
                temperature_c: None,
                wind_direction_degrees: None,
                wind_gust_mps: None,
                wind_speed_mps: None,
            };
            let hours = (0..24)
                .map(|hour| {
                    let mut anchors = [empty; 7];

                    // retain one actual populated lead
                    if hour == 0 {
                        anchors[0].temperature_c = Some(10.0);
                    }
                    ForecastAnchorParsedHour {
                        anchors,
                        valid_at: format!("2026-03-01T{hour:02}:00:00.000Z"),
                        valid_at_ms: request.start_ms + i64::from(hour) * 3_600_000,
                    }
                })
                .collect();
            Ok(ForecastAnchorAttemptPayload {
                attempts: 2,
                checksum: "d".repeat(64),
                http_status: 200,
                locations: vec![ForecastAnchorParsedLocation {
                    envelope: ForecastAnchorEnvelope {
                        elevation_m: None,
                        grid_cell: "48.1,-122.7".to_owned(),
                        latitude: 48.1,
                        longitude: -122.7,
                        timezone: ForecastAnchorTimezone::Utc,
                        utc_offset_seconds: 0,
                    },
                    generation_ms: Some(1.5),
                    hours,
                }],
                received_at: "2026-08-22T05:20:00.000Z".to_owned(),
            })
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct DatabaseFixtureError(&'static str);

    impl Display for DatabaseFixtureError {
        /// Render one injected database failure.
        fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.0)
        }
    }

    impl std::error::Error for DatabaseFixtureError {}

    struct ForecastAnchorDatabaseFixture {
        trace: Trace,
    }

    impl ForecastAnchorBackfillPoolPort for ForecastAnchorDatabaseFixture {
        type Error = DatabaseFixtureError;
        type Session = ForecastAnchorSessionFixture;

        /// Verify the exact resume query before returning an incomplete chunk.
        fn has_successful_chunk(
            &mut self,
            query: &ForecastAnchorSuccessfulChunkQuery,
        ) -> Result<usize, Self::Error> {
            assert_eq!(query.binds.len(), 6);
            self.trace.borrow_mut().push("db:resume:6".to_owned());
            Ok(0)
        }

        /// Acquire one source-scoped retained fake session.
        fn acquire_source_session(
            &mut self,
            source_id: &str,
        ) -> Result<Option<Self::Session>, Self::Error> {
            self.trace.borrow_mut().push("db:lock".to_owned());
            Ok(Some(ForecastAnchorSessionFixture {
                source_id: source_id.to_owned(),
                trace: Rc::clone(&self.trace),
            }))
        }
    }

    struct ForecastAnchorSessionFixture {
        source_id: String,
        trace: Trace,
    }

    impl RetainedSourceSession for ForecastAnchorSessionFixture {
        /// Return the retained fake source identity.
        fn source_id(&self) -> &str {
            &self.source_id
        }
    }

    impl ForecastAnchorBackfillCommandPort for ForecastAnchorSessionFixture {
        type Error = DatabaseFixtureError;

        /// Verify exact expired-run recovery binds.
        fn abandon_expired_runs(
            &mut self,
            query: &ForecastAnchorAbandonExpiredQuery,
        ) -> Result<Vec<String>, Self::Error> {
            assert_eq!(query.binds.len(), 2);
            self.trace.borrow_mut().push("db:abandon:2".to_owned());
            Ok(Vec::new())
        }

        /// Verify the running-row lookup occurs before insert.
        fn find_running_run(
            &mut self,
            query: &ForecastAnchorFindRunningQuery,
        ) -> Result<ForecastAnchorRunningLookup, Self::Error> {
            assert_eq!(query.binds.len(), 1);
            self.trace.borrow_mut().push("db:find:1".to_owned());
            Ok(ForecastAnchorRunningLookup {
                first_run_id: None,
                row_count: 0,
            })
        }

        /// Verify the exact ten-bind committed running row.
        fn insert_running_run(
            &mut self,
            query: &ForecastAnchorRunStartQuery,
        ) -> Result<Option<StartedForecastAnchorBackfillRun>, Self::Error> {
            assert_eq!(query.binds.len(), 10);
            assert_eq!(
                query.binds[2],
                ForecastAnchorRunStartBind::UtcInstant("2026-03-01T00:00:00.000Z".to_owned())
            );
            assert_eq!(
                query.binds[3],
                ForecastAnchorRunStartBind::UtcInstant("2026-03-02T00:00:00.000Z".to_owned())
            );
            assert_eq!(query.binds[8], ForecastAnchorRunStartBind::Count(0));
            assert_eq!(
                query.binds[9],
                ForecastAnchorRunStartBind::NullableJson(Some(SerializedJson::admitted(
                    r#"{"end_date":"2026-03-01","endpoint":"previous-runs/forecast","start_date":"2026-03-01"}"#,
                )))
            );
            self.trace.borrow_mut().push("db:start:10".to_owned());
            Ok(Some(StartedForecastAnchorBackfillRun {
                id: "run-1".to_owned(),
                started_at: "2026-08-22T05:20:00.000Z".to_owned(),
            }))
        }

        /// Record the successful transaction start.
        fn begin_transaction(&mut self) -> Result<(), Self::Error> {
            self.trace.borrow_mut().push("db:begin".to_owned());
            Ok(())
        }

        /// Verify the exact committed identity guard.
        fn assert_backfill_run(
            &mut self,
            query: &ForecastAnchorRunIdentityQuery,
        ) -> Result<usize, Self::Error> {
            assert_eq!(query.binds.len(), 7);
            self.trace.borrow_mut().push("db:assert:7".to_owned());
            Ok(1)
        }

        /// Verify the actual normalized anchor reaches the 24-bind upsert.
        fn upsert_forecast_anchor_records(
            &mut self,
            query: &ForecastAnchorRecordUpsertQuery,
        ) -> Result<usize, Self::Error> {
            assert_eq!(query.record_count, 1);
            assert_eq!(query.binds.len(), 24);
            self.trace.borrow_mut().push("db:records:1".to_owned());
            Ok(1)
        }

        /// Verify the successful chunk outcome follows the record write.
        fn upsert_backfill_outcome(
            &mut self,
            query: &ForecastAnchorOutcomeQuery,
        ) -> Result<(), Self::Error> {
            assert_eq!(query.binds.len(), 11);
            self.trace.borrow_mut().push("db:outcome:11".to_owned());
            Ok(())
        }

        /// Verify the exact successful finalization bind shape.
        fn finalize_successful_run(
            &mut self,
            query: &ForecastAnchorSuccessQuery,
        ) -> Result<usize, Self::Error> {
            assert_eq!(query.binds.len(), 7);
            self.trace.borrow_mut().push("db:finalize:7".to_owned());
            Ok(1)
        }

        /// Reject failure finalization in the successful fixture.
        fn finalize_failed_run(
            &mut self,
            _query: &ForecastAnchorFailureQuery,
        ) -> Result<usize, Self::Error> {
            unreachable!("successful fixture does not finalize failure")
        }

        /// Record atomic completion commit.
        fn commit_transaction(&mut self) -> Result<(), Self::Error> {
            self.trace.borrow_mut().push("db:commit".to_owned());
            Ok(())
        }

        /// Reject rollback in the successful fixture.
        fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
            self.trace.borrow_mut().push("db:rollback".to_owned());
            Ok(())
        }

        /// Release the retained source session last.
        fn release(self) -> Result<(), Self::Error> {
            self.trace.borrow_mut().push("db:release".to_owned());
            Ok(())
        }
    }

    struct ForecastAnchorValueFixture;

    impl ForecastAnchorBackfillRuntimeValues for ForecastAnchorValueFixture {
        /// Format known fixture instants through an admitted clock.
        fn utc_instant(
            &mut self,
            epoch_milliseconds: i64,
            value: ForecastAnchorTimeValue,
        ) -> Result<String, TaskFailure> {
            let instant = match value {
                ForecastAnchorTimeValue::AbandonAsOf => "2026-08-22T05:20:00.000Z",
                ForecastAnchorTimeValue::Deadline => "2026-08-22T05:22:00.000Z",
                ForecastAnchorTimeValue::IntervalStart => "2026-03-01T00:00:00.000Z",
                ForecastAnchorTimeValue::IntervalEndExclusive => "2026-03-02T00:00:00.000Z",
            };
            assert!(epoch_milliseconds > 0);
            Ok(instant.to_owned())
        }

        /// Admit the exact lowercase fixture digest.
        fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure> {
            assert_eq!(value.len(), 64);
            Ok(Sha256Digest::admitted(value))
        }

        /// Serialize the exact Node endpoint request evidence.
        fn request_metadata_json(
            &mut self,
            metadata: &JsonValue,
        ) -> Result<Option<SerializedJson>, TaskFailure> {
            let JsonValue::Object(fields) = metadata else {
                panic!("request metadata object expected");
            };
            assert_eq!(fields.len(), 3);
            assert_eq!(
                fields
                    .iter()
                    .map(|(key, _)| String::from_utf16(key).expect("request metadata key"))
                    .collect::<Vec<_>>(),
                ["end_date", "endpoint", "start_date"]
            );
            Ok(Some(SerializedJson::admitted(
                r#"{"end_date":"2026-03-01","endpoint":"previous-runs/forecast","start_date":"2026-03-01"}"#,
            )))
        }

        /// Attach canonical metadata JSON and record hash evidence.
        fn record_storage_evidence(
            &mut self,
            record: &NormalizedForecastAnchorRecord,
        ) -> Result<ForecastAnchorRecordStorageEvidence, TaskFailure> {
            assert_eq!(record.validAt, "2026-03-01T00:00:00.000Z");
            Ok(ForecastAnchorRecordStorageEvidence {
                content_hash: Sha256Digest::admitted("b".repeat(64)),
                provider_metadata_json: Some(SerializedJson::admitted(
                    r#"{"dataset":"previous_runs"}"#,
                )),
                quality_metadata_json: None,
            })
        }

        /// Serialize the actual bounded anchor counts.
        fn response_metadata_json(
            &mut self,
            metadata: &JsonValue,
        ) -> Result<Option<SerializedJson>, TaskFailure> {
            let JsonValue::Object(fields) = metadata else {
                panic!("response metadata object expected");
            };
            assert_eq!(fields.len(), 13);
            assert_eq!(
                fields
                    .iter()
                    .map(|(key, _)| String::from_utf16(key).expect("metadata key"))
                    .collect::<Vec<_>>(),
                [
                    "anchor_counts_by_lead",
                    "data_modification",
                    "generation_ms",
                    "http_status",
                    "location_response_count",
                    "null_anchor_count",
                    "populated_anchor_count",
                    "requested_anchor_count",
                    "requested_location_count",
                    "selected_field_count",
                    "upstream_http_response_count",
                    "weighted_api_call_cost",
                    "wind_gust_interval",
                ]
            );
            Ok(Some(SerializedJson::admitted(
                r#"{"populatedAnchorCount":1}"#,
            )))
        }

        /// Reject failure admission in the successful fixture.
        fn ingestion_error(
            &mut self,
            _failure: &ForecastAnchorBackfillProviderFailure,
        ) -> Result<AdmittedIngestionError, TaskFailure> {
            unreachable!("successful fixture does not admit a failure")
        }

        /// Classify a database error at the task boundary.
        fn task_failure(
            &mut self,
            operation: ForecastAnchorPersistenceOperation,
            _message: &str,
        ) -> TaskFailure {
            TaskFailure {
                attempts: 0,
                code: format!("forecast_anchor_{operation:?}"),
            }
        }

        /// Classify a database error inside the committed chunk boundary.
        fn operation_failure(
            &mut self,
            operation: ForecastAnchorPersistenceOperation,
            message: &str,
        ) -> ForecastAnchorBackfillOperationError {
            ForecastAnchorBackfillOperationError::unexpected(format!("{operation:?}: {message}"))
        }

        /// Retain one already-redacted cleanup diagnostic for the worker report.
        fn cleanup_failure(
            &mut self,
            operation: ForecastAnchorPersistenceOperation,
            message: &str,
        ) -> OpenMeteoArchiveCleanupFailure {
            OpenMeteoArchiveCleanupFailure::admitted(format!("{operation:?}: {message}"))
        }
    }

    struct FakeApplication {
        fail_readiness: bool,
        invocation: ForecastAnchorBackfillInvocation,
        trace: Trace,
    }

    impl ForecastAnchorBackfillApplicationPort for FakeApplication {
        type Pool = FakePool;
        type Provider = FakeProvider;
        type Time = FakeTime;

        // load the parsed one-shot invocation first
        fn load_invocation(&mut self) -> Result<ForecastAnchorBackfillInvocation, TaskFailure> {
            self.trace.borrow_mut().push("load".to_owned());
            Ok(self.invocation.clone())
        }

        // open one application-owned pool
        fn open_pool(
            &mut self,
            _invocation: &ForecastAnchorBackfillInvocation,
        ) -> Result<Self::Pool, TaskFailure> {
            self.trace.borrow_mut().push("pool".to_owned());
            Ok(FakePool {
                completions: 0,
                trace: Rc::clone(&self.trace),
            })
        }

        // bind the typed provider before readiness
        fn create_provider(
            &mut self,
            _invocation: &ForecastAnchorBackfillInvocation,
        ) -> Self::Provider {
            self.trace.borrow_mut().push("provider".to_owned());
            FakeProvider {
                trace: Rc::clone(&self.trace),
            }
        }

        // bind admitted time before readiness
        fn create_time(&mut self, _invocation: &ForecastAnchorBackfillInvocation) -> Self::Time {
            self.trace.borrow_mut().push("time".to_owned());
            FakeTime
        }

        // verify the database contract before source resolution
        fn assert_supported_database(&mut self, _pool: &mut Self::Pool) -> Result<(), TaskFailure> {
            self.trace.borrow_mut().push("readiness".to_owned());

            // exercise the close-protected failure path
            if self.fail_readiness {
                return Err(TaskFailure {
                    attempts: 0,
                    code: "database_not_ready".to_owned(),
                });
            }
            Ok(())
        }

        // resolve one active source after readiness
        fn resolve_source(
            &mut self,
            _pool: &mut Self::Pool,
            invocation: &ForecastAnchorBackfillInvocation,
        ) -> Result<ForecastAnchorBackfillSource, TaskFailure> {
            self.trace.borrow_mut().push("resolve".to_owned());
            Ok(source(&invocation.configuration))
        }

        // retain optional report-file ordering
        fn write_report_file(
            &mut self,
            path: &str,
            report: &ForecastAnchorBackfillReport,
        ) -> Result<(), TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("file:{path}:{}", report.exit_code));
            Ok(())
        }

        // retain stdout after optional report persistence
        fn write_stdout(
            &mut self,
            report: &ForecastAnchorBackfillReport,
        ) -> Result<(), TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("stdout:{}", report.exit_code));
            Ok(())
        }

        // close the pool through the one-shot finally boundary
        fn close_pool(&mut self, pool: Self::Pool) -> Result<(), TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("close:{}", pool.completions));
            Ok(())
        }
    }

    // prove original argv and loaded configuration feed the retained application lifecycle
    #[test]
    fn prepared_previous_runs_invocation_executes_without_preassembled_load() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut time = FakeTime;
        let mut material = InvocationMaterial;
        let prepared = prepare_forecast_anchor_backfill_invocation(
            &[
                "--site".to_owned(),
                "ballydidean".to_owned(),
                "--source".to_owned(),
                PREVIOUS_RUNS_SOURCE_KEY.to_owned(),
                "--from".to_owned(),
                "2026-03-01".to_owned(),
                "--to".to_owned(),
                "2026-03-01".to_owned(),
                "--resume".to_owned(),
                "--report".to_owned(),
                "report.json".to_owned(),
            ],
            &site_configuration(),
            &mut time,
            &mut material,
        )
        .expect("prepared Previous Runs invocation");
        let mut source_read = SourceReadFixture { query: None };
        let resolved =
            resolve_forecast_anchor_backfill_source(&mut source_read, &prepared, &mut material)
                .expect("matching active source");
        assert_eq!(resolved.id, "source-previous-runs");
        assert_eq!(
            source_read.query.expect("source query").binds,
            [
                weather_database::OpenMeteoBackfillSourceBind::Text("ballydidean".to_owned()),
                weather_database::OpenMeteoBackfillSourceBind::Text(
                    PREVIOUS_RUNS_SOURCE_KEY.to_owned(),
                ),
            ]
        );
        let mut application = FakeApplication {
            fail_readiness: false,
            invocation: invocation(),
            trace: Rc::clone(&trace),
        };

        let exit_code =
            run_prepared_forecast_anchor_backfill_application(prepared, &mut application)
                .expect("prepared application");

        assert_eq!(exit_code, 0);
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "pool",
                "provider",
                "time",
                "readiness",
                "resolve",
                "resume",
                "lock",
                "abandon",
                "start",
                "fetch",
                "complete:1",
                "release",
                "file:report.json:0",
                "stdout:0",
                "close:1",
            ]
        );
    }

    // prove the original omitted source remains assigned to the unported Archive backend
    #[test]
    fn omitted_source_is_not_rerouted_to_previous_runs() {
        let mut time = FakeTime;
        let mut material = InvocationMaterial;
        let result = prepare_forecast_anchor_backfill_invocation(
            &[
                "--site".to_owned(),
                "ballydidean".to_owned(),
                "--from".to_owned(),
                "2026-03-01".to_owned(),
                "--to".to_owned(),
                "2026-03-01".to_owned(),
            ],
            &site_configuration(),
            &mut time,
            &mut material,
        );

        assert_eq!(
            result,
            Err(ForecastAnchorBackfillInvocationError::ArchiveBackendRequired)
        );
    }

    #[test]
    fn one_shot_application_preserves_dependency_and_atomic_write_order() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut application = FakeApplication {
            fail_readiness: false,
            invocation: invocation(),
            trace: Rc::clone(&trace),
        };
        let exit_code =
            run_forecast_anchor_backfill_application(&mut application).expect("application");
        assert_eq!(exit_code, 0);
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "load",
                "pool",
                "provider",
                "time",
                "readiness",
                "resolve",
                "resume",
                "lock",
                "abandon",
                "start",
                "fetch",
                "complete:1",
                "release",
                "file:report.json:0",
                "stdout:0",
                "close:1",
            ]
        );
    }

    #[test]
    fn typed_worker_executor_persists_actual_anchor_through_database_plans() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let invocation = invocation();
        let source = source(&invocation.configuration);
        let database = ForecastAnchorDatabaseFixture {
            trace: Rc::clone(&trace),
        };
        let mut repository =
            DatabaseForecastAnchorBackfillRepository::new(database, ForecastAnchorValueFixture);
        let mut provider = FakeProvider {
            trace: Rc::clone(&trace),
        };
        let mut time = FakeTime;
        let report = execute_forecast_anchor_backfill(
            &invocation.arguments,
            &invocation.configuration,
            &source,
            &mut repository,
            &mut provider,
            &mut time,
        )
        .expect("typed database backfill");
        assert_eq!(report.exit_code, 0);
        assert_eq!(
            report.chunks[0].status,
            weather_worker::ForecastAnchorBackfillChunkStatus::Completed
        );
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "db:resume:6",
                "db:lock",
                "db:abandon:2",
                "db:find:1",
                "db:start:10",
                "fetch",
                "db:begin",
                "db:assert:7",
                "db:records:1",
                "db:outcome:11",
                "db:finalize:7",
                "db:commit",
                "db:release",
            ]
        );
    }

    #[test]
    fn one_shot_application_closes_pool_after_readiness_failure() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut application = FakeApplication {
            fail_readiness: true,
            invocation: invocation(),
            trace: Rc::clone(&trace),
        };
        let failure = run_forecast_anchor_backfill_application(&mut application)
            .expect_err("readiness failure");
        assert_eq!(failure.code, "database_not_ready");
        assert_eq!(
            trace.borrow().as_slice(),
            ["load", "pool", "provider", "time", "readiness", "close:0"]
        );
    }
}
