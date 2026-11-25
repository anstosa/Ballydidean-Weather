use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::native_forecast_training_export::{
    NativeForecastTrainingExportReceipt, write_native_forecast_training_package_archive,
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

/// prove the native archive bytes equal the exact predecessor tar and gzip pipeline.
#[test]
fn native_archive_matches_predecessor_pipeline() {
    let fixture = Fixture::new();
    let root = fixture.path.join("package");
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let member_directory = root.join("members/2026-10-01/station-hour");
    fs::create_dir_all(&member_directory).unwrap();
    set_directory_modes(&root);
    let files = [
        (
            "manifest.json",
            b"{\"contractVersion\":\"fixture\"}\n".as_slice(),
        ),
        ("manifest.sha256", b"fixture  manifest.json\n".as_slice()),
        (
            "members/2026-10-01/station-hour/ambient-maxweather.jsonl.gz",
            b"fixture-member".as_slice(),
        ),
    ];
    let mut native_files = BTreeMap::new();
    // retain the source's regular-file modes and sorted relative paths
    for (relative, bytes) in files {
        let path = root.join(relative);
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        native_files.insert(relative.to_owned(), path);
    }
    let mut native = Vec::new();
    write_native_forecast_training_package_archive(native_files, &mut native).unwrap();

    let tar = Command::new("tar")
        .args([
            "--create",
            "--format=ustar",
            "--sort=name",
            "--mtime=@0",
            "--owner=0",
            "--group=0",
            "--numeric-owner",
            "--directory",
        ])
        .arg(&root)
        .arg(".")
        .output()
        .unwrap();
    assert!(
        tar.status.success(),
        "{}",
        String::from_utf8_lossy(&tar.stderr)
    );
    let mut gzip = Command::new("gzip")
        .args(["--no-name", "--best"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    gzip.stdin.as_mut().unwrap().write_all(&tar.stdout).unwrap();
    drop(gzip.stdin.take());
    let predecessor = gzip.wait_with_output().unwrap();
    assert!(predecessor.status.success());
    assert_eq!(native, predecessor.stdout);
}

/// preserve the existing shell completion line as a closed native receipt projection.
#[test]
fn completion_report_matches_predecessor_text() {
    let receipt = NativeForecastTrainingExportReceipt {
        manifest_sha256: "a".repeat(64),
        row_count: 264,
    };
    assert_eq!(
        receipt.completion_report("2026-10-01", "2026-10-01"),
        format!(
            "Streaming verified Ballydidean forecast-training export 2026-10-01..2026-10-01 manifest={} rows<=4000000 oracle=3045600 export_rows_not_training_events\n",
            "a".repeat(64)
        )
    );
}

struct Fixture {
    path: PathBuf,
}

impl Fixture {
    /// create one process-owned disposable archive fixture.
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-native-forecast-training-export-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self { path }
    }
}

impl Drop for Fixture {
    /// remove only this process-owned fixture tree.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// apply the predecessor's fixed private mode to each created directory.
fn set_directory_modes(root: &std::path::Path) {
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
    // retain each real directory without following symbolic links
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        // descend only into fixture-owned directories
        if entry.file_type().unwrap().is_dir() {
            set_directory_modes(&entry.path());
        }
    }
}
