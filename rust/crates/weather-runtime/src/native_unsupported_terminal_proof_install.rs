//! Native installation transaction for one admitted unsupported-terminal proof.

use std::fmt::{Display, Formatter};
use std::fs::{self, DirBuilder};
use std::io::{ErrorKind, Read};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::MaintenanceCanonicalJson;
use crate::capture_epoch_operator::read_operator_file;
use crate::capture_epoch_witness::verify_operator_capture_epoch_witness;
use crate::maintenance_anchor_status::{MAXIMUM_ANCHOR_BYTES, read_private_anchor_slot};
use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_unsupported_terminal_proof::{
    NativeUnsupportedTerminalProofCurrent, NativeUnsupportedTerminalProofError,
    NativeUnsupportedTerminalProofFiles, NativeUnsupportedTerminalProofStore,
    read_unsupported_terminal_proof_current,
};
use crate::revision_custody_ack_reader::{ACK_V2_VERSION, read_revision_custody_acknowledgements};
use crate::shadow_metadata_custody_write_files::{
    cleanup_private_temporary, sync_private_directory, write_private_temporary,
};
use crate::{MaintenanceAnchorSlot, MaintenanceAnchorStatusJson};

const COMMAND: &str = "unsupported-terminal-proof-install-v1";
const EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";
const WITNESS_PATH: &str = "/opt/weather/current/deploy/state/adjustment-capture-epoch.json";
const INSTALLATION_VERSION: &str =
    "adjustment-maintenance-unsupported-terminal-proof-installation/v1";
const FAMILY_CURRENT_VERSION: &str = "adjustment-family-release-current-status/v1";

/// Bounded failures from the unsupported-terminal installation boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeUnsupportedTerminalProofInstallError {
    Arguments,
    Input,
    Identity,
    Custody,
    Epoch,
    Source,
    Root,
    UnknownEntry,
    Slot,
    PendingCollision,
    Predecessor,
    Codec,
}

impl Display for NativeUnsupportedTerminalProofInstallError {
    /// Return one bounded source-compatible failure without disclosing retained material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "unsupported-terminal-proof-install-v1 requires PROOF_SHA256",
            Self::Input => "adjustment unsupported terminal proof bytes are invalid",
            Self::Identity => "adjustment unsupported terminal proof identity is invalid",
            Self::Custody => "adjustment unsupported terminal custody differs",
            Self::Epoch => "adjustment unsupported terminal epoch differs",
            Self::Source => "adjustment unsupported terminal source differs",
            Self::Root => "adjustment unsupported terminal root is unsafe",
            Self::UnknownEntry => "adjustment unsupported terminal root contains an unknown entry",
            Self::Slot => "adjustment unsupported terminal slot is unsafe",
            Self::PendingCollision => "adjustment unsupported terminal pending collision",
            Self::Predecessor => "adjustment unsupported terminal predecessor is invalid",
            Self::Codec => "adjustment unsupported terminal proof serialization failed",
        })
    }
}

impl std::error::Error for NativeUnsupportedTerminalProofInstallError {}

impl From<NativeUnsupportedTerminalProofError> for NativeUnsupportedTerminalProofInstallError {
    /// Preserve the current-reader failure category at the mutating boundary.
    fn from(error: NativeUnsupportedTerminalProofError) -> Self {
        match error {
            NativeUnsupportedTerminalProofError::Arguments => Self::Arguments,
            NativeUnsupportedTerminalProofError::Root => Self::Root,
            NativeUnsupportedTerminalProofError::UnknownEntry => Self::UnknownEntry,
            NativeUnsupportedTerminalProofError::Slot => Self::Slot,
            NativeUnsupportedTerminalProofError::Proof => Self::Input,
            NativeUnsupportedTerminalProofError::Codec => Self::Codec,
        }
    }
}

/// Independently selected active control identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeUnsupportedTerminalProofControl<'a> {
    pub sha256: Option<&'a str>,
    pub version: Option<&'a str>,
}

