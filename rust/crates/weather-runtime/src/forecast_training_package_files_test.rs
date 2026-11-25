use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::NativeForecastTrainingPackageFiles;

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

struct ScratchDirectory(PathBuf);

impl ScratchDirectory {
    /// create one process-unique fixture directory
    fn new() -> Self {
        let sequence = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-forecast-package-files-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture root should be created");
        Self(path)
    }

    /// borrow the fixture directory path
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDirectory {
    /// remove only this fixture tree
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// read and enumerate one ordinary extracted package tree
#[test]
fn native_files_resolve_read_and_enumerate_regular_package_entries() {
    let scratch = ScratchDirectory::new();
    let canonical_scratch =
        fs::canonicalize(scratch.path()).expect("fixture root should be canonicalized");
    let working_directory = canonical_scratch.join("working");
    let package = canonical_scratch.join("package");
    let members = package.join("members");
    fs::create_dir(&working_directory).expect("working directory should be created");
    fs::create_dir(&package).expect("package directory should be created");
    fs::create_dir(&members).expect("members directory should be created");
    fs::write(package.join("manifest.json"), b"{\"contractVersion\":1}\n")
        .expect("manifest fixture should be written");
    fs::write(members.join("part.jsonl.gz"), b"compressed fixture")
        .expect("member fixture should be written");

    let files = NativeForecastTrainingPackageFiles::new(working_directory)
        .expect("absolute working directory should bind");
    let absolute = files
        .absolute_path("../package/./")
        .expect("package path should resolve");
    assert_eq!(
        absolute,
        package.to_str().expect("package path should be UTF-8")
    );
    assert_eq!(
        files
            .canonical_package_path(&absolute)
            .expect("package identity should resolve"),
        Some(absolute.clone())
    );
    assert_eq!(
        files
            .read_regular_file(&absolute, "manifest.json")
            .expect("manifest should be readable"),
        Some(b"{\"contractVersion\":1}\n".to_vec())
    );
    assert_eq!(
        files
            .collect_regular_tree_entries(&absolute)
            .expect("package tree should be readable"),
        Some(vec![
            "manifest.json".to_owned(),
            "members/".to_owned(),
            "members/part.jsonl.gz".to_owned(),
        ])
    );
}
