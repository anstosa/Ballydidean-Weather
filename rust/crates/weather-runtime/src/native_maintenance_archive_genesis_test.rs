use super::*;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::native_maintenance_cas_pack::{
    NativeMaintenanceCasMember, encode_native_maintenance_cas_pack,
    inspect_native_maintenance_cas_pack, inspect_native_maintenance_cas_stream,
};
use crate::native_maintenance_full_graph::{
    NativeMaintenanceFullGraphObjects, verify_native_maintenance_full_graph,
};
use crate::native_maintenance_graph_manifest::{
    NativeMaintenanceGraphEntry, build_native_maintenance_graph_manifest,
};
use crate::native_maintenance_retirement_files_v2::format_digest;

const NODE_FIXTURE: &str = r#"
import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  buildAdjustmentFutureOnlyGenesisGraphSegment,
} from "./scripts/research/adjustment_maintenance_controller.mjs";
import {
  canonicalJsonBytes,
  encodeCasPack,
} from "./scripts/research/adjustment_plaintext_archive.mjs";

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const epochAt = "2026-10-10T08:00:00.000Z";
const frontier = "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af";
const snapshotSha256 = sha256(Buffer.from(
  `adjustment-revision-serving-snapshot/v1\n${epochAt}\n0\n${frontier}\n`, "utf8",
));
const servingSnapshot = {
  archiveCommitOrdinal: "0",
  contractVersion: "adjustment-revision-serving-snapshot/v1",
  cutoffAt: epochAt,
  entries: [],
  entryCount: 0,
  frontierSha256: frontier,
  snapshotSha256,
};
const unsigned = {
  activationKind: "inert_v14_pre_activation",
  archiveCommitOrdinal: "0",
  catalogFrontierSha256: frontier,
  contractVersion: "adjustment-revision-capture-epoch-witness/v1",
  controlPlaneSha256: "a".repeat(64),
  controlPlaneVersion: "14",
  databaseMigrationHistorySha256:
    "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
  epochAt,
  servingSnapshotSha256: snapshotSha256,
  sourceCommit: "1".repeat(40),
  sourceRelease: "2026.10.10-1",
  sourceServerImageDigest: `sha256:${"c".repeat(64)}`,
  sourceWebImageDigest: `sha256:${"d".repeat(64)}`,
};
const witness = {
  ...unsigned,
  witnessSha256: sha256(canonicalJsonBytes(unsigned)),
};
const segment = buildAdjustmentFutureOnlyGenesisGraphSegment({ servingSnapshot, witness });
const admit = () => undefined;
admit.maximumBytes = 16 * 1024;
const validators = new Map([
  [witness.contractVersion, admit],
  [servingSnapshot.contractVersion, admit],
]);
writeFileSync(join(process.env.WEATHER_GENESIS_FIXTURE_ROOT, "witness.json"),
  canonicalJsonBytes(witness));
writeFileSync(join(process.env.WEATHER_GENESIS_FIXTURE_ROOT, "snapshot.json"),
  canonicalJsonBytes(servingSnapshot));
writeFileSync(join(process.env.WEATHER_GENESIS_FIXTURE_ROOT, "members.cas"),
  encodeCasPack(segment.members, { validators }));
"#;

