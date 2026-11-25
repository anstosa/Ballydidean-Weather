//! Native Linux filesystem adapter for the existing owner-private revision spool.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::native_revision_spool_file::{
    self as files, WriteError, ensure_child_directory, invalid, list_names, read_optional_file,
    read_private_file, require_directory, sync_directory,
};
use crate::rain_control_state_archive::{
    RainControlArchiveMetadataKind, RainControlExclusiveWrite, RainControlStateArchiveStorage,
};
use crate::rain_fixed_gauge_gap_archive::{
    RAIN_FIXED_GAUGE_GAP_DIRECTORY, RainFixedGaugeGapArchiveError, RainFixedGaugeGapFileName,
    RainFixedGaugeGapStorage,
};
use crate::revision_archive_canonical::{is_payload_name, is_sha256, lower_hex};
use crate::revision_online_spool::{
    RevisionOnlinePayloadAddress, RevisionOnlinePayloadKind, RevisionOnlineSpoolError,
    RevisionOnlineSpoolStorage,
};

mod archive_bindings;
mod cold_ack;
mod cold_ack_storage;
mod cold_entry;
mod cold_groups;
mod cold_page;
mod forecast_allocation;
mod forecast_archive_lock;
mod forecast_archive_lock_binding;
mod forecast_archive_native_operator;
mod forecast_archive_operator;
mod forecast_capture_coordinator;
mod forecast_capture_store;
mod forecast_channels;
mod forecast_cycle_capsule;
mod forecast_cycle_capsule_binding;
mod forecast_cycle_disk_ack;
mod forecast_cycle_disk_recovery;
mod forecast_cycle_disk_slots;
mod forecast_cycle_final_ack;
mod forecast_cycle_final_ack_binding;
mod forecast_cycle_final_binding;
mod forecast_cycle_final_store;
mod forecast_cycle_gap;
mod forecast_cycle_gap_binding;
mod forecast_cycle_input_seal;
mod forecast_cycle_input_seal_binding;
mod forecast_cycle_metadata_codec;
mod forecast_cycle_online_next;
mod forecast_cycle_online_page_ack;
mod forecast_cycle_online_seal;
mod forecast_cycle_online_transfer;
mod forecast_cycle_online_wire;
mod forecast_cycle_page_binding;
mod forecast_cycle_pending_final;
mod forecast_cycle_transfer_binding;
mod forecast_cycle_transfer_checkpoint;
mod forecast_material_binding;
mod forecast_objects;
mod forecast_scheduler;
mod forecast_scheduler_admission;
mod forecast_scheduler_archive;
mod forecast_scheduler_archive_payload;
mod forecast_scheduler_slot_binding;
mod forecast_scheduler_state;
mod forecast_scheduler_state_binding;
mod forecast_snapshot;
mod gap_ack;
mod gap_operator;
mod gap_transfer;
pub use archive_bindings::NativeRevisionSpoolArtifactCodec;
pub use cold_ack::NativeRevisionColdAckCodec;
pub use cold_entry::*;
pub use cold_groups::*;
pub use cold_page::NativeRevisionColdPageCodec;
pub use forecast_allocation::*;
pub use forecast_archive_lock::*;
pub use forecast_archive_lock_binding::*;
pub use forecast_archive_native_operator::*;
pub use forecast_archive_operator::*;
pub use forecast_capture_coordinator::*;
pub use forecast_capture_store::*;
pub use forecast_channels::*;
pub use forecast_cycle_capsule::*;
pub use forecast_cycle_disk_ack::*;
pub use forecast_cycle_disk_recovery::*;
pub use forecast_cycle_disk_slots::*;
pub use forecast_cycle_final_ack::*;
pub use forecast_cycle_final_binding::*;
pub use forecast_cycle_final_store::*;
pub use forecast_cycle_gap::*;
pub use forecast_cycle_input_seal::*;
pub use forecast_cycle_metadata_codec::*;
pub use forecast_cycle_online_next::*;
pub use forecast_cycle_online_page_ack::*;
pub use forecast_cycle_online_seal::*;
pub use forecast_cycle_online_transfer::*;
pub use forecast_cycle_online_wire::*;
pub use forecast_cycle_page_binding::*;
pub use forecast_cycle_pending_final::*;
pub use forecast_cycle_transfer_checkpoint::*;
pub use forecast_objects::*;
pub use forecast_scheduler::*;
pub use forecast_scheduler_admission::*;
pub use forecast_scheduler_archive::*;
pub use forecast_scheduler_archive_payload::*;
pub use forecast_scheduler_state::*;
pub use forecast_snapshot::*;
pub use gap_ack::{NativeRevisionGapAckDecoder, NativeRevisionGapAckStorage};
pub use gap_operator::{
    NativeRevisionGapCommandCodec, NativeRevisionGapCommandError, NativeRevisionGapCommandPorts,
};
pub use gap_transfer::{
    NativeRevisionGapMember, NativeRevisionGapPayloadKind, NativeRevisionGapProjection,
    NativeRevisionGapRainAuxiliary, NativeRevisionGapRainBinding, NativeRevisionGapShadowStage,
    NativeRevisionGapTransferCodec, NativeRevisionGapTransferError,
};

