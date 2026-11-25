//! Read-only native inspection of the fixed maintenance archive envelope.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, Metadata, OpenOptions};
use std::io::{ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::native_maintenance_backing_capacity::NativeMaintenanceCapacityEvidence;

const ARCHIVE_MAXIMUM_BYTES: u64 = 18 * 1_024 * 1_024 * 1_024;
const TOTAL_STORAGE_CEILING_BYTES: u64 = 64 * 1_024 * 1_024 * 1_024;
const FREE_SPACE_FLOOR_BYTES: u64 = 16 * 1_024 * 1_024 * 1_024;
const MAXIMUM_TASK_INODES: u64 = 17_899;
const MINIMUM_FREE_INODES: u64 = 32_768;

/// Value-free path, layout, history, arithmetic, and capacity failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceLocalArchiveEnvelopeError {
    Path,
    Layout,
    History,
    Capacity,
    Overflow,
}

impl Display for NativeMaintenanceLocalArchiveEnvelopeError {
    /// Render one bounded archive inspection failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Path => "maintenance archive envelope path is invalid",
            Self::Layout => "maintenance archive envelope layout is invalid",
            Self::History => "maintenance archive history is incomplete",
            Self::Capacity => "maintenance archive genesis capacity is refused",
            Self::Overflow => "maintenance archive envelope accounting overflowed",
        })
    }
}

impl std::error::Error for NativeMaintenanceLocalArchiveEnvelopeError {}

/// Exact current and prospective allocation and inode envelope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceLocalArchiveEnvelope {
    pub allocated_bytes: u64,
    pub file_count: u64,
    pub initialization_allocated_bytes: u64,
    pub initialization_atomic_allocated_bytes: u64,
    pub initialization_atomic_inodes: u64,
    pub initialization_directories: u64,
    pub initialization_inodes: u64,
    pub initialization_lock_files: u64,
    pub incoming_count: u64,
    pub object_count: u64,
    pub task_inodes: u64,
}

struct ProspectiveArchiveChain {
    archive_missing: u64,
    details: HashMap<PathBuf, Metadata>,
    missing: u64,
    state_root: PathBuf,
}

struct StateGenesis {
    allocated_bytes: u64,
    inodes: u64,
    missing_lock_files: u64,
}

