//! Exact verified current reader for the future-only input seal operator verb.

use std::{env, fmt, fs, path::PathBuf};

use weather_api::CurrentTimeResolver;
use weather_contracts::{AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum, JsonValue};
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::maintenance_anchor_status::{
    read_private_anchor_slot, uint64_field, verified_private_anchor_root,
};
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{
    CaptureEpochOperatorFile, CaptureEpochOperatorFiles, MaintenanceCanonicalJson,
    NativeCaptureEpochOperatorFiles, NativeWirePrimitives, verify_operator_capture_epoch_witness,
};

const COMMAND: &str = "future-input-seal-current-v2";
const EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";
const SEAL_DIRECTORY: &str = "future-input-seals";
const SEAL_VERSION: &str = "adjustment-future-only-input-seal/v2";
const CONTROL_VERSION_ENV: &str = "WEATHER_ANCHOR_CONTROL_VERSION";
const CONTROL_SHA256_ENV: &str = "WEATHER_ANCHOR_CONTROL_SHA256";
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
const SEAL_KEYS: &[&str] = &[
    "archiveCommitOrdinal",
    "burnSha256",
    "candidateArtifactRootSha256",
    "candidateReportSha256",
    "captureEpochWitnessSha256",
    "comparatorRootSha256",
    "confirmationAccessSha256",
    "confirmationChunkCount",
    "contractVersion",
    "custodyCheckpointSha256",
    "dueKey",
    "family",
    "frontierSha256",
    "fullMemberCount",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "inputClasses",
    "lifecycleLedgerRootSha256",
    "pageSha256",
    "policyReportSha256",
    "predecessorSealSha256",
    "requiredInputRootSha256",
    "sealedAt",
    "sequence",
    "sourceCommit",
    "targetRootSha256",
    "workstationJournalHeadSha256",
];
const HASH_KEYS: &[&str] = &[
    "burnSha256",
    "candidateArtifactRootSha256",
    "candidateReportSha256",
    "captureEpochWitnessSha256",
    "comparatorRootSha256",
    "confirmationAccessSha256",
    "custodyCheckpointSha256",
    "frontierSha256",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "lifecycleLedgerRootSha256",
    "pageSha256",
    "policyReportSha256",
    "requiredInputRootSha256",
    "targetRootSha256",
    "workstationJournalHeadSha256",
];

/// Bounded failures from verified capture, custody, closure, or canonical output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeFutureInputSealCurrentError {
    Arguments,
    ControlEnvironment,
    CaptureEpoch,
    ActiveEpoch,
    Root,
    UnknownEntry,
    Slot,
    Seal,
    SourceClosure,
    Codec,
}

impl fmt::Display for NativeFutureInputSealCurrentError {
    /// Render the predecessor's stable operator-facing failure category.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "future-input-seal-current-v2 takes no arguments",
            Self::ControlEnvironment => "future-only input seal control is unavailable",
            Self::CaptureEpoch => "future-only input seal capture epoch is invalid",
            Self::ActiveEpoch => "future-only input seal active epoch differs",
            Self::Root => "future-only input seal root is unsafe",
            Self::UnknownEntry => "future-only input seal root contains an unknown entry",
            Self::Slot => "future-only input seal slot is invalid",
            Self::Seal => "future-only input seal is invalid",
            Self::SourceClosure => "future-only input seal source closure differs",
            Self::Codec => "future-only input seal serialization failed",
        })
    }
}

impl std::error::Error for NativeFutureInputSealCurrentError {}

/// Exact canonical witness bytes and active control identity admitted by the shared verifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeFutureInputSealCaptureEpochWitness {
    pub canonical_bytes: Vec<u8>,
    pub control_version: String,
    pub control_sha256: String,
}

/// Exact typed projection returned by `readCurrentVerified` after every external binding.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeFutureInputSealCurrent {
    pub seal: JsonValue,
    pub seal_sha256: String,
}

/// Borrowed active-control values, preserving missing environment values distinctly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeFutureInputSealControl<'a> {
    pub version: Option<&'a str>,
    pub sha256: Option<&'a str>,
}

