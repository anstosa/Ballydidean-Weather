//! Installed qualified target/compensation publication and fixed family apply ownership.

use std::fmt::{self, Display, Formatter};
use std::io;

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    MaintenanceCanonicalJson, NativeFamilyReleaseCurrent, NativeMaintenanceArchiveSegmentClock,
    NativeMaintenanceArchiveWallClock, NativeMaintenanceCaptureFamily,
    NativeMaintenanceDatabaseLedgerReader, NativeMaintenanceDevelopmentRemotePort,
    NativeMaintenanceFamilyReleaseApplyInput, NativeMaintenanceFamilyReleaseApplyRequest,
    NativeMaintenanceFamilyReleaseApplyState, NativeMaintenanceFamilyReleaseApplyStatus,
    NativeMaintenanceInstalledTerminalMaterial, NativeMaintenanceJournalStore,
    NativeMaintenanceModelPublisher, NativeMaintenanceOwnerTerminalClass,
    NativeMaintenanceQualifiedAction, NativeMaintenanceQualifiedActionOutcomeInput,
    NativeMaintenanceQualifiedBaselineFiles, NativeMaintenanceQualifiedBaselineGit,
    NativeMaintenanceQualifiedCompensationBaseline, NativeMaintenanceQualifiedCompensationKind,
    NativeMaintenanceQualifiedReleasePackageError,
    NativeMaintenanceQualifiedReleaseTransactionInput, NativeMaintenanceReleaseLease,
    NativeOutboundHttp, NativeWirePrimitives, adjustment_rolling_local_date_at,
    apply_native_maintenance_family_release, build_native_maintenance_family_release_apply_request,
    build_native_maintenance_qualified_release_compensation,
    build_native_maintenance_qualified_release_target,
    read_native_maintenance_qualified_compensation_baseline,
    read_native_maintenance_qualified_release_transaction,
    record_native_maintenance_qualified_action_outcome,
    record_native_maintenance_qualified_release_transaction,
    verify_native_maintenance_family_release_apply_status,
};

const STATUS_COMMAND: &str = "adjustment-family-release-status";
const STATUS_VERSION: &str = "adjustment-family-release-status/v1";
const ABSENT_STATUS_KEYS: &[&str] = &["actionSha256", "contractVersion", "state"];
const STATUS_KEYS: &[&str] = &[
    "actionSha256",
    "compensatingRelease",
    "compensationState",
    "contractVersion",
    "family",
    "fencingToken",
    "outcome",
    "state",
    "targetRelease",
];
const TRANSACTION_KEYS: &[&str] = &[
    "compensation",
    "compensationAction",
    "family",
    "finalizationProof",
    "registrationSha256",
    "releaseRequest",
    "target",
    "targetAction",
];
const RELEASE_KEYS: &[&str] = &["actionSha256", "commitSha", "releaseTag"];
const REQUEST_KEYS: &[&str] = &[
    "actionSha256",
    "compensatingRelease",
    "expectedCurrentRelease",
    "expectedSettingsSha256",
    "expectedSourceRelease",
    "family",
    "fencingToken",
    "reportSha256",
    "targetRelease",
];

/// Read the fixed root transaction status in addition to applying a closed family request.
pub trait NativeMaintenanceQualifiedReleaseRemote: NativeMaintenanceDevelopmentRemotePort {
    /// Fetch one canonical status projection by the immutable target action identity.
    fn read_family_release_status(&mut self, action_sha256: &str) -> io::Result<Vec<u8>>;
}

impl NativeMaintenanceQualifiedReleaseRemote for NativeMaintenanceDatabaseLedgerReader {
    /// Reuse the forced-command transport with the predecessor's sole status operand.
    fn read_family_release_status(&mut self, action_sha256: &str) -> io::Result<Vec<u8>> {
        self.request_document(STATUS_COMMAND, &[action_sha256], None)
    }
}

