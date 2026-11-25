use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use super::{NativeHostOwnerAccessAcknowledgementFile, admitted_request_family};
use crate::NativeWirePrimitives;
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Disposable canonical root for one installed-owner fixture.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique canonical fixture directory.
    fn create() -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-host-owner-access-installed-{}-{sequence}",
            std::process::id(),
        ));
        fs::create_dir(&path).expect("create installed owner fixture");
        Self(fs::canonicalize(path).expect("canonical installed owner fixture"))
    }
}

impl Drop for FixtureRoot {
    /// Remove only this process-unique disposable fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Read the exact current acknowledgement bytes through the real no-follow host owner.
#[test]
fn installed_acknowledgement_reader_preserves_service_owned_bytes() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).expect("read fixture owner");
    assert_eq!(
        owner.uid(),
        owner.gid(),
        "source host files require uid=gid"
    );
    let path = root.0.join("current.json");
    let bytes = b"{\"contractVersion\":\"fixture\"}\n";
    write_file(&path, bytes, 0o600);
    let mut reader = NativeHostOwnerAccessAcknowledgementFile::for_fixture(path, owner.uid());
    assert_eq!(reader.read().expect("read acknowledgement"), bytes);
}

/// Select the family only after canonical bytes and their forced operand agree.
#[test]
fn installed_access_selects_family_from_canonical_operand() {
    let value = object(vec![
        (
            "contractVersion",
            string("adjustment-confirmation-access-burn-request/v3"),
        ),
        (
            "shadowRegistration",
            object(vec![("family", string("rain"))]),
        ),
        ("unused", JsonValue::Null),
    ]);
    let bytes = NativeWirePrimitives
        .encode_sorted_json(&value)
        .expect("encode request selection fixture");
    let digest = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &bytes,
    ));
    assert_eq!(
        admitted_request_family(&bytes, &digest).expect("select installed family"),
        "rain",
    );
}

/// Write one exact fixture file with its final admitted mode.
fn write_file(path: &Path, bytes: &[u8], mode: u32) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .expect("create installed owner fixture file");
    file.write_all(bytes)
        .expect("write installed owner fixture file");
    file.sync_all().expect("sync installed owner fixture file");
    drop(file);
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .expect("set installed owner fixture mode");
}
