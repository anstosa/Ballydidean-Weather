//! Capture-cycle adapter over the verified registration lifecycle remote owner.

use std::future::Future;
use std::pin::Pin;

use weather_api::CurrentTimeResolver;

use crate::MaintenanceCanonicalJson;
use crate::native_maintenance_capture_cycle_core::{
    NativeMaintenanceRegistrationLifecyclePort, NativeMaintenanceRegistrationSlotState,
    VerifiedNativeMaintenanceRegistrationLifecycleEntry,
    VerifiedNativeMaintenanceRegistrationLifecycleStatus,
    VerifiedNativeMaintenanceRegistrationSlot,
};
use crate::native_maintenance_registration_lifecycle_remote::{
    NativeMaintenanceRegistrationLifecycleRemote, NativeRegistrationLifecycleRemoteError,
    NativeRegistrationLifecycleRemoteOperationPort, NativeRegistrationLifecycleStatus,
    NativeRegistrationSlot, NativeRegistrationSlotState,
};
use crate::native_maintenance_rolling_schedule::NativeAdjustmentRollingScheduleBootstrap;

/// Capture-cycle lifecycle owner backed by one retained verified remote session.
pub struct NativeMaintenanceRegistrationLifecycleHost<Port, Codec, Time> {
    remote: NativeMaintenanceRegistrationLifecycleRemote<Port, Codec, Time>,
}

impl<Port, Codec, Time> NativeMaintenanceRegistrationLifecycleHost<Port, Codec, Time> {
    /// Bind the validated remote owner without opening or executing a host transport.
    pub const fn new(
        remote: NativeMaintenanceRegistrationLifecycleRemote<Port, Codec, Time>,
    ) -> Self {
        Self { remote }
    }

    /// Release the unchanged remote owner after capture lifecycle composition.
    pub fn into_remote(self) -> NativeMaintenanceRegistrationLifecycleRemote<Port, Codec, Time> {
        self.remote
    }
}

impl<Port, Codec, Time> NativeMaintenanceRegistrationLifecyclePort
    for NativeMaintenanceRegistrationLifecycleHost<Port, Codec, Time>
where
    Port: NativeRegistrationLifecycleRemoteOperationPort,
    Codec: MaintenanceCanonicalJson,
    Time: CurrentTimeResolver,
{
    type Error = NativeRegistrationLifecycleRemoteError<Port::Error>;

    /// Submit and validate the exact idempotent native schedule initialization.
    fn initialize_registration_schedule<'a>(
        &'a mut self,
        bootstrap: &'a NativeAdjustmentRollingScheduleBootstrap,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>> {
        Box::pin(async move {
            // validate the canonical acknowledgement before discarding its idempotence flag
            self.remote.initialize_schedule(bootstrap).await?;
            Ok(())
        })
    }

    /// Read the fully validated lifecycle and narrow only its core schedule projection.
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
    > {
        Box::pin(async move {
            let lifecycle = self.remote.read_lifecycle().await?;
            Ok(project_verified_lifecycle_for_core(lifecycle))
        })
    }
}

/// Narrow a fully verified rich lifecycle to the capture core's schedule-owned fields.
fn project_verified_lifecycle_for_core(
    lifecycle: NativeRegistrationLifecycleStatus,
) -> VerifiedNativeMaintenanceRegistrationLifecycleStatus {
    // preserve fixed source order while dropping only fields the frozen core cannot represent
    let entries = lifecycle
        .entries
        .map(
            |entry| VerifiedNativeMaintenanceRegistrationLifecycleEntry {
                slot: project_verified_slot_for_core(entry.slot),
            },
        )
        .into_iter()
        .collect();
    VerifiedNativeMaintenanceRegistrationLifecycleStatus {
        contract_version: lifecycle.contract_version,
        entries,
        epoch_at: lifecycle.epoch_at,
        epoch_witness_sha256: lifecycle.epoch_witness_sha256,
        horizon_end_at: lifecycle.horizon_end_at,
        schedule_contract_sha256: lifecycle.schedule_contract_sha256,
        snapshot_at: lifecycle.snapshot_at,
    }
}

