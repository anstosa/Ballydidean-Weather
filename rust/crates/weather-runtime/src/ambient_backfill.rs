//! Database binding for typed public-station historical backfill execution.

use std::fmt::Display;

use weather_contracts::{Nullable, PublicStationConfiguration, SourceCapability, SourceKind};
use weather_database::{
    AdmittedAmbientBackfillIdentity, AdmittedIngestionError, AmbientBackfillCommandPort,
    AmbientBackfillPoolPort, AmbientBackfillSourceReadPort,
    CompleteAmbientBackfill as DatabaseAmbientBackfillCompletion,
    FailAmbientBackfill as DatabaseAmbientBackfillFailure, RetainedSourceSession, SerializedJson,
    StartAmbientBackfillRun, ambient_backfill_abandon_expired_query,
    ambient_backfill_source_association_query, ambient_backfill_successful_chunk_query,
    complete_ambient_backfill, fail_ambient_backfill, start_ambient_backfill_run,
};
use weather_worker::{
    PublicStationBackfillChunkIdentity, PublicStationBackfillCompletion, PublicStationBackfillDate,
    PublicStationBackfillOperationError, PublicStationBackfillPlanningSource,
    PublicStationBackfillProviderFailure, PublicStationBackfillRepository,
    PublicStationBackfillRunStart, ScheduledSourceMaterialComparison,
    SupplementalPublicStationBackfillRepository, SupplementalPublicStationRecord,
    SupplementalPublicStationResponseMetadata, TaskFailure,
};

use crate::{
    AmbientWeatherRuntimeValues, RuntimeTimeValue, ScheduledDatabaseAdapter,
    SupplementalPublicStationDatabaseAdapter, ambient_weather_response_metadata,
    project_ambient_weather_record,
};

/// One Ambient backfill persistence boundary whose failure retains worker ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmbientBackfillPersistenceOperation {
    SourceAssociation,
    ResumeRead,
    Acquire,
    Abandon,
    Start,
    Complete,
    FailureFinalize,
    Release,
}

/// Canonical value owners for public-station association and backfill persistence.
pub trait AmbientBackfillRuntimeValues: AmbientWeatherRuntimeValues {
    /// Admit one configured UTC calendar date without implementing a time codec here.
    fn ambient_backfill_date(
        &mut self,
        value: &str,
    ) -> Result<PublicStationBackfillDate, TaskFailure>;

    /// Serialize exact endpoint and station request metadata.
    fn ambient_backfill_request_metadata_json(
        &mut self,
        endpoint: &str,
        station_key: &str,
    ) -> Result<Option<SerializedJson>, TaskFailure>;

    /// Admit one exact classified Ambient provider failure.
    fn ambient_backfill_ingestion_error(
        &mut self,
        failure: &PublicStationBackfillProviderFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure>;

    /// Classify one database failure at the task-level boundary.
    fn ambient_backfill_task_failure(
        &mut self,
        operation: AmbientBackfillPersistenceOperation,
        message: &str,
    ) -> TaskFailure;

    /// Classify one database failure inside the committed chunk boundary.
    fn ambient_backfill_operation_failure(
        &mut self,
        operation: AmbientBackfillPersistenceOperation,
        message: &str,
    ) -> PublicStationBackfillOperationError;
}

/// Existing Ambient worker repository backed by exact PostgreSQL plans.
pub struct DatabaseAmbientBackfillRepository<Database, Values> {
    database: Database,
    values: Values,
}

/// Source association required before planning provider-independent historical work.
pub trait PublicStationBackfillPlanningRepository: PublicStationBackfillRepository {
    /// Resolve selected active historical sources in checked catalog order.
    fn resolve_public_station_planning_sources(
        &mut self,
        configuration: &PublicStationConfiguration,
        selectors: &[String],
    ) -> Result<Vec<PublicStationBackfillPlanningSource>, TaskFailure>;
}

impl<Database, Values> DatabaseAmbientBackfillRepository<Database, Values> {
    /// Bind database and canonical value owners without performing I/O.
    pub const fn new(database: Database, values: Values) -> Self {
        Self { database, values }
    }

