use std::fs;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use weather_edge::RevisionArchiveCanonicalClock;

use super::{freeze_native_evaluation_edge_snapshot, stream_native_evaluation_archive_v1};

const FROZEN_AT: &str = "2026-10-07T12:00:00.000Z";
static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct FixtureClock;

impl RevisionArchiveCanonicalClock for FixtureClock {
    /// Return the exact predecessor fixture clock for the frozen empty catalog.
    fn canonical_now(&mut self) -> Option<String> {
        Some(FROZEN_AT.to_owned())
    }
}

/// Match the complete native v1 gzip archive and manifest identity to the source producer.
#[test]
fn native_evaluation_export_archive_matches_predecessor() {
    let fixture = predecessor_fixture();
    let root = fixture_root();
    let edge_root = root.join("evidence");
    // reproduce the complete four-directory evidence catalog admitted by both producers
    for directory in ["objects", "receipts", "channels", "channel-bindings"] {
        fs::create_dir_all(edge_root.join(directory)).unwrap();
    }

    let snapshot = freeze_native_evaluation_edge_snapshot(&edge_root, &mut FixtureClock).unwrap();
    assert_eq!(snapshot, fixture.snapshot);

    let mut archive = Vec::new();
    let manifest_sha256 = stream_native_evaluation_archive_v1(
        BufReader::new(Cursor::new(fixture.database_stream)),
        &snapshot,
        &edge_root,
        "2026-10-01",
        "2026-10-01",
        &mut archive,
    )
    .unwrap();
    assert_eq!(manifest_sha256, fixture.manifest_sha256);
    assert_eq!(archive, fixture.archive);
    fs::remove_dir_all(root).unwrap();
}

struct ExportFixture {
    archive: Vec<u8>,
    database_stream: Vec<u8>,
    manifest_sha256: String,
    snapshot: Vec<u8>,
}

