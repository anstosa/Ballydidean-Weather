//! Route ordered PostgreSQL discovery and heartbeat statements through the retained worker pool.

use std::fmt::{Display, Formatter};

use weather_database::{
    AdmittedRevisionUtcInstant, AppliedMigration,
    CompleteScheduledIngestion as DatabaseScheduledCompletion, DueSourceDiscoveryPlan,
    DueSourceDiscoveryRow, HealthMigrationLedgerReadPlan, PostgresServerVersionReadPlan,
    PostgresServerVersionReadRow, PreparedRainCaptureReceipt, RainCaptureClaimClockStatement,
    RainCaptureClaimGateRow, RainCaptureClaimGateStatement, RainCaptureClaimInsertStatement,
    RainCaptureClaimLockStatement, RainCaptureClaimPool, RainCaptureClaimRow,
    RainCaptureClaimSession, RainCaptureReceiptDatabase, ScheduledIngestionCommandPort,
    WorkerHeartbeatReadPlan, WorkerHeartbeatReadRow, WorkerHeartbeatUpdate,
    WorkerHeartbeatUpdatePlan, WorkerIterationQueryError, discover_due_sources_plan,
    update_worker_heartbeat_plan,
};
use weather_worker::{TaskFailure, WorkerActivity, WorkerDueSource, WorkerHeartbeat};

use crate::{
    OpenMeteoDatabaseBoundary, WorkerBootDatabase, WorkerIterationDatabasePort,
    WorkerScheduledCompletionError,
};

/// Database side of the exact discovery and heartbeat statements.
pub trait WorkerIterationPostgresPort {
    type Error: Display;

    /// Decode the query's ordered rows directly into the generated contract.
    fn query_due_sources(
        &mut self,
        plan: &DueSourceDiscoveryPlan,
    ) -> Result<Vec<DueSourceDiscoveryRow>, Self::Error>;

    /// Persist one complete worker heartbeat with the supplied typed binds.
    fn execute_worker_heartbeat(
        &mut self,
        plan: &WorkerHeartbeatUpdatePlan,
    ) -> Result<(), Self::Error>;
}

/// Approved canonical timestamp owner for iteration persistence.
pub trait WorkerIterationTime {
    type Error: Display;

    /// Format one captured epoch without changing the process clock.
    fn utc_milliseconds(
        &mut self,
        epoch_milliseconds: i64,
    ) -> Result<AdmittedRevisionUtcInstant, Self::Error>;
}

/// Retain a source's exact runtime selection without querying or skipping it silently.
pub trait WorkerDueSourceProjection {
    /// Distinguish an admitted task, an ordinary unconfigured source, and a source-local failure.
    fn project_due_source(
        &mut self,
        row: &DueSourceDiscoveryRow,
    ) -> Result<Option<WorkerDueSource>, TaskFailure>;
}

/// Typed runtime failure outside the isolated per-source handler.
#[derive(Debug, PartialEq, Eq)]
pub enum WorkerIterationPostgresError<DriverError, TimeError> {
    Driver(DriverError),
    Time(TimeError),
    Query(WorkerIterationQueryError),
}

impl<DriverError: Display, TimeError: Display> Display
    for WorkerIterationPostgresError<DriverError, TimeError>
{
    /// Report the failure through the existing worker persistence diagnostics.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Driver(error) => Display::fmt(error, formatter),
            Self::Time(error) => Display::fmt(error, formatter),
            Self::Query(error) => Display::fmt(error, formatter),
        }
    }
}

/// Real query projection over one existing role-bounded PostgreSQL driver.
pub struct PostgresWorkerIterationDatabase<Driver, Time, Projection> {
    driver: Driver,
    time: Time,
    projection: Projection,
    instance: String,
    version: String,
}

impl<Driver, Time, Projection> PostgresWorkerIterationDatabase<Driver, Time, Projection> {
    /// Borrow the existing driver without creating another worker database owner.
    pub(crate) fn driver_ref(&self) -> &Driver {
        &self.driver
    }

    /// Keep the driver and stable worker identity for each iteration.
    pub const fn new(
        driver: Driver,
        time: Time,
        projection: Projection,
        instance: String,
        version: String,
    ) -> Self {
        Self {
            driver,
            time,
            projection,
            instance,
            version,
        }
    }
}

impl<Driver, Time, Projection> WorkerIterationDatabasePort
    for PostgresWorkerIterationDatabase<Driver, Time, Projection>
