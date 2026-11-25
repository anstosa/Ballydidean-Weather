//! Assemble the closed wind performance report from admitted native pairs.

use std::collections::{BTreeMap, BTreeSet};

use weather_adjustment::{
    evaluate_forecast_adjustment_performance, prepare_forecast_adjustment_performance_pairs,
    score_balanced_forecast_adjustment_pairs,
};
use weather_contracts::{
    ForecastAdjustmentComparisonState, ForecastAdjustmentEvidenceClass,
    ForecastAdjustmentPerformanceEvaluation, ForecastAdjustmentPerformancePair,
    ForecastAdjustmentPreparedPairs, ForecastAdjustmentQualificationState,
    ForecastAdjustmentServingState, ForecastAdjustmentSupportState, JsonValue, Nullable,
};

use crate::{
    RainPerformanceDaypart, RainPerformanceReportCalendar, RainPerformanceReportPackageInputs,
    RainPerformanceSeason,
};

const REPORT_VERSION: &str = "forecast-adjustment-performance-report/v1";
const REPORT_UNIT: &str = "meters_per_second";
const SITE_KEY: &str = "ballydidean";

/// Counts of receipt-backed fail-raw decisions that do not belong to a lead band.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindPerformanceNonActiveCounts {
    pub disabled: u64,
    pub not_applicable: u64,
}

impl WindPerformanceNonActiveCounts {
    /// Return the complete receipt-backed population outside active pair bands.
    fn total(self) -> u64 {
        self.disabled + self.not_applicable
    }
}

/// Closed failures from wind report scoring and calendar partitioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindPerformanceReportError {
    Calendar,
    Scorecard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PairReviewState {
    comparison: ForecastAdjustmentComparisonState,
    qualification: ForecastAdjustmentQualificationState,
    support: ForecastAdjustmentSupportState,
}

