//! Exact native admission of the predecessor maintenance journal hash chain.

use std::fmt::{self, Display, Formatter};
use std::io::{ErrorKind, Read};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::MaintenanceCanonicalJson;
use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::capture_epoch_witness::resolve_operator_capture_clock;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};

/// Exact predecessor contract admitted for every hash-chain row.
pub const NATIVE_MAINTENANCE_JOURNAL_CONTRACT_VERSION: &str =
    "forecast-adjustment-maintenance-journal/v2";
/// Exact predecessor byte ceiling for the complete append-only journal.
pub const NATIVE_MAINTENANCE_JOURNAL_MAXIMUM_BYTES: usize = 64 * 1_024 * 1_024;

const JOURNAL_KEYS: &[&str] = &[
    "contractVersion",
    "generation",
    "kind",
    "recordKey",
    "dueKey",
    "recordedAt",
    "previousSha256",
    "payloadSha256",
    "payload",
    "recordSha256",
];

/// Value-free journal custody, syntax, chain, and identity failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceJournalError {
    File,
    Bytes,
    Record,
    Chain,
    PayloadIdentity,
    RecordIdentity,
    Codec,
}

impl Display for NativeMaintenanceJournalError {
    /// Render only the bounded source operation category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::File => "maintenance journal file is invalid",
            Self::Bytes => "maintenance journal bytes are invalid",
            Self::Record => "maintenance journal record is invalid",
            Self::Chain => "maintenance journal chain is invalid",
            Self::PayloadIdentity => "maintenance journal payload hash is invalid",
            Self::RecordIdentity => "maintenance journal record hash is invalid",
            Self::Codec => "maintenance journal serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceJournalError {}

/// One source-admitted journal row retaining its complete canonical value.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceJournalRecord {
    pub generation: u64,
    pub kind: JsonValue,
    pub record_key: String,
    pub due_key: Option<String>,
    pub recorded_at: String,
    pub previous_sha256: Option<String>,
    pub payload_sha256: String,
    pub payload: JsonValue,
    pub record_sha256: String,
    pub value: JsonValue,
}

/// Supply one coherent lock-scoped snapshot of the fixed journal member.
pub trait NativeMaintenanceJournalFiles {
    /// Preserve genuine journal absence as the empty genesis chain.
    fn read_journal(&mut self) -> Result<Vec<u8>, NativeMaintenanceJournalError>;
}

/// Read and verify one coherent journal snapshot without exposing unchecked rows.
pub fn read_verified_native_maintenance_journal(
    files: &mut impl NativeMaintenanceJournalFiles,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Vec<NativeMaintenanceJournalRecord>, NativeMaintenanceJournalError> {
    let bytes = files.read_journal()?;
    verify_native_maintenance_journal_bytes(&bytes, codec, time)
}

/// Stable no-follow reader for an already admitted owner-private journal root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceJournalHostFiles {
    root: PathBuf,
    uid: u32,
}

impl NativeMaintenanceJournalHostFiles {
    /// Bind an explicitly admitted source state root and its owning process identity.
    pub fn for_root(root: PathBuf, uid: u32) -> Self {
        Self { root, uid }
    }

    /// Derive the predecessor's fixed state root from an explicitly selected home.
    pub fn for_home(home: &Path, uid: u32) -> Self {
        Self::for_root(
            home.join(".weather")
                .join("adjustment-maintenance")
                .join("v2")
                .join("state"),
            uid,
        )
    }
}

impl NativeMaintenanceJournalFiles for NativeMaintenanceJournalHostFiles {
    /// Read one bounded single-link private journal through a stable descriptor.
    fn read_journal(&mut self) -> Result<Vec<u8>, NativeMaintenanceJournalError> {
        let path = self.root.join("journal.jsonl");
        // preserve only genuine absence as the source's empty genesis journal
        let mut file = match open_read_no_follow(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(NativeMaintenanceJournalError::File),
        };
        let before = file
            .metadata()
            .map_err(|_| NativeMaintenanceJournalError::File)?;
        // retain the source's regular, private, single-link, owner, and size checks
        if !before.is_file()
            || before.nlink() != 1
            || before.uid() != self.uid
            || before.permissions().mode() & 0o777 != 0o600
            || before.size() > NATIVE_MAINTENANCE_JOURNAL_MAXIMUM_BYTES as u64
        {
            return Err(NativeMaintenanceJournalError::File);
        }
        let mut bytes = Vec::with_capacity(before.size() as usize);
        (&mut file)
            .take(NATIVE_MAINTENANCE_JOURNAL_MAXIMUM_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| NativeMaintenanceJournalError::File)?;
        let after = file
            .metadata()
            .map_err(|_| NativeMaintenanceJournalError::File)?;
        // reject replacement, append, truncation, or mutation during the bounded read
        if bytes.len() > NATIVE_MAINTENANCE_JOURNAL_MAXIMUM_BYTES
            || bytes.len() as u64 != before.size()
            || before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.size() != after.size()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
        {
            return Err(NativeMaintenanceJournalError::File);
        }
        Ok(bytes)
    }
}

