//! Native PostgreSQL execution for retained scheduled-ingestion sessions.

use tokio_postgres::Row;
use weather_contracts::{
    IngestionErrorClassification, IngestionState, JsonValue, Nullable, SourceKind,
};
use weather_database::{
    AssertRunningScheduledBind, AssertRunningScheduledQuery, CheckpointUpdateBind,
    FinalizeFailedIngestionBind, FinalizeFailedIngestionQuery, FinalizeScheduledRunBind,
    FinalizeScheduledRunQuery, FindRunningIngestionBind, FindRunningIngestionQuery,
    GetScheduledCheckpointBind, GetScheduledCheckpointQuery, InitialCheckpointBind,
    InsertScheduledIngestionBind, InsertScheduledIngestionQuery, RetainedSourceSession,
    RunningIngestionLookup, ScheduledCheckpointDbRow, ScheduledCheckpointQuery,
    ScheduledCheckpointReadPort, ScheduledIngestionCommandPort, ScheduledRunFailureCommandPort,
    ScheduledRunIdentityBind, ScheduledRunIdentityQuery, ScheduledRunStartCommandPort,
    SerializedJson, StartedScheduledIngestion, WeatherRecordBind, WeatherRecordUpsertQuery,
};

use crate::{
    NativePostgresError, NativePostgresParameter as Parameter, NativePostgresPool,
    NativePostgresSession, NativePostgresType, OpenMeteoDatabaseBoundary, TaskFailure,
    native_postgres_weather_rows as field,
};

const ACQUIRE_SOURCE_SESSION_SQL: &str =
    "SELECT pg_try_advisory_lock(hashtextextended('weather-source:' || $1, 0)) AS acquired";
const RELEASE_SOURCE_SESSION_SQL: &str =
    "SELECT pg_advisory_unlock(hashtextextended('weather-source:' || $1, 0)) AS released";
const ABANDON_EXPIRED_RUNS_SQL: &str = r#"
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

/// One native connection retaining the source advisory lock until explicit release.
pub struct NativePostgresScheduledSession {
    session: Option<NativePostgresSession>,
    source_id: String,
}

impl NativePostgresScheduledSession {
    /// Retain one successfully acquired advisory lock and its connection.
    fn new(session: NativePostgresSession, source_id: String) -> Self {
        Self {
            session: Some(session),
            source_id,
        }
    }

    /// Borrow the still-retained PostgreSQL connection.
    fn session_mut(&mut self) -> Result<&mut NativePostgresSession, NativePostgresError> {
        self.session
            .as_mut()
            .ok_or(NativePostgresError::SessionReleased)
    }

    /// Prove advisory unlock before recycling the retained connection.
    fn release(mut self) -> Result<(), NativePostgresError> {
        let session = self
            .session
            .take()
            .ok_or(NativePostgresError::SessionReleased)?;
        unlock_source_session(session, &self.source_id)
    }
}

impl Drop for NativePostgresScheduledSession {
    /// Close or unlock every retained advisory-lock connection on abandoned ownership paths.
    fn drop(&mut self) {
        // preserve pool safety even when a caller omits explicit release
        if let Some(session) = self.session.take() {
            let _ = unlock_source_session(session, &self.source_id);
        }
    }
}

impl RetainedSourceSession for NativePostgresScheduledSession {
    /// Return the immutable source identity associated with the advisory lock.
    fn source_id(&self) -> &str {
        &self.source_id
    }
}

impl ScheduledRunStartCommandPort for NativePostgresScheduledSession {
    type Error = NativePostgresError;

    /// Read every running source row and retain the first exact bigint identity.
    fn find_running_ingestion(
        &mut self,
        query: &FindRunningIngestionQuery,
    ) -> Result<RunningIngestionLookup, Self::Error> {
        let parameters = find_running_parameters(&query.binds)?;
        let rows = self.session_mut()?.query(query.sql, &parameters)?;
        let ids = rows
            .iter()
            .map(|row| field::id(row, "id"))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RunningIngestionLookup {
            first_run_id: ids.first().cloned(),
            row_count: ids.len(),
        })
    }

    /// Insert one scheduled run and decode its returned identity, receipt clock, and state.
    fn insert_scheduled_ingestion(
        &mut self,
        query: &InsertScheduledIngestionQuery,
    ) -> Result<Option<StartedScheduledIngestion>, Self::Error> {
        let parameters = insert_scheduled_parameters(&query.binds)?;
        let rows = self.session_mut()?.query(query.sql, &parameters)?;
        optional_single_row(&rows, "scheduled ingestion insert")?
            .map(decode_started_ingestion)
            .transpose()
    }
}

