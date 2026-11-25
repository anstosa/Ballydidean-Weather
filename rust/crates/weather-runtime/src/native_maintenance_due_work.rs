//! Source-ordered semantic work for one already-leased maintenance due.

use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::pin::Pin;

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::maintenance_canonical_json::valid_hash;
use crate::native_maintenance_due_planner::{NativeMaintenanceDue, NativeMaintenanceDueMode};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{NativeMaintenanceCaptureFamily, NativeWirePrimitives};

const FIT_CONTRACT_VERSION: &str = "adjustment-fit-sandbox/v1";

/// Canonical terminal state of one semantic due-work attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueWorkState {
    Blocked,
    Completed,
    Failed,
}

impl NativeMaintenanceDueWorkState {
    /// Return the exact attempt-report state spelling.
    pub const fn as_str(self) -> &'static str {
        // preserve the closed source state set
        match self {
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

/// Canonical reason emitted by the source `runDueWork` state machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueWorkReason {
    CandidateArchiveUnavailable,
    CandidateParityFailed,
    ControlReferenceArchived,
    DailyCandidatePromoted,
    DailyCandidateRejected,
    DailyEvaluationFailed,
    DailyHistoryPending,
    DailyNoRegisteredCandidate,
    DailySupportFailed,
    DependencyFailed,
    DevelopmentCandidateArchived,
    FamilySlotBusy,
    FitFailed,
    HistoryUnavailable,
    MonthlyNoCandidate,
    RegistrationScheduleUnavailable,
    SemanticCatalogUnavailable,
    SemanticInputBlocked,
}

impl NativeMaintenanceDueWorkReason {
    /// Return the exact attempt-report reason spelling.
    pub const fn as_str(self) -> &'static str {
        // preserve every reviewed source outcome spelling
        match self {
            Self::CandidateArchiveUnavailable => "candidate_archive_unavailable",
            Self::CandidateParityFailed => "candidate_parity_failed",
            Self::ControlReferenceArchived => "control_reference_archived",
            Self::DailyCandidatePromoted => "daily_candidate_promoted",
            Self::DailyCandidateRejected => "daily_candidate_rejected",
            Self::DailyEvaluationFailed => "daily_evaluation_failed",
            Self::DailyHistoryPending => "daily_history_pending",
            Self::DailyNoRegisteredCandidate => "daily_no_registered_candidate",
            Self::DailySupportFailed => "daily_support_failed",
            Self::DependencyFailed => "dependency_failed",
            Self::DevelopmentCandidateArchived => "development_candidate_archived",
            Self::FamilySlotBusy => "family_slot_busy",
            Self::FitFailed => "fit_failed",
            Self::HistoryUnavailable => "history_unavailable",
            Self::MonthlyNoCandidate => "monthly_no_candidate",
            Self::RegistrationScheduleUnavailable => "registration_schedule_unavailable",
            Self::SemanticCatalogUnavailable => "semantic_catalog_unavailable",
            Self::SemanticInputBlocked => "semantic_input_blocked",
        }
    }
}

/// One source-admitted control reference retained for later release publication.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceControlReferenceMaterial {
    pub value: JsonValue,
}

/// Unvalidated control material returned by semantic inspection.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceControlReferenceInput {
    pub value: JsonValue,
}

/// Exact daily semantic evaluator input.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDailyEvaluationRequest {
    pub value: JsonValue,
}

/// Exact material for one already-burned daily terminal.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDailyTerminalMaterial {
    pub value: JsonValue,
}

/// Exact disjoint proof and owner material for an unsupported daily terminal.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDailyUnsupportedMaterial {
    pub value: JsonValue,
}

/// Opaque value-bearing monthly fitter input admitted by semantic inspection.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceFitInput {
    pub value: JsonValue,
}

/// Opaque runtime-readiness document bound to the fit process.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceAdmittedFitRuntimeReadiness {
    pub value: JsonValue,
}

/// Retained real-world parity input for one candidate.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceRetainedParityInput {
    pub value: JsonValue,
}