/// Source inputs retained before any immutable release or root-family mutation.
pub struct NativeMaintenanceInstalledQualifiedReleaseInput<'a> {
    pub action: &'a NativeMaintenanceQualifiedAction,
    pub material: &'a NativeMaintenanceInstalledTerminalMaterial,
    pub current: &'a NativeFamilyReleaseCurrent,
    pub lease: Option<&'a NativeMaintenanceReleaseLease>,
    pub finalization_proof: &'a JsonValue,
}

/// Immutable published coordinates retained in the qualified transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceInstalledQualifiedReleaseCoordinates {
    pub action_sha256: String,
    pub commit_sha: String,
    pub release_tag: String,
}

/// Complete paired publication and reconciled fixed-root result.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceInstalledQualifiedReleaseOutcome {
    pub transaction: JsonValue,
    pub target: NativeMaintenanceInstalledQualifiedReleaseCoordinates,
    pub compensation: NativeMaintenanceInstalledQualifiedReleaseCoordinates,
    pub compensation_kind: NativeMaintenanceQualifiedCompensationKind,
    pub status: NativeMaintenanceFamilyReleaseApplyStatus,
    pub terminal_class: NativeMaintenanceOwnerTerminalClass,
    pub serving_changed: bool,
}

/// Closed preparation, publication, recovery, transport, or journal refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceInstalledQualifiedReleaseError {
    Input,
    Clock,
    Baseline,
    Package,
    Publish,
    Transaction,
    Status,
    Apply,
    Journal,
}

