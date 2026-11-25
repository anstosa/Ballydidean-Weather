//! Native durable file effects for shadow-metadata custody transfer completion.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::shadow_metadata_custody::{
    NativeShadowMetadataCustodyFiles, ShadowMetadataCustodyError, ShadowMetadataCustodySlot,
};
use crate::shadow_metadata_custody_mutation::ShadowMetadataCustodyWrites;

// linux uapi directory and final-component link protections for supported targets
#[cfg(target_arch = "x86_64")]
const O_NOFOLLOW: i32 = 1 << 17;
#[cfg(target_arch = "aarch64")]
const O_NOFOLLOW: i32 = 1 << 15;
#[cfg(target_arch = "x86_64")]
const DIRECTORY_NOFOLLOW: i32 = (1 << 16) | O_NOFOLLOW;
#[cfg(target_arch = "aarch64")]
const DIRECTORY_NOFOLLOW: i32 = (1 << 14) | O_NOFOLLOW;
const TEMPORARY_ATTEMPTS: usize = 64;
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

impl ShadowMetadataCustodyWrites for NativeShadowMetadataCustodyFiles {
    /// Create only the fixed private proof directory beneath an existing private root.
    fn ensure_root(&mut self) -> Result<(), ShadowMetadataCustodyError> {
        let parent = self
            .proof_root
            .parent()
            .ok_or(ShadowMetadataCustodyError::Root)?;
        require_private_directory(parent, None, None)?;
        let mut builder = DirBuilder::new();
        // preserve a pre-existing path for exact validation rather than repairing it
        match builder.mode(0o700).create(&self.proof_root) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err(ShadowMetadataCustodyError::Root),
        }
        require_private_directory(&self.proof_root, Some(self.uid), Some(self.gid))
    }

    /// Publish the first preparation through an exclusive hard link and sync its directory.
    fn create_preparation(&mut self, bytes: &[u8]) -> Result<bool, ShadowMetadataCustodyError> {
        let temporary = write_private_temporary(&self.proof_root, "prepare", bytes)
            .map_err(|_| ShadowMetadataCustodyError::Write)?;
        let target = self.proof_root.join("preparation.json");
        // distinguish only a competing first preparation at the final exclusive link
        match fs::hard_link(&temporary, target) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                cleanup_private_temporary(&temporary)
                    .map_err(|_| ShadowMetadataCustodyError::Write)?;
                return Ok(false);
            }
            Err(_) => {
                cleanup_private_temporary(&temporary)
                    .map_err(|_| ShadowMetadataCustodyError::Write)?;
                return Err(ShadowMetadataCustodyError::Write);
            }
        }
        cleanup_private_temporary(&temporary).map_err(|_| ShadowMetadataCustodyError::Write)?;
        sync_private_directory(&self.proof_root).map_err(|_| ShadowMetadataCustodyError::Write)?;
        Ok(true)
    }

    /// Atomically replace one retained terminal slot and sync its directory.
    fn replace_previous(
        &mut self,
        slot: ShadowMetadataCustodySlot,
        bytes: &[u8],
    ) -> Result<(), ShadowMetadataCustodyError> {
        let filename = match slot {
            ShadowMetadataCustodySlot::Previous => "previous.json",
            ShadowMetadataCustodySlot::PreviousPreparation => "previous-preparation.json",
            ShadowMetadataCustodySlot::PreviousConsumption => "previous-consumption.json",
            ShadowMetadataCustodySlot::Current
            | ShadowMetadataCustodySlot::Pending
            | ShadowMetadataCustodySlot::Preparation => {
                return Err(ShadowMetadataCustodyError::Write);
            }
        };
        let temporary = write_private_temporary(&self.proof_root, "consume", bytes)
            .map_err(|_| ShadowMetadataCustodyError::Write)?;
        // rename only this call's complete private temporary over the selected fixed slot
        if fs::rename(&temporary, self.proof_root.join(filename)).is_err() {
            cleanup_private_temporary(&temporary).map_err(|_| ShadowMetadataCustodyError::Write)?;
            return Err(ShadowMetadataCustodyError::Write);
        }
        sync_private_directory(&self.proof_root).map_err(|_| ShadowMetadataCustodyError::Write)
    }
}

// require one literal canonical private directory without following a final link
fn require_private_directory(
    path: &Path,
    uid: Option<u32>,
    gid: Option<u32>,
) -> Result<(), ShadowMetadataCustodyError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ShadowMetadataCustodyError::Root)?;
    // retain the existing mode, path identity and optional service ownership
    if !metadata.is_dir()
        || metadata.mode() & 0o777 != 0o700
        || fs::canonicalize(path).map_err(|_| ShadowMetadataCustodyError::Root)? != path
        || uid.is_some_and(|value| metadata.uid() != value)
        || gid.is_some_and(|value| metadata.gid() != value)
    {
        return Err(ShadowMetadataCustodyError::Root);
    }
    Ok(())
}

// write and sync one uniquely owned private temporary before publication
pub(crate) fn write_private_temporary(
    directory: &Path,
    purpose: &str,
    bytes: &[u8],
) -> io::Result<PathBuf> {
    let (path, mut file) = create_private_temporary(directory, purpose)?;
    // remove only this call's unpublished temporary after an incomplete write or sync
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        // prefer a cleanup failure because it leaves a transaction inode behind
        return match cleanup_private_temporary(&path) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(cleanup),
        };
    }
    drop(file);
    Ok(path)
}

// exclusively create one bounded-attempt temporary in the already admitted directory
fn create_private_temporary(directory: &Path, purpose: &str) -> io::Result<(PathBuf, File)> {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // retry only a collision with an independently existing temporary name
    for _ in 0..TEMPORARY_ATTEMPTS {
        let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            ".custody-{purpose}-{}-{epoch}-{sequence}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "shadow metadata custody temporary names are exhausted",
    ))
}

// remove only the unique temporary created by the current operation
pub(crate) fn cleanup_private_temporary(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Sync one existing directory descriptor without following a replacement final component.
pub(crate) fn sync_private_directory(path: &Path) -> io::Result<()> {
    OpenOptions::new()
        .read(true)
        .custom_flags(DIRECTORY_NOFOLLOW)
        .open(path)?
        .sync_all()
}
