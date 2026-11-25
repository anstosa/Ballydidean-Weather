//! Installed finalized-anchor authority for qualified terminal retirement.

use std::fmt::{self, Display, Formatter};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::capture_epoch_operator::{
    CaptureEpochOperatorFile, CaptureEpochOperatorFiles, NativeCaptureEpochOperatorFiles,
};
use crate::capture_epoch_witness::verify_operator_capture_epoch_witness;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    FamilyCatalogImageVerification, MaintenanceCanonicalJson, NativeFamilyCatalogImageVerifier,
    NativeFamilyLineageSourceClosure, NativeFamilyReleaseCurrent, NativeFamilyReleaseCurrentError,
    NativeFamilyReleaseCurrentFiles, NativeFamilyReleaseCurrentHostFiles,
    NativeFamilyReleaseLineageTransaction, NativeFamilyReleaseLiveRuntime,
    NativeFamilyReleaseLiveRuntimeProcess, NativeFamilyReleaseStatusFiles,
    NativeFamilyReleaseStatusHostFiles, NativeFutureInputSealOperatorCaptureEpoch,
    NativeFutureInputSealStore, NativeMaintenanceAnchorCurrentV3Control,
    NativeMaintenanceAnchorStatusFiles, NativeMaintenanceOwnerTerminalClass,
    NativeMaintenanceOwnerTerminalInput, NativeWirePrimitives,
    PreparedNativeFamilyLineageLiveObservation, build_native_maintenance_owner_terminal_material,
    project_native_family_release_status, read_future_input_seal_current_verified,
    read_maintenance_anchor_current_v3_command, read_native_family_release_current,
    read_native_future_input_seal_process_control, system_worker_health_milliseconds,
    validate_native_family_release_transaction, validate_native_maintenance_archive_registration,
};

const MAXIMUM_REQUEST_BYTES: usize = 128 * 1024;
const REQUEST_VERSION: &str = "adjustment-shadow-terminal-retirement-request/v3";
const PROOF_VERSION: &str = "adjustment-maintenance-finalization-proof/v3";

/// Bounded qualified-retirement failures without retained host values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHostOwnerQualifiedTerminalError {
    Request,
    CaptureEpoch,
    Family,
    Catalog,
    Anchor,
    Authority,
    Sql,
}

impl Display for NativeHostOwnerQualifiedTerminalError {
    /// Render one stable root-operation failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Request => "native owner qualified retirement request is invalid",
            Self::CaptureEpoch => "native owner capture epoch authority is unavailable",
            Self::Family => "native owner family authority is unavailable",
            Self::Catalog => "native owner installed catalog is unavailable",
            Self::Anchor => "native owner finalized anchor is unavailable",
            Self::Authority => "native owner qualified retirement authority differs",
            Self::Sql => "native owner qualified retirement SQL projection failed",
        })
    }
}

impl std::error::Error for NativeHostOwnerQualifiedTerminalError {}

/// Exact canonical qualified retirement identity and its fixed SQL transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeHostOwnerQualifiedTerminalSql {
    pub request_sha256: String,
    pub sql: String,
}

/// Exact validated family transaction, categorical status and completion clock.
pub(crate) struct NativeHostOwnerQualifiedTransaction {
    pub document: JsonValue,
    pub status: JsonValue,
    pub completed_at: String,
    pub operator_off_unapplied: bool,
}

/// Read one action-addressed transaction through its exact root-owned validator.
pub(crate) trait NativeHostOwnerQualifiedTransactionPort {
    /// Return one terminal transaction only after complete lineage and status validation.
    fn read_terminal_transaction(
        &mut self,
        action_sha256: &str,
        time: &dyn CurrentTimeResolver,
    ) -> Result<NativeHostOwnerQualifiedTransaction, NativeHostOwnerQualifiedTerminalError>;
}

/// Fixed root-owned family transaction reader.
struct NativeHostOwnerInstalledQualifiedTransaction {
    files: NativeFamilyReleaseStatusHostFiles,
}

impl NativeHostOwnerInstalledQualifiedTransaction {
    /// Bind the exact predecessor family-release transaction root.
    fn native() -> Self {
        Self {
            files: NativeFamilyReleaseStatusHostFiles::native(),
        }
    }
}

impl NativeHostOwnerQualifiedTransactionPort for NativeHostOwnerInstalledQualifiedTransaction {
    /// Validate the same bytes as both one family status and one complete lineage transaction.
    fn read_terminal_transaction(
        &mut self,
        action_sha256: &str,
        time: &dyn CurrentTimeResolver,
    ) -> Result<NativeHostOwnerQualifiedTransaction, NativeHostOwnerQualifiedTerminalError> {
        let bytes = self
            .files
            .read_transaction(action_sha256)
            .map_err(|_| NativeHostOwnerQualifiedTerminalError::Authority)?
            .ok_or(NativeHostOwnerQualifiedTerminalError::Authority)?;
        let mut codec = NativeWirePrimitives;
        let document = codec
            .parse_canonical_object(&bytes)
            .ok_or(NativeHostOwnerQualifiedTerminalError::Authority)?;
        let status =
            project_native_family_release_status(action_sha256, Some(&bytes), &mut codec, time)
                .map_err(|_| NativeHostOwnerQualifiedTerminalError::Authority)?;
        let validated = validate_native_family_release_transaction(
            &NativeFamilyReleaseLineageTransaction {
                action_sha256: action_sha256.to_owned(),
                document: document.clone(),
            },
            &mut codec,
            time,
        )
        .map_err(|_| NativeHostOwnerQualifiedTerminalError::Authority)?;
        Ok(NativeHostOwnerQualifiedTransaction {
            document,
            status,
            completed_at: validated.completed_at,
            operator_off_unapplied: validated.operator_off_unapplied,
        })
    }
}

/// Read one exact verified finalized anchor through a closed installed owner.
pub(crate) trait NativeHostOwnerQualifiedAnchorPort {
    /// Return the current anchor object after seal, lineage, control and slot verification.
    fn read_current_anchor(&mut self) -> Result<JsonValue, NativeHostOwnerQualifiedTerminalError>;
}

/// Production current-anchor reader retaining all existing installed source closures.
struct NativeHostOwnerInstalledQualifiedAnchor {
    uid: u32,
    gid: u32,
}

impl NativeHostOwnerInstalledQualifiedAnchor {
    /// Bind the predecessor store to the effective process owner.
    fn native() -> Self {
        // libc returns the kernel-authenticated effective identities without filesystem input
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        Self { uid, gid }
    }
}

