//! Build retained live-v4 wind events from admitted historical forecast and station rows.

use std::collections::BTreeMap;

use weather_contracts::{
    ForecastAdjustmentCoefficient, ForecastAdjustmentCoefficientSeasonEnum,
    ForecastAdjustmentMetric, ForecastAdjustmentMetricBand, ForecastLeadBandKey,
    ForecastObservationStation, ForecastObservationStationKey, Nullable, SanitizedTrainingMetrics,
};

use crate::{
    AdmittedWindHierarchyObservation, AdmittedWindRowCalendar, WindRobustHierarchyError,
    fit_wind_robust_hierarchy,
};

const HOUR_MILLISECONDS: f64 = 3_600_000.0;
const MAINTENANCE_METRICS: [ForecastAdjustmentMetric; 2] = [
    ForecastAdjustmentMetric::WindGustMps,
    ForecastAdjustmentMetric::WindSpeedMps,
];

/// Admitted live-v4 material from the verified final-training phase, before jitter selection.
#[derive(Clone, Copy, Debug)]
pub struct AdmittedWindForecastTrainingRow<'a> {
    pub content_sha256: &'a str,
    pub metrics: &'a SanitizedTrainingMetrics,
    pub calendar: AdmittedWindRowCalendar,
    pub local_date: &'a str,
    pub reference_at: &'a str,
    pub reference_at_ms: i64,
    pub target_lead_hours: u16,
    pub valid_at: &'a str,
    pub valid_at_ms: i64,
}

/// One sanitized physical station at the matching instant.
#[derive(Clone, Copy, Debug)]
pub struct AdmittedWindStationTrainingRow<'a> {
    pub physical_station_key: ForecastObservationStationKey,
    pub valid_at_ms: i64,
    pub metrics: &'a SanitizedTrainingMetrics,
}

/// Preserve the original phase row index even for unselected cohorts.
#[derive(Clone, Copy, Debug)]
pub enum AdmittedWindTrainingRow<'a> {
    Retrieval(AdmittedWindForecastTrainingRow<'a>),
    Station(AdmittedWindStationTrainingRow<'a>),
    Other,
}

/// One equal-mass operational speed/gust event after original jitter and coverage selection.
#[derive(Clone, Debug, PartialEq)]
pub struct WindTrainingEvent<'a> {
    pub actual: f64,
    pub calendar: AdmittedWindRowCalendar,
    pub lead_band: ForecastLeadBandKey,
    pub local_date: &'a str,
    pub metric: ForecastAdjustmentMetric,
    pub raw_forecast: f64,
    pub reference_at: &'a str,
    pub stable_id: String,
    pub target_lead_hours: u16,
    pub valid_at: &'a str,
    pub valid_at_ms: i64,
}

/// Selected events and a shared station index for later station-excluded scoring.
#[derive(Debug, Default)]
pub struct WindTrainingSet<'a> {
    pub events: Vec<WindTrainingEvent<'a>>,
    stations_by_instant: BTreeMap<i64, Vec<AdmittedWindStationTrainingRow<'a>>>,
}

impl<'a> WindTrainingSet<'a> {
    /// Borrow matched physical rows without duplicating station data per lead.
    pub fn station_rows_at(&self, valid_at_ms: i64) -> &[AdmittedWindStationTrainingRow<'a>] {
        self.stations_by_instant
            .get(&valid_at_ms)
            .map_or(&[], Vec::as_slice)
    }
}

/// Ordinary refusal for invalid admitted data rather than silently dropping it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindTrainingEventError {
    InvalidForecastProvenance,
    InvalidForecastMetric,
    InvalidStationValues,
}

/// A final-training phase without any attached confirmation or publication authority.
#[derive(Debug)]
pub enum WindMaintenanceTrainingPreparation<'a> {
    Insufficient { failed_gates: Vec<String> },
    Ready(ReadyWindMaintenanceTraining<'a>),
}

/// Earlier support gates admitted this phase for development fitting only.
#[derive(Debug)]
pub struct ReadyWindMaintenanceTraining<'a>(WindTrainingSet<'a>);

