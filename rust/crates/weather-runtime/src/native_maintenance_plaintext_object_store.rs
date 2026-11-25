//! Durable fixed-root immutable object storage for the native plaintext archive.

use std::fmt::{self, Display, Formatter};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::native_maintenance_backing_capacity::{
    NativeMaintenanceBackingCapacity, NativeMaintenanceCapacityEvidence,
};
use crate::native_maintenance_cas_pack::{
    NativeMaintenanceCasInventory, NativeMaintenanceCasStreamValidator,
    inspect_native_maintenance_cas_stream,
};
use crate::native_maintenance_controller_lock::{
    NativeMaintenancePrivateDirectory, ensure_native_maintenance_private_directory,
};
use crate::native_maintenance_full_graph::{
    NativeMaintenanceFullGraphObjects, NativeMaintenanceVerifiedFullGraph,
};
use crate::native_maintenance_retirement_files_v2::format_digest;

pub const NATIVE_MAINTENANCE_ARCHIVE_ROOT_KIND: &str = "home_native_ext4_primary";

const ARCHIVE_MAXIMUM_BYTES: u64 = 18 * 1_024 * 1_024 * 1_024;
const TOTAL_STORAGE_CEILING_BYTES: u64 = 64 * 1_024 * 1_024 * 1_024;
const FREE_SPACE_FLOOR_BYTES: u64 = 16 * 1_024 * 1_024 * 1_024;
const MAXIMUM_COMMITTED_OBJECTS: u64 = 16_744;
const MAXIMUM_INCOMING_OBJECT_FILES: u64 = 1;
const MAXIMUM_ARCHIVE_FILES: u64 = 16_747;
const MAXIMUM_TASK_INODES: u64 = 17_899;
const STREAM_BUFFER_BYTES: usize = 256 * 1_024;
const EXT4_MAGIC: libc::c_long = 0xef53;
static INCOMING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// One explicit pre-mutation archive and physical-capacity observation.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenancePlaintextObjectAdmission<'a> {
    pub envelope: &'a NativeMaintenancePlaintextPublicationEnvelope,
    pub free_inode_floor: u64,
}

/// Exact source publication counts, allocation, filesystem, and backing evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenancePlaintextPublicationEnvelope {
    pub allocated_bytes: u64,
    pub backing: NativeMaintenanceBackingCapacity,
    pub block_size: u64,
    pub file_count: u64,
    pub free_bytes: u64,
    pub free_inodes: u64,
    pub incoming_count: u64,
    pub object_count: u64,
    pub task_inodes: u64,
}

/// Path-free result of one immutable content-addressed publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenancePlaintextObjectCommit {
    pub byte_length: u64,
    pub created: bool,
    pub object_sha256: String,
}

/// Closed failures for the fixed object root, admission, and durable publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenancePlaintextObjectStoreError {
    Directory,
    Filesystem,
    ObjectIdentity,
    ObjectMetadata,
    Capacity,
    File,
    Exhausted,
    Cas,
}

impl Display for NativeMaintenancePlaintextObjectStoreError {
    /// Render one path-free source operation failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Directory => "maintenance archive object directory is invalid",
            Self::Filesystem => "maintenance archive object filesystem is invalid",
            Self::ObjectIdentity => "maintenance archive object identity is invalid",
            Self::ObjectMetadata => "maintenance archive object metadata is invalid",
            Self::Capacity => "maintenance archive object capacity is refused",
            Self::File => "maintenance archive object file operation failed",
            Self::Exhausted => "maintenance archive incoming names are exhausted",
            Self::Cas => "maintenance archive CAS object is invalid",
        })
    }
}

impl std::error::Error for NativeMaintenancePlaintextObjectStoreError {}

/// One held no-follow immutable object reader with bounded random access.
#[derive(Debug)]
pub struct NativeMaintenancePlaintextObjectReader {
    file: File,
    size: u64,
}

impl NativeMaintenancePlaintextObjectReader {
    /// Return the size proven from the held private regular file.
    pub const fn size(&self) -> u64 {
        self.size
    }