impl NativeHostOwnerQualifiedAnchorPort for NativeHostOwnerInstalledQualifiedAnchor {
    /// Reuse the complete verified current-v3 owner rather than reading raw anchor bytes.
    fn read_current_anchor(&mut self) -> Result<JsonValue, NativeHostOwnerQualifiedTerminalError> {
        let control = read_native_future_input_seal_process_control()
            .map_err(|_| NativeHostOwnerQualifiedTerminalError::Anchor)?;
        let mut seals = NativeFutureInputSealStore::native(self.uid, self.gid);
        let mut capture = NativeFutureInputSealOperatorCaptureEpoch::native();
        let mut family_files = NativeFamilyReleaseCurrentHostFiles::native(0, 0);
        let mut runtime = NativeFamilyReleaseLiveRuntimeProcess::native();
        let mut verifier = NativeFamilyCatalogImageVerifier::new();
        let mut family_codec = NativeWirePrimitives;
        let live = PreparedNativeFamilyLineageLiveObservation {
            files: &mut family_files,
            runtime: &mut runtime,
            verifier: &mut verifier,
            codec: &mut family_codec,
            time: &NativeWirePrimitives,
        };
        let mut closure = NativeFamilyLineageSourceClosure::native(live);
        let mut anchors = NativeMaintenanceAnchorStatusFiles::native(self.uid, self.gid);
        let mut codec = NativeWirePrimitives;
        let bytes = read_maintenance_anchor_current_v3_command(
            "maintenance-anchor-current-v3",
            &[],
            &mut anchors,
            &mut codec,
            &NativeWirePrimitives,
            NativeMaintenanceAnchorCurrentV3Control {
                version: control.version.as_deref(),
                sha256: control.sha256.as_deref(),
            },
            |codec, time| {
                read_future_input_seal_current_verified(
                    &mut seals,
                    &mut capture,
                    control.borrowed(),
                    &mut closure,
                    codec,
                    time,
                )
            },
        )
        .map_err(|_| NativeHostOwnerQualifiedTerminalError::Anchor)?
        .ok_or(NativeHostOwnerQualifiedTerminalError::Anchor)?;
        let envelope = codec
            .parse_canonical_object(&bytes)
            .ok_or(NativeHostOwnerQualifiedTerminalError::Anchor)?;
        field(&envelope, "anchor")
            .cloned()
            .ok_or(NativeHostOwnerQualifiedTerminalError::Anchor)
    }
}

/// Independently supplied installed owners for a deterministic retirement composition.
pub(crate) struct NativeHostOwnerQualifiedTerminalOwners<
    'a,
    Capture,
    Family,
    Runtime,
    Verifier,
    Anchor,
> {
    pub capture: &'a mut Capture,
    pub family: &'a mut Family,
    pub runtime: &'a mut Runtime,
    pub verifier: &'a mut Verifier,
    pub anchor: &'a mut Anchor,
}

/// Independently supplied installed owners for qualified terminal record creation.
pub(crate) struct NativeHostOwnerQualifiedRecordOwners<
    'a,
    Capture,
    Family,
    Runtime,
    Verifier,
    Anchor,
    Transaction,
> {
    pub capture: &'a mut Capture,
    pub family: &'a mut Family,
    pub runtime: &'a mut Runtime,
    pub verifier: &'a mut Verifier,
    pub anchor: &'a mut Anchor,
    pub transaction: &'a mut Transaction,
    pub time: &'a dyn CurrentTimeResolver,
    pub now: fn() -> Option<i64>,
}

/// Build exact qualified terminal-record SQL from independently verified installed state.
pub fn build_native_host_owner_qualified_terminal_sql_from_installed(
    request_bytes: &[u8],
    expected_sha256: &str,
) -> Result<NativeHostOwnerQualifiedTerminalSql, NativeHostOwnerQualifiedTerminalError> {
    let mut capture = NativeCaptureEpochOperatorFiles;
    let mut family = NativeFamilyReleaseCurrentHostFiles::native(0, 0);
    let mut runtime = NativeFamilyReleaseLiveRuntimeProcess::native();
    let mut verifier = NativeFamilyCatalogImageVerifier::new();
    let mut anchor = NativeHostOwnerInstalledQualifiedAnchor::native();
    let mut transaction = NativeHostOwnerInstalledQualifiedTransaction::native();
    build_native_host_owner_qualified_terminal_sql_from_owners(
        request_bytes,
        expected_sha256,
        NativeHostOwnerQualifiedRecordOwners {
            capture: &mut capture,
            family: &mut family,
            runtime: &mut runtime,
            verifier: &mut verifier,
            anchor: &mut anchor,
            transaction: &mut transaction,
            time: &NativeWirePrimitives,
            now: system_worker_health_milliseconds,
        },
    )
}

/// Build exact qualified retirement SQL from only installed verified authorities.
pub fn build_native_host_owner_qualified_retirement_sql_from_installed(
    request_bytes: &[u8],
    expected_sha256: &str,
) -> Result<NativeHostOwnerQualifiedTerminalSql, NativeHostOwnerQualifiedTerminalError> {
    let mut capture = NativeCaptureEpochOperatorFiles;
    let mut family = NativeFamilyReleaseCurrentHostFiles::native(0, 0);
    let mut runtime = NativeFamilyReleaseLiveRuntimeProcess::native();
    let mut verifier = NativeFamilyCatalogImageVerifier::new();
    let mut anchor = NativeHostOwnerInstalledQualifiedAnchor::native();
    build_native_host_owner_qualified_retirement_sql_from_owners(
        request_bytes,
        expected_sha256,
        NativeHostOwnerQualifiedTerminalOwners {
            capture: &mut capture,
            family: &mut family,
            runtime: &mut runtime,
            verifier: &mut verifier,
            anchor: &mut anchor,
        },
    )
}

/// Compose qualified retirement after validating request and independent installed state.
pub(crate) fn build_native_host_owner_qualified_retirement_sql_from_owners<
    Capture,
    Family,
    Runtime,
    Verifier,
    Anchor,
