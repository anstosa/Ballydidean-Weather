//! Concrete installed controller due-work composition over native semantic, candidate and action owners.

use std::{future::Future, path::Path, pin::Pin};

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeMaintenanceCandidateOwnerError, NativeMaintenanceCandidateParity,
    NativeMaintenanceCandidateParityRequest, NativeMaintenanceCandidatePersistenceRequest,
    NativeMaintenanceCandidateRegistrationFuture, NativeMaintenanceCandidateRegistrationRequest,
    NativeMaintenanceCaptureFamily, NativeMaintenanceControlReferenceInput,
    NativeMaintenanceControlReferenceMaterial, NativeMaintenanceControllerInstalledPorts,
    NativeMaintenanceDailyDisposition, NativeMaintenanceDailyEvaluation,
    NativeMaintenanceDailyEvaluationRequest, NativeMaintenanceDailyQualifiedCompletion,
    NativeMaintenanceDailyTerminalAssemblyMode, NativeMaintenanceDailyTerminalMaterial,
    NativeMaintenanceDailyTerminalValues, NativeMaintenanceDailyTerminalValuesInput,
    NativeMaintenanceDailyUnsupportedMaterial, NativeMaintenanceDue, NativeMaintenanceDueEvidence,
    NativeMaintenanceDueExecutionPorts, NativeMaintenanceDueRelease,
    NativeMaintenanceDueReleaseContext, NativeMaintenanceDueWorkPorts, NativeMaintenanceFitRequest,
    NativeMaintenanceFitResult, NativeMaintenanceInstalledCandidateOwner,
    NativeMaintenanceInstalledSemanticError, NativeMaintenanceInstalledSemanticInspection,
    NativeMaintenanceInstalledSemanticOwner, NativeMaintenanceInstalledTerminalMaterial,
    NativeMaintenancePersistedCandidate, NativeMaintenanceSemanticInspection,
    NativeRegistrationLifecycleStatus, NativeRegistrationScheduleStatus, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness,
    assemble_native_maintenance_daily_rain_confirmation,
    assemble_native_maintenance_daily_rain_unsupported_confirmation,
    assemble_native_maintenance_daily_terminal_values, maintenance_canonical_json::valid_hash,
};

type ActionFuture<'a, T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + 'a>>;

/// Closed original action ownership boundary for fenced releases and terminal retirement.
pub trait NativeMaintenanceInstalledActionOwner {
    type Error;

    /// Bind the authenticated controller epoch and actual action owners after the process lock.
    fn initialize_locked(
        &mut self,
        witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    ) -> Result<(), Self::Error>;

    /// Validate the original fixed rain control-reference material before publication.
    fn validate_control_reference(
        &mut self,
        due: &NativeMaintenanceDue,
        now: &str,
        input: NativeMaintenanceControlReferenceInput,
    ) -> Result<NativeMaintenanceControlReferenceMaterial, Self::Error>;

    /// Complete the genuine unsupported owner terminal through the retained archive and journal.
    fn complete_unsupported<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        semantic_input: &'a str,
        material: &'a NativeMaintenanceInstalledTerminalMaterial,
    ) -> ActionFuture<'a, (), Self::Error>;

    /// Publish a qualified action and complete its actual owner retirement.
    fn complete_qualified<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        evaluation: &'a NativeMaintenanceDailyEvaluation,
        material: &'a NativeMaintenanceInstalledTerminalMaterial,
    ) -> ActionFuture<'a, NativeMaintenanceDailyQualifiedCompletion, Self::Error>;

    /// Complete one evaluated terminal with the original no-action disposition.
    fn complete_no_action<'a>(
        &'a mut self,
        disposition: NativeMaintenanceDailyDisposition,
        due: &'a NativeMaintenanceDue,
        evaluation: &'a NativeMaintenanceDailyEvaluation,
        material: &'a NativeMaintenanceInstalledTerminalMaterial,
    ) -> ActionFuture<'a, (), Self::Error>;

    /// Reconcile the exact retained owner terminal command before publishing an attempt.
    fn reconcile_terminal<'a>(&'a mut self, due_key: &'a str) -> ActionFuture<'a, (), Self::Error>;

    /// Apply the actual pre-month control reference under its fenced source action.
    fn publish_control_reference<'a>(
        &'a mut self,
        context: NativeMaintenanceDueReleaseContext<'a>,
    ) -> ActionFuture<'a, NativeMaintenanceDueRelease, Self::Error>;

    /// Apply the actual registered inactive candidate with its durable typed artifact ownership.
    fn publish_development_candidate<'a>(
        &'a mut self,
        context: NativeMaintenanceDueReleaseContext<'a>,
        candidate: &'a NativeMaintenanceInstalledCandidateOwner,
    ) -> ActionFuture<'a, NativeMaintenanceDueRelease, Self::Error>;
}

