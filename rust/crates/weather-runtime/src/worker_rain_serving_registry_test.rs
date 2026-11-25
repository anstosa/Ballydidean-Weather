//! Pure startup decision coverage for the four closed rain registry branches.

use super::*;
use crate::{ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar};
use weather_contracts::{
    ForecastAdjustmentMaintenanceRuntimePackage, JsonValue, RainHurdleWindPortableArtifact,
};
use weather_database::Sha256Digest;

struct Values;

impl ApiMaintenanceReceiptCanonicalValues for Values {
    /// Registry classification performs no clock work.
    fn canonical_utc_epoch_milliseconds(&self, _: &str) -> Option<i64> {
        unreachable!("rain registry classification has no clock")
    }

    /// Return the frozen policy-raw registry identity only for its exact byte count.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        (bytes.len() == RAW_REGISTRY_BYTES).then_some(Sha256Digest::admitted(RAW_REGISTRY_SHA256))
    }

    /// Registry classification derives no database receipt identity.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!("rain registry classification has no database receipt")
    }
}

impl ApiMaintenanceShadowEvidenceValues for Values {
    /// Registry classification starts from an already parsed document.
    fn decode_shadow_json(&self, _: &[u8]) -> Option<JsonValue> {
        unreachable!("rain registry classification does not parse artifact bytes")
    }

    /// Registry classification does not encode canonical JSON.
    fn encode_shadow_ordered_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        unreachable!("rain registry classification does not encode JSON")
    }

    /// Registry classification has no comparator payload.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        unreachable!("rain registry classification has no comparator")
    }

    /// Registry classification does not sort canonical JSON.
    fn encode_shadow_sorted_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        unreachable!("rain registry classification does not sort JSON")
    }

    /// Registry classification does not decode model package unions.
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        unreachable!("rain registry classification has no maintenance package")
    }

    /// Registry classification does not decode the compiled portable model.
    fn decode_shadow_rain_artifact(&self, _: &JsonValue) -> Option<RainHurdleWindPortableArtifact> {
        unreachable!("rain registry classification has no portable artifact")
    }

    /// General wind compatibility is outside the rain registry.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        unreachable!("rain registry classification has no general wind value")
    }
}

/// Keep the compiled incumbent available only to admitted serving selections.
#[test]
fn binds_the_compiled_incumbent_for_active_and_policy_raw_serving() {
    // active and policy raw both retain the incumbent for feature projection
    for (selection, expected) in [
        (RainAdjustmentRuntimeSelection::Active, true),
        (
            RainAdjustmentRuntimeSelection::DisabledPolicyRawWithComparatorAuthority,
            true,
        ),
        (RainAdjustmentRuntimeSelection::Disabled, false),
        (RainAdjustmentRuntimeSelection::Unconfigured, false),
    ] {
        let loaded = LoadedWorkerRainServing {
            selection,
            authority: None,
        };
        assert_eq!(loaded.model().is_some(), expected, "{selection:?}");
    }
}

/// Recognize the original active registry only at the compiled artifact identity.
#[test]
fn classifies_exact_legacy_active_and_rejects_wrong_artifact_receipt() {
    let active = legacy_registry(
        object(vec![("artifactSha256", string(RETAINED_RAIN_MODEL_SHA256))]),
        JsonValue::Null,
    );
    assert_eq!(
        classify_worker_rain_registry(&Values, &active, b"active"),
        Some(WorkerRainRegistryChoice::LegacyActive)
    );

    let wrong = legacy_registry(
        object(vec![("artifactSha256", string(&"0".repeat(64)))]),
        JsonValue::Null,
    );
    assert_eq!(
        classify_worker_rain_registry(&Values, &wrong, b"active"),
        None
    );
}

/// Require both the frozen raw byte count and policy-raw digest.
#[test]
fn classifies_only_the_exact_pinned_legacy_raw_bytes() {
    let raw = legacy_registry(JsonValue::Null, string("policy_raw"));
    let bytes = vec![b'r'; RAW_REGISTRY_BYTES];
    assert_eq!(
        classify_worker_rain_registry(&Values, &raw, &bytes),
        Some(WorkerRainRegistryChoice::LegacyRaw)
    );
    assert_eq!(
        classify_worker_rain_registry(&Values, &raw, &bytes[..RAW_REGISTRY_BYTES - 1]),
        None
    );
}

/// Preserve maintenance policy-raw selection without granting package authority.
#[test]
fn classifies_closed_maintenance_raw_and_rejects_unknown_contract() {
    let maintenance = object(vec![
        ("activePackage", JsonValue::Null),
        ("contractVersion", string(MAINTENANCE_REGISTRY)),
        ("rawReason", string("policy_raw")),
        ("siteKey", string("ballydidean")),
    ]);
    assert_eq!(
        classify_worker_rain_registry(&Values, &maintenance, b"maintenance"),
        Some(WorkerRainRegistryChoice::MaintenanceRaw)
    );

    let unknown = object(vec![
        ("activeArtifact", JsonValue::Null),
        (
            "contractVersion",
            string("forecast-adjustment-rain-runtime-registry/v9"),
        ),
        ("rawReason", string("policy_raw")),
        ("siteKey", string("ballydidean")),
    ]);
    assert_eq!(
        classify_worker_rain_registry(&Values, &unknown, b"unknown"),
        None
    );
}

/// Build one exact legacy registry with source-order fields.
fn legacy_registry(active_artifact: JsonValue, raw_reason: JsonValue) -> JsonValue {
    object(vec![
        ("activeArtifact", active_artifact),
        ("contractVersion", string(LEGACY_REGISTRY)),
        ("rawReason", raw_reason),
        ("siteKey", string("ballydidean")),
    ])
}

/// Build one dependency-free ordered JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one dependency-free JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
