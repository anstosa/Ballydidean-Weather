//! Bind worker startup and health to the same retained PostgreSQL pool as ingestion.

use std::fmt::{Display, Formatter};

use weather_contracts::JsDateOrString;
use weather_database::{
    AdmittedHealthMigrationIdentity, AmbientBackfillPoolPort, AmbientBackfillSourceReadPort,
    AppliedMigration, CompleteScheduledIngestion as DatabaseScheduledCompletion,
    DatabaseConfiguration, DatabasePoolConfiguration, HealthMigrationLedgerReadPlan,
    PostgresServerVersionReadPlan, PostgresServerVersionReadRow, PreparedRainCaptureReceipt,
    RainCaptureClaimPool, RainCaptureClaimRow, RainCaptureReceiptDatabase,
    ScheduledIngestionCommandPort, WorkerHeartbeatReadPlan, WorkerHeartbeatReadRow,
    admit_supported_postgres_version, health_migration_ledger_read_plan,
    postgres_server_version_read_plan, to_pool_configuration, verify_health_migration_readiness,
    worker_heartbeat_read_plan,
};
use weather_worker::{
    ScheduledSourceMaterialComparison, TaskFailure, WorkerDueSource, WorkerHealth, WorkerHeartbeat,
    WorkerProcessFailure, WorkerProcessPhase, WorkerProcessPool, WorkerProcessPoolFactory,
    worker_health,
};

use crate::{
    AdmittedHealthMigrationArtifacts, AmbientBackfillRuntimeValues,
    DatabaseAmbientBackfillRepository, OpenMeteoDatabaseBoundary, PublicStationStartupPool,
    WorkerIterationDatabasePort, WorkerScheduledCompletionError,
};

const WORKER_STALE_AFTER_MS: i64 = 30 * 60 * 1000;

/// Ordinary worker configuration with its admitted shared database settings.
pub trait DatabaseWorkerBootConfiguration {
    /// Borrow the settings before creating the one retained process pool.
    fn database_configuration(&self) -> &DatabaseConfiguration;

    /// Select the durable heartbeat belonging to this configured worker.
    fn worker_instance(&self) -> &str;
}

/// Driver boundary for startup/readiness and the same database used by the worker iteration.
pub trait WorkerBootDatabase {
    type Error: Display;
    /// Read the server's canonical numeric version without probing unrelated settings.
    fn query_server_version(
        &mut self,
        plan: &PostgresServerVersionReadPlan,
    ) -> Result<Option<PostgresServerVersionReadRow>, Self::Error>;

    /// Read only the ordered applied ledger; startup never runs migration scripts.
    fn query_migration_ledger(
        &mut self,
        plan: &HealthMigrationLedgerReadPlan,
    ) -> Result<Vec<AppliedMigration>, Self::Error>;

    /// Read the exact last loop and last success rows for this process's instance.
    fn query_worker_heartbeat(
        &mut self,
        plan: &WorkerHeartbeatReadPlan,
    ) -> Result<Option<WorkerHeartbeatReadRow>, Self::Error>;

    /// Close the retained pool, not the configured database or its persistent data.
    fn close_worker_pool(&mut self) -> Result<(), Self::Error>;
}

/// Share one bounded handle from the already-open worker pool for the detached archive job.
pub trait WorkerBootSharedArchiveDatabase: WorkerBootDatabase {
    type ArchiveDatabase;

    /// Clone or lease the existing driver's pool handle without opening another pool.
    fn share_public_station_archive_database(
        &mut self,
    ) -> Result<Self::ArchiveDatabase, TaskFailure>;
}

/// Create the retained pool using existing SSL, timeout, and connection-count settings.
pub trait WorkerBootDatabaseConnector {
    type Database: WorkerBootDatabase;
    type Error;

    /// Construct one pool using the approved PostgreSQL transport.
    fn open_worker_pool(
        &mut self,
        configuration: DatabasePoolConfiguration,
    ) -> Result<Self::Database, Self::Error>;
}

/// Runtime-owned file, digest and canonical UTC conversions needed during startup.
pub trait WorkerBootValues<Configuration> {
    type Error;

    /// Load the local ordered migration artifacts before querying the ledger.
    fn load_migration_artifacts(
        &mut self,
        configuration: &Configuration,
    ) -> Result<AdmittedHealthMigrationArtifacts, Self::Error>;

    /// Bind that admitted catalog and release to the digest of the actual ordered ledger.
    fn admit_migration_identity(
        &mut self,
        artifacts: AdmittedHealthMigrationArtifacts,
        applied: &[AppliedMigration],
    ) -> Result<AdmittedHealthMigrationIdentity, Self::Error>;

