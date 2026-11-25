//! Native installation transaction for one admitted future-only input seal.

use std::fmt::{Display, Formatter};
use std::fs::{self, DirBuilder};
use std::io::{ErrorKind, Read};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::maintenance_anchor_status::{MAXIMUM_ANCHOR_BYTES, read_private_anchor_slot};
use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_future_input_seal_current::{
    NativeFutureInputSealCaptureEpoch, NativeFutureInputSealCurrentError,
    NativeFutureInputSealFiles, NativeFutureInputSealStore, parse_future_input_seal,
};
use crate::revision_custody_ack_reader::{ACK_V2_VERSION, read_revision_custody_acknowledgements};
use crate::shadow_metadata_custody_write_files::{
    cleanup_private_temporary, sync_private_directory, write_private_temporary,
};
use crate::{
    MaintenanceAnchorSlot, MaintenanceAnchorStatusJson, MaintenanceCanonicalJson,
    NativeWirePrimitives,
};

const COMMAND: &str = "future-input-seal-install-v2";
const EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";
const INSTALLATION_VERSION: &str = "adjustment-future-only-input-seal-installation/v2";
const SEAL_DIRECTORY: &str = "future-input-seals";

/// Bounded failures from the future-only input seal installation boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeFutureInputSealInstallError {
    Arguments,
    Input,
    Identity,
    Custody,
    Epoch,
    Root,
    UnknownEntry,
    Slot,
    PendingCollision,
    Predecessor,
    Codec,
}

impl Display for NativeFutureInputSealInstallError {
    /// Render one stable operator-facing failure category without retained values.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "future-input-seal-install-v2 requires SEAL_SHA256",
            Self::Input => "future-only input seal bytes are invalid",
            Self::Identity => "future-only input seal identity is invalid",
            Self::Custody => "future-only input seal custody differs",
            Self::Epoch => "future-only input seal epoch differs",
            Self::Root => "future-only input seal root is unsafe",
            Self::UnknownEntry => "future-only input seal root contains an unknown entry",
            Self::Slot => "future-only input seal slot is unsafe",
            Self::PendingCollision => "future-only input seal pending collision",
            Self::Predecessor => "future-only input seal predecessor is invalid",
            Self::Codec => "future-only input seal serialization failed",
        })
    }
}

impl std::error::Error for NativeFutureInputSealInstallError {}

impl From<NativeFutureInputSealCurrentError> for NativeFutureInputSealInstallError {
    /// Preserve the current store's directly corresponding filesystem failures.
    fn from(error: NativeFutureInputSealCurrentError) -> Self {
        match error {
            NativeFutureInputSealCurrentError::Arguments => Self::Arguments,
            NativeFutureInputSealCurrentError::Root => Self::Root,
            NativeFutureInputSealCurrentError::UnknownEntry => Self::UnknownEntry,
            NativeFutureInputSealCurrentError::Slot => Self::Slot,
            NativeFutureInputSealCurrentError::Codec => Self::Codec,
            NativeFutureInputSealCurrentError::Seal => Self::Input,
            NativeFutureInputSealCurrentError::ControlEnvironment
            | NativeFutureInputSealCurrentError::CaptureEpoch
            | NativeFutureInputSealCurrentError::ActiveEpoch
            | NativeFutureInputSealCurrentError::SourceClosure => Self::Slot,
        }
    }
}

/// Independent authority that admits one exact retained packed-custody coordinate.
pub trait NativeFutureInputSealCustody {
    /// Require a matching current or direct-predecessor v2 custody acknowledgement.
    fn admit_custody(&mut self, seal: &JsonValue) -> Result<(), NativeFutureInputSealInstallError>;
}

/// Actual fixed-root revision-custody reader with caller-owned codec and time ports.
pub struct NativeFutureInputSealRevisionCustody<Codec, Time> {
    evidence_root: PathBuf,
    codec: Codec,
    time: Time,
}

impl<Codec, Time> NativeFutureInputSealRevisionCustody<Codec, Time> {
    /// Bind one explicit evidence root without reading its custody state early.
    pub const fn new(evidence_root: PathBuf, codec: Codec, time: Time) -> Self {
        Self {
            evidence_root,
            codec,
            time,
        }
    }
}

