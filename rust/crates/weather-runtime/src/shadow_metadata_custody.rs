//! Read compact custody state and finish only its already durable consumption checkpoint.

use std::fmt::{Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::PathBuf;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::maintenance_anchor_status::{clock_field, hash_field};
use crate::maintenance_canonical_json::{exact_keys, text, valid_hash};
use crate::shadow_metadata_custody_preparation::validate_shadow_custody_preparation;
use crate::shadow_metadata_custody_proof::validate_shadow_custody_proof;
use crate::{MaintenanceAnchorStatusJson, MaintenanceCanonicalJson};

const EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";
pub(crate) const MAXIMUM_DOCUMENT_BYTES: usize = 16 * 1024;
// linux directory descriptor flags match the installed x86 and arm64 targets
#[cfg(target_arch = "x86_64")]
const DIRECTORY_NOFOLLOW: i32 = (1 << 16) | (1 << 17);
#[cfg(target_arch = "aarch64")]
const DIRECTORY_NOFOLLOW: i32 = (1 << 14) | (1 << 15);
const STATUS_VERSION: &str = "adjustment-shadow-metadata-custody-status/v1";
pub(crate) const CONSUMPTION_VERSION: &str = "adjustment-shadow-metadata-custody-consumption/v1";

/// Fixed compact custody documents selected by the retained store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShadowMetadataCustodySlot {
    Current,
    Pending,
    Preparation,
    Previous,
    PreviousPreparation,
    PreviousConsumption,
}

impl ShadowMetadataCustodySlot {
    // retain the fixed predecessor slot filenames
    pub(crate) fn filename(self) -> &'static str {
        match self {
            Self::Current => "current.json",
            Self::Pending => "pending.json",
            Self::Preparation => "preparation.json",
            Self::Previous => "previous.json",
            Self::PreviousPreparation => "previous-preparation.json",
            Self::PreviousConsumption => "previous-consumption.json",
        }
    }
}

/// Only the two already consumed transient markers may be retired by the readers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShadowMetadataCompletedMarker {
    Current,
    Preparation,
}

/// Bounded command failure categories without path or raw content disclosure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShadowMetadataCustodyError {
    Arguments,
    Root,
    Slot,
    Codec,
    Checkpoint,
    ProofMismatch,
    Status,
    Remove,
    Sync,
    Write,
}

impl Display for ShadowMetadataCustodyError {
    /// Retain the original operator failure categories for the terminal owner.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "shadow metadata custody command arguments are invalid",
            Self::Root => "adjustment shadow metadata custody root is unsafe",
            Self::Slot => "adjustment shadow metadata custody document is invalid",
            Self::Codec => "adjustment shadow metadata custody serialization failed",
            Self::Checkpoint => "adjustment shadow metadata consumption checkpoint differs",
            Self::ProofMismatch => "adjustment shadow metadata consumption proof differs",
            Self::Status => "adjustment shadow metadata custody status differs",
            Self::Remove => "adjustment shadow metadata consumption cleanup failed",
            Self::Sync => "adjustment shadow metadata consumption directory sync failed",
            Self::Write => "adjustment shadow metadata custody write failed",
        })
    }
}

impl std::error::Error for ShadowMetadataCustodyError {}

/// The original fixed slot and already-completed marker operations.
pub trait ShadowMetadataCustodyFiles {
    /// Admit the current private root or return genuine absence.
    fn verified_root_present(&mut self) -> Result<bool, ShadowMetadataCustodyError>;

    /// Read one existing bounded document without interpreting that document as authority.
    fn read_slot(
        &mut self,
        slot: ShadowMetadataCustodySlot,
    ) -> Result<Option<Vec<u8>>, ShadowMetadataCustodyError>;

    /// Retire only one verified completed transient marker.
    fn remove_completed_marker(
        &mut self,
        marker: ShadowMetadataCompletedMarker,
    ) -> Result<(), ShadowMetadataCustodyError>;

    /// Flush the existing proof directory after completed consumption recovery.
    fn sync_root(&mut self) -> Result<(), ShadowMetadataCustodyError>;
}

