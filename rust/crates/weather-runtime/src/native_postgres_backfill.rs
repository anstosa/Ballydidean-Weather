//! Native PostgreSQL source reads for historical provider backfills.

use tokio_postgres::Row;
use weather_contracts::{IngestionErrorClassification, JsonValue, SourceKind};
use weather_database::{
    AmbientBackfillSourceAssociationBind, AmbientBackfillSourceAssociationQuery,
    AmbientBackfillSourceAssociationRow, AmbientBackfillSourceReadPort,
    ForecastAnchorAbandonExpiredQuery, ForecastAnchorBackfillCommandPort,
    ForecastAnchorBackfillPoolPort, ForecastAnchorFailureBind, ForecastAnchorFailureQuery,
    ForecastAnchorFindRunningBind, ForecastAnchorFindRunningQuery, ForecastAnchorIdentityBind,
    ForecastAnchorOutcome, ForecastAnchorOutcomeBind, ForecastAnchorOutcomeQuery,
    ForecastAnchorRecordBind, ForecastAnchorRecordUpsertQuery, ForecastAnchorRunIdentityQuery,
    ForecastAnchorRunStartBind, ForecastAnchorRunStartQuery, ForecastAnchorRunningLookup,
    ForecastAnchorSourceKindBind, ForecastAnchorSuccessBind, ForecastAnchorSuccessQuery,
    ForecastAnchorSuccessfulChunkQuery, OpenMeteoBackfillSourceBind, OpenMeteoBackfillSourceQuery,
    OpenMeteoBackfillSourceReadPort, OpenMeteoBackfillSourceRow, RetainedSourceSession,
    SerializedJson, StartedForecastAnchorBackfillRun, StartedTideBackfillRun,
    TideBackfillAbandonExpiredQuery, TideBackfillCommandPort, TideBackfillFailureBind,
    TideBackfillFailureQuery, TideBackfillFindRunningBind, TideBackfillFindRunningQuery,
    TideBackfillIdentityBind, TideBackfillOutcome, TideBackfillOutcomeBind,
    TideBackfillOutcomeQuery, TideBackfillPoolPort, TideBackfillRunIdentityQuery,
    TideBackfillRunStartBind, TideBackfillRunStartQuery, TideBackfillRunningLookup,
    TideBackfillSuccessBind, TideBackfillSuccessQuery, TideBackfillSuccessfulChunkQuery,
    WeatherRecordBind, WeatherRecordUpsertQuery,
};

use crate::{
    NativePostgresError, NativePostgresParameter as Parameter, NativePostgresPool,
    NativePostgresSession, NativePostgresType, native_postgres_weather_rows as field,
};

const ACQUIRE_SOURCE_SESSION_SQL: &str =
    "SELECT pg_try_advisory_lock(hashtextextended('weather-source:' || $1, 0)) AS acquired";
const RELEASE_SOURCE_SESSION_SQL: &str =
    "SELECT pg_advisory_unlock(hashtextextended('weather-source:' || $1, 0)) AS released";

/// One native connection retaining its historical source lock until explicit release.
pub struct NativePostgresBackfillSession {
    session: Option<NativePostgresSession>,
    source_id: String,
}

impl NativePostgresBackfillSession {
    /// Retain one successfully acquired advisory lock and its checked-out connection.
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
    fn release_owned(mut self) -> Result<(), NativePostgresError> {
        let session = self
            .session
            .take()
            .ok_or(NativePostgresError::SessionReleased)?;
        unlock_source_session(session, &self.source_id)
    }
}

impl Drop for NativePostgresBackfillSession {
    /// Close or unlock every retained source-lock connection on abandoned ownership paths.
    fn drop(&mut self) {
        // Preserve pool safety even when a caller omits explicit release.
        if let Some(session) = self.session.take() {
            let _ = unlock_source_session(session, &self.source_id);
        }
    }
}

impl RetainedSourceSession for NativePostgresBackfillSession {
    /// Return the immutable source identity associated with the advisory lock.
    fn source_id(&self) -> &str {
        &self.source_id
    }
}

impl AmbientBackfillSourceReadPort for NativePostgresPool {
    type Error = NativePostgresError;

    /// Execute the exact active Ambient source-association query on the shared pool.
    fn read_ambient_backfill_sources(
        &mut self,
        query: &AmbientBackfillSourceAssociationQuery,
    ) -> Result<Vec<AmbientBackfillSourceAssociationRow>, Self::Error> {
        let parameters = ambient_parameters(&query.binds)?;
        query_pool(self, query.sql, &parameters)?
            .iter()
            .map(decode_ambient_source)
            .collect()
    }
}

impl OpenMeteoBackfillSourceReadPort for NativePostgresPool {
    type Error = NativePostgresError;

    /// Execute the exact active Open-Meteo source lookup on the shared pool.
    fn read_open_meteo_backfill_source(
        &mut self,
        query: &OpenMeteoBackfillSourceQuery,
    ) -> Result<Option<OpenMeteoBackfillSourceRow>, Self::Error> {
        let parameters = open_meteo_parameters(&query.binds)?;
        let rows = query_pool(self, query.sql, &parameters)?;
        rows.first().map(decode_open_meteo_source).transpose()
    }
}

impl TideBackfillPoolPort for NativePostgresPool {
    type Error = NativePostgresError;
    type Session = NativePostgresBackfillSession;