    /// Read one source-sized random-access range without reopening the path.
    pub fn read_range(
        &self,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, NativeMaintenancePlaintextObjectStoreError> {
        // keep every direct range within the source stream buffer ceiling
        if length > STREAM_BUFFER_BYTES {
            return Err(NativeMaintenancePlaintextObjectStoreError::File);
        }
        let mut bytes = vec![0_u8; length];
        let count = self
            .file
            .read_at(&mut bytes, offset)
            .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
        bytes.truncate(count);
        Ok(bytes)
    }
}

impl Read for NativeMaintenancePlaintextObjectReader {
    /// Stream from the held descriptor for bounded CAS inspection.
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(bytes)
    }
}

impl Seek for NativeMaintenancePlaintextObjectReader {
    /// Seek within the same held immutable descriptor for verified member rereads.
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}

/// Fixed-root owner for immutable content-addressed object bytes.
#[derive(Debug)]
pub struct NativeMaintenancePlaintextObjectStore {
    archive_root: PathBuf,
    object_root: PathBuf,
    uid: u32,
}

impl NativeMaintenancePlaintextObjectStore {
    /// Initialize and prove the original fixed native-ext4 object root.
    pub fn initialize() -> Result<Self, NativeMaintenancePlaintextObjectStoreError> {
        let object_root =
            ensure_native_maintenance_private_directory(NativeMaintenancePrivateDirectory::Archive)
                .map_err(|_| NativeMaintenancePlaintextObjectStoreError::Directory)?;
        let archive_root = object_root
            .parent()
            .ok_or(NativeMaintenancePlaintextObjectStoreError::Directory)?;
        validate_private_directory(archive_root)?;
        validate_private_directory(&object_root)?;
        require_ext4(archive_root)?;
        Ok(Self {
            archive_root: archive_root.to_path_buf(),
            object_root,
            uid: current_uid(),
        })
    }

    /// Construct a disposable owner while retaining identical file semantics.
    #[cfg(test)]
    pub(crate) fn initialize_fixture(
        archive_root: PathBuf,
    ) -> Result<Self, NativeMaintenancePlaintextObjectStoreError> {
        validate_private_directory(&archive_root)?;
        let object_root = archive_root.join("objects");
        validate_private_directory(&object_root)?;
        Ok(Self {
            archive_root,
            object_root,
            uid: current_uid(),
        })
    }

