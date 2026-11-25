use std::io::Cursor;
use std::path::Path;
use std::process::Command;

use super::{
    project_native_registration_schedule_status_v3,
    project_native_registration_schedule_status_v3_from_operator,
};
use crate::{CaptureEpochOperatorFile, CaptureEpochOperatorFiles, CaptureEpochOperatorReadError};

struct FixtureOperator {
    witness: Vec<u8>,
}

impl CaptureEpochOperatorFiles for FixtureOperator {
    /// Return only the retained witness selected by the production projection.
    fn read(
        &mut self,
        artifact: CaptureEpochOperatorFile,
    ) -> Result<Vec<u8>, CaptureEpochOperatorReadError> {
        match artifact {
            CaptureEpochOperatorFile::Witness => Ok(self.witness.clone()),
            CaptureEpochOperatorFile::Snapshot => Err(CaptureEpochOperatorReadError::Arguments),
        }
    }
}

/// Match the original Node projector byte-for-byte on the actual migration ledger fixture.
#[test]
fn native_registration_schedule_projection_matches_predecessor() {
    let fixture = predecessor_fixture();
    let actual = project_native_registration_schedule_status_v3(
        Cursor::new(&fixture.envelope),
        &fixture.witness,
    )
    .unwrap();
    assert_eq!(actual, fixture.expected);
    let mut operator = FixtureOperator {
        witness: fixture.witness.clone(),
    };
    let installed = project_native_registration_schedule_status_v3_from_operator(
        Cursor::new(&fixture.envelope),
        &mut operator,
    )
    .unwrap();
    assert_eq!(installed, fixture.expected);

    let mut drift = String::from_utf8(fixture.envelope).unwrap();
    drift = drift.replacen("\"read_only\":\"on\"", "\"read_only\":\"off\"", 1);
    assert!(
        project_native_registration_schedule_status_v3(
            Cursor::new(drift.into_bytes()),
            &fixture.witness,
        )
        .is_err()
    );
}

struct ProjectionFixture {
    envelope: Vec<u8>,
    expected: Vec<u8>,
    witness: Vec<u8>,
}

/// Execute the checked predecessor source only as a differential test oracle.
fn predecessor_fixture() -> ProjectionFixture {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let script = r#"
import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
const root = process.argv[1];
const source = await import(pathToFileURL(join(root, "deploy/scripts/adjustment-evaluation-package.mjs")));
const rolling = await import(pathToFileURL(join(root, "scripts/research/adjustment_rolling_schedule.mjs")));
const archive = await import(pathToFileURL(join(root, "scripts/research/adjustment_plaintext_archive.mjs")));
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const migrationRoot = join(root, "packages/database/migrations");
const names = readdirSync(migrationRoot).filter((name) => name.endsWith(".sql")).sort();
const checksums = names.map((name) => digest(readFileSync(join(migrationRoot, name))));
const history = digest(Buffer.from(names.map((name, index) => `${name}:${checksums[index]}`).join("\n")));
const retainedEnd = names.indexOf("0021_adjustment_rolling_registration.sql");
const retainedHistory = digest(Buffer.from(names.slice(0, retainedEnd + 1).map(
  (name, index) => `${name}:${checksums[index]}`,
).join("\n")));
const epochAt = "2026-10-09T04:10:45.000Z";
const unsigned = {
  activationKind: "inert_v14_pre_activation",
  archiveCommitOrdinal: "0",
  catalogFrontierSha256: digest(Buffer.from("adjustment-revision-frontier/v1\n0\n")),
  contractVersion: "adjustment-revision-capture-epoch-witness/v1",
  controlPlaneSha256: "a".repeat(64),
  controlPlaneVersion: "14",
  databaseMigrationHistorySha256: retainedHistory,
  epochAt,
  servingSnapshotSha256: "b".repeat(64),
  sourceCommit: "c".repeat(40),
  sourceRelease: "2026.10.09-3",
  sourceServerImageDigest: `sha256:${"d".repeat(64)}`,
  sourceWebImageDigest: `sha256:${"e".repeat(64)}`,
};
const witness = { ...unsigned, witnessSha256: digest(archive.canonicalJsonBytes(unsigned)) };
const bootstrap = rolling.buildAdjustmentRollingScheduleBootstrap({
  epochAt,
  epochWitnessSha256: witness.witnessSha256,
});
const envelope = {
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
  payload: ["temperature", "wind", "rain"].map((family) => ({
    contractVersion: "adjustment-shadow-registration-slot/v3",
    epochWitnessSha256: witness.witnessSha256,
    family,
    horizonEndAt: bootstrap.horizonEndAt,
    registrationSha256: null,
    scheduleContractSha256: rolling.ADJUSTMENT_ROLLING_SCHEDULE_SHA256,
    state: "free",
    terminalAt: null,
  })),
  transaction: {
    created_at_utc: "2026-10-09T04:11:00.123456Z",
    idle_in_transaction_session_timeout: "30s",
    isolation_level: "repeatable read",
    lock_timeout: "5s",
    read_only: "on",
    statement_timeout: "5min",
  },
};
const expected = source.projectAdjustmentRegistrationScheduleStatusV3(envelope, witness);
// emit the exact three canonical source artifacts without an intermediate JSON reserialization
for (const bytes of [archive.canonicalJsonBytes(envelope), archive.canonicalJsonBytes(witness), archive.canonicalJsonBytes(expected)]) {
  process.stdout.write(`${bytes.toString("base64")}\n`);
}
"#;
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", script])
        .arg(root)
        .output()
        .expect("node predecessor fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let encoded = String::from_utf8(output.stdout).unwrap();
    let mut lines = encoded.lines();
    let envelope = decode_base64(lines.next().unwrap());
    let witness = decode_base64(lines.next().unwrap());
    let expected = decode_base64(lines.next().unwrap());
    assert!(lines.next().is_none());
    ProjectionFixture {
        envelope,
        expected,
        witness,
    }
}

/// Decode the fixture's standard padded base64 without adding a test dependency.
fn decode_base64(value: &str) -> Vec<u8> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    STANDARD.decode(value).unwrap()
}