    /// Execute the exact successful tide-chunk lookup on a short-lived pooled session.
    fn has_successful_chunk(
        &mut self,
        query: &TideBackfillSuccessfulChunkQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = tide_successful_chunk_parameters(&query.binds)?;
        Ok(query_pool(self, query.sql, &parameters)?.len())
    }

    /// Acquire one source-scoped retained PostgreSQL session.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, Self::Error> {
        acquire_source_session(self, source_id)
    }
}

impl ForecastAnchorBackfillPoolPort for NativePostgresPool {
    type Error = NativePostgresError;
    type Session = NativePostgresBackfillSession;

    /// Execute the exact successful forecast-anchor chunk lookup on the shared pool.
    fn has_successful_chunk(
        &mut self,
        query: &ForecastAnchorSuccessfulChunkQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = forecast_successful_chunk_parameters(&query.binds)?;
        Ok(query_pool(self, query.sql, &parameters)?.len())
    }

    /// Acquire one source-scoped retained PostgreSQL session.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, Self::Error> {
        acquire_source_session(self, source_id)
    }
}

impl TideBackfillCommandPort for NativePostgresBackfillSession {
    type Error = NativePostgresError;

    /// Recover expired tide rows and decode every returned bigint identity.
    fn abandon_expired_runs(
        &mut self,
        query: &TideBackfillAbandonExpiredQuery,
    ) -> Result<Vec<String>, Self::Error> {
        let parameters = tide_abandon_parameters(&query.binds)?;
        self.session_mut()?
            .query(query.sql, &parameters)?
            .iter()
            .map(|row| field::id(row, "id"))
            .collect()
    }

    /// Read every current running row while retaining the first exact identity.
    fn find_running_run(
        &mut self,
        query: &TideBackfillFindRunningQuery,
    ) -> Result<TideBackfillRunningLookup, Self::Error> {
        let parameters = tide_find_running_parameters(&query.binds)?;
        let rows = self.session_mut()?.query(query.sql, &parameters)?;
        let ids = rows
            .iter()
            .map(|row| field::id(row, "id"))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(TideBackfillRunningLookup {
            first_run_id: ids.first().cloned(),
            row_count: ids.len(),
        })
    }

    /// Insert one running tide row and decode its mandatory returned values.
    fn insert_running_run(
        &mut self,
        query: &TideBackfillRunStartQuery,
    ) -> Result<Option<StartedTideBackfillRun>, Self::Error> {
        let parameters = tide_run_start_parameters(&query.binds)?;
        let rows = self.session_mut()?.query(query.sql, &parameters)?;
        optional_single_row(&rows, "tide running-row insert")?
            .map(decode_started_tide_run)
            .transpose()
    }

    /// Begin the source-owned tide completion or failure transaction.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.begin()
    }

    /// Execute the exact tide committed-run identity guard.
    fn assert_backfill_run(
        &mut self,
        query: &TideBackfillRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = tide_run_identity_parameters(&query.binds)?;
        Ok(self.session_mut()?.query(query.sql, &parameters)?.len())
    }

    /// Upsert one bounded normalized tide weather-record batch.
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        let parameters = weather_record_parameters(&query.binds)?;
        self.session_mut()?.execute(&query.sql, &parameters)?;
        Ok(())
    }

    /// Upsert one successful or failed tide chunk outcome.
    fn upsert_backfill_outcome(
        &mut self,
        query: &TideBackfillOutcomeQuery,
    ) -> Result<(), Self::Error> {
        let parameters = tide_outcome_parameters(&query.binds)?;
        self.session_mut()?.execute(query.sql, &parameters)?;
        Ok(())
    }

    /// Guard-finalize one successful tide running row.
    fn finalize_successful_run(
        &mut self,
        query: &TideBackfillSuccessQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = tide_success_parameters(&query.binds)?;
        let affected = self.session_mut()?.execute(query.sql, &parameters)?;
        row_count(affected, "successful tide finalization")
    }

    /// Guard-finalize one failed tide running row.
    fn finalize_failed_run(
        &mut self,
        query: &TideBackfillFailureQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = tide_failure_parameters(&query.binds)?;
        let affected = self.session_mut()?.execute(query.sql, &parameters)?;
        row_count(affected, "failed tide finalization")
    }

    /// Commit the complete tide transaction.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.commit()
    }

    /// Roll back an incomplete tide transaction.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.rollback()
    }

    /// Unlock and return the retained source session to the pool.
    fn release(self) -> Result<(), Self::Error> {
        self.release_owned()
    }
}

impl ForecastAnchorBackfillCommandPort for NativePostgresBackfillSession {
    type Error = NativePostgresError;

    /// Recover expired anchor rows and decode every returned bigint identity.
    fn abandon_expired_runs(
        &mut self,
        query: &ForecastAnchorAbandonExpiredQuery,
    ) -> Result<Vec<String>, Self::Error> {
        let parameters = forecast_abandon_parameters(&query.binds)?;
        self.session_mut()?
            .query(query.sql, &parameters)?
            .iter()
            .map(|row| field::id(row, "id"))
            .collect()
    }