    /// Return the original path-free fixed-root identity.
    pub const fn root_kind(&self) -> &'static str {
        NATIVE_MAINTENANCE_ARCHIVE_ROOT_KIND
    }

    /// Measure the source publication envelope without charging directory allocations.
    pub fn measure(
        &self,
        capacity: &NativeMaintenanceCapacityEvidence,
    ) -> Result<
        NativeMaintenancePlaintextPublicationEnvelope,
        NativeMaintenancePlaintextObjectStoreError,
    > {
        let mut allocated_bytes = 0_u64;
        let mut object_count = 0_u64;
        let mut incoming_count = 0_u64;
        let entries = fs::read_dir(&self.object_root)
            .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
        // count every actual object-root entry without recursively scanning other storage
        for entry in entries {
            let entry = entry.map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| NativeMaintenancePlaintextObjectStoreError::ObjectMetadata)?;
            let details = fs::symlink_metadata(entry.path())
                .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
            require_private_file(&details, self.uid)?;
            // classify only committed content addresses or the source incoming namespace
            if object_sha256_from_name(&name).is_some() {
                object_count = object_count
                    .checked_add(1)
                    .ok_or(NativeMaintenancePlaintextObjectStoreError::Capacity)?;
            } else if name.starts_with(".incoming-") {
                incoming_count = incoming_count
                    .checked_add(1)
                    .ok_or(NativeMaintenancePlaintextObjectStoreError::Capacity)?;
            } else {
                return Err(NativeMaintenancePlaintextObjectStoreError::ObjectMetadata);
            }
            allocated_bytes = add_allocated_bytes(allocated_bytes, &details)?;
        }
        let mut pointer_count = 0_u64;
        // count only the two fixed pointer slots and their allocated blocks
        for name in ["head.current", "head.previous"] {
            let path = self.archive_root.join(name);
            // preserve genuine absence as an unused pointer slot
            match fs::symlink_metadata(path) {
                Ok(details) => {
                    require_private_file(&details, self.uid)?;
                    pointer_count = pointer_count
                        .checked_add(1)
                        .ok_or(NativeMaintenancePlaintextObjectStoreError::Capacity)?;
                    allocated_bytes = add_allocated_bytes(allocated_bytes, &details)?;
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(_) => return Err(NativeMaintenancePlaintextObjectStoreError::File),
            }
        }
        let file_count = object_count
            .checked_add(incoming_count)
            .and_then(|value| value.checked_add(pointer_count))
            .ok_or(NativeMaintenancePlaintextObjectStoreError::Capacity)?;
        Ok(NativeMaintenancePlaintextPublicationEnvelope {
            allocated_bytes,
            backing: capacity.backing,
            block_size: capacity.ext4_block_size,
            file_count,
            free_bytes: capacity.ext4_free_bytes,
            free_inodes: capacity.ext4_free_inodes,
            incoming_count,
            object_count,
            task_inodes: file_count,
        })
    }

    /// Describe only an owner-private committed object, treating absence as unpublished.
    pub fn has_object(
        &self,
        object_sha256: &str,
    ) -> Result<bool, NativeMaintenancePlaintextObjectStoreError> {
        let path = self.object_path(object_sha256)?;
        // preserve only genuine absence as an unpublished object
        match fs::symlink_metadata(path) {
            Ok(details) => {
                require_private_file(&details, self.uid)?;
                Ok(true)
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(_) => Err(NativeMaintenancePlaintextObjectStoreError::File),
        }
    }

    /// Open one held no-follow immutable object reader when the digest exists.
    pub fn open_object(
        &self,
        object_sha256: &str,
    ) -> Result<
        Option<NativeMaintenancePlaintextObjectReader>,
        NativeMaintenancePlaintextObjectStoreError,
    > {
        let path = self.object_path(object_sha256)?;
        let mut options = OpenOptions::new();
        options.read(true).custom_flags(libc::O_NOFOLLOW);
        let file = match options.open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(NativeMaintenancePlaintextObjectStoreError::File),
        };
        let details = file
            .metadata()
            .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
        require_private_file(&details, self.uid)?;
        Ok(Some(NativeMaintenancePlaintextObjectReader {
            file,
            size: details.size(),
        }))
    }

    /// Publish one exact digest-derived object without overwriting an existing inode.
    pub fn commit_object(
        &self,
        bytes: &[u8],
        admission: NativeMaintenancePlaintextObjectAdmission<'_>,
    ) -> Result<NativeMaintenancePlaintextObjectCommit, NativeMaintenancePlaintextObjectStoreError>
    {
        let byte_length = u64::try_from(bytes.len())
            .map_err(|_| NativeMaintenancePlaintextObjectStoreError::Capacity)?;
        let object_sha256 = lower_hex(&Sha256::digest(bytes));
        // validate and reuse exact existing immutable bytes before charging capacity
        if self.has_object(&object_sha256)? {
            self.require_object_hash(&object_sha256)?;
            return Ok(NativeMaintenancePlaintextObjectCommit {
                byte_length,
                created: false,
                object_sha256,
            });
        }
        admit_publication(byte_length, admission)?;
        let final_path = self.object_path(&object_sha256)?;
        let (temporary_path, mut temporary) = self.create_incoming()?;
        // write the incoming object through the source's fixed stream buffer
        for chunk in bytes.chunks(STREAM_BUFFER_BYTES) {
            temporary
                .write_all(chunk)
                .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
        }
        temporary
            .sync_all()
            .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
        drop(temporary);
        let details = fs::symlink_metadata(&temporary_path)
            .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
        require_private_file(&details, self.uid)?;
        // prove incoming length and content identity before exposing a committed name
        if details.size() != byte_length
            || hash_private_file(&temporary_path, self.uid)? != object_sha256
        {
            return Err(NativeMaintenancePlaintextObjectStoreError::ObjectIdentity);
        }
        // publish by link so no existing immutable filename can be overwritten
        let created = match fs::hard_link(&temporary_path, &final_path) {
            Ok(()) => true,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                self.require_object_hash(&object_sha256)?;
                false
            }
            Err(_) => return Err(NativeMaintenancePlaintextObjectStoreError::File),
        };
        fs::remove_file(&temporary_path)
            .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
        sync_directory(&self.object_root)?;
        self.require_object_hash(&object_sha256)?;
        Ok(NativeMaintenancePlaintextObjectCommit {
            byte_length,
            created,
            object_sha256,
        })
    }

    /// Stream-validate one retained CAS object and bind it to its requested digest.
    pub fn inspect_object(
        &self,
        object_sha256: &str,
        validators: &mut impl NativeMaintenanceCasStreamValidator,
    ) -> Result<Option<NativeMaintenanceCasInventory>, NativeMaintenancePlaintextObjectStoreError>
    {
        // preserve genuine absence without manufacturing an empty inventory
        let Some(reader) = self.open_object(object_sha256)? else {
            return Ok(None);
        };
        let size = reader.size();
        let inventory = inspect_native_maintenance_cas_stream(reader, size, validators)
            .ok_or(NativeMaintenancePlaintextObjectStoreError::Cas)?;
        // bind parser output to both the requested path digest and held file size
        if inventory.byte_length != size || lower_hex(&inventory.object_sha256) != object_sha256 {
            return Err(NativeMaintenancePlaintextObjectStoreError::ObjectIdentity);
        }
        Ok(Some(inventory))
    }

    /// Read the actual fixed owner-private current graph head, preserving empty genesis.
    pub fn read_head(
        &self,
    ) -> Result<Option<[u8; 32]>, NativeMaintenancePlaintextObjectStoreError> {
        use NativeMaintenancePlaintextObjectStoreError as Error;
        let path = self.archive_root.join("head.current");
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(Error::File),
        };
        let details = file.metadata().map_err(|_| Error::File)?;
        require_private_file(&details, self.uid)?;
        // admit only the exact source 64 byte lowercase digest and trailing LF
        if details.size() != 65 {
            return Err(Error::File);
        }
        let mut bytes = Vec::with_capacity(66);
        file.take(66)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::File)?;
        if bytes.len() != 65 || bytes[64] != b'\n' {
            return Err(Error::File);
        }
        let digest = std::str::from_utf8(&bytes[..64]).map_err(|_| Error::ObjectIdentity)?;
        let admitted =
            weather_edge::RevisionGapSha256::from_lower_hex(digest).ok_or(Error::ObjectIdentity)?;
        Ok(Some(*admitted.as_bytes()))
    }

    /// Rotate a separately verified full-graph head through the fixed pointer pair.
    pub fn rotate_verified_head(
        &self,
        verified: &NativeMaintenanceVerifiedFullGraph,
    ) -> Result<(), NativeMaintenancePlaintextObjectStoreError> {
        let digest = format_digest(verified.manifest_object_sha256);
        self.require_object_hash(&digest)?;
        let current_path = self.archive_root.join("head.current");
        let previous_path = self.archive_root.join("head.previous");
        let (temporary_path, mut temporary) = self.create_head_temporary()?;
        temporary
            .write_all(format!("{digest}\n").as_bytes())
            .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
        temporary
            .sync_all()
            .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
        drop(temporary);
        // remove only an existing previous slot before the source rotation
        match fs::remove_file(&previous_path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(NativeMaintenancePlaintextObjectStoreError::File),
        }
        // move current to previous while admitting the genesis absence
        match fs::rename(&current_path, &previous_path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(NativeMaintenancePlaintextObjectStoreError::File),
        }
        fs::rename(&temporary_path, &current_path)
            .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
        sync_directory(&self.archive_root)?;
        Ok(())
    }

    /// Derive one closed object path from an exact lowercase digest.
    fn object_path(
        &self,
        object_sha256: &str,
    ) -> Result<PathBuf, NativeMaintenancePlaintextObjectStoreError> {
        // reject paths, uppercase aliases, and every malformed object identity
        if !valid_sha256(object_sha256) {
            return Err(NativeMaintenancePlaintextObjectStoreError::ObjectIdentity);
        }
        Ok(self.object_root.join(format!("sha256-{object_sha256}.obj")))
    }

    /// Create one unique retained incoming file without scanning the object tree.
    fn create_incoming(
        &self,
    ) -> Result<(PathBuf, File), NativeMaintenancePlaintextObjectStoreError> {
        // retry only bounded process-unique name collisions
        for _ in 0..64 {
            let sequence = INCOMING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = self
                .object_root
                .join(format!(".incoming-{}-{sequence}", std::process::id()));
            let mut options = OpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW);
            // preserve collisions without weakening exclusive creation
            match options.open(&path) {
                Ok(file) => return Ok((path, file)),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(NativeMaintenancePlaintextObjectStoreError::File),
            }
        }
        Err(NativeMaintenancePlaintextObjectStoreError::Exhausted)
    }

    /// Create one unique fixed-root head temporary without replacing any file.
    fn create_head_temporary(
        &self,
    ) -> Result<(PathBuf, File), NativeMaintenancePlaintextObjectStoreError> {
        // retry only bounded process-unique temporary-name collisions
        for _ in 0..64 {
            let sequence = INCOMING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = self
                .archive_root
                .join(format!(".head-{}-{sequence}.tmp", std::process::id()));
            let mut options = OpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW);
            // preserve collisions without weakening exclusive creation
            match options.open(&path) {
                Ok(file) => return Ok((path, file)),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(NativeMaintenancePlaintextObjectStoreError::File),
            }
        }
        Err(NativeMaintenancePlaintextObjectStoreError::Exhausted)
    }

    /// Reopen and hash one committed object through a bounded buffer.
    fn require_object_hash(
        &self,
        object_sha256: &str,
    ) -> Result<(), NativeMaintenancePlaintextObjectStoreError> {
        let path = self.object_path(object_sha256)?;
        // refuse every existing filename whose actual immutable bytes do not match
        if hash_private_file(&path, self.uid)? != object_sha256 {
            return Err(NativeMaintenancePlaintextObjectStoreError::ObjectIdentity);
        }
        Ok(())
    }
}

