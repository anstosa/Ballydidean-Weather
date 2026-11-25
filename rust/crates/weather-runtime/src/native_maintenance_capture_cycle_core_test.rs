use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use super::{
    NativeMaintenanceCaptureEpochLedgerAuthority, NativeMaintenanceCaptureGenesisPort,
    NativeMaintenanceEvidenceReconciliationPort, NativeMaintenanceEvidenceSynchronization,
    NativeMaintenanceRegistrationLifecyclePort, NativeMaintenanceRegistrationSlotState,
    NativeMaintenanceRevisionGapDrain, VerifiedNativeMaintenanceRegistrationLifecycleEntry,
    VerifiedNativeMaintenanceRegistrationLifecycleStatus,
    VerifiedNativeMaintenanceRegistrationSlot, initialize_native_maintenance_capture_lifecycle,
    synchronize_native_maintenance_evidence,
};
use crate::native_maintenance_capture_readiness::{
    NativeMaintenanceCaptureFamily, VerifiedNativeMaintenanceCaptureEpochSnapshot,
    VerifiedNativeMaintenanceCaptureEpochWitness, VerifiedNativeMaintenanceDatabaseLedger,
};
use crate::{
    ADJUSTMENT_ROLLING_SCHEDULE_SHA256, NativeAdjustmentRollingScheduleBootstrap,
    NativeBackendRuntime,
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const HASH_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const HISTORY: &str = "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";
const EPOCH: &str = "2026-10-10T08:00:00.000Z";
const SNAPSHOT_AT: &str = "2026-10-10T08:00:01.000Z";

type Events = Rc<RefCell<Vec<&'static str>>>;

struct AuthorityFixture {
    events: Events,
}

impl NativeMaintenanceCaptureEpochLedgerAuthority for AuthorityFixture {
    type Error = &'static str;

    /// Return the fixed verified witness first.
    fn read_verified_epoch_witness(
        &mut self,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<VerifiedNativeMaintenanceCaptureEpochWitness, Self::Error>>
                + '_,
        >,
    > {
        Box::pin(async move {
            self.events.borrow_mut().push("witness");
            Ok(witness())
        })
    }

    /// Return the paired zero snapshot second.
    fn read_verified_epoch_snapshot(
        &mut self,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<VerifiedNativeMaintenanceCaptureEpochSnapshot, Self::Error>>
                + '_,
        >,
    > {
        Box::pin(async move {
            self.events.borrow_mut().push("snapshot");
            Ok(snapshot())
        })
    }

    /// Return the matching rolling ledger third.
    fn read_verified_database_ledger(
        &mut self,
    ) -> Pin<
        Box<dyn Future<Output = Result<VerifiedNativeMaintenanceDatabaseLedger, Self::Error>> + '_>,
    > {
        Box::pin(async move {
            self.events.borrow_mut().push("ledger");
            Ok(VerifiedNativeMaintenanceDatabaseLedger {
                contract_version: "adjustment-database-ledger/v3".to_owned(),
                migration_history_sha256: HISTORY.to_owned(),
                snapshot_at: SNAPSHOT_AT.to_owned(),
            })
        })
    }
}

struct LifecycleFixture {
    bootstrap: Option<NativeAdjustmentRollingScheduleBootstrap>,
    events: Events,
}

impl NativeMaintenanceRegistrationLifecyclePort for LifecycleFixture {
    type Error = &'static str;

    /// Retain the actual native bootstrap submitted by the core.
    fn initialize_registration_schedule<'a>(
        &'a mut self,
        bootstrap: &'a NativeAdjustmentRollingScheduleBootstrap,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>> {
        Box::pin(async move {
            self.events.borrow_mut().push("bootstrap");
            self.bootstrap = Some(bootstrap.clone());
            Ok(())
        })
    }

    /// Project the fixed three-family lifecycle from the retained bootstrap.
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
            self.events.borrow_mut().push("lifecycle");
            let bootstrap = self.bootstrap.as_ref().ok_or("bootstrap absent")?;
            Ok(lifecycle(bootstrap))
        })
    }
}

struct GenesisFixture {
    events: Events,
}

