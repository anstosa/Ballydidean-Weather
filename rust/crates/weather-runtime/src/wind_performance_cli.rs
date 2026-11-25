//! Requalify the active wind bundle against verified offline evidence.

use std::fmt::{self, Display};
use weather_contracts::JsonValue;
use weather_worker::{
    RainPerformanceReportCalendar, WindPerformanceNonActiveCounts, create_wind_performance_report,
};

use crate::maintenance_canonical_json::field;
use crate::rain_performance_cli::{
    one_option, option_values, parse_options, resolve_performance_source_revision,
};
use crate::temperature_performance_packages::load_verified_performance_packages_with_rows;
use crate::wind_performance_counterfactual::collect_counterfactual_wind_pairs;
use crate::wind_performance_issued::collect_issued_wind_pairs;
use crate::{
    AdjustmentEvaluationPackageVerificationValues, ForecastTrainingPackageVerificationValues,
    RainPerformanceCliError, RainPerformanceNativePackageValues,
    RainPerformancePackageCompositionValues, RainPerformancePackageLoaderValues,
    RainPerformanceResultWriter, WindPerformanceBand, WindPerformanceCounterfactualValues,
    WindPerformanceError,
};

/// The exact ordered mask from the admitted active wind runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindPerformanceActiveRuntime {
    pub bundle_sha256: String,
    pub enabled: Vec<WindPerformanceBand>,
}

/// Process owners required in addition to the verified package inputs.
pub trait WindPerformanceCliValues:
    WindPerformanceCounterfactualValues
    + RainPerformancePackageCompositionValues
    + RainPerformanceReportCalendar
{
    type Error: Display;

    /// capture the clock once before any package admission
    fn capture_wind_performance_clock(&mut self) -> String;

    /// read the original fallback revision only after the package effects
    fn wind_performance_repository_head(&mut self) -> Result<String, Self::Error>;

    /// load the exact active model and bind subsequent replay to it
    fn load_active_wind_performance_runtime(
        &mut self,
        generated_at: &str,
    ) -> Result<WindPerformanceActiveRuntime, WindPerformanceError>;
}

/// Ordinary errors from the wind requalification command.
#[derive(Debug)]
pub enum WindPerformanceCliError {
    Arguments(RainPerformanceCliError),
    Effect(String),
}

impl Display for WindPerformanceCliError {
    /// preserve predecessor command diagnostics
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // keep a typed parser refusal and the active effect diagnostic
        match self {
            Self::Arguments(error) => Display::fmt(error, formatter),
            Self::Effect(text) => formatter.write_str(text),
        }
    }
}

impl std::error::Error for WindPerformanceCliError {}

/// Run the original offline wind requalification and publish a private result.
pub fn run_wind_performance_cli_with_native_packages<
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
    values: &mut impl WindPerformanceCliValues,
    writer: &mut impl RainPerformanceResultWriter,
) -> Result<u8, WindPerformanceCliError>
where
    PackageValues: RainPerformancePackageLoaderValues,
    ForecastValues: ForecastTrainingPackageVerificationValues,
    AdjustmentValues: AdjustmentEvaluationPackageVerificationValues,
{
    let options = parse_options(argv).map_err(WindPerformanceCliError::Arguments)?;
    // dispatch before capturing the clock or performing package effects
    if options.command != "wind-requalify" {
        return Err(WindPerformanceCliError::Arguments(
            RainPerformanceCliError::UnsupportedCommand(options.command),
        ));
    }
    let generated = values.capture_wind_performance_clock();
    let forecast = option_values(&options.values, "--forecast-package");
    // an explicit adjustment argument shadows the legacy spelling
    let adjustment = if options.values.contains_key("--adjustment-package") {
        option_values(&options.values, "--adjustment-package")
    } else {
        option_values(&options.values, "--evaluation-packages")
    };
    // retain the original matched-package diagnostic after clock capture
    if forecast.is_empty() || forecast.len() != adjustment.len() {
        return Err(effect(
            "wind requalification requires matched forecast and adjustment packages",
        ));
    }
    let loaded = load_verified_performance_packages_with_rows(
        forecast,
        adjustment,
        packages,
        values,
        retains_wind_forecast_row,
    )
    .map_err(effect)?;
    let revision = resolve_performance_source_revision(&options.values, || {
        values.wind_performance_repository_head()
    })
    .map_err(WindPerformanceCliError::Arguments)?;
    let active = values
        .load_active_wind_performance_runtime(&generated)
        .map_err(effect)?;
    let replayed = collect_counterfactual_wind_pairs(
        &loaded.forecast_rows,
        &loaded.adjustment_rows,
        &active.enabled,
        &generated,
        values,
    )
    .map_err(effect)?;
    let issued = collect_issued_wind_pairs(
        &loaded.forecast_rows,
        &loaded.adjustment_rows,
        &loaded.edge_rows,
        &active.bundle_sha256,
        &active.enabled,
        values,
    )
    .map_err(effect)?;
    let report = create_wind_performance_report(
        replayed,
        issued.pairs,
        WindPerformanceNonActiveCounts {
            disabled: issued.disabled_count,
            not_applicable: issued.not_applicable_count,
        },
        &loaded.inputs,
        &active.bundle_sha256,
        &revision,
        &generated,
        values,
    )
    .map_err(|error| effect(format!("wind performance report failed: {error:?}")))?;
    let output =
        one_option(&options.values, "--output").map_err(WindPerformanceCliError::Arguments)?;
    let target = writer.admit_result_path(output).map_err(effect)?;
    let bytes = values
        .encode_canonical_json(&report)
        .ok_or_else(|| effect("wind performance report serialization failed"))?;
    writer.write_new_result(target, &bytes).map_err(effect)?;
    Ok(0)
}

/// Keep replay rows and only the physical station rows used by wind targets.
fn retains_wind_forecast_row(row: &JsonValue) -> bool {
    wire_equals(row, "record_kind", "legacy_v4_retrieval_snapshot")
        || (wire_equals(row, "record_kind", "station_hour")
            && !wire_equals(row, "physical_station_key", "ballydidean-ecowitt")
            && ["wind_speed_mps", "wind_gust_mps"]
                .iter()
                .any(|metric| !matches!(field(row, metric), Some(JsonValue::Null))))
}

/// Match an original ascii field spelling without a temporary string.
fn wire_equals(row: &JsonValue, key: &str, expected: &str) -> bool {
    matches!(field(row, key), Some(JsonValue::String(value))
        if value.iter().copied().eq(expected.encode_utf16()))
}

/// Preserve the exact diagnostic returned by the selected operation.
fn effect(error: impl Display) -> WindPerformanceCliError {
    WindPerformanceCliError::Effect(error.to_string())
}

#[cfg(test)]
#[path = "wind_performance_cli_test.rs"]
mod tests;