/// Inspect the exact fixed archive layout beneath one admitted literal home root.
pub fn inspect_native_maintenance_local_archive_envelope(
    home_root: &Path,
    capacity: &NativeMaintenanceCapacityEvidence,
) -> Result<NativeMaintenanceLocalArchiveEnvelope, NativeMaintenanceLocalArchiveEnvelopeError> {
    use NativeMaintenanceLocalArchiveEnvelopeError as Error;

    let uid = current_uid();
    let archive_root = home_root
        .join(".weather")
        .join("adjustment-maintenance")
        .join("v2")
        .join("archive-primary");
    let chain = inspect_prospective_archive_chain(home_root, uid)?;
    let object_root = archive_root.join("objects");
    let root_details = chain.details.get(&archive_root);
    let object_root_details = chain.details.get(&object_root);
    // refuse nonempty interrupted initialization before inspecting history
    if root_details.is_some()
        && object_root_details.is_none()
        && !read_private_directory(&archive_root, uid)?.is_empty()
    {
        return Err(Error::Layout);
    }
    let mut allocated_bytes = 0_u64;
    let mut incoming_count = 0_u64;
    let mut object_count = 0_u64;
    let mut object_hashes = HashSet::new();
    // charge both existing archive directory allocations exactly once
    for details in [root_details, object_root_details].into_iter().flatten() {
        allocated_bytes = checked_add(allocated_bytes, allocated(details)?)?;
    }
    // inspect complete object storage only after both directories exist
    if object_root_details.is_some() {
        assert_closed_archive_root(&archive_root, uid)?;
        // classify every literal object entry without opening its payload
        for name in read_private_directory(&object_root, uid)? {
            let name = name.to_str().ok_or(Error::Layout)?;
            let path = object_root.join(name);
            let details = fs::symlink_metadata(path).map_err(|_| Error::Layout)?;
            // reject special, linked, foreign, or broad archive entries
            if !is_private_file(&details, uid) {
                return Err(Error::Layout);
            }
            // retain only the two closed source filename classes
            if let Some(hash) = object_hash(name) {
                object_count = checked_add(object_count, 1)?;
                object_hashes.insert(hash.to_owned());
            } else if valid_incoming_name(name) {
                incoming_count = checked_add(incoming_count, 1)?;
            } else {
                return Err(Error::Layout);
            }
            allocated_bytes = checked_add(allocated_bytes, allocated(&details)?)?;
        }
    }
    let mut pointer_count = 0_u64;
    let mut pointers = HashMap::new();
    // count only existing exact current and previous pointer slots
    for name in ["head.current", "head.previous"] {
        let path = archive_root.join(name);
        // preserve only genuine pointer absence as genesis
        if let Some((hash, blocks)) = read_private_pointer(&path, uid)? {
            pointer_count = checked_add(pointer_count, 1)?;
            allocated_bytes = checked_add(allocated_bytes, blocks)?;
            pointers.insert(name, hash);
        }
    }
    let current_head = pointers.get("head.current");
    let previous_head = pointers.get("head.previous");
    // reject incomplete publication, incoming writes, and broken pointer history
    if incoming_count != 0
        || (previous_head.is_some() && current_head.is_none())
        || (object_count != 0 && current_head.is_none())
        || current_head.is_some_and(|hash| !object_hashes.contains(hash))
        || previous_head.is_some_and(|hash| !object_hashes.contains(hash))
    {
        return Err(Error::History);
    }
    // require one genuine positive ext4 allocation unit from typed capacity evidence
    if capacity.ext4_block_size == 0 {
        return Err(Error::Capacity);
    }
    let state_genesis = inspect_prospective_state_genesis(
        &chain.state_root,
        chain.details.get(&chain.state_root),
        capacity.ext4_block_size,
        uid,
    )?;
    let initialization_directories = chain.missing;
    let initialization_allocated_bytes = checked_add(
        checked_mul(initialization_directories, capacity.ext4_block_size)?,
        state_genesis.allocated_bytes,
    )?;
    let initialization_inodes = checked_add(initialization_directories, state_genesis.inodes)?;
    allocated_bytes = checked_add(
        allocated_bytes,
        checked_mul(chain.archive_missing, capacity.ext4_block_size)?,
    )?;
    let task_root = home_root
        .join(".weather")
        .join("adjustment-maintenance")
        .join("v2");
    let task_inodes = checked_add(
        count_private_task_inodes_if_present(&task_root, uid)?,
        initialization_inodes,
    )?;
    assert_prospective_capacity(
        capacity,
        allocated_bytes,
        initialization_allocated_bytes,
        initialization_inodes,
        task_inodes,
    )?;
    Ok(NativeMaintenanceLocalArchiveEnvelope {
        allocated_bytes,
        file_count: checked_add(checked_add(object_count, incoming_count)?, pointer_count)?,
        initialization_allocated_bytes,
        initialization_atomic_allocated_bytes: state_genesis.allocated_bytes,
        initialization_atomic_inodes: 1,
        initialization_directories,
        initialization_inodes,
        initialization_lock_files: state_genesis.missing_lock_files,
        incoming_count,
        object_count,
        task_inodes,
    })
}

