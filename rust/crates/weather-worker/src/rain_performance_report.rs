//! Assemble the closed rain performance report from admitted native evidence.

use std::collections::{BTreeMap, BTreeSet};

use weather_adjustment::{
    ForecastAdjustmentRainDiagnosticCalendar, ForecastAdjustmentScorecardError,
    evaluate_forecast_adjustment_performance, evaluate_forecast_adjustment_rain_diagnostics,
    prepare_forecast_adjustment_performance_pairs, score_balanced_forecast_adjustment_pairs,
    unavailable_forecast_adjustment_rain_diagnostics,
};
use weather_contracts::{
    ForecastAdjustmentComparisonState, ForecastAdjustmentEvidenceClass,
    ForecastAdjustmentPerformanceEvaluation, ForecastAdjustmentPerformancePair,
    ForecastAdjustmentPreparedPairs, ForecastAdjustmentQualificationState,
    ForecastAdjustmentRainAccumulationSummary, ForecastAdjustmentRainAccumulationSummaryHoursEnum,
    ForecastAdjustmentRainDiagnostics, ForecastAdjustmentRainPerformancePair,
    ForecastAdjustmentRainThresholdSummary,
    ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum, ForecastAdjustmentReliabilityBin,
    ForecastAdjustmentServingState, ForecastAdjustmentSupportState, JsonValue, Nullable,
};

use crate::RainPerformanceEvidence;

const REPORT_VERSION: &str = "forecast-adjustment-performance-report/v1";
const REPORT_UNIT: &str = "millimeters_per_hour";
const SITE_KEY: &str = "ballydidean";

/// Verified package identities and bounds copied into the report document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RainPerformanceReportPackageInputs {
    pub adjustment_evidence_manifest_sha256: String,
    pub adjustment_evidence_watermark_sha256: String,
    pub forecast_training_manifest_sha256: String,
    pub local_date_from: String,
    pub local_date_to: String,
    pub target_cutoff_at: String,
}

/// Separately matched Best Match amount diagnostic admitted by the runtime owner.
#[derive(Debug, Clone, PartialEq)]
pub struct RainPerformanceBestMatchDiagnostic {
    pub best_match_raw_mae: f64,
    pub date_count: u64,
    pub row_count: u64,
    pub source_adjusted_mae: f64,
    pub source_raw_mae: f64,
}

/// One exact local-calendar partition label set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RainPerformanceCalendarLabels {
    pub daypart: RainPerformanceDaypart,
    pub month: u8,
    pub season: RainPerformanceSeason,
}

/// Closed local daypart labels used by the TypeScript report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RainPerformanceDaypart {
    Afternoon,
    Evening,
    Morning,
    Night,
}

/// Closed local meteorological-season labels used by the TypeScript report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RainPerformanceSeason {
    Autumn,
    Spring,
    Summer,
    Winter,
}

/// Calendar values needed beyond preparation and rain diagnostics.
pub trait RainPerformanceReportCalendar: ForecastAdjustmentRainDiagnosticCalendar {
    /// Resolve the report's pinned month, season, and daypart labels.
    fn los_angeles_report_labels(
        &mut self,
        valid_at: &str,
    ) -> Option<RainPerformanceCalendarLabels>;
}

/// Closed failures from report scoring and calendar partitioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RainPerformanceReportError {
    Calendar,
    RainDiagnostics,
    Scorecard,
}