    /// Read every current anchor running row while retaining the first exact identity.
    fn find_running_run(
        &mut self,
        query: &ForecastAnchorFindRunningQuery,
    ) -> Result<ForecastAnchorRunningLookup, Self::Error> {
        let parameters = forecast_find_running_parameters(&query.binds)?;
        let rows = self.session_mut()?.query(query.sql, &parameters)?;
        let ids = rows
            .iter()
            .map(|row| field::id(row, "id"))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ForecastAnchorRunningLookup {
            first_run_id: ids.first().cloned(),
            row_count: ids.len(),
        })
    }

    /// Insert one running anchor row and decode its mandatory returned values.
    fn insert_running_run(
        &mut self,
        query: &ForecastAnchorRunStartQuery,
    ) -> Result<Option<StartedForecastAnchorBackfillRun>, Self::Error> {
        let parameters = forecast_run_start_parameters(&query.binds)?;
        let rows = self.session_mut()?.query(query.sql, &parameters)?;
        optional_single_row(&rows, "forecast-anchor running-row insert")?
            .map(decode_started_forecast_run)
            .transpose()
    }

    /// Begin the source-owned forecast-anchor completion or failure transaction.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.begin()
    }

    /// Execute the exact forecast-anchor committed-run identity guard.
    fn assert_backfill_run(
        &mut self,
        query: &ForecastAnchorRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = forecast_run_identity_parameters(&query.binds)?;
        Ok(self.session_mut()?.query(query.sql, &parameters)?.len())
    }

    /// Upsert one bounded forecast-anchor record batch.
    fn upsert_forecast_anchor_records(
        &mut self,
        query: &ForecastAnchorRecordUpsertQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = forecast_record_parameters(&query.binds)?;
        let affected = self.session_mut()?.execute(&query.sql, &parameters)?;
        row_count(affected, "forecast-anchor record upsert")
    }

    /// Upsert one successful or failed forecast-anchor chunk outcome.
    fn upsert_backfill_outcome(
        &mut self,
        query: &ForecastAnchorOutcomeQuery,
    ) -> Result<(), Self::Error> {
        let parameters = forecast_outcome_parameters(&query.binds)?;
        self.session_mut()?.execute(query.sql, &parameters)?;
        Ok(())
    }

    /// Guard-finalize one successful forecast-anchor running row.
    fn finalize_successful_run(
        &mut self,
        query: &ForecastAnchorSuccessQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = forecast_success_parameters(&query.binds)?;
        let affected = self.session_mut()?.execute(query.sql, &parameters)?;
        row_count(affected, "successful forecast-anchor finalization")
    }

    /// Guard-finalize one failed forecast-anchor running row.
    fn finalize_failed_run(
        &mut self,
        query: &ForecastAnchorFailureQuery,
    ) -> Result<usize, Self::Error> {
        let parameters = forecast_failure_parameters(&query.binds)?;
        let affected = self.session_mut()?.execute(query.sql, &parameters)?;
        row_count(affected, "failed forecast-anchor finalization")
    }

    /// Commit the complete forecast-anchor transaction.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.commit()
    }

    /// Roll back an incomplete forecast-anchor transaction.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.rollback()
    }

    /// Unlock and return the retained source session to the pool.
    fn release(self) -> Result<(), Self::Error> {
        self.release_owned()
    }
}