/// Owned process control values read from the exact predecessor environment keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeFutureInputSealProcessControl {
    pub version: Option<String>,
    pub sha256: Option<String>,
}

impl NativeFutureInputSealProcessControl {
    /// Borrow the retained optional values for one verified current read.
    pub fn borrowed(&self) -> NativeFutureInputSealControl<'_> {
        NativeFutureInputSealControl {
            version: self.version.as_deref(),
            sha256: self.sha256.as_deref(),
        }
    }
}

/// Read only the fixed source control keys without accepting caller-selected aliases.
pub fn read_native_future_input_seal_process_control()
-> Result<NativeFutureInputSealProcessControl, NativeFutureInputSealCurrentError> {
    Ok(NativeFutureInputSealProcessControl {
        version: optional_environment(CONTROL_VERSION_ENV)?,
        sha256: optional_environment(CONTROL_SHA256_ENV)?,
    })
}

/// Read-only filesystem boundary for the fixed future-input-seal transaction root.
pub trait NativeFutureInputSealFiles {
    /// Return genuine root absence or the complete existing fixed inventory.
    fn read_root(&mut self) -> Result<Option<Vec<String>>, NativeFutureInputSealCurrentError>;

    /// Read the optional current slot without following or mutating it.
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, NativeFutureInputSealCurrentError>;
}

/// Capture-epoch owner that returns only a witness admitted by its canonical verifier.
pub trait NativeFutureInputSealCaptureEpoch {
    /// Read and verify the root-owned capture epoch before seal-root observation.
    fn read_verified_capture_epoch(
        &mut self,
    ) -> Result<NativeFutureInputSealCaptureEpochWitness, NativeFutureInputSealCurrentError>;
}

/// Frozen release-lineage closure required before exposing non-null current seal bytes.
pub trait NativeFutureInputSealSourceClosure {
    /// Verify one exact source commit on the retained epoch-to-current terminal path.
    fn source_is_closed(
        &mut self,
        source_commit: &str,
    ) -> Result<bool, NativeFutureInputSealCurrentError>;
}

/// Native custody reader bound to one selected evidence root and service identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFutureInputSealStore {
    seal_root: PathBuf,
    uid: u32,
    gid: u32,
}

impl NativeFutureInputSealStore {
    /// Bind the deployed retained artifact path to the supplied service identity.
    pub fn native(uid: u32, gid: u32) -> Self {
        Self::for_evidence_root(PathBuf::from(EVIDENCE_ROOT), uid, gid)
    }

    /// Bind a disposable or explicitly selected evidence root without reading it.
    pub fn for_evidence_root(root: PathBuf, uid: u32, gid: u32) -> Self {
        Self {
            seal_root: root.join(SEAL_DIRECTORY),
            uid,
            gid,
        }
    }
}

impl NativeFutureInputSealFiles for NativeFutureInputSealStore {
    /// Admit the existing owner-private root before exposing its fixed names.
    fn read_root(&mut self) -> Result<Option<Vec<String>>, NativeFutureInputSealCurrentError> {
        // preserve genuine root absence as the verified reader's null state
        if !verified_private_anchor_root(&self.seal_root, self.uid, self.gid)
            .map_err(|_| NativeFutureInputSealCurrentError::Root)?
        {
            return Ok(None);
        }
        let mut names = fs::read_dir(&self.seal_root)
            .map_err(|_| NativeFutureInputSealCurrentError::Root)?
            .map(|entry| {
                entry
                    .map_err(|_| NativeFutureInputSealCurrentError::Root)?
                    .file_name()
                    .into_string()
                    .map_err(|_| NativeFutureInputSealCurrentError::UnknownEntry)
            })
            .collect::<Result<Vec<_>, _>>()?;
        names.sort_unstable();
        Ok(Some(names))
    }

    /// Reuse the stable-inode bounded custody read for the exact current slot.
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, NativeFutureInputSealCurrentError> {
        read_private_anchor_slot(&self.seal_root.join("current.json"), self.uid, self.gid)
            .map_err(|_| NativeFutureInputSealCurrentError::Slot)
    }
}

