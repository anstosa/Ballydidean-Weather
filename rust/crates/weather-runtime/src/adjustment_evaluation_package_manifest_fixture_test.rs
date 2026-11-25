use std::collections::BTreeMap;
use std::fmt::{self, Display};

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use super::*;

const ROOT: &str = "/fixture/adjustment";
const MANIFEST_BYTES: &[u8] = b"decoded adjustment manifest fixture";
const MANIFEST_SHA256: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LEDGER_SHA256: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const WATERMARK_SHA256: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ROW_SHA256: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const RECEIPT_SHA256: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

#[derive(Debug, Clone, Copy)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// Return one stable fixture effect failure.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

struct FixtureValues {
    files: BTreeMap<String, Vec<u8>>,
    manifest: JsonValue,
}

impl FixtureValues {
    /// Build one complete v1 manifest with deliberately nonlexical member order.
    fn complete() -> Self {
        let manifest = manifest();
        let files = BTreeMap::from([
            (MANIFEST_PATH.to_owned(), MANIFEST_BYTES.to_vec()),
            (
                MANIFEST_HASH_PATH.to_owned(),
                format!("{MANIFEST_SHA256}\n").into_bytes(),
            ),
            (
                format!("members/edge/receipts/sha256-{RECEIPT_SHA256}.json"),
                Vec::new(),
            ),
            ("members/rows.jsonl.gz".to_owned(), Vec::new()),
        ]);
        Self { files, manifest }
    }
}

impl AdjustmentEvaluationPackageManifestValues for FixtureValues {
    type Error = FixtureError;

    /// Read only the exact bounded fixture control files.
    fn read_bounded_regular_file(
        &mut self,
        root: &str,
        relative_path: &str,
        maximum_bytes: u64,
        _description: &str,
    ) -> Result<Vec<u8>, Self::Error> {
        let bytes = self
            .files
            .get(relative_path)
            .filter(|bytes| root == ROOT && bytes.len() as u64 <= maximum_bytes)
            .cloned()
            .ok_or(FixtureError("fixture control read failed"))?;
        Ok(bytes)
    }

    /// Decode the sole exact manifest byte population.
    fn decode_json(&mut self, bytes: &[u8]) -> Result<JsonValue, Self::Error> {
        // admit only the bytes snapshotted by the fixture read
        if bytes != MANIFEST_BYTES {
            return Err(FixtureError("fixture manifest decode failed"));
        }
        Ok(self.manifest.clone())
    }

    /// Return the exact fixture manifest or migration-ledger digest.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        Some(Sha256Digest::admitted(if bytes == MANIFEST_BYTES {
            MANIFEST_SHA256
        } else {
            LEDGER_SHA256
        }))
    }

    /// Admit the two exact fixture timestamps.
    fn utc_epoch_milliseconds(&mut self, instant: &JsonValue) -> Option<i64> {
        // retain only the fixture's string-valued Date.parse cases
        let JsonValue::String(units) = instant else {
            return None;
        };
        let instant = String::from_utf16(units).ok()?;
        // keep both original timestamp fields real and distinct
        match instant.as_str() {
            "2026-10-01T12:00:00.000Z" => Some(0),
            "2026-10-01T13:00:00.000Z" => Some(3_600_000),
            _ => None,
        }
    }

    /// Admit the sole fixture calendar date.
    fn local_date_ordinal(&mut self, date: &str) -> Option<i64> {
        (date == "2026-10-01").then_some(0)
    }

    /// Return every fixture file in lexical filesystem order.
    fn collect_regular_files(&mut self, root: &str) -> Result<Vec<String>, Self::Error> {
        // reject inventory requests outside the sole fixture root
        if root != ROOT {
            return Err(FixtureError("fixture inventory failed"));
        }
        Ok(self.files.keys().cloned().collect())
    }
}

/// Admit the complete manifest, preserving typed summaries and source member order.
#[test]
fn admits_v1_manifest_and_original_order_member_table() {
    let mut values = FixtureValues::complete();
    let first = read_adjustment_evaluation_package_manifest(ROOT, &mut values)
        .expect("first manifest snapshot should be admitted");
    let rebound = read_adjustment_evaluation_package_manifest(ROOT, &mut values)
        .expect("final manifest snapshot should be admitted");

    assert_eq!(rebound.manifest_bytes, first.manifest_bytes);
    assert_eq!(rebound.manifest_sha256, first.manifest_sha256);
    assert_eq!(rebound.manifest, first.manifest);
    let admitted = admit_adjustment_evaluation_package_manifest(ROOT, &mut values)
        .expect("complete v1 manifest should be admitted");

    assert_eq!(admitted.manifest_sha256, MANIFEST_SHA256);
    assert_eq!(admitted.edge_watermark_sha256, WATERMARK_SHA256);
    assert_eq!(
        (admitted.edge_object_count, admitted.edge_receipt_count),
        (0.0, 1.0)
    );
    assert_eq!(
        admitted.max_source_receipt_at,
        Some(string("2026-10-01T13:00:00.000Z"))
    );
    assert_eq!(admitted.max_revision_count, 18_014_398_509_481_984.0);
    assert_eq!((admitted.revised_row_count, admitted.row_count), (0, 0));
    assert_eq!(admitted.member_bytes, 0);
    assert_eq!(
        admitted
            .members
            .iter()
            .map(|member| member.path.clone())
            .collect::<Vec<_>>(),
        vec![
            format!("members/edge/receipts/sha256-{RECEIPT_SHA256}.json"),
            "members/rows.jsonl.gz".to_owned(),
        ]
    );
    assert_eq!(
        admitted.member("members/rows.jsonl.gz"),
        admitted.members.get(1)
    );
}