/// Synthetic parity input for one candidate.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceSyntheticParityInput {
    pub value: JsonValue,
}

/// Archive-derived registration context accompanying a complete parity pair.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceRegistrationContext {
    pub value: JsonValue,
}

/// Independent native/package parity result retained with a candidate.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceCandidateParity {
    pub value: JsonValue,
}

/// Source registration material built only after portable artifact identities exist.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceCandidateRegistrationMaterial {
    pub confirmation: JsonValue,
    pub shadow_registration: Option<NativeMaintenanceShadowRegistrationMaterial>,
    pub value: JsonValue,
}

/// Exact shadow registration identities retained for installed-candidate custody.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceShadowRegistrationMaterial {
    pub artifact_sha256: String,
    pub registration_sha256: String,
}

/// Candidate archive result returned by the persistence owner.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenancePersistedCandidate {
    pub candidate_graph_sha256: String,
    pub value: JsonValue,
}

/// Exact isolated fitter receipt before its independently derived receipt hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFitResult {
    pub candidate_json: Vec<u8>,
    pub candidate_sha256: String,
    pub code_snapshot_sha256: String,
    pub contract_version: String,
    pub family: NativeMaintenanceCaptureFamily,
    pub input_snapshot_sha256: String,
    pub runtime_readiness_sha256: String,
    pub stderr: crate::NativeMaintenanceFitLogReport,
    pub stdout: crate::NativeMaintenanceFitLogReport,
}

/// Database-owned family slot state needed before spending fitter capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueRegistrationSlotState {
    BusyV2Legacy,
    BusyV3,
    Free,
}

/// Verified value-blind family slot projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDueRegistrationSlot {
    pub family: NativeMaintenanceCaptureFamily,
    pub state: NativeMaintenanceDueRegistrationSlotState,
}

/// Complete monthly semantic inputs required by fitting and later release.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceMonthlyReadyInspection {
    pub fit_input: NativeMaintenanceFitInput,
    pub input_manifest_sha256: String,
    pub registration_context: Option<NativeMaintenanceRegistrationContext>,
    pub registration_slot: Option<NativeMaintenanceDueRegistrationSlot>,
    pub retained_parity_input: Option<NativeMaintenanceRetainedParityInput>,
    pub runtime_readiness: NativeMaintenanceAdmittedFitRuntimeReadiness,
    pub runtime_readiness_sha256: String,
    pub synthetic_parity_input: Option<NativeMaintenanceSyntheticParityInput>,
}

/// Closed semantic inspection admitted before due work begins.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeMaintenanceSemanticInspection {
    Idle {
        input_manifest_sha256: String,
    },
    Blocked {
        reason: NativeMaintenanceDueWorkReason,
    },
    ControlReference {
        input_manifest_sha256: String,
        material: NativeMaintenanceControlReferenceInput,
    },
    DailyUnsupported {
        input_manifest_sha256: String,
        material: NativeMaintenanceDailyUnsupportedMaterial,
    },
    DailyReady {
        input: NativeMaintenanceDailyEvaluationRequest,
        input_manifest_sha256: String,
        terminal_material: Option<NativeMaintenanceDailyTerminalMaterial>,
    },
    MonthlyNoCandidate {
        input_manifest_sha256: String,
    },
    MonthlyReady(NativeMaintenanceMonthlyReadyInspection),
}

/// Closed daily evaluation outcomes after exact policy-pair validation.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeMaintenanceDailyEvaluation {
    Pending {
        semantic_input_sha256: String,
    },
    Promote {
        semantic_input_sha256: String,
        value: JsonValue,
    },
    Reject {
        semantic_input_sha256: String,
        value: JsonValue,
    },
    SupportFailed {
        semantic_input_sha256: String,
        value: JsonValue,
    },
}

impl NativeMaintenanceDailyEvaluation {
    /// Borrow the evaluator-owned semantic input identity.
    fn semantic_input_sha256(&self) -> &str {
        // select the hash without changing the evaluator variant
        match self {
            Self::Pending {
                semantic_input_sha256,
            }
            | Self::Promote {
                semantic_input_sha256,
                ..
            }
            | Self::Reject {
                semantic_input_sha256,
                ..
            }
            | Self::SupportFailed {
                semantic_input_sha256,
                ..
            } => semantic_input_sha256,
        }
    }
}

