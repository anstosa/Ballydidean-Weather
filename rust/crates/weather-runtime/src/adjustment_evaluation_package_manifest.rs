//! Admission of one adjustment-evaluation v1 manifest and its declared member table.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display};

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};

const PACKAGE_VERSION: &str = "adjustment-evaluation-export-package/v1";
const DATABASE_MANIFEST_VERSION: &str = "adjustment-evaluation-export-manifest/v1";
const EDGE_SNAPSHOT_VERSION: &str = "forecast-adjustment-evidence-snapshot/v1";
const QUERY_CONTRACT_VERSION: &str = "adjustment-evaluation-export-query/v1";
const SCHEMA_MIGRATION: &str = "0017_adjustment_evaluation_export.sql";
const ROW_SCHEMA_SHA256: &str = "c21782034130d003e4af48fada4c8f6545f6f1c4dff550f92d0523c7d96d20e2";
const QUERY_CONTRACT_SHA256: &str =
    "c860039c72818a9b813ed9f9d93f5d8e115f4144e99d698734e9159ca3457bd4";
const MANIFEST_PATH: &str = "manifest.json";
const MANIFEST_HASH_PATH: &str = "manifest.sha256";
const MAXIMUM_DAYS: i64 = 14;
const MAXIMUM_ROWS: u64 = 8_192;
const MAXIMUM_COMPRESSED_BODY_BYTES: u64 = 32 * 1_024 * 1_024;
const MAXIMUM_CANONICAL_BYTES: u64 = 48 * 1_024 * 1_024;
const MAXIMUM_ARCHIVE_BYTES: u64 = 48 * 1_024 * 1_024;
const MAXIMUM_TEMPORARY_BYTES: u64 = 64 * 1_024 * 1_024;
const MAXIMUM_OTHER_TEMPORARY_BYTES: u64 = 16 * 1_024 * 1_024;
const MAXIMUM_DECOMPRESSION_RATIO: u64 = 64;

