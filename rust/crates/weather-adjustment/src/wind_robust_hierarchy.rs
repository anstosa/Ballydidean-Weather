use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    ForecastAdjustmentCoefficient, ForecastAdjustmentCoefficientDaypartEnum as Daypart,
    ForecastAdjustmentCoefficientLevelEnum as Level,
    ForecastAdjustmentCoefficientSeasonEnum as Season, ForecastAdjustmentMetric,
    ForecastLeadBandKey, Nullable,
};

const MONTH_MINIMUM_EFFECTIVE_EVENTS: f64 = 50.0;
const MONTH_PSEUDOCOUNT: f64 = 50.0;
const ROOT_MINIMUM_EFFECTIVE_EVENTS: f64 = 200.0;
const ROOT_PSEUDOCOUNT: f64 = 200.0;
const SEASON_MINIMUM_EFFECTIVE_EVENTS: f64 = 100.0;
const SEASON_PSEUDOCOUNT: f64 = 100.0;

/// One residual whose time and local-calendar fields were admitted by the training owner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdmittedWindHierarchyObservation<'a> {
    pub daypart: Daypart,
    pub month: u8,
    pub reference_at: Option<&'a str>,
    pub residual: f64,
    pub season: Season,
    pub stable_id: &'a str,
    pub target_lead_hours: i64,
    pub valid_at: &'a str,
    pub weight: f64,
}

/// Failure to fit an admitted residual collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindRobustHierarchyError {
    InvalidMonth,
    InvalidObservation,
}

impl Display for WindRobustHierarchyError {
    /// Render a stable fitter failure without exposing observation material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // keep the public error independent of retained training values
        match self {
            Self::InvalidMonth => formatter.write_str("wind hierarchy month is invalid"),
            Self::InvalidObservation => {
                formatter.write_str("wind hierarchy observation is invalid")
            }
        }
    }
}

