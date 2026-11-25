use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{
    admit_native_adjustment_confirmation_authorization_v2,
    frame_native_adjustment_confirmation_authorization_v2,
    frame_native_adjustment_confirmation_availability_v2,
    read_native_adjustment_confirmation_authorization_dates,
};

const REGISTRATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ACCESS: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// match the source availability frame for one nonempty value-free projection.
#[test]
fn availability_matches_source_projection() {
    let input = response(availability_payload(), 23);
    let native =
        frame_native_adjustment_confirmation_availability_v2(input.as_slice(), REGISTRATION)
            .expect("native availability");
    let source = source_command("frame-v2-availability", &[REGISTRATION], &input);
    assert_eq!(native, source);
}

/// match the source authorization frame and inclusive date projection.
#[test]
fn authorization_matches_source_projection_and_dates() {
    let input = response(authorization_payload(), 23);
    let native = frame_native_adjustment_confirmation_authorization_v2(
        input.as_slice(),
        REGISTRATION,
        ACCESS,
        3,
    )
    .expect("native authorization");
    let source = source_command(
        "frame-v2-authorization",
        &[REGISTRATION, ACCESS, "3"],
        &input,
    );
    assert_eq!(native, source);
    admit_native_adjustment_confirmation_authorization_v2(&native).expect("admitted authorization");
    assert_eq!(
        read_native_adjustment_confirmation_authorization_dates(&native)
            .expect("authorization dates"),
        ("2026-11-01".to_owned(), "2026-11-02".to_owned())
    );
}

/// refuse legacy migration prefixes and payload count drift.
#[test]
fn projection_requires_complete_manifest_and_exact_payload() {
    let legacy = response(availability_payload(), 17);
    assert!(
        frame_native_adjustment_confirmation_availability_v2(legacy.as_slice(), REGISTRATION)
            .is_err()
    );

    // admit each reviewed complete additive ledger
    for migration_count in 18..=23 {
        let complete = response(availability_payload(), migration_count);
        assert!(
            frame_native_adjustment_confirmation_availability_v2(
                complete.as_slice(),
                REGISTRATION,
            )
            .is_ok(),
            "migration count {migration_count}"
        );
    }

    let mut payload = availability_payload();
    payload["hotPredictionCount"] = json!(1);
    let drifted = response(payload, 23);
    assert!(
        frame_native_adjustment_confirmation_availability_v2(drifted.as_slice(), REGISTRATION)
            .is_err()
    );
}

/// build one exact SQL function response using the reviewed migration files.
fn response(payload: Value, migration_count: usize) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "databaseManifest": database_manifest(migration_count),
        "payload": payload,
        "transaction": {
            "created_at_utc": "2026-11-25T12:34:56.123456Z",
            "idle_in_transaction_session_timeout": "30s",
            "isolation_level": "repeatable read",
            "lock_timeout": "5s",
            "read_only": "on",
            "statement_timeout": "5min"
        }
    }))
    .expect("database response")
}

/// build the complete value-free availability payload in database order.
fn availability_payload() -> Value {
    json!({
        "contractVersion": "adjustment-confirmation-availability/v2",
        "expectedKeySetSha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "family": "rain",
        "finalizedMetadataRootSha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        "finalizedPredictionCount": 9,
        "finalizedThroughAt": null,
        "hotPredictionCount": 2,
        "hotPredictions": [
            {
                "dueKey": "capture/2026-11-24T00:35:00.000Z",
                "maxValidAt": "2026-11-25T00:00:00.000Z",
                "minValidAt": "2026-11-24T01:00:00.000Z",
                "predictionSha256": "1111111111111111111111111111111111111111111111111111111111111111",
                "rowCount": 23
            },
            {
                "dueKey": "capture/2026-11-24T06:35:00.000Z",
                "maxValidAt": "2026-11-25T06:00:00.000Z",
                "minValidAt": "2026-11-24T07:00:00.000Z",
                "predictionSha256": "2222222222222222222222222222222222222222222222222222222222222222",
                "rowCount": 23
            }
        ],
        "hotSetRootSha256": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        "intervalEndAt": "2026-11-26T00:00:00.000Z",
        "intervalStartAt": "2026-11-24T00:00:00.000Z",
        "metadataGeneration": 4,
        "missingExpectedDueKeys": null,
        "missingExpectedDueKeysStatus": "requires_anchored_cold_reconstruction",
        "registrationSha256": REGISTRATION,
        "targetCutoffAt": "2026-11-25T12:00:00.000Z"
    })
}