/// Assemble one canonicalizable rain report without package or clock I/O.
pub fn create_rain_performance_report(
    evidence: &RainPerformanceEvidence,
    inputs: &RainPerformanceReportPackageInputs,
    best_match_diagnostic: Option<&RainPerformanceBestMatchDiagnostic>,
    generated_at: &str,
    source_revision: &str,
    calendar: &mut impl RainPerformanceReportCalendar,
) -> Result<JsonValue, RainPerformanceReportError> {
    let selected_amount_pairs = evidence.selected_amount_pairs();
    let selected_probability_pairs = evidence.selected_probability_pairs();
    let prepared = prepare_forecast_adjustment_performance_pairs(selected_amount_pairs, calendar)
        .map_err(scorecard_error)?;
    let wet_rows = prepared
        .rows
        .iter()
        .filter(|pair| nullable_number(&pair.target).unwrap_or(0.0) >= 0.1)
        .collect::<Vec<_>>();
    let wet_dates = wet_rows
        .iter()
        .map(|pair| pair.localDate.as_str())
        .collect::<BTreeSet<_>>();
    let evaluation = optional_evaluation(&prepared.rows)?;
    let rain_support_sufficient = evaluation
        .as_ref()
        .is_some_and(|value| value.supportState == ForecastAdjustmentSupportState::Sufficient)
        && wet_rows.len() >= 100
        && wet_dates.len() >= 20;
    let rain_diagnostics = rain_diagnostics(selected_probability_pairs, calendar)?;
    let slices = calendar_slices(&prepared.rows, calendar)?;
    let support = scorecard_support(
        &prepared,
        selected_amount_pairs.len(),
        wet_dates.len(),
        wet_rows.len(),
    );
    let development_prepared =
        prepare_forecast_adjustment_performance_pairs(&evidence.development_amount_pairs, calendar)
            .map_err(scorecard_error)?;
    let development_metrics = descriptive_metric(&development_prepared.rows, 1)?;
    let family = object(vec![
        (
            "bestMatchDiagnostic",
            best_match_diagnostic.map_or(JsonValue::Null, best_match_json),
        ),
        (
            "comparisonState",
            text(if rain_support_sufficient {
                evaluation
                    .as_ref()
                    .map_or("unscored", |value| comparison_wire(value.comparisonState))
            } else {
                "unscored"
            }),
        ),
        (
            "evidenceClass",
            text(evidence_class_wire(evidence.evidence_class)),
        ),
        ("evidenceCutoffAt", text(&inputs.target_cutoff_at)),
        ("family", text("rain")),
        (
            "metrics",
            if rain_support_sufficient {
                scorecard_metric(evaluation.as_ref())
            } else {
                unavailable_metric()
            },
        ),
        (
            "qualificationState",
            text(if rain_support_sufficient {
                evaluation.as_ref().map_or("pending_support", |value| {
                    qualification_wire(value.qualificationState)
                })
            } else if evidence.evidence_class == ForecastAdjustmentEvidenceClass::Development {
                "development_only"
            } else {
                "pending_support"
            }),
        ),
        ("rainDiagnostics", rain_diagnostics),
        (
            "recommendation",
            text(
                if evaluation.as_ref().is_some_and(|value| {
                    value.comparisonState == ForecastAdjustmentComparisonState::Worse
                }) {
                    "review_disable"
                } else {
                    "review_candidate"
                },
            ),
        ),
        (
            "servingIdentitySha256",
            nullable_text(&evidence.serving_identity_sha256),
        ),
        ("servingState", text("authorized_active")),
        ("slices", JsonValue::Array(slices)),
        ("support", support),
        (
            "supportState",
            text(if rain_support_sufficient {
                "sufficient"
            } else {
                "insufficient"
            }),
        ),
    ]);

    Ok(object(vec![
        ("contractVersion", text(REPORT_VERSION)),
        ("family", family),
        ("generatedAt", text(generated_at)),
        ("inputs", package_inputs_json(inputs)),
        (
            "developmentReference",
            object(vec![
                ("evidenceClass", text("development")),
                ("metrics", development_metrics),
                (
                    "rowCount",
                    number(evidence.development_amount_pairs.len() as f64),
                ),
            ]),
        ),
        ("siteKey", text(SITE_KEY)),
        ("sourceRevision", text(source_revision)),
    ]))
}

/// Map shared preparation errors into the report's closed failure surface.
fn scorecard_error(_: ForecastAdjustmentScorecardError) -> RainPerformanceReportError {
    RainPerformanceReportError::Scorecard
}

