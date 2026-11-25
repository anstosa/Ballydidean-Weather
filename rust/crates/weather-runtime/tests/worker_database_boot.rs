use std::cell::RefCell;
use std::rc::Rc;

use weather_contracts::JsDateOrString;
use weather_database::{
    AdmittedHealthMigrationIdentity, AppliedMigration, DatabaseConfiguration,
    DatabasePoolConfiguration, HEALTH_MIGRATION_LEDGER_SQL, HealthMigrationLedgerReadPlan,
    MigrationDigest, MigrationScript, POSTGRES_SERVER_VERSION_NUM_SQL,
    PostgresServerVersionReadPlan, PostgresServerVersionReadRow, WORKER_HEARTBEAT_READ_SQL,
    WorkerHeartbeatReadPlan, WorkerHeartbeatReadRow, WorkerReadinessBind,
};
use weather_runtime::{
    AdmittedHealthMigrationArtifacts, DatabaseWorkerBootConfiguration, PostgresWorkerBootFactory,
    WorkerBootDatabase, WorkerBootDatabaseConnector, WorkerBootSharedArchiveDatabase,
    WorkerBootValues, WorkerHealthOutput, WorkerIterationDatabasePort, run_worker_health_check,
};
use weather_worker::{
    WorkerDueSource, WorkerHealth, WorkerHeartbeat, WorkerProcessPhase, WorkerProcessPool,
    WorkerProcessPoolFactory,
};

const NOW: i64 = 1_791_000_000_000;
const INSTANCE: &str = "worker-fixture";
const MIGRATION: &str = "0001_initial.sql";
type Trace = Rc<RefCell<Vec<&'static str>>>;

struct Configuration(DatabaseConfiguration);

impl DatabaseWorkerBootConfiguration for Configuration {
    /// Use the selected pinned fixture pool configuration.
    fn database_configuration(&self) -> &DatabaseConfiguration {
        &self.0
    }

    /// Use the configured source key rather than the latest unrelated worker row.
    fn worker_instance(&self) -> &str {
        INSTANCE
    }
}

struct Database {
    trace: Trace,
    version: &'static str,
    applied: bool,
    heartbeat: Option<WorkerHeartbeatReadRow>,
}

impl WorkerIterationDatabasePort for Database {
    type Error = &'static str;

    /// Record the due-source attempt made through the retained pool.
    fn discover_due_sources(&mut self, as_of_ms: i64) -> Result<Vec<WorkerDueSource>, Self::Error> {
        assert_eq!(as_of_ms, NOW);
        self.trace.borrow_mut().push("discover");
        Ok(Vec::new())
    }

    /// Confirm completed iteration writes use the same database owner.
    fn update_worker_heartbeat(&mut self, heartbeat: WorkerHeartbeat) -> Result<(), Self::Error> {
        assert_eq!(heartbeat.last_loop_at_ms, NOW);
        self.trace.borrow_mut().push("write-heartbeat");
        Ok(())
    }
}

impl WorkerBootDatabase for Database {
    type Error = &'static str;

    /// Read the single source version field before artifacts or ledger.
    fn query_server_version(
        &mut self,
        plan: &PostgresServerVersionReadPlan,
    ) -> Result<Option<PostgresServerVersionReadRow>, Self::Error> {
        assert_eq!(plan.sql, POSTGRES_SERVER_VERSION_NUM_SQL);
        self.trace.borrow_mut().push("version");
        Ok(Some(PostgresServerVersionReadRow {
            server_version_num: self.version.to_owned(),
        }))
    }

    /// Return only the ordinary read-only ordered ledger.
    fn query_migration_ledger(
        &mut self,
        plan: &HealthMigrationLedgerReadPlan,
    ) -> Result<Vec<AppliedMigration>, Self::Error> {
        assert_eq!(plan.sql, HEALTH_MIGRATION_LEDGER_SQL);
        self.trace.borrow_mut().push("ledger");
        Ok(self
            .applied
            .then(|| AppliedMigration {
                name: MIGRATION.to_owned(),
                digest: MigrationDigest::admitted("a".repeat(64)),
            })
            .into_iter()
            .collect())
    }

    /// Read the exact heartbeat bind and preserve a missing row separately.
    fn query_worker_heartbeat(
        &mut self,
        plan: &WorkerHeartbeatReadPlan,
    ) -> Result<Option<WorkerHeartbeatReadRow>, Self::Error> {
        assert_eq!(plan.sql, WORKER_HEARTBEAT_READ_SQL);
        assert_eq!(plan.binds, [WorkerReadinessBind::Text(INSTANCE.to_owned())]);
        self.trace.borrow_mut().push("heartbeat");
        Ok(self.heartbeat.clone())
    }

