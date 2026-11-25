//! Install exact custody-only anchor bytes after the original retained custody and epoch checks.

use std::io::Read;
use std::path::Path;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::capture_epoch_witness::verify_operator_capture_epoch_witness;
use crate::custody_anchor_validation::validate_custody_anchor;
use crate::maintenance_anchor_status::{MAXIMUM_ANCHOR_BYTES, uint64_field};
use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::revision_custody_ack_reader::{ACK_V2_VERSION, read_revision_custody_acknowledgements};
use crate::{
    CustodyAnchorKind, MaintenanceAnchorSlot, MaintenanceAnchorStatusError,
    MaintenanceAnchorStatusJson,
};

use MaintenanceAnchorSlot::{Current, Pending, Previous};
use MaintenanceAnchorStatusError::{Codec, Slot};

/// The original fixed service-owner filesystem effects for the custody anchor installers.
pub trait CustodyAnchorInstallFiles {
    /// Borrow the already selected evidence root for genuine packed acknowledgement reads.
    fn evidence_root(&self) -> &Path;

    /// Read the selected root-owned witness bytes.
    fn witness(&mut self) -> Result<Vec<u8>, MaintenanceAnchorStatusError>;

    /// Admit or create only the fixed private anchor child.
    fn ensure_root(&mut self, kind: CustodyAnchorKind) -> Result<(), MaintenanceAnchorStatusError>;

    /// Read one bounded original anchor slot.
    fn read_slot(
        &mut self,
        kind: CustodyAnchorKind,
        slot: MaintenanceAnchorSlot,
    ) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError>;

    /// Create the first complete pending anchor without replacing it.
    fn create_pending(
        &mut self,
        kind: CustodyAnchorKind,
        bytes: &[u8],
    ) -> Result<(), MaintenanceAnchorStatusError>;

    /// Atomically replace only the current or previous selected anchor.
    fn replace_slot(
        &mut self,
        kind: CustodyAnchorKind,
        slot: MaintenanceAnchorSlot,
        bytes: &[u8],
    ) -> Result<(), MaintenanceAnchorStatusError>;

    /// Remove only the already committed fixed pending marker.
    fn remove_pending(
        &mut self,
        kind: CustodyAnchorKind,
    ) -> Result<(), MaintenanceAnchorStatusError>;

    /// Sync the selected private anchor directory.
    fn sync_root(&mut self, kind: CustodyAnchorKind) -> Result<(), MaintenanceAnchorStatusError>;
}

/// Exact control identity selected by the installed service owner rather than anchor stdin.
#[derive(Clone, Copy)]
pub struct CustodyAnchorControl<'a> {
    pub sha256: Option<&'a str>,
    pub version: Option<&'a str>,
}

/// Execute the two original install verbs with bounded input and independently retained authority.
pub fn install_custody_anchor_command(
    action: &str,
    args: &[&str],
    input: &mut impl Read,
    files: &mut impl CustodyAnchorInstallFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
    control: CustodyAnchorControl<'_>,
) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError> {
    // select only the two disjoint custody installation contracts
    let kind = match action {
        "development-custody-anchor-install-v1" => CustodyAnchorKind::Development,
        "rain-control-custody-anchor-install-v1" => CustodyAnchorKind::RainControl,
        _ => return Ok(None),
    };
    let [expected] = args else {
        return Err(Slot);
    };
    // reject invalid hash operands before reading any bytes
    if !valid_hash(expected) {
        return Err(Slot);
    }
    let mut bytes = Vec::new();
    input
        .take(MAXIMUM_ANCHOR_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Slot)?;
    let anchor = parse_anchor(kind, &bytes, codec, time)?;
    // retain the independent active control identity and exact input hash
    if codec.sha256(&bytes).ok_or(Codec)?.as_str() != *expected
        || control.sha256.is_none()
        || control.version.is_none()
        || text(&anchor, "controlSha256").as_deref() != control.sha256
        || text(&anchor, "controlVersion").as_deref() != control.version
    {
        return Err(Slot);
    }
    let custody = read_revision_custody_acknowledgements(files.evidence_root(), codec, time)?;
    // select only the exact retained packed-custody checkpoint
    let selected = custody
        .iter()
        .flatten()
        .find(|entry| {
            text(entry, "contractVersion").as_deref() == Some(ACK_V2_VERSION)
                && field(entry, "acknowledgementSha256")
                    == field(&anchor, "custodyAcknowledgementSha256")
                && field(entry, "custodyCheckpointSha256")
                    == field(&anchor, "custodyCheckpointSha256")
                && field(entry, "pageSha256") == field(&anchor, "pageSha256")
        })
        .ok_or(Slot)?;
    // crossbind every copied custody coordinate before any filesystem mutation
    if ![
        ("acknowledgedAt", "custodyAcknowledgedAt"),
        ("nextArchiveCommitOrdinal", "archiveCommitOrdinal"),
        ("nextFrontierSha256", "frontierSha256"),
        ("memberRootSha256", "memberRootSha256"),
        ("startMemberSha256", "startMemberSha256"),
        ("startSha256", "startSha256"),
    ]
    .iter()
    .all(|(source, target)| field(selected, source) == field(&anchor, target))
    {
        return Err(Slot);
    }
    let witness = files.witness()?;
    verify_operator_capture_epoch_witness(&witness, codec, time).map_err(|_| Slot)?;
    // bind the full exact original witness, not merely its unsigned identity field
    if codec.sha256(&witness).ok_or(Codec)?.as_str()
        != text(&anchor, "captureEpochWitnessSha256").ok_or(Slot)?
    {
        return Err(Slot);
    }

    files.ensure_root(kind)?;
    install_anchor(kind, &anchor, &bytes, expected, files, codec, time)?;
    let contract = match kind {
        CustodyAnchorKind::Development => "adjustment-development-custody-anchor-installation/v1",
        CustodyAnchorKind::RainControl => "adjustment-rain-control-custody-anchor-installation/v1",
    };
    let installed = JsonValue::Object(vec![
        member("anchorSha256", expected),
        member("contractVersion", contract),
        member("state", "installed"),
    ]);
    Ok(Some(codec.encode_canonical_json(&installed).ok_or(Codec)?))
}