/// One disposable source-generated future-only genesis fixture root.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique bounded fixture directory.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-native-archive-genesis-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        Self(root)
    }

    /// Read one actual source-generated fixture artifact.
    fn read(&self, name: &str) -> Vec<u8> {
        fs::read(self.0.join(name)).expect("read source fixture")
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-owned disposable fixture root.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Generate exact source segment bytes through the predecessor builder and CAS encoder.
fn build_node_fixture(root: &Path) {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .current_dir(repository)
        .env("WEATHER_GENESIS_FIXTURE_ROOT", root)
        .output()
        .expect("run predecessor genesis fixture");
    // surface only the bounded predecessor fixture diagnostic
    if !output.status.success() {
        panic!(
            "predecessor genesis fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// In-memory immutable object owner used by the actual full-graph verifier.
struct FixtureObjects(HashMap<[u8; 32], Vec<u8>>);

impl NativeMaintenanceFullGraphObjects for FixtureObjects {
    type Reader = Cursor<Vec<u8>>;

    /// Open one exact retained fixture object and its actual byte length.
    fn open_object(&mut self, digest: [u8; 32]) -> Option<(Self::Reader, u64)> {
        let bytes = self.0.get(&digest)?.clone();
        let size = bytes.len() as u64;
        Some((Cursor::new(bytes), size))
    }
}

/// Match source member bytes and verify the complete native genesis graph.
#[test]
fn native_future_only_genesis_matches_source_and_verifies_full_graph() {
    let fixture = FixtureRoot::create();
    build_node_fixture(&fixture.0);
    let witness_bytes = fixture.read("witness.json");
    let snapshot_bytes = fixture.read("snapshot.json");
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    let genesis = build_native_maintenance_archive_future_only_genesis(
        &witness_bytes,
        &snapshot_bytes,
        &mut codec,
        &time,
    )
    .expect("build native genesis segment");
    assert_eq!(genesis.segment.members.len(), 2);
    assert_eq!(genesis.segment.members[0].payload, witness_bytes);
    assert_eq!(genesis.segment.members[1].payload, snapshot_bytes);
    assert_eq!(genesis.segment.cross_links.len(), 1);
    assert_eq!(
        genesis.segment.cross_links[0].relation,
        "binds_zero_frontier_snapshot"
    );
    assert_eq!(
        genesis.due_key,
        format!(
            "archive/future-only-genesis/{}",
            format_digest(genesis.segment.members[0].identity_sha256)
        )
    );

    let members = genesis
        .segment
        .members
        .iter()
        .map(|member| NativeMaintenanceCasMember {
            identity_sha256: member.identity_sha256,
            kind: &member.kind,
            payload: &member.payload,
        })
        .collect();
    let data_object = encode_native_maintenance_cas_pack(
        members,
        &mut NativeMaintenanceArchiveGenesisValidators::new(),
    )
    .expect("encode native genesis members");
    assert_eq!(data_object, fixture.read("members.cas"));
    let inventory = inspect_native_maintenance_cas_pack(
        &data_object,
        &mut NativeMaintenanceArchiveGenesisValidators::new(),
    )
    .expect("inspect native genesis members");
    let streamed = inspect_native_maintenance_cas_stream(
        data_object.as_slice(),
        data_object.len() as u64,
        &mut NativeMaintenanceArchiveGenesisValidators::new(),
    )
    .expect("stream native genesis members");
    assert_eq!(inventory, streamed);

    let object_digest = inventory.object_sha256;
    let entries = inventory
        .members
        .iter()
        .map(|member| NativeMaintenanceGraphEntry {
            identity_sha256: format_digest(member.identity_sha256),
            kind: member.kind.clone(),
            member_length: member.member_length,
            member_offset: member.member_offset,
            member_sha256: format_digest(member.member_sha256),
            object_sha256: format_digest(object_digest),
        })
        .collect();
    let manifest =
        build_native_maintenance_graph_manifest(None, entries, genesis.segment.cross_links)
            .expect("build native genesis graph");
    let manifest_payload = manifest.canonical_bytes().expect("encode genesis graph");
    let graph_object = encode_native_maintenance_cas_pack(
        vec![NativeMaintenanceCasMember {
            identity_sha256: Sha256::digest(&manifest_payload).into(),
            kind: "adjustment-graph-manifest/v2",
            payload: &manifest_payload,
        }],
        &mut NativeMaintenanceArchiveGenesisValidators::new(),
    )
    .expect("encode genesis graph object");
    let graph_digest: [u8; 32] = Sha256::digest(&graph_object).into();
    let mut objects = FixtureObjects(HashMap::from([
        (object_digest, data_object),
        (graph_digest, graph_object),
    ]));
    let verified = verify_native_maintenance_full_graph(
        &mut objects,
        graph_digest,
        &mut NativeMaintenanceArchiveGenesisValidators::new(),
        true,
    )
    .expect("verify native genesis full graph");
    assert_eq!(verified.member_count, 2);
    assert_eq!(verified.object_count, 1);
    assert_eq!(verified.predecessor_count, 0);
    assert_eq!(verified.manifest.cross_links.len(), 1);
}