>(
    request_bytes: &[u8],
    expected_sha256: &str,
    owners: NativeHostOwnerQualifiedTerminalOwners<'_, Capture, Family, Runtime, Verifier, Anchor>,
) -> Result<NativeHostOwnerQualifiedTerminalSql, NativeHostOwnerQualifiedTerminalError>
where
    Capture: CaptureEpochOperatorFiles,
    Family: NativeFamilyReleaseCurrentFiles,
    Runtime: NativeFamilyReleaseLiveRuntime,
    Verifier: FamilyCatalogImageVerification,
    Anchor: NativeHostOwnerQualifiedAnchorPort,
{
    let request = admit_retirement_request(request_bytes, expected_sha256)?;
    let record =
        field(&request, "terminalRecord").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let family_name =
        text(record, "family").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let witness_bytes = owners
        .capture
        .read(CaptureEpochOperatorFile::Witness)
        .map_err(|_| NativeHostOwnerQualifiedTerminalError::CaptureEpoch)?;
    let mut family_files = CatalogCapturingFiles::new(owners.family);
    let mut codec = NativeWirePrimitives;
    let _current = read_native_family_release_current(
        &family_name,
        &mut family_files,
        owners.runtime,
        owners.verifier,
        &mut codec,
        &NativeWirePrimitives,
    )
    .map_err(map_family_error)?;
    // preserve the source's independent installed catalog observation even though retirement uses no slot
    codec
        .parse_canonical_object(
            family_files
                .catalog
                .as_deref()
                .ok_or(NativeHostOwnerQualifiedTerminalError::Catalog)?,
        )
        .ok_or(NativeHostOwnerQualifiedTerminalError::Catalog)?;
    let anchor = owners.anchor.read_current_anchor()?;
    verify_qualified_retirement_authority(&request, &witness_bytes, &anchor)?;
    let sql = qualified_retirement_sql(&request, expected_sha256)
        .ok_or(NativeHostOwnerQualifiedTerminalError::Sql)?;
    Ok(NativeHostOwnerQualifiedTerminalSql {
        request_sha256: expected_sha256.to_owned(),
        sql,
    })
}

/// Compose qualified terminal creation after request, anchor and transaction verification.
pub(crate) fn build_native_host_owner_qualified_terminal_sql_from_owners<
    Capture,
    Family,
    Runtime,
    Verifier,
    Anchor,
    Transaction,
>(
    request_bytes: &[u8],
    expected_sha256: &str,
    owners: NativeHostOwnerQualifiedRecordOwners<
        '_,
        Capture,
        Family,
        Runtime,
        Verifier,
        Anchor,
        Transaction,
    >,
) -> Result<NativeHostOwnerQualifiedTerminalSql, NativeHostOwnerQualifiedTerminalError>
where
    Capture: CaptureEpochOperatorFiles,
    Family: NativeFamilyReleaseCurrentFiles,
    Runtime: NativeFamilyReleaseLiveRuntime,
    Verifier: FamilyCatalogImageVerification,
    Anchor: NativeHostOwnerQualifiedAnchorPort,
    Transaction: NativeHostOwnerQualifiedTransactionPort,
{
    let request = admit_terminal_request(request_bytes, expected_sha256)?;
    let shadow = field(&request, "shadowRegistration")
        .ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let family_name =
        text(shadow, "family").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let witness_bytes = owners
        .capture
        .read(CaptureEpochOperatorFile::Witness)
        .map_err(|_| NativeHostOwnerQualifiedTerminalError::CaptureEpoch)?;
    let mut family_files = CatalogCapturingFiles::new(owners.family);
    let mut codec = NativeWirePrimitives;
    let current = read_native_family_release_current(
        &family_name,
        &mut family_files,
        owners.runtime,
        owners.verifier,
        &mut codec,
        owners.time,
    )
    .map_err(map_family_error)?;
    codec
        .parse_canonical_object(
            family_files
                .catalog
                .as_deref()
                .ok_or(NativeHostOwnerQualifiedTerminalError::Catalog)?,
        )
        .ok_or(NativeHostOwnerQualifiedTerminalError::Catalog)?;
    let anchor = owners.anchor.read_current_anchor()?;
    let now = (owners.now)().ok_or(NativeHostOwnerQualifiedTerminalError::Authority)?;
    let completed_at = verify_qualified_terminal_authority(
        &request,
        &witness_bytes,
        &current,
        &anchor,
        owners.transaction,
        now,
        owners.time,
    )?;
    let sql = qualified_terminal_sql(&request, expected_sha256, &completed_at)
        .ok_or(NativeHostOwnerQualifiedTerminalError::Sql)?;
    Ok(NativeHostOwnerQualifiedTerminalSql {
        request_sha256: expected_sha256.to_owned(),
        sql,
    })
}

/// Canonically admit the full qualified terminal workstation request.
fn admit_terminal_request(
    bytes: &[u8],
    expected_sha256: &str,
) -> Result<JsonValue, NativeHostOwnerQualifiedTerminalError> {
    let mut wire = NativeWirePrimitives;
    // reject malformed bytes and a substituted forced operand before host observation
    if !(2..=MAXIMUM_REQUEST_BYTES).contains(&bytes.len()) || !valid_hash(expected_sha256) {
        return Err(NativeHostOwnerQualifiedTerminalError::Request);
    }
    let value = wire
        .parse_canonical_object(bytes)
        .ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    if digest(bytes) != expected_sha256 || validate_qualified_terminal_request(&value).is_none() {
        return Err(NativeHostOwnerQualifiedTerminalError::Request);
    }
    Ok(value)
}

