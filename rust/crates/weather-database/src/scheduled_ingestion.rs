use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{FieldPresence, NormalizedWeatherRecord, Nullable, SourceKind};

use crate::RetainedSourceSession;
use crate::revision_archive::{
    ChangedWeatherRevisionInput, StagedWeatherRevisionPublications, WeatherRevisionArchiveError,
    WeatherRevisionArchivePort, WeatherRevisionArchiveSelection,
    publish_committed_weather_revisions, select_changed_weather_revision_inputs,
    stage_changed_weather_revisions,
};

/// match JavaScript's largest exactly representable integer.
const JAVASCRIPT_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
/// match the source repository's PostgreSQL parameter-safe record batch.
const WEATHER_RECORD_BATCH_SIZE: usize = 2_000;

/// the source-owned running scheduled-ingestion identity guard.
pub const ASSERT_SCHEDULED_RUN_SQL: &str = r#"
      SELECT 1
      FROM ingestion_runs
      WHERE id = $1
        AND source_id = $2
        AND state = 'running'
        AND mode = 'scheduled'
        AND requested_start = $3
        AND requested_end_exclusive = $4
    "#;

/// the source-owned initial scheduled checkpoint insert.
pub const INSERT_SCHEDULED_CHECKPOINT_SQL: &str = r#"
        INSERT INTO ingestion_checkpoints (
          source_id,
          last_valid_at,
          window_start,
          window_end_exclusive,
          provider_cursor
        )
        VALUES ($1, $2, $3, $4, $5::jsonb)
        ON CONFLICT (source_id) DO NOTHING
      "#;

/// the source-owned scheduled checkpoint compare-and-set update.
pub const UPDATE_SCHEDULED_CHECKPOINT_SQL: &str = r#"
      UPDATE ingestion_checkpoints
      SET
        last_valid_at = $3,
        window_start = $4,
        window_end_exclusive = $5,
        provider_cursor = $6::jsonb,
        version = version + 1,
        last_committed_at = clock_timestamp()
      WHERE source_id = $1
        AND version = $2
    "#;

/// the source-owned successful scheduled-run finalization update.
pub const FINALIZE_SCHEDULED_RUN_SQL: &str = r#"
      UPDATE ingestion_runs
      SET
        state = 'succeeded',
        completed_at = clock_timestamp(),
        attempts = $3,
        record_count = $4,
        response_metadata = $5::jsonb,
        upstream_response_checksum = $6,
        error_classification = NULL,
        error_code = NULL,
        error_message = NULL
      WHERE id = $1
        AND source_id = $2
        AND state = 'running'
        AND mode = $7
    "#;

/// one JSON document already serialized by the admitted shared adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializedJson(String);

impl SerializedJson {
    /// admit one adapter-produced JSON document without re-encoding it.
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// borrow the adapter-produced document.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// one SHA-256 digest admitted from the shared hashing boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    /// admit one shared-adapter digest without implementing hashing here.
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// borrow the admitted lowercase hexadecimal digest.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// storage evidence that the shared adapter derives from one normalized row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherRecordStorageEvidence {
    pub content_hash: Sha256Digest,
    pub provider_metadata_json: Option<SerializedJson>,
    pub quality_metadata_json: Option<SerializedJson>,
}

/// one canonical row plus its precomputed hash and JSON storage evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedWeatherRecord {
    pub record: NormalizedWeatherRecord,
    pub storage: WeatherRecordStorageEvidence,
}

/// the complete scheduled-ingestion input after shared validation and encoding.
#[derive(Debug, Clone, PartialEq)]
pub struct CompleteScheduledIngestion {
    pub attempts: u64,
    pub expected_checkpoint_version: Option<u64>,
    pub last_valid_at: String,
    pub provider_cursor_json: Option<SerializedJson>,
    pub records: Vec<PreparedWeatherRecord>,
    pub response_metadata_json: Option<SerializedJson>,
    pub run_id: String,
    pub upstream_response_checksum: Option<Sha256Digest>,
    pub window_end_exclusive: String,
    pub window_start: String,
}

