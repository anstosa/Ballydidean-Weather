//! Emit the original cutoff-bound revision cold-transfer start document.

use std::fmt::{Display, Formatter};
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::capture_epoch_witness::{GENESIS_FRONTIER_SHA256, resolve_operator_capture_clock};
use crate::maintenance_anchor_status::uint64_field;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{CaptureEpochInitJson, MaintenanceCanonicalJson};

const INPUT_MAXIMUM_BYTES: usize = 5 * 1024 * 1024;
const SNAPSHOT_MAXIMUM_BYTES: usize = 4 * 1024 * 1024;
const OUTPUT_MAXIMUM_BYTES: usize = 4_832 * 1024;
const START_VERSION: &str = "adjustment-revision-cold-transfer-start/v1";
const SNAPSHOT_VERSION: &str = "adjustment-revision-serving-snapshot/v1";
const RECEIPT_VERSION: &str = "adjustment-revision-commit-receipt/v1";
const COLD_DIRECTORIES: &[&str] = &[
    "revision-commit-receipts",
    "revision-frontier-successors",
    "revision-projections",
    "shadow-revision-capsules",
    "rain-control-states",
    "rain-control-state-stage-receipts",
];
const ENVELOPE_KEYS: &[&str] = &["databaseManifest", "payload", "transaction"];
const TRANSACTION_KEYS: &[&str] = &[
    "created_at_utc",
    "idle_in_transaction_session_timeout",
    "isolation_level",
    "lock_timeout",
    "read_only",
    "statement_timeout",
];
const SNAPSHOT_KEYS: &[&str] = &[
    "archiveCommitOrdinal",
    "contractVersion",
    "cutoffAt",
    "entries",
    "entryCount",
    "frontierSha256",
    "snapshotSha256",
];
const ENTRY_KEYS: &[&str] = &["logicalReceivedAt", "receipt", "relation"];
const RECEIPT_KEYS: &[&str] = &[
    "archiveCommitOrdinal",
    "archiveCommittedAt",
    "contractVersion",
    "frontierSha256",
    "predecessorFrontierSha256",
    "projectionIdentitySha256",
    "projectionKind",
    "projectionSha256",
    "receiptSha256",
    "stageReceiptSha256",
];

/// Contain cold-start failures before any stdout bytes are released.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevisionColdStartError {
    Arguments,
    Input,
    Envelope,
    Transaction,
    Snapshot,
    SnapshotEntry,
    SnapshotIdentity,
    Genesis,
    Root,
    Output,
}

impl Display for RevisionColdStartError {
    /// Preserve bounded predecessor diagnostics without echoing database values.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "revision-cold-start-v1 takes no arguments",
            Self::Input => "revision cold start input is invalid",
            Self::Envelope => "revision cold start database envelope has invalid keys",
            Self::Transaction => "revision cold start transaction differs",
            Self::Snapshot => "adjustment revision serving snapshot is invalid",
            Self::SnapshotEntry => "adjustment revision serving snapshot entry is ineligible",
            Self::SnapshotIdentity => "adjustment revision serving snapshot identity differs",
            Self::Genesis => "adjustment revision cold start genesis differs",
            Self::Root => "private directory is invalid",
            Self::Output => "adjustment revision cold transfer start is too large",
        })
    }
}

impl std::error::Error for RevisionColdStartError {}

/// Verify the service-owned revision archive tree without creating or repairing it.
pub trait RevisionColdStartDirectories {
    /// Admit the fixed private root and all six cold-reader children.
    fn require_cold_directories(&mut self) -> Result<(), RevisionColdStartError>;
}

/// Actual filesystem owner for the predecessor's private cold-reader tree.
pub struct NativeRevisionColdStartDirectories {
    root: PathBuf,
}

impl NativeRevisionColdStartDirectories {
    /// Bind one already selected archive root without performing filesystem work.
    pub fn for_root(root: PathBuf) -> Self {
        Self { root }
    }
}

