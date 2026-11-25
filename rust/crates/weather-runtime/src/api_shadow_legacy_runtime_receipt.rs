//! Native validation of one promotable legacy qualification receipt.

use weather_contracts::{
    ForecastAdjustmentEvidenceRedundancyStatusEnum, ForecastAdjustmentQualificationGateName,
    ForecastAdjustmentQualificationReceiptV2ContractVersionLiteral, JsonValue,
};

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_shadow_legacy_runtime::{
    runtime_band, validate_runtime_bands, validate_runtime_provenance, validate_runtime_text,
};
use crate::api_shadow_legacy_values::{
    boolean_field, canonical_object_hash_matches, hash_field, text_equals,
};
use crate::maintenance_canonical_json::{exact_keys, field, text};

const RECEIPT_KEYS: &[&str] = &[
    "candidateArtifactSha256",
    "contractVersion",
    "enabledMetricBands",
    "evaluationEpochId",
    "evaluationReportSha256",
    "evidenceRedundancy",
    "gates",
    "holdoutAccessMarkerSha256",
    "lifecycleState",
    "passed",
    "preregistrationSha256",
    "qualificationReceiptSha256",
    "trainingProvenance",
];
const REDUNDANCY_KEYS: &[&str] = &["attestationSha256", "status", "verified"];
const GATE_KEYS: &[&str] = &["metricBand", "name", "passed", "reasonCode"];

/// Verify one closed, passing qualification receipt admitted to the runtime bundle.
pub(crate) fn verify_legacy_runtime_receipt(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    doc: &JsonValue,
) -> Option<()> {
    exact_keys(doc, RECEIPT_KEYS).then_some(())?;
    // bind the generated receipt contract before interpreting its nested evidence
    text_equals(
        doc,
        "contractVersion",
        ForecastAdjustmentQualificationReceiptV2ContractVersionLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    hash_field(doc, "candidateArtifactSha256")?;
    hash_field(doc, "evaluationReportSha256")?;
    hash_field(doc, "holdoutAccessMarkerSha256")?;
    hash_field(doc, "preregistrationSha256")?;
    let receipt_sha256 = hash_field(doc, "qualificationReceiptSha256")?;
    validate_runtime_text(doc, "evaluationEpochId")?;
    validate_runtime_provenance(field(doc, "trainingProvenance")?)?;
    let bands = validate_runtime_bands(field(doc, "enabledMetricBands")?)?;
    validate_redundancy(field(doc, "evidenceRedundancy")?)?;
    validate_passing_gates(field(doc, "gates")?, &bands)?;

    // admit only the promotable receipt state rather than the broader rejected archive shape
    if !boolean_field(doc, "passed")? || !text_equals(doc, "lifecycleState", "qualified") {
        return None;
    }
    canonical_object_hash_matches(values, doc, "qualificationReceiptSha256", &receipt_sha256)
}

/// Require one verified durable evidence copy in a generated status.
fn validate_redundancy(doc: &JsonValue) -> Option<()> {
    exact_keys(doc, REDUNDANCY_KEYS).then_some(())?;
    hash_field(doc, "attestationSha256")?;
    let status = text(doc, "status")?;
    let supported = ForecastAdjustmentEvidenceRedundancyStatusEnum::WIRE_VALUES
        .iter()
        .any(|(value, _)| *value == status.as_str());
    // qualification cannot reach the runtime with an unverified redundancy claim
    (supported && boolean_field(doc, "verified")?).then_some(())
}

/// Require exact pair-major coverage by every passing generated gate.
fn validate_passing_gates(doc: &JsonValue, bands: &[(String, String)]) -> Option<()> {
    let JsonValue::Array(gates) = doc else {
        return None;
    };
    let gate_names = ForecastAdjustmentQualificationGateName::WIRE_VALUES;
    let required_count = bands.len().checked_mul(gate_names.len())?;
    // reject omissions, duplicates, and additions before indexing expected coverage
    if gates.len() != required_count {
        return None;
    }
    // preserve the original pair-major generated gate order
    for (index, gate) in gates.iter().enumerate() {
        exact_keys(gate, GATE_KEYS).then_some(())?;
        let pair_index = index / gate_names.len();
        let gate_index = index % gate_names.len();
        let expected_pair = bands.get(pair_index)?;
        let (expected_name, _) = gate_names.get(gate_index)?;
        // a promotable receipt has the exact pair, named pass, and no failure reason
        if &runtime_band(field(gate, "metricBand")?)? != expected_pair
            || text(gate, "name")?.as_str() != *expected_name
            || !boolean_field(gate, "passed")?
            || !matches!(field(gate, "reasonCode"), Some(JsonValue::Null))
        {
            return None;
        }
    }
    Some(())
}
