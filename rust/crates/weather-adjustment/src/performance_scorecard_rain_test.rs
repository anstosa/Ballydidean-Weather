use std::collections::BTreeMap;

use weather_contracts::{
    ForecastAdjustmentEvidenceClass, ForecastAdjustmentRainPerformancePair,
    ForecastAdjustmentRainProbability, Nullable,
};

use super::*;

#[derive(Default)]
struct Calendar {
    epochs: BTreeMap<String, i64>,
}

impl ForecastAdjustmentScorecardCalendar for Calendar {
    /// Resolve only the fixture's explicit canonical clocks.
    fn canonical_utc_epoch_milliseconds(&mut self, value: &str) -> Option<i64> {
        self.epochs.get(value).copied()
    }

    /// Retain the original fixture's one Los Angeles date.
    fn los_angeles_local_date(&mut self, _value: &str, _epoch_milliseconds: i64) -> Option<String> {
        Some("2026-01-15".to_owned())
    }
}

impl ForecastAdjustmentRainDiagnosticCalendar for Calendar {
    /// Classify the January fixture through injected season evidence.
    fn los_angeles_is_winter(&mut self, value: &str) -> Option<bool> {
        self.epochs.contains_key(value).then_some(true)
    }
}

#[test]
fn named_probabilities_and_complete_same_run_windows_match_the_original_fixture() {
    let (pairs, mut calendar) = fixture_rows();
    let diagnostics =
        evaluate_forecast_adjustment_rain_diagnostics(&pairs, &mut calendar).expect("diagnostics");

    assert_eq!(diagnostics.thresholds.len(), 3);
    assert_eq!(diagnostics.thresholds[0].reliability.len(), 10);
    assert_eq!(diagnostics.thresholds[0].reliability[5].count, 0);
    assert!(diagnostics.thresholds[0].adjustedBrier > 0.0);
    assert_eq!(diagnostics.probabilityOrderViolationCount, 0);
    assert_eq!(
        diagnostics
            .accumulations
            .iter()
            .map(|entry| entry.completeWindows)
            .collect::<Vec<_>>(),
        vec![18, 12, 1]
    );
    assert_eq!(
        diagnostics.accumulations[2].adjustedMae,
        Nullable::Value(0.0)
    );
    assert_eq!(
        diagnostics.winterBalancedVolumeRatio,
        diagnostics.annualBalancedVolumeRatio
    );
}

#[test]
fn parity_and_probability_refusals_remain_fail_closed() {
    let (mut pairs, mut calendar) = fixture_rows();
    pairs[0].amountParity = false;
    assert_eq!(
        evaluate_forecast_adjustment_rain_diagnostics(&pairs, &mut calendar),
        Err(ForecastAdjustmentRainDiagnosticsError::AmountParity)
    );

    let (mut pairs, mut calendar) = fixture_rows();
    pairs[0].adjustedProbability.atLeast1_0 = 1.1;
    assert_eq!(
        evaluate_forecast_adjustment_rain_diagnostics(&pairs, &mut calendar),
        Err(ForecastAdjustmentRainDiagnosticsError::Probability)
    );
}

#[test]
fn unavailable_shape_keeps_null_brier_and_all_fixed_bins() {
    let diagnostics = unavailable_forecast_adjustment_rain_diagnostics();

    assert_eq!(diagnostics.accumulations.len(), 3);
    assert_eq!(diagnostics.thresholds.len(), 3);
    // preserve the CLI's null rather than inventing a numeric unavailable score
    for threshold in diagnostics.thresholds {
        assert_eq!(threshold.adjustedBrier, Nullable::Null);
        assert_eq!(
            threshold.rawBrier,
            ForecastAdjustmentRainThresholdSummaryRawBrierNull
        );
        assert_eq!(threshold.reliability.len(), 10);
        assert!(threshold.reliability.iter().all(|bin| bin.count == 0));
    }
}

/// Build the original twenty-three-hour probability fixture and admitted clocks.
fn fixture_rows() -> (Vec<ForecastAdjustmentRainPerformancePair>, Calendar) {
    let mut calendar = Calendar::default();
    let mut rows = Vec::new();
    // reproduce the exact named-probability fixture in source order
    for index in 0_u32..23 {
        let valid_at = valid_at(index);
        calendar
            .epochs
            .insert(valid_at.clone(), i64::from(index) * HOUR_MILLISECONDS);
        // preserve the original alternating dry wet and heavy targets
        let target = if index % 4 == 0 {
            3.0
        } else if index % 2 == 0 {
            0.2
        } else {
            0.0
        };
        rows.push(ForecastAdjustmentRainPerformancePair {
            adjustedPrediction: target,
            evidenceClass: ForecastAdjustmentEvidenceClass::Development,
            fallback: false,
            firstEdgeCommittedAt: Nullable::Null,
            horizonHours: f64::from(index + 1),
            key: format!("rain-{index}"),
            localDate: "2026-01-15".to_owned(),
            provenanceComplete: true,
            rawPrediction: 0.0,
            rowIdentity: format!("rain-row-{index}"),
            sourceReceiptAt: Nullable::Null,
            target: Nullable::Value(target),
            targetKey: format!("rain-target-{index}"),
            validAt: valid_at,
            vintageKey: "run-1".to_owned(),
            adjustedProbability: ForecastAdjustmentRainProbability {
                atLeast0_1: if target >= 0.1 { 0.9 } else { 0.1 },
                atLeast1_0: if target >= 1.0 { 0.8 } else { 0.05 },
                atLeast2_5: if target >= 2.5 { 0.7 } else { 0.01 },
            },
            amountParity: true,
            runKey: "run-1".to_owned(),
            targetTilingComplete: true,
        });
    }
    (rows, calendar)
}

/// Format the fixture's consecutive canonical UTC clock without a calendar implementation.
fn valid_at(index: u32) -> String {
    let absolute_hour = 8 + index;
    let day = 15 + absolute_hour / 24;
    let hour = absolute_hour % 24;
    format!("2026-01-{day:02}T{hour:02}:00:00.000Z")
}