/// Acquire one source advisory lock without recycling an ambiguously locked connection.
fn acquire_source_session(
    pool: &NativePostgresPool,
    source_id: &str,
) -> Result<Option<NativePostgresBackfillSession>, NativePostgresError> {
    let mut session = pool.checkout()?;
    let result = session.query(
        ACQUIRE_SOURCE_SESSION_SQL,
        &[Parameter::Text(source_id.to_owned())],
    );
    // Discard on query or decode uncertainty because the server may hold the session lock.
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
    // Release an unclaimed connection immediately.
    if !acquired {
        session.release()?;
        return Ok(None);
    }
    Ok(Some(NativePostgresBackfillSession::new(
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
    // Close the connection when unlock execution itself is uncertain.
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
    // Surface an ownership violation after the now-safe connection is returned.
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

/// Decode one exact tide running-row insert result.
fn decode_started_tide_run(row: &Row) -> Result<StartedTideBackfillRun, NativePostgresError> {
    Ok(StartedTideBackfillRun {
        id: field::id(row, "id")?,
        started_at: field::utc(row, "startedAt")?,
    })
}

/// Decode one exact forecast-anchor running-row insert result.
fn decode_started_forecast_run(
    row: &Row,
) -> Result<StartedForecastAnchorBackfillRun, NativePostgresError> {
    Ok(StartedForecastAnchorBackfillRun {
        id: field::id(row, "id")?,
        started_at: field::utc(row, "startedAt")?,
    })
}

/// Require exactly one row for a mandatory singleton query contract.
fn required_single_row<'a>(rows: &'a [Row], label: &str) -> Result<&'a Row, NativePostgresError> {
    // Reject both missing and duplicated advisory-lock receipts.
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
    // Reject duplicate insert receipts rather than silently choosing one.
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

/// Bind one generated bigint string to PostgreSQL int8.
fn bigint(value: &str, label: &str) -> Result<Parameter, NativePostgresError> {
    value
        .parse::<i64>()
        .map(Parameter::I64)
        .map_err(|error| NativePostgresError::decode(label, error))
}

/// Bind one safe repository count to its actual PostgreSQL integer column.
fn count(value: u64, label: &str) -> Result<Parameter, NativePostgresError> {
    i32::try_from(value)
        .map(Parameter::I32)
        .map_err(|error| NativePostgresError::decode(label, error))
}

/// Bind one fixed forecast lead to its actual PostgreSQL smallint column.
fn lead_hours(value: u64) -> Result<Parameter, NativePostgresError> {
    i16::try_from(value)
        .map(Parameter::I16)
        .map_err(|error| NativePostgresError::decode("forecast lead hours", error))
}

/// Bind one admitted UTC instant through the native timestamp owner.
fn timestamp(value: &str) -> Parameter {
    Parameter::Timestamp(value.to_owned())
}

/// Bind one source-admitted civil label through the driver's typed DATE owner.
fn date(value: &str) -> Parameter {
    Parameter::Date(value.to_owned())
}

/// Bind nullable serialized JSON without changing producer member order.
fn nullable_json(value: &Option<SerializedJson>) -> Parameter {
    // Preserve SQL null independently from a serialized JSON value.
    match value {
        Some(value) => Parameter::Json(value.as_str().to_owned()),
        None => Parameter::Null(NativePostgresType::Json),
    }
}

/// Bind nullable text to the actual PostgreSQL text parameter type.
fn nullable_text(value: &Option<String>) -> Parameter {
    // Preserve SQL null independently from an empty string.
    match value {
        Some(value) => Parameter::Text(value.clone()),
        None => Parameter::Null(NativePostgresType::Text),
    }
}

/// Bind one optional checksum to the schema's fixed textual digest column.
fn nullable_checksum(value: Option<&str>) -> Parameter {
    // Preserve SQL null independently from a present checksum.
    match value {
        Some(value) => Parameter::Text(value.to_owned()),
        None => Parameter::Null(NativePostgresType::Text),
    }
}

/// Bind one optional floating-point measurement without sentinel substitution.
fn nullable_number(value: Option<f64>) -> Parameter {
    // Preserve SQL null independently from a finite numeric zero.
    match value {
        Some(value) => Parameter::F64(value),
        None => Parameter::Null(NativePostgresType::F64),
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

/// Report one plan whose typed bind variants no longer match its fixed SQL placeholders.
fn bind_shape(label: &str) -> NativePostgresError {
    NativePostgresError::decode(label, "unexpected parameter shape")
}

/// Map the tide successful-chunk identity to its six exact PostgreSQL domains.
fn tide_successful_chunk_parameters(
    binds: &[TideBackfillIdentityBind; 6],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        TideBackfillIdentityBind::Text(source_id),
        TideBackfillIdentityBind::UtcInstant(interval_start),
        TideBackfillIdentityBind::UtcInstant(interval_end),
        TideBackfillIdentityBind::Fingerprint(fingerprint),
        TideBackfillIdentityBind::Text(adapter_version),
        TideBackfillIdentityBind::Text(chunk_plan_version),
    ] = binds
    else {
        return Err(bind_shape("tide successful-chunk binds"));
    };
    Ok(vec![
        bigint(source_id, "source id")?,
        timestamp(interval_start),
        timestamp(interval_end),
        Parameter::Text(fingerprint.as_str().to_owned()),
        Parameter::Text(adapter_version.clone()),
        Parameter::Text(chunk_plan_version.clone()),
    ])
}

/// Map tide expired-run recovery to source bigint and UTC timestamp domains.
fn tide_abandon_parameters(
    binds: &[TideBackfillIdentityBind; 2],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        TideBackfillIdentityBind::Text(source_id),
        TideBackfillIdentityBind::UtcInstant(as_of),
    ] = binds
    else {
        return Err(bind_shape("tide abandon-expired binds"));
    };
    Ok(vec![bigint(source_id, "source id")?, timestamp(as_of)])
}

/// Map one tide running-row lookup to its source bigint domain.
fn tide_find_running_parameters(
    binds: &[TideBackfillFindRunningBind; 1],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [TideBackfillFindRunningBind::Text(source_id)] = binds;
    Ok(vec![bigint(source_id, "source id")?])
}

/// Map every tide running-row insert bind to its actual PostgreSQL type.
fn tide_run_start_parameters(
    binds: &[TideBackfillRunStartBind; 10],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        TideBackfillRunStartBind::Text(source_id),
        TideBackfillRunStartBind::BackfillMode,
        TideBackfillRunStartBind::UtcInstant(interval_start),
        TideBackfillRunStartBind::UtcInstant(interval_end),
        TideBackfillRunStartBind::Fingerprint(fingerprint),
        TideBackfillRunStartBind::Text(adapter_version),
        TideBackfillRunStartBind::Text(chunk_plan_version),
        TideBackfillRunStartBind::UtcInstant(deadline_at),
        TideBackfillRunStartBind::Count(attempts),
        TideBackfillRunStartBind::NullableJson(metadata),
    ] = binds
    else {
        return Err(bind_shape("tide running-row insert binds"));
    };
    Ok(vec![
        bigint(source_id, "source id")?,
        Parameter::Text("backfill".to_owned()),
        timestamp(interval_start),
        timestamp(interval_end),
        Parameter::Text(fingerprint.as_str().to_owned()),
        Parameter::Text(adapter_version.clone()),
        Parameter::Text(chunk_plan_version.clone()),
        timestamp(deadline_at),
        count(*attempts, "attempts")?,
        nullable_json(metadata),
    ])
}

/// Map the committed tide run guard in exact placeholder order.
fn tide_run_identity_parameters(
    binds: &[TideBackfillIdentityBind; 7],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        TideBackfillIdentityBind::Text(run_id),
        TideBackfillIdentityBind::Text(source_id),
        TideBackfillIdentityBind::UtcInstant(interval_start),
        TideBackfillIdentityBind::UtcInstant(interval_end),
        TideBackfillIdentityBind::Fingerprint(fingerprint),
        TideBackfillIdentityBind::Text(adapter_version),
        TideBackfillIdentityBind::Text(chunk_plan_version),
    ] = binds
    else {
        return Err(bind_shape("tide run-identity binds"));
    };
    Ok(vec![
        bigint(run_id, "run id")?,
        bigint(source_id, "source id")?,
        timestamp(interval_start),
        timestamp(interval_end),
        Parameter::Text(fingerprint.as_str().to_owned()),
        Parameter::Text(adapter_version.clone()),
        Parameter::Text(chunk_plan_version.clone()),
    ])
}

/// Map one tide chunk outcome including its exact typed PostgreSQL dates.
fn tide_outcome_parameters(
    binds: &[TideBackfillOutcomeBind; 11],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        TideBackfillOutcomeBind::Text(source_id),
        TideBackfillOutcomeBind::UtcInstant(interval_start),
        TideBackfillOutcomeBind::UtcInstant(interval_end),
        TideBackfillOutcomeBind::Fingerprint(fingerprint),
        TideBackfillOutcomeBind::Text(adapter_version),
        TideBackfillOutcomeBind::Text(chunk_plan_version),
        TideBackfillOutcomeBind::Text(requested_from),
        TideBackfillOutcomeBind::Text(requested_to),
        TideBackfillOutcomeBind::Text(run_id),
        TideBackfillOutcomeBind::Outcome(outcome),
        TideBackfillOutcomeBind::NullableText(error_code),
    ] = binds
    else {
        return Err(bind_shape("tide outcome binds"));
    };
    let outcome = match outcome {
        TideBackfillOutcome::Succeeded => "succeeded",
        TideBackfillOutcome::Failed => "failed",
    };
    Ok(vec![
        bigint(source_id, "source id")?,
        timestamp(interval_start),
        timestamp(interval_end),
        Parameter::Text(fingerprint.as_str().to_owned()),
        Parameter::Text(adapter_version.clone()),
        Parameter::Text(chunk_plan_version.clone()),
        date(requested_from),
        date(requested_to),
        bigint(run_id, "run id")?,
        Parameter::Text(outcome.to_owned()),
        nullable_text(error_code),
    ])
}

