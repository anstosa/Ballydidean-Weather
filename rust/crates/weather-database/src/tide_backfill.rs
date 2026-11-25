//! Exact PostgreSQL plans and transactions for retained NOAA tide backfill chunks.

use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::IngestionErrorClassification;

use crate::{
    AdmittedIngestionError, PreparedWeatherRecord, RetainedSourceSession,
    ScheduledIngestionInputError, SerializedJson, Sha256Digest, WeatherRecordUpsertQuery,
    weather_record_upsert_query,
};

const JAVASCRIPT_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const WEATHER_RECORD_BATCH_SIZE: usize = 2_000;

pub const ABANDON_EXPIRED_TIDE_BACKFILL_RUNS_SQL: &str = r#"
      UPDATE ingestion_runs
      SET
        state = 'abandoned',
        completed_at = $2,
        error_classification = 'retryable',
        error_code = 'deadline_expired',
        error_message = 'ingestion run exceeded its recovery deadline'
      WHERE source_id = $1
        AND state = 'running'
        AND deadline_at < $2
      RETURNING id
    "#;

pub const HAS_SUCCESSFUL_TIDE_BACKFILL_CHUNK_SQL: &str = r#"
      SELECT 1
      FROM backfill_chunk_outcomes
      WHERE source_id = $1
        AND interval_start = $2
        AND interval_end_exclusive = $3
        AND source_config_fingerprint = $4
        AND adapter_version = $5
        AND chunk_plan_version = $6
        AND outcome = 'succeeded'
    "#;

pub const FIND_RUNNING_TIDE_BACKFILL_RUN_SQL: &str = r#"
      SELECT id, deadline_at
      FROM ingestion_runs
      WHERE source_id = $1
        AND state = 'running'
      FOR UPDATE
    "#;

pub const INSERT_TIDE_BACKFILL_RUN_SQL: &str = r#"
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

pub const ASSERT_TIDE_BACKFILL_RUN_SQL: &str = r#"
      SELECT 1
      FROM ingestion_runs
      WHERE id = $1
        AND source_id = $2
        AND state = 'running'
        AND mode = 'backfill'
        AND requested_start = $3
        AND requested_end_exclusive = $4
        AND source_config_fingerprint = $5
        AND adapter_version = $6
        AND chunk_plan_version = $7
    "#;

pub const UPSERT_TIDE_BACKFILL_OUTCOME_SQL: &str = r#"
      INSERT INTO backfill_chunk_outcomes (
        source_id,
        interval_start,
        interval_end_exclusive,
        source_config_fingerprint,
        adapter_version,
        chunk_plan_version,
        requested_from_date,
        requested_to_date,
        ingestion_run_id,
        outcome,
        error_code
      )
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
      ON CONFLICT (
        source_id,
        interval_start,
        interval_end_exclusive,
        source_config_fingerprint,
        adapter_version,
        chunk_plan_version
      ) DO UPDATE SET
        ingestion_run_id = EXCLUDED.ingestion_run_id,
        requested_from_date = EXCLUDED.requested_from_date,
        requested_to_date = EXCLUDED.requested_to_date,
        outcome = EXCLUDED.outcome,
        error_code = EXCLUDED.error_code,
        completed_at = clock_timestamp()
      WHERE NOT (
        backfill_chunk_outcomes.outcome = 'succeeded'
        AND EXCLUDED.outcome = 'failed'
      )
    "#;

pub const FINALIZE_SUCCESSFUL_TIDE_BACKFILL_RUN_SQL: &str = r#"
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

pub const FINALIZE_FAILED_TIDE_BACKFILL_RUN_SQL: &str = r#"
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

/// Exact durable chunk identity after shared time and digest admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmittedTideBackfillIdentity {
    pub adapter_version: String,
    pub chunk_plan_version: String,
    pub interval_end_exclusive: String,
    pub interval_start: String,
    pub requested_from_date: String,
    pub requested_to_date: String,
    pub source_config_fingerprint: Sha256Digest,
    pub source_id: String,
}