impl ScheduledCheckpointReadPort for NativePostgresScheduledSession {
    type Error = NativePostgresError;

    /// Read and decode one source checkpoint without JSON or timestamp string coercion.
    fn get_scheduled_checkpoint(
        &mut self,
        query: &GetScheduledCheckpointQuery,
    ) -> Result<Option<ScheduledCheckpointDbRow>, Self::Error> {
        let parameters = checkpoint_read_parameters(&query.binds)?;
        let rows = self.session_mut()?.query(query.sql, &parameters)?;
        optional_single_row(&rows, "scheduled checkpoint")?
            .map(decode_scheduled_checkpoint)
            .transpose()
    }
}

impl ScheduledIngestionCommandPort for NativePostgresScheduledSession {
    type Error = NativePostgresError;

    /// Begin the source-owned completion transaction on the retained connection.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.begin()
    }

    /// Execute the scheduled identity guard as a row-returning query.
    fn assert_scheduled_run(
        &mut self,
        query: &ScheduledRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = scheduled_identity_parameters(&query.binds)?;
        let rows = self.session_mut()?.query(query.sql, &parameters)?;
        Ok(rows.len())
    }

    /// Execute one bounded weather-record upsert using every typed dynamic bind.
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        let parameters = weather_record_parameters(&query.binds)?;
        self.session_mut()?.execute(&query.sql, &parameters)?;
        Ok(())
    }

    /// Execute the selected initial or compare-and-set checkpoint write.
    fn advance_scheduled_checkpoint(
        &mut self,
        query: &ScheduledCheckpointQuery,
    ) -> Result<usize, Self::Error> {
        // retain distinct bind domains for insert and update plans
        let affected = match query {
            ScheduledCheckpointQuery::Insert { sql, binds } => {
                let parameters = initial_checkpoint_parameters(binds)?;
                self.session_mut()?.execute(sql, &parameters)?
            }
            ScheduledCheckpointQuery::Update { sql, binds } => {
                let parameters = checkpoint_update_parameters(binds)?;
                self.session_mut()?.execute(sql, &parameters)?
            }
        };
        row_count(affected, "scheduled checkpoint write")
    }

    /// Guard-finalize one successful scheduled run.
    fn finalize_scheduled_run(
        &mut self,
        query: &FinalizeScheduledRunQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = finalize_scheduled_parameters(&query.binds)?;
        let affected = self.session_mut()?.execute(query.sql, &parameters)?;
        row_count(affected, "scheduled run finalization")
    }

    /// Commit the source-owned completion transaction.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.commit()
    }

    /// Roll back the source-owned completion transaction.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.rollback()
    }
}

impl ScheduledRunFailureCommandPort for NativePostgresScheduledSession {
    type Error = NativePostgresError;

    /// Begin the source-owned failure transaction on the retained connection.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.begin()
    }

    /// Execute the running scheduled-mode guard as a row-returning query.
    fn assert_running_scheduled(
        &mut self,
        query: &AssertRunningScheduledQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = assert_running_parameters(&query.binds)?;
        let rows = self.session_mut()?.query(query.sql, &parameters)?;
        Ok(rows.len())
    }

    /// Guard-finalize one failed scheduled run with its admitted evidence.
    fn finalize_failed_ingestion(
        &mut self,
        query: &FinalizeFailedIngestionQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = finalize_failed_parameters(&query.binds)?;
        let affected = self.session_mut()?.execute(query.sql, &parameters)?;
        row_count(affected, "failed scheduled run finalization")
    }

    /// Commit the source-owned failure transaction.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.commit()
    }

    /// Roll back the source-owned failure transaction.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.rollback()
    }
}

impl OpenMeteoDatabaseBoundary for NativePostgresPool {
    type Session = NativePostgresScheduledSession;