impl Display for NativeMaintenanceInstalledQualifiedReleaseError {
    /// Render one stable path-free owner failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "installed qualified release input is invalid",
            Self::Clock => "installed qualified release clock is invalid",
            Self::Baseline => "installed qualified release baseline is invalid",
            Self::Package => "installed qualified release package is invalid",
            Self::Publish => "installed qualified release publication failed",
            Self::Transaction => "installed qualified release transaction is invalid",
            Self::Status => "installed qualified release status is invalid",
            Self::Apply => "installed qualified release apply failed",
            Self::Journal => "installed qualified release journal failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceInstalledQualifiedReleaseError {}

/// Publish and reconcile one installed qualified action using the authenticated production Git baseline.
pub fn apply_native_maintenance_installed_qualified_release(
    input: NativeMaintenanceInstalledQualifiedReleaseInput<'_>,
    journal: &mut NativeMaintenanceJournalStore,
    http: &NativeOutboundHttp,
    remote: &mut impl NativeMaintenanceQualifiedReleaseRemote,
) -> Result<
    NativeMaintenanceInstalledQualifiedReleaseOutcome,
    NativeMaintenanceInstalledQualifiedReleaseError,
> {
    let mut baseline = NativeMaintenanceQualifiedBaselineGit::native();
    apply_native_maintenance_installed_qualified_release_with_baseline(
        input,
        journal,
        http,
        remote,
        &mut baseline,
    )
}

/// Publish with an explicit authenticated source reader for disposable fixtures and production delegation.
pub fn apply_native_maintenance_installed_qualified_release_with_baseline(
    input: NativeMaintenanceInstalledQualifiedReleaseInput<'_>,
    journal: &mut NativeMaintenanceJournalStore,
    http: &NativeOutboundHttp,
    remote: &mut impl NativeMaintenanceQualifiedReleaseRemote,
    baseline_files: &mut impl NativeMaintenanceQualifiedBaselineFiles,
) -> Result<
    NativeMaintenanceInstalledQualifiedReleaseOutcome,
    NativeMaintenanceInstalledQualifiedReleaseError,
> {
    use NativeMaintenanceInstalledQualifiedReleaseError as Error;

    let admitted = admit_input(&input)?;
    // resume only from the exact transaction that precedes every root-family mutation
    if let Some(transaction) = read_native_maintenance_qualified_release_transaction(
        journal,
        &admitted.registration_sha256,
    )
    .map_err(|_| Error::Journal)?
    {
        return recover_transaction(input, admitted, transaction, journal, remote);
    }
    let lease = input.lease.ok_or(Error::Input)?;
    validate_lease(&input, admitted.family, lease)?;
    // authenticate and freeze the incumbent before publishing the target commit
    let baseline = read_native_maintenance_qualified_compensation_baseline(
        input.current,
        admitted.family,
        baseline_files,
    )
    .map_err(|_| Error::Baseline)?;
    let compensation_kind = baseline_kind(&baseline);
    let now = clock()?;
    let release_date = adjustment_rolling_local_date_at(&now)
        .map_err(|_| Error::Clock)?
        .replace('-', ".");
    let target_package =
        build_native_maintenance_qualified_release_target(&input.action.action, input.material)
            .map_err(map_package)?;
    let mut publisher = NativeMaintenanceModelPublisher { journal, http };
    let target = publisher
        .publish_packaged(target_package.as_publish_package(
            &release_date,
            &lease.due_key,
            &lease.run_id,
        ))
        .map_err(|_| Error::Publish)?;
    let compensation_package = build_native_maintenance_qualified_release_compensation(
        &target_package,
        &target.commit_sha,
        &target.release_tag,
        &baseline,
    )
    .map_err(map_package)?;
    let compensation = publisher
        .publish_packaged(compensation_package.as_publish_package(
            &release_date,
            &lease.due_key,
            &lease.run_id,
        ))
        .map_err(|_| Error::Publish)?;
    publisher
        .record_pair(&target_package.action, &target, &compensation)
        .map_err(|_| Error::Publish)?;

    let request = build_request(
        input.current,
        &input.action.action,
        &target.release_tag,
        &compensation.release_tag,
    )?;
    let transaction = record_native_maintenance_qualified_release_transaction(
        journal,
        NativeMaintenanceQualifiedReleaseTransactionInput {
            family: admitted.family,
            registration_sha256: &admitted.registration_sha256,
            target: &target,
            target_action: &target_package.action,
            compensation: &compensation,
            compensation_action: &compensation_package.action,
            finalization_proof: input.finalization_proof,
            request: &request,
            now: &clock()?,
        },
    )
    .map_err(|_| Error::Journal)?;
    let status =
        apply_native_maintenance_family_release(remote, &request).map_err(|_| Error::Apply)?;
    bind_terminal_status(&status, &request)?;
    finish_outcome(
        journal,
        admitted,
        transaction,
        coordinates(
            &target.action_sha256,
            &target.commit_sha,
            &target.release_tag,
        ),
        coordinates(
            &compensation.action_sha256,
            &compensation.commit_sha,
            &compensation.release_tag,
        ),
        compensation_kind,
        status,
    )
}

struct AdmittedInput {
    family: NativeMaintenanceCaptureFamily,
    registration_sha256: String,
}

/// Crossbind all mutable public input fields before publication or recovery.
fn admit_input(
    input: &NativeMaintenanceInstalledQualifiedReleaseInput<'_>,
) -> Result<AdmittedInput, NativeMaintenanceInstalledQualifiedReleaseError> {
    use NativeMaintenanceInstalledQualifiedReleaseError as Error;

    let package =
        build_native_maintenance_qualified_release_target(&input.action.action, input.material)
            .map_err(map_package)?;
    let action_bytes = NativeWirePrimitives
        .encode_sorted_json(&input.action.action)
        .ok_or(Error::Input)?;
    let action_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &action_bytes,
    ));
    let registration_sha256 = text(
        &input.material.access.confirmation_registration,
        "registrationSha256",
    )
    .ok_or(Error::Input)?;
    // retain the exact original release fence, current compare-and-swap, and finalized action
    if action_sha256 != input.action.action_sha256
        || package.action_sha256 != action_sha256
        || input.action.policy_report_sha256
            != text(&input.action.action, "policyReportSha256").ok_or(Error::Input)?
        || input.current.family != package.family.wire()
        || input.current.commit
            != text(&input.action.action, "expectedSourceCommit").ok_or(Error::Input)?
        || input.current.release
            != text(&input.action.action, "expectedSourceRelease").ok_or(Error::Input)?
        || input.current.settings_sha256
            != text(&input.action.action, "expectedSettingsSha256").ok_or(Error::Input)?
        || text(input.finalization_proof, "actionSha256").as_deref() != Some(&action_sha256)
        || !valid_hash(&registration_sha256)
    {
        return Err(Error::Input);
    }
    Ok(AdmittedInput {
        family: package.family,
        registration_sha256,
    })
}