/// Prove every existing ancestor and count one contiguous prospective suffix.
fn inspect_prospective_archive_chain(
    home_root: &Path,
    uid: u32,
) -> Result<ProspectiveArchiveChain, NativeMaintenanceLocalArchiveEnvelopeError> {
    use NativeMaintenanceLocalArchiveEnvelopeError as Error;

    let home_details = fs::symlink_metadata(home_root).map_err(|_| Error::Path)?;
    // retain the source home anchor, ownership, permission, and literal path checks
    if !home_details.is_dir()
        || home_details.file_type().is_symlink()
        || home_details.uid() != uid
        || home_details.permissions().mode() & 0o022 != 0
        || fs::canonicalize(home_root).ok().as_deref() != Some(home_root)
        || !lexically_absolute(home_root)
    {
        return Err(Error::Path);
    }
    let mut details = HashMap::new();
    let mut missing = 0_u64;
    let mut path = home_root.to_path_buf();
    let mut parent_missing = false;
    // inspect the exact fixed archive initialization suffix without following links
    for segment in [
        ".weather",
        "adjustment-maintenance",
        "v2",
        "archive-primary",
        "objects",
    ] {
        path.push(segment);
        // every descendant of an absent parent remains prospectively absent
        if parent_missing {
            missing = checked_add(missing, 1)?;
            continue;
        }
        // begin one contiguous absent suffix only at a missing literal child
        let current = match fs::symlink_metadata(&path) {
            Ok(current) => current,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                parent_missing = true;
                missing = checked_add(missing, 1)?;
                continue;
            }
            Err(_) => return Err(Error::Layout),
        };
        // require owner-private, same-filesystem, literal existing ancestors
        if !is_private_directory(&current, uid)
            || current.dev() != home_details.dev()
            || fs::canonicalize(&path).ok().as_deref() != Some(path.as_path())
        {
            return Err(Error::Layout);
        }
        details.insert(path.clone(), current);
    }
    let archive_missing = missing;
    let task_root = home_root
        .join(".weather")
        .join("adjustment-maintenance")
        .join("v2");
    let state_root = task_root.join("state");
    // inspect the sibling state root only when its literal parent exists
    if details.contains_key(&task_root) {
        // admit only one prospectively absent state directory
        match fs::symlink_metadata(&state_root) {
            Ok(state) => {
                if !is_private_directory(&state, uid)
                    || state.dev() != home_details.dev()
                    || fs::canonicalize(&state_root).ok().as_deref() != Some(state_root.as_path())
                {
                    return Err(Error::Layout);
                }
                details.insert(state_root.clone(), state);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                missing = checked_add(missing, 1)?;
            }
            Err(_) => return Err(Error::Layout),
        }
    } else {
        missing = checked_add(missing, 1)?;
    }
    Ok(ProspectiveArchiveChain {
        archive_missing,
        details,
        missing,
        state_root,
    })
}

/// Inspect deterministic state files and charge the atomic journal-head peak.
fn inspect_prospective_state_genesis(
    state_root: &Path,
    state_details: Option<&Metadata>,
    block_size: u64,
    uid: u32,
) -> Result<StateGenesis, NativeMaintenanceLocalArchiveEnvelopeError> {
    use NativeMaintenanceLocalArchiveEnvelopeError as Error;

    let mut missing_lock_files = 0_u64;
    // inspect both persistent empty lock files
    for name in ["archive-job.lock", "journal.lock"] {
        // every file beneath an absent state root is prospectively absent
        if state_details.is_none() {
            missing_lock_files = checked_add(missing_lock_files, 1)?;
            continue;
        }
        // admit only an absent deterministic lock file
        match fs::symlink_metadata(state_root.join(name)) {
            Ok(details) if is_private_file(&details, uid) && details.size() == 0 => {}
            Ok(_) => return Err(Error::Layout),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                missing_lock_files = checked_add(missing_lock_files, 1)?;
            }
            Err(_) => return Err(Error::Layout),
        }
    }
    // validate an existing head before charging its atomic replacement
    if state_details.is_some() {
        // an absent genesis head is created by the same atomic temporary inode
        match fs::symlink_metadata(state_root.join("head.current")) {
            Ok(details) if is_private_file(&details, uid) => {}
            Ok(_) => return Err(Error::Layout),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(Error::Layout),
        }
    }
    Ok(StateGenesis {
        allocated_bytes: block_size,
        inodes: checked_add(missing_lock_files, 1)?,
        missing_lock_files,
    })
}

