//! Read-only native projection of one fixed adjustment-family transaction status.

use std::fmt::{Display, Formatter};
use std::fs;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::MaintenanceCanonicalJson;
use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::capture_epoch_witness::resolve_operator_capture_clock;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};

const STATUS_COMMAND: &str = "family-release-status";
const STATUS_VERSION: &str = "adjustment-family-release-status/v1";
const TRANSACTION_VERSION: &str = "adjustment-family-release-transaction/v1";
const OPERATOR_OFF_TRANSACTION_VERSION: &str = "adjustment-family-operator-off-transaction/v1";
const DEFAULT_STATE_ROOT: &str = "/opt/weather/current/deploy/state/adjustment-release-state";
const MAXIMUM_TRANSACTION_BYTES: usize = 512 * 1024;
const TRANSACTION_KEYS: &[&str] = &["compensation", "contractVersion", "proof", "record"];
const OPERATOR_OFF_DOCUMENT_KEYS: &[&str] = &["contractVersion", "record"];
const OPERATOR_OFF_RECORD_KEYS: &[&str] = &[
    "completedAt",
    "input",
    "sourceCommit",
    "sourceSettingsSha256",
    "state",
];
const INPUT_KEYS: &[&str] = &[
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

/// Contained argument, custody, transaction, or encoding failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeFamilyReleaseStatusError {
    Arguments,
    File,
    Transaction,
    Codec,
}

impl Display for NativeFamilyReleaseStatusError {
    /// Render one fixed operator-facing failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "family-release-status requires ACTION_SHA256",
            Self::File => "family release transaction file is invalid",
            Self::Transaction => "family release transaction is invalid",
            Self::Codec => "family release status serialization failed",
        })
    }
}

impl std::error::Error for NativeFamilyReleaseStatusError {}

/// Supply optional exact transaction bytes for one already admitted action identity.
pub trait NativeFamilyReleaseStatusFiles {
    /// Preserve genuine absence separately from an invalid retained file.
    fn read_transaction(
        &mut self,
        action_sha256: &str,
    ) -> Result<Option<Vec<u8>>, NativeFamilyReleaseStatusError>;
}

/// Fixed owner-private transaction-file reader for the deployed family journal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseStatusHostFiles {
    root: PathBuf,
    uid: u32,
    gid: u32,
}

impl NativeFamilyReleaseStatusHostFiles {
    /// Select the fixed root-owned production transaction journal.
    pub fn native() -> Self {
        Self::for_root(PathBuf::from(DEFAULT_STATE_ROOT), 0, 0)
    }

    /// Bind one explicit owner-private journal root for composition or disposable fixtures.
    pub fn for_root(root: PathBuf, uid: u32, gid: u32) -> Self {
        Self { root, uid, gid }
    }
}

impl Default for NativeFamilyReleaseStatusHostFiles {
    /// Select the fixed production reader without performing I/O.
    fn default() -> Self {
        Self::native()
    }
}

impl NativeFamilyReleaseStatusFiles for NativeFamilyReleaseStatusHostFiles {
    /// Read one content-addressed transaction through a stable no-follow descriptor.
    fn read_transaction(
        &mut self,
        action_sha256: &str,
    ) -> Result<Option<Vec<u8>>, NativeFamilyReleaseStatusError> {
        // reject path-shaped action identities before constructing the fixed filename
        if !valid_hash(action_sha256) {
            return Err(NativeFamilyReleaseStatusError::Arguments);
        }
        let path = self
            .root
            .join("transactions")
            .join(format!("sha256-{action_sha256}.json"));
        let file = match open_read_no_follow(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(NativeFamilyReleaseStatusError::File),
        };
        let before = file
            .metadata()
            .map_err(|_| NativeFamilyReleaseStatusError::File)?;
        // retain the source's private single-link file identity and byte ceiling
        if !before.is_file()
            || before.nlink() != 1
            || before.uid() != self.uid
            || before.gid() != self.gid
            || before.mode() & 0o777 != 0o600
            || !(1..=MAXIMUM_TRANSACTION_BYTES as u64).contains(&before.size())
        {
            return Err(NativeFamilyReleaseStatusError::File);
        }
        read_stable_transaction(file, before).map(Some)
    }
}

