use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::forecast_training_package_manifest::FORECAST_TRAINING_STATION_KEYS;
use crate::{NativeWirePrimitives, stream_native_forecast_training_export};

const DATE: &str = "2026-10-01";
const CONTRACT_HASHES: [(&str, &str); 8] = [
    (
        "aggregation_contract_sha256",
        "9c309ef5a00780167570746ad6c31b9128c266db50954fe4645287e1f2b31e64",
    ),
    (
        "coordinate_manifest_sha256",
        "04bfd93a03c393e977c8767a9aca6fe2a4cba9c263cb46e6987fa733b666ba58",
    ),
    (
        "metric_eligibility_sha256",
        "53731954b347836a26500b05a195ca15cf26214c4d561fe482c5ff87ef56a82e",
    ),
    (
        "query_contract_sha256",
        "3b7926c47bbdb208ac2e305ee7798bfe4ea9590ce2863f556e752a71d1158e76",
    ),
    (
        "row_schema_sha256",
        "2717b6c3c704a1b52c7748b59c37d635efd92d92efb9dc97ea4ddef97cd504fc",
    ),
    (
        "source_lineage_sha256",
        "261a134589a12c1bbbd9a783343950317fd1fbc87e08383e60e805b7761566cc",
    ),
    (
        "spatial_weights_sha256",
        "8ed5ce70d33edd4a5166049d9938cbaaf800151b6a0b3345d3005419e9041c74",
    ),
    (
        "station_manifest_sha256",
        "a1f76440c056987bbb434d5315e4916f961deeb2951fe889d785943f559cdd49",
    ),
];

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

/// Compare one complete transaction through the actual Node and native package pipelines.
#[test]
fn native_full_export_matches_node_build_tar_and_gzip() {
    let fixture = Fixture::new();
    let repository = repository_root();
    let stream = complete_transaction_stream(&repository);
    let input_path = fixture.path.join("transaction.jsonl");
    fs::write(&input_path, &stream).expect("write transaction fixture");

    let scratch = fixture.path.join("native-scratch");
    let mut native_archive = Vec::new();
    let native = stream_native_forecast_training_export(
        Cursor::new(&stream),
        DATE,
        DATE,
        &scratch,
        &mut native_archive,
    )
    .expect("build native forecast-training archive");
    assert_eq!(native.row_count, 264);

    let node_root = fixture.path.join("node-package");
    let script = repository.join("deploy/scripts/forecast-training-package.mjs");
    let node = Command::new("node")
        .arg(&script)
        .args(["build"])
        .arg(&input_path)
        .arg(&node_root)
        .args([DATE, DATE])
        .output()
        .expect("run Node forecast-training package builder");
    assert!(
        node.status.success(),
        "{}",
        String::from_utf8_lossy(&node.stderr)
    );
    let node_manifest = String::from_utf8(node.stdout)
        .expect("Node manifest identity is UTF-8")
        .trim()
        .to_owned();
    assert_eq!(native.manifest_sha256, node_manifest);

    let node_archive = package_archive(&node_root);
    assert_eq!(native_archive, node_archive);
}

/// Build a source-valid one-day stream with all eleven physical station hourly grids.
fn complete_transaction_stream(repository: &Path) -> Vec<u8> {
    let manifest = database_manifest(repository);
    let transaction = json!({
        "created_at_utc": "2026-10-02T07:00:00.000Z",
        "from_local_date": DATE,
        "idle_in_transaction_session_timeout": "30s",
        "isolation_level": "repeatable read",
        "lock_timeout": "5s",
        "read_only": "on",
        "statement_timeout": "15min",
        "to_local_date": DATE,
    });
    let mut bytes = Vec::new();
    write_json_line(
        &mut bytes,
        &json!({
            "payload": manifest,
            "record_type": "manifest",
            "transaction": transaction,
        }),
    );
    let start = NativeWirePrimitives
        .canonical_utc_milliseconds("2026-10-01T07:00:00.000Z")
        .expect("fixture start instant");
    // cover every actual local hour for every closed station identity
    for (station_key, _) in FORECAST_TRAINING_STATION_KEYS {
        // preserve the exact twenty-four-hour non-DST grid for this date
        for hour in 0_i64..24 {
            let valid_at = NativeWirePrimitives
                .format_utc_milliseconds(start + hour * 3_600_000)
                .expect("fixture valid-at instant");
            write_json_line(
                &mut bytes,
                &json!({
                    "payload": station_gap_row(station_key, &valid_at),
                    "record_type": "row",
                }),
            );
        }
    }
    bytes
}