/// Reject names outside the complete archive-root grammar.
fn assert_closed_archive_root(
    archive_root: &Path,
    uid: u32,
) -> Result<(), NativeMaintenanceLocalArchiveEnvelopeError> {
    use NativeMaintenanceLocalArchiveEnvelopeError::Layout;

    // retain only object storage and the two pointer slots
    for name in read_private_directory(archive_root, uid)? {
        if !matches!(
            name.to_str(),
            Some("objects" | "head.current" | "head.previous")
        ) {
            return Err(Layout);
        }
    }
    Ok(())
}

/// Hold one private directory inode while collecting its literal entries.
fn read_private_directory(
    path: &Path,
    uid: u32,
) -> Result<Vec<OsString>, NativeMaintenanceLocalArchiveEnvelopeError> {
    use NativeMaintenanceLocalArchiveEnvelopeError::Layout;

    let held = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(path)
        .map_err(|_| Layout)?;
    let held_details = held.metadata().map_err(|_| Layout)?;
    let before = fs::symlink_metadata(path).map_err(|_| Layout)?;
    // bind the literal path to one held owner-private directory
    if !is_private_directory(&held_details, uid)
        || held_details.dev() != before.dev()
        || held_details.ino() != before.ino()
    {
        return Err(Layout);
    }
    let descriptor = PathBuf::from(format!("/proc/self/fd/{}", held.as_raw_fd()));
    let mut names = Vec::new();
    // retain every literal entry name while the original descriptor stays open
    for entry in fs::read_dir(descriptor).map_err(|_| Layout)? {
        names.push(entry.map_err(|_| Layout)?.file_name());
    }
    let after = fs::symlink_metadata(path).map_err(|_| Layout)?;
    // reject replacement while or after collecting the directory population
    if held_details.dev() != after.dev() || held_details.ino() != after.ino() {
        return Err(Layout);
    }
    Ok(names)
}

/// Read one exact private pointer through a held no-follow descriptor.
fn read_private_pointer(
    path: &Path,
    uid: u32,
) -> Result<Option<(String, u64)>, NativeMaintenanceLocalArchiveEnvelopeError> {
    use NativeMaintenanceLocalArchiveEnvelopeError::Layout;

    let mut file = match open_read_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Layout),
    };
    let details = file.metadata().map_err(|_| Layout)?;
    let before = fs::symlink_metadata(path).map_err(|_| Layout)?;
    // bind one exact-size private pointer path to its held descriptor
    if !is_private_file(&details, uid)
        || details.size() != 65
        || before.dev() != details.dev()
        || before.ino() != details.ino()
    {
        return Err(Layout);
    }
    let mut bytes = Vec::with_capacity(66);
    (&mut file)
        .take(66)
        .read_to_end(&mut bytes)
        .map_err(|_| Layout)?;
    let after = fs::symlink_metadata(path).map_err(|_| Layout)?;
    // reject replacement, resizing, or alternate pointer spelling
    if after.dev() != details.dev()
        || after.ino() != details.ino()
        || bytes.len() != 65
        || bytes.last() != Some(&b'\n')
        || !bytes[..64]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(Layout);
    }
    let hash = String::from_utf8(bytes[..64].to_vec()).map_err(|_| Layout)?;
    Ok(Some((hash, allocated(&details)?)))
}

/// Count one existing private task tree or one wholly absent genesis tree.
fn count_private_task_inodes_if_present(
    root: &Path,
    uid: u32,
) -> Result<u64, NativeMaintenanceLocalArchiveEnvelopeError> {
    use NativeMaintenanceLocalArchiveEnvelopeError as Error;

    // accept only a wholly absent task root
    match fs::symlink_metadata(root) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
        Err(_) => return Err(Error::Layout),
    }
    let mut pending = vec![root.to_path_buf()];
    let mut count = 0_u64;
    // walk the fixed task root only through the global finite ceiling
    while let Some(path) = pending.pop() {
        let details = fs::symlink_metadata(&path).map_err(|_| Error::Layout)?;
        // reject links, foreign owners, broad modes, and special files
        if details.uid() != uid
            || details.file_type().is_symlink()
            || details.permissions().mode() & 0o077 != 0
            || !(details.is_dir() || details.is_file())
        {
            return Err(Error::Layout);
        }
        count = checked_add(count, 1)?;
        // stop the same unexpectedly oversized tree before further traversal
        if count > MAXIMUM_TASK_INODES {
            return Ok(count);
        }
        // descend only through held private literal directories
        if details.is_dir() {
            // retain every literal child for bounded later inspection
            for name in read_private_directory(&path, uid)? {
                pending.push(path.join(name));
            }
        }
    }
    Ok(count)
}

