//! Read the original operator capture evidence without changing installed state.

use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use weather_api::CurrentTimeResolver;
use weather_contracts::{
    AdjustmentRevisionServingSnapshot, AdjustmentRevisionServingSnapshotContractVersionLiteral,
    JsonValue,
};

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::capture_epoch_witness::{
    GENESIS_FRONTIER_SHA256, resolve_operator_capture_clock, verify_operator_capture_epoch_witness,
};
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{CAPTURE_EPOCH_WITNESS_MAX_BYTES, MaintenanceCanonicalJson};

pub(crate) const AUTHORITY_DIR: &str = "/opt/weather/current/deploy/state";
pub(crate) const WITNESS_FILE: &str = "adjustment-capture-epoch.json";
pub(crate) const SNAPSHOT_FILE: &str = "adjustment-capture-epoch-snapshot.json";
const SNAPSHOT_VERSION: &str = AdjustmentRevisionServingSnapshotContractVersionLiteral::WIRE_VALUE;
const SNAPSHOT_KEYS: &[&str] = &[
    "archiveCommitOrdinal",
    "contractVersion",
    "cutoffAt",
    "entries",
    "entryCount",
    "frontierSha256",
    "snapshotSha256",
];

/// Identify which original operator authority file to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureEpochOperatorFile {
    Witness,
    Snapshot,
}

/// Contain operator failures before bytes are emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureEpochOperatorReadError {
    Arguments,
    File,
    Witness,
    Snapshot,
    Mismatch,
}

/// Supply bounded file bytes from the operator's authority directory.
pub trait CaptureEpochOperatorFiles {
    /// Read only the selected evidence file.
    fn read(
        &mut self,
        artifact: CaptureEpochOperatorFile,
    ) -> Result<Vec<u8>, CaptureEpochOperatorReadError>;
}

/// Bind the deployed operator paths independently of the API's read-only mount.
#[derive(Debug, Default)]
pub struct NativeCaptureEpochOperatorFiles;

impl CaptureEpochOperatorFiles for NativeCaptureEpochOperatorFiles {
    /// Read the fixed root-owned evidence selected by the command.
    fn read(
        &mut self,
        artifact: CaptureEpochOperatorFile,
    ) -> Result<Vec<u8>, CaptureEpochOperatorReadError> {
        let filename = match artifact {
            CaptureEpochOperatorFile::Witness => WITNESS_FILE,
            CaptureEpochOperatorFile::Snapshot => SNAPSHOT_FILE,
        };
        read_operator_file(&Path::new(AUTHORITY_DIR).join(filename), 0, 0)
    }
}

