//! Exact retained due-terminal retirement reconciliation over the fixed owner transport.

use std::fmt::{self, Display, Formatter};
use std::io;

use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceCompleteDueTerminalRetirement,
    NativeMaintenanceDatabaseLedgerReader, NativeMaintenanceDueAttemptJournal,
    NativeMaintenanceDueAttemptJournalError, NativeMaintenanceDueTerminalRetirement,
    NativeMaintenanceDueTerminalRetirementKind, NativeWirePrimitives,
};

const QUALIFIED_OPERATION: &str = "adjustment-shadow-terminal-retire-v3";
const UNSUPPORTED_OPERATION: &str = "adjustment-shadow-unsupported-terminal-retire-v1";
const QUALIFIED_REQUEST_VERSION: &str = "adjustment-shadow-terminal-retirement-request/v3";
const UNSUPPORTED_REQUEST_VERSION: &str =
    "adjustment-shadow-unsupported-terminal-retirement-request/v1";
const RESPONSE_VERSION: &str = "adjustment-shadow-terminal-retirement-result/v3";
const TERMINAL_RECORD_VERSION: &str = "adjustment-shadow-terminal-record/v3";
const TERMINAL_TOMBSTONE_VERSION: &str = "adjustment-shadow-terminal-tombstone/v3";
const FINALIZATION_PROOF_VERSION: &str = "adjustment-maintenance-finalization-proof/v3";
const UNSUPPORTED_PROOF_VERSION: &str = "adjustment-maintenance-unsupported-terminal-proof/v1";
const MAXIMUM_POPULATION: u64 = 9_500_000;

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

/// The only two forced owner operations admitted by terminal reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueTerminalRemoteOperation {
    QualifiedV3,
    UnsupportedV1,
}

impl NativeMaintenanceDueTerminalRemoteOperation {
    /// Return the exact forced-command verb for this closed retirement class.
    pub const fn command(self) -> &'static str {
        match self {
            Self::QualifiedV3 => QUALIFIED_OPERATION,
            Self::UnsupportedV1 => UNSUPPORTED_OPERATION,
        }
    }
}

/// Narrow fixed transport for one canonical retirement request and identity operand.
pub trait NativeMaintenanceDueTerminalRemotePort {
    /// Send one exact owner operation through the retained forced command.
    fn retire(
        &mut self,
        operation: NativeMaintenanceDueTerminalRemoteOperation,
        request_sha256: &str,
        canonical_request: &[u8],
    ) -> io::Result<Vec<u8>>;
}

impl NativeMaintenanceDueTerminalRemotePort for NativeMaintenanceDatabaseLedgerReader {
    /// Reuse the production SSH owner transport without opening arbitrary verbs.
    fn retire(
        &mut self,
        operation: NativeMaintenanceDueTerminalRemoteOperation,
        request_sha256: &str,
        canonical_request: &[u8],
    ) -> io::Result<Vec<u8>> {
        self.request_document(
            operation.command(),
            &[request_sha256],
            Some(canonical_request),
        )
    }
}

/// Closed reconciliation, request, transport, response, and journal failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueTerminalRemoteError {
    Journal(NativeMaintenanceDueAttemptJournalError),
    Unavailable,
    Request,
    Transport,
    Response,
}

impl Display for NativeMaintenanceDueTerminalRemoteError {
    /// Render one path-free source failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Journal(_) => "due terminal retirement journal failed",
            Self::Unavailable => "daily terminal retirement is unavailable",
            Self::Request => "daily terminal retirement request is invalid",
            Self::Transport => "daily terminal retirement transport failed",
            Self::Response => "maintenance_stdin_response_invalid",
        })
    }
}

impl std::error::Error for NativeMaintenanceDueTerminalRemoteError {
    /// Preserve only an actual journal failure source.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // transport diagnostics remain private like the predecessor bridge
        match self {
            Self::Journal(error) => Some(error),
            _ => None,
        }
    }
}

impl From<NativeMaintenanceDueAttemptJournalError> for NativeMaintenanceDueTerminalRemoteError {
    /// Preserve the exact durable-journal refusal category.
    fn from(error: NativeMaintenanceDueAttemptJournalError) -> Self {
        Self::Journal(error)
    }
}