where
    Driver: WorkerIterationPostgresPort,
    Time: WorkerIterationTime,
    Projection: WorkerDueSourceProjection,
{
    type Error = WorkerIterationPostgresError<Driver::Error, Time::Error>;

    /// Read one bounded ordered set and retain a result slot for every returned row.
    fn discover_due_sources(&mut self, as_of_ms: i64) -> Result<Vec<WorkerDueSource>, Self::Error> {
        let as_of = self
            .time
            .utc_milliseconds(as_of_ms)
            .map_err(WorkerIterationPostgresError::Time)?;
        let plan =
            discover_due_sources_plan(&as_of, None).map_err(WorkerIterationPostgresError::Query)?;
        let rows = self
            .driver
            .query_due_sources(&plan)
            .map_err(WorkerIterationPostgresError::Driver)?;
        Ok(rows
            .into_iter()
            .map(
                // keep row-local selection failures inside their original ordered drain slot
                |row| match self.projection.project_due_source(&row) {
                    Ok(Some(source)) => source,
                    Ok(None) => WorkerDueSource::Unconfigured(row.id),
                    Err(failure) => WorkerDueSource::InvalidConfiguration {
                        source_id: row.id,
                        failure,
                    },
                },
            )
            .collect())
    }

    /// Use the actual iteration completion and retained success clocks for the upsert.
    fn update_worker_heartbeat(&mut self, heartbeat: WorkerHeartbeat) -> Result<(), Self::Error> {
        let last_loop_at = self
            .time
            .utc_milliseconds(heartbeat.last_loop_at_ms)
            .map_err(WorkerIterationPostgresError::Time)?;
        let last_success_at = heartbeat
            .last_success_at_ms
            .map(|epoch| self.time.utc_milliseconds(epoch))
            .transpose()
            .map_err(WorkerIterationPostgresError::Time)?;
        let plan = update_worker_heartbeat_plan(&WorkerHeartbeatUpdate {
            activity: heartbeat
                .activity
                .map(|WorkerActivity::Degraded| "degraded".to_owned()),
            instance: self.instance.clone(),
            last_loop_at,
            last_success_at,
            version: self.version.clone(),
        })
        .map_err(WorkerIterationPostgresError::Query)?;
        self.driver
            .execute_worker_heartbeat(&plan)
            .map_err(WorkerIterationPostgresError::Driver)
    }
}

impl<Driver: WorkerBootDatabase, Time, Projection> WorkerBootDatabase
    for PostgresWorkerIterationDatabase<Driver, Time, Projection>
{
    type Error = Driver::Error;

    /// Read PostgreSQL version from the same connection owner that drains the worker.
    fn query_server_version(
        &mut self,
        plan: &PostgresServerVersionReadPlan,
    ) -> Result<Option<PostgresServerVersionReadRow>, Self::Error> {
        self.driver.query_server_version(plan)
    }

    /// Read the existing ordered ledger without opening a second pool.
    fn query_migration_ledger(
        &mut self,
        plan: &HealthMigrationLedgerReadPlan,
    ) -> Result<Vec<AppliedMigration>, Self::Error> {
        self.driver.query_migration_ledger(plan)
    }

    /// Keep readiness and iteration heartbeat traffic on the same pool owner.
    fn query_worker_heartbeat(
        &mut self,
        plan: &WorkerHeartbeatReadPlan,
    ) -> Result<Option<WorkerHeartbeatReadRow>, Self::Error> {
        self.driver.query_worker_heartbeat(plan)
    }

    /// Close only the one retained underlying pool.
    fn close_worker_pool(&mut self) -> Result<(), Self::Error> {
        self.driver.close_worker_pool()
    }
}

impl<Driver, Time, Projection> OpenMeteoDatabaseBoundary
    for PostgresWorkerIterationDatabase<Driver, Time, Projection>
where
    Driver: OpenMeteoDatabaseBoundary,
{
    type Session = Driver::Session;

    /// Acquire the source session from the exact driver used by iteration queries.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure> {
        self.driver.acquire_source_session(source_id)
    }

    /// Abandon expired work through the same retained driver and session.
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        as_of: &str,
    ) -> Result<(), TaskFailure> {
        self.driver.abandon_expired_runs(session, as_of)
    }

    /// Forward the configured archive strategy through the same retained driver.
    fn complete_scheduled_ingestion(
        &mut self,
        input: &DatabaseScheduledCompletion,
        session: &mut Self::Session,
    ) -> Result<
        (),
        WorkerScheduledCompletionError<<Self::Session as ScheduledIngestionCommandPort>::Error>,
    > {
        self.driver.complete_scheduled_ingestion(input, session)
    }

    /// Release the source session to the exact driver that acquired it.
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
        self.driver.release_source_session(session)
    }
}

