use std::cmp::Ordering;

use weather_contracts::{Nullable, RainWindForecastHour};

use crate::RainHurdleWindInferenceError;

const FEATURE_COUNT: usize = 107;
const FORECAST_HOUR_COUNT: usize = 48;
const STATION_COUNT: usize = 12;
const DECISION_DELAY_HOURS: usize = 8;
const STATION_IDS: [u64; STATION_COUNT] = [
    64_255, 225_947, 38_270, 168_853, 126_537, 201_058, 203_055, 66_270, 34_768, 88_159, 126_197,
    27_140,
];

/// Exact UTC seasonal harmonics already resolved by the time owner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdmittedRainHurdleWindCalendar {
    pub annual_cos: f64,
    pub annual_sin: f64,
    pub daily_cos: f64,
    pub daily_sin: f64,
}

/// Fixed-station values already joined to the six decision-time lags.
#[derive(Debug, Clone, Copy)]
pub struct AdmittedRainHurdleWindStations<'a> {
    pub precipitation_by_lag: [&'a [Nullable<f64>; STATION_COUNT]; 6],
    pub station_weights: &'a [f64; STATION_COUNT],
    pub temperature_lag1: &'a [Nullable<f64>; STATION_COUNT],
}

/// Pure feature-builder inputs after receipt-time and hourly-key admission.
#[derive(Debug, Clone, Copy)]
pub struct AdmittedRainHurdleWindFeatureInput<'a> {
    pub calendar: AdmittedRainHurdleWindCalendar,
    pub current_hours: &'a [RainWindForecastHour; FORECAST_HOUR_COUNT],
    pub model_lead_hours: u8,
    pub prior_12h_hours: Option<&'a [RainWindForecastHour; FORECAST_HOUR_COUNT]>,
    pub prior_6h_hours: Option<&'a [RainWindForecastHour; FORECAST_HOUR_COUNT]>,
    pub stations: AdmittedRainHurdleWindStations<'a>,
}

#[derive(Debug, Clone, Copy)]
enum ForecastField {
    CloudCover,
    Precipitation,
    Pressure,
    RelativeHumidity,
    Temperature,
    WindDirection,
    WindSpeed,
}

struct FeatureWriter {
    length: usize,
    values: [f64; FEATURE_COUNT],
}

impl FeatureWriter {
    /// Create one empty fixed-capacity feature row.
    fn new() -> Self {
        Self {
            length: 0,
            values: [0.0; FEATURE_COUNT],
        }
    }

    /// Append one feature while preserving source order.
    fn push(&mut self, value: f64) -> Result<(), RainHurdleWindInferenceError> {
        let Some(slot) = self.values.get_mut(self.length) else {
            return Err(RainHurdleWindInferenceError::InvalidFeatureVector);
        };
        *slot = value;
        self.length += 1;
        Ok(())
    }

    /// Append one ordered feature slice.
    fn extend(&mut self, values: &[f64]) -> Result<(), RainHurdleWindInferenceError> {
        // preserve every source slice element without heap allocation
        for value in values {
            self.push(*value)?;
        }
        Ok(())
    }

    /// Convert the completed binary64 row through JavaScript Float32Array semantics.
    fn finish(self) -> Result<[f32; FEATURE_COUNT], RainHurdleWindInferenceError> {
        // allow NaN missing branches but reject wrong geometry and infinities
        if self.length != FEATURE_COUNT || self.values.iter().any(|value| value.is_infinite()) {
            return Err(RainHurdleWindInferenceError::InvalidFeatureVector);
        }
        Ok(std::array::from_fn(|index| self.values[index] as f32))
    }
}

