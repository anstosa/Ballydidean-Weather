//! Canonical cold-transfer start and packed-custody acknowledgement admission.

use std::fmt::{self, Display, Formatter};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::MaintenanceCanonicalJson;
use crate::capture_epoch_witness::resolve_operator_capture_clock;
use crate::maintenance_anchor_status::{
    clock_field, hash_field, nullable_hash_field, uint64_field,
};
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_revision_cold_drain::{
    ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES, NativeRevisionColdCustodyRequest,
    VerifiedNativeRevisionColdCustodyAcknowledgementV2, VerifiedNativeRevisionColdStart,
};
use crate::shadow_metadata_custody::canonical_self_address;

const START_VERSION: &str = "adjustment-revision-cold-transfer-start/v1";
const SNAPSHOT_VERSION: &str = "adjustment-revision-serving-snapshot/v1";
const RECEIPT_VERSION: &str = "adjustment-revision-commit-receipt/v1";
const ACK_V2_VERSION: &str = "adjustment-revision-custody-acknowledgement/v2";
const SNAPSHOT_MAXIMUM_BYTES: usize = 4 * 1024 * 1024;
const ACK_MAXIMUM_BYTES: usize = 16 * 1024;
const START_FIELDS: &[&str] = &[
    "contractVersion",
    "servingSnapshot",
    "startSha256",
    "watermarkArchiveCommitOrdinal",
    "watermarkFrontierSha256",
];
const SNAPSHOT_FIELDS: &[&str] = &[
    "archiveCommitOrdinal",
    "contractVersion",
    "cutoffAt",
    "entries",
    "entryCount",
    "frontierSha256",
    "snapshotSha256",
];
const SNAPSHOT_ENTRY_FIELDS: &[&str] = &["logicalReceivedAt", "receipt", "relation"];
const RECEIPT_FIELDS: &[&str] = &[
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
const ACK_V2_FIELDS: &[&str] = &[
    "acknowledgedAt",
    "acknowledgementSha256",
    "afterArchiveCommitOrdinal",
    "afterFrontierSha256",
    "authority",
    "contractVersion",
    "custodyCheckpointSha256",
    "memberRootSha256",
    "nextArchiveCommitOrdinal",
    "nextFrontierSha256",
    "pageSha256",
    "previousAcknowledgementSha256",
    "previousPageSha256",
    "retirementEntries",
    "startMemberSha256",
    "startSha256",
    "watermarkArchiveCommitOrdinal",
    "watermarkFrontierSha256",
];
const RETIREMENT_FIELDS: &[&str] = &["fileSha256", "identitySha256", "kind"];
const RETIREMENT_KINDS: &[&str] = &[
    "revision_commit_receipt",
    "revision_frontier_successor",
    "revision_commit_group",
    "revision_projection",
    "revision_publish_receipt",
    "revision_stage_receipt",
    "rain_control_state",
    "rain_control_state_stage_receipt",
    "revision_terminal_gap",
    "shadow_revision_capsule",
    "shadow_revision_publish_receipt",
    "shadow_revision_terminal_gap",
];

/// Bounded cold remote document refusal without retained payload reflection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceColdRemoteDocumentError {
    Size,
    Canonical,
    Shape,
    Clock,
    Identity,
    Binding,
}

impl Display for NativeMaintenanceColdRemoteDocumentError {
    /// Render one stable refusal category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Size => "maintenance cold remote document is too large",
            Self::Canonical => "maintenance cold remote document is not canonical",
            Self::Shape => "maintenance cold remote document shape is invalid",
            Self::Clock => "maintenance cold remote document clock is invalid",
            Self::Identity => "maintenance cold remote document identity is invalid",
            Self::Binding => "maintenance cold remote document binding differs",
        })
    }
}

impl std::error::Error for NativeMaintenanceColdRemoteDocumentError {}