    /// Recover both bound owners after the one-shot lifecycle.
    pub fn into_parts(self) -> (Database, Values) {
        (self.database, self.values)
    }
}

impl<Database, Values> PublicStationBackfillRepository
    for DatabaseAmbientBackfillRepository<Database, Values>
where
    Database: AmbientBackfillPoolPort,
    Database::Error: Display,
    Values: AmbientBackfillRuntimeValues,
{
    type Session = Database::Session;

    /// Execute the exact six-bind successful chunk lookup before locking.
    fn has_successful_chunk(
        &mut self,
        identity: &PublicStationBackfillChunkIdentity,
    ) -> Result<bool, TaskFailure> {
        let identity = admit_identity(identity, &mut self.values)?;
        self.database
            .has_successful_chunk(&ambient_backfill_successful_chunk_query(&identity))
            .map(|row_count| row_count == 1)
            .map_err(|error| {
                self.values.ambient_backfill_task_failure(
                    AmbientBackfillPersistenceOperation::ResumeRead,
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
                self.values.ambient_backfill_task_failure(
                    AmbientBackfillPersistenceOperation::Acquire,
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
    ) -> Result<(), PublicStationBackfillOperationError> {
        let as_of = self
            .values
            .utc_instant(now_ms, RuntimeTimeValue::AbandonAsOf)
            .map_err(operation_from_task_failure)?;
        session
            .abandon_expired_runs(&ambient_backfill_abandon_expired_query(
                session.source_id(),
                &as_of,
            ))
            .map(|_| ())
            .map_err(|error| {
                self.values.ambient_backfill_operation_failure(
                    AmbientBackfillPersistenceOperation::Abandon,
                    &error.to_string(),
                )
            })
    }

    /// Commit one exact running backfill row before provider I/O.
    fn start_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        start: PublicStationBackfillRunStart,
    ) -> Result<String, PublicStationBackfillOperationError> {
        let input = StartAmbientBackfillRun {
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
                .ambient_backfill_request_metadata_json(start.endpoint, &start.station_key)
                .map_err(operation_from_task_failure)?,
            source_config_fingerprint: self
                .values
                .sha256_digest(&start.source_config_fingerprint)
                .map_err(operation_from_task_failure)?,
        };
        start_ambient_backfill_run(&input, session)
            .map(|run| run.id)
            .map_err(|error| {
                self.values.ambient_backfill_operation_failure(
                    AmbientBackfillPersistenceOperation::Start,
                    &error.to_string(),
                )
            })
    }

    /// Persist normalized Ambient records and response evidence atomically.
    fn complete_backfill_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: PublicStationBackfillCompletion,
    ) -> Result<(), PublicStationBackfillOperationError> {
        let identity = admit_identity(&completion.identity, &mut self.values)
            .map_err(operation_from_task_failure)?;
        let mut records = Vec::with_capacity(completion.records.len());

        // preserve provider order through the shared normalized weather projection
        for record in completion.records {
            records.push(
                project_ambient_weather_record(record, &mut self.values)
                    .map_err(operation_from_task_failure)?,
            );
        }
        let response_metadata = ambient_weather_response_metadata(&completion.response_metadata);
        let response_metadata_json = Some(
            self.values
                .serialize_json_object(&response_metadata)
                .map_err(operation_from_task_failure)?,
        );
        let input = DatabaseAmbientBackfillCompletion {
            attempts: u64::from(completion.attempts),
            identity,
            records,
            response_metadata_json,
            run_id: completion.run_id,
            upstream_response_checksum: Some(
                self.values
                    .sha256_digest(&completion.upstream_response_checksum)
                    .map_err(operation_from_task_failure)?,
            ),
        };
        complete_projected_backfill(&mut self.values, session, &input)
    }

    /// Atomically retain the exact provider failure and failed chunk outcome.
    fn fail_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        run_id: &str,
        identity: &PublicStationBackfillChunkIdentity,
        failure: &PublicStationBackfillProviderFailure,
    ) -> Result<(), TaskFailure> {
        let input = DatabaseAmbientBackfillFailure {
            attempts: u64::from(failure.attempts),
            error: self.values.ambient_backfill_ingestion_error(failure)?,
            identity: admit_identity(identity, &mut self.values)?,
            run_id: run_id.to_owned(),
        };
        fail_ambient_backfill(&input, session).map_err(|error| {
            self.values.ambient_backfill_task_failure(
                AmbientBackfillPersistenceOperation::FailureFinalize,
                &error.to_string(),
            )
        })
    }

    /// Release the retained source session last.
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
        session.release().map_err(|error| {
            self.values.ambient_backfill_task_failure(
                AmbientBackfillPersistenceOperation::Release,
                &error.to_string(),
            )
        })
    }
}

impl<Database, Values> SupplementalPublicStationBackfillRepository
    for DatabaseAmbientBackfillRepository<Database, Values>
where
    Database: AmbientBackfillPoolPort,
    Database::Error: Display,
    Values: AmbientBackfillRuntimeValues,
{
    /// Persist normalized supplemental records through the same atomic backfill transaction.
    fn complete_supplemental_backfill_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: PublicStationBackfillCompletion<
            SupplementalPublicStationRecord,
            SupplementalPublicStationResponseMetadata,
        >,
    ) -> Result<(), PublicStationBackfillOperationError> {
        let identity = admit_identity(&completion.identity, &mut self.values)
            .map_err(operation_from_task_failure)?;
        let mut records = Vec::with_capacity(completion.records.len());

        // preserve provider order through the existing supplemental storage projection
        for record in completion.records {
            records.push(
                SupplementalPublicStationDatabaseAdapter::project_record(&mut self.values, record)
                    .map_err(operation_from_task_failure)?,
            );
        }
        let response_metadata_json =
            SupplementalPublicStationDatabaseAdapter::response_metadata_json(
                &mut self.values,
                &completion.response_metadata,
            )
            .map_err(operation_from_task_failure)?;
        let input = DatabaseAmbientBackfillCompletion {
            attempts: u64::from(completion.attempts),
            identity,
            records,
            response_metadata_json,
            run_id: completion.run_id,
            upstream_response_checksum: Some(
                self.values
                    .sha256_digest(&completion.upstream_response_checksum)
                    .map_err(operation_from_task_failure)?,
            ),
        };
        complete_projected_backfill(&mut self.values, session, &input)
    }
}