impl Error for WindRobustHierarchyError {}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FittedCell {
    coefficient: f64,
    effective_event_count: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CellFitConfiguration {
    correction_maximum: f64,
    correction_minimum: f64,
    direction: bool,
    minimum_effective_events: f64,
    parent_coefficient: f64,
    pseudocount: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct EstimatorRow {
    stable_id: String,
    value: f64,
    weight: f64,
}

/// Fit the frozen three-level robust hierarchy for one metric and lead band.
pub fn fit_wind_robust_hierarchy(
    metric: ForecastAdjustmentMetric,
    lead_band: ForecastLeadBandKey,
    observations: &[AdmittedWindHierarchyObservation<'_>],
) -> Result<Vec<ForecastAdjustmentCoefficient>, WindRobustHierarchyError> {
    // preserve the original empty-cell result
    if observations.is_empty() {
        return Ok(Vec::new());
    }

    validate_observations(observations)?;
    let direction = metric == ForecastAdjustmentMetric::WindDirectionDegrees;
    let (minimum, maximum) = correction_bounds(metric);
    let root_rows = observations.iter().collect::<Vec<_>>();
    let Some(root) = fit_cell(
        &root_rows,
        CellFitConfiguration {
            correction_maximum: maximum,
            correction_minimum: minimum,
            direction,
            minimum_effective_events: ROOT_MINIMUM_EFFECTIVE_EVENTS,
            parent_coefficient: 0.0,
            pseudocount: ROOT_PSEUDOCOUNT,
        },
    )?
    else {
        return Ok(Vec::new());
    };
    let mut coefficients = vec![ForecastAdjustmentCoefficient {
        coefficient: root.coefficient,
        daypart: Nullable::Null,
        effectiveEventCount: root.effective_event_count,
        leadBand: lead_band,
        level: Level::Value1,
        metric,
        month: Nullable::Null,
        season: Nullable::Null,
    }];
    let season_groups = group_by_season(observations);
    let mut season_coefficients = BTreeMap::<String, f64>::new();

    // emit supported season/daypart cells in canonical lexical key order
    for (key, rows) in season_groups {
        let Some(cell) = fit_cell(
            &rows,
            CellFitConfiguration {
                correction_maximum: maximum,
                correction_minimum: minimum,
                direction,
                minimum_effective_events: SEASON_MINIMUM_EFFECTIVE_EVENTS,
                parent_coefficient: root.coefficient,
                pseudocount: SEASON_PSEUDOCOUNT,
            },
        )?
        else {
            continue;
        };
        let first = rows[0];
        season_coefficients.insert(key, cell.coefficient);
        coefficients.push(ForecastAdjustmentCoefficient {
            coefficient: cell.coefficient,
            daypart: Nullable::Value(first.daypart),
            effectiveEventCount: cell.effective_event_count,
            leadBand: lead_band,
            level: Level::Value2,
            metric,
            month: Nullable::Null,
            season: Nullable::Value(first.season),
        });
    }

    // emit supported month/daypart cells in canonical lexical key order
    for (_key, rows) in group_by_month(observations) {
        let first = rows[0];
        let season = season_for_month(first.month);
        let season_key = season_group_key(season, first.daypart);
        let parent = season_coefficients
            .get(&season_key)
            .copied()
            .unwrap_or(root.coefficient);
        let Some(cell) = fit_cell(
            &rows,
            CellFitConfiguration {
                correction_maximum: maximum,
                correction_minimum: minimum,
                direction,
                minimum_effective_events: MONTH_MINIMUM_EFFECTIVE_EVENTS,
                parent_coefficient: parent,
                pseudocount: MONTH_PSEUDOCOUNT,
            },
        )?
        else {
            continue;
        };
        coefficients.push(ForecastAdjustmentCoefficient {
            coefficient: cell.coefficient,
            daypart: Nullable::Value(first.daypart),
            effectiveEventCount: cell.effective_event_count,
            leadBand: lead_band,
            level: Level::Value3,
            metric,
            month: Nullable::Value(u64::from(first.month)),
            season: Nullable::Null,
        });
    }

    Ok(coefficients)
}

/// Validate numerical residuals while trusting the admitted time strings.
fn validate_observations(
    observations: &[AdmittedWindHierarchyObservation<'_>],
) -> Result<(), WindRobustHierarchyError> {
    let mut has_positive_weight = false;

    // inspect each admitted residual once before numerical work
    for observation in observations {
        // reject nonfinite residual mass and negative weights
        if !observation.residual.is_finite()
            || !observation.weight.is_finite()
            || observation.weight < 0.0
        {
            return Err(WindRobustHierarchyError::InvalidObservation);
        }

        // retain the original positive-mass requirement
        if observation.weight > 0.0 {
            has_positive_weight = true;
        }

        // keep local months inside the original calendar domain
        if !(1..=12).contains(&observation.month) {
            return Err(WindRobustHierarchyError::InvalidMonth);
        }

        // keep each month's parent in its actual season
        if observation.season != season_for_month(observation.month) {
            return Err(WindRobustHierarchyError::InvalidObservation);
        }
    }

    // reject cells whose effective mass is zero
    if !has_positive_weight {
        return Err(WindRobustHierarchyError::InvalidObservation);
    }

    Ok(())
}

/// Fit one thresholded cell against its literal parent prior.
fn fit_cell(
    observations: &[&AdmittedWindHierarchyObservation<'_>],
    configuration: CellFitConfiguration,
) -> Result<Option<FittedCell>, WindRobustHierarchyError> {
    let effective_event_count = kish_effective_event_count(observations);

    // inherit without emission below the frozen support threshold
    if effective_event_count < configuration.minimum_effective_events {
        return Ok(None);
    }

    let mut sorted = observations.to_vec();
    sorted.sort_by(|left, right| compare_residual_observations(left, right));
    let estimator = sorted
        .iter()
        .map(|observation| EstimatorRow {
            stable_id: format!(
                "{}\0{}\0{}\0{}",
                observation.valid_at,
                observation.target_lead_hours,
                observation.reference_at.unwrap_or(""),
                observation.stable_id
            ),
            value: observation.residual,
            weight: observation.weight,
        })
        .collect::<Vec<_>>();
    let raw_coefficient = if configuration.direction {
        deterministic_weighted_circular_median(&estimator)?
    } else {
        deterministic_weighted_median(&estimator)?
    };
    let alpha = effective_event_count / (effective_event_count + configuration.pseudocount);
    let shrunk = if configuration.direction {
        wrap_180(
            configuration.parent_coefficient
                + alpha * wrap_180(raw_coefficient - configuration.parent_coefficient),
        )
    } else {
        configuration.parent_coefficient
            + alpha * (raw_coefficient - configuration.parent_coefficient)
    };
    let coefficient = cap_coefficient(
        shrunk,
        configuration.correction_minimum,
        configuration.correction_maximum,
    );

    Ok(Some(FittedCell {
        coefficient,
        effective_event_count,
    }))
}

/// Compute Kish effective count after aggregating atomic weights by event time.
fn kish_effective_event_count(observations: &[&AdmittedWindHierarchyObservation<'_>]) -> f64 {
    let mut sorted = observations.to_vec();
    sorted.sort_by(|left, right| {
        compare_text(left.valid_at, right.valid_at)
            .then_with(|| left.target_lead_hours.cmp(&right.target_lead_hours))
            .then_with(|| compare_text(left.stable_id, right.stable_id))
    });
    let mut event_weights = Vec::<(&str, f64)>::new();

    // aggregate each event's atomic weights in target and stable order
    for observation in sorted {
        // add to the current event because valid-at order is contiguous
        if let Some((valid_at, weight)) = event_weights.last_mut()
            && *valid_at == observation.valid_at
        {
            *weight += observation.weight;
            continue;
        }

        event_weights.push((observation.valid_at, observation.weight));
    }

    let mut sum = 0.0;
    let mut squared_sum = 0.0;

    // preserve the original valid-at insertion order for floating-point sums
    for (_, weight) in event_weights {
        sum += weight;
        squared_sum += weight * weight;
    }

    sum * sum / squared_sum
}

/// Select the exact weighted scalar median and its half-mass tie average.
fn deterministic_weighted_median(
    observations: &[EstimatorRow],
) -> Result<f64, WindRobustHierarchyError> {
    let mut sorted = observations.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| {
        left.value
            .partial_cmp(&right.value)
            .unwrap_or(Ordering::Equal)
            .then_with(|| compare_text(&left.stable_id, &right.stable_id))
    });
    let mut distinct = Vec::<(f64, f64)>::new();
    let mut total_weight = 0.0;

    // combine exactly equal values from left to right
    for observation in sorted {
        total_weight += observation.weight;

        // merge one exact distinct estimator value
        if let Some((value, weight)) = distinct.last_mut()
            && *value == observation.value
        {
            *weight += observation.weight;
            continue;
        }

        distinct.push((observation.value, observation.weight));
    }

    // retain the original positive-total estimator requirement
    if total_weight.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return Err(WindRobustHierarchyError::InvalidObservation);
    }

    let mut cumulative_weight = 0.0;

    // locate the first strict half-weight crossing
    for (index, (value, weight)) in distinct.iter().enumerate() {
        cumulative_weight += weight;

        // return the strict crossing unchanged
        if cumulative_weight > total_weight / 2.0 {
            return Ok(*value);
        }

        // average the exact tie with the next greater value
        if cumulative_weight == total_weight / 2.0 {
            return Ok(distinct
                .get(index + 1)
                .map_or(*value, |(next, _)| (*value + *next) / 2.0));
        }
    }

    distinct
        .last()
        .map(|(value, _)| *value)
        .ok_or(WindRobustHierarchyError::InvalidObservation)
}

/// Select the exact weighted circular median with the original tie hierarchy.
fn deterministic_weighted_circular_median(
    observations: &[EstimatorRow],
) -> Result<f64, WindRobustHierarchyError> {
    let mut normalized = observations
        .iter()
        .map(|observation| EstimatorRow {
            stable_id: observation.stable_id.clone(),
            value: wrap_180(observation.value),
            weight: observation.weight,
        })
        .collect::<Vec<_>>();
    normalized.sort_by(|left, right| {
        left.value
            .partial_cmp(&right.value)
            .unwrap_or(Ordering::Equal)
            .then_with(|| compare_text(&left.stable_id, &right.stable_id))
    });
    let mut candidates = normalized
        .iter()
        .map(|observation| observation.value)
        .collect::<Vec<_>>();
    candidates.dedup_by(|left, right| *left == *right);
    let mut best = None::<(f64, f64)>;

    // score every observed residual angle in numerical order
    for candidate in candidates {
        let mut objective = 0.0;

        // preserve stable normalized observation order for objective addition
        for observation in &normalized {
            objective += observation.weight * wrap_180(observation.value - candidate).abs();
        }

        let replace = match best {
            None => true,
            Some((best_candidate, best_objective)) => {
                objective < best_objective
                    || (objective == best_objective
                        && (candidate.abs() < best_candidate.abs()
                            || (candidate.abs() == best_candidate.abs()
                                && candidate < best_candidate)))
            }
        };

        // retain the objective, absolute-angle, then signed-angle winner
        if replace {
            best = Some((candidate, objective));
        }
    }

    best.map(|(candidate, _)| candidate)
        .ok_or(WindRobustHierarchyError::InvalidObservation)
}

/// Sort residuals into the canonical estimator material order.
fn compare_residual_observations(
    left: &AdmittedWindHierarchyObservation<'_>,
    right: &AdmittedWindHierarchyObservation<'_>,
) -> Ordering {
    left.residual
        .partial_cmp(&right.residual)
        .unwrap_or(Ordering::Equal)
        .then_with(|| compare_text(left.valid_at, right.valid_at))
        .then_with(|| left.target_lead_hours.cmp(&right.target_lead_hours))
        .then_with(|| {
            compare_text(
                left.reference_at.unwrap_or(""),
                right.reference_at.unwrap_or(""),
            )
        })
        .then_with(|| compare_text(left.stable_id, right.stable_id))
}

/// Compare admitted strings by JavaScript code-unit order.
fn compare_text(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

/// Wrap a finite angle into the signed half-open circle.
fn wrap_180(value: f64) -> f64 {
    let wrapped = (((value + 180.0) % 360.0) + 360.0) % 360.0 - 180.0;

    // normalize negative zero like the JavaScript primitive
    if wrapped == 0.0 { 0.0 } else { wrapped }
}

/// Cap a coefficient while preserving JavaScript NaN propagation.
fn cap_coefficient(coefficient: f64, minimum: f64, maximum: f64) -> f64 {
    // clamp only an ordered value below the lower bound
    if coefficient < minimum {
        return minimum;
    }

    // clamp only an ordered value above the upper bound
    if coefficient > maximum {
        return maximum;
    }

    coefficient
}

/// Return the frozen correction cap for one supported metric.
const fn correction_bounds(metric: ForecastAdjustmentMetric) -> (f64, f64) {
    // select the immutable v1 metric policy
    match metric {
        ForecastAdjustmentMetric::RelativeHumidityPercent => (-20.0, 20.0),
        ForecastAdjustmentMetric::TemperatureC => (-5.0, 5.0),
        ForecastAdjustmentMetric::WindDirectionDegrees => (-45.0, 45.0),
        ForecastAdjustmentMetric::WindGustMps => (-12.0, 12.0),
        ForecastAdjustmentMetric::WindSpeedMps => (-8.0, 8.0),
    }
}

/// Group samples by lexical season and daypart wire values.
fn group_by_season<'slice, 'data>(
    observations: &'slice [AdmittedWindHierarchyObservation<'data>],
) -> BTreeMap<String, Vec<&'slice AdmittedWindHierarchyObservation<'data>>> {
    let mut groups = BTreeMap::<String, Vec<&AdmittedWindHierarchyObservation<'_>>>::new();

    // retain every observation under its exact composite key
    for observation in observations {
        groups
            .entry(season_group_key(observation.season, observation.daypart))
            .or_default()
            .push(observation);
    }

    groups
}

/// Group samples by padded month and lexical daypart wire values.
fn group_by_month<'slice, 'data>(
    observations: &'slice [AdmittedWindHierarchyObservation<'data>],
) -> BTreeMap<String, Vec<&'slice AdmittedWindHierarchyObservation<'data>>> {
    let mut groups = BTreeMap::<String, Vec<&AdmittedWindHierarchyObservation<'_>>>::new();

    // retain every observation under its exact composite key
    for observation in observations {
        groups
            .entry(format!(
                "{:02}\0{}",
                observation.month,
                daypart_name(observation.daypart)
            ))
            .or_default()
            .push(observation);
    }

    groups
}

/// Build the original season/daypart composite key.
fn season_group_key(season: Season, daypart: Daypart) -> String {
    format!("{}\0{}", season_name(season), daypart_name(daypart))
}

/// Project one generated daypart to its canonical wire value.
const fn daypart_name(daypart: Daypart) -> &'static str {
    // preserve the generated literal vocabulary
    match daypart {
        Daypart::Afternoon => "afternoon",
        Daypart::Evening => "evening",
        Daypart::Morning => "morning",
        Daypart::Night => "night",
    }
}

/// Project one generated season to its canonical wire value.
const fn season_name(season: Season) -> &'static str {
    // preserve the generated literal vocabulary
    match season {
        Season::Autumn => "autumn",
        Season::Spring => "spring",
        Season::Summer => "summer",
        Season::Winter => "winter",
    }
}

/// Resolve the original meteorological season for a validated month.
const fn season_for_month(month: u8) -> Season {
    // retain the Northern Hemisphere meteorological calendar
    match month {
        3..=5 => Season::Spring,
        6..=8 => Season::Summer,
        9..=11 => Season::Autumn,
        _ => Season::Winter,
    }
}