/// Execute only the direct read-only family-release-status verb.
pub fn read_native_family_release_status_command(
    action: &str,
    arguments: &[&str],
    files: &mut impl NativeFamilyReleaseStatusFiles,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<Vec<u8>>, NativeFamilyReleaseStatusError> {
    // leave every other operator verb to its owning native handler
    if action != STATUS_COMMAND {
        return Ok(None);
    }
    let [action_sha256] = arguments else {
        return Err(NativeFamilyReleaseStatusError::Arguments);
    };
    // close the only operand before observing the journal
    if !valid_hash(action_sha256) {
        return Err(NativeFamilyReleaseStatusError::Arguments);
    }
    let bytes = files.read_transaction(action_sha256)?;
    let status =
        project_native_family_release_status(action_sha256, bytes.as_deref(), codec, time)?;
    codec
        .encode_canonical_json(&status)
        .map(Some)
        .ok_or(NativeFamilyReleaseStatusError::Codec)
}

/// Project exact absence or one admitted categorical family transaction status.
pub fn project_native_family_release_status(
    action_sha256: &str,
    transaction_bytes: Option<&[u8]>,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, NativeFamilyReleaseStatusError> {
    use NativeFamilyReleaseStatusError::Transaction;

    // close direct callers before parsing supplied material
    if !valid_hash(action_sha256) {
        return Err(NativeFamilyReleaseStatusError::Arguments);
    }
    let Some(bytes) = transaction_bytes else {
        return Ok(object(vec![
            string_field("actionSha256", action_sha256),
            string_field("contractVersion", STATUS_VERSION),
            string_field("state", "absent"),
        ]));
    };
    if !(1..=MAXIMUM_TRANSACTION_BYTES).contains(&bytes.len()) {
        return Err(Transaction);
    }
    let document = codec.parse_canonical_object(bytes).ok_or(Transaction)?;
    if codec.encode_canonical_json(&document).as_deref() != Some(bytes) {
        return Err(Transaction);
    }
    let version = text(&document, "contractVersion").ok_or(Transaction)?;
    // retain the disjoint no-mutation receipt without proof or compensation fields
    if version == OPERATOR_OFF_TRANSACTION_VERSION {
        if !exact_keys(&document, OPERATOR_OFF_DOCUMENT_KEYS) {
            return Err(Transaction);
        }
        let record = field(&document, "record").ok_or(Transaction)?;
        return operator_off_status(record, action_sha256, time).ok_or(Transaction);
    }
    if version != TRANSACTION_VERSION || !exact_keys(&document, TRANSACTION_KEYS) {
        return Err(Transaction);
    }
    let compensation = field(&document, "compensation").ok_or(Transaction)?;
    let record = field(&document, "record").ok_or(Transaction)?;
    let input = field(record, "input").ok_or(Transaction)?;
    let input_action = text(input, "actionSha256").ok_or(Transaction)?;
    let compensation_state = text(compensation, "state").ok_or(Transaction)?;
    // preserve the source reader's shallow writer and requested-action admission
    if input_action != action_sha256
        || !matches!(
            compensation_state.as_str(),
            "absent" | "failed" | "verified"
        )
    {
        return Err(Transaction);
    }
    let state = text(record, "state").ok_or(Transaction)?;
    let family = text(input, "family").ok_or(Transaction)?;
    let fencing_token = text(input, "fencingToken").ok_or(Transaction)?;
    let target_release = text(input, "targetRelease").ok_or(Transaction)?;
    let compensating_release = text(input, "compensatingRelease").ok_or(Transaction)?;
    // reproduce JavaScript nullish fallback without losing an explicit non-null outcome
    let outcome = field(record, "outcome")
        .filter(|value| !matches!(value, JsonValue::Null))
        .cloned()
        .unwrap_or(JsonValue::Null);
    // close the categorical projection emitted by valid durable transaction writers
    if !valid_family(&family)
        || !valid_u64(&fencing_token)
        || !valid_release(&target_release)
        || !valid_release(&compensating_release)
        || !valid_status_combination(&state, &compensation_state, &outcome)
    {
        return Err(Transaction);
    }
    Ok(object(vec![
        string_field("actionSha256", action_sha256),
        string_field("compensatingRelease", &compensating_release),
        string_field("compensationState", &compensation_state),
        string_field("contractVersion", STATUS_VERSION),
        string_field("family", &family),
        string_field("fencingToken", &fencing_token),
        ("outcome".encode_utf16().collect(), outcome),
        string_field("state", &state),
        string_field("targetRelease", &target_release),
    ]))
}

/// Validate and project one exact operator-off transaction record.
fn operator_off_status(
    record: &JsonValue,
    action_sha256: &str,
    time: &dyn CurrentTimeResolver,
) -> Option<JsonValue> {
    if !exact_keys(record, OPERATOR_OFF_RECORD_KEYS) {
        return None;
    }
    let input = field(record, "input")?;
    if !exact_keys(input, INPUT_KEYS) {
        return None;
    }
    let family = text(input, "family")?;
    let fence = text(input, "fencingToken")?;
    let target = text(input, "targetRelease")?;
    let compensation = text(input, "compensatingRelease")?;
    let current = text(input, "expectedCurrentRelease")?;
    let source = text(input, "expectedSourceRelease")?;
    let source_commit = text(record, "sourceCommit")?;
    let completed_at = text(record, "completedAt")?;
    // bind the exact source observation and no-mutation command identity
    if text(record, "state").as_deref() != Some("operator_off_unapplied")
        || text(input, "actionSha256").as_deref() != Some(action_sha256)
        || !valid_family(&family)
        || !valid_commit(&source_commit)
        || !valid_u64(&fence)
        || ![&target, &compensation, &current, &source]
            .into_iter()
            .all(|release| valid_release(release))
        || current != source
        || target == current
        || target == compensation
        || text(record, "sourceSettingsSha256") != text(input, "expectedSettingsSha256")
        || ![
            text(input, "actionSha256"),
            text(input, "expectedSettingsSha256"),
            text(input, "reportSha256"),
            text(record, "sourceSettingsSha256"),
        ]
        .into_iter()
        .all(|value| value.as_deref().is_some_and(valid_hash))
        || !valid_instant(&completed_at, time)
    {
        return None;
    }
    Some(object(vec![
        string_field("actionSha256", action_sha256),
        string_field("compensatingRelease", &compensation),
        string_field("compensationState", "absent"),
        string_field("contractVersion", STATUS_VERSION),
        string_field("family", &family),
        string_field("fencingToken", &fence),
        string_field("outcome", "operator_off_unapplied"),
        string_field("state", "operator_off_unapplied"),
        string_field("targetRelease", &target),
    ]))
}

/// Admit only source-emitted state, compensation and outcome combinations.
fn valid_status_combination(state: &str, compensation: &str, outcome: &JsonValue) -> bool {
    match state {
        "prepared" | "applying" => compensation == "absent" && matches!(outcome, JsonValue::Null),
        "acknowledged" => {
            compensation == "absent"
                && matches!(
                    outcome,
                    JsonValue::String(value)
                        if ["active", "deployed_operator_off"].iter().any(|expected| {
                            value.iter().copied().eq(expected.encode_utf16())
                        })
                )
        }
        "compensation_required" => matches!(outcome, JsonValue::Null),
        "operator_off_unapplied" => {
            compensation == "absent"
                && matches!(
                    outcome,
                    JsonValue::String(value)
                        if value.iter().copied().eq("operator_off_unapplied".encode_utf16())
                )
        }
        _ => false,
    }
}

/// Read one admitted descriptor to completion and reject in-place mutation.
fn read_stable_transaction(
    mut file: fs::File,
    before: fs::Metadata,
) -> Result<Vec<u8>, NativeFamilyReleaseStatusError> {
    let mut bytes = Vec::with_capacity(before.size() as usize);
    (&mut file)
        .take(MAXIMUM_TRANSACTION_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| NativeFamilyReleaseStatusError::File)?;
    let after = file
        .metadata()
        .map_err(|_| NativeFamilyReleaseStatusError::File)?;
    // reject truncation, growth, replacement or in-place timestamp mutation
    if bytes.len() as u64 != before.size()
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.size() != after.size()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
    {
        return Err(NativeFamilyReleaseStatusError::File);
    }
    Ok(bytes)
}

/// Recognize one exact family identifier.
fn valid_family(value: &str) -> bool {
    matches!(value, "temperature" | "wind" | "rain")
}

/// Recognize one immutable family release spelling.
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

/// Recognize one canonical positive unsigned 64-bit fence.
fn valid_u64(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|value| value.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

/// Recognize one lowercase immutable Git commit identity.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|value| value.is_ascii_digit() || matches!(value, b'a'..=b'f'))
}

/// Validate one exact millisecond UTC clock with the shared operator resolver.
fn valid_instant(value: &str, time: &dyn CurrentTimeResolver) -> bool {
    let shape = b"dddd-dd-ddTdd:dd:dd.dddZ";
    value.len() == shape.len()
        && shape
            .iter()
            .zip(value.bytes())
            .all(|(&expected, observed)| {
                // retain every literal separator and digit position
                if expected == b'd' {
                    observed.is_ascii_digit()
                } else {
                    observed == expected
                }
            })
        && resolve_operator_capture_clock(value, time).is_some()
}

/// Construct one JSON object while retaining explicit null members.
fn object(fields: Vec<(Vec<u16>, JsonValue)>) -> JsonValue {
    JsonValue::Object(fields)
}

/// Construct one exact UTF-16 string member.
fn string_field(name: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (
        name.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}

#[cfg(test)]
#[path = "native_family_release_status_test.rs"]
mod tests;