/// Source no-action disposition selected from a validated daily policy pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDailyDisposition {
    Rejected,
    SupportFailed,
}

/// Source-qualified completion result needed by the attempt report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDailyQualifiedCompletion {
    pub serving_changed: bool,
}

/// Owned candidate material published only after the immutable attempt report.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDevelopmentCandidateMaterial {
    pub candidate: NativeMaintenancePersistedCandidate,
    pub confirmation_registration_sha256: Option<String>,
    pub fit: NativeMaintenanceFitResult,
    pub inspection: NativeMaintenanceMonthlyReadyInspection,
    pub parity: Option<NativeMaintenanceCandidateParity>,
    pub registration_material: Option<NativeMaintenanceCandidateRegistrationMaterial>,
}

/// Complete report outcome plus optional material for later release publication.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDueWorkOutcome {
    pub action_eligible: bool,
    pub candidate_graph_sha256: Option<String>,
    pub candidate_sha256: Option<String>,
    pub control_reference_material: Option<NativeMaintenanceControlReferenceMaterial>,
    pub development_candidate: Option<NativeMaintenanceDevelopmentCandidateMaterial>,
    pub fit_receipt_sha256: Option<String>,
    pub reason: NativeMaintenanceDueWorkReason,
    pub semantic_input_sha256: Option<String>,
    pub serving_changed: bool,
    pub state: NativeMaintenanceDueWorkState,
}

/// Borrowed request to one isolated monthly fitter.
pub struct NativeMaintenanceFitRequest<'a> {
    pub family: NativeMaintenanceCaptureFamily,
    pub input: &'a NativeMaintenanceFitInput,
    pub runtime_readiness: &'a NativeMaintenanceAdmittedFitRuntimeReadiness,
    pub runtime_readiness_sha256: &'a str,
}

/// Borrowed independent candidate parity request.
pub struct NativeMaintenanceCandidateParityRequest<'a> {
    pub candidate_bytes: &'a [u8],
    pub family: NativeMaintenanceCaptureFamily,
    pub retained_input: Option<&'a NativeMaintenanceRetainedParityInput>,
    pub synthetic_input: Option<&'a NativeMaintenanceSyntheticParityInput>,
}

/// Borrowed registration construction request after fit and parity.
pub struct NativeMaintenanceCandidateRegistrationRequest<'a> {
    pub due: &'a NativeMaintenanceDue,
    pub fit: &'a NativeMaintenanceFitResult,
    pub inspection: &'a NativeMaintenanceMonthlyReadyInspection,
    pub parity: Option<&'a NativeMaintenanceCandidateParity>,
    pub requested_at: &'a str,
}

/// Borrowed immutable candidate persistence request.
pub struct NativeMaintenanceCandidatePersistenceRequest<'a> {
    pub due: &'a NativeMaintenanceDue,
    pub fit: &'a NativeMaintenanceFitResult,
    pub fit_receipt_sha256: &'a str,
    pub parity: Option<&'a NativeMaintenanceCandidateParity>,
    pub registration_material: Option<&'a NativeMaintenanceCandidateRegistrationMaterial>,
}

/// Future for optional candidate registration assembly.
pub type NativeMaintenanceCandidateRegistrationFuture<'a, E> = Pin<
    Box<
        dyn Future<Output = Result<Option<NativeMaintenanceCandidateRegistrationMaterial>, E>> + 'a,
    >,
>;

/// Closed source effects invoked by the due-work state machine.
pub trait NativeMaintenanceDueWorkPorts {
    type Error;

    /// Validate one rain control reference against its due and request clock.
    fn validate_control_reference(
        &mut self,
        due: &NativeMaintenanceDue,
        now: &str,
        input: NativeMaintenanceControlReferenceInput,
    ) -> Result<NativeMaintenanceControlReferenceMaterial, Self::Error>;

