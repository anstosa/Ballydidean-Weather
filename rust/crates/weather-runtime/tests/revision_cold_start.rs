//! Compare the Rust cold-start command with the unchanged Node archive owner.

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::{JsDateOrString, JsonValue};
use weather_database::Sha256Digest;
use weather_runtime::{
    CaptureEpochInitJson, MaintenanceCanonicalJson, NativeRevisionColdStartDirectories,
    RevisionColdStartError, read_revision_cold_start_command,
};

const COLD_DIRECTORIES: &[&str] = &[
    "revision-commit-receipts",
    "revision-frontier-successors",
    "revision-projections",
    "shadow-revision-capsules",
    "rain-control-states",
    "rain-control-state-stage-receipts",
];

#[derive(Clone, Default)]
struct ReceiptFixture {
    fields: Vec<String>,
}

#[derive(Clone, Default)]
struct EntryFixture {
    logical_received_at: String,
    relation: String,
    receipt: ReceiptFixture,
}

#[derive(Clone, Default)]
struct SnapshotFixture {
    archive_commit_ordinal: String,
    cutoff_at: String,
    entry_count: usize,
    frontier_sha256: String,
    snapshot_sha256: String,
    entries: Vec<EntryFixture>,
    bytes: Vec<u8>,
    unsigned_start_bytes: Vec<u8>,
    output_bytes: Vec<u8>,
}

#[derive(Default)]
struct CaseFixture {
    input: Vec<u8>,
    output: Vec<u8>,
    error: Vec<u8>,
    success: bool,
}

#[derive(Default)]
struct Oracle {
    snapshots: BTreeMap<String, SnapshotFixture>,
    cases: BTreeMap<String, CaseFixture>,
    hashes: Vec<(Vec<u8>, String)>,
    clocks: BTreeMap<String, i64>,
}

impl Oracle {
    /// Execute the original Node command and decode only its tab-safe fixture records.
    fn load() -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root");
        let output = Command::new("node")
            .current_dir(workspace)
            .arg("rust/crates/weather-runtime/tests/revision-cold-start-node-oracle.mjs")
            .output()
            .expect("run revision cold-start Node oracle");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("oracle TSV");
        let mut oracle = Self::default();
        // reconstruct the decoded contract values independently of output bytes
        for line in stdout.lines() {
            let parts = line.split('\t').collect::<Vec<_>>();
            match parts.as_slice() {
                ["snapshot", name, ordinal, cutoff, count, frontier, digest] => {
                    oracle.snapshots.insert(
                        (*name).into(),
                        SnapshotFixture {
                            archive_commit_ordinal: (*ordinal).into(),
                            cutoff_at: (*cutoff).into(),
                            entry_count: count.parse().expect("entry count"),
                            frontier_sha256: (*frontier).into(),
                            snapshot_sha256: (*digest).into(),
                            ..SnapshotFixture::default()
                        },
                    );
                }
                ["entry", name, index, logical, relation] => {
                    let snapshot = oracle.snapshots.get_mut(*name).expect("snapshot first");
                    let index: usize = index.parse().expect("entry index");
                    assert_eq!(snapshot.entries.len(), index);
                    snapshot.entries.push(EntryFixture {
                        logical_received_at: (*logical).into(),
                        relation: (*relation).into(),
                        receipt: ReceiptFixture::default(),
                    });
                }
                ["receipt", name, index, fields @ ..] => {
                    let index: usize = index.parse().expect("receipt index");
                    assert_eq!(fields.len(), 10);
                    oracle.snapshots.get_mut(*name).expect("snapshot").entries[index]
                        .receipt
                        .fields = fields.iter().map(|field| (*field).into()).collect();
                }
                ["encoding", name, snapshot, unsigned, output] => {
                    let fixture = oracle.snapshots.get_mut(*name).expect("snapshot");
                    fixture.bytes = decode_base64(snapshot);
                    fixture.unsigned_start_bytes = decode_base64(unsigned);
                    fixture.output_bytes = decode_base64(output);
                }
                ["hash", preimage, digest] => {
                    oracle
                        .hashes
                        .push((decode_base64(preimage), (*digest).into()));
                }
                ["clock", value, milliseconds] => {
                    oracle.clocks.insert(
                        (*value).into(),
                        milliseconds.parse().expect("epoch milliseconds"),
                    );
                }
                ["case", name, status, input, output, error] => {
                    oracle.cases.insert(
                        (*name).into(),
                        CaseFixture {
                            input: decode_base64(input),
                            output: decode_base64(output),
                            error: decode_base64(error),
                            success: *status == "ok",
                        },
                    );
                }
                _ => panic!("unexpected oracle row: {line}"),
            }
        }
        oracle
    }

    /// Recover the fixture snapshot selected by one exact JSON.parse input body.
    fn snapshot_for_input(&self, bytes: &[u8]) -> Option<&SnapshotFixture> {
        let name = self
            .cases
            .iter()
            .find_map(|(name, case)| (case.input == bytes).then_some(name.as_str()))?;
        let fixture_name = match name {
            "arity" => "nonzero",
            "missing_directory" => "empty",
            name => name,
        };
        self.snapshots.get(fixture_name)
    }
}

