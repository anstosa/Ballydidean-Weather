//! Exact legacy final and transfer metadata parity without a gzip implementation.

use super::*;
use crate::{
    NativeWirePrimitives,
    family_target_catalog::{object, string},
};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
use weather_contracts::{
    ForecastArchiveFinalAcknowledgementV1ContractVersionLiteral,
    ForecastArchiveFinalManifestV2ContractVersionLiteral,
    ForecastArchiveInputSealV1ContractVersionLiteral, JsonValue,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);

impl Fixture {
    /// Create only a process-unique private legacy archive fixture.
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "weather-legacy-metadata-v2-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed),
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}

impl Drop for Fixture {
    /// Remove only the unique fixture on completion.
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

/// Produce one canonical source metadata member without constructing forecast payloads.
fn write_member(path: &Path, bytes: &[u8]) {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path.parent().unwrap())
        .unwrap();
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

/// Construct a complete source-v2 final manifest with the genuine embedded metadata seal.
fn manifest(codec: &mut NativeWirePrimitives) -> JsonValue {
    let due = "capture/2026-10-08T00:35:00.000Z";
    let pages = JsonValue::Array(vec![object(vec![
        ("pageIndex", JsonValue::Number(0.0)),
        ("pageSha256", string(&"a".repeat(64))),
        ("payloadLength", JsonValue::Number(100.0)),
        ("payloadOffset", JsonValue::Number(0.0)),
        ("payloadSha256", string(&"b".repeat(64))),
        ("predecessorPageSha256", JsonValue::Null),
    ])]);
    let pages_bytes = codec.encode_canonical_json(&pages).unwrap();
    let page_root = crate::MaintenanceCanonicalJson::sha256(codec, &pages_bytes).unwrap();
    let empty = codec
        .encode_canonical_json(&JsonValue::Array(Vec::new()))
        .unwrap();
    let empty_root = crate::MaintenanceCanonicalJson::sha256(codec, &empty).unwrap();
    let required = object(
        [
            "actual_best_match",
            "native_source",
            "rain_gate_input",
            "serving_evidence",
            "shadow_body",
            "shadow_metadata",
            "target_revision",
        ]
        .into_iter()
        .map(|name| {
            (
                name,
                object(vec![
                    ("count", JsonValue::Number(1.0)),
                    ("rootSha256", string(&"c".repeat(64))),
                ]),
            )
        })
        .collect(),
    );
    let seal = object(vec![
        (
            "contractVersion",
            string(ForecastArchiveInputSealV1ContractVersionLiteral::WIRE_VALUE),
        ),
        ("dueKey", string(due)),
        ("generation", string("7")),
        ("pageRootSha256", string(page_root.as_str())),
        ("acknowledgementRootSha256", string(&"d".repeat(64))),
        ("acknowledgedProjectionCount", JsonValue::Number(1.0)),
        ("acknowledgedProjectionRootSha256", string(&"e".repeat(64))),
        ("evidenceGapRootSha256", string(empty_root.as_str())),
        ("requiredInputs", required),
        ("sealedAt", string("2026-10-08T00:42:00.000Z")),
    ]);
    object(vec![
        (
            "contractVersion",
            string(ForecastArchiveFinalManifestV2ContractVersionLiteral::WIRE_VALUE),
        ),
        ("dueKey", string(due)),
        ("generation", string("7")),
        ("finalizedAt", string("2026-10-08T00:43:00.000Z")),
        ("totalPayloadBytes", JsonValue::Number(100.0)),
        ("pageCount", JsonValue::Number(1.0)),
        ("acknowledgedProjectionCount", JsonValue::Number(1.0)),
        ("pageRootSha256", string(page_root.as_str())),
        ("evidenceGapCount", JsonValue::Number(0.0)),
        ("acknowledgementRootSha256", string(&"d".repeat(64))),
        ("acknowledgedProjectionRootSha256", string(&"e".repeat(64))),
        ("evidenceGapRootSha256", string(empty_root.as_str())),
        ("pages", pages),
        ("inputSeal", seal),
    ])
}

/// Compare the actual metadata-only native reader with the source final and ACK disk readers.
#[test]
fn canonical_v2_transfer_reads_exact_source_metadata_without_compression() {
    let fixture = Fixture::new();
    let mut canonical = NativeWirePrimitives;
    let document = manifest(&mut canonical);
    let final_bytes = canonical.encode_canonical_json(&document).unwrap();
    let digest = crate::MaintenanceCanonicalJson::sha256(&mut canonical, &final_bytes).unwrap();
    write_member(
        &fixture.0.join(format!(
            "online-pages/final-manifests/sha256-{}.json",
            digest.as_str()
        )),
        &final_bytes,
    );
    let acknowledgement = object(vec![
        (
            "contractVersion",
            string(ForecastArchiveFinalAcknowledgementV1ContractVersionLiteral::WIRE_VALUE),
        ),
        ("manifestSha256", string(digest.as_str())),
        ("graphSha256", string(&"f".repeat(64))),
        ("acknowledgedAt", string("2026-10-08T00:44:00.000Z")),
    ]);
    let ack_bytes = canonical.encode_canonical_json(&acknowledgement).unwrap();
    write_member(
        &fixture.0.join(format!(
            "online-pages/final-acknowledgements/sha256-{}.json",
            digest.as_str()
        )),
        &ack_bytes,
    );

    let mut authority = NativeMaintenanceLegacyMetadataV2::new(
        fixture.0.clone(),
        ArchiveWireCodec::new(NativeWirePrimitives),
        NativeWirePrimitives,
        ArchiveWireCodec::new(NativeWirePrimitives),
    );
    let observed = authority.read_transfer(digest.as_str()).unwrap().unwrap();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let original = Command::new("node")
        .current_dir(repo)
        .args([
            "--input-type=module",
            "--eval",
            r#"
import { AdjustmentCyclePageDiskPorts } from "./deploy/scripts/adjustment-evidence-store.mjs";
const ports = new AdjustmentCyclePageDiskPorts({root: process.argv[1]});
const final = await ports.readFinalManifest(process.argv[2]);
const ack = await ports.readFinalAcknowledgement(process.argv[2]);
process.stdout.write(JSON.stringify({
  acknowledgedAt: ack.acknowledgedAt,
  dueKey: final.manifest.dueKey,
  generation: final.manifest.generation,
  graphSha256: ack.graphSha256,
}));
"#,
        ])
        .arg(&fixture.0)
        .arg(digest.as_str())
        .output()
        .unwrap();
    assert!(
        original.status.success(),
        "{}",
        String::from_utf8_lossy(&original.stderr)
    );
    let native = serde_json::json!({
        "acknowledgedAt": observed.acknowledged_at,
        "dueKey": observed.due_key,
        "generation": observed.generation,
        "graphSha256": observed.graph_sha256,
    });
    assert_eq!(serde_json::to_vec(&native).unwrap(), original.stdout);
}
