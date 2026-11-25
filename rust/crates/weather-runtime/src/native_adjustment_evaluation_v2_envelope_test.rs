use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::{
    NativeAdjustmentEvaluationV2EnvelopeInput, NativeAdjustmentEvaluationV2ExportKind,
    stream_native_adjustment_evaluation_v2_envelope,
};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Match daily and persisted-burn frames byte-for-byte with the original streaming producer.
#[test]
fn native_adjustment_evaluation_v2_envelope_matches_predecessor() {
    let fixture = predecessor_fixture();
    let root = fixture_root();
    fs::create_dir(&root).unwrap();
    let archive = root.join("source-v1.tar.gz");
    let snapshot = root.join("edge-snapshot.json");
    let authorization = root.join("authorization.json");
    fs::write(&archive, &fixture.archive).unwrap();
    fs::write(&snapshot, &fixture.snapshot).unwrap();
    fs::write(&authorization, &fixture.authorization).unwrap();

    let mut daily = Vec::new();
    let daily_outcome = stream_native_adjustment_evaluation_v2_envelope(
        NativeAdjustmentEvaluationV2EnvelopeInput {
            authorization_path: None,
            edge_snapshot_path: &snapshot,
            export_kind: NativeAdjustmentEvaluationV2ExportKind::DailyMonitoring,
            from_local_date: "2026-10-01",
            source_archive_path: &archive,
            to_local_date: "2026-10-14",
        },
        &mut daily,
    )
    .unwrap();
    assert_eq!(daily, fixture.daily_frame);
    assert_eq!(daily_outcome.payload_bytes, fixture.archive.len() as u64);

    let mut confirmation = Vec::new();
    stream_native_adjustment_evaluation_v2_envelope(
        NativeAdjustmentEvaluationV2EnvelopeInput {
            authorization_path: Some(&authorization),
            edge_snapshot_path: &snapshot,
            export_kind: NativeAdjustmentEvaluationV2ExportKind::PostBurnConfirmation,
            from_local_date: "2026-10-01",
            source_archive_path: &archive,
            to_local_date: "2026-10-14",
        },
        &mut confirmation,
    )
    .unwrap();
    assert_eq!(confirmation, fixture.confirmation_frame);

    assert!(
        stream_native_adjustment_evaluation_v2_envelope(
            NativeAdjustmentEvaluationV2EnvelopeInput {
                authorization_path: None,
                edge_snapshot_path: &snapshot,
                export_kind: NativeAdjustmentEvaluationV2ExportKind::PostBurnConfirmation,
                from_local_date: "2026-10-01",
                source_archive_path: &archive,
                to_local_date: "2026-10-14",
            },
            &mut Vec::new(),
        )
        .is_err()
    );
    fs::remove_dir_all(root).unwrap();
}

struct EnvelopeFixture {
    archive: Vec<u8>,
    authorization: Vec<u8>,
    confirmation_frame: Vec<u8>,
    daily_frame: Vec<u8>,
    snapshot: Vec<u8>,
}