/// Assemble one canonicalizable wind report without package, bundle, or clock I/O.
#[allow(clippy::too_many_arguments)]
pub fn create_wind_performance_report(
    counterfactual_by_band: BTreeMap<String, Vec<ForecastAdjustmentPerformancePair>>,
    issued_by_band: BTreeMap<String, Vec<ForecastAdjustmentPerformancePair>>,
    nonactive: WindPerformanceNonActiveCounts,
    inputs: &RainPerformanceReportPackageInputs,
    serving_bundle_sha256: &str,
    source_revision: &str,
    generated_at: &str,
    calendar: &mut impl RainPerformanceReportCalendar,
) -> Result<JsonValue, WindPerformanceReportError> {
    let issued_row_count = issued_by_band.values().map(Vec::len).sum::<usize>();
    let issued_input_row_count = issued_row_count as u64 + nonactive.total();
    // prefer any captured issued evidence over retrospective replay
    let evidence_class = if issued_input_row_count > 0 {
        ForecastAdjustmentEvidenceClass::AsIssued
    } else {
        ForecastAdjustmentEvidenceClass::RetrospectiveCounterfactual
    };
    // keep issued and counterfactual populations disjoint
    let by_band = if issued_input_row_count > 0 {
        issued_by_band
    } else {
        counterfactual_by_band
    };
    let has_pair_band = !by_band.is_empty();

    let mut all_prepared_rows = Vec::new();
    let mut pair_reviews = Vec::with_capacity(by_band.len());
    let mut pair_states = Vec::with_capacity(by_band.len());
    let mut slices = Vec::with_capacity(by_band.len());
    let mut input_rows = 0_usize;
    let mut combined_exclusions = BTreeMap::<&'static str, u64>::new();
    let mut fallback_count = 0_u64;
    let mut excluded_count = 0_u64;

    // evaluate every enabled metric-band population independently
    for (key, pairs) in by_band {
        let prepared = prepare_forecast_adjustment_performance_pairs(&pairs, calendar)
            .map_err(|_| WindPerformanceReportError::Scorecard)?;
        let evaluation = optional_evaluation(&prepared.rows)?;
        input_rows += pairs.len();
        fallback_count += prepared.fallbackCount;
        excluded_count += exclusion_count(&prepared);
        add_exclusions(&mut combined_exclusions, &prepared);

        slices.push(object(vec![
            ("dimension", text("horizon")),
            ("label", text(&key.to_ascii_lowercase().replace(':', "-"))),
            ("metrics", descriptive_metric(&prepared.rows, 30)?),
            ("rowCount", number(prepared.rows.len() as f64)),
        ]));

        let state = pair_review_state(evaluation.as_ref(), evidence_class);
        pair_reviews.push(pair_review_json(
            &key,
            &prepared.rows,
            evaluation.as_ref(),
            state,
            evidence_class,
        ));
        pair_states.push(state);
        all_prepared_rows.extend(prepared.rows);
    }

    input_rows += nonactive.total() as usize;
    fallback_count += nonactive.total();
    excluded_count += nonactive.total();

    // retain only fail-raw states actually observed by the issued population
    if nonactive.disabled > 0 {
        combined_exclusions.insert("disabled", nonactive.disabled);
    }
    // retain only fail-raw states actually observed by the issued population
    if nonactive.not_applicable > 0 {
        combined_exclusions.insert("not_applicable", nonactive.not_applicable);
    }

    let every_pair_supported = pair_states
        .iter()
        .all(|state| state.support == ForecastAdjustmentSupportState::Sufficient);
    let any_pair_worse = pair_states
        .iter()
        .any(|state| state.comparison == ForecastAdjustmentComparisonState::Worse);
    let any_pair_unscored = pair_states
        .iter()
        .any(|state| state.comparison == ForecastAdjustmentComparisonState::Unscored);
    let any_pair_mixed = pair_states
        .iter()
        .any(|state| state.comparison == ForecastAdjustmentComparisonState::Mixed);
    let every_pair_qualified = pair_states
        .iter()
        .all(|state| state.qualification == ForecastAdjustmentQualificationState::Supported);
    let any_pair_rejected = pair_states
        .iter()
        .any(|state| state.qualification == ForecastAdjustmentQualificationState::Rejected);
    // preserve the original worst-first family comparison precedence
    let family_comparison = if any_pair_worse {
        ForecastAdjustmentComparisonState::Worse
    } else if any_pair_unscored {
        ForecastAdjustmentComparisonState::Unscored
    } else if any_pair_mixed {
        ForecastAdjustmentComparisonState::Mixed
    } else {
        ForecastAdjustmentComparisonState::Better
    };
    // keep counterfactual evidence outside issued qualification
    let family_qualification =
        if evidence_class == ForecastAdjustmentEvidenceClass::RetrospectiveCounterfactual {
            ForecastAdjustmentQualificationState::CounterfactualOnly
        } else if any_pair_rejected {
            ForecastAdjustmentQualificationState::Rejected
        } else if every_pair_qualified {
            ForecastAdjustmentQualificationState::Supported
        } else {
            ForecastAdjustmentQualificationState::PendingSupport
        };
    let aggregate_prepared =
        prepare_forecast_adjustment_performance_pairs(&all_prepared_rows, calendar)
            .map_err(|_| WindPerformanceReportError::Scorecard)?;
    let support = scorecard_support(
        &aggregate_prepared,
        input_rows,
        &combined_exclusions,
        excluded_count,
        fallback_count,
        has_pair_band,
    );
    slices.extend(calendar_slices(&all_prepared_rows, 30, calendar)?);
    // expose family metrics only after every enabled band reaches support
    let family_metrics = if every_pair_supported {
        descriptive_metric(&all_prepared_rows, 1)?
    } else {
        unavailable_metric()
    };
    // disable review only when at least one band is conclusively worse
    let family_recommendation = if any_pair_worse {
        "review_disable"
    } else {
        "review_candidate"
    };
    // mirror JavaScript's vacuous all-band support behavior
    let family_support = if every_pair_supported {
        "sufficient"
    } else {
        "insufficient"
    };

    Ok(object(vec![
        ("contractVersion", text(REPORT_VERSION)),
        (
            "family",
            object(vec![
                ("bestMatchDiagnostic", JsonValue::Null),
                ("comparisonState", text(comparison_wire(family_comparison))),
                ("evidenceClass", text(evidence_class_wire(evidence_class))),
                ("evidenceCutoffAt", text(&inputs.target_cutoff_at)),
                ("family", text("wind")),
                ("metrics", family_metrics),
                (
                    "qualificationState",
                    text(qualification_wire(family_qualification)),
                ),
                ("rainDiagnostics", JsonValue::Null),
                ("recommendation", text(family_recommendation)),
                ("servingIdentitySha256", text(serving_bundle_sha256)),
                ("servingState", text("authorized_active")),
                ("slices", JsonValue::Array(slices)),
                ("support", support),
                ("supportState", text(family_support)),
            ]),
        ),
        ("generatedAt", text(generated_at)),
        ("inputs", package_inputs_json(inputs)),
        ("pairReviews", JsonValue::Array(pair_reviews)),
        ("siteKey", text(SITE_KEY)),
        ("sourceRevision", text(source_revision)),
    ]))
}