/// Resume one retained owner retirement and complete it only after an exact acknowledgement.
pub fn reconcile_native_maintenance_due_terminal_retirement(
    journal: &mut NativeMaintenanceDueAttemptJournal<'_>,
    remote: &mut impl NativeMaintenanceDueTerminalRemotePort,
    due_key: &str,
    now: &str,
) -> Result<NativeMaintenanceDueTerminalRetirement, NativeMaintenanceDueTerminalRemoteError> {
    use NativeMaintenanceDueTerminalRemoteError as Error;
    let retained = journal
        .read_due_terminal_retirement(due_key)?
        .ok_or(Error::Unavailable)?;
    // exact completed retries do not consult clock or remote state
    if retained.completed {
        return Ok(retained);
    }
    let operation = match retained.kind {
        NativeMaintenanceDueTerminalRetirementKind::QualifiedV3 => {
            validate_qualified_request(&retained.request).ok_or(Error::Request)?;
            NativeMaintenanceDueTerminalRemoteOperation::QualifiedV3
        }
        NativeMaintenanceDueTerminalRetirementKind::UnsupportedV1 => {
            validate_unsupported_request(&retained.request).ok_or(Error::Request)?;
            NativeMaintenanceDueTerminalRemoteOperation::UnsupportedV1
        }
    };
    let mut wire = NativeWirePrimitives;
    let request_bytes = wire
        .encode_canonical_json(&retained.request)
        .ok_or(Error::Request)?;
    let request_sha256 =
        MaintenanceCanonicalJson::sha256(&mut wire, &request_bytes).ok_or(Error::Request)?;
    // refuse a retained request whose nested identity no longer matches its canonical bytes
    if request_sha256.as_str() != retained.request_sha256 {
        return Err(Error::Request);
    }
    let response_bytes = remote
        .retire(operation, request_sha256.as_str(), &request_bytes)
        .map_err(|_| Error::Transport)?;
    let response = wire
        .parse_canonical_object(&response_bytes)
        .ok_or(Error::Response)?;
    validate_response(&response, &retained.request).ok_or(Error::Response)?;
    journal.complete_due_terminal_retirement(NativeMaintenanceCompleteDueTerminalRetirement {
        due_key,
        now,
        request_sha256: request_sha256.as_str(),
    })?;
    Ok(NativeMaintenanceDueTerminalRetirement {
        completed: true,
        ..retained
    })
}

/// Validate one exact qualified retirement request and every cross-domain binding.
fn validate_qualified_request(value: &JsonValue) -> Option<()> {
    exact_keys(
        value,
        &[
            "contractVersion",
            "finalizationProof",
            "registrationSha256",
            "terminalRecord",
            "terminalTombstone",
        ],
    )
    .then_some(())?;
    (text(value, "contractVersion")? == QUALIFIED_REQUEST_VERSION).then_some(())?;
    let registration = text(value, "registrationSha256")?;
    valid_hash(&registration).then_some(())?;
    let proof = field(value, "finalizationProof")?;
    validate_finalization_proof(proof)?;
    let record = field(value, "terminalRecord")?;
    validate_terminal_record(record)?;
    let tombstone = field(value, "terminalTombstone")?;
    validate_terminal_tombstone(tombstone)?;
    // release only the registration represented by the finalized action and reconciliation
    (text(record, "registrationSha256")? == registration
        && text(tombstone, "registrationSha256")? == registration
        && text(tombstone, "reconciliationSha256")? == text(record, "reconciliationSha256")?
        && text(proof, "actionSha256")? == text(record, "actionSha256")?
        && text(proof, "confirmationAccessSha256")? == text(record, "accessSha256")?
        && text(proof, "family")? == text(record, "family")?
        && text(proof, "fullMemberRootSha256")? == text(record, "terminalMemberSha256")?
        && text(proof, "transferredAnchorSha256")? == text(record, "maintenanceAnchorSha256")?)
    .then_some(())
}

/// Validate one exact unsupported retirement request and its disjoint proof binding.
fn validate_unsupported_request(value: &JsonValue) -> Option<()> {
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
    (text(value, "contractVersion")? == UNSUPPORTED_REQUEST_VERSION).then_some(())?;
    let registration = text(value, "registrationSha256")?;
    valid_hash(&registration).then_some(())?;
    let proof = field(value, "unsupportedProof")?;
    validate_unsupported_proof(proof)?;
    let record = field(value, "terminalRecord")?;
    validate_terminal_record(record)?;
    let tombstone = field(value, "terminalTombstone")?;
    validate_terminal_tombstone(tombstone)?;
    let proof_sha256 = canonical_identity(proof)?;
    // preserve the proof-only authority and exact reconciled tombstone binding
    (text(record, "registrationSha256")? == registration
        && text(proof, "registrationSha256")? == registration
        && text(tombstone, "registrationSha256")? == registration
        && text(tombstone, "reconciliationSha256")? == text(record, "reconciliationSha256")?
        && text(proof, "actionSha256")? == text(record, "actionSha256")?
        && text(proof, "confirmationAccessSha256")? == text(record, "accessSha256")?
        && text(proof, "family")? == text(record, "family")?
        && text(proof, "fullMemberRootSha256")? == text(record, "terminalMemberSha256")?
        && proof_sha256 == text(record, "maintenanceAnchorSha256")?)
    .then_some(())
}

