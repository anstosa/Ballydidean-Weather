use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_contracts::JsonValue;

use crate::{
    NativeMaintenanceJournalError, NativeMaintenanceJournalFiles,
    NativeMaintenanceJournalHostFiles, NativeWirePrimitives,
    verify_native_maintenance_journal_bytes,
};

const NODE_FIXTURE: &str = r#"
import { createHash } from "node:crypto";
import { pathToFileURL } from "node:url";

const { verifyJournalBytes } = await import(
  pathToFileURL(process.env.WEATHER_MAINTENANCE_STATE_MODULE).href
);

// reproduce the source's recursive UTF-16 key ordering
function canonicalize(value) {
  if (Array.isArray(value)) {
    return value.map(canonicalize);
  }
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, canonicalize(value[key])]),
    );
  }
  return value;
}

// emit the exact source canonical framing
function canonicalJsonBytes(value) {
  return Buffer.from(`${JSON.stringify(canonicalize(value))}\n`, "utf8");
}

// bind one exact canonical value with SHA-256
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// append one source-shaped generation to the fixture chain
function append(records, addition) {
  const previousSha256 = records.at(-1)?.recordSha256 ?? null;
  const withoutHash = {
    contractVersion: "forecast-adjustment-maintenance-journal/v2",
    generation: String(records.length + 1),
    kind: addition.kind,
    recordKey: addition.recordKey,
    dueKey: addition.dueKey,
    recordedAt: addition.recordedAt,
    previousSha256,
    payloadSha256: sha256(canonicalJsonBytes(addition.payload)),
    payload: addition.payload,
  };
  records.push({
    ...withoutHash,
    recordSha256: sha256(canonicalJsonBytes(withoutHash)),
  });
}

const records = [];
append(records, {
  kind: "lease_acquired",
  recordKey: "lease/archive/run-1",
  dueKey: "archive/2026-11-24T12:00:00.000Z",
  recordedAt: "2026-11-24T12:00:00.000Z",
  payload: { fencingToken: "1", scope: "archive", state: "held" },
});
append(records, {
  kind: "lease_released",
  recordKey: "lease/archive/run-1/released",
  dueKey: null,
  recordedAt: "2026-11-24T12:01:00.000Z",
  payload: { fencingToken: "2", scope: "archive", state: "released" },
});
const bytes = Buffer.concat(records.map(canonicalJsonBytes));
const verified = verifyJournalBytes(bytes);
if (verified.length !== 2 || verified[1].previousSha256 !== verified[0].recordSha256) {
  throw new Error("source fixture verification differed");
}
process.stdout.write(bytes);
"#;

/// Remove one bounded disposable journal fixture root.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one unique process-local fixture directory.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-native-maintenance-journal-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("protect fixture root");
        Self(root)
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-owned disposable fixture root.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Generate a journal with the actual predecessor verifier before native admission.
fn node_journal_fixture() -> Vec<u8> {
    let module = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("scripts/research/adjustment_maintenance_state.mjs");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .env("WEATHER_MAINTENANCE_STATE_MODULE", module)
        .output()
        .expect("run predecessor journal verifier");
    // surface source fixture failures with their bounded diagnostic
    if !output.status.success() {
        panic!(
            "predecessor fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    output.stdout
}

/// Admit the same two-row chain as the actual predecessor verifier.
#[test]
fn native_verifier_matches_source_generated_journal_chain() {
    let bytes = node_journal_fixture();
    let mut values = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    let records = verify_native_maintenance_journal_bytes(&bytes, &mut values, &time)
        .expect("verify source journal");

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].generation, 1);
    assert_eq!(records[0].record_key, "lease/archive/run-1");
    assert_eq!(
        records[0].due_key.as_deref(),
        Some("archive/2026-11-24T12:00:00.000Z")
    );
    assert_eq!(records[1].generation, 2);
    assert_eq!(
        records[1].previous_sha256.as_deref(),
        Some(records[0].record_sha256.as_str())
    );
    assert!(matches!(&records[1].payload, JsonValue::Object(_)));

    let payload_needle = b"\"state\":\"released\"";
    let payload_offset = bytes
        .windows(payload_needle.len())
        .position(|window| window == payload_needle)
        .expect("fixture payload");
    let mut altered = bytes.clone();
    altered[payload_offset + b"\"state\":\"".len()] = b'R';
    assert_eq!(
        verify_native_maintenance_journal_bytes(&altered, &mut values, &time),
        Err(NativeMaintenanceJournalError::PayloadIdentity)
    );
}

/// Read genuine absence and one bounded private fixture without host access.
#[test]
fn host_reader_preserves_absent_genesis_and_private_stable_bytes() {
    let fixture = FixtureRoot::create();
    let uid = fixture.0.metadata().expect("fixture metadata").uid();
    let mut files = NativeMaintenanceJournalHostFiles::for_root(fixture.0.clone(), uid);
    assert!(files.read_journal().expect("absent genesis").is_empty());

    let bytes = node_journal_fixture();
    let path = fixture.0.join("journal.jsonl");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut file = options.open(&path).expect("create journal fixture");
    file.write_all(&bytes).expect("write journal fixture");
    file.sync_all().expect("sync journal fixture");
    drop(file);

    assert_eq!(files.read_journal().expect("read private journal"), bytes);
}
