//! Validate the operator-installed family target catalog without granting model authority.

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::capture_epoch_witness::resolve_operator_capture_clock;
use crate::maintenance_anchor_status::{
    MaintenanceAnchorStatusError, MaintenanceAnchorStatusJson, hash_field, regex_text,
};
use crate::maintenance_canonical_json::{exact_keys, field, text};

pub(crate) const V1: &str = "adjustment-installed-candidate-catalog/v1";
pub(crate) const V2: &str = "adjustment-installed-candidate-catalog/v2";
pub(crate) const V3: &str = "adjustment-installed-candidate-catalog/v3";

const ADJUSTMENT_FAMILY_MAXIMUM_CATALOG_BYTES: usize = 256 * 1_024;
const CATALOG_KEYS: &[&str] = &["contractVersion", "entries"];
const ENTRY_V1_KEYS: &[&str] = &["family", "receipt", "registration"];
const ENTRY_V2_KEYS: &[&str] = &["family", "receipt", "registration", "slot"];
const CONTROL_RECEIPT_KEYS: &[&str] = &[
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
const CONTROL_RECEIPT_HASH_KEYS: &[&str] = &[
    "actionSha256",
    "controlPlaneSha256",
    "controlStateSha256",
    "custodyAnchorSha256",
    "deployedSettingsSha256",
    "graphManifestSha256",
    "ordinalArtifactSha256",
    "sourceMemberRootSha256",
    "sourceReceiptRootSha256",
];
const CONTROL_RECEIPT_VERSION: &str = "adjustment-installed-rain-control-reference-receipt/v1";

#[derive(Clone, Copy, Eq, PartialEq)]
enum CatalogVersion {
    V1,
    V2,
    V3,
}

impl CatalogVersion {
    /// Return the original version-specific catalog cardinality limit.
    fn maximum_entries(self) -> usize {
        match self {
            Self::V1 => 3,
            Self::V2 => 6,
            Self::V3 => 7,
        }
    }

    /// Return the exact entry schema admitted by this catalog generation.
    fn entry_keys(self) -> &'static [&'static str] {
        match self {
            Self::V1 => ENTRY_V1_KEYS,
            Self::V2 | Self::V3 => ENTRY_V2_KEYS,
        }
    }
}

/// Validate the bounded operator catalog envelope and its additive control receipt only.
pub(crate) fn validate_family_target_catalog(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), MaintenanceAnchorStatusError> {
    let encoded = codec
        .encode_canonical_json(value)
        .ok_or(MaintenanceAnchorStatusError::Codec)?;
    // retain the source Buffer limit at the canonical document boundary
    if encoded.len() > ADJUSTMENT_FAMILY_MAXIMUM_CATALOG_BYTES {
        return Err(MaintenanceAnchorStatusError::Slot);
    }
    validate_catalog_envelope(value, codec, time).ok_or(MaintenanceAnchorStatusError::Slot)
}

