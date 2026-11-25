use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::{
    AdjustmentEvaluationMemberDigest, MaintenanceCanonicalJson,
    NativeAdjustmentEvaluationOpenedMember, NativeAdjustmentEvaluationPackageFiles,
    adjustment_evaluation_package_manifest::fixture_tests,
};

const MANIFEST_BYTES: &[u8] = b"original empty adjustment package fixture";
const EMPTY_ROW_MEMBER_BYTES: &[u8] = b"synthetic empty gzip stream";
pub(crate) const MANIFEST_DIGEST: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LEDGER_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(crate) const WATERMARK_DIGEST: &str =
    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ROW_DIGEST: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

pub(crate) struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    /// borrow the actual extracted package root
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for FixtureDirectory {
    /// remove only the bounded fixture package
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) struct FixtureValues {
    manifest: JsonValue,
    control_reads: Vec<String>,
    rows: Vec<(&'static [u8], JsonValue)>,
}

impl FixtureValues {
    /// expose control rereads to the full package loader fixture
    pub(crate) fn control_reads(&self) -> &[String] {
        &self.control_reads
    }
}
impl MaintenanceCanonicalJson for FixtureValues {
    /// keep the fixture outside unused canonical object owners
    fn parse_canonical_object(&mut self, _: &[u8]) -> Option<JsonValue> {
        None
    }

    /// encode the exact empty edge watermark input
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        // recognize only the declared synthetic rows and the empty watermark
        if value == &JsonValue::Array(Vec::new()) {
            return Some(b"[]\n".to_vec());
        }
        let (line, _) = self.rows.iter().find(|(_, row)| row == value)?;
        Some([*line, b"\n"].concat())
    }

    /// return only approved synthetic digest answers for the fixture
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        fixture_digest(bytes)
    }
}

/// map original fixture effects without implementing a production codec
fn fixture_digest(bytes: &[u8]) -> Option<Sha256Digest> {
    // map each known boundary preimage to its separately declared digest token
    let digest = match bytes {
        MANIFEST_BYTES => MANIFEST_DIGEST,
        EMPTY_ROW_MEMBER_BYTES => ROW_DIGEST,
        b"[]" => WATERMARK_DIGEST,
        bytes if bytes.starts_with(b"0001_initial_weather.sql:") => LEDGER_DIGEST,
        _ => return None,
    };
    Some(Sha256Digest::admitted(digest))
}

impl AdjustmentEvaluationPackageManifestValues for FixtureValues {
    type Error = io::Error;

    /// read real controls and count the strict post-verification reread
    fn read_bounded_regular_file(
        &mut self,
        root: &str,
        path: &str,
        bound: u64,
        description: &str,
    ) -> io::Result<Vec<u8>> {
        self.control_reads.push(path.to_owned());
        NativeAdjustmentEvaluationPackageFiles.read_bounded_control(root, path, bound, description)
    }

    /// decode only the exact unchanged fixture control file
    fn decode_json(&mut self, bytes: &[u8]) -> io::Result<JsonValue> {
        (bytes == MANIFEST_BYTES)
            .then(|| self.manifest.clone())
            .ok_or_else(|| io::Error::other("unexpected fixture manifest bytes"))
    }

    /// resolve the explicit digest fixtures shared with the edge stage
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        fixture_digest(bytes)
    }

    /// admit only the original instant used in the transaction and freeze
    fn utc_epoch_milliseconds(&mut self, value: &JsonValue) -> Option<i64> {
        fixture_epoch(value)
    }

    /// admit the sole canonical fixture package date
    fn local_date_ordinal(&mut self, date: &str) -> Option<i64> {
        (date == "2026-10-01").then_some(0)
    }

    /// enumerate the real extracted package
    fn collect_regular_files(&mut self, root: &str) -> io::Result<Vec<String>> {
        NativeAdjustmentEvaluationPackageFiles.list_regular_files(root)
    }
}

impl AdjustmentEvaluationPackageRowValues for FixtureValues {
    /// decode only the exact fixture row tokens
    fn decode_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        self.rows
            .iter()
            .find_map(|(line, row)| (*line == bytes).then(|| row.clone()))
    }

    /// admit only the fixture's stored row instants
    fn js_timestamp_milliseconds(&mut self, value: &JsonValue) -> Option<i64> {
        fixture_epoch(value)
    }

    /// keep the fixture's sole date label
    fn matches_canonical_local_date(&mut self, value: &JsonValue) -> bool {
        value == &wire("2026-10-01")
    }

    /// keep the inert public fixture payloads independent of production inspection
    fn payload_contains_forbidden_private_material(&mut self, _: &JsonValue) -> bool {
        false
    }
}

impl AdjustmentEvaluationEdgeVerificationValues for FixtureValues {
    type Error = AdjustmentEvaluationStreamError;

