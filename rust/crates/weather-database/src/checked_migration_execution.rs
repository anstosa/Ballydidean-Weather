//! Run the retained migration command sequence over an explicitly supplied driver session.

use std::path::Path;

use crate::{
    AtomicMigrationMode, AtomicMigrationPlanningError, MIGRATION_ADVISORY_LOCK_KEY,
    MigrationArtifactFilesystem, MigrationArtifactLoadError, MigrationArtifactNameOrder,
    MigrationArtifactSha256, MigrationCommandPort, MigrationDigest, MigrationExecutionResult,
    MigrationPlanningError, MigrationScript, PostgresServerVersionReadPlan,
    PostgresServerVersionReadRow, UnsupportedPostgresVersionError,
    admit_supported_postgres_version, load_migration_artifacts, plan_atomic_migration,
    plan_migrations, postgres_server_version_read_plan,
};

/// Exact retained ledger bootstrap statement; never executed without a supplied migration driver.
pub const MIGRATION_LEDGER_BOOTSTRAP_SQL: &str = "
      CREATE TABLE IF NOT EXISTS schema_migrations (
        name text PRIMARY KEY,
        checksum char(64) NOT NULL CHECK (checksum ~ '^[a-f0-9]{64}$'),
        applied_at timestamptz NOT NULL DEFAULT clock_timestamp()
      )
    ";
const LOCK_TIMEOUT_SQL: &str = "SELECT set_config('lock_timeout', $1, false)";
const STATEMENT_TIMEOUT_SQL: &str = "SELECT set_config('statement_timeout', $1, false)";
const INDEX_TIMEOUT_RESTORE_SQL: &str = "SET LOCAL statement_timeout = '120s';";
const EXTENDED_INDEX_MIGRATION: &str = "0023_forecast_product_read_index.sql";
const EXTENDED_INDEX_SHA256: &str =
    "7e48acb14710a625cdcbb796bf1bf49ab4e00bf1aeb9bb763ac86410e08663e0";

/// Complete migration mode selected by the retained one-shot command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationMode {
    Ordinary,
    AtomicMaintenanceV14,
    AtomicNormal21To23,
    AtomicDirect18To23,
    AtomicDirect18To23Index600s,
}

impl MigrationMode {
    /// Select the exact reviewed plan or leave ordinary per-file execution unchanged.
    const fn atomic(self) -> Option<AtomicMigrationMode> {
        match self {
            Self::Ordinary => None,
            Self::AtomicMaintenanceV14 => Some(AtomicMigrationMode::Maintenance18To21),
            Self::AtomicNormal21To23 => Some(AtomicMigrationMode::Normal21To23),
            Self::AtomicDirect18To23 => Some(AtomicMigrationMode::Direct18To23),
            Self::AtomicDirect18To23Index600s => Some(AtomicMigrationMode::Direct18To23Index600),
        }
    }
}

/// Already selected per-session timeout values from the database configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationSessionTimeouts {
    pub lock_ms: u32,
    pub statement_ms: u32,
}

/// A single acquired driver session used throughout migration.
pub trait CheckedMigrationSession: MigrationCommandPort {
    /// Read the actual server version before configuring waits or taking the lock.
    fn query_server_version(
        &mut self,
        plan: PostgresServerVersionReadPlan,
    ) -> Result<Option<PostgresServerVersionReadRow>, Self::Error>;