// construct the exact UTF-16 string member of a canonical response
fn member(key: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (
        key.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}

// admit a bounded original canonical anchor without granting installation authority
fn parse_anchor(
    kind: CustodyAnchorKind,
    bytes: &[u8],
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    // forbid any alternate representation of the retained canonical object
    if !(2..=MAXIMUM_ANCHOR_BYTES).contains(&bytes.len()) {
        return Err(Slot);
    }
    let anchor = codec.parse_canonical_object(bytes).ok_or(Slot)?;
    validate_custody_anchor(kind, &anchor, codec, time).ok_or(Slot)?;
    // retain the actual bytes already supplied by the independent file owner
    if codec.encode_canonical_json(&anchor).as_deref() != Some(bytes) {
        return Err(Slot);
    }
    Ok(anchor)
}

struct AnchorSlot {
    value: JsonValue,
    bytes: Vec<u8>,
}

// retain both exact original bytes and their decoded validated anchor
fn read_anchor(
    kind: CustodyAnchorKind,
    slot: MaintenanceAnchorSlot,
    files: &mut impl CustodyAnchorInstallFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<AnchorSlot>, MaintenanceAnchorStatusError> {
    files
        .read_slot(kind, slot)?
        .map(|bytes| {
            // validate all retained slots before they may participate in a retry
            let value = parse_anchor(kind, &bytes, codec, time)?;
            Ok(AnchorSlot { value, bytes })
        })
        .transpose()
}

// preserve the direct predecessor and the single pending transaction
fn install_anchor(
    kind: CustodyAnchorKind,
    anchor: &JsonValue,
    bytes: &[u8],
    expected: &str,
    files: &mut impl CustodyAnchorInstallFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), MaintenanceAnchorStatusError> {
    let current = read_anchor(kind, Current, files, codec, time)?;
    let pending = read_anchor(kind, Pending, files, codec, time)?;
    read_anchor(kind, Previous, files, codec, time)?;
    // converge only the byte-identical installed state and matching pending cleanup
    if current.as_ref().is_some_and(|value| value.bytes == bytes) {
        if let Some(pending) = pending {
            if pending.bytes != bytes {
                return Err(Slot);
            }
            files.remove_pending(kind)?;
            files.sync_root(kind)?;
        }
        return Ok(());
    }
    let (previous_hash, sequence) = match current {
        Some(current) => {
            let hash = codec.sha256(&current.bytes).ok_or(Codec)?;
            let sequence = uint64_field(&current.value, "sequence")
                .and_then(|value| value.checked_add(1))
                .ok_or(Slot)?;
            (
                JsonValue::String(hash.as_str().encode_utf16().collect()),
                sequence,
            )
        }
        None => (JsonValue::Null, 0),
    };
    // require the exact direct predecessor and next canonical sequence
    if field(anchor, "predecessorAnchorSha256") != Some(&previous_hash)
        || uint64_field(anchor, "sequence") != Some(sequence)
    {
        return Err(Slot);
    }
    // retain only one byte-identical in-progress transaction
    if let Some(pending) = pending {
        if pending.bytes != bytes {
            return Err(Slot);
        }
    } else {
        files.create_pending(kind, bytes)?;
    }
    commit_pending(kind, expected, files, codec, time)
}

// commit one exact pending anchor while retaining only its direct predecessor
fn commit_pending(
    kind: CustodyAnchorKind,
    expected: &str,
    files: &mut impl CustodyAnchorInstallFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), MaintenanceAnchorStatusError> {
    let pending = read_anchor(kind, Pending, files, codec, time)?;
    let current = read_anchor(kind, Current, files, codec, time)?;
    let current_hash = current
        .as_ref()
        .map(|slot| codec.sha256(&slot.bytes).ok_or(Codec))
        .transpose()?;
    // converge an already committed concurrent result after pending removal
    let Some(pending) = pending else {
        return current_hash
            .filter(|hash| hash.as_str() == expected)
            .map(|_| ())
            .ok_or(Slot);
    };
    // compare the exact retained pending target rather than replacing a different one
    if codec.sha256(&pending.bytes).ok_or(Codec)?.as_str() != expected {
        return Err(Slot);
    }
    // retain the original current bytes as history before replacing the current slot
    if let Some(current) = current
        && current_hash.is_some_and(|hash| hash.as_str() != expected)
    {
        files.replace_slot(kind, Previous, &current.bytes)?;
    }
    files.replace_slot(kind, Current, &pending.bytes)?;
    files.remove_pending(kind)?;
    files.sync_root(kind)
}