impl<'a> ReadyWindMaintenanceTraining<'a> {
    /// Borrow the original events and station index for development folds.
    pub const fn events(&self) -> &WindTrainingSet<'a> {
        &self.0
    }

    /// Fit one selected final-training pair without accessing confirmation data.
    pub fn fit_pair(
        &self,
        pair: &ForecastAdjustmentMetricBand,
    ) -> Result<Vec<ForecastAdjustmentCoefficient>, WindRobustHierarchyError> {
        fit_wind_training_event_pair(&self.0.events, pair)
    }
}

/// Apply the original support gates before running any development or final-training fitter.
pub fn prepare_wind_maintenance_training<'a>(
    rows: &[AdmittedWindTrainingRow<'a>],
    stations: &[ForecastObservationStation],
    expected_dates: &[(&str, ForecastAdjustmentCoefficientSeasonEnum)],
) -> Result<WindMaintenanceTrainingPreparation<'a>, WindTrainingEventError> {
    let training = build_retained_wind_training_events(rows, stations)?;
    let failed_gates =
        crate::retained_wind_sufficiency_failures(&training, stations, expected_dates);
    // preserve the literal support refusal rather than fitting a partial model
    if failed_gates.is_empty() {
        Ok(WindMaintenanceTrainingPreparation::Ready(
            ReadyWindMaintenanceTraining(training),
        ))
    } else {
        Ok(WindMaintenanceTrainingPreparation::Insufficient { failed_gates })
    }
}

#[derive(Clone, Copy)]
struct Candidate<'a> {
    row: AdmittedWindForecastTrainingRow<'a>,
    index: usize,
    distance: f64,
    band: ForecastLeadBandKey,
    raw: f64,
}

/// Build the original paired operational network events without rereading raw data.
pub fn build_retained_wind_training_events<'a>(
    rows: &[AdmittedWindTrainingRow<'a>],
    stations: &[ForecastObservationStation],
) -> Result<WindTrainingSet<'a>, WindTrainingEventError> {
    let mut result = WindTrainingSet::default();
    let mut selected: BTreeMap<(i64, usize, u16), Candidate<'a>> = BTreeMap::new();
    // index exact physical hours and select only the immutable live-v4 cohort
    for (index, row) in rows.iter().enumerate() {
        match row {
            AdmittedWindTrainingRow::Station(row) => {
                result
                    .stations_by_instant
                    .entry(row.valid_at_ms)
                    .or_default()
                    .push(*row);
            }
            AdmittedWindTrainingRow::Retrieval(row) => {
                // preserve all present speed/gust values before removing the disabled gust pair
                for (metric_index, metric) in MAINTENANCE_METRICS.into_iter().enumerate() {
                    let Some(raw) = wind_metric(row.metrics, metric) else {
                        continue;
                    };
                    let (band, distance) = forecast_priority(row)?;
                    // retain the original finite numeric admission for present forecasts
                    if !raw.is_finite() {
                        return Err(WindTrainingEventError::InvalidForecastMetric);
                    }
                    let candidate = Candidate {
                        row: *row,
                        index,
                        distance,
                        band,
                        raw,
                    };
                    let key = (row.valid_at_ms, metric_index, row.target_lead_hours);
                    selected
                        .entry(key)
                        .and_modify(|current| {
                            // replace only for the original distance, reference, and stable-id ordering
                            if candidate_precedes(&candidate, current) {
                                *current = candidate;
                            }
                        })
                        .or_insert(candidate);
                }
            }
            AdmittedWindTrainingRow::Other => {}
        }
    }
    let mut last_instant_metric = None;
    // map order is original valid instant, metric name, then target lead within live-v4
    for ((instant, metric_index, _), candidate) in selected {
        let metric = MAINTENANCE_METRICS[metric_index];
        let key = (instant, metric_index);
        // reuse exactly the same network result across target leads at one physical hour
        let actual = match last_instant_metric {
            Some((previous, cached)) if previous == key => cached,
            _ => {
                let actual = scalar_wind_network_actual(
                    result.station_rows_at(instant),
                    stations,
                    metric,
                    None,
                )?;
                last_instant_metric = Some((key, actual));
                actual
            }
        };
        let Some(actual) = actual else {
            continue;
        };
        // remove only the originally disabled recurring gust pair
        if metric == ForecastAdjustmentMetric::WindGustMps
            && candidate.band == ForecastLeadBandKey::Value049072
        {
            continue;
        }
        let metric_name = if metric == ForecastAdjustmentMetric::WindGustMps {
            "windGustMps"
        } else {
            "windSpeedMps"
        };
        let row = candidate.row;
        result.events.push(WindTrainingEvent {
            actual,
            calendar: row.calendar,
            lead_band: candidate.band,
            local_date: row.local_date,
            metric,
            raw_forecast: candidate.raw,
            reference_at: row.reference_at,
            stable_id: format!("{}:{}:{metric_name}", row.content_sha256, candidate.index),
            target_lead_hours: row.target_lead_hours,
            valid_at: row.valid_at,
            valid_at_ms: row.valid_at_ms,
        });
    }
    Ok(result)
}