struct Codec<'a>(&'a Oracle);

impl MaintenanceCanonicalJson for Codec<'_> {
    /// Accept no canonical file documents at this command boundary.
    fn parse_canonical_object(&mut self, _: &[u8]) -> Option<JsonValue> {
        None
    }

    /// Return only canonical snapshot and response encodings exported by Node.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        // bind values to the exact original recursively sorted bytes
        for snapshot in self.0.snapshots.values() {
            let snapshot_value = snapshot_value(snapshot);
            if value == &snapshot_value {
                return Some(snapshot.bytes.clone());
            }
            if value == &start_value(&snapshot_value, None) {
                return Some(snapshot.unsigned_start_bytes.clone());
            }
            let digest = self.0.hashes.iter().find_map(|(preimage, digest)| {
                (preimage == &snapshot.unsigned_start_bytes).then_some(digest.as_str())
            });
            if digest.is_some() && value == &start_value(&snapshot_value, digest) {
                return Some(snapshot.output_bytes.clone());
            }
        }
        None
    }

    /// Bind each digest request to an exact preimage observed in Node.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.0.hashes.iter().find_map(|(preimage, digest)| {
            (preimage == bytes).then(|| Sha256Digest::admitted(digest.clone()))
        })
    }
}

impl CaptureEpochInitJson for Codec<'_> {
    /// Decode only database envelope bytes actually supplied to the Node oracle.
    fn parse_database_envelope(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        let snapshot = self.0.snapshot_for_input(bytes)?;
        Some(database_envelope(snapshot))
    }
}

impl CurrentTimeResolver for Oracle {
    /// Resolve only canonical clocks observed by the original command fixture.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        let JsDateOrString::String(value) = value else {
            return Err(clock_error());
        };
        let utc = String::from_utf16(value).map_err(|_| clock_error())?;
        let epoch_milliseconds = self.clocks.get(&utc).copied().ok_or_else(clock_error)?;
        Ok(CanonicalInstant {
            epoch_milliseconds,
            utc,
        })
    }
}

