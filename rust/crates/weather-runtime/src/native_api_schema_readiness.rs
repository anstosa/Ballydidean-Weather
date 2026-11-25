//! Read-only schema-preserving API readiness over the configured native PostgreSQL pool.

use std::fmt::{self, Display, Formatter};

use weather_database::{
    AppliedMigration, health_migration_ledger_read_plan, to_pool_configuration,
    verify_health_migration_readiness,
};

use crate::{
    NativePostgresPool, NativePostgresSession, WeatherServiceExternalReadPort,
    WeatherServicePostgresReadPort, load_current_api_process_configuration,
    native_postgres_api_external,
};

const REQUIRED_MIGRATION_VERSION: &str = "0023_forecast_product_read_index.sql";
const READ_ONLY_SQL: &str = "SET TRANSACTION READ ONLY";
const READINESS_TIMEOUT_SQL: &str = "SET LOCAL statement_timeout = '15s'";

/// One bounded successful schema-preserving readiness receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeApiSchemaReadinessReceipt {
    pub migration_version: String,
}

/// Closed readiness failure without database credentials, SQL rows, or artifact paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeApiSchemaReadinessError {
    Configuration,
    Pool,
    Checkout,
    Transaction,
    Artifacts,
    Ledger,
    Migration,
    Version,
    Rollback,
    Release,
    Close,
}

impl Display for NativeApiSchemaReadinessError {
    /// Render one bounded owning-stage diagnostic.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Configuration => "API readiness configuration failed",
            Self::Pool => "API readiness pool preparation failed",
            Self::Checkout => "API readiness database checkout failed",
            Self::Transaction => "API readiness read-only transaction failed",
            Self::Artifacts => "API readiness migration artifacts failed",
            Self::Ledger => "API readiness migration ledger failed",
            Self::Migration => "API readiness migration validation failed",
            Self::Version => "API readiness migration version differs",
            Self::Rollback => "API readiness rollback failed",
            Self::Release => "API readiness session release failed",
            Self::Close => "API readiness pool close failed",
        })
    }
}

impl std::error::Error for NativeApiSchemaReadinessError {}

/// Verify the configured migration catalog and ledger in one rolled-back read-only transaction.
pub fn run_native_api_schema_readiness()
-> Result<NativeApiSchemaReadinessReceipt, NativeApiSchemaReadinessError> {
    use NativeApiSchemaReadinessError as Error;

    let configuration =
        load_current_api_process_configuration().map_err(|_| Error::Configuration)?;
    let source = configuration.migration.clone();
    let pool = NativePostgresPool::new(to_pool_configuration(&configuration.database))
        .map_err(|_| Error::Pool)?;
    let result = run_read_only_readiness(&pool, source);
    let closed = pool.close().map_err(|_| Error::Close);
    // preserve the owning readiness failure while still requiring a clean pool close on success
    match result {
        Ok(receipt) => {
            closed?;
            Ok(receipt)
        }
        Err(error) => Err(error),
    }
}

/// Execute the exact artifact and ledger validation on one explicit session.
fn run_read_only_readiness(
    pool: &NativePostgresPool,
    source: crate::ApiProcessMigrationSource,
) -> Result<NativeApiSchemaReadinessReceipt, NativeApiSchemaReadinessError> {
    use NativeApiSchemaReadinessError as Error;

    let mut session = pool.checkout().map_err(|_| Error::Checkout)?;
    // begin exactly once before reading either local migration artifacts or the applied ledger
    if session.begin().is_err() {
        let _ = session.discard();
        return Err(Error::Transaction);
    }
    let mut external = native_postgres_api_external(source);
    let transaction = execute_read_only_validation(&mut session, &mut external);
    let rollback = session.rollback().map_err(|_| Error::Rollback);
    // discard a connection whose rollback could not be confirmed
    if rollback.is_err() {
        let _ = session.discard();
        return Err(Error::Rollback);
    }
    session.release().map_err(|_| Error::Release)?;
    transaction
}

/// Begin once, force read-only semantics and validate the exact migration version.
fn execute_read_only_validation(
    session: &mut NativePostgresSession,
    external: &mut crate::NativePostgresApiExternal,
) -> Result<NativeApiSchemaReadinessReceipt, NativeApiSchemaReadinessError> {
    use NativeApiSchemaReadinessError as Error;

    session
        .batch_execute(READ_ONLY_SQL)
        .map_err(|_| Error::Transaction)?;
    session
        .batch_execute(READINESS_TIMEOUT_SQL)
        .map_err(|_| Error::Transaction)?;
    let artifacts = external
        .admit_health_migration_artifacts()
        .map_err(|_| Error::Artifacts)?;
    let applied: Vec<AppliedMigration> = session
        .query_health_migration_ledger(&health_migration_ledger_read_plan())
        .map_err(|_| Error::Ledger)?;
    let identity = external
        .admit_health_migration_identity(artifacts, &applied)
        .map_err(|_| Error::Migration)?;
    let readiness =
        verify_health_migration_readiness(&identity, &applied).map_err(|_| Error::Migration)?;
    // require the schema-preserving release's exact final artifact, not merely any current prefix
    if readiness.version != REQUIRED_MIGRATION_VERSION {
        return Err(Error::Version);
    }
    Ok(NativeApiSchemaReadinessReceipt {
        migration_version: readiness.version,
    })
}
