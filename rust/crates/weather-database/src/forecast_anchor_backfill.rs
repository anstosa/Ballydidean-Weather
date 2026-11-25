//! Exact SQL plans and transactions for Previous Runs forecast-anchor backfill persistence.

use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    FixedForecastLeadHours, IngestionErrorClassification, NormalizedForecastAnchorRecord, Nullable,
};

use crate::{AdmittedIngestionError, RetainedSourceSession, SerializedJson, Sha256Digest};

const JAVASCRIPT_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const FORECAST_ANCHOR_RECORD_BATCH_SIZE: usize = 2_000;

pub const ABANDON_EXPIRED_FORECAST_ANCHOR_RUNS_SQL: &str = r#"
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

pub const HAS_SUCCESSFUL_FORECAST_ANCHOR_CHUNK_SQL: &str = r#"
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

pub const FIND_RUNNING_FORECAST_ANCHOR_RUN_SQL: &str = r#"
      SELECT id, deadline_at
      FROM ingestion_runs
      WHERE source_id = $1
        AND state = 'running'
      FOR UPDATE
    "#;

pub const INSERT_FORECAST_ANCHOR_BACKFILL_RUN_SQL: &str = r#"
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

pub const ASSERT_FORECAST_ANCHOR_BACKFILL_RUN_SQL: &str = r#"
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

pub const UPSERT_FORECAST_ANCHOR_OUTCOME_SQL: &str = r#"
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

pub const FINALIZE_SUCCESSFUL_FORECAST_ANCHOR_RUN_SQL: &str = r#"
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

pub const FINALIZE_FAILED_FORECAST_ANCHOR_RUN_SQL: &str = r#"
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

/// Exact durable chunk identity after shared time, digest, and version admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmittedForecastAnchorBackfillIdentity {
    pub adapter_version: String,
    pub chunk_plan_version: String,
    pub interval_end_exclusive: String,
    pub interval_start: String,
    pub requested_from_date: String,
    pub requested_to_date: String,
    pub source_config_fingerprint: Sha256Digest,
    pub source_id: String,
}

/// Typed values shared by exact chunk identity plans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForecastAnchorIdentityBind {
    Text(String),
    UtcInstant(String),
    Fingerprint(Sha256Digest),
}

/// Exact successful-resume lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastAnchorSuccessfulChunkQuery {
    pub sql: &'static str,
    pub binds: [ForecastAnchorIdentityBind; 6],
}

/// Exact expired-running-row recovery query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastAnchorAbandonExpiredQuery {
    pub sql: &'static str,
    pub binds: [ForecastAnchorIdentityBind; 2],
}

/// One checked running-row start input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartForecastAnchorBackfillRun {
    pub adapter_version: String,
    pub attempts: u64,
    pub chunk_plan_version: String,
    pub deadline_at: String,
    pub interval_end_exclusive: String,
    pub interval_start: String,
    pub request_metadata_json: Option<SerializedJson>,
    pub source_config_fingerprint: Sha256Digest,
}

/// Typed values for the running-row lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForecastAnchorFindRunningBind {
    Text(String),
}

/// Exact running-row lookup before start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastAnchorFindRunningQuery {
    pub sql: &'static str,
    pub binds: [ForecastAnchorFindRunningBind; 1],
}

/// Typed values for one backfill running-row insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForecastAnchorRunStartBind {
    Text(String),
    BackfillMode,
    UtcInstant(String),
    Fingerprint(Sha256Digest),
    Count(u64),
    NullableJson(Option<SerializedJson>),
}

/// Exact running-row insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastAnchorRunStartQuery {
    pub sql: &'static str,
    pub binds: [ForecastAnchorRunStartBind; 10],
}

/// Driver row returned by the running-row insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartedForecastAnchorBackfillRun {
    pub id: String,
    pub started_at: String,
}

/// Existing running-row lookup result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastAnchorRunningLookup {
    pub first_run_id: Option<String>,
    pub row_count: usize,
}

/// Precomputed JSON and content-hash evidence for one normalized anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastAnchorRecordStorageEvidence {
    pub content_hash: Sha256Digest,
    pub provider_metadata_json: Option<SerializedJson>,
    pub quality_metadata_json: Option<SerializedJson>,
}

/// One normalized record plus admitted storage evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedForecastAnchorRecord {
    pub record: NormalizedForecastAnchorRecord,
    pub storage: ForecastAnchorRecordStorageEvidence,
}

/// Atomic successful completion input.
#[derive(Clone, Debug, PartialEq)]
pub struct CompleteForecastAnchorBackfill {
    pub attempts: u64,
    pub identity: AdmittedForecastAnchorBackfillIdentity,
    pub records: Vec<PreparedForecastAnchorRecord>,
    pub response_metadata_json: Option<SerializedJson>,
    pub run_id: String,
    pub upstream_response_checksum: Option<Sha256Digest>,
}

