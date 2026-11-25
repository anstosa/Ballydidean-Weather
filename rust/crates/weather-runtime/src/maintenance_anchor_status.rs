//! Read the original value-free maintenance anchor status without claiming action authority.

use std::fmt::{Display, Formatter};
use std::fs;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::MaintenanceCanonicalJson;
use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::capture_epoch_witness::resolve_operator_capture_clock;
use crate::maintenance_anchor_status_v2::validate_v2_anchor;
use crate::maintenance_anchor_status_v3::validate_v3_anchor;
use crate::maintenance_canonical_json::{field, text, valid_hash};

const EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";
pub(crate) const MAXIMUM_ANCHOR_BYTES: usize = 16 * 1024;
const STATUS_VERSION: &str = "adjustment-maintenance-anchor-status/v2";
const V3_ANCHOR_VERSION: &str = "adjustment-maintenance-anchor/v3";

/// State projected only after the corresponding complete anchor schema is admitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaintenanceAnchorState {
    Transferred,
    Finalized,
}

impl MaintenanceAnchorState {
    // map only the original two admitted wire states
    fn wire(self) -> &'static str {
        match self {
            Self::Transferred => "transferred",
            Self::Finalized => "finalized",
        }
    }
}

/// The three distinct fixed slots that status reads without recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum MaintenanceAnchorSlot {
    Current,
    Pending,
    Previous,
}

impl MaintenanceAnchorSlot {
    const ORDERED: [Self; 3] = [Self::Current, Self::Pending, Self::Previous];

    // bind the slot to its fixed retained file name
    pub(crate) fn filename(self) -> &'static str {
        match self {
            Self::Current => "current.json",
            Self::Pending => "pending.json",
            Self::Previous => "previous.json",
        }
    }
}

/// Contain malformed slot or file outcomes before emitting an operator projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaintenanceAnchorStatusError {
    Arguments,
    DevelopmentCustodyArguments,
    RainControlCustodyArguments,
    Root,
    UnknownEntry,
    Slot,
    Codec,
}

impl Display for MaintenanceAnchorStatusError {
    /// Return a bounded failure without projecting the underlying anchor contents.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "maintenance-anchor-status-v2 takes no arguments",
            Self::DevelopmentCustodyArguments => {
                "development-custody-anchor-current-v1 takes no arguments"
            }
            Self::RainControlCustodyArguments => {
                "rain-control-custody-anchor-current-v1 takes no arguments"
            }
            Self::Root => "maintenance anchor root is unsafe",
            Self::UnknownEntry => "maintenance anchor root contains an unknown entry",
            Self::Slot => "maintenance anchor slot is invalid",
            Self::Codec => "maintenance anchor status serialization failed",
        })
    }
}

impl std::error::Error for MaintenanceAnchorStatusError {}

/// The selected canonical codec and the existing JS coercion boundary used by regex fields.
pub trait MaintenanceAnchorStatusJson: MaintenanceCanonicalJson {
    /// Perform the original String conversion for the regex-validated fields.
    fn js_string(&mut self, value: &JsonValue) -> Option<Vec<u16>>;
}

/// Read the verified directory and fixed slots without installing or recovering anything.
pub trait MaintenanceAnchorStatusFiles {
    /// Return absent root or its observed verified names.
    fn read_root(&mut self) -> Result<Option<Vec<String>>, MaintenanceAnchorStatusError>;

    /// Read one bounded private canonical slot or expose its genuine absence.
    fn read_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
    ) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError>;
}

/// Actual read-only filesystem owner for the predecessor's maintenance anchor slots.
pub struct NativeMaintenanceAnchorStatusFiles {
    anchor_root: PathBuf,
    uid: u32,
    gid: u32,
}

impl NativeMaintenanceAnchorStatusFiles {
    /// Bind the fixed installed evidence directory and independently sampled real identity.
    pub fn native(uid: u32, gid: u32) -> Self {
        Self::for_evidence_root(PathBuf::from(EVIDENCE_ROOT), uid, gid)
    }

    /// Bind an already selected evidence root and expected owner.
    pub fn for_evidence_root(root: PathBuf, uid: u32, gid: u32) -> Self {
        Self {
            anchor_root: root.join("maintenance-anchors"),
            uid,
            gid,
        }
    }
}

