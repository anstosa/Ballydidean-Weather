//! Disposable legacy fixed-address retirement and retained-byte parity.

use super::*;
use crate::NativeWirePrimitives;
use std::{
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    /// Create a process-unique private retirement root.
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "weather-native-retirement-v2-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed),
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}

impl Drop for Fixture {
    /// Clear only the process-unique disposable fixture.
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

/// Materialize exactly one bounded original metadata member in a disposable directory.
fn seed(
    root: &Path,
    kind: NativeMaintenanceRetirementKindV2,
    id: char,
    bytes: &[u8],
) -> NativeMaintenanceRetirementEntryV2 {
    let entry = NativeMaintenanceRetirementEntryV2::new(
        kind,
        id.to_string().repeat(64),
        format_digest(NativeWirePrimitives.sha256(bytes)),
    )
    .unwrap();
    let path = entry.path(root).unwrap();
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path.parent().unwrap())
        .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
    entry
}

/// Verify only the named redundant members retire and exact absence replays idempotently.
#[test]
fn fixed_members_retire_with_exact_hash_and_retry() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let archive = seed(
        root,
        NativeMaintenanceRetirementKindV2::ArchiveAcknowledgement,
        'a',
        b"archive\n",
    );
    let revision = seed(
        root,
        NativeMaintenanceRetirementKindV2::RevisionCommitReceipt,
        'b',
        b"revision\n",
    );
    let retained = seed(
        root,
        NativeMaintenanceRetirementKindV2::RevisionCommitReceipt,
        'c',
        b"unrelated\n",
    );
    // collection observes both exact receipt file identities and the original source byte bounds
    assert_eq!(
        read_maintenance_revision_commit_receipt_names_v2(root).unwrap(),
        vec![
            format!("sha256-{}.json", revision.identity_sha256),
            format!("sha256-{}.json", retained.identity_sha256),
        ]
    );
    assert_eq!(
        read_expected_maintenance_member_v2(root, archive.kind, &archive.identity_sha256)
            .unwrap()
            .unwrap(),
        b"archive\n"
    );
    assert_eq!(
        parse_maintenance_retirement_entry_v2(&archive.to_json()).unwrap(),
        archive
    );
    let entries = [archive, revision];
    assert_eq!(
        retire_maintenance_hot_files_v2(root, &entries, &NativeWirePrimitives).unwrap(),
        2
    );
    assert_eq!(
        retire_maintenance_hot_files_v2(root, &entries, &NativeWirePrimitives).unwrap(),
        0
    );
    assert_eq!(
        fs::read(retained.path(root).unwrap()).unwrap(),
        b"unrelated\n"
    );
}
