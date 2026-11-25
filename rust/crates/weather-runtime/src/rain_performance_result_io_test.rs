use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{NativeRainPerformanceResultWriter, RainPerformanceResultWriter};

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

struct ScratchDirectory(PathBuf);

impl ScratchDirectory {
    /// Create one process-unique disposable directory.
    fn new() -> Self {
        let sequence = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-rain-result-{}-{sequence}",
            std::process::id()
        ));
        create_directory(&path);
        Self(path)
    }

    /// Borrow the disposable directory path.
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDirectory {
    /// Remove only this test's disposable directory tree.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Create one fixture directory with private permissions.
fn create_directory(path: &Path) {
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder
        .create(path)
        .expect("fixture directory should exist");
}

/// Exercise both original roots and retain exclusive report creation.
#[test]
fn native_writer_publishes_exact_bytes_to_both_private_roots() {
    let scratch = ScratchDirectory::new();
    let repository = scratch.path().join("repository");
    let home = scratch.path().join("home");
    let working_directory = scratch.path().join("working");
    create_directory(&repository);
    create_directory(&home);
    create_directory(&home.join(".weather"));
    create_directory(&working_directory);
    let owner_uid = fs::metadata(scratch.path())
        .expect("scratch metadata should remain available")
        .uid();
    let mut writer = NativeRainPerformanceResultWriter::new(
        repository.clone(),
        home.clone(),
        working_directory,
        owner_uid,
    )
    .expect("absolute fixture roots should bind");

    let repository_output = repository.join(".weather-data/reports/rain.json");
    let repository_request = "../repository/.weather-data/reports/rain.json";
    let repository_target = writer
        .admit_result_path(repository_request)
        .expect("repository output should be admitted");
    writer
        .write_new_result(repository_target, b"repository report\n")
        .expect("repository output should be created");
    assert_eq!(
        fs::read(&repository_output).expect("repository output should be readable"),
        b"repository report\n"
    );
    assert_eq!(
        fs::metadata(repository.join(".weather-data/reports"))
            .expect("private parent should exist")
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&repository_output)
            .expect("repository output metadata should exist")
            .mode()
            & 0o777,
        0o600
    );

    let home_output = home.join(".weather/research-work/rain.json");
    let home_target = writer
        .admit_result_path(
            home_output
                .to_str()
                .expect("fixture output should be UTF-8"),
        )
        .expect("home output should be admitted");
    writer
        .write_new_result(home_target, b"home report\n")
        .expect("home output should be created");
    assert_eq!(
        fs::read(&home_output).expect("home output should be readable"),
        b"home report\n"
    );

    let existing_target = writer
        .admit_result_path(repository_request)
        .expect("existing output ancestry should remain admitted");
    let error = writer
        .write_new_result(existing_target, b"replacement")
        .expect_err("existing output must not be replaced");
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        fs::read(repository_output).expect("repository output should remain readable"),
        b"repository report\n"
    );
}