/// typed values for the running-row identity guard in placeholder order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledRunIdentityBind {
    BigintText(String),
    UtcInstant(String),
}

/// the exact scheduled-run identity query and binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledRunIdentityQuery {
    pub sql: &'static str,
    pub binds: [ScheduledRunIdentityBind; 4],
}

/// typed values for one normalized weather-record upsert.
#[derive(Debug, Clone, PartialEq)]
pub enum WeatherRecordBind {
    BigintText(String),
    Text(String),
    NullableText(Option<String>),
    SourceKind(SourceKind),
    UtcInstant(String),
    NullableUtcInstant(Option<String>),
    NullableJson(Option<SerializedJson>),
    NullableNumber(Option<f64>),
    ContentHash(Sha256Digest),
}

/// one exact bounded weather-record upsert statement and bind sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherRecordUpsertQuery {
    pub sql: String,
    pub binds: Vec<WeatherRecordBind>,
    pub record_count: usize,
}

/// typed values for initial checkpoint insertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitialCheckpointBind {
    BigintText(String),
    UtcInstant(String),
    NullableJson(Option<SerializedJson>),
}

/// typed values for checkpoint compare-and-set update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointUpdateBind {
    BigintText(String),
    Version(u64),
    UtcInstant(String),
    NullableJson(Option<SerializedJson>),
}

/// one exact initial or compare-and-set checkpoint write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledCheckpointQuery {
    Insert {
        sql: &'static str,
        binds: [InitialCheckpointBind; 5],
    },
    Update {
        sql: &'static str,
        binds: [CheckpointUpdateBind; 6],
    },
}

/// typed values for successful-run finalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalizeScheduledRunBind {
    BigintText(String),
    Count(u64),
    NullableJson(Option<SerializedJson>),
    NullableChecksum(Option<Sha256Digest>),
    ScheduledMode,
}

/// one exact successful scheduled-run finalization query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizeScheduledRunQuery {
    pub sql: &'static str,
    pub binds: [FinalizeScheduledRunBind; 7],
}

/// the exact guarded operation labels emitted by the TypeScript repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduledIngestionGuard {
    ScheduledRunIdentity,
    InitialCheckpointInsert,
    CheckpointCompareAndSet,
    SuccessfulIngestionFinalization,
}

impl ScheduledIngestionGuard {
    /// return the source repository's exact failure label.
    pub const fn operation(self) -> &'static str {
        // preserve the owning TypeScript error wording
        match self {
            Self::ScheduledRunIdentity => "scheduled run identity check",
            Self::InitialCheckpointInsert => "initial checkpoint insert",
            Self::CheckpointCompareAndSet => "checkpoint compare-and-set",
            Self::SuccessfulIngestionFinalization => "successful ingestion finalization",
        }
    }
}

/// a bounded pre-command input failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledIngestionInputError {
    CompletionCountOutsideJavaScriptSafeInteger,
    RecordSourceMismatch { expected: String, actual: String },
}

/// a complete scheduled-ingestion transaction failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledIngestionExecutionError<CommandError> {
    Input(ScheduledIngestionInputError),
    GuardedWrite {
        guard: ScheduledIngestionGuard,
        row_count: usize,
    },
    RevisionArchive(WeatherRevisionArchiveError<CommandError>),
    Command(CommandError),
}

impl Display for ScheduledIngestionInputError {
    /// render the owning TypeScript input failure text.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the source input-error categories
        match self {
            Self::CompletionCountOutsideJavaScriptSafeInteger => {
                formatter.write_str("completion counts must be non-negative integers")
            }
            Self::RecordSourceMismatch { .. } => {
                formatter.write_str("weather record source does not match the locked source")
            }
        }
    }
}

impl Error for ScheduledIngestionInputError {}

