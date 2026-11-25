//! Read-only native owner for the predecessor's current unsupported-terminal proof.

use std::fmt::{Display, Formatter};
use std::fs;
use std::path::PathBuf;

use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::MaintenanceCanonicalJson;
use crate::maintenance_anchor_status::{read_private_anchor_slot, verified_private_anchor_root};
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};

const EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";
const PROOF_DIRECTORY: &str = "unsupported-terminal-proofs";
const PROOF_VERSION: &str = "adjustment-maintenance-unsupported-terminal-proof/v1";
const COMMAND: &str = "unsupported-terminal-proof-current-v1";
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
const PROOF_KEYS: &[&str] = &[
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
const HASH_KEYS: &[&str] = &[
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

/// Bounded failures from the current-only proof reader.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeUnsupportedTerminalProofError {
    Arguments,
    Root,
    UnknownEntry,
    Slot,
    Proof,
    Codec,
}

impl Display for NativeUnsupportedTerminalProofError {
    /// Return the predecessor's bounded operator-facing failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "unsupported-terminal-proof-current-v1 takes no arguments",
            Self::Root => "adjustment unsupported terminal root is unsafe",
            Self::UnknownEntry => "adjustment unsupported terminal root contains an unknown entry",
            Self::Slot => "adjustment unsupported terminal slot is invalid",
            Self::Proof => "adjustment unsupported terminal proof is invalid",
            Self::Codec => "adjustment unsupported terminal proof serialization failed",
        })
    }
}

impl std::error::Error for NativeUnsupportedTerminalProofError {}

/// Exact typed projection returned by `AdjustmentUnsupportedTerminalProofStore.readCurrent`.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeUnsupportedTerminalProofCurrent {
    pub proof: JsonValue,
    pub proof_sha256: String,
}

/// Read-only filesystem boundary for the fixed unsupported-terminal proof root.
pub trait NativeUnsupportedTerminalProofFiles {
    /// Return genuine root absence or the complete existing fixed inventory.
    fn read_root(&mut self) -> Result<Option<Vec<String>>, NativeUnsupportedTerminalProofError>;

    /// Read the optional current slot without following or mutating it.
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, NativeUnsupportedTerminalProofError>;
}

/// Native custody reader bound to one selected evidence root and service identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeUnsupportedTerminalProofStore {
    proof_root: PathBuf,
    uid: u32,
    gid: u32,
}

impl NativeUnsupportedTerminalProofStore {
    /// Bind the deployed retained artifact path to the supplied service identity.
    pub fn native(uid: u32, gid: u32) -> Self {
        Self::for_evidence_root(PathBuf::from(EVIDENCE_ROOT), uid, gid)
    }

    /// Bind a disposable or explicitly selected evidence root without reading it.
    pub fn for_evidence_root(root: PathBuf, uid: u32, gid: u32) -> Self {
        Self {
            proof_root: root.join(PROOF_DIRECTORY),
            uid,
            gid,
        }
    }
}

impl NativeUnsupportedTerminalProofFiles for NativeUnsupportedTerminalProofStore {
    /// Admit the existing owner-private root before exposing its fixed names.
    fn read_root(&mut self) -> Result<Option<Vec<String>>, NativeUnsupportedTerminalProofError> {
        // preserve genuine root absence as the predecessor's null state
        if !verified_private_anchor_root(&self.proof_root, self.uid, self.gid)
            .map_err(|_| NativeUnsupportedTerminalProofError::Root)?
        {
            return Ok(None);
        }
        let mut names = fs::read_dir(&self.proof_root)
            .map_err(|_| NativeUnsupportedTerminalProofError::Root)?
            .map(|entry| {
                entry
                    .map_err(|_| NativeUnsupportedTerminalProofError::Root)?
                    .file_name()
                    .into_string()
                    .map_err(|_| NativeUnsupportedTerminalProofError::UnknownEntry)
            })
            .collect::<Result<Vec<_>, _>>()?;
        names.sort_unstable();
        Ok(Some(names))
    }

    /// Reuse the stable-inode bounded custody read for the exact current slot.
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, NativeUnsupportedTerminalProofError> {
        read_private_anchor_slot(&self.proof_root.join("current.json"), self.uid, self.gid)
            .map_err(|_| NativeUnsupportedTerminalProofError::Slot)
    }
}

