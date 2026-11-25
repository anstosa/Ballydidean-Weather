//! PostgreSQL binding for the original Open-Meteo Archive weather-record backfill.

use std::fmt::Display;

use weather_contracts::{
    BackfillChunkIdentity, IngestionMode, JsonValue, NormalizedWeatherRecord, Nullable,
};
use weather_database::{
    AdmittedIngestionError, AdmittedTideBackfillIdentity,
    CompleteTideBackfill as DatabaseOpenMeteoArchiveCompletion,
    FailTideBackfill as DatabaseOpenMeteoArchiveFailure, OpenMeteoBackfillSourceQuery,
    OpenMeteoBackfillSourceReadPort, OpenMeteoBackfillSourceRow, PreparedWeatherRecord,
    RetainedSourceSession, SerializedJson, StartTideBackfillRun, TideBackfillCommandPort,
    TideBackfillPoolPort, WeatherRecordStorageEvidence, complete_tide_backfill, fail_tide_backfill,
    start_tide_backfill_run, tide_backfill_abandon_expired_query,
    tide_backfill_successful_chunk_query,
};
use weather_worker::{
    OpenMeteoArchiveCleanupFailure, OpenMeteoArchiveCompletion, OpenMeteoArchiveRepository,
    OpenMeteoArchiveRunStart, TaskFailure, TideBackfillOperationError, TideBackfillProviderFailure,
};

use crate::{OpenMeteoRuntimeValues, RuntimeTimeValue};

/// One Open-Meteo Archive persistence boundary whose failure retains worker ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenMeteoArchivePersistenceOperation {
    ResumeRead,
    Acquire,
    Abandon,
    Start,
    Complete,
    FailureFinalize,
    Release,
}

/// Canonical JSON and failure owners layered on the shared weather-record values.
pub trait OpenMeteoArchiveBackfillRuntimeValues: OpenMeteoRuntimeValues {
    /// Admit one exact classified Archive provider failure.
    fn open_meteo_archive_ingestion_error(
        &mut self,
        failure: &TideBackfillProviderFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure>;

    /// Classify one database failure at the task-level boundary.
    fn open_meteo_archive_task_failure(
        &mut self,
        operation: OpenMeteoArchivePersistenceOperation,
        message: &str,
    ) -> TaskFailure;

    /// Classify one database failure inside the committed chunk boundary.
    fn open_meteo_archive_operation_failure(
        &mut self,
        operation: OpenMeteoArchivePersistenceOperation,
        message: &str,
    ) -> TideBackfillOperationError;

    /// Admit the original redacted cleanup diagnostic from the typed driver error.
    fn open_meteo_archive_cleanup_failure(
        &mut self,
        operation: OpenMeteoArchivePersistenceOperation,
        message: &str,
    ) -> OpenMeteoArchiveCleanupFailure;
}

/// Existing Archive worker repository backed by the shared normalized-weather transaction.
pub struct DatabaseOpenMeteoArchiveBackfillRepository<Database, Values> {
    database: Database,
    values: Values,
}

impl<Database, Values> DatabaseOpenMeteoArchiveBackfillRepository<Database, Values> {
    /// Bind database and canonical value owners without performing I/O.
    pub const fn new(database: Database, values: Values) -> Self {
        Self { database, values }
    }

