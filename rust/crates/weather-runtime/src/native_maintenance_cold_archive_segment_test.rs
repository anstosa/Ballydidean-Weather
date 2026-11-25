use super::*;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::native_maintenance_cas_pack::{
    NativeMaintenanceCasMember, NativeMaintenanceCasMemberValidator,
    encode_native_maintenance_cas_pack,
};
use crate::native_maintenance_cold_remote_documents::verify_native_revision_cold_start_document;
use crate::native_maintenance_cold_remote_page::verify_native_maintenance_cold_remote_page;
use crate::native_maintenance_revision_cold_drain::NativeRevisionColdPageRequest;

const NODE_FIXTURE: &str = r#"
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  buildAdjustmentRevisionColdGraphSegment,
} from "./deploy/scripts/adjustment-evidence-store.mjs";
import {
  canonicalJsonBytes,
  encodeCasPack,
} from "./scripts/research/adjustment_plaintext_archive.mjs";

const root=process.env.WEATHER_COLD_SEGMENT_FIXTURE_ROOT;
const sha256=(bytes)=>createHash("sha256").update(bytes).digest("hex");
const page=JSON.parse(readFileSync(join(root,"page.json"),"utf8"));
const snapshotEntries=[];
// retain the fixture's actual receipt order in the reconstructed serving snapshot
for (const entry of page.entries) {
  const payload=JSON.parse(Buffer.from(entry.payload.bytesBase64,"base64").toString("utf8"));
  const receipts=entry.receipts ?? [entry.receipt];
  // preserve every grouped receipt as a distinct snapshot entry
  for (const receipt of receipts) {
    snapshotEntries.push({
      logicalReceivedAt:payload.logicalReceivedAt,
      receipt,
      relation:"weather_records",
    });
  }
}
const snapshot={
  archiveCommitOrdinal:page.watermarkArchiveCommitOrdinal,
  contractVersion:"adjustment-revision-serving-snapshot/v1",
  cutoffAt:"2026-10-08T02:00:00.000Z",
  entries:snapshotEntries,
  entryCount:snapshotEntries.length,
  frontierSha256:page.watermarkFrontierSha256,
  snapshotSha256:"",
};
snapshot.snapshotSha256=sha256(Buffer.from([
  snapshot.contractVersion,snapshot.cutoffAt,snapshot.archiveCommitOrdinal,
  snapshot.frontierSha256,snapshotEntries.map(entry=>entry.receipt.receiptSha256).join("\n"),
].join("\n")));
const startUnsigned={
  contractVersion:"adjustment-revision-cold-transfer-start/v1",
  servingSnapshot:snapshot,
  watermarkArchiveCommitOrdinal:page.watermarkArchiveCommitOrdinal,
  watermarkFrontierSha256:page.watermarkFrontierSha256,
};
const start={...startUnsigned,startSha256:sha256(canonicalJsonBytes(startUnsigned))};
// require reconstruction to bind the actual source-produced first page
if (start.startSha256!==page.startSha256) throw new Error("reconstructed start differs");
const segment=buildAdjustmentRevisionColdGraphSegment({page,start});
const admit=()=>undefined;
admit.maximumBytes=5*1024*1024;
const validators=new Map(segment.members.map(member=>[member.kind,admit]));
writeFileSync(join(root,"start.json"),canonicalJsonBytes(start));
writeFileSync(join(root,"segment.cas"),encodeCasPack(segment.members,{validators}));
writeFileSync(join(root,"cross-links.json"),canonicalJsonBytes(segment.crossLinks));
"#;

/// One process-owned bounded predecessor fixture root.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one isolated disposable fixture directory.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-native-cold-segment-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        Self(root)
    }

    /// Read one source-produced fixture artifact.
    fn read(&self, name: &str) -> Vec<u8> {
        fs::read(self.0.join(name)).expect("read fixture artifact")
    }
}

