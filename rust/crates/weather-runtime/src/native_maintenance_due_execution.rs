//! Source-ordered execution of one immutable due attempt under its ordinary journal lease.

use crate::{
    NativeMaintenanceArchiveJournal, NativeMaintenanceArchiveSegmentArchive,
    NativeMaintenanceArchiveSegmentClock, NativeMaintenanceAttemptMaterial,
    NativeMaintenanceAttemptReport, NativeMaintenanceControllerLeaseJournal, NativeMaintenanceDue,
    NativeMaintenanceDueAttemptJournal, NativeMaintenanceDueTerminalOutcome,
    NativeMaintenanceDueTerminalReason, NativeMaintenanceDueWorkOutcome,
    NativeMaintenanceDueWorkPorts, NativeMaintenanceDueWorkReason, NativeMaintenanceDueWorkState,
    NativeMaintenanceEvidenceSynchronization, NativeMaintenanceJournalStore,
    NativeMaintenanceRecordDevelopmentCandidateInstalled, NativeMaintenanceRecordDueAttemptContext,
    NativeMaintenanceSemanticInspection, NativeRevisionColdDrainResult,
    build_native_maintenance_attempt_report, maintenance_canonical_json::valid_hash,
    native_maintenance_attempt_archive_segment,
    native_maintenance_retirement_files_v2::format_digest,
    publish_native_maintenance_archive_member, run_native_maintenance_due_work,
    verify_native_maintenance_due,
};
use std::{future::Future, pin::Pin};

type DueFuture<'a, T, Error> = Pin<Box<dyn Future<Output = Result<T, Error>> + 'a>>;

/// The real evidence cycle available to a due's semantic assembler.
pub type NativeMaintenanceDueEvidence =
    NativeMaintenanceEvidenceSynchronization<NativeRevisionColdDrainResult>;

/// Context shared by the report-backed publication owners.
pub struct NativeMaintenanceDueReleaseContext<'a> {
    pub due: &'a NativeMaintenanceDue,
    pub input_head_sha256: &'a str,
    pub lifecycle_head_sha256: &'a str,
    pub report: &'a NativeMaintenanceAttemptReport,
    pub report_created_at: &'a str,
    pub outcome: &'a NativeMaintenanceDueWorkOutcome,
}

/// Acknowledged release receipt returned by the actual action owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDueRelease {
    pub action_sha256: String,
    pub acknowledged: bool,
}

/// Explicit semantic assembly and release owners extending the source due-work state machine.
pub trait NativeMaintenanceDueExecutionPorts: NativeMaintenanceDueWorkPorts {
    /// Inspect the genuine retained evidence with its original cutoff.
    fn inspect_semantic_input<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        input_head: &'a str,
        revision_cycle: Option<&'a NativeMaintenanceDueEvidence>,
    ) -> DueFuture<'a, NativeMaintenanceSemanticInspection, Self::Error>;

    /// Complete a real retained owner-retirement command before publishing its report.
    fn reconcile_terminal_outcome<'a>(
        &'a mut self,
        due_key: &'a str,
    ) -> DueFuture<'a, (), Self::Error>;

    /// Publish an archived control reference under its actual fenced action.
    fn publish_control_reference<'a>(
        &'a mut self,
        context: NativeMaintenanceDueReleaseContext<'a>,
    ) -> DueFuture<'a, NativeMaintenanceDueRelease, Self::Error>;

    /// Install a selected candidate inactive before completing its original due.
    fn publish_development_candidate<'a>(
        &'a mut self,
        context: NativeMaintenanceDueReleaseContext<'a>,
    ) -> DueFuture<'a, NativeMaintenanceDueRelease, Self::Error>;
}

/// Closed controller execution refusal by its first owning boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueExecutionError {
    Invalid,
    Journal,
    Clock,
    Work,
    Archive,
    Release,
}

/// Canonical value-free result of one attempted due.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDueExecutionResult {
    pub due_key: String,
    pub state: String,
    pub reason: Option<String>,
    pub manifest_object_sha256: Option<String>,
    pub report_sha256: Option<String>,
    pub output_sha256: Option<String>,
    pub action_sha256: Option<String>,
}

