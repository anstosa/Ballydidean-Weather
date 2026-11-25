use std::error::Error;
use std::fmt::{Display, Formatter};

const REVISION_FRONTIER_MIGRATION_NAME: &str = "0020_adjustment_revision_frontier.sql";
const ORIGINAL_REVISION_FRONTIER_SHA256: &str =
    "56027b4f2cb2c3c83e746f14dac753b75d573753934898fa7a52f78faa56499c";
const REVISED_REVISION_FRONTIER_SHA256: &str =
    "58d50e88fa767212865e222ec02e55283ceac5b00538aa2898b5ebc2873b28f6";
const FORECAST_INDEX_MIGRATION_NAME: &str = "0023_forecast_product_read_index.sql";
const ORIGINAL_FORECAST_INDEX_SHA256: &str =
    "e637542cb2e742fe8f22f60313a11f00adbc1d140a811ee718524849fcece12a";
const REVISED_FORECAST_INDEX_SHA256: &str =
    "7e48acb14710a625cdcbb796bf1bf49ab4e00bf1aeb9bb763ac86410e08663e0";
/// exact node migration advisory lock key
pub const MIGRATION_ADVISORY_LOCK_KEY: i64 = 8_032_416_683_782_917;

/// admitted adapter sha-256 digest
///
/// this crate compares digests but intentionally does not calculate them
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationDigest(String);

impl MigrationDigest {
    /// admit one adapter-calculated digest
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// borrow the admitted digest
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// immutable canonical migration script
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationScript {
    pub name: String,
    pub digest: MigrationDigest,
    pub sql: String,
}

/// immutable applied ledger row
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMigration {
    pub name: String,
    pub digest: MigrationDigest,
}

/// verified current and pending scripts
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationPlan<'a> {
    pub current: Vec<&'a MigrationScript>,
    pub pending: Vec<&'a MigrationScript>,
}

/// source-compatible planning failure
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationPlanningError {
    EmptyCatalog,
    HistoryDiverges,
    ChecksumMismatch { name: String },
}

/// typed migration command boundary
pub trait MigrationCommandPort {
    type Error;

    /// acquire the session advisory lock
    fn acquire_advisory_lock(&mut self, key: i64) -> Result<(), Self::Error>;

    /// read the ordered applied ledger
    fn read_applied_migrations(&mut self) -> Result<Vec<AppliedMigration>, Self::Error>;

    /// begin one migration transaction
    fn begin_transaction(&mut self) -> Result<(), Self::Error>;

    /// submit one opaque migration script
    fn submit_script(&mut self, script: &MigrationScript) -> Result<(), Self::Error>;

    /// record one applied artifact
    fn record_migration(&mut self, name: &str, digest: &MigrationDigest)
    -> Result<(), Self::Error>;

    /// commit one migration transaction
    fn commit_transaction(&mut self) -> Result<(), Self::Error>;

    /// roll back one started transaction
    fn rollback_transaction(&mut self) -> Result<(), Self::Error>;

    /// release the session advisory lock
    fn release_advisory_lock(&mut self, key: i64) -> Result<(), Self::Error>;

    /// release the acquired client boundary
    fn release_client(&mut self);
}

/// node-compatible ordinary migration result
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationExecutionResult {
    pub applied: Vec<String>,
    pub current: Vec<String>,
}

/// typed migration execution failure
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationExecutionError<PortError> {
    Planning(MigrationPlanningError),
    Command(PortError),
}

impl Display for MigrationPlanningError {
    /// render exact node error text
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the source error categories
        match self {
            Self::EmptyCatalog => {
                formatter.write_str("migration directory contains no ordered SQL files")
            }
            Self::HistoryDiverges => {
                formatter.write_str("migration history diverges from ordered artifacts")
            }
            Self::ChecksumMismatch { name } => {
                write!(formatter, "migration checksum mismatch: {name}")
            }
        }
    }
}

impl Error for MigrationPlanningError {}

impl<PortError: Display> Display for MigrationExecutionError<PortError> {
    /// render the owning failure text
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve planning and command categories
        match self {
            Self::Planning(error) => Display::fmt(error, formatter),
            Self::Command(error) => Display::fmt(error, formatter),
        }
    }
}

impl<PortError> Error for MigrationExecutionError<PortError> where PortError: Error + 'static {}

