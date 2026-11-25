//! Shared Unix custody checks for root-installed immutable artifacts.

use std::fs::{self, File};
use std::io::{self, ErrorKind, Read};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";

/// Require one nonsymlinked root-owned single-link immutable-image file.
pub(crate) fn require_root_owned_regular_file(path: &Path) -> io::Result<()> {
    let status = fs::symlink_metadata(path)?;
    let resolved = fs::canonicalize(path)?;
    // reject links, alternate resolution, ownership, and writable non-root modes
    if !status.file_type().is_file()
        || status.nlink() != 1
        || status.uid() != 0
        || status.gid() != 0
        || status.mode() & 0o777 != 0o644
        || resolved != path
    {
        return Err(io::Error::new(
            ErrorKind::PermissionDenied,
            "installed artifact file authority is invalid",
        ));
    }
    Ok(())
}

/// Require one root-owned file at the first exact read-only VFS mount entry.
pub(crate) fn require_root_read_only_file(path: &str) -> io::Result<()> {
    require_root_owned_regular_file(Path::new(path))?;
    let mountinfo = fs::read_to_string(MOUNTINFO_PATH)?;
    let read_only = mountinfo.lines().find_map(|line| {
        let mut fields = line.split(' ');
        // use the first exact literal mount entry, as the retained loader does
        (fields.nth(4) == Some(path)).then(|| {
            fields
                .next()
                .is_some_and(|options| options.split(',').any(|option| option == "ro"))
        })
    });
    // refuse missing mounts and writable first matches without scanning later entries
    if read_only != Some(true) {
        return Err(io::Error::new(
            ErrorKind::PermissionDenied,
            "installed artifact mount is not read only",
        ));
    }
    Ok(())
}

/// Read one admitted immutable artifact without retaining more than the configured maximum.
pub(crate) fn read_root_read_only_bounded(path: &str, maximum_bytes: usize) -> io::Result<Vec<u8>> {
    require_root_read_only_file(path)?;
    let guarded_bytes = maximum_bytes.checked_add(1).ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "installed artifact byte limit is invalid",
        )
    })?;
    let guarded_bytes = u64::try_from(guarded_bytes).map_err(|_| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "installed artifact byte limit is invalid",
        )
    })?;
    let mut bytes = Vec::new();
    File::open(path)?
        .take(guarded_bytes)
        .read_to_end(&mut bytes)?;
    // detect an oversized artifact from the single guarded byte before any decoding
    if bytes.len() > maximum_bytes {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "installed artifact exceeds its byte limit",
        ));
    }
    Ok(bytes)
}
