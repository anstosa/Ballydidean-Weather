use weather_contracts::{
    ForecastAdjustmentComparisonState, ForecastAdjustmentEvidenceClass,
    ForecastAdjustmentPerformancePair, ForecastAdjustmentQualificationState,
    ForecastAdjustmentServingState, ForecastAdjustmentSupportState, Nullable,
};

use super::*;

/// match the compiled TypeScript evaluation and frozen ten-date bootstrap.
#[test]
fn matches_compiled_typescript_ten_date_oracle() {
    let rows = oracle_rows();

    let evaluation = evaluate_forecast_adjustment_performance(
        &rows,
        10,
        7,
        ForecastAdjustmentServingState::PendingReview,
    )
    .expect("evaluation");

    assert_eq!(evaluation.dateCount, 10);
    assert_eq!(evaluation.eventCount, 10);
    assert_eq!(
        evaluation.evidenceClass,
        ForecastAdjustmentEvidenceClass::AsIssued
    );
    assert_eq!(
        evaluation.supportState,
        ForecastAdjustmentSupportState::Sufficient
    );
    assert_eq!(
        evaluation.comparisonState,
        ForecastAdjustmentComparisonState::Better
    );
    assert_eq!(
        evaluation.qualificationState,
        ForecastAdjustmentQualificationState::Supported
    );
    assert_eq!(
        evaluation.servingState,
        ForecastAdjustmentServingState::PendingReview
    );

    let Nullable::Value(bootstrap) = evaluation.bootstrap else {
        panic!("bootstrap");
    };
    assert_eq!(bootstrap.lowerSkill, 0.106_382_978_723_404_26);
    assert_eq!(bootstrap.upperSkill, 0.469_879_518_072_289_3);

    let Nullable::Value(metrics) = evaluation.metrics else {
        panic!("metrics");
    };
    assert_eq!(metrics.raw.mae, 7.0);
    assert_eq!(metrics.adjusted.mae, 4.15);
    assert_eq!(metrics.skill, 0.407_142_857_142_857_1);
    assert_eq!(evaluation.weightedRows.len(), 10);
    assert!(evaluation.weightedRows.iter().all(|row| row.weight == 0.1));
}

/// keep insufficient support separate from evidence and serving classifications.
#[test]
fn preserves_orthogonal_states_below_support() {
    let mut rows = oracle_rows();
    rows.truncate(6);
    // retain development qualification despite an unrelated serving state
    for row in &mut rows {
        row.evidenceClass = ForecastAdjustmentEvidenceClass::Development;
    }

    let evaluation = evaluate_forecast_adjustment_performance(
        &rows,
        1,
        7,
        ForecastAdjustmentServingState::AuthorizedActive,
    )
    .expect("evaluation");

    assert_eq!(
        evaluation.supportState,
        ForecastAdjustmentSupportState::Insufficient
    );
    assert_eq!(
        evaluation.comparisonState,
        ForecastAdjustmentComparisonState::Unscored
    );
    assert_eq!(evaluation.metrics, Nullable::Null);
    assert_eq!(evaluation.bootstrap, Nullable::Null);
    assert_eq!(
        evaluation.qualificationState,
        ForecastAdjustmentQualificationState::DevelopmentOnly
    );
    assert_eq!(
        evaluation.servingState,
        ForecastAdjustmentServingState::AuthorizedActive
    );
    assert_eq!(evaluation.weightedRows.len(), 6);
}

/// retain boundary validation and provenance qualification independently.
#[test]
fn rejects_invalid_boundaries_and_keeps_incomplete_provenance_pending() {
    let rows = oracle_rows();
    assert_eq!(
        evaluate_forecast_adjustment_performance(
            &rows,
            1,
            6,
            ForecastAdjustmentServingState::PendingReview,
        ),
        Err(ForecastAdjustmentPerformanceEvaluationError::InvalidSupportContract)
    );

    let mut incomplete = rows;
    incomplete[0].provenanceComplete = false;
    let evaluation = evaluate_forecast_adjustment_performance(
        &incomplete,
        10,
        7,
        ForecastAdjustmentServingState::AdminDisabled,
    )
    .expect("evaluation");
    assert_eq!(
        evaluation.comparisonState,
        ForecastAdjustmentComparisonState::Better
    );
    assert_eq!(
        evaluation.qualificationState,
        ForecastAdjustmentQualificationState::PendingSupport
    );
    assert_eq!(
        evaluation.servingState,
        ForecastAdjustmentServingState::AdminDisabled
    );
}

/// build the discriminating ten-date compiled-TypeScript oracle population.
fn oracle_rows() -> Vec<ForecastAdjustmentPerformancePair> {
    let raw = [1.0, 7.0, 2.0, 9.0, 3.0, 11.0, 4.0, 13.0, 5.0, 15.0];
    let adjusted = [0.5, 9.0, 1.0, 4.0, 6.0, 3.0, 8.0, 2.0, 7.0, 1.0];
    let mut rows = Vec::with_capacity(raw.len());

    // retain source order and one equal-mass event per date
    for index in 0..raw.len() {
        let day = index + 1;
        rows.push(ForecastAdjustmentPerformancePair {
            adjustedPrediction: adjusted[index],
            evidenceClass: ForecastAdjustmentEvidenceClass::AsIssued,
            fallback: false,
            firstEdgeCommittedAt: Nullable::Value(format!("2026-01-{day:02}T00:00:00.000Z")),
            horizonHours: 1.0,
            key: format!("key-{index}"),
            localDate: format!("2026-01-{day:02}"),
            provenanceComplete: true,
            rawPrediction: raw[index],
            rowIdentity: format!("row-{index}"),
            sourceReceiptAt: Nullable::Value(format!("2026-01-{day:02}T00:00:00.000Z")),
            target: Nullable::Value(0.0),
            targetKey: format!("target-{index}"),
            validAt: format!("2026-01-{day:02}T01:00:00.000Z"),
            vintageKey: format!("vintage-{index}"),
        });
    }
    rows
}