/// Validate all workstation domains before any installed authority can be consulted.
fn validate_qualified_terminal_request(value: &JsonValue) -> Option<()> {
    exact_keys(
        value,
        &[
            "action",
            "actionReceipt",
            "confirmationRegistration",
            "contractVersion",
            "finalizationProof",
            "localResult",
            "nativeAccess",
            "previousTombstone",
            "shadowRegistration",
            "sourceCommit",
            "terminalGraphManifestSha256",
            "terminalGraphVerifiedAt",
        ],
    )
    .then_some(())?;
    (text(value, "contractVersion")? == "adjustment-shadow-terminal-record-request/v3")
        .then_some(())?;
    let registration = field(value, "confirmationRegistration")?;
    crate::native_host_owner_terminal::validate_confirmation_registration(registration)?;
    let shadow = field(value, "shadowRegistration")?;
    let shadow_bytes = NativeWirePrimitives.encode_sorted_json(shadow)?;
    validate_native_maintenance_archive_registration(
        "adjustment-shadow-registration/v3",
        &shadow_bytes,
    )?;
    let result = field(value, "localResult")?;
    crate::native_host_owner_terminal::validate_local_result(result)?;
    let native = field(value, "nativeAccess")?;
    crate::native_host_owner_terminal::validate_native_access(native)?;
    let receipt = field(value, "actionReceipt")?;
    validate_terminal_receipt(receipt)?;
    let proof = field(value, "finalizationProof")?;
    crate::native_maintenance_due_terminal_remote::validate_finalization_proof(proof)?;
    crate::native_host_owner_terminal::validate_tombstone(
        field(value, "previousTombstone")?,
        true,
    )?;
    valid_hash(&text(value, "terminalGraphManifestSha256")?).then_some(())?;
    instant(value, "terminalGraphVerifiedAt")?;
    valid_commit(&text(value, "sourceCommit")?).then_some(())?;
    for key in [
        "candidateKind",
        "candidateSha256",
        "cohortLineageSha256",
        "contractVersion",
        "family",
        "firstTargetAt",
        "gateManifestSha256",
        "inputHeadSha256",
        "intervalEndExclusiveLocalDate",
        "intervalStartLocalDate",
        "registrationSha256",
        "reservedKeySha256",
        "sourceLineageSha256",
        "terminalAccessAt",
    ] {
        (field(result, key)? == field(registration, key)?).then_some(())?;
    }
    let receipt_completed =
        if text(receipt, "contractVersion")? == "adjustment-terminal-no-action-receipt/v3" {
            text(receipt, "completedAt")?
        } else {
            text(proof, "finalizedAt")?
        };
    (field(native, "registrationSha256") == field(shadow, "registrationSha256")
        && field(registration, "family") == field(shadow, "family")
        && field(result, "family") == field(shadow, "family")
        && field(registration, "candidateSha256") == field(shadow, "candidateSha256")
        && field(result, "candidateSha256") == field(shadow, "candidateSha256")
        && field(registration, "reservedKeySha256") == field(shadow, "reservedKeySha256")
        && field(result, "reservedKeySha256") == field(shadow, "reservedKeySha256")
        && field(native, "expectedKeySetSha256") == field(shadow, "reservedKeySha256")
        && field(native, "gateManifestSha256") == field(registration, "gateManifestSha256")
        && field(native, "targetCutoffAt") == field(shadow, "targetCutoffAt")
        && field(proof, "family") == field(shadow, "family")
        && field(proof, "sourceCommit") == field(value, "sourceCommit")
        && field(proof, "confirmationAccessSha256") == field(native, "accessSha256")
        && field(proof, "fullMemberRootSha256") == field(result, "fullMemberRootSha256")
        && field(proof, "policyReportSha256") == field(result, "candidateReportSha256")
        && field(receipt, "actionSha256") == field(proof, "actionSha256")
        && instant(native, "accessedAt")? >= instant(shadow, "terminalAt")?
        && instant(proof, "fullGraphVerifiedAt")? >= instant(native, "accessedAt")?
        && instant_text(&receipt_completed)? >= instant(proof, "finalizedAt")?
        && instant(value, "terminalGraphVerifiedAt")? >= instant_text(&receipt_completed)?)
    .then_some(())?;
    let promoted = text(result, "disposition")? == "promoted";
    // promoted results carry the immutable action; every no-action result carries none
    if promoted {
        let action = field(value, "action")?;
        (validate_model_action(action, result).is_some()
            && text(receipt, "contractVersion")? == "adjustment-family-release-status/v1"
            && field(receipt, "family") == field(action, "family")
            && field(receipt, "fencingToken") == field(action, "fencingToken")
            && canonical_digest(action)? == text(receipt, "actionSha256")?
            && field(action, "fullMemberRootSha256") == field(result, "fullMemberRootSha256")
            && field(action, "policyReportSha256") == field(result, "candidateReportSha256"))
        .then_some(())
    } else {
        (matches!(field(value, "action"), Some(JsonValue::Null))
            && text(receipt, "contractVersion")? == "adjustment-terminal-no-action-receipt/v3"
            && field(receipt, "registrationSha256") == field(shadow, "registrationSha256")
            && field(receipt, "disposition") == field(result, "disposition")
            && field(receipt, "fullMemberRootSha256") == field(result, "fullMemberRootSha256")
            && field(receipt, "policyReportSha256") == field(result, "candidateReportSha256"))
        .then_some(())
    }
}

/// Validate either a root family status or one exact no-action receipt.
fn validate_terminal_receipt(value: &JsonValue) -> Option<()> {
    // delegate the immutable no-action identity to the existing workstation validator
    if text(value, "contractVersion").as_deref() == Some("adjustment-terminal-no-action-receipt/v3")
    {
        return crate::native_host_owner_terminal::validate_no_action_receipt(value);
    }
    exact_keys(
        value,
        &[
            "actionSha256",
            "compensatingRelease",
            "compensationState",
            "contractVersion",
            "family",
            "fencingToken",
            "outcome",
            "state",
            "targetRelease",
        ],
    )
    .then_some(())?;
    (text(value, "contractVersion")? == "adjustment-family-release-status/v1").then_some(())?;
    valid_hash(&text(value, "actionSha256")?).then_some(())?;
    valid_family(&text(value, "family")?).then_some(())?;
    valid_u64(&text(value, "fencingToken")?).then_some(())?;
    valid_release(&text(value, "targetRelease")?).then_some(())?;
    valid_release(&text(value, "compensatingRelease")?).then_some(())?;
    let state = text(value, "state")?;
    let compensation = text(value, "compensationState")?;
    let outcome = field(value, "outcome")?;
    match state.as_str() {
        "acknowledged" => (compensation == "absent"
            && matches!(
                outcome,
                JsonValue::String(units)
                    if ["active", "deployed_operator_off"].iter().any(|expected| {
                        units.iter().copied().eq(expected.encode_utf16())
                    })
            ))
        .then_some(()),
        "compensation_required" => (compensation == "verified").then_some(()),
        "operator_off_unapplied" => (compensation == "absent"
            && matches!(
                outcome,
                JsonValue::String(units)
                    if units.iter().copied().eq("operator_off_unapplied".encode_utf16())
            ))
        .then_some(()),
        _ => None,
    }
}