/// Fit one original hierarchy from equal-mass network events for the specified pair.
pub fn fit_wind_training_event_pair(
    events: &[WindTrainingEvent<'_>],
    pair: &ForecastAdjustmentMetricBand,
) -> Result<Vec<ForecastAdjustmentCoefficient>, WindRobustHierarchyError> {
    let observations = events
        .iter()
        .filter(|event| event.metric == pair.metric && event.lead_band == pair.leadBand)
        .map(|event| {
            // form the original scalar residual after admitted network coverage
            AdmittedWindHierarchyObservation {
                daypart: event.calendar.daypart,
                month: u8::try_from(event.calendar.month).unwrap_or_default(),
                reference_at: Some(event.reference_at),
                residual: event.actual - event.raw_forecast,
                season: event.calendar.season,
                stable_id: &event.stable_id,
                target_lead_hours: i64::from(event.target_lead_hours),
                valid_at: event.valid_at,
                weight: 1.0,
            }
        })
        .collect::<Vec<_>>();
    fit_wind_robust_hierarchy(pair.metric, pair.leadBand, &observations)
}

/// Check the original retrieval geometry and derive its fixed lead band.
fn forecast_priority(
    row: &AdmittedWindForecastTrainingRow<'_>,
) -> Result<(ForecastLeadBandKey, f64), WindTrainingEventError> {
    use WindTrainingEventError::InvalidForecastProvenance;
    let latency = row
        .valid_at_ms
        .checked_sub(row.reference_at_ms)
        .ok_or(InvalidForecastProvenance)?;
    let lead = f64::from(row.target_lead_hours);
    let continuous = latency as f64 / HOUR_MILLISECONDS;
    // require the source's exact ceil-selected live-v4 lead identity
    if !(1..=168).contains(&row.target_lead_hours) || latency < 0 || continuous.ceil() != lead {
        return Err(InvalidForecastProvenance);
    }
    let band = ForecastLeadBandKey::WIRE_VALUES[usize::from((row.target_lead_hours - 1) / 24)].1;
    Ok((band, (continuous - lead).abs()))
}

/// Match the original jitter tie using admitted ISO clocks and ASCII digest/index components.
fn candidate_precedes(left: &Candidate<'_>, right: &Candidate<'_>) -> bool {
    left.distance
        .total_cmp(&right.distance)
        .then_with(|| left.row.reference_at.cmp(right.row.reference_at))
        .then_with(|| left.row.content_sha256.cmp(right.row.content_sha256))
        // the original ICU comparison orders the delimiter before a longer decimal index
        .then_with(|| left.index.to_string().cmp(&right.index.to_string()))
        .is_lt()
}

/// Read an original maintained scalar from the generated metric contract.
pub(crate) fn wind_metric(
    metrics: &SanitizedTrainingMetrics,
    metric: ForecastAdjustmentMetric,
) -> Option<f64> {
    let value = match metric {
        ForecastAdjustmentMetric::WindGustMps => &metrics.windGustMps,
        ForecastAdjustmentMetric::WindSpeedMps => &metrics.windSpeedMps,
        _ => return None,
    };
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(*value),
    }
}

