use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use weather_database::{
    AppliedMigration, CheckedMigrationCatalog, CheckedMigrationError, CheckedMigrationSession,
    MIGRATION_ADVISORY_LOCK_KEY, MIGRATION_LEDGER_BOOTSTRAP_SQL, MigrationArtifactNameOrder,
    MigrationArtifactSha256, MigrationCommandPort, MigrationDigest, MigrationExecutionError,
    MigrationMode, MigrationPlanningError, MigrationScript, MigrationSessionTimeouts,
    PostgresServerVersionReadPlan, PostgresServerVersionReadRow,
    ProcessMigrationArtifactFilesystem, execute_migrations, load_migration_artifacts,
    run_checked_migrations,
};

const INITIAL_SQL: &str =
    include_str!("../../../../packages/database/migrations/0001_initial_weather.sql");
const READINESS_SQL: &str =
    include_str!("../../../../packages/database/migrations/0002_worker_migration_readiness.sql");
const ECOWITT_SQL: &str =
    include_str!("../../../../packages/database/migrations/0003_ecowitt_measurements.sql");
const TEMPEST_SQL: &str =
    include_str!("../../../../packages/database/migrations/0004_tempest_metadata.sql");

#[derive(Debug, Clone, PartialEq, Eq)]
enum Trace {
    Version,
    Configure(&'static str, String),
    Control(&'static str),
    Lock(i64),
    ReadHistory,
    Begin,
    Script { name: String, sql: String },
    Record { name: String, digest: String },
    Commit,
    Rollback,
    Unlock(i64),
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureError(String);

impl Display for FixtureError {
    /// render one fixture failure
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for FixtureError {}

struct FixturePort {
    history: Vec<AppliedMigration>,
    trace: Vec<Trace>,
    fail_script: Option<String>,
}

impl CheckedMigrationSession for FixturePort {
    /// Witness the exact read-only PostgreSQL version query before lock or transaction.
    fn query_server_version(
        &mut self,
        plan: PostgresServerVersionReadPlan,
    ) -> Result<Option<PostgresServerVersionReadRow>, Self::Error> {
        assert_eq!(
            plan.sql,
            "SELECT current_setting('server_version_num') AS server_version_num"
        );
        self.trace.push(Trace::Version);
        Ok(Some(PostgresServerVersionReadRow {
            server_version_num: "160003".to_owned(),
        }))
    }

    /// Record bound timeout configuration without making any database connection.
    fn configure_timeout(&mut self, sql: &'static str, value: &str) -> Result<(), Self::Error> {
        self.trace.push(Trace::Configure(sql, value.to_owned()));
        Ok(())
    }

    /// Retain the existing statement as fixture data without executing DDL.
    fn execute_control_statement(&mut self, sql: &'static str) -> Result<(), Self::Error> {
        self.trace.push(Trace::Control(sql));
        Ok(())
    }
}

struct LoadedCatalog(Vec<MigrationScript>);

impl CheckedMigrationCatalog for LoadedCatalog {
    type Error = FixtureError;

    /// Offer the read-only artifact bytes from the repository to the fixture session.
    fn load_catalog(
        &mut self,
        _: &mut impl MigrationArtifactSha256,
    ) -> Result<Vec<MigrationScript>, Self::Error> {
        Ok(std::mem::take(&mut self.0))
    }
}

struct TestDigest;

impl MigrationArtifactSha256 for TestDigest {
    /// Use the test host's exact SHA-256 as an independent oracle, never a runtime helper.
    fn sha256(&mut self, bytes: &[u8]) -> MigrationDigest {
        let mut command = Command::new("sha256sum")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        command.stdin.take().unwrap().write_all(bytes).unwrap();
        let result = command.wait_with_output().unwrap();
        assert!(result.status.success());
        MigrationDigest::admitted(
            String::from_utf8(result.stdout)
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap(),
        )
    }
}

struct ShippedOrder;

impl MigrationArtifactNameOrder for ShippedOrder {
    /// Order the stock catalog's distinct numeric filename prefixes.
    fn compare_names(&self, left: &str, right: &str) -> std::cmp::Ordering {
        left[..4].cmp(&right[..4])
    }
}

/// Verify actual shipped artifact hashes and one commit per reviewed tail, without a database.
#[test]
fn checked_migration_executes_each_reviewed_bridge_as_one_transaction() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../packages/database/migrations");
    let catalog = load_migration_artifacts(
        &path,
        &ProcessMigrationArtifactFilesystem,
        &ShippedOrder,
        &mut TestDigest,
    )
    .unwrap();
    // cover all reviewed atomic execution variants and the distinct ordinary path
    for (mode, from, to, expected_commits) in [
        (MigrationMode::AtomicMaintenanceV14, 18, 21, 2),
        (MigrationMode::AtomicNormal21To23, 21, 23, 2),
        (MigrationMode::AtomicDirect18To23, 18, 23, 2),
        (MigrationMode::AtomicDirect18To23Index600s, 18, 23, 2),
        (MigrationMode::Ordinary, 21, 23, 3),
    ] {
        let mut port = FixturePort {
            history: catalog[..from].iter().map(applied).collect(),
            trace: Vec::new(),
            fail_script: None,
        };
        let result = run_checked_migrations(
            &mut port,
            &mut LoadedCatalog(catalog[..to].to_vec()),
            &mut TestDigest,
            mode,
            MigrationSessionTimeouts {
                lock_ms: 10_000,
                statement_ms: 30_000,
            },
        )
        .unwrap();
        assert_eq!(result.server_version_num, 160003);
        assert_eq!(result.migrations.current.len(), from);
        assert_eq!(result.migrations.applied.len(), to - from);
        assert_eq!(
            port.trace
                .iter()
                .filter(|entry| matches!(entry, Trace::Commit))
                .count(),
            expected_commits
        );
        assert_eq!(port.trace[0], Trace::Version);
        assert_eq!(
            port.trace[1],
            Trace::Configure(
                "SELECT set_config('lock_timeout', $1, false)",
                "10000ms".to_owned()
            )
        );
        assert_eq!(
            port.trace[2],
            Trace::Configure(
                "SELECT set_config('statement_timeout', $1, false)",
                "30000ms".to_owned()
            )
        );
        assert_eq!(port.trace[3], Trace::Lock(MIGRATION_ADVISORY_LOCK_KEY));
        assert_eq!(
            port.trace[5],
            Trace::Control(MIGRATION_LEDGER_BOOTSTRAP_SQL)
        );
        assert_eq!(port.trace[7], Trace::ReadHistory);
        assert_eq!(port.trace.last(), Some(&Trace::Release));
        assert_eq!(
            port.trace
                .contains(&Trace::Control("SET LOCAL statement_timeout = '120s';")),
            mode == MigrationMode::AtomicDirect18To23Index600s,
        );
    }

    let mut port = FixturePort {
        history: catalog[..21].iter().map(applied).collect(),
        trace: Vec::new(),
        fail_script: Some(catalog[22].name.clone()),
    };
    let result = run_checked_migrations(
        &mut port,
        &mut LoadedCatalog(catalog),
        &mut TestDigest,
        MigrationMode::AtomicNormal21To23,
        MigrationSessionTimeouts {
            lock_ms: 10_000,
            statement_ms: 30_000,
        },
    );
    assert!(matches!(result, Err(CheckedMigrationError::Command(_))));
    assert_eq!(
        port.trace
            .iter()
            .filter(|entry| matches!(entry, Trace::Commit))
            .count(),
        1
    );
    assert!(port.trace.ends_with(&[
        Trace::Rollback,
        Trace::Unlock(MIGRATION_ADVISORY_LOCK_KEY),
        Trace::Release
    ]));
}

impl MigrationCommandPort for FixturePort {
    type Error = FixtureError;

    /// record one advisory lock
    fn acquire_advisory_lock(&mut self, key: i64) -> Result<(), Self::Error> {
        self.trace.push(Trace::Lock(key));
        Ok(())
    }

    /// return the immutable fixture history
    fn read_applied_migrations(&mut self) -> Result<Vec<AppliedMigration>, Self::Error> {
        self.trace.push(Trace::ReadHistory);
        Ok(self.history.clone())
    }

    /// record one transaction start
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push(Trace::Begin);
        Ok(())
    }

    /// observe one opaque script payload
    fn submit_script(&mut self, script: &MigrationScript) -> Result<(), Self::Error> {
        self.trace.push(Trace::Script {
            name: script.name.clone(),
            sql: script.sql.clone(),
        });

        // inject the selected script failure
        if self.fail_script.as_deref() == Some(script.name.as_str()) {
            return Err(FixtureError(format!("script failed: {}", script.name)));
        }

        Ok(())
    }

    /// observe one ledger record
    fn record_migration(
        &mut self,
        name: &str,
        digest: &MigrationDigest,
    ) -> Result<(), Self::Error> {
        self.trace.push(Trace::Record {
            name: name.to_owned(),
            digest: digest.as_str().to_owned(),
        });
        Ok(())
    }

    /// record one transaction commit
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push(Trace::Commit);
        Ok(())
    }

    /// record one transaction rollback
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push(Trace::Rollback);
        Ok(())
    }

