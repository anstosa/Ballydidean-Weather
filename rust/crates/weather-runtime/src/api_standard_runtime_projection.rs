//! Bind the generated standard runtime projection to its verified canonical source graph.

use weather_contracts::{
    ForecastAdjustmentCandidateV2, ForecastAdjustmentCoefficient,
    ForecastAdjustmentCoefficientDaypartEnum, ForecastAdjustmentCoefficientLevelEnum,
    ForecastAdjustmentCoefficientSeasonEnum, ForecastAdjustmentForecastIdentity,
    ForecastAdjustmentMetric, ForecastAdjustmentMetricBand, ForecastAdjustmentMetricPolicy,
    ForecastAdjustmentRuntimeBundleV2, ForecastAdjustmentTrainingEnvelope,
    ForecastAdjustmentTrainingEnvelopeMetricEnum, ForecastAdjustmentTrainingProvenance,
    ForecastLeadBandKey, JsonValue, Nullable,
};

use crate::api_shadow_legacy_values::{boolean_field, integer_field, number_field};
use crate::maintenance_canonical_json::field;

/// Prove the serving projection and retained evidence identities came from the verified document.
pub(crate) fn standard_runtime_projection_matches(
    bundle: &ForecastAdjustmentRuntimeBundleV2,
    document: &JsonValue,
) -> bool {
    projection_matches(bundle, document).is_some()
}

/// Compare the complete serving candidate plus the evidence identities used during admission.
fn projection_matches(
    bundle: &ForecastAdjustmentRuntimeBundleV2,
    document: &JsonValue,
) -> Option<()> {
    candidate_matches(&bundle.candidate, field(document, "candidate")?)?;
    report_identity_matches(
        &bundle.evaluationReport,
        field(document, "evaluationReport")?,
    )?;
    receipt_identity_matches(
        &bundle.qualificationReceipt,
        field(document, "qualificationReceipt")?,
    )
}

/// Compare every mutable candidate scalar and every ordered serving collection.
pub(crate) fn candidate_matches(
    candidate: &ForecastAdjustmentCandidateV2,
    node: &JsonValue,
) -> Option<()> {
    // Bind all candidate-level addresses and training boundary identities.
    for (key, expected) in [
        (
            "candidateArtifactSha256",
            candidate.candidateArtifactSha256.as_str(),
        ),
        (
            "coefficientPayloadSha256",
            candidate.coefficientPayloadSha256.as_str(),
        ),
        (
            "developmentReportSha256",
            candidate.developmentReportSha256.as_str(),
        ),
        ("evaluationEpochId", candidate.evaluationEpochId.as_str()),
        (
            "exportManifestSha256",
            candidate.exportManifestSha256.as_str(),
        ),
        (
            "finalTrainingCutoff",
            candidate.finalTrainingCutoff.as_str(),
        ),
    ] {
        text_matches(node, key, expected).then_some(())?;
    }
    ordered_matches(
        node,
        "coefficients",
        &candidate.coefficients,
        coefficient_matches,
    )?;
    ordered_matches(
        node,
        "enabledMetricBands",
        &candidate.enabledMetricBands,
        metric_band_matches,
    )?;
    forecast_identity_matches(
        &candidate.forecastIdentity,
        field(node, "forecastIdentity")?,
    )?;
    ordered_matches(
        node,
        "metricPolicies",
        &candidate.metricPolicies,
        metric_policy_matches,
    )?;
    let fingerprint = field(node, "runtimeFingerprint")?;
    text_matches(
        fingerprint,
        "icuVersion",
        &candidate.runtimeFingerprint.icuVersion,
    )
    .then_some(())?;
    text_matches(
        fingerprint,
        "tzdataVersion",
        &candidate.runtimeFingerprint.tzdataVersion,
    )
    .then_some(())?;
    ordered_matches(
        node,
        "trainingEnvelopes",
        &candidate.trainingEnvelopes,
        training_envelope_matches,
    )?;
    training_provenance_matches(
        &candidate.trainingProvenance,
        field(node, "trainingProvenance")?,
    )
}

/// Compare one fitted coefficient without allocating a second candidate graph.
pub(crate) fn coefficient_matches(
    coefficient: &ForecastAdjustmentCoefficient,
    node: &JsonValue,
) -> Option<()> {
    number_matches(node, "coefficient", coefficient.coefficient).then_some(())?;
    nullable_wire_text_matches(
        node,
        "daypart",
        &coefficient.daypart,
        ForecastAdjustmentCoefficientDaypartEnum::WIRE_VALUES,
    )?;
    number_matches(node, "effectiveEventCount", coefficient.effectiveEventCount).then_some(())?;
    wire_text_matches(
        node,
        "leadBand",
        &coefficient.leadBand,
        ForecastLeadBandKey::WIRE_VALUES,
    )
    .then_some(())?;
    wire_number_matches(
        node,
        "level",
        &coefficient.level,
        ForecastAdjustmentCoefficientLevelEnum::WIRE_VALUES,
    )
    .then_some(())?;
    wire_text_matches(
        node,
        "metric",
        &coefficient.metric,
        ForecastAdjustmentMetric::WIRE_VALUES,
    )
    .then_some(())?;
    nullable_unsigned_matches(node, "month", &coefficient.month)?;
    nullable_wire_text_matches(
        node,
        "season",
        &coefficient.season,
        ForecastAdjustmentCoefficientSeasonEnum::WIRE_VALUES,
    )
}

