//! Create and reread the original authority files without replacing a prior epoch.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::capture_epoch_operator::{
    AUTHORITY_DIR, SNAPSHOT_FILE, WITNESS_FILE, read_operator_file,
};
use crate::{
    CAPTURE_EPOCH_WITNESS_MAX_BYTES, CaptureEpochDeploymentAuthority, CaptureEpochInitDeployment,
    CaptureEpochInitError, CaptureEpochInitPort, CaptureEpochOperatorFile,
    resolve_capture_epoch_deployment,
};

/// File owner selected by the operator, never by a command operand.
pub struct CaptureEpochAuthorityFiles {
    directory: PathBuf,
    uid: u32,
    gid: u32,
}

impl CaptureEpochAuthorityFiles {
    /// Select the original installed root-owned state path without opening a file.
    pub fn native() -> Self {
        Self::for_directory(PathBuf::from(AUTHORITY_DIR), 0, 0)
    }

    /// Bind an already selected directory and expected owner for an operator or fixture.
    pub fn for_directory(directory: PathBuf, uid: u32, gid: u32) -> Self {
        Self {
            directory,
            uid,
            gid,
        }
    }

    /// Retain ordinary absence separately from malformed authority contents.
    pub fn read_optional(
        &self,
        artifact: CaptureEpochOperatorFile,
    ) -> Result<Option<Vec<u8>>, CaptureEpochInitError> {
        let path = self.path(artifact);
        // treat only an absent entry as optional
        match fs::symlink_metadata(&path) {
            Ok(_) => read_operator_file(&path, self.uid, self.gid)
                .map(Some)
                .map_err(|_| CaptureEpochInitError::File),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(_) => Err(CaptureEpochInitError::File),
        }
    }

    /// Create a fixed authority file exclusively and sync its parent even on a racing winner.
    pub fn create_once(
        &self,
        artifact: CaptureEpochOperatorFile,
        bytes: &[u8],
    ) -> Result<(), CaptureEpochInitError> {
        use CaptureEpochInitError::File as Failure;
        // the original command never publishes an oversized authority document
        if !(2..=CAPTURE_EPOCH_WITNESS_MAX_BYTES).contains(&bytes.len())
            || fs::canonicalize(&self.directory).map_err(|_| Failure)? != self.directory
            || !fs::symlink_metadata(&self.directory)
                .map_err(|_| Failure)?
                .is_dir()
        {
            return Err(Failure);
        }
        let path = self.path(artifact);
        // exclusive create never follows or replaces an existing entry
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(path)
        {
            Ok(mut file) => {
                file.write_all(bytes).map_err(|_| Failure)?;
                file.set_permissions(fs::Permissions::from_mode(0o644))
                    .map_err(|_| Failure)?;
                file.sync_all().map_err(|_| Failure)?;
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err(Failure),
        }
        sync_directory(&self.directory)
    }

    // map the original fixed file names without permitting an operand path
    fn path(&self, artifact: CaptureEpochOperatorFile) -> PathBuf {
        self.directory.join(match artifact {
            CaptureEpochOperatorFile::Witness => WITNESS_FILE,
            CaptureEpochOperatorFile::Snapshot => SNAPSHOT_FILE,
        })
    }
}

// sync the retained parent after either successful creation or ordinary racing existence
fn sync_directory(directory: &Path) -> Result<(), CaptureEpochInitError> {
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| CaptureEpochInitError::File)
}

/// The operator's existing effective authority and service-owned directory initialization.
pub trait CaptureEpochInitializationHostAuthority: CaptureEpochDeploymentAuthority {
    /// Verify the original root-only effective process identity.
    fn require_root(&mut self) -> Result<(), CaptureEpochInitError>;

    /// Initialize only the original six service-owned cold archive children.
    fn initialize_cold_archive(&mut self) -> Result<(), CaptureEpochInitError>;
}

/// Compose target-proof authority and actual filesystem persistence for the init command.
pub struct CaptureEpochInitFilePort<Authority> {
    authority: Authority,
    files: CaptureEpochAuthorityFiles,
}

impl<Authority> CaptureEpochInitFilePort<Authority> {
    /// Bind the selected host authority and file namespace without doing I/O.
    pub const fn new(authority: Authority, files: CaptureEpochAuthorityFiles) -> Self {
        Self { authority, files }
    }
}

impl<Authority: CaptureEpochInitializationHostAuthority> CaptureEpochInitPort
    for CaptureEpochInitFilePort<Authority>
{
    /// Retain the original process identity owner.
    fn require_root(&mut self) -> Result<(), CaptureEpochInitError> {
        self.authority.require_root()
    }

    /// Derive the target from the original immutable environment and release proof.
    fn resolve_deployment(
        &mut self,
        target: &str,
    ) -> Result<CaptureEpochInitDeployment, CaptureEpochInitError> {
        resolve_capture_epoch_deployment(target, &mut self.authority)
    }

    /// Delegate the fixed cold archive preparation to the same host authority.
    fn initialize_cold_archive(&mut self) -> Result<(), CaptureEpochInitError> {
        self.authority.initialize_cold_archive()
    }

    /// Read the admitted immutable file from its selected namespace.
    fn read_optional(
        &mut self,
        artifact: CaptureEpochOperatorFile,
    ) -> Result<Option<Vec<u8>>, CaptureEpochInitError> {
        self.files.read_optional(artifact)
    }

    /// Persist through the actual exclusive authority writer.
    fn create_once(
        &mut self,
        artifact: CaptureEpochOperatorFile,
        bytes: &[u8],
    ) -> Result<(), CaptureEpochInitError> {
        self.files.create_once(artifact, bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;

    use super::*;

    struct FixtureDirectory(PathBuf);

    impl Drop for FixtureDirectory {
        // remove only the successfully created disposable directory
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Exercise actual exclusive file creation, readback and retry under current fixture ownership.
    #[test]
    fn authority_files_retain_the_first_bytes() {
        let root = std::env::temp_dir().join(format!(
            "weather-capture-epoch-files-{}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create unique fixture directory");
        let root = FixtureDirectory(root);
        let directory = fs::canonicalize(&root.0).expect("canonical fixture directory");
        let metadata = fs::metadata(&directory).expect("fixture owner");
        let store =
            CaptureEpochAuthorityFiles::for_directory(directory, metadata.uid(), metadata.gid());
        let artifact = CaptureEpochOperatorFile::Snapshot;
        assert_eq!(store.read_optional(artifact).unwrap(), None);
        store.create_once(artifact, b"{\"first\":true}\n").unwrap();
        store.create_once(artifact, b"{\"first\":false}\n").unwrap();
        assert_eq!(
            store.read_optional(artifact).unwrap().as_deref(),
            Some(&b"{\"first\":true}\n"[..])
        );
        assert_eq!(
            store
                .read_optional(CaptureEpochOperatorFile::Witness)
                .unwrap(),
            None
        );
    }
}
