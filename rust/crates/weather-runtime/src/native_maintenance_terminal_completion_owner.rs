//! Exact unsupported and no-action owner retirement after an admitted terminal record.

use std::fmt::{self, Display, Formatter};

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_policy_comparison::{object, string};
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceCaptureFamily, NativeMaintenanceDueAttemptJournal,
    NativeMaintenanceDueAttemptJournalError, NativeMaintenanceDueTerminalOutcome,
    NativeMaintenanceDueTerminalReason, NativeMaintenanceDueTerminalRemoteError,
    NativeMaintenanceDueTerminalRemotePort, NativeMaintenanceDueTerminalRetirement,
    NativeMaintenanceDueTerminalRetirementKind, NativeMaintenanceRecordDueTerminalOutcome,
    NativeMaintenanceRecordDueTerminalRetirement, NativeWirePrimitives,
    reconcile_native_maintenance_due_terminal_retirement,
};

const TERMINAL_RECORD_VERSION: &str = "adjustment-shadow-terminal-record/v3";
const TOMBSTONE_VERSION: &str = "adjustment-shadow-terminal-tombstone/v3";
const FINALIZATION_PROOF_VERSION: &str = "adjustment-maintenance-finalization-proof/v3";
const UNSUPPORTED_PROOF_VERSION: &str = "adjustment-maintenance-unsupported-terminal-proof/v1";

const TERMINAL_RECORD_KEYS: &[&str] = &[
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
const MAXIMUM_POPULATION: u64 = 9_500_000;
const INPUT_CLASS_NAMES: &[&str] = &[
    "actual_best_match",
    "artifact",
    "candidate",
    "comparator",
    "native_source",
    "rain_gate_input",
    "shadow_body",
    "shadow_source",
    "target",
    "target_revision",
];
const FINALIZATION_PROOF_KEYS: &[&str] = &[
    "actionSha256",
    "archiveCommitOrdinal",
    "burnSha256",
    "candidateArtifactRootSha256",
    "candidateReportSha256",
    "captureEpochWitnessSha256",
    "confirmationAccessSha256",
    "confirmationChunkCount",
    "contractVersion",
    "controlSha256",
    "controlVersion",
    "custodyCheckpointSha256",
    "dueKey",
    "family",
    "finalizedAt",
    "frontierSha256",
    "fullGraphVerifiedAt",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "inputSealSha256",
    "lifecycleLedgerRootSha256",
    "pageSha256",
    "policyReportSha256",
    "predecessorAnchorSha256",
    "publishedAt",
    "requiredInputRootSha256",
    "sequence",
    "sourceCommit",
    "transferredAnchorSha256",
    "workstationJournalHeadSha256",
];
const FINALIZATION_HASH_KEYS: &[&str] = &[
    "actionSha256",
    "burnSha256",
    "candidateArtifactRootSha256",
    "candidateReportSha256",
    "captureEpochWitnessSha256",
    "confirmationAccessSha256",
    "controlSha256",
    "custodyCheckpointSha256",
    "frontierSha256",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "inputSealSha256",
    "lifecycleLedgerRootSha256",
    "pageSha256",
    "policyReportSha256",
    "requiredInputRootSha256",
    "transferredAnchorSha256",
    "workstationJournalHeadSha256",
];
const UNSUPPORTED_PROOF_KEYS: &[&str] = &[
    "actionSha256",
    "archiveCommitOrdinal",
    "burnSha256",
    "candidateReportSha256",
    "captureEpochWitnessSha256",
    "confirmationAccessSha256",
    "confirmationChunkCount",
    "contractVersion",
    "controlSha256",
    "controlVersion",
    "custodyCheckpointSha256",
    "dueKey",
    "eligiblePredictionSetSha256",
    "expectedKeySetSha256",
    "family",
    "finalizedAt",
    "frontierSha256",
    "fullGraphVerifiedAt",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "inputClasses",
    "lifecycleLedgerRootSha256",
    "missingClassNames",
    "missingKeyCount",
    "missingKeySetSha256",
    "pageSha256",
    "policyReportSha256",
    "predecessorProofSha256",
    "registrationSha256",
    "requiredInputRootSha256",
    "sequence",
    "sourceCommit",
    "unsupportedReason",
    "workstationJournalHeadSha256",
];
const UNSUPPORTED_HASH_KEYS: &[&str] = &[
    "actionSha256",
    "burnSha256",
    "candidateReportSha256",
    "captureEpochWitnessSha256",
    "confirmationAccessSha256",
    "controlSha256",
    "custodyCheckpointSha256",
    "eligiblePredictionSetSha256",
    "expectedKeySetSha256",
    "frontierSha256",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "lifecycleLedgerRootSha256",
    "missingKeySetSha256",
    "pageSha256",
    "policyReportSha256",
    "registrationSha256",
    "requiredInputRootSha256",
    "workstationJournalHeadSha256",
];

/// Closed terminal classes retained by the original owner after an actual result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceOwnerTerminalClass {
    Unsupported,
    Rejected,
    ResourceRefused,
    SupportFailed,
    PromotedVerified { serving_changed: bool },
    PromotedOperatorOffUnapplied,
    OperatorOffCompensated,
    PromotionFailedCompensated,
}