/// Verify the complete bounded canonical generation and predecessor hash chain.
pub fn verify_native_maintenance_journal_bytes(
    bytes: &[u8],
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Vec<NativeMaintenanceJournalRecord>, NativeMaintenanceJournalError> {
    use NativeMaintenanceJournalError as Error;

    // preserve the predecessor's exact byte ceiling before allocating decoded text
    if bytes.len() > NATIVE_MAINTENANCE_JOURNAL_MAXIMUM_BYTES {
        return Err(Error::Bytes);
    }
    // preserve the exact empty genesis journal
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    // require the source's one final line-feed framing byte
    if bytes.last() != Some(&b'\n') {
        return Err(Error::Bytes);
    }
    // match Buffer.toString by admitting the decoded string that canonical JSON re-emits
    let decoded = String::from_utf8_lossy(bytes);
    let body = decoded.strip_suffix('\n').ok_or(Error::Bytes)?;
    let mut records = Vec::new();
    let mut previous_sha256: Option<String> = None;
    // verify every exact generation against its immediate predecessor
    for (index, line) in body.split('\n').enumerate() {
        let mut canonical_line = Vec::with_capacity(line.len() + 1);
        canonical_line.extend_from_slice(line.as_bytes());
        canonical_line.push(b'\n');
        let value = codec
            .parse_canonical_object(&canonical_line)
            .ok_or(Error::Record)?;
        // independently retain the source's canonical line comparison
        if codec.encode_canonical_json(&value).as_deref() != Some(canonical_line.as_slice()) {
            return Err(Error::Record);
        }
        let generation = u64::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1))
            .ok_or(Error::Chain)?;
        let record = verify_record(value, generation, previous_sha256.as_deref(), codec, time)?;
        previous_sha256 = Some(record.record_sha256.clone());
        records.push(record);
    }
    Ok(records)
}