    /// Sample the ordinary worker health clock before requesting the heartbeat row.
    fn health_now_ms(&mut self) -> Result<i64, Self::Error>;

    /// Convert one decoded PostgreSQL UTC timestamp using the approved time owner.
    fn heartbeat_epoch_ms(&mut self, value: &JsDateOrString) -> Result<i64, Self::Error>;
}

/// One-use factory for the pool and its startup material owner.
pub struct PostgresWorkerBootFactory<Connector, Values> {
    connector: Connector,
    values: Option<Values>,
}

impl<Connector, Values> PostgresWorkerBootFactory<Connector, Values> {
    /// Bind the external owners; do not connect until the worker admits its process configuration.
    pub const fn new(connector: Connector, values: Values) -> Self {
        Self {
            connector,
            values: Some(values),
        }
    }
}

impl<Configuration, Connector, Values> WorkerProcessPoolFactory<Configuration>
    for PostgresWorkerBootFactory<Connector, Values>
where
    Configuration: DatabaseWorkerBootConfiguration,
    Connector: WorkerBootDatabaseConnector,
    Values: WorkerBootValues<Configuration>,
{
    type Pool = PostgresWorkerBootPool<Connector::Database, Values>;

    /// Open one pool and move the startup state into that retained owner.
    fn create_pool(
        &mut self,
        configuration: &Configuration,
    ) -> Result<Self::Pool, WorkerProcessFailure> {
        // a consumed factory must not connect a second process pool
        let Some(values) = self.values.take() else {
            return Err(failure(
                WorkerProcessPhase::Pool,
                "worker_pool_already_created",
            ));
        };
        let database = self
            .connector
            .open_worker_pool(to_pool_configuration(
                configuration.database_configuration(),
            ))
            .map_err(|_| failure(WorkerProcessPhase::Pool, "worker_pool_open_failed"))?;
        Ok(PostgresWorkerBootPool {
            database: Some(database),
            values,
        })
    }
}

/// Exact worker pool used from supported-server readiness through ordinary iterations.
pub struct PostgresWorkerBootPool<Database: WorkerBootDatabase, Values> {
    database: Option<Database>,
    values: Values,
}

impl<Database: WorkerBootDatabase, Values> PostgresWorkerBootPool<Database, Values> {
    /// Borrow the same open database for other worker-owned typed operations.
    pub fn database_mut(&mut self) -> Option<&mut Database> {
        self.database.as_mut()
    }

    /// Share the retained driver's bounded archive handle only while the pool remains open.
    pub fn share_public_station_archive_database(
        &mut self,
    ) -> Result<Database::ArchiveDatabase, TaskFailure>
    where
        Database: WorkerBootSharedArchiveDatabase,
    {
        self.database_mut()
            .ok_or_else(worker_pool_closed_task_failure)?
            .share_public_station_archive_database()
    }
}

impl<Database: WorkerBootDatabase, Values> Drop for PostgresWorkerBootPool<Database, Values> {
    /// Close an aborted startup pool without issuing any queries after closure.
    fn drop(&mut self) {
        // the explicit close removes ownership before touching the driver
        if let Some(mut database) = self.database.take() {
            let _ = database.close_worker_pool();
        }
    }
}

impl<Configuration, Database, Values> WorkerProcessPool<Configuration>
    for PostgresWorkerBootPool<Database, Values>
