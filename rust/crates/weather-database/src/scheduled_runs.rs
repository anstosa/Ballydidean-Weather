use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    IngestionError, IngestionErrorClassification, IngestionState, JsonValue, Nullable,
    ScheduledCheckpointState,
};

use crate::{RetainedSourceSession, SerializedJson, Sha256Digest};

/// match JavaScript's largest exactly representable integer
const JAVASCRIPT_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// the source-owned running-row lookup before scheduled start
pub const FIND_RUNNING_INGESTION_SQL: &str = r#"
      SELECT id, deadline_at
      FROM ingestion_runs
      WHERE source_id = $1
        AND state = 'running'
      FOR UPDATE
    "#;

/// the source-owned scheduled running-row insert
pub const INSERT_SCHEDULED_INGESTION_RUN_SQL: &str = r#"
      INSERT INTO ingestion_runs (
        source_id,
        mode,
        requested_start,
        requested_end_exclusive,
        source_config_fingerprint,
        adapter_version,
        chunk_plan_version,
        deadline_at,
        attempts,
        request_metadata
      )
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb)
      RETURNING id, started_at AS "startedAt", state
    "#;

/// the source-owned scheduled checkpoint read
pub const GET_SCHEDULED_CHECKPOINT_SQL: &str = r#"
      SELECT
        source_id,
        last_valid_at,
        window_start,
        window_end_exclusive,
        provider_cursor,
        version,
        last_committed_at
      FROM ingestion_checkpoints
      WHERE source_id = $1
    "#;

/// the source-owned running scheduled-mode guard
pub const ASSERT_RUNNING_SCHEDULED_MODE_SQL: &str = r#"
      SELECT 1
      FROM ingestion_runs
      WHERE id = $1
        AND source_id = $2
        AND state = 'running'
        AND mode = $3
    "#;

/// the source-owned failed-run finalization update
pub const FINALIZE_FAILED_INGESTION_SQL: &str = r#"
        UPDATE ingestion_runs
        SET
          state = 'failed',
          completed_at = clock_timestamp(),
          attempts = $3,
          response_metadata = $4::jsonb,
          error_classification = $5,
          error_code = $6,
          error_message = $7
        WHERE id = $1
          AND source_id = $2
          AND state = 'running'
      "#;

/// one adapter version admitted by the shared domain validator
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedAdapterVersion(String);

impl AdmittedAdapterVersion {
    /// admit one shared-domain validated adapter version
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// borrow the admitted adapter version
    pub fn value(&self) -> &str {
        &self.0
    }
}

/// one ordered future scheduled window admitted by the shared time boundary
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedScheduledRunTimes {
    deadline_at: String,
    requested_end_exclusive: String,
    requested_start: String,
}

impl AdmittedScheduledRunTimes {
    /// admit one shared-domain validated scheduled window
    pub fn admitted(
        requested_start: impl Into<String>,
        requested_end_exclusive: impl Into<String>,
        deadline_at: impl Into<String>,
    ) -> Self {
        Self {
            deadline_at: deadline_at.into(),
            requested_end_exclusive: requested_end_exclusive.into(),
            requested_start: requested_start.into(),
        }
    }
}

/// one scheduled start after shared time and version validation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartScheduledIngestion {
    pub adapter_version: AdmittedAdapterVersion,
    pub attempts: u64,
    pub request_metadata_json: Option<SerializedJson>,
    pub source_config_fingerprint: Sha256Digest,
    pub times: AdmittedScheduledRunTimes,
}

/// one returned scheduled running row
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedScheduledIngestion {
    pub id: String,
    pub started_at: String,
    pub state: IngestionState,
}

/// the existing running-row lookup result used by Node error mapping
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningIngestionLookup {
    pub first_run_id: Option<String>,
    pub row_count: usize,
}

/// typed values for the running-row lookup
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindRunningIngestionBind {
    BigintText(String),
}