/// Source effect or typed action failure without unvalidated material.
#[derive(Debug)]
pub enum NativeMaintenanceInstalledDueError<ActionError> {
    Semantic(NativeMaintenanceInstalledSemanticError),
    Candidate(NativeMaintenanceCandidateOwnerError),
    Action(ActionError),
    Terminal,
    Input,
}

type DueFuture<'a, T, E> = ActionFuture<'a, T, NativeMaintenanceInstalledDueError<E>>;

/// Compose all admitted native due operations with the installed fenced action owner.
pub struct NativeMaintenanceInstalledDuePorts<Actions> {
    semantic: NativeMaintenanceInstalledSemanticOwner,
    actions: Actions,
    due: Option<NativeMaintenanceDue>,
    terminal: Option<NativeMaintenanceInstalledTerminalMaterial>,
    evaluation_identity: Option<[u8; 32]>,
    terminal_identity: Option<[u8; 32]>,
}

impl<Actions: NativeMaintenanceInstalledActionOwner> NativeMaintenanceInstalledDuePorts<Actions> {
    /// Bind the actual current release; all mutating operations remain deferred until the controller lock.
    pub fn bind_installed(
        root: &Path,
        commit: &str,
        actions: Actions,
    ) -> Result<Self, NativeMaintenanceInstalledDueError<Actions::Error>> {
        Ok(Self {
            semantic: NativeMaintenanceInstalledSemanticOwner::bind_installed(root, commit)
                .map_err(NativeMaintenanceInstalledDueError::Semantic)?,
            actions,
            due: None,
            terminal: None,
            evaluation_identity: None,
            terminal_identity: None,
        })
    }

    /// Require the exact retained terminal receipt rather than a caller-supplied shadow identity.
    fn check_terminal(
        &self,
        due: &NativeMaintenanceDue,
        value: &JsonValue,
    ) -> Result<(), NativeMaintenanceInstalledDueError<Actions::Error>> {
        if self.due.as_ref() != Some(due)
            || self.terminal.is_none()
            || self.terminal_identity.is_none()
            || self.terminal_identity != identity(value)
        {
            return Err(NativeMaintenanceInstalledDueError::Input);
        }
        Ok(())
    }
}

impl<Actions: NativeMaintenanceInstalledActionOwner> NativeMaintenanceControllerInstalledPorts
    for NativeMaintenanceInstalledDuePorts<Actions>
{
    /// Retain the authenticated initial epoch and registration transaction before any due can execute.
    fn bind_initial_scope(
        &mut self,
        schedule: NativeRegistrationScheduleStatus,
        lifecycle: NativeRegistrationLifecycleStatus,
        witness: VerifiedNativeMaintenanceCaptureEpochWitness,
        witness_bytes: Vec<u8>,
    ) -> Result<(), Self::Error> {
        self.actions
            .initialize_locked(&witness)
            .map_err(NativeMaintenanceInstalledDueError::Action)?;
        self.semantic
            .initialize_locked(schedule, lifecycle, witness, witness_bytes)
            .map_err(NativeMaintenanceInstalledDueError::Semantic)
    }
}

