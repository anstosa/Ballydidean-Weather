//! Original daily/monthly controller orchestration with its retained evidence cutoff.

use crate::{
    NativeMaintenanceDue, NativeMaintenanceDueEvidence, NativeMaintenanceDueExecutionResult,
    NativeMaintenanceDueMode, NativeMaintenanceDueReadPort, NativeRegistrationScheduleStatus,
    NativeWirePrimitives, plan_native_adjustment_maintenance_dues,
};
use std::{future::Future, pin::Pin};
use weather_edge::RevisionArchiveCanonicalCodec;

pub const NATIVE_MAINTENANCE_CONTROLLER_RUN_VERSION: &str =
    "forecast-adjustment-maintenance-controller-run/v1";
pub const NATIVE_MAINTENANCE_REQUIRED_ADAPTERS: [&str; 8] = [
    "daily_evaluation/v1",
    "future_only_input_seal/v2",
    "historical_fit_assembler/v2",
    "incumbent_comparator/v1",
    "registration_schedule/v3",
    "revision_custody_pack/v1",
    "semantic_cold_catalog/v1",
    "scorecard_publication/v2",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceControllerModeError {
    Arguments,
}

/// Parse only the original controller's single daily or monthly mode.
pub fn parse_native_maintenance_controller_mode(
    arguments: &[String],
) -> Result<NativeMaintenanceDueMode, NativeMaintenanceControllerModeError> {
    match arguments {
        [mode] if mode == "--daily" => Ok(NativeMaintenanceDueMode::Daily),
        [mode] if mode == "--monthly" => Ok(NativeMaintenanceDueMode::Monthly),
        _ => Err(NativeMaintenanceControllerModeError::Arguments),
    }
}

type ControllerFuture<'a, T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + 'a>>;

/// Production controller effects separated from the original sequential run state machine.
pub trait NativeMaintenanceControllerRunPorts: NativeMaintenanceDueReadPort {
    type ProcessLock;

    /// Acquire the original exclusive process lock and retain it across the full controller run.
    fn acquire_process_lock(&mut self) -> Result<Self::ProcessLock, Self::Error>;

    /// Open and reconcile the original fixed journal custody.
    fn initialize_journal(&mut self) -> ControllerFuture<'_, (), Self::Error>;

    /// Initialize the actual closed plaintext archive owner.
    fn initialize_archive(&mut self) -> ControllerFuture<'_, (), Self::Error>;

    /// Authenticate and initialize the rolling future-only lifecycle.
    fn initialize_lifecycle(&mut self) -> ControllerFuture<'_, (), Self::Error>;

    /// Read the exact verified post-initialization registration schedule.
    fn read_registration_schedule(
        &mut self,
    ) -> ControllerFuture<'_, NativeRegistrationScheduleStatus, Self::Error>;

    /// Synchronize at most one genuine revision transfer at the authenticated database cutoff.
    fn synchronize_evidence<'a>(
        &'a mut self,
        cutoff_at: &'a str,
    ) -> ControllerFuture<'a, NativeMaintenanceDueEvidence, Self::Error>;

    /// Flush the actual partial custody pack before any due semantic reads.
    fn flush_evidence(&mut self) -> ControllerFuture<'_, (), Self::Error>;

    /// Read the actual current canonical controller commit clock.
    fn canonical_now(&mut self) -> Result<String, Self::Error>;

    /// Execute one due with the original retained revision cycle.
    fn execute_due<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        now: &'a str,
        evidence: &'a NativeMaintenanceDueEvidence,
    ) -> ControllerFuture<'a, NativeMaintenanceDueExecutionResult, Self::Error>;
}

/// Actual controller run output with exact source mode, start and required adapter list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceControllerRun {
    pub contract_version: &'static str,
    pub mode: NativeMaintenanceDueMode,
    pub started_at: String,
    pub required_adapters: [&'static str; 8],
    pub attempts: Vec<NativeMaintenanceDueExecutionResult>,
}

/// Closed top-level refusal, preserving each actual owning phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceControllerRunError {
    Lock,
    Journal,
    Archive,
    Lifecycle,
    Schedule,
    Clock,
    Evidence,
    Planner,
}

