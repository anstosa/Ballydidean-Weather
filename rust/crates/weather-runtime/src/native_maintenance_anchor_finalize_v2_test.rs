//! Legacy finalization transaction over disposable exact redundant metadata.

use super::*;
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceRetirementExpectedV2,
    NativeMaintenanceRetirementExpectedV2Error, NativeWirePrimitives,
    native_maintenance_finalization_proof_v2::tests::{
        proof, replace, retirement_entries, transferred_anchor,
    },
    native_maintenance_retirement_files_v2::format_digest,
};
use std::{
    fs,
    io::{Cursor, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    /// Create one disposable private evidence root and current anchor directory.
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "weather-legacy-anchor-finalize-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed),
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path.join("maintenance-anchors"))
            .unwrap();
        Self(path.canonicalize().unwrap())
    }
}

impl Drop for Fixture {
    /// Clear only the unique disposable fixture.
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

struct Expected {
    graph: NativeMaintenanceRetirementExpectedV2,
    reads: usize,
}

impl NativeMaintenanceRetirementExpectedV2Authority for Expected {
    /// Supply one independently selected fixture graph and record each authority read.
    fn read_expected(
        &mut self,
        manifest_sha256: &str,
    ) -> Result<NativeMaintenanceRetirementExpectedV2, NativeMaintenanceRetirementExpectedV2Error>
    {
        assert_eq!(manifest_sha256, "9".repeat(64));
        self.reads += 1;
        Ok(self.graph.clone())
    }
}

/// Create one exact owner-private metadata file in the disposable legacy root.
fn write_private(path: &Path, bytes: &[u8]) {
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

/// Verify the genuine T-to-F slot commit, exact retirement set, and F idempotent retry.
#[test]
fn legacy_finalization_commits_exact_f_and_replays_only_original_retirement() {
    let fixture = Fixture::new();
    let root = &fixture.0;
    let metadata = fs::metadata(root).unwrap();
    let mut codec = NativeWirePrimitives;
    let anchor = transferred_anchor();
    let anchor_bytes = codec.encode_canonical_json(&anchor).unwrap();
    let anchor_digest = MaintenanceCanonicalJson::sha256(&mut codec, &anchor_bytes).unwrap();
    write_private(
        &root.join("maintenance-anchors/current.json"),
        &anchor_bytes,
    );
    let mut entries = retirement_entries();
    // seed the exact source-addressed cycle metadata and bind each hash to actual bytes
    for entry in &mut entries {
        let bytes = format!("{}\n", entry.kind.as_str()).into_bytes();
        entry.file_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
            &NativeWirePrimitives,
            &bytes,
        ));
        write_private(&entry.path(root).unwrap(), &bytes);
    }
    let retirement_json = JsonValue::Array(entries.iter().map(|entry| entry.to_json()).collect());
    let retirement_bytes = codec.encode_canonical_json(&retirement_json).unwrap();
    let retirement_hash = MaintenanceCanonicalJson::sha256(&mut codec, &retirement_bytes).unwrap();
    let mut proof_value = proof(&anchor, anchor_digest.as_str(), entries.clone());
    replace(
        &mut proof_value,
        "retirementSetRootSha256",
        retirement_hash.as_str().to_owned(),
    );
    let proof_bytes = codec.encode_canonical_json(&proof_value).unwrap();
    let proof_digest = MaintenanceCanonicalJson::sha256(&mut codec, &proof_bytes).unwrap();
    let mut expected = Expected {
        graph: NativeMaintenanceRetirementExpectedV2 {
            graph_sha256: "7".repeat(64),
            input_seal_sha256: "0".repeat(64),
            required_input_root_sha256: "1".repeat(64),
            revision_input_count: 4,
            complete_producers: true,
            entries: entries.clone(),
            canonical_entries: retirement_bytes.clone(),
            retirement_set_root_sha256: retirement_hash.as_str().to_owned(),
        },
        reads: 0,
    };
    let mut files = NativeMaintenanceAnchorV3InstallStore::for_evidence_root(
        root.clone(),
        metadata.uid(),
        metadata.gid(),
    );
    // the source legacy direct operator emits no body, even after actual durable retirement
    assert_eq!(
        finalize_maintenance_anchor_v2_command(
            (COMMAND, &[proof_digest.as_str()]),
            &mut Cursor::new(&proof_bytes),
            &mut files,
            &mut expected,
            &mut codec,
            &NativeWirePrimitives,
        )
        .unwrap(),
        Some(Vec::new()),
    );
    assert_eq!(expected.reads, 1);
    let current = fs::read(root.join("maintenance-anchors/current.json")).unwrap();
    let value = codec.parse_canonical_object(&current).unwrap();
    assert_eq!(text(&value, "ctfState").as_deref(), Some("finalized"));
    assert_eq!(
        text(&value, "finalizationProofSha256").as_deref(),
        Some(proof_digest.as_str())
    );
    assert_eq!(
        fs::read(root.join("maintenance-anchors/previous.json")).unwrap(),
        anchor_bytes
    );
    assert!(!root.join("maintenance-anchors/pending.json").exists());
    for entry in entries {
        // all independently bound redundant metadata must have been retired
        assert!(!entry.path(root).unwrap().exists());
    }
    // replay the exact finalized proof without rereading the graph whose hot members are gone
    assert_eq!(
        finalize_maintenance_anchor_v2_command(
            (COMMAND, &[proof_digest.as_str()]),
            &mut Cursor::new(&proof_bytes),
            &mut files,
            &mut expected,
            &mut codec,
            &NativeWirePrimitives,
        )
        .unwrap(),
        Some(Vec::new()),
    );
    assert_eq!(expected.reads, 1);
    assert_eq!(
        fs::read(root.join("maintenance-anchors/current.json")).unwrap(),
        current
    );
}