/// Typed identity bind values in PostgreSQL placeholder order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TideBackfillIdentityBind {
    Text(String),
    UtcInstant(String),
    Fingerprint(Sha256Digest),
}

/// Exact successful-chunk resume query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TideBackfillSuccessfulChunkQuery {
    pub sql: &'static str,
    pub binds: [TideBackfillIdentityBind; 6],
}

/// Exact expired-row recovery query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TideBackfillAbandonExpiredQuery {
    pub sql: &'static str,
    pub binds: [TideBackfillIdentityBind; 2],
}

/// One admitted running-row input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartTideBackfillRun {
    pub adapter_version: String,
    pub attempts: u64,
    pub chunk_plan_version: String,
    pub deadline_at: String,
    pub interval_end_exclusive: String,
    pub interval_start: String,
    pub request_metadata_json: Option<SerializedJson>,
    pub source_config_fingerprint: Sha256Digest,
}

/// Running-row lookup bind values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TideBackfillFindRunningBind {
    Text(String),
}

/// Exact running-row lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TideBackfillFindRunningQuery {
    pub sql: &'static str,
    pub binds: [TideBackfillFindRunningBind; 1],
}

/// Running-row insert bind values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TideBackfillRunStartBind {
    Text(String),
    BackfillMode,
    UtcInstant(String),
    Fingerprint(Sha256Digest),
    Count(u64),
    NullableJson(Option<SerializedJson>),
}

/// Exact running-row insert query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TideBackfillRunStartQuery {
    pub sql: &'static str,
    pub binds: [TideBackfillRunStartBind; 10],
}

/// Existing running-row result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TideBackfillRunningLookup {
    pub first_run_id: Option<String>,
    pub row_count: usize,
}

/// Inserted running-row result used by the one-shot worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartedTideBackfillRun {
    pub id: String,
    pub started_at: String,
}

/// Atomic successful tide completion input.
#[derive(Clone, Debug, PartialEq)]
pub struct CompleteTideBackfill {
    pub attempts: u64,
    pub identity: AdmittedTideBackfillIdentity,
    pub records: Vec<PreparedWeatherRecord>,
    pub response_metadata_json: Option<SerializedJson>,
    pub run_id: String,
    pub upstream_response_checksum: Option<Sha256Digest>,
}

/// Exact committed-run identity guard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TideBackfillRunIdentityQuery {
    pub sql: &'static str,
    pub binds: [TideBackfillIdentityBind; 7],
}

/// Durable chunk outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TideBackfillOutcome {
    Succeeded,
    Failed,
}

/// Chunk outcome bind values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TideBackfillOutcomeBind {
    Text(String),
    UtcInstant(String),
    Fingerprint(Sha256Digest),
    Outcome(TideBackfillOutcome),
    NullableText(Option<String>),
}

/// Exact chunk outcome upsert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TideBackfillOutcomeQuery {
    pub sql: &'static str,
    pub binds: [TideBackfillOutcomeBind; 11],
}

/// Successful finalization bind values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TideBackfillSuccessBind {
    Text(String),
    Count(u64),
    NullableJson(Option<SerializedJson>),
    NullableChecksum(Option<Sha256Digest>),
    BackfillMode,
}

/// Exact successful finalization query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TideBackfillSuccessQuery {
    pub sql: &'static str,
    pub binds: [TideBackfillSuccessBind; 7],
}

/// One admitted durable failed chunk.
#[derive(Clone, Debug, PartialEq)]
pub struct FailTideBackfill {
    pub attempts: u64,
    pub error: AdmittedIngestionError,
    pub identity: AdmittedTideBackfillIdentity,
    pub run_id: String,
}

/// Failed finalization bind values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TideBackfillFailureBind {
    Text(String),
    Count(u64),
    NullJson,
    Classification(IngestionErrorClassification),
}

