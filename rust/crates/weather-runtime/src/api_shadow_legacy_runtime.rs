//! Native admission of the retained general wind runtime's qualification graph.

use weather_contracts::{
    ForecastAdjustmentMetric, ForecastAdjustmentRuntimeBundleV2ContractVersionLiteral,
    ForecastLeadBandKey, JsonValue,
};

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_shadow_legacy_runtime_candidate::verify_legacy_runtime_candidate;
use crate::api_shadow_legacy_runtime_receipt::verify_legacy_runtime_receipt;
use crate::api_shadow_legacy_runtime_report::verify_legacy_runtime_report;
use crate::api_shadow_legacy_values::{
    canonical_clock, canonical_fields_equal, canonical_object_hash_matches, hash_field,
    js_trim_unit, text_equals,
};
use crate::maintenance_canonical_json::{exact_keys, field, text};

const BUNDLE_KEYS: &[&str] = &[
    "bundleSha256",
    "candidate",
    "contractVersion",
    "evaluationReport",
    "qualificationReceipt",
    "siteKey",
    "timezone",
];
const PAIR_KEYS: &[&str] = &["leadBand", "metric"];
const PROVENANCE_KEYS: &[&str] = &[
    "aggregationContractSha256",
    "coordinateManifestSha256",
    "metricEligibilitySha256",
    "observationSourceLineageSha256",
    "observationStationManifestSha256",
    "spatialWeightSha256",
];
const FORBIDDEN_NORMALIZED_KEYS: &[&str] = &[
    "apikey",
    "credential",
    "credentials",
    "decryptionkey",
    "deviceid",
    "devicemac",
    "encryptedmember",
    "encryptedmembers",
    "encryptionkey",
    "eventlosses",
    "evidencepath",
    "lanaddress",
    "macaddress",
    "memberpath",
    "outputpath",
    "password",
    "pereventlosses",
    "privatekey",
    "rawrow",
    "rawrows",
    "secret",
    "secrets",
    "snapshotpath",
];

/// Bind one closed general runtime bundle to its exact original qualification graph.
pub(crate) fn verify_legacy_wind_runtime_bundle(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    bundle: &JsonValue,
    identity: &str,
) -> Option<()> {
    check_retained_runtime_content(values, bundle)?;
    exact_keys(bundle, BUNDLE_KEYS).then_some(())?;
    // preserve the exact v2 identity without adding registry-only linkage rules
    if !text_equals(
        bundle,
        "contractVersion",
        ForecastAdjustmentRuntimeBundleV2ContractVersionLiteral::WIRE_VALUE,
    ) || hash_field(bundle, "bundleSha256")? != identity
    {
        return None;
    }
    canonical_object_hash_matches(values, bundle, "bundleSha256", identity)?;
    let candidate = field(bundle, "candidate")?;
    let report = field(bundle, "evaluationReport")?;
    let receipt = field(bundle, "qualificationReceipt")?;
    verify_legacy_runtime_candidate(values, candidate)?;
    verify_legacy_runtime_report(values, report)?;
    verify_legacy_runtime_receipt(values, receipt)?;
    bind_evidence(values, candidate, report, receipt)
}

/// Retain the exact immutable links, epoch, and chronological boundary.
fn bind_evidence(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    candidate: &JsonValue,
    report: &JsonValue,
    receipt: &JsonValue,
) -> Option<()> {
    let candidate_sha256 = field(candidate, "candidateArtifactSha256")?;
    let epoch = field(candidate, "evaluationEpochId")?;
    // require the same candidate and evaluation epoch in both embedded receipts
    for document in [report, receipt] {
        if field(document, "candidateArtifactSha256")? != candidate_sha256
            || field(document, "evaluationEpochId")? != epoch
        {
            return None;
        }
        canonical_fields_equal(
            values,
            field(candidate, "enabledMetricBands")?,
            field(document, "enabledMetricBands")?,
        )?;
        canonical_fields_equal(
            values,
            field(candidate, "trainingProvenance")?,
            field(document, "trainingProvenance")?,
        )?;
    }
    // bind the qualification to the exact report and locked holdout identities
    for key in [
        "evaluationReportSha256",
        "preregistrationSha256",
        "holdoutAccessMarkerSha256",
    ] {
        if field(report, key)? != field(receipt, key)? {
            return None;
        }
    }
    (canonical_clock(values, candidate, "finalTrainingCutoff")?
        < canonical_clock(values, report, "holdoutStartInclusive")?)
    .then_some(())
}