impl MaintenanceAnchorStatusFiles for NativeMaintenanceAnchorStatusFiles {
    /// Inspect only the existing private root, preserving genuine absence.
    fn read_root(&mut self) -> Result<Option<Vec<String>>, MaintenanceAnchorStatusError> {
        use MaintenanceAnchorStatusError::{Root, UnknownEntry};
        // preserve the original closed directory inventory only for maintenance status
        if !verified_private_anchor_root(&self.anchor_root, self.uid, self.gid)? {
            return Ok(None);
        }
        let mut names = fs::read_dir(&self.anchor_root)
            .map_err(|_| Root)?
            // expose directory names alone to the closed slot inventory
            .map(|entry| {
                entry
                    .map_err(|_| Root)?
                    .file_name()
                    .into_string()
                    .map_err(|_| UnknownEntry)
            })
            .collect::<Result<Vec<_>, _>>()?;
        names.sort_unstable();
        Ok(Some(names))
    }

    /// Open one exact private slot and detect changes across its bounded descriptor read.
    fn read_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
    ) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError> {
        read_private_anchor_slot(&self.anchor_root.join(slot.filename()), self.uid, self.gid)
    }
}

// share the exact private root admission without imposing an inventory on current-only readers
pub(crate) fn verified_private_anchor_root(
    path: &Path,
    uid: u32,
    gid: u32,
) -> Result<bool, MaintenanceAnchorStatusError> {
    use MaintenanceAnchorStatusError::Root;
    // preserve the distinction between absent and present but invalid state
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(Root),
    };
    // require the original directory ownership and canonical path
    if !metadata.is_dir()
        || metadata.uid() != uid
        || metadata.gid() != gid
        || metadata.mode() & 0o777 != 0o700
        || fs::canonicalize(path).map_err(|_| Root)? != path
    {
        return Err(Root);
    }
    Ok(true)
}

// share the exact bounded stable-inode private slot read across maintenance and custody roots
pub(crate) fn read_private_anchor_slot(
    path: &Path,
    uid: u32,
    gid: u32,
) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError> {
    use MaintenanceAnchorStatusError::Slot;
    // keep only missing paths optional
    let mut file = match open_read_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Slot),
    };
    let before = file.metadata().map_err(|_| Slot)?;
    let size = usize::try_from(before.size()).map_err(|_| Slot)?;
    // retain the original bounded private single-link file authority
    if !before.is_file()
        || before.uid() != uid
        || before.gid() != gid
        || before.nlink() != 1
        || before.mode() & 0o777 != 0o600
        || !(1..=MAXIMUM_ANCHOR_BYTES).contains(&size)
    {
        return Err(Slot);
    }
    let mut bytes = Vec::with_capacity(size);
    (&mut file)
        .take(MAXIMUM_ANCHOR_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Slot)?;
    let after = file.metadata().map_err(|_| Slot)?;
    // compare the same inode, size and nanosecond clocks after reading
    if bytes.len() != size
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.size() != after.size()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(Slot);
    }
    Ok(Some(bytes))
}