/// Minimal validated projection of the live family source authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeUnsupportedTerminalProofFamilyCurrent {
    pub contract_version: String,
    pub family: String,
    pub commit: String,
}

/// Port for the full live family inspection that remains independent of proof stdin.
pub trait NativeUnsupportedTerminalProofFamilyAuthority {
    /// Read the current source authority for exactly one admitted family.
    fn read_current_family(
        &mut self,
        family: &str,
    ) -> Result<
        NativeUnsupportedTerminalProofFamilyCurrent,
        NativeUnsupportedTerminalProofInstallError,
    >;
}

/// Durable effects required by the three-slot proof transaction.
pub trait NativeUnsupportedTerminalProofInstallFiles: NativeUnsupportedTerminalProofFiles {
    /// Borrow the selected evidence root for the existing custody acknowledgement reader.
    fn evidence_root(&self) -> &Path;

    /// Read the fixed root-owned capture-epoch witness.
    fn witness(&mut self) -> Result<Vec<u8>, NativeUnsupportedTerminalProofInstallError>;

    /// Create or admit only the fixed owner-private proof directory.
    fn ensure_root(&mut self) -> Result<(), NativeUnsupportedTerminalProofInstallError>;

    /// Read one optional stable proof slot.
    fn read_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
    ) -> Result<Option<Vec<u8>>, NativeUnsupportedTerminalProofInstallError>;

    /// Publish the first pending transaction exclusively.
    fn create_pending(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), NativeUnsupportedTerminalProofInstallError>;

    /// Atomically replace the current or direct-predecessor slot.
    fn replace_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
        bytes: &[u8],
    ) -> Result<(), NativeUnsupportedTerminalProofInstallError>;

    /// Remove the committed pending marker.
    fn remove_pending(&mut self) -> Result<(), NativeUnsupportedTerminalProofInstallError>;

    /// Sync the fixed proof directory after marker retirement.
    fn sync_root(&mut self) -> Result<(), NativeUnsupportedTerminalProofInstallError>;
}

/// Concrete native proof filesystem and fixed witness owner.
pub struct NativeUnsupportedTerminalProofInstallStore {
    evidence_root: PathBuf,
    proof_root: PathBuf,
    witness_path: PathBuf,
    uid: u32,
    gid: u32,
    witness_uid: u32,
    witness_gid: u32,
    reader: NativeUnsupportedTerminalProofStore,
}

impl NativeUnsupportedTerminalProofInstallStore {
    /// Bind the deployed evidence and witness paths without reading or creating them.
    pub fn native(uid: u32, gid: u32) -> Self {
        Self::for_paths(
            PathBuf::from(EVIDENCE_ROOT),
            PathBuf::from(WITNESS_PATH),
            uid,
            gid,
            0,
            0,
        )
    }

    /// Bind explicit fixture or installed paths and their independently selected identities.
    pub fn for_paths(
        evidence_root: PathBuf,
        witness_path: PathBuf,
        uid: u32,
        gid: u32,
        witness_uid: u32,
        witness_gid: u32,
    ) -> Self {
        let proof_root = evidence_root.join("unsupported-terminal-proofs");
        let reader =
            NativeUnsupportedTerminalProofStore::for_evidence_root(evidence_root.clone(), uid, gid);
        Self {
            evidence_root,
            proof_root,
            witness_path,
            uid,
            gid,
            witness_uid,
            witness_gid,
            reader,
        }
    }
}

impl NativeUnsupportedTerminalProofFiles for NativeUnsupportedTerminalProofInstallStore {
    /// Delegate fixed inventory admission to the current-only custody owner.
    fn read_root(&mut self) -> Result<Option<Vec<String>>, NativeUnsupportedTerminalProofError> {
        self.reader.read_root()
    }

    /// Delegate the exact current slot read to the current-only custody owner.
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, NativeUnsupportedTerminalProofError> {
        self.reader.read_current()
    }
}