/// Validate one complete source row and both canonical content identities.
fn verify_record(
    value: JsonValue,
    expected_generation: u64,
    expected_previous_sha256: Option<&str>,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<NativeMaintenanceJournalRecord, NativeMaintenanceJournalError> {
    use NativeMaintenanceJournalError as Error;

    let expected_generation_text = expected_generation.to_string();
    // close the record shape before interpreting any chain authority
    if !exact_keys(&value, JOURNAL_KEYS)
        || text(&value, "contractVersion").as_deref()
            != Some(NATIVE_MAINTENANCE_JOURNAL_CONTRACT_VERSION)
        || text(&value, "generation").as_deref() != Some(expected_generation_text.as_str())
    {
        return Err(Error::Chain);
    }
    let previous_sha256 = nullable_hash(&value, "previousSha256").ok_or(Error::Chain)?;
    // require strict null at genesis and exact predecessor identity afterward
    if previous_sha256.as_deref() != expected_previous_sha256 {
        return Err(Error::Chain);
    }
    let record_key = text(&value, "recordKey").ok_or(Error::Record)?;
    let due_key = nullable_text(&value, "dueKey").ok_or(Error::Record)?;
    let recorded_at = text(&value, "recordedAt").ok_or(Error::Record)?;
    let payload_sha256 = text(&value, "payloadSha256").ok_or(Error::Record)?;
    let record_sha256 = text(&value, "recordSha256").ok_or(Error::Record)?;
    // retain the exact source key, due-key, clock, and digest grammars
    if !valid_record_key(&record_key)
        || due_key
            .as_deref()
            .is_some_and(|value| !valid_due_key(value))
        || !valid_journal_instant(&recorded_at, time)
        || !valid_hash(&payload_sha256)
        || !valid_hash(&record_sha256)
    {
        return Err(Error::Record);
    }
    let payload = field(&value, "payload").ok_or(Error::Record)?.clone();
    // require the same ordinary object boundary as the source verifier
    if !matches!(payload, JsonValue::Object(_)) {
        return Err(Error::Record);
    }
    let payload_bytes = codec.encode_canonical_json(&payload).ok_or(Error::Codec)?;
    let computed_payload_sha256 = codec.sha256(&payload_bytes).ok_or(Error::Codec)?;
    // bind the complete recursively canonical payload bytes
    if computed_payload_sha256.as_str() != payload_sha256 {
        return Err(Error::PayloadIdentity);
    }
    let without_hash = without_field(&value, "recordSha256").ok_or(Error::Record)?;
    let record_bytes = codec
        .encode_canonical_json(&without_hash)
        .ok_or(Error::Codec)?;
    let computed_record_sha256 = codec.sha256(&record_bytes).ok_or(Error::Codec)?;
    // bind every source record field except its self-address
    if computed_record_sha256.as_str() != record_sha256 {
        return Err(Error::RecordIdentity);
    }
    Ok(NativeMaintenanceJournalRecord {
        generation: expected_generation,
        kind: field(&value, "kind").ok_or(Error::Record)?.clone(),
        record_key,
        due_key,
        recorded_at,
        previous_sha256,
        payload_sha256,
        payload,
        record_sha256,
        value,
    })
}

/// Decode the source's strict null-or-string predecessor representation.
fn nullable_hash(value: &JsonValue, key: &str) -> Option<Option<String>> {
    // preserve null independently from every string value
    match field(value, key)? {
        JsonValue::Null => Some(None),
        JsonValue::String(units) => Some(Some(String::from_utf16(units).ok()?)),
        _ => None,
    }
}

/// Decode the source's strict null-or-string due-key representation.
fn nullable_text(value: &JsonValue, key: &str) -> Option<Option<String>> {
    // preserve null independently from every string value
    match field(value, key)? {
        JsonValue::Null => Some(None),
        JsonValue::String(units) => Some(Some(String::from_utf16(units).ok()?)),
        _ => None,
    }
}

/// Remove one exact member while retaining every other decoded source value.
fn without_field(value: &JsonValue, removed: &str) -> Option<JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    // omit only the record self-address before canonical hashing
    Some(JsonValue::Object(
        fields
            .iter()
            .filter(|(name, _)| !name.iter().copied().eq(removed.encode_utf16()))
            .cloned()
            .collect(),
    ))
}

/// Recognize the predecessor's bounded ASCII journal record-key grammar.
fn valid_record_key(value: &str) -> bool {
    (1..=512).contains(&value.len())
        && !value.contains("..")
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| valid_record_key_byte(byte, index == 0))
}

/// Recognize one ASCII record-key byte at its source position.
fn valid_record_key_byte(byte: u8, first: bool) -> bool {
    byte.is_ascii_alphanumeric() || (!first && matches!(byte, b':' | b'.' | b'_' | b'/' | b'-'))
}

/// Recognize the predecessor's closed due-family and bounded suffix grammar.
fn valid_due_key(value: &str) -> bool {
    // split exactly once at the required family separator
    let Some((family, suffix)) = value.split_once('/') else {
        return false;
    };
    (1..=256).contains(&value.len())
        && matches!(
            family,
            "archive"
                | "capture"
                | "daily"
                | "monthly"
                | "control-reference"
                | "confirmation"
                | "release"
                | "rollback"
        )
        && !suffix.is_empty()
        && !value.contains("..")
        && !value.contains("//")
        && suffix.bytes().all(valid_due_key_byte)
}

/// Recognize one ASCII byte admitted in a due-key suffix.
fn valid_due_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'.' | b'_' | b'/' | b'-')
}

/// Admit only the source's four-digit canonical UTC millisecond instant.
fn valid_journal_instant(value: &str, time: &dyn CurrentTimeResolver) -> bool {
    let shape = b"dddd-dd-ddTdd:dd:dd.dddZ";
    value.len() == shape.len()
        && shape
            .iter()
            .zip(value.bytes())
            .all(|(&expected, observed)| {
                // require decimal digits at every variable source position
                if expected == b'd' {
                    observed.is_ascii_digit()
                } else {
                    observed == expected
                }
            })
        && resolve_operator_capture_clock(value, time).is_some()
}