/// Apply the original closed key and text protection on every retained JSON member.
fn check_retained_runtime_content(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    doc: &JsonValue,
) -> Option<()> {
    let mut stack = vec![doc];
    // walk the decoded tree without re-parsing or adding a sensitive-value grammar
    while let Some(value) = stack.pop() {
        match value {
            JsonValue::Array(children) => stack.extend(children),
            JsonValue::Object(fields) => {
                // keep the original normalized key guard before admitting its value
                for (key, child) in fields {
                    let normalized: String = key
                        .iter()
                        .filter_map(|unit| runtime_key_unit(*unit))
                        .collect();
                    if FORBIDDEN_NORMALIZED_KEYS.contains(&normalized.as_str()) {
                        return None;
                    }
                    stack.push(child);
                }
            }
            JsonValue::String(units) => values.check_legacy_wind_runtime_value(units)?,
            _ => {}
        }
    }
    Some(())
}

/// Normalize the ASCII set and the two ECMAScript Unicode case-fold aliases.
fn runtime_key_unit(unit: u16) -> Option<char> {
    match unit {
        0x30..=0x39 | 0x41..=0x5a | 0x61..=0x7a => {
            Some(char::from(unit as u8).to_ascii_lowercase())
        }
        0x017f => Some('\u{017f}'),
        0x212a => Some('k'),
        _ => None,
    }
}

/// Read one exact generated metric and lead-band pair.
pub(crate) fn runtime_band(doc: &JsonValue) -> Option<(String, String)> {
    exact_keys(doc, PAIR_KEYS).then_some(())?;
    let metric = text(doc, "metric")?;
    let lead = text(doc, "leadBand")?;
    (ForecastAdjustmentMetric::WIRE_VALUES
        .iter()
        .any(|(value, _)| *value == metric)
        && ForecastLeadBandKey::WIRE_VALUES
            .iter()
            .any(|(value, _)| *value == lead))
    .then_some((metric, lead))
}

/// Require the complete nonempty, unique and lexical enabled pair set.
pub(crate) fn validate_runtime_bands(doc: &JsonValue) -> Option<Vec<(String, String)>> {
    let JsonValue::Array(nodes) = doc else {
        return None;
    };
    // retain the frozen upper bound of all supported metric/lead-band pairs
    if !(1..=35).contains(&nodes.len()) {
        return None;
    }
    let pairs = nodes.iter().map(runtime_band).collect::<Option<Vec<_>>>()?;
    pairs
        .windows(2)
        .all(|pair| pair[0] < pair[1])
        .then_some(pairs)
}

/// Check the original bounded JS UTF-16 text while preserving leading/trailing spaces.
pub(crate) fn validate_runtime_text(node: &JsonValue, key: &str) -> Option<()> {
    let JsonValue::String(units) = field(node, key)? else {
        return None;
    };
    (units.len() <= 128 && units.iter().any(|unit| !js_trim_unit(*unit))).then_some(())
}

/// Require every retained training provenance digest under its exact key.
pub(crate) fn validate_runtime_provenance(doc: &JsonValue) -> Option<()> {
    exact_keys(doc, PROVENANCE_KEYS).then_some(())?;
    // verify the original closed provenance identity set
    for key in PROVENANCE_KEYS {
        hash_field(doc, key)?;
    }
    Some(())
}