/// Map successful tide finalization evidence in exact placeholder order.
fn tide_success_parameters(
    binds: &[TideBackfillSuccessBind; 7],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        TideBackfillSuccessBind::Text(run_id),
        TideBackfillSuccessBind::Text(source_id),
        TideBackfillSuccessBind::Count(attempts),
        TideBackfillSuccessBind::Count(record_count),
        TideBackfillSuccessBind::NullableJson(metadata),
        TideBackfillSuccessBind::NullableChecksum(checksum),
        TideBackfillSuccessBind::BackfillMode,
    ] = binds
    else {
        return Err(bind_shape("tide success binds"));
    };
    Ok(vec![
        bigint(run_id, "run id")?,
        bigint(source_id, "source id")?,
        count(*attempts, "attempts")?,
        count(*record_count, "record count")?,
        nullable_json(metadata),
        nullable_checksum(checksum.as_ref().map(|value| value.as_str())),
        Parameter::Text("backfill".to_owned()),
    ])
}

/// Map failed tide finalization evidence in exact placeholder order.
fn tide_failure_parameters(
    binds: &[TideBackfillFailureBind; 7],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        TideBackfillFailureBind::Text(run_id),
        TideBackfillFailureBind::Text(source_id),
        TideBackfillFailureBind::Count(attempts),
        TideBackfillFailureBind::NullJson,
        TideBackfillFailureBind::Classification(classification),
        TideBackfillFailureBind::Text(code),
        TideBackfillFailureBind::Text(message),
    ] = binds
    else {
        return Err(bind_shape("tide failure binds"));
    };
    Ok(vec![
        bigint(run_id, "run id")?,
        bigint(source_id, "source id")?,
        count(*attempts, "attempts")?,
        Parameter::Null(NativePostgresType::Json),
        Parameter::Text(wire_value(
            *classification,
            IngestionErrorClassification::WIRE_VALUES,
            "error classification",
        )?),
        Parameter::Text(code.clone()),
        Parameter::Text(message.clone()),
    ])
}

/// Map one bounded normalized weather-record query without positional type inference.
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
            WeatherRecordBind::UtcInstant(value) => Ok(timestamp(value)),
            WeatherRecordBind::NullableUtcInstant(Some(value)) => Ok(timestamp(value)),
            WeatherRecordBind::NullableUtcInstant(None) => {
                Ok(Parameter::Null(NativePostgresType::Timestamp))
            }
            WeatherRecordBind::NullableJson(value) => Ok(nullable_json(value)),
            WeatherRecordBind::NullableNumber(value) => Ok(nullable_number(*value)),
            WeatherRecordBind::ContentHash(value) => Ok(Parameter::Text(value.as_str().to_owned())),
        })
        .collect()
}