/// Compare one enabled metric-band pair in its original array position.
pub(crate) fn metric_band_matches(
    band: &ForecastAdjustmentMetricBand,
    node: &JsonValue,
) -> Option<()> {
    wire_text_matches(
        node,
        "leadBand",
        &band.leadBand,
        ForecastLeadBandKey::WIRE_VALUES,
    )
    .then_some(())?;
    wire_text_matches(
        node,
        "metric",
        &band.metric,
        ForecastAdjustmentMetric::WIRE_VALUES,
    )
    .then_some(())
}

/// Compare every mutable member of the forecast identity used by serving.
pub(crate) fn forecast_identity_matches(
    identity: &ForecastAdjustmentForecastIdentity,
    node: &JsonValue,
) -> Option<()> {
    // The cohort and reference-kind literals are already fixed by the generated Rust types.
    for (key, expected) in [
        ("adapterVersion", identity.adapterVersion.as_str()),
        ("contractEpoch", identity.contractEpoch.as_str()),
        ("dataset", identity.dataset.as_str()),
        (
            "sourceConfigFingerprint",
            identity.sourceConfigFingerprint.as_str(),
        ),
        ("sourceKey", identity.sourceKey.as_str()),
        ("upstreamModel", identity.upstreamModel.as_str()),
    ] {
        text_matches(node, key, expected).then_some(())?;
    }
    Some(())
}

/// Compare one ordered training envelope and its exact floating-point endpoints.
pub(crate) fn training_envelope_matches(
    envelope: &ForecastAdjustmentTrainingEnvelope,
    node: &JsonValue,
) -> Option<()> {
    wire_text_matches(
        node,
        "leadBand",
        &envelope.leadBand,
        ForecastLeadBandKey::WIRE_VALUES,
    )
    .then_some(())?;
    number_matches(node, "maximum", envelope.maximum).then_some(())?;
    wire_text_matches(
        node,
        "metric",
        &envelope.metric,
        ForecastAdjustmentTrainingEnvelopeMetricEnum::WIRE_VALUES,
    )
    .then_some(())?;
    number_matches(node, "minimum", envelope.minimum).then_some(())
}

/// Compare one ordered metric policy used to bound the fitted correction.
fn metric_policy_matches(policy: &ForecastAdjustmentMetricPolicy, node: &JsonValue) -> Option<()> {
    number_matches(node, "correctionMaximum", policy.correctionMaximum).then_some(())?;
    number_matches(node, "correctionMinimum", policy.correctionMinimum).then_some(())?;
    number_matches(node, "finalMaximum", policy.finalMaximum).then_some(())?;
    (boolean_field(node, "finalMaximumExclusive")? == policy.finalMaximumExclusive).then_some(())?;
    number_matches(node, "finalMinimum", policy.finalMinimum).then_some(())?;
    wire_text_matches(
        node,
        "metric",
        &policy.metric,
        ForecastAdjustmentMetric::WIRE_VALUES,
    )
    .then_some(())?;
    (boolean_field(node, "wrapsFinalValue")? == policy.wrapsFinalValue).then_some(())
}

/// Compare the six immutable training lineage addresses.
pub(crate) fn training_provenance_matches(
    provenance: &ForecastAdjustmentTrainingProvenance,
    node: &JsonValue,
) -> Option<()> {
    // Keep every provenance address tied to the verified source object.
    for (key, expected) in [
        (
            "aggregationContractSha256",
            provenance.aggregationContractSha256.as_str(),
        ),
        (
            "coordinateManifestSha256",
            provenance.coordinateManifestSha256.as_str(),
        ),
        (
            "metricEligibilitySha256",
            provenance.metricEligibilitySha256.as_str(),
        ),
        (
            "observationSourceLineageSha256",
            provenance.observationSourceLineageSha256.as_str(),
        ),
        (
            "observationStationManifestSha256",
            provenance.observationStationManifestSha256.as_str(),
        ),
        (
            "spatialWeightSha256",
            provenance.spatialWeightSha256.as_str(),
        ),
    ] {
        text_matches(node, key, expected).then_some(())?;
    }
    Some(())
}

