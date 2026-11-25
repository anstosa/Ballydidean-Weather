//! Native filesystem operations for strict adjustment evaluation packages.

use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use crate::adjustment_evaluation_package_stream::AdjustmentEvaluationOpenedMember;
use crate::api_maintenance_candidate_files::open_read_no_follow;

/// Local filesystem owner for the original adjustment package operations.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeAdjustmentEvaluationPackageFiles;

impl NativeAdjustmentEvaluationPackageFiles {
    /// read a bounded control member through one stable opened descriptor
    pub fn read_bounded_control(
        &self,
        root: &str,
        relative_path: &str,
        maximum_bytes: u64,
        description: &str,
    ) -> io::Result<Vec<u8>> {
        let path = admitted_path(root, relative_path)?;
        let mut file = open_read_no_follow(&path)?;
        let before = file.metadata()?;
        // bound storage by the exact size on the opened descriptor
        if !before.is_file() || before.len() > maximum_bytes {
            return Err(io::Error::other(format!(
                "{description} is not a bounded regular file"
            )));
        }
        let declared = usize::try_from(before.len()).map_err(io::Error::other)?;
        let mut bytes = Vec::with_capacity(declared.saturating_add(1));
        (&mut file)
            .take(before.len().saturating_add(1))
            .read_to_end(&mut bytes)?;
        // require exact input length and descriptor metadata after the read
        if bytes.len() != declared || !same_file_snapshot(&before, &file.metadata()?) {
            return Err(io::Error::other(format!(
                "{description} changed while reading"
            )));
        }
        Ok(bytes)
    }

    /// return the sorted package files without including empty directories
    pub fn list_regular_files(&self, root: &str) -> io::Result<Vec<String>> {
        let root = Path::new(root);
        let mut files = Vec::new();
        list_descendants(root, root, &mut files)?;
        files.sort_unstable();
        Ok(files)
    }

    /// open a declared member from the package and retain its initial metadata
    pub fn open_member(
        &self,
        root: &str,
        relative_path: &str,
    ) -> io::Result<NativeAdjustmentEvaluationOpenedMember> {
        let path = admitted_path(root, relative_path)?;
        let parent = path.parent().ok_or_else(invalid_member_location)?;
        let canonical_root = fs::canonicalize(root)?;
        let canonical_parent = fs::canonicalize(parent)?;
        let entry = fs::symlink_metadata(&path)?;
        // keep the opened kind and canonical parent within the original root
        if !entry.is_file() || !canonical_parent.starts_with(&canonical_root) {
            return Err(invalid_member_location());
        }
        let file = open_read_no_follow(&path)?;
        let before = file.metadata()?;
        Ok(NativeAdjustmentEvaluationOpenedMember { file, before })
    }
}

/// One native descriptor retaining its exact metadata at open time.
#[derive(Debug)]
pub struct NativeAdjustmentEvaluationOpenedMember {
    file: File,
    before: Metadata,
}

impl Read for NativeAdjustmentEvaluationOpenedMember {
    /// read only from the owned opened descriptor
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.file.read(buffer)
    }
}

impl AdjustmentEvaluationOpenedMember for NativeAdjustmentEvaluationOpenedMember {
    /// match the original regular descriptor and declared size
    fn has_declared_size(&self, size_bytes: u64) -> bool {
        self.before.is_file() && self.before.len() == size_bytes
    }

    /// compare the original opened descriptor against its final metadata
    fn is_unchanged(&self) -> io::Result<bool> {
        Ok(same_file_snapshot(&self.before, &self.file.metadata()?))
    }
}

/// resolve only fixed control paths and already admitted member declarations
fn admitted_path(root: &str, relative_path: &str) -> io::Result<PathBuf> {
    let relative_path = Path::new(relative_path);
    // require a nonempty relative path without dot or escape components
    if relative_path.as_os_str().is_empty()
        || !relative_path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(invalid_member_location());
    }
    Ok(Path::new(root).join(relative_path))
}

/// preserve the original regular package member diagnostic
fn invalid_member_location() -> io::Error {
    io::Error::other("package member must be a regular file below its fixed root")
}

/// compare the retained unix descriptor identity and nanosecond timestamps
fn same_file_snapshot(before: &Metadata, after: &Metadata) -> bool {
    before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

/// traverse the exact package file set without counting the directories
fn list_descendants(root: &Path, directory: &Path, files: &mut Vec<String>) -> io::Result<()> {
    // retain every ordinary file and reject any unsupported entry type
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        // match the existing symbolic entry diagnostic
        if kind.is_symlink() {
            return Err(io::Error::other("package contains a symbolic link"));
        }
        // descend only into directory entries
        if kind.is_dir() {
            list_descendants(root, &path, files)?;
        } else {
            // retain only ordinary package file leaves
            if !kind.is_file() {
                return Err(io::Error::other("package contains a special file"));
            }
            let relative = path.strip_prefix(root).map_err(io::Error::other)?;
            files.push(
                relative
                    .to_str()
                    .ok_or_else(|| io::Error::other("package pathname is not valid UTF-8"))?
                    .to_owned(),
            );
        }
    }
    Ok(())
}