/// Evaluate a nonempty report population under the fixed rain support gates.
fn optional_evaluation(
    rows: &[ForecastAdjustmentPerformancePair],
) -> Result<Option<ForecastAdjustmentPerformanceEvaluation>, RainPerformanceReportError> {
    // preserve empty evidence as an unscored report rather than an evaluation error
    if rows.is_empty() {
        return Ok(None);
    }

    evaluate_forecast_adjustment_performance(
        rows,
        1_000,
        180,
        ForecastAdjustmentServingState::PendingReview,
    )
    .map(Some)
    .map_err(|_| RainPerformanceReportError::Scorecard)
}

/// Select evaluated diagnostics only when a complete probability row survives preparation.
fn rain_diagnostics(
    pairs: &[ForecastAdjustmentRainPerformancePair],
    calendar: &mut impl RainPerformanceReportCalendar,
) -> Result<JsonValue, RainPerformanceReportError> {
    let complete = pairs
        .iter()
        .filter(|pair| pair.targetTilingComplete)
        .map(probability_amount_pair)
        .collect::<Vec<_>>();
    let prepared = prepare_forecast_adjustment_performance_pairs(&complete, calendar)
        .map_err(scorecard_error)?;

    // retain the all-null closed document when no complete row remains eligible
    if prepared.rows.is_empty() {
        return Ok(unavailable_rain_diagnostics_json(
            &unavailable_forecast_adjustment_rain_diagnostics(),
        ));
    }

    evaluate_forecast_adjustment_rain_diagnostics(pairs, calendar)
        .map(|value| available_rain_diagnostics_json(&value))
        .map_err(|_| RainPerformanceReportError::RainDiagnostics)
}

/// Copy the amount fields from one probability diagnostic row.
fn probability_amount_pair(
    pair: &ForecastAdjustmentRainPerformancePair,
) -> ForecastAdjustmentPerformancePair {
    ForecastAdjustmentPerformancePair {
        adjustedPrediction: pair.adjustedPrediction,
        evidenceClass: pair.evidenceClass,
        fallback: pair.fallback,
        firstEdgeCommittedAt: pair.firstEdgeCommittedAt.clone(),
        horizonHours: pair.horizonHours,
        key: pair.key.clone(),
        localDate: pair.localDate.clone(),
        provenanceComplete: pair.provenanceComplete,
        rawPrediction: pair.rawPrediction,
        rowIdentity: pair.rowIdentity.clone(),
        sourceReceiptAt: pair.sourceReceiptAt.clone(),
        target: pair.target.clone(),
        targetKey: pair.targetKey.clone(),
        validAt: pair.validAt.clone(),
        vintageKey: pair.vintageKey.clone(),
    }
}

