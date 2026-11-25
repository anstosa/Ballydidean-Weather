//! Cross-bind authorized family actions to their exact installed target catalog material.

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::capture_epoch_witness::verify_operator_capture_epoch_witness;
use crate::maintenance_canonical_json::{field, text};
use crate::{MaintenanceAnchorStatusError, MaintenanceAnchorStatusJson};

use MaintenanceAnchorStatusError::{Codec, Slot};

/// verify the inactive rain-control action against one immutable target entry.
pub(crate) fn verify_rain_target_binding(
    action: &JsonValue,
    action_sha256: &str,
    authority: &JsonValue,
    catalog: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<(), MaintenanceAnchorStatusError> {
    let entry = find_rain_control_entry(catalog);
    let receipt = entry.and_then(|value| field(value, "receipt"));

    // reject any pre-hash coordinate that differs from the original binding
    if text(authority, "contractVersion").as_deref()
        != Some("adjustment-rain-control-custody-anchor/v1")
        || text(authority, "actionKind").as_deref() != Some("control_reference")
        || text(action, "actionKind").as_deref() != Some("control_reference")
        || text(authority, "actionSha256").as_deref() != Some(action_sha256)
        || !same_field(authority, "sourceCommit", action, "expectedSourceCommit")
        || !same_field(authority, "fencingToken", action, "fencingToken")
        || !same_field(authority, "dueMonth", action, "dueMonth")
        || !same_field(
            authority,
            "graphManifestSha256",
            action,
            "graphManifestSha256",
        )
        || !same_field(
            authority,
            "controlStateSha256",
            action,
            "controlStateSha256",
        )
        || !same_field(
            authority,
            "ordinalArtifactSha256",
            action,
            "ordinalArtifactSha256",
        )
        || !same_field(
            authority,
            "sourceMemberRootSha256",
            action,
            "sourceMemberRootSha256",
        )
        || !same_field(
            authority,
            "sourceReceiptRootSha256",
            action,
            "sourceReceiptRootSha256",
        )
        || entry.is_none()
        || !matches!(
            entry.and_then(|value| field(value, "registration")),
            Some(JsonValue::Null)
        )
        || receipt
            .and_then(|value| text(value, "actionSha256"))
            .as_deref()
            != Some(action_sha256)
    {
        return Err(Slot);
    }

    let authority_bytes = codec.encode_canonical_json(authority).ok_or(Codec)?;
    let authority_sha256 = codec.sha256(&authority_bytes).ok_or(Codec)?;
    let receipt = receipt.ok_or(Slot)?;
    // bind the receipt to the full authority document and every control identity
    if text(receipt, "custodyAnchorSha256").as_deref() != Some(authority_sha256.as_str())
        || !same_field(receipt, "controlStateSha256", action, "controlStateSha256")
        || !same_field(
            receipt,
            "ordinalArtifactSha256",
            action,
            "ordinalArtifactSha256",
        )
        || !same_field(
            receipt,
            "graphManifestSha256",
            action,
            "graphManifestSha256",
        )
        || !same_field(
            receipt,
            "sourceMemberRootSha256",
            action,
            "sourceMemberRootSha256",
        )
        || !same_field(
            receipt,
            "sourceReceiptRootSha256",
            action,
            "sourceReceiptRootSha256",
        )
    {
        return Err(Slot);
    }
    Ok(())
}

/// verify the pending development action against its shadow target and capture witness.
pub(crate) fn verify_development_target_binding(
    action: &JsonValue,
    action_sha256: &str,
    authority: &JsonValue,
    catalog: &JsonValue,
    witness_bytes: &[u8],
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), MaintenanceAnchorStatusError> {
    let witness = verify_operator_capture_epoch_witness(witness_bytes, codec, time)
        .map_err(|_| Slot)?
        .witness;
    let witness_document_sha256 = codec.sha256(witness_bytes).ok_or(Codec)?;
    let entry = find_development_entry(catalog, field(action, "family"));
    let receipt = entry.and_then(|value| field(value, "receipt"));
    let registration = entry.and_then(|value| field(value, "registration"));

    // preserve the original pending-only cross-binding and comparison order
    if text(authority, "contractVersion").as_deref()
        != Some("adjustment-development-custody-anchor/v1")
        || text(authority, "actionKind").as_deref() != Some("shadow")
        || text(action, "actionKind").as_deref() != Some("shadow")
        || text(action, "policyDecision").as_deref() != Some("pending")
        || !matches!(field(action, "fullMemberRootSha256"), Some(JsonValue::Null))
        || text(authority, "actionSha256").as_deref() != Some(action_sha256)
        || !same_field(authority, "family", action, "family")
        || !same_field(
            authority,
            "candidateGraphSha256",
            action,
            "candidateGraphSha256",
        )
        || !same_field(authority, "candidateSha256", action, "candidateSha256")
        || !same_field(
            authority,
            "policyReportSha256",
            action,
            "policyReportSha256",
        )
        || !same_field(
            authority,
            "lifecycleLedgerRootSha256",
            action,
            "lifecycleHeadSha256",
        )
        || !same_field(authority, "sourceCommit", action, "expectedSourceCommit")
        || entry.is_none()
        || receipt
            .and_then(|value| text(value, "actionSha256"))
            .as_deref()
            != Some(action_sha256)
        || !same_optional_fields(
            registration,
            "artifactSha256",
            Some(authority),
            "artifactSha256",
        )
        || !same_optional_fields(receipt, "bundleSha256", Some(authority), "artifactSha256")
        || !same_optional_fields(
            registration,
            "registrationSha256",
            Some(authority),
            "registrationSha256",
        )
        || !same_optional_fields(
            registration,
            "sourceSha256",
            Some(authority),
            "sourceSha256",
        )
        || text(authority, "captureEpochWitnessSha256").as_deref()
            != Some(witness_document_sha256.as_str())
        || registration
            .and_then(|value| text(value, "epochWitnessSha256"))
            .as_deref()
            != Some(witness.witnessSha256.as_str())
    {
        return Err(Slot);
    }
    Ok(())
}

// select the original rain control entry without accepting a shadow alias
fn find_rain_control_entry(catalog: &JsonValue) -> Option<&JsonValue> {
    let JsonValue::Array(entries) = field(catalog, "entries")? else {
        return None;
    };
    entries.iter().find(|entry| {
        text(entry, "family").as_deref() == Some("rain")
            && text(entry, "slot").as_deref() == Some("control")
    })
}

// select the shadow entry using JavaScript strict equality for its family
fn find_development_entry<'a>(
    catalog: &'a JsonValue,
    family: Option<&JsonValue>,
) -> Option<&'a JsonValue> {
    let JsonValue::Array(entries) = field(catalog, "entries")? else {
        return None;
    };
    entries.iter().find(|entry| {
        strict_equal(field(entry, "family"), family)
            && text(entry, "slot").as_deref() == Some("shadow")
    })
}

// compare two named fields with JavaScript strict scalar equality
fn same_field(left: &JsonValue, left_key: &str, right: &JsonValue, right_key: &str) -> bool {
    strict_equal(field(left, left_key), field(right, right_key))
}

// compare fields borrowed through optional catalog members
fn same_optional_fields(
    left: Option<&JsonValue>,
    left_key: &str,
    right: Option<&JsonValue>,
    right_key: &str,
) -> bool {
    strict_equal(
        left.and_then(|value| field(value, left_key)),
        right.and_then(|value| field(value, right_key)),
    )
}

// reject structural equality for independently parsed arrays and objects
fn strict_equal(left: Option<&JsonValue>, right: Option<&JsonValue>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(JsonValue::Array(_) | JsonValue::Object(_)), _)
        | (_, Some(JsonValue::Array(_) | JsonValue::Object(_))) => false,
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}