/// Retain the driver's session while exposing the original worker task-error category.
pub struct WorkerRainCaptureTaskSession<Session>(Session);

impl<Session: RainCaptureClaimSession> RainCaptureClaimSession
    for WorkerRainCaptureTaskSession<Session>
{
    type Error = TaskFailure;

    /// Start the driver's actual claim transaction.
    fn begin(&mut self) -> Result<(), Self::Error> {
        self.0
            .begin()
            .map_err(|_| rain_capture_task_failure("rain_capture_claim_failed"))
    }

    /// Preserve the source advisory transaction lock.
    fn advisory_lock(
        &mut self,
        statement: &RainCaptureClaimLockStatement,
    ) -> Result<(), Self::Error> {
        self.0
            .advisory_lock(statement)
            .map_err(|_| rain_capture_task_failure("rain_capture_claim_failed"))
    }

    /// Sample the database clock through the retained client.
    fn server_clock(
        &mut self,
        statement: &RainCaptureClaimClockStatement,
    ) -> Result<Option<i64>, Self::Error> {
        self.0
            .server_clock(statement)
            .map_err(|_| rain_capture_task_failure("rain_capture_claim_failed"))
    }

    /// Retain the exact duplicate and quota gate query.
    fn claim_gate(
        &mut self,
        statement: &RainCaptureClaimGateStatement,
    ) -> Result<Option<RainCaptureClaimGateRow>, Self::Error> {
        self.0
            .claim_gate(statement)
            .map_err(|_| rain_capture_task_failure("rain_capture_claim_failed"))
    }

    /// Insert one immutable claim through the same session.
    fn insert_claim(
        &mut self,
        statement: &RainCaptureClaimInsertStatement,
    ) -> Result<Option<String>, Self::Error> {
        self.0
            .insert_claim(statement)
            .map_err(|_| rain_capture_task_failure("rain_capture_claim_failed"))
    }

    /// Commit the retained transaction.
    fn commit(&mut self) -> Result<(), Self::Error> {
        self.0
            .commit()
            .map_err(|_| rain_capture_task_failure("rain_capture_claim_failed"))
    }

    /// Roll back the actual transaction on failure.
    fn rollback(&mut self) -> Result<(), Self::Error> {
        self.0
            .rollback()
            .map_err(|_| rain_capture_task_failure("rain_capture_claim_failed"))
    }

    /// Return the original session to its process pool.
    fn release(&mut self) {
        self.0.release();
    }
}

/// Preserve the existing bounded worker error category at the boot/iteration boundary.
fn rain_capture_task_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

impl<Driver: RainCaptureClaimPool, Time, Projection> RainCaptureClaimPool
    for PostgresWorkerIterationDatabase<Driver, Time, Projection>
{
    type Error = TaskFailure;
    type Session = WorkerRainCaptureTaskSession<Driver::Session>;

    /// Use the iteration driver for the database-clock claim transaction.
    fn checkout(&mut self) -> Result<Self::Session, Self::Error> {
        self.driver
            .checkout()
            .map(WorkerRainCaptureTaskSession)
            .map_err(|_| rain_capture_task_failure("rain_capture_claim_failed"))
    }
}

impl<Driver: RainCaptureReceiptDatabase, Time, Projection> RainCaptureReceiptDatabase
    for PostgresWorkerIterationDatabase<Driver, Time, Projection>
{
    type Error = TaskFailure;

    /// Look up the immutable claim through the same retained driver.
    fn select_claim(
        &mut self,
        statement: &'static str,
        claim_id: &str,
    ) -> Result<Option<RainCaptureClaimRow>, Self::Error> {
        self.driver
            .select_claim(statement, claim_id)
            .map_err(|_| rain_capture_task_failure("rain_capture_receipt_append_failed"))
    }

    /// Retain evidence using the existing driver's append-only receipt statement.
    fn insert_receipt(
        &mut self,
        statement: &'static str,
        receipt: &PreparedRainCaptureReceipt,
    ) -> Result<(), Self::Error> {
        self.driver
            .insert_receipt(statement, receipt)
            .map_err(|_| rain_capture_task_failure("rain_capture_receipt_append_failed"))
    }
}
