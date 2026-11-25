use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_contracts::JsonValue;

use super::*;
use crate::NativeMaintenanceOwnedJournalAddition;

const NEXT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CHECKPOINT: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const MEMBER: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PAGE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const START: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const GRAPH: &str = "9999999999999999999999999999999999999999999999999999999999999999";
const SEALED_AT: &str = "2026-10-10T08:00:04.000Z";

const NODE_FIXTURE: &str = r#"
import { readFileSync } from "node:fs";
import { MaintenanceJournal } from "./scripts/research/adjustment_maintenance_state.mjs";

let bytes = readFileSync(process.env.WEATHER_COLD_PACK_PREFIX);
const store = {
  initialize: async () => ({ rootKind: "home_native_ext4_state" }),
  withLock: async (callback) => await callback({
    readJournal: async () => Buffer.from(bytes),
    append: async (more) => {
      bytes = Buffer.concat([bytes, more]);
    },
    writeHeads: async () => undefined,
  }),
};
const journal = new MaintenanceJournal(store);
await journal.recordRevisionCustodyPackSeal({
  checkpointSha256: "c".repeat(64),
  graphManifestSha256: "9".repeat(64),
  now: "2026-10-10T08:00:04.000Z",
  pages: [{
    memberRootSha256: "d".repeat(64),
    pageSha256: "e".repeat(64),
    startMemberSha256: "f".repeat(64),
    startSha256: "f".repeat(64),
  }],
});
process.stdout.write(bytes);
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
            "weather-cold-pack-seal-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("protect fixture root");
        Self(root)
    }
}

impl Drop for FixtureRoot {
    /// Remove only the bounded process-owned fixture root.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_cold_pack_seal_matches_source_and_is_idempotent() {
    let fixture = FixtureRoot::create();
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    let (mut store, _) =
        NativeMaintenanceJournalStore::initialize_fixture(fixture.0.clone(), &mut codec, &time)
            .expect("initialize fixture journal");
    let due_key = format!("archive/revision-custody-pack/{CHECKPOINT}");
    let request = request_value();
    let intent = object(vec![
        ("nextArchiveCommitOrdinal", string("1")),
        ("nextFrontierSha256", string(NEXT)),
        ("request", request),
    ]);
    store
        .mutate_checked(&mut codec, &time, |_| {
            Ok::<_, NativeMaintenanceColdPackSealJournalError>(vec![
                addition(
                    None,
                    INTENT_KIND,
                    format!("revision/custody/intent/{PAGE}"),
                    "2026-10-10T08:00:01.000Z",
                    intent,
                ),
                addition(
                    None,
                    COMPLETED_V1_KIND,
                    format!("revision/custody/completed/{PAGE}"),
                    "2026-10-10T08:00:02.000Z",
                    object(vec![
                        ("acknowledgementSha256", string(&"a".repeat(64))),
                        ("pageSha256", string(PAGE)),
                    ]),
                ),
                addition(
                    Some(&due_key),
                    DUE_COMPLETED_KIND,
                    format!("due/{due_key}/complete"),
                    "2026-10-10T08:00:03.000Z",
                    object(vec![("outputSha256", string(GRAPH))]),
                ),
            ])
        })
        .expect("append source-shaped prefix");
    let prefix = fs::read(fixture.0.join("journal.jsonl")).expect("read prefix");
    let prefix_path = fixture.0.join("prefix.jsonl");
    fs::write(&prefix_path, &prefix).expect("write bounded prefix copy");
    let page = NativeMaintenanceOpenPackPageRecord {
        offset: 0,
        start_length: 128,
        page_length: 256,
        member_root_sha256: MEMBER.to_owned(),
        start_member_sha256: Some(START.to_owned()),
        page_sha256: PAGE.to_owned(),
        start_sha256: START.to_owned(),
        transfer_sha256: "a".repeat(64),
    };

    record_native_maintenance_cold_pack_seal(
        &mut store,
        CHECKPOINT,
        GRAPH,
        SEALED_AT,
        std::slice::from_ref(&page),
    )
    .expect("record native pack seal");
    record_native_maintenance_cold_pack_seal(&mut store, CHECKPOINT, GRAPH, SEALED_AT, &[page])
        .expect("reuse native pack seal");
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let source = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .current_dir(repository)
        .env("WEATHER_COLD_PACK_PREFIX", prefix_path)
        .output()
        .expect("run source pack seal fixture");
    // surface only the predecessor's bounded fixture diagnostic
    if !source.status.success() {
        panic!(
            "predecessor cold pack seal fixture failed: {}",
            String::from_utf8_lossy(&source.stderr)
        );
    }
    assert_eq!(
        fs::read(fixture.0.join("journal.jsonl")).expect("read native seal journal"),
        source.stdout
    );
}

/// Encode the exact packed custody request referenced by the open-pack page.
fn request_value() -> JsonValue {
    object(vec![
        ("afterArchiveCommitOrdinal", string("0")),
        (
            "afterFrontierSha256",
            string("5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af"),
        ),
        ("custodyCheckpointSha256", string(CHECKPOINT)),
        ("memberRootSha256", string(MEMBER)),
        ("pageSha256", string(PAGE)),
        ("previousPageSha256", string(START)),
        ("startMemberSha256", string(START)),
        ("startSha256", string(START)),
        ("watermarkArchiveCommitOrdinal", string("1")),
        ("watermarkFrontierSha256", string(NEXT)),
    ])
}

/// Construct one exact owned fixture journal addition.
fn addition(
    due_key: Option<&str>,
    kind: &str,
    record_key: String,
    recorded_at: &str,
    payload: JsonValue,
) -> NativeMaintenanceOwnedJournalAddition {
    NativeMaintenanceOwnedJournalAddition {
        due_key: due_key.map(str::to_owned),
        kind: kind.to_owned(),
        payload,
        record_key,
        recorded_at: recorded_at.to_owned(),
    }
}