const MANIFEST_KEYS: &[&str] = &[
    "canonicalStreamBytes",
    "compressedBodyBytes",
    "contractVersion",
    "databaseManifest",
    "edgeEvidence",
    "fromLocalDate",
    "limits",
    "maxSourceReceiptAt",
    "members",
    "revisionDiagnostics",
    "siteKey",
    "siteTimezone",
    "sourceIdentities",
    "toLocalDate",
    "transaction",
];
const LIMIT_KEYS: &[&str] = &[
    "maxArchiveBytes",
    "maxCanonicalStreamBytes",
    "maxCompressedBodyBytes",
    "maxDays",
    "maxDecompressionRatio",
    "maxOtherSimultaneousTemporaryBytes",
    "maxRows",
    "maxSimultaneousHostTemporaryBytes",
];
const EDGE_EVIDENCE_KEYS: &[&str] = &[
    "contractVersion",
    "frozenAt",
    "objectCount",
    "receiptCount",
    "watermarkSha256",
];
const REVISION_DIAGNOSTIC_KEYS: &[&str] = &["maxRevisionCount", "revisedRowCount", "rowCount"];
const TRANSACTION_KEYS: &[&str] = &[
    "createdAtUtc",
    "idleInTransactionSessionTimeout",
    "isolationLevel",
    "lockTimeout",
    "readOnly",
    "statementTimeout",
];
const DATABASE_MANIFEST_KEYS: &[&str] = &[
    "contract_version",
    "migration_checksums",
    "migration_history_sha256",
    "migration_names",
    "query_contract_sha256",
    "query_contract_version",
    "row_schema_sha256",
    "schema_migration",
    "site_key",
    "site_timezone",
];
const MEMBER_KEYS: &[&str] = &["path", "sha256", "sizeBytes"];

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_initial_weather.sql",
        "f4264482606a3476b4b54e3089e66c900f0ad5104ea678a9bf17e5e5ba385a62",
    ),
    (
        "0002_worker_migration_readiness.sql",
        "954731c9ddb791c85defc3bffd887a3d30e9ff9fb5c9dac96e050ac52a796f03",
    ),
    (
        "0003_ecowitt_measurements.sql",
        "250e9b49176fda732b23f797a6dbce6878ca93818fa1bf6a408b5d6611ee5a48",
    ),
    (
        "0004_tempest_metadata.sql",
        "5487c90a4e70f8e049f9ed48c3ee3f025eb9f1908ed04a1febfb736c22f30d34",
    ),
    (
        "0005_source_supersession.sql",
        "462be10d1a35419b52068d09ae5254134e0168fed0ada66f957c87d290c8e6f1",
    ),
    (
        "0006_station_coordinates.sql",
        "33f3fc5b1b475add0e862b45488a4a3482435df0ae674599005bafd37c3886a8",
    ),
    (
        "0007_tide_sources.sql",
        "4e2ed3037ea82011e947e48102149a6ea6456e5fb9f504436f7ec35f6e191390",
    ),
    (
        "0008_ecowitt_property_sensors.sql",
        "3934568dcdbb840f5026a09bab676bede7ad7d2571c1d7415efdabb902dd8b81",
    ),
    (
        "0009_forecast_anchor_records.sql",
        "e144415b6ac8e19338cc4b53d7daeffbf3d32650af880ab7acb809c6f71ded20",
    ),
    (
        "0010_forecast_training_export.sql",
        "2590423b7787b0e5e5668bfa4af21db775b8926540b1d2e339d6656aa3f27dc8",
    ),
    (
        "0011_forecast_runtime_provenance.sql",
        "ebf5789636095982e6010a868cdfdd4d7449ca79fb1057adf3b8dd8f49a337dc",
    ),
    (
        "0012_hide_archive_only_forecasts_from_live_reads.sql",
        "663ed323104bb70f4d0e735c3b546fb111038be4d0fee83e0c76c018cf0da15c",
    ),
    (
        "0013_ecmwf_temperature_canary.sql",
        "e05f0397529108641ad9b9eb5381ac968f35449514fe4d8ed0ac502c9d5bf3ee",
    ),
    (
        "0014_rain_collection.sql",
        "2a311e2effcc975442c1011c627c125521cc4396323b32f0d0c4f92ea1ccba04",
    ),
    (
        "0015_rain_station_access.sql",
        "2f560ccec001246a3caa5d27476900b2694dfb7cc0701b35181265ef1fb78822",
    ),
    (
        "0016_rain_adjustment.sql",
        "9fa5659c032dc21fdf82dca693fa962d8b5212d1192ae35c4cfd04bef94be5f9",
    ),
    (
        "0017_adjustment_evaluation_export.sql",
        "6f18210453a18f0deefe95a0b70657cc4c4dd3fc20d0bab72f847ee04958ab90",
    ),
    (
        "0018_adjustment_maintenance_v2.sql",
        "c13d2c2c39096887712ae97f0b863f8a7ab52074ca320575f8c4c1f9b72a50a3",
    ),
    (
        "0019_adjustment_maintenance_recurring.sql",
        "f2d9fa34a041449443741963833de798d696eb3c6096c6bd02f81769619dfd44",
    ),
    (
        "0020_adjustment_revision_frontier.sql",
        "56027b4f2cb2c3c83e746f14dac753b75d573753934898fa7a52f78faa56499c",
    ),
    (
        "0021_adjustment_rolling_registration.sql",
        "ca29db99377001fca2e2e4268fd80d1cbe7b876f71f2d5ba04e575712cb9f13b",
    ),
    (
        "0022_forecast_product_catalog.sql",
        "8d04170868414cd17e78cde2a4d6dd8144dfd606cba4e171686523f045cb77d2",
    ),
    (
        "0023_forecast_product_read_index.sql",
        "e637542cb2e742fe8f22f60313a11f00adbc1d140a811ee718524849fcece12a",
    ),
];

/// Approved JSON, digest, clock, calendar, and package file owners.
pub trait AdjustmentEvaluationPackageManifestValues {
    type Error: Display;

    /// Read one no-follow regular control file within the supplied byte bound.
    fn read_bounded_regular_file(
        &mut self,
        root: &str,
        relative_path: &str,
        maximum_bytes: u64,
        description: &str,
    ) -> Result<Vec<u8>, Self::Error>;

    /// Decode one ordinary JSON value with the selected production codec.
    fn decode_json(&mut self, bytes: &[u8]) -> Result<JsonValue, Self::Error>;

    /// Hash one exact byte population with the selected SHA-256 owner.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest>;

    /// Parse one JSON value with the original `Date.parse` coercion contract.
    fn utc_epoch_milliseconds(&mut self, instant: &JsonValue) -> Option<i64>;

    /// Return a monotonic ordinal for one real canonical local date.
    fn local_date_ordinal(&mut self, date: &str) -> Option<i64>;

    /// Enumerate every real regular file below the fixed package root.
    fn collect_regular_files(&mut self, root: &str) -> Result<Vec<String>, Self::Error>;
}

/// One immutable package member admitted before its bytes are opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjustmentEvaluationPackageMember {
    pub path: String,
    pub sha256: Sha256Digest,
    pub size_bytes: u64,
}