/// Compare the evaluation identities that bind the candidate to its qualification evidence.
fn report_identity_matches(
    report: &weather_contracts::ForecastAdjustmentEvaluationReportV2,
    node: &JsonValue,
) -> Option<()> {
    // Bind every report scalar consumed by the admission graph.
    for (key, expected) in [
        (
            "candidateArtifactSha256",
            report.candidateArtifactSha256.as_str(),
        ),
        ("evaluationEpochId", report.evaluationEpochId.as_str()),
        (
            "evaluationReportSha256",
            report.evaluationReportSha256.as_str(),
        ),
        (
            "holdoutAccessMarkerSha256",
            report.holdoutAccessMarkerSha256.as_str(),
        ),
        (
            "preregistrationSha256",
            report.preregistrationSha256.as_str(),
        ),
    ] {
        text_matches(node, key, expected).then_some(())?;
    }
    ordered_matches(
        node,
        "enabledMetricBands",
        &report.enabledMetricBands,
        metric_band_matches,
    )?;
    training_provenance_matches(
        &report.trainingProvenance,
        field(node, "trainingProvenance")?,
    )
}

/// Compare the receipt identities that bind qualification to the admitted report.
fn receipt_identity_matches(
    receipt: &weather_contracts::ForecastAdjustmentQualificationReceiptV2,
    node: &JsonValue,
) -> Option<()> {
    // Bind every receipt scalar consumed by the admission graph and registry selection.
    for (key, expected) in [
        (
            "candidateArtifactSha256",
            receipt.candidateArtifactSha256.as_str(),
        ),
        ("evaluationEpochId", receipt.evaluationEpochId.as_str()),
        (
            "evaluationReportSha256",
            receipt.evaluationReportSha256.as_str(),
        ),
        (
            "holdoutAccessMarkerSha256",
            receipt.holdoutAccessMarkerSha256.as_str(),
        ),
        (
            "preregistrationSha256",
            receipt.preregistrationSha256.as_str(),
        ),
        (
            "qualificationReceiptSha256",
            receipt.qualificationReceiptSha256.as_str(),
        ),
    ] {
        text_matches(node, key, expected).then_some(())?;
    }
    ordered_matches(
        node,
        "enabledMetricBands",
        &receipt.enabledMetricBands,
        metric_band_matches,
    )?;
    training_provenance_matches(
        &receipt.trainingProvenance,
        field(node, "trainingProvenance")?,
    )
}

/// Compare one typed vector with the exact source array length and order.
pub(crate) fn ordered_matches<T>(
    node: &JsonValue,
    key: &str,
    projected: &[T],
    matches_item: fn(&T, &JsonValue) -> Option<()>,
) -> Option<()> {
    let JsonValue::Array(source) = field(node, key)? else {
        return None;
    };
    (source.len() == projected.len()).then_some(())?;
    projected
        .iter()
        .zip(source)
        .try_for_each(|(value, source)| matches_item(value, source))
}

/// Compare one exact UTF-16 JSON string without allocating a decoded copy.
pub(crate) fn text_matches(node: &JsonValue, key: &str, expected: &str) -> bool {
    let Some(JsonValue::String(actual)) = field(node, key) else {
        return false;
    };
    actual.iter().copied().eq(expected.encode_utf16())
}

/// Compare one finite JSON number with its exact projected binary64 value.
fn number_matches(node: &JsonValue, key: &str, expected: f64) -> bool {
    number_field(node, key).is_some_and(|actual| actual.to_bits() == expected.to_bits())
}

/// Compare one generated string enum through its generated wire table.
fn wire_text_matches<T: PartialEq>(
    node: &JsonValue,
    key: &str,
    expected: &T,
    values: &[(&str, T)],
) -> bool {
    values
        .iter()
        .find_map(|(wire, value)| (value == expected).then_some(*wire))
        .is_some_and(|wire| text_matches(node, key, wire))
}

/// Compare one generated unsigned-integer enum through its generated wire table.
fn wire_number_matches<T: PartialEq>(
    node: &JsonValue,
    key: &str,
    expected: &T,
    values: &[(u64, T)],
) -> bool {
    values
        .iter()
        .find_map(|(wire, value)| (value == expected).then_some(*wire))
        .and_then(|wire| i64::try_from(wire).ok())
        .is_some_and(|wire| integer_field(node, key) == Some(wire))
}

/// Compare one nullable generated string enum without collapsing null into absence.
fn nullable_wire_text_matches<T: PartialEq>(
    node: &JsonValue,
    key: &str,
    expected: &Nullable<T>,
    values: &[(&str, T)],
) -> Option<()> {
    // Preserve the source null/value distinction for hierarchy coordinates.
    match (field(node, key)?, expected) {
        (JsonValue::Null, Nullable::Null) => Some(()),
        (JsonValue::String(_), Nullable::Value(expected)) => {
            wire_text_matches(node, key, expected, values).then_some(())
        }
        _ => None,
    }
}

/// Compare one nullable safe unsigned integer without narrowing its projected value.
fn nullable_unsigned_matches(node: &JsonValue, key: &str, expected: &Nullable<u64>) -> Option<()> {
    // Preserve the source null/value distinction for the optional month coordinate.
    match (field(node, key)?, expected) {
        (JsonValue::Null, Nullable::Null) => Some(()),
        (JsonValue::Number(_), Nullable::Value(expected)) => i64::try_from(*expected)
            .ok()
            .is_some_and(|expected| integer_field(node, key) == Some(expected))
            .then_some(()),
        _ => None,
    }
}
