//! Installed authority and fixed SQL for unsupported terminal owner operations.

use std::fmt::{self, Display, Formatter};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::capture_epoch_operator::{
    CaptureEpochOperatorFile, CaptureEpochOperatorFiles, NativeCaptureEpochOperatorFiles,
};
use crate::capture_epoch_witness::verify_operator_capture_epoch_witness;
use crate::family_target_catalog::catalog_entries;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    FamilyCatalogImageVerification, MaintenanceCanonicalJson, NativeFamilyCatalogImageVerifier,
    NativeFamilyReleaseCurrent, NativeFamilyReleaseCurrentError, NativeFamilyReleaseCurrentFiles,
    NativeFamilyReleaseCurrentHostFiles, NativeFamilyReleaseLiveRuntime,
    NativeFamilyReleaseLiveRuntimeProcess, NativeUnsupportedTerminalProofCurrent,
    NativeUnsupportedTerminalProofFiles, NativeUnsupportedTerminalProofStore, NativeWirePrimitives,
    read_native_family_release_current, read_unsupported_terminal_proof_current,
    system_worker_health_milliseconds, validate_native_maintenance_archive_registration,
};

const MAXIMUM_REQUEST_BYTES: usize = 128 * 1024;
const RECORD_VERSION: &str = "adjustment-shadow-unsupported-terminal-record-request/v1";
const RETIREMENT_VERSION: &str = "adjustment-shadow-unsupported-terminal-retirement-request/v1";
const PROOF_VERSION: &str = "adjustment-maintenance-unsupported-terminal-proof/v1";
const TERMINAL_RECORD_VERSION: &str = "adjustment-shadow-terminal-record/v3";
const TOMBSTONE_VERSION: &str = "adjustment-shadow-terminal-tombstone/v3";
const CONTEXT_FIELDS: &[&str] = &[
    "accessState",
    "actionIdentitySha256",
    "actionState",
    "candidateKind",
    "candidateReportSha256",
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
];
const IMMUTABLE_FIELDS: &[&str] = &[
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
];
const NATIVE_ACCESS_FIELDS: &[&str] = &[
    "accessSha256",
    "accessedAt",
    "eligiblePredictionSetSha256",
    "expectedKeySetSha256",
    "gateManifestSha256",
    "journalHeadSha256",
    "maintenanceAnchorSha256",
    "metadataRootSha256",
    "registrationSha256",
    "revisionCatalogWatermarkSha256",
    "targetComparatorSnapshotRootSha256",
    "targetCutoffAt",
];
const TERMINAL_RECORD_FIELDS: &[&str] = &[
    "accessSha256",
    "actionCompletedAt",
    "actionDisposition",
    "actionSha256",
    "candidateSha256",
    "contractVersion",
    "family",
    "finalizedMetadataRootSha256",
    "finalizedPredictionCount",
    "maintenanceAnchorSha256",
    "metadataGeneration",
    "recordedAt",
    "reconciliationSha256",
    "registrationSha256",
    "reservedKeySha256",
    "sourceSha256",
    "terminalMemberSha256",
    "terminalResultSha256",
];

/// The two unsupported-only owner effects admitted by this installed authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHostOwnerUnsupportedKind {
    Record,
    Retire,
}

/// Bounded installed terminal failures without retained host values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHostOwnerTerminalError {
    Request,
    CaptureEpoch,
    Family,
    Catalog,
    Proof,
    Clock,
    Authority,
    Sql,
}

impl Display for NativeHostOwnerTerminalError {
    /// Render one stable operator-facing failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Request => "native owner terminal request is invalid",
            Self::CaptureEpoch => "native owner capture epoch authority is unavailable",
            Self::Family => "native owner family authority is unavailable",
            Self::Catalog => "native owner installed catalog is unavailable",
            Self::Proof => "native owner unsupported proof is unavailable",
            Self::Clock => "native owner wall clock is unavailable",
            Self::Authority => "native owner terminal authority differs",
            Self::Sql => "native owner terminal SQL projection failed",
        })
    }
}

impl std::error::Error for NativeHostOwnerTerminalError {}

/// Exact canonical request identity and fixed SQL emitted after installed checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeHostOwnerTerminalSql {
    pub kind: NativeHostOwnerUnsupportedKind,
    pub request_sha256: String,
    pub sql: String,
}

/// Independently supplied owners for deterministic installed-authority fixtures.
pub(crate) struct NativeHostOwnerTerminalOwners<'a, Capture, Family, Runtime, Verifier, Proof> {
    pub capture: &'a mut Capture,
    pub family: &'a mut Family,
    pub runtime: &'a mut Runtime,
    pub verifier: &'a mut Verifier,
    pub proof: &'a mut Proof,
    pub time: &'a dyn CurrentTimeResolver,
    pub now: fn() -> Option<i64>,
}

