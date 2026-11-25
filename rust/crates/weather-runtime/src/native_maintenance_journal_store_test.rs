use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_contracts::JsonValue;

use crate::NativeWirePrimitives;
use crate::native_maintenance_journal_store::{
    NativeMaintenanceJournalAddition, NativeMaintenanceJournalStore,
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

const NODE_FIXTURE: &str = r#"
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND,
  MaintenanceJournal,
} from "./scripts/research/adjustment_maintenance_state.mjs";
import { canonicalJsonBytes } from "./scripts/research/adjustment_plaintext_archive.mjs";

class MemoryJournalStore {
  constructor() {
    this.bytes = Buffer.alloc(0);
    this.heads = null;
  }

  // return the actual source state identity
  async initialize() {
    return { rootKind: ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND };
  }

  // execute one serialized in-memory source transaction
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
await journal.acquireLease({
  dueKey: "daily/2027-01-10",
  inputHeadSha256: "a".repeat(64),
  now: "2027-01-10T08:00:00.000Z",
  runId: "run-1",
  scope: "daily",
});
const restarted = await journal.initialize();
if (restarted.records !== 2 || restarted.head !== store.heads.current.recordSha256) {
  throw new Error("source restart projection differs");
}
writeFileSync(join(process.env.WEATHER_JOURNAL_FIXTURE_ROOT, "journal.expected"), store.bytes);
writeFileSync(join(process.env.WEATHER_JOURNAL_FIXTURE_ROOT, "current.expected"),
  canonicalJsonBytes(store.heads.current));
writeFileSync(join(process.env.WEATHER_JOURNAL_FIXTURE_ROOT, "previous.expected"),
  canonicalJsonBytes(store.heads.previous));
"#;

/// Remove one bounded disposable journal state root.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one unique literal private state directory.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-native-journal-store-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("protect fixture root");
        Self(root)
    }

    /// Read one actual source-generated expected artifact.
    fn expected(&self, name: &str) -> Vec<u8> {
        fs::read(self.0.join(name)).expect("read expected artifact")
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-owned disposable state root.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Generate exact source record and head bytes through MaintenanceJournal.
fn build_node_fixture(root: &Path) {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .current_dir(repository)
        .env("WEATHER_JOURNAL_FIXTURE_ROOT", root)
        .output()
        .expect("run predecessor journal fixture");
    // surface only the predecessor's bounded fixture diagnostic
    if !output.status.success() {
        panic!(
            "predecessor journal fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Construct one generated JSON string member.
fn string_field(key: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (
        key.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}

/// Replace one existing private fixture file and fsync its new contents.
fn replace_private_file(path: &Path, bytes: &[u8]) {
    let mut options = OpenOptions::new();
    options.write(true).truncate(true).mode(0o600);
    let mut file = options.open(path).expect("open fixture projection");
    file.write_all(bytes).expect("replace fixture projection");
    file.sync_all().expect("sync fixture projection");
}

/// Match source append bytes, durable heads, and restart reconciliation.
#[test]
fn native_journal_store_matches_source_append_and_repairs_heads_on_restart() {
    let fixture = FixtureRoot::create();
    build_node_fixture(&fixture.0);
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    let (mut store, genesis) =
        NativeMaintenanceJournalStore::initialize_fixture(fixture.0.clone(), &mut codec, &time)
            .expect("initialize fixture journal");
    assert_eq!(genesis.records, 0);
    assert_eq!(genesis.head_sha256, None);

    let due_payload = JsonValue::Object(vec![string_field("inputHeadSha256", HASH_A)]);
    let lease_payload = JsonValue::Object(vec![
        string_field("scope", "daily"),
        string_field("runId", "run-1"),
        string_field("dueKey", "daily/2027-01-10"),
        string_field("acquiredAt", "2027-01-10T08:00:00.000Z"),
        string_field("expiresAt", "2027-01-10T11:00:00.000Z"),
        string_field("inputHeadSha256", HASH_A),
        ("fencingToken".encode_utf16().collect(), JsonValue::Null),
    ]);
    let appended = store
        .append(
            &[
                NativeMaintenanceJournalAddition {
                    due_key: Some("daily/2027-01-10"),
                    kind: "due_registered",
                    payload: &due_payload,
                    record_key: "due/daily/2027-01-10",
                    recorded_at: "2027-01-10T08:00:00.000Z",
                },
                NativeMaintenanceJournalAddition {
                    due_key: Some("daily/2027-01-10"),
                    kind: "lease_acquired",
                    payload: &lease_payload,
                    record_key: "lease/daily/run-1/acquired",
                    recorded_at: "2027-01-10T08:00:00.000Z",
                },
            ],
            &mut codec,
            &time,
        )
        .expect("append fixture transaction");
    assert_eq!(appended.records, 2);
    assert_eq!(
        fs::read(fixture.0.join("journal.jsonl")).expect("read journal"),
        fixture.expected("journal.expected")
    );
    assert_eq!(
        fs::read(fixture.0.join("head.current")).expect("read current head"),
        fixture.expected("current.expected")
    );
    assert_eq!(
        fs::read(fixture.0.join("head.previous")).expect("read previous head"),
        fixture.expected("previous.expected")
    );

    replace_private_file(
        &fixture.0.join("head.current"),
        b"{\"generation\":\"0\",\"recordSha256\":null}\n",
    );
    drop(store);
    let (mut restarted, status) =
        NativeMaintenanceJournalStore::initialize_fixture(fixture.0.clone(), &mut codec, &time)
            .expect("restart fixture journal");
    assert_eq!(status.records, 2);
    assert_eq!(status.head_sha256, appended.head_sha256);
    assert_eq!(
        fs::read(fixture.0.join("head.current")).expect("read repaired head"),
        fixture.expected("current.expected")
    );
    assert_eq!(
        restarted
            .read_verified(&mut codec, &time)
            .expect("verify restarted journal")
            .len(),
        2
    );
}