    /// Checkout one connection and retain it only after the source lock is proven acquired.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure> {
        acquire_source_session(self, source_id).map_err(|_| task_failure("source_session_acquire"))
    }

    /// Recover expired runs and decode every returned run identity before continuing.
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        as_of: &str,
    ) -> Result<(), TaskFailure> {
        let parameters = [bigint(&session.source_id, "source id"), timestamp(as_of)];
        let parameters =
            collect_parameters(parameters).map_err(|_| task_failure("abandon_expired_runs"))?;
        let rows = session
            .session_mut()
            .and_then(|postgres| postgres.query(ABANDON_EXPIRED_RUNS_SQL, &parameters))
            .map_err(|_| task_failure("abandon_expired_runs"))?;
        rows.iter()
            .map(|row| field::id(row, "id"))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| task_failure("abandon_expired_runs"))?;
        Ok(())
    }

    /// Prove advisory unlock before returning the connection to the native pool.
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
        session
            .release()
            .map_err(|_| task_failure("source_session_release"))
    }
}

/// Acquire one source advisory lock without recycling an ambiguously locked connection.
fn acquire_source_session(
    pool: &NativePostgresPool,
    source_id: &str,
) -> Result<Option<NativePostgresScheduledSession>, NativePostgresError> {
    let mut session = pool.checkout()?;
    let result = session.query(
        ACQUIRE_SOURCE_SESSION_SQL,
        &[Parameter::Text(source_id.to_owned())],
    );
    // discard on query or decode uncertainty because the server may hold the session lock
    let acquired = match result {
        Ok(rows) => match decode_lock_result(&rows, "acquired") {
            Ok(acquired) => acquired,
            Err(error) => {
                let _ = session.discard();
                return Err(error);
            }
        },
        Err(error) => {
            let _ = session.discard();
            return Err(error);
        }
    };
    // release an unclaimed connection immediately
    if !acquired {
        session.release()?;
        return Ok(None);
    }
    Ok(Some(NativePostgresScheduledSession::new(
        session,
        source_id.to_owned(),
    )))
}

/// Unlock one retained source and recycle only a connection with proven cleanup state.
fn unlock_source_session(
    mut session: NativePostgresSession,
    source_id: &str,
) -> Result<(), NativePostgresError> {
    let result = session.query(
        RELEASE_SOURCE_SESSION_SQL,
        &[Parameter::Text(source_id.to_owned())],
    );
    // close the connection when unlock execution itself is uncertain
    let released = match result {
        Ok(rows) => match decode_lock_result(&rows, "released") {
            Ok(released) => released,
            Err(error) => {
                let _ = session.discard();
                return Err(error);
            }
        },
        Err(error) => {
            let _ = session.discard();
            return Err(error);
        }
    };
    session.release()?;
    // surface an ownership violation after the now-safe connection is returned
    if !released {
        return Err(NativePostgresError::decode(
            "released",
            "source advisory lock was not owned",
        ));
    }
    Ok(())
}

/// Decode the exact singleton boolean returned by one advisory-lock statement.
fn decode_lock_result(rows: &[Row], column: &str) -> Result<bool, NativePostgresError> {
    let row = required_single_row(rows, column)?;
    field::column(row, column)
}

/// Decode the exact row returned by a scheduled start insert.
fn decode_started_ingestion(row: &Row) -> Result<StartedScheduledIngestion, NativePostgresError> {
    Ok(StartedScheduledIngestion {
        id: field::id(row, "id")?,
        started_at: field::utc(row, "startedAt")?,
        state: field::wire_enum(row, "state", IngestionState::WIRE_VALUES)?,
    })
}

/// Decode one durable scheduled checkpoint through native PostgreSQL scalar types.
fn decode_scheduled_checkpoint(row: &Row) -> Result<ScheduledCheckpointDbRow, NativePostgresError> {
    let provider_cursor = match field::json(row, "provider_cursor")? {
        None | Some(JsonValue::Null) => Nullable::Null,
        Some(JsonValue::Object(fields)) => Nullable::Value(fields),
        Some(_) => {
            return Err(NativePostgresError::decode(
                "provider_cursor",
                "expected a JSON object or null",
            ));
        }
    };
    Ok(ScheduledCheckpointDbRow {
        last_committed_at: field::utc(row, "last_committed_at")?,
        last_valid_at: field::utc(row, "last_valid_at")?,
        provider_cursor,
        source_id: field::id(row, "source_id")?,
        version: field::unsigned(row, "version")? as f64,
        window_end_exclusive: field::utc(row, "window_end_exclusive")?,
        window_start: field::utc(row, "window_start")?,
    })
}