    /// Complete one unsupported daily terminal through archive and journal effects.
    fn complete_unsupported_daily<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        semantic_input_sha256: &'a str,
        material: &'a NativeMaintenanceDailyUnsupportedMaterial,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>>;

    /// Evaluate one closed daily semantic input without conferring lifecycle authority.
    fn evaluate_daily(
        &mut self,
        input: &NativeMaintenanceDailyEvaluationRequest,
    ) -> Result<NativeMaintenanceDailyEvaluation, Self::Error>;

    /// Complete one qualifying daily terminal and return actual serving change.
    fn complete_qualified_daily<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        evaluation: &'a NativeMaintenanceDailyEvaluation,
        material: &'a NativeMaintenanceDailyTerminalMaterial,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<NativeMaintenanceDailyQualifiedCompletion, Self::Error>>
                + 'a,
        >,
    >;

    /// Complete one evaluated no-action daily terminal.
    fn complete_no_action_daily<'a>(
        &'a mut self,
        disposition: NativeMaintenanceDailyDisposition,
        due: &'a NativeMaintenanceDue,
        evaluation: &'a NativeMaintenanceDailyEvaluation,
        material: &'a NativeMaintenanceDailyTerminalMaterial,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>>;

    /// Run one isolated family fitter.
    fn run_fit<'a>(
        &'a mut self,
        request: NativeMaintenanceFitRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<NativeMaintenanceFitResult, Self::Error>> + 'a>>;

    /// Build installed native/package parity, or return `None` when no adapter is installed.
    fn build_candidate_parity(
        &mut self,
        request: NativeMaintenanceCandidateParityRequest<'_>,
    ) -> Result<Option<NativeMaintenanceCandidateParity>, Self::Error>;

    /// Report whether immutable candidate persistence is installed.
    fn candidate_persistence_available(&self) -> bool;

    /// Build optional complete registration material after portable identities exist.
    fn build_candidate_registration<'a>(
        &'a mut self,
        request: NativeMaintenanceCandidateRegistrationRequest<'a>,
    ) -> NativeMaintenanceCandidateRegistrationFuture<'a, Self::Error>;

    /// Persist one candidate and its optional registration material immutably.
    fn persist_candidate<'a>(
        &'a mut self,
        request: NativeMaintenanceCandidatePersistenceRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<NativeMaintenancePersistedCandidate, Self::Error>> + 'a>>;

    /// Read the current journal head immediately before confirmation preregistration.
    fn read_confirmation_head<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<String, Self::Error>> + 'a>>;

    /// Occupy the local family slot before a released process can expose predictions.
    fn preregister_confirmation<'a>(
        &'a mut self,
        confirmation: &'a JsonValue,
        input_head_sha256: &'a str,
        now: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, Self::Error>> + 'a>>;
}

/// Due-work validation, serialization, or effect failures that are not report outcomes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueWorkError<Port> {
    InvalidInput,
    Codec,
    Port(Port),
}

impl<Port: Display> Display for NativeMaintenanceDueWorkError<Port> {
    /// Render a bounded diagnostic without reflecting value-bearing materials.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // keep port diagnostics distinct from source admission failures
        match self {
            Self::InvalidInput => formatter.write_str("maintenance due work input is invalid"),
            Self::Codec => formatter.write_str("maintenance due work serialization failed"),
            Self::Port(error) => write!(formatter, "maintenance due work effect failed: {error}"),
        }
    }
}

impl<Port: std::error::Error + 'static> std::error::Error for NativeMaintenanceDueWorkError<Port> {}

/// Validate the exact isolated fitter result and its candidate byte identity.
pub fn validate_native_maintenance_fit_result(
    value: &NativeMaintenanceFitResult,
    family: NativeMaintenanceCaptureFamily,
    runtime_readiness_sha256: &str,
) -> Result<(), NativeMaintenanceDueWorkError<()>> {
    // bind all immutable fitter identities before retaining output
    if !valid_hash(&value.candidate_sha256)
        || !valid_hash(&value.code_snapshot_sha256)
        || !valid_hash(&value.input_snapshot_sha256)
        || !valid_hash(&value.runtime_readiness_sha256)
        || value.contract_version != FIT_CONTRACT_VERSION
        || value.family != family
        || value.runtime_readiness_sha256 != runtime_readiness_sha256
        || std::str::from_utf8(&value.candidate_json).is_err()
        || format_digest(NativeWirePrimitives.sha256(&value.candidate_json))
            != value.candidate_sha256
    {
        return Err(NativeMaintenanceDueWorkError::InvalidInput);
    }
    Ok(())
}