/// Run the checked Node source with one ordinary temperature row and an empty edge catalog.
fn predecessor_fixture() -> ExportFixture {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let script = r#"
import { createHash } from "node:crypto";
import { readdir, readFile, mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PassThrough, Readable } from "node:stream";
import { pathToFileURL } from "node:url";
const root = process.argv[1];
const source = await import(pathToFileURL(join(root, "deploy/scripts/adjustment-evaluation-package.mjs")));
const evidence = await import(pathToFileURL(join(root, "deploy/scripts/adjustment-evidence-store.mjs")));
// hash exact fixture bytes with the predecessor's lowercase digest spelling
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const migrationRoot = join(root, "packages/database/migrations");
// retain the source migration-name ordering
const names = (await readdir(migrationRoot)).filter((name) => name.endsWith(".sql")).sort();
// hash each source migration byte-for-byte
const checksums = await Promise.all(names.map(async (name) => digest(await readFile(join(migrationRoot, name)))));
// bind the ordered migration names and checksums exactly
const history = digest(Buffer.from(names.map((name, index) => `${name}:${checksums[index]}`).join("\n")));
const manifest = {
  payload: {
    contract_version: "adjustment-evaluation-export-manifest/v1",
    migration_checksums: checksums,
    migration_history_sha256: history,
    migration_names: names,
    query_contract_sha256: "c860039c72818a9b813ed9f9d93f5d8e115f4144e99d698734e9159ca3457bd4",
    query_contract_version: "adjustment-evaluation-export-query/v1",
    row_schema_sha256: "c21782034130d003e4af48fada4c8f6545f6f1c4dff550f92d0523c7d96d20e2",
    schema_migration: "0017_adjustment_evaluation_export.sql",
    site_key: "ballydidean",
    site_timezone: "America/Los_Angeles",
  },
  record_type: "manifest",
  transaction: {
    created_at_utc: "2026-10-07T12:00:00.000000Z",
    from_local_date: "2026-10-01",
    idle_in_transaction_session_timeout: "30s",
    isolation_level: "repeatable read",
    lock_timeout: "5s",
    read_only: "on",
    statement_timeout: "5min",
    to_local_date: "2026-10-01",
  },
};
const row = {
  compressed_body_base64: null,
  content_hash: "a".repeat(64),
  first_received_at: "2026-10-01T06:03:00.000Z",
  last_received_at: "2026-10-01T06:03:00.000Z",
  local_date: "2026-10-01",
  payload: {
    contentHash: "a".repeat(64),
    modelLeadHours: 7,
    rawRelativeHumidityPercent: 80,
    rawTemperatureC: 12.5,
    rawWindSpeedMps: 3.1,
    runId: 1,
    validAt: "2026-10-01T07:00:00.000Z",
  },
  record_kind: "temperature_hour",
  record_revision_identity: "ecmwf-temperature-canary-hour:1:7",
  reference_at: "2026-10-01T00:00:00.000Z",
  revision_count: 0,
  site_key: "ballydidean",
  source_identity: "ecmwf-temperature-canary:1",
  valid_at: "2026-10-01T07:00:00.000Z",
};
const databaseStream = Buffer.from(`${JSON.stringify(manifest)}\n${JSON.stringify({ payload: row, record_type: "row" })}\n`);
const temporary = await mkdtemp(join(tmpdir(), "native-evaluation-v1-oracle-"));
try {
  const edgeRoot = join(temporary, "evidence");
  const staging = join(temporary, "staging");
  // reproduce the complete owner-private evidence directory contract
  for (const directory of ["objects", "receipts", "channels", "channel-bindings"]) {
    await mkdir(join(edgeRoot, directory), { mode: 0o700, recursive: true });
  }
  await mkdir(staging);
  const snapshotValue = await evidence.freezeAdjustmentEvidenceSnapshot({
    // hold the source canonical clock constant across the differential
    now: () => new Date("2026-10-07T12:00:00.000Z"),
    root: edgeRoot,
  });
  const snapshot = Buffer.from(`${JSON.stringify(snapshotValue)}\n`);
  const snapshotPath = join(temporary, "snapshot.json");
  await writeFile(snapshotPath, snapshot);
  const output = new PassThrough();
  const chunks = [];
  output.on("data", (chunk) => chunks.push(chunk));
  const manifestSha256 = await source.streamAdjustmentEvaluationPackageArchive({
    edgeSnapshotPath: snapshotPath,
    fromLocalDate: "2026-10-01",
    input: Readable.from([databaseStream]),
    output,
    temporaryRoot: staging,
    toLocalDate: "2026-10-01",
  });
  const archive = Buffer.concat(chunks);
  // emit the exact source snapshot, database stream, archive and returned manifest address
  for (const bytes of [snapshot, databaseStream, archive, Buffer.from(manifestSha256)]) {
    process.stdout.write(`${bytes.toString("base64")}\n`);
  }
} finally {
  await rm(temporary, { force: true, recursive: true });
}
"#;
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", script])
        .arg(root)
        .output()
        .expect("node v1 evaluation archive fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let encoded = String::from_utf8(output.stdout).unwrap();
    let mut lines = encoded.lines();
    let snapshot = decode(lines.next().unwrap());
    let database_stream = decode(lines.next().unwrap());
    let archive = decode(lines.next().unwrap());
    let manifest_sha256 = String::from_utf8(decode(lines.next().unwrap())).unwrap();
    assert!(lines.next().is_none());
    ExportFixture {
        archive,
        database_stream,
        manifest_sha256,
        snapshot,
    }
}

/// Allocate one unique disposable test root without introducing another package.
fn fixture_root() -> PathBuf {
    let sequence = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "weather-native-evaluation-v1-{}-{sequence}",
        std::process::id()
    ))
}

/// Decode one standard padded fixture frame.
fn decode(value: &str) -> Vec<u8> {
    STANDARD.decode(value).unwrap()
}