/// The admitted manifest identity and complete declared member table.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedAdjustmentEvaluationPackageManifest {
    pub canonical_stream_bytes: u64,
    pub compressed_body_bytes: u64,
    pub edge_object_count: f64,
    pub edge_receipt_count: f64,
    pub edge_watermark_sha256: String,
    pub from_local_date: String,
    pub manifest: JsonValue,
    pub manifest_bytes: Vec<u8>,
    pub manifest_sha256: String,
    pub max_revision_count: f64,
    pub max_source_receipt_at: Option<JsonValue>,
    pub member_bytes: u64,
    pub members: Vec<AdjustmentEvaluationPackageMember>,
    members_by_path: BTreeMap<String, usize>,
    pub revised_row_count: u64,
    pub row_count: u64,
    pub source_identities: Vec<Vec<u16>>,
    pub to_local_date: String,
}

impl AdmittedAdjustmentEvaluationPackageManifest {
    /// return the original ordered member at its admitted path index
    pub fn member(&self, path: &str) -> Option<&AdjustmentEvaluationPackageMember> {
        self.members_by_path
            .get(path)
            .and_then(|index| self.members.get(*index))
    }
}

/// One strictly read and admitted manifest snapshot before member inventory checks.
#[derive(Debug, Clone, PartialEq)]
pub struct AdjustmentEvaluationPackageManifestSnapshot {
    pub canonical_stream_bytes: u64,
    pub compressed_body_bytes: u64,
    pub edge_object_count: f64,
    pub edge_receipt_count: f64,
    pub edge_watermark_sha256: String,
    pub from_local_date: String,
    pub manifest: JsonValue,
    pub manifest_bytes: Vec<u8>,
    pub manifest_sha256: String,
    pub max_revision_count: f64,
    pub max_source_receipt_at: Option<JsonValue>,
    pub revised_row_count: u64,
    pub row_count: u64,
    pub source_identities: Vec<Vec<u16>>,
    pub to_local_date: String,
}