/// Actual native file effects for the original owner-selected compact custody directory.
pub struct NativeShadowMetadataCustodyFiles {
    pub(crate) proof_root: PathBuf,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
}

impl NativeShadowMetadataCustodyFiles {
    /// Select the fixed installed directory using the admitted real process identity.
    pub fn native(uid: u32, gid: u32) -> Self {
        Self::for_evidence_root(PathBuf::from(EVIDENCE_ROOT), uid, gid)
    }

    /// Bind a selected evidence root without performing any file operation.
    pub fn for_evidence_root(root: PathBuf, uid: u32, gid: u32) -> Self {
        Self {
            proof_root: root.join("shadow-metadata-custody-proofs"),
            uid,
            gid,
        }
    }
}

impl ShadowMetadataCustodyFiles for NativeShadowMetadataCustodyFiles {
    /// Observe the existing owner-private root for current-proof export.
    fn verified_root_present(&mut self) -> Result<bool, ShadowMetadataCustodyError> {
        use ShadowMetadataCustodyError::Root;
        // preserve genuine absence without creating directories
        let metadata = match fs::symlink_metadata(&self.proof_root) {
            Ok(value) => value,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(Root),
        };
        // retain the original proof-root ownership and canonical location
        if !metadata.is_dir()
            || metadata.uid() != self.uid
            || metadata.gid() != self.gid
            || metadata.mode() & 0o777 != 0o700
            || fs::canonicalize(&self.proof_root).map_err(|_| Root)? != self.proof_root
        {
            return Err(Root);
        }
        Ok(true)
    }

    /// Read a fixed slot through the original bounded private regular-file boundary.
    fn read_slot(
        &mut self,
        slot: ShadowMetadataCustodySlot,
    ) -> Result<Option<Vec<u8>>, ShadowMetadataCustodyError> {
        use ShadowMetadataCustodyError::Slot;
        let path = self.proof_root.join(slot.filename());
        // distinguish only ordinary missing files
        let file = match open_read_no_follow(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(Slot),
        };
        let metadata = file.metadata().map_err(|_| Slot)?;
        // match the original shared private-file mode and size admission
        if !metadata.is_file()
            || metadata.mode() & 0o777 != 0o600
            || metadata.size() > MAXIMUM_DOCUMENT_BYTES as u64
        {
            return Err(Slot);
        }
        let mut bytes = Vec::new();
        file.take(MAXIMUM_DOCUMENT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Slot)?;
        // never pass a grown oversized document through to the canonical owner
        if bytes.len() > MAXIMUM_DOCUMENT_BYTES {
            return Err(Slot);
        }
        Ok(Some(bytes))
    }

    /// Remove only the already-consumed current or preparation marker selected by recovery.
    fn remove_completed_marker(
        &mut self,
        marker: ShadowMetadataCompletedMarker,
    ) -> Result<(), ShadowMetadataCustodyError> {
        let slot = match marker {
            ShadowMetadataCompletedMarker::Current => ShadowMetadataCustodySlot::Current,
            ShadowMetadataCompletedMarker::Preparation => ShadowMetadataCustodySlot::Preparation,
        };
        fs::remove_file(self.proof_root.join(slot.filename()))
            .map_err(|_| ShadowMetadataCustodyError::Remove)
    }

    /// Commit the original directory persistence boundary for interrupted-consumption completion.
    fn sync_root(&mut self) -> Result<(), ShadowMetadataCustodyError> {
        OpenOptions::new()
            .read(true)
            .custom_flags(DIRECTORY_NOFOLLOW)
            .open(&self.proof_root)
            .and_then(|file| file.sync_all())
            .map_err(|_| ShadowMetadataCustodyError::Sync)
    }
}

