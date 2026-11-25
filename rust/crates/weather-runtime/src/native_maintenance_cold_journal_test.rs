use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::{
    MaintenanceCanonicalJson, NativeBackendRuntime, NativeMaintenanceJournalStore,
    initialize_native_maintenance_future_only_journal,
};

const GENESIS: &str = "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af";
const NEXT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CUSTODY: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const MEMBER: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PAGE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const START: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const INTENT_AT: &str = "2026-10-10T08:00:02.000Z";
const CURSOR_AT: &str = "2026-10-10T08:00:03.000Z";
const COMPLETE_AT: &str = "2026-10-10T08:00:04.000Z";

const NODE_FIXTURE: &str = r#"
import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { MaintenanceJournal } from "./scripts/research/adjustment_maintenance_state.mjs";
import { canonicalJsonBytes } from "./scripts/research/adjustment_plaintext_archive.mjs";

class MemoryJournalStore {
  constructor() {
    this.bytes = Buffer.alloc(0);
    this.heads = null;
  }

  // retain the source initialization contract
  async initialize() {
    return { rootKind: "home_native_ext4_state" };
  }

  // serialize every source journal transaction
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

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const epochAt = "2026-10-10T08:00:00.000Z";
const genesis = "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af";
const next = "b".repeat(64);
const custody = "c".repeat(64);
const member = "d".repeat(64);
const page = "e".repeat(64);
const start = "f".repeat(64);
const snapshotSha256 = sha256(Buffer.from(
  `adjustment-revision-serving-snapshot/v1\n${epochAt}\n0\n${genesis}\n`, "utf8",
));
const unsignedWitness = {
  activationKind: "inert_v14_pre_activation",
  archiveCommitOrdinal: "0",
  catalogFrontierSha256: genesis,
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
  ...unsignedWitness,
  witnessSha256: sha256(canonicalJsonBytes(unsignedWitness)),
};
const request = {
  afterArchiveCommitOrdinal: "0",
  afterFrontierSha256: genesis,
  custodyCheckpointSha256: custody,
  memberRootSha256: member,
  pageSha256: page,
  previousPageSha256: start,
  startMemberSha256: start,
  startSha256: start,
  watermarkArchiveCommitOrdinal: "1",
  watermarkFrontierSha256: next,
};
const retirementEntries = [
  { fileSha256: "5".repeat(64), identitySha256: "1".repeat(64), kind: "rain_control_state" },
  { fileSha256: "6".repeat(64), identitySha256: "2".repeat(64), kind: "revision_commit_receipt" },
  { fileSha256: "7".repeat(64), identitySha256: "3".repeat(64), kind: "revision_projection" },
  { fileSha256: "8".repeat(64), identitySha256: "4".repeat(64), kind: "revision_stage_receipt" },
];
const unsignedAcknowledgement = {
  acknowledgedAt: "2026-10-10T08:00:04.000Z",
  afterArchiveCommitOrdinal: "0",
  afterFrontierSha256: genesis,
  authority: "cold_custody_only",
  contractVersion: "adjustment-revision-custody-acknowledgement/v2",
  custodyCheckpointSha256: custody,
  memberRootSha256: member,
  nextArchiveCommitOrdinal: "1",
  nextFrontierSha256: next,
  pageSha256: page,
  previousAcknowledgementSha256: null,
  previousPageSha256: start,
  retirementEntries,
  startMemberSha256: start,
  startSha256: start,
  watermarkArchiveCommitOrdinal: "1",
  watermarkFrontierSha256: next,
};
const acknowledgement = {
  ...unsignedAcknowledgement,
  acknowledgementSha256: sha256(canonicalJsonBytes(unsignedAcknowledgement)),
};
const store = new MemoryJournalStore();
const journal = new MaintenanceJournal(store);
await journal.initialize();
await journal.initializeFutureOnlyLifecycle({
  now: "2026-10-10T08:00:01.000Z",
  witness,
});
await journal.recordRevisionCustodyIntent({
  nextArchiveCommitOrdinal: "1",
  nextFrontierSha256: next,
  now: "2026-10-10T08:00:02.000Z",
  request,
});
await journal.advanceRevisionCursor({
  afterArchiveCommitOrdinal: "0",
  afterFrontierSha256: genesis,
  custodyCheckpointSha256: custody,
  nextArchiveCommitOrdinal: "1",
  nextFrontierSha256: next,
  now: "2026-10-10T08:00:03.000Z",
  pageSha256: page,
  startSha256: start,
});
await journal.completeRevisionCustodyV2({
  acknowledgement,
  now: "2026-10-10T08:00:04.000Z",
});
writeFileSync(join(process.env.WEATHER_COLD_JOURNAL_ROOT, "witness.json"),
  canonicalJsonBytes(witness));
writeFileSync(join(process.env.WEATHER_COLD_JOURNAL_ROOT, "ack.json"),
  canonicalJsonBytes(acknowledgement));
writeFileSync(join(process.env.WEATHER_COLD_JOURNAL_ROOT, "journal.expected"), store.bytes);
"#;

/// One disposable owner-private journal root.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique private fixture directory.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-cold-journal-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("protect fixture root");
        Self(root)
    }

    /// Read one source-generated fixture member.
    fn read(&self, name: &str) -> Vec<u8> {
        fs::read(self.0.join(name)).expect("read fixture member")
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-owned fixture root.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Generate the exact source journal, witness, and custody acknowledgement.
fn build_node_fixture(root: &Path) {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .current_dir(repository)
        .env("WEATHER_COLD_JOURNAL_ROOT", root)
        .output()
        .expect("run predecessor cold journal fixture");
    // surface only a bounded predecessor diagnostic
    if !output.status.success() {
        panic!(
            "predecessor cold journal fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn native_cold_journal_matches_source_intent_cursor_and_completion() {
    let fixture = FixtureRoot::create();
    build_node_fixture(&fixture.0);
    let witness = fixture.read("witness.json");
    let acknowledgement_bytes = fixture.read("ack.json");
    let expected = fixture.read("journal.expected");
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    let (mut store, _) =
        NativeMaintenanceJournalStore::initialize_fixture(fixture.0.clone(), &mut codec, &time)
            .expect("initialize native journal");
    initialize_native_maintenance_future_only_journal(
        &mut store,
        "2026-10-10T08:00:01.000Z",
        &witness,
        &mut codec,
        &time,
    )
    .expect("append future-only genesis");
    let request = custody_request();
    let intent = NativeRevisionColdCustodyIntent {
        next_archive_commit_ordinal: "1".to_owned(),
        next_frontier_sha256: NEXT.to_owned(),
        request: request.clone(),
    };
    let advance = NativeRevisionColdCursorAdvance {
        after_archive_commit_ordinal: "0".to_owned(),
        after_frontier_sha256: GENESIS.to_owned(),
        custody_checkpoint_sha256: CUSTODY.to_owned(),
        next_archive_commit_ordinal: "1".to_owned(),
        next_frontier_sha256: NEXT.to_owned(),
        now: CURSOR_AT.to_owned(),
        page_sha256: PAGE.to_owned(),
        start_sha256: START.to_owned(),
    };
    let acknowledgement_value = codec
        .parse_canonical_object(&acknowledgement_bytes)
        .expect("parse source acknowledgement");
    let acknowledgement = VerifiedNativeRevisionColdCustodyAcknowledgementV2 {
        canonical_bytes: acknowledgement_bytes,
        next_archive_commit_ordinal: "1".to_owned(),
        next_frontier_sha256: NEXT.to_owned(),
        request,
        value: acknowledgement_value,
    };
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let mut journal = NativeMaintenanceColdJournal::new(&mut store);
        assert_eq!(
            journal
                .read_revision_cursor()
                .await
                .expect("genesis cursor"),
            NativeRevisionColdCursor {
                archive_commit_ordinal: "0".to_owned(),
                frontier_sha256: GENESIS.to_owned(),
            }
        );
        journal
            .record_revision_custody_intent(&intent, INTENT_AT)
            .await
            .expect("record intent");
        assert_eq!(
            journal
                .read_pending_revision_custody()
                .await
                .expect("read pending"),
            Some(intent.clone())
        );
        assert_eq!(
            journal
                .advance_revision_cursor(&advance)
                .await
                .expect("advance cursor"),
            NativeRevisionColdCursor {
                archive_commit_ordinal: "1".to_owned(),
                frontier_sha256: NEXT.to_owned(),
            }
        );
        journal
            .complete_revision_custody_v2(&acknowledgement, COMPLETE_AT)
            .await
            .expect("complete custody");
        assert_eq!(
            journal
                .read_pending_revision_custody()
                .await
                .expect("read completed custody"),
            None
        );
    });
    assert_eq!(
        fs::read(fixture.0.join("journal.jsonl")).expect("read native journal"),
        expected
    );
}

/// Build the source's exact packed custody request projection.
fn custody_request() -> NativeRevisionColdCustodyRequest {
    NativeRevisionColdCustodyRequest {
        page: NativeRevisionColdPageRequest {
            after_archive_commit_ordinal: "0".to_owned(),
            after_frontier_sha256: GENESIS.to_owned(),
            previous_page_sha256: START.to_owned(),
            start_sha256: START.to_owned(),
            watermark_archive_commit_ordinal: "1".to_owned(),
            watermark_frontier_sha256: NEXT.to_owned(),
        },
        custody_checkpoint_sha256: CUSTODY.to_owned(),
        member_root_sha256: MEMBER.to_owned(),
        page_sha256: PAGE.to_owned(),
        start_member_sha256: START.to_owned(),
    }
}