/// Validate the immutable promoted action bytes before installed transaction comparison.
fn validate_model_action(action: &JsonValue, result: &JsonValue) -> Option<()> {
    exact_keys(
        action,
        &[
            "actionKind",
            "candidateGraphSha256",
            "candidateSha256",
            "contractVersion",
            "createdAt",
            "expectedInstalledReceiptSha256",
            "expectedSettingsSha256",
            "expectedSourceCommit",
            "expectedSourceRelease",
            "family",
            "fencingToken",
            "fullMemberRootSha256",
            "lifecycleHeadSha256",
            "policyDecision",
            "policyReportSha256",
            "predecessorActionSha256",
            "reason",
            "reportCreatedAt",
            "siteKey",
            "validThrough",
        ],
    )
    .then_some(())?;
    (text(action, "actionKind")? == "promote"
        && text(action, "policyDecision")? == "qualified"
        && field(action, "fullMemberRootSha256") == field(result, "fullMemberRootSha256")
        && field(action, "policyReportSha256") == field(result, "candidateReportSha256"))
    .then_some(())
}

/// Authenticate qualified terminal completion against F, live incumbent and actual transaction.
#[allow(clippy::too_many_arguments)]
fn verify_qualified_terminal_authority(
    request: &JsonValue,
    witness_bytes: &[u8],
    current: &NativeFamilyReleaseCurrent,
    anchor: &JsonValue,
    transaction: &mut impl NativeHostOwnerQualifiedTransactionPort,
    now: i64,
    time: &dyn CurrentTimeResolver,
) -> Result<String, NativeHostOwnerQualifiedTerminalError> {
    let proof = field(request, "finalizationProof")
        .ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let shadow = field(request, "shadowRegistration")
        .ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let result =
        field(request, "localResult").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let receipt =
        field(request, "actionReceipt").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let mut witness_codec = NativeWirePrimitives;
    let witness = verify_operator_capture_epoch_witness(witness_bytes, &mut witness_codec, time)
        .map_err(|_| NativeHostOwnerQualifiedTerminalError::CaptureEpoch)?;
    // require the same full witness document, internal epoch member and finalized F proof
    if text(proof, "captureEpochWitnessSha256").as_deref() != Some(digest(witness_bytes).as_str())
        || text(shadow, "epochWitnessSha256").as_deref()
            != Some(witness.witness.witnessSha256.as_str())
        || !finalized_anchor_matches(proof, anchor)
    {
        return Err(NativeHostOwnerQualifiedTerminalError::Authority);
    }
    // no-action dispositions retain the live incumbent and its nonfuture completion receipt
    if text(result, "disposition").as_deref() != Some("promoted") {
        let completed =
            text(receipt, "completedAt").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
        if current.commit
            != text(request, "sourceCommit")
                .ok_or(NativeHostOwnerQualifiedTerminalError::Request)?
            || instant_text(&completed).ok_or(NativeHostOwnerQualifiedTerminalError::Request)? > now
        {
            return Err(NativeHostOwnerQualifiedTerminalError::Authority);
        }
        return Ok(completed);
    }
    let action_sha256 =
        text(proof, "actionSha256").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let actual = transaction.read_terminal_transaction(&action_sha256, time)?;
    let wire = NativeWirePrimitives;
    if wire.encode_sorted_json(&actual.status) != wire.encode_sorted_json(receipt) {
        return Err(NativeHostOwnerQualifiedTerminalError::Authority);
    }
    let record = field(&actual.document, "record")
        .ok_or(NativeHostOwnerQualifiedTerminalError::Authority)?;
    let request_action =
        field(request, "action").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    // operator-off retains a distinct no-mutation record but the same request action coordinates
    if actual.operator_off_unapplied {
        let input =
            field(record, "input").ok_or(NativeHostOwnerQualifiedTerminalError::Authority)?;
        if text(record, "sourceCommit") != text(request, "sourceCommit")
            || current.commit
                != text(request, "sourceCommit")
                    .ok_or(NativeHostOwnerQualifiedTerminalError::Request)?
            || current.release
                != text(request_action, "expectedSourceRelease")
                    .ok_or(NativeHostOwnerQualifiedTerminalError::Request)?
            || field(input, "family") != field(request_action, "family")
            || field(input, "fencingToken") != field(request_action, "fencingToken")
            || field(input, "reportSha256") != field(proof, "policyReportSha256")
            || field(input, "expectedSettingsSha256")
                != field(request_action, "expectedSettingsSha256")
            || field(input, "expectedSourceRelease")
                != field(request_action, "expectedSourceRelease")
            || field(input, "expectedCurrentRelease")
                != field(request_action, "expectedSourceRelease")
            || text(input, "actionSha256").as_deref() != Some(action_sha256.as_str())
            || instant_text(&actual.completed_at)
                .ok_or(NativeHostOwnerQualifiedTerminalError::Authority)?
                > now
        {
            return Err(NativeHostOwnerQualifiedTerminalError::Authority);
        }
        return Ok(actual.completed_at);
    }
    let actual_action =
        field(record, "action").ok_or(NativeHostOwnerQualifiedTerminalError::Authority)?;
    if wire.encode_sorted_json(actual_action) != wire.encode_sorted_json(request_action)
        || text(record, "sourceCommit") != text(request, "sourceCommit")
        || field(actual_action, "fullMemberRootSha256") != field(proof, "fullMemberRootSha256")
        || field(actual_action, "policyReportSha256") != field(proof, "policyReportSha256")
    {
        return Err(NativeHostOwnerQualifiedTerminalError::Authority);
    }
    Ok(actual.completed_at)
}

/// Canonically admit a retirement by rebuilding it through the existing typed owner validator.
fn admit_retirement_request(
    bytes: &[u8],
    expected_sha256: &str,
) -> Result<JsonValue, NativeHostOwnerQualifiedTerminalError> {
    let mut wire = NativeWirePrimitives;
    // close the forced operand before consulting installed state
    if !(2..=MAXIMUM_REQUEST_BYTES).contains(&bytes.len()) || !valid_hash(expected_sha256) {
        return Err(NativeHostOwnerQualifiedTerminalError::Request);
    }
    let value = wire
        .parse_canonical_object(bytes)
        .ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    if digest(bytes) != expected_sha256
        || !exact_keys(
            &value,
            &[
                "contractVersion",
                "finalizationProof",
                "registrationSha256",
                "terminalRecord",
                "terminalTombstone",
            ],
        )
        || text(&value, "contractVersion").as_deref() != Some(REQUEST_VERSION)
    {
        return Err(NativeHostOwnerQualifiedTerminalError::Request);
    }
    let proof =
        field(&value, "finalizationProof").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let record =
        field(&value, "terminalRecord").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let class = terminal_class(record).ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let due_key = text(proof, "dueKey").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let recorded_at =
        text(record, "recordedAt").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let semantic =
        text(proof, "policyReportSha256").ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let rebuilt =
        build_native_maintenance_owner_terminal_material(NativeMaintenanceOwnerTerminalInput {
            authority_proof: proof,
            class,
            due_key: &due_key,
            now: &recorded_at,
            semantic_input_sha256: &semantic,
            terminal_record: record,
        })
        .map_err(|_| NativeHostOwnerQualifiedTerminalError::Request)?;
    // require the typed predecessor retirement projection to reproduce every caller byte
    if rebuilt.retirement_request_bytes != bytes
        || rebuilt.retirement_request_sha256 != expected_sha256
    {
        return Err(NativeHostOwnerQualifiedTerminalError::Request);
    }
    Ok(value)
}