impl<Actions: NativeMaintenanceInstalledActionOwner> NativeMaintenanceDueWorkPorts
    for NativeMaintenanceInstalledDuePorts<Actions>
{
    type Error = NativeMaintenanceInstalledDueError<Actions::Error>;

    /// Delegate a genuine control reference to its original separate source action owner.
    fn validate_control_reference(
        &mut self,
        due: &NativeMaintenanceDue,
        now: &str,
        input: NativeMaintenanceControlReferenceInput,
    ) -> Result<NativeMaintenanceControlReferenceMaterial, Self::Error> {
        self.actions
            .validate_control_reference(due, now, input)
            .map_err(Self::Error::Action)
    }

    /// Complete the exact burned unsupported material, never a nominal completed report.
    fn complete_unsupported_daily<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        semantic_input: &'a str,
        material: &'a NativeMaintenanceDailyUnsupportedMaterial,
    ) -> DueFuture<'a, (), Actions::Error> {
        Box::pin(async move {
            self.check_terminal(due, &material.value)?;
            if !valid_hash(semantic_input) {
                return Err(Self::Error::Input);
            }
            self.actions
                .complete_unsupported(
                    due,
                    semantic_input,
                    self.terminal.as_ref().ok_or(Self::Error::Terminal)?,
                )
                .await
                .map_err(Self::Error::Action)
        })
    }

    /// Evaluate the actual retained complete candidate and package or the original unopened receipt.
    fn evaluate_daily(
        &mut self,
        input: &NativeMaintenanceDailyEvaluationRequest,
    ) -> Result<NativeMaintenanceDailyEvaluation, Self::Error> {
        let due = self.due.as_ref().ok_or(Self::Error::Input)?;
        if self.evaluation_identity != identity(&input.value) {
            return Err(Self::Error::Input);
        }
        if let Some(terminal) = self.terminal.as_ref() {
            terminal
                .evaluate(
                    due,
                    self.semantic
                        .candidate
                        .witness()
                        .ok_or(Self::Error::Input)?,
                )
                .map_err(|_| Self::Error::Terminal)
        } else {
            self.semantic
                .evaluate_pending(input, due)
                .ok_or(Self::Error::Input)
        }
    }

    /// Apply the qualifying action only from the retained exact terminal input.
    fn complete_qualified_daily<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        evaluation: &'a NativeMaintenanceDailyEvaluation,
        material: &'a NativeMaintenanceDailyTerminalMaterial,
    ) -> DueFuture<'a, NativeMaintenanceDailyQualifiedCompletion, Actions::Error> {
        Box::pin(async move {
            self.check_terminal(due, &material.value)?;
            self.actions
                .complete_qualified(
                    due,
                    evaluation,
                    self.terminal.as_ref().ok_or(Self::Error::Terminal)?,
                )
                .await
                .map_err(Self::Error::Action)
        })
    }

    /// Complete the original evaluated no-action disposition with actual terminal material.
    fn complete_no_action_daily<'a>(
        &'a mut self,
        disposition: NativeMaintenanceDailyDisposition,
        due: &'a NativeMaintenanceDue,
        evaluation: &'a NativeMaintenanceDailyEvaluation,
        material: &'a NativeMaintenanceDailyTerminalMaterial,
    ) -> DueFuture<'a, (), Actions::Error> {
        Box::pin(async move {
            self.check_terminal(due, &material.value)?;
            self.actions
                .complete_no_action(
                    disposition,
                    due,
                    evaluation,
                    self.terminal.as_ref().ok_or(Self::Error::Terminal)?,
                )
                .await
                .map_err(Self::Error::Action)
        })
    }

    /// Execute the real native fitter in the existing fixed isolated process envelope.
    fn run_fit<'a>(
        &'a mut self,
        request: NativeMaintenanceFitRequest<'a>,
    ) -> DueFuture<'a, NativeMaintenanceFitResult, Actions::Error> {
        Box::pin(async move {
            self.semantic
                .candidate
                .run_fit(request)
                .map_err(Self::Error::Candidate)
        })
    }

    /// Build real independent raw/package parity using the native model implementations.
    fn build_candidate_parity(
        &mut self,
        request: NativeMaintenanceCandidateParityRequest<'_>,
    ) -> Result<Option<NativeMaintenanceCandidateParity>, Self::Error> {
        self.semantic
            .candidate
            .build_candidate_parity(request)
            .map_err(Self::Error::Candidate)
    }

    /// Report actual installed candidate custody owner availability.
    fn candidate_persistence_available(&self) -> bool {
        self.semantic.candidate.candidate_persistence_available()
    }

    /// Assemble exact rolling registration after fitting and parity.
    fn build_candidate_registration<'a>(
        &'a mut self,
        request: NativeMaintenanceCandidateRegistrationRequest<'a>,
    ) -> NativeMaintenanceCandidateRegistrationFuture<'a, Self::Error> {
        Box::pin(async move {
            self.semantic
                .candidate
                .build_candidate_registration(request)
                .map_err(Self::Error::Candidate)
        })
    }

    /// Persist the actual immutable candidate graph before preregistration.
    fn persist_candidate<'a>(
        &'a mut self,
        request: NativeMaintenanceCandidatePersistenceRequest<'a>,
    ) -> DueFuture<'a, NativeMaintenancePersistedCandidate, Actions::Error> {
        Box::pin(async move {
            self.semantic
                .candidate
                .persist_candidate(request)
                .map_err(Self::Error::Candidate)
        })
    }

    /// Read the actual durable confirmation journal head.
    fn read_confirmation_head<'a>(&'a mut self) -> DueFuture<'a, String, Actions::Error> {
        Box::pin(async move {
            self.semantic
                .candidate
                .read_confirmation_head()
                .map_err(Self::Error::Candidate)
        })
    }

    /// Preregister the exact local family slot before inactive shadow application.
    fn preregister_confirmation<'a>(
        &'a mut self,
        confirmation: &'a JsonValue,
        input_head: &'a str,
        now: &'a str,
    ) -> DueFuture<'a, String, Actions::Error> {
        Box::pin(async move {
            self.semantic
                .candidate
                .preregister_confirmation(confirmation, input_head, now)
                .map_err(Self::Error::Candidate)
        })
    }
}