impl NativeUnsupportedTerminalProofInstallFiles for NativeUnsupportedTerminalProofInstallStore {
    /// Borrow the exact evidence root selected at construction.
    fn evidence_root(&self) -> &Path {
        &self.evidence_root
    }

    /// Read the installed witness through the existing owner-checked operator boundary.
    fn witness(&mut self) -> Result<Vec<u8>, NativeUnsupportedTerminalProofInstallError> {
        read_operator_file(&self.witness_path, self.witness_uid, self.witness_gid)
            .map_err(|_| NativeUnsupportedTerminalProofInstallError::Epoch)
    }

    /// Create only the fixed private child beneath an already private canonical evidence root.
    fn ensure_root(&mut self) -> Result<(), NativeUnsupportedTerminalProofInstallError> {
        require_private_directory(&self.evidence_root, None, None)?;
        let mut builder = DirBuilder::new();
        // preserve an existing proof root for exact validation rather than repairing it
        match builder.mode(0o700).create(&self.proof_root) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err(NativeUnsupportedTerminalProofInstallError::Root),
        }
        require_private_directory(&self.proof_root, Some(self.uid), Some(self.gid))
    }

    /// Read one bounded private slot with stable-inode custody.
    fn read_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
    ) -> Result<Option<Vec<u8>>, NativeUnsupportedTerminalProofInstallError> {
        read_private_anchor_slot(&self.proof_root.join(slot.filename()), self.uid, self.gid)
            .map_err(|_| NativeUnsupportedTerminalProofInstallError::Slot)
    }

    /// Hard-link one fully synced temporary as the first pending transaction.
    fn create_pending(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), NativeUnsupportedTerminalProofInstallError> {
        let temporary = write_private_temporary(&self.proof_root, "unsupported-pending", bytes)
            .map_err(|_| NativeUnsupportedTerminalProofInstallError::Slot)?;
        let target = self
            .proof_root
            .join(MaintenanceAnchorSlot::Pending.filename());
        // publish exclusively so a concurrent pending proof stays authoritative
        if fs::hard_link(&temporary, target).is_err() {
            cleanup_private_temporary(&temporary)
                .map_err(|_| NativeUnsupportedTerminalProofInstallError::Slot)?;
            return Err(NativeUnsupportedTerminalProofInstallError::PendingCollision);
        }
        cleanup_private_temporary(&temporary)
            .map_err(|_| NativeUnsupportedTerminalProofInstallError::Slot)?;
        sync_private_directory(&self.proof_root)
            .map_err(|_| NativeUnsupportedTerminalProofInstallError::Slot)
    }

    /// Rename one completely synced temporary over a non-pending fixed slot.
    fn replace_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
        bytes: &[u8],
    ) -> Result<(), NativeUnsupportedTerminalProofInstallError> {
        // retain pending publication as an exclusive-only operation
        if slot == MaintenanceAnchorSlot::Pending {
            return Err(NativeUnsupportedTerminalProofInstallError::Slot);
        }
        let temporary = write_private_temporary(&self.proof_root, "unsupported-replace", bytes)
            .map_err(|_| NativeUnsupportedTerminalProofInstallError::Slot)?;
        let target = self.proof_root.join(slot.filename());
        // replace only after the complete private temporary is durable
        if fs::rename(&temporary, target).is_err() {
            cleanup_private_temporary(&temporary)
                .map_err(|_| NativeUnsupportedTerminalProofInstallError::Slot)?;
            return Err(NativeUnsupportedTerminalProofInstallError::Slot);
        }
        sync_private_directory(&self.proof_root)
            .map_err(|_| NativeUnsupportedTerminalProofInstallError::Slot)
    }

    /// Remove only the fixed pending marker after current replacement is durable.
    fn remove_pending(&mut self) -> Result<(), NativeUnsupportedTerminalProofInstallError> {
        fs::remove_file(
            self.proof_root
                .join(MaintenanceAnchorSlot::Pending.filename()),
        )
        .map_err(|_| NativeUnsupportedTerminalProofInstallError::Slot)
    }

    /// Flush the admitted proof directory after the transaction marker is removed.
    fn sync_root(&mut self) -> Result<(), NativeUnsupportedTerminalProofInstallError> {
        sync_private_directory(&self.proof_root)
            .map_err(|_| NativeUnsupportedTerminalProofInstallError::Slot)
    }
}