/// Compare exact stdout for both zero and nonzero snapshots and reject retained failures.
#[test]
fn matches_original_cold_start_command_and_private_tree_order() {
    let oracle = Oracle::load();
    let root = PrivateRoot::new();

    // prove both accepted snapshot shapes produce byte-identical stdout
    for name in ["nonzero", "empty"] {
        let case = &oracle.cases[name];
        assert!(case.success);
        let mut directories = NativeRevisionColdStartDirectories::for_root(root.path.clone());
        let actual = read_revision_cold_start_command(
            "revision-cold-start-v1",
            &[],
            Cursor::new(&case.input),
            &mut directories,
            &mut Codec(&oracle),
            &oracle,
        )
        .expect("valid cold start");
        assert_eq!(actual.as_deref(), Some(case.output.as_slice()));
    }

    let invalid_relation = &oracle.cases["invalid_relation"];
    assert!(!invalid_relation.success && invalid_relation.output.is_empty());
    let error = read_revision_cold_start_command(
        "revision-cold-start-v1",
        &[],
        Cursor::new(&invalid_relation.input),
        &mut NativeRevisionColdStartDirectories::for_root(root.path.clone()),
        &mut Codec(&oracle),
        &oracle,
    )
    .expect_err("relation must match projection kind");
    assert_eq!(error, RevisionColdStartError::SnapshotEntry);
    assert_eq!(error.to_string().as_bytes(), invalid_relation.error);

    let invalid_genesis = &oracle.cases["invalid_genesis"];
    let error = read_revision_cold_start_command(
        "revision-cold-start-v1",
        &[],
        Cursor::new(&invalid_genesis.input),
        &mut NativeRevisionColdStartDirectories::for_root(root.path.clone()),
        &mut Codec(&oracle),
        &oracle,
    )
    .expect_err("zero cursor must use genesis");
    assert_eq!(error, RevisionColdStartError::Genesis);
    assert_eq!(error.to_string().as_bytes(), invalid_genesis.error);

    let arity = &oracle.cases["arity"];
    let error = read_revision_cold_start_command(
        "revision-cold-start-v1",
        &["unexpected"],
        Cursor::new(&arity.input),
        &mut NativeRevisionColdStartDirectories::for_root(root.path.clone()),
        &mut Codec(&oracle),
        &oracle,
    )
    .expect_err("arity must fail before input");
    assert_eq!(error, RevisionColdStartError::Arguments);
    assert_eq!(error.to_string().as_bytes(), arity.error);

    fs::remove_dir(root.path.join(COLD_DIRECTORIES.last().expect("last child")))
        .expect("remove one cold child");
    let missing = &oracle.cases["missing_directory"];
    let error = read_revision_cold_start_command(
        "revision-cold-start-v1",
        &[],
        Cursor::new(&missing.input),
        &mut NativeRevisionColdStartDirectories::for_root(root.path.clone()),
        &mut Codec(&oracle),
        &oracle,
    )
    .expect_err("missing fixed child");
    assert_eq!(error, RevisionColdStartError::Root);

    let ignored = read_revision_cold_start_command(
        "revision-cold-page-v1",
        &["not", "owned"],
        Cursor::new(Vec::new()),
        &mut NativeRevisionColdStartDirectories::for_root(PathBuf::from("/absent")),
        &mut Codec(&oracle),
        &oracle,
    )
    .expect("other command owner");
    assert_eq!(ignored, None);
}

/// Decode standard padded base64 emitted by Node without adding a package.
fn decode_base64(value: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    // decode complete four-character groups including the final padding
    for chunk in value.as_bytes().chunks_exact(4) {
        let units = [
            base64_unit(chunk[0]),
            base64_unit(chunk[1]),
            base64_unit(chunk[2]),
            base64_unit(chunk[3]),
        ];
        let bits = (u32::from(units[0]) << 18)
            | (u32::from(units[1]) << 12)
            | (u32::from(units[2]) << 6)
            | u32::from(units[3]);
        output.push((bits >> 16) as u8);
        // omit bytes represented by the original padding characters
        if chunk[2] != b'=' {
            output.push((bits >> 8) as u8);
        }
        if chunk[3] != b'=' {
            output.push(bits as u8);
        }
    }
    output
}

/// Map one admitted base64 character to its six-bit value.
fn base64_unit(value: u8) -> u8 {
    match value {
        b'A'..=b'Z' => value - b'A',
        b'a'..=b'z' => value - b'a' + 26,
        b'0'..=b'9' => value - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        b'=' => 0,
        _ => panic!("invalid base64 fixture"),
    }
}

/// Construct the exact decoded snapshot from the oracle's independent scalar rows.
fn snapshot_value(fixture: &SnapshotFixture) -> JsonValue {
    object(vec![
        (
            "archiveCommitOrdinal",
            string(&fixture.archive_commit_ordinal),
        ),
        (
            "contractVersion",
            string("adjustment-revision-serving-snapshot/v1"),
        ),
        ("cutoffAt", string(&fixture.cutoff_at)),
        (
            "entries",
            JsonValue::Array(fixture.entries.iter().map(entry_value).collect()),
        ),
        ("entryCount", JsonValue::Number(fixture.entry_count as f64)),
        ("frontierSha256", string(&fixture.frontier_sha256)),
        ("snapshotSha256", string(&fixture.snapshot_sha256)),
    ])
}

/// Restore one decoded current-pointer entry and its closed receipt object.
fn entry_value(fixture: &EntryFixture) -> JsonValue {
    let fields = &fixture.receipt.fields;
    object(vec![
        ("logicalReceivedAt", string(&fixture.logical_received_at)),
        ("receipt", receipt_value(fields)),
        ("relation", string(&fixture.relation)),
    ])
}

