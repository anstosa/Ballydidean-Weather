//! Native controller process lock and one complete source-ordered custody cycle.

use std::{
    fs::{self, DirBuilder, File, Metadata, OpenOptions},
    future::Future,
    io,
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    pin::Pin,
};

use crate::NativeMaintenanceCaptureCycleLockPort;

const STATE_SUFFIX: &[&str] = &[".weather", "adjustment-maintenance", "v2", "state"];
const ARCHIVE_SUFFIX: &[&str] = &[
    ".weather",
    "adjustment-maintenance",
    "v2",
    "archive-primary",
    "objects",
];

/// Fixed private maintenance directory destinations shared with the predecessor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenancePrivateDirectory {
    State,
    Archive,
}

/// Create only the original owner-private child chain without changing existing modes.
pub fn ensure_native_maintenance_private_directory(
    kind: NativeMaintenancePrivateDirectory,
) -> io::Result<PathBuf> {
    ensure_private_directory(Path::new("/home/ubuntu"), kind)
}

/// Retain the same nonblocking kernel file lock for the complete operation.
pub struct NativeMaintenanceControllerProcessLock {
    _held: File,
}

impl NativeMaintenanceControllerProcessLock {
    /// Acquire the installed controller lock using its fixed source home and file.
    pub fn acquire() -> io::Result<Self> {
        Self::acquire_in_home(Path::new("/home/ubuntu"))
    }

    /// Open one exact validated state directory and acquire the same flock on its original inode.
    fn acquire_in_home(home: &Path) -> io::Result<Self> {
        let state = ensure_private_directory(home, NativeMaintenancePrivateDirectory::State)?;
        let path = state.join("maintenance-controller.lock");
        let held = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        let metadata = held.metadata()?;
        let current = fs::symlink_metadata(&path)?;
        // retain source regular-file, uid, link-count, permission and inode admission
        if !metadata.is_file()
            || metadata.uid() != current_uid()
            || metadata.nlink() != 1
            || metadata.mode() & 0o777 != 0o600
            || !same_inode(&metadata, &current)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "maintenance controller lock is invalid",
            ));
        }
        // safety: the held file descriptor remains owned until this guard is dropped
        let status = unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        // do not queue concurrent controller or custody operations
        if status != 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::WouldBlock {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "maintenance controller lock is occupied",
                ));
            }
            return Err(error);
        }
        Ok(Self { _held: held })
    }
}

/// Exact independent state/archive/lifecycle owners required before one custody operation.
pub trait NativeMaintenanceCaptureCyclePorts {
    type Error;
    /// Initialize the actual maintenance journal under the controller process lock.
    fn initialize_journal(&mut self) -> Result<(), Self::Error>;
    /// Initialize the actual local archive under the same controller process lock.
    fn initialize_archive(&mut self) -> Result<(), Self::Error>;
    /// Initialize the genuine capture epoch and registration lifecycle.
    fn initialize_lifecycle(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>>;
    /// Sample the actual canonical cycle cutoff after lifecycle initialization.
    fn capture_cutoff(&mut self) -> Result<String, Self::Error>;
    /// Synchronize one complete original custody cycle, never a placeholder success.
    fn synchronize_evidence(
        &mut self,
        cutoff_at: String,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>>;
}

/// Bind the real native lock to the independently supplied complete capture ports.
pub struct NativeMaintenanceLockedCaptureCycle<Ports> {
    ports: Ports,
}

#[derive(Debug)]
pub enum NativeMaintenanceLockedCaptureCycleError<Error> {
    Lock(io::Error),
    Cycle(Error),
}

impl<Error: std::fmt::Display> std::fmt::Display
    for NativeMaintenanceLockedCaptureCycleError<Error>
{
    /// Retain the owning lock or source cycle diagnostic.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lock(error) => std::fmt::Display::fmt(error, formatter),
            Self::Cycle(error) => std::fmt::Display::fmt(error, formatter),
        }
    }
}

