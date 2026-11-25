use std::fs;
use std::io;
use std::path::PathBuf;

use super::*;
use crate::NativeAdjustmentEvaluationPackageFiles;

const ROW_MEMBER: &str = "members/rows.jsonl.gz";
const FIXTURE_UTC_EARLY: &str = "2026-10-01T09:00:00.000Z";
const FIXTURE_UTC_LATE: &str = "2026-10-01T12:00:00.000Z";
const FIXTURE_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BODY_BYTES: &[u8] = b"{\"hourly\":[0.5,0.0]}";

struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    /// create one bounded extracted-package fixture directory
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "weather-adjustment-package-stream-{}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("new fixture directory");
        Self(path)
    }
}

impl Drop for FixtureDirectory {
    /// remove only the bounded fixture
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct FixtureDigest(Vec<u8>);

impl AdjustmentEvaluationMemberDigest for FixtureDigest {
    /// record the input sequence without providing a production hash
    fn update(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    /// return a test digest only for the exact native file bytes
    fn finish(self) -> Option<Sha256Digest> {
        matches!(self.0.as_slice(), b"first\r\nsecond\n" | BODY_BYTES)
            .then(|| Sha256Digest::admitted(FIXTURE_DIGEST.to_owned()))
    }
}

struct FixtureRow {
    source_identity: Vec<u16>,
    last_received_at: JsonValue,
    revision_count: f64,
}

#[derive(Default)]
struct FixtureRows {
    accepted: usize,
}

impl AdjustmentEvaluationRowsConsumer for FixtureRows {
    type Row = FixtureRow;

    /// recognize one of the exact ordinary test records
    fn admit_row(&mut self, line: &str) -> Result<Self::Row, AdjustmentEvaluationStreamError> {
        // give the first record the later receipt and fewer revisions
        let (source, receipt_at, revisions) = match line {
            "first" => ("wind-source", FIXTURE_UTC_LATE, 0.0),
            "second" => ("temperature-source", FIXTURE_UTC_EARLY, 2.0),
            _ => {
                return Err(AdjustmentEvaluationStreamError::Effect(
                    "unexpected row".to_owned(),
                ));
            }
        };
        Ok(FixtureRow {
            source_identity: source.encode_utf16().collect(),
            last_received_at: JsonValue::String(receipt_at.encode_utf16().collect()),
            revision_count: revisions,
        })
    }

    /// borrow only the row summary fields
    fn observe_row(row: &Self::Row) -> AdjustmentEvaluationRowObservation<'_> {
        AdjustmentEvaluationRowObservation {
            source_identity: &row.source_identity,
            last_received_at: &row.last_received_at,
            revision_count: row.revision_count,
        }
    }

    /// keep the ordinary fixture inside its original string timestamp domain
    fn receipt_is_later(&mut self, _: &JsonValue, _: &JsonValue) -> bool {
        unreachable!("fixture timestamps are strings")
    }

    /// record each row accepted after its native count check
    fn accept_row(&mut self, _: Self::Row) -> Result<(), AdjustmentEvaluationStreamError> {
        self.accepted += 1;
        Ok(())
    }
}

/// exercise real control and member files through exact stream bytes and row diagnostics
#[test]
fn adjustment_stream_consumes_real_member_and_preserves_original_diagnostics() {
    let scratch = FixtureDirectory::new();
    let path = scratch.0.to_str().expect("UTF-8 fixture root");
    fs::create_dir(scratch.0.join("members")).expect("fixture members");
    fs::create_dir(scratch.0.join("empty")).expect("fixture empty directory");
    fs::write(scratch.0.join("manifest.json"), b"original manifest bytes")
        .expect("fixture control");
    let bytes = b"first\r\nsecond\n";
    let size = bytes.len() as u64;
    fs::write(scratch.0.join(ROW_MEMBER), bytes).expect("fixture rows");
    let files = NativeAdjustmentEvaluationPackageFiles;

    assert_eq!(
        files
            .read_bounded_control(path, "manifest.json", 32, "package manifest")
            .unwrap(),
        b"original manifest bytes",
    );
    assert_eq!(
        files.list_regular_files(path).unwrap(),
        ["manifest.json", ROW_MEMBER]
    );
    let mut rows = FixtureRows::default();
    let summary = consume_adjustment_evaluation_member(
        files.open_member(path, ROW_MEMBER).expect("opened member"),
        size,
        &Sha256Digest::admitted(FIXTURE_DIGEST.to_owned()),
        FixtureDigest::default(),
        // use a fixture passthrough codec while binding the actual native descriptor
        |decoded| consume_adjustment_evaluation_row_stream(decoded, size, &mut rows),
    )
    .expect("verified member and admitted rows");

    assert_eq!(summary.canonical_stream_bytes, size);
    assert_eq!(summary.row_count, 2);
    assert_eq!(summary.revised_row_count, 1);
    assert_eq!(summary.max_revision_count, 2.0);
    assert_eq!(
        summary.max_source_receipt_at,
        Some(JsonValue::String(FIXTURE_UTC_LATE.encode_utf16().collect())),
    );
    assert_eq!(
        summary.source_identities.into_iter().collect::<Vec<_>>(),
        ["temperature-source", "wind-source"]
            .map(|source| source.encode_utf16().collect::<Vec<_>>()),
    );
    assert!(summary.ends_with_line_feed);
    assert_eq!(rows.accepted, 2);
}

/// retain one final partial row and report its missing line ending
#[test]
fn adjustment_stream_counts_final_partial_record_before_checking_final_framing() {
    let mut rows = FixtureRows::default();
    let original = b"first\nsecond";
    let summary = consume_adjustment_evaluation_row_stream(
        io::Cursor::new(original),
        original.len() as u64,
        &mut rows,
    )
    .expect("ordinary partial final record");

    assert_eq!(rows.accepted, 2);
    assert_eq!(summary.row_count, 2);
    assert_eq!(summary.canonical_stream_bytes, original.len() as u64);
    assert!(!summary.ends_with_line_feed);
}

struct FixtureBodyStreams;

impl AdjustmentEvaluationStreamingValues for FixtureBodyStreams {
    type Digest = FixtureDigest;
    type Inflated<R: Read> = R;

    /// start an isolated plaintext digest fixture
    fn begin_sha256(&mut self) -> Self::Digest {
        FixtureDigest::default()
    }

    /// use test contents as the injected decompressed stream
    fn gunzip_stream<R: Read>(&mut self, decoded: R) -> io::Result<Self::Inflated<R>> {
        Ok(decoded)
    }
}

#[derive(Default)]
struct FixtureBodyInspection(Vec<u8>);

impl AdjustmentEvaluationBodyInspection for FixtureBodyInspection {
    /// receive the original body bytes through the streaming policy port
    fn contains_private_material(&mut self, bytes: &[u8]) -> bool {
        self.0.extend_from_slice(bytes);
        false
    }

    /// verify that the policy saw the complete admitted body
    fn finish_contains_private_material(self) -> bool {
        assert_eq!(self.0, BODY_BYTES);
        false
    }
}

/// bind one ordinary retained body in a separate streamed plaintext phase
#[test]
fn adjustment_stream_binds_body_plaintext_length_and_digest() {
    validate_adjustment_evaluation_body(
        io::Cursor::new(BODY_BYTES),
        BODY_BYTES.len() as u64,
        BODY_BYTES.len() as f64,
        Some(FIXTURE_DIGEST),
        &mut FixtureBodyStreams,
        FixtureBodyInspection::default(),
    )
    .expect("original body plaintext binding");
}