/// Map the forecast successful-chunk identity to its six exact PostgreSQL domains.
fn forecast_successful_chunk_parameters(
    binds: &[ForecastAnchorIdentityBind; 6],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        ForecastAnchorIdentityBind::Text(source_id),
        ForecastAnchorIdentityBind::UtcInstant(interval_start),
        ForecastAnchorIdentityBind::UtcInstant(interval_end),
        ForecastAnchorIdentityBind::Fingerprint(fingerprint),
        ForecastAnchorIdentityBind::Text(adapter_version),
        ForecastAnchorIdentityBind::Text(chunk_plan_version),
    ] = binds
    else {
        return Err(bind_shape("forecast successful-chunk binds"));
    };
    Ok(vec![
        bigint(source_id, "source id")?,
        timestamp(interval_start),
        timestamp(interval_end),
        Parameter::Text(fingerprint.as_str().to_owned()),
        Parameter::Text(adapter_version.clone()),
        Parameter::Text(chunk_plan_version.clone()),
    ])
}

/// Map forecast expired-run recovery to source bigint and UTC timestamp domains.
fn forecast_abandon_parameters(
    binds: &[ForecastAnchorIdentityBind; 2],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        ForecastAnchorIdentityBind::Text(source_id),
        ForecastAnchorIdentityBind::UtcInstant(as_of),
    ] = binds
    else {
        return Err(bind_shape("forecast abandon-expired binds"));
    };
    Ok(vec![bigint(source_id, "source id")?, timestamp(as_of)])
}

/// Map one forecast running-row lookup to its source bigint domain.
fn forecast_find_running_parameters(
    binds: &[ForecastAnchorFindRunningBind; 1],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [ForecastAnchorFindRunningBind::Text(source_id)] = binds;
    Ok(vec![bigint(source_id, "source id")?])
}

/// Map every forecast running-row insert bind to its actual PostgreSQL type.
fn forecast_run_start_parameters(
    binds: &[ForecastAnchorRunStartBind; 10],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        ForecastAnchorRunStartBind::Text(source_id),
        ForecastAnchorRunStartBind::BackfillMode,
        ForecastAnchorRunStartBind::UtcInstant(interval_start),
        ForecastAnchorRunStartBind::UtcInstant(interval_end),
        ForecastAnchorRunStartBind::Fingerprint(fingerprint),
        ForecastAnchorRunStartBind::Text(adapter_version),
        ForecastAnchorRunStartBind::Text(chunk_plan_version),
        ForecastAnchorRunStartBind::UtcInstant(deadline_at),
        ForecastAnchorRunStartBind::Count(attempts),
        ForecastAnchorRunStartBind::NullableJson(metadata),
    ] = binds
    else {
        return Err(bind_shape("forecast running-row insert binds"));
    };
    Ok(vec![
        bigint(source_id, "source id")?,
        Parameter::Text("backfill".to_owned()),
        timestamp(interval_start),
        timestamp(interval_end),
        Parameter::Text(fingerprint.as_str().to_owned()),
        Parameter::Text(adapter_version.clone()),
        Parameter::Text(chunk_plan_version.clone()),
        timestamp(deadline_at),
        count(*attempts, "attempts")?,
        nullable_json(metadata),
    ])
}

/// Map the committed forecast run guard in exact placeholder order.
fn forecast_run_identity_parameters(
    binds: &[ForecastAnchorIdentityBind; 7],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        ForecastAnchorIdentityBind::Text(run_id),
        ForecastAnchorIdentityBind::Text(source_id),
        ForecastAnchorIdentityBind::UtcInstant(interval_start),
        ForecastAnchorIdentityBind::UtcInstant(interval_end),
        ForecastAnchorIdentityBind::Fingerprint(fingerprint),
        ForecastAnchorIdentityBind::Text(adapter_version),
        ForecastAnchorIdentityBind::Text(chunk_plan_version),
    ] = binds
    else {
        return Err(bind_shape("forecast run-identity binds"));
    };
    Ok(vec![
        bigint(run_id, "run id")?,
        bigint(source_id, "source id")?,
        timestamp(interval_start),
        timestamp(interval_end),
        Parameter::Text(fingerprint.as_str().to_owned()),
        Parameter::Text(adapter_version.clone()),
        Parameter::Text(chunk_plan_version.clone()),
    ])
}