/// Build the original closed support counters in property order.
fn scorecard_support(
    prepared: &ForecastAdjustmentPreparedPairs,
    input_row_count: usize,
    wet_date_count: usize,
    wet_row_count: usize,
) -> JsonValue {
    let date_count = prepared
        .rows
        .iter()
        .map(|pair| pair.localDate.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let valid_hour_count = prepared
        .rows
        .iter()
        .map(|pair| pair.validAt.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let vintage_count = prepared
        .rows
        .iter()
        .map(|pair| pair.vintageKey.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let excluded_count = prepared.exclusions.invalid_target
        + prepared.exclusions.issued_after_valid
        + prepared.exclusions.missing_target
        + prepared.exclusions.receipt_chronology_missing
        + prepared.exclusions.source_late;
    let fallback_reasons = if prepared.fallbackCount == 0 {
        object(Vec::new())
    } else {
        object(vec![(
            "raw_fallback",
            number(prepared.fallbackCount as f64),
        )])
    };
    let gap_count =
        input_row_count - prepared.rows.len() + prepared.diagnostics.provenance_incomplete as usize;

    object(vec![
        ("dateCount", number(date_count as f64)),
        (
            "effectiveWeightSum",
            number(if prepared.rows.is_empty() { 0.0 } else { 1.0 }),
        ),
        ("eventCount", number(prepared.rows.len() as f64)),
        ("excludedCount", number(excluded_count as f64)),
        (
            "exclusionReasons",
            object(vec![
                (
                    "invalid_target",
                    number(prepared.exclusions.invalid_target as f64),
                ),
                (
                    "issued_after_valid",
                    number(prepared.exclusions.issued_after_valid as f64),
                ),
                (
                    "missing_target",
                    number(prepared.exclusions.missing_target as f64),
                ),
                (
                    "receipt_chronology_missing",
                    number(prepared.exclusions.receipt_chronology_missing as f64),
                ),
                (
                    "source_late",
                    number(prepared.exclusions.source_late as f64),
                ),
                (
                    "provenance_incomplete",
                    number(prepared.diagnostics.provenance_incomplete as f64),
                ),
            ]),
        ),
        ("fallbackCount", number(prepared.fallbackCount as f64)),
        ("fallbackReasons", fallback_reasons),
        ("gapCount", number(gap_count as f64)),
        ("rowCount", number(input_row_count as f64)),
        ("targetRowCount", number(prepared.rows.len() as f64)),
        ("validHourCount", number(valid_hour_count as f64)),
        ("vintageCount", number(vintage_count as f64)),
        ("wetDateCount", number(wet_date_count as f64)),
        ("wetRowCount", number(wet_row_count as f64)),
    ])
}

/// Partition selected prepared rows under the admitted local calendar.
fn calendar_slices(
    rows: &[ForecastAdjustmentPerformancePair],
    calendar: &mut impl RainPerformanceReportCalendar,
) -> Result<Vec<JsonValue>, RainPerformanceReportError> {
    let mut months = BTreeMap::<String, Vec<ForecastAdjustmentPerformancePair>>::new();
    let mut seasons = BTreeMap::<String, Vec<ForecastAdjustmentPerformancePair>>::new();
    let mut dayparts = BTreeMap::<String, Vec<ForecastAdjustmentPerformancePair>>::new();

    // assign every row to one label in each fixed dimension
    for row in rows {
        let labels = calendar
            .los_angeles_report_labels(&row.validAt)
            .ok_or(RainPerformanceReportError::Calendar)?;
        // reject labels outside the closed calendar month domain
        if !(1..=12).contains(&labels.month) {
            return Err(RainPerformanceReportError::Calendar);
        }
        months
            .entry(format!("{:02}", labels.month))
            .or_default()
            .push(row.clone());
        seasons
            .entry(labels.season.wire().to_owned())
            .or_default()
            .push(row.clone());
        dayparts
            .entry(labels.daypart.wire().to_owned())
            .or_default()
            .push(row.clone());
    }

    let mut slices = Vec::new();
    append_slices(&mut slices, "month", months)?;
    append_slices(&mut slices, "season", seasons)?;
    append_slices(&mut slices, "daypart", dayparts)?;
    Ok(slices)
}

/// Append lexically ordered slices for one calendar dimension.
fn append_slices(
    slices: &mut Vec<JsonValue>,
    dimension: &str,
    groups: BTreeMap<String, Vec<ForecastAdjustmentPerformancePair>>,
) -> Result<(), RainPerformanceReportError> {
    // preserve the original stable lexical label order
    for (label, rows) in groups {
        slices.push(object(vec![
            ("dimension", text(dimension)),
            ("label", text(&label)),
            ("metrics", descriptive_metric(&rows, 1_000)?),
            ("rowCount", number(rows.len() as f64)),
        ]));
    }
    Ok(())
}

/// Score a supported descriptive population without aggregate bootstrap claims.
fn descriptive_metric(
    rows: &[ForecastAdjustmentPerformancePair],
    minimum_rows: usize,
) -> Result<JsonValue, RainPerformanceReportError> {
    let date_count = rows
        .iter()
        .map(|pair| pair.localDate.as_str())
        .collect::<BTreeSet<_>>()
        .len();

    // retain null metrics below either descriptive support gate
    if rows.len() < minimum_rows || date_count < 7 {
        return Ok(unavailable_metric());
    }

    let metrics = score_balanced_forecast_adjustment_pairs(rows)
        .map_err(scorecard_error)?
        .metrics;
    Ok(metrics_json(&metrics, JsonValue::Null))
}

/// Project one evaluated aggregate or an unavailable metric document.
fn scorecard_metric(evaluation: Option<&ForecastAdjustmentPerformanceEvaluation>) -> JsonValue {
    let Some(evaluation) = evaluation else {
        return unavailable_metric();
    };
    let Nullable::Value(metrics) = &evaluation.metrics else {
        return unavailable_metric();
    };
    let interval = match &evaluation.bootstrap {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => object(vec![
            ("lower", number(value.lowerSkill * 100.0)),
            ("upper", number(value.upperSkill * 100.0)),
        ]),
    };
    metrics_json(metrics, interval)
}

/// Encode the shared amount metric projection in original property order.
fn metrics_json(
    metrics: &weather_contracts::ForecastAdjustmentPairedMetricSummary,
    skill_interval: JsonValue,
) -> JsonValue {
    object(vec![
        ("adjustedBias", number(metrics.adjusted.bias)),
        ("adjustedMae", number(metrics.adjusted.mae)),
        ("adjustedP95", number(metrics.adjusted.p95AbsoluteError)),
        ("adjustedRmse", number(metrics.adjusted.rmse)),
        ("deltaMae", number(metrics.delta.mae)),
        ("rawBias", number(metrics.raw.bias)),
        ("rawMae", number(metrics.raw.mae)),
        ("rawP95", number(metrics.raw.p95AbsoluteError)),
        ("rawRmse", number(metrics.raw.rmse)),
        ("skillInterval95", skill_interval),
        ("skillPercent", number(metrics.skill * 100.0)),
        ("unit", text(REPORT_UNIT)),
    ])
}

/// Return the original all-null metric document.
fn unavailable_metric() -> JsonValue {
    object(vec![
        ("adjustedBias", JsonValue::Null),
        ("adjustedMae", JsonValue::Null),
        ("adjustedP95", JsonValue::Null),
        ("adjustedRmse", JsonValue::Null),
        ("deltaMae", JsonValue::Null),
        ("rawBias", JsonValue::Null),
        ("rawMae", JsonValue::Null),
        ("rawP95", JsonValue::Null),
        ("rawRmse", JsonValue::Null),
        ("skillInterval95", JsonValue::Null),
        ("skillPercent", JsonValue::Null),
        ("unit", text(REPORT_UNIT)),
    ])
}

/// Encode available rain diagnostics through the generated contract fields.
fn available_rain_diagnostics_json(value: &ForecastAdjustmentRainDiagnostics) -> JsonValue {
    object(vec![
        (
            "accumulations",
            JsonValue::Array(value.accumulations.iter().map(accumulation_json).collect()),
        ),
        (
            "annualBalancedVolumeRatio",
            nullable_json_number(&value.annualBalancedVolumeRatio),
        ),
        (
            "heavyAdjustedMae",
            nullable_json_number(&value.heavyAdjustedMae),
        ),
        ("heavyRawMae", nullable_json_number(&value.heavyRawMae)),
        (
            "probabilityOrderViolationCount",
            number(value.probabilityOrderViolationCount as f64),
        ),
        (
            "thresholds",
            JsonValue::Array(
                value
                    .thresholds
                    .iter()
                    .map(available_threshold_json)
                    .collect(),
            ),
        ),
        (
            "wetAdjustedMae",
            nullable_json_number(&value.wetAdjustedMae),
        ),
        ("wetRawMae", nullable_json_number(&value.wetRawMae)),
        (
            "winterBalancedVolumeRatio",
            nullable_json_number(&value.winterBalancedVolumeRatio),
        ),
    ])
}

/// Encode the fixed unavailable rain diagnostic through its typed owner.
fn unavailable_rain_diagnostics_json(
    value: &weather_adjustment::UnavailableForecastAdjustmentRainDiagnostics,
) -> JsonValue {
    object(vec![
        (
            "accumulations",
            JsonValue::Array(value.accumulations.iter().map(accumulation_json).collect()),
        ),
        (
            "annualBalancedVolumeRatio",
            nullable_json_number(&value.annualBalancedVolumeRatio),
        ),
        (
            "heavyAdjustedMae",
            nullable_json_number(&value.heavyAdjustedMae),
        ),
        ("heavyRawMae", nullable_json_number(&value.heavyRawMae)),
        (
            "probabilityOrderViolationCount",
            number(value.probabilityOrderViolationCount as f64),
        ),
        (
            "thresholds",
            JsonValue::Array(
                value
                    .thresholds
                    .iter()
                    .map(unavailable_threshold_json)
                    .collect(),
            ),
        ),
        (
            "wetAdjustedMae",
            nullable_json_number(&value.wetAdjustedMae),
        ),
        ("wetRawMae", nullable_json_number(&value.wetRawMae)),
        (
            "winterBalancedVolumeRatio",
            nullable_json_number(&value.winterBalancedVolumeRatio),
        ),
    ])
}

/// Encode one rain accumulation window.
fn accumulation_json(value: &ForecastAdjustmentRainAccumulationSummary) -> JsonValue {
    object(vec![
        ("adjustedMae", nullable_json_number(&value.adjustedMae)),
        ("completeWindows", number(value.completeWindows as f64)),
        ("hours", number(accumulation_hours(value.hours) as f64)),
        ("rawMae", nullable_json_number(&value.rawMae)),
    ])
}

/// Encode one evaluated rain threshold.
fn available_threshold_json(value: &ForecastAdjustmentRainThresholdSummary) -> JsonValue {
    threshold_json(
        number(value.adjustedBrier),
        &value.csi,
        value.falseAlarms,
        &value.far,
        value.hits,
        value.misses,
        &value.pod,
        &value.reliability,
        value.thresholdMmPerHour,
    )
}

/// Encode one unavailable rain threshold with its explicit null Brier score.
fn unavailable_threshold_json(
    value: &weather_adjustment::UnavailableForecastAdjustmentRainThresholdSummary,
) -> JsonValue {
    threshold_json(
        nullable_json_number(&value.adjustedBrier),
        &value.csi,
        value.falseAlarms,
        &value.far,
        value.hits,
        value.misses,
        &value.pod,
        &value.reliability,
        value.thresholdMmPerHour,
    )
}

/// Encode the common rain-threshold document.
#[allow(clippy::too_many_arguments)]
fn threshold_json(
    adjusted_brier: JsonValue,
    csi: &Nullable<f64>,
    false_alarms: u64,
    far: &Nullable<f64>,
    hits: u64,
    misses: u64,
    pod: &Nullable<f64>,
    reliability: &[ForecastAdjustmentReliabilityBin],
    threshold: ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum,
) -> JsonValue {
    object(vec![
        ("adjustedBrier", adjusted_brier),
        ("csi", nullable_json_number(csi)),
        ("falseAlarms", number(false_alarms as f64)),
        ("far", nullable_json_number(far)),
        ("hits", number(hits as f64)),
        ("misses", number(misses as f64)),
        ("pod", nullable_json_number(pod)),
        ("rawBrier", JsonValue::Null),
        (
            "reliability",
            JsonValue::Array(reliability.iter().map(reliability_json).collect()),
        ),
        ("thresholdMmPerHour", number(threshold_value(threshold))),
    ])
}

/// Encode one reliability bin.
fn reliability_json(value: &ForecastAdjustmentReliabilityBin) -> JsonValue {
    object(vec![
        ("count", number(value.count as f64)),
        (
            "meanProbability",
            nullable_json_number(&value.meanProbability),
        ),
        (
            "observedFrequency",
            nullable_json_number(&value.observedFrequency),
        ),
    ])
}

/// Encode verified package inputs without accepting an untyped JSON object.
fn package_inputs_json(value: &RainPerformanceReportPackageInputs) -> JsonValue {
    object(vec![
        (
            "adjustmentEvidenceManifestSha256",
            text(&value.adjustment_evidence_manifest_sha256),
        ),
        (
            "adjustmentEvidenceWatermarkSha256",
            text(&value.adjustment_evidence_watermark_sha256),
        ),
        (
            "forecastTrainingManifestSha256",
            text(&value.forecast_training_manifest_sha256),
        ),
        ("localDateFrom", text(&value.local_date_from)),
        ("localDateTo", text(&value.local_date_to)),
        ("targetCutoffAt", text(&value.target_cutoff_at)),
    ])
}

/// Encode one admitted Best Match result in original property order.
fn best_match_json(value: &RainPerformanceBestMatchDiagnostic) -> JsonValue {
    object(vec![
        ("bestMatchRawMae", number(value.best_match_raw_mae)),
        ("dateCount", number(value.date_count as f64)),
        ("rowCount", number(value.row_count as f64)),
        ("sourceAdjustedMae", number(value.source_adjusted_mae)),
        ("sourceRawMae", number(value.source_raw_mae)),
        ("unit", text(REPORT_UNIT)),
    ])
}

/// Return one accumulation enum's exact numeric wire value.
const fn accumulation_hours(value: ForecastAdjustmentRainAccumulationSummaryHoursEnum) -> u64 {
    match value {
        ForecastAdjustmentRainAccumulationSummaryHoursEnum::Value6 => 6,
        ForecastAdjustmentRainAccumulationSummaryHoursEnum::Value12 => 12,
        ForecastAdjustmentRainAccumulationSummaryHoursEnum::Value23 => 23,
    }
}

/// Return one threshold enum's exact binary64 wire value.
const fn threshold_value(
    value: ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum,
) -> f64 {
    match value {
        ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::Value0Point1 => 0.1,
        ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::Value1 => 1.0,
        ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::Value2Point5 => 2.5,
    }
}

/// Return one evidence class's exact report wire label.
const fn evidence_class_wire(value: ForecastAdjustmentEvidenceClass) -> &'static str {
    match value {
        ForecastAdjustmentEvidenceClass::AsIssued => "as_issued",
        ForecastAdjustmentEvidenceClass::ProspectiveReceipt => "prospective_receipt",
        ForecastAdjustmentEvidenceClass::RetrospectiveCounterfactual => {
            "retrospective_counterfactual"
        }
        ForecastAdjustmentEvidenceClass::Development => "development",
    }
}

/// Return one comparison state's exact report wire label.
const fn comparison_wire(value: ForecastAdjustmentComparisonState) -> &'static str {
    match value {
        ForecastAdjustmentComparisonState::Unscored => "unscored",
        ForecastAdjustmentComparisonState::Better => "better",
        ForecastAdjustmentComparisonState::Mixed => "mixed",
        ForecastAdjustmentComparisonState::Worse => "worse",
    }
}

/// Return one qualification state's exact report wire label.
const fn qualification_wire(value: ForecastAdjustmentQualificationState) -> &'static str {
    match value {
        ForecastAdjustmentQualificationState::DevelopmentOnly => "development_only",
        ForecastAdjustmentQualificationState::CounterfactualOnly => "counterfactual_only",
        ForecastAdjustmentQualificationState::PendingSupport => "pending_support",
        ForecastAdjustmentQualificationState::Supported => "supported",
        ForecastAdjustmentQualificationState::Rejected => "rejected",
    }
}