/// one exact running-row lookup
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindRunningIngestionQuery {
    pub sql: &'static str,
    pub binds: [FindRunningIngestionBind; 1],
}

/// typed values for the scheduled running-row insert
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertScheduledIngestionBind {
    BigintText(String),
    Text(String),
    ScheduledMode,
    UtcInstant(String),
    Fingerprint(Sha256Digest),
    NullChunkPlanVersion,
    Count(u64),
    NullableJson(Option<SerializedJson>),
}

/// one exact scheduled running-row insert
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertScheduledIngestionQuery {
    pub sql: &'static str,
    pub binds: [InsertScheduledIngestionBind; 10],
}

/// the typed scheduled-start command boundary
pub trait ScheduledRunStartCommandPort: RetainedSourceSession {
    type Error;

    /// read any running source row under the retained source session
    fn find_running_ingestion(
        &mut self,
        query: &FindRunningIngestionQuery,
    ) -> Result<RunningIngestionLookup, Self::Error>;

    /// insert and return one scheduled running row
    fn insert_scheduled_ingestion(
        &mut self,
        query: &InsertScheduledIngestionQuery,
    ) -> Result<Option<StartedScheduledIngestion>, Self::Error>;
}

/// a scheduled-start repository failure
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledRunStartError<CommandError> {
    ExistingRunningIngestion { run_id: String },
    MissingInsertedRow,
    Command(CommandError),
}

impl<CommandError: Display> Display for ScheduledRunStartError<CommandError> {
    /// render the source scheduled-start failure text
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve running-row, insert, and command failures
        match self {
            Self::ExistingRunningIngestion { run_id } => {
                write!(
                    formatter,
                    "source already has a running ingestion: {run_id}"
                )
            }
            Self::MissingInsertedRow => {
                formatter.write_str("running ingestion insert returned no row")
            }
            Self::Command(error) => Display::fmt(error, formatter),
        }
    }
}

impl<CommandError> Error for ScheduledRunStartError<CommandError> where CommandError: Error + 'static
{}

/// start one scheduled running row before provider input and output
pub fn start_scheduled_ingestion<Port: ScheduledRunStartCommandPort>(
    input: &StartScheduledIngestion,
    port: &mut Port,
) -> Result<StartedScheduledIngestion, ScheduledRunStartError<Port::Error>> {
    let source_id = port.source_id().to_owned();
    let lookup = port
        .find_running_ingestion(&find_running_ingestion_query(&source_id))
        .map_err(ScheduledRunStartError::Command)?;

    // reject replacement before explicit recovery
    if lookup.row_count != 0 {
        return Err(ScheduledRunStartError::ExistingRunningIngestion {
            run_id: lookup.first_run_id.unwrap_or_else(|| "unknown".to_owned()),
        });
    }

    let inserted = port
        .insert_scheduled_ingestion(&insert_scheduled_ingestion_query(input, &source_id))
        .map_err(ScheduledRunStartError::Command)?;

    // mirror requireRow for a missing RETURNING row
    match inserted {
        Some(row) => Ok(row),
        None => Err(ScheduledRunStartError::MissingInsertedRow),
    }
}

/// build the exact running-row lookup
pub fn find_running_ingestion_query(source_id: &str) -> FindRunningIngestionQuery {
    FindRunningIngestionQuery {
        sql: FIND_RUNNING_INGESTION_SQL,
        binds: [FindRunningIngestionBind::BigintText(source_id.to_owned())],
    }
}