impl NativeMaintenanceFullGraphObjects for NativeMaintenancePlaintextObjectStore {
    type Reader = NativeMaintenancePlaintextObjectReader;

    /// Open one exact digest through the retained no-follow object descriptor.
    fn open_object(&mut self, digest: [u8; 32]) -> Option<(Self::Reader, u64)> {
        let reader =
            NativeMaintenancePlaintextObjectStore::open_object(self, &format_digest(digest))
                .ok()??;
        let size = reader.size();
        Some((reader, size))
    }
}

/// Admit the source's complete peak file, inode, allocation, and dual-floor delta.
fn admit_publication(
    logical_bytes: u64,
    admission: NativeMaintenancePlaintextObjectAdmission<'_>,
) -> Result<(), NativeMaintenancePlaintextObjectStoreError> {
    use NativeMaintenancePlaintextObjectStoreError as Error;

    let envelope = admission.envelope;
    let block_size = envelope.block_size;
    // require a genuine positive allocation unit before rounded accounting
    if block_size == 0 {
        return Err(Error::Capacity);
    }
    // preserve the source zero-byte allocation and checked block rounding
    let allocated_delta = if logical_bytes == 0 {
        0
    } else {
        logical_bytes
            .checked_add(block_size - 1)
            .and_then(|value| value.checked_div(block_size))
            .and_then(|value| value.checked_mul(block_size))
            .ok_or(Error::Capacity)?
    };
    // preserve every exact committed, incoming, peak file, and task-inode ceiling
    if envelope
        .object_count
        .checked_add(1)
        .ok_or(Error::Capacity)?
        > MAXIMUM_COMMITTED_OBJECTS
        || envelope
            .incoming_count
            .checked_add(1)
            .ok_or(Error::Capacity)?
            > MAXIMUM_INCOMING_OBJECT_FILES
        || envelope.file_count.checked_add(2).ok_or(Error::Capacity)? > MAXIMUM_ARCHIVE_FILES
        || envelope.task_inodes.checked_add(1).ok_or(Error::Capacity)? > MAXIMUM_TASK_INODES
    {
        return Err(Error::Capacity);
    }
    let next_allocated = envelope
        .allocated_bytes
        .checked_add(allocated_delta)
        .ok_or(Error::Capacity)?;
    let next_backing_allocated = envelope
        .backing
        .allocated_bytes
        .checked_add(allocated_delta)
        .ok_or(Error::Capacity)?;
    let required_free_bytes = FREE_SPACE_FLOOR_BYTES
        .checked_add(allocated_delta)
        .ok_or(Error::Capacity)?;
    let required_free_inodes = admission
        .free_inode_floor
        .checked_add(1)
        .ok_or(Error::Capacity)?;
    // preserve allocated ceilings and both source byte and inode floors
    if next_allocated > ARCHIVE_MAXIMUM_BYTES
        || next_backing_allocated > TOTAL_STORAGE_CEILING_BYTES
        || envelope.free_bytes < required_free_bytes
        || envelope.backing.free_bytes < required_free_bytes
        || envelope.free_inodes < required_free_inodes
        || envelope.backing.free_inodes < required_free_inodes
    {
        return Err(Error::Capacity);
    }
    Ok(())
}