/// Fixed source kind admitted for this historical forecast product.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForecastAnchorSourceKindBind {
    Forecast,
}

/// Typed values for one bounded anchor-record upsert.
#[derive(Clone, Debug, PartialEq)]
pub enum ForecastAnchorRecordBind {
    Text(String),
    SourceKind(ForecastAnchorSourceKindBind),
    Fingerprint(Sha256Digest),
    UtcInstant(String),
    LeadHours(u64),
    NullableJson(Option<SerializedJson>),
    NullableNumber(Option<f64>),
    ContentHash(Sha256Digest),
}

/// Exact bounded forecast-anchor upsert.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastAnchorRecordUpsertQuery {
    pub sql: String,
    pub binds: Vec<ForecastAnchorRecordBind>,
    pub record_count: usize,
}

/// Typed values for the committed running-row identity guard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastAnchorRunIdentityQuery {
    pub sql: &'static str,
    pub binds: [ForecastAnchorIdentityBind; 7],
}

/// Durable chunk outcome values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForecastAnchorOutcome {
    Succeeded,
    Failed,
}

/// Typed values for a chunk outcome upsert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForecastAnchorOutcomeBind {
    Text(String),
    UtcInstant(String),
    Fingerprint(Sha256Digest),
    Outcome(ForecastAnchorOutcome),
    NullableText(Option<String>),
}

/// Exact successful or failed chunk outcome upsert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastAnchorOutcomeQuery {
    pub sql: &'static str,
    pub binds: [ForecastAnchorOutcomeBind; 11],
}

/// Typed successful finalization values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForecastAnchorSuccessBind {
    Text(String),
    Count(u64),
    NullableJson(Option<SerializedJson>),
    NullableChecksum(Option<Sha256Digest>),
    BackfillMode,
}

/// Exact successful running-row finalization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastAnchorSuccessQuery {
    pub sql: &'static str,
    pub binds: [ForecastAnchorSuccessBind; 7],
}

/// One admitted durable failure.
#[derive(Clone, Debug, PartialEq)]
pub struct FailForecastAnchorBackfill {
    pub attempts: u64,
    pub error: AdmittedIngestionError,
    pub identity: AdmittedForecastAnchorBackfillIdentity,
    pub run_id: String,
}

/// Typed failed finalization values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForecastAnchorFailureBind {
    Text(String),
    Count(u64),
    NullJson,
    Classification(IngestionErrorClassification),
}

/// Exact failed running-row finalization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForecastAnchorFailureQuery {
    pub sql: &'static str,
    pub binds: [ForecastAnchorFailureBind; 7],
}

/// Guarded lifecycle command labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForecastAnchorBackfillGuard {
    BackfillRunIdentity,
    AnchorRecordUpsert,
    SuccessfulFinalization,
    FailedFinalization,
}

impl ForecastAnchorBackfillGuard {
    /// Return exact TypeScript-compatible operation wording.
    pub const fn operation(self) -> &'static str {
        // preserve distinct transaction failure ownership
        match self {
            Self::BackfillRunIdentity => "backfill run identity check",
            Self::AnchorRecordUpsert => "forecast anchor upsert",
            Self::SuccessfulFinalization => "successful ingestion finalization",
            Self::FailedFinalization => "failed ingestion finalization",
        }
    }
}

/// Input rejected before transaction mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForecastAnchorBackfillInputError {
    CountOutsideJavaScriptSafeInteger,
    IdentitySourceMismatch,
    RecordSourceMismatch,
    RecordProvenanceMismatch,
}

impl Display for ForecastAnchorBackfillInputError {
    /// Render the owning repository's failure text.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // retain source-compatible validation categories
        match self {
            Self::CountOutsideJavaScriptSafeInteger => {
                formatter.write_str("completion counts must be non-negative integers")
            }
            Self::IdentitySourceMismatch => {
                formatter.write_str("backfill identity source does not match the locked source")
            }
            Self::RecordSourceMismatch => {
                formatter.write_str("forecast anchor source does not match the locked source")
            }
            Self::RecordProvenanceMismatch => formatter
                .write_str("forecast anchor provenance does not match the backfill identity"),
        }
    }
}

impl Error for ForecastAnchorBackfillInputError {}

/// Complete transaction failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForecastAnchorBackfillExecutionError<CommandError> {
    Input(ForecastAnchorBackfillInputError),
    ExistingRunningIngestion {
        run_id: String,
    },
    MissingInsertedRow,
    GuardedWrite {
        guard: ForecastAnchorBackfillGuard,
        row_count: usize,
    },
    Command(CommandError),
}