    /// Recover both bound owners after the one-shot lifecycle.
    pub fn into_parts(self) -> (Database, Values) {
        (self.database, self.values)
    }
}

impl<Database, Values> OpenMeteoBackfillSourceReadPort
    for DatabaseOpenMeteoArchiveBackfillRepository<Database, Values>
where
    Database: OpenMeteoBackfillSourceReadPort,
{
    type Error = Database::Error;

    /// Forward the original source lookup through the same process-owned pool.
    fn read_open_meteo_backfill_source(
        &mut self,
        query: &OpenMeteoBackfillSourceQuery,
    ) -> Result<Option<OpenMeteoBackfillSourceRow>, Self::Error> {
        self.database.read_open_meteo_backfill_source(query)
    }
}

impl<Database, Values> OpenMeteoArchiveRepository
    for DatabaseOpenMeteoArchiveBackfillRepository<Database, Values>
where
    Database: TideBackfillPoolPort,
    Database::Error: Display,
    Values: OpenMeteoArchiveBackfillRuntimeValues,
{
    type Session = Database::Session;

    /// Execute the exact six-bind successful chunk lookup before locking.
    fn has_successful_chunk(
        &mut self,
        identity: &BackfillChunkIdentity,
    ) -> Result<bool, TaskFailure> {
        let identity = admit_identity(identity, &mut self.values)?;
        self.database
            .has_successful_chunk(&tide_backfill_successful_chunk_query(&identity))
            .map(|row_count| row_count == 1)
            .map_err(|error| {
                self.values.open_meteo_archive_task_failure(
                    OpenMeteoArchivePersistenceOperation::ResumeRead,
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
                self.values.open_meteo_archive_task_failure(
                    OpenMeteoArchivePersistenceOperation::Acquire,
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
        started_at_ms: i64,
    ) -> Result<(), TideBackfillOperationError> {
        let as_of = self
            .values
            .utc_instant(started_at_ms, RuntimeTimeValue::AbandonAsOf)
            .map_err(operation_from_task_failure)?;
        session
            .abandon_expired_runs(&tide_backfill_abandon_expired_query(
                session.source_id(),
                &as_of,
            ))
            .map(|_| ())
            .map_err(|error| {
                self.values.open_meteo_archive_operation_failure(
                    OpenMeteoArchivePersistenceOperation::Abandon,
                    &error.to_string(),
                )
            })
    }

    /// Commit one exact running backfill row before provider I/O.
    fn start_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        start: OpenMeteoArchiveRunStart,
    ) -> Result<String, TideBackfillOperationError> {
        // keep the generic transaction fixed to the original backfill mode
        if start.mode != IngestionMode::Backfill {
            return Err(TideBackfillOperationError::unexpected(
                "open_meteo_archive_ingestion_mode_invalid",
            ));
        }
        let input = StartTideBackfillRun {
            adapter_version: start.adapter_version.to_owned(),
            attempts: 0,
            chunk_plan_version: start.chunk_plan_version.to_owned(),
            deadline_at: self
                .values
                .utc_instant(start.deadline_at_ms, RuntimeTimeValue::Deadline)
                .map_err(operation_from_task_failure)?,
            interval_end_exclusive: admit_utc(
                &mut self.values,
                &start.requested_end_exclusive,
                RuntimeTimeValue::RequestedEndExclusive,
            )
            .map_err(operation_from_task_failure)?,
            interval_start: admit_utc(
                &mut self.values,
                &start.requested_start,
                RuntimeTimeValue::RequestedStart,
            )
            .map_err(operation_from_task_failure)?,
            request_metadata_json: Some(
                self.values
                    .serialize_json_object(&request_metadata(&start))
                    .map_err(operation_from_task_failure)?,
            ),
            source_config_fingerprint: self
                .values
                .sha256_digest(&start.source_config_fingerprint)
                .map_err(operation_from_task_failure)?,
        };
        start_tide_backfill_run(&input, session)
            .map(|run| run.id)
            .map_err(|error| {
                self.values.open_meteo_archive_operation_failure(
                    OpenMeteoArchivePersistenceOperation::Start,
                    &error.to_string(),
                )
            })
    }

    /// Persist normalized Archive records and response evidence atomically.
    fn complete_backfill_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: OpenMeteoArchiveCompletion,
    ) -> Result<(), TideBackfillOperationError> {
        let identity = admit_identity(&completion.identity, &mut self.values)
            .map_err(operation_from_task_failure)?;
        let mut records = Vec::with_capacity(completion.records.len());

        // preserve provider order through the shared weather-record upsert batches
        for record in completion.records {
            records.push(
                prepare_record(record, &mut self.values).map_err(operation_from_task_failure)?,
            );
        }
        let input = DatabaseOpenMeteoArchiveCompletion {
            attempts: u64::from(completion.attempts),
            identity,
            records,
            response_metadata_json: serialize_object(
                &mut self.values,
                &completion.response_metadata,
            )
            .map_err(operation_from_task_failure)?,
            run_id: completion.run_id,
            upstream_response_checksum: Some(
                self.values
                    .sha256_digest(&completion.upstream_response_checksum)
                    .map_err(operation_from_task_failure)?,
            ),
        };
        complete_tide_backfill(&input, session).map_err(|error| {
            self.values.open_meteo_archive_operation_failure(
                OpenMeteoArchivePersistenceOperation::Complete,
                &error.to_string(),
            )
        })
    }

    /// Atomically retain the exact provider failure and failed chunk outcome.
    fn fail_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        run_id: &str,
        identity: &BackfillChunkIdentity,
        failure: &TideBackfillProviderFailure,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
        use OpenMeteoArchivePersistenceOperation::FailureFinalize;
        let error = self
            .values
            .open_meteo_archive_ingestion_error(failure)
            .map_err(|error| {
                self.values
                    .open_meteo_archive_cleanup_failure(FailureFinalize, &error.code)
            })?;
        let identity = admit_identity(identity, &mut self.values).map_err(|error| {
            self.values
                .open_meteo_archive_cleanup_failure(FailureFinalize, &error.code)
        })?;
        let input = DatabaseOpenMeteoArchiveFailure {
            attempts: u64::from(failure.attempts),
            error,
            identity,
            run_id: run_id.to_owned(),
        };
        fail_tide_backfill(&input, session).map_err(|error| {
            self.values.open_meteo_archive_cleanup_failure(
                OpenMeteoArchivePersistenceOperation::FailureFinalize,
                &error.to_string(),
            )
        })
    }

    /// Release the retained source session last.
    fn release_source_session(
        &mut self,
        session: Self::Session,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
        session.release().map_err(|error| {
            self.values.open_meteo_archive_cleanup_failure(
                OpenMeteoArchivePersistenceOperation::Release,
                &error.to_string(),
            )
        })
    }
}

/// Project the original provider endpoint and inclusive dates before JSON serialization.
fn request_metadata(start: &OpenMeteoArchiveRunStart) -> [(Vec<u16>, JsonValue); 3] {
    [
        ("end_date", start.end_date.as_str()),
        ("endpoint", start.endpoint),
        ("start_date", start.start_date.as_str()),
    ]
    .map(|(key, value)| {
        (
            key.encode_utf16().collect(),
            JsonValue::String(value.encode_utf16().collect()),
        )
    })
}

// admit one generated chunk identity through existing UTC and digest owners
fn admit_identity<Values: OpenMeteoArchiveBackfillRuntimeValues>(
    identity: &BackfillChunkIdentity,
    values: &mut Values,
) -> Result<AdmittedTideBackfillIdentity, TaskFailure> {
    Ok(AdmittedTideBackfillIdentity {
        adapter_version: identity.adapterVersion.clone(),
        chunk_plan_version: identity.chunkPlanVersion.clone(),
        interval_end_exclusive: admit_utc(
            values,
            &identity.intervalEndExclusive,
            RuntimeTimeValue::RequestedEndExclusive,
        )?,
        interval_start: admit_utc(
            values,
            &identity.intervalStart,
            RuntimeTimeValue::RequestedStart,
        )?,
        requested_from_date: identity.requestedFromDate.clone(),
        requested_to_date: identity.requestedToDate.clone(),
        source_config_fingerprint: values.sha256_digest(&identity.sourceConfigFingerprint)?,
        source_id: identity.sourceId.clone(),
    })
}

// round-trip one admitted UTC instant through the existing canonical time boundary
fn admit_utc<Values: OpenMeteoArchiveBackfillRuntimeValues>(
    values: &mut Values,
    instant: &str,
    role: RuntimeTimeValue,
) -> Result<String, TaskFailure> {
    let epoch_milliseconds = values.epoch_milliseconds(instant, role)?;
    values.utc_instant(epoch_milliseconds, role)
}

// prepare the exact generated row and its separately stored metadata evidence
fn prepare_record<Values: OpenMeteoArchiveBackfillRuntimeValues>(
    record: NormalizedWeatherRecord,
    values: &mut Values,
) -> Result<PreparedWeatherRecord, TaskFailure> {
    let provider_metadata_json = serialize_nullable_object(values, &record.metadata.provider)?;
    let quality_metadata_json = serialize_nullable_object(values, &record.metadata.quality)?;
    let content_hash = values.weather_record_content_hash(&record)?;

    Ok(PreparedWeatherRecord {
        record,
        storage: WeatherRecordStorageEvidence {
            content_hash,
            provider_metadata_json,
            quality_metadata_json,
        },
    })
}

// serialize one provider-owned response metadata object without implementing JSON
fn serialize_object<Values: OpenMeteoArchiveBackfillRuntimeValues>(
    values: &mut Values,
    value: &JsonValue,
) -> Result<Option<SerializedJson>, TaskFailure> {
    let JsonValue::Object(fields) = value else {
        return Err(TaskFailure {
            attempts: 0,
            code: "invalid_payload".to_owned(),
        });
    };
    values.serialize_json_object(fields).map(Some)
}

// serialize one nullable generated metadata object through the existing JSON owner
fn serialize_nullable_object<Values: OpenMeteoArchiveBackfillRuntimeValues>(
    values: &mut Values,
    value: &Nullable<Vec<(Vec<u16>, JsonValue)>>,
) -> Result<Option<SerializedJson>, TaskFailure> {
    match value {
        Nullable::Null => Ok(None),
        Nullable::Value(fields) => values.serialize_json_object(fields).map(Some),
    }
}

// retain local preparation failures inside the worker chunk catch boundary
fn operation_from_task_failure(failure: TaskFailure) -> TideBackfillOperationError {
    TideBackfillOperationError::unexpected(failure.code)
}