/// Evaluate a nonempty metric-band population under the fixed wind gates.
fn optional_evaluation(
    rows: &[ForecastAdjustmentPerformancePair],
) -> Result<Option<ForecastAdjustmentPerformanceEvaluation>, WindPerformanceReportError> {
    // preserve empty evidence as an explicit unscored pair review
    if rows.is_empty() {
        return Ok(None);
    }

    evaluate_forecast_adjustment_performance(
        rows,
        30,
        7,
        ForecastAdjustmentServingState::PendingReview,
    )
    .map(Some)
    .map_err(|_| WindPerformanceReportError::Scorecard)
}

/// Derive the pair review states without reading values back out of JSON.
fn pair_review_state(
    evaluation: Option<&ForecastAdjustmentPerformanceEvaluation>,
    evidence_class: ForecastAdjustmentEvidenceClass,
) -> PairReviewState {
    // retain the original explicit defaults for an empty prepared population
    if let Some(evaluation) = evaluation {
        return PairReviewState {
            comparison: evaluation.comparisonState,
            qualification: evaluation.qualificationState,
            support: evaluation.supportState,
        };
    }

    // default empty issued and counterfactual bands independently
    let qualification = if evidence_class == ForecastAdjustmentEvidenceClass::AsIssued {
        ForecastAdjustmentQualificationState::PendingSupport
    } else {
        ForecastAdjustmentQualificationState::CounterfactualOnly
    };
    PairReviewState {
        comparison: ForecastAdjustmentComparisonState::Unscored,
        qualification,
        support: ForecastAdjustmentSupportState::Insufficient,
    }
}