/// Reproduce the 107 frozen forecast, station, pressure, vintage, and wind features.
pub fn build_admitted_rain_hurdle_wind_features(
    input: AdmittedRainHurdleWindFeatureInput<'_>,
) -> Result<[f32; FEATURE_COUNT], RainHurdleWindInferenceError> {
    // retain the trained model-lead scope before deriving native lead indices
    if !(1..=23).contains(&input.model_lead_hours) {
        return Err(RainHurdleWindInferenceError::InvalidFeatureVector);
    }
    // require finite admitted harmonics and fixed positive station weights
    if [
        input.calendar.annual_sin,
        input.calendar.annual_cos,
        input.calendar.daily_sin,
        input.calendar.daily_cos,
    ]
    .iter()
    .any(|value| !value.is_finite())
        || input
            .stations
            .station_weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
    {
        return Err(RainHurdleWindInferenceError::InvalidFeatureVector);
    }
    validate_forecast_hours(input.current_hours)?;
    // validate only present admitted prior cycles
    if let Some(hours) = input.prior_6h_hours {
        validate_forecast_hours(hours)?;
    }
    if let Some(hours) = input.prior_12h_hours {
        validate_forecast_hours(hours)?;
    }
    validate_station_values(&input.stations)?;

    let lead = DECISION_DELAY_HOURS + usize::from(input.model_lead_hours);
    let rain: [f64; FORECAST_HOUR_COUNT] = std::array::from_fn(|index| {
        source(input.current_hours, index + 1, ForecastField::Precipitation)
    });
    let raw = rain[lead - 1];
    let latest = nullable_station_values(input.stations.precipitation_by_lag[0]);
    let latest_mean = network_summary(&latest, input.stations.station_weights)[0];
    let observed_temperature = network_temperature(
        &nullable_station_values(input.stations.temperature_lag1),
        input.stations.station_weights,
    );
    let mut writer = FeatureWriter::new();
    writer.extend(&[
        f64::from(input.model_lead_hours),
        input.calendar.annual_sin,
        input.calendar.annual_cos,
        input.calendar.daily_sin,
        input.calendar.daily_cos,
        raw,
        raw.ln_1p(),
        mean(&rain[lead - 2..lead + 1]),
        maximum(&rain[lead - 4..lead + 3]),
        sum(&rain[lead - 1..lead + 5]),
        sum(&rain[8..32]),
        source(input.current_hours, lead, ForecastField::Temperature),
        source(input.current_hours, lead, ForecastField::RelativeHumidity),
        source(input.current_hours, lead, ForecastField::WindSpeed),
        source(input.current_hours, lead, ForecastField::CloudCover),
        observed_temperature,
        latest_mean - rain[6],
    ])?;

    // append six fixed station network summaries in source lag order
    for values in input.stations.precipitation_by_lag {
        writer.extend(&network_summary(
            &nullable_station_values(values),
            input.stations.station_weights,
        ))?;
    }
    // append each station's native values for the 1h, 3h, and 6h lags
    for lag_index in [0, 2, 3] {
        writer.extend(&nullable_station_values(
            input.stations.precipitation_by_lag[lag_index],
        ))?;
    }

    let pressure: [f64; FORECAST_HOUR_COUNT] = std::array::from_fn(|index| {
        source(input.current_hours, index + 1, ForecastField::Pressure)
    });
    let pressure_at = pressure[lead - 1];
    let pressure_window = &pressure[lead - 4..lead + 3];
    writer.extend(&[
        pressure_at,
        pressure_at - pressure[lead - 4],
        pressure_at - pressure[lead - 7],
        pressure[lead + 5] - pressure_at,
        maximum(pressure_window) - minimum(pressure_window),
        pressure_at - pressure[6],
    ])?;

    let mut previous = [f64::NAN; 2];
    let mut prior_means = [f64::NAN; 2];
    // preserve exact 6h then 12h vintage ordering and missing cycles
    for (index, (lag, older)) in [
        (6_usize, input.prior_6h_hours),
        (12_usize, input.prior_12h_hours),
    ]
    .into_iter()
    .enumerate()
    {
        let old_lead = lead + lag;
        // leave both vintage features missing when that causal run is absent
        if let Some(older) = older {
            previous[index] = source(older, old_lead, ForecastField::Precipitation);
            prior_means[index] = mean(&[
                source(older, old_lead - 1, ForecastField::Precipitation),
                source(older, old_lead, ForecastField::Precipitation),
                source(older, old_lead + 1, ForecastField::Precipitation),
            ]);
        }
    }
    let (vintages, vintage_count) = finite_vintages(raw, previous);
    let vintage_slice = &vintages[..vintage_count];
    let vintage_mean = mean(vintage_slice);
    let vintage_variance = mean_squared_difference(vintage_slice, vintage_mean);
    writer.extend(&[
        previous[0],
        previous[1],
        raw - previous[0],
        raw - previous[1],
        prior_means[0],
        prior_means[1],
        vintage_mean,
        vintage_variance.sqrt(),
        maximum(vintage_slice) - minimum(vintage_slice),
        mean_boolean(vintage_slice, 0.1),
        mean_boolean(vintage_slice, 1.0),
        vintage_count as f64,
    ])?;

    // append same-initialization humidity, cloud, and wind-speed tendencies
    for field in [
        ForecastField::RelativeHumidity,
        ForecastField::CloudCover,
        ForecastField::WindSpeed,
    ] {
        let current = source(input.current_hours, lead, field);
        writer.extend(&[
            current - source(input.current_hours, lead - 3, field),
            source(input.current_hours, lead + 3, field) - current,
        ])?;
    }
    let past = wind_components(input.current_hours, lead - 3);
    let at = wind_components(input.current_hours, lead);
    let future = wind_components(input.current_hours, lead + 3);
    writer.extend(&[
        at[0],
        at[1],
        at[0] - past[0],
        at[1] - past[1],
        future[0] - at[0],
        future[1] - at[1],
    ])?;
    writer.finish()
}