/// Resume an exact persisted pair without rebuilding current source or republishing refs.
fn recover_transaction(
    input: NativeMaintenanceInstalledQualifiedReleaseInput<'_>,
    admitted: AdmittedInput,
    transaction: JsonValue,
    journal: &mut NativeMaintenanceJournalStore,
    remote: &mut impl NativeMaintenanceQualifiedReleaseRemote,
) -> Result<
    NativeMaintenanceInstalledQualifiedReleaseOutcome,
    NativeMaintenanceInstalledQualifiedReleaseError,
> {
    use NativeMaintenanceInstalledQualifiedReleaseError as Error;

    let recovered = decode_transaction(&input, &admitted, &transaction)?;
    let bytes = remote
        .read_family_release_status(&recovered.target.action_sha256)
        .map_err(|_| Error::Status)?;
    let state = admit_recovery_status(&recovered.request, &bytes)?;
    let status = match state {
        // finish only absent or source-admitted interrupted root work with the persisted request
        RecoveryStatus::Retry => {
            apply_native_maintenance_family_release(remote, &recovered.request)
                .map_err(|_| Error::Apply)?
        }
        RecoveryStatus::Terminal(status) => status,
    };
    bind_terminal_status(&status, &recovered.request)?;
    finish_outcome(
        journal,
        admitted,
        transaction,
        recovered.target,
        recovered.compensation,
        recovered.compensation_kind,
        status,
    )
}

struct RecoveredTransaction {
    target: NativeMaintenanceInstalledQualifiedReleaseCoordinates,
    compensation: NativeMaintenanceInstalledQualifiedReleaseCoordinates,
    compensation_kind: NativeMaintenanceQualifiedCompensationKind,
    request: NativeMaintenanceFamilyReleaseApplyRequest,
}

