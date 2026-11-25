use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    NativeUnsupportedTerminalProofInstallFiles, NativeUnsupportedTerminalProofInstallStore,
};
use crate::MaintenanceAnchorSlot;

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Disposable private evidence root for the native transaction-file smoke.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique canonical owner-private directory.
    fn create() -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-unsupported-proof-install-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("unique disposable evidence root");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-unique disposable fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Exercise the genuine exclusive-pending and atomic current/previous filesystem effects.
#[test]
fn native_store_publishes_and_retires_one_private_transaction() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).unwrap();
    let witness = root.0.join("unused-witness.json");
    let mut store = NativeUnsupportedTerminalProofInstallStore::for_paths(
        root.0.clone(),
        witness,
        owner.uid(),
        owner.gid(),
        owner.uid(),
        owner.gid(),
    );
    store.ensure_root().unwrap();
    store.create_pending(b"pending-proof\n").unwrap();
    assert_eq!(
        store.read_slot(MaintenanceAnchorSlot::Pending).unwrap(),
        Some(b"pending-proof\n".to_vec())
    );

    store
        .replace_slot(MaintenanceAnchorSlot::Previous, b"previous-proof\n")
        .unwrap();
    store
        .replace_slot(MaintenanceAnchorSlot::Current, b"current-proof\n")
        .unwrap();
    store.remove_pending().unwrap();
    store.sync_root().unwrap();

    let proof_root = root.0.join("unsupported-terminal-proofs");
    assert_eq!(
        fs::read(proof_root.join("current.json")).unwrap(),
        b"current-proof\n"
    );
    assert_eq!(
        fs::read(proof_root.join("previous.json")).unwrap(),
        b"previous-proof\n"
    );
    assert!(!proof_root.join("pending.json").exists());
    // every retained transaction file remains owner-private
    for name in ["current.json", "previous.json"] {
        let metadata = fs::metadata(proof_root.join(name)).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert_eq!(metadata.nlink(), 1);
    }
}