/// Restore the ten receipt fields in their generated-contract order.
fn receipt_value(fields: &[String]) -> JsonValue {
    object(vec![
        ("archiveCommitOrdinal", string(&fields[0])),
        ("archiveCommittedAt", string(&fields[1])),
        ("contractVersion", string(&fields[2])),
        ("frontierSha256", string(&fields[3])),
        ("predecessorFrontierSha256", string(&fields[4])),
        ("projectionIdentitySha256", string(&fields[5])),
        ("projectionKind", string(&fields[6])),
        ("projectionSha256", string(&fields[7])),
        ("receiptSha256", string(&fields[8])),
        ("stageReceiptSha256", string(&fields[9])),
    ])
}

/// Restore the JSON.parse database envelope used by the original command.
fn database_envelope(fixture: &SnapshotFixture) -> JsonValue {
    object(vec![
        ("databaseManifest", object(Vec::new())),
        ("payload", snapshot_value(fixture)),
        (
            "transaction",
            object(vec![
                ("created_at_utc", string(&fixture.cutoff_at)),
                ("idle_in_transaction_session_timeout", string("30s")),
                ("isolation_level", string("repeatable read")),
                ("lock_timeout", string("5s")),
                ("read_only", string("on")),
                ("statement_timeout", string("5min")),
            ]),
        ),
    ])
}

/// Restore either phase of the exact cold-transfer start document.
fn start_value(snapshot: &JsonValue, digest: Option<&str>) -> JsonValue {
    let archive_commit_ordinal = text_field(snapshot, "archiveCommitOrdinal");
    let frontier_sha256 = text_field(snapshot, "frontierSha256");
    let mut fields = vec![
        (
            "contractVersion",
            string("adjustment-revision-cold-transfer-start/v1"),
        ),
        ("servingSnapshot", snapshot.clone()),
        (
            "watermarkArchiveCommitOrdinal",
            string(&archive_commit_ordinal),
        ),
        ("watermarkFrontierSha256", string(&frontier_sha256)),
    ];
    // append only the address of the already materialized unsigned document
    if let Some(digest) = digest {
        fields.push(("startSha256", string(digest)));
    }
    object(fields)
}

/// Read one known string field from a fixture object.
fn text_field(value: &JsonValue, name: &str) -> String {
    let JsonValue::Object(fields) = value else {
        panic!("fixture object");
    };
    fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
        .and_then(|value| match value {
            JsonValue::String(value) => String::from_utf16(value).ok(),
            _ => None,
        })
        .expect("fixture text field")
}

/// Construct one dependency-free decoded JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Construct one dependency-free decoded JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Return one bounded clock failure without leaking a fixture value.
fn clock_error() -> CurrentProjectionError {
    CurrentProjectionError {
        code: "fixture.clock",
        detail: "clock not exported by Node".into(),
    }
}

struct PrivateRoot {
    path: PathBuf,
}

impl PrivateRoot {
    /// Create one unique disposable mode-0700 cold-reader tree.
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "weather-revision-cold-start-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("private fixture root");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("root mode");
        // create only the original six cold-reader children
        for directory in COLD_DIRECTORIES {
            let child = path.join(directory);
            fs::create_dir(&child).expect("private fixture child");
            fs::set_permissions(child, fs::Permissions::from_mode(0o700)).expect("child mode");
        }
        Self { path }
    }
}

impl Drop for PrivateRoot {
    /// Remove every disposable fixture directory after success or unwind.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Feed the original database JSON envelope into the actual native archive executable.
#[test]
fn native_evidence_cold_start_matches_node_database_envelopes() {
    use std::io::Write as _;
    use std::process::Stdio;

    let oracle = Oracle::load();
    let root = PrivateRoot::new();
    // both successful source snapshots must reach the actual executable without fixture codecs
    for name in ["nonzero", "empty"] {
        let case = &oracle.cases[name];
        let mut child = Command::new(env!("CARGO_BIN_EXE_weather-evidence"))
            .arg("revision-cold-start-v1")
            .env("WEATHER_ADJUSTMENT_EVIDENCE_ROOT", &root.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&case.input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, case.output, "{name}");
    }
    // the admitted snapshot reader must not create a new archive record
    for directory in COLD_DIRECTORIES {
        assert_eq!(fs::read_dir(root.path.join(directory)).unwrap().count(), 0);
    }
}
