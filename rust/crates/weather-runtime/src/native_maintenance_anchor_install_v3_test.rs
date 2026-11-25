use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{Cursor, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use weather_contracts::JsonValue;

use super::{
    NativeMaintenanceAnchorInstallV3Control, NativeMaintenanceAnchorInstallV3SealStore,
    NativeMaintenanceAnchorV3InstallStore, SEAL_BINDINGS, install_maintenance_anchor_v3_command,
};
use crate::native_future_input_seal_current::NativeFutureInputSealCurrent;
use crate::native_future_input_seal_current_test::valid_future_input_seal;
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Disposable evidence root removed after one ordinary anchor transaction fixture.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique canonical owner-private evidence root.
    fn create() -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-maintenance-anchor-install-v3-{}-{sequence}",
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

/// Install, retry, and advance one exact v3 transaction with real slot owners.
#[test]
fn installs_idempotently_and_rotates_the_direct_predecessor() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).unwrap();
    let seal_root = root.0.join("future-input-seals");
    DirBuilder::new().mode(0o700).create(&seal_root).unwrap();
    let seal_path = seal_root.join("current.json");
    let mut codec = NativeWirePrimitives;
    let genesis_seal = valid_future_input_seal(&mut codec, &"c".repeat(64));
    let genesis_seal_bytes = codec.encode_canonical_json(&genesis_seal).unwrap();
    write_private(&seal_path, &genesis_seal_bytes, true);
    let genesis_seal_current = current_seal(&genesis_seal, &genesis_seal_bytes, &mut codec);

    let mut seal_store = NativeMaintenanceAnchorInstallV3SealStore::for_evidence_root(
        root.0.clone(),
        owner.uid(),
        owner.gid(),
        NativeWirePrimitives,
        NativeWirePrimitives,
    );
    let mut files = NativeMaintenanceAnchorV3InstallStore::for_evidence_root(
        root.0.clone(),
        owner.uid(),
        owner.gid(),
    );
    let control_sha256 = "b".repeat(64);
    let control = NativeMaintenanceAnchorInstallV3Control {
        version: Some("14"),
        sha256: Some(&control_sha256),
    };
    let genesis_anchor =
        transferred_anchor(&genesis_seal_current, &control_sha256, JsonValue::Null);
    let genesis_anchor_bytes = codec.encode_canonical_json(&genesis_anchor).unwrap();
    let genesis_anchor_sha256 = codec
        .sha256(&genesis_anchor_bytes)
        .unwrap()
        .as_str()
        .to_owned();

    let response = install_maintenance_anchor_v3_command(
        ("maintenance-anchor-install-v3", &[&genesis_anchor_sha256]),
        &mut Cursor::new(&genesis_anchor_bytes),
        &mut files,
        &mut seal_store,
        &mut codec,
        &NativeWirePrimitives,
        control,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        response,
        codec
            .encode_canonical_json(&JsonValue::Object(vec![
                text_member("anchorSha256", &genesis_anchor_sha256),
                text_member(
                    "contractVersion",
                    "adjustment-maintenance-anchor-installation/v3",
                ),
                text_member("state", "transferred"),
            ]))
            .unwrap()
    );
    let anchor_root = root.0.join("maintenance-anchors");
    assert_eq!(
        fs::read(anchor_root.join("current.json")).unwrap(),
        genesis_anchor_bytes
    );
    assert!(!anchor_root.join("pending.json").exists());

    // replay the exact transferred anchor through the raw seal binding
    install_maintenance_anchor_v3_command(
        ("maintenance-anchor-install-v3", &[&genesis_anchor_sha256]),
        &mut Cursor::new(&genesis_anchor_bytes),
        &mut files,
        &mut seal_store,
        &mut codec,
        &NativeWirePrimitives,
        control,
    )
    .unwrap();

    let mut successor_seal = genesis_seal;
    replace_field(
        &mut successor_seal,
        "predecessorSealSha256",
        json_text(&genesis_seal_current.seal_sha256),
    );
    replace_field(&mut successor_seal, "sequence", json_text("1"));
    let successor_seal_bytes = codec.encode_canonical_json(&successor_seal).unwrap();
    write_private(&seal_path, &successor_seal_bytes, false);
    let successor_seal_current = current_seal(&successor_seal, &successor_seal_bytes, &mut codec);
    let successor_anchor = transferred_anchor(
        &successor_seal_current,
        &control_sha256,
        json_text(&genesis_anchor_sha256),
    );
    let successor_anchor_bytes = codec.encode_canonical_json(&successor_anchor).unwrap();
    let successor_anchor_sha256 = codec
        .sha256(&successor_anchor_bytes)
        .unwrap()
        .as_str()
        .to_owned();
    install_maintenance_anchor_v3_command(
        ("maintenance-anchor-install-v3", &[&successor_anchor_sha256]),
        &mut Cursor::new(&successor_anchor_bytes),
        &mut files,
        &mut seal_store,
        &mut codec,
        &NativeWirePrimitives,
        control,
    )
    .unwrap();

    assert_eq!(
        fs::read(anchor_root.join("current.json")).unwrap(),
        successor_anchor_bytes
    );
    assert_eq!(
        fs::read(anchor_root.join("previous.json")).unwrap(),
        genesis_anchor_bytes
    );
    assert!(!anchor_root.join("pending.json").exists());
}

