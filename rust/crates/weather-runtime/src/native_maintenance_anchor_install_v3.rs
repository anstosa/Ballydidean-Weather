//! Native transferred installation for the future-only v3 maintenance anchor.

use std::fmt::{Display, Formatter};
use std::fs::{self, DirBuilder};
use std::io::{ErrorKind, Read};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::maintenance_anchor_status::{
    MAXIMUM_ANCHOR_BYTES, MaintenanceAnchorState, clock_field, read_private_anchor_slot,
};
use crate::maintenance_anchor_status_v3::validate_v3_anchor;
use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_future_input_seal_current::{
    NativeFutureInputSealCurrent, NativeFutureInputSealFiles, NativeFutureInputSealStore,
    parse_future_input_seal,
};
use crate::shadow_metadata_custody_write_files::{
    cleanup_private_temporary, sync_private_directory, write_private_temporary,
};
use crate::{
    MaintenanceAnchorSlot, MaintenanceAnchorStatusError, MaintenanceAnchorStatusFiles,
    MaintenanceAnchorStatusJson, NativeWirePrimitives,
};

const COMMAND: &str = "maintenance-anchor-install-v3";
const EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";
const ANCHOR_DIRECTORY: &str = "maintenance-anchors";
const INSTALLATION_VERSION: &str = "adjustment-maintenance-anchor-installation/v3";
const FINALIZATION_PROOF_VERSION: &str = "adjustment-maintenance-finalization-proof/v3";
const SEAL_BINDINGS: [&str; 20] = [
    "archiveCommitOrdinal",
    "burnSha256",
    "candidateArtifactRootSha256",
    "candidateReportSha256",
    "captureEpochWitnessSha256",
    "confirmationAccessSha256",
    "confirmationChunkCount",
    "custodyCheckpointSha256",
    "dueKey",
    "family",
    "frontierSha256",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "lifecycleLedgerRootSha256",
    "pageSha256",
    "policyReportSha256",
    "requiredInputRootSha256",
    "sequence",
    "sourceCommit",
    "workstationJournalHeadSha256",
];

/// Bounded failures from the future-only transferred-anchor installation boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceAnchorInstallV3Error {
    Arguments,
    Input,
    Identity,
    Control,
    Seal,
    SealBinding,
    Root,
    UnknownEntry,
    Slot,
    PendingCollision,
    Predecessor,
    Codec,
}

impl Display for NativeMaintenanceAnchorInstallV3Error {
    /// Render one stable operator-facing failure category without retained values.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "maintenance-anchor-install-v3 requires ANCHOR_SHA256",
            Self::Input => "maintenance-anchor-install-v3 requires a v3 anchor",
            Self::Identity => "maintenance anchor identity is invalid",
            Self::Control => "maintenance anchor control identity is invalid",
            Self::Seal => "future-only maintenance anchor active seal unavailable",
            Self::SealBinding => "future-only maintenance anchor seal binding is invalid",
            Self::Root => "maintenance anchor root is unsafe",
            Self::UnknownEntry => "maintenance anchor root contains an unknown entry",
            Self::Slot => "maintenance anchor slot is unsafe",
            Self::PendingCollision => "maintenance anchor pending collision",
            Self::Predecessor => "maintenance anchor predecessor is invalid",
            Self::Codec => "maintenance anchor serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceAnchorInstallV3Error {}

/// Independently selected optional active-control constraints.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceAnchorInstallV3Control<'a> {
    pub version: Option<&'a str>,
    pub sha256: Option<&'a str>,
}

/// Read-only owner for the raw current seal used by the source installation path.
pub trait NativeMaintenanceAnchorInstallV3Seal {
    /// Return only a complete canonical current seal and its exact byte identity.
    fn read_current_raw(
        &mut self,
    ) -> Result<Option<NativeFutureInputSealCurrent>, NativeMaintenanceAnchorInstallV3Error>;
}