/// Verify one complete canonical cutoff-bound cold transfer start.
pub fn verify_native_revision_cold_start_document(
    bytes: &[u8],
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<VerifiedNativeRevisionColdStart, NativeMaintenanceColdRemoteDocumentError> {
    use NativeMaintenanceColdRemoteDocumentError as Error;

    // bound the complete response before canonical parsing or nested validation
    if !(2..=ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES).contains(&bytes.len()) {
        return Err(Error::Size);
    }
    let value = codec
        .parse_canonical_object(bytes)
        .ok_or(Error::Canonical)?;
    // accept only the fixed start envelope before borrowing its snapshot
    if !exact_keys(&value, START_FIELDS) {
        return Err(Error::Shape);
    }
    let snapshot = field(&value, "servingSnapshot").ok_or(Error::Shape)?;
    validate_serving_snapshot(snapshot, codec, time)?;
    let cutoff_at = required_text(snapshot, "cutoffAt")?;
    let snapshot_ordinal = required_text(snapshot, "archiveCommitOrdinal")?;
    let snapshot_frontier = required_text(snapshot, "frontierSha256")?;
    let watermark_archive_commit_ordinal = required_text(&value, "watermarkArchiveCommitOrdinal")?;
    let watermark_frontier_sha256 = required_text(&value, "watermarkFrontierSha256")?;
    let start_sha256 = required_text(&value, "startSha256")?;
    let unsigned = without_field(&value, "startSha256")?;
    let preimage = codec
        .encode_canonical_json(&unsigned)
        .ok_or(Error::Canonical)?;
    let computed = codec.sha256(&preimage).ok_or(Error::Identity)?;
    // bind the watermark and self-address to the exact verified serving snapshot
    if text(&value, "contractVersion").as_deref() != Some(START_VERSION)
        || watermark_archive_commit_ordinal != snapshot_ordinal
        || watermark_frontier_sha256 != snapshot_frontier
        || computed.as_str() != start_sha256
    {
        return Err(Error::Binding);
    }
    Ok(VerifiedNativeRevisionColdStart {
        cutoff_at,
        canonical_bytes: bytes.to_vec(),
        value,
        start_sha256,
        watermark_archive_commit_ordinal,
        watermark_frontier_sha256,
    })
}

/// Verify one complete canonical v2 acknowledgement and bind its request coordinates.
pub fn verify_native_revision_cold_custody_acknowledgement_v2_document(
    bytes: &[u8],
    expected_request: &NativeRevisionColdCustodyRequest,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<
    VerifiedNativeRevisionColdCustodyAcknowledgementV2,
    NativeMaintenanceColdRemoteDocumentError,
> {
    use NativeMaintenanceColdRemoteDocumentError as Error;

    // retain the source acknowledgement ceiling before parsing caller-owned bytes
    if !(2..=ACK_MAXIMUM_BYTES).contains(&bytes.len()) {
        return Err(Error::Size);
    }
    let value = codec
        .parse_canonical_object(bytes)
        .ok_or(Error::Canonical)?;
    validate_custody_acknowledgement_v2(&value, codec, time)?;
    // prevent the typed request field from concealing a reply to another archive page
    if !acknowledgement_matches_request(&value, expected_request) {
        return Err(Error::Binding);
    }
    Ok(VerifiedNativeRevisionColdCustodyAcknowledgementV2 {
        canonical_bytes: bytes.to_vec(),
        next_archive_commit_ordinal: required_text(&value, "nextArchiveCommitOrdinal")?,
        next_frontier_sha256: required_text(&value, "nextFrontierSha256")?,
        request: expected_request.clone(),
        value,
    })
}

/// Validate the complete nonzero-capable serving snapshot retained by the source.
fn validate_serving_snapshot(
    value: &JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeMaintenanceColdRemoteDocumentError> {
    use NativeMaintenanceColdRemoteDocumentError as Error;

    let Some(JsonValue::Array(entries)) = field(value, "entries") else {
        return Err(Error::Shape);
    };
    let encoded = codec.encode_canonical_json(value).ok_or(Error::Canonical)?;
    // preserve the exact metadata-only shape, entry count and nested byte ceiling
    if !exact_keys(value, SNAPSHOT_FIELDS)
        || text(value, "contractVersion").as_deref() != Some(SNAPSHOT_VERSION)
        || entries.len() > 4_096
        || !matches!(field(value, "entryCount"), Some(JsonValue::Number(count)) if *count == entries.len() as f64)
        || encoded.last() != Some(&b'\n')
        || encoded.len().saturating_sub(1) > SNAPSHOT_MAXIMUM_BYTES
    {
        return Err(Error::Shape);
    }
    let cutoff = required_text(value, "cutoffAt")?;
    let cutoff_milliseconds = resolve_operator_capture_clock(&cutoff, time)
        .ok_or(Error::Clock)?
        .epoch_milliseconds;
    let watermark = uint64_field(value, "archiveCommitOrdinal").ok_or(Error::Shape)?;
    let frontier = required_text(value, "frontierSha256")?;
    let snapshot_sha256 = required_text(value, "snapshotSha256")?;
    // reject malformed snapshot roots before iterating over receipt entries
    if !valid_hash(&frontier) || !valid_hash(&snapshot_sha256) {
        return Err(Error::Identity);
    }
    let mut prior_ordinal = 0_u64;
    let mut receipt_hashes = Vec::with_capacity(entries.len());
    // preserve source order while rejecting rollback and watermark overflow
    for entry in entries {
        let (ordinal, receipt_sha256) =
            validate_snapshot_entry(entry, cutoff_milliseconds, watermark, time)?;
        // require strictly increasing current-pointer ordinals
        if ordinal <= prior_ordinal {
            return Err(Error::Binding);
        }
        prior_ordinal = ordinal;
        receipt_hashes.push(receipt_sha256);
    }
    let preimage = format!(
        "{SNAPSHOT_VERSION}\n{cutoff}\n{watermark}\n{frontier}\n{}",
        receipt_hashes.join("\n")
    );
    let computed = codec.sha256(preimage.as_bytes()).ok_or(Error::Identity)?;
    // bind every ordered current receipt address to the snapshot identity
    if computed.as_str() != snapshot_sha256 {
        return Err(Error::Identity);
    }
    Ok(())
}

/// Validate one serving snapshot entry and return its ordered receipt address.
fn validate_snapshot_entry(
    value: &JsonValue,
    cutoff_milliseconds: i64,
    watermark: u64,
    time: &dyn CurrentTimeResolver,
) -> Result<(u64, String), NativeMaintenanceColdRemoteDocumentError> {
    use NativeMaintenanceColdRemoteDocumentError as Error;

    // reject entry extensions before reading its relation or receipt
    if !exact_keys(value, SNAPSHOT_ENTRY_FIELDS) {
        return Err(Error::Shape);
    }
    let logical = required_text(value, "logicalReceivedAt")?;
    let logical_milliseconds = resolve_operator_capture_clock(&logical, time)
        .ok_or(Error::Clock)?
        .epoch_milliseconds;
    let relation = required_text(value, "relation")?;
    let receipt = field(value, "receipt").ok_or(Error::Shape)?;
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
        return Err(Error::Binding);
    }
    Ok((ordinal, receipt_sha256))
}

/// Validate one complete closed database receipt used by a snapshot entry.
fn validate_receipt(
    value: &JsonValue,
    time: &dyn CurrentTimeResolver,
) -> Result<(u64, String, String), NativeMaintenanceColdRemoteDocumentError> {
    use NativeMaintenanceColdRemoteDocumentError as Error;

    // reject receipt extensions before decoding its scalar contract
    if !exact_keys(value, RECEIPT_FIELDS) {
        return Err(Error::Shape);
    }
    let ordinal = uint64_field(value, "archiveCommitOrdinal").ok_or(Error::Shape)?;
    let projection_kind = required_text(value, "projectionKind")?;
    let projection_identity = required_text(value, "projectionIdentitySha256")?;
    let projection_sha256 = required_text(value, "projectionSha256")?;
    let receipt_sha256 = required_text(value, "receiptSha256")?;
    let hash_fields = [
        "frontierSha256",
        "predecessorFrontierSha256",
        "projectionIdentitySha256",
        "projectionSha256",
        "receiptSha256",
        "stageReceiptSha256",
    ];
    // retain only the source receipt contract, kinds, clock and fixed hashes
    if ordinal == 0
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
        || hash_fields.iter().any(|name| !hash_field(value, name))
        || (projection_kind != "shadow_prediction" && projection_identity != projection_sha256)
    {
        return Err(Error::Binding);
    }
    Ok((ordinal, receipt_sha256, projection_kind))
}

/// Validate the exact v2 custody acknowledgement and stable retirement set.
pub(crate) fn validate_custody_acknowledgement_v2(
    value: &JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeMaintenanceColdRemoteDocumentError> {
    use NativeMaintenanceColdRemoteDocumentError as Error;

    // reject extensions and legacy identity aliases at the v2 boundary
    if !exact_keys(value, ACK_V2_FIELDS) {
        return Err(Error::Shape);
    }
    clock_field(value, "acknowledgedAt", time).ok_or(Error::Clock)?;
    let after = uint64_field(value, "afterArchiveCommitOrdinal").ok_or(Error::Shape)?;
    let next = uint64_field(value, "nextArchiveCommitOrdinal").ok_or(Error::Shape)?;
    uint64_field(value, "watermarkArchiveCommitOrdinal").ok_or(Error::Shape)?;
    let hash_fields = [
        "acknowledgementSha256",
        "afterFrontierSha256",
        "custodyCheckpointSha256",
        "memberRootSha256",
        "nextFrontierSha256",
        "pageSha256",
        "previousPageSha256",
        "startSha256",
        "watermarkFrontierSha256",
    ];
    // require every nonnullable and nullable identity before semantic binding
    if hash_fields.iter().any(|name| !hash_field(value, name))
        || !nullable_hash_field(value, "previousAcknowledgementSha256")
        || !nullable_hash_field(value, "startMemberSha256")
    {
        return Err(Error::Identity);
    }
    // prohibit semantic authority, alternate versions, and empty cursor transitions
    if text(value, "authority").as_deref() != Some("cold_custody_only")
        || text(value, "contractVersion").as_deref() != Some(ACK_V2_VERSION)
        || next <= after
    {
        return Err(Error::Binding);
    }
    let Some(JsonValue::Array(entries)) = field(value, "retirementEntries") else {
        return Err(Error::Shape);
    };
    // preserve the bounded retirement population before walking its fixed addresses
    if !(4..=10).contains(&entries.len()) {
        return Err(Error::Shape);
    }
    let mut prior: Option<(String, String)> = None;
    // validate source-stable kind and identity order without granting retirement authority
    for entry in entries {
        // reject entry extensions and malformed fixed identities before ordering
        if !exact_keys(entry, RETIREMENT_FIELDS)
            || !hash_field(entry, "fileSha256")
            || !hash_field(entry, "identitySha256")
        {
            return Err(Error::Identity);
        }
        let kind = required_text(entry, "kind")?;
        let identity = required_text(entry, "identitySha256")?;
        // reject foreign kinds and descending source order while preserving stable equals
        if !RETIREMENT_KINDS.contains(&kind.as_str())
            || prior
                .as_ref()
                .is_some_and(|prior| prior > &(kind.clone(), identity.clone()))
        {
            return Err(Error::Binding);
        }
        prior = Some((kind, identity));
    }
    canonical_self_address(value, "acknowledgementSha256", codec).ok_or(Error::Identity)
}

/// Bind every remote acknowledgement request coordinate to the archived caller request.
fn acknowledgement_matches_request(
    value: &JsonValue,
    expected: &NativeRevisionColdCustodyRequest,
) -> bool {
    text(value, "afterArchiveCommitOrdinal").as_deref()
        == Some(expected.page.after_archive_commit_ordinal.as_str())
        && text(value, "afterFrontierSha256").as_deref()
            == Some(expected.page.after_frontier_sha256.as_str())
        && text(value, "custodyCheckpointSha256").as_deref()
            == Some(expected.custody_checkpoint_sha256.as_str())
        && text(value, "memberRootSha256").as_deref() == Some(expected.member_root_sha256.as_str())
        && text(value, "pageSha256").as_deref() == Some(expected.page_sha256.as_str())
        && text(value, "previousPageSha256").as_deref()
            == Some(expected.page.previous_page_sha256.as_str())
        && text(value, "startMemberSha256").as_deref()
            == Some(expected.start_member_sha256.as_str())
        && text(value, "startSha256").as_deref() == Some(expected.page.start_sha256.as_str())
        && text(value, "watermarkArchiveCommitOrdinal").as_deref()
            == Some(expected.page.watermark_archive_commit_ordinal.as_str())
        && text(value, "watermarkFrontierSha256").as_deref()
            == Some(expected.page.watermark_frontier_sha256.as_str())
}

/// Remove one exact self-address member without changing any retained values.
fn without_field(
    value: &JsonValue,
    name: &str,
) -> Result<JsonValue, NativeMaintenanceColdRemoteDocumentError> {
    let JsonValue::Object(fields) = value else {
        return Err(NativeMaintenanceColdRemoteDocumentError::Shape);
    };
    Ok(JsonValue::Object(
        fields
            .iter()
            // exclude only the fixed self-address field from the hash preimage
            .filter(|(key, _)| !key.iter().copied().eq(name.encode_utf16()))
            .cloned()
            .collect(),
    ))
}

/// Decode one required UTF-16 JSON string.
fn required_text(
    value: &JsonValue,
    name: &str,
) -> Result<String, NativeMaintenanceColdRemoteDocumentError> {
    text(value, name).ok_or(NativeMaintenanceColdRemoteDocumentError::Shape)
}

#[cfg(test)]
#[path = "native_maintenance_cold_remote_documents_test.rs"]
mod tests;
