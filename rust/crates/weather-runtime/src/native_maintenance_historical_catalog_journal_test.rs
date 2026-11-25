use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_contracts::JsonValue;

use super::{
    NativeMaintenanceHistoricalCatalogJournal, NativeMaintenanceHistoricalCatalogJournalError,
    project_native_maintenance_historical_catalog_journal,
};
use crate::{NativeMaintenanceJournalRecord, NativeMaintenanceJournalStore, NativeWirePrimitives};

/// Compare actual verified-store cursor and mapping projection with the source journal.
#[test]
fn verified_store_projection_matches_source_legacy_pack_and_pending_order() {
    let fixture = FixtureRoot::create();
    build_node_fixture(&fixture.0);
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    let (mut store, _) =
        NativeMaintenanceJournalStore::initialize_fixture(fixture.0.clone(), &mut codec, &time)
            .expect("initialize verified fixture journal");
    let state = NativeMaintenanceHistoricalCatalogJournal::new(&mut store)
        .read_state(&mut codec, &time)
        .expect("project historical journal");
    let expected: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.0.join("projection.expected")).expect("read source projection"),
    )
    .expect("decode source projection");
    let cursor = state.cursor.expect("source fixture cursor");

    assert_eq!(
        cursor.archive_commit_ordinal,
        expected["cursor"]["archiveCommitOrdinal"]
            .as_str()
            .expect("source cursor ordinal")
    );
    assert_eq!(
        cursor.frontier_sha256,
        expected["cursor"]["frontierSha256"]
            .as_str()
            .expect("source cursor frontier")
    );
    let mappings = expected["mappings"].as_array().expect("source mappings");
    assert_eq!(state.mappings.len(), mappings.len());
    // preserve legacy-first then sealed-pack page order exactly
    for (actual, expected) in state.mappings.iter().zip(mappings) {
        assert_eq!(
            actual.graph_manifest_sha256,
            expected["graphManifestSha256"].as_str().unwrap()
        );
        assert_eq!(actual.page_sha256, expected["pageSha256"].as_str().unwrap());
        assert_eq!(
            actual.start_sha256,
            expected["startSha256"].as_str().unwrap()
        );
    }
    assert_eq!(state.mappings.len(), 3);
}

/// Reject duplicate page mappings even when their generic journal rows are hash-chain admitted.
#[test]
fn pure_projection_refuses_duplicate_legacy_page_mapping() {
    let page = hash('a');
    let records = vec![legacy_record(1, &page), legacy_record(2, &page)];

    assert_eq!(
        project_native_maintenance_historical_catalog_journal(&records),
        Err(NativeMaintenanceHistoricalCatalogJournalError::State)
    );
}

/// Remove one bounded process-owned disposable journal root.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one private literal fixture directory.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-native-historical-journal-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("protect fixture root");
        Self(root)
    }
}

