#![allow(non_snake_case)]

use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{ForecastLeadBandKey, SourceKind};

mod rain_collection_catalog;
pub use rain_collection_catalog::*;
mod rain_control_state;
pub use rain_control_state::*;

/// Every metric accepted by the canonical weather record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricName {
    ApparentTemperatureC,
    BlackGlobeTemperatureC,
    CloudCoverPercent,
    Pm25MicrogramsPerCubicMeter,
    PrecipitationMm,
    PrecipitationRateMmPerHour,
    PressureHpa,
    RelativeHumidityPercent,
    SoilElectricalConductivityMicrosiemensPerCm,
    SoilMoisturePercent,
    SolarRadiationWm2,
    TemperatureC,
    UvIndex,
    WaterLevelM,
    WindDirectionDegrees,
    WindGustMps,
    WindSpeedMps,
    WetBulbGlobeTemperatureC,
}

impl MetricName {
    /// Return the TypeScript contract key for this metric.
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::ApparentTemperatureC => "apparentTemperatureC",
            Self::BlackGlobeTemperatureC => "blackGlobeTemperatureC",
            Self::CloudCoverPercent => "cloudCoverPercent",
            Self::Pm25MicrogramsPerCubicMeter => "pm25MicrogramsPerCubicMeter",
            Self::PrecipitationMm => "precipitationMm",
            Self::PrecipitationRateMmPerHour => "precipitationRateMmPerHour",
            Self::PressureHpa => "pressureHpa",
            Self::RelativeHumidityPercent => "relativeHumidityPercent",
            Self::SoilElectricalConductivityMicrosiemensPerCm => {
                "soilElectricalConductivityMicrosiemensPerCm"
            }
            Self::SoilMoisturePercent => "soilMoisturePercent",
            Self::SolarRadiationWm2 => "solarRadiationWm2",
            Self::TemperatureC => "temperatureC",
            Self::UvIndex => "uvIndex",
            Self::WaterLevelM => "waterLevelM",
            Self::WindDirectionDegrees => "windDirectionDegrees",
            Self::WindGustMps => "windGustMps",
            Self::WindSpeedMps => "windSpeedMps",
            Self::WetBulbGlobeTemperatureC => "wetBulbGlobeTemperatureC",
        }
    }
}

/// Units accepted by the existing TypeScript normalizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Celsius,
    Degree,
    Fahrenheit,
    Hectopascal,
    Inch,
    InchOfMercury,
    InchPerHour,
    Index,
    KilometerPerHour,
    Knot,
    MicrogramPerCubicMeter,
    MicrosiemensPerCentimeter,
    Meter,
    MeterPerSecond,
    MilePerHour,
    Millimeter,
    MillimeterPerHour,
    Pascal,
    Percent,
    WattPerSquareMeter,
}

impl Unit {
    /// Return the TypeScript contract key for this unit.
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Celsius => "c",
            Self::Degree => "degree",
            Self::Fahrenheit => "f",
            Self::Hectopascal => "hectopascal",
            Self::Inch => "inch",
            Self::InchOfMercury => "inch_of_mercury",
            Self::InchPerHour => "inch_per_hour",
            Self::Index => "index",
            Self::KilometerPerHour => "kilometer_per_hour",
            Self::Knot => "knot",
            Self::MicrogramPerCubicMeter => "microgram_per_cubic_meter",
            Self::MicrosiemensPerCentimeter => "microsiemens_per_centimeter",
            Self::Meter => "meter",
            Self::MeterPerSecond => "meter_per_second",
            Self::MilePerHour => "mile_per_hour",
            Self::Millimeter => "millimeter",
            Self::MillimeterPerHour => "millimeter_per_hour",
            Self::Pascal => "pascal",
            Self::Percent => "percent",
            Self::WattPerSquareMeter => "watt_per_square_meter",
        }
    }
}

/// Failures emitted by canonical weather validation.
#[derive(Debug, Clone, PartialEq)]
pub enum DomainError {
    NonFiniteMetric {
        metric: MetricName,
    },
    MetricOutOfRange {
        metric: MetricName,
        minimum: f64,
        maximum: f64,
        maximum_exclusive: bool,
    },
    UnsupportedUnit {
        metric: MetricName,
        unit: Unit,
    },
    InvalidLatitude,
    InvalidLongitude,
    NonIntegerForecastLeadHours,
    ForecastLeadHoursOutOfRange,
    UnsupportedSourceKind(String),
}

