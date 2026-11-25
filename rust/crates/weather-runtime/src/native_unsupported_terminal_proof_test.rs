use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use weather_contracts::JsonValue;

use super::{NativeUnsupportedTerminalProofStore, read_unsupported_terminal_proof_current_command};
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Disposable evidence root removed after one ordinary current-read fixture.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique canonical temporary root.
    fn create() -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-unsupported-terminal-proof-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("unique disposable evidence root");
        Self(fs::canonicalize(path).expect("canonical fixture root"))
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-unique disposable fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Preserve genuine absence and return the exact canonical proof envelope once present.
#[test]
fn reads_nullable_current_proof_without_mutating_retained_bytes() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).expect("fixture owner");
    let mut store = NativeUnsupportedTerminalProofStore::for_evidence_root(
        root.0.clone(),
        owner.uid(),
        owner.gid(),
    );
    let mut codec = NativeWirePrimitives;
    assert_eq!(
        read_unsupported_terminal_proof_current_command(
            "unsupported-terminal-proof-current-v1",
            &[],
            &mut store,
            &mut codec,
            &NativeWirePrimitives,
        )
        .unwrap()
        .unwrap(),
        b"null\n"
    );

    let proof_root = root.0.join("unsupported-terminal-proofs");
    DirBuilder::new().mode(0o700).create(&proof_root).unwrap();
    let proof = valid_proof(&mut codec);
    let proof_bytes = codec.encode_canonical_json(&proof).unwrap();
    let proof_sha256 = codec.sha256(&proof_bytes).unwrap();
    let current_path = proof_root.join("current.json");
    let mut current_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&current_path)
        .unwrap();
    current_file.write_all(&proof_bytes).unwrap();
    current_file.sync_all().unwrap();
    drop(current_file);

    let expected = codec
        .encode_canonical_json(&JsonValue::Object(vec![
            ("proof".encode_utf16().collect(), proof),
            (
                "proofSha256".encode_utf16().collect(),
                json_text(proof_sha256.as_str()),
            ),
        ]))
        .unwrap();
    let actual = read_unsupported_terminal_proof_current_command(
        "unsupported-terminal-proof-current-v1",
        &[],
        &mut store,
        &mut codec,
        &NativeWirePrimitives,
    )
    .unwrap()
    .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(fs::read(&current_path).unwrap(), proof_bytes);

    // a valid non-genesis proof retains its exact direct predecessor digest
    let mut successor = valid_proof(&mut codec);
    let JsonValue::Object(members) = &mut successor else {
        panic!("fixture proof must be an object");
    };
    for (name, value) in members {
        // keep the original predecessor grammar and increment only the sequence
        if name.as_slice() == "predecessorProofSha256".encode_utf16().collect::<Vec<_>>() {
            *value = json_text(proof_sha256.as_str());
        } else if name.as_slice() == "sequence".encode_utf16().collect::<Vec<_>>() {
            *value = json_text("1");
        }
    }
    let successor_bytes = codec.encode_canonical_json(&successor).unwrap();
    fs::write(&current_path, &successor_bytes).unwrap();
    let successor_hash = codec.sha256(&successor_bytes).unwrap();
    let expected_successor = codec
        .encode_canonical_json(&JsonValue::Object(vec![
            ("proof".encode_utf16().collect(), successor),
            (
                "proofSha256".encode_utf16().collect(),
                json_text(successor_hash.as_str()),
            ),
        ]))
        .unwrap();
    assert_eq!(
        read_unsupported_terminal_proof_current_command(
            "unsupported-terminal-proof-current-v1",
            &[],
            &mut store,
            &mut codec,
            &NativeWirePrimitives,
        )
        .unwrap()
        .unwrap(),
        expected_successor,
    );
    assert_eq!(fs::read(current_path).unwrap(), successor_bytes);
}

