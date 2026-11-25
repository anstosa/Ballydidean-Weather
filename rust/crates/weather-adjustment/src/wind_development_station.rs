//! Station-excluded development scoring for admitted retained wind events.

use std::collections::HashSet;

use weather_contracts::{
    ForecastAdjustmentCoefficient, ForecastAdjustmentMetricBand, ForecastObservationStation,
    ForecastObservationStationKey, Nullable,
};

use crate::wind_row_evaluation::select_hierarchy_coefficient;
use crate::wind_training_events::wind_metric;
use crate::{
    AdmittedWindHierarchyObservation, NonFiniteCorrectionInput, WindBootstrapPairedEvent,
    WindRobustHierarchyError, WindTrainingEventError, WindTrainingSet, apply_capped_correction,
    fit_wind_robust_hierarchy, scalar_wind_network_actual,
};

const AUXILIARY_TRAINING_FLOOR: usize = 500;
const HELD_STATION_SCORE_FLOOR: usize = 100;

/// Already-admitted disjoint development date partitions for one fold.
#[derive(Clone, Copy, Debug)]
pub struct AdmittedWindDevelopmentFold<'a> {
    pub training_local_dates: &'a [&'a str],
    pub score_local_dates: &'a [&'a str],
}

/// One held-station observation and its two forecast predictions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RetainedWindDevelopmentPairedEvent<'a> {
    pub local_date: &'a str,
    pub event: WindBootstrapPairedEvent,
}

/// One scoreable station in original station-catalog order.
#[derive(Debug, PartialEq)]
pub struct RetainedWindDevelopmentStationScore<'station, 'data> {
    pub station: &'station ForecastObservationStation,
    pub coefficients: Vec<ForecastAdjustmentCoefficient>,
    pub paired_events: Vec<RetainedWindDevelopmentPairedEvent<'data>>,
    pub remaining_network_score_events: usize,
    pub score_matches: usize,
    pub training_matches: usize,
}

/// Ordinary numerical refusal while deriving one station-excluded fold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindDevelopmentStationError {
    Network(WindTrainingEventError),
    Hierarchy(WindRobustHierarchyError),
    Correction(NonFiniteCorrectionInput),
}

impl From<WindTrainingEventError> for WindDevelopmentStationError {
    /// Preserve the source network admission failure.
    fn from(error: WindTrainingEventError) -> Self {
        Self::Network(error)
    }
}

impl From<WindRobustHierarchyError> for WindDevelopmentStationError {
    /// Preserve the hierarchy fitter's ordinary refusal.
    fn from(error: WindRobustHierarchyError) -> Self {
        Self::Hierarchy(error)
    }
}

impl From<NonFiniteCorrectionInput> for WindDevelopmentStationError {
    /// Preserve invalid arithmetic rather than fabricating a paired value.
    fn from(error: NonFiniteCorrectionInput) -> Self {
        Self::Correction(error)
    }
}