const LOCK_DIRECTORY: &str = "revision-spool-locks";
const LOCK_NAME: &str = "spool.lock";
const LOCK_FILE_BYTES: usize = 2_048;
const LOCK_ATTEMPTS: usize = 200;
const LOCK_RETRY: Duration = Duration::from_millis(10);
const LOCK_DURATION_MILLISECONDS: i64 = 300_000;
const LOCK_DURATION: Duration = Duration::from_millis(LOCK_DURATION_MILLISECONDS as u64);
const CHILDREN: [&str; 17] = [
    "revision-projections",
    "revision-stage-receipts",
    "revision-commit-receipts",
    "revision-frontier-successors",
    "revision-publish-receipts",
    "revision-gaps",
    "shadow-revision-capsules",
    "shadow-revision-stages",
    "shadow-revision-publish-receipts",
    "shadow-revision-gaps",
    "revision-custody-acknowledgements",
    "revision-gap-transfer",
    LOCK_DIRECTORY,
    "rain-control-states",
    "rain-control-state-stage-receipts",
    "rain-control-state-gaps",
    RAIN_FIXED_GAUGE_GAP_DIRECTORY,
];

/// The exact deployed lock's canonical timestamp and JSON adapter.
pub trait RevisionSpoolLeaseCodec {
    /// Encode the exact canonical lock document with its UTC expiry and kernel-random token.
    fn encode_canonical_lease(&self, expires_at_milliseconds: i64, token: &str) -> Option<Vec<u8>>;

    /// Admit the complete canonical lock document before trusting its expiry.
    fn validated_lease_expiry(&self, canonical_document: &[u8]) -> Option<i64>;
}

/// One concrete filesystem handle scoped to a previously admitted private root.
pub struct NativeRevisionSpool<LeaseCodec> {
    root: PathBuf,
    codec: LeaseCodec,
    leased: bool,
}

impl<LeaseCodec: RevisionSpoolLeaseCodec> NativeRevisionSpool<LeaseCodec> {
    /// Bind an explicit pre-existing archive root without creating or repairing it.
    pub fn new(root: PathBuf, codec: LeaseCodec) -> io::Result<Self> {
        require_directory(&root)?;
        Ok(Self {
            root,
            codec,
            leased: false,
        })
    }

    /// Preserve the source's fixed custody tree and admit no other directories.
    pub fn ensure_revision_directories(&self) -> io::Result<()> {
        require_directory(&self.root)?;
        // create only the fixed private children in the source order
        for child in CHILDREN {
            ensure_child_directory(&self.root.join(child))?;
        }
        Ok(())
    }

    /// Execute once under the same canonical cross-process lease as the Node runtime.
    fn with_lease<T, Error: Copy>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, Error>,
        storage_error: Error,
    ) -> Result<T, Error> {
        // never recursively acquire a second lease through this storage handle
        if self.leased {
            return Err(storage_error);
        }
        let lease = Lease::acquire(&self.root.join(LOCK_DIRECTORY), &self.codec)
            .map_err(|_| storage_error)?;
        self.leased = true;
        let result = operation(self);
        self.leased = false;
        lease.finish().map_err(|_| storage_error)?;
        result
    }

    /// Require one shared lease before any mutable native storage operation.
    fn require_lease(&self) -> io::Result<()> {
        // fail before touching archive paths outside the admitted lifetime
        if !self.leased {
            return Err(invalid("revision spool mutation has no held lease"));
        }
        Ok(())
    }

    /// Publish immutable content into one fixed destination directory.
    fn write_once(&self, directory: &str, name: &str, bytes: &[u8]) -> Result<(), WriteError> {
        self.require_lease().map_err(unstarted_write_failure)?;
        self.publish_once(directory, name, bytes)
    }

    /// Publish one immutable metadata entry where the source permits an independent write.
    fn publish_once(&self, directory: &str, name: &str, bytes: &[u8]) -> Result<(), WriteError> {
        let directory = self.root.join(directory);
        let unique = uuid_from_linux_entropy().map_err(unstarted_write_failure)?;
        let temporary = directory.join(format!(".capture-{unique}.tmp"));
        files::publish_immutable(&directory.join(name), &temporary, bytes)
    }

    /// Form one bound member path from the closed directory and a typed digest.
    fn payload_path(&self, address: &RevisionOnlinePayloadAddress) -> PathBuf {
        let (kind, name) = address.components();
        self.root.join(kind.directory()).join(name)
    }

    /// Bind a metadata basename to an admitted state identity.
    fn metadata_name(state_digest: &str) -> io::Result<String> {
        // keep the persisted namespace independent from raw path input
        if !is_sha256(state_digest) {
            return Err(invalid("revision spool metadata digest is invalid"));
        }
        Ok(format!("sha256-{state_digest}.json"))
    }
}

