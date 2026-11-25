use std::cmp::Ordering;
use std::fs;
use std::os::unix::fs::MetadataExt;

use weather_contracts::{
    CanonicalWeatherMetrics, ForecastAdjustmentMetric, ForecastAdjustmentRawForecastProvenance,
    ForecastLeadBandKey,
};

use super::*;
use crate::{
    NativeRainPerformanceResultWriter, RainPerformanceCliEnvironment,
    TemperaturePerformanceTargetValues, WindPerformanceReplayDecision,
    adjustment_evaluation_package_verification::tests as adjustment,
    forecast_training_package_verification::full_fixture_tests as forecast,
    maintenance_canonical_json::text, rain_performance_cli::tests::FixtureEnvironment,
};

const FIXTURE_BUNDLE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

impl TemperaturePerformanceTargetValues for FixtureEnvironment {
    /// keep unused locale ordering within the inert wind fixture
    fn compare_js_locale(&self, left: &str, right: &str) -> Ordering {
        left.cmp(right)
    }
}

impl WindPerformanceCounterfactualValues for FixtureEnvironment {
    /// no retrieval rows are present in this complete station-gap fixture
    fn replay_admitted_wind_forecast(
        &mut self,
        _: &str,
        _: &CanonicalWeatherMetrics,
        _: &ForecastAdjustmentRawForecastProvenance,
    ) -> Result<Option<WindPerformanceReplayDecision>, WindPerformanceError> {
        unreachable!("the fixture contains no forecast retrieval rows")
    }
}

impl WindPerformanceCliValues for FixtureEnvironment {
    type Error = &'static str;

    /// share the original fixture command clock
    fn capture_wind_performance_clock(&mut self) -> String {
        self.capture_generated_at()
    }

    /// keep revision resolution on the shared test owner
    fn wind_performance_repository_head(&mut self) -> Result<String, Self::Error> {
        self.repository_head()
    }

    /// provide the exact thirteen original metric-band definitions
    fn load_active_wind_performance_runtime(
        &mut self,
        generated_at: &str,
    ) -> Result<WindPerformanceActiveRuntime, WindPerformanceError> {
        assert_eq!(generated_at, "2026-09-16T00:00:00.000Z");
        let mut enabled = Vec::new();
        // preserve both metrics with only the omitted gust lead exception
        for metric in [
            ForecastAdjustmentMetric::WindGustMps,
            ForecastAdjustmentMetric::WindSpeedMps,
        ] {
            for &(_, lead_band) in ForecastLeadBandKey::WIRE_VALUES {
                // the original active gust mask excludes this one band
                if metric == ForecastAdjustmentMetric::WindGustMps
                    && lead_band == ForecastLeadBandKey::Value049072
                {
                    continue;
                }
                enabled.push(WindPerformanceBand { metric, lead_band });
            }
        }
        Ok(WindPerformanceActiveRuntime {
            bundle_sha256: FIXTURE_BUNDLE.to_owned(),
            enabled,
        })
    }
}

/// Verify both extracted packages and publish the complete native thirteen-band report.
#[test]
fn wind_performance_command_verifies_real_files_and_writes_thirteen_reviews() {
    let forecast_directory = forecast::FixtureDirectory::new();
    let mut forecast_values = forecast::FixtureValues::complete_day(&forecast_directory);
    let mut package_values = forecast_values.clone();
    let (adjustment_directory, mut adjustment_values) = adjustment::empty_package_fixture();
    let forecast_path = forecast_directory.package_path();
    let scratch = forecast_path.parent().expect("fixture repository root");
    let output_path = scratch.join(".weather-data/wind/requalification.json");
    let mut writer = NativeRainPerformanceResultWriter::new(
        scratch.to_path_buf(),
        scratch.to_path_buf(),
        scratch.to_path_buf(),
        fs::metadata(scratch).expect("fixture metadata").uid(),
    )
    .expect("native wind output owner");
    let mut values = FixtureEnvironment::default();
    let mut packages = RainPerformanceNativePackageValues::new(
        &mut package_values,
        &mut forecast_values,
        &mut adjustment_values,
    );
    let arguments = [
        "wind-requalify",
        "--forecast-package",
        forecast_path.to_str().expect("forecast root"),
        "--evaluation-packages",
        adjustment_directory
            .path()
            .to_str()
            .expect("adjustment root"),
        "--source-revision",
        "cccccccccccccccccccccccccccccccccccccccc",
        "--output",
        output_path.to_str().expect("fixture output"),
    ]
    .map(str::to_owned);
    let status = run_wind_performance_cli_with_native_packages(
        &arguments,
        &mut packages,
        &mut values,
        &mut writer,
    )
    .expect("complete native wind command");
    assert_eq!(status, 0);
    assert!(
        !fs::read(output_path)
            .expect("exclusive real output")
            .is_empty()
    );
    assert_eq!(forecast_values.effects(), ["snapshot", "verify"]);
    assert_eq!(
        adjustment_values.control_reads(),
        ["manifest.json", "manifest.sha256"].repeat(4)
    );
    let report = values
        .take_report()
        .expect("typed report before fixture serialization");
    let family = field(&report, "family").expect("wind family card");
    assert_eq!(
        text(family, "servingIdentitySha256").as_deref(),
        Some(FIXTURE_BUNDLE)
    );
    assert_eq!(
        text(family, "qualificationState").as_deref(),
        Some("counterfactual_only")
    );
    assert_eq!(
        text(family, "supportState").as_deref(),
        Some("insufficient")
    );
    let Some(JsonValue::Array(reviews)) = field(&report, "pairReviews") else {
        panic!("forecast report should carry review rows");
    };
    assert_eq!(reviews.len(), 13);
    assert_eq!(
        text(&reviews[0], "pairIdentity").as_deref(),
        Some("windGustMps:001-024")
    );
}