/// Shared operator-witness adapter retaining the exact file, codec, and time owners.
pub struct NativeFutureInputSealOperatorCaptureEpoch<Files, Codec, Time> {
    files: Files,
    codec: Codec,
    time: Time,
}

impl<Files, Codec, Time> NativeFutureInputSealOperatorCaptureEpoch<Files, Codec, Time> {
    /// Bind explicit root-owned witness readers without observing their state early.
    pub const fn new(files: Files, codec: Codec, time: Time) -> Self {
        Self { files, codec, time }
    }
}

impl
    NativeFutureInputSealOperatorCaptureEpoch<
        NativeCaptureEpochOperatorFiles,
        NativeWirePrimitives,
        NativeWirePrimitives,
    >
{
    /// Select the fixed deployed operator witness and shared native verifier.
    pub const fn native() -> Self {
        Self::new(
            NativeCaptureEpochOperatorFiles,
            NativeWirePrimitives,
            NativeWirePrimitives,
        )
    }
}

impl<Files, Codec, Time> NativeFutureInputSealCaptureEpoch
    for NativeFutureInputSealOperatorCaptureEpoch<Files, Codec, Time>
where
    Files: CaptureEpochOperatorFiles,
    Codec: MaintenanceCanonicalJson,
    Time: CurrentTimeResolver,
{
    /// Verify the exact operator witness before returning any active-control material.
    fn read_verified_capture_epoch(
        &mut self,
    ) -> Result<NativeFutureInputSealCaptureEpochWitness, NativeFutureInputSealCurrentError> {
        let bytes = self
            .files
            .read(CaptureEpochOperatorFile::Witness)
            .map_err(|_| NativeFutureInputSealCurrentError::CaptureEpoch)?;
        let admitted = verify_operator_capture_epoch_witness(&bytes, &mut self.codec, &self.time)
            .map_err(|_| NativeFutureInputSealCurrentError::CaptureEpoch)?;
        let control_version = match admitted.witness.controlPlaneVersion {
            AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value14 => "14",
            AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value17 => "17",
        };
        Ok(NativeFutureInputSealCaptureEpochWitness {
            canonical_bytes: bytes,
            control_version: control_version.to_owned(),
            control_sha256: admitted.witness.controlPlaneSha256,
        })
    }
}

/// Read and validate the exact nullable current seal after epoch, control, and closure checks.
pub fn read_future_input_seal_current_verified<Time>(
    files: &mut impl NativeFutureInputSealFiles,
    capture_epoch: &mut impl NativeFutureInputSealCaptureEpoch,
    control: NativeFutureInputSealControl<'_>,
    source_closure: &mut impl NativeFutureInputSealSourceClosure,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &Time,
) -> Result<Option<NativeFutureInputSealCurrent>, NativeFutureInputSealCurrentError>
where
    Time: CurrentTimeResolver + RevisionArchiveCanonicalCodec,
{
    let witness = capture_epoch.read_verified_capture_epoch()?;
    // require the actual active v14 control before observing even genuine genesis absence
    if control.version != Some("14")
        || control.version != Some(witness.control_version.as_str())
        || control.sha256 != Some(witness.control_sha256.as_str())
    {
        return Err(NativeFutureInputSealCurrentError::ActiveEpoch);
    }
    let Some(names) = files.read_root()? else {
        return Ok(None);
    };
    // reject foreign state outside the fixed bounded seal transaction slots
    if names.iter().any(|name| {
        !matches!(
            name.as_str(),
            "current.json" | "pending.json" | "previous.json"
        )
    }) {
        return Err(NativeFutureInputSealCurrentError::UnknownEntry);
    }
    let Some(bytes) = files.read_current()? else {
        return Ok(None);
    };
    let seal = parse_future_input_seal(&bytes, codec, time)
        .ok_or(NativeFutureInputSealCurrentError::Seal)?;
    let witness_sha256 = codec
        .sha256(&witness.canonical_bytes)
        .ok_or(NativeFutureInputSealCurrentError::Codec)?;
    // bind the complete retained epoch bytes before consulting release ancestry
    if text(&seal, "captureEpochWitnessSha256").as_deref() != Some(witness_sha256.as_str()) {
        return Err(NativeFutureInputSealCurrentError::ActiveEpoch);
    }
    let source_commit =
        text(&seal, "sourceCommit").ok_or(NativeFutureInputSealCurrentError::Seal)?;
    // expose proof bytes only for a source on the retained terminal ancestry
    if !source_closure.source_is_closed(&source_commit)? {
        return Err(NativeFutureInputSealCurrentError::SourceClosure);
    }
    let seal_sha256 = codec
        .sha256(&bytes)
        .ok_or(NativeFutureInputSealCurrentError::Codec)?;
    Ok(Some(NativeFutureInputSealCurrent {
        seal,
        seal_sha256: seal_sha256.as_str().to_owned(),
    }))
}