/// Preserve one verified slot while translating the duplicate frozen core state enum.
fn project_verified_slot_for_core(
    slot: NativeRegistrationSlot,
) -> VerifiedNativeMaintenanceRegistrationSlot {
    // translate the duplicated closed state set without accepting a fallback value
    let state = match slot.state {
        NativeRegistrationSlotState::BusyV2Legacy => {
            NativeMaintenanceRegistrationSlotState::BusyV2Legacy
        }
        NativeRegistrationSlotState::BusyV3 => NativeMaintenanceRegistrationSlotState::BusyV3,
        NativeRegistrationSlotState::Free => NativeMaintenanceRegistrationSlotState::Free,
    };
    VerifiedNativeMaintenanceRegistrationSlot {
        contract_version: slot.contract_version,
        epoch_witness_sha256: slot.epoch_witness_sha256,
        family: slot.family,
        horizon_end_at: slot.horizon_end_at,
        registration_sha256: slot.registration_sha256,
        schedule_contract_sha256: slot.schedule_contract_sha256,
        state,
        terminal_at: slot.terminal_at,
    }
}

#[cfg(test)]
mod tests {
    use crate::native_maintenance_capture_readiness::NativeMaintenanceCaptureFamily;
    use crate::native_maintenance_registration_lifecycle_remote::NativeRegistrationLifecycleEntry;

    use super::*;

    /// Preserve every core-visible field and translate all three closed slot states.
    #[test]
    fn projection_preserves_fixed_order_and_slot_geometry() {
        let lifecycle = NativeRegistrationLifecycleStatus {
            contract_version: "adjustment-registration-lifecycle-status/v4".to_owned(),
            entries: [
                entry(
                    NativeMaintenanceCaptureFamily::Temperature,
                    NativeRegistrationSlotState::BusyV3,
                    Some("temperature-registration"),
                    Some("2027-01-01T08:00:00.000Z"),
                ),
                entry(
                    NativeMaintenanceCaptureFamily::Wind,
                    NativeRegistrationSlotState::BusyV2Legacy,
                    Some("wind-registration"),
                    Some("2027-02-01T08:00:00.000Z"),
                ),
                entry(
                    NativeMaintenanceCaptureFamily::Rain,
                    NativeRegistrationSlotState::Free,
                    None,
                    None,
                ),
            ],
            epoch_at: "2026-01-01T08:00:00.000Z".to_owned(),
            epoch_witness_sha256: "epoch-witness".to_owned(),
            horizon_end_at: "2029-01-01T08:00:00.000Z".to_owned(),
            schedule_contract_sha256: "schedule-contract".to_owned(),
            snapshot_at: "2026-02-01T08:00:00.000Z".to_owned(),
        };

        let projected = project_verified_lifecycle_for_core(lifecycle);

        assert_eq!(
            projected.contract_version,
            "adjustment-registration-lifecycle-status/v4"
        );
        assert_eq!(projected.entries.len(), 3);
        assert_eq!(
            projected.entries[0].slot.state,
            NativeMaintenanceRegistrationSlotState::BusyV3
        );
        assert_eq!(
            projected.entries[1].slot.state,
            NativeMaintenanceRegistrationSlotState::BusyV2Legacy
        );
        assert_eq!(
            projected.entries[2].slot.state,
            NativeMaintenanceRegistrationSlotState::Free
        );
        assert_eq!(
            projected.entries[0].slot.registration_sha256.as_deref(),
            Some("temperature-registration")
        );
        assert_eq!(
            projected.entries[1].slot.family,
            NativeMaintenanceCaptureFamily::Wind
        );
        assert_eq!(projected.epoch_witness_sha256, "epoch-witness");
        assert_eq!(projected.schedule_contract_sha256, "schedule-contract");
    }

    /// Construct one rich lifecycle row whose core-owned portion is the supplied slot.
    fn entry(
        family: NativeMaintenanceCaptureFamily,
        state: NativeRegistrationSlotState,
        registration_sha256: Option<&str>,
        terminal_at: Option<&str>,
    ) -> NativeRegistrationLifecycleEntry {
        NativeRegistrationLifecycleEntry {
            active_registration: None,
            latest_registration_sha256: registration_sha256.map(str::to_owned),
            metadata: None,
            predecessor: None,
            slot: NativeRegistrationSlot {
                contract_version: "adjustment-shadow-registration-slot/v3".to_owned(),
                epoch_witness_sha256: "epoch-witness".to_owned(),
                family,
                horizon_end_at: "2029-01-01T08:00:00.000Z".to_owned(),
                registration_sha256: registration_sha256.map(str::to_owned),
                schedule_contract_sha256: "schedule-contract".to_owned(),
                state,
                terminal_at: terminal_at.map(str::to_owned),
            },
        }
    }
}