    /// record one advisory unlock
    fn release_advisory_lock(&mut self, key: i64) -> Result<(), Self::Error> {
        self.trace.push(Trace::Unlock(key));
        Ok(())
    }

    /// record client release
    fn release_client(&mut self) {
        self.trace.push(Trace::Release);
    }
}

/// build four real repository scripts
fn catalog() -> Vec<MigrationScript> {
    vec![
        MigrationScript {
            name: "0001_initial_weather.sql".to_owned(),
            digest: MigrationDigest::admitted(
                "f4264482606a3476b4b54e3089e66c900f0ad5104ea678a9bf17e5e5ba385a62",
            ),
            sql: INITIAL_SQL.to_owned(),
        },
        MigrationScript {
            name: "0002_worker_migration_readiness.sql".to_owned(),
            digest: MigrationDigest::admitted(
                "954731c9ddb791c85defc3bffd887a3d30e9ff9fb5c9dac96e050ac52a796f03",
            ),
            sql: READINESS_SQL.to_owned(),
        },
        MigrationScript {
            name: "0003_ecowitt_measurements.sql".to_owned(),
            digest: MigrationDigest::admitted(
                "250e9b49176fda732b23f797a6dbce6878ca93818fa1bf6a408b5d6611ee5a48",
            ),
            sql: ECOWITT_SQL.to_owned(),
        },
        MigrationScript {
            name: "0004_tempest_metadata.sql".to_owned(),
            digest: MigrationDigest::admitted(
                "5487c90a4e70f8e049f9ed48c3ee3f025eb9f1908ed04a1febfb736c22f30d34",
            ),
            sql: TEMPEST_SQL.to_owned(),
        },
    ]
}