/// Encode one metric-band review in the original property order.
fn pair_review_json(
    key: &str,
    rows: &[ForecastAdjustmentPerformancePair],
    evaluation: Option<&ForecastAdjustmentPerformanceEvaluation>,
    state: PairReviewState,
    evidence_class: ForecastAdjustmentEvidenceClass,
) -> JsonValue {
    let date_count = rows
        .iter()
        .map(|pair| pair.localDate.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    // preserve the original pair recommendation precedence
    let recommendation = if state.comparison == ForecastAdjustmentComparisonState::Worse {
        "review_disable"
    } else if evidence_class == ForecastAdjustmentEvidenceClass::AsIssued
        && state.qualification == ForecastAdjustmentQualificationState::Supported
    {
        "retain"
    } else {
        "review_candidate"
    };

    object(vec![
        ("comparisonState", text(comparison_wire(state.comparison))),
        ("dateCount", number(date_count as f64)),
        ("metrics", scorecard_metric(evaluation)),
        ("pairIdentity", text(key)),
        (
            "qualificationState",
            text(qualification_wire(state.qualification)),
        ),
        ("recommendation", text(recommendation)),
        ("rowCount", number(rows.len() as f64)),
        ("supportState", text(support_wire(state.support))),
    ])
}

/// Add one prepared band's bounded exclusion and diagnostic counters.
fn add_exclusions(
    combined: &mut BTreeMap<&'static str, u64>,
    prepared: &ForecastAdjustmentPreparedPairs,
) {
    let values = [
        ("invalid_target", prepared.exclusions.invalid_target),
        ("issued_after_valid", prepared.exclusions.issued_after_valid),
        ("missing_target", prepared.exclusions.missing_target),
        (
            "receipt_chronology_missing",
            prepared.exclusions.receipt_chronology_missing,
        ),
        ("source_late", prepared.exclusions.source_late),
        (
            "provenance_incomplete",
            prepared.diagnostics.provenance_incomplete,
        ),
    ];

    // sum every required prepared counter across independent bands
    for (reason, count) in values {
        *combined.entry(reason).or_default() += count;
    }
}

/// Return the total explicit preparation exclusions for one band.
const fn exclusion_count(prepared: &ForecastAdjustmentPreparedPairs) -> u64 {
    prepared.exclusions.invalid_target
        + prepared.exclusions.issued_after_valid
        + prepared.exclusions.missing_target
        + prepared.exclusions.receipt_chronology_missing
        + prepared.exclusions.source_late
}

/// Build the original closed support counters with wind-wide overrides.
fn scorecard_support(
    prepared: &ForecastAdjustmentPreparedPairs,
    input_row_count: usize,
    combined_exclusions: &BTreeMap<&'static str, u64>,
    excluded_count: u64,
    fallback_count: u64,
    has_pair_band: bool,
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
    // emit fallback details only for observed raw fallbacks
    let fallback_reasons = if fallback_count == 0 {
        object(Vec::new())
    } else {
        object(vec![("raw_fallback", number(fallback_count as f64))])
    };
    let gap_count =
        input_row_count - prepared.rows.len() + prepared.diagnostics.provenance_incomplete as usize;
    let mut exclusion_reasons = Vec::new();
    // preserve required prepared-counter insertion order when bands exist
    if has_pair_band {
        for reason in [
            "invalid_target",
            "issued_after_valid",
            "missing_target",
            "receipt_chronology_missing",
            "source_late",
            "provenance_incomplete",
        ] {
            exclusion_reasons.push((
                reason.encode_utf16().collect(),
                number(*combined_exclusions.get(reason).unwrap_or(&0) as f64),
            ));
        }
    }
    // append observed fail-raw states in their original object order
    for reason in ["disabled", "not_applicable"] {
        // omit absent states instead of manufacturing diagnostics
        if let Some(count) = combined_exclusions.get(reason) {
            exclusion_reasons.push((reason.encode_utf16().collect(), number(*count as f64)));
        }
    }
    // retain the aggregate's closed binary support mass
    let effective_weight_sum = if prepared.rows.is_empty() { 0.0 } else { 1.0 };

    object(vec![
        ("dateCount", number(date_count as f64)),
        ("effectiveWeightSum", number(effective_weight_sum)),
        ("eventCount", number(prepared.rows.len() as f64)),
        ("excludedCount", number(excluded_count as f64)),
        ("exclusionReasons", JsonValue::Object(exclusion_reasons)),
        ("fallbackCount", number(fallback_count as f64)),
        ("fallbackReasons", fallback_reasons),
        ("gapCount", number(gap_count as f64)),
        ("rowCount", number(input_row_count as f64)),
        ("targetRowCount", number(prepared.rows.len() as f64)),
        ("validHourCount", number(valid_hour_count as f64)),
        ("vintageCount", number(vintage_count as f64)),
        ("wetDateCount", number(0.0)),
        ("wetRowCount", number(0.0)),
    ])
}

/// Partition prepared rows under the pinned local report calendar.
fn calendar_slices(
    rows: &[ForecastAdjustmentPerformancePair],
    minimum_rows: usize,
    calendar: &mut impl RainPerformanceReportCalendar,
) -> Result<Vec<JsonValue>, WindPerformanceReportError> {
    let mut months = BTreeMap::<String, Vec<ForecastAdjustmentPerformancePair>>::new();
    let mut seasons = BTreeMap::<String, Vec<ForecastAdjustmentPerformancePair>>::new();
    let mut dayparts = BTreeMap::<String, Vec<ForecastAdjustmentPerformancePair>>::new();

    // assign every row to exactly one label in each fixed dimension
    for row in rows {
        let labels = calendar
            .los_angeles_report_labels(&row.validAt)
            .ok_or(WindPerformanceReportError::Calendar)?;
        // reject labels outside the closed month domain
        if !(1..=12).contains(&labels.month) {
            return Err(WindPerformanceReportError::Calendar);
        }
        months
            .entry(format!("{:02}", labels.month))
            .or_default()
            .push(row.clone());
        seasons
            .entry(season_wire(labels.season).to_owned())
            .or_default()
            .push(row.clone());
        dayparts
            .entry(daypart_wire(labels.daypart).to_owned())
            .or_default()
            .push(row.clone());
    }

    let mut slices = Vec::new();
    append_slices(&mut slices, "month", months, minimum_rows)?;
    append_slices(&mut slices, "season", seasons, minimum_rows)?;
    append_slices(&mut slices, "daypart", dayparts, minimum_rows)?;
    Ok(slices)
}

/// Append lexically ordered descriptive slices for one calendar dimension.
fn append_slices(
    slices: &mut Vec<JsonValue>,
    dimension: &str,
    groups: BTreeMap<String, Vec<ForecastAdjustmentPerformancePair>>,
    minimum_rows: usize,
) -> Result<(), WindPerformanceReportError> {
    // preserve the original stable lexical label order
    for (label, rows) in groups {
        slices.push(object(vec![
            ("dimension", text(dimension)),
            ("label", text(&label)),
            ("metrics", descriptive_metric(&rows, minimum_rows)?),
            ("rowCount", number(rows.len() as f64)),
        ]));
    }
    Ok(())
}

/// Score a supported descriptive population without aggregate bootstrap claims.
fn descriptive_metric(
    rows: &[ForecastAdjustmentPerformancePair],
    minimum_rows: usize,
) -> Result<JsonValue, WindPerformanceReportError> {
    let date_count = rows
        .iter()
        .map(|pair| pair.localDate.as_str())
        .collect::<BTreeSet<_>>()
        .len();

    // preserve unsupported slices as explicit null metrics
    if rows.len() < minimum_rows || date_count < 7 {
        return Ok(unavailable_metric());
    }

    let metrics = score_balanced_forecast_adjustment_pairs(rows)
        .map_err(|_| WindPerformanceReportError::Scorecard)?
        .metrics;
    Ok(metrics_json(&metrics, JsonValue::Null))
}

/// Project one evaluated aggregate or an unavailable metric document.
fn scorecard_metric(evaluation: Option<&ForecastAdjustmentPerformanceEvaluation>) -> JsonValue {
    // preserve an absent evaluation as unavailable metrics
    let Some(evaluation) = evaluation else {
        return unavailable_metric();
    };
    // preserve insufficient evaluation metrics as unavailable
    let Nullable::Value(metrics) = &evaluation.metrics else {
        return unavailable_metric();
    };
    // project bootstrap bounds only when the evaluator produced them
    let interval = match &evaluation.bootstrap {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => object(vec![
            ("lower", number(value.lowerSkill * 100.0)),
            ("upper", number(value.upperSkill * 100.0)),
        ]),
    };
    metrics_json(metrics, interval)
}

/// Encode the shared wind amount metric projection in original property order.
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

/// Return the original all-null wind metric document.
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

/// Return one evidence class's exact report wire label.
const fn evidence_class_wire(value: ForecastAdjustmentEvidenceClass) -> &'static str {
    // translate only the closed generated evidence variants
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
    // translate only the closed generated comparison variants
    match value {
        ForecastAdjustmentComparisonState::Unscored => "unscored",
        ForecastAdjustmentComparisonState::Better => "better",
        ForecastAdjustmentComparisonState::Mixed => "mixed",
        ForecastAdjustmentComparisonState::Worse => "worse",
    }
}

