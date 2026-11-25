//! Admit the original database export envelope used to initialize a capture epoch.

use weather_api::CurrentTimeResolver;
use weather_contracts::{AdjustmentRevisionServingSnapshot, JsonValue};

use crate::capture_epoch_operator::verify_snapshot;
use crate::capture_epoch_witness::resolve_operator_capture_clock;
use crate::maintenance_canonical_json::{exact_keys, field, text};
use crate::{CaptureEpochInitError, MaintenanceCanonicalJson};

const DATABASE_ENVELOPE_KEYS: &[&str] = &["databaseManifest", "payload", "transaction"];
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
const DATABASE_TRANSACTION_KEYS: &[&str] = &[
    "created_at_utc",
    "idle_in_transaction_session_timeout",
    "isolation_level",
    "lock_timeout",
    "read_only",
    "statement_timeout",
];
const DATABASE_MANIFEST_VERSION: &str = "adjustment-evaluation-export-manifest/v1";

#[derive(Clone, Copy)]
struct ReviewedLedger {
    checksum: &'static str,
    count: usize,
    history_sha256: &'static str,
    name: &'static str,
}

const REVIEWED_LEDGERS: &[ReviewedLedger] = &[
    ReviewedLedger {
        checksum: "ca29db99377001fca2e2e4268fd80d1cbe7b876f71f2d5ba04e575712cb9f13b",
        count: 21,
        history_sha256: "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
        name: "0021_adjustment_rolling_registration.sql",
    },
    ReviewedLedger {
        checksum: "e637542cb2e742fe8f22f60313a11f00adbc1d140a811ee718524849fcece12a",
        count: 23,
        history_sha256: "107ce430f1ac9478d54d94646da7fa4a54cbe7112db13efd7244d8894d0defdf",
        name: "0023_forecast_product_read_index.sql",
    },
    ReviewedLedger {
        checksum: "7e48acb14710a625cdcbb796bf1bf49ab4e00bf1aeb9bb763ac86410e08663e0",
        count: 23,
        history_sha256: "3aec6846d009a2bb2cee02279dceb9bd2d3572dc1b3a83eed2ce9a1724311345",
        name: "0023_forecast_product_read_index.sql",
    },
];

/// Retain the reviewed ledger identity and optional canonical zero snapshot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AdmittedCaptureEpochDatabase {
    pub migration_history_sha256: String,
    pub serving_snapshot: Option<(AdjustmentRevisionServingSnapshot, Vec<u8>)>,
}

/// Validate the exact database export boundary used by capture-epoch initialization.
pub(crate) fn validate_capture_epoch_database_envelope(
    value: &JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<AdmittedCaptureEpochDatabase, CaptureEpochInitError> {
    use CaptureEpochInitError::{DatabaseInput, Snapshot};

    // admit only the predecessor's three-member database envelope
    if !exact_keys(value, DATABASE_ENVELOPE_KEYS) {
        return Err(DatabaseInput);
    }
    let manifest = field(value, "databaseManifest").ok_or(DatabaseInput)?;
    // retain every original manifest member without interpreting unrelated values
    if !exact_keys(manifest, DATABASE_MANIFEST_KEYS) {
        return Err(DatabaseInput);
    }
    let migration_history_sha256 = validate_reviewed_ledger(manifest, codec)?;
    let transaction = field(value, "transaction").ok_or(DatabaseInput)?;
    let created_at_utc = validate_transaction(transaction, time)?;
    let payload = field(value, "payload").ok_or(DatabaseInput)?;

    // allow retry metadata to omit the mutable current-frontier snapshot
    if matches!(payload, JsonValue::Null) {
        return Ok(AdmittedCaptureEpochDatabase {
            migration_history_sha256,
            serving_snapshot: None,
        });
    }

    let bytes = codec.encode_canonical_json(payload).ok_or(Snapshot)?;
    let snapshot = verify_snapshot(&bytes, codec, time).map_err(|_| Snapshot)?;
    // bind the empty genesis receipt to the exact database transaction clock
    if snapshot.cutoffAt != created_at_utc {
        return Err(Snapshot);
    }
    Ok(AdmittedCaptureEpochDatabase {
        migration_history_sha256,
        serving_snapshot: Some((snapshot, bytes)),
    })
}

/// Match one complete ordered migration ledger against a reviewed identity.
fn validate_reviewed_ledger(
    manifest: &JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<String, CaptureEpochInitError> {
    use CaptureEpochInitError::Ledger;

    let history_sha256 = text(manifest, "migration_history_sha256").ok_or(Ledger)?;
    let reviewed = REVIEWED_LEDGERS
        .iter()
        .find(|ledger| ledger.history_sha256 == history_sha256)
        .copied()
        .ok_or(Ledger)?;
    let names = string_array(manifest, "migration_names").ok_or(Ledger)?;
    let checksums = string_array(manifest, "migration_checksums").ok_or(Ledger)?;

    // require the exact version, cardinality and reviewed final migration
    if text(manifest, "contract_version").as_deref() != Some(DATABASE_MANIFEST_VERSION)
        || names.len() != reviewed.count
        || checksums.len() != reviewed.count
        || names.last().map(String::as_str) != Some(reviewed.name)
        || checksums.last().map(String::as_str) != Some(reviewed.checksum)
    {
        return Err(Ledger);
    }

    let preimage = names
        .iter()
        .zip(&checksums)
        .map(|(name, checksum)| format!("{name}:{checksum}"))
        .collect::<Vec<_>>()
        .join("\n");
    let computed = codec.sha256(preimage.as_bytes()).ok_or(Ledger)?;
    // prove the full ordered ledger rather than only its reviewed tail
    if computed.as_str() != reviewed.history_sha256 {
        return Err(Ledger);
    }
    Ok(history_sha256)
}

/// Validate the fixed read-only repeatable-read database transaction.
fn validate_transaction(
    transaction: &JsonValue,
    time: &dyn CurrentTimeResolver,
) -> Result<String, CaptureEpochInitError> {
    use CaptureEpochInitError::Transaction;

    // require the complete predecessor transaction record without extensions
    if !exact_keys(transaction, DATABASE_TRANSACTION_KEYS) {
        return Err(Transaction);
    }
    let created_at_utc = text(transaction, "created_at_utc").ok_or(Transaction)?;
    // retain both canonical clock identity and every fixed transaction setting
    if resolve_operator_capture_clock(&created_at_utc, time).is_none()
        || text(transaction, "idle_in_transaction_session_timeout").as_deref() != Some("30s")
        || text(transaction, "isolation_level").as_deref() != Some("repeatable read")
        || text(transaction, "lock_timeout").as_deref() != Some("5s")
        || text(transaction, "read_only").as_deref() != Some("on")
        || text(transaction, "statement_timeout").as_deref() != Some("5min")
    {
        return Err(Transaction);
    }
    Ok(created_at_utc)
}

/// Decode one manifest array without altering its string values.
fn string_array(value: &JsonValue, key: &str) -> Option<Vec<String>> {
    let JsonValue::Array(values) = field(value, key)? else {
        return None;
    };
    values
        .iter()
        .map(|value| match value {
            JsonValue::String(units) => String::from_utf16(units).ok(),
            _ => None,
        })
        .collect()
}