impl<Database, Values> PublicStationBackfillPlanningRepository
    for DatabaseAmbientBackfillRepository<Database, Values>
where
    Database: AmbientBackfillPoolPort
        + AmbientBackfillSourceReadPort<Error = <Database as AmbientBackfillPoolPort>::Error>,
    <Database as AmbientBackfillPoolPort>::Error: Display,
    Values: AmbientBackfillRuntimeValues + ScheduledSourceMaterialComparison,
{
    /// Associate every selected historical adapter through the original active database join.
    fn resolve_public_station_planning_sources(
        &mut self,
        configuration: &PublicStationConfiguration,
        selectors: &[String],
    ) -> Result<Vec<PublicStationBackfillPlanningSource>, TaskFailure> {
        let configured = configuration
            .stations
            .iter()
            .flat_map(|station| {
                station.sources.iter().filter_map(move |source| {
                    let selector_matches = selectors.is_empty() || selectors.contains(&source.key);

                    // retain the original source-owned historical selection scope
                    (source.active
                        && source.capabilities.contains(&SourceCapability::Historical)
                        && selector_matches)
                        .then_some((station, source))
                })
            })
            .collect::<Vec<_>>();

        // reject unknown inactive or nonhistorical selectors before database access
        if !selectors.is_empty() && configured.len() != selectors.len() {
            return Err(self.values.ambient_backfill_task_failure(
                AmbientBackfillPersistenceOperation::SourceAssociation,
                "one or more requested public-station sources are unavailable",
            ));
        }

        let source_keys = configured
            .iter()
            .map(|(_, source)| source.key.clone())
            .collect::<Vec<_>>();
        let rows = self
            .database
            .read_ambient_backfill_sources(&ambient_backfill_source_association_query(
                &configuration.siteKey,
                &source_keys,
            ))
            .map_err(|error| {
                self.values.ambient_backfill_task_failure(
                    AmbientBackfillPersistenceOperation::SourceAssociation,
                    &error.to_string(),
                )
            })?;
        let mut resolved = Vec::with_capacity(configured.len());

        // preserve station and source catalog order rather than SQL or selector order
        for (station, source) in configured {
            let Some(row) = rows.iter().rev().find(|row| row.source_key == source.key) else {
                return Err(self.values.ambient_backfill_task_failure(
                    AmbientBackfillPersistenceOperation::SourceAssociation,
                    &format!("active public-station source {} is missing", source.key),
                ));
            };
            let material_matches = self
                .values
                .value_and_object_equal(&row.material_provider_config, &source.adapterConfig)
                .unwrap_or(false);
            let identity_matches = row.site_slug == configuration.siteKey
                && row.station_slug == station.key
                && row.provider_key == source.providerKey
                && row.source_key == source.key
                && row.source_kind == SourceKind::PhysicalSensor
                && row.source_config_fingerprint == source.fingerprint
                && row.timezone == station.timezone
                && material_matches;

            // fail closed on any durable identity or canonical material drift
            if !identity_matches {
                return Err(self.values.ambient_backfill_task_failure(
                    AmbientBackfillPersistenceOperation::SourceAssociation,
                    &format!(
                        "public-station source {} identity does not match configuration",
                        source.key
                    ),
                ));
            }

            let history_start = required_history_start(&source.historyStartDate, &source.key)?;
            resolved.push(PublicStationBackfillPlanningSource {
                adapter: source.adapter,
                source_id: row.source_id.clone(),
                source_key: row.source_key.clone(),
                source_config_fingerprint: row.source_config_fingerprint.clone(),
                history_start: self.values.ambient_backfill_date(history_start)?,
                history_end: optional_date(&source.historyEndDate, &mut self.values)?,
                maximum_chunk_days: source.maximumChunkDays,
            });
        }

        Ok(resolved)
    }
}