impl<CommandError: Display> Display for ScheduledIngestionExecutionError<CommandError> {
    /// render exact source-compatible transaction failure text.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve input, guarded-write, and adapter command failures
        match self {
            Self::Input(error) => Display::fmt(error, formatter),
            Self::GuardedWrite { guard, .. } => {
                write!(
                    formatter,
                    "{} did not affect exactly one row",
                    guard.operation()
                )
            }
            Self::RevisionArchive(error) => Display::fmt(error, formatter),
            Self::Command(error) => Display::fmt(error, formatter),
        }
    }
}

impl<CommandError> Error for ScheduledIngestionExecutionError<CommandError> where
    CommandError: Error + 'static
{
}

/// the injected database-command boundary for one retained source session.
pub trait ScheduledIngestionCommandPort: RetainedSourceSession {
    type Error;

    /// start the source transaction.
    fn begin_transaction(&mut self) -> Result<(), Self::Error>;

    /// execute the running scheduled-row identity guard.
    fn assert_scheduled_run(
        &mut self,
        query: &ScheduledRunIdentityQuery,
    ) -> Result<usize, Self::Error>;

    /// execute one bounded normalized weather-record upsert.
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error>;

    /// execute the initial or compare-and-set checkpoint write.
    fn advance_scheduled_checkpoint(
        &mut self,
        query: &ScheduledCheckpointQuery,
    ) -> Result<usize, Self::Error>;

    /// guard-finalize the running scheduled row.
    fn finalize_scheduled_run(
        &mut self,
        query: &FinalizeScheduledRunQuery,
    ) -> Result<usize, Self::Error>;

    /// commit the complete scheduled transaction.
    fn commit_transaction(&mut self) -> Result<(), Self::Error>;

    /// roll back an incomplete scheduled transaction.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error>;
}

/// execute the TypeScript scheduled-ingestion transaction over an injected port.
pub fn complete_scheduled_ingestion<Port: ScheduledIngestionCommandPort>(
    input: &CompleteScheduledIngestion,
    port: &mut Port,
) -> Result<(), ScheduledIngestionExecutionError<Port::Error>> {
    complete_scheduled_ingestion_with_strategy(input, port, &mut DisabledRevisionArchive)
}

/// execute scheduled ingestion with the optional revision archive on the same port.
pub fn complete_scheduled_ingestion_with_revision_archive<Port>(
    input: &CompleteScheduledIngestion,
    port: &mut Port,
) -> Result<(), ScheduledIngestionExecutionError<<Port as ScheduledIngestionCommandPort>::Error>>
where
    Port: ScheduledIngestionCommandPort
        + WeatherRevisionArchivePort<Error = <Port as ScheduledIngestionCommandPort>::Error>,
{
    complete_scheduled_ingestion_with_strategy(input, port, &mut EnabledRevisionArchive)
}

/// execute one shared scheduled transaction with the selected archive strategy.
fn complete_scheduled_ingestion_with_strategy<Port, Archive>(
    input: &CompleteScheduledIngestion,
    port: &mut Port,
    archive: &mut Archive,
) -> Result<(), ScheduledIngestionExecutionError<Port::Error>>
where
    Port: ScheduledIngestionCommandPort,
    Archive: ScheduledRevisionArchive<Port>,
{
    // preserve validateCompletionCounts before BEGIN
    if input.attempts > JAVASCRIPT_MAX_SAFE_INTEGER
        || u64::try_from(input.records.len()).unwrap_or(u64::MAX) > JAVASCRIPT_MAX_SAFE_INTEGER
    {
        return Err(ScheduledIngestionExecutionError::Input(
            ScheduledIngestionInputError::CompletionCountOutsideJavaScriptSafeInteger,
        ));
    }

    let source_id = port.source_id().to_owned();

    // reject cross-source rows before any database command
    for record in &input.records {
        // preserve the retained session as the only write authority
        if record.record.sourceId != source_id {
            return Err(ScheduledIngestionExecutionError::Input(
                ScheduledIngestionInputError::RecordSourceMismatch {
                    expected: source_id,
                    actual: record.record.sourceId.clone(),
                },
            ));
        }
    }

    port.begin_transaction()
        .map_err(ScheduledIngestionExecutionError::Command)?;

    let operation = execute_scheduled_ingestion_body(input, &source_id, port, archive);

    // mirror withTransaction rollback for any body or COMMIT failure
    match operation {
        Ok(staged) => {
            // keep commit inside the transaction try block
            if let Err(error) = port.commit_transaction() {
                return rollback_after_failure(
                    port,
                    ScheduledIngestionExecutionError::Command(error),
                );
            }

            archive.publish_after_commit(staged, port);
            Ok(())
        }
        Err(error) => rollback_after_failure(port, error),
    }
}

