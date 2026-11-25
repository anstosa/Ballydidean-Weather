use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
    ForecastObservationStationKey, Nullable,
};

const HOUR_MS: i64 = 3_600_000;
const TARGET_DELAY_HOURS: i64 = 7;
const LONG_WINDOW_HOURS: u8 = 72;
const SHORT_WINDOW_HOURS: i64 = 24;
const MINIMUM_NETWORK_STATIONS: usize = 3;
const NEAREST_ELIGIBLE_STATION_COUNT: u8 = 3;

/// One admitted station temperature for a single causal target hour.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedTemperatureRecentErrorStation {
    pub physical_station_key: ForecastObservationStationKey,
    pub temperature_c: Nullable<f64>,
}

/// One forecast selected causally for a single target hour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdmittedTemperatureRecentErrorForecast<'a> {
    pub key: &'a str,
    pub raw_temperature_c: f64,
    pub run_initialized_at: &'a str,
}

/// One pre-grouped causal slot after repository and calendar admission.
#[derive(Debug, Clone, Copy)]
pub struct AdmittedTemperatureRecentErrorHour<'a> {
    pub age_hours: u8,
    pub forecast: Option<AdmittedTemperatureRecentErrorForecast<'a>>,
    pub local_date: &'a str,
    pub stations: &'a [AdmittedTemperatureRecentErrorStation],
    pub valid_at: &'a str,
    pub valid_at_epoch_ms: i64,
}

/// Pre-resolved target and window times plus bounded causal evidence.
#[derive(Debug, Clone, Copy)]
pub struct AdmittedTemperatureRecentErrorInput<'a> {
    pub hours: &'a [AdmittedTemperatureRecentErrorHour<'a>],
    pub target_run_initialized_at: &'a str,
    pub target_run_initialized_at_epoch_ms: i64,
    pub window_end_valid_at: &'a str,
    pub window_end_valid_at_epoch_ms: i64,
}

/// Invalid admitted geometry or numeric evidence supplied to the reducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemperatureRecentErrorReductionError {
    DuplicateHour,
    InvalidEvidence,
    InvalidTimeGeometry,
}

impl Display for TemperatureRecentErrorReductionError {
    /// Describe one bounded reducer admission failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::DuplicateHour => "temperature recent-error hours must be unique",
            Self::InvalidEvidence => "temperature recent-error evidence is invalid",
            Self::InvalidTimeGeometry => "temperature recent-error time geometry is invalid",
        })
    }
}

impl Error for TemperatureRecentErrorReductionError {}

#[derive(Debug, Clone, Copy)]
struct SelectedTemperatureError<'a> {
    error_c: f64,
    key: &'a str,
    local_date: &'a str,
    run_initialized_at: &'a str,
    valid_at: &'a str,
    valid_at_epoch_ms: i64,
}

#[derive(Debug, Clone, Copy)]
struct StationIdentity {
    key: &'static str,
    nearest_rank: u8,
    weight: f64,
}

#[derive(Debug, Clone, Copy)]
struct WeightedStationValue {
    identity: StationIdentity,
    value: f64,
}