/// Build the actual checked thirteen-migration database manifest from repository bytes.
fn database_manifest(repository: &Path) -> Value {
    let migration_root = repository.join("packages/database/migrations");
    let mut names = fs::read_dir(&migration_root)
        .expect("read migration directory")
        .map(|entry| {
            entry
                .expect("read migration entry")
                .file_name()
                .into_string()
                .expect("migration name is UTF-8")
        })
        .filter(|name| name.ends_with(".sql"))
        .collect::<Vec<_>>();
    names.sort();
    names.truncate(13);
    assert_eq!(names.len(), 13);
    assert_eq!(names[9], "0010_forecast_training_export.sql");
    let checksums = names
        .iter()
        .map(|name| sha256(&fs::read(migration_root.join(name)).expect("read migration bytes")))
        .collect::<Vec<_>>();
    let history = names
        .iter()
        .zip(&checksums)
        .map(|(name, checksum)| format!("{name}:{checksum}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut manifest = serde_json::Map::new();
    // retain every frozen contract hash under its database-manifest key
    for (key, value) in CONTRACT_HASHES {
        manifest.insert(key.to_owned(), Value::String(value.to_owned()));
    }
    manifest.extend([
        (
            "contract_version".to_owned(),
            Value::String("forecast-training-export-manifest/v1".to_owned()),
        ),
        (
            "migration_checksums".to_owned(),
            Value::Array(checksums.into_iter().map(Value::String).collect()),
        ),
        (
            "migration_history_sha256".to_owned(),
            Value::String(sha256(history.as_bytes())),
        ),
        (
            "migration_names".to_owned(),
            Value::Array(names.into_iter().map(Value::String).collect()),
        ),
        (
            "query_contract_version".to_owned(),
            Value::String("forecast-training-export-query/v2".to_owned()),
        ),
        (
            "schema_migration".to_owned(),
            Value::String("0010_forecast_training_export.sql".to_owned()),
        ),
        (
            "site_key".to_owned(),
            Value::String("ballydidean".to_owned()),
        ),
        (
            "site_timezone".to_owned(),
            Value::String("America/Los_Angeles".to_owned()),
        ),
    ]);
    Value::Object(manifest)
}

/// Build one explicit no-value station hour admitted by both implementations.
fn station_gap_row(station_key: &str, valid_at: &str) -> Value {
    json!({
        "adapter_contracts": [],
        "collision_count": 0,
        "content_hashes": [],
        "contract_epoch": "physical-station-hourly/v1",
        "dataset": null,
        "exclusion_reason_codes": ["station_coverage_insufficient"],
        "ingestion_run_ids": [],
        "physical_station_key": station_key,
        "provider_family": provider_family(station_key),
        "record_kind": "station_hour",
        "received_at": null,
        "reference_at": null,
        "reference_kind": null,
        "relative_humidity_percent": null,
        "site_key": "ballydidean",
        "source_config_fingerprints": [],
        "source_keys": [],
        "target_lead_hours": null,
        "temperature_c": null,
        "upstream_model": null,
        "valid_at": valid_at,
        "wind_direction_degrees": null,
        "wind_gust_mps": null,
        "wind_speed_mps": null,
    })
}

/// Return the source's closed provider family for one physical station key.
fn provider_family(station_key: &str) -> &'static str {
    match station_key {
        "ambient-maxweather" | "ambient-merlin" => "ambient",
        "ballydidean-ecowitt" => "ecowitt",
        "netatmo-nearby" => "netatmo",
        "tempest-126537" | "tempest-168853" | "tempest-201058" | "tempest-203055"
        | "tempest-225947" | "tempest-38270" | "tempest-64255" => "tempest",
        _ => panic!("forecast-training station roster drifted"),
    }
}

/// Append one compact JSON transaction record with its exact line delimiter.
fn write_json_line(output: &mut Vec<u8>, value: &Value) {
    serde_json::to_writer(&mut *output, value).expect("serialize transaction record");
    output.push(b'\n');
}

/// Run the predecessor's exact deterministic outer tar and gzip commands.
fn package_archive(root: &Path) -> Vec<u8> {
    let tar = Command::new("tar")
        .args([
            "--create",
            "--format=ustar",
            "--sort=name",
            "--mtime=@0",
            "--owner=0",
            "--group=0",
            "--numeric-owner",
            "--directory",
        ])
        .arg(root)
        .arg(".")
        .output()
        .expect("run deterministic tar");
    assert!(
        tar.status.success(),
        "{}",
        String::from_utf8_lossy(&tar.stderr)
    );
    let mut gzip = Command::new("gzip")
        .args(["--no-name", "--best"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start deterministic gzip");
    gzip.stdin
        .as_mut()
        .expect("gzip stdin")
        .write_all(&tar.stdout)
        .expect("write tar to gzip");
    drop(gzip.stdin.take());
    let output = gzip.wait_with_output().expect("finish deterministic gzip");
    assert!(output.status.success());
    output.stdout
}

/// Hash exact fixture or repository bytes with the native SHA-256 primitive.
fn sha256(bytes: &[u8]) -> String {
    RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Locate the workspace root from the weather-runtime package directory.
fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace root")
        .to_path_buf()
}

/// Disposable transaction, extracted package, and scratch owner.
struct Fixture {
    path: PathBuf,
}

impl Fixture {
    /// Create one unique process-owned temporary root.
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-native-forecast-training-differential-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create differential fixture root");
        Self { path }
    }
}

impl Drop for Fixture {
    /// Remove only this test's process-owned temporary tree.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
