//! Database binding for the existing typed NOAA tide one-shot backfill executor.

use std::fmt::Display;

use weather_database::{
    AdmittedIngestionError, AdmittedTideBackfillIdentity,
    CompleteTideBackfill as DatabaseTideBackfillCompletion,
    FailTideBackfill as DatabaseTideBackfillFailure, RetainedSourceSession, SerializedJson,
    StartTideBackfillRun, TideBackfillCommandPort, TideBackfillPoolPort, complete_tide_backfill,
    fail_tide_backfill, start_tide_backfill_run, tide_backfill_abandon_expired_query,
    tide_backfill_successful_chunk_query,
};
use weather_worker::{
    TaskFailure, TideBackfillChunkIdentity, TideBackfillCompletion, TideBackfillOperationError,
    TideBackfillProviderFailure, TideBackfillRepository, TideBackfillRunStart,
};

use crate::{
    NoaaTideRuntimeValues, RuntimeTimeValue, noaa_tide_response_metadata, project_noaa_tide_record,
};

/// One tide backfill persistence boundary whose failure retains worker ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TideBackfillPersistenceOperation {
    ResumeRead,
    Acquire,
    Abandon,
    Start,
    Complete,
    FailureFinalize,
    Release,
}

/// Canonical request, failure, and shared NOAA value owners for one-shot persistence.
pub trait TideBackfillRuntimeValues: NoaaTideRuntimeValues {
    /// Serialize the exact endpoint and station request metadata.
    fn tide_backfill_request_metadata_json(
        &mut self,
        endpoint: &str,
        station_id: &str,
    ) -> Result<Option<SerializedJson>, TaskFailure>;

    /// Admit one exact classified tide provider failure.
    fn tide_backfill_ingestion_error(
        &mut self,
        failure: &TideBackfillProviderFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure>;

    /// Classify one database failure at the task-level boundary.
    fn tide_backfill_task_failure(
        &mut self,
        operation: TideBackfillPersistenceOperation,
        message: &str,
    ) -> TaskFailure;

    /// Classify one database failure inside the committed chunk catch boundary.
    fn tide_backfill_operation_failure(
        &mut self,
        operation: TideBackfillPersistenceOperation,
        message: &str,
    ) -> TideBackfillOperationError;
}

/// Existing tide worker repository backed by exact PostgreSQL plans.
pub struct DatabaseTideBackfillRepository<Database, Values> {
    database: Database,
    values: Values,
}

impl<Database, Values> DatabaseTideBackfillRepository<Database, Values> {
    /// Bind database and canonical value owners without performing I/O.
    pub const fn new(database: Database, values: Values) -> Self {
        Self { database, values }
    }

    /// Recover both bound owners after the one-shot lifecycle.
    pub fn into_parts(self) -> (Database, Values) {
        (self.database, self.values)
    }
}

impl<Database, Values> TideBackfillRepository for DatabaseTideBackfillRepository<Database, Values>
where
    Database: TideBackfillPoolPort,
    Database::Error: Display,
    Values: TideBackfillRuntimeValues,
{
    type Session = Database::Session;

    /// Execute the exact six-bind successful chunk lookup before locking.
    fn has_successful_chunk(
        &mut self,
        identity: &TideBackfillChunkIdentity,
    ) -> Result<bool, TaskFailure> {
        let identity = admit_identity(identity, &mut self.values)?;
        self.database
            .has_successful_chunk(&tide_backfill_successful_chunk_query(&identity))
            .map(|row_count| row_count == 1)
            .map_err(|error| {
                self.values.tide_backfill_task_failure(
                    TideBackfillPersistenceOperation::ResumeRead,
                    &error.to_string(),
                )
            })
    }

    /// Acquire one retained source session and reject mismatched authority.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure> {
        let session = self
            .database
            .acquire_source_session(source_id)
            .map_err(|error| {
                self.values.tide_backfill_task_failure(
                    TideBackfillPersistenceOperation::Acquire,
                    &error.to_string(),
                )
            })?;

        // release a session that does not retain the requested source
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

    /// Recover expired rows through the exact retained-session query.
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        now_ms: i64,
    ) -> Result<(), TideBackfillOperationError> {
        let as_of = self
            .values
            .utc_instant(now_ms, RuntimeTimeValue::AbandonAsOf)
            .map_err(operation_from_task_failure)?;
        session
            .abandon_expired_runs(&tide_backfill_abandon_expired_query(
                session.source_id(),
                &as_of,
            ))
            .map(|_| ())
            .map_err(|error| {
                self.values.tide_backfill_operation_failure(
                    TideBackfillPersistenceOperation::Abandon,
                    &error.to_string(),
                )
            })
    }