impl RevisionColdStartDirectories for NativeRevisionColdStartDirectories {
    /// Require literal mode-0700 directories and reject aliases or final links.
    fn require_cold_directories(&mut self) -> Result<(), RevisionColdStartError> {
        require_private_directory(&self.root, None)?;
        // check only the six fixed child names in predecessor order
        for directory in COLD_DIRECTORIES {
            require_private_directory(&self.root.join(directory), Some(&self.root))?;
        }
        Ok(())
    }
}

/// Execute only `revision-cold-start-v1` and return its exact canonical stdout bytes.
pub fn read_revision_cold_start_command(
    action: &str,
    arguments: &[&str],
    input: impl Read,
    directories: &mut impl RevisionColdStartDirectories,
    codec: &mut impl CaptureEpochInitJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<Vec<u8>>, RevisionColdStartError> {
    use RevisionColdStartError as Error;

    // leave every other archive verb to its retained command owner
    if action != "revision-cold-start-v1" {
        return Ok(None);
    }
    // reject operands before consuming database input or observing storage
    if !arguments.is_empty() {
        return Err(Error::Arguments);
    }
    let bytes = read_bounded(input)?;
    let envelope = codec.parse_database_envelope(&bytes).ok_or(Error::Input)?;
    let snapshot = validate_database_envelope(&envelope, codec, time)?;
    directories.require_cold_directories()?;
    // bind the reserved zero cursor only after admitting the complete cold tree
    if text(snapshot, "archiveCommitOrdinal").as_deref() == Some("0")
        && text(snapshot, "frontierSha256").as_deref() != Some(GENESIS_FRONTIER_SHA256)
    {
        return Err(Error::Genesis);
    }

    let unsigned = cold_start_value(snapshot, None);
    let preimage = codec
        .encode_canonical_json(&unsigned)
        .ok_or(Error::Output)?;
    let start_sha256 = codec.sha256(&preimage).ok_or(Error::Output)?;
    let start = cold_start_value(snapshot, Some(start_sha256.as_str()));
    let output = codec.encode_canonical_json(&start).ok_or(Error::Output)?;
    // preserve the independent transfer wire ceiling after full materialization
    if output.len() > OUTPUT_MAXIMUM_BYTES {
        return Err(Error::Output);
    }
    Ok(Some(output))
}

/// Read one finite stdin body without allocating beyond the predecessor ceiling.
fn read_bounded(mut input: impl Read) -> Result<Vec<u8>, RevisionColdStartError> {
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take(INPUT_MAXIMUM_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RevisionColdStartError::Input)?;
    // reject overflow before invoking the selected JSON decoder
    if bytes.len() > INPUT_MAXIMUM_BYTES {
        return Err(RevisionColdStartError::Input);
    }
    Ok(bytes)
}

/// Admit the original command envelope and return its unchanged snapshot value.
fn validate_database_envelope<'a>(
    value: &'a JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<&'a JsonValue, RevisionColdStartError> {
    use RevisionColdStartError as Error;

    // require only the original three top-level database members
    if !exact_keys(value, ENVELOPE_KEYS)
        || !matches!(field(value, "databaseManifest"), Some(JsonValue::Object(_)))
    {
        return Err(Error::Envelope);
    }
    let transaction = field(value, "transaction").ok_or(Error::Envelope)?;
    // preserve the exact read-only repeatable-read transaction settings
    if !exact_keys(transaction, TRANSACTION_KEYS)
        || text(transaction, "idle_in_transaction_session_timeout").as_deref() != Some("30s")
        || text(transaction, "isolation_level").as_deref() != Some("repeatable read")
        || text(transaction, "lock_timeout").as_deref() != Some("5s")
        || text(transaction, "read_only").as_deref() != Some("on")
        || text(transaction, "statement_timeout").as_deref() != Some("5min")
    {
        return Err(Error::Transaction);
    }
    let snapshot = field(value, "payload").ok_or(Error::Envelope)?;
    validate_serving_snapshot(snapshot, codec, time)?;
    Ok(snapshot)
}

/// Validate the complete nonzero-capable serving snapshot retained by Node.
fn validate_serving_snapshot(
    value: &JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), RevisionColdStartError> {
    use RevisionColdStartError as Error;

    let JsonValue::Array(entries) = field(value, "entries").ok_or(Error::Snapshot)? else {
        return Err(Error::Snapshot);
    };
    let encoded = codec.encode_canonical_json(value).ok_or(Error::Snapshot)?;
    // account for the canonical codec's single LF excluded by canonicalJson length
    if !exact_keys(value, SNAPSHOT_KEYS)
        || text(value, "contractVersion").as_deref() != Some(SNAPSHOT_VERSION)
        || entries.len() > 4_096
        || !matches!(field(value, "entryCount"), Some(JsonValue::Number(count)) if *count == entries.len() as f64)
        || encoded.last() != Some(&b'\n')
        || encoded.len().saturating_sub(1) > SNAPSHOT_MAXIMUM_BYTES
    {
        return Err(Error::Snapshot);
    }
    let cutoff = text(value, "cutoffAt").ok_or(Error::Snapshot)?;
    let cutoff_milliseconds = resolve_operator_capture_clock(&cutoff, time)
        .ok_or(Error::Snapshot)?
        .epoch_milliseconds;
    let watermark = uint64_field(value, "archiveCommitOrdinal").ok_or(Error::Snapshot)?;
    let frontier = text(value, "frontierSha256").ok_or(Error::Snapshot)?;
    let snapshot_sha256 = text(value, "snapshotSha256").ok_or(Error::Snapshot)?;
    // reject malformed roots before iterating over their claimed receipt entries
    if !valid_hash(&frontier) || !valid_hash(&snapshot_sha256) {
        return Err(Error::Snapshot);
    }

    let mut prior_ordinal = 0_u64;
    let mut receipt_hashes = Vec::with_capacity(entries.len());
    // retain the snapshot's ordered current-pointer relation and watermark rules
    for entry in entries {
        let (ordinal, receipt_sha256) =
            validate_snapshot_entry(entry, cutoff_milliseconds, watermark, time)?;
        // reject rollback, duplicates and entries beyond the immutable watermark
        if ordinal <= prior_ordinal {
            return Err(Error::SnapshotEntry);
        }
        prior_ordinal = ordinal;
        receipt_hashes.push(receipt_sha256);
    }
    let preimage = format!(
        "{SNAPSHOT_VERSION}\n{cutoff}\n{watermark}\n{frontier}\n{}",
        receipt_hashes.join("\n")
    );
    let computed = codec
        .sha256(preimage.as_bytes())
        .ok_or(Error::SnapshotIdentity)?;
    // bind every ordered current receipt address to the snapshot identity
    if computed.as_str() != snapshot_sha256 {
        return Err(Error::SnapshotIdentity);
    }
    Ok(())
}

/// Admit one current-pointer entry and return its ordinal and receipt address.
fn validate_snapshot_entry(
    value: &JsonValue,
    cutoff_milliseconds: i64,
    watermark: u64,
    time: &dyn CurrentTimeResolver,
) -> Result<(u64, String), RevisionColdStartError> {
    use RevisionColdStartError as Error;

    if !exact_keys(value, ENTRY_KEYS) {
        return Err(Error::SnapshotEntry);
    }
    let logical = text(value, "logicalReceivedAt").ok_or(Error::SnapshotEntry)?;
    let logical_milliseconds = resolve_operator_capture_clock(&logical, time)
        .ok_or(Error::SnapshotEntry)?
        .epoch_milliseconds;
    let relation = text(value, "relation").ok_or(Error::SnapshotEntry)?;
    let receipt = field(value, "receipt").ok_or(Error::SnapshotEntry)?;
    let (ordinal, receipt_sha256, projection_kind) = validate_receipt(receipt, time)?;
    let relation_matches = match relation.as_str() {
        "weather_records" => matches!(
            projection_kind.as_str(),
            "actual_best_match" | "target_revision"
        ),
        "rain_adjustment_runs" => projection_kind == "rain_gate_input",
        "ecmwf_temperature_canary_runs" | "forecast_anchor_records" => {
            projection_kind == "native_source"
        }
        _ => false,
    };
    // bind both logical time and database ordinal to the frozen cutoff
    if logical_milliseconds > cutoff_milliseconds || ordinal > watermark || !relation_matches {
        return Err(Error::SnapshotEntry);
    }
    Ok((ordinal, receipt_sha256))
}

/// Admit the complete closed database receipt used by one snapshot entry.
fn validate_receipt(
    value: &JsonValue,
    time: &dyn CurrentTimeResolver,
) -> Result<(u64, String, String), RevisionColdStartError> {
    use RevisionColdStartError::SnapshotEntry as Error;

    let ordinal = uint64_field(value, "archiveCommitOrdinal").ok_or(Error)?;
    let projection_kind = text(value, "projectionKind").ok_or(Error)?;
    let projection_identity = text(value, "projectionIdentitySha256").ok_or(Error)?;
    let projection_sha256 = text(value, "projectionSha256").ok_or(Error)?;
    let receipt_sha256 = text(value, "receiptSha256").ok_or(Error)?;
    let hash_fields = [
        "frontierSha256",
        "predecessorFrontierSha256",
        "projectionIdentitySha256",
        "projectionSha256",
        "receiptSha256",
        "stageReceiptSha256",
    ];
    // retain the closed receipt contract and every fixed scalar grammar
    if !exact_keys(value, RECEIPT_KEYS)
        || ordinal == 0
        || text(value, "contractVersion").as_deref() != Some(RECEIPT_VERSION)
        || !matches!(
            projection_kind.as_str(),
            "actual_best_match"
                | "native_source"
                | "rain_gate_input"
                | "shadow_prediction"
                | "target_revision"
        )
        || text(value, "archiveCommittedAt")
            .and_then(|value| resolve_operator_capture_clock(&value, time))
            .is_none()
        || hash_fields.iter().any(|name| {
            text(value, name)
                .as_deref()
                .is_none_or(|hash| !valid_hash(hash))
        })
        || (projection_kind != "shadow_prediction" && projection_identity != projection_sha256)
    {
        return Err(Error);
    }
    Ok((ordinal, receipt_sha256, projection_kind))
}

/// Build the unsigned or fully addressed cold-transfer start document.
fn cold_start_value(snapshot: &JsonValue, start_sha256: Option<&str>) -> JsonValue {
    let mut fields = vec![
        json_entry(
            "contractVersion",
            JsonValue::String(START_VERSION.encode_utf16().collect()),
        ),
        json_entry("servingSnapshot", snapshot.clone()),
        json_entry(
            "watermarkArchiveCommitOrdinal",
            JsonValue::String(
                text(snapshot, "archiveCommitOrdinal")
                    .unwrap_or_default()
                    .encode_utf16()
                    .collect(),
            ),
        ),
        json_entry(
            "watermarkFrontierSha256",
            JsonValue::String(
                text(snapshot, "frontierSha256")
                    .unwrap_or_default()
                    .encode_utf16()
                    .collect(),
            ),
        ),
    ];
    // add only the self-address after hashing the complete unsigned document
    if let Some(start_sha256) = start_sha256 {
        fields.push(json_entry(
            "startSha256",
            JsonValue::String(start_sha256.encode_utf16().collect()),
        ));
    }
    JsonValue::Object(fields)
}

/// Encode one generated-contract object member without a JSON implementation.
fn json_entry(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Require one literal owner-private directory at its already resolved path.
fn require_private_directory(
    path: &Path,
    expected_parent: Option<&Path>,
) -> Result<(), RevisionColdStartError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| RevisionColdStartError::Root)?;
    let canonical = fs::canonicalize(path).map_err(|_| RevisionColdStartError::Root)?;
    // reject final links, aliases, public modes and child escapes
    if !metadata.is_dir()
        || metadata.mode() & 0o777 != 0o700
        || canonical != path
        || expected_parent.is_some_and(|parent| canonical.parent() != Some(parent))
    {
        return Err(RevisionColdStartError::Root);
    }
    Ok(())
}