    /// Track every real close request made by the runtime owner.
    fn close_worker_pool(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("close");
        Ok(())
    }
}

struct SharedArchiveDatabase(Trace);

impl WorkerBootSharedArchiveDatabase for Database {
    type ArchiveDatabase = SharedArchiveDatabase;

    /// Clone the fixture's already-open pool witness without invoking its connector.
    fn share_public_station_archive_database(
        &mut self,
    ) -> Result<Self::ArchiveDatabase, weather_worker::TaskFailure> {
        self.trace.borrow_mut().push("share-archive");
        Ok(SharedArchiveDatabase(Rc::clone(&self.trace)))
    }
}

struct Connector(Option<Database>);

impl WorkerBootDatabaseConnector for Connector {
    type Database = Database;
    type Error = &'static str;

    /// Inspect the actual pool settings while allocating no database resources.
    fn open_worker_pool(
        &mut self,
        configuration: DatabasePoolConfiguration,
    ) -> Result<Database, Self::Error> {
        assert_eq!(configuration.max, 10);
        assert_eq!(configuration.host, "db-fixture.invalid");
        assert_eq!(configuration.port, 5432);
        assert_eq!(configuration.statement_timeout, 30_000);
        assert!(configuration.ssl);
        let database = self.0.take().ok_or("second open")?;
        database.trace.borrow_mut().push("open");
        Ok(database)
    }
}

struct Values(Trace);

impl WorkerBootValues<Configuration> for Values {
    type Error = &'static str;

    /// Admit one immutable fixture artifact without migration filesystem access.
    fn load_migration_artifacts(
        &mut self,
        _: &Configuration,
    ) -> Result<AdmittedHealthMigrationArtifacts, Self::Error> {
        self.0.borrow_mut().push("artifacts");
        Ok(AdmittedHealthMigrationArtifacts {
            artifacts: vec![MigrationScript {
                name: MIGRATION.to_owned(),
                digest: MigrationDigest::admitted("a".repeat(64)),
                sql: "fixture-only".to_owned(),
            }],
            authorization: None,
            release: "development".to_owned(),
        })
    }

    /// Provide the digest witness only after loading the actual read result.
    fn admit_migration_identity(
        &mut self,
        artifacts: AdmittedHealthMigrationArtifacts,
        _: &[AppliedMigration],
    ) -> Result<AdmittedHealthMigrationIdentity, Self::Error> {
        self.0.borrow_mut().push("digest");
        Ok(AdmittedHealthMigrationIdentity {
            applied_history_sha256: MigrationDigest::admitted("b".repeat(64)),
            artifacts: artifacts.artifacts,
            authorization: artifacts.authorization,
            release: artifacts.release,
        })
    }

    /// Capture the Node-compatible default health clock before SQL.
    fn health_now_ms(&mut self) -> Result<i64, Self::Error> {
        self.0.borrow_mut().push("clock");
        Ok(NOW)
    }

    /// Transfer actual decoded fixture epochs without implementing a civil-time parser.
    fn heartbeat_epoch_ms(&mut self, value: &JsDateOrString) -> Result<i64, Self::Error> {
        self.0.borrow_mut().push("admit-time");
        let JsDateOrString::DateEpochMilliseconds(epoch) = value else {
            return Err("unexpected fixture timestamp");
        };
        Ok(*epoch)
    }
}

struct Output(Trace, Option<WorkerHealth>);

impl WorkerHealthOutput for Output {
    type Error = &'static str;

    /// Accept only one complete read result before close.
    fn write_health(&mut self, health: WorkerHealth) -> Result<(), Self::Error> {
        assert!(self.1.replace(health).is_none());
        self.0.borrow_mut().push("output");
        Ok(())
    }
}

/// Create a fresh dry-run fixture with no external database.
fn fixture(
    version: &'static str,
    applied: bool,
    heartbeat: Option<WorkerHeartbeatReadRow>,
) -> (
    Configuration,
    PostgresWorkerBootFactory<Connector, Values>,
    Output,
    Trace,
) {
    let trace = Trace::default();
    let configuration = Configuration(DatabaseConfiguration {
        application_name: "weather-worker".to_owned(),
        database: "weather-fixture".to_owned(),
        host: "db-fixture.invalid".to_owned(),
        lock_timeout_ms: 10_000,
        password: "fixture-only".to_owned(),
        port: 5432,
        ssl: true,
        statement_timeout_ms: 30_000,
        user: "weather-fixture".to_owned(),
    });
    let factory = PostgresWorkerBootFactory::new(
        Connector(Some(Database {
            trace: Rc::clone(&trace),
            version,
            applied,
            heartbeat,
        })),
        Values(Rc::clone(&trace)),
    );
    let output = Output(Rc::clone(&trace), None);
    (configuration, factory, output, trace)
}