where
    Configuration: DatabaseWorkerBootConfiguration,
    Database: WorkerBootDatabase,
    Values: WorkerBootValues<Configuration>,
{
    /// Gate worker readiness on PostgreSQL support before checking the immutable applied ledger.
    fn assert_readiness(
        &mut self,
        configuration: &Configuration,
    ) -> Result<(), WorkerProcessFailure> {
        let database = self
            .database
            .as_mut()
            .ok_or_else(|| failure(WorkerProcessPhase::Readiness, "worker_pool_closed"))?;
        let version = database
            .query_server_version(&postgres_server_version_read_plan())
            .map_err(|_| {
                failure(
                    WorkerProcessPhase::Readiness,
                    "postgres_version_read_failed",
                )
            })?;
        admit_supported_postgres_version(version.as_ref()).map_err(|_| {
            failure(
                WorkerProcessPhase::Readiness,
                "unsupported_postgres_version",
            )
        })?;

        let artifacts = self
            .values
            .load_migration_artifacts(configuration)
            .map_err(|_| {
                failure(
                    WorkerProcessPhase::Readiness,
                    "migration_artifacts_unavailable",
                )
            })?;
        let applied = database
            .query_migration_ledger(&health_migration_ledger_read_plan())
            .map_err(|_| {
                failure(
                    WorkerProcessPhase::Readiness,
                    "migration_ledger_unavailable",
                )
            })?;
        let identity = self
            .values
            .admit_migration_identity(artifacts, &applied)
            .map_err(|_| {
                failure(
                    WorkerProcessPhase::Readiness,
                    "migration_identity_unavailable",
                )
            })?;
        verify_health_migration_readiness(&identity, &applied)
            .map_err(|_| failure(WorkerProcessPhase::Readiness, "migration_not_ready"))?;
        Ok(())
    }

    /// Return the persisted continuation and freshness state for the exact worker instance.
    fn read_durable_health(
        &mut self,
        configuration: &Configuration,
    ) -> Result<WorkerHealth, WorkerProcessFailure> {
        let database = self
            .database
            .as_mut()
            .ok_or_else(|| failure(WorkerProcessPhase::DurableHealth, "worker_pool_closed"))?;
        let now = self.values.health_now_ms().map_err(|_| {
            failure(
                WorkerProcessPhase::DurableHealth,
                "worker_health_clock_failed",
            )
        })?;
        let row = database
            .query_worker_heartbeat(&worker_heartbeat_read_plan(configuration.worker_instance()))
            .map_err(|_| {
                failure(
                    WorkerProcessPhase::DurableHealth,
                    "worker_heartbeat_unavailable",
                )
            })?;
        // a missing row is live but not ready; the process may create its first heartbeat
        let Some(row) = row else {
            return Ok(worker_health(now, None, None, false, WORKER_STALE_AFTER_MS));
        };
        let last_loop = self
            .values
            .heartbeat_epoch_ms(&row.last_loop_at)
            .map_err(|_| {
                failure(
                    WorkerProcessPhase::DurableHealth,
                    "worker_heartbeat_time_invalid",
                )
            })?;
        let last_success = row
            .last_success_at
            .as_ref()
            .map(|timestamp| self.values.heartbeat_epoch_ms(timestamp))
            .transpose()
            .map_err(|_| {
                failure(
                    WorkerProcessPhase::DurableHealth,
                    "worker_heartbeat_time_invalid",
                )
            })?;
        Ok(worker_health(
            now,
            Some(last_loop),
            last_success,
            true,
            WORKER_STALE_AFTER_MS,
        ))
    }

    /// Retire the pool exactly once, even when the driver reports a close failure.
    fn close(&mut self) -> Result<(), WorkerProcessFailure> {
        self.database.take().map_or(Ok(()), |mut database| {
            database
                .close_worker_pool()
                .map_err(|_| failure(WorkerProcessPhase::Close, "worker_pool_close_failed"))
        })
    }
}

/// Bounded database failure returned to the existing worker iteration coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostgresWorkerIterationError(&'static str);

impl Display for PostgresWorkerIterationError {
    /// Render an operation category without borrowing driver diagnostics.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl<Database: WorkerBootDatabase + WorkerIterationDatabasePort, Values> WorkerIterationDatabasePort
    for PostgresWorkerBootPool<Database, Values>
{
    type Error = PostgresWorkerIterationError;

    /// Discover due sources through the exact pool that passed startup readiness.
    fn discover_due_sources(&mut self, as_of_ms: i64) -> Result<Vec<WorkerDueSource>, Self::Error> {
        self.database_mut()
            .ok_or(PostgresWorkerIterationError("worker_pool_closed"))?
            .discover_due_sources(as_of_ms)
            .map_err(|_| PostgresWorkerIterationError("worker_due_sources_failed"))
    }

    /// Write a completed heartbeat through the retained iteration database.
    fn update_worker_heartbeat(&mut self, heartbeat: WorkerHeartbeat) -> Result<(), Self::Error> {
        self.database_mut()
            .ok_or(PostgresWorkerIterationError("worker_pool_closed"))?
            .update_worker_heartbeat(heartbeat)
            .map_err(|_| PostgresWorkerIterationError("worker_heartbeat_write_failed"))
    }
}

impl<Database, Values> OpenMeteoDatabaseBoundary for PostgresWorkerBootPool<Database, Values>
where
    Database: WorkerBootDatabase + OpenMeteoDatabaseBoundary,
{
    type Session = Database::Session;

    /// Acquire a source session from the same open worker database.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure> {
        self.database_mut()
            .ok_or_else(worker_pool_closed_task_failure)?
            .acquire_source_session(source_id)
    }