/// Require exactly one row for a mandatory singleton query contract.
fn required_single_row<'a>(rows: &'a [Row], label: &str) -> Result<&'a Row, NativePostgresError> {
    // reject both missing and duplicated lock receipts
    if rows.len() != 1 {
        return Err(NativePostgresError::decode(
            label,
            "query did not return exactly one row",
        ));
    }
    Ok(&rows[0])
}

/// Preserve a nullable singleton row while rejecting impossible duplicates.
fn optional_single_row<'a>(
    rows: &'a [Row],
    label: &str,
) -> Result<Option<&'a Row>, NativePostgresError> {
    // reject duplicate rows rather than silently choosing one
    if rows.len() > 1 {
        return Err(NativePostgresError::decode(
            label,
            "query returned more than one row",
        ));
    }
    Ok(rows.first())
}

/// Convert one database affected-row count into the repository's platform count.
fn row_count(value: u64, label: &str) -> Result<usize, NativePostgresError> {
    usize::try_from(value).map_err(|error| NativePostgresError::decode(label, error))
}

/// Bind one predecessor bigint string as PostgreSQL int8.
fn bigint(value: &str, label: &str) -> Result<Parameter, NativePostgresError> {
    value
        .parse::<i64>()
        .map(Parameter::I64)
        .map_err(|error| NativePostgresError::decode(label, error))
}

/// Bind one safe unsigned repository count to its actual integer column type.
fn count(value: u64, label: &str) -> Result<Parameter, NativePostgresError> {
    i32::try_from(value)
        .map(Parameter::I32)
        .map_err(|error| NativePostgresError::decode(label, error))
}

/// Bind one checkpoint version to its actual bigint column type.
fn version(value: u64) -> Result<Parameter, NativePostgresError> {
    i64::try_from(value)
        .map(Parameter::I64)
        .map_err(|error| NativePostgresError::decode("checkpoint version", error))
}

/// Bind one admitted UTC instant through the native timestamp owner.
fn timestamp(value: &str) -> Result<Parameter, NativePostgresError> {
    Ok(Parameter::Timestamp(value.to_owned()))
}

/// Bind nullable serialized JSON without changing producer member order.
fn nullable_json(value: &Option<SerializedJson>) -> Parameter {
    // preserve SQL null independently from a serialized JSON value
    match value {
        Some(value) => Parameter::Json(value.as_str().to_owned()),
        None => Parameter::Null(NativePostgresType::Json),
    }
}

/// Bind nullable text to the actual PostgreSQL text parameter type.
fn nullable_text(value: &Option<String>) -> Parameter {
    // preserve SQL null independently from an empty string
    match value {
        Some(value) => Parameter::Text(value.clone()),
        None => Parameter::Null(NativePostgresType::Text),
    }
}

/// Bind nullable timestamp text to the actual PostgreSQL timestamp parameter type.
fn nullable_timestamp(value: &Option<String>) -> Parameter {
    // preserve SQL null independently from an admitted timestamp
    match value {
        Some(value) => Parameter::Timestamp(value.clone()),
        None => Parameter::Null(NativePostgresType::Timestamp),
    }
}

/// Resolve one generated enum to its exact database wire spelling.
fn wire_value<T: Copy + PartialEq>(
    value: T,
    values: &[(&str, T)],
    label: &str,
) -> Result<String, NativePostgresError> {
    values
        .iter()
        .find_map(|(wire, candidate)| (*candidate == value).then_some((*wire).to_owned()))
        .ok_or_else(|| NativePostgresError::decode(label, "unsupported enum value"))
}

/// Collect a fixed sequence of fallible parameter conversions in placeholder order.
fn collect_parameters<const N: usize>(
    parameters: [Result<Parameter, NativePostgresError>; N],
) -> Result<Vec<Parameter>, NativePostgresError> {
    parameters.into_iter().collect()
}

/// Map the running-row lookup's source bigint bind.
fn find_running_parameters(
    binds: &[FindRunningIngestionBind; 1],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            FindRunningIngestionBind::BigintText(value) => bigint(value, "source id"),
        })
        .collect()
}