/// Execute the two original status verbs, preserving their narrow completed-consumption recovery.
pub fn read_shadow_metadata_custody_command(
    action: &str,
    args: &[&str],
    files: &mut impl ShadowMetadataCustodyFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<Vec<u8>>, ShadowMetadataCustodyError> {
    use ShadowMetadataCustodyError::{Arguments, Codec, ProofMismatch};
    use ShadowMetadataCustodySlot::PreviousConsumption;

    let hash = match action {
        "shadow-metadata-custody-status-v1" if args.is_empty() => {
            return read_custody_status(files, codec, time).map(|(_, bytes)| Some(bytes));
        }
        "shadow-metadata-custody-consumed-v1" if args.len() == 1 && valid_hash(args[0]) => args[0],
        "shadow-metadata-custody-status-v1" | "shadow-metadata-custody-consumed-v1" => {
            return Err(Arguments);
        }
        _ => return Ok(None),
    };
    // recover only a durable predecessor checkpoint before reading its exact receipt
    finish_consumption(files, codec, time)?;
    let value = if let Some(value) = read_document(PreviousConsumption, files, codec, time)? {
        // never expose a different proof from the bounded last-consumed checkpoint
        if text(&value, "proofSha256").as_deref() != Some(hash) {
            return Err(ProofMismatch);
        }
        value
    } else {
        JsonValue::Null
    };
    let bytes = codec.encode_canonical_json(&value).ok_or(Codec)?;
    // preserve the original combined command output ceiling
    if bytes.len() > 2 * MAXIMUM_DOCUMENT_BYTES {
        return Err(ShadowMetadataCustodyError::Status);
    }
    Ok(Some(bytes))
}

// read and close the status once for both archive and owner command entrypoints
pub(crate) fn read_custody_status(
    files: &mut impl ShadowMetadataCustodyFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(JsonValue, Vec<u8>), ShadowMetadataCustodyError> {
    use ShadowMetadataCustodyError::{Codec, Status};
    use ShadowMetadataCustodySlot::{Current, Preparation};
    finish_consumption(files, codec, time)?;
    // verify the root only at the same current-proof export boundary
    let proof = if files.verified_root_present()? {
        read_document(Current, files, codec, time)?
    } else {
        None
    };
    let preparation = read_document(Preparation, files, codec, time)?;
    // do not expose orphaned or cross-proof preparation state
    if let Some(preparation) = preparation.as_ref() {
        let proof = proof.as_ref().ok_or(Status)?;
        if text(preparation, "proofSha256") != text(proof, "proofSha256") {
            return Err(Status);
        }
    }
    let value = JsonValue::Object(vec![
        (
            "contractVersion".encode_utf16().collect(),
            JsonValue::String(STATUS_VERSION.encode_utf16().collect()),
        ),
        (
            "preparation".encode_utf16().collect(),
            preparation.unwrap_or(JsonValue::Null),
        ),
        (
            "proof".encode_utf16().collect(),
            proof.unwrap_or(JsonValue::Null),
        ),
    ]);
    let bytes = codec.encode_canonical_json(&value).ok_or(Codec)?;
    // preserve the original combined status output ceiling
    if bytes.len() > 2 * MAXIMUM_DOCUMENT_BYTES {
        return Err(Status);
    }
    Ok((value, bytes))
}

// recover only a matching durable consumption with both retained redundant copies
pub(crate) fn finish_consumption(
    files: &mut impl ShadowMetadataCustodyFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), ShadowMetadataCustodyError> {
    use ShadowMetadataCustodyError::Checkpoint;
    use ShadowMetadataCustodySlot::{
        Current, Preparation, Previous, PreviousConsumption, PreviousPreparation,
    };
    let Some(consumed) = read_document(PreviousConsumption, files, codec, time)? else {
        return Ok(());
    };
    let current = read_document(Current, files, codec, time)?;
    let preparation = read_document(Preparation, files, codec, time)?;
    let proof = text(&consumed, "proofSha256").ok_or(Checkpoint)?;
    let prepared = text(&consumed, "preparationSha256").ok_or(Checkpoint)?;
    // a later proof or preparation must remain untouched by an earlier terminal checkpoint
    if current
        .as_ref()
        .is_some_and(|value| text(value, "proofSha256").as_deref() != Some(proof.as_str()))
        || preparation.as_ref().is_some_and(|value| {
            text(value, "preparationSha256").as_deref() != Some(prepared.as_str())
        })
    {
        return Ok(());
    }
    let previous = read_document(Previous, files, codec, time)?;
    let previous_preparation = read_document(PreviousPreparation, files, codec, time)?;
    // both exact retained documents must agree with the durable terminal receipt
    if previous
        .as_ref()
        .and_then(|value| text(value, "proofSha256"))
        .as_deref()
        != Some(proof.as_str())
        || previous_preparation
            .as_ref()
            .and_then(|value| text(value, "preparationSha256"))
            .as_deref()
            != Some(prepared.as_str())
    {
        return Err(Checkpoint);
    }
    // preserve exact removal order and require directory persistence even on an already cleaned retry
    if current.is_some() {
        files.remove_completed_marker(ShadowMetadataCompletedMarker::Current)?;
    }
    if preparation.is_some() {
        files.remove_completed_marker(ShadowMetadataCompletedMarker::Preparation)?;
    }
    files.sync_root()
}

// parse one exact canonical slot through its complete independent typed validator
pub(crate) fn read_document(
    slot: ShadowMetadataCustodySlot,
    files: &mut impl ShadowMetadataCustodyFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<JsonValue>, ShadowMetadataCustodyError> {
    use ShadowMetadataCustodyError::Slot;
    let Some(bytes) = files.read_slot(slot)? else {
        return Ok(None);
    };
    // retain the prior bounded canonical file contract
    if !(2..=MAXIMUM_DOCUMENT_BYTES).contains(&bytes.len()) {
        return Err(Slot);
    }
    let value = codec.parse_canonical_object(&bytes).ok_or(Slot)?;
    match slot {
        ShadowMetadataCustodySlot::Current
        | ShadowMetadataCustodySlot::Pending
        | ShadowMetadataCustodySlot::Previous => validate_shadow_custody_proof(&value, codec, time),
        ShadowMetadataCustodySlot::Preparation | ShadowMetadataCustodySlot::PreviousPreparation => {
            validate_shadow_custody_preparation(&value, codec, time)
        }
        ShadowMetadataCustodySlot::PreviousConsumption => validate_consumption(&value, time),
    }
    .ok_or(Slot)?;
    // retain original canonical bytes after full schema validation
    if codec.encode_canonical_json(&value).as_deref() != Some(bytes.as_slice()) {
        return Err(Slot);
    }
    Ok(Some(value))
}

// validate the exact durable metadata-consumption checkpoint and terminal state
pub(crate) fn validate_consumption(
    value: &JsonValue,
    time: &dyn CurrentTimeResolver,
) -> Option<()> {
    (exact_keys(
        value,
        &[
            "consumedAt",
            "contractVersion",
            "preparationSha256",
            "proofSha256",
            "state",
        ],
    ) && clock_field(value, "consumedAt", time).is_some()
        && hash_field(value, "preparationSha256")
        && hash_field(value, "proofSha256")
        && text(value, "contractVersion").as_deref() == Some(CONSUMPTION_VERSION)
        && text(value, "state").as_deref() == Some("consumed"))
    .then_some(())
}

// recompute each predecessor's exact canonical unsigned self-address before admitting it
pub(crate) fn canonical_self_address(
    value: &JsonValue,
    key: &str,
    codec: &mut impl MaintenanceCanonicalJson,
) -> Option<()> {
    let expected = text(value, key)?;
    let JsonValue::Object(fields) = value else {
        return None;
    };
    let unsigned = JsonValue::Object(
        fields
            .iter()
            // exclude only the original self-address field
            .filter(|(name, _)| !name.iter().copied().eq(key.encode_utf16()))
            .cloned()
            .collect(),
    );
    let preimage = codec.encode_canonical_json(&unsigned)?;
    let checksum = codec.sha256(&preimage)?;
    (checksum.as_str() == expected
        && codec.encode_canonical_json(value)?.len() <= MAXIMUM_DOCUMENT_BYTES)
        .then_some(())
}