/// Map one bounded forecast-anchor record batch with exact 24-bind group domains.
fn forecast_record_parameters(
    binds: &[ForecastAnchorRecordBind],
) -> Result<Vec<Parameter>, NativePostgresError> {
    // Refuse a truncated or merged row group before positional Text disambiguation.
    if !binds.len().is_multiple_of(24) {
        return Err(bind_shape("forecast record binds"));
    }
    let mut parameters = Vec::with_capacity(binds.len());
    // Map each closed 24-bind group using the schema's exact column position.
    for (index, bind) in binds.iter().enumerate() {
        let position = index % 24;
        let parameter = match (position, bind) {
            (0, ForecastAnchorRecordBind::Text(value)) => bigint(value, "source id")?,
            (1, ForecastAnchorRecordBind::SourceKind(ForecastAnchorSourceKindBind::Forecast)) => {
                Parameter::Text("forecast".to_owned())
            }
            (2, ForecastAnchorRecordBind::Fingerprint(value)) => {
                Parameter::Text(value.as_str().to_owned())
            }
            (3 | 10, ForecastAnchorRecordBind::UtcInstant(value)) => timestamp(value),
            (4, ForecastAnchorRecordBind::LeadHours(value)) => lead_hours(*value)?,
            (5..=8 | 11, ForecastAnchorRecordBind::Text(value)) => Parameter::Text(value.clone()),
            (9, ForecastAnchorRecordBind::Text(value)) => bigint(value, "run id")?,
            (12 | 13, ForecastAnchorRecordBind::NullableJson(value)) => nullable_json(value),
            (14..=22, ForecastAnchorRecordBind::NullableNumber(value)) => nullable_number(*value),
            (23, ForecastAnchorRecordBind::ContentHash(value)) => {
                Parameter::Text(value.as_str().to_owned())
            }
            _ => return Err(bind_shape("forecast record binds")),
        };
        parameters.push(parameter);
    }
    Ok(parameters)
}

/// Map one forecast chunk outcome including its exact typed PostgreSQL dates.
fn forecast_outcome_parameters(
    binds: &[ForecastAnchorOutcomeBind; 11],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        ForecastAnchorOutcomeBind::Text(source_id),
        ForecastAnchorOutcomeBind::UtcInstant(interval_start),
        ForecastAnchorOutcomeBind::UtcInstant(interval_end),
        ForecastAnchorOutcomeBind::Fingerprint(fingerprint),
        ForecastAnchorOutcomeBind::Text(adapter_version),
        ForecastAnchorOutcomeBind::Text(chunk_plan_version),
        ForecastAnchorOutcomeBind::Text(requested_from),
        ForecastAnchorOutcomeBind::Text(requested_to),
        ForecastAnchorOutcomeBind::Text(run_id),
        ForecastAnchorOutcomeBind::Outcome(outcome),
        ForecastAnchorOutcomeBind::NullableText(error_code),
    ] = binds
    else {
        return Err(bind_shape("forecast outcome binds"));
    };
    let outcome = match outcome {
        ForecastAnchorOutcome::Succeeded => "succeeded",
        ForecastAnchorOutcome::Failed => "failed",
    };
    Ok(vec![
        bigint(source_id, "source id")?,
        timestamp(interval_start),
        timestamp(interval_end),
        Parameter::Text(fingerprint.as_str().to_owned()),
        Parameter::Text(adapter_version.clone()),
        Parameter::Text(chunk_plan_version.clone()),
        date(requested_from),
        date(requested_to),
        bigint(run_id, "run id")?,
        Parameter::Text(outcome.to_owned()),
        nullable_text(error_code),
    ])
}

/// Map successful forecast finalization evidence in exact placeholder order.
fn forecast_success_parameters(
    binds: &[ForecastAnchorSuccessBind; 7],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        ForecastAnchorSuccessBind::Text(run_id),
        ForecastAnchorSuccessBind::Text(source_id),
        ForecastAnchorSuccessBind::Count(attempts),
        ForecastAnchorSuccessBind::Count(record_count),
        ForecastAnchorSuccessBind::NullableJson(metadata),
        ForecastAnchorSuccessBind::NullableChecksum(checksum),
        ForecastAnchorSuccessBind::BackfillMode,
    ] = binds
    else {
        return Err(bind_shape("forecast success binds"));
    };
    Ok(vec![
        bigint(run_id, "run id")?,
        bigint(source_id, "source id")?,
        count(*attempts, "attempts")?,
        count(*record_count, "record count")?,
        nullable_json(metadata),
        nullable_checksum(checksum.as_ref().map(|value| value.as_str())),
        Parameter::Text("backfill".to_owned()),
    ])
}

/// Map failed forecast finalization evidence in exact placeholder order.
fn forecast_failure_parameters(
    binds: &[ForecastAnchorFailureBind; 7],
) -> Result<Vec<Parameter>, NativePostgresError> {
    let [
        ForecastAnchorFailureBind::Text(run_id),
        ForecastAnchorFailureBind::Text(source_id),
        ForecastAnchorFailureBind::Count(attempts),
        ForecastAnchorFailureBind::NullJson,
        ForecastAnchorFailureBind::Classification(classification),
        ForecastAnchorFailureBind::Text(code),
        ForecastAnchorFailureBind::Text(message),
    ] = binds
    else {
        return Err(bind_shape("forecast failure binds"));
    };
    Ok(vec![
        bigint(run_id, "run id")?,
        bigint(source_id, "source id")?,
        count(*attempts, "attempts")?,
        Parameter::Null(NativePostgresType::Json),
        Parameter::Text(wire_value(
            *classification,
            IngestionErrorClassification::WIRE_VALUES,
            "error classification",
        )?),
        Parameter::Text(code.clone()),
        Parameter::Text(message.clone()),
    ])
}

/// Preserve the Ambient query's exact site-text and source-key-array bind domains.
fn ambient_parameters(
    binds: &[AmbientBackfillSourceAssociationBind; 2],
) -> Result<[Parameter; 2], NativePostgresError> {
    // reject a manually constructed plan with reordered parameter domains
    let [
        AmbientBackfillSourceAssociationBind::Text(site_slug),
        AmbientBackfillSourceAssociationBind::TextArray(source_keys),
    ] = binds
    else {
        return Err(NativePostgresError::decode(
            "ambient backfill source binds",
            "unexpected parameter shape",
        ));
    };
    Ok([
        Parameter::Text(site_slug.clone()),
        Parameter::TextArray(source_keys.clone()),
    ])
}

