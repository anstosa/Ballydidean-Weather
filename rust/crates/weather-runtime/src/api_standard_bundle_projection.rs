//! project a verified standard runtime bundle into generated contracts.

use weather_contracts::{self as contracts, JsonValue, Nullable};

use crate::api_forecast_model_projection::{
    array, project_candidate_v2, project_metric_band, project_training_provenance, string_enum,
    unsigned_field,
};
use crate::api_shadow_legacy_values::{boolean_field, number_field};
use crate::maintenance_canonical_json::{field, text};

/// project the complete standard bundle after native verification.
pub(crate) fn project_verified_standard_bundle(
    document: &JsonValue,
) -> Option<contracts::ForecastAdjustmentRuntimeBundleV2> {
    Some(contracts::ForecastAdjustmentRuntimeBundleV2 {
        candidate: project_candidate_v2(field(document, "candidate")?)?,
        evaluationReport: project_evaluation_report(field(document, "evaluationReport")?)?,
        qualificationReceipt: project_qualification_receipt(field(
            document,
            "qualificationReceipt",
        )?)?,
        bundleSha256: text(document, "bundleSha256")?,
        contractVersion: contracts::ForecastAdjustmentRuntimeBundleV2ContractVersionLiteral,
        siteKey: contracts::ForecastAdjustmentRuntimeBundleV2SiteKeyLiteral,
        timezone: contracts::ForecastAdjustmentRuntimeBundleV2TimezoneLiteral,
    })
}

/// project the complete verified evaluation report.
fn project_evaluation_report(
    report: &JsonValue,
) -> Option<contracts::ForecastAdjustmentEvaluationReportV2> {
    Some(contracts::ForecastAdjustmentEvaluationReportV2 {
        candidateArtifactSha256: text(report, "candidateArtifactSha256")?,
        contractVersion: contracts::ForecastAdjustmentEvaluationReportV2ContractVersionLiteral,
        enabledMetricBands: array(report, "enabledMetricBands", project_metric_band)?,
        evaluationEpochId: text(report, "evaluationEpochId")?,
        evaluationReportSha256: text(report, "evaluationReportSha256")?,
        holdoutAccessMarkerSha256: text(report, "holdoutAccessMarkerSha256")?,
        holdoutEndExclusive: text(report, "holdoutEndExclusive")?,
        holdoutEndLocalDate: text(report, "holdoutEndLocalDate")?,
        holdoutStartInclusive: text(report, "holdoutStartInclusive")?,
        holdoutStartLocalDate: text(report, "holdoutStartLocalDate")?,
        metricBandEvaluations: array(
            report,
            "metricBandEvaluations",
            project_metric_band_evaluation,
        )?,
        preregistrationSha256: text(report, "preregistrationSha256")?,
        trainingProvenance: project_training_provenance(field(report, "trainingProvenance")?)?,
    })
}

/// project one verified metric-band evaluation and its ordered evidence.
fn project_metric_band_evaluation(
    evaluation: &JsonValue,
) -> Option<contracts::ForecastAdjustmentMetricBandEvaluation> {
    Some(contracts::ForecastAdjustmentMetricBandEvaluation {
        criticalSlices: array(evaluation, "criticalSlices", project_critical_slice)?,
        evaluatedSeasonDaypartKeys: array(
            evaluation,
            "evaluatedSeasonDaypartKeys",
            project_season_daypart_key,
        )?,
        metricBand: project_metric_band(field(evaluation, "metricBand")?)?,
        network: project_paired_score(field(evaluation, "network")?)?,
        providerBalanced: project_paired_score(field(evaluation, "providerBalanced")?)?,
        scoreableStationKeys: array(evaluation, "scoreableStationKeys", project_station_key)?,
    })
}

