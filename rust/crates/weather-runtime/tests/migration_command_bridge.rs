use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use weather_database::{
    AppliedMigration, CheckedMigrationError, CheckedMigrationSession, MigrationArtifactLoadError,
    MigrationArtifactNameOrder, MigrationArtifactSha256, MigrationCommandPort, MigrationDigest,
    MigrationMode, MigrationScript, MigrationSessionTimeouts, PostgresServerVersionReadPlan,
    PostgresServerVersionReadRow,
};
use weather_runtime::{
    CheckedMigrationCommandPool, MigrationCommandBootstrapPort, MigrationCommandEnvironment,
    run_checked_migration_command,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Error {
    Migration,
}

struct Session {
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl MigrationCommandPort for Session {
    type Error = Error;

    /// mark an unexpected advisory-lock attempt.
    fn acquire_advisory_lock(&mut self, _key: i64) -> Result<(), Self::Error> {
        panic!("unsupported server must fail before advisory lock")
    }

    /// mark an unexpected ledger read.
    fn read_applied_migrations(&mut self) -> Result<Vec<AppliedMigration>, Self::Error> {
        panic!("unsupported server must fail before ledger read")
    }

    /// mark an unexpected transaction start.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        panic!("unsupported server must fail before transaction")
    }

    /// mark an unexpected script submission.
    fn submit_script(&mut self, _script: &MigrationScript) -> Result<(), Self::Error> {
        panic!("unsupported server must fail before script submission")
    }

    /// mark an unexpected ledger write.
    fn record_migration(
        &mut self,
        _name: &str,
        _digest: &MigrationDigest,
    ) -> Result<(), Self::Error> {
        panic!("unsupported server must fail before ledger write")
    }

    /// mark an unexpected transaction commit.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        panic!("unsupported server must fail before commit")
    }

    /// mark an unexpected transaction rollback.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        panic!("unsupported server must fail before rollback")
    }

    /// mark an unexpected advisory unlock.
    fn release_advisory_lock(&mut self, _key: i64) -> Result<(), Self::Error> {
        panic!("unsupported server never acquires the advisory lock")
    }

    /// retain release of the checked-out client after version rejection.
    fn release_client(&mut self) {
        self.events.borrow_mut().push("release_client");
    }
}

impl CheckedMigrationSession for Session {
    /// return an ordinary PostgreSQL version below the admitted floor.
    fn query_server_version(
        &mut self,
        _plan: PostgresServerVersionReadPlan,
    ) -> Result<Option<PostgresServerVersionReadRow>, Self::Error> {
        self.events.borrow_mut().push("query_version");
        Ok(Some(PostgresServerVersionReadRow {
            server_version_num: "140012".to_owned(),
        }))
    }

    /// mark an unexpected timeout configuration.
    fn configure_timeout(&mut self, _sql: &'static str, _value: &str) -> Result<(), Self::Error> {
        panic!("unsupported server must fail before timeout configuration")
    }

    /// mark an unexpected control statement.
    fn execute_control_statement(&mut self, _sql: &'static str) -> Result<(), Self::Error> {
        panic!("unsupported server must fail before control statements")
    }
}

struct Pool {
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl MigrationCommandBootstrapPort for Pool {
    type Error = Error;
    type SiteBootstrap = ();
    type EcowittBootstrap = ();
    type TempestBootstrap = ();
    type TideBootstrap = ();
    type PublicStationBootstrap = ();

    /// retain any unexpected site bootstrap.
    fn bootstrap_site(&mut self, _configuration_path: &str) -> Result<(), Self::Error> {
        self.events.borrow_mut().push("site");
        Ok(())
    }

    /// retain any unexpected Ecowitt bootstrap.
    fn bootstrap_ecowitt(&mut self, _configuration_path: &str) -> Result<(), Self::Error> {
        self.events.borrow_mut().push("ecowitt");
        Ok(())
    }

    /// retain any unexpected Tempest bootstrap.
    fn bootstrap_tempest(&mut self, _configuration_path: &str) -> Result<(), Self::Error> {
        self.events.borrow_mut().push("tempest");
        Ok(())
    }

    /// retain any unexpected tide bootstrap.
    fn bootstrap_tide(&mut self, _configuration_path: &str) -> Result<(), Self::Error> {
        self.events.borrow_mut().push("tide");
        Ok(())
    }

    /// retain any unexpected public-station bootstrap.
    fn bootstrap_public_stations(&mut self, _configuration_path: &str) -> Result<(), Self::Error> {
        self.events.borrow_mut().push("stations");
        Ok(())
    }

    /// retain the mandatory pool close after the migration failure.
    fn close_pool(&mut self) -> Result<(), Self::Error> {
        self.events.borrow_mut().push("close");
        Ok(())
    }
}

impl CheckedMigrationCommandPool for Pool {
    type Session = Session;

    /// return one session sharing lifecycle observations with the pool fixture.
    fn acquire_migration_session(&mut self) -> Result<Self::Session, Self::Error> {
        self.events.borrow_mut().push("checkout");
        Ok(Session {
            events: Rc::clone(&self.events),
        })
    }

    /// retain exact classification of the unsupported-version failure.
    fn migration_error(
        &mut self,
        error: CheckedMigrationError<Self::Error, MigrationArtifactLoadError<std::io::Error>>,
    ) -> Self::Error {
        assert!(matches!(
            error,
            CheckedMigrationError::UnsupportedPostgres(_)
        ));
        self.events.borrow_mut().push("map_error");
        Error::Migration
    }
}

struct Order;

impl MigrationArtifactNameOrder for Order {
    /// provide the source-owned filename comparison if the catalog is reached.
    fn compare_names(&self, left: &str, right: &str) -> Ordering {
        left.cmp(right)
    }
}

struct Sha256;

impl MigrationArtifactSha256 for Sha256 {
    /// provide an admitted fixture digest if the catalog is reached.
    fn sha256(&mut self, _decoded_utf8_bytes: &[u8]) -> MigrationDigest {
        MigrationDigest::admitted("0".repeat(64))
    }
}

/// release the rejected session and pool without entering any bootstrap stage.
#[test]
fn unsupported_postgres_releases_session_and_closes_pool() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut pool = Pool {
        events: Rc::clone(&events),
    };
    let result = run_checked_migration_command(
        &mut pool,
        MigrationMode::Ordinary,
        MigrationCommandEnvironment::default().resolve(),
        &Order,
        &mut Sha256,
        MigrationSessionTimeouts {
            lock_ms: 1_000,
            statement_ms: 120_000,
        },
    );
    assert_eq!(result, Err(Error::Migration));
    assert_eq!(
        *events.borrow(),
        [
            "checkout",
            "query_version",
            "release_client",
            "map_error",
            "close",
        ]
    );
}
