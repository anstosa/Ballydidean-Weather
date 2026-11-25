//! Rain-only probability, volume, and accumulation diagnostics for the performance scorecard.

use std::collections::BTreeMap;

use weather_contracts::{
    ForecastAdjustmentPerformancePair, ForecastAdjustmentRainAccumulationSummary,
    ForecastAdjustmentRainAccumulationSummaryHoursEnum, ForecastAdjustmentRainDiagnostics,
    ForecastAdjustmentRainPerformancePair, ForecastAdjustmentRainProbability,
    ForecastAdjustmentRainThresholdSummary, ForecastAdjustmentRainThresholdSummaryRawBrierNull,
    ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum, ForecastAdjustmentReliabilityBin,
    ForecastAdjustmentWeightedPair, Nullable,
};

use crate::{
    ForecastAdjustmentScorecardCalendar, balance_forecast_adjustment_performance_pairs,
    prepare_forecast_adjustment_performance_pairs,
};

const HOUR_MILLISECONDS: i64 = 3_600_000;

/// Calendar evidence needed in addition to the amount scorecard's date projection.
pub trait ForecastAdjustmentRainDiagnosticCalendar: ForecastAdjustmentScorecardCalendar {
    /// Resolve whether one canonical UTC hour belongs to Los Angeles winter.
    fn los_angeles_is_winter(&mut self, valid_at: &str) -> Option<bool>;
}

/// Closed failures from rain diagnostic admission and calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastAdjustmentRainDiagnosticsError {
    AmountParity,
    Calendar,
    Probability,
    Scorecard,
    Target,
}

