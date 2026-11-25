//! native unix filesystem primitives for extracted forecast-training packages

use std::cmp::Ordering;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use crate::api_maintenance_candidate_files::open_read_no_follow;

/// native filesystem values bound to one trusted process directory
#[derive(Debug, Clone)]
pub struct NativeForecastTrainingPackageFiles {
    working_directory: PathBuf,
}

impl NativeForecastTrainingPackageFiles {
    /// bind one trusted absolute working directory
    pub fn new(working_directory: PathBuf) -> io::Result<Self> {
        // require an absolute process directory
        if !working_directory.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "forecast package working directory must be absolute",
            ));
        }

        Ok(Self {
            working_directory: normalize_absolute(&working_directory),
        })
    }

    /// resolve one path lexically from the captured working directory
    pub fn absolute_path(&self, raw: &str) -> io::Result<String> {
        let requested = Path::new(raw);
        // replace the working directory for absolute input
        let absolute = if requested.is_absolute() {
            normalize_absolute(requested)
        } else {
            normalize_absolute(&self.working_directory.join(requested))
        };
        path_string(absolute)
    }

    /// return one existing nonsymbolic package path's canonical identity
    pub fn canonical_package_path(&self, absolute: &str) -> io::Result<Option<String>> {
        let path = Path::new(absolute);
        // refuse ambient resolution outside the captured path flow
        if !path.is_absolute() {
            return Ok(None);
        }
        // inspect the final component without following it
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            // retain missing roots as ordinary invalid input
            Err(error) if missing_path_component(&error) => return Ok(None),
            Err(error) => return Err(error),
        };

        // let a nonsymbolic file fail at the control file check
        if metadata.file_type().is_symlink() {
            return Ok(None);
        }

        fs::canonicalize(path).and_then(path_string).map(Some)
    }

    /// read one nonsymbolic regular file below the package root
    pub fn read_regular_file(
        &self,
        root: &str,
        relative_path: &str,
    ) -> io::Result<Option<Vec<u8>>> {
        // require one lexical member below the root
        let Some(path) = member_path(root, relative_path) else {
            return Ok(None);
        };
        // inspect the final component without following it
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            // retain missing members as ordinary invalid input
            Err(error) if missing_path_component(&error) => return Ok(None),
            Err(error) => return Err(error),
        };

        // require the final input node to be a regular file
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Ok(None);
        }

        let mut file = open_read_no_follow(&path)?;
        // recheck the opened identity before reading bytes
        if !file.metadata()?.is_file() {
            return Ok(None);
        }

        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }

    /// enumerate real directories and regular files below the package root
    pub fn collect_regular_tree_entries(&self, root: &str) -> io::Result<Option<Vec<String>>> {
        let root = Path::new(root);
        // require the verifier's admitted absolute root
        if !root.is_absolute() {
            return Ok(None);
        }
        // inspect the root without following it
        let metadata = match fs::symlink_metadata(root) {
            Ok(metadata) => metadata,
            // retain missing roots as ordinary invalid input
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };

        // reject symbolic and nondirectory roots
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Ok(None);
        }

        let mut output = Vec::new();
        collect_entries(root, root, &mut output)?;
        Ok(Some(output))
    }
}

/// treat an absent or nondirectory parent as a missing descendant
fn missing_path_component(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// remove lexical dot segments from one absolute path
fn normalize_absolute(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::from("/");

    // fold components like posix path resolution
    for component in path.components() {
        // preserve only path-bearing components
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(value) => normalized.push(value),
            Component::Prefix(_) => unreachable!("native Unix paths have no prefix component"),
        }
    }

    normalized
}

/// convert one native path to the verifier string contract
fn path_string(path: PathBuf) -> io::Result<String> {
    path.into_os_string().into_string().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "forecast package path must be UTF-8",
        )
    })
}

/// resolve one nonempty relative member below the lexical root
fn member_path(root: &str, relative_path: &str) -> Option<PathBuf> {
    let root = Path::new(root);
    let relative = Path::new(relative_path);
    // require the admitted absolute root and a relative member
    if !root.is_absolute() || relative.as_os_str().is_empty() || relative.is_absolute() {
        return None;
    }

    let normalized_root = normalize_absolute(root);
    let candidate = normalize_absolute(&normalized_root.join(relative));
    let child = candidate.strip_prefix(&normalized_root).ok()?;
    // exclude the root and every escaping member spelling
    if child.as_os_str().is_empty() {
        return None;
    }
    Some(candidate)
}

/// collect one directory in deterministic depth-first order
fn collect_entries(root: &Path, directory: &Path, output: &mut Vec<String>) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| js_name_cmp(&left.file_name(), &right.file_name()));

    // inspect every child without following its final component
    for entry in entries {
        let file_type = entry.file_type()?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .expect("walked entries remain below their root");
        let Some(relative) = relative.to_str() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "forecast package entry path must be UTF-8",
            ));
        };
        let relative = relative.replace(std::path::MAIN_SEPARATOR, "/");

        // descend only through real directories
        if file_type.is_dir() {
            output.push(format!("{relative}/"));
            collect_entries(root, &path, output)?;
        } else {
            // retain only ordinary file leaves
            if file_type.is_file() {
                output.push(relative);
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "package contains a symbolic or special file",
                ));
            }
        }
    }

    Ok(())
}

/// compare native names by javascript utf-16 units
fn js_name_cmp(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> Ordering {
    let left = left.to_string_lossy();
    let right = right.to_string_lossy();
    left.encode_utf16().cmp(right.encode_utf16())
}

#[cfg(test)]
#[path = "forecast_training_package_files_test.rs"]
mod tests;
