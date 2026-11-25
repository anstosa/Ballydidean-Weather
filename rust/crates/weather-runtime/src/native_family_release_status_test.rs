use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use weather_contracts::JsonValue;

use super::{NativeFamilyReleaseStatusHostFiles, read_native_family_release_status_command};
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};

const ACTION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one canonical process-unique private journal root.
    fn create() -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-family-release-status-{}-{sequence}",
            std::process::id()
        ));
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}

impl Drop for FixtureRoot {
    /// Remove only this disposable status fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Preserve exact absent status when no transaction path exists.
#[test]
fn projects_absent_status_without_creating_journal_state() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).unwrap();
    let mut files =
        NativeFamilyReleaseStatusHostFiles::for_root(root.0.clone(), owner.uid(), owner.gid());
    let mut codec = NativeWirePrimitives;

    let actual = read_native_family_release_status_command(
        "family-release-status",
        &[ACTION],
        &mut files,
        &mut codec,
        &NativeWirePrimitives,
    )
    .unwrap()
    .unwrap();

    assert_eq!(
        actual,
        b"{\"actionSha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"contractVersion\":\"adjustment-family-release-status/v1\",\"state\":\"absent\"}\n"
    );
    assert!(!root.0.join("transactions").exists());
}

/// Project an in-flight transaction with its required explicit null outcome.
#[test]
fn projects_prepared_status_with_null_outcome() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).unwrap();
    let transactions = root.0.join("transactions");
    DirBuilder::new().mode(0o700).create(&transactions).unwrap();
    let document = object(vec![
        (
            "compensation",
            object(vec![
                ("actionSha256", string(HASH)),
                ("release", string("2026.10.10-2")),
                ("state", string("absent")),
                ("verifiedAt", JsonValue::Null),
            ]),
        ),
        (
            "contractVersion",
            string("adjustment-family-release-transaction/v1"),
        ),
        ("proof", object(Vec::new())),
        (
            "record",
            object(vec![
                (
                    "input",
                    object(vec![
                        ("actionSha256", string(ACTION)),
                        ("compensatingRelease", string("2026.10.10-2")),
                        ("family", string("temperature")),
                        ("fencingToken", string("7")),
                        ("targetRelease", string("2026.10.10-1")),
                    ]),
                ),
                ("state", string("prepared")),
            ]),
        ),
    ]);
    let mut codec = NativeWirePrimitives;
    write_transaction(&transactions, &mut codec, &document);
    let mut files =
        NativeFamilyReleaseStatusHostFiles::for_root(root.0.clone(), owner.uid(), owner.gid());

    let actual = read_native_family_release_status_command(
        "family-release-status",
        &[ACTION],
        &mut files,
        &mut codec,
        &NativeWirePrimitives,
    )
    .unwrap()
    .unwrap();

    assert_eq!(
        actual,
        b"{\"actionSha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"compensatingRelease\":\"2026.10.10-2\",\"compensationState\":\"absent\",\"contractVersion\":\"adjustment-family-release-status/v1\",\"family\":\"temperature\",\"fencingToken\":\"7\",\"outcome\":null,\"state\":\"prepared\",\"targetRelease\":\"2026.10.10-1\"}\n"
    );
}

/// Preserve the disjoint durable operator-off categorical receipt.
#[test]
fn projects_operator_off_status_without_synthetic_proof() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).unwrap();
    let transactions = root.0.join("transactions");
    DirBuilder::new().mode(0o700).create(&transactions).unwrap();
    let input = object(vec![
        ("actionSha256", string(ACTION)),
        ("compensatingRelease", string("2026.10.10-2")),
        ("expectedCurrentRelease", string("2026.10.09-1")),
        ("expectedSettingsSha256", string(HASH)),
        ("expectedSourceRelease", string("2026.10.09-1")),
        ("family", string("temperature")),
        ("fencingToken", string("7")),
        ("reportSha256", string(HASH)),
        ("targetRelease", string("2026.10.10-1")),
    ]);
    let document = object(vec![
        (
            "contractVersion",
            string("adjustment-family-operator-off-transaction/v1"),
        ),
        (
            "record",
            object(vec![
                ("completedAt", string("2026-10-10T09:00:00.000Z")),
                ("input", input),
                ("sourceCommit", string(&"c".repeat(40))),
                ("sourceSettingsSha256", string(HASH)),
                ("state", string("operator_off_unapplied")),
            ]),
        ),
    ]);
    let mut codec = NativeWirePrimitives;
    write_transaction(&transactions, &mut codec, &document);
    let mut files =
        NativeFamilyReleaseStatusHostFiles::for_root(root.0.clone(), owner.uid(), owner.gid());

    let actual = read_native_family_release_status_command(
        "family-release-status",
        &[ACTION],
        &mut files,
        &mut codec,
        &NativeWirePrimitives,
    )
    .unwrap()
    .unwrap();

    assert_eq!(
        actual,
        b"{\"actionSha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"compensatingRelease\":\"2026.10.10-2\",\"compensationState\":\"absent\",\"contractVersion\":\"adjustment-family-release-status/v1\",\"family\":\"temperature\",\"fencingToken\":\"7\",\"outcome\":\"operator_off_unapplied\",\"state\":\"operator_off_unapplied\",\"targetRelease\":\"2026.10.10-1\"}\n"
    );
}

/// Write one exact canonical private transaction fixture.
fn write_transaction(transactions: &Path, codec: &mut NativeWirePrimitives, document: &JsonValue) {
    let bytes = codec.encode_canonical_json(document).unwrap();
    let path = transactions.join(format!("sha256-{ACTION}.json"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
}

/// Build one object from source-named fixture members.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            // preserve caller field population before canonical encoding
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one exact UTF-16 source string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