/// execute every scheduled operation between BEGIN and COMMIT.
fn execute_scheduled_ingestion_body<Port, Archive>(
    input: &CompleteScheduledIngestion,
    source_id: &str,
    port: &mut Port,
    archive: &mut Archive,
) -> Result<Archive::Staged, ScheduledIngestionExecutionError<Port::Error>>
where
    Port: ScheduledIngestionCommandPort,
    Archive: ScheduledRevisionArchive<Port>,
{
    let identity = scheduled_run_identity_query(input, source_id);
    let row_count = port
        .assert_scheduled_run(&identity)
        .map_err(ScheduledIngestionExecutionError::Command)?;
    require_guarded_row(row_count, ScheduledIngestionGuard::ScheduledRunIdentity)?;

    let changed = archive.select_changed(&input.records, port)?;

    // preserve the source repository's bounded batch order
    for records in input.records.chunks(WEATHER_RECORD_BATCH_SIZE) {
        let query = weather_record_upsert_query(source_id, &input.run_id, records)
            .map_err(ScheduledIngestionExecutionError::Input)?;
        port.upsert_weather_records(&query)
            .map_err(ScheduledIngestionExecutionError::Command)?;
    }

    let staged = archive.stage_changed(&input.run_id, &changed, port)?;

    let checkpoint = scheduled_checkpoint_query(input, source_id);
    let checkpoint_guard = checkpoint_guard(&checkpoint);
    let row_count = port
        .advance_scheduled_checkpoint(&checkpoint)
        .map_err(ScheduledIngestionExecutionError::Command)?;
    require_guarded_row(row_count, checkpoint_guard)?;

    let finalization = finalize_scheduled_run_query(input, source_id);
    let row_count = port
        .finalize_scheduled_run(&finalization)
        .map_err(ScheduledIngestionExecutionError::Command)?;
    require_guarded_row(
        row_count,
        ScheduledIngestionGuard::SuccessfulIngestionFinalization,
    )?;

    Ok(staged)
}

/// archive behavior injected into the one shared scheduled transaction.
trait ScheduledRevisionArchive<Port: ScheduledIngestionCommandPort> {
    type Staged;

    /// select changed rows before the serving upsert.
    fn select_changed(
        &mut self,
        records: &[PreparedWeatherRecord],
        port: &mut Port,
    ) -> Result<Vec<ChangedWeatherRevisionInput>, ScheduledIngestionExecutionError<Port::Error>>;

    /// stage changed rows after the serving upsert.
    fn stage_changed(
        &mut self,
        run_id: &str,
        changed: &[ChangedWeatherRevisionInput],
        port: &mut Port,
    ) -> Result<Self::Staged, ScheduledIngestionExecutionError<Port::Error>>;

    /// publish staged work only after the shared transaction commits.
    fn publish_after_commit(&mut self, staged: Self::Staged, port: &mut Port);
}

/// the ordinary no-archiver fast path.
struct DisabledRevisionArchive;

impl<Port: ScheduledIngestionCommandPort> ScheduledRevisionArchive<Port>
    for DisabledRevisionArchive
{
    type Staged = ();

    /// omit selection without touching the archive adapter.
    fn select_changed(
        &mut self,
        _records: &[PreparedWeatherRecord],
        _port: &mut Port,
    ) -> Result<Vec<ChangedWeatherRevisionInput>, ScheduledIngestionExecutionError<Port::Error>>
    {
        Ok(Vec::new())
    }

    /// omit staging without creating a savepoint.
    fn stage_changed(
        &mut self,
        _run_id: &str,
        _changed: &[ChangedWeatherRevisionInput],
        _port: &mut Port,
    ) -> Result<Self::Staged, ScheduledIngestionExecutionError<Port::Error>> {
        Ok(())
    }

    /// retain the no-publication fast path after commit.
    fn publish_after_commit(&mut self, _staged: Self::Staged, _port: &mut Port) {}
}