/// Actual raw seal reader retaining its selected store, codec, and clock owners.
pub struct NativeMaintenanceAnchorInstallV3SealStore<Codec, Time> {
    files: NativeFutureInputSealStore,
    codec: Codec,
    time: Time,
}

impl<Codec, Time> NativeMaintenanceAnchorInstallV3SealStore<Codec, Time> {
    /// Bind one explicit evidence root and service identity without observing it.
    pub fn for_evidence_root(
        evidence_root: PathBuf,
        uid: u32,
        gid: u32,
        codec: Codec,
        time: Time,
    ) -> Self {
        Self {
            files: NativeFutureInputSealStore::for_evidence_root(evidence_root, uid, gid),
            codec,
            time,
        }
    }
}

impl NativeMaintenanceAnchorInstallV3SealStore<NativeWirePrimitives, NativeWirePrimitives> {
    /// Select the installed evidence root and native canonical value owners.
    pub fn native(uid: u32, gid: u32) -> Self {
        Self::for_evidence_root(
            PathBuf::from(EVIDENCE_ROOT),
            uid,
            gid,
            NativeWirePrimitives,
            NativeWirePrimitives,
        )
    }
}

impl<Codec, Time> NativeMaintenanceAnchorInstallV3Seal
    for NativeMaintenanceAnchorInstallV3SealStore<Codec, Time>
where
    Codec: MaintenanceAnchorStatusJson,
    Time: RevisionArchiveCanonicalCodec,
{
    /// Reuse the exact seal parser without imposing verified-current closure semantics.
    fn read_current_raw(
        &mut self,
    ) -> Result<Option<NativeFutureInputSealCurrent>, NativeMaintenanceAnchorInstallV3Error> {
        // require genuine root presence before reading the raw current slot
        if self
            .files
            .read_root()
            .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Seal)?
            .is_none()
        {
            return Ok(None);
        }
        let Some(bytes) = self
            .files
            .read_current()
            .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Seal)?
        else {
            return Ok(None);
        };
        let seal = parse_future_input_seal(&bytes, &mut self.codec, &self.time)
            .ok_or(NativeMaintenanceAnchorInstallV3Error::Seal)?;
        let seal_sha256 = self
            .codec
            .sha256(&bytes)
            .ok_or(NativeMaintenanceAnchorInstallV3Error::Codec)?
            .as_str()
            .to_owned();
        Ok(Some(NativeFutureInputSealCurrent { seal, seal_sha256 }))
    }
}

/// Durable effects shared by v3 transfer and finalization transactions.
pub trait NativeMaintenanceAnchorV3InstallFiles: MaintenanceAnchorStatusFiles {
    /// Create or admit only the fixed owner-private anchor directory.
    fn ensure_root(&mut self) -> Result<(), NativeMaintenanceAnchorInstallV3Error>;

    /// Read one optional stable anchor slot.
    fn read_anchor_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
    ) -> Result<Option<Vec<u8>>, NativeMaintenanceAnchorInstallV3Error>;

    /// Publish the first pending transaction exclusively.
    fn create_pending(&mut self, bytes: &[u8])
    -> Result<(), NativeMaintenanceAnchorInstallV3Error>;

    /// Atomically replace the current or direct-predecessor slot.
    fn replace_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
        bytes: &[u8],
    ) -> Result<(), NativeMaintenanceAnchorInstallV3Error>;

    /// Remove the committed pending marker.
    fn remove_pending(&mut self) -> Result<(), NativeMaintenanceAnchorInstallV3Error>;

    /// Sync the fixed anchor directory after marker retirement.
    fn sync_root(&mut self) -> Result<(), NativeMaintenanceAnchorInstallV3Error>;
}

/// Concrete native v3 maintenance-anchor filesystem owner.
pub struct NativeMaintenanceAnchorV3InstallStore {
    evidence_root: PathBuf,
    anchor_root: PathBuf,
    uid: u32,
    gid: u32,
}

impl NativeMaintenanceAnchorV3InstallStore {
    /// Bind the deployed evidence path to the selected installing identity.
    pub fn native(uid: u32, gid: u32) -> Self {
        Self::for_evidence_root(PathBuf::from(EVIDENCE_ROOT), uid, gid)
    }