impl NativeMaintenanceOwnerTerminalClass {
    /// Return the exact due-outcome reason for this terminal class.
    const fn reason(self) -> NativeMaintenanceDueTerminalReason {
        match self {
            Self::Rejected => NativeMaintenanceDueTerminalReason::DailyCandidateRejected,
            Self::ResourceRefused => NativeMaintenanceDueTerminalReason::DailySupportFailed,
            Self::PromotedVerified { .. }
            | Self::PromotedOperatorOffUnapplied
            | Self::OperatorOffCompensated
            | Self::PromotionFailedCompensated => {
                NativeMaintenanceDueTerminalReason::DailyCandidatePromoted
            }
            Self::Unsupported | Self::SupportFailed => {
                NativeMaintenanceDueTerminalReason::DailySupportFailed
            }
        }
    }

    /// Return the exact owner row disposition for this terminal class.
    const fn record_disposition(self) -> &'static str {
        match self {
            Self::Rejected => "rejected_no_action",
            Self::ResourceRefused => "resource_refused_no_action",
            Self::PromotedVerified { .. } => "promoted_verified",
            Self::PromotedOperatorOffUnapplied => "promoted_operator_off_unapplied",
            Self::OperatorOffCompensated => "operator_off_compensated",
            Self::PromotionFailedCompensated => "promotion_failed_compensated",
            Self::Unsupported | Self::SupportFailed => "support_failed_no_action",
        }
    }

    /// Return the exact forced retirement class.
    const fn retirement_kind(self) -> NativeMaintenanceDueTerminalRetirementKind {
        match self {
            Self::Unsupported => NativeMaintenanceDueTerminalRetirementKind::UnsupportedV1,
            Self::Rejected
            | Self::ResourceRefused
            | Self::SupportFailed
            | Self::PromotedVerified { .. }
            | Self::PromotedOperatorOffUnapplied
            | Self::OperatorOffCompensated
            | Self::PromotionFailedCompensated => {
                NativeMaintenanceDueTerminalRetirementKind::QualifiedV3
            }
        }
    }
}

/// Borrowed source documents after the remote owner has produced the terminal row.
pub struct NativeMaintenanceOwnerTerminalInput<'a> {
    pub authority_proof: &'a JsonValue,
    pub class: NativeMaintenanceOwnerTerminalClass,
    pub due_key: &'a str,
    pub now: &'a str,
    pub semantic_input_sha256: &'a str,
    pub terminal_record: &'a JsonValue,
}

/// Exact immutable tombstone, retirement request, and value-free due outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceOwnerTerminalMaterial {
    pub family: NativeMaintenanceCaptureFamily,
    pub outcome: NativeMaintenanceDueTerminalOutcome,
    pub retirement_kind: NativeMaintenanceDueTerminalRetirementKind,
    pub retirement_request: JsonValue,
    pub retirement_request_bytes: Vec<u8>,
    pub retirement_request_sha256: String,
    pub tombstone: JsonValue,
    pub tombstone_bytes: Vec<u8>,
    pub tombstone_sha256: String,
}

/// Fixed archive and lifecycle-journal effects required before owner retirement.
pub trait NativeMaintenanceOwnerTombstonePort {
    type Error;

    /// Publish the immutable tombstone and then retain its family pointer in the journal.
    fn publish_and_record_tombstone(
        &mut self,
        family: NativeMaintenanceCaptureFamily,
        now: &str,
        tombstone_sha256: &str,
        tombstone_bytes: &[u8],
        tombstone: &JsonValue,
    ) -> Result<(), Self::Error>;
}

