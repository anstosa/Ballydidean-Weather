use super::*;
use crate::{
    ADJUSTMENT_ROLLING_SCHEDULE_SHA256, NativeBackendRuntime, NativeMaintenanceCaptureFamily,
    NativeMaintenanceDueState, NativeMaintenanceDueStatus,
    NativeMaintenanceEvidenceSynchronization, NativeMaintenanceRevisionGapDrain,
    NativeRegistrationSlot, NativeRegistrationSlotState,
};
use std::{cell::Cell, rc::Rc};

const NOW: &str = "2026-12-10T12:00:00.000Z";
const EPOCH: &str = "2026-10-07T08:00:00.000Z";
const HORIZON: &str = "2027-10-08T07:00:00.000Z";
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Guard(Rc<Cell<bool>>);
impl Drop for Guard {
    /// Release the isolated fixture lock after the final due.
    fn drop(&mut self) {
        self.0.set(false);
    }
}

#[derive(Default)]
struct Ports {
    held: Rc<Cell<bool>>,
    events: Vec<String>,
    due_count: usize,
}
impl Ports {
    /// Require every source effect to occur under the active controller lock.
    fn push(&mut self, event: &str) {
        assert!(self.held.get());
        self.events.push(event.to_owned());
    }
}
impl NativeMaintenanceDueReadPort for Ports {
    type Error = ();
    /// Return a genuine fixture absent state for each original scheduled key.
    fn inspect_due_keys<'a>(
        &'a mut self,
        due_keys: &'a [String],
    ) -> ControllerFuture<'a, Vec<NativeMaintenanceDueState>, Self::Error> {
        Box::pin(async move {
            self.push("plan");
            Ok(due_keys
                .iter()
                .map(|key| NativeMaintenanceDueState {
                    due_key: key.clone(),
                    input_head_sha256: None,
                    output_sha256: None,
                    status: NativeMaintenanceDueStatus::Absent,
                })
                .collect())
        })
    }
}
impl NativeMaintenanceControllerRunPorts for Ports {
    type ProcessLock = Guard;
    /// Acquire the fixture lock and retain one owner.
    fn acquire_process_lock(&mut self) -> Result<Guard, ()> {
        assert!(!self.held.replace(true));
        self.push("lock");
        Ok(Guard(Rc::clone(&self.held)))
    }
    /// Initialize journal first.
    fn initialize_journal(&mut self) -> ControllerFuture<'_, (), ()> {
        Box::pin(async move {
            self.push("journal");
            Ok(())
        })
    }
    /// Initialize archive second.
    fn initialize_archive(&mut self) -> ControllerFuture<'_, (), ()> {
        Box::pin(async move {
            self.push("archive");
            Ok(())
        })
    }
    /// Authenticate the lifecycle before cutoff reads.
    fn initialize_lifecycle(&mut self) -> ControllerFuture<'_, (), ()> {
        Box::pin(async move {
            self.push("lifecycle");
            Ok(())
        })
    }
    /// Read the exact fixed source schedule geometry.
    fn read_registration_schedule(
        &mut self,
    ) -> ControllerFuture<'_, NativeRegistrationScheduleStatus, ()> {
        Box::pin(async move {
            self.push("schedule");
            let slot = |family| NativeRegistrationSlot {
                contract_version: "adjustment-shadow-registration-slot/v3".to_owned(),
                epoch_witness_sha256: HASH.to_owned(),
                family,
                horizon_end_at: HORIZON.to_owned(),
                registration_sha256: None,
                schedule_contract_sha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256.to_owned(),
                state: NativeRegistrationSlotState::Free,
                terminal_at: None,
            };
            Ok(NativeRegistrationScheduleStatus {
                contract_version: "adjustment-registration-schedule-status/v3",
                epoch_at: EPOCH.to_owned(),
                epoch_witness_sha256: HASH.to_owned(),
                horizon_end_at: HORIZON.to_owned(),
                schedule_contract_sha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256.to_owned(),
                slots: [
                    slot(NativeMaintenanceCaptureFamily::Temperature),
                    slot(NativeMaintenanceCaptureFamily::Wind),
                    slot(NativeMaintenanceCaptureFamily::Rain),
                ],
                snapshot_at: NOW.to_owned(),
            })
        })
    }
    /// Confirm the authenticated snapshot is the shared revision cutoff.
    fn synchronize_evidence<'a>(
        &'a mut self,
        cutoff_at: &'a str,
    ) -> ControllerFuture<'a, NativeMaintenanceDueEvidence, ()> {
        Box::pin(async move {
            assert_eq!(cutoff_at, NOW);
            self.push("synchronize");
            Ok(NativeMaintenanceEvidenceSynchronization::GapCaptured {
                gap: NativeMaintenanceRevisionGapDrain::Captured {
                    graph_manifest_sha256: HASH.to_owned(),
                    page_sha256: HASH.to_owned(),
                },
            })
        })
    }
    /// Flush before selecting any due.
    fn flush_evidence(&mut self) -> ControllerFuture<'_, (), ()> {
        Box::pin(async move {
            self.push("flush");
            Ok(())
        })
    }
    /// Sample a fresh canonical due clock.
    fn canonical_now(&mut self) -> Result<String, ()> {
        self.push("clock");
        Ok(NOW.to_owned())
    }
    /// Preserve the caller's original evidence and let one independent attempt fail.
    fn execute_due<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        now: &'a str,
        evidence: &'a NativeMaintenanceDueEvidence,
    ) -> ControllerFuture<'a, NativeMaintenanceDueExecutionResult, ()> {
        Box::pin(async move {
            assert_eq!(now, NOW);
            assert!(matches!(
                evidence,
                NativeMaintenanceEvidenceSynchronization::GapCaptured { .. }
            ));
            self.push(&due.due_key);
            self.due_count += 1;
            if self.due_count == 1 {
                return Err(());
            }
            Ok(NativeMaintenanceDueExecutionResult {
                due_key: due.due_key.clone(),
                state: "blocked".to_owned(),
                reason: Some("semantic_input_blocked".to_owned()),
                manifest_object_sha256: None,
                report_sha256: None,
                output_sha256: None,
                action_sha256: None,
            })
        })
    }
}

