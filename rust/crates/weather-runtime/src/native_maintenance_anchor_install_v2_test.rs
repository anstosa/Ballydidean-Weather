use std::fs;
use std::io::Cursor;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use weather_contracts::JsonValue;

use super::{
    NativeMaintenanceAnchorInstallV2Control, NativeMaintenanceAnchorInstallV2Error,
    NativeMaintenanceAnchorV2Transfer, NativeMaintenanceAnchorV2TransferAuthority,
    install_maintenance_anchor_v2_command,
};
use crate::native_maintenance_anchor_install_v3::NativeMaintenanceAnchorV3InstallStore;
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Disposable evidence root removed after one ordinary legacy transaction fixture.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique canonical owner-private evidence root.
    fn create() -> Self {
        use std::os::unix::fs::DirBuilderExt;

        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-maintenance-anchor-install-v2-{}-{sequence}",
            std::process::id()
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-unique disposable fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct FixtureTransferAuthority;

impl NativeMaintenanceAnchorV2TransferAuthority for FixtureTransferAuthority {
    /// Project the exact cycle identity encoded by either fixture manifest digest.
    fn read_transfer(
        &mut self,
        manifest_sha256: &str,
    ) -> Result<Option<NativeMaintenanceAnchorV2Transfer>, NativeMaintenanceAnchorInstallV2Error>
    {
        let generation = if manifest_sha256 == "1".repeat(64) {
            "0"
        } else if manifest_sha256 == "2".repeat(64) {
            "1"
        } else {
            return Ok(None);
        };
        let hour = if generation == "0" { "00" } else { "06" };
        Ok(Some(NativeMaintenanceAnchorV2Transfer {
            due_key: format!("capture/2026-10-08T{hour}:35:00.000Z"),
            generation: generation.to_owned(),
            graph_sha256: "3".repeat(64),
            acknowledged_at: format!("2026-10-08T{hour}:36:00.000Z"),
        }))
    }
}

/// Install, retry, and rotate one exact legacy transaction without a response body.
#[test]
fn installs_legacy_transfer_and_retains_the_direct_predecessor() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).unwrap();
    let mut files = NativeMaintenanceAnchorV3InstallStore::for_evidence_root(
        root.0.clone(),
        owner.uid(),
        owner.gid(),
    );
    let mut authority = FixtureTransferAuthority;
    let mut codec = NativeWirePrimitives;
    let control_sha256 = "4".repeat(64);
    let control = NativeMaintenanceAnchorInstallV2Control {
        version: Some("13"),
        sha256: Some(&control_sha256),
    };
    let genesis = transferred_anchor("0", "1", "00", JsonValue::Null, &control_sha256);
    let genesis_bytes = codec.encode_canonical_json(&genesis).unwrap();
    let genesis_sha256 = codec.sha256(&genesis_bytes).unwrap().as_str().to_owned();

    let response = install_maintenance_anchor_v2_command(
        ("maintenance-anchor-install-v2", &[&genesis_sha256]),
        &mut Cursor::new(&genesis_bytes),
        &mut files,
        &mut authority,
        &mut codec,
        &NativeWirePrimitives,
        control,
    )
    .unwrap()
    .unwrap();
    assert!(response.is_empty());
    let anchor_root = root.0.join("maintenance-anchors");
    assert_eq!(
        fs::read(anchor_root.join("current.json")).unwrap(),
        genesis_bytes
    );

    // converge one exact already-current legacy retry
    install_maintenance_anchor_v2_command(
        ("maintenance-anchor-install-v2", &[&genesis_sha256]),
        &mut Cursor::new(&genesis_bytes),
        &mut files,
        &mut authority,
        &mut codec,
        &NativeWirePrimitives,
        control,
    )
    .unwrap();

    let successor = transferred_anchor("1", "2", "06", json_text(&genesis_sha256), &control_sha256);
    let successor_bytes = codec.encode_canonical_json(&successor).unwrap();
    let successor_sha256 = codec.sha256(&successor_bytes).unwrap().as_str().to_owned();
    install_maintenance_anchor_v2_command(
        ("maintenance-anchor-install-v2", &[&successor_sha256]),
        &mut Cursor::new(&successor_bytes),
        &mut files,
        &mut authority,
        &mut codec,
        &NativeWirePrimitives,
        control,
    )
    .unwrap();

    assert_eq!(
        fs::read(anchor_root.join("current.json")).unwrap(),
        successor_bytes
    );
    assert_eq!(
        fs::read(anchor_root.join("previous.json")).unwrap(),
        genesis_bytes
    );
    assert!(!anchor_root.join("pending.json").exists());
}

/// Assemble one source-valid transferred v2 anchor for an admitted final closure.
fn transferred_anchor(
    sequence: &str,
    manifest_nibble: &str,
    hour: &str,
    predecessor: JsonValue,
    control_sha256: &str,
) -> JsonValue {
    JsonValue::Object(vec![
        ("actionSha256".encode_utf16().collect(), JsonValue::Null),
        hash_member("archiveObjectSha256", "5"),
        text_member("catalogGeneration", sequence),
        hash_member("catalogRootSha256", "6"),
        hash_member("catalogWatermarkSha256", "7"),
        text_member("contractVersion", "adjustment-maintenance-anchor/v2"),
        text_member("controlSha256", control_sha256),
        text_member("controlVersion", "13"),
        text_member("ctfState", "transferred"),
        text_member("dueKey", &format!("capture/2026-10-08T{hour}:35:00.000Z")),
        (
            "frontierCount".encode_utf16().collect(),
            JsonValue::Number(1.0),
        ),
        hash_member("frontierRootSha256", "8"),
        text_member(
            "fullGraphVerifiedAt",
            &format!("2026-10-08T{hour}:35:30.000Z"),
        ),
        (
            "fullMemberRootSha256".encode_utf16().collect(),
            JsonValue::Null,
        ),
        text_member("generation", sequence),
        hash_member("graphManifestSha256", "3"),
        hash_member("lifecycleLedgerRootSha256", "9"),
        text_member("manifestSha256", &manifest_nibble.repeat(64)),
        (
            "predecessorAnchorSha256".encode_utf16().collect(),
            predecessor,
        ),
        text_member("publishedAt", &format!("2026-10-08T{hour}:37:00.000Z")),
        ("reportSha256".encode_utf16().collect(), JsonValue::Null),
        (
            "revisionSnapshotRootSha256".encode_utf16().collect(),
            JsonValue::Null,
        ),
        text_member("sequence", sequence),
        text_member("sourceCommit", &"a".repeat(40)),
        hash_member("workstationJournalHeadSha256", "b"),
    ])
}

/// Construct one repeated-nibble SHA-256 member.
fn hash_member(name: &str, nibble: &str) -> (Vec<u16>, JsonValue) {
    text_member(name, &nibble.repeat(64))
}

/// Construct one canonical object text member.
fn text_member(name: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), json_text(value))
}

/// Construct one UTF-16 JSON string value.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