/// Reduce admitted causal forecast/observation slots into the canonical model state.
pub fn build_admitted_temperature_recent_error_state(
    input: AdmittedTemperatureRecentErrorInput<'_>,
) -> Result<EcmwfTemperatureCanaryRecentErrorState, TemperatureRecentErrorReductionError> {
    let expected_window_end = input
        .target_run_initialized_at_epoch_ms
        .checked_sub(TARGET_DELAY_HOURS * HOUR_MS)
        .ok_or(TemperatureRecentErrorReductionError::InvalidTimeGeometry)?;
    // retain the source's fixed seven-hour causal delay
    if input.window_end_valid_at_epoch_ms != expected_window_end {
        return Err(TemperatureRecentErrorReductionError::InvalidTimeGeometry);
    }

    let mut seen_ages = [false; LONG_WINDOW_HOURS as usize];
    let mut selected = Vec::with_capacity(input.hours.len().min(LONG_WINDOW_HOURS as usize));
    // consider only the same inclusive 72 hourly slots as the source loop
    for hour in input.hours {
        if hour.age_hours >= LONG_WINDOW_HOURS {
            continue;
        }
        let age_index = usize::from(hour.age_hours);
        // reject multiple pre-grouped rows for one source slot
        if seen_ages[age_index] {
            return Err(TemperatureRecentErrorReductionError::DuplicateHour);
        }
        seen_ages[age_index] = true;
        let expected_valid_at = input
            .window_end_valid_at_epoch_ms
            .checked_sub(i64::from(hour.age_hours) * HOUR_MS)
            .ok_or(TemperatureRecentErrorReductionError::InvalidTimeGeometry)?;
        // bind every admitted string/calendar row to its pre-resolved slot
        if hour.valid_at_epoch_ms != expected_valid_at || hour.local_date.is_empty() {
            return Err(TemperatureRecentErrorReductionError::InvalidTimeGeometry);
        }
        let Some(forecast) = hour.forecast else {
            continue;
        };
        // reject nonfinite forecasts before residual arithmetic
        if !forecast.raw_temperature_c.is_finite()
            || forecast.key.is_empty()
            || forecast.run_initialized_at.is_empty()
            || hour.valid_at.is_empty()
        {
            return Err(TemperatureRecentErrorReductionError::InvalidEvidence);
        }
        let Some(actual_temperature_c) = scalar_network_temperature(hour.stations)? else {
            continue;
        };
        let error_c = actual_temperature_c - forecast.raw_temperature_c;
        // retain only defined residuals from admitted finite inputs
        if !error_c.is_finite() {
            return Err(TemperatureRecentErrorReductionError::InvalidEvidence);
        }
        selected.push(SelectedTemperatureError {
            error_c,
            key: forecast.key,
            local_date: hour.local_date,
            run_initialized_at: forecast.run_initialized_at,
            valid_at: hour.valid_at,
            valid_at_epoch_ms: hour.valid_at_epoch_ms,
        });
    }

    selected.sort_by(|left, right| {
        left.valid_at
            .cmp(right.valid_at)
            .then_with(|| left.run_initialized_at.cmp(right.run_initialized_at))
            .then_with(|| left.key.cmp(right.key))
    });
    let short_start = input
        .window_end_valid_at_epoch_ms
        .checked_sub((SHORT_WINDOW_HOURS - 1) * HOUR_MS)
        .ok_or(TemperatureRecentErrorReductionError::InvalidTimeGeometry)?;
    let short = selected
        .iter()
        .filter(|item| item.valid_at_epoch_ms >= short_start)
        .copied()
        .collect::<Vec<_>>();
    let local_dates = selected
        .iter()
        .map(|item| item.local_date)
        .collect::<BTreeSet<_>>();
    let short_supported = short.len() >= 6;
    let long_supported = selected.len() >= 24;
    let raw_b72 = long_supported.then(|| median_errors(&selected));
    let maximum_source_run = selected.iter().map(|item| item.run_initialized_at).max();
    let maximum_valid = selected.last();

    Ok(EcmwfTemperatureCanaryRecentErrorState {
        b24C: optional_statistic(
            short_supported.then(|| clip_state_statistic(median_errors(&short))),
        ),
        b72C: optional_statistic(raw_b72.map(clip_state_statistic)),
        cohort: EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
        localDates: local_dates.len() as u64,
        mad72C: optional_statistic(raw_b72.map(|center| {
            let deviations = selected
                .iter()
                .map(|item| (item.error_c - center).abs())
                .collect::<Vec<_>>();
            median_values(&deviations).min(6.0)
        })),
        maximumSourceRunInitializedAt: optional_string(maximum_source_run),
        maximumSourceValidAt: optional_string(maximum_valid.map(|item| item.valid_at)),
        n24: short.len() as u64,
        n72: selected.len() as u64,
        sourceKeys: selected.iter().map(|item| item.key.to_owned()).collect(),
        supported: short_supported && long_supported && local_dates.len() >= 2,
        targetRunInitializedAt: input.target_run_initialized_at.to_owned(),
        windowEndValidAt: input.window_end_valid_at.to_owned(),
    })
}

/// Compute the source network target after its coverage and nearest-station gates.
fn scalar_network_temperature(
    stations: &[AdmittedTemperatureRecentErrorStation],
) -> Result<Option<f64>, TemperatureRecentErrorReductionError> {
    let mut values = stations
        .iter()
        .filter_map(|station| match station.temperature_c {
            Nullable::Value(value) => Some(WeightedStationValue {
                identity: station_identity(station.physical_station_key),
                value,
            }),
            Nullable::Null => None,
        })
        .collect::<Vec<_>>();
    // return missing before normalization when physical coverage fails
    if values.len() < MINIMUM_NETWORK_STATIONS
        || !values
            .iter()
            .any(|station| station.identity.nearest_rank <= NEAREST_ELIGIBLE_STATION_COUNT)
    {
        return Ok(None);
    }
    values.sort_by(|left, right| left.identity.key.cmp(right.identity.key));
    let mut identities = BTreeSet::new();
    let mut total_weight = 0.0;
    // validate and sum in lexical station-key order
    for station in &values {
        if !station.value.is_finite() || !identities.insert(station.identity.key) {
            return Err(TemperatureRecentErrorReductionError::InvalidEvidence);
        }
        total_weight += station.identity.weight;
    }
    // retain the strictly positive frozen station mass
    if !total_weight.is_finite() || total_weight <= 0.0 {
        return Err(TemperatureRecentErrorReductionError::InvalidEvidence);
    }
    Ok(Some(deterministic_weighted_median(&values, total_weight)))
}