impl NativeFutureInputSealRevisionCustody<NativeWirePrimitives, NativeWirePrimitives> {
    /// Select the installed evidence root and shared native value owners.
    pub fn native() -> Self {
        Self::new(
            PathBuf::from(EVIDENCE_ROOT),
            NativeWirePrimitives,
            NativeWirePrimitives,
        )
    }
}

impl<Codec, Time> NativeFutureInputSealCustody for NativeFutureInputSealRevisionCustody<Codec, Time>
where
    Codec: MaintenanceAnchorStatusJson,
    Time: CurrentTimeResolver,
{
    /// Search only the validated current and retained direct predecessor acknowledgements.
    fn admit_custody(&mut self, seal: &JsonValue) -> Result<(), NativeFutureInputSealInstallError> {
        let acknowledgements = read_revision_custody_acknowledgements(
            &self.evidence_root,
            &mut self.codec,
            &self.time,
        )
        .map_err(|_| NativeFutureInputSealInstallError::Custody)?;
        // accept only one exact v2 packed-custody coordinate from the retained pair
        let admitted = acknowledgements.iter().flatten().any(|acknowledgement| {
            text(acknowledgement, "contractVersion").as_deref() == Some(ACK_V2_VERSION)
                && field(acknowledgement, "custodyCheckpointSha256")
                    == field(seal, "custodyCheckpointSha256")
                && field(acknowledgement, "pageSha256") == field(seal, "pageSha256")
                && field(acknowledgement, "nextArchiveCommitOrdinal")
                    == field(seal, "archiveCommitOrdinal")
                && field(acknowledgement, "nextFrontierSha256") == field(seal, "frontierSha256")
        });
        // reject absence and every partial coordinate match identically
        if !admitted {
            return Err(NativeFutureInputSealInstallError::Custody);
        }
        Ok(())
    }
}

/// Durable effects required by the compact three-slot seal transaction.
pub trait NativeFutureInputSealInstallFiles: NativeFutureInputSealFiles {
    /// Create or admit only the fixed owner-private seal directory.
    fn ensure_root(&mut self) -> Result<(), NativeFutureInputSealInstallError>;

    /// Read one optional stable seal slot.
    fn read_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
    ) -> Result<Option<Vec<u8>>, NativeFutureInputSealInstallError>;

    /// Publish the first pending transaction exclusively.
    fn create_pending(&mut self, bytes: &[u8]) -> Result<(), NativeFutureInputSealInstallError>;

    /// Atomically replace the current or direct-predecessor slot.
    fn replace_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
        bytes: &[u8],
    ) -> Result<(), NativeFutureInputSealInstallError>;

    /// Remove the committed pending marker.
    fn remove_pending(&mut self) -> Result<(), NativeFutureInputSealInstallError>;

    /// Sync the fixed seal directory after marker retirement.
    fn sync_root(&mut self) -> Result<(), NativeFutureInputSealInstallError>;
}

/// Concrete native future-only seal filesystem owner.
pub struct NativeFutureInputSealInstallStore {
    evidence_root: PathBuf,
    seal_root: PathBuf,
    uid: u32,
    gid: u32,
    reader: NativeFutureInputSealStore,
}

impl NativeFutureInputSealInstallStore {
    /// Bind the deployed evidence path to the selected installing identity.
    pub fn native(uid: u32, gid: u32) -> Self {
        Self::for_evidence_root(PathBuf::from(EVIDENCE_ROOT), uid, gid)
    }

    /// Bind one explicit evidence root without creating or observing it.
    pub fn for_evidence_root(evidence_root: PathBuf, uid: u32, gid: u32) -> Self {
        let seal_root = evidence_root.join(SEAL_DIRECTORY);
        let reader = NativeFutureInputSealStore::for_evidence_root(evidence_root.clone(), uid, gid);
        Self {
            evidence_root,
            seal_root,
            uid,
            gid,
            reader,
        }
    }
}

impl NativeFutureInputSealFiles for NativeFutureInputSealInstallStore {
    /// Delegate fixed inventory admission to the current-only store.
    fn read_root(&mut self) -> Result<Option<Vec<String>>, NativeFutureInputSealCurrentError> {
        self.reader.read_root()
    }

