use std::fs;
use std::os::unix::fs::MetadataExt;

use crate::{
    NativeRainPerformanceResultWriter, RainPerformanceNativePackageValues,
    adjustment_evaluation_package_verification::tests as adjustment_fixture,
    forecast_training_package_verification::full_fixture_tests as forecast_fixture,
    maintenance_canonical_json::{field, text},
    rain_performance_cli::tests::{FixtureAdmission, FixtureEnvironment, FixtureReplayTime},
    run_forecast_adjustment_performance_cli_with_native_package_loader,
};

/// run the public native command from both extracted packages to an exclusive report
#[test]
fn rain_performance_native_cli_verifies_both_real_packages_and_writes_report() {
    let forecast_directory = forecast_fixture::FixtureDirectory::new();
    let mut forecast_values = forecast_fixture::FixtureValues::complete_day(&forecast_directory);
    let mut package_values = forecast_values.clone();
    let (adjustment_directory, mut adjustment_values) = adjustment_fixture::empty_package_fixture();
    let forecast_path = forecast_directory.package_path();
    let report_root = forecast_path.parent().expect("fixture repository root");
    let output_path = report_root.join(".weather-data/reports/rain.json");
    let owner = fs::metadata(report_root)
        .expect("fixture root metadata")
        .uid();
    let mut writer = NativeRainPerformanceResultWriter::new(
        report_root.to_path_buf(),
        report_root.to_path_buf(),
        report_root.to_path_buf(),
        owner,
    )
    .expect("native output owner");
    let mut environment = FixtureEnvironment::default();
    let mut admission = FixtureAdmission;
    let mut replay_time = FixtureReplayTime;
    let mut packages = RainPerformanceNativePackageValues::new(
        &mut package_values,
        &mut forecast_values,
        &mut adjustment_values,
    );
    let argv = [
        "rain-evaluate",
        "--forecast-package",
        forecast_path.to_str().expect("UTF-8 forecast fixture"),
        "--adjustment-package",
        adjustment_directory
            .path()
            .to_str()
            .expect("UTF-8 adjustment fixture"),
        "--source-revision",
        "cccccccccccccccccccccccccccccccccccccccc",
        "--output",
        output_path.to_str().expect("UTF-8 output"),
    ]
    .map(str::to_owned);

    let status = run_forecast_adjustment_performance_cli_with_native_package_loader(
        &argv,
        &mut environment,
        &mut packages,
        &mut admission,
        &mut replay_time,
        &mut writer,
    )
    .expect("native rain report");
    assert_eq!(status, 0);
    assert_eq!(
        fs::read(&output_path).expect("real report file"),
        b"native-rain-report\n"
    );
    assert_eq!(forecast_values.effects(), ["snapshot", "verify"]);

    let report = environment
        .take_report()
        .expect("real report passed to the fixture codec");
    assert_eq!(
        text(&report, "contractVersion").as_deref(),
        Some("forecast-adjustment-performance-report/v1"),
    );
    let family = field(&report, "family").expect("rain report family");
    assert_eq!(
        text(family, "supportState").as_deref(),
        Some("insufficient")
    );
    let inputs = field(&report, "inputs").expect("verified package inputs");
    // preserve both admitted identities and the original cutoff in the report
    for (key, expected) in [
        (
            "forecastTrainingManifestSha256",
            forecast_fixture::MANIFEST_DIGEST,
        ),
        (
            "adjustmentEvidenceManifestSha256",
            adjustment_fixture::MANIFEST_DIGEST,
        ),
        (
            "adjustmentEvidenceWatermarkSha256",
            adjustment_fixture::WATERMARK_DIGEST,
        ),
        ("localDateFrom", "2026-10-01"),
        ("localDateTo", "2026-10-01"),
        ("targetCutoffAt", "2026-10-02T07:00:00.000Z"),
    ] {
        assert_eq!(text(inputs, key).as_deref(), Some(expected), "{key}");
    }
    assert_eq!(
        adjustment_values.control_reads(),
        ["manifest.json", "manifest.sha256"].repeat(4),
    );
}