/// convert one artifact to a ledger row
fn applied(script: &MigrationScript) -> AppliedMigration {
    AppliedMigration {
        name: script.name.clone(),
        digest: script.digest.clone(),
    }
}

// verifies exact successful command order
#[test]
fn executes_each_pending_script_in_its_own_transaction() {
    let catalog = catalog();
    let mut port = FixturePort {
        history: vec![applied(&catalog[0]), applied(&catalog[1])],
        trace: Vec::new(),
        fail_script: None,
    };

    let result = execute_migrations(&catalog, &mut port).expect("migration execution");

    assert_eq!(
        result.current,
        [
            "0001_initial_weather.sql",
            "0002_worker_migration_readiness.sql"
        ]
    );
    assert_eq!(
        result.applied,
        ["0003_ecowitt_measurements.sql", "0004_tempest_metadata.sql"]
    );
    assert_eq!(
        port.trace,
        vec![
            Trace::Lock(MIGRATION_ADVISORY_LOCK_KEY),
            Trace::ReadHistory,
            Trace::Begin,
            Trace::Script {
                name: "0003_ecowitt_measurements.sql".to_owned(),
                sql: ECOWITT_SQL.to_owned(),
            },
            Trace::Record {
                name: "0003_ecowitt_measurements.sql".to_owned(),
                digest: catalog[2].digest.as_str().to_owned(),
            },
            Trace::Commit,
            Trace::Begin,
            Trace::Script {
                name: "0004_tempest_metadata.sql".to_owned(),
                sql: TEMPEST_SQL.to_owned(),
            },
            Trace::Record {
                name: "0004_tempest_metadata.sql".to_owned(),
                digest: catalog[3].digest.as_str().to_owned(),
            },
            Trace::Commit,
            Trace::Unlock(MIGRATION_ADVISORY_LOCK_KEY),
            Trace::Release,
        ]
    );
}

