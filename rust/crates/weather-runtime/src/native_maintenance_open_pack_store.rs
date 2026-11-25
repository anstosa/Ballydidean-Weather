//! Owner-private bounded revision custody files with exact checkpoint durability.

use crate::{
    NativeMaintenanceCapacityEvidence, NativeMaintenancePrivateDirectory,
    ensure_native_maintenance_private_directory, measure_native_maintenance_capacity_evidence,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const NATIVE_COLD_PACK_MAXIMUM_BYTES: usize = 32 * 1024 * 1024;
pub const NATIVE_COLD_TRANSFER_MAXIMUM_BYTES: usize = 4_832 * 1024;
pub const NATIVE_COLD_PACK_METADATA_MAXIMUM_BYTES: usize = 256 * 1024;
const FLOOR: u64 = 16 * 1024 * 1024 * 1024;
const CEILING: u64 = 64 * 1024 * 1024 * 1024;
const INODE_FLOOR: u64 = 32_768;
static NEXT: AtomicU64 = AtomicU64::new(0);

/// Exact storage effects of the source open pack protocol.
pub trait NativeMaintenanceOpenPackStore {
    /// Read a bounded checkpoint or an empty absent result.
    fn read_metadata(&mut self) -> io::Result<Vec<u8>>;
    /// Read the exact retained raw payload size.
    fn payload_size(&mut self) -> io::Result<usize>;
    /// Read one committed bounded payload slice.
    fn read_payload(&mut self, offset: usize, length: usize) -> io::Result<Vec<u8>>;
    /// Append and fsync one canonical raw range.
    fn append_payload(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// Publish a complete atomic metadata checkpoint.
    fn write_metadata(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// Trim only an uncheckpointed trailing payload suffix.
    fn truncate_payload(&mut self, length: usize) -> io::Result<()>;
    /// Remove the exact already sealed open pack files.
    fn remove(&mut self) -> io::Result<()>;
}

/// Fixed source state root and path-free physical capacity owner.
pub struct NativeMaintenanceFixedOpenPackStore {
    root: PathBuf,
    capacity: fn() -> io::Result<NativeMaintenanceCapacityEvidence>,
}

impl NativeMaintenanceFixedOpenPackStore {
    /// Bind the source private root without creating the two pack files.
    pub fn initialize() -> io::Result<Self> {
        let root =
            ensure_native_maintenance_private_directory(NativeMaintenancePrivateDirectory::State)?;
        Ok(Self {
            root,
            capacity: || measure_native_maintenance_capacity_evidence().map_err(|_| invalid()),
        })
    }

    /// construct an isolated fixture owner without entering production state
    #[cfg(test)]
    pub(crate) fn fixture(
        root: PathBuf,
        capacity: fn() -> io::Result<NativeMaintenanceCapacityEvidence>,
    ) -> Self {
        Self { root, capacity }
    }

    // require dual local and actual physical backing admission immediately before allocation
    fn admit(&self, bytes: usize, inodes: u64) -> io::Result<()> {
        let measured = (self.capacity)()?;
        let block = measured.ext4_block_size;
        let count = u64::try_from(bytes).map_err(|_| invalid())?;
        let allocated = count
            .checked_add(block.checked_sub(1).ok_or_else(invalid)?)
            .and_then(|n| n.checked_div(block))
            .and_then(|n| n.checked_mul(block))
            .ok_or_else(invalid)?;
        let required = FLOOR.checked_add(allocated).ok_or_else(invalid)?;
        let required_inodes = INODE_FLOOR.checked_add(inodes).ok_or_else(invalid)?;
        // preserve the source staging ceiling and both independent space floors
        if count > 4 * 1024 * 1024 * 1024
            || measured
                .backing
                .allocated_bytes
                .checked_add(allocated)
                .is_none_or(|n| n > CEILING)
            || measured.backing.free_bytes < required
            || measured.ext4_free_bytes < required
            || measured.backing.free_inodes < required_inodes
            || measured.ext4_free_inodes < required_inodes
        {
            return Err(invalid());
        }
        Ok(())
    }

    // map only the fixed source raw and checkpoint names
    fn payload(&self) -> PathBuf {
        self.root.join("revision-custody-open.payload")
    }
    fn metadata(&self) -> PathBuf {
        self.root.join("revision-custody-open.current")
    }

    // fsync the parent after each durable namespace effect
    fn sync_root(&self) -> io::Result<()> {
        File::open(&self.root)?.sync_all()
    }
}

impl NativeMaintenanceOpenPackStore for NativeMaintenanceFixedOpenPackStore {
    /// Return only exact private bounded metadata.
    fn read_metadata(&mut self) -> io::Result<Vec<u8>> {
        let path = self.metadata();
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let length = validate_file(&path, &file)?;
        if length > NATIVE_COLD_PACK_METADATA_MAXIMUM_BYTES {
            return Err(invalid());
        }
        let mut bytes = Vec::with_capacity(length);
        Read::by_ref(&mut file)
            .take((NATIVE_COLD_PACK_METADATA_MAXIMUM_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() != length {
            return Err(invalid());
        }
        Ok(bytes)
    }

    /// Obtain the validated existing payload length.
    fn payload_size(&mut self) -> io::Result<usize> {
        let path = self.payload();
        match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
        {
            Ok(file) => validate_file(&path, &file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
            Err(error) => Err(error),
        }
    }

    /// Return one complete bounded exact slice from the same admitted descriptor.
    fn read_payload(&mut self, offset: usize, length: usize) -> io::Result<Vec<u8>> {
        let path = self.payload();
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        let size = validate_file(&path, &file)?;
        if length == 0
            || offset
                .checked_add(length)
                .is_none_or(|end| end > size || end > NATIVE_COLD_PACK_MAXIMUM_BYTES)
        {
            return Err(invalid());
        }
        let mut bytes = vec![0; length];
        file.seek(SeekFrom::Start(offset as u64))?;
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    /// Append only a bounded source transfer range after capacity admission.
    fn append_payload(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() || bytes.len() > NATIVE_COLD_TRANSFER_MAXIMUM_BYTES {
            return Err(invalid());
        }
        self.admit(bytes.len(), 0)?;
        let path = self.payload();
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        let size = validate_file(&path, &file)?;
        if size
            .checked_add(bytes.len())
            .is_none_or(|end| end > NATIVE_COLD_PACK_MAXIMUM_BYTES)
        {
            return Err(invalid());
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        self.sync_root()
    }

    /// Replace checkpoint metadata only after its private incoming file is fsynced.
    fn write_metadata(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() || bytes.len() > NATIVE_COLD_PACK_METADATA_MAXIMUM_BYTES {
            return Err(invalid());
        }
        self.admit(bytes.len(), 1)?;
        let path = self.root.join(format!(
            ".revision-custody-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        let result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            validate_file(&path, &file)?;
            fs::rename(&path, self.metadata())?;
            self.sync_root()
        })();
        // remove only the unpublished private path owned by this operation
        if result.is_err() && path.exists() {
            let _ = fs::remove_file(path);
        }
        result
    }

    /// Trim an uncommitted suffix without expanding the existing raw file.
    fn truncate_payload(&mut self, length: usize) -> io::Result<()> {
        let path = self.payload();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        if length > validate_file(&path, &file)? {
            return Err(invalid());
        }
        file.set_len(length as u64)?;
        file.sync_all()?;
        self.sync_root()
    }

    /// Remove only admitted fixed owner-private files after the seal owner verifies the checkpoint.
    fn remove(&mut self) -> io::Result<()> {
        for path in [self.payload(), self.metadata()] {
            let file = match OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            validate_file(&path, &file)?;
            fs::remove_file(path)?;
            self.sync_root()?;
        }
        Ok(())
    }
}

// retain the source file metadata and literal path admission on the held descriptor
fn validate_file(path: &Path, file: &File) -> io::Result<usize> {
    let stat = file.metadata()?;
    let named = fs::symlink_metadata(path)?;
    // safety: getuid/getgid have no memory preconditions
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    if !stat.is_file()
        || stat.uid() != uid
        || stat.gid() != gid
        || stat.nlink() != 1
        || stat.mode() & 0o777 != 0o600
        || stat.dev() != named.dev()
        || stat.ino() != named.ino()
        || path.canonicalize()? != path
    {
        return Err(invalid());
    }
    usize::try_from(stat.len()).map_err(|_| invalid())
}

// bounded source filesystem failure
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "revision custody pack storage unavailable",
    )
}