impl RainPerformanceDaypart {
    /// Return the exact TypeScript daypart label.
    const fn wire(self) -> &'static str {
        match self {
            Self::Afternoon => "afternoon",
            Self::Evening => "evening",
            Self::Morning => "morning",
            Self::Night => "night",
        }
    }
}

impl RainPerformanceSeason {
    /// Return the exact TypeScript season label.
    const fn wire(self) -> &'static str {
        match self {
            Self::Autumn => "autumn",
            Self::Spring => "spring",
            Self::Summer => "summer",
            Self::Winter => "winter",
        }
    }
}

/// Borrow a nullable number without changing its absence semantics.
const fn nullable_number(value: &Nullable<f64>) -> Option<f64> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(*value),
    }
}

/// Encode a nullable number.
fn nullable_json_number(value: &Nullable<f64>) -> JsonValue {
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => number(*value),
    }
}

/// Encode nullable text.
fn nullable_text(value: &Nullable<String>) -> JsonValue {
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => text(value),
    }
}

/// Encode one finite report number.
const fn number(value: f64) -> JsonValue {
    JsonValue::Number(value)
}

/// Encode one JavaScript string as UTF-16 storage.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Encode one ordered JSON object.
fn object(entries: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

#[cfg(test)]
#[path = "rain_performance_report_test.rs"]
mod tests;