/// Complete material plus the actual acknowledged retirement state.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceOwnerTerminalCompletion {
    pub material: NativeMaintenanceOwnerTerminalMaterial,
    pub retirement: NativeMaintenanceDueTerminalRetirement,
}

/// Closed owner-material, archive, journal, and forced-transport failures.
#[derive(Debug)]
pub enum NativeMaintenanceOwnerTerminalError<ArchiveError> {
    Material,
    Archive(ArchiveError),
    Journal(NativeMaintenanceDueAttemptJournalError),
    Remote(NativeMaintenanceDueTerminalRemoteError),
}

impl<ArchiveError> Display for NativeMaintenanceOwnerTerminalError<ArchiveError> {
    /// Render one bounded owner-completion failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Material => "maintenance owner terminal material differs",
            Self::Archive(_) => "maintenance owner terminal tombstone publication failed",
            Self::Journal(_) => "maintenance owner terminal journal failed",
            Self::Remote(_) => "maintenance owner terminal retirement failed",
        })
    }
}

impl<ArchiveError: fmt::Debug> std::error::Error
    for NativeMaintenanceOwnerTerminalError<ArchiveError>
{
}

/// Build the exact tombstone and disjoint unsupported or no-action retirement request.
pub fn build_native_maintenance_owner_terminal_material(
    input: NativeMaintenanceOwnerTerminalInput<'_>,
) -> Result<NativeMaintenanceOwnerTerminalMaterial, NativeMaintenanceOwnerTerminalError<()>> {
    use NativeMaintenanceOwnerTerminalError::Material;

    let family = validate_terminal_record(input.terminal_record).ok_or(Material)?;
    let now = NativeWirePrimitives
        .canonical_utc_milliseconds(input.now)
        .ok_or(Material)?;
    let recorded_at = instant(input.terminal_record, "recordedAt").ok_or(Material)?;
    // prohibit relabelling one owner result as another terminal class
    if text(input.terminal_record, "actionDisposition").as_deref()
        != Some(input.class.record_disposition())
        || !valid_hash(input.semantic_input_sha256)
        || !valid_daily_due_key(input.due_key)
        || now < recorded_at
    {
        return Err(Material);
    }
    let registration_sha256 = text(input.terminal_record, "registrationSha256").ok_or(Material)?;
    let tombstone = object(vec![
        ("contractVersion", string(TOMBSTONE_VERSION)),
        (
            "reconciliationSha256",
            string(&text(input.terminal_record, "reconciliationSha256").ok_or(Material)?),
        ),
        ("registrationSha256", string(&registration_sha256)),
        (
            "terminalResultSha256",
            string(&text(input.terminal_record, "terminalResultSha256").ok_or(Material)?),
        ),
    ]);
    let mut wire = NativeWirePrimitives;
    let tombstone_bytes = wire.encode_canonical_json(&tombstone).ok_or(Material)?;
    let tombstone_sha256 = MaintenanceCanonicalJson::sha256(&mut wire, &tombstone_bytes)
        .ok_or(Material)?
        .as_str()
        .to_owned();
    let retirement_request = match input.class {
        NativeMaintenanceOwnerTerminalClass::Unsupported => {
            validate_unsupported_proof(
                input.authority_proof,
                input.terminal_record,
                &registration_sha256,
            )
            .ok_or(Material)?;
            object(vec![
                (
                    "contractVersion",
                    string("adjustment-shadow-unsupported-terminal-retirement-request/v1"),
                ),
                ("registrationSha256", string(&registration_sha256)),
                ("terminalRecord", input.terminal_record.clone()),
                ("terminalTombstone", tombstone.clone()),
                ("unsupportedProof", input.authority_proof.clone()),
            ])
        }
        NativeMaintenanceOwnerTerminalClass::Rejected
        | NativeMaintenanceOwnerTerminalClass::ResourceRefused
        | NativeMaintenanceOwnerTerminalClass::SupportFailed
        | NativeMaintenanceOwnerTerminalClass::PromotedVerified { .. }
        | NativeMaintenanceOwnerTerminalClass::PromotedOperatorOffUnapplied
        | NativeMaintenanceOwnerTerminalClass::OperatorOffCompensated
        | NativeMaintenanceOwnerTerminalClass::PromotionFailedCompensated => {
            validate_finalization_proof(
                input.authority_proof,
                input.terminal_record,
                &registration_sha256,
            )
            .ok_or(Material)?;
            object(vec![
                (
                    "contractVersion",
                    string("adjustment-shadow-terminal-retirement-request/v3"),
                ),
                ("finalizationProof", input.authority_proof.clone()),
                ("registrationSha256", string(&registration_sha256)),
                ("terminalRecord", input.terminal_record.clone()),
                ("terminalTombstone", tombstone.clone()),
            ])
        }
    };
    let retirement_request_bytes = wire
        .encode_canonical_json(&retirement_request)
        .ok_or(Material)?;
    let retirement_request_sha256 =
        MaintenanceCanonicalJson::sha256(&mut wire, &retirement_request_bytes)
            .ok_or(Material)?
            .as_str()
            .to_owned();
    let outcome = NativeMaintenanceDueTerminalOutcome::completed(
        input.class.reason(),
        input.semantic_input_sha256.to_owned(),
        matches!(
            input.class,
            NativeMaintenanceOwnerTerminalClass::PromotedVerified {
                serving_changed: true
            }
        ),
    )
    .ok_or(Material)?;
    Ok(NativeMaintenanceOwnerTerminalMaterial {
        family,
        outcome,
        retirement_kind: input.class.retirement_kind(),
        retirement_request,
        retirement_request_bytes,
        retirement_request_sha256,
        tombstone,
        tombstone_bytes,
        tombstone_sha256,
    })
}