/// Run one exact source-ordered semantic attempt without lease or report publication effects.
pub async fn run_native_maintenance_due_work<Ports>(
    due: &NativeMaintenanceDue,
    now: &str,
    inspection: NativeMaintenanceSemanticInspection,
    ports: &mut Ports,
) -> Result<NativeMaintenanceDueWorkOutcome, NativeMaintenanceDueWorkError<Ports::Error>>
where
    Ports: NativeMaintenanceDueWorkPorts,
{
    use NativeMaintenanceDueWorkError as Error;
    use NativeMaintenanceDueWorkReason as Reason;
    use NativeMaintenanceDueWorkState as State;

    validate_due_shape(due).map_err(|_| Error::InvalidInput)?;
    let inspection_hash = inspection_manifest_hash(&inspection);
    // reject malformed semantic manifest identities before invoking effects
    if inspection_hash.is_some_and(|hash| !valid_hash(hash)) {
        return Err(Error::InvalidInput);
    }

    let inspection = match inspection {
        // complete a genuine no-candidate daily snapshot without invented values
        NativeMaintenanceSemanticInspection::Idle {
            input_manifest_sha256,
        } => {
            if due.mode != NativeMaintenanceDueMode::Daily {
                return Err(Error::InvalidInput);
            }
            return Ok(outcome(
                Reason::DailyNoRegisteredCandidate,
                State::Completed,
                Some(input_manifest_sha256),
            ));
        }
        // retain an honest semantic refusal without fitting
        NativeMaintenanceSemanticInspection::Blocked { reason } => {
            if !matches!(
                reason,
                Reason::DependencyFailed
                    | Reason::HistoryUnavailable
                    | Reason::SemanticCatalogUnavailable
                    | Reason::SemanticInputBlocked
            ) {
                return Err(Error::InvalidInput);
            }
            return Ok(outcome(reason, State::Blocked, None));
        }
        // archive one genuine pre-month rain reference without occupying a model slot
        NativeMaintenanceSemanticInspection::ControlReference {
            input_manifest_sha256,
            material,
        } => {
            if due.mode != NativeMaintenanceDueMode::Monthly
                || due.family != Some(NativeMaintenanceCaptureFamily::Rain)
                || !due.due_key.starts_with("control-reference/rain/")
            {
                return Err(Error::InvalidInput);
            }
            let material = ports
                .validate_control_reference(due, now, material)
                .map_err(Error::Port)?;
            let mut result = outcome(
                Reason::ControlReferenceArchived,
                State::Completed,
                Some(input_manifest_sha256),
            );
            result.control_reference_material = Some(material);
            return Ok(result);
        }
        // complete one unsupported daily member through its disjoint proof
        NativeMaintenanceSemanticInspection::DailyUnsupported {
            input_manifest_sha256,
            material,
        } => {
            if due.mode != NativeMaintenanceDueMode::Daily {
                return Err(Error::InvalidInput);
            }
            ports
                .complete_unsupported_daily(due, &input_manifest_sha256, &material)
                .await
                .map_err(Error::Port)?;
            return Ok(outcome(
                Reason::DailySupportFailed,
                State::Completed,
                Some(input_manifest_sha256),
            ));
        }
        // evaluate only the closed daily terminal projection
        NativeMaintenanceSemanticInspection::DailyReady {
            input,
            input_manifest_sha256,
            terminal_material,
        } => {
            if due.mode != NativeMaintenanceDueMode::Daily {
                return Err(Error::InvalidInput);
            }
            let evaluation = match ports.evaluate_daily(&input) {
                Ok(evaluation) if valid_hash(evaluation.semantic_input_sha256()) => evaluation,
                Ok(_) | Err(_) => {
                    return Ok(outcome(
                        Reason::DailyEvaluationFailed,
                        State::Failed,
                        Some(input_manifest_sha256),
                    ));
                }
            };
            let semantic_input_sha256 = evaluation.semantic_input_sha256().to_owned();
            // retain incomplete history for an exact later retry
            if matches!(evaluation, NativeMaintenanceDailyEvaluation::Pending { .. }) {
                return Ok(outcome(
                    Reason::DailyHistoryPending,
                    State::Blocked,
                    Some(semantic_input_sha256),
                ));
            }
            // fail a qualifying result that lacks terminal owner material
            if matches!(evaluation, NativeMaintenanceDailyEvaluation::Promote { .. })
                && terminal_material.is_none()
            {
                return Ok(outcome(
                    Reason::DailyEvaluationFailed,
                    State::Failed,
                    Some(semantic_input_sha256),
                ));
            }
            // finalize a passing member through immutable release and owner retirement
            if let NativeMaintenanceDailyEvaluation::Promote { .. } = evaluation {
                let completion = ports
                    .complete_qualified_daily(
                        due,
                        &evaluation,
                        terminal_material.as_ref().ok_or(Error::InvalidInput)?,
                    )
                    .await
                    .map_err(Error::Port)?;
                let mut result = outcome(
                    Reason::DailyCandidatePromoted,
                    State::Completed,
                    Some(semantic_input_sha256),
                );
                result.action_eligible = true;
                result.serving_changed = completion.serving_changed;
                return Ok(result);
            }
            let (disposition, reason) = match evaluation {
                NativeMaintenanceDailyEvaluation::Reject { .. } => (
                    NativeMaintenanceDailyDisposition::Rejected,
                    Reason::DailyCandidateRejected,
                ),
                NativeMaintenanceDailyEvaluation::SupportFailed { .. } => (
                    NativeMaintenanceDailyDisposition::SupportFailed,
                    Reason::DailySupportFailed,
                ),
                NativeMaintenanceDailyEvaluation::Pending { .. }
                | NativeMaintenanceDailyEvaluation::Promote { .. } => {
                    return Err(Error::InvalidInput);
                }
            };
            let Some(material) = terminal_material.as_ref() else {
                return Ok(outcome(
                    Reason::DailyEvaluationFailed,
                    State::Failed,
                    Some(semantic_input_sha256),
                ));
            };
            ports
                .complete_no_action_daily(disposition, due, &evaluation, material)
                .await
                .map_err(Error::Port)?;
            return Ok(outcome(
                reason,
                State::Completed,
                Some(semantic_input_sha256),
            ));
        }
        // never spend fitter capacity on a genuine monthly no-candidate snapshot
        NativeMaintenanceSemanticInspection::MonthlyNoCandidate {
            input_manifest_sha256,
        } => {
            if due.mode != NativeMaintenanceDueMode::Monthly {
                return Err(Error::InvalidInput);
            }
            return Ok(outcome(
                Reason::MonthlyNoCandidate,
                State::Completed,
                Some(input_manifest_sha256),
            ));
        }
        NativeMaintenanceSemanticInspection::MonthlyReady(inspection) => inspection,
    };
    if due.mode != NativeMaintenanceDueMode::Monthly {
        return Err(Error::InvalidInput);
    }
    let Some(family) = due.family else {
        return Err(Error::InvalidInput);
    };
    // admit parity only as the complete retained, synthetic, and context cohort
    if inspection.retained_parity_input.is_some() != inspection.synthetic_parity_input.is_some()
        || inspection.registration_context.is_some() != inspection.retained_parity_input.is_some()
        || !valid_hash(&inspection.runtime_readiness_sha256)
    {
        return Err(Error::InvalidInput);
    }
    // refuse an unavailable or family-mismatched database schedule
    let Some(slot) = inspection.registration_slot else {
        return Ok(outcome(
            Reason::RegistrationScheduleUnavailable,
            State::Blocked,
            Some(inspection.input_manifest_sha256),
        ));
    };
    if slot.family != family {
        return Ok(outcome(
            Reason::RegistrationScheduleUnavailable,
            State::Blocked,
            Some(inspection.input_manifest_sha256),
        ));
    }
    // refuse an occupied family slot before spending fitter capacity
    if slot.state != NativeMaintenanceDueRegistrationSlotState::Free {
        return Ok(outcome(
            Reason::FamilySlotBusy,
            State::Blocked,
            Some(inspection.input_manifest_sha256),
        ));
    }

    let fit = match ports
        .run_fit(NativeMaintenanceFitRequest {
            family,
            input: &inspection.fit_input,
            runtime_readiness: &inspection.runtime_readiness,
            runtime_readiness_sha256: &inspection.runtime_readiness_sha256,
        })
        .await
    {
        Ok(fit)
            if validate_native_maintenance_fit_result(
                &fit,
                family,
                &inspection.runtime_readiness_sha256,
            )
            .is_ok() =>
        {
            fit
        }
        Ok(_) | Err(_) => {
            return Ok(outcome(
                Reason::FitFailed,
                State::Failed,
                Some(inspection.input_manifest_sha256),
            ));
        }
    };
    let fit_receipt_sha256 = fit_receipt_sha256(&fit).ok_or(Error::Codec)?;
    let parity = match ports.build_candidate_parity(NativeMaintenanceCandidateParityRequest {
        candidate_bytes: &fit.candidate_json,
        family,
        retained_input: inspection.retained_parity_input.as_ref(),
        synthetic_input: inspection.synthetic_parity_input.as_ref(),
    }) {
        Ok(parity) => parity,
        Err(_) => {
            let mut result = outcome(
                Reason::CandidateParityFailed,
                State::Failed,
                Some(inspection.input_manifest_sha256),
            );
            result.fit_receipt_sha256 = Some(fit_receipt_sha256);
            return Ok(result);
        }
    };
    // refuse promotion before deriving registration when retention is unavailable
    if !ports.candidate_persistence_available() {
        let mut result = outcome(
            Reason::CandidateArchiveUnavailable,
            State::Blocked,
            Some(inspection.input_manifest_sha256),
        );
        result.fit_receipt_sha256 = Some(fit_receipt_sha256);
        return Ok(result);
    }
    let registration_material = ports
        .build_candidate_registration(NativeMaintenanceCandidateRegistrationRequest {
            due,
            fit: &fit,
            inspection: &inspection,
            parity: parity.as_ref(),
            requested_at: now,
        })
        .await
        .map_err(Error::Port)?;
    // require exact custody identities on every optional shadow registration
    if registration_material.as_ref().is_some_and(|registration| {
        registration
            .shadow_registration
            .as_ref()
            .is_some_and(|shadow| {
                !valid_hash(&shadow.artifact_sha256) || !valid_hash(&shadow.registration_sha256)
            })
    }) {
        return Err(Error::InvalidInput);
    }
    let candidate = ports
        .persist_candidate(NativeMaintenanceCandidatePersistenceRequest {
            due,
            fit: &fit,
            fit_receipt_sha256: &fit_receipt_sha256,
            parity: parity.as_ref(),
            registration_material: registration_material.as_ref(),
        })
        .await
        .map_err(Error::Port)?;
    // bind the returned archive graph before retaining release material
    if !valid_hash(&candidate.candidate_graph_sha256) {
        return Err(Error::InvalidInput);
    }
    let mut confirmation_registration_sha256 = None;
    // occupy the local slot only after persistence and only for complete material
    if let Some(registration) = registration_material.as_ref() {
        let input_head_sha256 = ports.read_confirmation_head().await.map_err(Error::Port)?;
        if !valid_hash(&input_head_sha256) {
            return Err(Error::InvalidInput);
        }
        let registration_sha256 = ports
            .preregister_confirmation(&registration.confirmation, &input_head_sha256, now)
            .await
            .map_err(Error::Port)?;
        if !valid_hash(&registration_sha256) {
            return Err(Error::InvalidInput);
        }
        confirmation_registration_sha256 = Some(registration_sha256);
    }
    let candidate_graph_sha256 = candidate.candidate_graph_sha256.clone();
    let candidate_sha256 = fit.candidate_sha256.clone();
    let semantic_input_sha256 = inspection.input_manifest_sha256.clone();
    let development_candidate = NativeMaintenanceDevelopmentCandidateMaterial {
        candidate,
        confirmation_registration_sha256,
        fit,
        inspection,
        parity,
        registration_material,
    };
    Ok(NativeMaintenanceDueWorkOutcome {
        action_eligible: false,
        candidate_graph_sha256: Some(candidate_graph_sha256),
        candidate_sha256: Some(candidate_sha256),
        control_reference_material: None,
        development_candidate: Some(development_candidate),
        fit_receipt_sha256: Some(fit_receipt_sha256),
        reason: Reason::DevelopmentCandidateArchived,
        semantic_input_sha256: Some(semantic_input_sha256),
        serving_changed: false,
        state: State::Completed,
    })
}