/// Validate the closed catalog fields while leaving unrelated model values opaque.
fn validate_catalog_envelope(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Option<()> {
    exact_keys(value, CATALOG_KEYS).then_some(())?;
    let version = catalog_version(text(value, "contractVersion")?.as_str())?;
    let JsonValue::Array(entries) = field(value, "entries")? else {
        return None;
    };
    // retain the exact v1, v2 and v3 maximum entry counts
    if entries.len() > version.maximum_entries() {
        return None;
    }

    let mut identities = Vec::with_capacity(entries.len());
    let mut control_receipt = None;
    // validate only the envelope of every entry before selecting the additive control receipt
    for entry in entries {
        exact_keys(entry, version.entry_keys()).then_some(())?;
        let family = text(entry, "family")?;
        // admit only the three original family names
        if !matches!(family.as_str(), "temperature" | "wind" | "rain") {
            return None;
        }
        let slot = match version {
            CatalogVersion::V1 => None,
            CatalogVersion::V2 | CatalogVersion::V3 => Some(text(entry, "slot")?),
        };
        // close the version-specific active, shadow and control slot set
        if !valid_slot(version, slot.as_deref()) {
            return None;
        }
        // keep the sole control role rain-only and registration-free
        if slot.as_deref() == Some("control")
            && (family != "rain" || !matches!(field(entry, "registration"), Some(JsonValue::Null)))
        {
            return None;
        }
        let identity = (family, slot.clone().unwrap_or_default());
        // reject a repeated v1 family or v2/v3 family-slot identity
        if identities.iter().any(|existing| existing == &identity) {
            return None;
        }
        identities.push(identity);
        // retain only the control receipt for its separate custody-only validation
        if slot.as_deref() == Some("control") {
            control_receipt = field(entry, "receipt");
        }
    }
    // do not confer stricter candidate authority on unrelated model receipts or registrations
    if let Some(receipt) = control_receipt {
        validate_control_receipt(receipt, codec, time)?;
    }
    Some(())
}

/// Decode one of the three installed catalog contract versions.
fn catalog_version(value: &str) -> Option<CatalogVersion> {
    match value {
        V1 => Some(CatalogVersion::V1),
        V2 => Some(CatalogVersion::V2),
        V3 => Some(CatalogVersion::V3),
        _ => None,
    }
}

/// Match the version-specific slot grammar without inspecting model receipts.
fn valid_slot(version: CatalogVersion, slot: Option<&str>) -> bool {
    match version {
        CatalogVersion::V1 => slot.is_none(),
        CatalogVersion::V2 => matches!(slot, Some("active" | "shadow")),
        CatalogVersion::V3 => matches!(slot, Some("active" | "shadow" | "control")),
    }
}

/// Validate the closed custody-only rain control-reference receipt.
fn validate_control_receipt(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Option<()> {
    exact_keys(value, CONTROL_RECEIPT_KEYS).then_some(())?;
    // require every immutable control and deployment identity
    if !CONTROL_RECEIPT_HASH_KEYS
        .iter()
        .all(|key| hash_field(value, key))
    {
        return None;
    }
    let due_month = regex_text(value, "dueMonth", codec)?;
    let deployed_commit = regex_text(value, "deployedCommit", codec)?;
    let deployed_image_digest = regex_text(value, "deployedImageDigest", codec)?;
    let deployed_release = regex_text(value, "deployedRelease", codec)?;
    let fencing_token = regex_text(value, "fencingToken", codec)?;
    let installed_at = text(value, "installedAt")?;
    // preserve strict version/action equality and the original regex-coercing deployment fields
    (text(value, "contractVersion")?.as_str() == CONTROL_RECEIPT_VERSION
        && text(value, "actionKind")?.as_str() == "control_reference"
        && valid_due_month(&due_month)
        && valid_commit(&deployed_commit)
        && valid_image_digest(&deployed_image_digest)
        && valid_release(&deployed_release)
        && valid_fencing_token(&fencing_token)
        && valid_installed_at(&installed_at, time))
    .then_some(())
}

/// Match any four-digit year and an exact 01-through-12 month.
fn valid_due_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && matches!(
            &bytes[5..],
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

/// Match the exact forty-character lowercase hexadecimal Git identity.
fn valid_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(lower_hex)
}

/// Match the exact lowercase sha256 container digest spelling.
fn valid_image_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| digest.len() == 64 && digest.bytes().all(lower_hex))
}

/// Match the original fixed-width release date and unbounded positive revision spelling.
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 12
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'.'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'.'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'-'
        && matches!(bytes[11], b'1'..=b'9')
        && bytes[12..].iter().all(u8::is_ascii_digit)
}

/// Match the positive decimal grammar and the original unsigned 64-bit ceiling.
fn valid_fencing_token(value: &str) -> bool {
    (1..=20).contains(&value.len())
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

/// Match the ordinary millisecond UTC spelling and require a canonical JavaScript date.
fn valid_installed_at(value: &str, time: &dyn CurrentTimeResolver) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 24
        && bytes
            .iter()
            .zip(b"dddd-dd-ddTdd:dd:dd.dddZ")
            .all(|(&observed, &expected)| {
                // require ASCII digits only at the variable clock positions
                if expected == b'd' {
                    observed.is_ascii_digit()
                } else {
                    observed == expected
                }
            })
        && resolve_operator_capture_clock(value, time).is_some()
}

/// Recognize one lowercase hexadecimal byte.
const fn lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
}