/// Build one unsupported terminal record from only installed root and service authorities.
pub fn build_native_host_owner_unsupported_terminal_sql_from_installed(
    request_bytes: &[u8],
    expected_sha256: &str,
) -> Result<NativeHostOwnerTerminalSql, NativeHostOwnerTerminalError> {
    build_from_installed(
        NativeHostOwnerUnsupportedKind::Record,
        request_bytes,
        expected_sha256,
    )
}

/// Build one unsupported terminal retirement from its retained proof and terminal row.
pub fn build_native_host_owner_unsupported_retirement_sql_from_installed(
    request_bytes: &[u8],
    expected_sha256: &str,
) -> Result<NativeHostOwnerTerminalSql, NativeHostOwnerTerminalError> {
    build_from_installed(
        NativeHostOwnerUnsupportedKind::Retire,
        request_bytes,
        expected_sha256,
    )
}

/// Bind the fixed production paths while retaining the predecessor process owner identity.
fn build_from_installed(
    kind: NativeHostOwnerUnsupportedKind,
    request_bytes: &[u8],
    expected_sha256: &str,
) -> Result<NativeHostOwnerTerminalSql, NativeHostOwnerTerminalError> {
    let mut capture = NativeCaptureEpochOperatorFiles;
    let mut family = NativeFamilyReleaseCurrentHostFiles::native(0, 0);
    let mut runtime = NativeFamilyReleaseLiveRuntimeProcess::native();
    let mut verifier = NativeFamilyCatalogImageVerifier::new();
    // match the predecessor proof store's effective-process owner boundary
    let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    let mut proof = NativeUnsupportedTerminalProofStore::native(uid, gid);
    build_native_host_owner_unsupported_sql_from_owners(
        kind,
        request_bytes,
        expected_sha256,
        NativeHostOwnerTerminalOwners {
            capture: &mut capture,
            family: &mut family,
            runtime: &mut runtime,
            verifier: &mut verifier,
            proof: &mut proof,
            time: &NativeWirePrimitives,
            now: system_worker_health_milliseconds,
        },
    )
}

/// Compose one unsupported operation after every independent installed observation.
pub(crate) fn build_native_host_owner_unsupported_sql_from_owners<
    Capture,
    Family,
    Runtime,
    Verifier,
    Proof,
>(
    kind: NativeHostOwnerUnsupportedKind,
    request_bytes: &[u8],
    expected_sha256: &str,
    owners: NativeHostOwnerTerminalOwners<'_, Capture, Family, Runtime, Verifier, Proof>,
) -> Result<NativeHostOwnerTerminalSql, NativeHostOwnerTerminalError>
where
    Capture: CaptureEpochOperatorFiles,
    Family: NativeFamilyReleaseCurrentFiles,
    Runtime: NativeFamilyReleaseLiveRuntime,
    Verifier: FamilyCatalogImageVerification,
    Proof: NativeUnsupportedTerminalProofFiles,
{
    let request = admit_request(kind, request_bytes, expected_sha256)?;
    let family_name =
        request_family(kind, &request).ok_or(NativeHostOwnerTerminalError::Request)?;
    let witness_bytes = owners
        .capture
        .read(CaptureEpochOperatorFile::Witness)
        .map_err(|_| NativeHostOwnerTerminalError::CaptureEpoch)?;
    let mut family_files = CatalogCapturingFiles::new(owners.family);
    let mut wire = NativeWirePrimitives;
    let current = read_native_family_release_current(
        &family_name,
        &mut family_files,
        owners.runtime,
        owners.verifier,
        &mut wire,
        owners.time,
    )
    .map_err(map_family_error)?;
    let catalog = wire
        .parse_canonical_object(
            family_files
                .catalog
                .as_deref()
                .ok_or(NativeHostOwnerTerminalError::Catalog)?,
        )
        .ok_or(NativeHostOwnerTerminalError::Catalog)?;
    let time_codec = NativeWirePrimitives;
    let retained = read_unsupported_terminal_proof_current(owners.proof, &mut wire, &time_codec)
        .map_err(|_| NativeHostOwnerTerminalError::Proof)?
        .ok_or(NativeHostOwnerTerminalError::Proof)?;
    let now = (owners.now)().ok_or(NativeHostOwnerTerminalError::Clock)?;
    verify_installed_authority(
        kind,
        &request,
        &witness_bytes,
        &current,
        &catalog,
        &retained,
        now,
        owners.time,
    )?;
    let sql = match kind {
        NativeHostOwnerUnsupportedKind::Record => unsupported_record_sql(&request, expected_sha256)
            .ok_or(NativeHostOwnerTerminalError::Sql)?,
        NativeHostOwnerUnsupportedKind::Retire => {
            retirement_sql(&request, expected_sha256).ok_or(NativeHostOwnerTerminalError::Sql)?
        }
    };
    Ok(NativeHostOwnerTerminalSql {
        kind,
        request_sha256: expected_sha256.to_owned(),
        sql,
    })
}