/// Decode and crossbind the complete durable transaction used by interrupted release recovery.
fn decode_transaction(
    input: &NativeMaintenanceInstalledQualifiedReleaseInput<'_>,
    admitted: &AdmittedInput,
    transaction: &JsonValue,
) -> Result<RecoveredTransaction, NativeMaintenanceInstalledQualifiedReleaseError> {
    use NativeMaintenanceInstalledQualifiedReleaseError as Error;

    if !exact_keys(transaction, TRANSACTION_KEYS)
        || text(transaction, "family").as_deref() != Some(admitted.family.wire())
        || text(transaction, "registrationSha256").as_deref() != Some(&admitted.registration_sha256)
        || NativeWirePrimitives
            .encode_sorted_json(field(transaction, "targetAction").ok_or(Error::Transaction)?)
            != NativeWirePrimitives.encode_sorted_json(&input.action.action)
        || NativeWirePrimitives
            .encode_sorted_json(field(transaction, "finalizationProof").ok_or(Error::Transaction)?)
            != NativeWirePrimitives.encode_sorted_json(input.finalization_proof)
    {
        return Err(Error::Transaction);
    }
    let target = decode_coordinates(field(transaction, "target").ok_or(Error::Transaction)?)?;
    let compensation =
        decode_coordinates(field(transaction, "compensation").ok_or(Error::Transaction)?)?;
    let compensation_action = field(transaction, "compensationAction").ok_or(Error::Transaction)?;
    let compensation_kind = match text(compensation_action, "actionKind").as_deref() {
        Some("compensate_raw") => NativeMaintenanceQualifiedCompensationKind::Raw,
        Some("compensate_incumbent") => NativeMaintenanceQualifiedCompensationKind::Incumbent,
        _ => return Err(Error::Transaction),
    };
    let compensation_action_bytes = NativeWirePrimitives
        .encode_sorted_json(compensation_action)
        .ok_or(Error::Transaction)?;
    // authenticate both transaction action identities before reusing their remote operands
    let target_fence = text(&input.action.action, "fencingToken").ok_or(Error::Transaction)?;
    // bind compensation to the published target and the original qualified authority
    if target.action_sha256 != input.action.action_sha256
        || compensation.action_sha256
            != format_digest(RevisionArchiveCanonicalCodec::sha256(
                &NativeWirePrimitives,
                &compensation_action_bytes,
            ))
        || text(compensation_action, "predecessorActionSha256").as_deref()
            != Some(&target.action_sha256)
        || text(compensation_action, "expectedSourceCommit").as_deref() != Some(&target.commit_sha)
        || text(compensation_action, "expectedSourceRelease").as_deref()
            != Some(&target.release_tag)
        || text(compensation_action, "family").as_deref() != Some(admitted.family.wire())
        || text(compensation_action, "fencingToken").as_deref() != Some(&target_fence)
        || text(compensation_action, "policyReportSha256").as_deref()
            != Some(&input.action.policy_report_sha256)
    {
        return Err(Error::Transaction);
    }
    let request_value = field(transaction, "releaseRequest").ok_or(Error::Transaction)?;
    if !exact_keys(request_value, REQUEST_KEYS) {
        return Err(Error::Transaction);
    }
    let request = build_native_maintenance_family_release_apply_request(
        NativeMaintenanceFamilyReleaseApplyInput {
            target_release: &text(request_value, "targetRelease").ok_or(Error::Transaction)?,
            compensating_release: &text(request_value, "compensatingRelease")
                .ok_or(Error::Transaction)?,
            expected_current_release: &text(request_value, "expectedCurrentRelease")
                .ok_or(Error::Transaction)?,
            expected_source_release: &text(request_value, "expectedSourceRelease")
                .ok_or(Error::Transaction)?,
            expected_settings_sha256: &text(request_value, "expectedSettingsSha256")
                .ok_or(Error::Transaction)?,
            family: &text(request_value, "family").ok_or(Error::Transaction)?,
            action_sha256: &text(request_value, "actionSha256").ok_or(Error::Transaction)?,
            report_sha256: &text(request_value, "reportSha256").ok_or(Error::Transaction)?,
            fencing_token: &text(request_value, "fencingToken").ok_or(Error::Transaction)?,
        },
    )
    .map_err(|_| Error::Transaction)?;
    let arguments = request.arguments();
    // require the persisted request to retain the current/action/published pair identities
    if arguments[0] != target.release_tag
        || arguments[1] != compensation.release_tag
        || arguments[2] != input.current.release
        || arguments[4] != input.current.settings_sha256
        || arguments[5] != admitted.family.wire()
        || arguments[6] != input.action.action_sha256
        || arguments[7] != input.action.policy_report_sha256
        || arguments[8] != text(&input.action.action, "fencingToken").ok_or(Error::Transaction)?
    {
        return Err(Error::Transaction);
    }
    Ok(RecoveredTransaction {
        target,
        compensation,
        compensation_kind,
        request,
    })
}

/// Decode one immutable release tuple without manufacturing omitted publisher metadata.
fn decode_coordinates(
    value: &JsonValue,
) -> Result<
    NativeMaintenanceInstalledQualifiedReleaseCoordinates,
    NativeMaintenanceInstalledQualifiedReleaseError,