/// Bind the finalization proof to the actual retained finalized anchor and epoch document.
fn verify_qualified_retirement_authority(
    request: &JsonValue,
    witness_bytes: &[u8],
    anchor: &JsonValue,
) -> Result<(), NativeHostOwnerQualifiedTerminalError> {
    let proof = field(request, "finalizationProof")
        .ok_or(NativeHostOwnerQualifiedTerminalError::Request)?;
    let mut witness_codec = NativeWirePrimitives;
    verify_operator_capture_epoch_witness(witness_bytes, &mut witness_codec, &NativeWirePrimitives)
        .map_err(|_| NativeHostOwnerQualifiedTerminalError::CaptureEpoch)?;
    // retirement has no shadow registration; bind only proof, witness and finalized F authority
    if text(proof, "contractVersion").as_deref() != Some(PROOF_VERSION)
        || text(proof, "captureEpochWitnessSha256").as_deref()
            != Some(digest(witness_bytes).as_str())
        || !finalized_anchor_matches(proof, anchor)
    {
        return Err(NativeHostOwnerQualifiedTerminalError::Authority);
    }
    Ok(())
}

/// Require one exact finalized anchor to authorize every action-carried proof identity.
fn finalized_anchor_matches(proof: &JsonValue, anchor: &JsonValue) -> bool {
    let Some(proof_sha256) = canonical_digest(proof) else {
        return false;
    };
    text(anchor, "contractVersion").as_deref() == Some("adjustment-maintenance-anchor/v3")
        && text(anchor, "ctfState").as_deref() == Some("finalized")
        && text(anchor, "finalizationProofSha256").as_deref() == Some(proof_sha256.as_str())
        && [
            "actionSha256",
            "fullMemberRootSha256",
            "lifecycleLedgerRootSha256",
            "policyReportSha256",
            "sourceCommit",
        ]
        .iter()
        .all(|key| field(anchor, key) == field(proof, key))
}

/// Classify one already typed terminal row for exact retirement reconstruction.
fn terminal_class(record: &JsonValue) -> Option<NativeMaintenanceOwnerTerminalClass> {
    match text(record, "actionDisposition")?.as_str() {
        "rejected_no_action" => Some(NativeMaintenanceOwnerTerminalClass::Rejected),
        "resource_refused_no_action" => Some(NativeMaintenanceOwnerTerminalClass::ResourceRefused),
        "support_failed_no_action" => Some(NativeMaintenanceOwnerTerminalClass::SupportFailed),
        "promoted_verified" => Some(NativeMaintenanceOwnerTerminalClass::PromotedVerified {
            serving_changed: false,
        }),
        "promoted_operator_off_unapplied" => {
            Some(NativeMaintenanceOwnerTerminalClass::PromotedOperatorOffUnapplied)
        }
        "operator_off_compensated" => {
            Some(NativeMaintenanceOwnerTerminalClass::OperatorOffCompensated)
        }
        "promotion_failed_compensated" => {
            Some(NativeMaintenanceOwnerTerminalClass::PromotionFailedCompensated)
        }
        _ => None,
    }
}