/// Execute the exact zero-operand current verb and encode its nullable envelope.
pub fn read_future_input_seal_current_command<Time>(
    command: (&str, &[&str]),
    files: &mut impl NativeFutureInputSealFiles,
    capture_epoch: &mut impl NativeFutureInputSealCaptureEpoch,
    control: NativeFutureInputSealControl<'_>,
    source_closure: &mut impl NativeFutureInputSealSourceClosure,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &Time,
) -> Result<Option<Vec<u8>>, NativeFutureInputSealCurrentError>
where
    Time: CurrentTimeResolver + RevisionArchiveCanonicalCodec,
{
    let (action, arguments) = command;
    // leave unrelated operator verbs to their existing owners
    if action != COMMAND {
        return Ok(None);
    }
    // reject operands before reading control, witness, or retained custody state
    if !arguments.is_empty() {
        return Err(NativeFutureInputSealCurrentError::Arguments);
    }
    let value = match read_future_input_seal_current_verified(
        files,
        capture_epoch,
        control,
        source_closure,
        codec,
        time,
    )? {
        Some(current) => current_json(current),
        None => JsonValue::Null,
    };
    Ok(Some(
        codec
            .encode_canonical_json(&value)
            .ok_or(NativeFutureInputSealCurrentError::Codec)?,
    ))
}

/// Project the already verified current envelope with no additional fields.
fn current_json(current: NativeFutureInputSealCurrent) -> JsonValue {
    JsonValue::Object(vec![
        ("seal".encode_utf16().collect(), current.seal),
        (
            "sealSha256".encode_utf16().collect(),
            JsonValue::String(current.seal_sha256.encode_utf16().collect()),
        ),
    ])
}

