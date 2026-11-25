//! evaluate the original paired forecast-adjustment performance scorecard.

use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    ForecastAdjustmentBootstrapSummary, ForecastAdjustmentBootstrapSummaryContractVersionLiteral,
    ForecastAdjustmentBootstrapSummaryReplicatesLiteral, ForecastAdjustmentComparisonState,
    ForecastAdjustmentEvidenceClass, ForecastAdjustmentPairedMetricSummary,
    ForecastAdjustmentPerformanceEvaluation, ForecastAdjustmentPerformancePair,
    ForecastAdjustmentQualificationState, ForecastAdjustmentServingState,
    ForecastAdjustmentSupportState, Nullable,
};

use crate::performance_scorecard_amount::{
    BalancedForecastAdjustmentScore, ForecastAdjustmentScorecardError,
    score_balanced_forecast_adjustment_pairs,
};
use crate::wind_moving_block_bootstrap::{
    WIND_MOVING_BLOCK_LOWER_INDEX, WIND_MOVING_BLOCK_REPLICATES, WIND_MOVING_BLOCK_UPPER_INDEX,
    WindMovingBlockBootstrapError, create_wind_moving_block_start_plan,
    expand_wind_non_circular_block_starts,
};

/// closed failures from scorecard support validation or exact scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastAdjustmentPerformanceEvaluationError {
    Bootstrap(WindMovingBlockBootstrapError),
    InvalidSupportContract,
    OneEvidenceClassRequired,
    Scorecard(ForecastAdjustmentScorecardError),
}

impl Display for ForecastAdjustmentPerformanceEvaluationError {
    /// render the original boundary failures or their exact arithmetic source.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // keep boundary validation distinct from arithmetic failures
        match self {
            Self::Bootstrap(error) => Display::fmt(error, formatter),
            Self::InvalidSupportContract => {
                formatter.write_str("forecast adjustment support contract is invalid")
            }
            Self::OneEvidenceClassRequired => {
                formatter.write_str("forecast adjustment evaluation needs one evidence class")
            }
            Self::Scorecard(error) => write!(formatter, "forecast adjustment scorecard: {error:?}"),
        }
    }
}

impl Error for ForecastAdjustmentPerformanceEvaluationError {}

impl From<ForecastAdjustmentScorecardError> for ForecastAdjustmentPerformanceEvaluationError {
    /// retain the amount scorer's closed failure identity.
    fn from(error: ForecastAdjustmentScorecardError) -> Self {
        Self::Scorecard(error)
    }
}