/// Decode one canonical request and run its complete unsupported workstation grammar.
fn admit_request(
    kind: NativeHostOwnerUnsupportedKind,
    bytes: &[u8],
    expected_sha256: &str,
) -> Result<JsonValue, NativeHostOwnerTerminalError> {
    let mut wire = NativeWirePrimitives;
    // refuse bytes that cannot be the fixed forced-command document
    if !(2..=MAXIMUM_REQUEST_BYTES).contains(&bytes.len()) || !valid_hash(expected_sha256) {
        return Err(NativeHostOwnerTerminalError::Request);
    }
    let value = wire
        .parse_canonical_object(bytes)
        .ok_or(NativeHostOwnerTerminalError::Request)?;
    let actual = digest(bytes);
    let valid = match kind {
        NativeHostOwnerUnsupportedKind::Record => validate_unsupported_record(&value),
        NativeHostOwnerUnsupportedKind::Retire => validate_unsupported_retirement(&value),
    };
    // bind the validator result to the exact caller operand before observing host state
    if actual != expected_sha256 || valid.is_none() {
        return Err(NativeHostOwnerTerminalError::Request);
    }
    Ok(value)
}

/// Authenticate the retained proof, epoch, family, catalog and wall-clock domains.
#[allow(clippy::too_many_arguments)]
fn verify_installed_authority(
    kind: NativeHostOwnerUnsupportedKind,
    request: &JsonValue,
    witness_bytes: &[u8],
    current: &NativeFamilyReleaseCurrent,
    catalog: &JsonValue,
    retained: &NativeUnsupportedTerminalProofCurrent,
    now: i64,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeHostOwnerTerminalError> {
    let wire = NativeWirePrimitives;
    let mut witness_codec = NativeWirePrimitives;
    let witness = verify_operator_capture_epoch_witness(witness_bytes, &mut witness_codec, time)
        .map_err(|_| NativeHostOwnerTerminalError::CaptureEpoch)?;
    let proof = retained_unsupported_proof(request, retained)
        .ok_or(NativeHostOwnerTerminalError::Authority)?;
    let family = request_family(kind, request).ok_or(NativeHostOwnerTerminalError::Request)?;
    let witness_sha256 = digest(witness_bytes);
    // require the fully validated retained proof rather than any caller proof claims
    if text(proof, "captureEpochWitnessSha256").as_deref() != Some(witness_sha256.as_str())
        || text(proof, "sourceCommit").as_deref() != Some(current.commit.as_str())
        || text(proof, "family").as_deref() != Some(family.as_str())
        || current.family != family
    {
        return Err(NativeHostOwnerTerminalError::Authority);
    }
    // retirement has no shadow registration; record creation binds the live catalog slot and clock
    if kind == NativeHostOwnerUnsupportedKind::Record {
        let shadow =
            field(request, "shadowRegistration").ok_or(NativeHostOwnerTerminalError::Request)?;
        let entry = catalog_entries(catalog)
            .map_err(|_| NativeHostOwnerTerminalError::Catalog)?
            .iter()
            .find(|entry| {
                text(entry, "family").as_deref() == Some(family.as_str())
                    && text(entry, "slot").as_deref() == Some("shadow")
            })
            .and_then(|entry| field(entry, "registration"))
            .ok_or(NativeHostOwnerTerminalError::Authority)?;
        let receipt =
            field(request, "actionReceipt").ok_or(NativeHostOwnerTerminalError::Request)?;
        let completed =
            instant(receipt, "completedAt").ok_or(NativeHostOwnerTerminalError::Request)?;
        let entry_bytes = wire
            .encode_sorted_json(entry)
            .ok_or(NativeHostOwnerTerminalError::Catalog)?;
        let shadow_bytes = wire
            .encode_sorted_json(shadow)
            .ok_or(NativeHostOwnerTerminalError::Request)?;
        // preserve the internal witness member, exact installed registration and nonfuture receipt
        if text(shadow, "epochWitnessSha256").as_deref()
            != Some(witness.witness.witnessSha256.as_str())
            || entry_bytes != shadow_bytes
            || completed > now
        {
            return Err(NativeHostOwnerTerminalError::Authority);
        }
    }
    Ok(())
}

/// Return only a request proof exactly equal to the validated retained current proof.
fn retained_unsupported_proof<'a>(
    request: &'a JsonValue,
    retained: &NativeUnsupportedTerminalProofCurrent,
) -> Option<&'a JsonValue> {
    let proof = field(request, "unsupportedProof")?;
    let proof_bytes = NativeWirePrimitives.encode_sorted_json(proof)?;
    let retained_bytes = NativeWirePrimitives.encode_sorted_json(&retained.proof)?;
    (proof_bytes == retained_bytes && digest(&proof_bytes) == retained.proof_sha256)
        .then_some(proof)
}