/// Exact failed finalization query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TideBackfillFailureQuery {
    pub sql: &'static str,
    pub binds: [TideBackfillFailureBind; 7],
}

/// Guard labels retained from the Node repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TideBackfillGuard {
    BackfillRunIdentity,
    SuccessfulFinalization,
    FailedFinalization,
}

impl TideBackfillGuard {
    /// Return exact repository operation wording.
    pub const fn operation(self) -> &'static str {
        // preserve distinct lifecycle failure owners
        match self {
            Self::BackfillRunIdentity => "backfill run identity check",
            Self::SuccessfulFinalization => "successful ingestion finalization",
            Self::FailedFinalization => "failed ingestion finalization",
        }
    }
}

/// Input rejected before or during transaction planning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TideBackfillInputError {
    CountOutsideJavaScriptSafeInteger,
    IdentitySourceMismatch,
    Record(ScheduledIngestionInputError),
}

impl Display for TideBackfillInputError {
    /// Render exact source-compatible input failure text.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // retain the original owning validation category
        match self {
            Self::CountOutsideJavaScriptSafeInteger => {
                formatter.write_str("completion counts must be non-negative integers")
            }
            Self::IdentitySourceMismatch => {
                formatter.write_str("backfill identity source does not match the locked source")
            }
            Self::Record(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for TideBackfillInputError {}

/// Tide backfill command or lifecycle failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TideBackfillExecutionError<CommandError> {
    Input(TideBackfillInputError),
    ExistingRunningIngestion {
        run_id: String,
    },
    MissingInsertedRow,
    GuardedWrite {
        guard: TideBackfillGuard,
        row_count: usize,
    },
    Command(CommandError),
}

impl<CommandError: Display> Display for TideBackfillExecutionError<CommandError> {
    /// Render exact source-compatible transaction failure text.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve input, lifecycle, and driver error ownership
        match self {
            Self::Input(error) => Display::fmt(error, formatter),
            Self::ExistingRunningIngestion { run_id } => {
                write!(
                    formatter,
                    "source already has a running ingestion: {run_id}"
                )
            }
            Self::MissingInsertedRow => {
                formatter.write_str("running ingestion insert returned no row")
            }
            Self::GuardedWrite { guard, .. } => write!(
                formatter,
                "{} did not affect exactly one row",
                guard.operation()
            ),
            Self::Command(error) => Display::fmt(error, formatter),
        }
    }
}

impl<CommandError> Error for TideBackfillExecutionError<CommandError> where
    CommandError: Error + 'static
{
}

/// Source-retained commands required by the tide backfill repository.
pub trait TideBackfillCommandPort: RetainedSourceSession {
    type Error;

    /// Recover expired work under the retained lock.
    fn abandon_expired_runs(
        &mut self,
        query: &TideBackfillAbandonExpiredQuery,
    ) -> Result<Vec<String>, Self::Error>;

    /// Look up a current running row before start.
    fn find_running_run(
        &mut self,
        query: &TideBackfillFindRunningQuery,
    ) -> Result<TideBackfillRunningLookup, Self::Error>;

    /// Insert one committed running row.
    fn insert_running_run(
        &mut self,
        query: &TideBackfillRunStartQuery,
    ) -> Result<Option<StartedTideBackfillRun>, Self::Error>;

    /// Begin one completion or failure transaction.
    fn begin_transaction(&mut self) -> Result<(), Self::Error>;

    /// Execute the exact committed identity guard.
    fn assert_backfill_run(
        &mut self,
        query: &TideBackfillRunIdentityQuery,
    ) -> Result<usize, Self::Error>;

    /// Upsert one bounded normalized weather-record batch.
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error>;

    /// Upsert one successful or failed chunk outcome.
    fn upsert_backfill_outcome(
        &mut self,
        query: &TideBackfillOutcomeQuery,
    ) -> Result<(), Self::Error>;