impl Display for DomainError {
    /// Render the same stable concepts used by the TypeScript error messages.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteMetric { metric } => {
                write!(formatter, "{} must be finite", metric.wire_value())
            }
            Self::MetricOutOfRange {
                metric,
                minimum,
                maximum,
                maximum_exclusive,
            } => {
                // preserve the exclusive wind-direction boundary message
                if *maximum_exclusive {
                    write!(
                        formatter,
                        "{} must be less than {maximum}",
                        metric.wire_value()
                    )
                } else {
                    write!(
                        formatter,
                        "{} must be between {minimum} and {maximum}",
                        metric.wire_value(),
                    )
                }
            }
            Self::UnsupportedUnit { metric, unit } => write!(
                formatter,
                "{} is not supported for {}",
                unit.wire_value(),
                metric.wire_value(),
            ),
            Self::InvalidLatitude => formatter.write_str("latitude must be between -90 and 90"),
            Self::InvalidLongitude => formatter.write_str("longitude must be between -180 and 180"),
            Self::NonIntegerForecastLeadHours => {
                formatter.write_str("targetLeadHours must be an integer")
            }
            Self::ForecastLeadHoursOutOfRange => {
                formatter.write_str("targetLeadHours must be between 1 and 168")
            }
            Self::UnsupportedSourceKind(value) => {
                write!(formatter, "unsupported source kind: {value}")
            }
        }
    }
}

impl Error for DomainError {}

#[derive(Debug, Clone, Copy)]
struct MetricBounds {
    minimum: f64,
    maximum: f64,
    maximum_exclusive: bool,
}

impl MetricBounds {
    /// Create an inclusive canonical metric range.
    const fn inclusive(minimum: f64, maximum: f64) -> Self {
        Self {
            minimum,
            maximum,
            maximum_exclusive: false,
        }
    }

    /// Create a canonical metric range with an excluded maximum.
    const fn maximum_exclusive(minimum: f64, maximum: f64) -> Self {
        Self {
            minimum,
            maximum,
            maximum_exclusive: true,
        }
    }
}

/// Convert a provider value to its canonical unit and validate its range.
pub fn normalize_metric_value(
    metric: MetricName,
    value: Option<f64>,
    unit: Unit,
) -> Result<Option<f64>, DomainError> {
    // preserve missing values without applying a unit conversion
    let Some(value) = value else {
        return Ok(None);
    };

    // reject non-finite provider inputs before arithmetic
    if !value.is_finite() {
        return Err(DomainError::NonFiniteMetric { metric });
    }

    let converted = convert_metric_value(metric, value, unit)?;
    validate_metric_value(metric, converted)?;
    Ok(Some(converted))
}

/// Validate a value that is already expressed in its canonical unit.
pub fn validate_metric_value(metric: MetricName, value: f64) -> Result<(), DomainError> {
    // reject non-finite canonical values
    if !value.is_finite() {
        return Err(DomainError::NonFiniteMetric { metric });
    }

    let range = metric_bounds(metric);
    // reject values outside the inclusive range
    if value < range.minimum || value > range.maximum {
        return Err(DomainError::MetricOutOfRange {
            metric,
            minimum: range.minimum,
            maximum: range.maximum,
            maximum_exclusive: range.maximum_exclusive,
        });
    }

    // reject an exclusive upper boundary
    if range.maximum_exclusive && value == range.maximum {
        return Err(DomainError::MetricOutOfRange {
            metric,
            minimum: range.minimum,
            maximum: range.maximum,
            maximum_exclusive: true,
        });
    }

    Ok(())
}

/// Validate and retain a latitude and longitude pair.
pub fn validate_coordinates(latitude: f64, longitude: f64) -> Result<(f64, f64), DomainError> {
    // enforce the latitude domain
    if !latitude.is_finite() || !(-90.0..=90.0).contains(&latitude) {
        return Err(DomainError::InvalidLatitude);
    }

    // enforce the longitude domain
    if !longitude.is_finite() || !(-180.0..=180.0).contains(&longitude) {
        return Err(DomainError::InvalidLongitude);
    }

    Ok((latitude, longitude))
}

/// Parse the exact source-kind strings generated from TypeScript.
pub fn parse_source_kind(value: &str) -> Result<SourceKind, DomainError> {
    SourceKind::WIRE_VALUES
        .iter()
        .find_map(|(wire_value, source_kind)| (*wire_value == value).then_some(*source_kind))
        .ok_or_else(|| DomainError::UnsupportedSourceKind(value.to_owned()))
}

/// Describe a source kind with the existing TypeScript presentation text.
pub const fn source_kind_label(source_kind: SourceKind) -> &'static str {
    match source_kind {
        SourceKind::PhysicalSensor => "physical sensor measurement",
        SourceKind::ModelCurrent => "model-derived current conditions",
        SourceKind::Reanalysis => "gridded historical reanalysis",
        SourceKind::Forecast => "forecast model product",
        SourceKind::TideObservation => "observed tide level",
        SourceKind::TidePrediction => "tide prediction",
    }
}

/// Return whether a source is a direct physical measurement.
pub const fn is_physical_measurement(source_kind: SourceKind) -> bool {
    matches!(source_kind, SourceKind::PhysicalSensor)
}