impl<CommandError: Display> Display for ForecastAnchorBackfillExecutionError<CommandError> {
    /// Render exact source-compatible transaction failure text.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve input, lifecycle, and command ownership
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
            Self::GuardedWrite {
                guard: ForecastAnchorBackfillGuard::AnchorRecordUpsert,
                ..
            } => formatter.write_str("forecast anchor upsert did not persist every input row"),
            Self::GuardedWrite { guard, .. } => write!(
                formatter,
                "{} did not affect exactly one row",
                guard.operation()
            ),
            Self::Command(error) => Display::fmt(error, formatter),
        }
    }
}

impl<CommandError> Error for ForecastAnchorBackfillExecutionError<CommandError> where
    CommandError: Error + 'static
{
}

/// Complete database boundary for one retained source and owning pool.
pub trait ForecastAnchorBackfillCommandPort: RetainedSourceSession {
    type Error;

    /// Recover expired rows under the retained lock.
    fn abandon_expired_runs(
        &mut self,
        query: &ForecastAnchorAbandonExpiredQuery,
    ) -> Result<Vec<String>, Self::Error>;

    /// Look up an existing running row before start.
    fn find_running_run(
        &mut self,
        query: &ForecastAnchorFindRunningQuery,
    ) -> Result<ForecastAnchorRunningLookup, Self::Error>;

    /// Insert one committed running row.
    fn insert_running_run(
        &mut self,
        query: &ForecastAnchorRunStartQuery,
    ) -> Result<Option<StartedForecastAnchorBackfillRun>, Self::Error>;

    /// Begin one completion or failure transaction.
    fn begin_transaction(&mut self) -> Result<(), Self::Error>;

    /// Execute the exact committed backfill identity guard.
    fn assert_backfill_run(
        &mut self,
        query: &ForecastAnchorRunIdentityQuery,
    ) -> Result<usize, Self::Error>;

    /// Upsert one bounded anchor record batch.
    fn upsert_forecast_anchor_records(
        &mut self,
        query: &ForecastAnchorRecordUpsertQuery,
    ) -> Result<usize, Self::Error>;

    /// Upsert one successful or failed chunk outcome.
    fn upsert_backfill_outcome(
        &mut self,
        query: &ForecastAnchorOutcomeQuery,
    ) -> Result<(), Self::Error>;

    /// Guard-finalize one successful running row.
    fn finalize_successful_run(
        &mut self,
        query: &ForecastAnchorSuccessQuery,
    ) -> Result<usize, Self::Error>;

    /// Guard-finalize one failed running row.
    fn finalize_failed_run(
        &mut self,
        query: &ForecastAnchorFailureQuery,
    ) -> Result<usize, Self::Error>;

    /// Commit the owning transaction.
    fn commit_transaction(&mut self) -> Result<(), Self::Error>;

    /// Roll back an incomplete transaction.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error>;

    /// Release the retained source lock and client.
    fn release(self) -> Result<(), Self::Error>;
}

/// Pool boundary for exact resume and source-lock acquisition.
pub trait ForecastAnchorBackfillPoolPort {
    type Error;
    type Session: ForecastAnchorBackfillCommandPort<Error = Self::Error>;

    /// Execute one exact successful-chunk lookup.
    fn has_successful_chunk(
        &mut self,
        query: &ForecastAnchorSuccessfulChunkQuery,
    ) -> Result<usize, Self::Error>;

    /// Acquire one source-scoped retained session.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, Self::Error>;
}

impl<Pool: ForecastAnchorBackfillPoolPort + ?Sized> ForecastAnchorBackfillPoolPort for &mut Pool {
    type Error = Pool::Error;
    type Session = Pool::Session;