/// Execute exactly one source due with real lease, attempt context, archive and action ownership.
pub async fn execute_native_maintenance_due<Archive, Clock, Ports>(
    due: &NativeMaintenanceDue,
    now: &str,
    revision_cycle: Option<&NativeMaintenanceDueEvidence>,
    journal: &mut NativeMaintenanceJournalStore,
    archive: &mut Archive,
    clock: &mut Clock,
    ports: &mut Ports,
) -> Result<NativeMaintenanceDueExecutionResult, NativeMaintenanceDueExecutionError>
where
    Archive: NativeMaintenanceArchiveSegmentArchive,
    Clock: NativeMaintenanceArchiveSegmentClock,
    Ports: NativeMaintenanceDueExecutionPorts,
{
    use NativeMaintenanceDueExecutionError as Error;
    if !verify_native_maintenance_due(due) {
        return Err(Error::Invalid);
    }
    let initial = NativeMaintenanceControllerLeaseJournal::new(journal)
        .state(&due.due_key)
        .map_err(|_| Error::Journal)?;
    // return immutable completion before replaying an already completed due
    if let Some(output) = initial.due.output_sha256 {
        return Ok(NativeMaintenanceDueExecutionResult {
            due_key: due.due_key.clone(),
            state: "already_complete".to_owned(),
            reason: None,
            output_sha256: Some(format_digest(output)),
            manifest_object_sha256: None,
            report_sha256: None,
            action_sha256: None,
        });
    }
    let mut lease = NativeMaintenanceControllerLeaseJournal::new(journal);
    lease
        .reconcile_expired(&due.scope, now)
        .map_err(|_| Error::Journal)?;
    let current = lease.state(&due.due_key).map_err(|_| Error::Journal)?;
    let Some(input_head) = initial.due.input_head_sha256.or(current.head_sha256) else {
        return Ok(NativeMaintenanceDueExecutionResult {
            due_key: due.due_key.clone(),
            state: "blocked".to_owned(),
            reason: Some("history_unavailable".to_owned()),
            output_sha256: None,
            manifest_object_sha256: None,
            report_sha256: None,
            action_sha256: None,
        });
    };
    let head = current.head_sha256.ok_or(Error::Journal)?;
    let run = NativeMaintenanceControllerLeaseJournal::run_id(&due.due_key, head)
        .ok_or(Error::Invalid)?;
    lease
        .acquire(&due.scope, &due.due_key, &run, now, input_head)
        .map_err(|_| Error::Journal)?;
    // preserve the first failure and release only this acquired ordinary controller lease
    let result = execute_leased(
        due,
        now,
        revision_cycle,
        journal,
        archive,
        clock,
        ports,
        input_head,
        &run,
    )
    .await;
    if result.is_err()
        && let Some(now) = clock.now()
    {
        let _ = NativeMaintenanceControllerLeaseJournal::new(journal).release(
            &due.scope,
            &due.due_key,
            &run,
            &now,
        );
    }
    result
}