/// Add one file's fixed 512-byte allocated block charge without wrapping.
fn add_allocated_bytes(
    current: u64,
    details: &Metadata,
) -> Result<u64, NativeMaintenancePlaintextObjectStoreError> {
    let allocated = details
        .blocks()
        .checked_mul(512)
        .ok_or(NativeMaintenancePlaintextObjectStoreError::Capacity)?;
    current
        .checked_add(allocated)
        .ok_or(NativeMaintenancePlaintextObjectStoreError::Capacity)
}

/// Borrow the digest from one exact committed object filename.
fn object_sha256_from_name(name: &str) -> Option<&str> {
    let digest = name.strip_prefix("sha256-")?.strip_suffix(".obj")?;
    valid_sha256(digest).then_some(digest)
}

/// Prove one literal current-owner private directory and its canonical path.
fn validate_private_directory(
    path: &Path,
) -> Result<(), NativeMaintenancePlaintextObjectStoreError> {
    let details = fs::symlink_metadata(path)
        .map_err(|_| NativeMaintenancePlaintextObjectStoreError::Directory)?;
    let canonical = fs::canonicalize(path)
        .map_err(|_| NativeMaintenancePlaintextObjectStoreError::Directory)?;
    // reject links, foreign ownership, broad modes, and moved literal roots
    if !details.is_dir()
        || details.file_type().is_symlink()
        || details.uid() != current_uid()
        || details.permissions().mode() & 0o777 != 0o700
        || canonical != path
    {
        return Err(NativeMaintenancePlaintextObjectStoreError::Directory);
    }
    Ok(())
}