/// the archive-enabled shared transaction strategy.
struct EnabledRevisionArchive;

impl<Port> ScheduledRevisionArchive<Port> for EnabledRevisionArchive
where
    Port: ScheduledIngestionCommandPort
        + WeatherRevisionArchivePort<Error = <Port as ScheduledIngestionCommandPort>::Error>,
{
    type Staged =
        StagedWeatherRevisionPublications<<Port as WeatherRevisionArchivePort>::Publication>;

    /// select exact changed identities before any serving upsert.
    fn select_changed(
        &mut self,
        records: &[PreparedWeatherRecord],
        port: &mut Port,
    ) -> Result<
        Vec<ChangedWeatherRevisionInput>,
        ScheduledIngestionExecutionError<<Port as ScheduledIngestionCommandPort>::Error>,
    > {
        select_changed_weather_revision_inputs(
            WeatherRevisionArchiveSelection::Enabled,
            records,
            port,
        )
        .map_err(ScheduledIngestionExecutionError::RevisionArchive)
    }

    /// stage exact changed rows on the retained transaction port.
    fn stage_changed(
        &mut self,
        run_id: &str,
        changed: &[ChangedWeatherRevisionInput],
        port: &mut Port,
    ) -> Result<
        Self::Staged,
        ScheduledIngestionExecutionError<<Port as ScheduledIngestionCommandPort>::Error>,
    > {
        stage_changed_weather_revisions(run_id, changed, port)
            .map_err(ScheduledIngestionExecutionError::RevisionArchive)
    }

    /// admit and publish staged work after the successful shared commit.
    fn publish_after_commit(&mut self, staged: Self::Staged, port: &mut Port) {
        let committed = staged
            .after_caller_commit(Ok::<(), std::convert::Infallible>(()))
            .expect("infallible committed revision admission");
        publish_committed_weather_revisions(committed, port);
    }
}

/// roll back and preserve the source transaction's rollback-error precedence.
fn rollback_after_failure<Port: ScheduledIngestionCommandPort>(
    port: &mut Port,
    failure: ScheduledIngestionExecutionError<Port::Error>,
) -> Result<(), ScheduledIngestionExecutionError<Port::Error>> {
    // a failing ROLLBACK masks the original error in withTransaction
    match port.rollback_transaction() {
        Ok(()) => Err(failure),
        Err(rollback_error) => Err(ScheduledIngestionExecutionError::Command(rollback_error)),
    }
}

/// require the one-row lifecycle guard used by the TypeScript repository.
fn require_guarded_row<CommandError>(
    row_count: usize,
    guard: ScheduledIngestionGuard,
) -> Result<(), ScheduledIngestionExecutionError<CommandError>> {
    // reject stale or duplicated lifecycle transitions
    if row_count != 1 {
        return Err(ScheduledIngestionExecutionError::GuardedWrite { guard, row_count });
    }

    Ok(())
}

/// build the exact running-row identity query.
pub fn scheduled_run_identity_query(
    input: &CompleteScheduledIngestion,
    source_id: &str,
) -> ScheduledRunIdentityQuery {
    ScheduledRunIdentityQuery {
        sql: ASSERT_SCHEDULED_RUN_SQL,
        binds: [
            ScheduledRunIdentityBind::BigintText(input.run_id.clone()),
            ScheduledRunIdentityBind::BigintText(source_id.to_owned()),
            ScheduledRunIdentityBind::UtcInstant(input.window_start.clone()),
            ScheduledRunIdentityBind::UtcInstant(input.window_end_exclusive.clone()),
        ],
    }
}

