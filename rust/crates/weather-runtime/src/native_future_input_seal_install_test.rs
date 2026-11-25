use std::fs::{self, DirBuilder};
use std::io::Cursor;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use weather_contracts::JsonValue;

use super::{
    NativeFutureInputSealCustody, NativeFutureInputSealInstallError,
    NativeFutureInputSealInstallStore, install_future_input_seal_command,
};
use crate::native_future_input_seal_current::{
    NativeFutureInputSealCaptureEpoch, NativeFutureInputSealCaptureEpochWitness,
    NativeFutureInputSealCurrentError,
};
use crate::native_future_input_seal_current_test::valid_future_input_seal;
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Disposable evidence root removed after one ordinary installation fixture.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique canonical owner-private evidence root.
    fn create() -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-future-input-seal-install-{}-{sequence}",
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

struct FixtureCustody {
    calls: usize,
}

impl NativeFutureInputSealCustody for FixtureCustody {
    /// Admit the source-valid seal while recording the independent authority call.
    fn admit_custody(&mut self, seal: &JsonValue) -> Result<(), NativeFutureInputSealInstallError> {
        assert_eq!(text(seal, "archiveCommitOrdinal"), "42");
        self.calls += 1;
        Ok(())
    }
}

struct FixtureCaptureEpoch {
    witness: NativeFutureInputSealCaptureEpochWitness,
}

impl NativeFutureInputSealCaptureEpoch for FixtureCaptureEpoch {
    /// Return one fixture value already standing in for the actual witness verifier.
    fn read_verified_capture_epoch(
        &mut self,
    ) -> Result<NativeFutureInputSealCaptureEpochWitness, NativeFutureInputSealCurrentError> {
        Ok(self.witness.clone())
    }
}

/// Install, retry, and advance one exact transaction while retaining its predecessor.
#[test]
fn installs_idempotently_and_retains_only_the_direct_predecessor() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).unwrap();
    let mut files = NativeFutureInputSealInstallStore::for_evidence_root(
        root.0.clone(),
        owner.uid(),
        owner.gid(),
    );
    let witness_bytes = b"verified capture epoch fixture\n".to_vec();
    let mut codec = NativeWirePrimitives;
    let witness_sha256 = codec.sha256(&witness_bytes).unwrap();
    let mut capture_epoch = FixtureCaptureEpoch {
        witness: NativeFutureInputSealCaptureEpochWitness {
            canonical_bytes: witness_bytes,
            control_version: "14".to_owned(),
            control_sha256: "a".repeat(64),
        },
    };
    let mut custody = FixtureCustody { calls: 0 };
    let genesis = valid_future_input_seal(&mut codec, witness_sha256.as_str());
    let genesis_bytes = codec.encode_canonical_json(&genesis).unwrap();
    let genesis_sha256 = codec.sha256(&genesis_bytes).unwrap().as_str().to_owned();

    let first = install_future_input_seal_command(
        ("future-input-seal-install-v2", &[&genesis_sha256]),
        &mut Cursor::new(&genesis_bytes),
        &mut files,
        &mut custody,
        &mut capture_epoch,
        &mut codec,
        &NativeWirePrimitives,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        first,
        codec
            .encode_canonical_json(&JsonValue::Object(vec![
                text_member(
                    "contractVersion",
                    "adjustment-future-only-input-seal-installation/v2",
                ),
                text_member("sealSha256", &genesis_sha256),
                text_member("state", "installed"),
            ]))
            .unwrap()
    );
    let seal_root = root.0.join("future-input-seals");
    assert_eq!(
        fs::read(seal_root.join("current.json")).unwrap(),
        genesis_bytes
    );
    assert!(!seal_root.join("pending.json").exists());

    // replay the installed bytes through the same custody and epoch boundaries
    install_future_input_seal_command(
        ("future-input-seal-install-v2", &[&genesis_sha256]),
        &mut Cursor::new(&genesis_bytes),
        &mut files,
        &mut custody,
        &mut capture_epoch,
        &mut codec,
        &NativeWirePrimitives,
    )
    .unwrap();

    let mut successor = genesis;
    replace_field(
        &mut successor,
        "predecessorSealSha256",
        json_text(&genesis_sha256),
    );
    replace_field(&mut successor, "sequence", json_text("1"));
    let successor_bytes = codec.encode_canonical_json(&successor).unwrap();
    let successor_sha256 = codec.sha256(&successor_bytes).unwrap().as_str().to_owned();
    install_future_input_seal_command(
        ("future-input-seal-install-v2", &[&successor_sha256]),
        &mut Cursor::new(&successor_bytes),
        &mut files,
        &mut custody,
        &mut capture_epoch,
        &mut codec,
        &NativeWirePrimitives,
    )
    .unwrap();

    assert_eq!(
        fs::read(seal_root.join("current.json")).unwrap(),
        successor_bytes
    );
    assert_eq!(
        fs::read(seal_root.join("previous.json")).unwrap(),
        genesis_bytes
    );
    assert!(!seal_root.join("pending.json").exists());
    assert_eq!(custody.calls, 3);
}

/// Replace one existing fixture object field without changing its source order.
fn replace_field(value: &mut JsonValue, name: &str, replacement: JsonValue) {
    let JsonValue::Object(members) = value else {
        panic!("fixture seal is not an object");
    };
    let key: Vec<u16> = name.encode_utf16().collect();
    // mutate only the one named source field
    if let Some((_, value)) = members.iter_mut().find(|(candidate, _)| *candidate == key) {
        *value = replacement;
        return;
    }
    panic!("fixture field is absent");
}

/// Decode one known fixture text field for the custody assertion.
fn text(value: &JsonValue, name: &str) -> String {
    let JsonValue::Object(members) = value else {
        panic!("fixture seal is not an object");
    };
    let key: Vec<u16> = name.encode_utf16().collect();
    let (_, JsonValue::String(units)) = members
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .expect("fixture field")
    else {
        panic!("fixture field is not text");
    };
    String::from_utf16(units).unwrap()
}

/// Construct one canonical object text member.
fn text_member(name: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), json_text(value))
}

/// Construct one UTF-16 JSON string value.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