/// Closed ordinary failures from manifest and declaration admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdjustmentEvaluationPackageManifestError {
    Effect(String),
    ManifestHash,
    Object(&'static str),
    Keys(&'static str),
    Contract,
    Limits,
    EdgeEvidence,
    RevisionDiagnostics,
    Transaction,
    Date(&'static str),
    DateRange,
    DatabaseIdentity,
    MemberPath,
    MemberIdentity,
    MemberAllocation,
    Inventory,
}

impl Display for AdjustmentEvaluationPackageManifestError {
    /// Preserve the predecessor's stable package diagnostics.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // retain exact text for each closed validation stage
        match self {
            Self::Effect(message) => formatter.write_str(message),
            Self::ManifestHash => formatter.write_str("package manifest hash differs"),
            Self::Object(description) => write!(formatter, "{description} must be an object"),
            Self::Keys(description) => write!(formatter, "{description} has unexpected keys"),
            Self::Contract => formatter.write_str("package manifest contract is invalid"),
            Self::Limits => formatter.write_str("package limits are invalid"),
            Self::EdgeEvidence => formatter.write_str("edge evidence identity is invalid"),
            Self::RevisionDiagnostics => formatter.write_str("revision diagnostics are invalid"),
            Self::Transaction => {
                formatter.write_str("package transaction or source identities are invalid")
            }
            Self::Date(description) => write!(formatter, "{description} must use YYYY-MM-DD"),
            Self::DateRange => {
                formatter.write_str("export range must contain 1 to 14 inclusive dates")
            }
            Self::DatabaseIdentity => formatter.write_str("database manifest identity is invalid"),
            Self::MemberPath => formatter.write_str("package member path is invalid"),
            Self::MemberIdentity => formatter.write_str("package member identity is invalid"),
            Self::MemberAllocation => {
                formatter.write_str("package member allocation exceeds the host temporary limit")
            }
            Self::Inventory => formatter.write_str("package contains undeclared files"),
        }
    }
}

impl std::error::Error for AdjustmentEvaluationPackageManifestError {}

/// Read and admit the control-file snapshot used both before and after member verification.
pub fn read_adjustment_evaluation_package_manifest(
    root: &str,
    values: &mut impl AdjustmentEvaluationPackageManifestValues,
) -> Result<AdjustmentEvaluationPackageManifestSnapshot, AdjustmentEvaluationPackageManifestError> {
    let manifest_bytes = values
        .read_bounded_regular_file(
            root,
            MANIFEST_PATH,
            MAXIMUM_OTHER_TEMPORARY_BYTES,
            "package manifest",
        )
        .map_err(effect)?;
    let hash_bytes = values
        .read_bounded_regular_file(root, MANIFEST_HASH_PATH, 65, "package manifest hash")
        .map_err(effect)?;
    let recorded_hash = String::from_utf8_lossy(&hash_bytes).trim().to_owned();
    let manifest_sha256 = values
        .sha256(&manifest_bytes)
        .ok_or(AdjustmentEvaluationPackageManifestError::ManifestHash)?;

    // bind the hash control file to the exact decoded manifest bytes
    if !valid_hash(&recorded_hash) || recorded_hash != manifest_sha256.as_str() {
        return Err(AdjustmentEvaluationPackageManifestError::ManifestHash);
    }

    let manifest = values.decode_json(&manifest_bytes).map_err(effect)?;
    let admitted_manifest = validate_manifest(&manifest, values)?;
    Ok(AdjustmentEvaluationPackageManifestSnapshot {
        canonical_stream_bytes: admitted_manifest.canonical_stream_bytes,
        compressed_body_bytes: admitted_manifest.compressed_body_bytes,
        edge_object_count: admitted_manifest.edge.object_count,
        edge_receipt_count: admitted_manifest.edge.receipt_count,
        edge_watermark_sha256: admitted_manifest.edge.watermark_sha256,
        from_local_date: admitted_manifest.from_local_date,
        manifest,
        manifest_bytes,
        manifest_sha256: manifest_sha256.as_str().to_owned(),
        max_revision_count: admitted_manifest.revisions.max_revision_count,
        max_source_receipt_at: admitted_manifest.max_source_receipt_at,
        revised_row_count: admitted_manifest.revisions.revised_row_count,
        row_count: admitted_manifest.revisions.row_count,
        source_identities: admitted_manifest.source_identities,
        to_local_date: admitted_manifest.to_local_date,
    })
}

/// Read, admit, and inventory one complete adjustment package manifest boundary.
pub fn admit_adjustment_evaluation_package_manifest(
    root: &str,
    values: &mut impl AdjustmentEvaluationPackageManifestValues,
) -> Result<AdmittedAdjustmentEvaluationPackageManifest, AdjustmentEvaluationPackageManifestError> {
    let snapshot = read_adjustment_evaluation_package_manifest(root, values)?;
    let declarations = validate_member_declarations(&snapshot.manifest)?;
    let mut actual_files = values.collect_regular_files(root).map_err(effect)?;
    actual_files.sort();
    let expected_files = declarations
        .by_path
        .keys()
        .cloned()
        .chain([MANIFEST_PATH.to_owned(), MANIFEST_HASH_PATH.to_owned()])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    // reject any missing, undeclared, or differently spelled file
    if actual_files != expected_files {
        return Err(AdjustmentEvaluationPackageManifestError::Inventory);
    }

    Ok(AdmittedAdjustmentEvaluationPackageManifest {
        canonical_stream_bytes: snapshot.canonical_stream_bytes,
        compressed_body_bytes: snapshot.compressed_body_bytes,
        edge_object_count: snapshot.edge_object_count,
        edge_receipt_count: snapshot.edge_receipt_count,
        edge_watermark_sha256: snapshot.edge_watermark_sha256,
        from_local_date: snapshot.from_local_date,
        manifest: snapshot.manifest,
        manifest_bytes: snapshot.manifest_bytes,
        manifest_sha256: snapshot.manifest_sha256,
        max_revision_count: snapshot.max_revision_count,
        max_source_receipt_at: snapshot.max_source_receipt_at,
        member_bytes: declarations.bytes,
        members: declarations.ordered,
        members_by_path: declarations.by_path,
        revised_row_count: snapshot.revised_row_count,
        row_count: snapshot.row_count,
        source_identities: snapshot.source_identities,
        to_local_date: snapshot.to_local_date,
    })
}

struct AdmittedManifestSummary {
    canonical_stream_bytes: u64,
    compressed_body_bytes: u64,
    edge: AdmittedEdgeEvidence,
    from_local_date: String,
    max_source_receipt_at: Option<JsonValue>,
    revisions: AdmittedRevisionDiagnostics,
    source_identities: Vec<Vec<u16>>,
    to_local_date: String,
}

struct AdmittedEdgeEvidence {
    object_count: f64,
    receipt_count: f64,
    watermark_sha256: String,
}

struct AdmittedRevisionDiagnostics {
    max_revision_count: f64,
    revised_row_count: u64,
    row_count: u64,
}

/// Validate the complete v1 manifest before inspecting its member table.
fn validate_manifest(
    manifest: &JsonValue,
    values: &mut impl AdjustmentEvaluationPackageManifestValues,
) -> Result<AdmittedManifestSummary, AdjustmentEvaluationPackageManifestError> {
    require_exact_keys(manifest, MANIFEST_KEYS, "package manifest")?;

    // bind the fixed package, site, and timezone identities first
    if text(manifest, "contractVersion").as_deref() != Some(PACKAGE_VERSION)
        || text(manifest, "siteKey").as_deref() != Some("ballydidean")
        || text(manifest, "siteTimezone").as_deref() != Some("America/Los_Angeles")
    {
        return Err(AdjustmentEvaluationPackageManifestError::Contract);
    }

    let (canonical_stream_bytes, compressed_body_bytes) = validate_limits(
        manifest,
        required_object(manifest, "limits", "package limits")?,
    )?;
    let edge = validate_edge_evidence(
        required_object(manifest, "edgeEvidence", "edge evidence")?,
        values,
    )?;
    let revisions = validate_revision_diagnostics(required_object(
        manifest,
        "revisionDiagnostics",
        "revision diagnostics",
    )?)?;
    let (max_source_receipt_at, source_identities) =
        validate_transaction_and_sources(manifest, values)?;
    let (from_local_date, to_local_date) = validate_date_range(manifest, values)?;
    validate_database_manifest(
        required_object(manifest, "databaseManifest", "database manifest")?,
        values,
    )?;
    Ok(AdmittedManifestSummary {
        canonical_stream_bytes,
        compressed_body_bytes,
        edge,
        from_local_date,
        max_source_receipt_at,
        revisions,
        source_identities,
        to_local_date,
    })
}

/// Bind every published package allocation limit and summary byte count.
fn validate_limits(
    manifest: &JsonValue,
    limits: &JsonValue,
) -> Result<(u64, u64), AdjustmentEvaluationPackageManifestError> {
    require_exact_keys(limits, LIMIT_KEYS, "package limits")?;
    let canonical_bytes = unsigned_field(manifest, "canonicalStreamBytes");
    let compressed_body_bytes = unsigned_field(manifest, "compressedBodyBytes");

    // bind every published resource limit and both bounded allocation summaries
    if unsigned_field(limits, "maxArchiveBytes") != Some(MAXIMUM_ARCHIVE_BYTES)
        || unsigned_field(limits, "maxCanonicalStreamBytes") != Some(MAXIMUM_CANONICAL_BYTES)
        || unsigned_field(limits, "maxCompressedBodyBytes") != Some(MAXIMUM_COMPRESSED_BODY_BYTES)
        || unsigned_field(limits, "maxDays") != Some(MAXIMUM_DAYS as u64)
        || unsigned_field(limits, "maxDecompressionRatio") != Some(MAXIMUM_DECOMPRESSION_RATIO)
        || unsigned_field(limits, "maxOtherSimultaneousTemporaryBytes")
            != Some(MAXIMUM_OTHER_TEMPORARY_BYTES)
        || unsigned_field(limits, "maxRows") != Some(MAXIMUM_ROWS)
        || unsigned_field(limits, "maxSimultaneousHostTemporaryBytes")
            != Some(MAXIMUM_TEMPORARY_BYTES)
        || canonical_bytes.is_none_or(|value| value > MAXIMUM_CANONICAL_BYTES)
        || compressed_body_bytes.is_none_or(|value| value > MAXIMUM_COMPRESSED_BODY_BYTES)
    {
        return Err(AdjustmentEvaluationPackageManifestError::Limits);
    }
    Ok((
        canonical_bytes.expect("checked canonical byte count"),
        compressed_body_bytes.expect("checked compressed body byte count"),
    ))
}

/// Validate one frozen edge-evidence summary.
fn validate_edge_evidence(
    edge: &JsonValue,
    values: &mut impl AdjustmentEvaluationPackageManifestValues,
) -> Result<AdmittedEdgeEvidence, AdjustmentEvaluationPackageManifestError> {
    require_exact_keys(edge, EDGE_EVIDENCE_KEYS, "edge evidence")?;
    let frozen_at = field(edge, "frozenAt");
    let watermark = text(edge, "watermarkSha256");
    let object_count = nonnegative_integer_field(edge, "objectCount");
    let receipt_count = nonnegative_integer_field(edge, "receiptCount");

    // retain the exact snapshot contract, timestamp, counts, and watermark
    if text(edge, "contractVersion").as_deref() != Some(EDGE_SNAPSHOT_VERSION)
        || frozen_at
            .and_then(|instant| values.utc_epoch_milliseconds(instant))
            .is_none()
        || watermark.as_ref().is_none_or(|value| !valid_hash(value))
        || object_count.is_none()
        || receipt_count.is_none()
    {
        return Err(AdjustmentEvaluationPackageManifestError::EdgeEvidence);
    }
    Ok(AdmittedEdgeEvidence {
        object_count: object_count.expect("checked object count"),
        receipt_count: receipt_count.expect("checked receipt count"),
        watermark_sha256: watermark.expect("checked edge watermark"),
    })
}

/// Validate one bounded revision summary.
fn validate_revision_diagnostics(
    diagnostics: &JsonValue,
) -> Result<AdmittedRevisionDiagnostics, AdjustmentEvaluationPackageManifestError> {
    require_exact_keys(
        diagnostics,
        REVISION_DIAGNOSTIC_KEYS,
        "revision diagnostics",
    )?;
    let Some(maximum_revision_count) = nonnegative_integer_field(diagnostics, "maxRevisionCount")
    else {
        return Err(AdjustmentEvaluationPackageManifestError::RevisionDiagnostics);
    };
    let Some(revised_row_count) = unsigned_field(diagnostics, "revisedRowCount") else {
        return Err(AdjustmentEvaluationPackageManifestError::RevisionDiagnostics);
    };
    let Some(row_count) = unsigned_field(diagnostics, "rowCount") else {
        return Err(AdjustmentEvaluationPackageManifestError::RevisionDiagnostics);
    };
    // retain the original row and revised-row bounds
    if row_count > MAXIMUM_ROWS || revised_row_count > row_count {
        return Err(AdjustmentEvaluationPackageManifestError::RevisionDiagnostics);
    }
    Ok(AdmittedRevisionDiagnostics {
        max_revision_count: maximum_revision_count,
        revised_row_count,
        row_count,
    })
}

/// Validate transaction settings, timestamp fields, members, and source identities.
fn validate_transaction_and_sources(
    manifest: &JsonValue,
    values: &mut impl AdjustmentEvaluationPackageManifestValues,
) -> Result<(Option<JsonValue>, Vec<Vec<u16>>), AdjustmentEvaluationPackageManifestError> {
    let transaction = required_object(manifest, "transaction", "package transaction")?;
    require_exact_keys(transaction, TRANSACTION_KEYS, "package transaction")?;
    let created_at = field(transaction, "createdAtUtc");
    let max_source_receipt_at = field(manifest, "maxSourceReceiptAt");
    let members = array(manifest, "members");
    let source_identities = array(manifest, "sourceIdentities");

    // bind transaction constants and both bounded arrays before date validation
    if created_at
        .and_then(|instant| values.utc_epoch_milliseconds(instant))
        .is_none()
        || text(transaction, "isolationLevel").as_deref() != Some("repeatable read")
        || text(transaction, "readOnly").as_deref() != Some("on")
        || text(transaction, "statementTimeout").as_deref() != Some("5min")
        || text(transaction, "lockTimeout").as_deref() != Some("5s")
        || text(transaction, "idleInTransactionSessionTimeout").as_deref() != Some("30s")
        || max_source_receipt_at.is_none()
        || max_source_receipt_at.is_some_and(|instant| {
            !matches!(instant, JsonValue::Null) && values.utc_epoch_milliseconds(instant).is_none()
        })
        || members.is_none()
        || source_identities.is_none_or(|identities| {
            identities
                .iter()
                .any(|identity| !matches!(identity, JsonValue::String(units) if units.len() <= 256))
        })
    {
        return Err(AdjustmentEvaluationPackageManifestError::Transaction);
    }
    // preserve source identity UTF-16 units in original manifest order
    let source_identities = source_identities
        .expect("checked source identities")
        .iter()
        .map(|identity| match identity {
            JsonValue::String(units) => units.clone(),
            _ => unreachable!("source identity type checked"),
        })
        .collect();
    Ok((
        max_source_receipt_at
            .filter(|value| !matches!(value, JsonValue::Null))
            .cloned(),
        source_identities,
    ))
}

/// Validate one real, nonempty, at-most-fourteen-day local-date interval.
fn validate_date_range(
    manifest: &JsonValue,
    values: &mut impl AdjustmentEvaluationPackageManifestValues,
) -> Result<(String, String), AdjustmentEvaluationPackageManifestError> {
    let from = text(manifest, "fromLocalDate")
        .filter(|date| canonical_date(date, values))
        .ok_or(AdjustmentEvaluationPackageManifestError::Date(
            "fromLocalDate",
        ))?;
    let to = text(manifest, "toLocalDate")
        .filter(|date| canonical_date(date, values))
        .ok_or(AdjustmentEvaluationPackageManifestError::Date(
            "toLocalDate",
        ))?;
    let from_ordinal =
        values
            .local_date_ordinal(&from)
            .ok_or(AdjustmentEvaluationPackageManifestError::Date(
                "fromLocalDate",
            ))?;
    let to_ordinal =
        values
            .local_date_ordinal(&to)
            .ok_or(AdjustmentEvaluationPackageManifestError::Date(
                "toLocalDate",
            ))?;
    let days = to_ordinal
        .checked_sub(from_ordinal)
        .and_then(|difference| difference.checked_add(1));

    // reject reversed, empty, or oversized inclusive ranges
    if !matches!(days, Some(1..=MAXIMUM_DAYS)) {
        return Err(AdjustmentEvaluationPackageManifestError::DateRange);
    }
    Ok((from, to))
}

/// Validate the unchanged query contract against one reviewed migration ledger.
pub(crate) fn validate_database_manifest(
    database: &JsonValue,
    values: &mut impl AdjustmentEvaluationPackageManifestValues,
) -> Result<(), AdjustmentEvaluationPackageManifestError> {
    require_exact_keys(database, DATABASE_MANIFEST_KEYS, "database manifest")?;
    let migration_history = text(database, "migration_history_sha256");
    let names = string_array(database, "migration_names");
    let checksums = string_array(database, "migration_checksums");

    // bind every immutable query and site identity before ledger hashing
    if text(database, "contract_version").as_deref() != Some(DATABASE_MANIFEST_VERSION)
        || text(database, "schema_migration").as_deref() != Some(SCHEMA_MIGRATION)
        || text(database, "query_contract_version").as_deref() != Some(QUERY_CONTRACT_VERSION)
        || text(database, "site_key").as_deref() != Some("ballydidean")
        || text(database, "site_timezone").as_deref() != Some("America/Los_Angeles")
        || text(database, "row_schema_sha256").as_deref() != Some(ROW_SCHEMA_SHA256)
        || text(database, "query_contract_sha256").as_deref() != Some(QUERY_CONTRACT_SHA256)
        || migration_history
            .as_ref()
            .is_none_or(|value| !valid_hash(value))
        || names.is_none()
        || checksums.is_none()
    {
        return Err(AdjustmentEvaluationPackageManifestError::DatabaseIdentity);
    }
    let names = names.expect("checked names");
    let checksums = checksums.expect("checked checksums");

    // admit only exact migration prefixes ending at 0017 through 0023
    if names.len() != checksums.len()
        || !(17..=MIGRATIONS.len()).contains(&names.len())
        || !names
            .iter()
            .zip(&checksums)
            .zip(MIGRATIONS)
            .all(|((name, checksum), expected)| {
                name == expected.0 && checksum == expected.1 && valid_hash(checksum)
            })
    {
        return Err(AdjustmentEvaluationPackageManifestError::DatabaseIdentity);
    }

    let preimage = names
        .iter()
        .zip(&checksums)
        .map(|(name, checksum)| format!("{name}:{checksum}"))
        .collect::<Vec<_>>()
        .join("\n");
    let computed = values.sha256(preimage.as_bytes());

    // bind the claimed ledger hash to the exact reviewed prefix bytes
    if computed.as_ref().map(Sha256Digest::as_str) != migration_history.as_deref() {
        return Err(AdjustmentEvaluationPackageManifestError::DatabaseIdentity);
    }
    Ok(())
}

struct MemberDeclarations {
    ordered: Vec<AdjustmentEvaluationPackageMember>,
    by_path: BTreeMap<String, usize>,
    bytes: u64,
}

/// validate every declared member and calculate the bounded extracted allocation
fn validate_member_declarations(
    manifest: &JsonValue,
) -> Result<MemberDeclarations, AdjustmentEvaluationPackageManifestError> {
    let members =
        array(manifest, "members").ok_or(AdjustmentEvaluationPackageManifestError::Transaction)?;
    let mut ordered = Vec::with_capacity(members.len());
    let mut admitted = BTreeMap::new();
    let mut member_bytes = 0.0_f64;

    // validate each immutable member in source order
    for member in members {
        require_exact_keys(member, MEMBER_KEYS, "package member")?;
        let path =
            text(member, "path").ok_or(AdjustmentEvaluationPackageManifestError::MemberPath)?;

        // reject paths outside the four closed member populations
        if !valid_member_path(&path) {
            return Err(AdjustmentEvaluationPackageManifestError::MemberPath);
        }

        let sha256 = text(member, "sha256");
        let size_bytes = nonnegative_integer_field(member, "sizeBytes");

        // reject duplicates, malformed identities, and escaping spellings
        if path.contains("..")
            || sha256.as_ref().is_none_or(|value| !valid_hash(value))
            || size_bytes.is_none()
            || admitted.contains_key(&path)
        {
            return Err(AdjustmentEvaluationPackageManifestError::MemberIdentity);
        }
        let size_bytes = size_bytes.expect("checked size");
        member_bytes += size_bytes;

        // retain accepted values only after the complete identity check
        let member = AdjustmentEvaluationPackageMember {
            path,
            sha256: Sha256Digest::admitted(sha256.expect("checked digest")),
            size_bytes: size_bytes as u64,
        };
        admitted.insert(member.path.clone(), ordered.len());
        ordered.push(member);
    }

    // enforce the complete extracted member allocation before filesystem enumeration
    if member_bytes > MAXIMUM_TEMPORARY_BYTES as f64 {
        return Err(AdjustmentEvaluationPackageManifestError::MemberAllocation);
    }
    Ok(MemberDeclarations {
        ordered,
        by_path: admitted,
        bytes: member_bytes as u64,
    })
}

/// Require one object with exactly the predecessor's expected key set.
fn require_exact_keys(
    value: &JsonValue,
    keys: &[&str],
    description: &'static str,
) -> Result<(), AdjustmentEvaluationPackageManifestError> {
    // distinguish the original object-type error from schema drift
    if !matches!(value, JsonValue::Object(_)) {
        return Err(AdjustmentEvaluationPackageManifestError::Object(
            description,
        ));
    }
    // preserve the original exact-key diagnostic
    if !exact_keys(value, keys) {
        return Err(AdjustmentEvaluationPackageManifestError::Keys(description));
    }
    Ok(())
}

/// Borrow one required nested object with the original description.
fn required_object<'a>(
    document: &'a JsonValue,
    key: &str,
    description: &'static str,
) -> Result<&'a JsonValue, AdjustmentEvaluationPackageManifestError> {
    let value = field(document, key).ok_or(AdjustmentEvaluationPackageManifestError::Object(
        description,
    ))?;

    // retain the predecessor's object type failure
    if !matches!(value, JsonValue::Object(_)) {
        return Err(AdjustmentEvaluationPackageManifestError::Object(
            description,
        ));
    }
    Ok(value)
}