/// Preserve the Open-Meteo query's two ordered text bind domains.
fn open_meteo_parameters(
    binds: &[OpenMeteoBackfillSourceBind; 2],
) -> Result<[Parameter; 2], NativePostgresError> {
    // retain both text placeholders without reinterpreting source identity
    let [
        OpenMeteoBackfillSourceBind::Text(site_slug),
        OpenMeteoBackfillSourceBind::Text(source_key),
    ] = binds;
    Ok([
        Parameter::Text(site_slug.clone()),
        Parameter::Text(source_key.clone()),
    ])
}

/// Execute one source read and recycle only the driver's classified session state.
fn query_pool(
    pool: &NativePostgresPool,
    sql: &str,
    parameters: &[Parameter],
) -> Result<Vec<Row>, NativePostgresError> {
    let mut session = pool.checkout()?;
    let result = session.query(sql, parameters);
    let release = session.release();
    // retain the statement error while still returning successful sessions to the pool
    match result {
        Ok(rows) => {
            release?;
            Ok(rows)
        }
        Err(error) => {
            let _ = release;
            Err(error)
        }
    }
}

/// Decode one required JSON or JSONB source configuration without losing object order.
fn required_json(row: &Row, name: &str) -> Result<JsonValue, NativePostgresError> {
    field::json(row, name)?.ok_or_else(|| NativePostgresError::decode(name, "unexpected SQL null"))
}

/// Decode one Ambient source association through the generated exact row contract.
fn decode_ambient_source(
    row: &Row,
) -> Result<AmbientBackfillSourceAssociationRow, NativePostgresError> {
    Ok(AmbientBackfillSourceAssociationRow {
        material_provider_config: required_json(row, "materialProviderConfig")?,
        provider_key: field::column(row, "providerKey")?,
        site_slug: field::column(row, "siteSlug")?,
        source_config_fingerprint: field::column(row, "sourceConfigFingerprint")?,
        source_id: field::id(row, "sourceId")?,
        source_key: field::column(row, "sourceKey")?,
        source_kind: field::wire_enum(row, "sourceKind", SourceKind::WIRE_VALUES)?,
        station_slug: field::column(row, "stationSlug")?,
        timezone: field::column(row, "timezone")?,
    })
}

/// Decode one Open-Meteo source through the generated exact row contract.
fn decode_open_meteo_source(row: &Row) -> Result<OpenMeteoBackfillSourceRow, NativePostgresError> {
    Ok(OpenMeteoBackfillSourceRow {
        latitude: field::column(row, "latitude")?,
        longitude: field::column(row, "longitude")?,
        material_provider_config: required_json(row, "materialProviderConfig")?,
        provider_key: field::column(row, "providerKey")?,
        site_slug: field::column(row, "siteSlug")?,
        source_config_fingerprint: field::column(row, "sourceConfigFingerprint")?,
        source_id: field::id(row, "sourceId")?,
        source_key: field::column(row, "sourceKey")?,
        source_kind: field::wire_enum(row, "sourceKind", SourceKind::WIRE_VALUES)?,
        station_slug: field::column(row, "stationSlug")?,
        timezone: field::column(row, "timezone")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin Ambient bind projection to PostgreSQL text and text-array parameters.
    #[test]
    fn ambient_source_bind_domains_are_exact() {
        let parameters = ambient_parameters(&[
            AmbientBackfillSourceAssociationBind::Text("ballydidean".to_owned()),
            AmbientBackfillSourceAssociationBind::TextArray(vec!["ambient-east".to_owned()]),
        ])
        .expect("source binds should be admitted");

        assert_eq!(
            parameters,
            [
                Parameter::Text("ballydidean".to_owned()),
                Parameter::TextArray(vec!["ambient-east".to_owned()]),
            ]
        );
    }

    /// Reject an Ambient plan whose enum variants do not match placeholder order.
    #[test]
    fn ambient_source_bind_domains_reject_reordering() {
        let error = ambient_parameters(&[
            AmbientBackfillSourceAssociationBind::TextArray(vec!["ambient-east".to_owned()]),
            AmbientBackfillSourceAssociationBind::Text("ballydidean".to_owned()),
        ])
        .expect_err("reordered source binds should fail");

        assert!(matches!(error, NativePostgresError::Decode { .. }));
    }

    /// Pin Open-Meteo bind projection to its two ordered PostgreSQL text parameters.
    #[test]
    fn open_meteo_source_bind_domains_are_exact() {
        let parameters = open_meteo_parameters(&[
            OpenMeteoBackfillSourceBind::Text("ballydidean".to_owned()),
            OpenMeteoBackfillSourceBind::Text("open-meteo-reanalysis-v1".to_owned()),
        ])
        .expect("source binds should be admitted");

        assert_eq!(
            parameters,
            [
                Parameter::Text("ballydidean".to_owned()),
                Parameter::Text("open-meteo-reanalysis-v1".to_owned()),
            ]
        );
    }
}

#[cfg(test)]
#[path = "native_postgres_backfill_write_test.rs"]
mod write_tests;