/// Execute the two read verbs and leave all other archive verbs to their owners.
pub fn read_capture_epoch_operator_command(
    action: &str,
    arguments: &[&str],
    files: &mut impl CaptureEpochOperatorFiles,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<Vec<u8>>, CaptureEpochOperatorReadError> {
    use CaptureEpochOperatorFile::{Snapshot, Witness};
    use CaptureEpochOperatorReadError as Error;

    let include_witness = match action {
        "revision-capture-epoch-read-v1" => true,
        "revision-capture-epoch-snapshot-read-v1" => false,
        _ => return Ok(None),
    };
    // reject operands before opening either authority file
    if !arguments.is_empty() {
        return Err(Error::Arguments);
    }

    // the snapshot-only export does not need the witness to be present
    if !include_witness {
        let bytes = files.read(Snapshot)?;
        verify_snapshot(&bytes, codec, time)?;
        return Ok(Some(bytes));
    }

    let witness_bytes = files.read(Witness)?;
    let witness = verify_operator_capture_epoch_witness(&witness_bytes, codec, time)
        .map_err(|_| Error::Witness)?
        .witness;

    let snapshot_bytes = files.read(Snapshot)?;
    let snapshot = verify_snapshot(&snapshot_bytes, codec, time)?;
    // withhold the witness unless both independent snapshot bindings agree
    if snapshot.cutoffAt != witness.epochAt
        || snapshot.snapshotSha256 != witness.servingSnapshotSha256
    {
        return Err(Error::Mismatch);
    }
    Ok(Some(witness_bytes))
}

/// Check the exact canonical empty-genesis receipt before releasing its original bytes.
pub(crate) fn verify_snapshot(
    bytes: &[u8],
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<AdjustmentRevisionServingSnapshot, CaptureEpochOperatorReadError> {
    use CaptureEpochOperatorReadError::Snapshot;

    // bound the original evidence bytes before handing them to the canonical codec
    if !(2..=CAPTURE_EPOCH_WITNESS_MAX_BYTES).contains(&bytes.len()) {
        return Err(Snapshot);
    }
    let value = codec.parse_canonical_object(bytes).ok_or(Snapshot)?;
    // admit only the fixed zero ordinal and zero rows from the genesis reader
    if !exact_keys(&value, SNAPSHOT_KEYS)
        || text(&value, "archiveCommitOrdinal").as_deref() != Some("0")
        || text(&value, "contractVersion").as_deref() != Some(SNAPSHOT_VERSION)
        || text(&value, "frontierSha256").as_deref() != Some(GENESIS_FRONTIER_SHA256)
        || !matches!(field(&value, "entries"), Some(JsonValue::Array(entries)) if entries.is_empty())
        || !matches!(field(&value, "entryCount"), Some(JsonValue::Number(count)) if *count == 0.0)
        || codec.encode_canonical_json(&value).as_deref() != Some(bytes)
    {
        return Err(Snapshot);
    }
    let cutoff = text(&value, "cutoffAt").ok_or(Snapshot)?;
    let digest = text(&value, "snapshotSha256").ok_or(Snapshot)?;
    // keep canonical time identity as well as the digest's exact wire spelling
    if !valid_hash(&digest) || resolve_operator_capture_clock(&cutoff, time).is_none() {
        return Err(Snapshot);
    }
    // hash the predecessor's newline-delimited zero-row snapshot preimage
    let preimage = format!("{SNAPSHOT_VERSION}\n{cutoff}\n0\n{GENESIS_FRONTIER_SHA256}\n");
    let computed = codec.sha256(preimage.as_bytes()).ok_or(Snapshot)?;
    // compare the admitted digest before reusing any bytes
    if computed.as_str() != digest {
        return Err(Snapshot);
    }
    Ok(AdjustmentRevisionServingSnapshot {
        archiveCommitOrdinal: "0".to_owned(),
        contractVersion: AdjustmentRevisionServingSnapshotContractVersionLiteral,
        cutoffAt: cutoff,
        entries: Vec::new(),
        entryCount: 0,
        frontierSha256: GENESIS_FRONTIER_SHA256.to_owned(),
        snapshotSha256: digest,
    })
}

/// Read the original single-link regular file through one bounded descriptor.
pub(crate) fn read_operator_file(
    path: &Path,
    uid: u32,
    gid: u32,
) -> Result<Vec<u8>, CaptureEpochOperatorReadError> {
    use CaptureEpochOperatorReadError::File;

    let before = fs::symlink_metadata(path).map_err(|_| File)?;
    // reject non-files before opening the final component
    if !before.is_file() {
        return Err(File);
    }
    let file = open_read_no_follow(path).map_err(|_| File)?;
    let opened = file.metadata().map_err(|_| File)?;
    let length = usize::try_from(opened.size()).map_err(|_| File)?;
    // admit only the exact installed owner, size and mode observed on the descriptor
    if !opened.is_file()
        || opened.nlink() != 1
        || opened.dev() != before.dev()
        || opened.ino() != before.ino()
        || opened.uid() != uid
        || opened.gid() != gid
        || opened.mode() & 0o777 != 0o644
        || !(2..=CAPTURE_EPOCH_WITNESS_MAX_BYTES).contains(&length)
    {
        return Err(File);
    }
    let mut bytes = Vec::with_capacity(length);
    file.take((length + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| File)?;
    // do not export a partial or unexpectedly grown file
    if bytes.len() != length {
        return Err(File);
    }
    Ok(bytes)
}