    /// bind a real bounded member if the strict verifier asks to consume one
    fn read_bound_edge_member(
        &mut self,
        root: &str,
        member: &AdjustmentEvaluationEdgeMember,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, Self::Error> {
        read_verified_adjustment_evaluation_member(
            root,
            &member.path,
            member.size_bytes,
            &member.sha256,
            maximum_bytes,
            self,
        )
    }

    /// perform no edge decompression for the exact empty freeze
    fn gunzip_bounded(&mut self, _: &[u8], _: usize) -> Option<Vec<u8>> {
        unreachable!("empty edge")
    }

    /// perform no edge recompression for the exact empty freeze
    fn gzip_deterministic(&mut self, _: &[u8]) -> Option<Vec<u8>> {
        unreachable!("empty edge")
    }
}

#[derive(Default)]
pub(crate) struct FixtureDigest(Vec<u8>);

impl AdjustmentEvaluationMemberDigest for FixtureDigest {
    /// record the exact consumed mock gzip byte population
    fn update(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    /// resolve the exact fixture's compressed digest binding
    fn finish(self) -> Option<Sha256Digest> {
        fixture_digest(&self.0)
    }
}

impl AdjustmentEvaluationStreamingValues for FixtureValues {
    type Digest = FixtureDigest;
    type Inflated<R: Read> = Cursor<Vec<u8>>;

    /// start one distinct test digest with no accumulated bytes
    fn begin_sha256(&mut self) -> Self::Digest {
        FixtureDigest::default()
    }

    /// drain and identify the exact simulated gzip stream
    fn gunzip_stream<R: Read>(&mut self, mut compressed: R) -> io::Result<Self::Inflated<R>> {
        let mut bytes = Vec::new();
        compressed.read_to_end(&mut bytes)?;
        assert_eq!(bytes, EMPTY_ROW_MEMBER_BYTES);
        let mut decoded = Vec::new();
        // emit each declared fixture row with its original lf frame
        for (line, _) in &self.rows {
            decoded.extend_from_slice(line);
            decoded.push(b'\n');
        }
        Ok(Cursor::new(decoded))
    }
}

pub(crate) struct UnusedBodyInspection;

impl AdjustmentEvaluationBodyInspection for UnusedBodyInspection {
    /// stop the fixture if an unplanned body reaches inspection
    fn contains_private_material(&mut self, _: &[u8]) -> bool {
        unreachable!("empty rows")
    }

    /// stop the fixture if an unplanned body reaches final inspection
    fn finish_contains_private_material(self) -> bool {
        unreachable!("empty rows")
    }
}

impl AdjustmentEvaluationPackageVerificationValues for FixtureValues {
    type OpenedMember = NativeAdjustmentEvaluationOpenedMember;
    type BodyInspection = UnusedBodyInspection;

    /// bind the real fixture's opened row descriptor
    fn open_declared_member(
        &mut self,
        root: &str,
        member_path: &str,
    ) -> io::Result<Self::OpenedMember> {
        NativeAdjustmentEvaluationPackageFiles.open_member(root, member_path)
    }

    /// stop the fixture if a body is unexpectedly declared
    fn begin_body_inspection(&mut self) -> Self::BodyInspection {
        unreachable!("empty rows")
    }

    /// stop the fixture if a row is unexpectedly declared
    fn receipt_is_later(&mut self, _: &JsonValue, _: &JsonValue) -> bool {
        unreachable!("empty rows")
    }
}

/// create one empty retained package with real files and mock codec owners
pub(crate) fn empty_package_fixture() -> (FixtureDirectory, FixtureValues) {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "weather-adjustment-strict-fixture-{}-{sequence}",
        std::process::id(),
    ));
    fs::create_dir(&root).expect("new strict fixture");
    let scratch = FixtureDirectory(root);
    fs::create_dir(scratch.0.join("members")).expect("fixture members");
    fs::write(scratch.0.join("manifest.json"), MANIFEST_BYTES).expect("fixture manifest");
    fs::write(
        scratch.0.join("manifest.sha256"),
        format!("{MANIFEST_DIGEST}\n"),
    )
    .expect("fixture digest");
    fs::write(scratch.0.join(ROW_MEMBER), EMPTY_ROW_MEMBER_BYTES).expect("fixture row member");

    let mut manifest = fixture_tests::manifest();
    replace_field(&mut manifest, "maxSourceReceiptAt", |field| {
        *field = JsonValue::Null
    });
    replace_field(&mut manifest, "revisionDiagnostics", |diagnostics| {
        replace_field(diagnostics, "maxRevisionCount", |field| {
            *field = JsonValue::Number(0.0)
        });
    });
    replace_field(&mut manifest, "edgeEvidence", |edge| {
        replace_field(edge, "receiptCount", |field| {
            *field = JsonValue::Number(0.0)
        });
    });
    replace_field(&mut manifest, "members", |members| {
        let JsonValue::Array(members) = members else {
            panic!("fixture members")
        };
        members.remove(0);
        replace_field(&mut members[0], "sizeBytes", |field| {
            *field = JsonValue::Number(EMPTY_ROW_MEMBER_BYTES.len() as f64);
        });
    });

    let values = FixtureValues {
        manifest,
        control_reads: Vec::new(),
        rows: Vec::new(),
    };
    (scratch, values)
}

