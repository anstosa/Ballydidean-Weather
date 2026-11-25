use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::{
    AppliedMigration, MigrationArtifactSha256, MigrationDigest, MigrationPlanningError,
    MigrationScript, plan_migrations,
};

pub const MAINTENANCE_18_TO_21_HISTORY_SHA256: &str =
    "adb2aade2b019eb0e7a4df5b5bda38017ecbe143613df79cd873f67659068c93";
pub const NORMAL_21_TO_23_HISTORY_SHA256: &str =
    "1db25d8a515e85b4c71c3a3dbe4a1f61ebdfdf14e095a24b307daa039dfb5201";
pub const DIRECT_18_TO_23_HISTORY_SHA256: &str =
    "3fa9ed85218278aa3b20537d9fbe9b88ef1adee7d7b2f80fde832c45d9cb4554";
pub const DIRECT_18_TO_23_INDEX_600_HISTORY_SHA256: &str =
    "1cfd6251eeb66d443e36d661299c9e49e73465fab55e5dab2cfa4ee86dd36ad2";
pub const DIRECT_18_TO_23_EXECUTION_VARIANT: &str = "atomic-direct-18-to-23-revised-0020";
pub const DIRECT_18_TO_23_INDEX_600_EXECUTION_VARIANT: &str =
    "atomic-direct-18-to-23-revised-0020-index-600s";
pub const REVISED_REVISION_FRONTIER_SHA256: &str =
    "58d50e88fa767212865e222ec02e55283ceac5b00538aa2898b5ebc2873b28f6";
pub const REVISED_FORECAST_INDEX_SHA256: &str =
    "7e48acb14710a625cdcbb796bf1bf49ab4e00bf1aeb9bb763ac86410e08663e0";

const REVISION_FRONTIER_MIGRATION_NAME: &str = "0020_adjustment_revision_frontier.sql";
const ORIGINAL_REVISION_FRONTIER_SHA256: &str =
    "56027b4f2cb2c3c83e746f14dac753b75d573753934898fa7a52f78faa56499c";
const REVISION_RECEIPT_CHECK: &str =
    "CHECK (weather_adjustment_revision_receipt_valid_v1(adjustment_revision_receipt));";
const DEFERRED_REVISION_RECEIPT_CHECK: &str =
    "CHECK (weather_adjustment_revision_receipt_valid_v1(adjustment_revision_receipt)) NOT VALID;";
const FORECAST_INDEX_MIGRATION_NAME: &str = "0023_forecast_product_read_index.sql";
const ORIGINAL_FORECAST_INDEX_SHA256: &str =
    "e637542cb2e742fe8f22f60313a11f00adbc1d140a811ee718524849fcece12a";
const FORECAST_INDEX_TIMEOUT: &str = "SET LOCAL statement_timeout = '120s';";
const EXTENDED_FORECAST_INDEX_TIMEOUT: &str = "SET LOCAL statement_timeout = '600s';";

/// One mutually exclusive reviewed atomic migration lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicMigrationMode {
    Maintenance18To21,
    Normal21To23,
    Direct18To23,
    Direct18To23Index600,
}

/// Fully admitted atomic tail ready for one transaction owned by the executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtomicMigrationPlan {
    pub mode: AtomicMigrationMode,
    pub current: Vec<String>,
    pub pending: Vec<MigrationScript>,
    pub execution_variant: Option<&'static str>,
    pub history_sha256: Option<MigrationDigest>,
    pub revised_20_sha256: Option<MigrationDigest>,
    pub revised_23_sha256: Option<MigrationDigest>,
}

/// Exact reviewed-lane admission failure before any database mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtomicMigrationPlanningError {
    Planning(MigrationPlanningError),
    ExactBridgeRequired(AtomicMigrationMode),
    RevisionCheckCount,
    RevisionChecksumMismatch,
    ForecastIndexArtifactRequired,
    ForecastIndexTimeoutCount,
    ForecastIndexChecksumMismatch,
}

