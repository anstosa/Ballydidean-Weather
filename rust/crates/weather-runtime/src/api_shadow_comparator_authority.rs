//! Admission of embedded comparator serving artifacts and receipts.

use weather_api::admit_rain_portable_artifact;
use weather_contracts::{
    AdjustmentShadowFamily, JsonValue,
    LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveArtifactSha256Literal,
};

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_maintenance_candidate_package::verify_comparator_maintenance_package;
use crate::api_shadow_legacy_runtime::verify_legacy_wind_runtime_bundle;
use crate::api_shadow_legacy_temperature::verify_legacy_temperature_bundle;
use crate::api_shadow_legacy_wind_canary::verify_legacy_wind_canary_bundle;
use crate::api_shadow_wire_validation::{ShadowFamily, ordered_keys};
use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::worker_rain_serving_registry::RETAINED_RAIN_ARTIFACT;

const AUTHORITY_KEYS: &[&str] = &[
    "artifactBase64",
    "artifactIdentitySha256",
    "artifactMemberSha256",
    "authorityKind",
    "receiptBase64",
    "receiptMemberSha256",
];
const QUALIFIED_RECEIPT_KEYS: &[&str] = &[
    "actionKind",
    "actionSha256",
    "bundleSha256",
    "candidateGraphSha256",
    "candidateSha256",
    "contractVersion",
    "controlPlaneSha256",
    "deployedCommit",
    "deployedImageDigest",
    "deployedRelease",
    "deployedSettingsSha256",
    "fencingToken",
    "fullMemberRootSha256",
    "installedAt",
    "lifecycleHeadSha256",
    "policyDecision",
    "policyReportSha256",
    "registrationSha256",
    "sourceSha256",
];
const TEMP_RAW_REGISTRY_KEYS: &[&str] =
    &["activeBundle", "contractVersion", "rawReason", "siteKey"];
const WIND_RAW_REGISTRY_KEYS: &[&str] = &[
    "activeBundle",
    "contractVersion",
    "enabledMetricBands",
    "rawReason",
    "siteKey",
];
const MAINTENANCE_RAW_REGISTRY_KEYS: &[&str] =
    &["activePackage", "contractVersion", "rawReason", "siteKey"];
const RAIN_RAW_REGISTRY_KEYS: &[&str] =
    &["activeArtifact", "contractVersion", "rawReason", "siteKey"];
const RAIN_ACTIVE_ARTIFACT_KEYS: &[&str] = &["artifactSha256"];
const MINIMUM_BASE64_UNITS: usize = 4;
const MAXIMUM_BASE64_UNITS: usize = 1_000_000;

/// Admit one exact comparator authority with its original artifact and receipt binding.
pub(crate) fn admit_shadow_comparator_authority(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    node: &JsonValue,
    family: ShadowFamily,
) -> Option<()> {
    ordered_keys(node, AUTHORITY_KEYS)?;
    let receipt_member_sha256 = text(node, "receiptMemberSha256")?;
    let authority_kind = text(node, "authorityKind")?;
    // restrict authority generation before allocating either embedded member
    if !valid_hash(&receipt_member_sha256)
        || !matches!(
            authority_kind.as_str(),
            "legacy_active" | "maintenance_qualified" | "policy_raw"
        )
    {
        return None;
    }
    let receipt_bytes = base64_member(values, field(node, "receiptBase64")?)?;
    if !member_digest_matches(values, &receipt_bytes, &receipt_member_sha256) {
        return None;
    }
    let receipt = values.decode_shadow_json(&receipt_bytes)?;
    // Node's JSON member loader admits only document objects
    if !matches!(receipt, JsonValue::Object(_)) {
        return None;
    }
    // receipts alone require exact recursively sorted canonical bytes with one LF
    if values.encode_shadow_sorted_json(&receipt)? != receipt_bytes {
        return None;
    }

    // policy-raw authorities contain no inferred artifact material
    if authority_kind == "policy_raw" {
        if !matches!(field(node, "artifactBase64"), Some(JsonValue::Null))
            || !matches!(field(node, "artifactIdentitySha256"), Some(JsonValue::Null))
            || !matches!(field(node, "artifactMemberSha256"), Some(JsonValue::Null))
        {
            return None;
        }
        return validate_raw_receipt(&receipt, family);
    }

    let artifact_identity_sha256 = text(node, "artifactIdentitySha256")?;
    let artifact_member_sha256 = text(node, "artifactMemberSha256")?;
    if !valid_hash(&artifact_identity_sha256) || !valid_hash(&artifact_member_sha256) {
        return None;
    }
    let artifact_bytes = base64_member(values, field(node, "artifactBase64")?)?;
    if !member_digest_matches(values, &artifact_bytes, &artifact_member_sha256) {
        return None;
    }
    // exact retained rain bytes already prove the JSON member without allocating its model graph
    if authority_kind == "legacy_active" && family == ShadowFamily::Rain {
        return validate_legacy_rain(&artifact_bytes, &receipt, &artifact_identity_sha256);
    }
    let artifact = values.decode_shadow_json(&artifact_bytes)?;
    // artifact model verification always starts from an admitted document object
    if !matches!(artifact, JsonValue::Object(_)) {
        return None;
    }
    match authority_kind.as_str() {
        "maintenance_qualified" => validate_qualified_authority(
            values,
            &artifact,
            &artifact_bytes,
            &receipt,
            &artifact_identity_sha256,
            &artifact_member_sha256,
            family,
        ),
        "legacy_active" => validate_legacy_authority(
            values,
            &artifact,
            &receipt_bytes,
            &artifact_identity_sha256,
            family,
        ),
        _ => None,
    }
}