/// Construct one value-free report outcome with all action material absent.
fn outcome(
    reason: NativeMaintenanceDueWorkReason,
    state: NativeMaintenanceDueWorkState,
    semantic_input_sha256: Option<String>,
) -> NativeMaintenanceDueWorkOutcome {
    NativeMaintenanceDueWorkOutcome {
        action_eligible: false,
        candidate_graph_sha256: None,
        candidate_sha256: None,
        control_reference_material: None,
        development_candidate: None,
        fit_receipt_sha256: None,
        reason,
        semantic_input_sha256,
        serving_changed: false,
        state,
    }
}

/// Borrow the inspection manifest hash when the source shape carries one.
fn inspection_manifest_hash(inspection: &NativeMaintenanceSemanticInspection) -> Option<&str> {
    // preserve the single manifest coordinate for each value-bearing shape
    match inspection {
        NativeMaintenanceSemanticInspection::Idle {
            input_manifest_sha256,
        }
        | NativeMaintenanceSemanticInspection::ControlReference {
            input_manifest_sha256,
            ..
        }
        | NativeMaintenanceSemanticInspection::DailyUnsupported {
            input_manifest_sha256,
            ..
        }
        | NativeMaintenanceSemanticInspection::DailyReady {
            input_manifest_sha256,
            ..
        }
        | NativeMaintenanceSemanticInspection::MonthlyNoCandidate {
            input_manifest_sha256,
        } => Some(input_manifest_sha256),
        NativeMaintenanceSemanticInspection::MonthlyReady(inspection) => {
            Some(&inspection.input_manifest_sha256)
        }
        NativeMaintenanceSemanticInspection::Blocked { .. } => None,
    }
}