/// Preserve exact original mode grammar, initialization/evidence order and independent due errors.
#[test]
fn native_maintenance_controller_run_preserves_source_sequence() {
    let mut ports = Ports::default();
    let mode = parse_native_maintenance_controller_mode(&["--monthly".to_owned()]).unwrap();
    let runtime = NativeBackendRuntime::new().unwrap();
    let result = runtime
        .block_on(run_native_maintenance_controller(&mut ports, mode))
        .unwrap();
    assert!(!ports.held.get());
    assert_eq!(result.started_at, NOW);
    assert_eq!(result.attempts.len(), 3);
    assert_eq!(
        result.attempts[0].reason.as_deref(),
        Some("controller_error")
    );
    assert_eq!(
        result.attempts[1].reason.as_deref(),
        Some("semantic_input_blocked")
    );
    assert_eq!(
        ports.events,
        [
            "lock",
            "journal",
            "archive",
            "lifecycle",
            "schedule",
            "synchronize",
            "flush",
            "plan",
            "clock",
            "monthly/temperature/2026-12",
            "clock",
            "monthly/wind/2026-12",
            "clock",
            "monthly/rain/2026-12"
        ]
    );
}

/// The installed CLI preserves the predecessor's optional attempt fields and canonical run envelope.
#[test]
fn controller_run_serialization_preserves_source_optional_fields() {
    let run = NativeMaintenanceControllerRun {
        contract_version: NATIVE_MAINTENANCE_CONTROLLER_RUN_VERSION,
        mode: NativeMaintenanceDueMode::Monthly,
        started_at: "2026-11-01T12:00:00.000Z".to_owned(),
        required_adapters: NATIVE_MAINTENANCE_REQUIRED_ADAPTERS,
        attempts: vec![
            NativeMaintenanceDueExecutionResult {
                due_key: "monthly/rain/2026-11".to_owned(),
                state: "already_complete".to_owned(),
                reason: None,
                manifest_object_sha256: None,
                report_sha256: None,
                output_sha256: Some("a".repeat(64)),
                action_sha256: None,
            },
            NativeMaintenanceDueExecutionResult {
                due_key: "monthly/wind/2026-11".to_owned(),
                state: "failed".to_owned(),
                reason: Some("controller_error".to_owned()),
                manifest_object_sha256: None,
                report_sha256: None,
                output_sha256: None,
                action_sha256: None,
            },
        ],
    };
    let encoded = encode_native_maintenance_controller_run(&run).unwrap();
    let expected = format!(
        "{{\"attempts\":[{{\"dueKey\":\"monthly/rain/2026-11\",\"outputSha256\":\"{}\",\"state\":\"already_complete\"}},{{\"dueKey\":\"monthly/wind/2026-11\",\"reason\":\"controller_error\",\"state\":\"failed\"}}],\"contractVersion\":\"forecast-adjustment-maintenance-controller-run/v1\",\"mode\":\"monthly\",\"requiredAdapters\":[{}],\"startedAt\":\"2026-11-01T12:00:00.000Z\"}}\n",
        "a".repeat(64),
        NATIVE_MAINTENANCE_REQUIRED_ADAPTERS
            .iter()
            .map(|adapter| format!("\"{adapter}\""))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert_eq!(encoded, expected.as_bytes());
}