/// project one verified aggregate or slice score.
fn project_paired_score(score: &JsonValue) -> Option<contracts::ForecastAdjustmentPairedScore> {
    Some(contracts::ForecastAdjustmentPairedScore {
        adjustedLoss: number_field(score, "adjustedLoss")?,
        bootstrapLowerBound: number_field(score, "bootstrapLowerBound")?,
        bootstrapUpperBound: number_field(score, "bootstrapUpperBound")?,
        eventCount: unsigned_field(score, "eventCount")?,
        rawLoss: number_field(score, "rawLoss")?,
        skill: number_field(score, "skill")?,
    })
}

/// project one verified discriminated critical-slice score.
fn project_critical_slice(
    slice: &JsonValue,
) -> Option<contracts::ForecastAdjustmentCriticalSliceScore> {
    let score = project_paired_score(slice)?;
    // preserve the generated variant selected by the verified discriminator
    match text(slice, "kind")?.as_str() {
        "nearest_three" => Some(
            contracts::ForecastAdjustmentCriticalSliceScore::NearestThree(
                contracts::ForecastAdjustmentCriticalSliceScoreNearestThree {
                    adjustedLoss: score.adjustedLoss,
                    bootstrapLowerBound: score.bootstrapLowerBound,
                    bootstrapUpperBound: score.bootstrapUpperBound,
                    eventCount: score.eventCount,
                    rawLoss: score.rawLoss,
                    skill: score.skill,
                    key: contracts::ForecastAdjustmentCriticalSliceScoreNearestThreeKeyLiteral,
                    kind: contracts::ForecastAdjustmentCriticalSliceScoreNearestThreeKindLiteral,
                },
            ),
        ),
        "provider_family" => Some(
            contracts::ForecastAdjustmentCriticalSliceScore::ProviderFamily(
                contracts::ForecastAdjustmentCriticalSliceScoreProviderFamily {
                    adjustedLoss: score.adjustedLoss,
                    bootstrapLowerBound: score.bootstrapLowerBound,
                    bootstrapUpperBound: score.bootstrapUpperBound,
                    eventCount: score.eventCount,
                    rawLoss: score.rawLoss,
                    skill: score.skill,
                    key: string_enum(
                        slice,
                        "key",
                        contracts::ForecastObservationProviderFamily::WIRE_VALUES,
                    )?,
                    kind: contracts::ForecastAdjustmentCriticalSliceScoreProviderFamilyKindLiteral,
                },
            ),
        ),
        "season_daypart" => Some(
            contracts::ForecastAdjustmentCriticalSliceScore::SeasonDaypart(
                contracts::ForecastAdjustmentCriticalSliceScoreSeasonDaypart {
                    adjustedLoss: score.adjustedLoss,
                    bootstrapLowerBound: score.bootstrapLowerBound,
                    bootstrapUpperBound: score.bootstrapUpperBound,
                    eventCount: score.eventCount,
                    rawLoss: score.rawLoss,
                    skill: score.skill,
                    key: string_enum(
                        slice,
                        "key",
                        contracts::ForecastSeasonDaypartKey::WIRE_VALUES,
                    )?,
                    kind: contracts::ForecastAdjustmentCriticalSliceScoreSeasonDaypartKindLiteral,
                },
            ),
        ),
        "station" => Some(contracts::ForecastAdjustmentCriticalSliceScore::Station(
            contracts::ForecastAdjustmentCriticalSliceScoreStation {
                adjustedLoss: score.adjustedLoss,
                bootstrapLowerBound: score.bootstrapLowerBound,
                bootstrapUpperBound: score.bootstrapUpperBound,
                eventCount: score.eventCount,
                rawLoss: score.rawLoss,
                skill: score.skill,
                key: string_enum(
                    slice,
                    "key",
                    contracts::ForecastObservationStationKey::WIRE_VALUES,
                )?,
                kind: contracts::ForecastAdjustmentCriticalSliceScoreStationKindLiteral,
            },
        )),
        _ => None,
    }
}