    /// Borrow the exact evidence root retained by this shared durable transaction owner.
    pub(crate) fn evidence_root(&self) -> &std::path::Path {
        &self.evidence_root
    }

    /// Bind one explicit evidence root without creating or observing it.
    pub fn for_evidence_root(evidence_root: PathBuf, uid: u32, gid: u32) -> Self {
        Self {
            anchor_root: evidence_root.join(ANCHOR_DIRECTORY),
            evidence_root,
            uid,
            gid,
        }
    }
}

impl MaintenanceAnchorStatusFiles for NativeMaintenanceAnchorV3InstallStore {
    /// Admit an existing private root before exposing its fixed inventory.
    fn read_root(&mut self) -> Result<Option<Vec<String>>, MaintenanceAnchorStatusError> {
        // preserve genuine root absence for the shared status contract
        if !crate::maintenance_anchor_status::verified_private_anchor_root(
            &self.anchor_root,
            self.uid,
            self.gid,
        )? {
            return Ok(None);
        }
        let mut names = fs::read_dir(&self.anchor_root)
            .map_err(|_| MaintenanceAnchorStatusError::Root)?
            .map(|entry| {
                entry
                    .map_err(|_| MaintenanceAnchorStatusError::Root)?
                    .file_name()
                    .into_string()
                    .map_err(|_| MaintenanceAnchorStatusError::UnknownEntry)
            })
            .collect::<Result<Vec<_>, _>>()?;
        names.sort_unstable();
        Ok(Some(names))
    }

    /// Read one bounded private slot with the shared stable-inode reader.
    fn read_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
    ) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError> {
        read_private_anchor_slot(&self.anchor_root.join(slot.filename()), self.uid, self.gid)
    }
}

impl NativeMaintenanceAnchorV3InstallFiles for NativeMaintenanceAnchorV3InstallStore {
    /// Create only the fixed child beneath an already private canonical evidence root.
    fn ensure_root(&mut self) -> Result<(), NativeMaintenanceAnchorInstallV3Error> {
        require_private_directory(&self.evidence_root, None, None)?;
        let mut builder = DirBuilder::new();
        // preserve an existing anchor root for exact validation rather than repairing it
        match builder.mode(0o700).create(&self.anchor_root) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err(NativeMaintenanceAnchorInstallV3Error::Root),
        }
        require_private_directory(&self.anchor_root, Some(self.uid), Some(self.gid))
    }

    /// Read one bounded private slot with stable-inode custody.
    fn read_anchor_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
    ) -> Result<Option<Vec<u8>>, NativeMaintenanceAnchorInstallV3Error> {
        read_private_anchor_slot(&self.anchor_root.join(slot.filename()), self.uid, self.gid)
            .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Slot)
    }

    /// Hard-link one fully synced temporary as the first pending transaction.
    fn create_pending(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), NativeMaintenanceAnchorInstallV3Error> {
        let temporary = write_private_temporary(&self.anchor_root, "anchor-v3-pending", bytes)
            .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Slot)?;
        let target = self
            .anchor_root
            .join(MaintenanceAnchorSlot::Pending.filename());
        // publish exclusively so an independently pending anchor stays authoritative
        if fs::hard_link(&temporary, target).is_err() {
            cleanup_private_temporary(&temporary)
                .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Slot)?;
            return Err(NativeMaintenanceAnchorInstallV3Error::PendingCollision);
        }
        cleanup_private_temporary(&temporary)
            .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Slot)?;
        sync_private_directory(&self.anchor_root)
            .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Slot)
    }

    /// Rename one completely synced temporary over a non-pending fixed slot.
    fn replace_slot(
        &mut self,
        slot: MaintenanceAnchorSlot,
        bytes: &[u8],
    ) -> Result<(), NativeMaintenanceAnchorInstallV3Error> {
        // retain pending publication as an exclusive-only operation
        if slot == MaintenanceAnchorSlot::Pending {
            return Err(NativeMaintenanceAnchorInstallV3Error::Slot);
        }
        let temporary = write_private_temporary(&self.anchor_root, "anchor-v3-replace", bytes)
            .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Slot)?;
        let target = self.anchor_root.join(slot.filename());
        // replace only after the complete private temporary is durable
        if fs::rename(&temporary, target).is_err() {
            cleanup_private_temporary(&temporary)
                .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Slot)?;
            return Err(NativeMaintenanceAnchorInstallV3Error::Slot);
        }
        sync_private_directory(&self.anchor_root)
            .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Slot)
    }

    /// Remove only the fixed pending marker after current replacement is durable.
    fn remove_pending(&mut self) -> Result<(), NativeMaintenanceAnchorInstallV3Error> {
        fs::remove_file(
            self.anchor_root
                .join(MaintenanceAnchorSlot::Pending.filename()),
        )
        .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Slot)
    }

    /// Flush the admitted anchor directory after transaction marker retirement.
    fn sync_root(&mut self) -> Result<(), NativeMaintenanceAnchorInstallV3Error> {
        sync_private_directory(&self.anchor_root)
            .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Slot)
    }
}