/// Validate generated forecast rows before pure feature projection.
fn validate_forecast_hours(
    hours: &[RainWindForecastHour; FORECAST_HOUR_COUNT],
) -> Result<(), RainHurdleWindInferenceError> {
    // retain exact one-based lead ordering and finite present values
    for (index, hour) in hours.iter().enumerate() {
        if hour.leadHours != (index + 1) as f64
            || forecast_fields(hour).iter().any(nullable_nonfinite)
        {
            return Err(RainHurdleWindInferenceError::InvalidFeatureVector);
        }
    }
    Ok(())
}

/// Return every nullable physical forecast field for validation.
fn forecast_fields(hour: &RainWindForecastHour) -> [&Nullable<f64>; 7] {
    [
        &hour.precipitationMm,
        &hour.temperatureC,
        &hour.relativeHumidityPercent,
        &hour.cloudCoverPercent,
        &hour.pressureHpa,
        &hour.windSpeedMps,
        &hour.windDirectionDegrees,
    ]
}

/// Reject a nonfinite present generated value while retaining null.
fn nullable_nonfinite(value: &&Nullable<f64>) -> bool {
    matches!(value, Nullable::Value(value) if !value.is_finite())
}

/// Validate every admitted station value as null or finite.
fn validate_station_values(
    stations: &AdmittedRainHurdleWindStations<'_>,
) -> Result<(), RainHurdleWindInferenceError> {
    // check every precipitation lag in fixed station order
    for values in stations.precipitation_by_lag {
        if values.iter().any(|value| nullable_nonfinite(&value)) {
            return Err(RainHurdleWindInferenceError::InvalidFeatureVector);
        }
    }
    // apply the same present-value rule to lag-one station temperature
    if stations
        .temperature_lag1
        .iter()
        .any(|value| nullable_nonfinite(&value))
    {
        return Err(RainHurdleWindInferenceError::InvalidFeatureVector);
    }
    Ok(())
}

/// Read one one-based forecast field with null represented by NaN.
fn source(
    hours: &[RainWindForecastHour; FORECAST_HOUR_COUNT],
    lead: usize,
    field: ForecastField,
) -> f64 {
    let hour = &hours[lead - 1];
    let value = match field {
        ForecastField::CloudCover => &hour.cloudCoverPercent,
        ForecastField::Precipitation => &hour.precipitationMm,
        ForecastField::Pressure => &hour.pressureHpa,
        ForecastField::RelativeHumidity => &hour.relativeHumidityPercent,
        ForecastField::Temperature => &hour.temperatureC,
        ForecastField::WindDirection => &hour.windDirectionDegrees,
        ForecastField::WindSpeed => &hour.windSpeedMps,
    };
    nullable_value(value)
}

/// Translate one generated nullable into the source's NaN missing sentinel.
fn nullable_value(value: &Nullable<f64>) -> f64 {
    // preserve explicit null rather than treating it as zero
    match value {
        Nullable::Null => f64::NAN,
        Nullable::Value(value) => *value,
    }
}

/// Copy one fixed station row into its numerical missing representation.
fn nullable_station_values(values: &[Nullable<f64>; STATION_COUNT]) -> [f64; STATION_COUNT] {
    std::array::from_fn(|index| nullable_value(&values[index]))
}

/// Compute weighted rain mean, wet fraction, maximum, and support.
fn network_summary(values: &[f64; STATION_COUNT], weights: &[f64; STATION_COUNT]) -> [f64; 4] {
    let mut weighted_amount = 0.0;
    let mut weighted_wet = 0.0;
    let mut weight_total = 0.0;
    let mut maximum = f64::NEG_INFINITY;
    let mut support = 0_usize;
    // accumulate only available station values in catalog order
    for index in 0..STATION_COUNT {
        let value = values[index];
        // preserve missing gauge exclusion from every aggregate
        if value.is_finite() {
            let weight = weights[index];
            weighted_amount += value * weight;
            weighted_wet += boolean_number(value >= 0.1) * weight;
            weight_total += weight;
            maximum = maximum.max(value);
            support += 1;
        }
    }
    // retain the all-missing summary and literal zero support
    if support == 0 {
        return [f64::NAN, f64::NAN, f64::NAN, 0.0];
    }
    [
        weighted_amount / weight_total,
        weighted_wet / weight_total,
        maximum,
        support as f64,
    ]
}