    /// Abandon expired work through the retained worker database when it remains open.
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        as_of: &str,
    ) -> Result<(), TaskFailure> {
        self.database_mut()
            .ok_or_else(worker_pool_closed_task_failure)?
            .abandon_expired_runs(session, as_of)
    }

    /// Forward completion through the open pool without losing its archive strategy.
    fn complete_scheduled_ingestion(
        &mut self,
        input: &DatabaseScheduledCompletion,
        session: &mut Self::Session,
    ) -> Result<
        (),
        WorkerScheduledCompletionError<<Self::Session as ScheduledIngestionCommandPort>::Error>,
    > {
        self.database_mut()
            .ok_or_else(worker_pool_closed_task_failure)
            .map_err(WorkerScheduledCompletionError::Task)?
            .complete_scheduled_ingestion(input, session)
    }

    /// Release the source session through its retained worker database owner.
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
        self.database_mut()
            .ok_or_else(worker_pool_closed_task_failure)?
            .release_source_session(session)
    }
}

impl<Database, Values> PublicStationStartupPool for PostgresWorkerBootPool<Database, Values>
where
    Database: WorkerBootSharedArchiveDatabase,
    Database::ArchiveDatabase: AmbientBackfillPoolPort
        + AmbientBackfillSourceReadPort<
            Error = <Database::ArchiveDatabase as AmbientBackfillPoolPort>::Error,
        > + Send
        + 'static,
    <Database::ArchiveDatabase as AmbientBackfillPoolPort>::Error: Display,
    Values:
        AmbientBackfillRuntimeValues + ScheduledSourceMaterialComparison + Clone + Send + 'static,
{
    type Repository = DatabaseAmbientBackfillRepository<Database::ArchiveDatabase, Values>;

    /// Move one shared pool handle and a cloned stateless value owner into the startup thread.
    fn share_startup_repository(&mut self) -> Result<Self::Repository, TaskFailure> {
        let database = self.share_public_station_archive_database()?;
        let values = self.values.clone();
        Ok(DatabaseAmbientBackfillRepository::new(database, values))
    }
}

/// Return the stable zero-attempt failure used after explicit worker-pool closure.
fn worker_pool_closed_task_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "worker_pool_closed".to_owned(),
    }
}

impl<Database, Values> RainCaptureClaimPool for PostgresWorkerBootPool<Database, Values>
where
    Database: WorkerBootDatabase + RainCaptureClaimPool<Error = TaskFailure>,
{
    type Error = TaskFailure;
    type Session = Database::Session;

    /// Use the startup-approved worker pool rather than connecting independently.
    fn checkout(&mut self) -> Result<Self::Session, Self::Error> {
        self.database_mut()
            .ok_or_else(worker_pool_closed_task_failure)?
            .checkout()
    }
}

impl<Database, Values> RainCaptureReceiptDatabase for PostgresWorkerBootPool<Database, Values>
where
    Database: WorkerBootDatabase + RainCaptureReceiptDatabase<Error = TaskFailure>,
{
    type Error = TaskFailure;

    /// Read the original provider claim while the retained pool is open.
    fn select_claim(
        &mut self,
        statement: &'static str,
        claim_id: &str,
    ) -> Result<Option<RainCaptureClaimRow>, Self::Error> {
        self.database_mut()
            .ok_or_else(worker_pool_closed_task_failure)?
            .select_claim(statement, claim_id)
    }

    /// Append one checked immutable provider receipt through the same pool.
    fn insert_receipt(
        &mut self,
        statement: &'static str,
        receipt: &PreparedRainCaptureReceipt,
    ) -> Result<(), Self::Error> {
        self.database_mut()
            .ok_or_else(worker_pool_closed_task_failure)?
            .insert_receipt(statement, receipt)
    }
}

/// Approved adapter which formats the typed worker health as the existing stdout JSON line.
pub trait WorkerHealthOutput {
    type Error;

    /// Write the one measured health result before returning its container status.
    fn write_health(&mut self, health: WorkerHealth) -> Result<(), Self::Error>;
}

/// Run the source one-shot health check, closing the pool on success or ordinary failure.
pub fn run_worker_health_check<Configuration, Factory, Output>(
    configuration: &Configuration,
    factory: &mut Factory,
    output: &mut Output,
) -> Result<u8, WorkerProcessFailure>
where
    Factory: WorkerProcessPoolFactory<Configuration>,
    Output: WorkerHealthOutput,
{
    let mut pool = factory.create_pool(configuration)?;
    let result = (|| {
        pool.assert_readiness(configuration)?;
        let health = pool.read_durable_health(configuration)?;
        output.write_health(health).map_err(|_| {
            failure(
                WorkerProcessPhase::DurableHealth,
                "worker_health_output_failed",
            )
        })?;
        Ok(u8::from(!health.ready))
    })();
    // retain the source finally semantics, including an ordinary pool-close failure
    pool.close()?;
    result
}

/// Retain one operation category and phase without leaking connection settings.
fn failure(phase: WorkerProcessPhase, code: &'static str) -> WorkerProcessFailure {
    WorkerProcessFailure {
        phase,
        code: code.to_owned(),
    }
}