/// project the complete verified qualification receipt.
fn project_qualification_receipt(
    receipt: &JsonValue,
) -> Option<contracts::ForecastAdjustmentQualificationReceiptV2> {
    Some(contracts::ForecastAdjustmentQualificationReceiptV2 {
        candidateArtifactSha256: text(receipt, "candidateArtifactSha256")?,
        contractVersion: contracts::ForecastAdjustmentQualificationReceiptV2ContractVersionLiteral,
        enabledMetricBands: array(receipt, "enabledMetricBands", project_metric_band)?,
        evaluationEpochId: text(receipt, "evaluationEpochId")?,
        evaluationReportSha256: text(receipt, "evaluationReportSha256")?,
        evidenceRedundancy: project_evidence_redundancy(field(receipt, "evidenceRedundancy")?)?,
        gates: array(receipt, "gates", project_qualification_gate)?,
        holdoutAccessMarkerSha256: text(receipt, "holdoutAccessMarkerSha256")?,
        lifecycleState: string_enum(
            receipt,
            "lifecycleState",
            contracts::ForecastAdjustmentQualificationReceiptV2LifecycleStateEnum::WIRE_VALUES,
        )?,
        passed: boolean_field(receipt, "passed")?,
        preregistrationSha256: text(receipt, "preregistrationSha256")?,
        qualificationReceiptSha256: text(receipt, "qualificationReceiptSha256")?,
        trainingProvenance: project_training_provenance(field(receipt, "trainingProvenance")?)?,
    })
}

/// project one verified evidence-redundancy claim.
fn project_evidence_redundancy(
    redundancy: &JsonValue,
) -> Option<contracts::ForecastAdjustmentEvidenceRedundancy> {
    Some(contracts::ForecastAdjustmentEvidenceRedundancy {
        attestationSha256: text(redundancy, "attestationSha256")?,
        status: string_enum(
            redundancy,
            "status",
            contracts::ForecastAdjustmentEvidenceRedundancyStatusEnum::WIRE_VALUES,
        )?,
        verified: boolean_field(redundancy, "verified")?,
    })
}

/// project one verified qualification gate.
fn project_qualification_gate(
    gate: &JsonValue,
) -> Option<contracts::ForecastAdjustmentQualificationGate> {
    Some(contracts::ForecastAdjustmentQualificationGate {
        metricBand: project_metric_band(field(gate, "metricBand")?)?,
        name: string_enum(
            gate,
            "name",
            contracts::ForecastAdjustmentQualificationGateName::WIRE_VALUES,
        )?,
        passed: boolean_field(gate, "passed")?,
        reasonCode: project_nullable_reason(field(gate, "reasonCode")?)?,
    })
}

/// project one verified nullable reason code.
fn project_nullable_reason(
    value: &JsonValue,
) -> Option<Nullable<contracts::ForecastAdjustmentReasonCode>> {
    // retain the source null instead of treating it as a missing field
    match value {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::String(units) => Some(Nullable::Value(scalar_string_enum(
            units,
            contracts::ForecastAdjustmentReasonCode::WIRE_VALUES,
        )?)),
        _ => None,
    }
}

/// project one verified season-daypart scalar.
fn project_season_daypart_key(value: &JsonValue) -> Option<contracts::ForecastSeasonDaypartKey> {
    let JsonValue::String(units) = value else {
        return None;
    };
    scalar_string_enum(units, contracts::ForecastSeasonDaypartKey::WIRE_VALUES)
}

/// project one verified station scalar.
fn project_station_key(value: &JsonValue) -> Option<contracts::ForecastObservationStationKey> {
    let JsonValue::String(units) = value else {
        return None;
    };
    scalar_string_enum(units, contracts::ForecastObservationStationKey::WIRE_VALUES)
}

/// decode one generated string enum from verified UTF-16 units.
fn scalar_string_enum<T: Copy>(units: &[u16], values: &[(&str, T)]) -> Option<T> {
    values.iter().find_map(|(wire, value)| {
        units
            .iter()
            .copied()
            .eq(wire.encode_utf16())
            .then_some(*value)
    })
}