/// Run the original process-lock-serialized controller and every bounded due sequentially.
pub async fn run_native_maintenance_controller<Ports>(
    ports: &mut Ports,
    mode: NativeMaintenanceDueMode,
) -> Result<NativeMaintenanceControllerRun, NativeMaintenanceControllerRunError>
where
    Ports: NativeMaintenanceControllerRunPorts,
{
    use NativeMaintenanceControllerRunError as Error;
    let _guard = ports.acquire_process_lock().map_err(|_| Error::Lock)?;
    ports
        .initialize_journal()
        .await
        .map_err(|_| Error::Journal)?;
    ports
        .initialize_archive()
        .await
        .map_err(|_| Error::Archive)?;
    ports
        .initialize_lifecycle()
        .await
        .map_err(|_| Error::Lifecycle)?;
    let schedule = ports
        .read_registration_schedule()
        .await
        .map_err(|_| Error::Schedule)?;
    let started_at = schedule.snapshot_at.clone();
    NativeWirePrimitives
        .canonical_utc_milliseconds(&started_at)
        .ok_or(Error::Clock)?;
    // capture a single authenticated database snapshot before exposing immutable due inputs
    let evidence = ports
        .synchronize_evidence(&started_at)
        .await
        .map_err(|_| Error::Evidence)?;
    ports.flush_evidence().await.map_err(|_| Error::Evidence)?;
    let dues = plan_native_adjustment_maintenance_dues(ports, mode, &started_at, &schedule)
        .await
        .map_err(|_| Error::Planner)?;
    let mut attempts = Vec::with_capacity(dues.len());
    // independent due failures cannot prevent subsequent due reconciliation
    for due in dues {
        let attempted = match ports.canonical_now() {
            Ok(now)
                if NativeWirePrimitives
                    .canonical_utc_milliseconds(&now)
                    .is_some() =>
            {
                ports.execute_due(&due, &now, &evidence).await.ok()
            }
            _ => None,
        };
        attempts.push(
            attempted.unwrap_or_else(|| NativeMaintenanceDueExecutionResult {
                due_key: due.due_key,
                state: "failed".to_owned(),
                reason: Some("controller_error".to_owned()),
                manifest_object_sha256: None,
                report_sha256: None,
                output_sha256: None,
                action_sha256: None,
            }),
        );
    }
    Ok(NativeMaintenanceControllerRun {
        contract_version: NATIVE_MAINTENANCE_CONTROLLER_RUN_VERSION,
        mode,
        started_at,
        required_adapters: NATIVE_MAINTENANCE_REQUIRED_ADAPTERS,
        attempts,
    })
}

#[cfg(test)]
#[path = "native_maintenance_controller_run_test.rs"]
mod tests;

/// Encode one exact value-free controller run with source-specific optional attempt fields.
pub fn encode_native_maintenance_controller_run(
    run: &NativeMaintenanceControllerRun,
) -> Option<Vec<u8>> {
    use crate::native_maintenance_policy_comparison::{object, string};
    use weather_contracts::JsonValue;
    use weather_edge::ArchiveWirePrimitives;

    let mut attempts = Vec::with_capacity(run.attempts.len());
    // retain absent fields rather than replacing the predecessor's optional keys with nulls
    for attempt in &run.attempts {
        let mut fields = vec![
            ("dueKey", string(&attempt.due_key)),
            ("state", string(&attempt.state)),
        ];
        for (key, value) in [
            ("reason", &attempt.reason),
            ("manifestObjectSha256", &attempt.manifest_object_sha256),
            ("reportSha256", &attempt.report_sha256),
            ("outputSha256", &attempt.output_sha256),
            ("actionSha256", &attempt.action_sha256),
        ] {
            if let Some(value) = value {
                fields.push((key, string(value)));
            }
        }
        attempts.push(object(fields));
    }
    NativeWirePrimitives.encode_sorted_json(&object(vec![
        ("attempts", JsonValue::Array(attempts)),
        ("contractVersion", string(run.contract_version)),
        ("mode", string(run.mode.as_str())),
        (
            "requiredAdapters",
            JsonValue::Array(
                run.required_adapters
                    .iter()
                    .map(|adapter| string(adapter))
                    .collect(),
            ),
        ),
        ("startedAt", string(&run.started_at)),
    ]))
}