    /// Guard-finalize one successful run.
    fn finalize_successful_run(
        &mut self,
        query: &TideBackfillSuccessQuery,
    ) -> Result<usize, Self::Error>;

    /// Guard-finalize one failed run.
    fn finalize_failed_run(
        &mut self,
        query: &TideBackfillFailureQuery,
    ) -> Result<usize, Self::Error>;

    /// Commit the complete transaction.
    fn commit_transaction(&mut self) -> Result<(), Self::Error>;

    /// Roll back an incomplete transaction.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error>;

    /// Release the retained source lock and client.
    fn release(self) -> Result<(), Self::Error>;
}

/// Pool commands required before and during source-lock acquisition.
pub trait TideBackfillPoolPort {
    type Error;
    type Session: TideBackfillCommandPort<Error = Self::Error>;

    /// Execute the exact successful-chunk lookup.
    fn has_successful_chunk(
        &mut self,
        query: &TideBackfillSuccessfulChunkQuery,
    ) -> Result<usize, Self::Error>;

    /// Acquire one source-scoped retained session.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, Self::Error>;
}

impl<Pool: TideBackfillPoolPort + ?Sized> TideBackfillPoolPort for &mut Pool {
    type Error = Pool::Error;
    type Session = Pool::Session;

    /// Reuse exact resume lookup through a process-owned pool borrow.
    fn has_successful_chunk(
        &mut self,
        query: &TideBackfillSuccessfulChunkQuery,
    ) -> Result<usize, Self::Error> {
        (**self).has_successful_chunk(query)
    }

    /// Acquire the retained session without transferring or opening another pool.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, Self::Error> {
        (**self).acquire_source_session(source_id)
    }
}

/// Build the exact successful resume lookup.
pub fn tide_backfill_successful_chunk_query(
    identity: &AdmittedTideBackfillIdentity,
) -> TideBackfillSuccessfulChunkQuery {
    TideBackfillSuccessfulChunkQuery {
        sql: HAS_SUCCESSFUL_TIDE_BACKFILL_CHUNK_SQL,
        binds: [
            TideBackfillIdentityBind::Text(identity.source_id.clone()),
            TideBackfillIdentityBind::UtcInstant(identity.interval_start.clone()),
            TideBackfillIdentityBind::UtcInstant(identity.interval_end_exclusive.clone()),
            TideBackfillIdentityBind::Fingerprint(identity.source_config_fingerprint.clone()),
            TideBackfillIdentityBind::Text(identity.adapter_version.clone()),
            TideBackfillIdentityBind::Text(identity.chunk_plan_version.clone()),
        ],
    }
}

/// Build the exact expired-run recovery query.
pub fn tide_backfill_abandon_expired_query(
    source_id: &str,
    as_of: &str,
) -> TideBackfillAbandonExpiredQuery {
    TideBackfillAbandonExpiredQuery {
        sql: ABANDON_EXPIRED_TIDE_BACKFILL_RUNS_SQL,
        binds: [
            TideBackfillIdentityBind::Text(source_id.to_owned()),
            TideBackfillIdentityBind::UtcInstant(as_of.to_owned()),
        ],
    }
}

/// Start one exact running backfill row.
pub fn start_tide_backfill_run<Port: TideBackfillCommandPort>(
    input: &StartTideBackfillRun,
    port: &mut Port,
) -> Result<StartedTideBackfillRun, TideBackfillExecutionError<Port::Error>> {
    let source_id = port.source_id().to_owned();
    let lookup = port
        .find_running_run(&TideBackfillFindRunningQuery {
            sql: FIND_RUNNING_TIDE_BACKFILL_RUN_SQL,
            binds: [TideBackfillFindRunningBind::Text(source_id.clone())],
        })
        .map_err(TideBackfillExecutionError::Command)?;

    // reject replacement before explicit recovery
    if lookup.row_count != 0 {
        return Err(TideBackfillExecutionError::ExistingRunningIngestion {
            run_id: lookup.first_run_id.unwrap_or_else(|| "unknown".to_owned()),
        });
    }
    port.insert_running_run(&tide_backfill_run_start_query(input, &source_id))
        .map_err(TideBackfillExecutionError::Command)?
        .ok_or(TideBackfillExecutionError::MissingInsertedRow)
}