> {
    use NativeMaintenanceInstalledQualifiedReleaseError as Error;

    let action_sha256 = text(value, "actionSha256").ok_or(Error::Transaction)?;
    let commit_sha = text(value, "commitSha").ok_or(Error::Transaction)?;
    let release_tag = text(value, "releaseTag").ok_or(Error::Transaction)?;
    // close all retained coordinates before they can reach status or action journals
    if !exact_keys(value, RELEASE_KEYS)
        || !valid_hash(&action_sha256)
        || !valid_commit(&commit_sha)
        || !valid_release(&release_tag)
    {
        return Err(Error::Transaction);
    }
    Ok(coordinates(&action_sha256, &commit_sha, &release_tag))
}

enum RecoveryStatus {
    Retry,
    Terminal(NativeMaintenanceFamilyReleaseApplyStatus),
}

/// Admit exact absent/in-flight/terminal status and select only source retryable states.
fn admit_recovery_status(
    request: &NativeMaintenanceFamilyReleaseApplyRequest,
    bytes: &[u8],
) -> Result<RecoveryStatus, NativeMaintenanceInstalledQualifiedReleaseError> {
    use NativeMaintenanceInstalledQualifiedReleaseError as Error;

    let value = MaintenanceCanonicalJson::parse_canonical_object(&mut NativeWirePrimitives, bytes)
        .ok_or(Error::Status)?;
    if NativeWirePrimitives.encode_sorted_json(&value).as_deref() != Some(bytes) {
        return Err(Error::Status);
    }
    let state = text(&value, "state").ok_or(Error::Status)?;
    // preserve genuine absence without guessing any release coordinates
    if state == "absent" {
        return (exact_keys(&value, ABSENT_STATUS_KEYS)
            && text(&value, "contractVersion").as_deref() == Some(STATUS_VERSION)
            && text(&value, "actionSha256").as_deref() == Some(request.action_sha256()))
        .then_some(RecoveryStatus::Retry)
        .ok_or(Error::Status);
    }
    if !exact_keys(&value, STATUS_KEYS) || !status_matches_request(&value, request) {
        return Err(Error::Status);
    }
    // retry source in-flight work and an unfinished failed compensation only
    if matches!(state.as_str(), "prepared" | "applying") {
        return (text(&value, "compensationState").as_deref() == Some("absent")
            && matches!(field(&value, "outcome"), Some(JsonValue::Null)))
        .then_some(RecoveryStatus::Retry)
        .ok_or(Error::Status);
    }
    let status = verify_native_maintenance_family_release_apply_status(request, bytes)
        .map_err(|_| Error::Status)?;
    if status.state == NativeMaintenanceFamilyReleaseApplyState::CompensationRequired
        && !matches!(
            status.compensation_state,
            crate::NativeMaintenanceFamilyReleaseCompensationState::Verified
        )
    {
        return Ok(RecoveryStatus::Retry);
    }
    Ok(RecoveryStatus::Terminal(status))
}

/// Construct the source's fixed nine-operand compare-and-swap request.
fn build_request(
    current: &NativeFamilyReleaseCurrent,
    action: &JsonValue,
    target_release: &str,
    compensation_release: &str,
) -> Result<
    NativeMaintenanceFamilyReleaseApplyRequest,
    NativeMaintenanceInstalledQualifiedReleaseError,
> {
    use NativeMaintenanceInstalledQualifiedReleaseError as Error;

    build_native_maintenance_family_release_apply_request(
        NativeMaintenanceFamilyReleaseApplyInput {
            target_release,
            compensating_release: compensation_release,
            expected_current_release: &current.release,
            expected_source_release: &text(action, "expectedSourceRelease").ok_or(Error::Input)?,
            expected_settings_sha256: &text(action, "expectedSettingsSha256")
                .ok_or(Error::Input)?,
            family: &text(action, "family").ok_or(Error::Input)?,
            action_sha256: &format_digest(RevisionArchiveCanonicalCodec::sha256(
                &NativeWirePrimitives,
                &NativeWirePrimitives
                    .encode_sorted_json(action)
                    .ok_or(Error::Input)?,
            )),
            report_sha256: &text(action, "policyReportSha256").ok_or(Error::Input)?,
            fencing_token: &text(action, "fencingToken").ok_or(Error::Input)?,
        },
    )
    .map_err(|_| Error::Input)
}