/// Publish, retain, send, and acknowledge one exact unsupported or no-action retirement.
pub fn complete_native_maintenance_owner_terminal<Archive>(
    input: NativeMaintenanceOwnerTerminalInput<'_>,
    archive: &mut Archive,
    journal: &mut NativeMaintenanceDueAttemptJournal<'_>,
    remote: &mut impl NativeMaintenanceDueTerminalRemotePort,
) -> Result<
    NativeMaintenanceOwnerTerminalCompletion,
    NativeMaintenanceOwnerTerminalError<Archive::Error>,
>
where
    Archive: NativeMaintenanceOwnerTombstonePort,
{
    use NativeMaintenanceOwnerTerminalError as Error;

    let due_key = input.due_key.to_owned();
    let now = input.now.to_owned();
    let material =
        build_native_maintenance_owner_terminal_material(input).map_err(|_| Error::Material)?;
    archive
        .publish_and_record_tombstone(
            material.family,
            &now,
            &material.tombstone_sha256,
            &material.tombstone_bytes,
            &material.tombstone,
        )
        .map_err(Error::Archive)?;
    journal
        .record_due_terminal_outcome(NativeMaintenanceRecordDueTerminalOutcome {
            due_key: &due_key,
            now: &now,
            outcome: &material.outcome,
        })
        .map_err(Error::Journal)?;
    let retained = journal
        .record_due_terminal_retirement(NativeMaintenanceRecordDueTerminalRetirement {
            due_key: &due_key,
            kind: material.retirement_kind,
            now: &now,
            request: &material.retirement_request,
        })
        .map_err(Error::Journal)?;
    // never resend a command already acknowledged by the exact durable request identity
    let retirement = if retained.completed {
        retained
    } else {
        reconcile_native_maintenance_due_terminal_retirement(journal, remote, &due_key, &now)
            .map_err(Error::Remote)?
    };
    Ok(NativeMaintenanceOwnerTerminalCompletion {
        material,
        retirement,
    })
}