/// Emit the fixed qualified terminal transaction after action authority verification.
fn qualified_terminal_sql(
    request: &JsonValue,
    _request_sha256: &str,
    completed_at: &str,
) -> Option<String> {
    let proof = field(request, "finalizationProof")?;
    let shadow = field(request, "shadowRegistration")?;
    let native = field(request, "nativeAccess")?;
    let result = field(request, "localResult")?;
    let receipt = field(request, "actionReceipt")?;
    let maintenance_anchor_sha256 = text(proof, "transferredAnchorSha256")?;
    let terminal_result_sha256 = canonical_digest(result)?;
    let disposition = match text(result, "disposition")?.as_str() {
        "promoted" => match text(receipt, "state")?.as_str() {
            "operator_off_unapplied" => "promoted_operator_off_unapplied",
            "acknowledged" => "promoted_verified",
            "compensation_required" => "promotion_failed_compensated",
            _ => return None,
        },
        "rejected" => "rejected_no_action",
        "resource_refused" => "resource_refused_no_action",
        "support_failed" => "support_failed_no_action",
        _ => return None,
    };
    let previous = field(request, "previousTombstone")?;
    let (previous_guard, previous_setting) = previous_sql(previous)?;
    let row = owner_terminal_row_sql("terminal");
    Some(format!(
        "BEGIN;\nSET LOCAL statement_timeout = '30s';\nSET LOCAL lock_timeout = '5s';\nSET LOCAL weather.adjustment_maintenance_anchor_kind = 'terminal_action_reconciliation';\nSET LOCAL weather.adjustment_maintenance_anchor_sha256 = '{maintenance_anchor_sha256}';\nSET LOCAL weather.adjustment_terminal_member_root_sha256 = '{member_root}';\nDO $terminal$\nDECLARE registration adjustment_shadow_registrations_v2%ROWTYPE;\n  access adjustment_confirmation_accesses_v2%ROWTYPE; previous adjustment_shadow_terminal_results_v2%ROWTYPE;\n  reconciliation text; argument jsonb;\nBEGIN\n  PERFORM pg_advisory_xact_lock(hashtextextended('{registration}', 0));\n  SELECT * INTO STRICT registration FROM adjustment_shadow_registrations_v2\n    WHERE registration_sha256 = '{registration}' FOR UPDATE;\n  SELECT * INTO STRICT access FROM adjustment_confirmation_accesses_v2\n    WHERE registration_sha256 = registration.registration_sha256;\n  -- require the real owner access and current registered candidate rather than claimed hashes\n  IF registration.family <> '{family}' OR registration.candidate_sha256 <> '{candidate}'\n    OR registration.artifact_sha256 <> '{artifact}' OR registration.source_sha256 <> '{source}'\n    OR registration.reserved_key_sha256 <> '{reserved}'\n    OR access.access_sha256 <> '{access}' OR access.journal_head_sha256 <> '{journal}'\n    OR access.maintenance_anchor_sha256 <> '{access_anchor}'\n    OR access.gate_manifest_sha256 <> '{gate}'\n    OR access.eligible_prediction_set_sha256 <> '{eligible}'\n    OR access.expected_key_set_sha256 <> '{expected}'\n    OR access.metadata_root_sha256 <> '{metadata}'\n    OR access.target_comparator_snapshot_root_sha256 <> '{comparator}'\n    OR access.revision_catalog_watermark_sha256 <> '{watermark}'\n    OR access.target_cutoff_at <> '{cutoff}'::timestamptz\n    OR date_trunc('milliseconds', access.accessed_at) <> '{accessed}'::timestamptz THEN\n    RAISE EXCEPTION 'terminal record actual owner access differs';\n  END IF;\n  SELECT * INTO previous FROM adjustment_shadow_terminal_results_v2 WHERE family = registration.family;\n  -- rotate only the exact prior tombstone already retained in the terminal graph\n  IF {previous_guard} THEN RAISE EXCEPTION 'terminal record predecessor differs'; END IF;\n  {previous_setting}\n  reconciliation := encode(sha256(convert_to('adjustment-shadow-terminal-reconciliation/v2' || E'\\n'\n    || registration.registration_sha256 || E'\\n' || registration.family || E'\\n' || registration.candidate_sha256 || E'\\n'\n    || registration.source_sha256 || E'\\n' || registration.reserved_key_sha256 || E'\\n{member_root}\\n{terminal_result_sha256}\\n'\n    || access.access_sha256 || E'\\n{action}\\n{disposition}\\n{completed_at}\\n'\n    || registration.finalized_metadata_root_sha256 || E'\\n' || registration.metadata_generation::text || E'\\n'\n    || registration.finalized_prediction_count::text || E'\\n{maintenance_anchor_sha256}', 'UTF8')), 'hex');\n  argument := jsonb_build_object('actionCompletedAt', '{completed_at}', 'actionDisposition', '{disposition}',\n    'actionSha256', '{action}', 'maintenanceAnchorSha256', '{maintenance_anchor_sha256}',\n    'reconciliationSha256', reconciliation, 'registrationSha256', registration.registration_sha256,\n    'terminalMemberSha256', '{member_root}', 'terminalResultSha256', '{terminal_result_sha256}');\n  PERFORM weather_record_adjustment_shadow_terminal_result_v2(argument);\nEND;\n$terminal$;\nSELECT json_build_object('contractVersion', 'adjustment-shadow-terminal-record-result/v3',\n  'reconciliationSha256', terminal.reconciliation_sha256, 'registrationSha256', terminal.registration_sha256,\n  'state', 'recorded', 'terminalRecord', {row})\nFROM adjustment_shadow_terminal_results_v2 terminal JOIN adjustment_shadow_registrations_v2 registration USING (registration_sha256)\nWHERE terminal.registration_sha256 = '{registration}';\nCOMMIT;\n",
        member_root = text(proof, "fullMemberRootSha256")?,
        registration = text(shadow, "registrationSha256")?,
        family = text(shadow, "family")?,
        candidate = text(shadow, "candidateSha256")?,
        artifact = text(shadow, "artifactSha256")?,
        source = text(shadow, "sourceSha256")?,
        reserved = text(shadow, "reservedKeySha256")?,
        access = text(native, "accessSha256")?,
        journal = text(native, "journalHeadSha256")?,
        access_anchor = text(native, "maintenanceAnchorSha256")?,
        gate = text(native, "gateManifestSha256")?,
        eligible = text(native, "eligiblePredictionSetSha256")?,
        expected = text(native, "expectedKeySetSha256")?,
        metadata = text(native, "metadataRootSha256")?,
        comparator = text(native, "targetComparatorSnapshotRootSha256")?,
        watermark = text(native, "revisionCatalogWatermarkSha256")?,
        cutoff = text(native, "targetCutoffAt")?,
        accessed = text(native, "accessedAt")?,
        action = text(proof, "actionSha256")?,
    ))
}

/// Derive the exact predecessor guard and optional retained-tombstone setting.
fn previous_sql(previous: &JsonValue) -> Option<(String, String)> {
    // explicit absence requires no same-registration predecessor in the database
    if matches!(previous, JsonValue::Null) {
        return Some((
            "previous.registration_sha256 IS NOT NULL AND previous.registration_sha256 <> registration.registration_sha256".to_owned(),
            String::new(),
        ));
    }
    let registration = text(previous, "registrationSha256")?;
    let reconciliation = text(previous, "reconciliationSha256")?;
    let result = text(previous, "terminalResultSha256")?;
    Some((
        format!(
            "(previous.registration_sha256 IS NULL OR previous.registration_sha256 <> '{registration}'\n      OR previous.reconciliation_sha256 <> '{reconciliation}'\n      OR previous.terminal_result_sha256 <> '{result}')\n      AND (previous.registration_sha256 IS NULL OR previous.registration_sha256 <> registration.registration_sha256)"
        ),
        format!(
            "PERFORM set_config('weather.adjustment_previous_terminal_tombstone_sha256', '{reconciliation}', true);"
        ),
    ))
}