/// Crossbind a terminal response to all persisted request coordinates.
fn bind_terminal_status(
    status: &NativeMaintenanceFamilyReleaseApplyStatus,
    request: &NativeMaintenanceFamilyReleaseApplyRequest,
) -> Result<(), NativeMaintenanceInstalledQualifiedReleaseError> {
    let arguments = request.arguments();
    // the shared response decoder binds action only, so retain the source transaction crossbinding here
    if status.action_sha256 != arguments[6]
        || status.target_release != arguments[0]
        || status.compensating_release != arguments[1]
        || status.family.as_deref() != Some(arguments[5])
        || status.fencing_token != arguments[8]
    {
        return Err(NativeMaintenanceInstalledQualifiedReleaseError::Status);
    }
    Ok(())
}

/// Validate one decoded non-absent root status against the persisted request.
fn status_matches_request(
    value: &JsonValue,
    request: &NativeMaintenanceFamilyReleaseApplyRequest,
) -> bool {
    let arguments = request.arguments();
    text(value, "contractVersion").as_deref() == Some(STATUS_VERSION)
        && text(value, "actionSha256").as_deref() == Some(arguments[6])
        && text(value, "targetRelease").as_deref() == Some(arguments[0])
        && text(value, "compensatingRelease").as_deref() == Some(arguments[1])
        && text(value, "family").as_deref() == Some(arguments[5])
        && text(value, "fencingToken").as_deref() == Some(arguments[8])
}

/// Persist the categorical local action result and project the owner terminal class.
fn finish_outcome(
    journal: &mut NativeMaintenanceJournalStore,
    admitted: AdmittedInput,
    transaction: JsonValue,
    target: NativeMaintenanceInstalledQualifiedReleaseCoordinates,
    compensation: NativeMaintenanceInstalledQualifiedReleaseCoordinates,
    compensation_kind: NativeMaintenanceQualifiedCompensationKind,
    status: NativeMaintenanceFamilyReleaseApplyStatus,
) -> Result<
    NativeMaintenanceInstalledQualifiedReleaseOutcome,
    NativeMaintenanceInstalledQualifiedReleaseError,
> {
    use NativeMaintenanceFamilyReleaseApplyState as State;
    use NativeMaintenanceInstalledQualifiedReleaseError as Error;

    let policy_report_sha256 = field(&transaction, "targetAction")
        .and_then(|value| text(value, "policyReportSha256"))
        .ok_or(Error::Transaction)?;
    let serving_changed = record_native_maintenance_qualified_action_outcome(
        journal,
        NativeMaintenanceQualifiedActionOutcomeInput {
            family: admitted.family,
            registration_sha256: &admitted.registration_sha256,
            action_sha256: &target.action_sha256,
            compensation_action_sha256: &compensation.action_sha256,
            policy_report_sha256: &policy_report_sha256,
            fencing_token: &status.fencing_token,
            status: &status,
            compensation_kind,
            now: &clock()?,
        },
    )
    .map_err(|_| Error::Journal)?;
    let terminal_class = match status.state {
        State::Acknowledged => {
            NativeMaintenanceOwnerTerminalClass::PromotedVerified { serving_changed }
        }
        State::OperatorOffUnapplied => {
            NativeMaintenanceOwnerTerminalClass::PromotedOperatorOffUnapplied
        }
        State::CompensationRequired => {
            NativeMaintenanceOwnerTerminalClass::PromotionFailedCompensated
        }
    };
    Ok(NativeMaintenanceInstalledQualifiedReleaseOutcome {
        transaction,
        target,
        compensation,
        compensation_kind,
        status,
        terminal_class,
        serving_changed,
    })
}