/// build the exact initial or compare-and-set checkpoint query.
pub fn scheduled_checkpoint_query(
    input: &CompleteScheduledIngestion,
    source_id: &str,
) -> ScheduledCheckpointQuery {
    // select the same checkpoint branch as expectedCheckpointVersion
    match input.expected_checkpoint_version {
        None => ScheduledCheckpointQuery::Insert {
            sql: INSERT_SCHEDULED_CHECKPOINT_SQL,
            binds: [
                InitialCheckpointBind::BigintText(source_id.to_owned()),
                InitialCheckpointBind::UtcInstant(input.last_valid_at.clone()),
                InitialCheckpointBind::UtcInstant(input.window_start.clone()),
                InitialCheckpointBind::UtcInstant(input.window_end_exclusive.clone()),
                InitialCheckpointBind::NullableJson(input.provider_cursor_json.clone()),
            ],
        },
        Some(version) => ScheduledCheckpointQuery::Update {
            sql: UPDATE_SCHEDULED_CHECKPOINT_SQL,
            binds: [
                CheckpointUpdateBind::BigintText(source_id.to_owned()),
                CheckpointUpdateBind::Version(version),
                CheckpointUpdateBind::UtcInstant(input.last_valid_at.clone()),
                CheckpointUpdateBind::UtcInstant(input.window_start.clone()),
                CheckpointUpdateBind::UtcInstant(input.window_end_exclusive.clone()),
                CheckpointUpdateBind::NullableJson(input.provider_cursor_json.clone()),
            ],
        },
    }
}

/// identify the exact guarded checkpoint operation.
fn checkpoint_guard(query: &ScheduledCheckpointQuery) -> ScheduledIngestionGuard {
    // retain distinct TypeScript error labels for insert and update
    match query {
        ScheduledCheckpointQuery::Insert { .. } => ScheduledIngestionGuard::InitialCheckpointInsert,
        ScheduledCheckpointQuery::Update { .. } => ScheduledIngestionGuard::CheckpointCompareAndSet,
    }
}

/// build the exact successful scheduled-run finalization query.
pub fn finalize_scheduled_run_query(
    input: &CompleteScheduledIngestion,
    source_id: &str,
) -> FinalizeScheduledRunQuery {
    FinalizeScheduledRunQuery {
        sql: FINALIZE_SCHEDULED_RUN_SQL,
        binds: [
            FinalizeScheduledRunBind::BigintText(input.run_id.clone()),
            FinalizeScheduledRunBind::BigintText(source_id.to_owned()),
            FinalizeScheduledRunBind::Count(input.attempts),
            FinalizeScheduledRunBind::Count(input.records.len() as u64),
            FinalizeScheduledRunBind::NullableJson(input.response_metadata_json.clone()),
            FinalizeScheduledRunBind::NullableChecksum(input.upstream_response_checksum.clone()),
            FinalizeScheduledRunBind::ScheduledMode,
        ],
    }
}

