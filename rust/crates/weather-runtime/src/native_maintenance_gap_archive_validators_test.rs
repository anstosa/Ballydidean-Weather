use super::*;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::native_maintenance_archive_genesis::build_native_maintenance_archive_future_only_genesis;
use crate::native_maintenance_archive_segment::NativeMaintenanceArchiveSegment;
use crate::native_maintenance_cas_pack::{
    NativeMaintenanceCasInventory, NativeMaintenanceCasMember, NativeMaintenanceCasMemberValidator,
    encode_native_maintenance_cas_pack, inspect_native_maintenance_cas_pack,
    inspect_native_maintenance_cas_stream,
};
use crate::native_maintenance_full_graph::{
    NativeMaintenanceFullGraphObjects, verify_native_maintenance_full_graph,
};
use crate::native_maintenance_graph_manifest::{
    NativeMaintenanceGraphEntry, build_native_maintenance_graph_manifest,
};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::native_maintenance_revision_gap_drain::build_native_maintenance_gap_segment;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
const FRONTIER: &str = "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af";
const EPOCH_AT: &str = "2026-10-10T08:00:00.000Z";

/// One disposable source gap-spool fixture root with the oracle's required name grammar.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one owner-private process-unique source fixture root.
    fn create() -> Self {
        let root = std::env::temp_dir().join(format!(
            "weather-native-spool-{}-{}-1",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .expect("create gap fixture root");
        Self(root)
    }
}

impl Drop for FixtureRoot {
    /// Remove only this process-owned disposable source fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Generate and return one actual predecessor classified gap page.
fn source_gap_page(root: &Path) -> Vec<u8> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .arg(
            repository.join("rust/crates/weather-edge/tests/revision-gap-transfer-node-oracle.mjs"),
        )
        .arg("--native-fixture")
        .arg(root)
        .arg("classified")
        .output()
        .expect("run gap source oracle");
    // surface only the source oracle's bounded diagnostic
    if !output.status.success() {
        panic!(
            "gap source oracle failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = String::from_utf8(output.stdout).expect("decode gap source witness");
    let page = output
        .lines()
        .find_map(|line| line.strip_prefix("page_bytes="))
        .expect("source gap page");
    format!("{page}\n").into_bytes()
}

/// Build one exact retained witness and snapshot pair for ancestry validation.
fn genesis_bytes() -> (Vec<u8>, Vec<u8>) {
    let snapshot_sha256 = format_digest(
        Sha256::digest(
            format!("adjustment-revision-serving-snapshot/v1\n{EPOCH_AT}\n0\n{FRONTIER}\n")
                .as_bytes(),
        )
        .into(),
    );
    let snapshot = format!(
        concat!(
            "{{\"archiveCommitOrdinal\":\"0\",",
            "\"contractVersion\":\"adjustment-revision-serving-snapshot/v1\",",
            "\"cutoffAt\":\"{EPOCH_AT}\",\"entries\":[],\"entryCount\":0,",
            "\"frontierSha256\":\"{FRONTIER}\",",
            "\"snapshotSha256\":\"{snapshot_sha256}\"}}\n"
        ),
        EPOCH_AT = EPOCH_AT,
        FRONTIER = FRONTIER,
        snapshot_sha256 = snapshot_sha256,
    );
    let unsigned = format!(
        concat!(
            "{{\"activationKind\":\"inert_v14_pre_activation\",",
            "\"archiveCommitOrdinal\":\"0\",\"catalogFrontierSha256\":\"{FRONTIER}\",",
            "\"contractVersion\":\"adjustment-revision-capture-epoch-witness/v1\",",
            "\"controlPlaneSha256\":\"{}\",\"controlPlaneVersion\":\"14\",",
            "\"databaseMigrationHistorySha256\":",
            "\"6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424\",",
            "\"epochAt\":\"{EPOCH_AT}\",\"servingSnapshotSha256\":\"{snapshot_sha256}\",",
            "\"sourceCommit\":\"{}\",\"sourceRelease\":\"2026.10.10-1\",",
            "\"sourceServerImageDigest\":\"sha256:{}\",",
            "\"sourceWebImageDigest\":\"sha256:{}\"}}\n"
        ),
        "a".repeat(64),
        "1".repeat(40),
        "c".repeat(64),
        "d".repeat(64),
        FRONTIER = FRONTIER,
        EPOCH_AT = EPOCH_AT,
        snapshot_sha256 = snapshot_sha256,
    );
    let witness_sha256 = format_digest(Sha256::digest(unsigned.as_bytes()).into());
    let witness = format!(
        "{},\"witnessSha256\":\"{}\"}}\n",
        unsigned
            .strip_suffix("}\n")
            .expect("unsigned witness suffix"),
        witness_sha256,
    );
    (witness.into_bytes(), snapshot.into_bytes())
}

/// Encode one actual source segment with the mixed-history registry.
fn encode_segment(segment: &NativeMaintenanceArchiveSegment) -> Vec<u8> {
    let members = segment
        .members
        .iter()
        .map(|member| NativeMaintenanceCasMember {
            identity_sha256: member.identity_sha256,
            kind: &member.kind,
            payload: &member.payload,
        })
        .collect();
    encode_native_maintenance_cas_pack(members, &mut NativeMaintenanceGapArchiveValidators::new())
        .expect("encode archive segment")
}

/// Build one canonical graph object over one already inspected segment object.
fn graph_object(
    inventory: &NativeMaintenanceCasInventory,
    segment: &NativeMaintenanceArchiveSegment,
    predecessor: Option<[u8; 32]>,
) -> ([u8; 32], Vec<u8>) {
    let object = format_digest(inventory.object_sha256);
    let entries = inventory
        .members
        .iter()
        .map(|member| NativeMaintenanceGraphEntry {
            identity_sha256: format_digest(member.identity_sha256),
            kind: member.kind.clone(),
            member_length: member.member_length,
            member_offset: member.member_offset,
            member_sha256: format_digest(member.member_sha256),
            object_sha256: object.clone(),
        })
        .collect();
    let manifest = build_native_maintenance_graph_manifest(
        predecessor.map(format_digest),
        entries,
        segment.cross_links.clone(),
    )
    .expect("build archive graph");
    let payload = manifest.canonical_bytes().expect("encode archive graph");
    let bytes = encode_native_maintenance_cas_pack(
        vec![NativeMaintenanceCasMember {
            identity_sha256: Sha256::digest(&payload).into(),
            kind: "adjustment-graph-manifest/v2",
            payload: &payload,
        }],
        &mut NativeMaintenanceGapArchiveValidators::new(),
    )
    .expect("encode archive graph object");
    (Sha256::digest(&bytes).into(), bytes)
}

/// In-memory immutable objects for the actual predecessor-chain verifier.
struct FixtureObjects(HashMap<[u8; 32], Vec<u8>>);

impl NativeMaintenanceFullGraphObjects for FixtureObjects {
    type Reader = Cursor<Vec<u8>>;

    /// Open one actual retained fixture object and its measured size.
    fn open_object(&mut self, digest: [u8; 32]) -> Option<(Self::Reader, u64)> {
        let bytes = self.0.get(&digest)?.clone();
        let size = bytes.len() as u64;
        Some((Cursor::new(bytes), size))
    }
}

/// Validate every actual gap member and a mixed gap-to-genesis graph chain.
#[test]
fn gap_archive_registry_validates_source_segment_and_genesis_ancestry() {
    let fixture = FixtureRoot::create();
    let page = source_gap_page(&fixture.0);
    let gap_segment = build_native_maintenance_gap_segment(&page).expect("build gap segment");
    assert_eq!(gap_segment.members.len(), 4);
    // validate every source-emitted checkpoint, gap, body, and stage receipt independently
    for member in &gap_segment.members {
        assert!(
            NativeMaintenanceGapArchiveValidators::new().validate(&member.kind, &member.payload,)
        );
    }
    let gap_object = encode_segment(&gap_segment);
    let gap_inventory = inspect_native_maintenance_cas_stream(
        gap_object.as_slice(),
        gap_object.len() as u64,
        &mut NativeMaintenanceGapArchiveValidators::new(),
    )
    .expect("stream source gap object");

    let (witness, snapshot) = genesis_bytes();
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    let genesis = build_native_maintenance_archive_future_only_genesis(
        &witness, &snapshot, &mut codec, &time,
    )
    .expect("build genesis ancestry segment");
    let genesis_object = encode_segment(&genesis.segment);
    let genesis_inventory = inspect_native_maintenance_cas_pack(
        &genesis_object,
        &mut NativeMaintenanceGapArchiveValidators::new(),
    )
    .expect("inspect genesis ancestry object");
    let (genesis_graph_digest, genesis_graph) =
        graph_object(&genesis_inventory, &genesis.segment, None);
    let (gap_graph_digest, gap_graph) =
        graph_object(&gap_inventory, &gap_segment, Some(genesis_graph_digest));
    let mut objects = FixtureObjects(HashMap::from([
        (genesis_inventory.object_sha256, genesis_object),
        (genesis_graph_digest, genesis_graph),
        (gap_inventory.object_sha256, gap_object),
        (gap_graph_digest, gap_graph),
    ]));
    let verified = verify_native_maintenance_full_graph(
        &mut objects,
        gap_graph_digest,
        &mut NativeMaintenanceGapArchiveValidators::new(),
        true,
    )
    .expect("verify mixed gap and genesis graph chain");
    assert_eq!(verified.predecessor_count, 1);
    assert_eq!(verified.member_count, 4);
    assert_eq!(verified.object_count, 1);
}