/// Produce actual predecessor frames from isolated disposable source files.
fn predecessor_fixture() -> EnvelopeFixture {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let script = r#"
import { createHash } from "node:crypto";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { PassThrough, Readable } from "node:stream";
import { pathToFileURL } from "node:url";
import { gzipSync } from "node:zlib";
const root = process.argv[1];
const source = await import(pathToFileURL(join(root, "deploy/scripts/adjustment-evaluation-package.mjs")));
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const migrationRoot = join(root, "packages/database/migrations");
const names = (await readdir(migrationRoot)).filter((name) => name.endsWith(".sql")).sort();
const checksums = await Promise.all(names.map(async (name) => digest(await readFile(join(migrationRoot, name)))));
const history = digest(Buffer.from(names.map((name, index) => `${name}:${checksums[index]}`).join("\n")));
const response = {
  databaseManifest: {
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
  payload: {
    accessSha256: "6".repeat(64),
    chunkCount: 27,
    chunkIndex: 0,
    contractVersion: "adjustment-confirmation-export-authorization/v2",
    eligiblePredictionSetSha256: "7".repeat(64),
    expectedKeySetSha256: "2".repeat(64),
    family: "temperature",
    fromLocalDate: "2026-10-01",
    metadataRootSha256: "8".repeat(64),
    registrationSha256: "1".repeat(64),
    revisionCatalogWatermarkSha256: "9".repeat(64),
    targetComparatorSnapshotRootSha256: "a".repeat(64),
    targetCutoffAt: "2026-10-08T06:59:59.000Z",
    toLocalDateExclusive: "2026-10-15",
  },
  transaction: {
    created_at_utc: "2026-10-08T12:00:00.000000Z",
    idle_in_transaction_session_timeout: "30s",
    isolation_level: "repeatable read",
    lock_timeout: "5s",
    read_only: "on",
    statement_timeout: "5min",
  },
};
const authorization = await source.frameAdjustmentConfirmationAuthorizationV2({
  accessSha256: "6".repeat(64),
  chunkIndex: 0,
  input: Readable.from([JSON.stringify(response)]),
  registrationSha256: "1".repeat(64),
});
const archive = gzipSync(Buffer.from("native v1 package fixture\n"), { level: 9, mtime: 0 });
const snapshot = Buffer.from(`${JSON.stringify({
  contractVersion: "forecast-adjustment-evidence-snapshot/v1",
  entries: [],
  frozenAt: "2026-10-07T12:00:00.000Z",
  watermarkSha256: "b".repeat(64),
})}\n`);
const temporary = await mkdtemp(join(tmpdir(), "native-v2-envelope-oracle-"));
try {
  const archivePath = join(temporary, "source-v1.tar.gz");
  const snapshotPath = join(temporary, "snapshot.json");
  const authorizationPath = join(temporary, "authorization.json");
  await writeFile(archivePath, archive);
  await writeFile(snapshotPath, snapshot);
  await writeFile(authorizationPath, authorization);
  // collect one complete predecessor stream into deterministic fixture bytes
  const stream = async (exportKind, authorizationPathValue) => {
    const output = new PassThrough();
    const chunks = [];
    output.on("data", (chunk) => chunks.push(chunk));
    await source.streamAdjustmentEvaluationV2EnvelopeArchive({
      authorizationPath: authorizationPathValue,
      edgeSnapshotPath: snapshotPath,
      exportKind,
      fromLocalDate: "2026-10-01",
      output,
      sourceArchivePath: archivePath,
      toLocalDate: "2026-10-14",
    });
    return Buffer.concat(chunks);
  };
  const daily = await stream("daily_monitoring", null);
  const confirmation = await stream("post_burn_confirmation", authorizationPath);
  // emit exact binary source material and both predecessor frames
  for (const bytes of [archive, snapshot, authorization, daily, confirmation]) {
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
        .expect("node v2 envelope fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let encoded = String::from_utf8(output.stdout).unwrap();
    let mut lines = encoded.lines();
    let archive = decode(lines.next().unwrap());
    let snapshot = decode(lines.next().unwrap());
    let authorization = decode(lines.next().unwrap());
    let daily_frame = decode(lines.next().unwrap());
    let confirmation_frame = decode(lines.next().unwrap());
    assert!(lines.next().is_none());
    EnvelopeFixture {
        archive,
        authorization,
        confirmation_frame,
        daily_frame,
        snapshot,
    }
}

/// Allocate one unique disposable fixture directory without adding a dependency.
fn fixture_root() -> PathBuf {
    let sequence = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "weather-native-v2-envelope-{}-{sequence}",
        std::process::id()
    ))
}

/// Decode one standard padded fixture frame.
fn decode(value: &str) -> Vec<u8> {
    STANDARD.decode(value).unwrap()
}
