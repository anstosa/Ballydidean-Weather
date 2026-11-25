//! Publish one original rain performance report below the two private storage roots.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

/// Two-phase private result publication used by the rain performance command.
pub trait RainPerformanceResultWriter {
    type Output;

    /// Admit the requested path before the command serializes its report.
    fn admit_result_path(&mut self, raw: &str) -> io::Result<Self::Output>;

    /// Write already serialized bytes without replacing an earlier report.
    fn write_new_result(&mut self, target: Self::Output, bytes: &[u8]) -> io::Result<()>;
}

/// An admitted private output whose filesystem path remains owned by this module.
#[derive(Debug)]
pub struct AdmittedPrivateRainPerformanceResultPath {
    path: PathBuf,
}

/// Native Unix publisher for the original repository and home research roots.
#[derive(Debug)]
pub struct NativeRainPerformanceResultWriter {
    roots: [PathBuf; 2],
    working_directory: PathBuf,
    owner_uid: u32,
}

impl NativeRainPerformanceResultWriter {
    /// Bind trusted process roots, working directory and owner identity once.
    pub fn new(
        repository_root: PathBuf,
        home_directory: PathBuf,
        working_directory: PathBuf,
        owner_uid: u32,
    ) -> io::Result<Self> {
        let repository_root = normalize_absolute(repository_root)?;
        let home_directory = normalize_absolute(home_directory)?;
        let working_directory = normalize_absolute(working_directory)?;
        Ok(Self {
            roots: [
                repository_root.join(".weather-data"),
                home_directory.join(".weather/research-work"),
            ],
            working_directory,
            owner_uid,
        })
    }
}

impl RainPerformanceResultWriter for NativeRainPerformanceResultWriter {
    type Output = AdmittedPrivateRainPerformanceResultPath;

    /// Resolve and establish one private owned directory ancestry.
    fn admit_result_path(&mut self, raw: &str) -> io::Result<Self::Output> {
        let output = resolve_from(&self.working_directory, raw);
        let root = self
            .roots
            .iter()
            .find(|candidate| strict_descendant(&output, candidate))
            .cloned()
            .ok_or_else(outside_private_storage)?;
        let base = root.parent().ok_or_else(canonical_ancestry_required)?;

        // require the configured base spelling to name its canonical directory
        if fs::canonicalize(base)? != base {
            return Err(canonical_ancestry_required());
        }

        create_private_directory(&root)?;
        verify_private_directory(&root, self.owner_uid)?;

        let parent = output.parent().ok_or_else(outside_private_storage)?;
        let relative_parent = parent
            .strip_prefix(&root)
            .map_err(|_| outside_private_storage())?;
        let mut current = root;

        // create and verify each output parent in original path order
        for component in relative_parent.components() {
            current.push(component.as_os_str());
            create_private_directory(&current)?;
            verify_private_directory(&current, self.owner_uid)?;
        }

        Ok(AdmittedPrivateRainPerformanceResultPath { path: output })
    }

    /// Create the final report exclusively with its original owner-only mode.
    fn write_new_result(&mut self, target: Self::Output, bytes: &[u8]) -> io::Result<()> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(target.path)?
            .write_all(bytes)
    }
}

/// Resolve a configured absolute path without consulting filesystem links.
fn normalize_absolute(path: PathBuf) -> io::Result<PathBuf> {
    // reject process configuration that cannot represent Node's absolute roots
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "rain performance roots must be absolute",
        ));
    }

    Ok(normalize(path))
}

/// Match POSIX path resolution against the command's captured working directory.
fn resolve_from(working_directory: &Path, raw: &str) -> PathBuf {
    let requested = Path::new(raw);
    // replace rather than extend the working directory for an absolute request
    if requested.is_absolute() {
        normalize(requested.to_path_buf())
    } else {
        normalize(working_directory.join(requested))
    }
}

/// Remove dot segments while retaining the absolute root boundary.
fn normalize(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();

    // fold each lexical component like Node's path resolution
    for component in path.components() {
        match component {
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(value) => normalized.push(value),
            Component::Prefix(_) => unreachable!("native Unix paths have no prefix component"),
        }
    }

    normalized
}

/// Require a separator-delimited child rather than the storage root itself.
fn strict_descendant(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .is_ok_and(|relative| !relative.as_os_str().is_empty())
}

/// Create one directory with the original private mode or retain an existing node.
fn create_private_directory(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);

    // defer existing-node classification to the metadata check
    match builder.create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

/// Require one directory to remain private and owned by the admitted process user.
fn verify_private_directory(path: &Path, owner_uid: u32) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;

    // preserve the original owned directory boundary at every admitted level
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != owner_uid
        || (metadata.mode() & 0o077) != 0
    {
        return Err(private_owned_ancestry_required());
    }

    Ok(())
}

/// Build the original outside-private-storage refusal as an I/O boundary error.
fn outside_private_storage() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "performance output is outside private research storage",
    )
}

/// Build the original canonical-base refusal as an I/O boundary error.
fn canonical_ancestry_required() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "performance output requires canonical ancestry",
    )
}

/// Build the original private-owned-ancestry refusal as an I/O boundary error.
fn private_owned_ancestry_required() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "performance output requires private owned ancestry",
    )
}

#[cfg(test)]
#[path = "rain_performance_result_io_test.rs"]
mod tests;