/// Validate one root-qualified package and exact installed receipt.
fn validate_qualified_authority(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    artifact: &JsonValue,
    artifact_bytes: &[u8],
    receipt: &JsonValue,
    artifact_identity_sha256: &str,
    artifact_member_sha256: &str,
    family: ShadowFamily,
) -> Option<()> {
    ordered_keys(receipt, QUALIFIED_RECEIPT_KEYS)?;
    // retain the exact promoted qualified receipt identity fields
    if text(receipt, "contractVersion")? != "adjustment-installed-candidate-receipt/v2"
        || text(receipt, "actionKind")? != "promote"
        || text(receipt, "policyDecision")? != "qualified"
        || text(receipt, "bundleSha256")? != artifact_identity_sha256
    {
        return None;
    }
    for key in [
        "fullMemberRootSha256",
        "actionSha256",
        "registrationSha256",
        "candidateGraphSha256",
        "candidateSha256",
        "sourceSha256",
        "policyReportSha256",
        "lifecycleHeadSha256",
    ] {
        // preserve Node String(singleton-array) coercion only for these receipt guards
        if !qualified_receipt_hash(receipt, key) {
            return None;
        }
    }
    match family {
        ShadowFamily::Temperature | ShadowFamily::Wind => {
            let family = match family {
                ShadowFamily::Temperature => AdjustmentShadowFamily::Temperature,
                _ => AdjustmentShadowFamily::Wind,
            };
            verify_comparator_maintenance_package(
                values,
                artifact,
                artifact_identity_sha256,
                family,
            )
            .then_some(())
        }
        ShadowFamily::Rain => {
            // the portable rain identity is the exact raw SHA and its JSON cannot start with a BOM
            if artifact_identity_sha256 != artifact_member_sha256
                || artifact_bytes.starts_with(&[0xef, 0xbb, 0xbf])
            {
                return None;
            }
            admit_rain_portable_artifact(
                artifact,
                |doc| values.decode_shadow_rain_artifact(doc),
                |feature_names| values.sha256(&values.encode_shadow_ordered_json(feature_names)?),
            )?;
            Some(())
        }
    }
}

/// Preserve Node's recursive singleton-array string coercion for qualified receipt hash guards.
fn qualified_receipt_hash(receipt: &JsonValue, key: &str) -> bool {
    let Some(mut value) = field(receipt, key) else {
        return false;
    };
    // unwrap only the array shape whose JavaScript string form equals its sole member
    loop {
        match value {
            JsonValue::String(units) => {
                return String::from_utf16(units).is_ok_and(|value| valid_hash(&value));
            }
            JsonValue::Array(items) if items.len() == 1 => value = &items[0],
            _ => return false,
        }
    }
}

/// Validate an actual legacy artifact and its nested or registry receipt.
fn validate_legacy_authority(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    artifact: &JsonValue,
    receipt_bytes: &[u8],
    artifact_identity_sha256: &str,
    family: ShadowFamily,
) -> Option<()> {
    match family {
        ShadowFamily::Temperature => {
            verify_legacy_temperature_bundle(values, artifact, artifact_identity_sha256)?;
            validate_legacy_nested(values, artifact, receipt_bytes, "authorization")
        }
        ShadowFamily::Wind => {
            // the general predecessor bundle requires its separate qualification graph
            if text(artifact, "contractVersion")? == "forecast-adjustment-runtime-bundle/v2" {
                verify_legacy_wind_runtime_bundle(values, artifact, artifact_identity_sha256)?;
                return validate_legacy_nested(
                    values,
                    artifact,
                    receipt_bytes,
                    "qualificationReceipt",
                );
            }
            verify_legacy_wind_canary_bundle(values, artifact, artifact_identity_sha256)?;
            validate_legacy_nested(values, artifact, receipt_bytes, "authorization")
        }
        // the original compiled rain generation is admitted directly from raw bytes
        ShadowFamily::Rain => None,
    }
}

/// Compare the already verified legacy artifact's nested receipt canonical bytes exactly.
fn validate_legacy_nested(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    artifact: &JsonValue,
    receipt_bytes: &[u8],
    receipt_key: &str,
) -> Option<()> {
    let nested_receipt = field(artifact, receipt_key)?;
    // legacy nested receipts retain the same object-only parse boundary
    if !matches!(nested_receipt, JsonValue::Object(_)) {
        return None;
    }
    (values.encode_shadow_sorted_json(nested_receipt)? == receipt_bytes).then_some(())
}

