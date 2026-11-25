//! Project admitted forecast values into the generated TypeScript contract shapes.

use weather_contracts as c;
use weather_contracts::{JsonValue, Nullable};

use crate::api_shadow_legacy_values::{boolean_field, number_field, text_equals};
use crate::maintenance_canonical_json::{exact_keys, field, text};

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// project an ordered array without dropping or defaulting any member
pub(crate) fn array<T>(
    node: &JsonValue,
    key: &str,
    project: impl Fn(&JsonValue) -> Option<T>,
) -> Option<Vec<T>> {
    let JsonValue::Array(items) = field(node, key)? else {
        return None;
    };
    items.iter().map(project).collect()
}

/// match an exact generated text enum without allocating a string
pub(crate) fn string_enum<T: Copy>(node: &JsonValue, key: &str, table: &[(&str, T)]) -> Option<T> {
    let JsonValue::String(text) = field(node, key)? else {
        return None;
    };
    table.iter().find_map(|(wire, item)| {
        text.iter()
            .copied()
            .eq(wire.encode_utf16())
            .then_some(*item)
    })
}

/// read an exact safe unsigned integer from an admitted number
pub(crate) fn unsigned_field(node: &JsonValue, key: &str) -> Option<u64> {
    let value = number_field(node, key)?;
    // preserve the generated JavaScript safe-integer bound before narrowing
    ((0.0..=MAX_SAFE_INTEGER).contains(&value) && value.fract() == 0.0).then_some(value as u64)
}

/// match a generated safe unsigned integer enum
pub(crate) fn unsigned_enum<T: Copy>(node: &JsonValue, key: &str, table: &[(u64, T)]) -> Option<T> {
    let value = unsigned_field(node, key)?;
    table
        .iter()
        .find_map(|(wire, item)| (*wire == value).then_some(*item))
}

/// project a nullable field without treating a missing or malformed value as null
pub(crate) fn nullable<T>(
    node: &JsonValue,
    key: &str,
    project: impl FnOnce(&JsonValue, &str) -> Option<T>,
) -> Option<Nullable<T>> {
    // distinguish the explicit null from every present value
    match field(node, key)? {
        JsonValue::Null => Some(Nullable::Null),
        _ => project(node, key).map(Nullable::Value),
    }
}

/// project one exact generated metric-band pair
pub(crate) fn project_metric_band(node: &JsonValue) -> Option<c::ForecastAdjustmentMetricBand> {
    exact_keys(node, &["leadBand", "metric"]).then_some(())?;
    Some(c::ForecastAdjustmentMetricBand {
        leadBand: string_enum(node, "leadBand", c::ForecastLeadBandKey::WIRE_VALUES)?,
        metric: string_enum(node, "metric", c::ForecastAdjustmentMetric::WIRE_VALUES)?,
    })
}

/// retain every fitted numeric value and optional hierarchy coordinate
pub(crate) fn project_coefficient(node: &JsonValue) -> Option<c::ForecastAdjustmentCoefficient> {
    exact_keys(
        node,
        &[
            "coefficient",
            "daypart",
            "effectiveEventCount",
            "leadBand",
            "level",
            "metric",
            "month",
            "season",
        ],
    )
    .then_some(())?;
    Some(c::ForecastAdjustmentCoefficient {
        coefficient: number_field(node, "coefficient")?,
        daypart: nullable(node, "daypart", |node, key| {
            string_enum(
                node,
                key,
                c::ForecastAdjustmentCoefficientDaypartEnum::WIRE_VALUES,
            )
        })?,
        effectiveEventCount: number_field(node, "effectiveEventCount")?,
        leadBand: string_enum(node, "leadBand", c::ForecastLeadBandKey::WIRE_VALUES)?,
        level: unsigned_enum(
            node,
            "level",
            c::ForecastAdjustmentCoefficientLevelEnum::WIRE_VALUES,
        )?,
        metric: string_enum(node, "metric", c::ForecastAdjustmentMetric::WIRE_VALUES)?,
        month: nullable(node, "month", unsigned_field)?,
        season: nullable(node, "season", |node, key| {
            string_enum(
                node,
                key,
                c::ForecastAdjustmentCoefficientSeasonEnum::WIRE_VALUES,
            )
        })?,
    })
}

/// project one bounded scalar forecast envelope
pub(crate) fn project_envelope(node: &JsonValue) -> Option<c::ForecastAdjustmentTrainingEnvelope> {
    exact_keys(node, &["leadBand", "maximum", "metric", "minimum"]).then_some(())?;
    Some(c::ForecastAdjustmentTrainingEnvelope {
        leadBand: string_enum(node, "leadBand", c::ForecastLeadBandKey::WIRE_VALUES)?,
        maximum: number_field(node, "maximum")?,
        metric: string_enum(
            node,
            "metric",
            c::ForecastAdjustmentTrainingEnvelopeMetricEnum::WIRE_VALUES,
        )?,
        minimum: number_field(node, "minimum")?,
    })
}