    /// Delegate the exact current slot read to the current-only store.
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, NativeFutureInputSealCurrentError> {
        self.reader.read_current()
    }
}

impl NativeFutureInputSealInstallFiles for NativeFutureInputSealInstallStore {
    /// Create only the fixed child beneath an already private canonical evidence root.
    fn ensure_root(&mut self) -> Result<(), NativeFutureInputSealInstallError> {
        require_private_directory(&self.evidence_root, None, None)?;
        let mut builder = DirBuilder::new();
        // preserve an existing seal root for exact validation rather than repairing it
        match builder.mode(0o700).create(&self.seal_root) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err(NativeFutureInputSealInstallError::Root),
        }
        require_private_directory(&self.seal_root, Some(self.uid), Some(self.gid))
    }

    /// Read one bounded private slot with stable-inode custody.
    fn read_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
    ) -> Result<Option<Vec<u8>>, NativeFutureInputSealInstallError> {
        read_private_anchor_slot(&self.seal_root.join(slot.filename()), self.uid, self.gid)
            .map_err(|_| NativeFutureInputSealInstallError::Slot)
    }

    /// Hard-link one fully synced temporary as the first pending transaction.
    fn create_pending(&mut self, bytes: &[u8]) -> Result<(), NativeFutureInputSealInstallError> {
        let temporary = write_private_temporary(&self.seal_root, "future-seal-pending", bytes)
            .map_err(|_| NativeFutureInputSealInstallError::Slot)?;
        let target = self
            .seal_root
            .join(MaintenanceAnchorSlot::Pending.filename());
        // publish exclusively so an independently pending seal stays authoritative
        if fs::hard_link(&temporary, target).is_err() {
            cleanup_private_temporary(&temporary)
                .map_err(|_| NativeFutureInputSealInstallError::Slot)?;
            return Err(NativeFutureInputSealInstallError::PendingCollision);
        }
        cleanup_private_temporary(&temporary)
            .map_err(|_| NativeFutureInputSealInstallError::Slot)?;
        sync_private_directory(&self.seal_root).map_err(|_| NativeFutureInputSealInstallError::Slot)
    }

    /// Rename one completely synced temporary over a non-pending fixed slot.
    fn replace_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
        bytes: &[u8],
    ) -> Result<(), NativeFutureInputSealInstallError> {
        // retain pending publication as an exclusive-only operation
        if slot == MaintenanceAnchorSlot::Pending {
            return Err(NativeFutureInputSealInstallError::Slot);
        }
        let temporary = write_private_temporary(&self.seal_root, "future-seal-replace", bytes)
            .map_err(|_| NativeFutureInputSealInstallError::Slot)?;
        let target = self.seal_root.join(slot.filename());
        // replace only after the complete private temporary is durable
        if fs::rename(&temporary, target).is_err() {
            cleanup_private_temporary(&temporary)
                .map_err(|_| NativeFutureInputSealInstallError::Slot)?;
            return Err(NativeFutureInputSealInstallError::Slot);
        }
        sync_private_directory(&self.seal_root).map_err(|_| NativeFutureInputSealInstallError::Slot)
    }

    /// Remove only the fixed pending marker after current replacement is durable.
    fn remove_pending(&mut self) -> Result<(), NativeFutureInputSealInstallError> {
        fs::remove_file(
            self.seal_root
                .join(MaintenanceAnchorSlot::Pending.filename()),
        )
        .map_err(|_| NativeFutureInputSealInstallError::Slot)
    }

    /// Flush the admitted seal directory after transaction marker retirement.
    fn sync_root(&mut self) -> Result<(), NativeFutureInputSealInstallError> {
        sync_private_directory(&self.seal_root).map_err(|_| NativeFutureInputSealInstallError::Slot)
    }
}