/// Validate the exact closed successful owner response and request binding.
fn validate_response(response: &JsonValue, request: &JsonValue) -> Option<()> {
    exact_keys(
        response,
        &[
            "contractVersion",
            "reconciliationSha256",
            "registrationSha256",
            "state",
        ],
    )
    .then_some(())?;
    (text(response, "contractVersion")? == RESPONSE_VERSION
        && text(response, "state")? == "retired")
        .then_some(())?;
    let registration = text(response, "registrationSha256")?;
    let reconciliation = text(response, "reconciliationSha256")?;
    (valid_hash(&registration) && valid_hash(&reconciliation)).then_some(())?;
    let terminal = field(request, "terminalRecord")?;
    (registration == text(request, "registrationSha256")?
        && reconciliation == text(terminal, "reconciliationSha256")?)
    .then_some(())
}

/// Validate the complete future-only finalization proof retained in a request.
pub(crate) fn validate_finalization_proof(value: &JsonValue) -> Option<()> {
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
    ((sequence == 0) == predecessor_absent).then_some(())
}

/// Validate the complete unsupported proof and its population-derived identities.
fn validate_unsupported_proof(value: &JsonValue) -> Option<()> {
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
        // retain only the source-required populated class evidence
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
    (artifact_count? >= 1
        && candidate_count? == 1
        && candidate_root? == expected_candidate_root
        && field(value, "missingClassNames")? == &JsonValue::Array(missing)
        && text(value, "requiredInputRootSha256")? == canonical_identity(classes)?)
    .then_some(())
}

/// Validate one owner-projected terminal row needed for retirement.
fn validate_terminal_record(value: &JsonValue) -> Option<()> {
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
    let family = text(value, "family")?;
    (text(value, "contractVersion")? == TERMINAL_RECORD_VERSION && valid_family(&family))
        .then_some(())?;
    matches!(
        text(value, "actionDisposition")?.as_str(),
        "operator_off_compensated"
            | "promoted_operator_off_unapplied"
            | "promoted_verified"
            | "promotion_failed_compensated"
            | "rejected_no_action"
            | "resource_refused_no_action"
            | "support_failed_no_action"
    )
    .then_some(())?;
    integer(value, "metadataGeneration", 1, 9_007_199_254_740_991)?;
    integer(value, "finalizedPredictionCount", 1, 9_007_199_254_740_991)?;
    Some(())
}

/// Validate one archived predecessor or current terminal tombstone.
fn validate_terminal_tombstone(value: &JsonValue) -> Option<()> {
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
    (text(value, "contractVersion")? == TERMINAL_TOMBSTONE_VERSION).then_some(())?;
    // validate the three immutable tombstone identities
    for key in [
        "reconciliationSha256",
        "registrationSha256",
        "terminalResultSha256",
    ] {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    Some(())
}

/// Hash one canonical JSON value through the approved native primitive.
fn canonical_identity(value: &JsonValue) -> Option<String> {
    let mut wire = NativeWirePrimitives;
    let bytes = wire.encode_canonical_json(value)?;
    MaintenanceCanonicalJson::sha256(&mut wire, &bytes).map(|digest| digest.as_str().to_owned())
}

/// Decode one exact canonical UTC instant to its sortable epoch value.
fn instant(value: &JsonValue, key: &str) -> Option<i64> {
    NativeWirePrimitives.canonical_utc_milliseconds(&text(value, key)?)
}

/// Decode one bounded nonnegative safe JSON integer.
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

/// Recognize one exact family spelling.
fn valid_family(value: &str) -> bool {
    matches!(value, "temperature" | "wind" | "rain")
}

/// Bind one confirmation due key to its family and candidate identity.
fn valid_confirmation_due(value: &str, family: &str) -> bool {
    let Some(remainder) = value.strip_prefix("confirmation/") else {
        return false;
    };
    let Some((due_family, digest)) = remainder.split_once('/') else {
        return false;
    };
    due_family == family && valid_hash(digest) && !digest.contains('/')
}

/// Recognize the source's positive six-digit control version.
fn valid_control_version(value: &str) -> bool {
    (1..=6).contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && !value.starts_with('0')
}

/// Recognize one fixed-length lowercase hexadecimal value.
fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Build one source JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_due_terminal_remote_test.rs"]
mod tests;