/// Return one qualification state's exact report wire label.
const fn qualification_wire(value: ForecastAdjustmentQualificationState) -> &'static str {
    // translate only the closed generated qualification variants
    match value {
        ForecastAdjustmentQualificationState::DevelopmentOnly => "development_only",
        ForecastAdjustmentQualificationState::CounterfactualOnly => "counterfactual_only",
        ForecastAdjustmentQualificationState::PendingSupport => "pending_support",
        ForecastAdjustmentQualificationState::Supported => "supported",
        ForecastAdjustmentQualificationState::Rejected => "rejected",
    }
}

/// Return one support state's exact report wire label.
const fn support_wire(value: ForecastAdjustmentSupportState) -> &'static str {
    // translate only the closed generated support variants
    match value {
        ForecastAdjustmentSupportState::Sufficient => "sufficient",
        ForecastAdjustmentSupportState::Insufficient => "insufficient",
        ForecastAdjustmentSupportState::Invalid => "invalid",
    }
}

/// Return the exact TypeScript daypart label.
const fn daypart_wire(value: RainPerformanceDaypart) -> &'static str {
    // translate only the admitted report dayparts
    match value {
        RainPerformanceDaypart::Afternoon => "afternoon",
        RainPerformanceDaypart::Evening => "evening",
        RainPerformanceDaypart::Morning => "morning",
        RainPerformanceDaypart::Night => "night",
    }
}

/// Return the exact TypeScript season label.
const fn season_wire(value: RainPerformanceSeason) -> &'static str {
    // translate only the admitted report seasons
    match value {
        RainPerformanceSeason::Autumn => "autumn",
        RainPerformanceSeason::Spring => "spring",
        RainPerformanceSeason::Summer => "summer",
        RainPerformanceSeason::Winter => "winter",
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
#[path = "wind_performance_report_test.rs"]
mod tests;