#[allow(clippy::too_many_arguments)]
// execute source receipt and release stages after an actual ordinary lease acquisition
async fn execute_leased<Archive, Clock, Ports>(
    due: &NativeMaintenanceDue,
    now: &str,
    revision_cycle: Option<&NativeMaintenanceDueEvidence>,
    journal: &mut NativeMaintenanceJournalStore,
    archive: &mut Archive,
    clock: &mut Clock,
    ports: &mut Ports,
    input_head: [u8; 32],
    run: &str,
) -> Result<NativeMaintenanceDueExecutionResult, NativeMaintenanceDueExecutionError>
where
    Archive: NativeMaintenanceArchiveSegmentArchive,
    Clock: NativeMaintenanceArchiveSegmentClock,
    Ports: NativeMaintenanceDueExecutionPorts,
{
    use NativeMaintenanceDueExecutionError as Error;
    let input_head = format_digest(input_head);
    let leased_head = NativeMaintenanceControllerLeaseJournal::new(journal)
        .state(&due.due_key)
        .map_err(|_| Error::Journal)?
        .head_sha256
        .map(format_digest)
        .ok_or(Error::Journal)?;
    let mut attempt = NativeMaintenanceDueAttemptJournal::new(journal);
    let context = match attempt
        .read_due_attempt_context(&due.due_key)
        .map_err(|_| Error::Journal)?
    {
        Some(context) => context,
        None => attempt
            .record_due_attempt_context(NativeMaintenanceRecordDueAttemptContext {
                due_key: &due.due_key,
                input_head_sha256: &input_head,
                lifecycle_head_sha256: &leased_head,
                now: &clock.now().ok_or(Error::Clock)?,
            })
            .map_err(|_| Error::Journal)?,
    };
    if context.input_head_sha256 != input_head {
        return Err(Error::Journal);
    }
    // prefer the immutable retained terminal outcome over all new model work
    let outcome = if let Some(retained) = attempt
        .read_due_terminal_outcome(&due.due_key)
        .map_err(|_| Error::Journal)?
    {
        ports
            .reconcile_terminal_outcome(&due.due_key)
            .await
            .map_err(|_| Error::Work)?;
        retained_outcome(retained)
    } else {
        match ports
            .inspect_semantic_input(due, &input_head, revision_cycle)
            .await
        {
            Ok(inspection) => run_native_maintenance_due_work(due, now, inspection, ports)
                .await
                .map_err(|_| Error::Work)?,
            Err(_) => dependency_blocked(),
        }
    };
    let report = build_native_maintenance_attempt_report(NativeMaintenanceAttemptMaterial {
        due_key: &due.due_key,
        mode: due.mode.as_str(),
        family: due.family.map(|family| family.wire()),
        original_cutoff_at: &due.original_cutoff_at,
        input_head_sha256: &input_head,
        lifecycle_head_sha256: &context.lifecycle_head_sha256,
        reason: outcome.reason.as_str(),
        state: outcome.state.as_str(),
        candidate_graph_sha256: outcome.candidate_graph_sha256.as_deref(),
        candidate_sha256: outcome.candidate_sha256.as_deref(),
        fit_receipt_sha256: outcome.fit_receipt_sha256.as_deref(),
        semantic_input_sha256: outcome.semantic_input_sha256.as_deref(),
        action_eligible: outcome.action_eligible,
        serving_changed: outcome.serving_changed,
    })
    .ok_or(Error::Invalid)?;
    let segment = native_maintenance_attempt_archive_segment(&report).ok_or(Error::Archive)?;
    let member = segment.members.into_iter().next().ok_or(Error::Archive)?;
    let publication = publish_native_maintenance_archive_member(
        archive,
        &mut NativeMaintenanceArchiveJournal::new(journal),
        clock,
        member,
    )
    .map_err(|_| Error::Archive)?;
    if publication.member_sha256 != report.report_sha256 {
        return Err(Error::Archive);
    }
    let graph = format_digest(publication.manifest_object_sha256);
    let mut action = None;
    // action receipts are accepted only after the attempt is durably archived
    if outcome.control_reference_material.is_some() {
        let lifecycle_head = required_head(journal, due)?;
        let created = clock.now().ok_or(Error::Clock)?;
        let release = ports
            .publish_control_reference(NativeMaintenanceDueReleaseContext {
                due,
                input_head_sha256: &input_head,
                lifecycle_head_sha256: &lifecycle_head,
                report: &report,
                report_created_at: &created,
                outcome: &outcome,
            })
            .await
            .map_err(|_| Error::Release)?;
        verify_release(&release)?;
        action = Some(release.action_sha256);
    }
    if let Some(development) = &outcome.development_candidate {
        let lifecycle_head = required_head(journal, due)?;
        let created = clock.now().ok_or(Error::Clock)?;
        let release = ports
            .publish_development_candidate(NativeMaintenanceDueReleaseContext {
                due,
                input_head_sha256: &input_head,
                lifecycle_head_sha256: &lifecycle_head,
                report: &report,
                report_created_at: &created,
                outcome: &outcome,
            })
            .await
            .map_err(|_| Error::Release)?;
        verify_release(&release)?;
        // derive the exact installed inactive candidate receipt from the source's preregistered material
        if let Some(shadow) = development
            .registration_material
            .as_ref()
            .and_then(|material| material.shadow_registration.as_ref())
        {
            let family = due.family.ok_or(Error::Invalid)?;
            let candidate_graph = outcome
                .candidate_graph_sha256
                .as_deref()
                .ok_or(Error::Invalid)?;
            let candidate = outcome.candidate_sha256.as_deref().ok_or(Error::Invalid)?;
            let registration = development
                .confirmation_registration_sha256
                .as_deref()
                .ok_or(Error::Invalid)?;
            NativeMaintenanceDueAttemptJournal::new(journal)
                .record_development_candidate_installed(
                    NativeMaintenanceRecordDevelopmentCandidateInstalled {
                        action_sha256: &release.action_sha256,
                        artifact_sha256: &shadow.artifact_sha256,
                        candidate_graph_sha256: candidate_graph,
                        candidate_sha256: candidate,
                        family,
                        now: &clock.now().ok_or(Error::Clock)?,
                        registration_sha256: registration,
                        shadow_registration_sha256: &shadow.registration_sha256,
                    },
                )
                .map_err(|_| Error::Journal)?;
        }
        action = Some(release.action_sha256);
    }
    let mut lease = NativeMaintenanceControllerLeaseJournal::new(journal);
    // blocked and failed reports retain their immutable registered due for later retry
    if outcome.state.as_str() == "completed" {
        lease
            .complete(
                &due.due_key,
                &clock.now().ok_or(Error::Clock)?,
                publication.manifest_object_sha256,
            )
            .map_err(|_| Error::Journal)?;
    }
    lease
        .release(
            &due.scope,
            &due.due_key,
            run,
            &clock.now().ok_or(Error::Clock)?,
        )
        .map_err(|_| Error::Journal)?;
    Ok(NativeMaintenanceDueExecutionResult {
        due_key: due.due_key.clone(),
        state: outcome.state.as_str().to_owned(),
        reason: Some(outcome.reason.as_str().to_owned()),
        manifest_object_sha256: Some(graph),
        report_sha256: Some(report.report_sha256),
        output_sha256: None,
        action_sha256: action,
    })
}