/// build the exact scheduled running-row insert
pub fn insert_scheduled_ingestion_query(
    input: &StartScheduledIngestion,
    source_id: &str,
) -> InsertScheduledIngestionQuery {
    InsertScheduledIngestionQuery {
        sql: INSERT_SCHEDULED_INGESTION_RUN_SQL,
        binds: [
            InsertScheduledIngestionBind::BigintText(source_id.to_owned()),
            InsertScheduledIngestionBind::ScheduledMode,
            InsertScheduledIngestionBind::UtcInstant(input.times.requested_start.clone()),
            InsertScheduledIngestionBind::UtcInstant(input.times.requested_end_exclusive.clone()),
            InsertScheduledIngestionBind::Fingerprint(input.source_config_fingerprint.clone()),
            InsertScheduledIngestionBind::Text(input.adapter_version.value().to_owned()),
            InsertScheduledIngestionBind::NullChunkPlanVersion,
            InsertScheduledIngestionBind::UtcInstant(input.times.deadline_at.clone()),
            InsertScheduledIngestionBind::Count(input.attempts),
            InsertScheduledIngestionBind::NullableJson(input.request_metadata_json.clone()),
        ],
    }
}

/// typed values for the checkpoint read
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GetScheduledCheckpointBind {
    BigintText(String),
}

/// one exact scheduled checkpoint read
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetScheduledCheckpointQuery {
    pub sql: &'static str,
    pub binds: [GetScheduledCheckpointBind; 1],
}

/// one decoded checkpoint row after admitted time and number conversion
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledCheckpointDbRow {
    pub last_committed_at: String,
    pub last_valid_at: String,
    pub provider_cursor: Nullable<Vec<(Vec<u16>, JsonValue)>>,
    pub source_id: String,
    pub version: f64,
    pub window_end_exclusive: String,
    pub window_start: String,
}

/// the typed checkpoint read boundary
pub trait ScheduledCheckpointReadPort: RetainedSourceSession {
    type Error;

    /// read one optional checkpoint row under the retained source session
    fn get_scheduled_checkpoint(
        &mut self,
        query: &GetScheduledCheckpointQuery,
    ) -> Result<Option<ScheduledCheckpointDbRow>, Self::Error>;
}

/// one checkpoint read or projection failure
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledCheckpointReadError<CommandError> {
    SourceMismatch { expected: String, actual: String },
    Command(CommandError),
}

impl<CommandError: Display> Display for ScheduledCheckpointReadError<CommandError> {
    /// render the source-compatible checkpoint failure text
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve projection and command error categories
        match self {
            Self::SourceMismatch { .. } => {
                formatter.write_str("checkpoint source does not match the retained source")
            }
            Self::Command(error) => Display::fmt(error, formatter),
        }
    }
}

impl<CommandError> Error for ScheduledCheckpointReadError<CommandError> where
    CommandError: Error + 'static
{
}

/// read and project one scheduled checkpoint without reordering fields
pub fn get_scheduled_checkpoint<Port: ScheduledCheckpointReadPort>(
    port: &mut Port,
) -> Result<Option<ScheduledCheckpointState>, ScheduledCheckpointReadError<Port::Error>> {
    let source_id = port.source_id().to_owned();
    let query = GetScheduledCheckpointQuery {
        sql: GET_SCHEDULED_CHECKPOINT_SQL,
        binds: [GetScheduledCheckpointBind::BigintText(source_id.clone())],
    };
    let row = port
        .get_scheduled_checkpoint(&query)
        .map_err(ScheduledCheckpointReadError::Command)?;

    // represent an uninitialized checkpoint as null
    match row {
        None => Ok(None),
        Some(row) => {
            // reject a driver row outside the retained source identity
            if row.source_id != source_id {
                return Err(ScheduledCheckpointReadError::SourceMismatch {
                    expected: source_id,
                    actual: row.source_id,
                });
            }

            Ok(Some(ScheduledCheckpointState {
                lastCommittedAt: row.last_committed_at,
                lastValidAt: row.last_valid_at,
                providerCursor: row.provider_cursor,
                sourceId: row.source_id,
                version: row.version,
                windowEndExclusive: row.window_end_exclusive,
                windowStart: row.window_start,
            }))
        }
    }
}

/// one validated scheduled failure and pre-serialized response evidence
#[derive(Debug, Clone, PartialEq)]
pub struct FailScheduledIngestion {
    pub attempts: u64,
    pub error: AdmittedIngestionError,
    pub response_metadata_json: Option<SerializedJson>,
    pub run_id: String,
}