/// build one exact bounded weather-record upsert query.
pub fn weather_record_upsert_query(
    source_id: &str,
    run_id: &str,
    records: &[PreparedWeatherRecord],
) -> Result<WeatherRecordUpsertQuery, ScheduledIngestionInputError> {
    let mut binds = Vec::with_capacity(records.len() * 32);
    let mut placeholders = Vec::with_capacity(records.len());

    // preserve caller order and exact 32-value row bind groups
    for record in records {
        // reject cross-source writes at the same transaction point as TypeScript
        if record.record.sourceId != source_id {
            return Err(ScheduledIngestionInputError::RecordSourceMismatch {
                expected: source_id.to_owned(),
                actual: record.record.sourceId.clone(),
            });
        }

        let first_parameter = binds.len() + 1;
        append_weather_record_binds(&mut binds, run_id, record);
        placeholders.push(weather_record_placeholder(first_parameter));
    }

    Ok(WeatherRecordUpsertQuery {
        sql: format!(
            r#"
      INSERT INTO weather_records (
          source_id,
          source_kind,
          valid_at,
          product_run_at,
          first_ingestion_run_id,
          last_ingestion_run_id,
          first_received_at,
          last_received_at,
          upstream_timezone,
          upstream_model,
          device_vendor,
          device_model,
          device_serial,
          quality_metadata,
          provider_metadata,
          temperature_c,
          apparent_temperature_c,
          precipitation_mm,
          wind_speed_mps,
          wind_gust_mps,
          pressure_hpa,
          relative_humidity_percent,
          cloud_cover_percent,
          wind_direction_degrees,
          black_globe_temperature_c,
          pm25_micrograms_per_cubic_meter,
          precipitation_rate_mm_per_hour,
          soil_electrical_conductivity_us_cm,
          soil_moisture_percent,
          solar_radiation_wm2,
          uv_index,
          wet_bulb_globe_temperature_c,
          water_level_m,
          content_hash
      )
      VALUES {}
      ON CONFLICT ON CONSTRAINT weather_records_identity_key DO UPDATE SET
        last_ingestion_run_id = EXCLUDED.last_ingestion_run_id,
        last_received_at = EXCLUDED.last_received_at,
        upstream_timezone = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.upstream_timezone ELSE weather_records.upstream_timezone END,
        upstream_model = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.upstream_model ELSE weather_records.upstream_model END,
        device_vendor = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.device_vendor ELSE weather_records.device_vendor END,
        device_model = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.device_model ELSE weather_records.device_model END,
        device_serial = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.device_serial ELSE weather_records.device_serial END,
        quality_metadata = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.quality_metadata ELSE weather_records.quality_metadata END,
        provider_metadata = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.provider_metadata ELSE weather_records.provider_metadata END,
        temperature_c = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.temperature_c ELSE weather_records.temperature_c END,
        apparent_temperature_c = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.apparent_temperature_c ELSE weather_records.apparent_temperature_c END,
        precipitation_mm = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.precipitation_mm ELSE weather_records.precipitation_mm END,
        wind_speed_mps = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wind_speed_mps ELSE weather_records.wind_speed_mps END,
        wind_gust_mps = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wind_gust_mps ELSE weather_records.wind_gust_mps END,
        pressure_hpa = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.pressure_hpa ELSE weather_records.pressure_hpa END,
        relative_humidity_percent = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.relative_humidity_percent ELSE weather_records.relative_humidity_percent END,
        cloud_cover_percent = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.cloud_cover_percent ELSE weather_records.cloud_cover_percent END,
        wind_direction_degrees = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wind_direction_degrees ELSE weather_records.wind_direction_degrees END,
        black_globe_temperature_c = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.black_globe_temperature_c ELSE weather_records.black_globe_temperature_c END,
        pm25_micrograms_per_cubic_meter = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.pm25_micrograms_per_cubic_meter ELSE weather_records.pm25_micrograms_per_cubic_meter END,
        precipitation_rate_mm_per_hour = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.precipitation_rate_mm_per_hour ELSE weather_records.precipitation_rate_mm_per_hour END,
        soil_electrical_conductivity_us_cm = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.soil_electrical_conductivity_us_cm ELSE weather_records.soil_electrical_conductivity_us_cm END,
        soil_moisture_percent = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.soil_moisture_percent ELSE weather_records.soil_moisture_percent END,
        solar_radiation_wm2 = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.solar_radiation_wm2 ELSE weather_records.solar_radiation_wm2 END,
        uv_index = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.uv_index ELSE weather_records.uv_index END,
        wet_bulb_globe_temperature_c = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wet_bulb_globe_temperature_c ELSE weather_records.wet_bulb_globe_temperature_c END,
        water_level_m = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.water_level_m ELSE weather_records.water_level_m END,
        revision_count = weather_records.revision_count + CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN 1 ELSE 0 END,
        content_hash = EXCLUDED.content_hash
    "#,
            placeholders.join(",\n        ")
        ),
        binds,
        record_count: records.len(),
    })
}