/// Compute the original spatial scalar median with an optional excluded station.
pub fn scalar_wind_network_actual(
    rows: &[AdmittedWindStationTrainingRow<'_>],
    stations: &[ForecastObservationStation],
    metric: ForecastAdjustmentMetric,
    excluded: Option<ForecastObservationStationKey>,
) -> Result<Option<f64>, WindTrainingEventError> {
    let mut eligible = Vec::with_capacity(stations.len().min(rows.len()));
    // retain only configured, eligible, present observations with their original weights
    for row in rows {
        let Some(station) = stations
            .iter()
            .find(|station| station.key == row.physical_station_key)
        else {
            continue;
        };
        let Some(value) = wind_metric(row.metrics, metric) else {
            continue;
        };
        // omit the requested physical station before validating network support
        if excluded == Some(station.key) || !station.eligibleMetrics.contains(&metric) {
            continue;
        }
        eligible.push((station, value));
    }
    // require three stations and one nearest-three station before numeric validation
    if eligible.len() < 3 || !eligible.iter().any(|(station, _)| station.nearestRank <= 3) {
        return Ok(None);
    }
    eligible.sort_by_key(|(station, _)| station_key(station.key));
    let mut previous = None;
    let mut total_weight = 0.0;
    // normalize in original station-key order before median-value ordering
    for (station, value) in &eligible {
        // preserve duplicate detection and finite positive weight requirements
        if previous == Some(station.key)
            || station.nearestRank == 0
            || !value.is_finite()
            || !station.unnormalizedSpatialWeight.is_finite()
            || station.unnormalizedSpatialWeight <= 0.0
        {
            return Err(WindTrainingEventError::InvalidStationValues);
        }
        previous = Some(station.key);
        total_weight += station.unnormalizedSpatialWeight;
    }
    // value ties retain station-key order rather than imposing a different numeric addition order
    eligible.sort_by(|(left, a), (right, b)| {
        a.partial_cmp(b)
            .expect("admitted finite station values")
            .then_with(|| station_key(left.key).cmp(station_key(right.key)))
    });
    let mut normalized_total = 0.0;
    let mut distinct: Vec<(f64, f64)> = Vec::with_capacity(eligible.len());
    // accumulate equal scalar values in original estimator order
    for (station, value) in eligible {
        let weight = station.unnormalizedSpatialWeight / total_weight;
        normalized_total += weight;
        // group exact numeric equality before finding the half-weight crossing
        if let Some((last, prior)) = distinct.last_mut()
            && *last == value
        {
            *prior += weight;
        } else {
            distinct.push((value, weight));
        }
    }
    // refuse a zero or nonfinite normalized estimator
    if !normalized_total.is_finite() || normalized_total <= 0.0 {
        return Err(WindTrainingEventError::InvalidStationValues);
    }
    let mut accumulated = 0.0;
    let half = normalized_total / 2.0;
    // return the strict crossing or the original midpoint exact tie
    for (index, (value, weight)) in distinct.iter().copied().enumerate() {
        accumulated += weight;
        // select the first distinct value above one half
        if accumulated > half {
            return Ok(Some(value));
        }
        // average the next higher value only on exact half-weight ties
        if accumulated == half {
            return Ok(Some(
                distinct
                    .get(index + 1)
                    .map_or(value, |(next, _)| (value + next) / 2.0),
            ));
        }
    }
    Ok(distinct.last().map(|(value, _)| *value))
}

/// Borrow the original ASCII physical-station identity for stable ordering.
pub(crate) fn station_key(value: ForecastObservationStationKey) -> &'static str {
    ForecastObservationStationKey::WIRE_VALUES
        .iter()
        .find_map(|(wire, key)| (*key == value).then_some(*wire))
        .expect("generated station key")
}
