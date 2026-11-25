//! Source-ordered lifecycle initialization and one bounded evidence synchronization cycle.

use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::pin::Pin;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsDateOrString;

use crate::maintenance_canonical_json::valid_hash;
use crate::native_maintenance_capture_readiness::{
    NativeMaintenanceCaptureFamily, VerifiedNativeMaintenanceCaptureEpochSnapshot,
    VerifiedNativeMaintenanceCaptureEpochWitness, VerifiedNativeMaintenanceDatabaseLedger,
};
use crate::{
    ADJUSTMENT_ROLLING_SCHEDULE_SHA256, NativeAdjustmentRollingScheduleBootstrap,
    NativeAdjustmentRollingScheduleBootstrapInput, NativeMaintenanceRollingScheduleError,
    NativeWirePrimitives, build_adjustment_rolling_schedule_bootstrap,
};

const LIFECYCLE_VERSION: &str = "adjustment-registration-lifecycle-status/v4";
const SCHEDULE_VERSION: &str = "adjustment-registration-schedule-status/v3";
const SLOT_VERSION: &str = "adjustment-shadow-registration-slot/v3";

/// Closed database-owned registration slot occupancy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceRegistrationSlotState {
    BusyV2Legacy,
    BusyV3,
    Free,
}

impl NativeMaintenanceRegistrationSlotState {
    /// Return the source database state spelling.
    pub const fn as_str(self) -> &'static str {
        // retain only the three reviewed database states
        match self {
            Self::BusyV2Legacy => "busy_v2_legacy",
            Self::BusyV3 => "busy_v3",
            Self::Free => "free",
        }
    }
}

/// Exact value-blind schedule slot already verified by the lifecycle reader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceRegistrationSlot {
    pub contract_version: String,
    pub epoch_witness_sha256: String,
    pub family: NativeMaintenanceCaptureFamily,
    pub horizon_end_at: String,
    pub registration_sha256: Option<String>,
    pub schedule_contract_sha256: String,
    pub state: NativeMaintenanceRegistrationSlotState,
    pub terminal_at: Option<String>,
}

/// One verified lifecycle row narrowed to its schedule-owned slot projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceRegistrationLifecycleEntry {
    pub slot: VerifiedNativeMaintenanceRegistrationSlot,
}

/// Full verified lifecycle head required after idempotent schedule initialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceRegistrationLifecycleStatus {
    pub contract_version: String,
    pub entries: Vec<VerifiedNativeMaintenanceRegistrationLifecycleEntry>,
    pub epoch_at: String,
    pub epoch_witness_sha256: String,
    pub horizon_end_at: String,
    pub schedule_contract_sha256: String,
    pub snapshot_at: String,
}

/// Exact planner schedule projected from the verified lifecycle rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceRegistrationScheduleStatus {
    pub contract_version: &'static str,
    pub epoch_at: String,
    pub epoch_witness_sha256: String,
    pub horizon_end_at: String,
    pub schedule_contract_sha256: String,
    pub slots: Vec<VerifiedNativeMaintenanceRegistrationSlot>,
    pub snapshot_at: String,
}

/// Result of the complete source lifecycle initialization sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCaptureLifecycleInitialization {
    pub schedule: VerifiedNativeMaintenanceRegistrationScheduleStatus,
    pub serving_snapshot_sha256: String,
    pub witness_sha256: String,
}

/// Verified immutable epoch, snapshot, and rolling database-ledger reads.
pub trait NativeMaintenanceCaptureEpochLedgerAuthority {
    type Error;