impl<Actions: NativeMaintenanceInstalledActionOwner> NativeMaintenanceDueExecutionPorts
    for NativeMaintenanceInstalledDuePorts<Actions>
{
    /// Read actual semantic catalog, burn terminal access and assemble the genuine value population.
    fn inspect_semantic_input<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        input_head: &'a str,
        evidence: Option<&'a NativeMaintenanceDueEvidence>,
    ) -> DueFuture<'a, NativeMaintenanceSemanticInspection, Actions::Error> {
        Box::pin(async move {
            if !valid_hash(input_head) {
                return Err(Self::Error::Input);
            }
            self.due = Some(due.clone());
            self.terminal = None;
            self.evaluation_identity = None;
            self.terminal_identity = None;
            let inspected = self
                .semantic
                .inspect(due, evidence)
                .await
                .map_err(Self::Error::Semantic)?;
            let inspection = match inspected {
                NativeMaintenanceInstalledSemanticInspection::Ready(inspection) => *inspection,
                NativeMaintenanceInstalledSemanticInspection::DailyTerminal { catalog, access } => {
                    let witness = self
                        .semantic
                        .candidate
                        .witness()
                        .ok_or(Self::Error::Input)?;
                    let witness_json = NativeWirePrimitives
                        .decode_json(
                            self.semantic
                                .candidate
                                .witness_bytes()
                                .ok_or(Self::Error::Input)?,
                        )
                        .ok_or(Self::Error::Input)?;
                    let family = catalog
                        .active_entry
                        .as_ref()
                        .ok_or(Self::Error::Input)?
                        .slot
                        .family;
                    let clock = self.semantic.controller_clock().ok_or(Self::Error::Input)?;
                    let make_input = || NativeMaintenanceDailyTerminalValuesInput {
                        access: &access.local_burn,
                        catalog_input_manifest_sha256: &catalog.catalog.input_manifest_sha256,
                        clock_at: clock,
                        confirmation_registration: &access.confirmation_registration,
                        due,
                        epoch_witness: &witness_json,
                        history: &catalog.catalog.history,
                        plan: &access.plan,
                        shadow_registration: &access.shadow_registration,
                        source_commit: &access.source_commit,
                        verified_epoch_witness: witness,
                    };
                    let assemble = |mode| {
                        if family == NativeMaintenanceCaptureFamily::Rain {
                            match mode {
                                NativeMaintenanceDailyTerminalAssemblyMode::Complete => assemble_native_maintenance_daily_rain_confirmation(make_input()),
                                NativeMaintenanceDailyTerminalAssemblyMode::Unsupported => assemble_native_maintenance_daily_rain_unsupported_confirmation(make_input()),
                            }.map_err(|_| Self::Error::Terminal)
                        } else {
                            assemble_native_maintenance_daily_terminal_values(make_input(), mode)
                                .map_err(|_| Self::Error::Terminal)
                        }
                    };
                    let mode = if access.unsupported {
                        NativeMaintenanceDailyTerminalAssemblyMode::Unsupported
                    } else {
                        NativeMaintenanceDailyTerminalAssemblyMode::Complete
                    };
                    let mut assembly = assemble(mode)?;
                    // preserve the original complete-plan burn when later source target values are unavailable
                    if matches!(
                        assembly,
                        NativeMaintenanceDailyTerminalValues::Pending { .. }
                    ) {
                        assembly =
                            assemble(NativeMaintenanceDailyTerminalAssemblyMode::Unsupported)?;
                    }
                    let terminal = self
                        .semantic
                        .finish_daily_terminal(&catalog, *access, assembly)
                        .map_err(Self::Error::Semantic)?;
                    let inspection = terminal.inspection(due).ok_or(Self::Error::Terminal)?;
                    self.terminal = Some(terminal);
                    inspection
                }
            };
            match &inspection {
                NativeMaintenanceSemanticInspection::DailyReady {
                    input,
                    terminal_material,
                    ..
                } => {
                    self.evaluation_identity = identity(&input.value);
                    self.terminal_identity = terminal_material
                        .as_ref()
                        .and_then(|material| identity(&material.value));
                }
                NativeMaintenanceSemanticInspection::DailyUnsupported { material, .. } => {
                    self.terminal_identity = identity(&material.value)
                }
                _ => {}
            }
            Ok(inspection)
        })
    }

    /// Reconcile only the exact retained source owner terminal.
    fn reconcile_terminal_outcome<'a>(
        &'a mut self,
        due_key: &'a str,
    ) -> DueFuture<'a, (), Actions::Error> {
        Box::pin(async move {
            self.actions
                .reconcile_terminal(due_key)
                .await
                .map_err(Self::Error::Action)
        })
    }

    /// Install the fenced original control reference action.
    fn publish_control_reference<'a>(
        &'a mut self,
        context: NativeMaintenanceDueReleaseContext<'a>,
    ) -> DueFuture<'a, NativeMaintenanceDueRelease, Actions::Error> {
        Box::pin(async move {
            self.actions
                .publish_control_reference(context)
                .await
                .map_err(Self::Error::Action)
        })
    }

    /// Install the real registered candidate inactive only after its immutable report is retained.
    fn publish_development_candidate<'a>(
        &'a mut self,
        context: NativeMaintenanceDueReleaseContext<'a>,
    ) -> DueFuture<'a, NativeMaintenanceDueRelease, Actions::Error> {
        Box::pin(async move {
            self.actions
                .publish_development_candidate(context, &self.semantic.candidate)
                .await
                .map_err(Self::Error::Action)
        })
    }
}

/// Bind opaque source projections to the actual retained typed input without retaining duplicate wire bytes.
fn identity(value: &JsonValue) -> Option<[u8; 32]> {
    let wire = NativeWirePrimitives;
    Some(RevisionArchiveCanonicalCodec::sha256(
        &wire,
        &wire.encode_sorted_json(value)?,
    ))
}