/// Compute the exact strict-half weighted median with stable station-key ties.
fn deterministic_weighted_median(values: &[WeightedStationValue], total_weight: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| {
        left.value
            .partial_cmp(&right.value)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.identity.key.cmp(right.identity.key))
    });
    let mut distinct = Vec::<(f64, f64)>::with_capacity(sorted.len());
    // combine exact equal values in source order
    for station in sorted {
        let normalized_weight = station.identity.weight / total_weight;
        // add this station to the existing exact-value cell when present
        if let Some(last) = distinct.last_mut()
            && last.0 == station.value
        {
            last.1 += normalized_weight;
        } else {
            distinct.push((station.value, normalized_weight));
        }
    }
    let mut cumulative_weight = 0.0;
    // return the first strict half crossing or exact midpoint tie
    for (index, (value, weight)) in distinct.iter().enumerate() {
        cumulative_weight += weight;
        if cumulative_weight > 0.5 {
            return *value;
        }
        // average the next greater value at an exact half-mass tie
        if cumulative_weight == 0.5 {
            return distinct
                .get(index + 1)
                .map_or(*value, |next| (*value + next.0) / 2.0);
        }
    }
    distinct.last().map_or(f64::NAN, |last| last.0)
}

/// Resolve one generated station key to its frozen source rank and weight.
fn station_identity(key: ForecastObservationStationKey) -> StationIdentity {
    match key {
        ForecastObservationStationKey::AmbientMaxweather => StationIdentity {
            key: "ambient-maxweather",
            nearest_rank: 7,
            weight: 0.740_663_912_119_109,
        },
        ForecastObservationStationKey::AmbientMerlin => StationIdentity {
            key: "ambient-merlin",
            nearest_rank: 3,
            weight: 0.828_203_973_166_963,
        },
        ForecastObservationStationKey::BallydideanEcowitt => StationIdentity {
            key: "ballydidean-ecowitt",
            nearest_rank: 1,
            weight: 1.0,
        },
        ForecastObservationStationKey::NetatmoNearby => StationIdentity {
            key: "netatmo-nearby",
            nearest_rank: 11,
            weight: 0.532_051_312_934_748_6,
        },
        ForecastObservationStationKey::Tempest126537 => StationIdentity {
            key: "tempest-126537",
            nearest_rank: 8,
            weight: 0.671_559_608_319_100_8,
        },
        ForecastObservationStationKey::Tempest168853 => StationIdentity {
            key: "tempest-168853",
            nearest_rank: 6,
            weight: 0.775_136_628_203_077,
        },
        ForecastObservationStationKey::Tempest201058 => StationIdentity {
            key: "tempest-201058",
            nearest_rank: 9,
            weight: 0.670_592_564_378_282,
        },
        ForecastObservationStationKey::Tempest203055 => StationIdentity {
            key: "tempest-203055",
            nearest_rank: 10,
            weight: 0.594_717_803_370_693_7,
        },
        ForecastObservationStationKey::Tempest225947 => StationIdentity {
            key: "tempest-225947",
            nearest_rank: 4,
            weight: 0.819_043_331_232_708_2,
        },
        ForecastObservationStationKey::Tempest38270 => StationIdentity {
            key: "tempest-38270",
            nearest_rank: 5,
            weight: 0.778_491_831_165_954_9,
        },
        ForecastObservationStationKey::Tempest64255 => StationIdentity {
            key: "tempest-64255",
            nearest_rank: 2,
            weight: 0.836_755_263_292_231_6,
        },
    }
}

/// Compute the source's ordinary numeric median over one nonempty slice.
fn median_errors(values: &[SelectedTemperatureError<'_>]) -> f64 {
    median_values(&values.iter().map(|item| item.error_c).collect::<Vec<_>>())
}

/// Compute one numeric median with the same even-sample average.
fn median_values(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let middle = sorted.len() / 2;
    // average both center values for an even sample
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

/// Cap one signed state statistic to the source range.
fn clip_state_statistic(value: f64) -> f64 {
    value.clamp(-6.0, 6.0)
}

/// Translate one optional finite statistic to generated nullable storage.
fn optional_statistic(value: Option<f64>) -> Nullable<f64> {
    match value {
        Some(value) => Nullable::Value(value),
        None => Nullable::Null,
    }
}

/// Translate one borrowed optional string to generated nullable storage.
fn optional_string(value: Option<&str>) -> Nullable<String> {
    match value {
        Some(value) => Nullable::Value(value.to_owned()),
        None => Nullable::Null,
    }
}