/// Map every scheduled-start bind to its actual PostgreSQL type.
fn insert_scheduled_parameters(
    binds: &[InsertScheduledIngestionBind; 10],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            InsertScheduledIngestionBind::BigintText(value) => bigint(value, "source id"),
            InsertScheduledIngestionBind::Text(value) => Ok(Parameter::Text(value.clone())),
            InsertScheduledIngestionBind::ScheduledMode => {
                Ok(Parameter::Text("scheduled".to_owned()))
            }
            InsertScheduledIngestionBind::UtcInstant(value) => timestamp(value),
            InsertScheduledIngestionBind::Fingerprint(value) => {
                Ok(Parameter::Text(value.as_str().to_owned()))
            }
            InsertScheduledIngestionBind::NullChunkPlanVersion => {
                Ok(Parameter::Null(NativePostgresType::Text))
            }
            InsertScheduledIngestionBind::Count(value) => count(*value, "attempts"),
            InsertScheduledIngestionBind::NullableJson(value) => Ok(nullable_json(value)),
        })
        .collect()
}

/// Map the checkpoint read's source bigint bind.
fn checkpoint_read_parameters(
    binds: &[GetScheduledCheckpointBind; 1],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            GetScheduledCheckpointBind::BigintText(value) => bigint(value, "source id"),
        })
        .collect()
}

/// Map the scheduled running-row identity plan in placeholder order.
fn scheduled_identity_parameters(
    binds: &[ScheduledRunIdentityBind; 4],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            ScheduledRunIdentityBind::BigintText(value) => bigint(value, "run or source id"),
            ScheduledRunIdentityBind::UtcInstant(value) => timestamp(value),
        })
        .collect()
}

/// Map every normalized weather-record bind without positional type guessing.
fn weather_record_parameters(
    binds: &[WeatherRecordBind],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            WeatherRecordBind::BigintText(value) => bigint(value, "weather record id"),
            WeatherRecordBind::Text(value) => Ok(Parameter::Text(value.clone())),
            WeatherRecordBind::NullableText(value) => Ok(nullable_text(value)),
            WeatherRecordBind::SourceKind(value) => Ok(Parameter::Text(wire_value(
                *value,
                SourceKind::WIRE_VALUES,
                "source kind",
            )?)),
            WeatherRecordBind::UtcInstant(value) => timestamp(value),
            WeatherRecordBind::NullableUtcInstant(value) => Ok(nullable_timestamp(value)),
            WeatherRecordBind::NullableJson(value) => Ok(nullable_json(value)),
            WeatherRecordBind::NullableNumber(Some(value)) => Ok(Parameter::F64(*value)),
            WeatherRecordBind::NullableNumber(None) => Ok(Parameter::Null(NativePostgresType::F64)),
            WeatherRecordBind::ContentHash(value) => Ok(Parameter::Text(value.as_str().to_owned())),
        })
        .collect()
}

/// Map the initial checkpoint insert in placeholder order.
fn initial_checkpoint_parameters(
    binds: &[InitialCheckpointBind; 5],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            InitialCheckpointBind::BigintText(value) => bigint(value, "source id"),
            InitialCheckpointBind::UtcInstant(value) => timestamp(value),
            InitialCheckpointBind::NullableJson(value) => Ok(nullable_json(value)),
        })
        .collect()
}

/// Map the checkpoint compare-and-set update in placeholder order.
fn checkpoint_update_parameters(
    binds: &[CheckpointUpdateBind; 6],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            CheckpointUpdateBind::BigintText(value) => bigint(value, "source id"),
            CheckpointUpdateBind::Version(value) => version(*value),
            CheckpointUpdateBind::UtcInstant(value) => timestamp(value),
            CheckpointUpdateBind::NullableJson(value) => Ok(nullable_json(value)),
        })
        .collect()
}

/// Map successful scheduled-run finalization evidence in placeholder order.
fn finalize_scheduled_parameters(
    binds: &[FinalizeScheduledRunBind; 7],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            FinalizeScheduledRunBind::BigintText(value) => bigint(value, "run or source id"),
            FinalizeScheduledRunBind::Count(value) => count(*value, "completion count"),
            FinalizeScheduledRunBind::NullableJson(value) => Ok(nullable_json(value)),
            FinalizeScheduledRunBind::NullableChecksum(Some(value)) => {
                Ok(Parameter::Text(value.as_str().to_owned()))
            }
            FinalizeScheduledRunBind::NullableChecksum(None) => {
                Ok(Parameter::Null(NativePostgresType::Text))
            }
            FinalizeScheduledRunBind::ScheduledMode => Ok(Parameter::Text("scheduled".to_owned())),
        })
        .collect()
}