impl<LeaseCodec: RevisionSpoolLeaseCodec> RevisionOnlineSpoolStorage
    for NativeRevisionSpool<LeaseCodec>
{
    /// Acquire the original lock around the shared value-bearing capacity operation.
    fn with_revision_spool_lease<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, RevisionOnlineSpoolError>,
    ) -> Result<T, RevisionOnlineSpoolError> {
        self.with_lease(operation, RevisionOnlineSpoolError::Storage)
    }

    /// Only the exact incoming target requires reading its actual prior bytes.
    fn read_existing_payload(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError> {
        read_optional_file(&self.payload_path(address), maximum_bytes).map_err(spool_error)
    }

    /// Enumerate one fixed value-bearing directory under the retained lease.
    fn list_payload_names(
        &mut self,
        kind: RevisionOnlinePayloadKind,
    ) -> Result<Vec<String>, RevisionOnlineSpoolError> {
        self.require_lease().map_err(spool_error)?;
        list_names(&self.root.join(kind.directory()), None).map_err(spool_error)
    }

    /// Inspect checked real file length without reading or allocating the retained content.
    fn payload_file_length(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> Result<usize, RevisionOnlineSpoolError> {
        self.require_lease().map_err(spool_error)?;
        files::private_file_length(&self.payload_path(address), maximum_bytes).map_err(spool_error)
    }
}

impl<LeaseCodec: RevisionSpoolLeaseCodec> RainControlStateArchiveStorage
    for NativeRevisionSpool<LeaseCodec>
{
    /// Ensure the same concrete revision custody tree before archiving a state.
    fn ensure_revision_root(&mut self) -> Result<(), RevisionOnlineSpoolError> {
        self.ensure_revision_directories().map_err(spool_error)
    }

    /// Publish exactly one new state under the shared lock before its receipt.
    fn write_state_exclusive(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        bytes: &[u8],
    ) -> Result<(), RevisionOnlineSpoolError> {
        let (kind, name) = address.components();
        // keep this owner restricted to the exact monthly state directory and bound
        if kind != RevisionOnlinePayloadKind::RainControlState || bytes.len() > kind.maximum_bytes()
        {
            return Err(RevisionOnlineSpoolError::Input);
        }
        self.write_once(kind.directory(), name, bytes).map_err(
            // keep filesystem details outside the private archive response
            |_| RevisionOnlineSpoolError::Storage,
        )
    }

    /// Read the actual immutable receipt from its fixed directory.
    fn read_metadata(
        &mut self,
        kind: RainControlArchiveMetadataKind,
        state_sha256: &str,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError> {
        self.require_lease().map_err(spool_error)?;
        let name = Self::metadata_name(state_sha256).map_err(spool_error)?;
        read_optional_file(&self.root.join(kind.directory()).join(name), maximum_bytes)
            .map_err(spool_error)
    }

    /// Preserve the source's distinct existing-stage retry disposition.
    fn write_metadata_exclusive(
        &mut self,
        kind: RainControlArchiveMetadataKind,
        state_sha256: &str,
        bytes: &[u8],
    ) -> Result<RainControlExclusiveWrite, RevisionOnlineSpoolError> {
        let name = Self::metadata_name(state_sha256).map_err(spool_error)?;
        // keep the fixed native metadata write as bounded as its read side
        if bytes.len() > 4_096 {
            return Err(RevisionOnlineSpoolError::Input);
        }
        match self.write_once(kind.directory(), &name, bytes) {
            Ok(()) => Ok(RainControlExclusiveWrite::Created),
            Err(error) if existing_immutable_name(&error) => {
                Ok(RainControlExclusiveWrite::AlreadyPresent)
            }
            Err(_) => Err(RevisionOnlineSpoolError::Storage),
        }
    }

    /// Read one exact retained projection only within the shared stage/gap lease.
    fn read_projection(
        &mut self,
        name: &str,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, RevisionOnlineSpoolError> {
        self.require_lease().map_err(spool_error)?;
        // keep the authorized projection type's fixed basename grammar
        if !is_payload_name(name) {
            return Err(RevisionOnlineSpoolError::Census);
        }
        read_private_file(
            &self
                .root
                .join(RevisionOnlinePayloadKind::Projection.directory())
                .join(name),
            maximum_bytes,
        )
        .map_err(spool_error)
    }
}

impl<LeaseCodec: RevisionSpoolLeaseCodec> RainFixedGaugeGapStorage
    for NativeRevisionSpool<LeaseCodec>
{
    /// Ensure the same stable owner-private tree before a fixed gauge read or mutation.
    fn ensure_revision_root(&mut self) -> Result<(), RainFixedGaugeGapArchiveError> {
        self.ensure_revision_directories()
            .map_err(gap_storage_error)
    }

    /// Serialize fixed gauge gaps with the other real revision writers.
    fn with_revision_spool_mutation<T>(
        &mut self,
        mutation: impl FnOnce(&mut Self) -> Result<T, RainFixedGaugeGapArchiveError>,
    ) -> Result<T, RainFixedGaugeGapArchiveError> {
        self.with_lease(mutation, RainFixedGaugeGapArchiveError::Storage)
    }

    /// Read immutable status without a needless exclusive lease.
    fn read_gap_file(
        &mut self,
        name: &RainFixedGaugeGapFileName,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RainFixedGaugeGapArchiveError> {
        read_optional_file(
            &self
                .root
                .join(RAIN_FIXED_GAUGE_GAP_DIRECTORY)
                .join(name.as_str()),
            maximum_bytes,
        )
        .map_err(gap_storage_error)
    }

    /// Stop at enough entries to decide the fixed metadata bound.
    fn list_gap_names(
        &mut self,
        maximum_names: usize,
    ) -> Result<Vec<String>, RainFixedGaugeGapArchiveError> {
        self.require_lease().map_err(gap_storage_error)?;
        list_names(
            &self.root.join(RAIN_FIXED_GAUGE_GAP_DIRECTORY),
            Some(maximum_names),
        )
        .map_err(gap_storage_error)
    }

    /// Publish the first permanent disposition durably without replacing it.
    fn write_gap_exclusive(
        &mut self,
        name: &RainFixedGaugeGapFileName,
        bytes: &[u8],
    ) -> Result<(), RainFixedGaugeGapArchiveError> {
        // retain the source's small fixed receipt envelope
        if bytes.len() > 4_096 {
            return Err(RainFixedGaugeGapArchiveError::Input);
        }
        self.write_once(RAIN_FIXED_GAUGE_GAP_DIRECTORY, name.as_str(), bytes)
            .map_err(|_| RainFixedGaugeGapArchiveError::Storage)
    }
}

/// The one retained lease, released only if its exact bytes remain present.
struct Lease {
    path: PathBuf,
    bytes: Vec<u8>,
    released: bool,
}

impl Lease {
    /// Acquire the Node-compatible lease within its existing bounded retry limit.
    fn acquire(directory: &Path, codec: &impl RevisionSpoolLeaseCodec) -> io::Result<Self> {
        let path = directory.join(LOCK_NAME);
        let now = wall_milliseconds(SystemTime::now())?;
        let expires = now.checked_add(LOCK_DURATION_MILLISECONDS).ok_or_else(
            // contain wall-clock overflow instead of inventing a lifetime
            || invalid("revision spool lease expiry overflows"),
        )?;
        let token = uuid_from_linux_entropy()?;
        let bytes = codec.encode_canonical_lease(expires, &token).ok_or_else(
            // require the approved canonical JSON and time owner
            || invalid("revision spool lease could not be encoded"),
        )?;
        // require outbound and inbound codec agreement before publishing the shared lease
        if bytes.len() > LOCK_FILE_BYTES || codec.validated_lease_expiry(&bytes) != Some(expires) {
            return Err(invalid(
                "revision spool lease codec differs from canonical expiry",
            ));
        }
        // retry contention only; unrelated disk failures fail closed immediately
        for _ in 0..LOCK_ATTEMPTS {
            match files::create_synced_file(&path, &bytes) {
                Ok(()) => {
                    sync_directory(directory)?;
                    return Ok(Self {
                        path,
                        bytes,
                        released: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    // retire only the original protocol's expired or crash-left lease
                    if lease_expired(&path, codec)? {
                        retire_expired(directory, &path)?;
                        continue;
                    }
                    thread::sleep(LOCK_RETRY);
                }
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "revision spool lease is busy",
        ))
    }

    /// Surface the ordinary finally-path release failure to the caller.
    fn finish(mut self) -> io::Result<()> {
        self.released = true;
        self.release()
    }

    /// Preserve an unrelated holder by comparing the complete durable lease bytes.
    fn release(&self) -> io::Result<()> {
        // release only the exact lease created by this invocation
        if read_optional_file(&self.path, LOCK_FILE_BYTES)?.as_deref()
            == Some(self.bytes.as_slice())
        {
            fs::remove_file(&self.path)?;
            let directory = self.path.parent().ok_or_else(
                // constrain parent fsync to the retained lock directory
                || invalid("revision spool lock has no parent"),
            )?;
            sync_directory(directory)?;
        }
        Ok(())
    }
}

impl Drop for Lease {
    /// Best-effort release while unwinding, preserving the normal explicit error path.
    fn drop(&mut self) {
        // avoid a second release after the explicit finish path
        if !self.released {
            let _ = self.release();
        }
    }
}

/// Trust only complete canonical expiry, falling back to age after an incomplete write.
fn lease_expired(path: &Path, codec: &impl RevisionSpoolLeaseCodec) -> io::Result<bool> {
    let now = SystemTime::now();
    // use the canonical lease when available before trying the existing age fallback
    if let Ok(bytes) = read_private_file(path, LOCK_FILE_BYTES)
        && let Some(expiry) = codec.validated_lease_expiry(&bytes)
    {
        return Ok(expiry <= wall_milliseconds(now)?);
    }
    let changed = files::lease_metadata(path)?.modified()?;
    Ok(now
        .duration_since(changed)
        .is_ok_and(|age| age >= LOCK_DURATION))
}

/// Move and remove one expired lease using the same fixed-directory protocol.
fn retire_expired(directory: &Path, path: &Path) -> io::Result<()> {
    let unique = uuid_from_linux_entropy()?;
    let stale = directory.join(format!(".stale-{unique}.lock"));
    match fs::rename(path, &stale) {
        Ok(()) => {
            fs::remove_file(stale)?;
            sync_directory(directory)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Read UUIDv4 randomness directly from the Linux kernel, without a crypto package.
fn uuid_from_linux_entropy() -> io::Result<String> {
    let mut bytes = [0_u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut full = [0_u8; 32];
    full[..16].copy_from_slice(&bytes);
    let hex = lower_hex(full);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

/// Convert the native wall clock to existing UTC milliseconds without civil date parsing.
fn wall_milliseconds(now: SystemTime) -> io::Result<i64> {
    let elapsed = now.duration_since(UNIX_EPOCH).map_err(
        // reject an unrepresentable pre-epoch operational wall clock
        |_| invalid("revision spool wall clock predates Unix time"),
    )?;
    i64::try_from(elapsed.as_millis()).map_err(
        // contain a value outside the existing signed timestamp envelope
        |_| invalid("revision spool wall clock overflows UTC milliseconds"),
    )
}

/// Keep only a clean existing-target outcome, not a failed temporary cleanup.
fn existing_immutable_name(error: &WriteError) -> bool {
    !error.final_created
        && !error.temporary_cleanup_failed
        && error.source.kind() == io::ErrorKind::AlreadyExists
}

/// Classify an error before any publish attempt.
fn unstarted_write_failure(source: io::Error) -> WriteError {
    WriteError {
        source,
        final_created: false,
        temporary_cleanup_failed: false,
    }
}

/// Redact operating-system details at the value-bearing spool boundary.
fn spool_error(_: io::Error) -> RevisionOnlineSpoolError {
    RevisionOnlineSpoolError::Storage
}

/// Redact operating-system details at the fixed-gauge archive boundary.
fn gap_storage_error(_: io::Error) -> RainFixedGaugeGapArchiveError {
    RainFixedGaugeGapArchiveError::Storage
}
