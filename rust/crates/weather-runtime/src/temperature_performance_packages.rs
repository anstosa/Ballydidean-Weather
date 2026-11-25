//! Retain the verified target rows and package evidence the temperature command consumes.

use std::fmt::{self, Display};

use weather_contracts::JsonValue;
use weather_worker::RainPerformanceReportPackageInputs;

use crate::maintenance_canonical_json::field;
use crate::rain_performance_cli::composition_error_message;
use crate::rain_performance_package_loader::load_native_performance_pair_with_forecast_rows;
use crate::{
    AdjustmentEvaluationPackageVerificationValues, ForecastTrainingPackageVerificationValues,
    JoinedRainPerformanceEdgeRow, RainPerformanceNativePackageValues,
    RainPerformancePackageCompositionError, RainPerformancePackageCompositionValues,
    RainPerformancePackageEdgeEvidence, RainPerformancePackageLoaderError,
    RainPerformancePackageLoaderValues, compose_admitted_rain_performance_package_pairs,
};

/// The verified package inputs used by native temperature fit-data projection.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedTemperaturePerformancePackages {
    pub inputs: RainPerformanceReportPackageInputs,
    pub forecast_rows: Vec<JsonValue>,
    pub adjustment_rows: Vec<JsonValue>,
    pub edge_rows: Vec<JoinedRainPerformanceEdgeRow>,
}

/// Original package load and composition refusals at the temperature boundary.
#[derive(Debug, PartialEq)]
pub enum TemperaturePerformancePackageLoadError {
    MatchedPackagesRequired,
    Package(RainPerformancePackageLoaderError),
    Composition(RainPerformancePackageCompositionError),
    GenericEdgeRequired,
}

impl Display for TemperaturePerformancePackageLoadError {
    /// preserve the predecessor's shared package diagnostics
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // distinguish matching, verification, and composition failures
        match self {
            Self::MatchedPackagesRequired => formatter
                .write_str("performance package composition requires matched package pairs"),
            Self::Package(error) => Display::fmt(error, formatter),
            Self::Composition(error) => formatter.write_str(composition_error_message(*error)),
            Self::GenericEdgeRequired => {
                formatter.write_str("captured temperature decision must be an object")
            }
        }
    }
}

impl std::error::Error for TemperaturePerformancePackageLoadError {}

/// Load each native pair in argument order, then compose disjoint date blocks.
pub fn load_verified_temperature_performance_packages<
    PackageValues,
    ForecastValues,
    AdjustmentValues,
>(
    forecast_paths: &[String],
    adjustment_paths: &[String],
    values: &mut RainPerformanceNativePackageValues<
        '_,
        PackageValues,
        ForecastValues,
        AdjustmentValues,
    >,
    composition: &mut impl RainPerformancePackageCompositionValues,
) -> Result<VerifiedTemperaturePerformancePackages, TemperaturePerformancePackageLoadError>
where
    PackageValues: RainPerformancePackageLoaderValues,
    ForecastValues: ForecastTrainingPackageVerificationValues,
    AdjustmentValues: AdjustmentEvaluationPackageVerificationValues,
{
    load_verified_performance_packages_with_rows(
        forecast_paths,
        adjustment_paths,
        values,
        composition,
        temperature_target_row,
    )
}

/// Load verified package evidence with only the forecast populations the caller uses.
pub(crate) fn load_verified_performance_packages_with_rows<
    PackageValues,
    ForecastValues,
    AdjustmentValues,