impl Display for AtomicMigrationPlanningError {
    /// Render the existing reviewed-lane diagnostics.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Planning(error) => Display::fmt(error, formatter),
            Self::ExactBridgeRequired(mode) => formatter.write_str(match mode {
                AtomicMigrationMode::Maintenance18To21 => {
                    "atomic maintenance v14 requires the exact 18-to-21 migration bridge"
                }
                AtomicMigrationMode::Normal21To23 => {
                    "atomic normal forecast upgrade requires the exact 21-to-23 migration bridge"
                }
                AtomicMigrationMode::Direct18To23 => {
                    "atomic direct migration requires the exact 18-to-23 revised migration bridge"
                }
                AtomicMigrationMode::Direct18To23Index600 => {
                    "atomic direct index-600 migration requires the exact 18-to-23 revised migration bridge"
                }
            }),
            Self::RevisionCheckCount => formatter
                .write_str("atomic direct migration requires four exact revision receipt checks"),
            Self::RevisionChecksumMismatch => {
                formatter.write_str("atomic direct revised migration checksum mismatch")
            }
            Self::ForecastIndexArtifactRequired => formatter.write_str(
                "atomic direct index-600 migration requires the exact forecast index artifact",
            ),
            Self::ForecastIndexTimeoutCount => formatter.write_str(
                "atomic direct index-600 migration requires one exact index timeout",
            ),
            Self::ForecastIndexChecksumMismatch => formatter
                .write_str("atomic direct revised forecast index checksum mismatch"),
        }
    }
}

impl Error for AtomicMigrationPlanningError {}

/// Admit one reviewed catalog and exact source/target ledger into an atomic tail.
pub fn plan_atomic_migration<Sha>(
    mode: AtomicMigrationMode,
    catalog: &[MigrationScript],
    applied: &[AppliedMigration],
    sha256: &mut Sha,
) -> Result<AtomicMigrationPlan, AtomicMigrationPlanningError>
where
    Sha: MigrationArtifactSha256,
{
    // verify the original applied prefix before entering any atomic lane
    plan_migrations(catalog, applied).map_err(AtomicMigrationPlanningError::Planning)?;
    let mut selected = catalog.to_vec();
    let canonical_history = hash_history(&selected, sha256);

    let (source_count, target_count, reviewed_history) = match mode {
        AtomicMigrationMode::Maintenance18To21 => (18, 21, MAINTENANCE_18_TO_21_HISTORY_SHA256),
        AtomicMigrationMode::Normal21To23
        | AtomicMigrationMode::Direct18To23
        | AtomicMigrationMode::Direct18To23Index600 => (
            if matches!(mode, AtomicMigrationMode::Normal21To23) {
                21
            } else {
                18
            },
            23,
            NORMAL_21_TO_23_HISTORY_SHA256,
        ),
    };

    // bind every mode to the exact canonical catalog before applying reviewed substitutions
    if selected.len() != target_count || canonical_history.as_str() != reviewed_history {
        return Err(AtomicMigrationPlanningError::ExactBridgeRequired(mode));
    }

    let direct = matches!(
        mode,
        AtomicMigrationMode::Direct18To23 | AtomicMigrationMode::Direct18To23Index600
    );
    // derive only the two reviewed direct-lane substitutions
    if direct {
        revise_frontier(&mut selected, sha256)?;
    }
    if mode == AtomicMigrationMode::Direct18To23Index600 {
        revise_forecast_index(&mut selected, sha256)?;
    }

    let selected_history = hash_history(&selected, sha256);
    let expected_selected_history = match mode {
        AtomicMigrationMode::Direct18To23 => DIRECT_18_TO_23_HISTORY_SHA256,
        AtomicMigrationMode::Direct18To23Index600 => DIRECT_18_TO_23_INDEX_600_HISTORY_SHA256,
        _ => reviewed_history,
    };
    let exact_source = applied.len() == source_count;
    // only the direct lanes require the exact target's complete alternate history
    let exact_target = applied.len() == target_count
        && (!direct || hash_applied_history(applied, sha256).as_str() == expected_selected_history);

    // reject partial tails and alternate target ledgers
    if selected_history.as_str() != expected_selected_history || (!exact_source && !exact_target) {
        return Err(AtomicMigrationPlanningError::ExactBridgeRequired(mode));
    }

    let plan =
        plan_migrations(&selected, applied).map_err(AtomicMigrationPlanningError::Planning)?;
    Ok(AtomicMigrationPlan {
        mode,
        current: plan
            .current
            .iter()
            .map(|migration| migration.name.clone())
            .collect(),
        pending: plan.pending.into_iter().cloned().collect(),
        execution_variant: match mode {
            AtomicMigrationMode::Direct18To23 => Some(DIRECT_18_TO_23_EXECUTION_VARIANT),
            AtomicMigrationMode::Direct18To23Index600 => {
                Some(DIRECT_18_TO_23_INDEX_600_EXECUTION_VARIANT)
            }
            _ => None,
        },
        history_sha256: matches!(
            mode,
            AtomicMigrationMode::Direct18To23 | AtomicMigrationMode::Direct18To23Index600
        )
        .then_some(selected_history),
        revised_20_sha256: matches!(
            mode,
            AtomicMigrationMode::Direct18To23 | AtomicMigrationMode::Direct18To23Index600
        )
        .then(|| MigrationDigest::admitted(REVISED_REVISION_FRONTIER_SHA256)),
        revised_23_sha256: (mode == AtomicMigrationMode::Direct18To23Index600)
            .then(|| MigrationDigest::admitted(REVISED_FORECAST_INDEX_SHA256)),
    })
}

