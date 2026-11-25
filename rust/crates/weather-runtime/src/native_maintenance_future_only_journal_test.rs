use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_contracts::JsonValue;

use super::*;
use crate::{NativeMaintenanceJournalAddition, NativeWirePrimitives};

const NOW: &str = "2026-10-10T08:00:01.000Z";

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

  // retain the source initialization surface
  async initialize() {
    return { rootKind: "home_native_ext4_state" };
  }

  // serialize every source mutation over one in-memory byte chain
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
const frontier = "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af";
const snapshotSha256 = sha256(Buffer.from(
  `adjustment-revision-serving-snapshot/v1\n${epochAt}\n0\n${frontier}\n`, "utf8",
));
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
const store = new MemoryJournalStore();
const journal = new MaintenanceJournal(store);
await journal.initialize();
await journal.initializeFutureOnlyLifecycle({
  now: "2026-10-10T08:00:01.000Z",
  witness,
});
await journal.initializeFutureOnlyLifecycle({
  now: "2026-10-10T08:00:02.000Z",
  witness,
});
writeFileSync(join(process.env.WEATHER_FUTURE_JOURNAL_ROOT, "witness.json"),
  canonicalJsonBytes(witness));
writeFileSync(join(process.env.WEATHER_FUTURE_JOURNAL_ROOT, "journal.expected"), store.bytes);
"#;

/// One disposable owner-private journal root.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique private fixture directory.
    fn create(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-future-journal-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("protect fixture root");
        Self(root)
    }

    /// Read one source-generated fixture artifact.
    fn read(&self, name: &str) -> Vec<u8> {
        fs::read(self.0.join(name)).expect("read fixture artifact")
    }
}

impl Drop for FixtureRoot {
    /// Remove only the bounded fixture directory.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Generate the exact witness and idempotent source journal record.
fn build_node_fixture(root: &Path) {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .current_dir(repository)
        .env("WEATHER_FUTURE_JOURNAL_ROOT", root)
        .output()
        .expect("run predecessor future journal fixture");
    // surface only a bounded predecessor diagnostic
    if !output.status.success() {
        panic!(
            "predecessor future journal fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn native_future_only_journal_matches_source_and_is_idempotent() {
    let fixture = FixtureRoot::create("source");
    build_node_fixture(&fixture.0);
    let witness = fixture.read("witness.json");
    let expected = fixture.read("journal.expected");
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    let (mut store, status) =
        NativeMaintenanceJournalStore::initialize_fixture(fixture.0.clone(), &mut codec, &time)
            .expect("initialize native journal");
    assert_eq!(status.records, 0);

    initialize_native_maintenance_future_only_journal(&mut store, NOW, &witness, &mut codec, &time)
        .expect("append future-only genesis");
    assert_eq!(
        fs::read(fixture.0.join("journal.jsonl")).expect("read native journal"),
        expected
    );
    initialize_native_maintenance_future_only_journal(
        &mut store,
        "2026-10-10T08:00:02.000Z",
        &witness,
        &mut codec,
        &time,
    )
    .expect("reuse future-only genesis");
    assert_eq!(
        fs::read(fixture.0.join("journal.jsonl")).expect("reread native journal"),
        expected
    );
}

#[test]
fn native_future_only_journal_rejects_predating_clock_without_append() {
    let source = FixtureRoot::create("witness");
    build_node_fixture(&source.0);
    let witness = source.read("witness.json");
    let fixture = FixtureRoot::create("predating");
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    let (mut store, _) =
        NativeMaintenanceJournalStore::initialize_fixture(fixture.0.clone(), &mut codec, &time)
            .expect("initialize native journal");

    assert_eq!(
        initialize_native_maintenance_future_only_journal(
            &mut store,
            "2026-10-10T07:59:59.999Z",
            &witness,
            &mut codec,
            &time,
        ),
        Err(NativeMaintenanceFutureOnlyJournalError::Clock)
    );
    assert!(
        store
            .read_verified(&mut codec, &time)
            .expect("read empty journal")
            .is_empty()
    );
}

#[test]
fn native_future_only_journal_rejects_record_key_collision() {
    let source = FixtureRoot::create("collision-witness");
    build_node_fixture(&source.0);
    let witness = source.read("witness.json");
    let fixture = FixtureRoot::create("collision");
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    let (mut store, _) =
        NativeMaintenanceJournalStore::initialize_fixture(fixture.0.clone(), &mut codec, &time)
            .expect("initialize native journal");
    let foreign_payload = JsonValue::Object(Vec::new());
    store
        .append(
            &[NativeMaintenanceJournalAddition {
                due_key: None,
                kind: GENESIS_KIND,
                payload: &foreign_payload,
                record_key: GENESIS_RECORD_KEY,
                recorded_at: NOW,
            }],
            &mut codec,
            &time,
        )
        .expect("append colliding genesis");

    assert_eq!(
        initialize_native_maintenance_future_only_journal(
            &mut store, NOW, &witness, &mut codec, &time,
        ),
        Err(NativeMaintenanceFutureOnlyJournalError::Collision)
    );
}