impl NativeMaintenanceCaptureGenesisPort for GenesisFixture {
    type Error = &'static str;

    /// Return one canonical post-lifecycle local clock.
    fn canonical_now(&mut self) -> Result<String, Self::Error> {
        self.events.borrow_mut().push("clock");
        Ok(SNAPSHOT_AT.to_owned())
    }

    /// Record the real journal mutation position in the fixture sequence.
    fn initialize_future_only_lifecycle<'a>(
        &'a mut self,
        now: &'a str,
        witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>> {
        Box::pin(async move {
            assert_eq!(now, SNAPSHOT_AT);
            assert_eq!(witness.witness_sha256, HASH_A);
            self.events.borrow_mut().push("journal-genesis");
            Ok(())
        })
    }

    /// Record the archive genesis only after journal initialization.
    fn archive_future_only_genesis<'a>(
        &'a mut self,
        snapshot: &'a VerifiedNativeMaintenanceCaptureEpochSnapshot,
        witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>> {
        Box::pin(async move {
            assert_eq!(snapshot.snapshot_sha256, witness.serving_snapshot_sha256);
            self.events.borrow_mut().push("archive-genesis");
            Ok(())
        })
    }
}

/// Preserve the exact source initialization order around the real rolling builder.
#[test]
fn native_capture_lifecycle_initializes_verified_epoch_and_genesis_in_order() {
    let runtime = NativeBackendRuntime::new().unwrap();
    runtime.block_on(async {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut authority = AuthorityFixture {
            events: Rc::clone(&events),
        };
        let mut lifecycle = LifecycleFixture {
            bootstrap: None,
            events: Rc::clone(&events),
        };
        let mut genesis = GenesisFixture {
            events: Rc::clone(&events),
        };
        let initialized = initialize_native_maintenance_capture_lifecycle(
            &mut authority,
            &mut lifecycle,
            &mut genesis,
        )
        .await
        .unwrap();
        assert_eq!(initialized.witness_sha256, HASH_A);
        assert_eq!(initialized.serving_snapshot_sha256, HASH_B);
        assert_eq!(initialized.schedule.slots.len(), 3);
        assert_eq!(
            events.borrow().as_slice(),
            [
                "witness",
                "snapshot",
                "ledger",
                "bootstrap",
                "lifecycle",
                "clock",
                "journal-genesis",
                "archive-genesis",
            ],
        );
    });
}

struct EvidenceFixture {
    events: Events,
    gaps: VecDeque<NativeMaintenanceRevisionGapDrain>,
}

impl NativeMaintenanceEvidenceReconciliationPort for EvidenceFixture {
    type ColdCycle = &'static str;
    type Error = &'static str;