/// Build the exact running-row insert.
pub fn tide_backfill_run_start_query(
    input: &StartTideBackfillRun,
    source_id: &str,
) -> TideBackfillRunStartQuery {
    TideBackfillRunStartQuery {
        sql: INSERT_TIDE_BACKFILL_RUN_SQL,
        binds: [
            TideBackfillRunStartBind::Text(source_id.to_owned()),
            TideBackfillRunStartBind::BackfillMode,
            TideBackfillRunStartBind::UtcInstant(input.interval_start.clone()),
            TideBackfillRunStartBind::UtcInstant(input.interval_end_exclusive.clone()),
            TideBackfillRunStartBind::Fingerprint(input.source_config_fingerprint.clone()),
            TideBackfillRunStartBind::Text(input.adapter_version.clone()),
            TideBackfillRunStartBind::Text(input.chunk_plan_version.clone()),
            TideBackfillRunStartBind::UtcInstant(input.deadline_at.clone()),
            TideBackfillRunStartBind::Count(input.attempts),
            TideBackfillRunStartBind::NullableJson(input.request_metadata_json.clone()),
        ],
    }
}

/// Execute one atomic successful tide backfill transaction.
pub fn complete_tide_backfill<Port: TideBackfillCommandPort>(
    input: &CompleteTideBackfill,
    port: &mut Port,
) -> Result<(), TideBackfillExecutionError<Port::Error>> {
    validate_completion(input, port.source_id())?;
    port.begin_transaction()
        .map_err(TideBackfillExecutionError::Command)?;
    let result = complete_tide_backfill_body(input, port);
    finish_transaction(result, port)
}

// execute successful commands between BEGIN and COMMIT
fn complete_tide_backfill_body<Port: TideBackfillCommandPort>(
    input: &CompleteTideBackfill,
    port: &mut Port,
) -> Result<(), TideBackfillExecutionError<Port::Error>> {
    let source_id = port.source_id().to_owned();
    let row_count = port
        .assert_backfill_run(&tide_backfill_run_identity_query(
            &input.run_id,
            &source_id,
            &input.identity,
        ))
        .map_err(TideBackfillExecutionError::Command)?;
    require_guard(row_count, TideBackfillGuard::BackfillRunIdentity)?;

    // preserve normalized provider order through bounded shared upserts
    for records in input.records.chunks(WEATHER_RECORD_BATCH_SIZE) {
        let query =
            weather_record_upsert_query(&source_id, &input.run_id, records).map_err(|error| {
                TideBackfillExecutionError::Input(TideBackfillInputError::Record(error))
            })?;
        port.upsert_weather_records(&query)
            .map_err(TideBackfillExecutionError::Command)?;
    }
    port.upsert_backfill_outcome(&tide_backfill_outcome_query(
        &input.run_id,
        &input.identity,
        TideBackfillOutcome::Succeeded,
        None,
    ))
    .map_err(TideBackfillExecutionError::Command)?;
    let row_count = port
        .finalize_successful_run(&tide_backfill_success_query(input, &source_id))
        .map_err(TideBackfillExecutionError::Command)?;
    require_guard(row_count, TideBackfillGuard::SuccessfulFinalization)
}