/// Execute the exact install verb after all independent authority inputs are supplied.
pub fn install_unsupported_terminal_proof_command(
    command: (&str, &[&str]),
    input: &mut impl Read,
    files: &mut impl NativeUnsupportedTerminalProofInstallFiles,
    family_authority: &mut impl NativeUnsupportedTerminalProofFamilyAuthority,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &(impl CurrentTimeResolver + RevisionArchiveCanonicalCodec),
    control: NativeUnsupportedTerminalProofControl<'_>,
) -> Result<Option<Vec<u8>>, NativeUnsupportedTerminalProofInstallError> {
    use NativeUnsupportedTerminalProofInstallError as Error;

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
    let parsed = parse_proof(&bytes, codec, time)?;
    // bind exact stdin bytes and the independently configured active control
    if parsed.proof_sha256 != *expected_sha256
        || control.sha256.is_none()
        || control.version.is_none()
        || text(&parsed.proof, "controlSha256").as_deref() != control.sha256
        || text(&parsed.proof, "controlVersion").as_deref() != control.version
    {
        return Err(Error::Identity);
    }

    admit_custody(&parsed.proof, files.evidence_root(), codec, time)?;
    let witness = files.witness()?;
    verify_operator_capture_epoch_witness(&witness, codec, time).map_err(|_| Error::Epoch)?;
    // bind the complete canonical witness file rather than only its self-address field
    if codec.sha256(&witness).ok_or(Error::Codec)?.as_str()
        != text(&parsed.proof, "captureEpochWitnessSha256").ok_or(Error::Input)?
    {
        return Err(Error::Epoch);
    }
    let family = text(&parsed.proof, "family").ok_or(Error::Input)?;
    let current_family = family_authority.read_current_family(&family)?;
    // require the actual live family status contract and exact source commit
    if current_family.contract_version != FAMILY_CURRENT_VERSION
        || current_family.family != family
        || current_family.commit != text(&parsed.proof, "sourceCommit").ok_or(Error::Input)?
    {
        return Err(Error::Source);
    }

    files.ensure_root()?;
    require_fixed_inventory(files)?;
    install_proof(&parsed, &bytes, expected_sha256, files, codec, time)?;
    let response = JsonValue::Object(vec![
        member("contractVersion", INSTALLATION_VERSION),
        member("proofSha256", expected_sha256),
        member("state", "installed"),
    ]);
    Ok(Some(
        codec.encode_canonical_json(&response).ok_or(Error::Codec)?,
    ))
}

/// Cross-bind the proof only to the exact current v2 custody checkpoint.
fn admit_custody(
    proof: &JsonValue,
    evidence_root: &Path,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &(impl CurrentTimeResolver + RevisionArchiveCanonicalCodec),
) -> Result<(), NativeUnsupportedTerminalProofInstallError> {
    use NativeUnsupportedTerminalProofInstallError as Error;

    let custody = read_revision_custody_acknowledgements(evidence_root, codec, time)
        .map_err(|_| Error::Custody)?;
    let current = custody[0].as_ref().ok_or(Error::Custody)?;
    let acknowledged = text(current, "acknowledgedAt")
        .and_then(|value| time.canonical_utc_milliseconds(&value))
        .ok_or(Error::Custody)?;
    let verified = text(proof, "fullGraphVerifiedAt")
        .and_then(|value| time.canonical_utc_milliseconds(&value))
        .ok_or(Error::Custody)?;
    // retain current-only custody and every exact copied coordinate
    if text(current, "contractVersion").as_deref() != Some(ACK_V2_VERSION)
        || field(current, "custodyCheckpointSha256") != field(proof, "custodyCheckpointSha256")
        || field(current, "pageSha256") != field(proof, "pageSha256")
        || field(current, "nextArchiveCommitOrdinal") != field(proof, "archiveCommitOrdinal")
        || field(current, "nextFrontierSha256") != field(proof, "frontierSha256")
        || verified < acknowledged
    {
        return Err(Error::Custody);
    }
    Ok(())
}