/// one ingestion error admitted by the shared domain validator
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedIngestionError(IngestionError);

impl AdmittedIngestionError {
    /// admit one shared-domain validated ingestion error
    pub fn admitted(error: IngestionError) -> Self {
        Self(error)
    }

    /// borrow the validated ingestion error
    pub fn value(&self) -> &IngestionError {
        &self.0
    }
}

/// typed values for the scheduled running-mode guard
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssertRunningScheduledBind {
    BigintText(String),
    ScheduledMode,
}

/// one exact scheduled running-mode guard
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertRunningScheduledQuery {
    pub sql: &'static str,
    pub binds: [AssertRunningScheduledBind; 3],
}

/// typed values for failed-run finalization
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalizeFailedIngestionBind {
    BigintText(String),
    Text(String),
    Count(u64),
    NullableJson(Option<SerializedJson>),
    Classification(IngestionErrorClassification),
}

/// one exact failed-run finalization query
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizeFailedIngestionQuery {
    pub sql: &'static str,
    pub binds: [FinalizeFailedIngestionBind; 7],
}

/// the exact failed-run guarded operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduledRunFailureGuard {
    RunningMode,
    FailedFinalization,
}

impl ScheduledRunFailureGuard {
    /// return the source repository's exact failure label
    pub const fn operation(self) -> &'static str {
        // preserve the owning TypeScript error wording
        match self {
            Self::RunningMode => "running ingestion mode check",
            Self::FailedFinalization => "failed ingestion finalization",
        }
    }
}

/// the typed failed scheduled-run command boundary
pub trait ScheduledRunFailureCommandPort: RetainedSourceSession {
    type Error;

    /// start the failed-run transaction
    fn begin_transaction(&mut self) -> Result<(), Self::Error>;

    /// execute the scheduled running-mode guard
    fn assert_running_scheduled(
        &mut self,
        query: &AssertRunningScheduledQuery,
    ) -> Result<usize, Self::Error>;

    /// guard-finalize the failed running row
    fn finalize_failed_ingestion(
        &mut self,
        query: &FinalizeFailedIngestionQuery,
    ) -> Result<usize, Self::Error>;

    /// commit the failed-run transaction
    fn commit_transaction(&mut self) -> Result<(), Self::Error>;

    /// roll back an incomplete failed-run transaction
    fn rollback_transaction(&mut self) -> Result<(), Self::Error>;
}

/// a scheduled failure transaction error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledRunFailureError<CommandError> {
    CompletionCountOutsideJavaScriptSafeInteger,
    GuardedWrite {
        guard: ScheduledRunFailureGuard,
        row_count: usize,
    },
    Command(CommandError),
}

impl<CommandError: Display> Display for ScheduledRunFailureError<CommandError> {
    /// render exact source-compatible failure text
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve count, guarded-write, and adapter command failures
        match self {
            Self::CompletionCountOutsideJavaScriptSafeInteger => {
                formatter.write_str("completion counts must be non-negative integers")
            }
            Self::GuardedWrite { guard, .. } => {
                write!(
                    formatter,
                    "{} did not affect exactly one row",
                    guard.operation()
                )
            }
            Self::Command(error) => Display::fmt(error, formatter),
        }
    }
}

impl<CommandError> Error for ScheduledRunFailureError<CommandError> where
    CommandError: Error + 'static
{
}