/// Borrow one required array.
fn array<'a>(document: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(document, key)? else {
        return None;
    };
    Some(values)
}

/// Decode one array of exact UTF-16 strings.
fn string_array(document: &JsonValue, key: &str) -> Option<Vec<String>> {
    let values = array(document, key)?;
    let mut output = Vec::with_capacity(values.len());

    // decode every string without replacement
    for value in values {
        let JsonValue::String(units) = value else {
            return None;
        };
        output.push(String::from_utf16(units).ok()?);
    }
    Some(output)
}

/// Read one nonnegative integer that is representable by the admitted Rust output.
fn unsigned_field(document: &JsonValue, key: &str) -> Option<u64> {
    let value = nonnegative_integer_field(document, key)?;

    // every admitted summary value is below its small published bound
    (value <= u64::MAX as f64).then_some(value as u64)
}

/// Read one nonnegative JavaScript integer without imposing a pre-cap safe-integer rule.
fn nonnegative_integer_field(document: &JsonValue, key: &str) -> Option<f64> {
    let JsonValue::Number(value) = field(document, key)? else {
        return None;
    };

    // reproduce Number.isInteger and the original nonnegative check
    (value.is_finite() && value.fract() == 0.0 && *value >= 0.0).then_some(*value)
}

/// Recognize one canonical calendar spelling admitted by the calendar owner.
fn canonical_date(date: &str, values: &mut impl AdjustmentEvaluationPackageManifestValues) -> bool {
    let bytes = date.as_bytes();

    // preserve the exact YYYY-MM-DD grammar before consulting the calendar
    bytes.len() == 10
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
        && values.local_date_ordinal(date).is_some()
}

/// Recognize one closed adjustment member path without adding a regex package.
fn valid_member_path(path: &str) -> bool {
    // retain the one fixed row-stream member
    if path == "members/rows.jsonl.gz" {
        return true;
    }
    valid_hashed_member(path, "members/bodies/sha256-", ".json.gz")
        || valid_hashed_member(path, "members/edge/objects/sha256-", ".json.gz")
        || valid_hashed_member(path, "members/edge/receipts/sha256-", ".json")
}

/// Recognize one lower-hex content-addressed member path.
fn valid_hashed_member(path: &str, prefix: &str, suffix: &str) -> bool {
    let Some(hash) = path
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(suffix))
    else {
        return false;
    };
    valid_hash(hash)
}

/// Map one approved effect failure without changing its diagnostic.
fn effect(error: impl Display) -> AdjustmentEvaluationPackageManifestError {
    AdjustmentEvaluationPackageManifestError::Effect(error.to_string())
}

#[cfg(test)]
#[path = "adjustment_evaluation_package_manifest_fixture_test.rs"]
pub(crate) mod fixture_tests;
