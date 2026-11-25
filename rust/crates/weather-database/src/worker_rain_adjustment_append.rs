//! Insert the original serving run and retain its session for optional revision custody.

use weather_contracts::{PersistedRainAdjustmentRevision, RainAdjustmentHour, RainAdjustmentRun};

use crate::SerializedJson;

const INSERT_SQL: &str = r#"
      INSERT INTO rain_adjustment_runs (run_initialized_at, model_sha256, input_sha256,
        forecast_claim_id, first_received_at, decision_at, hours)
      VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)
      ON CONFLICT (run_initialized_at, model_sha256) DO NOTHING
      RETURNING run_initialized_at AS "runInitializedAt", model_sha256 AS "modelSha256",
        input_sha256 AS "inputSha256", generated_at AS "generatedAt", hours,
        encode(sha256(convert_to(hours::text, 'UTF8')), 'hex') AS "storedContentSha256"
"#;

/// Exact insert statement and seven binds; the last is the original hours JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerRainAdjustmentInsertPlan<'a> {
    pub sql: &'static str,
    pub binds: [&'a str; 7],
}

/// Ingest-role client retained until the serving transaction has ended.
pub trait WorkerRainAdjustmentAppendSession {
    type Error;

    /// Begin the transaction before inserting the serving row.
    fn begin(&mut self) -> Result<(), Self::Error>;
    /// Return exact PostgreSQL-authored fields or no row on an ordinary conflict.
    fn insert(
        &mut self,
        plan: WorkerRainAdjustmentInsertPlan<'_>,
    ) -> Result<Option<PersistedRainAdjustmentRevision>, Self::Error>;
    /// Commit the serving row and any attached revision pointer.
    fn commit(&mut self) -> Result<(), Self::Error>;
    /// Restore a failed or abandoned transaction before returning the connection.
    fn rollback(&mut self) -> Result<(), Self::Error>;
    /// Return this client to its existing worker pool.
    fn release(&mut self);
}

/// Check out one client from the existing worker ingest pool.
pub trait WorkerRainAdjustmentAppendPool {
    type Error;
    type Session: WorkerRainAdjustmentAppendSession<Error = Self::Error>;

    /// Acquire the session that owns insertion and the optional revision bind.
    fn checkout(&mut self) -> Result<Self::Session, Self::Error>;
}

/// Approved serializer for the generated TypeScript persistence type.
pub trait WorkerRainAdjustmentAppendValues {
    type Error;

    /// Serialize the hours array for its single JSONB parameter.
    fn encode_hours(&mut self, hours: &[RainAdjustmentHour])
    -> Result<SerializedJson, Self::Error>;
}

/// Distinguish primitive serialization from transaction failures.
#[derive(Debug, PartialEq, Eq)]
pub enum WorkerRainAdjustmentAppendError<DriverError, CodecError> {
    Driver(DriverError),
    Values(CodecError),
}

/// A newly inserted serving revision in its still-open database transaction.
pub struct OpenWorkerRainAdjustment<Session: WorkerRainAdjustmentAppendSession> {
    pub persisted: PersistedRainAdjustmentRevision,
    pub transaction: WorkerRainAdjustmentTransaction<Session>,
}

/// The retained session, separate from the exact result row returned by PostgreSQL.
pub struct WorkerRainAdjustmentTransaction<Session: WorkerRainAdjustmentAppendSession> {
    session: Option<Session>,
}

impl<Session: WorkerRainAdjustmentAppendSession> WorkerRainAdjustmentTransaction<Session> {
    /// Borrow the open session for the existing revision savepoint owner.
    pub fn session_mut(&mut self) -> Option<&mut Session> {
        self.session.as_mut()
    }

    /// Commit then return the client before any post-commit archive work.
    pub fn commit(&mut self) -> Result<(), Session::Error> {
        // an already released handle owns no further database effects
        if let Some(session) = self.session.as_mut() {
            session.commit()?;
            self.release();
        }
        Ok(())
    }

    /// Restore and release the client without replacing the caller's primary failure.
    pub fn rollback(&mut self) {
        // an already released handle owns no transaction to restore
        if let Some(session) = self.session.as_mut() {
            let _ = session.rollback();
            self.release();
        }
    }

    /// Release at most once.
    fn release(&mut self) {
        // take ownership before handing the connection back
        if let Some(mut session) = self.session.take() {
            session.release();
        }
    }
}

impl<Session: WorkerRainAdjustmentAppendSession> Drop for WorkerRainAdjustmentTransaction<Session> {
    /// Restore an uncommitted transaction if its owner exits early.
    fn drop(&mut self) {
        self.rollback();
    }
}

/// A retained transaction for a new row, or no transaction for an ordinary conflict.
pub type WorkerRainAdjustmentAppendResult<Pool, Values> = Result<
    Option<OpenWorkerRainAdjustment<<Pool as WorkerRainAdjustmentAppendPool>::Session>>,
    WorkerRainAdjustmentAppendError<
        <Pool as WorkerRainAdjustmentAppendPool>::Error,
        <Values as WorkerRainAdjustmentAppendValues>::Error,
    >,
>;

/// Insert one serving run; commit and release immediately if it already exists.
pub fn begin_worker_rain_adjustment_append<Pool, Values>(
    pool: &mut Pool,
    values: &mut Values,
    run: &RainAdjustmentRun,
) -> WorkerRainAdjustmentAppendResult<Pool, Values>
where
    Pool: WorkerRainAdjustmentAppendPool,
    Values: WorkerRainAdjustmentAppendValues,
{
    use WorkerRainAdjustmentAppendError::{Driver, Values as Codec};

    let mut session = pool.checkout().map_err(Driver)?;
    let result = (|| {
        session.begin().map_err(Driver)?;
        let json = values.encode_hours(&run.hours).map_err(Codec)?;
        let persisted = session
            .insert(WorkerRainAdjustmentInsertPlan {
                sql: INSERT_SQL,
                binds: [
                    &run.runInitializedAt,
                    &run.modelSha256,
                    &run.inputSha256,
                    &run.forecastClaimId,
                    &run.firstReceivedAt,
                    &run.decisionAt,
                    json.as_str(),
                ],
            })
            .map_err(Driver)?;
        // the conflict is successful only after the original empty transaction commits
        if persisted.is_none() {
            session.commit().map_err(Driver)?;
        }
        Ok(persisted)
    })();
    match result {
        Ok(Some(persisted)) => Ok(Some(OpenWorkerRainAdjustment {
            persisted,
            transaction: WorkerRainAdjustmentTransaction {
                session: Some(session),
            },
        })),
        Ok(None) => {
            session.release();
            Ok(None)
        }
        Err(error) => {
            let rollback = session.rollback();
            session.release();
            // the source transaction propagates a rollback error if recovery itself fails
            rollback.map_err(Driver)?;
            Err(error)
        }
    }
}

#[cfg(test)]
#[path = "worker_rain_adjustment_append_test.rs"]
mod tests;