// finalize either public-station record family through one exact atomic receipt transaction
fn complete_projected_backfill<Session, Values>(
    values: &mut Values,
    session: &mut Session,
    input: &DatabaseAmbientBackfillCompletion,
) -> Result<(), PublicStationBackfillOperationError>
where
    Session: AmbientBackfillCommandPort,
    Session::Error: Display,
    Values: AmbientBackfillRuntimeValues,
{
    complete_ambient_backfill(input, session).map_err(|error| {
        values.ambient_backfill_operation_failure(
            AmbientBackfillPersistenceOperation::Complete,
            &error.to_string(),
        )
    })
}

// admit one worker chunk identity through shared time and digest owners
fn admit_identity<Values: AmbientBackfillRuntimeValues>(
    identity: &PublicStationBackfillChunkIdentity,
    values: &mut Values,
) -> Result<AdmittedAmbientBackfillIdentity, TaskFailure> {
    Ok(AdmittedAmbientBackfillIdentity {
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
fn operation_from_task_failure(failure: TaskFailure) -> PublicStationBackfillOperationError {
    PublicStationBackfillOperationError::unexpected(failure.code)
}

// require the configured archive lower bound without parsing it here
fn required_history_start<'a>(
    value: &'a Nullable<String>,
    source_key: &str,
) -> Result<&'a str, TaskFailure> {
    // reject impossible checked configuration before any provider operation
    match value {
        Nullable::Value(value) => Ok(value),
        Nullable::Null => Err(TaskFailure {
            attempts: 0,
            code: format!("historical source {source_key} has no start date"),
        }),
    }
}

// project one optional checked UTC date through the admitted date owner
fn optional_date<Values: AmbientBackfillRuntimeValues>(
    value: &Nullable<String>,
    values: &mut Values,
) -> Result<Option<PublicStationBackfillDate>, TaskFailure> {
    // retain configuration null rather than inventing an upper bound
    match value {
        Nullable::Null => Ok(None),
        Nullable::Value(value) => values.ambient_backfill_date(value).map(Some),
    }
}
