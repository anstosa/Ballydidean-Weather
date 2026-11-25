//! Native PostgreSQL execution for the rain-capture claim transaction.

use std::time::SystemTime;

use tokio_postgres::Row;
use weather_database::{
    RainCaptureClaimClockStatement, RainCaptureClaimGateRow, RainCaptureClaimGateStatement,
    RainCaptureClaimInsertStatement, RainCaptureClaimKind, RainCaptureClaimLockStatement,
    RainCaptureClaimPool, RainCaptureClaimSession,
};

use crate::{
    NativePostgresError, NativePostgresParameter as Parameter, NativePostgresPool,
    NativePostgresSession, NativePostgresType, native_postgres_weather_rows as field,
};

/// One retained native connection for the advisory-locked claim transaction.
pub struct NativeRainCaptureClaimSession {
    session: Option<NativePostgresSession>,
}

impl NativeRainCaptureClaimSession {
    /// Retain one checked-out connection until the claim owner releases it.
    fn new(session: NativePostgresSession) -> Self {
        Self {
            session: Some(session),
        }
    }

    /// Borrow the still-retained PostgreSQL connection.
    fn session_mut(&mut self) -> Result<&mut NativePostgresSession, NativePostgresError> {
        self.session
            .as_mut()
            .ok_or(NativePostgresError::SessionReleased)
    }
}

impl RainCaptureClaimPool for NativePostgresPool {
    type Error = NativePostgresError;
    type Session = NativeRainCaptureClaimSession;

    /// Check out one connection retained across the complete claim transaction.
    fn checkout(&mut self) -> Result<Self::Session, Self::Error> {
        NativePostgresPool::checkout(self).map(NativeRainCaptureClaimSession::new)
    }
}

impl RainCaptureClaimSession for NativeRainCaptureClaimSession {
    type Error = NativePostgresError;

    /// Begin the transaction before acquiring its transaction-scoped lock.
    fn begin(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.begin()
    }

    /// Acquire the exact advisory transaction lock and require its single result row.
    fn advisory_lock(
        &mut self,
        statement: &RainCaptureClaimLockStatement,
    ) -> Result<(), Self::Error> {
        let rows = self
            .session_mut()?
            .query(statement.sql, &[Parameter::I64(statement.lock_id)])?;
        require_one_row(&rows, "rain capture advisory lock")
    }

    /// Read the database clock without replacing it with the process clock.
    fn server_clock(
        &mut self,
        statement: &RainCaptureClaimClockStatement,
    ) -> Result<Option<i64>, Self::Error> {
        let rows = self.session_mut()?.query(statement.sql, &[])?;
        optional_single_row(&rows, "rain capture server clock")?
            .map(|row| {
                let time = field::column::<SystemTime>(row, "now")?;
                field::epoch_milliseconds(time)
                    .ok_or_else(|| NativePostgresError::decode("now", "timestamp is out of range"))
            })
            .transpose()
    }

    /// Execute and decode the complete duplicate, quota, storage, and cooldown gate.
    fn claim_gate(
        &mut self,
        statement: &RainCaptureClaimGateStatement,
    ) -> Result<Option<RainCaptureClaimGateRow>, Self::Error> {
        let parameters = claim_gate_parameters(statement);
        let rows = self.session_mut()?.query(statement.sql, &parameters)?;
        optional_single_row(&rows, "rain capture claim gate")?
            .map(decode_claim_gate)
            .transpose()
    }

    /// Insert one immutable claim and decode its native UUID result.
    fn insert_claim(
        &mut self,
        statement: &RainCaptureClaimInsertStatement,
    ) -> Result<Option<String>, Self::Error> {
        let parameters = claim_insert_parameters(statement)?;
        let rows = self.session_mut()?.query(statement.sql, &parameters)?;
        optional_single_row(&rows, "rain capture claim insert")?
            .map(|row| field::id(row, "id"))
            .transpose()
    }

    /// Commit the active claim transaction.
    fn commit(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.commit()
    }

    /// Roll back the active claim transaction after any body or commit failure.
    fn rollback(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.rollback()
    }

    /// Return the retained connection after every admitted or rejected claim.
    fn release(&mut self) {
        // consume the session exactly once because the database trait cannot report cleanup errors
        if let Some(session) = self.session.take() {
            let _ = session.release();
        }
    }
}

/// Convert the gate's closed bind domain to exact PostgreSQL parameter types.
fn claim_gate_parameters(statement: &RainCaptureClaimGateStatement) -> Vec<Parameter> {
    vec![
        Parameter::Text(statement.binds.slot_key.clone()),
        Parameter::Text(claim_kind(statement.binds.kind).to_owned()),
        Parameter::Timestamp(statement.binds.now.clone()),
        Parameter::Timestamp(statement.binds.day_start.clone()),
        optional_timestamp(statement.binds.run_initialized_at.as_ref()),
    ]
}

/// Convert the insert's closed bind domain to exact PostgreSQL parameter types.
fn claim_insert_parameters(
    statement: &RainCaptureClaimInsertStatement,
) -> Result<Vec<Parameter>, NativePostgresError> {
    Ok(vec![
        Parameter::Text(claim_kind(statement.binds.kind).to_owned()),
        Parameter::Text(statement.binds.slot_key.clone()),
        optional_i32(statement.binds.station_id, "station_id")?,
        optional_timestamp(statement.binds.run_initialized_at.as_ref()),
        statement
            .binds
            .attempt
            .map_or(Parameter::Null(NativePostgresType::I32), |attempt| {
                Parameter::I32(i32::from(attempt))
            }),
        optional_timestamp(statement.binds.window_start.as_ref()),
        optional_timestamp(statement.binds.window_end_exclusive.as_ref()),
        Parameter::Text(statement.binds.release.clone()),
        Parameter::Json(statement.binds.policy_json.clone()),
        Parameter::Text(statement.binds.policy_sha256.clone()),
    ])
}