/// Build one complete v1 manifest using the original 0023 database ledger.
pub(crate) fn manifest() -> JsonValue {
    object(vec![
        ("canonicalStreamBytes", number(0.0)),
        ("compressedBodyBytes", number(0.0)),
        ("contractVersion", string(PACKAGE_VERSION)),
        ("databaseManifest", database_manifest()),
        (
            "edgeEvidence",
            object(vec![
                ("contractVersion", string(EDGE_SNAPSHOT_VERSION)),
                ("frozenAt", string("2026-10-01T12:00:00.000Z")),
                ("objectCount", number(0.0)),
                ("receiptCount", number(1.0)),
                ("watermarkSha256", string(WATERMARK_SHA256)),
            ]),
        ),
        ("fromLocalDate", string("2026-10-01")),
        ("limits", limits()),
        ("maxSourceReceiptAt", string("2026-10-01T13:00:00.000Z")),
        (
            "members",
            JsonValue::Array(vec![
                member(
                    &format!("members/edge/receipts/sha256-{RECEIPT_SHA256}.json"),
                    RECEIPT_SHA256,
                ),
                member("members/rows.jsonl.gz", ROW_SHA256),
            ]),
        ),
        (
            "revisionDiagnostics",
            object(vec![
                ("maxRevisionCount", number(18_014_398_509_481_984.0)),
                ("revisedRowCount", number(0.0)),
                ("rowCount", number(0.0)),
            ]),
        ),
        ("siteKey", string("ballydidean")),
        ("siteTimezone", string("America/Los_Angeles")),
        ("sourceIdentities", JsonValue::Array(Vec::new())),
        ("toLocalDate", string("2026-10-01")),
        (
            "transaction",
            object(vec![
                ("createdAtUtc", string("2026-10-01T12:00:00.000Z")),
                ("idleInTransactionSessionTimeout", string("30s")),
                ("isolationLevel", string("repeatable read")),
                ("lockTimeout", string("5s")),
                ("readOnly", string("on")),
                ("statementTimeout", string("5min")),
            ]),
        ),
    ])
}

/// Build the exact published resource-limit object.
fn limits() -> JsonValue {
    object(vec![
        ("maxArchiveBytes", number(MAXIMUM_ARCHIVE_BYTES as f64)),
        (
            "maxCanonicalStreamBytes",
            number(MAXIMUM_CANONICAL_BYTES as f64),
        ),
        (
            "maxCompressedBodyBytes",
            number(MAXIMUM_COMPRESSED_BODY_BYTES as f64),
        ),
        ("maxDays", number(MAXIMUM_DAYS as f64)),
        (
            "maxDecompressionRatio",
            number(MAXIMUM_DECOMPRESSION_RATIO as f64),
        ),
        (
            "maxOtherSimultaneousTemporaryBytes",
            number(MAXIMUM_OTHER_TEMPORARY_BYTES as f64),
        ),
        ("maxRows", number(MAXIMUM_ROWS as f64)),
        (
            "maxSimultaneousHostTemporaryBytes",
            number(MAXIMUM_TEMPORARY_BYTES as f64),
        ),
    ])
}

/// Build the exact reviewed database manifest and 0023 migration prefix.
fn database_manifest() -> JsonValue {
    object(vec![
        ("contract_version", string(DATABASE_MANIFEST_VERSION)),
        (
            "migration_checksums",
            string_array(MIGRATIONS.iter().map(|entry| entry.1)),
        ),
        ("migration_history_sha256", string(LEDGER_SHA256)),
        (
            "migration_names",
            string_array(MIGRATIONS.iter().map(|entry| entry.0)),
        ),
        ("query_contract_sha256", string(QUERY_CONTRACT_SHA256)),
        ("query_contract_version", string(QUERY_CONTRACT_VERSION)),
        ("row_schema_sha256", string(ROW_SCHEMA_SHA256)),
        ("schema_migration", string(SCHEMA_MIGRATION)),
        ("site_key", string("ballydidean")),
        ("site_timezone", string("America/Los_Angeles")),
    ])
}

/// Build one exact immutable member declaration.
fn member(path: &str, sha256: &str) -> JsonValue {
    object(vec![
        ("path", string(path)),
        ("sha256", string(sha256)),
        ("sizeBytes", number(0.0)),
    ])
}

/// Build one dependency-free JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one dependency-free JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one dependency-free JSON number.
fn number(value: f64) -> JsonValue {
    JsonValue::Number(value)
}

/// Build one dependency-free JSON string array.
fn string_array<'a>(values: impl Iterator<Item = &'a str>) -> JsonValue {
    JsonValue::Array(values.map(string).collect())
}