/// Map an eligible application lead to the generated TypeScript band enum.
pub fn forecast_lead_band_for(target_lead_hours: f64) -> Result<ForecastLeadBandKey, DomainError> {
    // require the same finite integer input admitted by Number.isInteger
    if !target_lead_hours.is_finite() || target_lead_hours.fract() != 0.0 {
        return Err(DomainError::NonIntegerForecastLeadHours);
    }

    let lead_band = match target_lead_hours as i64 {
        1..=24 => ForecastLeadBandKey::Value001024,
        25..=48 => ForecastLeadBandKey::Value025048,
        49..=72 => ForecastLeadBandKey::Value049072,
        73..=96 => ForecastLeadBandKey::Value073096,
        97..=120 => ForecastLeadBandKey::Value097120,
        121..=144 => ForecastLeadBandKey::Value121144,
        145..=168 => ForecastLeadBandKey::Value145168,
        _ => return Err(DomainError::ForecastLeadHoursOutOfRange),
    };
    Ok(lead_band)
}

/// Convert one supported unit without validating the resulting range.
fn convert_metric_value(metric: MetricName, value: f64, unit: Unit) -> Result<f64, DomainError> {
    let converted = match (metric, unit) {
        (
            MetricName::TemperatureC
            | MetricName::ApparentTemperatureC
            | MetricName::BlackGlobeTemperatureC
            | MetricName::WetBulbGlobeTemperatureC,
            Unit::Celsius,
        ) => value,
        (
            MetricName::TemperatureC
            | MetricName::ApparentTemperatureC
            | MetricName::BlackGlobeTemperatureC
            | MetricName::WetBulbGlobeTemperatureC,
            Unit::Fahrenheit,
        ) => (value - 32.0) * (5.0 / 9.0),
        (MetricName::PrecipitationMm, Unit::Millimeter) => value,
        (MetricName::PrecipitationMm, Unit::Inch) => value * 25.4,
        (MetricName::PrecipitationRateMmPerHour, Unit::MillimeterPerHour) => value,
        (MetricName::PrecipitationRateMmPerHour, Unit::InchPerHour) => value * 25.4,
        (MetricName::WindSpeedMps | MetricName::WindGustMps, Unit::MeterPerSecond) => value,
        (MetricName::WindSpeedMps | MetricName::WindGustMps, Unit::KilometerPerHour) => value / 3.6,
        (MetricName::WindSpeedMps | MetricName::WindGustMps, Unit::Knot) => value * 0.514_444,
        (MetricName::WindSpeedMps | MetricName::WindGustMps, Unit::MilePerHour) => value * 0.447_04,
        (MetricName::PressureHpa, Unit::Hectopascal) => value,
        (MetricName::PressureHpa, Unit::Pascal) => value / 100.0,
        (MetricName::PressureHpa, Unit::InchOfMercury) => value * 33.863_886_666_7,
        (
            MetricName::RelativeHumidityPercent
            | MetricName::CloudCoverPercent
            | MetricName::SoilMoisturePercent,
            Unit::Percent,
        ) => value,
        (MetricName::Pm25MicrogramsPerCubicMeter, Unit::MicrogramPerCubicMeter) => value,
        (
            MetricName::SoilElectricalConductivityMicrosiemensPerCm,
            Unit::MicrosiemensPerCentimeter,
        ) => value,
        (MetricName::SolarRadiationWm2, Unit::WattPerSquareMeter) => value,
        (MetricName::UvIndex, Unit::Index) => value,
        (MetricName::WaterLevelM, Unit::Meter) => value,
        (MetricName::WindDirectionDegrees, Unit::Degree) => value,
        _ => return Err(DomainError::UnsupportedUnit { metric, unit }),
    };
    Ok(converted)
}

/// Return the exact range from the TypeScript canonical metric table.
const fn metric_bounds(metric: MetricName) -> MetricBounds {
    match metric {
        MetricName::ApparentTemperatureC | MetricName::TemperatureC => {
            MetricBounds::inclusive(-100.0, 70.0)
        }
        MetricName::BlackGlobeTemperatureC | MetricName::WetBulbGlobeTemperatureC => {
            MetricBounds::inclusive(-100.0, 125.0)
        }
        MetricName::CloudCoverPercent
        | MetricName::RelativeHumidityPercent
        | MetricName::SoilMoisturePercent => MetricBounds::inclusive(0.0, 100.0),
        MetricName::Pm25MicrogramsPerCubicMeter => MetricBounds::inclusive(0.0, 999.0),
        MetricName::PrecipitationMm => MetricBounds::inclusive(0.0, 2_000.0),
        MetricName::PrecipitationRateMmPerHour => MetricBounds::inclusive(0.0, 10_000.0),
        MetricName::PressureHpa => MetricBounds::inclusive(100.0, 1_200.0),
        MetricName::SoilElectricalConductivityMicrosiemensPerCm => {
            MetricBounds::inclusive(0.0, 10_000.0)
        }
        MetricName::SolarRadiationWm2 => MetricBounds::inclusive(0.0, 2_500.0),
        MetricName::UvIndex => MetricBounds::inclusive(0.0, 20.0),
        MetricName::WaterLevelM => MetricBounds::inclusive(-20.0, 30.0),
        MetricName::WindDirectionDegrees => MetricBounds::maximum_exclusive(0.0, 360.0),
        MetricName::WindGustMps | MetricName::WindSpeedMps => MetricBounds::inclusive(0.0, 150.0),
    }
}
