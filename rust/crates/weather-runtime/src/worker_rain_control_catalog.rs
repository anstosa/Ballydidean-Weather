//! Admit the independent root-installed rain maintenance control reference.

use weather_contracts::JsonValue;

use crate::api_maintenance_candidate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceCandidateError, exact_keys, field, text, valid_hash,
};
use crate::api_maintenance_candidate_catalog::select_installed_catalog_entry;

const RECEIPT_KEYS: &[&str] = &[
    "actionKind",
    "actionSha256",
    "contractVersion",
    "controlPlaneSha256",
    "controlStateSha256",
    "custodyAnchorSha256",
    "deployedCommit",
    "deployedImageDigest",
    "deployedRelease",
    "deployedSettingsSha256",
    "dueMonth",
    "fencingToken",
    "graphManifestSha256",
    "installedAt",
    "ordinalArtifactSha256",
    "sourceMemberRootSha256",
    "sourceReceiptRootSha256",
];
const SELECTOR_KEYS: &[&str] = &[
    "actionSha256",
    "contractVersion",
    "controlStatePath",
    "controlStateSha256",
    "dueMonth",
    "ordinalArtifactPath",
    "ordinalArtifactSha256",
    "siteKey",
];
const ACTION_KEYS: &[&str] = &[
    "actionKind",
    "contractVersion",
    "createdAt",
    "dueMonth",
    "expectedCatalogReceiptSha256",
    "expectedSettingsSha256",
    "expectedSourceCommit",
    "expectedSourceRelease",
    "family",
    "fencingToken",
    "graphManifestSha256",
    "ordinalArtifactSha256",
    "predecessorActionSha256",
    "reason",
    "controlStateSha256",
    "sourceMemberRootSha256",
    "sourceReceiptRootSha256",
    "validThrough",
];

#[cfg(test)]
#[path = "worker_rain_control_catalog_test.rs"]
mod tests;

/// Exact installed control identities retained before reading content-addressed members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkerRainControlCatalog {
    pub action_sha256: String,
    pub control_state_sha256: String,
    pub ordinal_artifact_sha256: String,
    pub due_month: String,
    pub deployed_settings_sha256: String,
    pub fencing_token: String,
    pub graph_manifest_sha256: String,
    pub source_member_root_sha256: String,
    pub source_receipt_root_sha256: String,
}