impl Drop for FixtureRoot {
    /// Remove only this process-owned disposable fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Generate exact source journal bytes and its projected cursor and mappings.
fn build_node_fixture(root: &Path) {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .current_dir(repository)
        .env("WEATHER_JOURNAL_FIXTURE_ROOT", root)
        .output()
        .expect("run source journal fixture");
    assert!(
        output.status.success(),
        "source journal fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Build one generic verified-row projection fixture.
fn legacy_record(generation: u64, page_sha256: &str) -> NativeMaintenanceJournalRecord {
    let payload = object(vec![
        ("afterArchiveCommitOrdinal", string("0")),
        ("afterFrontierSha256", string(&hash('b'))),
        ("graphManifestSha256", string(&hash('c'))),
        ("nextArchiveCommitOrdinal", string("1")),
        ("nextFrontierSha256", string(&hash('d'))),
        ("pageSha256", string(page_sha256)),
        ("startSha256", string(&hash('e'))),
    ]);
    NativeMaintenanceJournalRecord {
        generation,
        kind: string("revision_cursor_advanced"),
        record_key: format!("revision/cursor/{generation}"),
        due_key: None,
        recorded_at: "2026-10-10T08:00:00.000Z".to_owned(),
        previous_sha256: None,
        payload_sha256: hash('f'),
        payload,
        record_sha256: hash('1'),
        value: object(Vec::new()),
    }
}

/// Construct one source JSON object fixture.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Construct one source JSON string fixture.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Produce one digest-shaped fixture string.
fn hash(character: char) -> String {
    character.to_string().repeat(64)
}

const NODE_FIXTURE: &str = r#"
import { createHash } from "node:crypto";
import { chmodSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND,
  MaintenanceJournal,
} from "./scripts/research/adjustment_maintenance_state.mjs";
import { canonicalJsonBytes } from "./scripts/research/adjustment_plaintext_archive.mjs";

const sha = (value) => createHash("sha256").update(value).digest("hex");
const H = Object.fromEntries([
  "graph1", "graphPack", "member1", "member2", "member3", "member4",
  "page1", "page2", "page3", "page4", "start1", "start2", "start4",
  "startMember1", "startMember2", "startMember4", "checkpoint2", "checkpoint3",
  "checkpoint4", "frontier1", "frontier2", "frontier3", "frontier4",
].map((name) => [name, sha(name)]));

class MemoryJournalStore {
  constructor() {
    this.bytes = Buffer.alloc(0);
    this.heads = null;
  }
  // return the exact source root identity
  async initialize() {
    return { rootKind: ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND };
  }
  // execute one serialized source transaction
  async withLock(callback) {
    return await callback({
      readJournal: async () => Buffer.from(this.bytes),
      append: async (bytes, heads) => {
        this.bytes = Buffer.concat([this.bytes, bytes]);
        this.heads = structuredClone(heads);
      },
      writeHeads: async (heads) => {
        this.heads = structuredClone(heads);
      },
    });
  }
}

const store = new MemoryJournalStore();
const journal = new MaintenanceJournal(store);
await journal.initialize();
const genesis = sha("adjustment-revision-frontier/v1\n0\n");
const witnessUnsigned = {
  activationKind: "inert_v14_pre_activation",
  archiveCommitOrdinal: "0",
  catalogFrontierSha256: genesis,
  contractVersion: "adjustment-revision-capture-epoch-witness/v1",
  controlPlaneSha256: sha("control"),
  controlPlaneVersion: "14",
  databaseMigrationHistorySha256:
    "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
  epochAt: "2026-10-10T08:00:00.000Z",
  servingSnapshotSha256: sha("snapshot"),
  sourceCommit: "1".repeat(40),
  sourceRelease: "2026.10.10-1",
  sourceServerImageDigest: `sha256:${sha("server")}`,
  sourceWebImageDigest: `sha256:${sha("web")}`,
};
await journal.initializeFutureOnlyLifecycle({
  now: "2026-10-10T08:00:00.100Z",
  witness: {
    ...witnessUnsigned,
    witnessSha256: sha(`${JSON.stringify(witnessUnsigned)}\n`),
  },
});

// publish one exact due output through its required lease lifecycle
async function publish(dueKey, outputSha256, suffix, second) {
  const inputHeadSha256 = (await journal.status()).headSha256;
  const base = `2026-10-10T08:00:${String(second).padStart(2, "0")}`;
  await journal.acquireLease({
    dueKey,
    inputHeadSha256,
    now: `${base}.100Z`,
    runId: `archive-${suffix}`,
    scope: "archive",
  });
  await journal.completeDue({ dueKey, now: `${base}.200Z`, outputSha256 });
  await journal.releaseLease({
    dueKey,
    now: `${base}.300Z`,
    runId: `archive-${suffix}`,
    scope: "archive",
  });
}

// retain one legacy individual graph mapping
await publish(`archive/revision-cold-page/${H.page1}`, H.graph1, "legacy", 1);
const legacyRequest = {
  afterArchiveCommitOrdinal: "0",
  afterFrontierSha256: genesis,
  graphManifestSha256: H.graph1,
  memberRootSha256: H.member1,
  pageSha256: H.page1,
  previousPageSha256: H.start1,
  startMemberSha256: H.startMember1,
  startSha256: H.start1,
  watermarkArchiveCommitOrdinal: "1",
  watermarkFrontierSha256: H.frontier1,
};
await journal.recordRevisionCustodyIntent({
  nextArchiveCommitOrdinal: "1",
  nextFrontierSha256: H.frontier1,
  now: "2026-10-10T08:00:02.100Z",
  request: legacyRequest,
});
await journal.advanceRevisionCursor({
  afterArchiveCommitOrdinal: "0",
  afterFrontierSha256: genesis,
  graphManifestSha256: H.graph1,
  nextArchiveCommitOrdinal: "1",
  nextFrontierSha256: H.frontier1,
  now: "2026-10-10T08:00:02.200Z",
  pageSha256: H.page1,
  startSha256: H.start1,
});
await journal.completeRevisionCustody({
  acknowledgementSha256: sha("legacy-ack"),
  now: "2026-10-10T08:00:02.300Z",
  pageSha256: H.page1,
});

let previousAcknowledgementSha256 = null;
// append and complete one packed page while retaining its exact request
async function packedPage(index, afterFrontierSha256, nextFrontierSha256,
  checkpointSha256, pageSha256, startSha256, startMemberSha256) {
  const after = String(index - 1);
  const next = String(index);
  const request = {
    afterArchiveCommitOrdinal: after,
    afterFrontierSha256,
    custodyCheckpointSha256: checkpointSha256,
    memberRootSha256: H[`member${index}`],
    pageSha256,
    previousPageSha256: index === 2 ? startSha256 : H[`page${index - 1}`],
    startMemberSha256,
    startSha256,
    watermarkArchiveCommitOrdinal: next,
    watermarkFrontierSha256: nextFrontierSha256,
  };
  await journal.recordRevisionCustodyIntent({
    nextArchiveCommitOrdinal: next,
    nextFrontierSha256,
    now: `2026-10-10T08:00:0${index}.400Z`,
    request,
  });
  await journal.advanceRevisionCursor({
    afterArchiveCommitOrdinal: after,
    afterFrontierSha256,
    custodyCheckpointSha256: checkpointSha256,
    nextArchiveCommitOrdinal: next,
    nextFrontierSha256,
    now: `2026-10-10T08:00:0${index}.500Z`,
    pageSha256,
    startSha256,
  });
  const retirementEntries = ["revision_commit_receipt", "revision_frontier_successor",
    "revision_projection", "revision_stage_receipt"].map((kind, offset) => ({
      fileSha256: sha(`file-${index}-${offset}`),
      identitySha256: sha(`identity-${index}-${offset}`),
      kind,
    }));
  const unsigned = {
    acknowledgedAt: `2026-10-10T08:00:0${index}.600Z`,
    afterArchiveCommitOrdinal: request.afterArchiveCommitOrdinal,
    afterFrontierSha256: request.afterFrontierSha256,
    authority: "cold_custody_only",
    contractVersion: "adjustment-revision-custody-acknowledgement/v2",
    custodyCheckpointSha256: request.custodyCheckpointSha256,
    memberRootSha256: request.memberRootSha256,
    nextArchiveCommitOrdinal: next,
    nextFrontierSha256,
    pageSha256,
    previousAcknowledgementSha256,
    previousPageSha256: request.previousPageSha256,
    retirementEntries,
    startMemberSha256,
    startSha256,
    watermarkArchiveCommitOrdinal: request.watermarkArchiveCommitOrdinal,
    watermarkFrontierSha256: request.watermarkFrontierSha256,
  };
  const acknowledgement = {
    ...unsigned,
    acknowledgementSha256: sha(canonicalJsonBytes(unsigned)),
  };
  await journal.completeRevisionCustodyV2({
    acknowledgement,
    now: `2026-10-10T08:00:0${index}.700Z`,
  });
  previousAcknowledgementSha256 = acknowledgement.acknowledgementSha256;
  return request;
}

const request2 = await packedPage(2, H.frontier1, H.frontier2,
  H.checkpoint2, H.page2, H.start2, H.startMember2);
const request3 = await packedPage(3, H.frontier2, H.frontier3,
  H.checkpoint3, H.page3, H.start2, null);
await publish(`archive/revision-custody-pack/${H.checkpoint3}`, H.graphPack, "pack", 10);
await journal.recordRevisionCustodyPackSeal({
  checkpointSha256: H.checkpoint3,
  graphManifestSha256: H.graphPack,
  now: "2026-10-10T08:00:11.100Z",
  pages: [request2, request3].map((request) => ({
    memberRootSha256: request.memberRootSha256,
    pageSha256: request.pageSha256,
    startMemberSha256: request.startMemberSha256,
    startSha256: request.startSha256,
  })),
});

// leave the newest packed cursor pending and therefore absent from catalog mappings
const pendingRequest = {
  afterArchiveCommitOrdinal: "3",
  afterFrontierSha256: H.frontier3,
  custodyCheckpointSha256: H.checkpoint4,
  memberRootSha256: H.member4,
  pageSha256: H.page4,
  previousPageSha256: H.start4,
  startMemberSha256: H.startMember4,
  startSha256: H.start4,
  watermarkArchiveCommitOrdinal: "4",
  watermarkFrontierSha256: H.frontier4,
};
await journal.recordRevisionCustodyIntent({
  nextArchiveCommitOrdinal: "4",
  nextFrontierSha256: H.frontier4,
  now: "2026-10-10T08:00:11.200Z",
  request: pendingRequest,
});
await journal.advanceRevisionCursor({
  afterArchiveCommitOrdinal: "3",
  afterFrontierSha256: H.frontier3,
  custodyCheckpointSha256: H.checkpoint4,
  nextArchiveCommitOrdinal: "4",
  nextFrontierSha256: H.frontier4,
  now: "2026-10-10T08:00:11.300Z",
  pageSha256: H.page4,
  startSha256: H.start4,
});

const projection = {
  cursor: await journal.readRevisionCursor(),
  mappings: await journal.listRevisionCatalogPages(),
};
const root = process.env.WEATHER_JOURNAL_FIXTURE_ROOT;
writeFileSync(join(root, "journal.jsonl"), store.bytes);
chmodSync(join(root, "journal.jsonl"), 0o600);
writeFileSync(join(root, "projection.expected"), canonicalJsonBytes(projection));
"#;