/// Validate the one exact compiled legacy rain artifact and active registry receipt.
fn validate_legacy_rain(
    artifact_bytes: &[u8],
    receipt: &JsonValue,
    artifact_identity_sha256: &str,
) -> Option<()> {
    if artifact_identity_sha256
        != LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveArtifactSha256Literal::WIRE_VALUE
        || artifact_bytes != RETAINED_RAIN_ARTIFACT
    {
        return None;
    }
    ordered_keys(receipt, RAIN_RAW_REGISTRY_KEYS)?;
    let active_artifact = field(receipt, "activeArtifact")?;
    ordered_keys(active_artifact, RAIN_ACTIVE_ARTIFACT_KEYS)?;
    (text(receipt, "contractVersion")? == "forecast-adjustment-rain-runtime-registry/v1"
        && matches!(field(receipt, "rawReason"), Some(JsonValue::Null))
        && text(active_artifact, "artifactSha256")? == artifact_identity_sha256)
        .then_some(())
}

/// Validate one explicit reviewed raw registry instead of an environment toggle.
fn validate_raw_receipt(receipt: &JsonValue, family: ShadowFamily) -> Option<()> {
    let contract_version = text(receipt, "contractVersion")?;
    // preserve the two legacy raw registry shapes before maintenance registries
    if family == ShadowFamily::Temperature
        && contract_version == "forecast-adjustment-temperature-canary-registry/v2"
    {
        ordered_keys(receipt, TEMP_RAW_REGISTRY_KEYS)?;
        return raw_registry_fields(receipt, "activeBundle");
    }
    if family == ShadowFamily::Wind
        && contract_version == "forecast-adjustment-wind-canary-registry/v2"
    {
        ordered_keys(receipt, WIND_RAW_REGISTRY_KEYS)?;
        let JsonValue::Array(enabled) = field(receipt, "enabledMetricBands")? else {
            return None;
        };
        if enabled.len() != 13 {
            return None;
        }
        return raw_registry_fields(receipt, "activeBundle");
    }
    if contract_version == maintenance_registry_version(family) {
        ordered_keys(receipt, MAINTENANCE_RAW_REGISTRY_KEYS)?;
        return raw_registry_fields(receipt, "activePackage");
    }
    if family == ShadowFamily::Rain
        && contract_version == "forecast-adjustment-rain-runtime-registry/v1"
    {
        ordered_keys(receipt, RAIN_RAW_REGISTRY_KEYS)?;
        return raw_registry_fields(receipt, "activeArtifact");
    }
    None
}

/// Check the shared inactive-member, policy reason, and site identity.
fn raw_registry_fields(receipt: &JsonValue, inactive_key: &str) -> Option<()> {
    (matches!(field(receipt, inactive_key), Some(JsonValue::Null))
        && text(receipt, "rawReason")? == "policy_raw"
        && text(receipt, "siteKey")? == "ballydidean")
        .then_some(())
}

/// Decode one bounded canonical padded base64 member.
fn base64_member(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    value: &JsonValue,
) -> Option<Vec<u8>> {
    let JsonValue::String(encoded) = value else {
        return None;
    };
    if !(MINIMUM_BASE64_UNITS..=MAXIMUM_BASE64_UNITS).contains(&encoded.len()) {
        return None;
    }
    values.decode_shadow_comparator_base64(encoded)
}

/// Compare one exact member digest with the approved SHA-256 owner.
fn member_digest_matches(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    bytes: &[u8],
    expected: &str,
) -> bool {
    values
        .sha256(bytes)
        .is_some_and(|digest| digest.as_str() == expected)
}

/// Render the original maintenance raw registry contract.
const fn maintenance_registry_version(family: ShadowFamily) -> &'static str {
    match family {
        ShadowFamily::Temperature => "forecast-adjustment-temperature-maintenance-registry/v1",
        ShadowFamily::Wind => "forecast-adjustment-wind-maintenance-registry/v1",
        ShadowFamily::Rain => "forecast-adjustment-rain-maintenance-registry/v1",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build one ordered JSON object at the decoded receipt boundary.
    fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
        JsonValue::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key.encode_utf16().collect(), value))
                .collect(),
        )
    }

    /// Build one exact JavaScript text value.
    fn string(value: &str) -> JsonValue {
        JsonValue::String(value.encode_utf16().collect())
    }

    /// Compare the original compiled incumbent bytes rather than reserializing the model.
    #[test]
    fn binds_the_legacy_rain_registry_to_the_exact_generated_artifact() {
        let identity =
            LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveArtifactSha256Literal::WIRE_VALUE;
        let receipt = object(vec![
            (
                "activeArtifact",
                object(vec![("artifactSha256", string(identity))]),
            ),
            (
                "contractVersion",
                string("forecast-adjustment-rain-runtime-registry/v1"),
            ),
            ("rawReason", JsonValue::Null),
            ("siteKey", string("ballydidean")),
        ]);
        assert!(validate_legacy_rain(RETAINED_RAIN_ARTIFACT, &receipt, identity).is_some());
        assert!(
            validate_legacy_rain(
                &RETAINED_RAIN_ARTIFACT[..RETAINED_RAIN_ARTIFACT.len() - 1],
                &receipt,
                identity,
            )
            .is_none()
        );
    }
}