/// append one source-ordered 32-value weather-record bind group.
fn append_weather_record_binds(
    binds: &mut Vec<WeatherRecordBind>,
    run_id: &str,
    prepared: &PreparedWeatherRecord,
) {
    let record = &prepared.record;
    let (device_vendor, device_model, device_serial) = device_binds(&record.metadata.device);

    binds.extend([
        WeatherRecordBind::BigintText(record.sourceId.clone()),
        WeatherRecordBind::SourceKind(record.sourceKind),
        WeatherRecordBind::UtcInstant(record.validAt.clone()),
        WeatherRecordBind::NullableUtcInstant(nullable_clone(&record.productRunAt)),
        WeatherRecordBind::BigintText(run_id.to_owned()),
        WeatherRecordBind::UtcInstant(record.receivedAt.clone()),
        WeatherRecordBind::Text(record.metadata.upstreamTimezone.clone()),
        WeatherRecordBind::NullableText(nullable_clone(&record.metadata.model)),
        WeatherRecordBind::NullableText(device_vendor),
        WeatherRecordBind::NullableText(device_model),
        WeatherRecordBind::NullableText(device_serial),
        WeatherRecordBind::NullableJson(prepared.storage.quality_metadata_json.clone()),
        WeatherRecordBind::NullableJson(prepared.storage.provider_metadata_json.clone()),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.temperatureC)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.apparentTemperatureC)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.precipitationMm)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.windSpeedMps)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.windGustMps)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.pressureHpa)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.relativeHumidityPercent)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.cloudCoverPercent)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.windDirectionDegrees)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.blackGlobeTemperatureC)),
        WeatherRecordBind::NullableNumber(nullable_copy(
            &record.metrics.pm25MicrogramsPerCubicMeter,
        )),
        WeatherRecordBind::NullableNumber(nullable_copy(
            &record.metrics.precipitationRateMmPerHour,
        )),
        WeatherRecordBind::NullableNumber(nullable_copy(
            &record.metrics.soilElectricalConductivityMicrosiemensPerCm,
        )),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.soilMoisturePercent)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.solarRadiationWm2)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.uvIndex)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.wetBulbGlobeTemperatureC)),
        WeatherRecordBind::NullableNumber(nullable_copy(&record.metrics.waterLevelM)),
        WeatherRecordBind::ContentHash(prepared.storage.content_hash.clone()),
    ]);
}

/// convert generated nullable owned storage without changing its value.
fn nullable_clone<Value: Clone>(value: &Nullable<Value>) -> Option<Value> {
    // preserve generated null versus value semantics
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(value.clone()),
    }
}

/// convert generated nullable copy storage without changing its value.
fn nullable_copy<Value: Copy>(value: &Nullable<Value>) -> Option<Value> {
    // preserve generated null versus value semantics
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(*value),
    }
}

/// project optional generated device fields to nullable SQL text binds.
fn device_binds(
    device: &Nullable<weather_contracts::DeviceMetadata>,
) -> (Option<String>, Option<String>, Option<String>) {
    // preserve null device metadata and missing individual fields
    match device {
        Nullable::Null => (None, None, None),
        Nullable::Value(device) => (
            field_presence_clone(&device.vendor),
            field_presence_clone(&device.model),
            field_presence_clone(&device.serial),
        ),
    }
}

/// convert one generated optional field to a nullable SQL bind.
fn field_presence_clone<Value: Clone>(value: &FieldPresence<Value>) -> Option<Value> {
    // preserve missing versus present generated storage
    match value {
        FieldPresence::Missing => None,
        FieldPresence::Value(value) => Some(value.clone()),
    }
}

/// create one exact 34-column tuple from a 32-value bind group.
fn weather_record_placeholder(first_parameter: usize) -> String {
    // create the same 32 base placeholders before duplicating storage columns
    let mut parameters = (0..32)
        .map(|index| format!("${}", first_parameter + index))
        .collect::<Vec<_>>();
    parameters[11].push_str("::jsonb");
    parameters[12].push_str("::jsonb");
    parameters.insert(5, parameters[4].clone());
    parameters.insert(7, parameters[6].clone());
    format!("({})", parameters.join(", "))
}
