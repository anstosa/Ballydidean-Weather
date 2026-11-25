use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use weather_contracts::JsonValue;

use super::{
    NativeFutureInputSealCaptureEpoch, NativeFutureInputSealCaptureEpochWitness,
    NativeFutureInputSealControl, NativeFutureInputSealCurrentError,
    NativeFutureInputSealSourceClosure, NativeFutureInputSealStore,
    read_future_input_seal_current_command,
};
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Disposable evidence root removed after one ordinary verified-current fixture.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique canonical temporary root.
    fn create() -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-future-input-seal-current-{}-{sequence}",
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

struct FixtureCaptureEpoch {
    witness: NativeFutureInputSealCaptureEpochWitness,
}

impl NativeFutureInputSealCaptureEpoch for FixtureCaptureEpoch {
    /// Return one fixture value already standing in for the shared witness verifier.
    fn read_verified_capture_epoch(
        &mut self,
    ) -> Result<NativeFutureInputSealCaptureEpochWitness, NativeFutureInputSealCurrentError> {
        Ok(self.witness.clone())
    }
}

struct FixtureSourceClosure {
    calls: usize,
    closed: bool,
}

impl NativeFutureInputSealSourceClosure for FixtureSourceClosure {
    /// Record the exact source-closure boundary without reading host release state.
    fn source_is_closed(
        &mut self,
        source_commit: &str,
    ) -> Result<bool, NativeFutureInputSealCurrentError> {
        assert_eq!(source_commit, "9".repeat(40));
        self.calls += 1;
        Ok(self.closed)
    }
}

/// Preserve verified genesis null, then return the exact canonical current envelope.
#[test]
fn reads_only_epoch_bound_and_source_closed_current_seal() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).expect("fixture owner");
    let mut files =
        NativeFutureInputSealStore::for_evidence_root(root.0.clone(), owner.uid(), owner.gid());
    let canonical_witness = b"verified capture epoch fixture\n".to_vec();
    let mut codec = NativeWirePrimitives;
    let witness_sha256 = codec.sha256(&canonical_witness).unwrap();
    let mut capture_epoch = FixtureCaptureEpoch {
        witness: NativeFutureInputSealCaptureEpochWitness {
            canonical_bytes: canonical_witness,
            control_version: "14".to_owned(),
            control_sha256: "a".repeat(64),
        },
    };
    let control_sha256 = "a".repeat(64);
    let control = NativeFutureInputSealControl {
        version: Some("14"),
        sha256: Some(&control_sha256),
    };
    let mut closure = FixtureSourceClosure {
        calls: 0,
        closed: true,
    };
    assert_eq!(
        read_future_input_seal_current_command(
            ("future-input-seal-current-v2", &[]),
            &mut files,
            &mut capture_epoch,
            control,
            &mut closure,
            &mut codec,
            &NativeWirePrimitives,
        )
        .unwrap()
        .unwrap(),
        b"null\n"
    );
    assert_eq!(closure.calls, 0);

    let seal_root = root.0.join("future-input-seals");
    DirBuilder::new().mode(0o700).create(&seal_root).unwrap();
    let seal = valid_future_input_seal(&mut codec, witness_sha256.as_str());
    let seal_bytes = codec.encode_canonical_json(&seal).unwrap();
    let seal_sha256 = codec.sha256(&seal_bytes).unwrap();
    let current_path = seal_root.join("current.json");
    let mut current = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&current_path)
        .unwrap();
    current.write_all(&seal_bytes).unwrap();
    current.sync_all().unwrap();
    drop(current);

    let expected = codec
        .encode_canonical_json(&JsonValue::Object(vec![
            ("seal".encode_utf16().collect(), seal),
            (
                "sealSha256".encode_utf16().collect(),
                json_text(seal_sha256.as_str()),
            ),
        ]))
        .unwrap();
    let actual = read_future_input_seal_current_command(
        ("future-input-seal-current-v2", &[]),
        &mut files,
        &mut capture_epoch,
        control,
        &mut closure,
        &mut codec,
        &NativeWirePrimitives,
    )
    .unwrap()
    .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(closure.calls, 1);
    assert_eq!(fs::read(current_path).unwrap(), seal_bytes);

    closure.closed = false;
    assert_eq!(
        read_future_input_seal_current_command(
            ("future-input-seal-current-v2", &[]),
            &mut files,
            &mut capture_epoch,
            control,
            &mut closure,
            &mut codec,
            &NativeWirePrimitives,
        ),
        Err(NativeFutureInputSealCurrentError::SourceClosure)
    );
}

/// Build one source-valid temperature genesis seal with every derived root bound.
pub(crate) fn valid_future_input_seal(
    codec: &mut NativeWirePrimitives,
    capture_epoch_sha256: &str,
) -> JsonValue {
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
    let candidate_artifact_root = digest(
        codec,
        &JsonValue::Array(vec![json_text(&candidate_root), json_text(&populated_root)]),
    );
    JsonValue::Object(vec![
        text_field("archiveCommitOrdinal", "42"),
        hash_field("burnSha256", '8'),
        text_field("candidateArtifactRootSha256", &candidate_artifact_root),
        hash_field("candidateReportSha256", '9'),
        text_field("captureEpochWitnessSha256", capture_epoch_sha256),
        text_field("comparatorRootSha256", &populated_root),
        hash_field("confirmationAccessSha256", 'a'),
        number_field("confirmationChunkCount", 27.0),
        text_field("contractVersion", "adjustment-future-only-input-seal/v2"),
        hash_field("custodyCheckpointSha256", 'b'),
        text_field("dueKey", &format!("confirmation/temperature/{candidate}")),
        text_field("family", "temperature"),
        hash_field("frontierSha256", 'c'),
        number_field("fullMemberCount", 1.0),
        hash_field("fullMemberRootSha256", 'd'),
        hash_field("graphManifestSha256", 'e'),
        ("inputClasses".encode_utf16().collect(), input_classes),
        hash_field("lifecycleLedgerRootSha256", 'f'),
        hash_field("pageSha256", '0'),
        hash_field("policyReportSha256", '1'),
        (
            "predecessorSealSha256".encode_utf16().collect(),
            JsonValue::Null,
        ),
        text_field("requiredInputRootSha256", &required_input_root),
        text_field("sealedAt", "2026-10-08T02:12:00.000Z"),
        text_field("sequence", "0"),
        text_field("sourceCommit", &"9".repeat(40)),
        text_field("targetRootSha256", &populated_root),
        hash_field("workstationJournalHeadSha256", '2'),
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