/// atomically finalize one admitted scheduled failure
pub fn fail_scheduled_ingestion<Port: ScheduledRunFailureCommandPort>(
    input: &FailScheduledIngestion,
    port: &mut Port,
) -> Result<(), ScheduledRunFailureError<Port::Error>> {
    // preserve completion-count validation before begin
    if input.attempts > JAVASCRIPT_MAX_SAFE_INTEGER {
        return Err(ScheduledRunFailureError::CompletionCountOutsideJavaScriptSafeInteger);
    }

    let source_id = port.source_id().to_owned();
    port.begin_transaction()
        .map_err(ScheduledRunFailureError::Command)?;
    let operation = execute_failed_scheduled_body(input, &source_id, port);

    // mirror withTransaction rollback for body and commit failures
    match operation {
        Ok(()) => {
            // keep commit inside the transaction try block
            if let Err(error) = port.commit_transaction() {
                return rollback_failed_scheduled(port, ScheduledRunFailureError::Command(error));
            }

            Ok(())
        }
        Err(error) => rollback_failed_scheduled(port, error),
    }
}

/// execute scheduled failure operations between begin and commit
fn execute_failed_scheduled_body<Port: ScheduledRunFailureCommandPort>(
    input: &FailScheduledIngestion,
    source_id: &str,
    port: &mut Port,
) -> Result<(), ScheduledRunFailureError<Port::Error>> {
    let assertion = assert_running_scheduled_query(input, source_id);
    let row_count = port
        .assert_running_scheduled(&assertion)
        .map_err(ScheduledRunFailureError::Command)?;
    require_failed_guard(row_count, ScheduledRunFailureGuard::RunningMode)?;

    let finalization = finalize_failed_ingestion_query(input, source_id);
    let row_count = port
        .finalize_failed_ingestion(&finalization)
        .map_err(ScheduledRunFailureError::Command)?;
    require_failed_guard(row_count, ScheduledRunFailureGuard::FailedFinalization)?;
    Ok(())
}

/// roll back and preserve rollback-error precedence
fn rollback_failed_scheduled<Port: ScheduledRunFailureCommandPort>(
    port: &mut Port,
    failure: ScheduledRunFailureError<Port::Error>,
) -> Result<(), ScheduledRunFailureError<Port::Error>> {
    // a failing rollback masks the original transaction failure
    match port.rollback_transaction() {
        Ok(()) => Err(failure),
        Err(error) => Err(ScheduledRunFailureError::Command(error)),
    }
}

/// require one guarded failed-run row
fn require_failed_guard<CommandError>(
    row_count: usize,
    guard: ScheduledRunFailureGuard,
) -> Result<(), ScheduledRunFailureError<CommandError>> {
    // reject stale or duplicated lifecycle transitions
    if row_count != 1 {
        return Err(ScheduledRunFailureError::GuardedWrite { guard, row_count });
    }

    Ok(())
}

/// build the exact scheduled running-mode guard
pub fn assert_running_scheduled_query(
    input: &FailScheduledIngestion,
    source_id: &str,
) -> AssertRunningScheduledQuery {
    AssertRunningScheduledQuery {
        sql: ASSERT_RUNNING_SCHEDULED_MODE_SQL,
        binds: [
            AssertRunningScheduledBind::BigintText(input.run_id.clone()),
            AssertRunningScheduledBind::BigintText(source_id.to_owned()),
            AssertRunningScheduledBind::ScheduledMode,
        ],
    }
}

/// build the exact failed scheduled-run finalization
pub fn finalize_failed_ingestion_query(
    input: &FailScheduledIngestion,
    source_id: &str,
) -> FinalizeFailedIngestionQuery {
    let error = input.error.value();
    FinalizeFailedIngestionQuery {
        sql: FINALIZE_FAILED_INGESTION_SQL,
        binds: [
            FinalizeFailedIngestionBind::BigintText(input.run_id.clone()),
            FinalizeFailedIngestionBind::BigintText(source_id.to_owned()),
            FinalizeFailedIngestionBind::Count(input.attempts),
            FinalizeFailedIngestionBind::NullableJson(input.response_metadata_json.clone()),
            FinalizeFailedIngestionBind::Classification(error.classification),
            FinalizeFailedIngestionBind::Text(error.code.clone()),
            FinalizeFailedIngestionBind::Text(error.message.clone()),
        ],
    }
}