/// Assemble every exact seal binding and the additive transferred-anchor fields.
fn transferred_anchor(
    seal: &NativeFutureInputSealCurrent,
    control_sha256: &str,
    predecessor: JsonValue,
) -> JsonValue {
    let mut fields = SEAL_BINDINGS
        .iter()
        .map(|key| {
            (
                key.encode_utf16().collect(),
                object_field(&seal.seal, key).clone(),
            )
        })
        .collect::<Vec<_>>();
    let sealed_at = object_field(&seal.seal, "sealedAt").clone();
    fields.extend([
        text_member("actionSha256", &"a".repeat(64)),
        text_member("contractVersion", "adjustment-maintenance-anchor/v3"),
        text_member("controlSha256", control_sha256),
        text_member("controlVersion", "14"),
        text_member("ctfState", "transferred"),
        (
            "fullGraphVerifiedAt".encode_utf16().collect(),
            sealed_at.clone(),
        ),
        text_member("inputSealSha256", &seal.seal_sha256),
        (
            "predecessorAnchorSha256".encode_utf16().collect(),
            predecessor,
        ),
        ("publishedAt".encode_utf16().collect(), sealed_at),
    ]);
    JsonValue::Object(fields)
}

/// Bind one parsed fixture seal to the digest of its exact canonical bytes.
fn current_seal(
    seal: &JsonValue,
    bytes: &[u8],
    codec: &mut NativeWirePrimitives,
) -> NativeFutureInputSealCurrent {
    NativeFutureInputSealCurrent {
        seal: seal.clone(),
        seal_sha256: codec.sha256(bytes).unwrap().as_str().to_owned(),
    }
}

/// Create or replace one fixture slot while preserving its private mode.
fn write_private(path: &std::path::Path, bytes: &[u8], create: bool) {
    let mut options = OpenOptions::new();
    options.write(true).truncate(true).mode(0o600);
    // use exclusive creation only for the first fixture publication
    if create {
        options.create_new(true);
    }
    let mut file = options.open(path).unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

/// Replace one existing fixture object member without changing unrelated values.
fn replace_field(value: &mut JsonValue, name: &str, replacement: JsonValue) {
    let JsonValue::Object(members) = value else {
        panic!("fixture value is not an object");
    };
    let key: Vec<u16> = name.encode_utf16().collect();
    // mutate only the one required source field
    if let Some((_, value)) = members.iter_mut().find(|(candidate, _)| *candidate == key) {
        *value = replacement;
        return;
    }
    panic!("fixture field is absent");
}

/// Borrow one required fixture object member.
fn object_field<'a>(value: &'a JsonValue, name: &str) -> &'a JsonValue {
    let JsonValue::Object(members) = value else {
        panic!("fixture value is not an object");
    };
    let key: Vec<u16> = name.encode_utf16().collect();
    &members
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .expect("fixture field")
        .1
}

/// Construct one canonical object text member.
fn text_member(name: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), json_text(value))
}

/// Construct one UTF-16 JSON string value.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
