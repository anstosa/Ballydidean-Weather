//! Differential future-only finalization through real private native and source slots.

use std::{
    fs,
    io::{Cursor, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use super::*;
use crate::native_maintenance_anchor_install_v3::NativeMaintenanceAnchorV3InstallStore;
use crate::{
    MaintenanceCanonicalJson, NativeFutureInputSealCurrent, NativeWirePrimitives,
    native_future_input_seal_current_test::valid_future_input_seal,
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    /// Create both disposable private roots for the source and native transactions.
    fn new() -> Self {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "weather-finalize-anchor-v3-{}-{id}",
            std::process::id()
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for name in ["node", "native"] {
            // both owners begin with a genuinely private evidence root and anchor directory
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(name))
                .unwrap();
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(name).join("maintenance-anchors"))
                .unwrap();
        }
        Self(root.canonicalize().unwrap())
    }
}

impl Drop for Fixture {
    /// Remove only the disposable differential fixture.
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

/// Create a source-valid transferred anchor and its separately canonical v3 proof.
fn materials(codec: &mut NativeWirePrimitives) -> (Vec<u8>, Vec<u8>, String) {
    let seal_value = valid_future_input_seal(codec, &"a".repeat(64));
    let seal_bytes = codec.encode_canonical_json(&seal_value).unwrap();
    let seal = NativeFutureInputSealCurrent {
        seal: seal_value,
        seal_sha256: codec.sha256(&seal_bytes).unwrap().as_str().to_owned(),
    };
    let mut fields = Vec::with_capacity(PROOF_BINDINGS.len() + 2);
    // copy exact source seal fields; T-only identities are appended separately
    for key in PROOF_BINDINGS {
        let value = match *key {
            "actionSha256" | "controlSha256" => json_text(&"b".repeat(64)),
            "controlVersion" => json_text("14"),
            "inputSealSha256" => json_text(&seal.seal_sha256),
            "predecessorAnchorSha256" => JsonValue::Null,
            "publishedAt" | "fullGraphVerifiedAt" => field(&seal.seal, "sealedAt").unwrap().clone(),
            _ => field(&seal.seal, key).unwrap().clone(),
        };
        fields.push((key.encode_utf16().collect(), value));
    }
    let mut anchor_fields = fields.clone();
    anchor_fields.push((
        "contractVersion".encode_utf16().collect(),
        json_text(ANCHOR_VERSION),
    ));
    anchor_fields.push((
        "ctfState".encode_utf16().collect(),
        json_text("transferred"),
    ));
    let anchor_bytes = codec
        .encode_canonical_json(&JsonValue::Object(anchor_fields))
        .unwrap();
    let anchor_digest = codec.sha256(&anchor_bytes).unwrap().as_str().to_owned();
    fields.push((
        "contractVersion".encode_utf16().collect(),
        json_text(PROOF_VERSION),
    ));
    fields.push((
        "finalizedAt".encode_utf16().collect(),
        json_text("2026-10-08T03:00:00.000Z"),
    ));
    fields.push((
        "transferredAnchorSha256".encode_utf16().collect(),
        json_text(&anchor_digest),
    ));
    let proof_bytes = codec
        .encode_canonical_json(&JsonValue::Object(fields))
        .unwrap();
    let proof_digest = codec.sha256(&proof_bytes).unwrap().as_str().to_owned();
    (anchor_bytes, proof_bytes, proof_digest)
}

/// Seed only the disposable current T slot with the exact source anchor bytes.
fn seed(root: &Path, bytes: &[u8]) {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join("maintenance-anchors/current.json"))
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

/// Execute the genuine original v3 command with explicit disposable disk and stdin.
fn source_finalize(root: &Path, proof: &[u8], digest: &str) -> Vec<u8> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let mut child = Command::new("node")
        .current_dir(repo)
        .args([
            "--input-type=module",
            "--eval",
            r#"
import { readFileSync } from "node:fs";
import { runAdjustmentArchiveCommand } from "./deploy/scripts/adjustment-evidence-store.mjs";
await runAdjustmentArchiveCommand(["maintenance-anchor-finalize-v3", process.argv[2]], {
  root: process.argv[1], stdinBytes: readFileSync(0), stdout: process.stdout,
});
"#,
        ])
        .arg(root)
        .arg(digest)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(proof).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

/// Compare the exact Node and native finalization output and durable direct predecessor bytes.
#[test]
fn native_future_anchor_finalize_matches_original_and_replays_idempotently() {
    let fixture = Fixture::new();
    let native_root = fixture.0.join("native");
    let source_root = fixture.0.join("node");
    let metadata = fs::metadata(&native_root).unwrap();
    let mut codec = NativeWirePrimitives;
    let (anchor, proof, digest) = materials(&mut codec);
    seed(&native_root, &anchor);
    seed(&source_root, &anchor);
    let mut files = NativeMaintenanceAnchorV3InstallStore::for_evidence_root(
        native_root.clone(),
        metadata.uid(),
        metadata.gid(),
    );
    // source and native owners execute both first finalization and exact retry
    for _ in 0..2 {
        let original = source_finalize(&source_root, &proof, &digest);
        let native = finalize_maintenance_anchor_v3_command(
            (COMMAND, &[&digest]),
            &mut Cursor::new(&proof),
            &mut files,
            &mut codec,
            &NativeWirePrimitives,
        )
        .unwrap()
        .unwrap();
        assert_eq!(native, original);
        // preserve both full canonical slot identities with no remaining pending marker
        for slot in ["current.json", "previous.json"] {
            let relative = Path::new("maintenance-anchors").join(slot);
            assert_eq!(
                fs::read(native_root.join(&relative)).unwrap(),
                fs::read(source_root.join(&relative)).unwrap()
            );
        }
        assert!(
            !native_root
                .join("maintenance-anchors/pending.json")
                .exists()
        );
        assert!(
            !source_root
                .join("maintenance-anchors/pending.json")
                .exists()
        );
    }
    assert_eq!(
        fs::read(native_root.join("maintenance-anchors/previous.json")).unwrap(),
        anchor
    );
}