/// Parse one complete canonical seal through the shared current/install contract.
pub(crate) fn parse_future_input_seal(
    bytes: &[u8],
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Option<JsonValue> {
    let seal = codec.parse_canonical_object(bytes)?;
    validate_seal(&seal, codec, time)?;
    // prohibit alternate encodings of one otherwise valid seal document
    (codec.encode_canonical_json(&seal)?.as_slice() == bytes).then_some(seal)
}

/// Validate the complete future-only seal contract and all derived population roots.
fn validate_seal(
    value: &JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Option<()> {
    exact_keys(value, SEAL_KEYS).then_some(())?;
    (text(value, "contractVersion")? == SEAL_VERSION).then_some(())?;
    let family = text(value, "family")?;
    matches!(family.as_str(), "temperature" | "wind" | "rain").then_some(())?;
    let due_key = text(value, "dueKey")?;
    let due = due_key.strip_prefix("confirmation/")?;
    let (due_family, due_digest) = due.split_once('/')?;
    (due_family == family && valid_hash(due_digest)).then_some(())?;
    let expected_chunks = if family == "rain" { 24 } else { 27 };
    (bounded_integer(value, "confirmationChunkCount", 0, 27)? == expected_chunks).then_some(())?;
    (bounded_integer(value, "fullMemberCount", 1, 1)? == 1).then_some(())?;
    valid_lower_hex(&text(value, "sourceCommit")?, 40).then_some(())?;
    uint64_field(value, "archiveCommitOrdinal")?;
    let sequence = uint64_field(value, "sequence")?;
    time.canonical_utc_milliseconds(&text(value, "sealedAt")?)?;
    // validate every nonnullable cross-plane identity before derived roots
    for key in HASH_KEYS {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    let predecessor_absent = match field(value, "predecessorSealSha256")? {
        JsonValue::Null => true,
        JsonValue::String(units) => {
            // a validated digest is present even when it is syntactically valid
            valid_hash(&String::from_utf16(units).ok()?).then_some(())?;
            false
        }
        _ => return None,
    };
    ((sequence == 0) == predecessor_absent).then_some(())?;

    let input_classes = field(value, "inputClasses")?;
    exact_keys(input_classes, INPUT_CLASS_NAMES).then_some(())?;
    let empty_root = canonical_digest(codec, &JsonValue::Array(Vec::new()))?;
    let mut candidate_count = None;
    let mut candidate_root = None;
    let mut artifact_root = None;
    let mut comparator_root = None;
    let mut target_root = None;
    // validate every exact sorted-identity population root declaration
    for name in INPUT_CLASS_NAMES {
        let entry = field(input_classes, name)?;
        exact_keys(entry, &["count", "rootSha256"]).then_some(())?;
        let count = bounded_integer(entry, "count", 0, MAXIMUM_POPULATION)?;
        let root = text(entry, "rootSha256")?;
        valid_hash(&root).then_some(())?;
        (count != 0 || root == empty_root).then_some(())?;
        // require every genuine family input and keep rain-only absence exact elsewhere
        if family == "rain" || *name != "rain_gate_input" {
            (count >= 1).then_some(())?;
        } else {
            (count == 0 && root == empty_root).then_some(())?;
        }
        // retain only named roots needed by the source's derived bindings
        match *name {
            "artifact" => artifact_root = Some(root),
            "candidate" => {
                candidate_count = Some(count);
                candidate_root = Some(root);
            }
            "comparator" => comparator_root = Some(root),
            "target" => target_root = Some(root),
            _ => {}
        }
    }
    let candidate_root = candidate_root?;
    let expected_candidate_root = canonical_digest(
        codec,
        &JsonValue::Array(vec![JsonValue::String(due_digest.encode_utf16().collect())]),
    )?;
    (candidate_count? == 1 && candidate_root == expected_candidate_root).then_some(())?;
    (text(value, "requiredInputRootSha256")? == canonical_digest(codec, input_classes)?)
        .then_some(())?;
    (text(value, "comparatorRootSha256")? == comparator_root?).then_some(())?;
    (text(value, "targetRootSha256")? == target_root?).then_some(())?;
    let candidate_artifact_root = canonical_digest(
        codec,
        &JsonValue::Array(vec![
            JsonValue::String(candidate_root.encode_utf16().collect()),
            JsonValue::String(artifact_root?.encode_utf16().collect()),
        ]),
    )?;
    (text(value, "candidateArtifactRootSha256")? == candidate_artifact_root).then_some(())?;
    Some(())
}

/// Hash one canonical source value through the caller's exact codec.
fn canonical_digest(
    codec: &mut impl MaintenanceCanonicalJson,
    value: &JsonValue,
) -> Option<String> {
    let bytes = codec.encode_canonical_json(value)?;
    Some(codec.sha256(&bytes)?.as_str().to_owned())
}

/// Decode one bounded nonnegative JSON safe integer without coercion.
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

/// Recognize one exact lowercase hexadecimal field of fixed width.
fn valid_lower_hex(value: &str, width: usize) -> bool {
    value.len() == width
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Read one optional Unicode process value while preserving absence.
fn optional_environment(
    name: &'static str,
) -> Result<Option<String>, NativeFutureInputSealCurrentError> {
    // distinguish missing configuration from an invalid non-Unicode value
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => {
            Err(NativeFutureInputSealCurrentError::ControlEnvironment)
        }
    }
}