impl Drop for FixtureRoot {
    /// Remove only this process-owned disposable fixture directory.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Accept only test members already admitted by the production cold-page decoder.
struct FixtureMemberValidator;

impl NativeMaintenanceCasMemberValidator for FixtureMemberValidator {
    /// Preserve exact member bytes while the test compares the source CAS encoding.
    fn validate(&mut self, _: &str, _: &[u8]) -> bool {
        true
    }
}

/// Materialize the original mixed page and build its source graph segment.
fn build_node_fixture(root: &Path) {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let oracle =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native-cold-custody-node-oracle.mjs");
    let page = Command::new("node")
        .arg(oracle)
        .arg("fixtures")
        .arg(root)
        .arg("mixed")
        .current_dir(&repository)
        .output()
        .expect("run predecessor cold-page fixture");
    // surface only the bounded source fixture diagnostic
    if !page.status.success() {
        panic!(
            "predecessor cold-page fixture failed: {}",
            String::from_utf8_lossy(&page.stderr)
        );
    }
    let segment = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .current_dir(repository)
        .env("WEATHER_COLD_SEGMENT_FIXTURE_ROOT", root)
        .output()
        .expect("run predecessor cold-segment fixture");
    // retain the original source error if graph composition diverges
    if !segment.status.success() {
        panic!(
            "predecessor cold-segment fixture failed: {}",
            String::from_utf8_lossy(&segment.stderr)
        );
    }
}

/// Decode the source request operands without inventing fixture cursor values.
fn fixture_request(root: &Path) -> NativeRevisionColdPageRequest {
    let operands = fs::read_to_string(root.join("operands.txt")).expect("read fixture operands");
    let values: Vec<_> = operands.lines().collect();
    NativeRevisionColdPageRequest {
        watermark_archive_commit_ordinal: values[0].to_owned(),
        watermark_frontier_sha256: values[1].to_owned(),
        start_sha256: values[2].to_owned(),
        after_archive_commit_ordinal: values[3].to_owned(),
        after_frontier_sha256: values[4].to_owned(),
        previous_page_sha256: values[5].to_owned(),
    }
}

/// Encode the native crosslinks through the same recursively sorted JSON primitive.
fn cross_link_bytes(links: &[NativeMaintenanceGraphCrossLink]) -> Vec<u8> {
    let values = links
        .iter()
        .map(|link| {
            object(vec![
                ("fromIdentitySha256", string(&link.from_identity_sha256)),
                ("relation", string(&link.relation)),
                ("toIdentitySha256", string(&link.to_identity_sha256)),
            ])
        })
        .collect();
    NativeWirePrimitives
        .encode_sorted_json(&JsonValue::Array(values))
        .expect("encode native links")
}

/// Match the source checkpoint, member order, raw bytes, and graph relations end to end.
#[test]
fn native_cold_archive_segment_matches_source_graph_and_cas_bytes() {
    let fixture = FixtureRoot::create();
    build_node_fixture(&fixture.0);
    let request = fixture_request(&fixture.0);
    let page_bytes = fixture.read("page.json");
    let page = verify_native_maintenance_cold_remote_page(&page_bytes, &request)
        .expect("verify source cold page");
    let start_bytes = fixture.read("start.json");
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    let start = verify_native_revision_cold_start_document(&start_bytes, &mut codec, &time)
        .expect("verify source cold start");

    let built = build_native_maintenance_cold_archive_segment(&page, Some(&start))
        .expect("build native cold segment");
    assert_eq!(
        built.due_key,
        format!("archive/revision-cold-page/{}", page.page_sha256)
    );
    assert!(built.start_member_sha256.is_some());
    assert_eq!(
        built.segment.members[0].identity_sha256,
        parse_digest(&page.page_sha256).unwrap()
    );
    assert_eq!(built.segment.members[1].payload, start_bytes);

    let members = built
        .segment
        .members
        .iter()
        .map(|member| NativeMaintenanceCasMember {
            identity_sha256: member.identity_sha256,
            kind: &member.kind,
            payload: &member.payload,
        })
        .collect();
    let cas = encode_native_maintenance_cas_pack(members, &mut FixtureMemberValidator)
        .expect("encode native cold segment");
    assert_eq!(cas, fixture.read("segment.cas"));
    assert_eq!(
        cross_link_bytes(&built.segment.cross_links),
        fixture.read("cross-links.json")
    );
}

/// Enforce the source first-page start-member boundary before any segment is returned.
#[test]
fn native_cold_archive_segment_requires_start_only_on_first_page() {
    let fixture = FixtureRoot::create();
    build_node_fixture(&fixture.0);
    let request = fixture_request(&fixture.0);
    let page = verify_native_maintenance_cold_remote_page(&fixture.read("page.json"), &request)
        .expect("verify source cold page");
    assert_eq!(
        build_native_maintenance_cold_archive_segment(&page, None),
        Err(NativeMaintenanceColdArchiveSegmentError::Start)
    );
}