/// Preserve the exact database discriminator for one claim family.
const fn claim_kind(kind: RainCaptureClaimKind) -> &'static str {
    match kind {
        RainCaptureClaimKind::Forecast => "forecast",
        RainCaptureClaimKind::Station => "station",
    }
}

/// Retain nullable canonical UTC instants as typed timestamptz parameters.
fn optional_timestamp(value: Option<&String>) -> Parameter {
    value.map_or(Parameter::Null(NativePostgresType::Timestamp), |value| {
        Parameter::Timestamp(value.clone())
    })
}

/// Bound an optional unsigned domain value to PostgreSQL's signed integer column.
fn optional_i32(value: Option<u64>, name: &'static str) -> Result<Parameter, NativePostgresError> {
    value.map_or(Ok(Parameter::Null(NativePostgresType::I32)), |value| {
        i32::try_from(value)
            .map(Parameter::I32)
            .map_err(|_| NativePostgresError::InvalidParameter(name))
    })
}

/// Decode every gate field from its native PostgreSQL result representation.
fn decode_claim_gate(row: &Row) -> Result<RainCaptureClaimGateRow, NativePostgresError> {
    let latest = field::column::<Option<SystemTime>>(row, "latest")?
        .map(|time| {
            field::epoch_milliseconds(time)
                .ok_or_else(|| NativePostgresError::decode("latest", "timestamp is out of range"))
        })
        .transpose()?;
    Ok(RainCaptureClaimGateRow {
        existing: field::column(row, "existing")?,
        pending: field::column(row, "pending")?,
        latest_ms: latest,
        today: field::unsigned(row, "today")?,
        rolling: field::unsigned(row, "rolling")?,
        storage_day: field::unsigned(row, "storage_day")?,
        storage_total: field::unsigned(row, "storage_total")?,
        paused: field::column(row, "paused")?,
        valid_prior: field::column(row, "valid_prior")?,
    })
}

/// Require one statement whose contract always returns exactly one row.
fn require_one_row(rows: &[Row], label: &'static str) -> Result<(), NativePostgresError> {
    // reject missing or duplicate lock rows instead of assuming lock acquisition
    if rows.len() != 1 {
        return Err(NativePostgresError::decode(
            label,
            "query did not return exactly one row",
        ));
    }
    Ok(())
}

/// Require an optional query contract to return no more than one row.
fn optional_single_row<'a>(
    rows: &'a [Row],
    label: &'static str,
) -> Result<Option<&'a Row>, NativePostgresError> {
    // reject impossible duplicate rows instead of silently choosing one
    if rows.len() > 1 {
        return Err(NativePostgresError::decode(
            label,
            "query returned more than one row",
        ));
    }
    Ok(rows.first())
}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_database::{RainCaptureClaimGateBinds, RainCaptureClaimInsertBinds};

    /// Preserve the exact gate placeholder order and null timestamp type.
    #[test]
    fn maps_claim_gate_parameters() {
        let statement = RainCaptureClaimGateStatement {
            sql: "gate",
            binds: RainCaptureClaimGateBinds {
                slot_key: "forecast:slot".to_owned(),
                kind: RainCaptureClaimKind::Forecast,
                now: "2026-11-24T20:00:00.000Z".to_owned(),
                day_start: "2026-11-24T00:00:00.000Z".to_owned(),
                run_initialized_at: None,
            },
        };
        assert_eq!(
            claim_gate_parameters(&statement),
            vec![
                Parameter::Text("forecast:slot".to_owned()),
                Parameter::Text("forecast".to_owned()),
                Parameter::Timestamp("2026-11-24T20:00:00.000Z".to_owned()),
                Parameter::Timestamp("2026-11-24T00:00:00.000Z".to_owned()),
                Parameter::Null(NativePostgresType::Timestamp),
            ]
        );
    }

    /// Preserve the exact station insert order and PostgreSQL column domains.
    #[test]
    fn maps_claim_insert_parameters() {
        let statement = RainCaptureClaimInsertStatement {
            sql: "insert",
            binds: RainCaptureClaimInsertBinds {
                kind: RainCaptureClaimKind::Station,
                slot_key: "station:64255:slot".to_owned(),
                station_id: Some(64_255),
                run_initialized_at: None,
                attempt: None,
                window_start: Some("2026-11-24T18:00:00.000Z".to_owned()),
                window_end_exclusive: Some("2026-11-24T20:00:00.000Z".to_owned()),
                release: "2026.11.24-1".to_owned(),
                policy_json: "{}".to_owned(),
                policy_sha256: "0".repeat(64),
            },
        };
        assert_eq!(
            claim_insert_parameters(&statement).unwrap(),
            vec![
                Parameter::Text("station".to_owned()),
                Parameter::Text("station:64255:slot".to_owned()),
                Parameter::I32(64_255),
                Parameter::Null(NativePostgresType::Timestamp),
                Parameter::Null(NativePostgresType::I32),
                Parameter::Timestamp("2026-11-24T18:00:00.000Z".to_owned()),
                Parameter::Timestamp("2026-11-24T20:00:00.000Z".to_owned()),
                Parameter::Text("2026.11.24-1".to_owned()),
                Parameter::Json("{}".to_owned()),
                Parameter::Text("0".repeat(64)),
            ]
        );
    }

    /// Reject station identifiers outside the physical integer column range.
    #[test]
    fn rejects_out_of_range_station_identifier() {
        assert!(matches!(
            optional_i32(Some(u64::MAX), "station_id"),
            Err(NativePostgresError::InvalidParameter("station_id"))
        ));
    }
}