impl From<WindMovingBlockBootstrapError> for ForecastAdjustmentPerformanceEvaluationError {
    /// retain the frozen plan's closed failure identity.
    fn from(error: WindMovingBlockBootstrapError) -> Self {
        Self::Bootstrap(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DateLoss {
    adjusted_mae: f64,
    raw_mae: f64,
}

/// produce orthogonal support, comparison, qualification, and serving states.
pub fn evaluate_forecast_adjustment_performance(
    rows: &[ForecastAdjustmentPerformancePair],
    minimum_rows: usize,
    minimum_dates: usize,
    serving_state: ForecastAdjustmentServingState,
) -> Result<ForecastAdjustmentPerformanceEvaluation, ForecastAdjustmentPerformanceEvaluationError> {
    // require the frozen block length and a nonempty row threshold
    if minimum_dates < 7 || minimum_rows < 1 {
        return Err(ForecastAdjustmentPerformanceEvaluationError::InvalidSupportContract);
    }

    // reject empty success before reading the retained evidence class
    if rows.is_empty() {
        return Err(ForecastAdjustmentPerformanceEvaluationError::OneEvidenceClassRequired);
    }

    let evidence_class = rows[0].evidenceClass;
    // reject mixed evidence before status derivation
    if rows.iter().any(|row| row.evidenceClass != evidence_class) {
        return Err(ForecastAdjustmentPerformanceEvaluationError::OneEvidenceClassRequired);
    }

    let date_count = rows
        .iter()
        .map(|row| row.localDate.as_str())
        .collect::<HashSet<_>>()
        .len();
    let sufficient = date_count >= minimum_dates && rows.len() >= minimum_rows;
    let BalancedForecastAdjustmentScore {
        metrics: scored_metrics,
        weighted_rows,
    } = score_balanced_forecast_adjustment_pairs(rows)?;
    let metrics = if sufficient {
        Nullable::Value(scored_metrics)
    } else {
        Nullable::Null
    };
    let bootstrap = if sufficient {
        bootstrap_balanced_forecast_adjustment_pairs(rows)?
    } else {
        Nullable::Null
    };
    let comparison = comparison_state(&metrics, &bootstrap);
    let provenance_complete = rows.iter().all(|row| row.provenanceComplete);
    let qualification_state =
        qualification_state(evidence_class, provenance_complete, sufficient, comparison);

    Ok(ForecastAdjustmentPerformanceEvaluation {
        bootstrap,
        comparisonState: comparison,
        dateCount: date_count as u64,
        evidenceClass: evidence_class,
        eventCount: rows.len() as u64,
        metrics,
        qualificationState: qualification_state,
        servingState: serving_state,
        supportState: if sufficient {
            ForecastAdjustmentSupportState::Sufficient
        } else {
            ForecastAdjustmentSupportState::Insufficient
        },
        weightedRows: weighted_rows,
    })
}

/// run the exact frozen single-window moving-block bootstrap.
fn bootstrap_balanced_forecast_adjustment_pairs(
    rows: &[ForecastAdjustmentPerformancePair],
) -> Result<
    Nullable<ForecastAdjustmentBootstrapSummary>,
    ForecastAdjustmentPerformanceEvaluationError,
> {
    let mut pairs_by_date = BTreeMap::<&str, Vec<ForecastAdjustmentPerformancePair>>::new();

    // preserve source order inside every sorted local-date population
    for row in rows {
        pairs_by_date
            .entry(row.localDate.as_str())
            .or_default()
            .push(row.clone());
    }

    // expose explicit insufficient support below the frozen block length
    if pairs_by_date.len() < 7 {
        return Ok(Nullable::Null);
    }

    let mut losses_by_date = Vec::with_capacity(pairs_by_date.len());
    // precompute every date's invariant nested-weight losses once
    for date_rows in pairs_by_date.values() {
        let scored = score_balanced_forecast_adjustment_pairs(date_rows)?;
        losses_by_date.push(DateLoss {
            adjusted_mae: scored.metrics.adjusted.mae,
            raw_mae: scored.metrics.raw.mae,
        });
    }

    let plan = create_wind_moving_block_start_plan(&[losses_by_date.len()])?;
    let mut skills = Vec::with_capacity(WIND_MOVING_BLOCK_REPLICATES);
    // score the exact frozen start plan without dropping replicates
    for replicate in &plan {
        let [starts] = replicate.as_slice() else {
            return Err(WindMovingBlockBootstrapError::InvalidBlockPlan.into());
        };
        let offsets = expand_wind_non_circular_block_starts(starts, losses_by_date.len())?;
        skills.push(sampled_date_skill(&losses_by_date, &offsets)?);
    }

    skills.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));

    // retain the exact frozen replicate and quantile contract
    if skills.len() != WIND_MOVING_BLOCK_REPLICATES {
        return Err(WindMovingBlockBootstrapError::InvalidBlockPlan.into());
    }

    Ok(Nullable::Value(ForecastAdjustmentBootstrapSummary {
        contractVersion: ForecastAdjustmentBootstrapSummaryContractVersionLiteral,
        lowerSkill: skills[WIND_MOVING_BLOCK_LOWER_INDEX],
        replicates: ForecastAdjustmentBootstrapSummaryReplicatesLiteral,
        upperSkill: skills[WIND_MOVING_BLOCK_UPPER_INDEX],
    }))
}

/// score one repeated sequence of equal-mass sampled local-date slots.
fn sampled_date_skill(
    losses_by_date: &[DateLoss],
    offsets: &[usize],
) -> Result<f64, ForecastAdjustmentPerformanceEvaluationError> {
    let mut raw = 0.0;
    let mut adjusted = 0.0;
    let selected_date_count = offsets.len() as f64;

    // preserve the source's left-to-right division then addition order
    for &offset in offsets {
        let losses = losses_by_date
            .get(offset)
            .ok_or(WindMovingBlockBootstrapError::InvalidBlockStart)?;
        raw += losses.raw_mae / selected_date_count;
        adjusted += losses.adjusted_mae / selected_date_count;
    }

    paired_skill(raw, adjusted).map_err(Into::into)
}

/// compute the original zero-safe paired skill for a sampled date sequence.
fn paired_skill(
    raw_loss: f64,
    adjusted_loss: f64,
) -> Result<f64, ForecastAdjustmentScorecardError> {
    // reject invalid sampled losses
    if !raw_loss.is_finite() || !adjusted_loss.is_finite() || raw_loss < 0.0 || adjusted_loss < 0.0
    {
        return Err(ForecastAdjustmentScorecardError::InvalidPair);
    }
    // preserve equal zero loss
    if raw_loss == 0.0 && adjusted_loss == 0.0 {
        return Ok(0.0);
    }
    // preserve harmed zero baseline
    if raw_loss == 0.0 {
        return Ok(-1.0);
    }
    Ok((raw_loss - adjusted_loss) / raw_loss)
}

/// classify paired skill without conflating support or qualification.
fn comparison_state(
    metrics: &Nullable<ForecastAdjustmentPairedMetricSummary>,
    bootstrap: &Nullable<ForecastAdjustmentBootstrapSummary>,
) -> ForecastAdjustmentComparisonState {
    let (Nullable::Value(metrics), Nullable::Value(bootstrap)) = (metrics, bootstrap) else {
        return ForecastAdjustmentComparisonState::Unscored;
    };

    // require interval-wide improvement before calling better
    if metrics.skill > 0.0 && bootstrap.lowerSkill > 0.0 {
        return ForecastAdjustmentComparisonState::Better;
    }
    // require interval-wide harm before calling worse
    if metrics.skill < 0.0 && bootstrap.upperSkill < 0.0 {
        return ForecastAdjustmentComparisonState::Worse;
    }
    ForecastAdjustmentComparisonState::Mixed
}

/// derive qualification without changing support, comparison, or serving state.
fn qualification_state(
    evidence_class: ForecastAdjustmentEvidenceClass,
    provenance_complete: bool,
    sufficient: bool,
    comparison: ForecastAdjustmentComparisonState,
) -> ForecastAdjustmentQualificationState {
    // prevent development evidence from making prospective claims
    if evidence_class == ForecastAdjustmentEvidenceClass::Development {
        return ForecastAdjustmentQualificationState::DevelopmentOnly;
    }
    // prevent counterfactual evidence from making issuance claims
    if evidence_class == ForecastAdjustmentEvidenceClass::RetrospectiveCounterfactual {
        return ForecastAdjustmentQualificationState::CounterfactualOnly;
    }
    // retain pending support for incomplete or inconclusive evidence
    if !provenance_complete
        || !sufficient
        || matches!(
            comparison,
            ForecastAdjustmentComparisonState::Mixed | ForecastAdjustmentComparisonState::Unscored
        )
    {
        return ForecastAdjustmentQualificationState::PendingSupport;
    }
    // reject demonstrated harm independently from serving state
    if comparison == ForecastAdjustmentComparisonState::Worse {
        return ForecastAdjustmentQualificationState::Rejected;
    }
    ForecastAdjustmentQualificationState::Supported
}

#[cfg(test)]
#[path = "performance_scorecard_evaluation_test.rs"]
mod tests;