/// Execute one atomic failed tide backfill finalization.
pub fn fail_tide_backfill<Port: TideBackfillCommandPort>(
    input: &FailTideBackfill,
    port: &mut Port,
) -> Result<(), TideBackfillExecutionError<Port::Error>> {
    let source_id = port.source_id().to_owned();

    // match safe-integer validation before transaction mutation
    if input.attempts > JAVASCRIPT_MAX_SAFE_INTEGER {
        return Err(TideBackfillExecutionError::Input(
            TideBackfillInputError::CountOutsideJavaScriptSafeInteger,
        ));
    }
    port.begin_transaction()
        .map_err(TideBackfillExecutionError::Command)?;
    let result = (|| {
        // bind the chunk identity inside the Node transaction boundary
        if input.identity.source_id != source_id {
            return Err(TideBackfillExecutionError::Input(
                TideBackfillInputError::IdentitySourceMismatch,
            ));
        }
        let row_count = port
            .assert_backfill_run(&tide_backfill_run_identity_query(
                &input.run_id,
                &source_id,
                &input.identity,
            ))
            .map_err(TideBackfillExecutionError::Command)?;
        require_guard(row_count, TideBackfillGuard::BackfillRunIdentity)?;
        port.upsert_backfill_outcome(&tide_backfill_outcome_query(
            &input.run_id,
            &input.identity,
            TideBackfillOutcome::Failed,
            Some(input.error.value().code.clone()),
        ))
        .map_err(TideBackfillExecutionError::Command)?;
        let row_count = port
            .finalize_failed_run(&tide_backfill_failure_query(input, &source_id))
            .map_err(TideBackfillExecutionError::Command)?;
        require_guard(row_count, TideBackfillGuard::FailedFinalization)
    })();
    finish_transaction(result, port)
}

// validate source identity and completion counts before BEGIN
fn validate_completion<CommandError>(
    input: &CompleteTideBackfill,
    source_id: &str,
) -> Result<(), TideBackfillExecutionError<CommandError>> {
    // require the exact retained source identity first
    if input.identity.source_id != source_id {
        return Err(TideBackfillExecutionError::Input(
            TideBackfillInputError::IdentitySourceMismatch,
        ));
    }
    // preserve JavaScript safe-integer counts
    if input.attempts > JAVASCRIPT_MAX_SAFE_INTEGER
        || u64::try_from(input.records.len()).unwrap_or(u64::MAX) > JAVASCRIPT_MAX_SAFE_INTEGER
    {
        return Err(TideBackfillExecutionError::Input(
            TideBackfillInputError::CountOutsideJavaScriptSafeInteger,
        ));
    }
    Ok(())
}

/// Build the exact committed-run identity guard.
pub fn tide_backfill_run_identity_query(
    run_id: &str,
    source_id: &str,
    identity: &AdmittedTideBackfillIdentity,
) -> TideBackfillRunIdentityQuery {
    TideBackfillRunIdentityQuery {
        sql: ASSERT_TIDE_BACKFILL_RUN_SQL,
        binds: [
            TideBackfillIdentityBind::Text(run_id.to_owned()),
            TideBackfillIdentityBind::Text(source_id.to_owned()),
            TideBackfillIdentityBind::UtcInstant(identity.interval_start.clone()),
            TideBackfillIdentityBind::UtcInstant(identity.interval_end_exclusive.clone()),
            TideBackfillIdentityBind::Fingerprint(identity.source_config_fingerprint.clone()),
            TideBackfillIdentityBind::Text(identity.adapter_version.clone()),
            TideBackfillIdentityBind::Text(identity.chunk_plan_version.clone()),
        ],
    }
}