// construct the exact predecessor's retained terminal receipt projection for interrupted due work
fn retained_outcome(
    retained: NativeMaintenanceDueTerminalOutcome,
) -> NativeMaintenanceDueWorkOutcome {
    let reason = match retained.reason {
        NativeMaintenanceDueTerminalReason::DailyCandidatePromoted => {
            NativeMaintenanceDueWorkReason::DailyCandidatePromoted
        }
        NativeMaintenanceDueTerminalReason::DailyCandidateRejected => {
            NativeMaintenanceDueWorkReason::DailyCandidateRejected
        }
        NativeMaintenanceDueTerminalReason::DailySupportFailed => {
            NativeMaintenanceDueWorkReason::DailySupportFailed
        }
    };
    NativeMaintenanceDueWorkOutcome {
        state: NativeMaintenanceDueWorkState::Completed,
        reason,
        candidate_graph_sha256: retained.candidate_graph_sha256,
        candidate_sha256: retained.candidate_sha256,
        fit_receipt_sha256: retained.fit_receipt_sha256,
        semantic_input_sha256: Some(retained.semantic_input_sha256),
        action_eligible: retained.action_eligible,
        serving_changed: retained.serving_changed,
        control_reference_material: None,
        development_candidate: None,
    }
}

// reflect the predecessor's actual semantic dependency refusal without fabricating evidence
fn dependency_blocked() -> NativeMaintenanceDueWorkOutcome {
    NativeMaintenanceDueWorkOutcome {
        state: NativeMaintenanceDueWorkState::Blocked,
        reason: NativeMaintenanceDueWorkReason::DependencyFailed,
        candidate_graph_sha256: None,
        candidate_sha256: None,
        fit_receipt_sha256: None,
        semantic_input_sha256: None,
        action_eligible: false,
        serving_changed: false,
        control_reference_material: None,
        development_candidate: None,
    }
}

// sample the actual verified lifecycle head for one later action
fn required_head(
    journal: &mut NativeMaintenanceJournalStore,
    due: &NativeMaintenanceDue,
) -> Result<String, NativeMaintenanceDueExecutionError> {
    NativeMaintenanceControllerLeaseJournal::new(journal)
        .state(&due.due_key)
        .map_err(|_| NativeMaintenanceDueExecutionError::Journal)?
        .head_sha256
        .map(format_digest)
        .ok_or(NativeMaintenanceDueExecutionError::Journal)
}

// require the actual root acknowledgement and immutable action identity
fn verify_release(
    release: &NativeMaintenanceDueRelease,
) -> Result<(), NativeMaintenanceDueExecutionError> {
    (release.acknowledged && valid_hash(&release.action_sha256))
        .then_some(())
        .ok_or(NativeMaintenanceDueExecutionError::Release)
}
