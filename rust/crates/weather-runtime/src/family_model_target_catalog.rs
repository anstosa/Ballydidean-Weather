//! Build the original model-family target catalog from one admitted action and image.

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::family_target_catalog::{
    FamilyTargetCatalogInput, FamilyTargetImageRead, add_shadow_slot, canonical_member,
    catalog_entries, clone_member, object, receipt_environment, sort_catalog_entries, string,
};
use crate::family_target_catalog_validation::{V1, V2, V3, validate_family_target_catalog};
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{MaintenanceAnchorStatusError, MaintenanceAnchorStatusJson};

use MaintenanceAnchorStatusError::{Codec, Slot};

const SHADOW_PROJECTION_VERSION: &str = "forecast-adjustment-shadow-catalog-projection/v1";
const INSTALLED_RECEIPT_VERSION: &str = "adjustment-installed-candidate-receipt/v2";
const SHADOW_PROJECTION_MAXIMUM_BYTES: usize = 128 * 1024;

/// Derive the original installed model catalog without mutating release state or running the image.
pub(crate) fn build_model_target_catalog(
    input: &FamilyTargetCatalogInput<'_>,
    image: &mut impl FamilyTargetImageRead,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    let legacy = text(input.source, "contractVersion").as_deref() == Some(V1);
    // upgrade every legacy family entry to the original implicit shadow slot before selection
    let source_entries = catalog_entries(input.source)?
        .iter()
        .map(|entry| {
            // attach the implicit legacy slot without changing the retained entry contents
            if legacy {
                add_shadow_slot(entry)
            } else {
                Ok(entry.clone())
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    // remove only this model family's mutable slots while preserving its independent control slot
    let mut entries = source_entries
        .iter()
        .filter(|entry| {
            text(entry, "family").as_deref() != Some(input.family)
                || text(entry, "slot").as_deref() == Some("control")
        })
        .cloned()
        .collect::<Vec<_>>();
    let active = source_entries.iter().find(|entry| {
        text(entry, "family").as_deref() == Some(input.family)
            && text(entry, "slot").as_deref() == Some("active")
    });
    let shadow = source_entries.iter().find(|entry| {
        text(entry, "family").as_deref() == Some(input.family)
            && text(entry, "slot").as_deref() == Some("shadow")
    });
    let action_kind = text(input.action, "actionKind");

    // retain serving authority while a distinct inactive candidate is installed
    if action_kind.as_deref() == Some("shadow")
        && let Some(active) = active
    {
        entries.push(active.clone());
    }
    // raw fallback removes only active authority and preserves a pending candidate
    if action_kind.as_deref() == Some("raw")
        && let Some(shadow) = shadow
    {
        entries.push(shadow.clone());
    }
    // install the action-bound public projection only into the inactive slot
    if action_kind.as_deref() == Some("shadow") {
        entries.push(build_shadow_entry(input, image, codec)?);
    }
    // consume the exact pending candidate when qualified active authority is promoted
    if action_kind.as_deref() == Some("promote") {
        entries.push(build_active_entry(input, shadow.ok_or(Slot)?, codec)?);
    }
    // the catalog does not retain a separately qualified predecessor for prior rollback
    if action_kind.as_deref() == Some("rollback_prior") {
        return Err(Slot);
    }

    sort_catalog_entries(&mut entries);
    let source_is_v3 = text(input.source, "contractVersion").as_deref() == Some(V3);
    let has_control = entries
        .iter()
        .any(|entry| text(entry, "slot").as_deref() == Some("control"));
    // retain v3 once observed and introduce it whenever an independent control slot exists
    let version = if source_is_v3 || has_control { V3 } else { V2 };
    let value = object(vec![
        ("contractVersion", string(version)),
        ("entries", JsonValue::Array(entries)),
    ]);
    validate_family_target_catalog(&value, codec, time)?;
    Ok(value)
}

// bind one shadow action to its exact canonical public image projection and receipt
fn build_shadow_entry(
    input: &FamilyTargetCatalogInput<'_>,
    image: &mut impl FamilyTargetImageRead,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    let candidate = text(input.action, "candidateSha256")
        .filter(|value| valid_hash(value))
        .ok_or(Slot)?;
    let path = format!(
        "config/forecast-adjustments/ballydidean/shadow-catalog/{}/sha256-{candidate}.json",
        input.family
    );
    let bytes = image.read_member(&path, SHADOW_PROJECTION_MAXIMUM_BYTES)?;
    let projection = canonical_member(&bytes, SHADOW_PROJECTION_MAXIMUM_BYTES, codec)?;
    // close the public projection before comparing it with this exact selected action
    if !exact_keys(
        &projection,
        &[
            "actionSha256",
            "artifactSha256",
            "bundleSha256",
            "candidateGraphSha256",
            "candidateSha256",
            "contractVersion",
            "family",
            "paritySha256",
            "registration",
            "siteKey",
        ],
    ) || text(&projection, "contractVersion").as_deref() != Some(SHADOW_PROJECTION_VERSION)
        || text(&projection, "actionSha256").as_deref() != Some(input.action_sha256)
        || text(&projection, "family").as_deref() != Some(input.family)
        || text(&projection, "candidateSha256").as_deref() != Some(candidate.as_str())
        || !strict_scalar_equal(
            field(&projection, "candidateGraphSha256"),
            field(input.action, "candidateGraphSha256"),
        )
        || text(&projection, "siteKey").as_deref() != Some("ballydidean")
    {
        return Err(Slot);
    }

    let settings = codec.sha256(input.settings_bytes).ok_or(Codec)?;
    let mut receipt = vec![
        ("actionKind", string("shadow")),
        ("actionSha256", string(input.action_sha256)),
        ("bundleSha256", clone_member(&projection, "bundleSha256")?),
        (
            "candidateGraphSha256",
            clone_member(&projection, "candidateGraphSha256")?,
        ),
        ("candidateSha256", string(&candidate)),
        ("contractVersion", string(INSTALLED_RECEIPT_VERSION)),
    ];
    receipt.extend(receipt_environment(input, settings.as_str())?);
    receipt.extend([
        ("fencingToken", clone_member(input.action, "fencingToken")?),
        ("fullMemberRootSha256", JsonValue::Null),
        (
            "lifecycleHeadSha256",
            clone_member(input.action, "lifecycleHeadSha256")?,
        ),
        (
            "policyDecision",
            clone_member(input.action, "policyDecision")?,
        ),
        (
            "policyReportSha256",
            clone_member(input.action, "policyReportSha256")?,
        ),
    ]);
    // mirror JavaScript optional properties by omitting missing nested registration members
    receipt.extend(
        ["registrationSha256", "sourceSha256"]
            .into_iter()
            .filter_map(|key| {
                nested_field(&projection, "registration", key)
                    .cloned()
                    .map(|v| (key, v))
            }),
    );
    Ok(object(vec![
        ("family", string(input.family)),
        ("receipt", object(receipt)),
        ("registration", clone_member(&projection, "registration")?),
        ("slot", string("shadow")),
    ]))
}

// require the selected installed shadow and project its immutable artifact into active authority
fn build_active_entry(
    input: &FamilyTargetCatalogInput<'_>,
    shadow: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    // preserve the original closed pending entry before observing any nested receipt value
    if !exact_keys(shadow, &["family", "receipt", "registration", "slot"]) {
        return Err(Slot);
    }
    let source_receipt = field(shadow, "receipt").ok_or(Slot)?;
    // bind qualification to the pending candidate, graph and exact registered bundle
    if text(source_receipt, "contractVersion").as_deref() != Some(INSTALLED_RECEIPT_VERSION)
        || text(source_receipt, "actionKind").as_deref() != Some("shadow")
        || text(source_receipt, "policyDecision").as_deref() != Some("pending")
        || field(source_receipt, "fullMemberRootSha256") != Some(&JsonValue::Null)
        || !strict_scalar_equal(
            field(source_receipt, "candidateSha256"),
            field(input.action, "candidateSha256"),
        )
        || !strict_scalar_equal(
            field(source_receipt, "candidateGraphSha256"),
            field(input.action, "candidateGraphSha256"),
        )
        || !strict_scalar_equal(
            nested_field(shadow, "registration", "candidateSha256"),
            field(input.action, "candidateSha256"),
        )
        || !strict_scalar_equal(
            nested_field(shadow, "registration", "artifactSha256"),
            field(source_receipt, "bundleSha256"),
        )
    {
        return Err(Slot);
    }
    let registration = field(shadow, "registration").ok_or(Slot)?;
    let settings = codec.sha256(input.settings_bytes).ok_or(Codec)?;
    let mut receipt = vec![
        ("actionKind", string("promote")),
        ("actionSha256", string(input.action_sha256)),
        (
            "bundleSha256",
            clone_member(source_receipt, "bundleSha256")?,
        ),
        (
            "candidateGraphSha256",
            clone_member(input.action, "candidateGraphSha256")?,
        ),
        (
            "candidateSha256",
            clone_member(input.action, "candidateSha256")?,
        ),
        ("contractVersion", string(INSTALLED_RECEIPT_VERSION)),
    ];
    receipt.extend(receipt_environment(input, settings.as_str())?);
    receipt.extend([
        ("fencingToken", clone_member(input.action, "fencingToken")?),
        (
            "fullMemberRootSha256",
            clone_member(input.action, "fullMemberRootSha256")?,
        ),
        (
            "lifecycleHeadSha256",
            clone_member(input.action, "lifecycleHeadSha256")?,
        ),
        (
            "policyDecision",
            clone_member(input.action, "policyDecision")?,
        ),
        (
            "policyReportSha256",
            clone_member(input.action, "policyReportSha256")?,
        ),
        (
            "registrationSha256",
            clone_member(registration, "registrationSha256")?,
        ),
        ("sourceSha256", clone_member(registration, "sourceSha256")?),
    ]);
    Ok(object(vec![
        ("family", string(input.family)),
        ("receipt", object(receipt)),
        ("registration", registration.clone()),
        ("slot", string("active")),
    ]))
}

// borrow one optional nested member with JavaScript optional-property semantics
fn nested_field<'a>(value: &'a JsonValue, parent: &str, key: &str) -> Option<&'a JsonValue> {
    field(field(value, parent)?, key)
}

// preserve JavaScript strict equality for separately parsed JSON scalar locations
fn strict_scalar_equal(left: Option<&JsonValue>, right: Option<&JsonValue>) -> bool {
    // separately parsed arrays and objects can never share JavaScript reference identity
    if left.is_some_and(|value| matches!(value, JsonValue::Array(_) | JsonValue::Object(_)))
        || right.is_some_and(|value| matches!(value, JsonValue::Array(_) | JsonValue::Object(_)))
    {
        return false;
    }
    left == right
}