/// project the exact served forecast identity from typed values
pub(crate) fn project_forecast_identity(
    node: &JsonValue,
) -> Option<c::ForecastAdjustmentForecastIdentity> {
    exact_keys(
        node,
        &[
            "adapterVersion",
            "cohort",
            "contractEpoch",
            "dataset",
            "referenceKind",
            "sourceConfigFingerprint",
            "sourceKey",
            "upstreamModel",
        ],
    )
    .then_some(())?;
    text_equals(
        node,
        "cohort",
        c::ForecastAdjustmentForecastIdentityCohortLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    text_equals(
        node,
        "referenceKind",
        c::ForecastAdjustmentForecastIdentityReferenceKindLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    Some(c::ForecastAdjustmentForecastIdentity {
        adapterVersion: text(node, "adapterVersion")?,
        cohort: c::ForecastAdjustmentForecastIdentityCohortLiteral,
        contractEpoch: text(node, "contractEpoch")?,
        dataset: text(node, "dataset")?,
        referenceKind: c::ForecastAdjustmentForecastIdentityReferenceKindLiteral,
        sourceConfigFingerprint: text(node, "sourceConfigFingerprint")?,
        sourceKey: text(node, "sourceKey")?,
        upstreamModel: text(node, "upstreamModel")?,
    })
}

/// project the admitted immutable training lineage
pub(crate) fn project_training_provenance(
    node: &JsonValue,
) -> Option<c::ForecastAdjustmentTrainingProvenance> {
    exact_keys(
        node,
        &[
            "aggregationContractSha256",
            "coordinateManifestSha256",
            "metricEligibilitySha256",
            "observationSourceLineageSha256",
            "observationStationManifestSha256",
            "spatialWeightSha256",
        ],
    )
    .then_some(())?;
    Some(c::ForecastAdjustmentTrainingProvenance {
        aggregationContractSha256: text(node, "aggregationContractSha256")?,
        coordinateManifestSha256: text(node, "coordinateManifestSha256")?,
        metricEligibilitySha256: text(node, "metricEligibilitySha256")?,
        observationSourceLineageSha256: text(node, "observationSourceLineageSha256")?,
        observationStationManifestSha256: text(node, "observationStationManifestSha256")?,
        spatialWeightSha256: text(node, "spatialWeightSha256")?,
    })
}

/// project one exact clamp and range policy
fn project_metric_policy(node: &JsonValue) -> Option<c::ForecastAdjustmentMetricPolicy> {
    exact_keys(
        node,
        &[
            "correctionMaximum",
            "correctionMinimum",
            "finalMaximum",
            "finalMaximumExclusive",
            "finalMinimum",
            "metric",
            "wrapsFinalValue",
        ],
    )
    .then_some(())?;
    Some(c::ForecastAdjustmentMetricPolicy {
        correctionMaximum: number_field(node, "correctionMaximum")?,
        correctionMinimum: number_field(node, "correctionMinimum")?,
        finalMaximum: number_field(node, "finalMaximum")?,
        finalMaximumExclusive: boolean_field(node, "finalMaximumExclusive")?,
        finalMinimum: number_field(node, "finalMinimum")?,
        metric: string_enum(node, "metric", c::ForecastAdjustmentMetric::WIRE_VALUES)?,
        wrapsFinalValue: boolean_field(node, "wrapsFinalValue")?,
    })
}

/// project the shared qualified or standard v2 forecast fit without an adapter hook
pub(crate) fn project_candidate_v2(node: &JsonValue) -> Option<c::ForecastAdjustmentCandidateV2> {
    exact_keys(
        node,
        &[
            "algorithmContractVersion",
            "candidateArtifactSha256",
            "coefficientPayloadSha256",
            "coefficients",
            "contractVersion",
            "developmentReportSha256",
            "enabledMetricBands",
            "evaluationEpochId",
            "exportManifestSha256",
            "finalTrainingCutoff",
            "forecastIdentity",
            "metricPolicies",
            "runtimeFingerprint",
            "siteKey",
            "timezone",
            "trainingEnvelopes",
            "trainingProvenance",
        ],
    )
    .then_some(())?;
    // admit fixed literals before constructing their zero-sized generated representations
    text_equals(
        node,
        "algorithmContractVersion",
        c::ForecastAdjustmentCandidateV2AlgorithmContractVersionLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    text_equals(
        node,
        "contractVersion",
        c::ForecastAdjustmentCandidateV2ContractVersionLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    text_equals(
        node,
        "siteKey",
        c::ForecastAdjustmentCandidateV2SiteKeyLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    text_equals(
        node,
        "timezone",
        c::ForecastAdjustmentCandidateV2TimezoneLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    let fingerprint = field(node, "runtimeFingerprint")?;
    exact_keys(fingerprint, &["icuVersion", "tzdataVersion"]).then_some(())?;
    Some(c::ForecastAdjustmentCandidateV2 {
        algorithmContractVersion: c::ForecastAdjustmentCandidateV2AlgorithmContractVersionLiteral,
        candidateArtifactSha256: text(node, "candidateArtifactSha256")?,
        coefficientPayloadSha256: text(node, "coefficientPayloadSha256")?,
        coefficients: array(node, "coefficients", project_coefficient)?,
        contractVersion: c::ForecastAdjustmentCandidateV2ContractVersionLiteral,
        developmentReportSha256: text(node, "developmentReportSha256")?,
        enabledMetricBands: array(node, "enabledMetricBands", project_metric_band)?,
        evaluationEpochId: text(node, "evaluationEpochId")?,
        exportManifestSha256: text(node, "exportManifestSha256")?,
        finalTrainingCutoff: text(node, "finalTrainingCutoff")?,
        forecastIdentity: project_forecast_identity(field(node, "forecastIdentity")?)?,
        metricPolicies: array(node, "metricPolicies", project_metric_policy)?,
        runtimeFingerprint: c::ForecastAdjustmentCandidateV2RuntimeFingerprint {
            icuVersion: text(fingerprint, "icuVersion")?,
            tzdataVersion: text(fingerprint, "tzdataVersion")?,
        },
        siteKey: c::ForecastAdjustmentCandidateV2SiteKeyLiteral,
        timezone: c::ForecastAdjustmentCandidateV2TimezoneLiteral,
        trainingEnvelopes: array(node, "trainingEnvelopes", project_envelope)?,
        trainingProvenance: project_training_provenance(field(node, "trainingProvenance")?)?,
    })
}
