//! Build the original root-installed target catalog without mutating the installed release.

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::family_model_target_catalog::build_model_target_catalog;
use crate::family_target_catalog_validation::{V1, V3, validate_family_target_catalog};
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{MaintenanceAnchorStatusError, MaintenanceAnchorStatusJson};

use MaintenanceAnchorStatusError::{Codec, Slot};

const CONTROL_ACTION_VERSION: &str = "forecast-adjustment-rain-control-reference-action/v1";
const CONTROL_SELECTOR_VERSION: &str = "forecast-adjustment-rain-control-reference-registry/v1";
const RUNTIME_PACKAGE_MAX_BYTES: usize = 8 * 1024 * 1024;

/// The already admitted release inputs used by the original catalog constructors.
pub struct FamilyTargetCatalogInput<'a> {
    pub action: &'a JsonValue,
    pub action_sha256: &'a str,
    pub family: &'a str,
    pub source: &'a JsonValue,
    pub environment: &'a JsonValue,
    pub image_commit: &'a str,
    pub settings_bytes: &'a [u8],
    pub clock: &'a dyn Fn() -> Result<String, MaintenanceAnchorStatusError>,
    pub custody_anchor_sha256: Option<&'a str>,
}

/// Read one exact bounded immutable image member through the existing image owner.
pub trait FamilyTargetImageRead {
    /// Return the selected file bytes without running any application or release mutation.
    fn read_member(
        &mut self,
        path: &str,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, MaintenanceAnchorStatusError>;
}

/// Select the original model or independent rain-control catalog constructor.
pub fn build_family_target_catalog(
    input: &FamilyTargetCatalogInput<'_>,
    image: &mut impl FamilyTargetImageRead,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    // preserve the separate control-reference path selected by the existing release operator
    if text(input.action, "contractVersion").as_deref() == Some(CONTROL_ACTION_VERSION) {
        build_rain_target_catalog(input, image, codec, time)
    } else {
        build_model_target_catalog(input, image, codec, time)
    }
}

// retain the original source's array before selecting or rotating any family slot
pub(crate) fn catalog_entries(
    catalog: &JsonValue,
) -> Result<&[JsonValue], MaintenanceAnchorStatusError> {
    let Some(JsonValue::Array(entries)) = field(catalog, "entries") else {
        return Err(Slot);
    };
    Ok(entries)
}

// retain a generated string with no alternative serialization
pub(crate) fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

// construct an exact name-value object for subsequent canonical encoding
pub(crate) fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

// retain one exact admitted field without changing its scalar or nested bytes
pub(crate) fn clone_member(
    value: &JsonValue,
    key: &str,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    field(value, key).cloned().ok_or(Slot)
}

// read the canonical bounded object selected from immutable target-image material
pub(crate) fn canonical_member(
    bytes: &[u8],
    maximum: usize,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    // maintain the actual image reader's original material byte limits
    if !(2..=maximum).contains(&bytes.len()) {
        return Err(Slot);
    }
    let value = codec.parse_canonical_object(bytes).ok_or(Slot)?;
    if !matches!(value, JsonValue::Object(_))
        || codec.encode_canonical_json(&value).as_deref() != Some(bytes)
    {
        return Err(Slot);
    }
    Ok(value)
}

// upgrade only a legacy entry's missing slot, retaining every nested original value
pub(crate) fn add_shadow_slot(
    entry: &JsonValue,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    let JsonValue::Object(mut fields) = entry.clone() else {
        return Err(Slot);
    };
    let slot = "slot".encode_utf16().collect::<Vec<_>>();
    // preserve object spread followed by the explicit shadow slot assignment
    fields.retain(|(key, _)| key != &slot);
    fields.push((slot, string("shadow")));
    Ok(JsonValue::Object(fields))
}

// order the fixed family and slot strings exactly like the original locale comparison
pub(crate) fn sort_catalog_entries(entries: &mut [JsonValue]) {
    entries.sort_by_key(|entry| (text(entry, "family"), text(entry, "slot")));
}

// project the shared deployment receipt fields only after the image members have been admitted
pub(crate) fn receipt_environment(
    input: &FamilyTargetCatalogInput<'_>,
    settings_sha256: &str,
) -> Result<Vec<(&'static str, JsonValue)>, MaintenanceAnchorStatusError> {
    let reference = text(input.environment, "WEATHER_SERVER_IMAGE").ok_or(Slot)?;
    let digest = reference.split('@').nth(1).ok_or(Slot)?;
    Ok(vec![
        (
            "controlPlaneSha256",
            clone_member(input.environment, "WEATHER_CONTROL_PLANE_SHA256")?,
        ),
        ("deployedCommit", string(input.image_commit)),
        ("deployedImageDigest", string(digest)),
        (
            "deployedRelease",
            clone_member(input.environment, "WEATHER_RELEASE")?,
        ),
        ("deployedSettingsSha256", string(settings_sha256)),
        ("installedAt", string(&(input.clock)()?)),
    ])
}

// hash one exact canonical retained slot receipt, preserving null for an absent control
fn previous_control_receipt(
    source: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    let control = catalog_entries(source)?.iter().find(|entry| {
        text(entry, "family").as_deref() == Some("rain")
            && text(entry, "slot").as_deref() == Some("control")
    });
    let Some(control) = control else {
        return Ok(JsonValue::Null);
    };
    let receipt = field(control, "receipt").ok_or(Slot)?;
    let bytes = codec.encode_canonical_json(receipt).ok_or(Codec)?;
    let hash = codec.sha256(&bytes).ok_or(Codec)?;
    Ok(string(hash.as_str()))
}

// build the separate rain control catalog with exact source compare-and-swap and image byte identity
fn build_rain_target_catalog(
    input: &FamilyTargetCatalogInput<'_>,
    image: &mut impl FamilyTargetImageRead,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    let action = input.action;
    let custody = input
        .custody_anchor_sha256
        .filter(|value| valid_hash(value))
        .ok_or(Slot)?;
    // preserve the original fixed custody-only action and operand checks
    if text(action, "actionKind").as_deref() != Some("control_reference")
        || text(action, "family").as_deref() != Some("rain")
        || !valid_hash(input.action_sha256)
    {
        return Err(Slot);
    }
    validate_family_target_catalog(input.source, codec, time)?;
    let prior = previous_control_receipt(input.source, codec)?;
    // verify the exact previous control receipt before observing the settings or image
    if field(action, "expectedCatalogReceiptSha256") != Some(&prior) {
        return Err(Slot);
    }
    let settings = codec.sha256(input.settings_bytes).ok_or(Codec)?;
    if text(action, "expectedSettingsSha256").as_deref() != Some(settings.as_str()) {
        return Err(Slot);
    }
    let selector = image.read_member(
        "config/forecast-adjustments/ballydidean-rain-control-reference.json",
        16 * 1024,
    )?;
    let selector = canonical_member(&selector, 16 * 1024, codec)?;
    // require the fixed selector's complete path-free identity contract
    if !exact_keys(
        &selector,
        &[
            "actionSha256",
            "contractVersion",
            "controlStatePath",
            "controlStateSha256",
            "dueMonth",
            "ordinalArtifactPath",
            "ordinalArtifactSha256",
            "siteKey",
        ],
    ) {
        return Err(Slot);
    }
    let state_hash = text(action, "controlStateSha256").ok_or(Slot)?;
    let artifact_hash = text(action, "ordinalArtifactSha256").ok_or(Slot)?;
    let state_path = format!("rain-maintenance-control-states/sha256-{state_hash}.json");
    let artifact_path = format!("rain-runtime-artifacts/sha256-{artifact_hash}.json");
    // bind every selected image identity before reading the two content-addressed members
    if text(&selector, "actionSha256").as_deref() != Some(input.action_sha256)
        || text(&selector, "contractVersion").as_deref() != Some(CONTROL_SELECTOR_VERSION)
        || text(&selector, "siteKey").as_deref() != Some("ballydidean")
        || field(&selector, "dueMonth") != field(action, "dueMonth")
        || field(&selector, "controlStateSha256") != field(action, "controlStateSha256")
        || field(&selector, "ordinalArtifactSha256") != field(action, "ordinalArtifactSha256")
        || text(&selector, "controlStatePath").as_deref() != Some(state_path.as_str())
        || text(&selector, "ordinalArtifactPath").as_deref() != Some(artifact_path.as_str())
        || !valid_hash(&state_hash)
        || !valid_hash(&artifact_hash)
    {
        return Err(Slot);
    }
    check_image_member(image, codec, &state_path, 64 * 1024, &state_hash)?;
    check_image_member(
        image,
        codec,
        &artifact_path,
        RUNTIME_PACKAGE_MAX_BYTES,
        &artifact_hash,
    )?;

    let mut receipt = receipt_environment(input, settings.as_str())?;
    receipt.extend([
        ("actionKind", string("control_reference")),
        ("actionSha256", string(input.action_sha256)),
        (
            "contractVersion",
            string("adjustment-installed-rain-control-reference-receipt/v1"),
        ),
        ("custodyAnchorSha256", string(custody)),
    ]);
    // retain the exact action-supplied immutable control receipt coordinates
    for key in [
        "controlStateSha256",
        "dueMonth",
        "fencingToken",
        "graphManifestSha256",
        "ordinalArtifactSha256",
        "sourceMemberRootSha256",
        "sourceReceiptRootSha256",
    ] {
        receipt.push((key, clone_member(action, key)?));
    }
    let receipt = object(receipt);
    let legacy = text(input.source, "contractVersion").as_deref() == Some(V1);
    let mut entries = Vec::with_capacity(7);
    // rotate only the independent control slot and preserve untouched active/shadow entries
    for entry in catalog_entries(input.source)? {
        if text(entry, "slot").as_deref() != Some("control") {
            entries.push(if legacy {
                add_shadow_slot(entry)?
            } else {
                entry.clone()
            });
        }
    }
    entries.push(object(vec![
        ("family", string("rain")),
        ("receipt", receipt),
        ("registration", JsonValue::Null),
        ("slot", string("control")),
    ]));
    sort_catalog_entries(&mut entries);
    let value = object(vec![
        ("contractVersion", string(V3)),
        ("entries", JsonValue::Array(entries)),
    ]);
    validate_family_target_catalog(&value, codec, time)?;
    Ok(value)
}

// verify one content-addressed image member without reading or allocating beyond its original cap
fn check_image_member(
    image: &mut impl FamilyTargetImageRead,
    codec: &mut impl MaintenanceAnchorStatusJson,
    relative: &str,
    maximum: usize,
    expected: &str,
) -> Result<(), MaintenanceAnchorStatusError> {
    let path = format!("config/forecast-adjustments/ballydidean/{relative}");
    let bytes = image.read_member(&path, maximum)?;
    // reject failed material bounds or a differing original content digest
    if !(2..=maximum).contains(&bytes.len())
        || codec.sha256(&bytes).ok_or(Codec)?.as_str() != expected
    {
        return Err(Slot);
    }
    Ok(())
}