/// Bind readiness, heartbeat freshness and one-shot output to the same pool.
#[test]
fn readiness_and_health_use_source_order_and_one_close() {
    let (configuration, mut factory, mut output, trace) = fixture(
        "150000",
        true,
        Some(WorkerHeartbeatReadRow {
            last_loop_at: JsDateOrString::DateEpochMilliseconds(NOW - 60_000),
            last_success_at: Some(JsDateOrString::DateEpochMilliseconds(NOW - 70_000)),
        }),
    );
    assert_eq!(
        run_worker_health_check(&configuration, &mut factory, &mut output),
        Ok(0)
    );
    let health = output.1.unwrap();
    assert!(health.ready);
    assert_eq!(health.last_success_at_ms, Some(NOW - 70_000));
    assert_eq!(
        *trace.borrow(),
        [
            "open",
            "version",
            "artifacts",
            "ledger",
            "digest",
            "clock",
            "heartbeat",
            "admit-time",
            "admit-time",
            "output",
            "close",
        ]
    );
}

/// Fail before time or heartbeat reads when the server or applied ledger is not ready.
#[test]
fn startup_refusal_closes_without_reporting_health() {
    // unsupported versions precede catalog work; a pending script follows ledger work
    for (version, applied, expected, code) in [
        (
            "140099",
            true,
            &["open", "version", "close"][..],
            "unsupported_postgres_version",
        ),
        (
            "160002",
            false,
            &["open", "version", "artifacts", "ledger", "digest", "close"][..],
            "migration_not_ready",
        ),
    ] {
        let (configuration, mut factory, mut output, trace) = fixture(version, applied, None);
        let error = run_worker_health_check(&configuration, &mut factory, &mut output).unwrap_err();
        assert_eq!(error.phase, WorkerProcessPhase::Readiness);
        assert_eq!(error.code, code);
        assert!(output.1.is_none());
        assert_eq!(&*trace.borrow(), expected);
    }
}

/// Keep the admitted pool alive for iteration even before its first durable heartbeat.
#[test]
fn first_iteration_reuses_the_ready_pool_with_no_prior_heartbeat() {
    let (configuration, mut factory, _, trace) = fixture("160001", true, None);
    let mut pool = factory.create_pool(&configuration).unwrap();
    pool.assert_readiness(&configuration).unwrap();
    let health = pool.read_durable_health(&configuration).unwrap();
    assert!(health.live);
    assert!(!health.ready);
    assert!(health.stale);
    assert!(pool.discover_due_sources(NOW).unwrap().is_empty());
    pool.update_worker_heartbeat(WorkerHeartbeat {
        activity: None,
        last_loop_at_ms: NOW,
        last_success_at_ms: None,
    })
    .unwrap();
    WorkerProcessPool::<Configuration>::close(&mut pool).unwrap();
    drop(pool);
    assert_eq!(
        *trace.borrow(),
        [
            "open",
            "version",
            "artifacts",
            "ledger",
            "digest",
            "clock",
            "heartbeat",
            "discover",
            "write-heartbeat",
            "close",
        ]
    );
}

/// Share one startup archive handle from the single retained worker pool.
#[test]
fn startup_archive_shares_without_reopening_the_connector() {
    let (configuration, mut factory, _, trace) = fixture("160001", true, None);
    let mut pool = factory.create_pool(&configuration).unwrap();
    let archive = pool.share_public_station_archive_database().unwrap();
    assert!(Rc::ptr_eq(&archive.0, &trace));
    let second = factory
        .create_pool(&configuration)
        .err()
        .expect("second open refused");
    assert_eq!(second.phase, WorkerProcessPhase::Pool);
    assert_eq!(second.code, "worker_pool_already_created");
    WorkerProcessPool::<Configuration>::close(&mut pool).unwrap();
    let error = pool
        .share_public_station_archive_database()
        .err()
        .expect("closed pool refused");
    assert_eq!(error.attempts, 0);
    assert_eq!(error.code, "worker_pool_closed");
    assert_eq!(*trace.borrow(), ["open", "share-archive", "close"]);
}