>(
    forecast_paths: &[String],
    adjustment_paths: &[String],
    values: &mut RainPerformanceNativePackageValues<
        '_,
        PackageValues,
        ForecastValues,
        AdjustmentValues,
    >,
    composition: &mut impl RainPerformancePackageCompositionValues,
    retain: impl Fn(&JsonValue) -> bool,
) -> Result<VerifiedTemperaturePerformancePackages, TemperaturePerformancePackageLoadError>
where
    PackageValues: RainPerformancePackageLoaderValues,
    ForecastValues: ForecastTrainingPackageVerificationValues,
    AdjustmentValues: AdjustmentEvaluationPackageVerificationValues,
{
    use TemperaturePerformancePackageLoadError as Error;

    // refuse empty or unmatched inputs before opening either package
    if forecast_paths.is_empty() || forecast_paths.len() != adjustment_paths.len() {
        return Err(Error::MatchedPackagesRequired);
    }
    let mut loaded = Vec::with_capacity(forecast_paths.len());
    // verify each pair exactly once while retaining its needed forecast station rows
    for (forecast, adjustment) in forecast_paths.iter().zip(adjustment_paths) {
        let mut rows = Vec::new();
        let pair =
            load_native_performance_pair_with_forecast_rows(forecast, adjustment, values, |row| {
                // keep the exact target population without storing unrelated retrieval rows
                if retain(&row) {
                    rows.push(row);
                }
            })
            .map_err(Error::Package)?;
        loaded.push((pair, rows));
    }
    // keep everything ordered by package start, not by supplied arguments
    loaded.sort_by(|left, right| {
        left.0
            .inputs
            .local_date_from
            .cmp(&right.0.inputs.local_date_from)
    });
    let mut forecast_rows = Vec::new();
    let pairs = loaded
        .into_iter()
        .map(|(pair, rows)| {
            forecast_rows.extend(rows);
            pair
        })
        .collect();
    let composed = compose_admitted_rain_performance_package_pairs(pairs, composition)
        .map_err(Error::Composition)?;
    let (inputs, adjustment_rows, edges) = composed.into_shared_evidence();
    let edge_rows = edges
        .into_iter()
        .map(|edge| {
            // the strict shared package join retains original rows rather than a rain-only projection
            match edge {
                RainPerformancePackageEdgeEvidence::Joined(value) => Ok(*value),
                RainPerformancePackageEdgeEvidence::Projected(_) => Err(Error::GenericEdgeRequired),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(VerifiedTemperaturePerformancePackages {
        inputs,
        forecast_rows,
        adjustment_rows,
        edge_rows,
    })
}

/// Match only the rows used by the original temperature target filter.
fn temperature_target_row(row: &JsonValue) -> bool {
    wire_equals(row, "record_kind", "station_hour")
        && !wire_equals(row, "physical_station_key", "ballydidean-ecowitt")
        && !matches!(field(row, "temperature_c"), Some(JsonValue::Null))
}

/// Compare a verified string without allocating its UTF-8 form.
fn wire_equals(row: &JsonValue, key: &str, expected: &str) -> bool {
    matches!(field(row, key), Some(JsonValue::String(value))
        if value.iter().copied().eq(expected.encode_utf16()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// retain only the targets the original temperature loader reads
    #[test]
    fn keeps_station_temperature_rows_without_forecast_or_ecowitt_rows() {
        let string = |value: &str| JsonValue::String(value.encode_utf16().collect());
        let row = |kind, station, temperature| {
            JsonValue::Object(
                [
                    ("record_kind", string(kind)),
                    ("physical_station_key", string(station)),
                    ("temperature_c", temperature),
                ]
                .into_iter()
                .map(|(key, value)| (key.encode_utf16().collect(), value))
                .collect(),
            )
        };
        assert!(temperature_target_row(&row(
            "station_hour",
            "ambient-merlin",
            JsonValue::Number(11.0),
        )));
        assert!(!temperature_target_row(&row(
            "station_hour",
            "ballydidean-ecowitt",
            JsonValue::Number(99.0),
        )));
        assert!(!temperature_target_row(&row(
            "station_hour",
            "ambient-merlin",
            JsonValue::Null,
        )));
        assert!(!temperature_target_row(&row(
            "fixed_lead_anchor",
            "ambient-merlin",
            JsonValue::Number(10.0),
        )));
    }
}