/// Build the exact chunk outcome upsert.
pub fn tide_backfill_outcome_query(
    run_id: &str,
    identity: &AdmittedTideBackfillIdentity,
    outcome: TideBackfillOutcome,
    error_code: Option<String>,
) -> TideBackfillOutcomeQuery {
    TideBackfillOutcomeQuery {
        sql: UPSERT_TIDE_BACKFILL_OUTCOME_SQL,
        binds: [
            TideBackfillOutcomeBind::Text(identity.source_id.clone()),
            TideBackfillOutcomeBind::UtcInstant(identity.interval_start.clone()),
            TideBackfillOutcomeBind::UtcInstant(identity.interval_end_exclusive.clone()),
            TideBackfillOutcomeBind::Fingerprint(identity.source_config_fingerprint.clone()),
            TideBackfillOutcomeBind::Text(identity.adapter_version.clone()),
            TideBackfillOutcomeBind::Text(identity.chunk_plan_version.clone()),
            TideBackfillOutcomeBind::Text(identity.requested_from_date.clone()),
            TideBackfillOutcomeBind::Text(identity.requested_to_date.clone()),
            TideBackfillOutcomeBind::Text(run_id.to_owned()),
            TideBackfillOutcomeBind::Outcome(outcome),
            TideBackfillOutcomeBind::NullableText(error_code),
        ],
    }
}

/// Build exact successful finalization evidence.
pub fn tide_backfill_success_query(
    input: &CompleteTideBackfill,
    source_id: &str,
) -> TideBackfillSuccessQuery {
    TideBackfillSuccessQuery {
        sql: FINALIZE_SUCCESSFUL_TIDE_BACKFILL_RUN_SQL,
        binds: [
            TideBackfillSuccessBind::Text(input.run_id.clone()),
            TideBackfillSuccessBind::Text(source_id.to_owned()),
            TideBackfillSuccessBind::Count(input.attempts),
            TideBackfillSuccessBind::Count(input.records.len() as u64),
            TideBackfillSuccessBind::NullableJson(input.response_metadata_json.clone()),
            TideBackfillSuccessBind::NullableChecksum(input.upstream_response_checksum.clone()),
            TideBackfillSuccessBind::BackfillMode,
        ],
    }
}

/// Build exact failed finalization evidence.
pub fn tide_backfill_failure_query(
    input: &FailTideBackfill,
    source_id: &str,
) -> TideBackfillFailureQuery {
    let error = input.error.value();
    TideBackfillFailureQuery {
        sql: FINALIZE_FAILED_TIDE_BACKFILL_RUN_SQL,
        binds: [
            TideBackfillFailureBind::Text(input.run_id.clone()),
            TideBackfillFailureBind::Text(source_id.to_owned()),
            TideBackfillFailureBind::Count(input.attempts),
            TideBackfillFailureBind::NullJson,
            TideBackfillFailureBind::Classification(error.classification),
            TideBackfillFailureBind::Text(error.code.clone()),
            TideBackfillFailureBind::Text(error.message.clone()),
        ],
    }
}

// finish a transaction with Node-compatible rollback precedence
fn finish_transaction<Port: TideBackfillCommandPort>(
    result: Result<(), TideBackfillExecutionError<Port::Error>>,
    port: &mut Port,
) -> Result<(), TideBackfillExecutionError<Port::Error>> {
    // keep COMMIT inside the rollback-protected boundary
    match result {
        Ok(()) => {
            // roll back when the commit command itself fails
            if let Err(error) = port.commit_transaction() {
                return rollback_after_failure(port, TideBackfillExecutionError::Command(error));
            }
            Ok(())
        }
        Err(error) => rollback_after_failure(port, error),
    }
}

// roll back while preserving rollback-error precedence
fn rollback_after_failure<Port: TideBackfillCommandPort>(
    port: &mut Port,
    failure: TideBackfillExecutionError<Port::Error>,
) -> Result<(), TideBackfillExecutionError<Port::Error>> {
    // a failed ROLLBACK masks the original transaction error
    match port.rollback_transaction() {
        Ok(()) => Err(failure),
        Err(error) => Err(TideBackfillExecutionError::Command(error)),
    }
}

// require one exact lifecycle row
fn require_guard<CommandError>(
    row_count: usize,
    guard: TideBackfillGuard,
) -> Result<(), TideBackfillExecutionError<CommandError>> {
    // reject stale or duplicated lifecycle transitions
    if row_count != 1 {
        return Err(TideBackfillExecutionError::GuardedWrite { guard, row_count });
    }
    Ok(())
}
