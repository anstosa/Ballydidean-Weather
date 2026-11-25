//! Retained backing-allocation census for the fixed workstation maintenance buckets.

use std::{
    collections::HashSet,
    fs::{self, Metadata, OpenOptions},
    io,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

const MAXIMUM_CENSUS_ENTRIES: u64 = 10_000_000;

/// Native read-only aggregate over the original deduplicated backing inode census.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceBackingCensus {
    pub allocated_bytes: u64,
    pub entry_count: u64,
}

/// Enumerate the exact four predecessor storage buckets relative to the admitted home.
pub fn native_maintenance_backing_roots(home: &Path) -> [PathBuf; 4] {
    [
        home.join(".weather"),
        home.join("weather/.omx/evidence"),
        home.join("weather/deploy/backups"),
        home.join(".local/lib/weather-adjustment-maintenance"),
    ]
}

/// Charge each physical inode once while preserving source entry count and no-link traversal.
pub fn measure_native_maintenance_backing_census(
    roots: &[PathBuf],
    expected_device: u64,
) -> io::Result<NativeMaintenanceBackingCensus> {
    // preserve the frozen one-to-four canonical absolute bucket shape
    if !(1..=4).contains(&roots.len()) || roots.iter().any(|root| !valid_root(root)) {
        return Err(invalid_census());
    }
    let mut seen = HashSet::new();
    let mut census = NativeMaintenanceBackingCensus {
        allocated_bytes: 0,
        entry_count: 0,
    };
    // skip only absent admitted buckets
    for root in roots {
        let details = match fs::symlink_metadata(root) {
            Ok(details) => details,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let mut pending = vec![(root.clone(), details)];
        // traverse one observed subtree without reading file payloads
        while let Some((path, mut details)) = pending.pop() {
            // rebind every directory before traversal
            if details.is_dir() {
                details = stable_metadata(&path, &details)?;
            }
            census.entry_count = census
                .entry_count
                .checked_add(1)
                .ok_or_else(invalid_census)?;
            // preserve the source census device, supported inode types and finite entry ceiling
            if census.entry_count > MAXIMUM_CENSUS_ENTRIES
                || details.dev() != expected_device
                || !(details.is_dir() || details.is_file() || details.file_type().is_symlink())
            {
                return Err(invalid_census());
            }
            // count aliases without charging the same retained allocation twice
            if seen.insert((details.dev(), details.ino())) {
                census.allocated_bytes = census
                    .allocated_bytes
                    .checked_add(
                        details
                            .blocks()
                            .checked_mul(512)
                            .ok_or_else(invalid_census)?,
                    )
                    .ok_or_else(invalid_census)?;
            }
            // hold the original directory inode while collecting literal entries
            if details.is_dir() {
                let held = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
                    .open(&path)?;
                require_same(&details, &held.metadata()?)?;
                stable_metadata(&path, &details)?;
                let directory = PathBuf::from(format!("/proc/self/fd/{}", held.as_raw_fd()));
                // preserve source observation order-independent counting and fail on disappearance
                for child in fs::read_dir(directory)? {
                    let child_path = path.join(child?.file_name());
                    let child_details = fs::symlink_metadata(&child_path)?;
                    pending.push((child_path, child_details));
                    // reject a potentially oversized pending population before allocating further
                    if census.entry_count.saturating_add(pending.len() as u64)
                        > MAXIMUM_CENSUS_ENTRIES
                    {
                        return Err(invalid_census());
                    }
                }
                stable_metadata(&path, &details)?;
            }
        }
    }
    Ok(census)
}

/// Admit canonical lexical absolute paths without filesystem resolution.
fn valid_root(root: &Path) -> bool {
    let normalized = root.components().collect::<PathBuf>();
    root.is_absolute()
        && root.to_str().is_some_and(|value| value.len() <= 4096)
        && normalized.as_os_str() == root.as_os_str()
        && !root
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
}

/// Re-read one literal path and retain only the same observed inode and mode.
fn stable_metadata(path: &Path, expected: &Metadata) -> io::Result<Metadata> {
    let actual = fs::symlink_metadata(path)?;
    require_same(expected, &actual)?;
    Ok(actual)
}

/// Bind a path or open directory to the original census observation.
fn require_same(expected: &Metadata, actual: &Metadata) -> io::Result<()> {
    // reject a census whose observed inode or mode changed
    if expected.dev() != actual.dev()
        || expected.ino() != actual.ino()
        || expected.mode() != actual.mode()
    {
        return Err(invalid_census());
    }
    Ok(())
}

/// Preserve one value-free census failure category.
fn invalid_census() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "archive backing census is invalid",
    )
}

#[cfg(test)]
#[path = "native_maintenance_backing_census_test.rs"]
mod tests;
