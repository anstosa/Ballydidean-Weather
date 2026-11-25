use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::JsDateOrString;

use crate::{
    AppliedMigration, MigrationDigest, MigrationPlanningError, MigrationScript, plan_migrations,
};

const REVISION_FRONTIER_MIGRATION_NAME: &str = "0020_adjustment_revision_frontier.sql";
const FORECAST_INDEX_MIGRATION_NAME: &str = "0023_forecast_product_read_index.sql";
const DIRECT_18_TO_23_HISTORY_SHA256: &str =
    "3fa9ed85218278aa3b20537d9fbe9b88ef1adee7d7b2f80fde832c45d9cb4554";
const DIRECT_18_TO_23_INDEX_600_HISTORY_SHA256: &str =
    "1cfd6251eeb66d443e36d661299c9e49e73465fab55e5dab2cfa4ee86dd36ad2";

/// Exact connectivity probe issued before every other health read.
pub const HEALTH_PING_SQL: &str = include_str!("../fixtures/health-ping.sql");
/// Exact ordered migration ledger read used by readiness validation.
pub const HEALTH_MIGRATION_LEDGER_SQL: &str =
    include_str!("../fixtures/health-migration-ledger.sql");
/// Exact latest-worker query used after migration readiness is classified.
pub const HEALTH_LATEST_WORKER_HEARTBEAT_SQL: &str =
    include_str!("../fixtures/health-latest-worker-heartbeat.sql");

/// Closed zero-bind domain shared by all three health statements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthReadBind {}

/// One exact connectivity probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthPingReadPlan {
    pub binds: [HealthReadBind; 0],
    pub sql: &'static str,
}

/// One exact ordered migration-ledger query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthMigrationLedgerReadPlan {
    pub binds: [HealthReadBind; 0],
    pub sql: &'static str,
}

/// One exact latest-worker-heartbeat query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthLatestWorkerHeartbeatReadPlan {
    pub binds: [HealthReadBind; 0],
    pub sql: &'static str,
}

/// One decoded heartbeat timestamp before canonical time admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthLatestWorkerHeartbeatRow {
    pub last_loop_at: JsDateOrString,
}

/// Optional immutable-deployment authorization admitted outside this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedHealthMigrationAuthorization {
    pub history_sha256: MigrationDigest,
    pub release: String,
}

/// Local artifact and applied-ledger identity admitted by file and hash owners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedHealthMigrationIdentity {
    pub applied_history_sha256: MigrationDigest,
    pub artifacts: Vec<MigrationScript>,
    pub authorization: Option<AdmittedHealthMigrationAuthorization>,
    pub release: String,
}

/// One current migration version after exact artifact-ledger verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthMigrationReadiness {
    pub version: String,
}

/// Bounded source-compatible readiness failure caught by API health.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthMigrationReadinessError {
    AlternateHistoryMismatch,
    AuthorizationHistoryMismatch,
    AuthorizationReleaseMismatch,
    Planning(MigrationPlanningError),
    PendingArtifacts,
}

impl Display for HealthMigrationReadinessError {
    /// Render the production readiness failure categories.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlternateHistoryMismatch => {
                formatter.write_str("migration alternate history is not an exact reviewed ledger")
            }
            Self::AuthorizationHistoryMismatch => {
                formatter.write_str("migration authorization history mismatch")
            }
            Self::AuthorizationReleaseMismatch => {
                formatter.write_str("migration authorization release mismatch")
            }
            Self::Planning(error) => Display::fmt(error, formatter),
            Self::PendingArtifacts => {
                formatter.write_str("database has pending migration artifacts")
            }
        }
    }
}

impl Error for HealthMigrationReadinessError {}

/// Build the zero-bind connectivity probe.
pub const fn health_ping_read_plan() -> HealthPingReadPlan {
    HealthPingReadPlan {
        binds: [],
        sql: HEALTH_PING_SQL,
    }
}

/// Build the zero-bind ordered migration-ledger read.
pub const fn health_migration_ledger_read_plan() -> HealthMigrationLedgerReadPlan {
    HealthMigrationLedgerReadPlan {
        binds: [],
        sql: HEALTH_MIGRATION_LEDGER_SQL,
    }
}

/// Build the zero-bind latest-worker-heartbeat read.
pub const fn health_latest_worker_heartbeat_read_plan() -> HealthLatestWorkerHeartbeatReadPlan {
    HealthLatestWorkerHeartbeatReadPlan {
        binds: [],
        sql: HEALTH_LATEST_WORKER_HEARTBEAT_SQL,
    }
}

/// verify one complete local artifact catalog against the ordered applied ledger.
pub fn verify_health_migration_readiness(
    identity: &AdmittedHealthMigrationIdentity,
    applied: &[AppliedMigration],
) -> Result<HealthMigrationReadiness, HealthMigrationReadinessError> {
    let prefix_length = applied.len().min(identity.artifacts.len());
    let applied_prefix = &applied[..prefix_length];
    let plan = plan_migrations(&identity.artifacts, applied_prefix)
        .map_err(HealthMigrationReadinessError::Planning)?;

    // preserve ordinary divergence when trailing rows have no authorization
    if applied.len() > identity.artifacts.len() && identity.authorization.is_none() {
        return Err(HealthMigrationReadinessError::Planning(
            MigrationPlanningError::HistoryDiverges,
        ));
    }

    // bind any supplied authorization to the exact running release
    if identity
        .authorization
        .as_ref()
        .is_some_and(|authorization| authorization.release != identity.release)
    {
        return Err(HealthMigrationReadinessError::AuthorizationReleaseMismatch);
    }

    // admit trailing applied rows only through the exact complete-ledger digest
    if applied.len() > identity.artifacts.len()
        && identity
            .authorization
            .as_ref()
            .is_some_and(|authorization| {
                authorization.history_sha256 != identity.applied_history_sha256
            })
    {
        return Err(HealthMigrationReadinessError::AuthorizationHistoryMismatch);
    }

    // require every shipped artifact before reporting the local version
    if !plan.pending.is_empty() {
        return Err(HealthMigrationReadinessError::PendingArtifacts);
    }

    let revised_frontier = plan
        .current
        .iter()
        .zip(applied_prefix)
        .any(|(artifact, ledger)| {
            artifact.name == REVISION_FRONTIER_MIGRATION_NAME && artifact.digest != ledger.digest
        });
    let revised_index = plan
        .current
        .iter()
        .zip(applied_prefix)
        .any(|(artifact, ledger)| {
            artifact.name == FORECAST_INDEX_MIGRATION_NAME && artifact.digest != ledger.digest
        });
    let history_sha256 = identity.applied_history_sha256.as_str();

    // bind local alternate checksums to reviewed ledgers
    if applied.len() == identity.artifacts.len()
        && ((revised_index && history_sha256 != DIRECT_18_TO_23_INDEX_600_HISTORY_SHA256)
            || (revised_frontier
                && !revised_index
                && history_sha256 != DIRECT_18_TO_23_HISTORY_SHA256))
    {
        return Err(HealthMigrationReadinessError::AlternateHistoryMismatch);
    }

    let version = plan
        .current
        .last()
        .expect("nonempty migration plan after successful planning")
        .name
        .clone();
    Ok(HealthMigrationReadiness { version })
}