    /// Read the root-authenticated future-only epoch witness.
    fn read_verified_epoch_witness(
        &mut self,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<VerifiedNativeMaintenanceCaptureEpochWitness, Self::Error>>
                + '_,
        >,
    >;

    /// Read the exact retained zero-frontier serving snapshot.
    fn read_verified_epoch_snapshot(
        &mut self,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<VerifiedNativeMaintenanceCaptureEpochSnapshot, Self::Error>>
                + '_,
        >,
    >;

    /// Read the complete verified rolling database migration ledger.
    fn read_verified_database_ledger(
        &mut self,
    ) -> Pin<
        Box<dyn Future<Output = Result<VerifiedNativeMaintenanceDatabaseLedger, Self::Error>> + '_>,
    >;
}

/// Actual owner-only schedule initialization and verified lifecycle reader.
pub trait NativeMaintenanceRegistrationLifecyclePort {
    type Error;

    /// Submit the real native rolling bootstrap through the fixed owner boundary.
    fn initialize_registration_schedule<'a>(
        &'a mut self,
        bootstrap: &'a NativeAdjustmentRollingScheduleBootstrap,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>>;

    /// Read the exact post-initialization lifecycle and all three fixed family rows.
    fn read_verified_registration_lifecycle(
        &mut self,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        VerifiedNativeMaintenanceRegistrationLifecycleStatus,
                        Self::Error,
                    >,
                > + '_,
        >,
    >;
}

/// Real local journal and archive genesis mutations under the controller lock.
pub trait NativeMaintenanceCaptureGenesisPort {
    type Error;

    /// Sample the actual canonical local commit clock after lifecycle admission.
    fn canonical_now(&mut self) -> Result<String, Self::Error>;

    /// Initialize the future-only journal lifecycle for the verified witness.
    fn initialize_future_only_lifecycle<'a>(
        &'a mut self,
        now: &'a str,
        witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>>;

    /// Archive the genuine witness/snapshot genesis through the durable graph owner.
    fn archive_future_only_genesis<'a>(
        &'a mut self,
        snapshot: &'a VerifiedNativeMaintenanceCaptureEpochSnapshot,
        witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>>;
}

/// Failure from one exact lifecycle owner or cross-owner binding.
#[derive(Debug)]
pub enum NativeMaintenanceCaptureLifecycleCoreError<Authority, Lifecycle, Genesis> {
    Authority(Authority),
    Lifecycle(Lifecycle),
    Genesis(Genesis),
    RollingSchedule(NativeMaintenanceRollingScheduleError),
    InvalidBinding,
}

impl<Authority: Display, Lifecycle: Display, Genesis: Display> Display
    for NativeMaintenanceCaptureLifecycleCoreError<Authority, Lifecycle, Genesis>
{
    /// Retain the owning effect diagnostic or one bounded binding refusal.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // preserve the first failing owner in source sequence
        match self {
            Self::Authority(error) => Display::fmt(error, formatter),
            Self::Lifecycle(error) => Display::fmt(error, formatter),
            Self::Genesis(error) => Display::fmt(error, formatter),
            Self::RollingSchedule(error) => Display::fmt(error, formatter),
            Self::InvalidBinding => formatter.write_str("capture lifecycle binding differs"),
        }
    }
}

/// Execute the exact epoch, schedule, journal, and archive genesis sequence.
pub async fn initialize_native_maintenance_capture_lifecycle<Authority, Lifecycle, Genesis>(
    authority: &mut Authority,
    lifecycle: &mut Lifecycle,
    genesis: &mut Genesis,
) -> Result<
    NativeMaintenanceCaptureLifecycleInitialization,
    NativeMaintenanceCaptureLifecycleCoreError<Authority::Error, Lifecycle::Error, Genesis::Error>,
>
where
    Authority: NativeMaintenanceCaptureEpochLedgerAuthority,
    Lifecycle: NativeMaintenanceRegistrationLifecyclePort,
    Genesis: NativeMaintenanceCaptureGenesisPort,
{
    let witness = authority
        .read_verified_epoch_witness()
        .await
        .map_err(NativeMaintenanceCaptureLifecycleCoreError::Authority)?;
    let snapshot = authority
        .read_verified_epoch_snapshot()
        .await
        .map_err(NativeMaintenanceCaptureLifecycleCoreError::Authority)?;
    let database = authority
        .read_verified_database_ledger()
        .await
        .map_err(NativeMaintenanceCaptureLifecycleCoreError::Authority)?;
    // retain the forced witness/snapshot pair and exact rolling ledger before mutation
    if database.contract_version != "adjustment-database-ledger/v3"
        || database.migration_history_sha256 != witness.database_migration_history_sha256
        || snapshot.archive_commit_ordinal != "0"
        || snapshot.entry_count != 0
        || !snapshot.entries_empty
        || snapshot.cutoff_at != witness.epoch_at
        || snapshot.frontier_sha256 != witness.catalog_frontier_sha256
        || snapshot.snapshot_sha256 != witness.serving_snapshot_sha256
    {
        return Err(NativeMaintenanceCaptureLifecycleCoreError::InvalidBinding);
    }
    let bootstrap = build_adjustment_rolling_schedule_bootstrap(
        NativeAdjustmentRollingScheduleBootstrapInput {
            epoch_at: &witness.epoch_at,
            epoch_witness_sha256: &witness.witness_sha256,
        },
    )
    .map_err(NativeMaintenanceCaptureLifecycleCoreError::RollingSchedule)?;
    lifecycle
        .initialize_registration_schedule(&bootstrap)
        .await
        .map_err(NativeMaintenanceCaptureLifecycleCoreError::Lifecycle)?;
    let verified_lifecycle = lifecycle
        .read_verified_registration_lifecycle()
        .await
        .map_err(NativeMaintenanceCaptureLifecycleCoreError::Lifecycle)?;
    let schedule = schedule_from_verified_lifecycle(verified_lifecycle)
        .ok_or(NativeMaintenanceCaptureLifecycleCoreError::InvalidBinding)?;
    // bind the post-initialization database head to this exact retained epoch
    if schedule.epoch_at != witness.epoch_at
        || schedule.epoch_witness_sha256 != witness.witness_sha256
    {
        return Err(NativeMaintenanceCaptureLifecycleCoreError::InvalidBinding);
    }
    let now = genesis
        .canonical_now()
        .map_err(NativeMaintenanceCaptureLifecycleCoreError::Genesis)?;
    // reject a substituted local clock before either append-only genesis mutation
    if canonical_instant(&now).is_none() {
        return Err(NativeMaintenanceCaptureLifecycleCoreError::InvalidBinding);
    }
    genesis
        .initialize_future_only_lifecycle(&now, &witness)
        .await
        .map_err(NativeMaintenanceCaptureLifecycleCoreError::Genesis)?;
    genesis
        .archive_future_only_genesis(&snapshot, &witness)
        .await
        .map_err(NativeMaintenanceCaptureLifecycleCoreError::Genesis)?;
    Ok(NativeMaintenanceCaptureLifecycleInitialization {
        schedule,
        serving_snapshot_sha256: snapshot.snapshot_sha256,
        witness_sha256: witness.witness_sha256,
    })
}

/// Rebuild the source planner schedule from one verified lifecycle projection.
pub fn schedule_from_verified_lifecycle(
    lifecycle: VerifiedNativeMaintenanceRegistrationLifecycleStatus,
) -> Option<VerifiedNativeMaintenanceRegistrationScheduleStatus> {
    let epoch = canonical_instant(&lifecycle.epoch_at)?;
    let horizon = canonical_instant(&lifecycle.horizon_end_at)?;
    let snapshot = canonical_instant(&lifecycle.snapshot_at)?;
    // require the exact rolling contract, future horizon, and fixed family population
    if lifecycle.contract_version != LIFECYCLE_VERSION
        || !valid_hash(&lifecycle.epoch_witness_sha256)
        || lifecycle.schedule_contract_sha256 != ADJUSTMENT_ROLLING_SCHEDULE_SHA256
        || snapshot < epoch
        || horizon <= epoch
        || lifecycle.entries.len() != 3
    {
        return None;
    }
    let expected = [
        NativeMaintenanceCaptureFamily::Temperature,
        NativeMaintenanceCaptureFamily::Wind,
        NativeMaintenanceCaptureFamily::Rain,
    ];
    let mut slots = Vec::with_capacity(3);
    // bind every database slot to the shared schedule head in source family order
    for (entry, family) in lifecycle.entries.into_iter().zip(expected) {
        let slot = entry.slot;
        let occupied = slot.state != NativeMaintenanceRegistrationSlotState::Free;
        // preserve slot identity, occupancy, and optional terminal grammar exactly
        if slot.contract_version != SLOT_VERSION
            || slot.family != family
            || slot.epoch_witness_sha256 != lifecycle.epoch_witness_sha256
            || slot.schedule_contract_sha256 != lifecycle.schedule_contract_sha256
            || slot.horizon_end_at != lifecycle.horizon_end_at
            || occupied != slot.registration_sha256.is_some()
            || occupied != slot.terminal_at.is_some()
            || slot
                .registration_sha256
                .as_deref()
                .is_some_and(|value| !valid_hash(value))
            || slot
                .terminal_at
                .as_deref()
                .is_some_and(|value| canonical_instant(value).is_none())
        {
            return None;
        }
        slots.push(slot);
    }
    Some(VerifiedNativeMaintenanceRegistrationScheduleStatus {
        contract_version: SCHEDULE_VERSION,
        epoch_at: lifecycle.epoch_at,
        epoch_witness_sha256: lifecycle.epoch_witness_sha256,
        horizon_end_at: lifecycle.horizon_end_at,
        schedule_contract_sha256: lifecycle.schedule_contract_sha256,
        slots,
        snapshot_at: lifecycle.snapshot_at,
    })
}

/// One exact bounded gap-spool outcome before cold-catalog reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceRevisionGapDrain {
    Idle {
        page_sha256: String,
    },
    Captured {
        graph_manifest_sha256: String,
        page_sha256: String,
    },
}

/// Complete synchronization result preserving the mutually exclusive cycle budget.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceEvidenceSynchronization<Cold> {
    GapCaptured {
        gap: NativeMaintenanceRevisionGapDrain,
    },
    Cold {
        cycle: Cold,
    },
}