/// Require only the closed structural guarantees consumed by this state machine.
fn validate_due_shape(due: &NativeMaintenanceDue) -> Result<(), ()> {
    // bind daily and monthly family presence before semantic dispatch
    if due.due_key.is_empty()
        || due.original_cutoff_at.is_empty()
        || due.scope.is_empty()
        || (due.mode == NativeMaintenanceDueMode::Daily) != due.family.is_none()
    {
        return Err(());
    }
    Ok(())
}

/// Derive the exact source receipt identity from the sanitized fitter result.
fn fit_receipt_sha256(value: &NativeMaintenanceFitResult) -> Option<String> {
    let document = JsonValue::Object(vec![
        string_field("candidateSha256", &value.candidate_sha256),
        string_field("codeSnapshotSha256", &value.code_snapshot_sha256),
        string_field("contractVersion", &value.contract_version),
        string_field("family", value.family.wire()),
        string_field("inputSnapshotSha256", &value.input_snapshot_sha256),
        string_field("runtimeReadinessSha256", &value.runtime_readiness_sha256),
    ]);
    let bytes = NativeWirePrimitives.encode_sorted_json(&document)?;
    Some(format_digest(NativeWirePrimitives.sha256(&bytes)))
}

/// Build one UTF-16 JSON string field used by the generated wire value.
fn string_field(name: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (
        name.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}

#[cfg(test)]
#[path = "native_maintenance_due_work_test.rs"]
mod tests;