// verifies rollback and stop behavior
#[test]
fn rolls_back_the_failed_script_and_stops_the_tail() {
    let catalog = catalog();
    let mut port = FixturePort {
        history: vec![applied(&catalog[0])],
        trace: Vec::new(),
        fail_script: Some("0003_ecowitt_measurements.sql".to_owned()),
    };

    let error = execute_migrations(&catalog, &mut port).expect_err("script failure");

    assert_eq!(
        error,
        MigrationExecutionError::Command(FixtureError(
            "script failed: 0003_ecowitt_measurements.sql".to_owned()
        ))
    );
    assert_eq!(
        port.trace,
        vec![
            Trace::Lock(MIGRATION_ADVISORY_LOCK_KEY),
            Trace::ReadHistory,
            Trace::Begin,
            Trace::Script {
                name: "0002_worker_migration_readiness.sql".to_owned(),
                sql: READINESS_SQL.to_owned(),
            },
            Trace::Record {
                name: "0002_worker_migration_readiness.sql".to_owned(),
                digest: catalog[1].digest.as_str().to_owned(),
            },
            Trace::Commit,
            Trace::Begin,
            Trace::Script {
                name: "0003_ecowitt_measurements.sql".to_owned(),
                sql: ECOWITT_SQL.to_owned(),
            },
            Trace::Rollback,
            Trace::Unlock(MIGRATION_ADVISORY_LOCK_KEY),
            Trace::Release,
        ]
    );
}

// verifies planning failure cleanup without a transaction
#[test]
fn releases_lock_and_client_after_history_rejection() {
    let catalog = catalog();
    let mut port = FixturePort {
        history: vec![AppliedMigration {
            name: catalog[0].name.clone(),
            digest: MigrationDigest::admitted("0".repeat(64)),
        }],
        trace: Vec::new(),
        fail_script: None,
    };

    let error = execute_migrations(&catalog, &mut port).expect_err("history drift");

    assert_eq!(
        error,
        MigrationExecutionError::Planning(MigrationPlanningError::ChecksumMismatch {
            name: "0001_initial_weather.sql".to_owned(),
        })
    );
    assert_eq!(
        port.trace,
        vec![
            Trace::Lock(MIGRATION_ADVISORY_LOCK_KEY),
            Trace::ReadHistory,
            Trace::Unlock(MIGRATION_ADVISORY_LOCK_KEY),
            Trace::Release,
        ]
    );
}

// verifies empty artifacts fail before history lookup
#[test]
fn releases_lock_and_client_for_an_empty_catalog() {
    let mut port = FixturePort {
        history: Vec::new(),
        trace: Vec::new(),
        fail_script: None,
    };

    let error = execute_migrations(&[], &mut port).expect_err("empty catalog");

    assert_eq!(
        error,
        MigrationExecutionError::Planning(MigrationPlanningError::EmptyCatalog)
    );
    assert_eq!(
        port.trace,
        vec![
            Trace::Lock(MIGRATION_ADVISORY_LOCK_KEY),
            Trace::Unlock(MIGRATION_ADVISORY_LOCK_KEY),
            Trace::Release,
        ]
    );
}