impl<Error: std::error::Error + 'static> std::error::Error
    for NativeMaintenanceLockedCaptureCycleError<Error>
{
    /// Preserve the underlying actual owner error.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lock(error) => Some(error),
            Self::Cycle(error) => Some(error),
        }
    }
}

impl<Ports> NativeMaintenanceLockedCaptureCycle<Ports> {
    /// Retain mandatory actual archive and journal ports without starting a job.
    pub fn new(ports: Ports) -> Self {
        Self { ports }
    }
}

impl<Ports: NativeMaintenanceCaptureCyclePorts> NativeMaintenanceCaptureCycleLockPort
    for NativeMaintenanceLockedCaptureCycle<Ports>
{
    type Error = NativeMaintenanceLockedCaptureCycleError<Ports::Error>;

    /// Hold the original exclusive controller lock across every step of the complete cycle.
    fn run_complete_capture_cycle_with_process_lock(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>> {
        Box::pin(async {
            let _lock = NativeMaintenanceControllerProcessLock::acquire()
                .map_err(NativeMaintenanceLockedCaptureCycleError::Lock)?;
            run_locked_cycle(&mut self.ports)
                .await
                .map_err(NativeMaintenanceLockedCaptureCycleError::Cycle)
        })
    }
}

/// Execute the predecessor's exact lifecycle sequence under the already owned lock.
async fn run_locked_cycle<Ports: NativeMaintenanceCaptureCyclePorts>(
    ports: &mut Ports,
) -> Result<(), Ports::Error> {
    ports.initialize_journal()?;
    ports.initialize_archive()?;
    ports.initialize_lifecycle().await?;
    let cutoff = ports.capture_cutoff()?;
    ports.synchronize_evidence(cutoff).await
}

/// Hold each admitted parent inode while creating the fixed child directory.
fn ensure_private_directory(
    home: &Path,
    kind: NativeMaintenancePrivateDirectory,
) -> io::Result<PathBuf> {
    let root = fs::symlink_metadata(home)?;
    // require the literal current owner's non-writable home before any directory creation
    if !root.is_dir()
        || root.uid() != current_uid()
        || root.mode() & 0o022 != 0
        || home.canonicalize()?.as_os_str() != home.as_os_str()
    {
        return Err(invalid_directory());
    }
    let suffix = match kind {
        NativeMaintenancePrivateDirectory::State => STATE_SUFFIX,
        NativeMaintenancePrivateDirectory::Archive => ARCHIVE_SUFFIX,
    };
    let mut path = home.to_path_buf();
    // create or admit only each frozen literal private child
    for segment in suffix {
        let held = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(&path)?;
        let opened = held.metadata()?;
        // bind the held parent to the path before the prospective creation
        if !same_inode(&opened, &fs::symlink_metadata(&path)?)
            || path.canonicalize()?.as_os_str() != path.as_os_str()
        {
            return Err(invalid_directory());
        }
        let relative = PathBuf::from(format!("/proc/self/fd/{}", held.as_raw_fd())).join(segment);
        match DirBuilder::new().mode(0o700).create(&relative) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let child = path.join(segment);
        let metadata = fs::symlink_metadata(&child)?;
        // preserve the original exact private child permissions and current parent identity
        if !metadata.is_dir()
            || metadata.uid() != current_uid()
            || metadata.mode() & 0o777 != 0o700
            || child.canonicalize()?.as_os_str() != child.as_os_str()
            || !same_inode(&opened, &fs::symlink_metadata(&path)?)
        {
            return Err(invalid_directory());
        }
        held.sync_all()?;
        path = child;
    }
    Ok(path)
}

/// Return the process's real owner identity used by the predecessor.
fn current_uid() -> u32 {
    // safety: getuid takes no pointer or process mutation argument
    unsafe { libc::getuid() }
}

/// Compare two observed filesystem identities without opening any content.
fn same_inode(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// Keep directory admission failures independent of retained private paths.
fn invalid_directory() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "adjustment private directory refused",
    )
}

#[cfg(test)]
#[path = "native_maintenance_controller_lock_test.rs"]
mod tests;