/// create the nonempty adjustment package used by the original temperature command fixture
pub(crate) fn temperature_package_fixture() -> (FixtureDirectory, FixtureValues) {
    let (directory, mut values) = empty_package_fixture();
    let hash = "a".repeat(64);
    let (run_at, valid_at, receipt) = (
        "2026-10-01T00:00:00.000Z",
        "2026-10-01T07:00:00.000Z",
        "2026-10-01T06:03:00.000Z",
    );
    let source = "ecmwf-temperature-canary:1";
    let common = |kind, key, payload| {
        wire_object(vec![
            ("body_member_path", JsonValue::Null),
            ("content_hash", wire(&hash)),
            ("first_received_at", wire(receipt)),
            ("last_received_at", wire(receipt)),
            ("local_date", wire("2026-10-01")),
            ("payload", payload),
            ("record_kind", wire(kind)),
            ("record_revision_identity", wire(key)),
            ("reference_at", wire(run_at)),
            ("revision_count", JsonValue::Number(0.0)),
            ("site_key", wire("ballydidean")),
            ("source_identity", wire(source)),
            ("valid_at", wire(valid_at)),
        ])
    };
    let run = common(
        "temperature_run",
        "ecmwf-temperature-canary-run:1",
        wire_object(vec![
            ("adapterVersion", wire("ecmwf-temperature/v1")),
            ("contentHash", wire(&hash)),
            ("firstReceivedAt", wire(receipt)),
            ("id", JsonValue::Number(1.0)),
            ("lastReceivedAt", wire(receipt)),
            ("modelCycle", wire("00z")),
            ("providerResponseSha256", wire(&hash)),
            ("recentErrorState", JsonValue::Null),
            ("recentErrorStateSha256", JsonValue::Null),
            ("revisionCount", JsonValue::Number(0.0)),
            ("runInitializedAt", wire(run_at)),
            ("stateReason", JsonValue::Null),
            ("stateStatus", wire("ready")),
            ("upstreamModel", wire("ecmwf_ifs025")),
        ]),
    );
    let hour = common(
        "temperature_hour",
        "ecmwf-temperature-canary-hour:1:7",
        wire_object(vec![
            ("contentHash", wire(&hash)),
            ("modelLeadHours", JsonValue::Number(7.0)),
            ("rawRelativeHumidityPercent", JsonValue::Number(80.0)),
            ("rawTemperatureC", JsonValue::Number(12.5)),
            ("rawWindSpeedMps", JsonValue::Number(3.1)),
            ("runId", JsonValue::Number(1.0)),
            ("validAt", wire(valid_at)),
        ]),
    );
    values.rows = vec![
        (b"fixture-temperature-run", run),
        (b"fixture-temperature-hour", hour),
    ];
    let bytes = values
        .rows
        .iter()
        .map(|(line, _)| line.len() + 1)
        .sum::<usize>();
    replace_field(&mut values.manifest, "canonicalStreamBytes", |field| {
        *field = JsonValue::Number(bytes as f64)
    });
    replace_field(&mut values.manifest, "revisionDiagnostics", |diagnostics| {
        replace_field(diagnostics, "rowCount", |field| {
            *field = JsonValue::Number(2.0)
        });
    });
    replace_field(&mut values.manifest, "maxSourceReceiptAt", |field| {
        *field = wire(receipt)
    });
    replace_field(&mut values.manifest, "sourceIdentities", |field| {
        *field = JsonValue::Array(vec![wire(source)]);
    });
    (directory, values)
}

/// verify an entire empty retained package through actual files and mock codec owners
#[test]
fn adjustment_strict_verifier_loads_real_files_and_rereads_controls() {
    let (scratch, mut values) = empty_package_fixture();
    let result = verify_adjustment_evaluation_package(
        scratch.path().to_str().expect("UTF-8 fixture root"),
        &mut values,
    )
    .expect("complete empty v1 original package");
    assert_eq!(result.row_count, 0);
    assert_eq!(result.manifest_sha256, MANIFEST_DIGEST);
    assert_eq!(
        values.control_reads(),
        [
            "manifest.json",
            "manifest.sha256",
            "manifest.json",
            "manifest.sha256"
        ],
    );
}

/// adjust one known input field on a test fixture only
fn replace_field(value: &mut JsonValue, key: &str, replace: impl FnOnce(&mut JsonValue)) {
    let JsonValue::Object(fields) = value else {
        panic!("fixture object")
    };
    let (_, field) = fields
        .iter_mut()
        .find(|(field, _)| field.iter().copied().eq(key.encode_utf16()))
        .expect("existing fixture key");
    replace(field);
}

/// construct one test-only canonical string value
fn wire(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// construct one typed object in the original field order
fn wire_object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// resolve only the named transaction and record fixtures
fn fixture_epoch(value: &JsonValue) -> Option<i64> {
    let JsonValue::String(units) = value else {
        return None;
    };
    // keep all fixture event boundaries distinct in verification
    match String::from_utf16(units).ok()?.as_str() {
        "2026-10-01T00:00:00.000Z" => Some(0),
        "2026-10-01T06:03:00.000Z" => Some(21_780_000),
        "2026-10-01T07:00:00.000Z" => Some(25_200_000),
        "2026-10-01T12:00:00.000Z" => Some(43_200_000),
        _ => None,
    }
}
