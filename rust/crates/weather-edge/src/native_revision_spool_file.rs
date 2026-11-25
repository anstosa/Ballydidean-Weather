//! Linux std-backed immutable file primitives for the retained revision spool.

use std::fs::{self, DirBuilder, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

// linux uapi asm-generic/fcntl.h and arm64's compat-aware asm/fcntl.h
#[cfg(target_arch = "x86_64")]
const O_DIRECTORY: i32 = 1 << 16;
#[cfg(target_arch = "x86_64")]
const O_NOFOLLOW: i32 = 1 << 17;
#[cfg(target_arch = "aarch64")]
const O_DIRECTORY: i32 = 1 << 14;
#[cfg(target_arch = "aarch64")]
const O_NOFOLLOW: i32 = 1 << 15;
const O_NONBLOCK: i32 = 1 << 11;

const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const ORDINARY_MODE_MASK: u32 = 0o777;

/// Retain whether a write created the final name before its last sync failed.
#[derive(Debug)]
pub(crate) struct WriteError {
    pub(crate) source: io::Error,
    pub(crate) final_created: bool,
    pub(crate) temporary_cleanup_failed: bool,
}

/// Require a real owner-private directory at the configured literal path.
pub(crate) fn require_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let canonical = fs::canonicalize(path)?;
    // preserve the mounted archive's existing private directory contract
    if !metadata.is_dir()
        || metadata.mode() & ORDINARY_MODE_MASK != PRIVATE_DIR_MODE
        || canonical != path
    {
        return Err(invalid(
            "revision spool directory is not owner-private and canonical",
        ));
    }
    Ok(())
}

/// Create only the specified fixed child without repairing an existing path.
pub(crate) fn ensure_child_directory(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(
        // require a fixed child rather than a new arbitrary root
        || invalid("revision spool child has no parent"),
    )?;
    // retain the original canonical parent requirement
    if fs::canonicalize(parent)? != parent {
        return Err(invalid("revision spool parent is not canonical"));
    }
    let mut builder = DirBuilder::new();
    match builder.mode(PRIVATE_DIR_MODE).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    require_directory(path)
}

/// Sync a directory descriptor rather than following a replacement final component.
pub(crate) fn sync_directory(path: &Path) -> io::Result<()> {
    OpenOptions::new()
        .read(true)
        .custom_flags(O_DIRECTORY | O_NOFOLLOW)
        .open(path)?
        .sync_all()
}

/// Read one exact regular file with a hard bound even if its size grows while open.
pub(crate) fn read_private_file(path: &Path, maximum: usize) -> io::Result<Vec<u8>> {
    let (file, size) = open_private_file(path, maximum)?;
    let limit = (maximum as u64).saturating_add(1);
    let mut bytes = Vec::with_capacity(size);
    file.take(limit).read_to_end(&mut bytes)?;
    // never pass a body beyond its source-owned bound to the parser
    if bytes.len() > maximum {
        return Err(invalid("revision spool file exceeds its read bound"));
    }
    Ok(bytes)
}

/// Treat only an actual missing name as absence.
pub(crate) fn read_optional_file(path: &Path, maximum: usize) -> io::Result<Option<Vec<u8>>> {
    match read_private_file(path, maximum) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Read checked length from the validated open descriptor without copying the body.
pub(crate) fn private_file_length(path: &Path, maximum: usize) -> io::Result<usize> {
    open_private_file(path, maximum).map(
        // discard the validated descriptor after consuming only its length
        |(_, size)| size,
    )
}

/// Retain the file age of an incomplete crash-left lease.
pub(crate) fn lease_metadata(path: &Path) -> io::Result<Metadata> {
    fs::symlink_metadata(path)
}

/// Create one new direct file and synchronize its contents.
pub(crate) fn create_synced_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = open_new_private_file(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Publish one complete file through a synced temporary and exclusive hard link.
pub(crate) fn publish_immutable(
    target: &Path,
    temporary: &Path,
    bytes: &[u8],
) -> Result<(), WriteError> {
    let mut file = open_new_private_file(temporary).map_err(
        // do not remove a temporary that this invocation did not create
        |source| WriteError {
            source,
            final_created: false,
            temporary_cleanup_failed: false,
        },
    )?;
    let mut final_created = false;
    let mut temporary_exists = true;
    let result = (|| {
        // make all data durable before the public immutable name can appear
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::hard_link(temporary, target)?;
        final_created = true;
        fs::remove_file(temporary)?;
        temporary_exists = false;
        let directory = target.parent().ok_or_else(
            // require the source-owned parent for its final durability step
            || invalid("revision spool member has no parent"),
        )?;
        sync_directory(directory)
    })();
    result.map_err(
        // preserve whether reconciliation may find the final immutable name
        |source| {
            let temporary_cleanup_failed = temporary_exists
                && fs::remove_file(temporary).is_err_and(
                    // retain cleanup failure except on an already-removed owned temporary
                    |error| error.kind() != io::ErrorKind::NotFound,
                );
            WriteError {
                source,
                final_created,
                temporary_cleanup_failed,
            }
        },
    )
}

/// Enumerate file basenames only, optionally stopping at the ordinary census ceiling.
pub(crate) fn list_names(path: &Path, cap: Option<usize>) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    // preserve the native enumeration order without sorting the shared spool
    for entry in fs::read_dir(path)?.take(cap.unwrap_or(usize::MAX)) {
        let name = entry?.file_name().into_string().map_err(
            // the archived basename grammars are all plain ascii
            |_| invalid("revision spool member name is not UTF-8"),
        )?;
        names.push(name);
    }
    Ok(names)
}

/// Read one private regular file through a final-component nofollow descriptor.
fn open_private_file(path: &Path, maximum: usize) -> io::Result<(File, usize)> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    // match mode and regular-file shape on the descriptor used for this read
    if !metadata.is_file()
        || metadata.mode() & ORDINARY_MODE_MASK != PRIVATE_FILE_MODE
        || metadata.len() > maximum as u64
    {
        return Err(invalid(
            "revision spool private file does not match its bound",
        ));
    }
    Ok((file, metadata.len() as usize))
}

/// Use exclusive creation with the source's private file mode.
fn open_new_private_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(PRIVATE_FILE_MODE)
        .custom_flags(O_NOFOLLOW)
        .open(path)
}

/// Contain invalid filesystem shape at the private archive boundary.
pub(crate) fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
