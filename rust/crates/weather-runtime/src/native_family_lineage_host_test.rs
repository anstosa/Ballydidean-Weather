use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{NativeFamilyLineageFiles, NativeFamilyLineageHostFiles};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Disposable lineage root removed after one ordinary filesystem fixture.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique canonical owner-private root.
    fn create() -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-family-lineage-host-{}-{sequence}",
            std::process::id()
        ));
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-unique disposable fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Read exact current bytes and a sorted filename-bound transaction inventory.
#[test]
fn reads_stable_owner_private_lineage_inventory() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).unwrap();
    let transactions = root.0.join("transactions");
    DirBuilder::new().mode(0o700).create(&transactions).unwrap();
    write_file(&root.0.join("current.json"), b"{\"current\":true}\n");
    let first = "a".repeat(64);
    let second = "b".repeat(64);
    write_file(
        &transactions.join(format!("sha256-{second}.json")),
        b"{\"second\":true}\n",
    );
    write_file(
        &transactions.join(format!("sha256-{first}.json")),
        b"{\"first\":true}\n",
    );
    let mut files =
        NativeFamilyLineageHostFiles::for_root(root.0.clone(), owner.uid(), owner.gid());

    assert_eq!(files.read_current().unwrap(), b"{\"current\":true}\n");
    let observed = files.read_transactions().unwrap();
    assert_eq!(observed.len(), 2);
    assert_eq!(observed[0].action_sha256, first);
    assert_eq!(observed[0].canonical_bytes, b"{\"first\":true}\n");
    assert_eq!(observed[1].action_sha256, second);
    assert_eq!(observed[1].canonical_bytes, b"{\"second\":true}\n");
}

/// Write one fixture file with the required owner-private mode.
fn write_file(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