/// Map the scheduled running-mode guard in placeholder order.
fn assert_running_parameters(
    binds: &[AssertRunningScheduledBind; 3],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            AssertRunningScheduledBind::BigintText(value) => bigint(value, "run or source id"),
            AssertRunningScheduledBind::ScheduledMode => {
                Ok(Parameter::Text("scheduled".to_owned()))
            }
        })
        .collect()
}

/// Map failed scheduled-run finalization evidence in placeholder order.
fn finalize_failed_parameters(
    binds: &[FinalizeFailedIngestionBind; 7],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            FinalizeFailedIngestionBind::BigintText(value) => bigint(value, "run or source id"),
            FinalizeFailedIngestionBind::Text(value) => Ok(Parameter::Text(value.clone())),
            FinalizeFailedIngestionBind::Count(value) => count(*value, "attempts"),
            FinalizeFailedIngestionBind::NullableJson(value) => Ok(nullable_json(value)),
            FinalizeFailedIngestionBind::Classification(value) => Ok(Parameter::Text(wire_value(
                *value,
                IngestionErrorClassification::WIRE_VALUES,
                "error classification",
            )?)),
        })
        .collect()
}

/// Map one native boundary failure to its stable worker task code.
fn task_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_database::Sha256Digest;

    /// Preserve bigint, timestamp, enum, nullable JSON, and count types without positional inference.
    #[test]
    fn scheduled_plans_map_to_exact_postgres_parameter_types() {
        let parameters = insert_scheduled_parameters(&[
            InsertScheduledIngestionBind::BigintText("41".to_owned()),
            InsertScheduledIngestionBind::ScheduledMode,
            InsertScheduledIngestionBind::UtcInstant("2026-11-24T20:00:00.000Z".to_owned()),
            InsertScheduledIngestionBind::UtcInstant("2026-11-24T21:00:00.000Z".to_owned()),
            InsertScheduledIngestionBind::Fingerprint(Sha256Digest::admitted("a".repeat(64))),
            InsertScheduledIngestionBind::Text("open-meteo/v1".to_owned()),
            InsertScheduledIngestionBind::NullChunkPlanVersion,
            InsertScheduledIngestionBind::UtcInstant("2026-11-24T21:15:00.000Z".to_owned()),
            InsertScheduledIngestionBind::Count(2),
            InsertScheduledIngestionBind::NullableJson(Some(SerializedJson::admitted(
                r#"{"request":"one"}"#,
            ))),
        ])
        .unwrap();
        assert_eq!(
            parameters,
            vec![
                Parameter::I64(41),
                Parameter::Text("scheduled".to_owned()),
                Parameter::Timestamp("2026-11-24T20:00:00.000Z".to_owned()),
                Parameter::Timestamp("2026-11-24T21:00:00.000Z".to_owned()),
                Parameter::Text("a".repeat(64)),
                Parameter::Text("open-meteo/v1".to_owned()),
                Parameter::Null(NativePostgresType::Text),
                Parameter::Timestamp("2026-11-24T21:15:00.000Z".to_owned()),
                Parameter::I32(2),
                Parameter::Json(r#"{"request":"one"}"#.to_owned()),
            ]
        );
    }

    /// Reject predecessor bigint strings and counts that cannot bind to their actual columns.
    #[test]
    fn scheduled_numeric_domains_reject_unrepresentable_values() {
        assert!(bigint("source-1", "source id").is_err());
        assert!(count(u64::MAX, "attempts").is_err());
        assert!(version(u64::MAX).is_err());
    }

    /// Retain typed weather values independently from SQL placeholder position.
    #[test]
    fn weather_record_binds_preserve_domains() {
        let parameters = weather_record_parameters(&[
            WeatherRecordBind::BigintText("7".to_owned()),
            WeatherRecordBind::SourceKind(SourceKind::ModelCurrent),
            WeatherRecordBind::NullableUtcInstant(None),
            WeatherRecordBind::NullableJson(None),
            WeatherRecordBind::NullableNumber(Some(12.5)),
            WeatherRecordBind::ContentHash(Sha256Digest::admitted("b".repeat(64))),
        ])
        .unwrap();
        assert_eq!(
            parameters,
            vec![
                Parameter::I64(7),
                Parameter::Text("model_current".to_owned()),
                Parameter::Null(NativePostgresType::Timestamp),
                Parameter::Null(NativePostgresType::Json),
                Parameter::F64(12.5),
                Parameter::Text("b".repeat(64)),
            ]
        );
    }
}
