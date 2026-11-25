//! Focused native current-anchor result using a source-valid retained seal fixture.

use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use weather_contracts::JsonValue;

use super::*;
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceAnchorStatusFiles, NativeWirePrimitives,
    native_future_input_seal_current_test::valid_future_input_seal,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create a disposable owner-private root with a unique fixture identity.
    fn new() -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "weather-native-current-anchor-{}-{id}",
            std::process::id()
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
}

impl Drop for FixtureRoot {
    /// Remove only the created disposable root.
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

/// Match source JSON UTF-16 values without normalizing their spelling.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Assemble exactly the source copied fields and additive transferred anchor identities.
fn transferred_anchor(seal: &NativeFutureInputSealCurrent, control: &str) -> JsonValue {
    let mut fields = SEAL_BINDINGS
        .iter()
        .map(|key| {
            (
                key.encode_utf16().collect(),
                field(&seal.seal, key).unwrap().clone(),
            )
        })
        .collect::<Vec<_>>();
    let sealed = field(&seal.seal, "sealedAt").unwrap().clone();
    let action = "a".repeat(64);
    let additions = [
        ("actionSha256", json_text(&action)),
        (
            "contractVersion",
            json_text("adjustment-maintenance-anchor/v3"),
        ),
        ("controlSha256", json_text(control)),
        ("controlVersion", json_text("14")),
        ("ctfState", json_text("transferred")),
        ("fullGraphVerifiedAt", sealed.clone()),
        ("inputSealSha256", json_text(&seal.seal_sha256)),
        ("predecessorAnchorSha256", JsonValue::Null),
        ("publishedAt", sealed),
    ];
    fields.extend(
        additions
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value)),
    );
    JsonValue::Object(fields)
}

/// Ask the actual Node v3 current-envelope validator to admit the exact native bytes.
fn validate_source_envelope(bytes: &[u8]) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let mut child = Command::new("node")
        .current_dir(root)
        .args(["--input-type=module", "--eval", r#"
import { readFileSync } from "node:fs";
import { validateAdjustmentMaintenanceAnchorCurrentV3 } from "./deploy/scripts/adjustment-evidence-store.mjs";
const bytes = readFileSync(0);
validateAdjustmentMaintenanceAnchorCurrentV3(JSON.parse(bytes));
process.stdout.write(bytes);
"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn().unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, bytes);
}

/// Verify source v3 current identity and genuine empty root without modifying custody state.
#[test]
fn current_v3_envelope_uses_verified_seal_and_exact_retained_bytes() {
    let fixture = FixtureRoot::new();
    let metadata = fs::metadata(&fixture.0).unwrap();
    let mut files = NativeMaintenanceAnchorStatusFiles::for_evidence_root(
        fixture.0.clone(),
        metadata.uid(),
        metadata.gid(),
    );
    let hash = "b".repeat(64);
    let control = NativeMaintenanceAnchorCurrentV3Control {
        version: Some("14"),
        sha256: Some(&hash),
    };
    let mut codec = NativeWirePrimitives;
    let value = valid_future_input_seal(&mut codec, &"c".repeat(64));
    let seal_bytes = codec.encode_canonical_json(&value).unwrap();
    let seal = NativeFutureInputSealCurrent {
        seal: value,
        seal_sha256: codec.sha256(&seal_bytes).unwrap().as_str().to_owned(),
    };

    // source verifies the current seal even before genesis anchor absence is observed
    let mut seal_reads = 0;
    let missing = read_maintenance_anchor_current_v3_command(
        COMMAND,
        &[],
        &mut files,
        &mut codec,
        &NativeWirePrimitives,
        control,
        |_, _| {
            seal_reads += 1;
            Ok::<_, ()>(None)
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(seal_reads, 1);
    assert_eq!(missing, b"null\n");

    let root = fixture.0.join("maintenance-anchors");
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let path = root.join("current.json");
    let anchor = transferred_anchor(&seal, &hash);
    let anchor_bytes = codec.encode_canonical_json(&anchor).unwrap();
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .unwrap()
        .write_all(&anchor_bytes)
        .unwrap();
    let expected_sha = codec.sha256(&anchor_bytes).unwrap();
    let actual = read_maintenance_anchor_current_v3_command(
        COMMAND,
        &[],
        &mut files,
        &mut codec,
        &NativeWirePrimitives,
        control,
        |_, _| Ok::<_, ()>(Some(seal)),
    )
    .unwrap()
    .unwrap();
    let expected = codec
        .encode_canonical_json(&JsonValue::Object(vec![
            ("anchor".encode_utf16().collect(), anchor),
            (
                "anchorSha256".encode_utf16().collect(),
                json_text(expected_sha.as_str()),
            ),
        ]))
        .unwrap();
    assert_eq!(actual, expected);
    validate_source_envelope(&actual);
    assert_eq!(fs::read(path).unwrap(), anchor_bytes);
}