/// Read and validate the exact nullable current proof projection without encoding it.
pub fn read_unsupported_terminal_proof_current(
    files: &mut impl NativeUnsupportedTerminalProofFiles,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<Option<NativeUnsupportedTerminalProofCurrent>, NativeUnsupportedTerminalProofError> {
    use NativeUnsupportedTerminalProofError::{Codec, Proof, UnknownEntry};

    let Some(names) = files.read_root()? else {
        return Ok(None);
    };
    // admit only the three transaction slots retained by the source store
    if names.iter().any(|name| {
        !matches!(
            name.as_str(),
            "current.json" | "pending.json" | "previous.json"
        )
    }) {
        return Err(UnknownEntry);
    }
    let Some(bytes) = files.read_current()? else {
        return Ok(None);
    };
    let proof = codec.parse_canonical_object(&bytes).ok_or(Proof)?;
    validate_proof(&proof, codec, time).ok_or(Proof)?;
    // prohibit any alternate JSON encoding at the retained proof boundary
    if codec.encode_canonical_json(&proof).as_deref() != Some(bytes.as_slice()) {
        return Err(Proof);
    }
    let proof_sha256 = codec.sha256(&bytes).ok_or(Codec)?;
    // retain the exact lowercase digest envelope contract
    if !valid_hash(proof_sha256.as_str()) {
        return Err(Codec);
    }
    Ok(Some(NativeUnsupportedTerminalProofCurrent {
        proof,
        proof_sha256: proof_sha256.as_str().to_owned(),
    }))
}

/// Execute the exact zero-operand current verb and encode its nullable envelope.
pub fn read_unsupported_terminal_proof_current_command(
    action: &str,
    arguments: &[&str],
    files: &mut impl NativeUnsupportedTerminalProofFiles,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<Option<Vec<u8>>, NativeUnsupportedTerminalProofError> {
    use NativeUnsupportedTerminalProofError::{Arguments, Codec};

    // leave unrelated operator verbs to their existing owners
    if action != COMMAND {
        return Ok(None);
    }
    // reject operands before opening the retained custody root
    if !arguments.is_empty() {
        return Err(Arguments);
    }
    let value = match read_unsupported_terminal_proof_current(files, codec, time)? {
        Some(current) => current_json(current),
        None => JsonValue::Null,
    };
    Ok(Some(codec.encode_canonical_json(&value).ok_or(Codec)?))
}

/// Project the already validated current envelope with no additional fields.
fn current_json(current: NativeUnsupportedTerminalProofCurrent) -> JsonValue {
    JsonValue::Object(vec![
        ("proof".encode_utf16().collect(), current.proof),
        (
            "proofSha256".encode_utf16().collect(),
            JsonValue::String(current.proof_sha256.encode_utf16().collect()),
        ),
    ])
}

/// Validate the complete v1 proof contract and its population-derived identities.
fn validate_proof(
    value: &JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Option<()> {
    exact_keys(value, PROOF_KEYS).then_some(())?;
    let family = text(value, "family")?;
    matches!(family.as_str(), "temperature" | "wind" | "rain").then_some(())?;
    (text(value, "contractVersion")? == PROOF_VERSION).then_some(())?;
    let due_key = text(value, "dueKey")?;
    let due = due_key.strip_prefix("confirmation/")?;
    let (due_family, due_digest) = due.split_once('/')?;
    (due_family == family && valid_hash(due_digest)).then_some(())?;
    let expected_chunks = if family == "rain" { 24 } else { 27 };
    (bounded_integer(
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
    let finalized = canonical_instant(value, "finalizedAt", time)?;
    let verified = canonical_instant(value, "fullGraphVerifiedAt", time)?;
    (finalized >= verified).then_some(())?;
    // validate every terminal, graph, custody, and population digest
    for key in HASH_KEYS {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    let predecessor_absent = match field(value, "predecessorProofSha256")? {
        JsonValue::Null => true,
        JsonValue::String(units) => {
            // a validated digest is present even when it is syntactically valid
            valid_hash(&String::from_utf16(units).ok()?).then_some(())?;
            false
        }
        _ => return None,
    };
    ((sequence == 0) == predecessor_absent).then_some(())?;
    bounded_integer(value, "missingKeyCount", 1, MAXIMUM_POPULATION)?;

    let input_classes = field(value, "inputClasses")?;
    exact_keys(input_classes, INPUT_CLASS_NAMES).then_some(())?;
    let empty_root = canonical_digest(codec, &JsonValue::Array(Vec::new()))?;
    let mut missing_names = Vec::new();
    let mut artifact_count = None;
    let mut candidate_count = None;
    let mut candidate_root = None;
    // validate each exact sorted-identity population in source order
    for name in INPUT_CLASS_NAMES {
        let entry = field(input_classes, name)?;
        exact_keys(entry, &["count", "rootSha256"]).then_some(())?;
        let count = bounded_integer(entry, "count", 0, MAXIMUM_POPULATION)?;
        let root = text(entry, "rootSha256")?;
        valid_hash(&root).then_some(())?;
        (count != 0 || root == empty_root).then_some(())?;
        // derive only genuinely missing classes, excluding the non-rain gate placeholder
        if count == 0 && (family == "rain" || *name != "rain_gate_input") {
            missing_names.push(JsonValue::String(name.encode_utf16().collect()));
        }
        // retain the two source-required populated classes for their exact checks
        match *name {
            "artifact" => artifact_count = Some(count),
            "candidate" => {
                candidate_count = Some(count);
                candidate_root = Some(root);
            }
            _ => {}
        }
    }
    let expected_candidate_root = canonical_digest(
        codec,
        &JsonValue::Array(vec![JsonValue::String(due_digest.encode_utf16().collect())]),
    )?;
    (artifact_count? >= 1 && candidate_count? == 1 && candidate_root? == expected_candidate_root)
        .then_some(())?;
    (field(value, "missingClassNames")? == &JsonValue::Array(missing_names)).then_some(())?;
    let required_root = canonical_digest(codec, input_classes)?;
    (text(value, "requiredInputRootSha256")? == required_root).then_some(())?;
    Some(())
}

/// Decode one bounded nonnegative JSON integer without JS coercion.
fn bounded_integer(value: &JsonValue, key: &str, minimum: u64, maximum: u64) -> Option<u64> {
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    (number.is_finite()
        && number.fract() == 0.0
        && *number >= minimum as f64
        && *number <= maximum as f64)
        .then_some(*number as u64)
}

/// Decode one canonical decimal unsigned 64-bit source string.
fn uint64_text(value: &JsonValue, key: &str) -> Option<u64> {
    let value = text(value, key)?;
    let canonical = value == "0"
        || value
            .as_bytes()
            .first()
            .is_some_and(|first| (b'1'..=b'9').contains(first))
            && value.as_bytes().iter().all(u8::is_ascii_digit);
    canonical.then_some(())?;
    value.parse().ok()
}

/// Admit the source's one-to-six digit positive control version.
fn valid_control_version(value: &str) -> bool {
    (1..=6).contains(&value.len())
        && value
            .as_bytes()
            .first()
            .is_some_and(|first| (b'1'..=b'9').contains(first))
        && value.as_bytes().iter().all(u8::is_ascii_digit)
}

/// Admit one exact-width lowercase hexadecimal source identity.
fn valid_lower_hex(value: &str, width: usize) -> bool {
    value.len() == width
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Parse the source's fixed four-digit millisecond UTC instant.
fn canonical_instant(
    value: &JsonValue,
    key: &str,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Option<i64> {
    let value = text(value, key)?;
    let bytes = value.as_bytes();
    let shape = bytes.len() == 24
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes.get(10) == Some(&b'T')
        && bytes.get(13) == Some(&b':')
        && bytes.get(16) == Some(&b':')
        && bytes.get(19) == Some(&b'.')
        && bytes.get(23) == Some(&b'Z')
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 23) || byte.is_ascii_digit()
        });
    shape.then_some(())?;
    time.canonical_utc_milliseconds(&value)
}

/// Hash one canonical JSON value through the approved shared adapter.
fn canonical_digest(
    codec: &mut impl MaintenanceCanonicalJson,
    value: &JsonValue,
) -> Option<String> {
    let bytes = codec.encode_canonical_json(value)?;
    let digest = codec.sha256(&bytes)?;
    valid_hash(digest.as_str()).then(|| digest.as_str().to_owned())
}

#[cfg(test)]
#[path = "native_unsupported_terminal_proof_test.rs"]
mod tests;