/// Execute the original closed status verb with no state mutation or recovery authority.
pub fn read_maintenance_anchor_status_command(
    action: &str,
    args: &[&str],
    files: &mut impl MaintenanceAnchorStatusFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError> {
    use MaintenanceAnchorStatusError::{Arguments, Codec, UnknownEntry};
    // leave other retained operator commands to their owners
    if action != "maintenance-anchor-status-v2" {
        return Ok(None);
    }
    // reject operands before observing the directory
    if !args.is_empty() {
        return Err(Arguments);
    }
    let names = files.read_root()?;
    let privacy = if names.is_some() {
        "verified_private"
    } else {
        "unavailable"
    };
    let root = if names.as_ref().is_some_and(|names| !names.is_empty()) {
        "verified"
    } else {
        "unavailable"
    };
    let mut slots = [
        SlotProjection::Absent,
        SlotProjection::Absent,
        SlotProjection::Absent,
    ];
    // a wholly absent root must not consult the fixed slots
    if let Some(names) = names {
        // retain the closed three-member directory before reading any slot
        if names.iter().any(|name| {
            !MaintenanceAnchorSlot::ORDERED
                .iter()
                .any(|slot| name == slot.filename())
        }) {
            return Err(UnknownEntry);
        }
        // project every original slot in order, including currently absent ones
        for (slot, projection) in MaintenanceAnchorSlot::ORDERED.into_iter().zip(&mut slots) {
            if let Some(bytes) = files.read_slot(slot)? {
                *projection = project_slot(&bytes, codec, time)?;
            }
        }
    }
    codec
        .encode_canonical_json(&status_json(privacy, root, slots))
        .map(Some)
        .ok_or(Codec)
}

enum SlotProjection {
    Absent,
    Present {
        checksum: String,
        size: usize,
        state: MaintenanceAnchorState,
    },
}

// admit the complete original schema before exposing only its small status projection
fn project_slot(
    bytes: &[u8],
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<SlotProjection, MaintenanceAnchorStatusError> {
    use MaintenanceAnchorStatusError::{Codec, Slot};
    // keep the original canonical byte bounds even when a fixture provides already decoded data
    if !(2..=MAXIMUM_ANCHOR_BYTES).contains(&bytes.len()) {
        return Err(Slot);
    }
    let value = codec.parse_canonical_object(bytes).ok_or(Slot)?;
    let state = if text(&value, "contractVersion").as_deref() == Some(V3_ANCHOR_VERSION) {
        validate_v3_anchor(&value, codec, time)
    } else {
        validate_v2_anchor(&value, codec, time)
    }
    .ok_or(Slot)?;
    // require the exact predecessor canonical JSON plus its single LF
    if codec.encode_canonical_json(&value).as_deref() != Some(bytes) {
        return Err(Slot);
    }
    let checksum = codec.sha256(bytes).ok_or(Codec)?;
    Ok(SlotProjection::Present {
        checksum: checksum.as_str().to_owned(),
        size: bytes.len(),
        state,
    })
}

// encode the sorted original status object with constant non-authoritative readiness
fn status_json(privacy: &str, root: &str, slots: [SlotProjection; 3]) -> JsonValue {
    let [current, pending, previous] = slots;
    let readiness = if root != "verified" {
        "not_established"
    } else if matches!(
        current,
        SlotProjection::Present {
            state: MaintenanceAnchorState::Finalized,
            ..
        }
    ) {
        "finalized_anchor_installed"
    } else {
        "transferred_anchor_only"
    };
    JsonValue::Object(vec![
        entry("actionEligible", JsonValue::Boolean(false)),
        string_entry("contractVersion", STATUS_VERSION),
        string_entry("privacyState", privacy),
        string_entry("rootState", root),
        string_entry("schemaReadiness", readiness),
        entry(
            "slots",
            JsonValue::Object(vec![
                entry("current", slot_json(current)),
                entry("pending", slot_json(pending)),
                entry("previous", slot_json(previous)),
            ]),
        ),
    ])
}

// return the exact absent or present bounded slot object without raw anchor fields
fn slot_json(slot: SlotProjection) -> JsonValue {
    let (checksum, size, state) = match slot {
        SlotProjection::Absent => (JsonValue::Null, JsonValue::Null, "absent"),
        SlotProjection::Present {
            checksum,
            size,
            state,
        } => (
            JsonValue::String(checksum.encode_utf16().collect()),
            JsonValue::Number(size as f64),
            state.wire(),
        ),
    };
    JsonValue::Object(vec![
        entry("sha256", checksum),
        entry("sizeBytes", size),
        string_entry("state", state),
    ])
}

// encode a generated JSON property name without altering its value
fn entry(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

// encode one original string field
fn string_entry(key: &str, value: &str) -> (Vec<u16>, JsonValue) {
    entry(key, JsonValue::String(value.encode_utf16().collect()))
}

// read the exact predecessor state discriminator
pub(crate) fn anchor_state(value: &JsonValue) -> Option<MaintenanceAnchorState> {
    match text(value, "ctfState")?.as_str() {
        "transferred" => Some(MaintenanceAnchorState::Transferred),
        "finalized" => Some(MaintenanceAnchorState::Finalized),
        _ => None,
    }
}

// require the exact base keys and only the state-dependent additional finalization keys
pub(crate) fn exact_anchor_keys(
    value: &JsonValue,
    base: &[&str],
    finalized: &[&str],
    state: MaintenanceAnchorState,
) -> bool {
    let finalized = if state == MaintenanceAnchorState::Finalized {
        finalized
    } else {
        &[]
    };
    matches!(value, JsonValue::Object(fields) if fields.len() == base.len() + finalized.len())
        && base
            .iter()
            .chain(finalized)
            .all(|key| field(value, key).is_some())
}

// admit a canonical unsigned decimal string with the exact uint64 bound
pub(crate) fn uint64_field(value: &JsonValue, key: &str) -> Option<u64> {
    let digits = text(value, key)?;
    // reject alternate decimal encodings before parsing
    if digits.is_empty()
        || (digits.len() > 1 && digits.starts_with('0'))
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    digits.parse().ok()
}

// bind one canonical JS UTC instant to its exact comparison clock
pub(crate) fn clock_field(
    value: &JsonValue,
    key: &str,
    time: &dyn CurrentTimeResolver,
) -> Option<i64> {
    let instant = text(value, key)?;
    resolve_operator_capture_clock(&instant, time).map(|resolved| resolved.epoch_milliseconds)
}

// require one exact lowercase SHA-256 field
pub(crate) fn hash_field(value: &JsonValue, key: &str) -> bool {
    text(value, key).as_deref().is_some_and(valid_hash)
}

// retain the original explicit null or admitted lowercase hash
pub(crate) fn nullable_hash_field(value: &JsonValue, key: &str) -> bool {
    matches!(field(value, key), Some(JsonValue::Null)) || hash_field(value, key)
}

// preserve regex coercion only for the predecessor fields that used it
pub(crate) fn regex_text(
    value: &JsonValue,
    key: &str,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Option<String> {
    // avoid crossing the dynamic scalar boundary for ordinary string fields
    if let Some(value) = text(value, key) {
        return Some(value);
    }
    let units = codec.js_string(field(value, key)?)?;
    String::from_utf16(&units).ok()
}

// retain the exact positive one-to-six digit control version
pub(crate) fn valid_control_version(value: &str) -> bool {
    (1..=6).contains(&value.len())
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

// retain the exact forty lowercase hexadecimal Git source identity
pub(crate) fn valid_source_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    use super::*;

    struct FixtureRoot(PathBuf);

    impl Drop for FixtureRoot {
        // remove only the fixture directory owned by this test
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Observe genuine missing, empty and populated slots without changing retained bytes.
    #[test]
    fn native_status_reads_only_the_fixed_existing_slots() {
        let path = std::env::temp_dir().join(format!(
            "weather-maintenance-anchor-status-{}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("unique disposable root");
        let root = FixtureRoot(path);
        let directory = fs::canonicalize(&root.0).expect("canonical fixture root");
        let owner = fs::metadata(&directory).expect("fixture owner");
        let mut files = NativeMaintenanceAnchorStatusFiles::for_evidence_root(
            directory.clone(),
            owner.uid(),
            owner.gid(),
        );
        assert_eq!(files.read_root().unwrap(), None);

        let anchor_root = directory.join("maintenance-anchors");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&anchor_root)
            .unwrap();
        assert_eq!(files.read_root().unwrap(), Some(Vec::new()));
        let path = anchor_root.join("pending.json");
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .unwrap();
            file.write_all(b"{\"retained\":true}\n").unwrap();
        }
        assert_eq!(
            files.read_root().unwrap(),
            Some(vec!["pending.json".to_owned()])
        );
        assert_eq!(
            files.read_slot(MaintenanceAnchorSlot::Current).unwrap(),
            None
        );
        assert_eq!(
            files
                .read_slot(MaintenanceAnchorSlot::Pending)
                .unwrap()
                .as_deref(),
            Some(&b"{\"retained\":true}\n"[..])
        );
        assert_eq!(
            files.read_slot(MaintenanceAnchorSlot::Previous).unwrap(),
            None
        );
        assert_eq!(fs::read(&path).unwrap(), b"{\"retained\":true}\n");
    }
}