    /// Bind the millisecond text value to the exact existing session statement.
    fn configure_timeout(&mut self, sql: &'static str, value: &str) -> Result<(), Self::Error>;

    /// Run one retained control statement in the already selected transaction.
    fn execute_control_statement(&mut self, sql: &'static str) -> Result<(), Self::Error>;
}

/// Read local SQL only after the source has taken its session lock and bootstrapped the ledger.
pub trait CheckedMigrationCatalog {
    type Error;

    /// Load the ordered immutable artifact scripts and their admitted SHA-256.
    fn load_catalog(
        &mut self,
        sha256: &mut impl MigrationArtifactSha256,
    ) -> Result<Vec<MigrationScript>, Self::Error>;
}

/// The concrete read-only migration directory plus its source ordering owner.
pub struct FilesystemCheckedMigrationCatalog<'a, Filesystem, Order> {
    pub directory: &'a Path,
    pub filesystem: &'a Filesystem,
    pub order: &'a Order,
}

impl<Filesystem: MigrationArtifactFilesystem, Order: MigrationArtifactNameOrder>
    CheckedMigrationCatalog for FilesystemCheckedMigrationCatalog<'_, Filesystem, Order>
{
    type Error = MigrationArtifactLoadError<Filesystem::Error>;

    /// Load the original bytes through the shared filesystem/runtime readiness catalog.
    fn load_catalog(
        &mut self,
        sha256: &mut impl MigrationArtifactSha256,
    ) -> Result<Vec<MigrationScript>, Self::Error> {
        load_migration_artifacts(self.directory, self.filesystem, self.order, sha256)
    }
}

/// Ordinary migration output plus the admitted server and exact alternate-bridge evidence.
#[derive(Debug, PartialEq, Eq)]
pub struct CheckedMigrationResult {
    pub migrations: MigrationExecutionResult,
    pub server_version_num: u64,
    pub evidence: Option<AtomicMigrationEvidence>,
}

/// Exact receipt fields for a reviewed alternate migration ledger.
#[derive(Debug, PartialEq, Eq)]
pub struct AtomicMigrationEvidence {
    pub execution_variant: &'static str,
    pub history_sha256: Option<MigrationDigest>,
    pub revised_20_sha256: Option<MigrationDigest>,
    pub revised_23_sha256: Option<MigrationDigest>,
}

/// Preserve the exact owning stage without submitting any statement from a failure response.
#[derive(Debug)]
pub enum CheckedMigrationError<PortError, CatalogError> {
    Command(PortError),
    Catalog(CatalogError),
    UnsupportedPostgres(UnsupportedPostgresVersionError),
    InvalidTimeouts,
    Planning(MigrationPlanningError),
    Atomic(AtomicMigrationPlanningError),
}

/// Run one migration session and return exactly what this session observed or committed.
pub fn run_checked_migrations<Port, Catalog, Sha256>(
    port: &mut Port,
    catalog: &mut Catalog,
    sha256: &mut Sha256,
    mode: MigrationMode,
    timeouts: MigrationSessionTimeouts,
) -> Result<CheckedMigrationResult, CheckedMigrationError<Port::Error, Catalog::Error>>
where
    Port: CheckedMigrationSession,
    Catalog: CheckedMigrationCatalog,
    Sha256: MigrationArtifactSha256,
{
    let mut lock_held = false;
    let result = (|| {
        let row = port
            .query_server_version(postgres_server_version_read_plan())
            .map_err(CheckedMigrationError::Command)?;
        let server_version_num = admit_supported_postgres_version(row.as_ref())
            .map_err(CheckedMigrationError::UnsupportedPostgres)?;
        // mirror the source's paired timeout admission before touching either setting
        if !(100..=60_000).contains(&timeouts.lock_ms)
            || !(100..=300_000).contains(&timeouts.statement_ms)
        {
            return Err(CheckedMigrationError::InvalidTimeouts);
        }
        port.configure_timeout(LOCK_TIMEOUT_SQL, &format!("{}ms", timeouts.lock_ms))
            .map_err(CheckedMigrationError::Command)?;
        port.configure_timeout(
            STATEMENT_TIMEOUT_SQL,
            &format!("{}ms", timeouts.statement_ms),
        )
        .map_err(CheckedMigrationError::Command)?;
        port.acquire_advisory_lock(MIGRATION_ADVISORY_LOCK_KEY)
            .map_err(CheckedMigrationError::Command)?;
        lock_held = true;
        transaction(port, |port| {
            port.execute_control_statement(MIGRATION_LEDGER_BOOTSTRAP_SQL)
        })
        .map_err(CheckedMigrationError::Command)?;
        let scripts = catalog
            .load_catalog(sha256)
            .map_err(CheckedMigrationError::Catalog)?;
        let applied = port
            .read_applied_migrations()
            .map_err(CheckedMigrationError::Command)?;
        let (migrations, evidence) = execute_pending(port, &scripts, &applied, sha256, mode)?;
        Ok(CheckedMigrationResult {
            migrations,
            server_version_num,
            evidence,
        })
    })();

    // keep advisory unlock before client release and preserve the source error precedence
    if lock_held {
        port.release_advisory_lock(MIGRATION_ADVISORY_LOCK_KEY)
            .map_err(CheckedMigrationError::Command)?;
    }
    port.release_client();
    result
}

type PendingResult<PortError, CatalogError> = Result<
    (MigrationExecutionResult, Option<AtomicMigrationEvidence>),
    CheckedMigrationError<PortError, CatalogError>,
>;

/// Run ordinary scripts individually or one reviewed atomic suffix in a single transaction.
fn execute_pending<Port, CatalogError>(
    port: &mut Port,
    scripts: &[MigrationScript],
    applied: &[crate::AppliedMigration],
    sha256: &mut impl MigrationArtifactSha256,
    mode: MigrationMode,
) -> PendingResult<Port::Error, CatalogError>
where
    Port: CheckedMigrationSession,
{
    // preserve per-file commits in ordinary mode
    let Some(atomic) = mode.atomic() else {
        let plan = plan_migrations(scripts, applied).map_err(CheckedMigrationError::Planning)?;
        let mut submitted = Vec::with_capacity(plan.pending.len());
        // commit only one current file and its ledger row at a time
        for script in plan.pending {
            transaction(port, |port| submit_with_ledger(port, script))
                .map_err(CheckedMigrationError::Command)?;
            submitted.push(script.name.clone());
        }
        return Ok((
            MigrationExecutionResult {
                applied: submitted,
                current: plan
                    .current
                    .into_iter()
                    .map(|script| script.name.clone())
                    .collect(),
            },
            None,
        ));
    };

    let plan = plan_atomic_migration(atomic, scripts, applied, sha256)
        .map_err(CheckedMigrationError::Atomic)?;
    let pending = &plan.pending;
    // a reviewed already-current target must not open another migration transaction
    if !pending.is_empty() {
        transaction(port, |port| {
            // keep the entire admitted suffix inside this single unit of work
            for script in pending {
                submit_with_ledger(port, script)?;
            }
            Ok(())
        })
        .map_err(CheckedMigrationError::Command)?;
    }
    Ok((
        MigrationExecutionResult {
            applied: pending.iter().map(|script| script.name.clone()).collect(),
            current: plan.current,
        },
        plan.execution_variant.map(
            // include alternate ledger evidence only on the reviewed direct lanes
            |execution_variant| AtomicMigrationEvidence {
                execution_variant,
                history_sha256: plan.history_sha256,
                revised_20_sha256: plan.revised_20_sha256,
                revised_23_sha256: plan.revised_23_sha256,
            },
        ),
    ))
}

/// Submit exact script and record its digest with the retained index timeout restoration.
fn submit_with_ledger<Port: CheckedMigrationSession>(
    port: &mut Port,
    script: &MigrationScript,
) -> Result<(), Port::Error> {
    port.submit_script(script)?;
    // restore the original statement cap only after the reviewed extended index bytes
    if script.name == EXTENDED_INDEX_MIGRATION && script.digest.as_str() == EXTENDED_INDEX_SHA256 {
        port.execute_control_statement(INDEX_TIMEOUT_RESTORE_SQL)?;
    }
    port.record_migration(&script.name, &script.digest)
}

/// Use Node's begin/commit/rollback order and its rollback-error precedence.
fn transaction<Port, Operation>(port: &mut Port, operation: Operation) -> Result<(), Port::Error>
where
    Port: CheckedMigrationSession,
    Operation: FnOnce(&mut Port) -> Result<(), Port::Error>,
{
    port.begin_transaction()?;
    let result = operation(port).and_then(|()| port.commit_transaction());
    // return the rollback error, when present, before the operation error
    if let Err(error) = result {
        port.rollback_transaction()?;
        return Err(error);
    }
    Ok(())
}