/// Require the source store's three-name transaction namespace after root creation.
fn require_fixed_inventory(
    files: &mut impl NativeUnsupportedTerminalProofInstallFiles,
) -> Result<(), NativeUnsupportedTerminalProofInstallError> {
    use NativeUnsupportedTerminalProofInstallError::{Root, UnknownEntry};

    let names = files.read_root()?.ok_or(Root)?;
    // reject foreign, hidden, or temporary state before reading any retained slot
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

/// Parse exact canonical proof bytes through the shared current-proof validator.
fn parse_proof(
    bytes: &[u8],
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<NativeUnsupportedTerminalProofCurrent, NativeUnsupportedTerminalProofInstallError> {
    use NativeUnsupportedTerminalProofInstallError::Input;

    // preserve the predecessor's bounded canonical proof input contract
    if !(2..=MAXIMUM_ANCHOR_BYTES).contains(&bytes.len()) {
        return Err(Input);
    }
    let mut files = MemoryProofFiles(bytes);
    read_unsupported_terminal_proof_current(&mut files, codec, time)?.ok_or(Input)
}

/// Retain one parsed slot with its exact original bytes.
struct ProofSlot {
    current: NativeUnsupportedTerminalProofCurrent,
    bytes: Vec<u8>,
}

/// Read and validate one retained transaction slot before it participates in installation.
fn read_proof_slot(
    slot: MaintenanceAnchorSlot,
    files: &mut impl NativeUnsupportedTerminalProofInstallFiles,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<Option<ProofSlot>, NativeUnsupportedTerminalProofInstallError> {
    files
        .read_slot(slot)?
        .map(|bytes| {
            let current = parse_proof(&bytes, codec, time)?;
            Ok(ProofSlot { current, bytes })
        })
        .transpose()
}

/// Preserve the direct predecessor and one byte-identical pending transaction.
fn install_proof(
    proof: &NativeUnsupportedTerminalProofCurrent,
    bytes: &[u8],
    expected_sha256: &str,
    files: &mut impl NativeUnsupportedTerminalProofInstallFiles,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<(), NativeUnsupportedTerminalProofInstallError> {
    use NativeUnsupportedTerminalProofInstallError as Error;

    let current = read_proof_slot(MaintenanceAnchorSlot::Current, files, codec, time)?;
    let pending = read_proof_slot(MaintenanceAnchorSlot::Pending, files, codec, time)?;
    read_proof_slot(MaintenanceAnchorSlot::Previous, files, codec, time)?;
    // converge only a byte-identical installed retry and matching pending cleanup
    if current.as_ref().is_some_and(|slot| slot.bytes == bytes) {
        // remove only the same proof left by an interrupted idempotent retry
        if let Some(pending) = pending {
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
            let sequence = proof_sequence(&current.current.proof)?
                .checked_add(1)
                .ok_or(Error::Predecessor)?;
            (Some(current.current.proof_sha256), sequence)
        }
        None => (None, 0),
    };
    // require the exact current digest and next unsigned sequence
    if nullable_hash(&proof.proof, "predecessorProofSha256")? != predecessor
        || proof_sequence(&proof.proof)? != sequence
    {
        return Err(Error::Predecessor);
    }
    // retain only one byte-identical in-progress transaction
    if let Some(pending) = pending {
        if pending.bytes != bytes {
            return Err(Error::PendingCollision);
        }
    } else {
        files.create_pending(bytes)?;
    }
    commit_pending(expected_sha256, files, codec, time)
}

/// Commit one exact pending proof while retaining only its direct predecessor.
fn commit_pending(
    expected_sha256: &str,
    files: &mut impl NativeUnsupportedTerminalProofInstallFiles,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<(), NativeUnsupportedTerminalProofInstallError> {
    use NativeUnsupportedTerminalProofInstallError as Error;

    let pending = read_proof_slot(MaintenanceAnchorSlot::Pending, files, codec, time)?;
    let current = read_proof_slot(MaintenanceAnchorSlot::Current, files, codec, time)?;
    // converge only the already committed expected proof after pending retirement
    let Some(pending) = pending else {
        return current
            .filter(|slot| slot.current.proof_sha256 == expected_sha256)
            .map(|_| ())
            .ok_or(Error::Slot);
    };
    // refuse a pending transaction whose exact bytes have another identity
    if pending.current.proof_sha256 != expected_sha256 {
        return Err(Error::Identity);
    }
    // retain only a genuinely different current proof as direct history
    if let Some(current) = current
        && current.current.proof_sha256 != expected_sha256
    {
        files.replace_slot(MaintenanceAnchorSlot::Previous, &current.bytes)?;
    }
    files.replace_slot(MaintenanceAnchorSlot::Current, &pending.bytes)?;
    files.remove_pending()?;
    files.sync_root()
}

/// Decode one already validated proof sequence.
fn proof_sequence(proof: &JsonValue) -> Result<u64, NativeUnsupportedTerminalProofInstallError> {
    text(proof, "sequence")
        .ok_or(NativeUnsupportedTerminalProofInstallError::Predecessor)?
        .parse()
        .map_err(|_| NativeUnsupportedTerminalProofInstallError::Predecessor)
}

/// Decode one already validated nullable predecessor identity.
fn nullable_hash(
    proof: &JsonValue,
    key: &str,
) -> Result<Option<String>, NativeUnsupportedTerminalProofInstallError> {
    match field(proof, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .ok()
            .filter(|value| valid_hash(value))
            .map(Some)
            .ok_or(NativeUnsupportedTerminalProofInstallError::Predecessor),
        _ => Err(NativeUnsupportedTerminalProofInstallError::Predecessor),
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
) -> Result<(), NativeUnsupportedTerminalProofInstallError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| NativeUnsupportedTerminalProofInstallError::Root)?;
    // retain exact mode, real path, and selected owner without repairing existing state
    if !metadata.is_dir()
        || metadata.mode() & 0o777 != 0o700
        || fs::canonicalize(path).map_err(|_| NativeUnsupportedTerminalProofInstallError::Root)?
            != path
        || uid.is_some_and(|expected| metadata.uid() != expected)
        || gid.is_some_and(|expected| metadata.gid() != expected)
    {
        return Err(NativeUnsupportedTerminalProofInstallError::Root);
    }
    Ok(())
}

/// In-memory adapter that reuses the complete current-proof parser for stdin and retained slots.
struct MemoryProofFiles<'a>(&'a [u8]);

impl NativeUnsupportedTerminalProofFiles for MemoryProofFiles<'_> {
    /// Present exactly one fixed current slot to the shared validator.
    fn read_root(&mut self) -> Result<Option<Vec<String>>, NativeUnsupportedTerminalProofError> {
        Ok(Some(vec!["current.json".to_owned()]))
    }

    /// Return the exact borrowed proof bytes without filesystem mutation.
    fn read_current(&mut self) -> Result<Option<Vec<u8>>, NativeUnsupportedTerminalProofError> {
        Ok(Some(self.0.to_vec()))
    }
}

#[cfg(test)]
#[path = "native_unsupported_terminal_proof_install_test.rs"]
mod tests;