/// Compute the weighted station-temperature median with source tie ordering.
fn network_temperature(values: &[f64; STATION_COUNT], weights: &[f64; STATION_COUNT]) -> f64 {
    let mut available = Vec::with_capacity(STATION_COUNT);
    // retain station identity for deterministic equal-value ordering
    for index in 0..STATION_COUNT {
        // skip missing temperatures without inventing a station value
        if values[index].is_finite() {
            available.push((values[index], weights[index], STATION_IDS[index]));
        }
    }
    // preserve the three-station and near-site support gate
    if available.len() < 3 || !values[..3].iter().any(|value| value.is_finite()) {
        return f64::NAN;
    }
    available.sort_by(|left, right| {
        left.0
            .partial_cmp(&right.0)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.2.cmp(&right.2))
    });
    let half = available.iter().map(|item| item.1).sum::<f64>() / 2.0;
    let mut cumulative = 0.0;
    // return the first ordered value crossing half the available weight
    for (value, weight, _) in available {
        cumulative += weight;
        // preserve the inclusive weighted-median boundary
        if cumulative >= half {
            return value;
        }
    }
    f64::NAN
}

/// Preserve JavaScript reduce mean including NaN propagation.
fn mean(values: &[f64]) -> f64 {
    sum(values) / values.len() as f64
}

/// Preserve source-order floating-point summation.
fn sum(values: &[f64]) -> f64 {
    let mut total = 0.0;
    // retain JavaScript reduce left-to-right addition
    for value in values {
        total += value;
    }
    total
}

/// Preserve NaN propagation for a source maximum.
fn maximum(values: &[f64]) -> f64 {
    // reject the whole window when any source member is missing
    if values.iter().any(|value| value.is_nan()) {
        return f64::NAN;
    }
    values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

/// Preserve JavaScript Math.min NaN propagation.
fn minimum(values: &[f64]) -> f64 {
    // reject the whole window when any source member is missing
    if values.iter().any(|value| value.is_nan()) {
        return f64::NAN;
    }
    values.iter().copied().fold(f64::INFINITY, f64::min)
}

/// Retain only finite raw and earlier-cycle vintage values.
fn finite_vintages(raw: f64, previous: [f64; 2]) -> ([f64; 3], usize) {
    let mut values = [0.0; 3];
    let mut length = 0;
    // preserve raw then 6h then 12h vintage ordering
    for value in [raw, previous[0], previous[1]] {
        // omit only nonfinite vintage members
        if value.is_finite() {
            values[length] = value;
            length += 1;
        }
    }
    (values, length)
}

/// Compute mean squared difference in source order.
fn mean_squared_difference(values: &[f64], center: f64) -> f64 {
    let mut total = 0.0;
    // retain source-order squared residual accumulation
    for value in values {
        total += (value - center).powi(2);
    }
    total / values.len() as f64
}

/// Compute one numeric threshold fraction.
fn mean_boolean(values: &[f64], threshold: f64) -> f64 {
    let mut total = 0.0;
    // retain literal Number(boolean) conversion before mean
    for value in values {
        total += boolean_number(*value >= threshold);
    }
    total / values.len() as f64
}

/// Compute meteorological wind-from components at binary64 precision.
fn wind_components(hours: &[RainWindForecastHour; FORECAST_HOUR_COUNT], lead: usize) -> [f64; 2] {
    let speed = source(hours, lead, ForecastField::WindSpeed);
    let direction = source(hours, lead, ForecastField::WindDirection);
    // retain both missing components when either source field is absent
    if !speed.is_finite() || !direction.is_finite() {
        return [f64::NAN, f64::NAN];
    }
    let radians = direction.rem_euclid(360.0) * std::f64::consts::PI / 180.0;
    [-speed * radians.sin(), -speed * radians.cos()]
}

/// Convert one boolean feature to JavaScript Number semantics.
fn boolean_number(value: bool) -> f64 {
    // preserve exact zero and one values
    if value { 1.0 } else { 0.0 }
}