/// Score parity-proven fixed-gauge rain rows through the original weighted diagnostics.
pub fn evaluate_forecast_adjustment_rain_diagnostics(
    pairs: &[ForecastAdjustmentRainPerformancePair],
    calendar: &mut impl ForecastAdjustmentRainDiagnosticCalendar,
) -> Result<ForecastAdjustmentRainDiagnostics, ForecastAdjustmentRainDiagnosticsError> {
    // stop probability claims when any saved amount or application state differs
    if pairs.iter().any(|pair| !pair.amountParity) {
        return Err(ForecastAdjustmentRainDiagnosticsError::AmountParity);
    }
    let complete = pairs
        .iter()
        .filter(|pair| pair.targetTilingComplete)
        .cloned()
        .collect::<Vec<_>>();
    let amount_pairs = complete.iter().map(amount_pair).collect::<Vec<_>>();
    let prepared = prepare_forecast_adjustment_performance_pairs(&amount_pairs, calendar)
        .map_err(|_| ForecastAdjustmentRainDiagnosticsError::Scorecard)?;
    let weighted = balance_forecast_adjustment_performance_pairs(&prepared.rows)
        .map_err(|_| ForecastAdjustmentRainDiagnosticsError::Scorecard)?;
    let probabilities = last_probabilities_by_key(&complete);
    let run_keys = last_run_keys_by_key(&complete);
    let rain_rows = weighted
        .into_iter()
        .map(|pair| {
            let probability = probabilities
                .get(&pair.key)
                .cloned()
                .filter(valid_probability)
                .ok_or(ForecastAdjustmentRainDiagnosticsError::Probability)?;
            Ok(WeightedRainRow { pair, probability })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut winter = Vec::new();
    // retain only rows whose injected Los Angeles calendar evidence says winter
    for row in &rain_rows {
        let is_winter = calendar
            .los_angeles_is_winter(&row.pair.validAt)
            .ok_or(ForecastAdjustmentRainDiagnosticsError::Calendar)?;
        // collect only the injected calendar's winter rows for separate rebalancing
        if is_winter {
            winter.push(unweighted_pair(&row.pair));
        }
    }
    // preserve an unavailable winter ratio when no winter population exists
    let winter_ratio = if winter.is_empty() {
        Nullable::Null
    } else {
        let balanced = balance_forecast_adjustment_performance_pairs(&winter)
            .map_err(|_| ForecastAdjustmentRainDiagnosticsError::Scorecard)?;
        nullable(volume_ratio(&balanced, Prediction::Adjusted)?)
    };
    Ok(ForecastAdjustmentRainDiagnostics {
        accumulations: [6_u64, 12, 23]
            .into_iter()
            .map(|hours| rain_accumulation_summary(&rain_rows, &run_keys, hours, calendar))
            .collect::<Result<Vec<_>, _>>()?,
        annualBalancedVolumeRatio: nullable(volume_ratio_weighted_rain(
            &rain_rows,
            Prediction::Adjusted,
        )?),
        heavyAdjustedMae: nullable(conditional_mae(&rain_rows, Prediction::Adjusted, 2.5)?),
        heavyRawMae: nullable(conditional_mae(&rain_rows, Prediction::Raw, 2.5)?),
        probabilityOrderViolationCount: u64::try_from(
            rain_rows
                .iter()
                .filter(|row| {
                    row.probability.atLeast0_1 < row.probability.atLeast1_0
                        || row.probability.atLeast1_0 < row.probability.atLeast2_5
                })
                .count(),
        )
        .map_err(|_| ForecastAdjustmentRainDiagnosticsError::Scorecard)?,
        thresholds: [
            Threshold::AtLeast0Point1,
            Threshold::AtLeast1,
            Threshold::AtLeast2Point5,
        ]
        .into_iter()
        .map(|threshold| rain_threshold_summary(&rain_rows, threshold))
        .collect::<Result<Vec<_>, _>>()?,
        wetAdjustedMae: nullable(conditional_mae(&rain_rows, Prediction::Adjusted, 0.1)?),
        wetRawMae: nullable(conditional_mae(&rain_rows, Prediction::Raw, 0.1)?),
        winterBalancedVolumeRatio: winter_ratio,
    })
}

/// Return the fixed closed diagnostic shape used when no probability rows qualify.
pub fn unavailable_forecast_adjustment_rain_diagnostics()
-> UnavailableForecastAdjustmentRainDiagnostics {
    UnavailableForecastAdjustmentRainDiagnostics {
        accumulations: [6_u64, 12, 23]
            .into_iter()
            .map(|hours| ForecastAdjustmentRainAccumulationSummary {
                adjustedMae: Nullable::Null,
                completeWindows: 0,
                hours: accumulation_hours(hours),
                rawMae: Nullable::Null,
            })
            .collect(),
        annualBalancedVolumeRatio: Nullable::Null,
        heavyAdjustedMae: Nullable::Null,
        heavyRawMae: Nullable::Null,
        probabilityOrderViolationCount: 0,
        thresholds: [
            Threshold::AtLeast0Point1,
            Threshold::AtLeast1,
            Threshold::AtLeast2Point5,
        ]
        .into_iter()
        .map(unavailable_threshold)
        .collect(),
        wetAdjustedMae: Nullable::Null,
        wetRawMae: Nullable::Null,
        winterBalancedVolumeRatio: Nullable::Null,
    }
}

/// Closed unavailable threshold shape whose Brier score is explicitly null.
#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq)]
pub struct UnavailableForecastAdjustmentRainThresholdSummary {
    pub adjustedBrier: Nullable<f64>,
    pub csi: Nullable<f64>,
    pub falseAlarms: u64,
    pub far: Nullable<f64>,
    pub hits: u64,
    pub misses: u64,
    pub pod: Nullable<f64>,
    pub rawBrier: ForecastAdjustmentRainThresholdSummaryRawBrierNull,
    pub reliability: Vec<ForecastAdjustmentReliabilityBin>,
    pub thresholdMmPerHour: ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum,
}

/// Closed report shape returned when no probability row qualifies.
#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq)]
pub struct UnavailableForecastAdjustmentRainDiagnostics {
    pub accumulations: Vec<ForecastAdjustmentRainAccumulationSummary>,
    pub annualBalancedVolumeRatio: Nullable<f64>,
    pub heavyAdjustedMae: Nullable<f64>,
    pub heavyRawMae: Nullable<f64>,
    pub probabilityOrderViolationCount: u64,
    pub thresholds: Vec<UnavailableForecastAdjustmentRainThresholdSummary>,
    pub wetAdjustedMae: Nullable<f64>,
    pub wetRawMae: Nullable<f64>,
    pub winterBalancedVolumeRatio: Nullable<f64>,
}

#[derive(Debug, Clone)]
struct WeightedRainRow {
    pair: ForecastAdjustmentWeightedPair,
    probability: ForecastAdjustmentRainProbability,
}

#[derive(Debug, Clone, Copy)]
enum Prediction {
    Adjusted,
    Raw,
}

#[derive(Debug, Clone, Copy)]
enum Threshold {
    AtLeast0Point1,
    AtLeast1,
    AtLeast2Point5,
}

impl Threshold {
    /// Return the exact threshold number used for event classification.
    const fn value(self) -> f64 {
        match self {
            Self::AtLeast0Point1 => 0.1,
            Self::AtLeast1 => 1.0,
            Self::AtLeast2Point5 => 2.5,
        }
    }

    /// Return the generated literal variant for the report document.
    const fn wire(self) -> ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum {
        match self {
            Self::AtLeast0Point1 => {
                ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::Value0Point1
            }
            Self::AtLeast1 => ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::Value1,
            Self::AtLeast2Point5 => {
                ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::Value2Point5
            }
        }
    }

    /// Select the matching named occurrence probability head.
    const fn probability(self, probability: &ForecastAdjustmentRainProbability) -> f64 {
        match self {
            Self::AtLeast0Point1 => probability.atLeast0_1,
            Self::AtLeast1 => probability.atLeast1_0,
            Self::AtLeast2Point5 => probability.atLeast2_5,
        }
    }
}

/// Copy only the amount-scorecard fields from the generated rain extension.
fn amount_pair(pair: &ForecastAdjustmentRainPerformancePair) -> ForecastAdjustmentPerformancePair {
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

/// Preserve JavaScript Map replacement semantics for duplicate pair keys.
fn last_probabilities_by_key(
    pairs: &[ForecastAdjustmentRainPerformancePair],
) -> BTreeMap<String, ForecastAdjustmentRainProbability> {
    let mut result = BTreeMap::new();
    // later rows replace earlier probability values with the same operational key
    for pair in pairs {
        result.insert(pair.key.clone(), pair.adjustedProbability.clone());
    }
    result
}

/// Preserve JavaScript Map replacement semantics for run identities.
fn last_run_keys_by_key(
    pairs: &[ForecastAdjustmentRainPerformancePair],
) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    // later rows replace earlier run identities with the same operational key
    for pair in pairs {
        result.insert(pair.key.clone(), pair.runKey.clone());
    }
    result
}

/// Require every named binary head to be a finite probability.
fn valid_probability(probability: &ForecastAdjustmentRainProbability) -> bool {
    [
        probability.atLeast0_1,
        probability.atLeast1_0,
        probability.atLeast2_5,
    ]
    .into_iter()
    .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
}

/// Score one named probability head and deterministic adjusted event threshold.
fn rain_threshold_summary(
    rows: &[WeightedRainRow],
    threshold: Threshold,
) -> Result<ForecastAdjustmentRainThresholdSummary, ForecastAdjustmentRainDiagnosticsError> {
    let value = threshold.value();
    let mut adjusted_brier = 0.0;
    let mut hits = 0_u64;
    let mut misses = 0_u64;
    let mut false_alarms = 0_u64;
    // retain probability and deterministic amount diagnostics as separate calculations
    for row in rows {
        let observed = binary(target_value(&row.pair.target)? >= value);
        let probability = threshold.probability(&row.probability);
        adjusted_brier += row.pair.weight * (probability - observed).powi(2);
        hits += u64::from(observed == 1.0 && row.pair.adjustedPrediction >= value);
        misses += u64::from(observed == 1.0 && row.pair.adjustedPrediction < value);
        false_alarms += u64::from(observed == 0.0 && row.pair.adjustedPrediction >= value);
    }
    Ok(ForecastAdjustmentRainThresholdSummary {
        adjustedBrier: adjusted_brier,
        csi: ratio(hits, hits + misses + false_alarms),
        falseAlarms: false_alarms,
        far: ratio(false_alarms, hits + false_alarms),
        hits,
        misses,
        pod: ratio(hits, hits + misses),
        rawBrier: ForecastAdjustmentRainThresholdSummaryRawBrierNull,
        reliability: reliability_bins(rows, threshold)?,
        thresholdMmPerHour: threshold.wire(),
    })
}

/// Calculate all ten fixed calibration bins without dropping empty bins.
fn reliability_bins(
    rows: &[WeightedRainRow],
    threshold: Threshold,
) -> Result<Vec<ForecastAdjustmentReliabilityBin>, ForecastAdjustmentRainDiagnosticsError> {
    let mut bins = Vec::with_capacity(10);
    // build each decile in original increasing index order
    for index in 0_u8..10 {
        let lower = f64::from(index) / 10.0;
        let upper = f64::from(index + 1) / 10.0;
        let selected = rows
            .iter()
            .filter(|row| {
                let value = threshold.probability(&row.probability);
                value >= lower && (index == 9 && value <= 1.0 || index != 9 && value < upper)
            })
            .collect::<Vec<_>>();
        let weight = selected.iter().map(|row| row.pair.weight).sum::<f64>();
        // preserve every empty or zero-weight decile explicitly
        if selected.is_empty() || weight == 0.0 {
            bins.push(ForecastAdjustmentReliabilityBin {
                count: 0,
                meanProbability: Nullable::Null,
                observedFrequency: Nullable::Null,
            });
            continue;
        }
        let mean = selected
            .iter()
            .map(|row| row.pair.weight / weight * threshold.probability(&row.probability))
            .sum::<f64>();
        let observed = selected
            .iter()
            .map(|row| {
                Ok(row.pair.weight / weight
                    * binary(target_value(&row.pair.target)? >= threshold.value()))
            })
            .collect::<Result<Vec<_>, ForecastAdjustmentRainDiagnosticsError>>()?
            .into_iter()
            .sum::<f64>();
        bins.push(ForecastAdjustmentReliabilityBin {
            count: u64::try_from(selected.len())
                .map_err(|_| ForecastAdjustmentRainDiagnosticsError::Scorecard)?,
            meanProbability: Nullable::Value(mean),
            observedFrequency: Nullable::Value(observed),
        });
    }
    Ok(bins)
}

/// Score complete consecutive same-run accumulation windows.
fn rain_accumulation_summary(
    rows: &[WeightedRainRow],
    run_keys: &BTreeMap<String, String>,
    hours: u64,
    calendar: &mut impl ForecastAdjustmentRainDiagnosticCalendar,
) -> Result<ForecastAdjustmentRainAccumulationSummary, ForecastAdjustmentRainDiagnosticsError> {
    let mut by_run: Vec<(String, Vec<&ForecastAdjustmentWeightedPair>)> = Vec::new();
    // partition adjusted hours by the exact run identity supplied before coalescing
    for row in rows {
        let run_key = run_keys
            .get(&row.pair.key)
            .ok_or(ForecastAdjustmentRainDiagnosticsError::Scorecard)?;
        // preserve first-run insertion order while appending every matching weighted row
        if let Some((_, run_rows)) = by_run.iter_mut().find(|(key, _)| key == run_key) {
            run_rows.push(&row.pair);
        } else {
            by_run.push((run_key.clone(), vec![&row.pair]));
        }
    }
    let mut raw_errors = Vec::new();
    let mut adjusted_errors = Vec::new();
    // scan each model run independently to prohibit cross-initialization windows
    for (_, mut run_rows) in by_run {
        run_rows.sort_by(|left, right| left.validAt.cmp(&right.validAt));
        let width = usize::try_from(hours)
            .map_err(|_| ForecastAdjustmentRainDiagnosticsError::Scorecard)?;
        // omit a run that cannot contain one full requested horizon
        if run_rows.len() < width {
            continue;
        }
        // retain every complete sliding window of the selected horizon
        for start in 0..=run_rows.len() - width {
            let window = &run_rows[start..start + width];
            let first = calendar
                .canonical_utc_epoch_milliseconds(&window[0].validAt)
                .ok_or(ForecastAdjustmentRainDiagnosticsError::Calendar)?;
            let mut complete = true;
            // verify exact hourly spacing through the injected UTC owner
            for (index, row) in window.iter().enumerate() {
                let clock = calendar
                    .canonical_utc_epoch_milliseconds(&row.validAt)
                    .ok_or(ForecastAdjustmentRainDiagnosticsError::Calendar)?;
                let offset = i64::try_from(index)
                    .ok()
                    .and_then(|index| index.checked_mul(HOUR_MILLISECONDS))
                    .and_then(|offset| first.checked_add(offset));
                // reject any gap or duplicate inside the candidate window
                if Some(clock) != offset {
                    complete = false;
                    break;
                }
            }
            // exclude every candidate that failed exact-hour admission
            if !complete {
                continue;
            }
            let target = window
                .iter()
                .map(|row| target_value(&row.target))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .sum::<f64>();
            let raw = window.iter().map(|row| row.rawPrediction).sum::<f64>();
            let adjusted = window.iter().map(|row| row.adjustedPrediction).sum::<f64>();
            raw_errors.push((raw - target).abs());
            adjusted_errors.push((adjusted - target).abs());
        }
    }
    Ok(ForecastAdjustmentRainAccumulationSummary {
        adjustedMae: mean(&adjusted_errors),
        completeWindows: u64::try_from(adjusted_errors.len())
            .map_err(|_| ForecastAdjustmentRainDiagnosticsError::Scorecard)?,
        hours: accumulation_hours(hours),
        rawMae: mean(&raw_errors),
    })
}

/// Calculate a target-conditional weighted absolute error.
fn conditional_mae(
    rows: &[WeightedRainRow],
    prediction: Prediction,
    minimum_target: f64,
) -> Result<Option<f64>, ForecastAdjustmentRainDiagnosticsError> {
    let selected = rows
        .iter()
        .filter_map(|row| {
            target_value(&row.pair.target)
                .ok()
                .filter(|target| *target >= minimum_target)
                .map(|target| (row, target))
        })
        .collect::<Vec<_>>();
    let total_weight = selected.iter().map(|(row, _)| row.pair.weight).sum::<f64>();
    // preserve unsupported conditional populations as unavailable
    if selected.is_empty() || total_weight == 0.0 {
        return Ok(None);
    }
    Ok(Some(
        selected
            .iter()
            .map(|(row, target)| {
                row.pair.weight / total_weight * (prediction.value(&row.pair) - target).abs()
            })
            .sum(),
    ))
}

/// Calculate the adjusted or raw weighted volume ratio for rain rows.
fn volume_ratio_weighted_rain(
    rows: &[WeightedRainRow],
    prediction: Prediction,
) -> Result<Option<f64>, ForecastAdjustmentRainDiagnosticsError> {
    let pairs = rows.iter().map(|row| row.pair.clone()).collect::<Vec<_>>();
    volume_ratio(&pairs, prediction)
}

/// Calculate the adjusted or raw weighted volume ratio without dividing dry observations.
fn volume_ratio(
    rows: &[ForecastAdjustmentWeightedPair],
    prediction: Prediction,
) -> Result<Option<f64>, ForecastAdjustmentRainDiagnosticsError> {
    let observed = rows
        .iter()
        .map(|row| Ok(row.weight * target_value(&row.target)?))
        .collect::<Result<Vec<_>, ForecastAdjustmentRainDiagnosticsError>>()?
        .into_iter()
        .sum::<f64>();
    // retain dry populations as unavailable volume ratios
    if observed == 0.0 {
        return Ok(None);
    }
    Ok(Some(
        rows.iter()
            .map(|row| row.weight * prediction.value(row))
            .sum::<f64>()
            / observed,
    ))
}

impl Prediction {
    /// Select the generated prediction field named by the original calculation.
    const fn value(self, pair: &ForecastAdjustmentWeightedPair) -> f64 {
        match self {
            Self::Adjusted => pair.adjustedPrediction,
            Self::Raw => pair.rawPrediction,
        }
    }
}

/// Remove a prior diagnostic weight before rebalancing the winter population.
fn unweighted_pair(pair: &ForecastAdjustmentWeightedPair) -> ForecastAdjustmentPerformancePair {
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

/// Read the post-preparation target that the TypeScript compiler proves non-null.
fn target_value(target: &Nullable<f64>) -> Result<f64, ForecastAdjustmentRainDiagnosticsError> {
    match target {
        Nullable::Value(value) => Ok(*value),
        Nullable::Null => Err(ForecastAdjustmentRainDiagnosticsError::Target),
    }
}

/// Project one optional scalar into the generated nullable representation.
fn nullable(value: Option<f64>) -> Nullable<f64> {
    value.map_or(Nullable::Null, Nullable::Value)
}

/// Project one event predicate into the original numeric observation flag.
const fn binary(value: bool) -> f64 {
    // match JavaScript Number(boolean) without another numeric policy
    if value { 1.0 } else { 0.0 }
}

/// Divide a categorical count only when its denominator is nonzero.
fn ratio(numerator: u64, denominator: u64) -> Nullable<f64> {
    // preserve undefined categorical rates as null
    if denominator == 0 {
        Nullable::Null
    } else {
        Nullable::Value(numerator as f64 / denominator as f64)
    }
}

/// Calculate one unweighted accumulation error mean.
fn mean(values: &[f64]) -> Nullable<f64> {
    // preserve empty accumulation populations as null
    if values.is_empty() {
        Nullable::Null
    } else {
        Nullable::Value(values.iter().sum::<f64>() / values.len() as f64)
    }
}

/// Select the generated accumulation-hour literal.
const fn accumulation_hours(hours: u64) -> ForecastAdjustmentRainAccumulationSummaryHoursEnum {
    match hours {
        6 => ForecastAdjustmentRainAccumulationSummaryHoursEnum::Value6,
        12 => ForecastAdjustmentRainAccumulationSummaryHoursEnum::Value12,
        23 => ForecastAdjustmentRainAccumulationSummaryHoursEnum::Value23,
        _ => unreachable!(),
    }
}

/// Build one closed unavailable threshold document including all ten empty bins.
fn unavailable_threshold(
    threshold: Threshold,
) -> UnavailableForecastAdjustmentRainThresholdSummary {
    UnavailableForecastAdjustmentRainThresholdSummary {
        adjustedBrier: Nullable::Null,
        csi: Nullable::Null,
        falseAlarms: 0,
        far: Nullable::Null,
        hits: 0,
        misses: 0,
        pod: Nullable::Null,
        rawBrier: ForecastAdjustmentRainThresholdSummaryRawBrierNull,
        reliability: (0..10)
            .map(|_| ForecastAdjustmentReliabilityBin {
                count: 0,
                meanProbability: Nullable::Null,
                observedFrequency: Nullable::Null,
            })
            .collect(),
        thresholdMmPerHour: threshold.wire(),
    }
}

#[cfg(test)]
#[path = "performance_scorecard_rain_test.rs"]
mod tests;