/// Borrowed bounded evidence cycle result.
pub type NativeMaintenanceColdCycleFuture<'a, Cycle, Error> =
    Pin<Box<dyn Future<Output = Result<Cycle, Error>> + 'a>>;

/// Real metadata reconciliation, gap drain, and cold-page custody effects.
pub trait NativeMaintenanceEvidenceReconciliationPort {
    type ColdCycle;
    type Error;

    /// Reconcile the prior durable metadata proof without manufacturing one.
    fn reconcile_shadow_metadata(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>>;

    /// Drain at most one actual gap-spool page before any cold read.
    fn drain_revision_gap(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<NativeMaintenanceRevisionGapDrain, Self::Error>> + '_>>;

    /// Drain one actual cold page under the original two-body cycle ceiling.
    fn drain_revision_cold_page<'a>(
        &'a mut self,
        cutoff_at: &'a str,
    ) -> NativeMaintenanceColdCycleFuture<'a, Self::ColdCycle, Self::Error>;
}

/// Evidence synchronization refusal from its actual port or typed gap receipt.
#[derive(Debug)]
pub enum NativeMaintenanceEvidenceSynchronizationError<Error> {
    Port(Error),
    InvalidGap,
}

/// Preserve pre-gap reconciliation, exclusive gap budget, and cold-only post reconcile.
pub async fn synchronize_native_maintenance_evidence<Port>(
    port: &mut Port,
    cutoff_at: &str,
) -> Result<
    NativeMaintenanceEvidenceSynchronization<Port::ColdCycle>,
    NativeMaintenanceEvidenceSynchronizationError<Port::Error>,
>
where
    Port: NativeMaintenanceEvidenceReconciliationPort,
{
    port.reconcile_shadow_metadata()
        .await
        .map_err(NativeMaintenanceEvidenceSynchronizationError::Port)?;
    let gap = port
        .drain_revision_gap()
        .await
        .map_err(NativeMaintenanceEvidenceSynchronizationError::Port)?;
    // charge any genuine nonidle gap as the complete cycle without a cold read or post reconcile
    match gap {
        NativeMaintenanceRevisionGapDrain::Captured {
            ref graph_manifest_sha256,
            ref page_sha256,
        } => {
            if !valid_hash(graph_manifest_sha256) || !valid_hash(page_sha256) {
                return Err(NativeMaintenanceEvidenceSynchronizationError::InvalidGap);
            }
            return Ok(NativeMaintenanceEvidenceSynchronization::GapCaptured { gap });
        }
        NativeMaintenanceRevisionGapDrain::Idle { ref page_sha256 } => {
            if !valid_hash(page_sha256) {
                return Err(NativeMaintenanceEvidenceSynchronizationError::InvalidGap);
            }
        }
    }
    let cycle = port
        .drain_revision_cold_page(cutoff_at)
        .await
        .map_err(NativeMaintenanceEvidenceSynchronizationError::Port)?;
    // compact only after the cold branch completed its read and custody effects
    port.reconcile_shadow_metadata()
        .await
        .map_err(NativeMaintenanceEvidenceSynchronizationError::Port)?;
    Ok(NativeMaintenanceEvidenceSynchronization::Cold { cycle })
}

/// Resolve one exact canonical UTC millisecond string for cross-owner comparisons.
fn canonical_instant(value: &str) -> Option<i64> {
    let resolved = NativeWirePrimitives::new()
        .resolve(&JsDateOrString::String(value.encode_utf16().collect()))
        .ok()?;
    (resolved.utc == value).then_some(resolved.epoch_milliseconds)
}

#[cfg(test)]
#[path = "native_maintenance_capture_cycle_core_test.rs"]
mod tests;