/// Execute the exact v3 transferred-anchor install verb.
pub fn install_maintenance_anchor_v3_command<Time>(
    command: (&str, &[&str]),
    input: &mut impl Read,
    files: &mut impl NativeMaintenanceAnchorV3InstallFiles,
    seal_store: &mut impl NativeMaintenanceAnchorInstallV3Seal,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &Time,
    control: NativeMaintenanceAnchorInstallV3Control<'_>,
) -> Result<Option<Vec<u8>>, NativeMaintenanceAnchorInstallV3Error>
where
    Time: CurrentTimeResolver + RevisionArchiveCanonicalCodec,
{
    use NativeMaintenanceAnchorInstallV3Error as Error;

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
    let anchor = parse_transferred_anchor(&bytes, codec, time)?;
    let anchor_sha256 = codec.sha256(&bytes).ok_or(Error::Codec)?;
    // bind the operand to the exact canonical transferred payload
    if anchor_sha256.as_str() != *expected_sha256 {
        return Err(Error::Identity);
    }
    // apply only independently configured control values, preserving source optionality
    if control
        .sha256
        .is_some_and(|expected| text(&anchor, "controlSha256").as_deref() != Some(expected))
        || control
            .version
            .is_some_and(|expected| text(&anchor, "controlVersion").as_deref() != Some(expected))
    {
        return Err(Error::Control);
    }
    let seal = seal_store.read_current_raw()?.ok_or(Error::Seal)?;
    // bind every copied custody/producer identity and the exact raw seal document
    if !anchor_matches_seal(&anchor, &seal)
        || clock_field(&anchor, "fullGraphVerifiedAt", time).ok_or(Error::SealBinding)?
            < clock_field(&seal.seal, "sealedAt", time).ok_or(Error::SealBinding)?
    {
        return Err(Error::SealBinding);
    }

    files.ensure_root()?;
    require_fixed_inventory(files)?;
    install_anchor(&anchor, &bytes, expected_sha256, &seal, files, codec, time)?;
    let response = JsonValue::Object(vec![
        member("anchorSha256", expected_sha256),
        member("contractVersion", INSTALLATION_VERSION),
        member("state", "transferred"),
    ]);
    Ok(Some(
        codec.encode_canonical_json(&response).ok_or(Error::Codec)?,
    ))
}