    /// Record each pre- or post-cold reconciliation distinctly by position.
    fn reconcile_shadow_metadata(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>> {
        Box::pin(async move {
            self.events.borrow_mut().push("reconcile");
            Ok(())
        })
    }

    /// Return the selected typed gap outcome.
    fn drain_revision_gap(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<NativeMaintenanceRevisionGapDrain, Self::Error>> + '_>>
    {
        Box::pin(async move {
            self.events.borrow_mut().push("gap");
            self.gaps.pop_front().ok_or("gap absent")
        })
    }

    /// Record the cold branch and its exact cutoff.
    fn drain_revision_cold_page<'a>(
        &'a mut self,
        cutoff_at: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Self::ColdCycle, Self::Error>> + 'a>> {
        Box::pin(async move {
            assert_eq!(cutoff_at, SNAPSHOT_AT);
            self.events.borrow_mut().push("cold");
            Ok("cold-page")
        })
    }
}

/// A nonidle gap consumes the complete cycle and skips cold/post-read effects.
#[test]
fn native_capture_gap_is_the_complete_cycle_budget() {
    let runtime = NativeBackendRuntime::new().unwrap();
    runtime.block_on(async {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut port = EvidenceFixture {
            events: Rc::clone(&events),
            gaps: VecDeque::from([NativeMaintenanceRevisionGapDrain::Captured {
                graph_manifest_sha256: HASH_C.to_owned(),
                page_sha256: HASH_D.to_owned(),
            }]),
        };
        let result = synchronize_native_maintenance_evidence(&mut port, SNAPSHOT_AT)
            .await
            .unwrap();
        assert!(matches!(
            result,
            NativeMaintenanceEvidenceSynchronization::GapCaptured { .. }
        ));
        assert_eq!(events.borrow().as_slice(), ["reconcile", "gap"]);
    });
}

/// An idle gap runs one cold page and reconciles only after that branch completes.
#[test]
fn native_capture_cold_branch_reconciles_after_read() {
    let runtime = NativeBackendRuntime::new().unwrap();
    runtime.block_on(async {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut port = EvidenceFixture {
            events: Rc::clone(&events),
            gaps: VecDeque::from([NativeMaintenanceRevisionGapDrain::Idle {
                page_sha256: HASH_D.to_owned(),
            }]),
        };
        let result = synchronize_native_maintenance_evidence(&mut port, SNAPSHOT_AT)
            .await
            .unwrap();
        assert_eq!(
            result,
            NativeMaintenanceEvidenceSynchronization::Cold { cycle: "cold-page" },
        );
        assert_eq!(
            events.borrow().as_slice(),
            ["reconcile", "gap", "cold", "reconcile"],
        );
    });
}

/// Construct one complete retained final-v14 witness projection.
fn witness() -> VerifiedNativeMaintenanceCaptureEpochWitness {
    VerifiedNativeMaintenanceCaptureEpochWitness {
        catalog_frontier_sha256: HASH_C.to_owned(),
        control_plane_sha256: HASH_D.to_owned(),
        control_plane_version: "14".to_owned(),
        database_migration_history_sha256: HISTORY.to_owned(),
        epoch_at: EPOCH.to_owned(),
        serving_snapshot_sha256: HASH_B.to_owned(),
        source_commit: "1".repeat(40),
        source_release: "2026.10.10-1".to_owned(),
        source_server_image_digest: format!("sha256:{HASH_D}"),
        witness_sha256: HASH_A.to_owned(),
    }
}

/// Construct the exact paired empty serving snapshot projection.
fn snapshot() -> VerifiedNativeMaintenanceCaptureEpochSnapshot {
    VerifiedNativeMaintenanceCaptureEpochSnapshot {
        archive_commit_ordinal: "0".to_owned(),
        cutoff_at: EPOCH.to_owned(),
        entry_count: 0,
        entries_empty: true,
        frontier_sha256: HASH_C.to_owned(),
        snapshot_sha256: HASH_B.to_owned(),
    }
}

/// Project the fixture lifecycle from the actual native bootstrap result.
fn lifecycle(
    bootstrap: &NativeAdjustmentRollingScheduleBootstrap,
) -> VerifiedNativeMaintenanceRegistrationLifecycleStatus {
    let entries = [
        NativeMaintenanceCaptureFamily::Temperature,
        NativeMaintenanceCaptureFamily::Wind,
        NativeMaintenanceCaptureFamily::Rain,
    ]
    .into_iter()
    .map(
        |family| VerifiedNativeMaintenanceRegistrationLifecycleEntry {
            slot: VerifiedNativeMaintenanceRegistrationSlot {
                contract_version: "adjustment-shadow-registration-slot/v3".to_owned(),
                epoch_witness_sha256: bootstrap.epoch_witness_sha256.clone(),
                family,
                horizon_end_at: bootstrap.horizon_end_at.clone(),
                registration_sha256: None,
                schedule_contract_sha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256.to_owned(),
                state: NativeMaintenanceRegistrationSlotState::Free,
                terminal_at: None,
            },
        },
    )
    .collect();
    VerifiedNativeMaintenanceRegistrationLifecycleStatus {
        contract_version: "adjustment-registration-lifecycle-status/v4".to_owned(),
        entries,
        epoch_at: bootstrap.epoch_at.clone(),
        epoch_witness_sha256: bootstrap.epoch_witness_sha256.clone(),
        horizon_end_at: bootstrap.horizon_end_at.clone(),
        schedule_contract_sha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256.to_owned(),
        snapshot_at: SNAPSHOT_AT.to_owned(),
    }
}