/// Validate the complete unsupported terminal record request and all local cross-bindings.
fn validate_unsupported_record(value: &JsonValue) -> Option<()> {
    exact_keys(
        value,
        &[
            "actionReceipt",
            "confirmationRegistration",
            "contractVersion",
            "localResult",
            "nativeAccess",
            "previousTombstone",
            "shadowRegistration",
            "sourceCommit",
            "terminalGraphManifestSha256",
            "terminalGraphVerifiedAt",
            "unsupportedProof",
        ],
    )
    .then_some(())?;
    (text(value, "contractVersion")? == RECORD_VERSION).then_some(())?;
    let registration = field(value, "confirmationRegistration")?;
    validate_confirmation_registration(registration)?;
    let shadow = field(value, "shadowRegistration")?;
    let shadow_bytes = NativeWirePrimitives.encode_sorted_json(shadow)?;
    validate_native_maintenance_archive_registration(
        "adjustment-shadow-registration/v3",
        &shadow_bytes,
    )?;
    let result = field(value, "localResult")?;
    validate_local_result(result)?;
    let native = field(value, "nativeAccess")?;
    validate_native_access(native)?;
    let receipt = field(value, "actionReceipt")?;
    validate_no_action_receipt(receipt)?;
    validate_tombstone(field(value, "previousTombstone")?, true)?;
    valid_hash(&text(value, "terminalGraphManifestSha256")?).then_some(())?;
    instant(value, "terminalGraphVerifiedAt")?;
    valid_lower_hex(&text(value, "sourceCommit")?, 40).then_some(())?;
    let proof = field(value, "unsupportedProof")?;
    (text(proof, "contractVersion")? == PROOF_VERSION).then_some(())?;
    let due = text(proof, "dueKey")?;
    let due = due.strip_prefix("confirmation/")?;
    let (due_family, due_candidate) = due.split_once('/')?;
    (valid_hash(due_candidate)
        && due_family == text(shadow, "family")?
        && due_candidate == text(shadow, "candidateSha256")?)
    .then_some(())?;
    // freeze every local registration identity across terminal evaluation
    for key in IMMUTABLE_FIELDS {
        (field(result, key)? == field(registration, key)?).then_some(())?;
    }
    (text(result, "disposition")? == "support_failed"
        && matches!(field(result, "actionIdentitySha256"), Some(JsonValue::Null))
        && field(native, "registrationSha256") == field(shadow, "registrationSha256")
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
        && field(proof, "registrationSha256") == field(shadow, "registrationSha256")
        && field(proof, "confirmationAccessSha256") == field(native, "accessSha256")
        && field(proof, "fullMemberRootSha256") == field(result, "fullMemberRootSha256")
        && field(proof, "policyReportSha256") == field(result, "candidateReportSha256")
        && field(proof, "graphManifestSha256") == field(value, "terminalGraphManifestSha256")
        && field(receipt, "actionSha256") == field(proof, "actionSha256")
        && text(receipt, "disposition")? == "support_failed"
        && field(receipt, "registrationSha256") == field(shadow, "registrationSha256")
        && field(receipt, "fullMemberRootSha256") == field(proof, "fullMemberRootSha256")
        && field(receipt, "policyReportSha256") == field(proof, "policyReportSha256")
        && instant(native, "accessedAt")? >= instant(shadow, "terminalAt")?
        && instant(proof, "fullGraphVerifiedAt")? >= instant(native, "accessedAt")?
        && instant(receipt, "completedAt")? >= instant(proof, "finalizedAt")?
        && instant(value, "terminalGraphVerifiedAt")? >= instant(receipt, "completedAt")?)
    .then_some(())
}

/// Validate one unsupported retirement without requiring an absent shadow registration.
fn validate_unsupported_retirement(value: &JsonValue) -> Option<()> {
    exact_keys(
        value,
        &[
            "contractVersion",
            "registrationSha256",
            "terminalRecord",
            "terminalTombstone",
            "unsupportedProof",
        ],
    )
    .then_some(())?;
    (text(value, "contractVersion")? == RETIREMENT_VERSION).then_some(())?;
    let registration = text(value, "registrationSha256")?;
    valid_hash(&registration).then_some(())?;
    let proof = field(value, "unsupportedProof")?;
    (text(proof, "contractVersion")? == PROOF_VERSION).then_some(())?;
    let record = field(value, "terminalRecord")?;
    validate_terminal_record(record)?;
    let tombstone = field(value, "terminalTombstone")?;
    validate_tombstone(tombstone, false)?;
    let proof_sha256 = canonical_digest(proof)?;
    (text(record, "registrationSha256")? == registration
        && text(proof, "registrationSha256")? == registration
        && text(tombstone, "registrationSha256")? == registration
        && field(tombstone, "reconciliationSha256") == field(record, "reconciliationSha256")
        && field(proof, "actionSha256") == field(record, "actionSha256")
        && field(proof, "confirmationAccessSha256") == field(record, "accessSha256")
        && field(proof, "family") == field(record, "family")
        && field(proof, "fullMemberRootSha256") == field(record, "terminalMemberSha256")
        && text(record, "maintenanceAnchorSha256")? == proof_sha256)
        .then_some(())
}