/// Require the original matching live release fence before any first publication.
fn validate_lease(
    input: &NativeMaintenanceInstalledQualifiedReleaseInput<'_>,
    family: NativeMaintenanceCaptureFamily,
    lease: &NativeMaintenanceReleaseLease,
) -> Result<(), NativeMaintenanceInstalledQualifiedReleaseError> {
    let candidate = text(&input.action.action, "candidateSha256")
        .ok_or(NativeMaintenanceInstalledQualifiedReleaseError::Input)?;
    let fence = text(&input.action.action, "fencingToken")
        .ok_or(NativeMaintenanceInstalledQualifiedReleaseError::Input)?;
    // publication may proceed only under the original still-retained release coordinates
    if lease.fencing_token != fence
        || lease.due_key != format!("confirmation/{}/{candidate}", family.wire())
        || lease.run_id != format!("release-promote-{}", &candidate[..32])
    {
        return Err(NativeMaintenanceInstalledQualifiedReleaseError::Input);
    }
    Ok(())
}

/// Project the retained baseline into the source's closed compensation category.
fn baseline_kind(
    baseline: &NativeMaintenanceQualifiedCompensationBaseline,
) -> NativeMaintenanceQualifiedCompensationKind {
    match baseline {
        NativeMaintenanceQualifiedCompensationBaseline::Raw => {
            NativeMaintenanceQualifiedCompensationKind::Raw
        }
        NativeMaintenanceQualifiedCompensationBaseline::Incumbent { .. } => {
            NativeMaintenanceQualifiedCompensationKind::Incumbent
        }
    }
}

/// Freeze one actual or recovered immutable publication tuple.
fn coordinates(
    action_sha256: &str,
    commit_sha: &str,
    release_tag: &str,
) -> NativeMaintenanceInstalledQualifiedReleaseCoordinates {
    NativeMaintenanceInstalledQualifiedReleaseCoordinates {
        action_sha256: action_sha256.to_owned(),
        commit_sha: commit_sha.to_owned(),
        release_tag: release_tag.to_owned(),
    }
}

/// Sample one canonical wall-clock instant at a journal boundary.
fn clock() -> Result<String, NativeMaintenanceInstalledQualifiedReleaseError> {
    NativeMaintenanceArchiveWallClock
        .now()
        .ok_or(NativeMaintenanceInstalledQualifiedReleaseError::Clock)
}

/// Preserve the package builder's input-vs-material failure boundary.
fn map_package(
    error: NativeMaintenanceQualifiedReleasePackageError,
) -> NativeMaintenanceInstalledQualifiedReleaseError {
    match error {
        NativeMaintenanceQualifiedReleasePackageError::Input
        | NativeMaintenanceQualifiedReleasePackageError::Action => {
            NativeMaintenanceInstalledQualifiedReleaseError::Input
        }
        NativeMaintenanceQualifiedReleasePackageError::Baseline => {
            NativeMaintenanceInstalledQualifiedReleaseError::Baseline
        }
        NativeMaintenanceQualifiedReleasePackageError::Candidate
        | NativeMaintenanceQualifiedReleasePackageError::Package
        | NativeMaintenanceQualifiedReleasePackageError::Codec
        | NativeMaintenanceQualifiedReleasePackageError::Git => {
            NativeMaintenanceInstalledQualifiedReleaseError::Package
        }
    }
}

/// Recognize one exact lowercase Git object identity.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Recognize one date-shaped positive immutable release tag.
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 12
        && bytes.get(4) == Some(&b'.')
        && bytes.get(7) == Some(&b'.')
        && bytes.get(10) == Some(&b'-')
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[11..].iter().all(u8::is_ascii_digit)
        && bytes[11] != b'0'
}
