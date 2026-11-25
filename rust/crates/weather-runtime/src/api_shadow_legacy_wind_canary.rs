//! Verification of the retained legacy wind transfer canary generations.

use weather_contracts::{
    ForecastAdjustmentWindCanaryCandidateV1ContractVersionLiteral,
    ForecastAdjustmentWindCanaryRuntimeBundleV1ContractVersionLiteral,
    ForecastAdjustmentWindCanaryRuntimeBundleV2ContractVersionLiteral, JsonValue,
};

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_shadow_legacy_values::{
    canonical_clock, canonical_object_hash_matches, canonical_value_hash_matches, hash_field,
    text_equals, validate_text_field,
};
use crate::api_shadow_legacy_wind_evidence::verify_wind_canary_evidence;
use crate::api_shadow_legacy_wind_material::validate_wind_canary_candidate_material;
use crate::maintenance_canonical_json::{exact_keys, field, text};

const BUNDLE_KEYS: &[&str] = &[
    "artifactKind",
    "authorization",
    "bundleSha256",
    "candidate",
    "contractVersion",
    "siteKey",
    "timezone",
    "transferReport",
];
const CANDIDATE_KEYS: &[&str] = &[
    "algorithmContractVersion",
    "artifactKind",
    "candidateArtifactSha256",
    "coefficientPayloadSha256",
    "coefficients",
    "contractVersion",
    "enabledMetricBands",
    "exportManifestSha256",
    "finalTrainingCutoff",
    "runtimeFingerprint",
    "servedForecastIdentity",
    "siteKey",
    "timezone",
    "trainingEnvelopes",
    "trainingForecastIdentity",
    "trainingProvenance",
];
const FINGERPRINT_KEYS: &[&str] = &["icuVersion", "tzdataVersion"];
// canonical node hashes of the exact source-defined wind canary lineage
const TRAINING_IDENTITY: &str = "4cc285ce09bd743cc5d3b24c55722a9b0e68083f7b51c7009906b31d51b6cf15";
const SERVING_IDENTITY: &str = "8348b5c48d7fe614ced9442b04e8a68cfe704e3d541eab3b8d0ab33a41cd4079";
const TRAINING_PROVENANCE: &str =
    "cfa905468fb15fb10b3abf02a39db5823d8d3bb17c6dddc625749eb9fe3bb8c3";

/// Admit the fitted canary, bridge report, and matching original finite or permanent authority.
pub(crate) fn verify_legacy_wind_canary_bundle(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    doc: &JsonValue,
    identity: &str,
) -> Option<()> {
    exact_keys(doc, BUNDLE_KEYS).then_some(())?;
    // bind the selected digest and fixed transfer canary identity before verifying nested evidence
    if !text_equals(doc, "artifactKind", "wind_transfer_canary_runtime_bundle")
        || !text_equals(doc, "siteKey", "ballydidean")
        || !text_equals(doc, "timezone", "America/Los_Angeles")
        || hash_field(doc, "bundleSha256")? != identity
    {
        return None;
    }
    let permanent = match text(doc, "contractVersion")?.as_str() {
        ForecastAdjustmentWindCanaryRuntimeBundleV1ContractVersionLiteral::WIRE_VALUE => false,
        ForecastAdjustmentWindCanaryRuntimeBundleV2ContractVersionLiteral::WIRE_VALUE => true,
        _ => return None,
    };
    let candidate = field(doc, "candidate")?;
    verify_candidate(values, candidate)?;
    verify_wind_canary_evidence(
        values,
        field(doc, "authorization")?,
        field(doc, "transferReport")?,
        candidate,
        permanent,
    )?;
    canonical_object_hash_matches(values, doc, "bundleSha256", identity)
}

/// Verify candidate lineage, exact fitted addresses, and original numerical material.
fn verify_candidate(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    doc: &JsonValue,
) -> Option<()> {
    exact_keys(doc, CANDIDATE_KEYS).then_some(())?;
    // keep the isolated candidate generation and the retained fitting algorithm
    if !text_equals(doc, "artifactKind", "wind_transfer_canary_candidate")
        || !text_equals(
            doc,
            "contractVersion",
            ForecastAdjustmentWindCanaryCandidateV1ContractVersionLiteral::WIRE_VALUE,
        )
        || !text_equals(
            doc,
            "algorithmContractVersion",
            "robust-hierarchical-median/v1",
        )
        || !text_equals(doc, "siteKey", "ballydidean")
        || !text_equals(doc, "timezone", "America/Los_Angeles")
    {
        return None;
    }
    let own_hash = hash_field(doc, "candidateArtifactSha256")?;
    let coefficient_hash = hash_field(doc, "coefficientPayloadSha256")?;
    hash_field(doc, "exportManifestSha256")?;
    canonical_clock(values, doc, "finalTrainingCutoff")?;
    let fingerprint = field(doc, "runtimeFingerprint")?;
    exact_keys(fingerprint, FINGERPRINT_KEYS).then_some(())?;
    validate_text_field(fingerprint, "icuVersion")?;
    validate_text_field(fingerprint, "tzdataVersion")?;
    canonical_object_hash_matches(values, doc, "candidateArtifactSha256", &own_hash)?;
    canonical_value_hash_matches(values, field(doc, "coefficients")?, &coefficient_hash)?;
    // compare canonical source lineages independent of decoded member order
    for (key, expected) in [
        ("trainingForecastIdentity", TRAINING_IDENTITY),
        ("servedForecastIdentity", SERVING_IDENTITY),
        ("trainingProvenance", TRAINING_PROVENANCE),
    ] {
        canonical_value_hash_matches(values, field(doc, key)?, expected)?;
    }
    validate_wind_canary_candidate_material(doc)
}