/// Score all eligible stations for one admitted speed or gust metric-band fold.
///
/// The caller admits that both date lists are unique and disjoint, the pair is one
/// maintained speed/gust pair, and every event belongs to the same retained phase.
pub fn score_wind_development_stations<'station, 'data>(
    training: &WindTrainingSet<'data>,
    stations: &'station [ForecastObservationStation],
    pair: &ForecastAdjustmentMetricBand,
    fold: &AdmittedWindDevelopmentFold<'_>,
) -> Result<Vec<RetainedWindDevelopmentStationScore<'station, 'data>>, WindDevelopmentStationError>
{
    let training_dates: HashSet<&str> = fold.training_local_dates.iter().copied().collect();
    let score_dates: HashSet<&str> = fold.score_local_dates.iter().copied().collect();
    let mut scored = Vec::new();
    // preserve the original frozen station-catalog order
    for station in stations {
        let mut observations = Vec::new();
        let mut remaining_score_events = Vec::new();
        // partition this pair once while retaining borrowed stable identifiers
        for event in &training.events {
            // skip events owned by another metric-band pair
            if event.metric != pair.metric || event.lead_band != pair.leadBand {
                continue;
            }
            let rows = training.station_rows_at(event.valid_at_ms);
            // require some usable held-station value before either partition can match
            if !has_station_metric(rows, station.key, pair.metric) {
                continue;
            }
            // build the station-excluded auxiliary residual for a training date
            if training_dates.contains(event.local_date)
                && let Some(actual) =
                    scalar_wind_network_actual(rows, stations, pair.metric, Some(station.key))?
            {
                observations.push(AdmittedWindHierarchyObservation {
                    daypart: event.calendar.daypart,
                    month: u8::try_from(event.calendar.month).unwrap_or_default(),
                    reference_at: Some(event.reference_at),
                    residual: actual - event.raw_forecast,
                    season: event.calendar.season,
                    stable_id: &event.stable_id,
                    target_lead_hours: i64::from(event.target_lead_hours),
                    valid_at: event.valid_at,
                    weight: 1.0,
                });
            }
            // retain score dates whose remaining network is still supported
            if score_dates.contains(event.local_date)
                && scalar_wind_network_actual(rows, stations, pair.metric, Some(station.key))?
                    .is_some()
            {
                remaining_score_events.push(event);
            }
        }
        // skip fitting when either literal floor already makes this station unscoreable
        if observations.len() < AUXILIARY_TRAINING_FLOOR
            || remaining_score_events.len() < HELD_STATION_SCORE_FLOOR
        {
            continue;
        }
        let coefficients = fit_wind_robust_hierarchy(pair.metric, pair.leadBand, &observations)?;
        let remaining_network_score_events = remaining_score_events.len();
        let mut paired_events = Vec::with_capacity(remaining_score_events.len());
        // score the first matching held row through the fitted auxiliary hierarchy
        for event in remaining_score_events {
            let Some(actual) = first_station_metric(
                training.station_rows_at(event.valid_at_ms),
                station.key,
                pair.metric,
            ) else {
                continue;
            };
            let selected = select_hierarchy_coefficient(
                &coefficients,
                pair.metric,
                pair.leadBand,
                event.calendar,
            );
            let Nullable::Value(coefficient) = selected.coefficient else {
                continue;
            };
            let adjusted_prediction =
                apply_capped_correction(pair.metric, event.raw_forecast, coefficient)?;
            paired_events.push(RetainedWindDevelopmentPairedEvent {
                local_date: event.local_date,
                event: WindBootstrapPairedEvent {
                    actual,
                    adjusted_prediction,
                    raw_prediction: event.raw_forecast,
                },
            });
        }
        // retain only literally scoreable auxiliary models
        if paired_events.len() < HELD_STATION_SCORE_FLOOR {
            continue;
        }
        let score_matches = paired_events.len();
        scored.push(RetainedWindDevelopmentStationScore {
            station,
            coefficients,
            paired_events,
            remaining_network_score_events,
            score_matches,
            training_matches: observations.len(),
        });
    }
    Ok(scored)
}

/// Check whether any matching row has a nonnull maintained metric.
fn has_station_metric(
    rows: &[crate::AdmittedWindStationTrainingRow<'_>],
    station: ForecastObservationStationKey,
    metric: weather_contracts::ForecastAdjustmentMetric,
) -> bool {
    rows.iter().any(|row| {
        row.physical_station_key == station && wind_metric(row.metrics, metric).is_some()
    })
}

/// Preserve the source's first matching held-row selection.
fn first_station_metric(
    rows: &[crate::AdmittedWindStationTrainingRow<'_>],
    station: ForecastObservationStationKey,
    metric: weather_contracts::ForecastAdjustmentMetric,
) -> Option<f64> {
    rows.iter()
        .find(|row| row.physical_station_key == station)
        .and_then(|row| wind_metric(row.metrics, metric))
}