/// Parse one exact canonical v3 transferred anchor without admitting legacy v2.
fn parse_transferred_anchor(
    bytes: &[u8],
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, NativeMaintenanceAnchorInstallV3Error> {
    let anchor = codec
        .parse_canonical_object(bytes)
        .ok_or(NativeMaintenanceAnchorInstallV3Error::Input)?;
    // require both the exact v3 grammar and transferred state
    if validate_v3_anchor(&anchor, codec, time) != Some(MaintenanceAnchorState::Transferred)
        || codec.encode_canonical_json(&anchor).as_deref() != Some(bytes)
    {
        return Err(NativeMaintenanceAnchorInstallV3Error::Input);
    }
    Ok(anchor)
}

/// Require the source store's three-name transaction namespace after root creation.
fn require_fixed_inventory(
    files: &mut impl NativeMaintenanceAnchorV3InstallFiles,
) -> Result<(), NativeMaintenanceAnchorInstallV3Error> {
    let names = files
        .read_root()
        .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Root)?
        .ok_or(NativeMaintenanceAnchorInstallV3Error::Root)?;
    // reject foreign, hidden, or temporary state before retained slot reads
    if names.iter().any(|name| {
        !matches!(
            name.as_str(),
            "current.json" | "pending.json" | "previous.json"
        )
    }) {
        return Err(NativeMaintenanceAnchorInstallV3Error::UnknownEntry);
    }
    Ok(())
}

/// Retain one parsed transaction slot with its exact bytes and identity.
struct AnchorSlot {
    anchor: JsonValue,
    anchor_sha256: String,
    bytes: Vec<u8>,
}

