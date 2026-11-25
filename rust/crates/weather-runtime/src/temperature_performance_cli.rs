//! Run the native temperature fit-data command over matched extracted packages.

use std::fmt::{self, Display};
use std::io::{self, Write};

use crate::rain_performance_cli::{option_values, parse_options};
use crate::{
    AdjustmentEvaluationPackageVerificationValues, ForecastTrainingPackageVerificationValues,
    RainPerformanceCliError, RainPerformanceNativePackageValues,
    RainPerformancePackageCompositionValues, RainPerformancePackageLoaderValues,
    TemperaturePerformanceDataProjectionError, TemperaturePerformanceDataProjectionValues,
    TemperaturePerformancePackageLoadError, load_verified_temperature_performance_packages,
    project_temperature_performance_data,
};

const SELECTED_COMMAND: &str = "temperature-data";

/// Ordinary failures from the stdout temperature data command.
#[derive(Debug)]
pub enum TemperaturePerformanceCliError {
    Arguments(RainPerformanceCliError),
    Packages(TemperaturePerformancePackageLoadError),
    Projection(TemperaturePerformanceDataProjectionError),
    Serialization,
    Write(io::Error),
}

impl Display for TemperaturePerformanceCliError {
    /// preserve the predecessor diagnostics through the command boundary
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // forward phase diagnostics without fake success bytes
        match self {
            Self::Arguments(error) => Display::fmt(error, formatter),
            Self::Packages(error) => Display::fmt(error, formatter),
            Self::Projection(error) => Display::fmt(error, formatter),
            Self::Serialization => {
                formatter.write_str("temperature performance data serialization failed")
            }
            Self::Write(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for TemperaturePerformanceCliError {}

/// Verify both package families and move the projected fit data to stdout.
pub fn run_temperature_performance_data_cli_with_native_packages<
    PackageValues,
    ForecastValues,
    AdjustmentValues,
>(
    argv: &[String],
    packages: &mut RainPerformanceNativePackageValues<
        '_,
        PackageValues,
        ForecastValues,
        AdjustmentValues,
    >,
    values: &mut (
             impl TemperaturePerformanceDataProjectionValues + RainPerformancePackageCompositionValues
         ),
    stdout: &mut impl Write,
) -> Result<u8, TemperaturePerformanceCliError>
where
    PackageValues: RainPerformancePackageLoaderValues,
    ForecastValues: ForecastTrainingPackageVerificationValues,
    AdjustmentValues: AdjustmentEvaluationPackageVerificationValues,
{
    use TemperaturePerformanceCliError as Error;

    let options = parse_options(argv).map_err(Error::Arguments)?;
    // dispatch before performing any file or output effect
    if options.command != SELECTED_COMMAND {
        return Err(Error::Arguments(
            RainPerformanceCliError::UnsupportedCommand(options.command),
        ));
    }
    let loaded = load_verified_temperature_performance_packages(
        option_values(&options.values, "--forecast-package"),
        option_values(&options.values, "--adjustment-package"),
        packages,
        values,
    )
    .map_err(Error::Packages)?;
    let document = project_temperature_performance_data(
        &loaded.forecast_rows,
        &loaded.adjustment_rows,
        &loaded.edge_rows,
        &loaded.inputs,
        values,
    )
    .map_err(Error::Projection)?;
    let bytes = values
        .encode_canonical_json(&document)
        .ok_or(Error::Serialization)?;
    stdout.write_all(&bytes).map_err(Error::Write)?;
    Ok(0)
}

#[cfg(test)]
#[path = "temperature_performance_cli_test.rs"]
mod tests;