    /// Reuse exact resume lookup through a process-owned pool borrow.
    fn has_successful_chunk(
        &mut self,
        query: &ForecastAnchorSuccessfulChunkQuery,
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
pub fn forecast_anchor_successful_chunk_query(
    identity: &AdmittedForecastAnchorBackfillIdentity,
) -> ForecastAnchorSuccessfulChunkQuery {
    ForecastAnchorSuccessfulChunkQuery {
        sql: HAS_SUCCESSFUL_FORECAST_ANCHOR_CHUNK_SQL,
        binds: [
            ForecastAnchorIdentityBind::Text(identity.source_id.clone()),
            ForecastAnchorIdentityBind::UtcInstant(identity.interval_start.clone()),
            ForecastAnchorIdentityBind::UtcInstant(identity.interval_end_exclusive.clone()),
            ForecastAnchorIdentityBind::Fingerprint(identity.source_config_fingerprint.clone()),
            ForecastAnchorIdentityBind::Text(identity.adapter_version.clone()),
            ForecastAnchorIdentityBind::Text(identity.chunk_plan_version.clone()),
        ],
    }
}

/// Build the exact expired-run recovery query.
pub fn forecast_anchor_abandon_expired_query(
    source_id: &str,
    as_of: &str,
) -> ForecastAnchorAbandonExpiredQuery {
    ForecastAnchorAbandonExpiredQuery {
        sql: ABANDON_EXPIRED_FORECAST_ANCHOR_RUNS_SQL,
        binds: [
            ForecastAnchorIdentityBind::Text(source_id.to_owned()),
            ForecastAnchorIdentityBind::UtcInstant(as_of.to_owned()),
        ],
    }
}

/// Start one exact backfill running row before provider work.
pub fn start_forecast_anchor_backfill_run<Port: ForecastAnchorBackfillCommandPort>(
    input: &StartForecastAnchorBackfillRun,
    port: &mut Port,
) -> Result<StartedForecastAnchorBackfillRun, ForecastAnchorBackfillExecutionError<Port::Error>> {
    let source_id = port.source_id().to_owned();
    let lookup = port
        .find_running_run(&ForecastAnchorFindRunningQuery {
            sql: FIND_RUNNING_FORECAST_ANCHOR_RUN_SQL,
            binds: [ForecastAnchorFindRunningBind::Text(source_id.clone())],
        })
        .map_err(ForecastAnchorBackfillExecutionError::Command)?;

    // reject replacement before explicit recovery
    if lookup.row_count != 0 {
        return Err(
            ForecastAnchorBackfillExecutionError::ExistingRunningIngestion {
                run_id: lookup.first_run_id.unwrap_or_else(|| "unknown".to_owned()),
            },
        );
    }
    let inserted = port
        .insert_running_run(&forecast_anchor_run_start_query(input, &source_id))
        .map_err(ForecastAnchorBackfillExecutionError::Command)?;

    // require the insert RETURNING row
    inserted.ok_or(ForecastAnchorBackfillExecutionError::MissingInsertedRow)
}

/// Build the exact backfill running-row insert.
pub fn forecast_anchor_run_start_query(
    input: &StartForecastAnchorBackfillRun,
    source_id: &str,
) -> ForecastAnchorRunStartQuery {
    ForecastAnchorRunStartQuery {
        sql: INSERT_FORECAST_ANCHOR_BACKFILL_RUN_SQL,
        binds: [
            ForecastAnchorRunStartBind::Text(source_id.to_owned()),
            ForecastAnchorRunStartBind::BackfillMode,
            ForecastAnchorRunStartBind::UtcInstant(input.interval_start.clone()),
            ForecastAnchorRunStartBind::UtcInstant(input.interval_end_exclusive.clone()),
            ForecastAnchorRunStartBind::Fingerprint(input.source_config_fingerprint.clone()),
            ForecastAnchorRunStartBind::Text(input.adapter_version.clone()),
            ForecastAnchorRunStartBind::Text(input.chunk_plan_version.clone()),
            ForecastAnchorRunStartBind::UtcInstant(input.deadline_at.clone()),
            ForecastAnchorRunStartBind::Count(input.attempts),
            ForecastAnchorRunStartBind::NullableJson(input.request_metadata_json.clone()),
        ],
    }
}

/// Execute the atomic successful forecast-anchor completion transaction.
pub fn complete_forecast_anchor_backfill<Port: ForecastAnchorBackfillCommandPort>(
    input: &CompleteForecastAnchorBackfill,
    port: &mut Port,
) -> Result<(), ForecastAnchorBackfillExecutionError<Port::Error>> {
    validate_completion_input(input, port.source_id())?;
    port.begin_transaction()
        .map_err(ForecastAnchorBackfillExecutionError::Command)?;
    let operation = complete_forecast_anchor_backfill_body(input, port);
    finish_transaction(operation, port)
}

// execute successful work between BEGIN and COMMIT
fn complete_forecast_anchor_backfill_body<Port: ForecastAnchorBackfillCommandPort>(
    input: &CompleteForecastAnchorBackfill,
    port: &mut Port,
) -> Result<(), ForecastAnchorBackfillExecutionError<Port::Error>> {
    let source_id = port.source_id().to_owned();
    let row_count = port
        .assert_backfill_run(&forecast_anchor_run_identity_query(
            &input.run_id,
            &source_id,
            &input.identity,
        ))
        .map_err(ForecastAnchorBackfillExecutionError::Command)?;
    require_guard(row_count, ForecastAnchorBackfillGuard::BackfillRunIdentity)?;

    // preserve the source's bounded record batch order
    for records in input.records.chunks(FORECAST_ANCHOR_RECORD_BATCH_SIZE) {
        let query = forecast_anchor_record_upsert_query(&source_id, &input.run_id, records)
            .map_err(ForecastAnchorBackfillExecutionError::Input)?;
        let row_count = port
            .upsert_forecast_anchor_records(&query)
            .map_err(ForecastAnchorBackfillExecutionError::Command)?;
        require_exact_count(
            row_count,
            records.len(),
            ForecastAnchorBackfillGuard::AnchorRecordUpsert,
        )?;
    }
    port.upsert_backfill_outcome(&forecast_anchor_outcome_query(
        &input.run_id,
        &input.identity,
        ForecastAnchorOutcome::Succeeded,
        None,
    ))
    .map_err(ForecastAnchorBackfillExecutionError::Command)?;
    let row_count = port
        .finalize_successful_run(&forecast_anchor_success_query(input, &source_id))
        .map_err(ForecastAnchorBackfillExecutionError::Command)?;
    require_guard(
        row_count,
        ForecastAnchorBackfillGuard::SuccessfulFinalization,
    )
}

/// Execute the atomic failed forecast-anchor finalization transaction.
pub fn fail_forecast_anchor_backfill<Port: ForecastAnchorBackfillCommandPort>(
    input: &FailForecastAnchorBackfill,
    port: &mut Port,
) -> Result<(), ForecastAnchorBackfillExecutionError<Port::Error>> {
    let source_id = port.source_id().to_owned();

    // preserve JavaScript safe-integer failure attempts
    if input.attempts > JAVASCRIPT_MAX_SAFE_INTEGER {
        return Err(ForecastAnchorBackfillExecutionError::Input(
            ForecastAnchorBackfillInputError::CountOutsideJavaScriptSafeInteger,
        ));
    }

    // retain exact source identity before transaction commands
    if input.identity.source_id != source_id {
        return Err(ForecastAnchorBackfillExecutionError::Input(
            ForecastAnchorBackfillInputError::IdentitySourceMismatch,
        ));
    }
    port.begin_transaction()
        .map_err(ForecastAnchorBackfillExecutionError::Command)?;
    let operation = (|| {
        let row_count = port
            .assert_backfill_run(&forecast_anchor_run_identity_query(
                &input.run_id,
                &source_id,
                &input.identity,
            ))
            .map_err(ForecastAnchorBackfillExecutionError::Command)?;
        require_guard(row_count, ForecastAnchorBackfillGuard::BackfillRunIdentity)?;
        port.upsert_backfill_outcome(&forecast_anchor_outcome_query(
            &input.run_id,
            &input.identity,
            ForecastAnchorOutcome::Failed,
            Some(input.error.value().code.clone()),
        ))
        .map_err(ForecastAnchorBackfillExecutionError::Command)?;
        let row_count = port
            .finalize_failed_run(&forecast_anchor_failure_query(input, &source_id))
            .map_err(ForecastAnchorBackfillExecutionError::Command)?;
        require_guard(row_count, ForecastAnchorBackfillGuard::FailedFinalization)
    })();
    finish_transaction(operation, port)
}

// commit success or roll back while retaining rollback-error precedence
fn finish_transaction<Port: ForecastAnchorBackfillCommandPort>(
    operation: Result<(), ForecastAnchorBackfillExecutionError<Port::Error>>,
    port: &mut Port,
) -> Result<(), ForecastAnchorBackfillExecutionError<Port::Error>> {
    // mirror withTransaction commit and rollback behavior
    match operation {
        Ok(()) => {
            // keep COMMIT inside the rollback-protected boundary
            if let Err(error) = port.commit_transaction() {
                return rollback_after_failure(
                    port,
                    ForecastAnchorBackfillExecutionError::Command(error),
                );
            }
            Ok(())
        }
        Err(error) => rollback_after_failure(port, error),
    }
}

// roll back and preserve rollback-error precedence
fn rollback_after_failure<Port: ForecastAnchorBackfillCommandPort>(
    port: &mut Port,
    failure: ForecastAnchorBackfillExecutionError<Port::Error>,
) -> Result<(), ForecastAnchorBackfillExecutionError<Port::Error>> {
    // a failing ROLLBACK masks the original transaction failure
    match port.rollback_transaction() {
        Ok(()) => Err(failure),
        Err(error) => Err(ForecastAnchorBackfillExecutionError::Command(error)),
    }
}

// validate completion identity and provenance before BEGIN
fn validate_completion_input<CommandError>(
    input: &CompleteForecastAnchorBackfill,
    source_id: &str,
) -> Result<(), ForecastAnchorBackfillExecutionError<CommandError>> {
    // bind the durable identity to the retained session
    if input.identity.source_id != source_id {
        return Err(ForecastAnchorBackfillExecutionError::Input(
            ForecastAnchorBackfillInputError::IdentitySourceMismatch,
        ));
    }

    // validate every record before transaction mutation
    for prepared in &input.records {
        // bind record provenance to the exact durable identity
        if prepared.record.sourceConfigFingerprint
            != input.identity.source_config_fingerprint.as_str()
            || prepared.record.adapterVersion != input.identity.adapter_version
        {
            return Err(ForecastAnchorBackfillExecutionError::Input(
                ForecastAnchorBackfillInputError::RecordProvenanceMismatch,
            ));
        }
    }

    // preserve JavaScript safe-integer completion counts after provenance validation
    if input.attempts > JAVASCRIPT_MAX_SAFE_INTEGER
        || u64::try_from(input.records.len()).unwrap_or(u64::MAX) > JAVASCRIPT_MAX_SAFE_INTEGER
    {
        return Err(ForecastAnchorBackfillExecutionError::Input(
            ForecastAnchorBackfillInputError::CountOutsideJavaScriptSafeInteger,
        ));
    }
    Ok(())
}

/// Build the exact committed-run identity guard.
pub fn forecast_anchor_run_identity_query(
    run_id: &str,
    source_id: &str,
    identity: &AdmittedForecastAnchorBackfillIdentity,
) -> ForecastAnchorRunIdentityQuery {
    ForecastAnchorRunIdentityQuery {
        sql: ASSERT_FORECAST_ANCHOR_BACKFILL_RUN_SQL,
        binds: [
            ForecastAnchorIdentityBind::Text(run_id.to_owned()),
            ForecastAnchorIdentityBind::Text(source_id.to_owned()),
            ForecastAnchorIdentityBind::UtcInstant(identity.interval_start.clone()),
            ForecastAnchorIdentityBind::UtcInstant(identity.interval_end_exclusive.clone()),
            ForecastAnchorIdentityBind::Fingerprint(identity.source_config_fingerprint.clone()),
            ForecastAnchorIdentityBind::Text(identity.adapter_version.clone()),
            ForecastAnchorIdentityBind::Text(identity.chunk_plan_version.clone()),
        ],
    }
}

/// Build one exact bounded anchor-record upsert.
pub fn forecast_anchor_record_upsert_query(
    source_id: &str,
    run_id: &str,
    records: &[PreparedForecastAnchorRecord],
) -> Result<ForecastAnchorRecordUpsertQuery, ForecastAnchorBackfillInputError> {
    let mut binds = Vec::with_capacity(records.len() * 24);
    let mut placeholders = Vec::with_capacity(records.len());

    // preserve caller order and exact 24-value bind groups
    for prepared in records {
        // reject cross-source rows at the record construction boundary
        if prepared.record.sourceId != source_id {
            return Err(ForecastAnchorBackfillInputError::RecordSourceMismatch);
        }
        let first_parameter = binds.len() + 1;
        append_record_binds(&mut binds, source_id, run_id, prepared);
        placeholders.push(forecast_anchor_record_placeholder(first_parameter));
    }
    Ok(ForecastAnchorRecordUpsertQuery {
        sql: format!(
            r#"
      INSERT INTO forecast_anchor_records (
        source_id, source_kind, source_config_fingerprint, valid_at, lead_hours,
        dataset, upstream_model, contract_epoch, adapter_version,
        first_ingestion_run_id, last_ingestion_run_id,
        first_received_at, last_received_at, upstream_timezone,
        quality_metadata, provider_metadata, temperature_c, apparent_temperature_c,
        precipitation_mm, wind_speed_mps, wind_gust_mps, pressure_hpa,
        relative_humidity_percent, cloud_cover_percent, wind_direction_degrees, content_hash
      )
      VALUES {}
      ON CONFLICT ON CONSTRAINT forecast_anchor_records_identity_key DO UPDATE SET
        last_ingestion_run_id = EXCLUDED.last_ingestion_run_id,
        last_received_at = EXCLUDED.last_received_at,
        quality_metadata = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.quality_metadata ELSE forecast_anchor_records.quality_metadata END,
        provider_metadata = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.provider_metadata ELSE forecast_anchor_records.provider_metadata END,
        temperature_c = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.temperature_c ELSE forecast_anchor_records.temperature_c END,
        apparent_temperature_c = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.apparent_temperature_c ELSE forecast_anchor_records.apparent_temperature_c END,
        precipitation_mm = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.precipitation_mm ELSE forecast_anchor_records.precipitation_mm END,
        wind_speed_mps = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wind_speed_mps ELSE forecast_anchor_records.wind_speed_mps END,
        wind_gust_mps = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wind_gust_mps ELSE forecast_anchor_records.wind_gust_mps END,
        pressure_hpa = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.pressure_hpa ELSE forecast_anchor_records.pressure_hpa END,
        relative_humidity_percent = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.relative_humidity_percent ELSE forecast_anchor_records.relative_humidity_percent END,
        cloud_cover_percent = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.cloud_cover_percent ELSE forecast_anchor_records.cloud_cover_percent END,
        wind_direction_degrees = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wind_direction_degrees ELSE forecast_anchor_records.wind_direction_degrees END,
        revision_count = forecast_anchor_records.revision_count + CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN 1 ELSE 0 END,
        content_hash = EXCLUDED.content_hash
      WHERE forecast_anchor_records.source_kind = EXCLUDED.source_kind
        AND forecast_anchor_records.source_config_fingerprint = EXCLUDED.source_config_fingerprint
        AND forecast_anchor_records.dataset = EXCLUDED.dataset
        AND forecast_anchor_records.upstream_model = EXCLUDED.upstream_model
        AND forecast_anchor_records.contract_epoch = EXCLUDED.contract_epoch
        AND forecast_anchor_records.adapter_version = EXCLUDED.adapter_version
        AND forecast_anchor_records.upstream_timezone = EXCLUDED.upstream_timezone
    "#,
            placeholders.join(",\n        ")
        ),
        binds,
        record_count: records.len(),
    })
}

// append one record's exact 24 stored values
fn append_record_binds(
    binds: &mut Vec<ForecastAnchorRecordBind>,
    source_id: &str,
    run_id: &str,
    prepared: &PreparedForecastAnchorRecord,
) {
    let record = &prepared.record;
    binds.extend([
        ForecastAnchorRecordBind::Text(source_id.to_owned()),
        ForecastAnchorRecordBind::SourceKind(ForecastAnchorSourceKindBind::Forecast),
        ForecastAnchorRecordBind::Fingerprint(Sha256Digest::admitted(
            record.sourceConfigFingerprint.clone(),
        )),
        ForecastAnchorRecordBind::UtcInstant(record.validAt.clone()),
        ForecastAnchorRecordBind::LeadHours(lead_hours(record.leadHours)),
        ForecastAnchorRecordBind::Text("previous_runs".to_owned()),
        ForecastAnchorRecordBind::Text("best_match".to_owned()),
        ForecastAnchorRecordBind::Text(record.contractEpoch.clone()),
        ForecastAnchorRecordBind::Text(record.adapterVersion.clone()),
        ForecastAnchorRecordBind::Text(run_id.to_owned()),
        ForecastAnchorRecordBind::UtcInstant(record.receivedAt.clone()),
        ForecastAnchorRecordBind::Text(record.metadata.upstreamTimezone.clone()),
        ForecastAnchorRecordBind::NullableJson(prepared.storage.quality_metadata_json.clone()),
        ForecastAnchorRecordBind::NullableJson(prepared.storage.provider_metadata_json.clone()),
        ForecastAnchorRecordBind::NullableNumber(nullable_number(&record.metrics.temperatureC)),
        ForecastAnchorRecordBind::NullableNumber(nullable_number(
            &record.metrics.apparentTemperatureC,
        )),
        ForecastAnchorRecordBind::NullableNumber(nullable_number(&record.metrics.precipitationMm)),
        ForecastAnchorRecordBind::NullableNumber(nullable_number(&record.metrics.windSpeedMps)),
        ForecastAnchorRecordBind::NullableNumber(nullable_number(&record.metrics.windGustMps)),
        ForecastAnchorRecordBind::NullableNumber(nullable_number(&record.metrics.pressureHpa)),
        ForecastAnchorRecordBind::NullableNumber(nullable_number(
            &record.metrics.relativeHumidityPercent,
        )),
        ForecastAnchorRecordBind::NullableNumber(nullable_number(
            &record.metrics.cloudCoverPercent,
        )),
        ForecastAnchorRecordBind::NullableNumber(nullable_number(
            &record.metrics.windDirectionDegrees,
        )),
        ForecastAnchorRecordBind::ContentHash(prepared.storage.content_hash.clone()),
    ]);
}

// create one exact 26-column placeholder tuple from 24 binds
fn forecast_anchor_record_placeholder(first: usize) -> String {
    let mut parameters = (0..24)
        .map(|offset| format!("${}", first + offset))
        .collect::<Vec<_>>();
    parameters[12].push_str("::jsonb");
    parameters[13].push_str("::jsonb");
    parameters.insert(10, parameters[9].clone());
    parameters.insert(12, parameters[11].clone());
    format!("({})", parameters.join(", "))
}

/// Build one exact chunk outcome upsert.
pub fn forecast_anchor_outcome_query(
    run_id: &str,
    identity: &AdmittedForecastAnchorBackfillIdentity,
    outcome: ForecastAnchorOutcome,
    error_code: Option<String>,
) -> ForecastAnchorOutcomeQuery {
    ForecastAnchorOutcomeQuery {
        sql: UPSERT_FORECAST_ANCHOR_OUTCOME_SQL,
        binds: [
            ForecastAnchorOutcomeBind::Text(identity.source_id.clone()),
            ForecastAnchorOutcomeBind::UtcInstant(identity.interval_start.clone()),
            ForecastAnchorOutcomeBind::UtcInstant(identity.interval_end_exclusive.clone()),
            ForecastAnchorOutcomeBind::Fingerprint(identity.source_config_fingerprint.clone()),
            ForecastAnchorOutcomeBind::Text(identity.adapter_version.clone()),
            ForecastAnchorOutcomeBind::Text(identity.chunk_plan_version.clone()),
            ForecastAnchorOutcomeBind::Text(identity.requested_from_date.clone()),
            ForecastAnchorOutcomeBind::Text(identity.requested_to_date.clone()),
            ForecastAnchorOutcomeBind::Text(run_id.to_owned()),
            ForecastAnchorOutcomeBind::Outcome(outcome),
            ForecastAnchorOutcomeBind::NullableText(error_code),
        ],
    }
}

/// Build exact successful finalization evidence.
pub fn forecast_anchor_success_query(
    input: &CompleteForecastAnchorBackfill,
    source_id: &str,
) -> ForecastAnchorSuccessQuery {
    ForecastAnchorSuccessQuery {
        sql: FINALIZE_SUCCESSFUL_FORECAST_ANCHOR_RUN_SQL,
        binds: [
            ForecastAnchorSuccessBind::Text(input.run_id.clone()),
            ForecastAnchorSuccessBind::Text(source_id.to_owned()),
            ForecastAnchorSuccessBind::Count(input.attempts),
            ForecastAnchorSuccessBind::Count(input.records.len() as u64),
            ForecastAnchorSuccessBind::NullableJson(input.response_metadata_json.clone()),
            ForecastAnchorSuccessBind::NullableChecksum(input.upstream_response_checksum.clone()),
            ForecastAnchorSuccessBind::BackfillMode,
        ],
    }
}

/// Build exact failed finalization evidence.
pub fn forecast_anchor_failure_query(
    input: &FailForecastAnchorBackfill,
    source_id: &str,
) -> ForecastAnchorFailureQuery {
    let error = input.error.value();
    ForecastAnchorFailureQuery {
        sql: FINALIZE_FAILED_FORECAST_ANCHOR_RUN_SQL,
        binds: [
            ForecastAnchorFailureBind::Text(input.run_id.clone()),
            ForecastAnchorFailureBind::Text(source_id.to_owned()),
            ForecastAnchorFailureBind::Count(input.attempts),
            ForecastAnchorFailureBind::NullJson,
            ForecastAnchorFailureBind::Classification(error.classification),
            ForecastAnchorFailureBind::Text(error.code.clone()),
            ForecastAnchorFailureBind::Text(error.message.clone()),
        ],
    }
}

// require a one-row lifecycle guard
fn require_guard<CommandError>(
    row_count: usize,
    guard: ForecastAnchorBackfillGuard,
) -> Result<(), ForecastAnchorBackfillExecutionError<CommandError>> {
    require_exact_count(row_count, 1, guard)
}

// require the exact expected affected-row count
fn require_exact_count<CommandError>(
    row_count: usize,
    expected: usize,
    guard: ForecastAnchorBackfillGuard,
) -> Result<(), ForecastAnchorBackfillExecutionError<CommandError>> {
    // fail the transaction on stale lifecycle or immutable-provenance conflicts
    if row_count != expected {
        return Err(ForecastAnchorBackfillExecutionError::GuardedWrite { guard, row_count });
    }
    Ok(())
}

// project generated nullable numbers into database binds
fn nullable_number(value: &Nullable<f64>) -> Option<f64> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(*value),
    }
}

// project the fixed generated lead enum to its database integer
fn lead_hours(value: FixedForecastLeadHours) -> u64 {
    match value {
        FixedForecastLeadHours::Value24 => 24,
        FixedForecastLeadHours::Value48 => 48,
        FixedForecastLeadHours::Value72 => 72,
        FixedForecastLeadHours::Value96 => 96,
        FixedForecastLeadHours::Value120 => 120,
        FixedForecastLeadHours::Value144 => 144,
        FixedForecastLeadHours::Value168 => 168,
    }
}