/// Select and admit the v3 rain control slot without loading its referenced files.
pub(crate) fn select_worker_rain_control_catalog(
    catalog: &JsonValue,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<Option<WorkerRainControlCatalog>, ApiMaintenanceCandidateError> {
    let Some(entry) = select_installed_catalog_entry(catalog, "rain", "control")? else {
        return Ok(None);
    };
    let receipt = field(entry, "receipt").ok_or(ApiMaintenanceCandidateError::Catalog)?;
    // close every receipt member before interpreting its root transaction authority
    if !exact_keys(receipt, RECEIPT_KEYS) {
        return Err(ApiMaintenanceCandidateError::Catalog);
    }
    let action_sha256 = receipt_hash(receipt, "actionSha256")?;
    receipt_hash(receipt, "controlPlaneSha256")?;
    let control_state_sha256 = receipt_hash(receipt, "controlStateSha256")?;
    receipt_hash(receipt, "custodyAnchorSha256")?;
    let deployed_settings_sha256 = receipt_hash(receipt, "deployedSettingsSha256")?;
    let graph_manifest_sha256 = receipt_hash(receipt, "graphManifestSha256")?;
    let ordinal_artifact_sha256 = receipt_hash(receipt, "ordinalArtifactSha256")?;
    let source_member_root_sha256 = receipt_hash(receipt, "sourceMemberRootSha256")?;
    let source_receipt_root_sha256 = receipt_hash(receipt, "sourceReceiptRootSha256")?;
    let due_month = receipt_text(receipt, "dueMonth")?;
    let fencing_token = receipt_text(receipt, "fencingToken")?;
    let installed_at = receipt_text(receipt, "installedAt")?;
    // preserve the exact deployment spelling and installed-clock admission from Node
    if !text_is(
        receipt,
        "contractVersion",
        "adjustment-installed-rain-control-reference-receipt/v1",
    ) || !text_is(receipt, "actionKind", "control_reference")
        || !valid_month(&due_month)
        || !receipt_text(receipt, "deployedCommit").is_ok_and(|value| valid_commit(&value))
        || !receipt_text(receipt, "deployedImageDigest")
            .is_ok_and(|value| valid_image_digest(&value))
        || !receipt_text(receipt, "deployedRelease").is_ok_and(|value| valid_release(&value))
        || !valid_fencing_token(&fencing_token)
        || !codec.is_canonical_utc_instant(&installed_at)
    {
        return Err(ApiMaintenanceCandidateError::Catalog);
    }
    Ok(Some(WorkerRainControlCatalog {
        action_sha256,
        control_state_sha256,
        ordinal_artifact_sha256,
        due_month,
        deployed_settings_sha256,
        fencing_token,
        graph_manifest_sha256,
        source_member_root_sha256,
        source_receipt_root_sha256,
    }))
}

/// Validate the fixed public selector and its independently addressed pre-month action.
pub(crate) fn validate_worker_rain_control_catalog_members(
    codec: &mut impl ApiMaintenanceCandidateCodec,
    selected: &WorkerRainControlCatalog,
    selector_bytes: &[u8],
    action_bytes: &[u8],
) -> Result<String, ApiMaintenanceCandidateError> {
    let selector = codec
        .parse_canonical_object(selector_bytes)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    let action = codec
        .parse_canonical_object(action_bytes)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    validate_selector(&selector, selected)?;
    validate_action(&action, action_bytes, selected, codec)
}

/// Bind the public selector to the exact two content-addressed control members.
fn validate_selector(
    selector: &JsonValue,
    selected: &WorkerRainControlCatalog,
) -> Result<(), ApiMaintenanceCandidateError> {
    let control_path = format!(
        "rain-maintenance-control-states/sha256-{}.json",
        selected.control_state_sha256
    );
    let ordinal_path = format!(
        "rain-runtime-artifacts/sha256-{}.json",
        selected.ordinal_artifact_sha256
    );
    // reject another site, action, month, member, or public path substitution
    if !exact_keys(selector, SELECTOR_KEYS)
        || !text_is(
            selector,
            "contractVersion",
            "forecast-adjustment-rain-control-reference-registry/v1",
        )
        || !text_is(selector, "siteKey", "ballydidean")
        || !text_is(selector, "actionSha256", &selected.action_sha256)
        || !text_is(selector, "dueMonth", &selected.due_month)
        || !text_is(
            selector,
            "controlStateSha256",
            &selected.control_state_sha256,
        )
        || !text_is(
            selector,
            "ordinalArtifactSha256",
            &selected.ordinal_artifact_sha256,
        )
        || !text_is(selector, "controlStatePath", &control_path)
        || !text_is(selector, "ordinalArtifactPath", &ordinal_path)
    {
        return Err(ApiMaintenanceCandidateError::Package);
    }
    Ok(())
}

/// Bind one immutable pre-month action to its exact installed control receipt.
fn validate_action(
    action: &JsonValue,
    bytes: &[u8],
    selected: &WorkerRainControlCatalog,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<String, ApiMaintenanceCandidateError> {
    let created_at = text(action, "createdAt").ok_or(ApiMaintenanceCandidateError::Action)?;
    let valid_through = text(action, "validThrough").ok_or(ApiMaintenanceCandidateError::Action)?;
    // admit the two clocks before using their canonical lexical chronology
    if !exact_keys(action, ACTION_KEYS)
        || !codec.is_canonical_utc_instant(&created_at)
        || !codec.is_canonical_utc_instant(&valid_through)
        || created_at >= valid_through
        || !optional_hash(action, "expectedCatalogReceiptSha256")
        || !matches!(
            field(action, "predecessorActionSha256"),
            Some(JsonValue::Null)
        )
        || !text(action, "expectedSourceCommit").is_some_and(|value| valid_commit(&value))
        || !text(action, "expectedSourceRelease").is_some_and(|value| valid_release(&value))
    {
        return Err(ApiMaintenanceCandidateError::Action);
    }
    let action_sha256 = codec
        .sha256(bytes)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    // cross-bind every state, graph, and root transaction identity without serving authority
    if action_sha256.as_str() != selected.action_sha256
        || !text_is(
            action,
            "contractVersion",
            "forecast-adjustment-rain-control-reference-action/v1",
        )
        || !text_is(action, "actionKind", "control_reference")
        || !text_is(action, "reason", "premonth_reference")
        || !text_is(action, "family", "rain")
        || !text_is(action, "dueMonth", &selected.due_month)
        || !text_is(action, "fencingToken", &selected.fencing_token)
        || !text_is(
            action,
            "graphManifestSha256",
            &selected.graph_manifest_sha256,
        )
        || !text_is(action, "controlStateSha256", &selected.control_state_sha256)
        || !text_is(
            action,
            "ordinalArtifactSha256",
            &selected.ordinal_artifact_sha256,
        )
        || !text_is(
            action,
            "sourceMemberRootSha256",
            &selected.source_member_root_sha256,
        )
        || !text_is(
            action,
            "sourceReceiptRootSha256",
            &selected.source_receipt_root_sha256,
        )
        || !text_is(
            action,
            "expectedSettingsSha256",
            &selected.deployed_settings_sha256,
        )
    {
        return Err(ApiMaintenanceCandidateError::Action);
    }
    Ok(created_at)
}

/// Decode one receipt string while preserving catalog error ownership.
fn receipt_text(value: &JsonValue, key: &str) -> Result<String, ApiMaintenanceCandidateError> {
    text(value, key).ok_or(ApiMaintenanceCandidateError::Catalog)
}

/// Decode one required receipt hash.
fn receipt_hash(value: &JsonValue, key: &str) -> Result<String, ApiMaintenanceCandidateError> {
    let identity = receipt_text(value, key)?;
    // reject an addressed member before retaining its identity
    if !valid_hash(&identity) {
        return Err(ApiMaintenanceCandidateError::Catalog);
    }
    Ok(identity)
}

/// Recognize one nullable lowercase SHA-256 field.
fn optional_hash(value: &JsonValue, key: &str) -> bool {
    matches!(field(value, key), Some(JsonValue::Null))
        || text(value, key).is_some_and(|identity| valid_hash(&identity))
}

/// Compare one required string field.
fn text_is(value: &JsonValue, key: &str, expected: &str) -> bool {
    text(value, key).as_deref() == Some(expected)
}

/// Recognize one exact calendar month spelling without parsing a date.
fn valid_month(value: &str) -> bool {
    value.len() == 7
        && value.as_bytes()[..4].iter().all(u8::is_ascii_digit)
        && value.as_bytes()[4] == b'-'
        && matches!(
            &value.as_bytes()[5..],
            b"01"
                | b"02"
                | b"03"
                | b"04"
                | b"05"
                | b"06"
                | b"07"
                | b"08"
                | b"09"
                | b"10"
                | b"11"
                | b"12"
        )
}

/// Recognize one exact deployed commit spelling.
fn valid_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(is_lower_hex)
}

/// Recognize one exact deployed image digest spelling.
fn valid_image_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(valid_hash)
}

/// Recognize the immutable release spelling without interpreting its date.
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 12
        && bytes.get(4) == Some(&b'.')
        && bytes.get(7) == Some(&b'.')
        && bytes.get(10) == Some(&b'-')
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && (1..=3).contains(&bytes[11..].len())
        && bytes[11] != b'0'
        && bytes[11..].iter().all(u8::is_ascii_digit)
}

/// Recognize one positive bounded decimal fencing token.
fn valid_fencing_token(value: &str) -> bool {
    (1..=20).contains(&value.len())
        && value.as_bytes()[0] != b'0'
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Recognize one lowercase hexadecimal byte.
const fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
}