/// Read and validate one retained v3 anchor slot before installation uses it.
fn read_anchor_slot(
    slot: MaintenanceAnchorSlot,
    files: &mut impl NativeMaintenanceAnchorV3InstallFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<AnchorSlot>, NativeMaintenanceAnchorInstallV3Error> {
    files
        .read_anchor_slot(slot)?
        .map(|bytes| {
            let anchor = codec
                .parse_canonical_object(&bytes)
                .ok_or(NativeMaintenanceAnchorInstallV3Error::Slot)?;
            // retained slots may be transferred or finalized but must remain exact v3
            if validate_v3_anchor(&anchor, codec, time).is_none()
                || codec.encode_canonical_json(&anchor).as_deref() != Some(bytes.as_slice())
            {
                return Err(NativeMaintenanceAnchorInstallV3Error::Slot);
            }
            let anchor_sha256 = codec
                .sha256(&bytes)
                .ok_or(NativeMaintenanceAnchorInstallV3Error::Codec)?
                .as_str()
                .to_owned();
            Ok(AnchorSlot {
                anchor,
                anchor_sha256,
                bytes,
            })
        })
        .transpose()
}

/// Install one transferred anchor or converge its exact current/finalized replay.
fn install_anchor(
    anchor: &JsonValue,
    bytes: &[u8],
    expected_sha256: &str,
    seal: &NativeFutureInputSealCurrent,
    files: &mut impl NativeMaintenanceAnchorV3InstallFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeMaintenanceAnchorInstallV3Error> {
    use NativeMaintenanceAnchorInstallV3Error as Error;

    let current = read_anchor_slot(MaintenanceAnchorSlot::Current, files, codec, time)?;
    let pending = read_anchor_slot(MaintenanceAnchorSlot::Pending, files, codec, time)?;
    read_anchor_slot(MaintenanceAnchorSlot::Previous, files, codec, time)?;
    // finish an exact already-current retry without advancing sequence
    if current.as_ref().is_some_and(|slot| slot.bytes == bytes) {
        // preserve a different pending transaction rather than hiding drift
        if let Some(pending) = pending {
            // retire only a byte-identical duplicate marker
            if pending.bytes != bytes {
                return Err(Error::PendingCollision);
            }
            files.remove_pending()?;
            files.sync_root()?;
        }
        return Ok(());
    }
    // replay T only when current F is its exact authenticated finalization successor
    if let Some(current) = current.as_ref()
        && anchor_matches_seal(&current.anchor, seal)
        && finalized_succeeds_transfer(current, anchor, expected_sha256, codec)?
    {
        // preserve an unrelated pending transaction on the finalized replay path
        if let Some(pending) = pending {
            // finish only a duplicate already-current F commit marker
            if pending.bytes != current.bytes {
                return Err(Error::PendingCollision);
            }
            files.remove_pending()?;
            files.sync_root()?;
        }
        return Ok(());
    }
    let (predecessor, sequence) = match current {
        Some(current) => {
            let sequence = anchor_sequence(&current.anchor)?
                .checked_add(1)
                .ok_or(Error::Predecessor)?;
            (Some(current.anchor_sha256), sequence)
        }
        None => (None, 0),
    };
    // require the next exact predecessor without reset or replay
    if nullable_hash(anchor, "predecessorAnchorSha256")? != predecessor
        || anchor_sequence(anchor)? != sequence
    {
        return Err(Error::Predecessor);
    }
    // preserve only one byte-identical in-progress transaction
    if let Some(pending) = pending {
        // refuse replacement of an independently pending transaction
        if pending.bytes != bytes {
            return Err(Error::PendingCollision);
        }
    } else {
        files.create_pending(bytes)?;
    }
    commit_maintenance_anchor_v3_pending(expected_sha256, files, codec, time)
}

/// Commit one exact pending v3 anchor while retaining only its direct predecessor.
pub(crate) fn commit_maintenance_anchor_v3_pending(
    expected_sha256: &str,
    files: &mut impl NativeMaintenanceAnchorV3InstallFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeMaintenanceAnchorInstallV3Error> {
    use NativeMaintenanceAnchorInstallV3Error as Error;

    let pending = read_anchor_slot(MaintenanceAnchorSlot::Pending, files, codec, time)?;
    let current = read_anchor_slot(MaintenanceAnchorSlot::Current, files, codec, time)?;
    // converge only the already committed expected anchor after pending retirement
    let Some(pending) = pending else {
        return current
            .filter(|slot| slot.anchor_sha256 == expected_sha256)
            .map(|_| ())
            .ok_or(Error::Slot);
    };
    // refuse a pending transaction whose exact bytes have another identity
    if pending.anchor_sha256 != expected_sha256 {
        return Err(Error::Identity);
    }
    // retain only a genuinely different current anchor as direct history
    if let Some(current) = current
        && current.anchor_sha256 != expected_sha256
    {
        files.replace_slot(MaintenanceAnchorSlot::Previous, &current.bytes)?;
    }
    files.replace_slot(MaintenanceAnchorSlot::Current, &pending.bytes)?;
    files.remove_pending()?;
    files.sync_root()
}

/// Compare every source-copied seal scalar and exact immutable seal digest.
fn anchor_matches_seal(anchor: &JsonValue, seal: &NativeFutureInputSealCurrent) -> bool {
    text(anchor, "inputSealSha256").as_deref() == Some(seal.seal_sha256.as_str())
        && SEAL_BINDINGS
            .iter()
            .all(|key| field(anchor, key).is_some() && field(anchor, key) == field(&seal.seal, key))
}

/// Recognize only the exact finalized successor of the supplied transferred anchor.
fn finalized_succeeds_transfer(
    finalized: &AnchorSlot,
    transferred: &JsonValue,
    transferred_sha256: &str,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<bool, NativeMaintenanceAnchorInstallV3Error> {
    // keep this replay path disjoint from transferred and malformed state
    if text(&finalized.anchor, "ctfState").as_deref() != Some("finalized")
        || text(transferred, "ctfState").as_deref() != Some("transferred")
    {
        return Ok(false);
    }
    let finalized_at = field(&finalized.anchor, "finalizedAt")
        .cloned()
        .ok_or(NativeMaintenanceAnchorInstallV3Error::Slot)?;
    let mut proof_members = clone_object_except(transferred, &["contractVersion", "ctfState"])?;
    proof_members.push(member("contractVersion", FINALIZATION_PROOF_VERSION));
    proof_members.push(("finalizedAt".encode_utf16().collect(), finalized_at.clone()));
    proof_members.push(member("transferredAnchorSha256", transferred_sha256));
    let proof = JsonValue::Object(proof_members);
    let proof_bytes = codec
        .encode_canonical_json(&proof)
        .ok_or(NativeMaintenanceAnchorInstallV3Error::Codec)?;
    let proof_sha256 = codec
        .sha256(&proof_bytes)
        .ok_or(NativeMaintenanceAnchorInstallV3Error::Codec)?
        .as_str()
        .to_owned();

    let mut expected_members = clone_object_except(transferred, &[])?;
    replace_member(&mut expected_members, "ctfState", json_text("finalized"))?;
    expected_members.push(member("finalizationProofSha256", &proof_sha256));
    expected_members.push(("finalizedAt".encode_utf16().collect(), finalized_at));
    let expected = codec
        .encode_canonical_json(&JsonValue::Object(expected_members))
        .ok_or(NativeMaintenanceAnchorInstallV3Error::Codec)?;
    Ok(
        text(&finalized.anchor, "finalizationProofSha256").as_deref()
            == Some(proof_sha256.as_str())
            && finalized.bytes == expected,
    )
}

/// Clone one object while omitting the exact named source members.
fn clone_object_except(
    value: &JsonValue,
    excluded: &[&str],
) -> Result<Vec<(Vec<u16>, JsonValue)>, NativeMaintenanceAnchorInstallV3Error> {
    let JsonValue::Object(members) = value else {
        return Err(NativeMaintenanceAnchorInstallV3Error::Slot);
    };
    Ok(members
        .iter()
        .filter(|(key, _)| {
            String::from_utf16(key)
                .ok()
                .is_some_and(|name| !excluded.contains(&name.as_str()))
        })
        .cloned()
        .collect())
}

/// Replace one required object member without changing unrelated values.
fn replace_member(
    members: &mut [(Vec<u16>, JsonValue)],
    name: &str,
    replacement: JsonValue,
) -> Result<(), NativeMaintenanceAnchorInstallV3Error> {
    let key: Vec<u16> = name.encode_utf16().collect();
    // mutate only the required existing source member
    if let Some((_, value)) = members.iter_mut().find(|(candidate, _)| *candidate == key) {
        *value = replacement;
        return Ok(());
    }
    Err(NativeMaintenanceAnchorInstallV3Error::Slot)
}

/// Decode one already validated anchor sequence.
fn anchor_sequence(anchor: &JsonValue) -> Result<u64, NativeMaintenanceAnchorInstallV3Error> {
    text(anchor, "sequence")
        .ok_or(NativeMaintenanceAnchorInstallV3Error::Predecessor)?
        .parse()
        .map_err(|_| NativeMaintenanceAnchorInstallV3Error::Predecessor)
}

/// Decode one already validated nullable predecessor identity.
fn nullable_hash(
    anchor: &JsonValue,
    key: &str,
) -> Result<Option<String>, NativeMaintenanceAnchorInstallV3Error> {
    match field(anchor, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .ok()
            .filter(|value| valid_hash(value))
            .map(Some)
            .ok_or(NativeMaintenanceAnchorInstallV3Error::Predecessor),
        _ => Err(NativeMaintenanceAnchorInstallV3Error::Predecessor),
    }
}

/// Construct one exact canonical string response member.
fn member(key: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), json_text(value))
}

/// Construct one UTF-16 JSON string value.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Require one literal canonical private directory with optional service ownership.
fn require_private_directory(
    path: &Path,
    uid: Option<u32>,
    gid: Option<u32>,
) -> Result<(), NativeMaintenanceAnchorInstallV3Error> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| NativeMaintenanceAnchorInstallV3Error::Root)?;
    // retain exact mode, real path, and selected owner without repairing state
    if !metadata.is_dir()
        || metadata.mode() & 0o777 != 0o700
        || fs::canonicalize(path).map_err(|_| NativeMaintenanceAnchorInstallV3Error::Root)? != path
        || uid.is_some_and(|expected| metadata.uid() != expected)
        || gid.is_some_and(|expected| metadata.gid() != expected)
    {
        return Err(NativeMaintenanceAnchorInstallV3Error::Root);
    }
    Ok(())
}

#[cfg(test)]
#[path = "native_maintenance_anchor_install_v3_test.rs"]
mod tests;