/// Emit the fixed source retirement transaction after the typed request and anchor checks.
fn qualified_retirement_sql(request: &JsonValue, request_sha256: &str) -> Option<String> {
    let identity = text(request, "registrationSha256")?;
    let record = field(request, "terminalRecord")?;
    let reconciliation = text(record, "reconciliationSha256")?;
    let record_literal = canonical_literal(record)?;
    let argument = object(vec![
        ("reconciliationSha256", string(&reconciliation)),
        ("registrationSha256", string(&identity)),
        ("retirementAnchorSha256", string(request_sha256)),
    ]);
    let argument_literal = canonical_literal(&argument)?;
    let row = owner_terminal_row_sql("terminal");
    Some(format!(
        "BEGIN;\nSET LOCAL statement_timeout = '30s';\nSET LOCAL lock_timeout = '5s';\nSET LOCAL weather.adjustment_maintenance_anchor_kind = 'terminal_registration_retirement';\nSET LOCAL weather.adjustment_maintenance_anchor_sha256 = '{request_sha256}';\nDO $retire$\nDECLARE actual jsonb; retained_terminal adjustment_shadow_terminal_results_v2%ROWTYPE;\nBEGIN\n  PERFORM pg_advisory_xact_lock(hashtextextended('{identity}', 0));\n  SELECT * INTO STRICT retained_terminal FROM adjustment_shadow_terminal_results_v2 WHERE registration_sha256 = '{identity}';\n  -- bind every retained terminal field even after its hot registration was retired\n  SELECT {row} INTO actual FROM adjustment_shadow_terminal_results_v2 terminal\n    JOIN adjustment_shadow_registration_windows_v3 registration USING (registration_sha256)\n    WHERE terminal.registration_sha256 = '{identity}';\n  IF actual IS NULL OR actual <> $record${record_literal}$record$::jsonb THEN\n    RAISE EXCEPTION 'terminal retirement actual row differs';\n  END IF;\n  -- preserve the exact durable tombstone when a completed retirement response was lost\n  IF NOT EXISTS (SELECT 1 FROM adjustment_shadow_registrations_v2 WHERE registration_sha256 = '{identity}') THEN\n    RETURN;\n  END IF;\n  PERFORM weather_retire_adjustment_shadow_registration_v2($argument${argument_literal}$argument$::jsonb);\nEND;\n$retire$;\nSELECT json_build_object('contractVersion', 'adjustment-shadow-terminal-retirement-result/v3',\n  'reconciliationSha256', terminal.reconciliation_sha256, 'registrationSha256', terminal.registration_sha256, 'state', 'retired')\nFROM adjustment_shadow_terminal_results_v2 terminal WHERE terminal.registration_sha256 = '{identity}';\nCOMMIT;\n"
    ))
}

/// Render the fixed owner terminal row projection for a trusted alias.
fn owner_terminal_row_sql(alias: &str) -> String {
    format!(
        "jsonb_build_object('accessSha256', {alias}.confirmation_access_sha256, 'actionCompletedAt', to_char({alias}.action_completed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'actionDisposition', {alias}.action_disposition, 'actionSha256', {alias}.action_sha256, 'candidateSha256', registration.candidate_sha256, 'contractVersion', 'adjustment-shadow-terminal-record/v3', 'family', {alias}.family, 'finalizedMetadataRootSha256', {alias}.metadata_root_sha256, 'finalizedPredictionCount', {alias}.finalized_prediction_count, 'maintenanceAnchorSha256', {alias}.maintenance_anchor_sha256, 'metadataGeneration', {alias}.metadata_generation, 'recordedAt', to_char({alias}.recorded_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'reconciliationSha256', {alias}.reconciliation_sha256, 'registrationSha256', {alias}.registration_sha256, 'reservedKeySha256', {alias}.reserved_key_sha256, 'sourceSha256', {alias}.source_sha256, 'terminalMemberSha256', {alias}.terminal_member_sha256, 'terminalResultSha256', {alias}.terminal_result_sha256)"
    )
}

/// Retain exact catalog bytes already used by the current-family owner.
struct CatalogCapturingFiles<'a, Files> {
    inner: &'a mut Files,
    catalog: Option<Vec<u8>>,
}

impl<'a, Files> CatalogCapturingFiles<'a, Files> {
    /// Wrap one installed file owner without another catalog observation.
    const fn new(inner: &'a mut Files) -> Self {
        Self {
            inner,
            catalog: None,
        }
    }
}

impl<Files> NativeFamilyReleaseCurrentFiles for CatalogCapturingFiles<'_, Files>
where
    Files: NativeFamilyReleaseCurrentFiles,
{
    /// Forward the current release marker read.
    fn read_current_release(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        self.inner.read_current_release()
    }

    /// Forward the release environment read.
    fn read_environment(
        &mut self,
        release: &str,
    ) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        self.inner.read_environment(release)
    }

    /// Forward the installed settings read.
    fn read_settings(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        self.inner.read_settings()
    }

    /// Capture the catalog bytes already admitted by current-family verification.
    fn read_catalog(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        let bytes = self.inner.read_catalog()?;
        self.catalog = Some(bytes.clone());
        Ok(bytes)
    }

    /// Forward the optional family current acknowledgement.
    fn read_current_authority(
        &mut self,
    ) -> Result<Option<Vec<u8>>, NativeFamilyReleaseCurrentError> {
        self.inner.read_current_authority()
    }
}

/// Map installed catalog failures separately from other live-family failures.
fn map_family_error(
    error: NativeFamilyReleaseCurrentError,
) -> NativeHostOwnerQualifiedTerminalError {
    match error {
        NativeFamilyReleaseCurrentError::Catalog => NativeHostOwnerQualifiedTerminalError::Catalog,
        _ => NativeHostOwnerQualifiedTerminalError::Family,
    }
}

/// Encode one canonical JSON value without its final LF for a fixed SQL literal.
fn canonical_literal(value: &JsonValue) -> Option<String> {
    let mut bytes = NativeWirePrimitives.encode_sorted_json(value)?;
    (bytes.pop() == Some(b'\n')).then_some(())?;
    String::from_utf8(bytes).ok()
}

/// Hash one exact byte sequence under the source digest spelling.
fn digest(bytes: &[u8]) -> String {
    format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        bytes,
    ))
}

/// Hash one canonical LF-terminated JSON value.
fn canonical_digest(value: &JsonValue) -> Option<String> {
    Some(digest(&NativeWirePrimitives.encode_sorted_json(value)?))
}

/// Parse one canonical object clock field.
fn instant(value: &JsonValue, key: &str) -> Option<i64> {
    instant_text(&text(value, key)?)
}

/// Parse one canonical UTC millisecond instant.
fn instant_text(value: &str) -> Option<i64> {
    NativeWirePrimitives.canonical_utc_milliseconds(value)
}

/// Admit only one lowercase public commit identity.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Admit only the three closed adjustment families.
fn valid_family(value: &str) -> bool {
    matches!(value, "temperature" | "wind" | "rain")
}

/// Admit one canonical positive unsigned fence.
fn valid_u64(value: &str) -> bool {
    !value.is_empty()
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

/// Admit one immutable source release tag.
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    let revision = bytes.get(11..).unwrap_or_default();
    bytes.len() >= 12
        && bytes.get(4) == Some(&b'.')
        && bytes.get(7) == Some(&b'.')
        && bytes.get(10) == Some(&b'-')
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && matches!(revision.first(), Some(b'1'..=b'9'))
        && revision.iter().all(u8::is_ascii_digit)
}

#[cfg(test)]
#[path = "native_host_owner_qualified_terminal_test.rs"]
mod tests;