    /// Commit one exact running backfill row before provider I/O.
    fn start_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        start: TideBackfillRunStart,
    ) -> Result<String, TideBackfillOperationError> {
        let input = StartTideBackfillRun {
            adapter_version: start.adapter_version.to_owned(),
            attempts: 0,
            chunk_plan_version: start.chunk_plan_version.to_owned(),
            deadline_at: self
                .values
                .utc_instant(start.deadline_at_ms, RuntimeTimeValue::Deadline)
                .map_err(operation_from_task_failure)?,
            interval_end_exclusive: self
                .values
                .utc_instant(
                    start.requested_end_exclusive_ms,
                    RuntimeTimeValue::RequestedEndExclusive,
                )
                .map_err(operation_from_task_failure)?,
            interval_start: self
                .values
                .utc_instant(start.requested_start_ms, RuntimeTimeValue::RequestedStart)
                .map_err(operation_from_task_failure)?,
            request_metadata_json: self
                .values
                .tide_backfill_request_metadata_json(start.endpoint, &start.station_id)
                .map_err(operation_from_task_failure)?,
            source_config_fingerprint: self
                .values
                .sha256_digest(&start.source_config_fingerprint)
                .map_err(operation_from_task_failure)?,
        };
        start_tide_backfill_run(&input, session)
            .map(|run| run.id)
            .map_err(|error| {
                self.values.tide_backfill_operation_failure(
                    TideBackfillPersistenceOperation::Start,
                    &error.to_string(),
                )
            })
    }

    /// Persist actual normalized NOAA records and response evidence atomically.
    fn complete_backfill_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: TideBackfillCompletion,
    ) -> Result<(), TideBackfillOperationError> {
        let identity = admit_identity(&completion.identity, &mut self.values)
            .map_err(operation_from_task_failure)?;
        let mut records = Vec::with_capacity(completion.records.len());

        // preserve provider order through the shared normalized weather projection
        for record in completion.records {
            records.push(
                project_noaa_tide_record(record, &mut self.values)
                    .map_err(operation_from_task_failure)?,
            );
        }
        let response_metadata = noaa_tide_response_metadata(&completion.response_metadata);
        let input = DatabaseTideBackfillCompletion {
            attempts: u64::from(completion.attempts),
            identity,
            records,
            response_metadata_json: Some(
                self.values
                    .serialize_json_object(&response_metadata)
                    .map_err(operation_from_task_failure)?,
            ),
            run_id: completion.run_id,
            upstream_response_checksum: Some(
                self.values
                    .sha256_digest(&completion.upstream_response_checksum)
                    .map_err(operation_from_task_failure)?,
            ),
        };
        complete_tide_backfill(&input, session).map_err(|error| {
            self.values.tide_backfill_operation_failure(
                TideBackfillPersistenceOperation::Complete,
                &error.to_string(),
            )
        })
    }

    /// Atomically retain the exact provider failure and failed chunk outcome.
    fn fail_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        run_id: &str,
        identity: &TideBackfillChunkIdentity,
        failure: &TideBackfillProviderFailure,
    ) -> Result<(), TaskFailure> {
        let input = DatabaseTideBackfillFailure {
            attempts: u64::from(failure.attempts),
            error: self.values.tide_backfill_ingestion_error(failure)?,
            identity: admit_identity(identity, &mut self.values)?,
            run_id: run_id.to_owned(),
        };
        fail_tide_backfill(&input, session).map_err(|error| {
            self.values.tide_backfill_task_failure(
                TideBackfillPersistenceOperation::FailureFinalize,
                &error.to_string(),
            )
        })
    }

    /// Release the retained source session last.
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
        session.release().map_err(|error| {
            self.values.tide_backfill_task_failure(
                TideBackfillPersistenceOperation::Release,
                &error.to_string(),
            )
        })
    }
}

// admit one worker chunk identity through shared time and digest owners
fn admit_identity<Values: TideBackfillRuntimeValues>(
    identity: &TideBackfillChunkIdentity,
    values: &mut Values,
) -> Result<AdmittedTideBackfillIdentity, TaskFailure> {
    Ok(AdmittedTideBackfillIdentity {
        adapter_version: identity.adapter_version.to_owned(),
        chunk_plan_version: identity.chunk_plan_version.to_owned(),
        interval_end_exclusive: values.utc_instant(
            identity.interval_end_exclusive.start_ms(),
            RuntimeTimeValue::RequestedEndExclusive,
        )?,
        interval_start: values.utc_instant(
            identity.interval_start.start_ms(),
            RuntimeTimeValue::RequestedStart,
        )?,
        requested_from_date: identity.requested_from_date.clone(),
        requested_to_date: identity.requested_to_date.clone(),
        source_config_fingerprint: values.sha256_digest(&identity.source_config_fingerprint)?,
        source_id: identity.source_id.clone(),
    })
}

// retain local preparation failures inside the worker catch boundary
fn operation_from_task_failure(failure: TaskFailure) -> TideBackfillOperationError {
    TideBackfillOperationError::unexpected(failure.code)
}