/// build one exact two-day rain authorization payload.
fn authorization_payload() -> Value {
    json!({
        "accessSha256": ACCESS,
        "chunkCount": 24,
        "chunkIndex": 3,
        "contractVersion": "adjustment-confirmation-export-authorization/v2",
        "eligiblePredictionSetSha256": "1111111111111111111111111111111111111111111111111111111111111111",
        "expectedKeySetSha256": "2222222222222222222222222222222222222222222222222222222222222222",
        "family": "rain",
        "fromLocalDate": "2026-11-01",
        "metadataRootSha256": "3333333333333333333333333333333333333333333333333333333333333333",
        "registrationSha256": REGISTRATION,
        "revisionCatalogWatermarkSha256": "4444444444444444444444444444444444444444444444444444444444444444",
        "targetComparatorSnapshotRootSha256": "5555555555555555555555555555555555555555555555555555555555555555",
        "targetCutoffAt": "2026-11-25T12:00:00.000Z",
        "toLocalDateExclusive": "2026-11-03"
    })
}

/// derive one exact reviewed database manifest from the retained migration bytes.
fn database_manifest(migration_count: usize) -> Value {
    let directory = repository_root().join("packages/database/migrations");
    let mut migrations = fs::read_dir(directory)
        .expect("migration directory")
        .map(|entry| entry.expect("migration entry").path())
        .filter(|path| {
            path.extension().and_then(|value| value.to_str()) == Some("sql")
                && path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.starts_with("00"))
        })
        .collect::<Vec<_>>();
    migrations.sort();
    migrations.truncate(migration_count);
    let names = migrations
        .iter()
        .map(|path| {
            path.file_name()
                .expect("migration name")
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    let checksums = migrations
        .iter()
        .map(|path| lower_hex(Sha256::digest(fs::read(path).expect("migration bytes"))))
        .collect::<Vec<_>>();
    let history = names
        .iter()
        .zip(&checksums)
        .map(|(name, checksum)| format!("{name}:{checksum}"))
        .collect::<Vec<_>>()
        .join("\n");
    json!({
        "contract_version": "adjustment-evaluation-export-manifest/v1",
        "migration_checksums": checksums,
        "migration_history_sha256": lower_hex(Sha256::digest(history)),
        "migration_names": names,
        "query_contract_sha256": "c860039c72818a9b813ed9f9d93f5d8e115f4144e99d698734e9159ca3457bd4",
        "query_contract_version": "adjustment-evaluation-export-query/v1",
        "row_schema_sha256": "c21782034130d003e4af48fada4c8f6545f6f1c4dff550f92d0523c7d96d20e2",
        "schema_migration": "0017_adjustment_evaluation_export.sql",
        "site_key": "ballydidean",
        "site_timezone": "America/Los_Angeles"
    })
}

/// execute one retained source projection with fixture stdin.
fn source_command(command: &str, arguments: &[&str], input: &[u8]) -> Vec<u8> {
    let source = repository_root().join("deploy/scripts/adjustment-evaluation-package.mjs");
    let mut child = Command::new("node")
        .arg(source)
        .arg(command)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("source projection process");
    child
        .stdin
        .take()
        .expect("source projection stdin")
        .write_all(input)
        .expect("source projection request");
    let output = child.wait_with_output().expect("source projection result");
    assert!(
        output.status.success(),
        "source projection failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

/// resolve the repository root from the weather-runtime crate.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repository root")
}

/// encode one digest as lowercase hexadecimal.
fn lower_hex(bytes: impl AsRef<[u8]>) -> String {
    let mut output = String::with_capacity(bytes.as_ref().len() * 2);
    // append both nibbles in source byte order
    for byte in bytes.as_ref() {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("hex digest");
    }
    output
}