/// Validate the exact local preregistration identity.
pub(crate) fn validate_confirmation_registration(value: &JsonValue) -> Option<()> {
    exact_keys(value, CONTEXT_FIELDS).then_some(())?;
    let family = text(value, "family")?;
    let expected_kind = match family.as_str() {
        "temperature" => "temperature-delayed-mos/v1",
        "wind" => "wind-robust-hierarchical-median/v1",
        "rain" => "rain-hurdle-wind-occurrence-amount/v1",
        _ => return None,
    };
    for key in [
        "candidateSha256",
        "cohortLineageSha256",
        "gateManifestSha256",
        "inputHeadSha256",
        "registrationSha256",
        "reservedKeySha256",
        "sourceLineageSha256",
    ] {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    for key in ["firstTargetAt", "terminalAccessAt"] {
        instant(value, key)?;
    }
    (text(value, "contractVersion")? == "forecast-adjustment-lifecycle-ledger/v2"
        && text(value, "candidateKind")? == expected_kind
        && text(value, "accessState")? == "registered"
        && text(value, "actionState")? == "none"
        && matches!(field(value, "candidateReportSha256"), Some(JsonValue::Null))
        && matches!(field(value, "actionIdentitySha256"), Some(JsonValue::Null))
        && valid_local_date(&text(value, "intervalStartLocalDate")?)
        && valid_local_date(&text(value, "intervalEndExclusiveLocalDate")?))
    .then_some(())?;
    let payload_fields = [
        "contractVersion",
        "family",
        "candidateKind",
        "candidateSha256",
        "cohortLineageSha256",
        "sourceLineageSha256",
        "reservedKeySha256",
        "intervalStartLocalDate",
        "intervalEndExclusiveLocalDate",
        "firstTargetAt",
        "terminalAccessAt",
        "gateManifestSha256",
        "inputHeadSha256",
    ];
    let payload = object(
        payload_fields
            .iter()
            .map(|key| Some((*key, field(value, key)?.clone())))
            .collect::<Option<Vec<_>>>()?,
    );
    (canonical_digest(&payload)? == text(value, "registrationSha256")?).then_some(())
}

/// Validate one local terminal projection without assigning it owner authority.
pub(crate) fn validate_local_result(value: &JsonValue) -> Option<()> {
    let keys = CONTEXT_FIELDS
        .iter()
        .copied()
        .chain([
            "disposition",
            "fullMemberRootSha256",
            "nextConfirmationEligibleAt",
        ])
        .collect::<Vec<_>>();
    exact_keys(value, &keys).then_some(())?;
    for key in [
        "registrationSha256",
        "candidateReportSha256",
        "fullMemberRootSha256",
    ] {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    instant(value, "nextConfirmationEligibleAt")?;
    let disposition = text(value, "disposition")?;
    matches!(
        disposition.as_str(),
        "promoted" | "rejected" | "resource_refused" | "support_failed"
    )
    .then_some(())?;
    let action_pending = disposition == "promoted";
    (text(value, "accessState")? == "opened"
        && text(value, "actionState")?
            == if action_pending {
                "action_pending"
            } else {
                "terminal_no_action"
            }
        && action_pending == text(value, "actionIdentitySha256").is_some_and(|v| valid_hash(&v)))
    .then_some(())
}

/// Validate and recompute one owner-native confirmation access identity.
pub(crate) fn validate_native_access(value: &JsonValue) -> Option<()> {
    exact_keys(value, NATIVE_ACCESS_FIELDS).then_some(())?;
    for key in NATIVE_ACCESS_FIELDS
        .iter()
        .copied()
        .filter(|key| key.ends_with("Sha256"))
    {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    instant(value, "accessedAt")?;
    instant(value, "targetCutoffAt")?;
    let preimage = [
        "adjustment-confirmation-access/v2".to_owned(),
        text(value, "registrationSha256")?,
        text(value, "journalHeadSha256")?,
        text(value, "maintenanceAnchorSha256")?,
        text(value, "gateManifestSha256")?,
        text(value, "eligiblePredictionSetSha256")?,
        text(value, "expectedKeySetSha256")?,
        text(value, "metadataRootSha256")?,
        text(value, "targetComparatorSnapshotRootSha256")?,
        text(value, "revisionCatalogWatermarkSha256")?,
        text(value, "targetCutoffAt")?,
    ]
    .join("\n");
    (digest(preimage.as_bytes()) == text(value, "accessSha256")?).then_some(())
}

/// Validate the source no-action receipt and its independent identity.
pub(crate) fn validate_no_action_receipt(value: &JsonValue) -> Option<()> {
    exact_keys(
        value,
        &[
            "actionSha256",
            "completedAt",
            "contractVersion",
            "disposition",
            "fullMemberRootSha256",
            "policyReportSha256",
            "registrationSha256",
            "state",
        ],
    )
    .then_some(())?;
    for key in [
        "actionSha256",
        "fullMemberRootSha256",
        "policyReportSha256",
        "registrationSha256",
    ] {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    instant(value, "completedAt")?;
    let disposition = text(value, "disposition")?;
    matches!(
        disposition.as_str(),
        "rejected" | "resource_refused" | "support_failed"
    )
    .then_some(())?;
    let identity = object(vec![
        (
            "contractVersion",
            string("adjustment-terminal-no-action-identity/v3"),
        ),
        ("disposition", string(&disposition)),
        (
            "policyReportSha256",
            string(&text(value, "policyReportSha256")?),
        ),
        (
            "registrationSha256",
            string(&text(value, "registrationSha256")?),
        ),
    ]);
    (text(value, "contractVersion")? == "adjustment-terminal-no-action-receipt/v3"
        && text(value, "state")? == "verified_no_action"
        && text(value, "actionSha256")? == canonical_digest(&identity)?)
    .then_some(())
}

/// Validate one nullable predecessor or required current tombstone.
pub(crate) fn validate_tombstone(value: &JsonValue, nullable: bool) -> Option<()> {
    // preserve explicit absence only for record creation
    if nullable && matches!(value, JsonValue::Null) {
        return Some(());
    }
    exact_keys(
        value,
        &[
            "contractVersion",
            "reconciliationSha256",
            "registrationSha256",
            "terminalResultSha256",
        ],
    )
    .then_some(())?;
    (text(value, "contractVersion")? == TOMBSTONE_VERSION).then_some(())?;
    for key in [
        "reconciliationSha256",
        "registrationSha256",
        "terminalResultSha256",
    ] {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    Some(())
}

/// Validate the owner-projected terminal row retained for retirement.
fn validate_terminal_record(value: &JsonValue) -> Option<()> {
    exact_keys(value, TERMINAL_RECORD_FIELDS).then_some(())?;
    for key in TERMINAL_RECORD_FIELDS
        .iter()
        .copied()
        .filter(|key| key.ends_with("Sha256"))
    {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    instant(value, "actionCompletedAt")?;
    instant(value, "recordedAt")?;
    (text(value, "contractVersion")? == TERMINAL_RECORD_VERSION
        && valid_family(&text(value, "family")?)
        && matches!(
            text(value, "actionDisposition")?.as_str(),
            "operator_off_compensated"
                | "promoted_operator_off_unapplied"
                | "promoted_verified"
                | "promotion_failed_compensated"
                | "rejected_no_action"
                | "resource_refused_no_action"
                | "support_failed_no_action"
        )
        && positive_integer(value, "metadataGeneration").is_some()
        && positive_integer(value, "finalizedPredictionCount").is_some())
    .then_some(())
}

/// Build the exact source unsupported terminal transaction after authority validation.
fn unsupported_record_sql(request: &JsonValue, _request_sha256: &str) -> Option<String> {
    let proof = field(request, "unsupportedProof")?;
    let shadow = field(request, "shadowRegistration")?;
    let native = field(request, "nativeAccess")?;
    let result = field(request, "localResult")?;
    let receipt = field(request, "actionReceipt")?;
    let completed_at = text(receipt, "completedAt")?;
    let maintenance_anchor_sha256 = canonical_digest(proof)?;
    let terminal_result_sha256 = canonical_digest(result)?;
    let previous = field(request, "previousTombstone")?;
    let (previous_guard, previous_setting) = previous_sql(previous)?;
    let row = owner_terminal_row_sql("terminal");
    Some(format!(
        "BEGIN;\nSET LOCAL statement_timeout = '30s';\nSET LOCAL lock_timeout = '5s';\nSET LOCAL weather.adjustment_maintenance_anchor_kind = 'terminal_action_reconciliation';\nSET LOCAL weather.adjustment_maintenance_anchor_sha256 = '{maintenance_anchor_sha256}';\nSET LOCAL weather.adjustment_terminal_member_root_sha256 = '{member_root}';\nDO $terminal$\nDECLARE registration adjustment_shadow_registrations_v2%ROWTYPE;\n  access adjustment_confirmation_accesses_v2%ROWTYPE; previous adjustment_shadow_terminal_results_v2%ROWTYPE;\n  reconciliation text; argument jsonb;\nBEGIN\n  PERFORM pg_advisory_xact_lock(hashtextextended('{registration}', 0));\n  SELECT * INTO STRICT registration FROM adjustment_shadow_registrations_v2\n    WHERE registration_sha256 = '{registration}' FOR UPDATE;\n  SELECT * INTO STRICT access FROM adjustment_confirmation_accesses_v2\n    WHERE registration_sha256 = registration.registration_sha256;\n  -- require the real owner access and current registered candidate rather than claimed hashes\n  IF registration.family <> '{family}' OR registration.candidate_sha256 <> '{candidate}'\n    OR registration.artifact_sha256 <> '{artifact}' OR registration.source_sha256 <> '{source}'\n    OR registration.reserved_key_sha256 <> '{reserved}'\n    OR access.access_sha256 <> '{access}' OR access.journal_head_sha256 <> '{journal}'\n    OR access.maintenance_anchor_sha256 <> '{access_anchor}'\n    OR access.gate_manifest_sha256 <> '{gate}'\n    OR access.eligible_prediction_set_sha256 <> '{eligible}'\n    OR access.expected_key_set_sha256 <> '{expected}'\n    OR access.metadata_root_sha256 <> '{metadata}'\n    OR access.target_comparator_snapshot_root_sha256 <> '{comparator}'\n    OR access.revision_catalog_watermark_sha256 <> '{watermark}'\n    OR access.target_cutoff_at <> '{cutoff}'::timestamptz\n    OR date_trunc('milliseconds', access.accessed_at) <> '{accessed}'::timestamptz THEN\n    RAISE EXCEPTION 'terminal record actual owner access differs';\n  END IF;\n  SELECT * INTO previous FROM adjustment_shadow_terminal_results_v2 WHERE family = registration.family;\n  -- rotate only the exact prior tombstone already retained in the terminal graph\n  IF {previous_guard} THEN RAISE EXCEPTION 'terminal record predecessor differs'; END IF;\n  {previous_setting}\n  reconciliation := encode(sha256(convert_to('adjustment-shadow-terminal-reconciliation/v2' || E'\\n'\n    || registration.registration_sha256 || E'\\n' || registration.family || E'\\n' || registration.candidate_sha256 || E'\\n'\n    || registration.source_sha256 || E'\\n' || registration.reserved_key_sha256 || E'\\n{member_root}\\n{terminal_result_sha256}\\n'\n    || access.access_sha256 || E'\\n{action}\\nsupport_failed_no_action\\n{completed_at}\\n'\n    || registration.finalized_metadata_root_sha256 || E'\\n' || registration.metadata_generation::text || E'\\n'\n    || registration.finalized_prediction_count::text || E'\\n{maintenance_anchor_sha256}', 'UTF8')), 'hex');\n  argument := jsonb_build_object('actionCompletedAt', '{completed_at}', 'actionDisposition', 'support_failed_no_action',\n    'actionSha256', '{action}', 'maintenanceAnchorSha256', '{maintenance_anchor_sha256}',\n    'reconciliationSha256', reconciliation, 'registrationSha256', registration.registration_sha256,\n    'terminalMemberSha256', '{member_root}', 'terminalResultSha256', '{terminal_result_sha256}');\n  PERFORM weather_record_adjustment_shadow_terminal_result_v2(argument);\nEND;\n$terminal$;\nSELECT json_build_object('contractVersion', 'adjustment-shadow-terminal-record-result/v3',\n  'reconciliationSha256', terminal.reconciliation_sha256, 'registrationSha256', terminal.registration_sha256,\n  'state', 'recorded', 'terminalRecord', {row})\nFROM adjustment_shadow_terminal_results_v2 terminal JOIN adjustment_shadow_registrations_v2 registration USING (registration_sha256)\nWHERE terminal.registration_sha256 = '{registration}';\nCOMMIT;\n",
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

/// Build the exact source retirement transaction without consulting an absent shadow field.
fn retirement_sql(request: &JsonValue, request_sha256: &str) -> Option<String> {
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

/// Render the fixed terminal result projection for one trusted SQL alias.
fn owner_terminal_row_sql(alias: &str) -> String {
    format!(
        "jsonb_build_object('accessSha256', {alias}.confirmation_access_sha256, 'actionCompletedAt', to_char({alias}.action_completed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'actionDisposition', {alias}.action_disposition, 'actionSha256', {alias}.action_sha256, 'candidateSha256', registration.candidate_sha256, 'contractVersion', 'adjustment-shadow-terminal-record/v3', 'family', {alias}.family, 'finalizedMetadataRootSha256', {alias}.metadata_root_sha256, 'finalizedPredictionCount', {alias}.finalized_prediction_count, 'maintenanceAnchorSha256', {alias}.maintenance_anchor_sha256, 'metadataGeneration', {alias}.metadata_generation, 'recordedAt', to_char({alias}.recorded_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'reconciliationSha256', {alias}.reconciliation_sha256, 'registrationSha256', {alias}.registration_sha256, 'reservedKeySha256', {alias}.reserved_key_sha256, 'sourceSha256', {alias}.source_sha256, 'terminalMemberSha256', {alias}.terminal_member_sha256, 'terminalResultSha256', {alias}.terminal_result_sha256)"
    )
}

/// Derive the exact predecessor guard and retained pointer setting.
fn previous_sql(previous: &JsonValue) -> Option<(String, String)> {
    // absence requires the database to have no same-registration predecessor
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

/// Return the family selected only by the already validated request grammar.
fn request_family(kind: NativeHostOwnerUnsupportedKind, request: &JsonValue) -> Option<String> {
    let source = match kind {
        NativeHostOwnerUnsupportedKind::Record => field(request, "shadowRegistration")?,
        NativeHostOwnerUnsupportedKind::Retire => field(request, "terminalRecord")?,
    };
    let family = text(source, "family")?;
    valid_family(&family).then_some(family)
}

/// Retain exact catalog bytes already admitted by the current-family owner.
struct CatalogCapturingFiles<'a, Files> {
    inner: &'a mut Files,
    catalog: Option<Vec<u8>>,
}

impl<'a, Files> CatalogCapturingFiles<'a, Files> {
    /// Wrap one installed file owner without a duplicate catalog read.
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
    /// Forward the exact current release marker.
    fn read_current_release(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        self.inner.read_current_release()
    }

    /// Forward one exact release environment read.
    fn read_environment(
        &mut self,
        release: &str,
    ) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        self.inner.read_environment(release)
    }

    /// Forward the independently owned settings read.
    fn read_settings(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        self.inner.read_settings()
    }

    /// Capture the exact catalog bytes already used by the current authority.
    fn read_catalog(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        let bytes = self.inner.read_catalog()?;
        self.catalog = Some(bytes.clone());
        Ok(bytes)
    }

    /// Forward the optional completed family authority read.
    fn read_current_authority(
        &mut self,
    ) -> Result<Option<Vec<u8>>, NativeFamilyReleaseCurrentError> {
        self.inner.read_current_authority()
    }
}

/// Map current-family catalog failures separately from other installed state.
fn map_family_error(error: NativeFamilyReleaseCurrentError) -> NativeHostOwnerTerminalError {
    match error {
        NativeFamilyReleaseCurrentError::Catalog => NativeHostOwnerTerminalError::Catalog,
        _ => NativeHostOwnerTerminalError::Family,
    }
}

/// Encode one value as source canonical JSON without its final LF for SQL jsonb literals.
fn canonical_literal(value: &JsonValue) -> Option<String> {
    let mut bytes = NativeWirePrimitives.encode_sorted_json(value)?;
    (bytes.pop() == Some(b'\n')).then_some(())?;
    String::from_utf8(bytes).ok()
}

/// Hash one exact byte sequence under the source SHA-256 spelling.
fn digest(bytes: &[u8]) -> String {
    format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        bytes,
    ))
}

/// Hash one source-canonical LF-terminated JSON value.
pub(crate) fn canonical_digest(value: &JsonValue) -> Option<String> {
    Some(digest(&NativeWirePrimitives.encode_sorted_json(value)?))
}

/// Parse one exact canonical clock from a selected object field.
fn instant(value: &JsonValue, key: &str) -> Option<i64> {
    NativeWirePrimitives.canonical_utc_milliseconds(&text(value, key)?)
}

/// Admit only the three closed adjustment families.
fn valid_family(value: &str) -> bool {
    matches!(value, "temperature" | "wind" | "rain")
}

/// Admit only exact lowercase fixed-width hexadecimal material.
fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Admit only source 20xx local-date labels without normalization.
fn valid_local_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[0] == b'2'
        && bytes[1] == b'0'
        && bytes[2].is_ascii_digit()
        && bytes[3].is_ascii_digit()
        && bytes[4] == b'-'
        && bytes[5].is_ascii_digit()
        && bytes[6].is_ascii_digit()
        && bytes[7] == b'-'
        && bytes[8].is_ascii_digit()
        && bytes[9].is_ascii_digit()
}

/// Decode one positive JavaScript-safe integer field.
fn positive_integer(value: &JsonValue, key: &str) -> Option<u64> {
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    (*number >= 1.0 && *number <= 9_007_199_254_740_991.0 && number.fract() == 0.0)
        .then_some(*number as u64)
}

#[cfg(test)]
#[path = "native_host_owner_terminal_test.rs"]
mod tests;