/// Validate one owner terminal row and return its closed family.
pub(crate) fn validate_terminal_record(
    value: &JsonValue,
) -> Option<NativeMaintenanceCaptureFamily> {
    exact_keys(value, TERMINAL_RECORD_KEYS).then_some(())?;
    // bind every terminal row identity without accepting aliases
    for key in TERMINAL_RECORD_KEYS
        .iter()
        .filter(|key| key.ends_with("Sha256"))
    {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    instant(value, "actionCompletedAt")?;
    instant(value, "recordedAt")?;
    (text(value, "contractVersion")? == TERMINAL_RECORD_VERSION).then_some(())?;
    integer(value, "metadataGeneration", 1, 9_007_199_254_740_991)?;
    integer(value, "finalizedPredictionCount", 1, 9_007_199_254_740_991)?;
    match text(value, "family")?.as_str() {
        "temperature" => Some(NativeMaintenanceCaptureFamily::Temperature),
        "wind" => Some(NativeMaintenanceCaptureFamily::Wind),
        "rain" => Some(NativeMaintenanceCaptureFamily::Rain),
        _ => None,
    }
}

/// Validate the complete future-only finalization proof and terminal-row binding.
fn validate_finalization_proof(
    value: &JsonValue,
    record: &JsonValue,
    registration_sha256: &str,
) -> Option<()> {
    exact_keys(value, FINALIZATION_PROOF_KEYS).then_some(())?;
    (text(value, "contractVersion")? == FINALIZATION_PROOF_VERSION).then_some(())?;
    let family = text(value, "family")?;
    valid_family(&family).then_some(())?;
    valid_confirmation_due(&text(value, "dueKey")?, &family).then_some(())?;
    let expected_chunks = if family == "rain" { 24 } else { 27 };
    (integer(
        value,
        "confirmationChunkCount",
        expected_chunks,
        expected_chunks,
    )? == expected_chunks)
        .then_some(())?;
    valid_control_version(&text(value, "controlVersion")?).then_some(())?;
    valid_lower_hex(&text(value, "sourceCommit")?, 40).then_some(())?;
    uint64_text(value, "archiveCommitOrdinal")?;
    let sequence = uint64_text(value, "sequence")?;
    let verified = instant(value, "fullGraphVerifiedAt")?;
    let published = instant(value, "publishedAt")?;
    let finalized = instant(value, "finalizedAt")?;
    (verified <= published && published <= finalized).then_some(())?;
    // validate every authority identity under the finalization contract
    for key in FINALIZATION_HASH_KEYS {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    let predecessor_absent = nullable_hash(value, "predecessorAnchorSha256")?;
    ((sequence == 0) == predecessor_absent).then_some(())?;
    // bind the exact finalized action and member roots to the owner row
    (text(record, "registrationSha256")?.as_str() == registration_sha256
        && text(value, "actionSha256")? == text(record, "actionSha256")?
        && text(value, "confirmationAccessSha256")? == text(record, "accessSha256")?
        && family == text(record, "family")?
        && text(value, "fullMemberRootSha256")? == text(record, "terminalMemberSha256")?
        && text(value, "transferredAnchorSha256")? == text(record, "maintenanceAnchorSha256")?)
    .then_some(())
}

/// Validate the complete unsupported proof and terminal-row binding.
fn validate_unsupported_proof(
    value: &JsonValue,
    record: &JsonValue,
    registration_sha256: &str,
) -> Option<()> {
    exact_keys(value, UNSUPPORTED_PROOF_KEYS).then_some(())?;
    (text(value, "contractVersion")? == UNSUPPORTED_PROOF_VERSION).then_some(())?;
    let family = text(value, "family")?;
    valid_family(&family).then_some(())?;
    let due_key = text(value, "dueKey")?;
    valid_confirmation_due(&due_key, &family).then_some(())?;
    let candidate_sha256 = due_key.rsplit_once('/')?.1;
    let expected_chunks = if family == "rain" { 24 } else { 27 };
    (integer(
        value,
        "confirmationChunkCount",
        expected_chunks,
        expected_chunks,
    )? == expected_chunks)
        .then_some(())?;
    matches!(
        text(value, "unsupportedReason")?.as_str(),
        "permanent_capture_gap" | "permanent_source_gap" | "permanent_target_gap"
    )
    .then_some(())?;
    valid_control_version(&text(value, "controlVersion")?).then_some(())?;
    valid_lower_hex(&text(value, "sourceCommit")?, 40).then_some(())?;
    uint64_text(value, "archiveCommitOrdinal")?;
    let sequence = uint64_text(value, "sequence")?;
    let verified = instant(value, "fullGraphVerifiedAt")?;
    let finalized = instant(value, "finalizedAt")?;
    (verified <= finalized).then_some(())?;
    // validate every terminal, graph, custody, and population digest
    for key in UNSUPPORTED_HASH_KEYS {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    let predecessor_absent = nullable_hash(value, "predecessorProofSha256")?;
    ((sequence == 0) == predecessor_absent).then_some(())?;
    integer(value, "missingKeyCount", 1, MAXIMUM_POPULATION)?;
    let classes = field(value, "inputClasses")?;
    exact_keys(classes, INPUT_CLASS_NAMES).then_some(())?;
    let empty_root = canonical_identity(&JsonValue::Array(Vec::new()))?;
    let mut missing = Vec::new();
    let mut artifact_count = None;
    let mut candidate_count = None;
    let mut candidate_root = None;
    // preserve exact class order and derive the same missing-class projection
    for name in INPUT_CLASS_NAMES {
        let entry = field(classes, name)?;
        exact_keys(entry, &["count", "rootSha256"]).then_some(())?;
        let count = integer(entry, "count", 0, MAXIMUM_POPULATION)?;
        let root = text(entry, "rootSha256")?;
        valid_hash(&root).then_some(())?;
        (count != 0 || root == empty_root).then_some(())?;
        // omit the intentional non-rain gate placeholder
        if count == 0 && (family == "rain" || *name != "rain_gate_input") {
            missing.push(string(name));
        }
        match *name {
            "artifact" => artifact_count = Some(count),
            "candidate" => {
                candidate_count = Some(count);
                candidate_root = Some(root);
            }
            _ => {}
        }
    }
    let expected_candidate_root =
        canonical_identity(&JsonValue::Array(vec![string(candidate_sha256)]))?;
    // bind proof population and disjoint terminal-row identities
    (artifact_count? >= 1
        && candidate_count? == 1
        && candidate_root? == expected_candidate_root
        && field(value, "missingClassNames")? == &JsonValue::Array(missing)
        && text(value, "requiredInputRootSha256")? == canonical_identity(classes)?
        && text(value, "registrationSha256")?.as_str() == registration_sha256
        && text(value, "actionSha256")? == text(record, "actionSha256")?
        && text(value, "confirmationAccessSha256")? == text(record, "accessSha256")?
        && family == text(record, "family")?
        && text(value, "fullMemberRootSha256")? == text(record, "terminalMemberSha256")?)
    .then_some(())
}

/// Hash one canonical JSON value through the approved native primitive.
fn canonical_identity(value: &JsonValue) -> Option<String> {
    let mut wire = NativeWirePrimitives;
    let bytes = wire.encode_canonical_json(value)?;
    MaintenanceCanonicalJson::sha256(&mut wire, &bytes).map(|digest| digest.as_str().to_owned())
}

/// Decode one canonical UTC instant to its sortable epoch value.
fn instant(value: &JsonValue, key: &str) -> Option<i64> {
    NativeWirePrimitives.canonical_utc_milliseconds(&text(value, key)?)
}

/// Decode one bounded safe JSON integer.
fn integer(value: &JsonValue, key: &str, minimum: u64, maximum: u64) -> Option<u64> {
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    (*number >= minimum as f64
        && *number <= maximum as f64
        && number.is_finite()
        && number.fract() == 0.0)
        .then_some(*number as u64)
}

/// Decode one canonical unsigned 64-bit decimal string.
fn uint64_text(value: &JsonValue, key: &str) -> Option<u64> {
    let text = text(value, key)?;
    ((!text.is_empty())
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0')))
    .then_some(())?;
    text.parse().ok()
}

/// Validate one nullable hash and report whether it is absent.
fn nullable_hash(value: &JsonValue, key: &str) -> Option<bool> {
    match field(value, key)? {
        JsonValue::Null => Some(true),
        JsonValue::String(units) => valid_hash(&String::from_utf16(units).ok()?).then_some(false),
        _ => None,
    }
}

/// Admit one closed maintenance family spelling.
fn valid_family(value: &str) -> bool {
    matches!(value, "temperature" | "wind" | "rain")
}

/// Bind one confirmation due key to its family and candidate identity.
fn valid_confirmation_due(value: &str, family: &str) -> bool {
    value
        .strip_prefix("confirmation/")
        .and_then(|suffix| suffix.split_once('/'))
        .is_some_and(|(due_family, candidate)| due_family == family && valid_hash(candidate))
}

/// Admit one exact daily due key without accepting rollover dates.
fn valid_daily_due_key(value: &str) -> bool {
    let Some(date) = value.strip_prefix("daily/") else {
        return false;
    };
    // bind the displayed date to the same canonical UTC calendar label
    date.len() == 10
        && NativeWirePrimitives
            .canonical_utc_milliseconds(&format!("{date}T00:00:00.000Z"))
            .and_then(|instant| NativeWirePrimitives.format_utc_milliseconds(instant))
            .is_some_and(|instant| instant.starts_with(date))
}

/// Admit the closed predecessor control-version spelling.
fn valid_control_version(value: &str) -> bool {
    (1..=6).contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && !value.starts_with('0')
}

/// Admit one exact lowercase hexadecimal identity width.
fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "native_maintenance_terminal_completion_owner_test.rs"]
mod tests;