/// Replace exactly four reviewed frontier checks and bind the resulting bytes.
fn revise_frontier<Sha: MigrationArtifactSha256>(
    catalog: &mut [MigrationScript],
    sha256: &mut Sha,
) -> Result<(), AtomicMigrationPlanningError> {
    let migration = catalog
        .iter_mut()
        .find(|migration| migration.name == REVISION_FRONTIER_MIGRATION_NAME)
        .filter(|migration| migration.digest.as_str() == ORIGINAL_REVISION_FRONTIER_SHA256)
        .ok_or(AtomicMigrationPlanningError::ExactBridgeRequired(
            AtomicMigrationMode::Direct18To23,
        ))?;
    if migration.sql.matches(REVISION_RECEIPT_CHECK).count() != 4 {
        return Err(AtomicMigrationPlanningError::RevisionCheckCount);
    }
    let revised = migration
        .sql
        .replace(REVISION_RECEIPT_CHECK, DEFERRED_REVISION_RECEIPT_CHECK);
    let digest = sha256.sha256(revised.as_bytes());
    if digest.as_str() != REVISED_REVISION_FRONTIER_SHA256 {
        return Err(AtomicMigrationPlanningError::RevisionChecksumMismatch);
    }
    migration.sql = revised;
    migration.digest = digest;
    Ok(())
}

/// Replace exactly one reviewed forecast-index timeout and bind the resulting bytes.
fn revise_forecast_index<Sha: MigrationArtifactSha256>(
    catalog: &mut [MigrationScript],
    sha256: &mut Sha,
) -> Result<(), AtomicMigrationPlanningError> {
    let migration = catalog
        .iter_mut()
        .find(|migration| migration.name == FORECAST_INDEX_MIGRATION_NAME)
        .filter(|migration| migration.digest.as_str() == ORIGINAL_FORECAST_INDEX_SHA256)
        .ok_or(AtomicMigrationPlanningError::ForecastIndexArtifactRequired)?;
    if migration.sql.matches(FORECAST_INDEX_TIMEOUT).count() != 1 {
        return Err(AtomicMigrationPlanningError::ForecastIndexTimeoutCount);
    }
    let revised =
        migration
            .sql
            .replacen(FORECAST_INDEX_TIMEOUT, EXTENDED_FORECAST_INDEX_TIMEOUT, 1);
    let digest = sha256.sha256(revised.as_bytes());
    if digest.as_str() != REVISED_FORECAST_INDEX_SHA256 {
        return Err(AtomicMigrationPlanningError::ForecastIndexChecksumMismatch);
    }
    migration.sql = revised;
    migration.digest = digest;
    Ok(())
}

/// Construct and hash the exact ordered catalog ledger preimage.
fn hash_history<Sha: MigrationArtifactSha256>(
    migrations: &[MigrationScript],
    sha256: &mut Sha,
) -> MigrationDigest {
    let preimage = migrations
        .iter()
        .map(|migration| format!("{}:{}\n", migration.name, migration.digest.as_str()))
        .collect::<String>();
    sha256.sha256(preimage.as_bytes())
}

/// Construct and hash the exact ordered applied-ledger preimage.
fn hash_applied_history<Sha: MigrationArtifactSha256>(
    migrations: &[AppliedMigration],
    sha256: &mut Sha,
) -> MigrationDigest {
    let preimage = migrations
        .iter()
        .map(|migration| format!("{}:{}\n", migration.name, migration.digest.as_str()))
        .collect::<String>();
    sha256.sha256(preimage.as_bytes())
}