/// Preserve every archive, aggregate, dual-free-space, and inode bound.
fn assert_prospective_capacity(
    capacity: &NativeMaintenanceCapacityEvidence,
    allocated_bytes: u64,
    initialization_allocated_bytes: u64,
    initialization_inodes: u64,
    task_inodes: u64,
) -> Result<(), NativeMaintenanceLocalArchiveEnvelopeError> {
    use NativeMaintenanceLocalArchiveEnvelopeError as Error;

    let next_free_bytes = checked_add(FREE_SPACE_FLOOR_BYTES, initialization_allocated_bytes)?;
    let next_free_inodes = checked_add(MINIMUM_FREE_INODES, initialization_inodes)?;
    let next_backing_allocated = checked_add(
        capacity.backing.allocated_bytes,
        initialization_allocated_bytes,
    )?;
    // refuse any next state beyond either storage layer or finite inode ceiling
    if allocated_bytes > ARCHIVE_MAXIMUM_BYTES
        || task_inodes > MAXIMUM_TASK_INODES
        || capacity.ext4_free_bytes < next_free_bytes
        || capacity.backing.free_bytes < next_free_bytes
        || capacity.ext4_free_inodes < next_free_inodes
        || capacity.backing.free_inodes < next_free_inodes
        || next_backing_allocated > TOTAL_STORAGE_CEILING_BYTES
    {
        return Err(Error::Capacity);
    }
    Ok(())
}

/// Recognize one exact committed object filename and borrow its digest.
fn object_hash(name: &str) -> Option<&str> {
    let hash = name.strip_prefix("sha256-")?.strip_suffix(".obj")?;
    (hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(hash)
}

/// Recognize one exact incoming object filename class.
fn valid_incoming_name(name: &str) -> bool {
    name.strip_prefix(".incoming-").is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

/// Classify one exact owner-private archive directory.
fn is_private_directory(details: &Metadata, uid: u32) -> bool {
    details.is_dir()
        && !details.file_type().is_symlink()
        && details.uid() == uid
        && details.permissions().mode() & 0o777 == 0o700
}

/// Classify one exact owner-private single-link archive file.
fn is_private_file(details: &Metadata, uid: u32) -> bool {
    details.is_file()
        && !details.file_type().is_symlink()
        && details.uid() == uid
        && details.permissions().mode() & 0o777 == 0o600
        && details.nlink() == 1
}

/// Require one lexical absolute path without parent traversal components.
fn lexically_absolute(path: &Path) -> bool {
    let normalized = path.components().collect::<PathBuf>();
    path.is_absolute()
        && normalized.as_os_str() == path.as_os_str()
        && !path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
}

/// Convert stat blocks into the source's fixed 512-byte allocation unit.
fn allocated(details: &Metadata) -> Result<u64, NativeMaintenanceLocalArchiveEnvelopeError> {
    checked_mul(details.blocks(), 512)
}

/// Add two bounded allocation or count values without wrapping.
fn checked_add(left: u64, right: u64) -> Result<u64, NativeMaintenanceLocalArchiveEnvelopeError> {
    left.checked_add(right)
        .ok_or(NativeMaintenanceLocalArchiveEnvelopeError::Overflow)
}

/// Multiply two bounded allocation or count values without wrapping.
fn checked_mul(left: u64, right: u64) -> Result<u64, NativeMaintenanceLocalArchiveEnvelopeError> {
    left.checked_mul(right)
        .ok_or(NativeMaintenanceLocalArchiveEnvelopeError::Overflow)
}

/// Read the same real process identity used by the predecessor owner checks.
fn current_uid() -> u32 {
    // libc exposes the process real uid without any host mutation
    unsafe { libc::getuid() }
}