/// Execute the exact install verb after independent custody and epoch admission.
pub fn install_future_input_seal_command(
    command: (&str, &[&str]),
    input: &mut impl Read,
    files: &mut impl NativeFutureInputSealInstallFiles,
    custody: &mut impl NativeFutureInputSealCustody,
    capture_epoch: &mut impl NativeFutureInputSealCaptureEpoch,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<Option<Vec<u8>>, NativeFutureInputSealInstallError> {
    use NativeFutureInputSealInstallError as Error;

    let (action, arguments) = command;
    // leave every unrelated evidence command to its existing owner
    if action != COMMAND {
        return Ok(None);
    }
    let [expected_sha256] = arguments else {
        return Err(Error::Arguments);
    };
    // reject a malformed identity operand before consuming stdin
    if !valid_hash(expected_sha256) {
        return Err(Error::Arguments);
    }
    let mut bytes = Vec::new();
    input
        .take(MAXIMUM_ANCHOR_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Input)?;
    // preserve the exact bounded canonical document contract
    if !(2..=MAXIMUM_ANCHOR_BYTES).contains(&bytes.len()) {
        return Err(Error::Input);
    }
    let seal = parse_future_input_seal(&bytes, codec, time).ok_or(Error::Input)?;
    let seal_sha256 = codec.sha256(&bytes).ok_or(Error::Codec)?;
    // bind the operand to the exact canonical input bytes
    if seal_sha256.as_str() != *expected_sha256 {
        return Err(Error::Identity);
    }
    custody.admit_custody(&seal)?;
    let witness = capture_epoch
        .read_verified_capture_epoch()
        .map_err(|_| Error::Epoch)?;
    let witness_sha256 = codec.sha256(&witness.canonical_bytes).ok_or(Error::Codec)?;
    // bind the complete admitted witness bytes without exposing witness fields
    if text(&seal, "captureEpochWitnessSha256").as_deref() != Some(witness_sha256.as_str()) {
        return Err(Error::Epoch);
    }

    files.ensure_root()?;
    require_fixed_inventory(files)?;
    install_seal(&seal, &bytes, expected_sha256, files, codec, time)?;
    let response = JsonValue::Object(vec![
        member("contractVersion", INSTALLATION_VERSION),
        member("sealSha256", expected_sha256),
        member("state", "installed"),
    ]);
    Ok(Some(
        codec.encode_canonical_json(&response).ok_or(Error::Codec)?,
    ))
}

/// Require the source store's three-name transaction namespace after root creation.
fn require_fixed_inventory(
    files: &mut impl NativeFutureInputSealInstallFiles,
) -> Result<(), NativeFutureInputSealInstallError> {
    use NativeFutureInputSealInstallError::{Root, UnknownEntry};

    let names = files
        .read_root()
        .map_err(NativeFutureInputSealInstallError::from)?
        .ok_or(Root)?;
    // reject foreign, hidden, or temporary state before retained slot reads
    if names.iter().any(|name| {
        !matches!(
            name.as_str(),
            "current.json" | "pending.json" | "previous.json"
        )
    }) {
        return Err(UnknownEntry);
    }
    Ok(())
}

/// Retain one parsed transaction slot with its exact bytes and identity.
struct SealSlot {
    seal: JsonValue,
    seal_sha256: String,
    bytes: Vec<u8>,
}

/// Read and validate one retained transaction slot before installation uses it.
fn read_seal_slot(
    slot: MaintenanceAnchorSlot,
    files: &mut impl NativeFutureInputSealInstallFiles,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<Option<SealSlot>, NativeFutureInputSealInstallError> {
    files
        .read_slot(slot)?
        .map(|bytes| {
            let seal = parse_future_input_seal(&bytes, codec, time)
                .ok_or(NativeFutureInputSealInstallError::Input)?;
            let seal_sha256 = codec
                .sha256(&bytes)
                .ok_or(NativeFutureInputSealInstallError::Codec)?
                .as_str()
                .to_owned();
            Ok(SealSlot {
                seal,
                seal_sha256,
                bytes,
            })
        })
        .transpose()
}

/// Preserve the direct predecessor and one byte-identical pending transaction.
fn install_seal(
    seal: &JsonValue,
    bytes: &[u8],
    expected_sha256: &str,
    files: &mut impl NativeFutureInputSealInstallFiles,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<(), NativeFutureInputSealInstallError> {
    use NativeFutureInputSealInstallError as Error;

    let current = read_seal_slot(MaintenanceAnchorSlot::Current, files, codec, time)?;
    let pending = read_seal_slot(MaintenanceAnchorSlot::Pending, files, codec, time)?;
    read_seal_slot(MaintenanceAnchorSlot::Previous, files, codec, time)?;
    // converge only a byte-identical installed retry and matching pending cleanup
    if current.as_ref().is_some_and(|slot| slot.bytes == bytes) {
        // remove only the same seal left by an interrupted idempotent retry
        if let Some(pending) = pending {
            // reject an independently pending transaction before cleanup
            if pending.bytes != bytes {
                return Err(Error::PendingCollision);
            }
            files.remove_pending()?;
            files.sync_root()?;
        }
        return Ok(());
    }
    let (predecessor, sequence) = match current {
        Some(current) => {
            let sequence = seal_sequence(&current.seal)?
                .checked_add(1)
                .ok_or(Error::Predecessor)?;
            (Some(current.seal_sha256), sequence)
        }
        None => (None, 0),
    };
    // require the exact current digest and next unsigned sequence
    if nullable_hash(seal, "predecessorSealSha256")? != predecessor
        || seal_sequence(seal)? != sequence
    {
        return Err(Error::Predecessor);
    }
    // retain only one byte-identical in-progress transaction
    if let Some(pending) = pending {
        // refuse replacement of an independently pending transaction
        if pending.bytes != bytes {
            return Err(Error::PendingCollision);
        }
    } else {
        files.create_pending(bytes)?;
    }
    commit_pending(expected_sha256, files, codec, time)
}

/// Commit one exact pending seal while retaining only its direct predecessor.
fn commit_pending(
    expected_sha256: &str,
    files: &mut impl NativeFutureInputSealInstallFiles,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<(), NativeFutureInputSealInstallError> {
    use NativeFutureInputSealInstallError as Error;

    let pending = read_seal_slot(MaintenanceAnchorSlot::Pending, files, codec, time)?;
    let current = read_seal_slot(MaintenanceAnchorSlot::Current, files, codec, time)?;
    // converge only the already committed expected seal after pending retirement
    let Some(pending) = pending else {
        return current
            .filter(|slot| slot.seal_sha256 == expected_sha256)
            .map(|_| ())
            .ok_or(Error::Slot);
    };
    // refuse a pending transaction whose exact bytes have another identity
    if pending.seal_sha256 != expected_sha256 {
        return Err(Error::Identity);
    }
    // retain only a genuinely different current seal as direct history
    if let Some(current) = current
        && current.seal_sha256 != expected_sha256
    {
        files.replace_slot(MaintenanceAnchorSlot::Previous, &current.bytes)?;
    }
    files.replace_slot(MaintenanceAnchorSlot::Current, &pending.bytes)?;
    files.remove_pending()?;
    files.sync_root()
}

/// Decode one already validated seal sequence.
fn seal_sequence(seal: &JsonValue) -> Result<u64, NativeFutureInputSealInstallError> {
    text(seal, "sequence")
        .ok_or(NativeFutureInputSealInstallError::Predecessor)?
        .parse()
        .map_err(|_| NativeFutureInputSealInstallError::Predecessor)
}

/// Decode one already validated nullable predecessor identity.
fn nullable_hash(
    seal: &JsonValue,
    key: &str,
) -> Result<Option<String>, NativeFutureInputSealInstallError> {
    match field(seal, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .ok()
            .filter(|value| valid_hash(value))
            .map(Some)
            .ok_or(NativeFutureInputSealInstallError::Predecessor),
        _ => Err(NativeFutureInputSealInstallError::Predecessor),
    }
}

/// Construct one exact canonical string response member.
fn member(key: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (
        key.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}

/// Require one literal canonical private directory with optional service ownership.
fn require_private_directory(
    path: &Path,
    uid: Option<u32>,
    gid: Option<u32>,
) -> Result<(), NativeFutureInputSealInstallError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| NativeFutureInputSealInstallError::Root)?;
    // retain exact mode, real path, and selected owner without repairing state
    if !metadata.is_dir()
        || metadata.mode() & 0o777 != 0o700
        || fs::canonicalize(path).map_err(|_| NativeFutureInputSealInstallError::Root)? != path
        || uid.is_some_and(|expected| metadata.uid() != expected)
        || gid.is_some_and(|expected| metadata.gid() != expected)
    {
        return Err(NativeFutureInputSealInstallError::Root);
    }
    Ok(())
}

#[cfg(test)]
#[path = "native_future_input_seal_install_test.rs"]
mod tests;