/// compare migration catalog and ledger
///
/// returned references follow the node migrator's canonical filename order
pub fn plan_migrations<'a>(
    catalog: &'a [MigrationScript],
    applied: &[AppliedMigration],
) -> Result<MigrationPlan<'a>, MigrationPlanningError> {
    // retain the source non-empty catalog invariant
    if catalog.is_empty() {
        return Err(MigrationPlanningError::EmptyCatalog);
    }

    let mut ordered = catalog.iter().collect::<Vec<_>>();
    // reproduce the filename-sorted node catalog
    ordered.sort_by(|left, right| left.name.cmp(&right.name));

    // validate the complete applied prefix
    for (index, applied_migration) in applied.iter().enumerate() {
        // require one matching artifact
        let Some(artifact) = ordered.get(index) else {
            return Err(MigrationPlanningError::HistoryDiverges);
        };

        // reject deleted or retroactively inserted files
        if artifact.name != applied_migration.name {
            return Err(MigrationPlanningError::HistoryDiverges);
        }

        // admit only source-reviewed alternate checksums
        if artifact.digest != applied_migration.digest
            && !is_reviewed_alternate_pair(artifact, applied_migration)
        {
            return Err(MigrationPlanningError::ChecksumMismatch {
                name: artifact.name.clone(),
            });
        }
    }

    let applied_count = applied.len();
    let pending = ordered.split_off(applied_count);
    Ok(MigrationPlan {
        current: ordered,
        pending,
    })
}

/// execute the ordinary migration plan through typed commands
pub fn execute_migrations<Port>(
    catalog: &[MigrationScript],
    port: &mut Port,
) -> Result<MigrationExecutionResult, MigrationExecutionError<Port::Error>>
where
    Port: MigrationCommandPort,
{
    // release the client when lock acquisition fails
    if let Err(error) = port.acquire_advisory_lock(MIGRATION_ADVISORY_LOCK_KEY) {
        port.release_client();
        return Err(MigrationExecutionError::Command(error));
    }

    let result = execute_locked_migrations(catalog, port);
    let unlock_result = port.release_advisory_lock(MIGRATION_ADVISORY_LOCK_KEY);

    // retain node cleanup ordering and unlock precedence
    if let Err(error) = unlock_result {
        return Err(MigrationExecutionError::Command(error));
    }

    port.release_client();
    result
}

/// execute one advisory-locked catalog
fn execute_locked_migrations<Port>(
    catalog: &[MigrationScript],
    port: &mut Port,
) -> Result<MigrationExecutionResult, MigrationExecutionError<Port::Error>>
where
    Port: MigrationCommandPort,
{
    // reject an empty catalog before reading history
    if catalog.is_empty() {
        return Err(MigrationExecutionError::Planning(
            MigrationPlanningError::EmptyCatalog,
        ));
    }

    let history = port
        .read_applied_migrations()
        .map_err(MigrationExecutionError::Command)?;
    let plan = plan_migrations(catalog, &history).map_err(MigrationExecutionError::Planning)?;
    // copy the exact preexisting migration names
    let current = plan
        .current
        .iter()
        .map(|script| script.name.clone())
        .collect();
    let mut applied = Vec::with_capacity(plan.pending.len());

    // execute the deterministic pending tail serially
    for script in plan.pending {
        execute_one_migration(script, port)?;
        applied.push(script.name.clone());
    }

    Ok(MigrationExecutionResult { applied, current })
}

/// execute one per-file migration transaction
fn execute_one_migration<Port>(
    script: &MigrationScript,
    port: &mut Port,
) -> Result<(), MigrationExecutionError<Port::Error>>
where
    Port: MigrationCommandPort,
{
    port.begin_transaction()
        .map_err(MigrationExecutionError::Command)?;

    // roll back a failed script submission
    if let Err(error) = port.submit_script(script) {
        return rollback_after_failure(port, error);
    }

    // roll back a failed ledger insertion
    if let Err(error) = port.record_migration(&script.name, &script.digest) {
        return rollback_after_failure(port, error);
    }

    // roll back a failed commit like node withTransaction
    if let Err(error) = port.commit_transaction() {
        return rollback_after_failure(port, error);
    }

    Ok(())
}

/// roll back and preserve node error precedence
fn rollback_after_failure<Port>(
    port: &mut Port,
    operation_error: Port::Error,
) -> Result<(), MigrationExecutionError<Port::Error>>
where
    Port: MigrationCommandPort,
{
    // let a rollback failure replace the operation failure
    match port.rollback_transaction() {
        Ok(()) => Err(MigrationExecutionError::Command(operation_error)),
        Err(rollback_error) => Err(MigrationExecutionError::Command(rollback_error)),
    }
}

/// recognize reviewed alternate pairs
fn is_reviewed_alternate_pair(artifact: &MigrationScript, applied: &AppliedMigration) -> bool {
    let revised_frontier = artifact.name == REVISION_FRONTIER_MIGRATION_NAME
        && artifact.digest.as_str() == ORIGINAL_REVISION_FRONTIER_SHA256
        && applied.digest.as_str() == REVISED_REVISION_FRONTIER_SHA256;
    let revised_index = artifact.name == FORECAST_INDEX_MIGRATION_NAME
        && artifact.digest.as_str() == ORIGINAL_FORECAST_INDEX_SHA256
        && applied.digest.as_str() == REVISED_FORECAST_INDEX_SHA256;

    revised_frontier || revised_index
}