/// Require the original native ext4 filesystem identity for production storage.
fn require_ext4(path: &Path) -> Result<(), NativeMaintenancePlaintextObjectStoreError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| NativeMaintenancePlaintextObjectStoreError::Filesystem)?;
    let mut filesystem = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // safety: the nul-terminated path remains live and libc initializes output on success
    let status = unsafe { libc::statfs(path.as_ptr(), filesystem.as_mut_ptr()) };
    // refuse failed observations before reading the output buffer
    if status != 0 {
        return Err(NativeMaintenancePlaintextObjectStoreError::Filesystem);
    }
    // safety: successful statfs initialized the complete structure
    let filesystem = unsafe { filesystem.assume_init() };
    // retain only the source native-ext4 archive filesystem
    if filesystem.f_type != EXT4_MAGIC {
        return Err(NativeMaintenancePlaintextObjectStoreError::Filesystem);
    }
    Ok(())
}

/// Require one owner-private single-link regular object entry.
fn require_private_file(
    details: &Metadata,
    uid: u32,
) -> Result<(), NativeMaintenancePlaintextObjectStoreError> {
    // reject links, special files, foreign ownership, broad modes, and aliases
    if !details.is_file()
        || details.file_type().is_symlink()
        || details.uid() != uid
        || details.permissions().mode() & 0o777 != 0o600
        || details.nlink() != 1
    {
        return Err(NativeMaintenancePlaintextObjectStoreError::ObjectMetadata);
    }
    Ok(())
}

/// Hash one no-follow private file through the original bounded buffer.
fn hash_private_file(
    path: &Path,
    uid: u32,
) -> Result<String, NativeMaintenancePlaintextObjectStoreError> {
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
    let details = file
        .metadata()
        .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
    require_private_file(&details, uid)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
    // consume the complete held file without retaining its object bytes
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
        // stop only on exact descriptor EOF
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(lower_hex(&hash.finalize()))
}

/// Fsync one held no-follow private object directory.
fn sync_directory(path: &Path) -> Result<(), NativeMaintenancePlaintextObjectStoreError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY);
    let directory = options
        .open(path)
        .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)?;
    directory
        .sync_all()
        .map_err(|_| NativeMaintenancePlaintextObjectStoreError::File)
}

/// Recognize one exact lowercase SHA-256 source identity.
fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Encode one digest using the source lowercase hexadecimal spelling.
fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    // encode every digest byte without formatting or locale behavior
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

/// Read the same real process identity used by the predecessor owner checks.
fn current_uid() -> u32 {
    // safety: getuid observes the current process without pointer arguments
    unsafe { libc::getuid() }
}