/// Build one source-valid temperature proof with a deliberately absent rain-only input class.
fn valid_proof(codec: &mut NativeWirePrimitives) -> JsonValue {
    let candidate = "6".repeat(64);
    let empty_root = digest(codec, &JsonValue::Array(Vec::new()));
    let populated_root = digest(codec, &JsonValue::Array(vec![json_text(&"7".repeat(64))]));
    let candidate_root = digest(codec, &JsonValue::Array(vec![json_text(&candidate)]));
    let input_classes = JsonValue::Object(
        [
            "actual_best_match",
            "artifact",
            "candidate",
            "comparator",
            "native_source",
            "rain_gate_input",
            "shadow_body",
            "shadow_source",
            "target",
            "target_revision",
        ]
        .into_iter()
        .map(|name| {
            let (count, root) = match name {
                "rain_gate_input" => (0.0, empty_root.clone()),
                "candidate" => (1.0, candidate_root.clone()),
                _ => (1.0, populated_root.clone()),
            };
            (
                name.encode_utf16().collect(),
                JsonValue::Object(vec![
                    ("count".encode_utf16().collect(), JsonValue::Number(count)),
                    ("rootSha256".encode_utf16().collect(), json_text(&root)),
                ]),
            )
        })
        .collect(),
    );
    let required_input_root = digest(codec, &input_classes);
    JsonValue::Object(vec![
        hash_field("actionSha256", '8'),
        text_field("archiveCommitOrdinal", "42"),
        hash_field("burnSha256", '9'),
        hash_field("candidateReportSha256", 'a'),
        hash_field("captureEpochWitnessSha256", 'b'),
        hash_field("confirmationAccessSha256", 'c'),
        number_field("confirmationChunkCount", 27.0),
        text_field(
            "contractVersion",
            "adjustment-maintenance-unsupported-terminal-proof/v1",
        ),
        hash_field("controlSha256", 'd'),
        text_field("controlVersion", "14"),
        hash_field("custodyCheckpointSha256", 'e'),
        text_field("dueKey", &format!("confirmation/temperature/{candidate}")),
        hash_field("eligiblePredictionSetSha256", 'f'),
        hash_field("expectedKeySetSha256", '0'),
        text_field("family", "temperature"),
        text_field("finalizedAt", "2026-10-08T02:12:00.000Z"),
        hash_field("frontierSha256", '1'),
        text_field("fullGraphVerifiedAt", "2026-10-08T02:11:00.000Z"),
        hash_field("fullMemberRootSha256", '2'),
        hash_field("graphManifestSha256", '3'),
        ("inputClasses".encode_utf16().collect(), input_classes),
        hash_field("lifecycleLedgerRootSha256", '4'),
        (
            "missingClassNames".encode_utf16().collect(),
            JsonValue::Array(Vec::new()),
        ),
        number_field("missingKeyCount", 1.0),
        hash_field("missingKeySetSha256", '5'),
        hash_field("pageSha256", '6'),
        hash_field("policyReportSha256", '7'),
        (
            "predecessorProofSha256".encode_utf16().collect(),
            JsonValue::Null,
        ),
        hash_field("registrationSha256", '8'),
        text_field("requiredInputRootSha256", &required_input_root),
        text_field("sequence", "0"),
        text_field("sourceCommit", &"9".repeat(40)),
        text_field("unsupportedReason", "permanent_source_gap"),
        hash_field("workstationJournalHeadSha256", 'a'),
    ])
}

/// Hash one fixture value through the same canonical adapter used by production.
fn digest(codec: &mut NativeWirePrimitives, value: &JsonValue) -> String {
    let bytes = codec.encode_canonical_json(value).unwrap();
    codec.sha256(&bytes).unwrap().as_str().to_owned()
}

/// Build one UTF-16 JSON string value.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one named UTF-16 JSON string field.
fn text_field(name: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), json_text(value))
}

/// Build one named JSON number field.
fn number_field(name: &str, value: f64) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), JsonValue::Number(value))
}

/// Build one named repeated-nibble SHA-256 fixture field.
fn hash_field(name: &str, nibble: char) -> (Vec<u16>, JsonValue) {
    text_field(name, &nibble.to_string().repeat(64))
}
