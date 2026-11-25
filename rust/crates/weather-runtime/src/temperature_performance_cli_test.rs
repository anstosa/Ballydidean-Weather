use std::cmp::Ordering;

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use super::*;
use crate::{
    ForecastTrainingPackageRowTime, MaintenanceCanonicalJson, TemperaturePerformanceTargetValues,
    adjustment_evaluation_package_verification::tests as adjustment,
    forecast_training_package_verification::full_fixture_tests as forecast,
    maintenance_canonical_json::{field, text},
};

#[derive(Default)]
struct FixtureValues {
    document: Option<JsonValue>,
}

impl MaintenanceCanonicalJson for FixtureValues {
    /// keep parsing in the owning verified package fixtures
    fn parse_canonical_object(&mut self, _: &[u8]) -> Option<JsonValue> {
        None
    }

    /// capture the real projected document before its test stdout encoding
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        self.document = Some(value.clone());
        Some(b"fixture-temperature-data\n".to_vec())
    }

    /// avoid inventing a digest when the single gap cohort requires none
    fn sha256(&mut self, _: &[u8]) -> Option<Sha256Digest> {
        unreachable!("fixture has no target or composition digest")
    }
}

impl ForecastTrainingPackageRowTime for FixtureValues {
    /// keep target timing unused in the explicit gap population
    fn utc_epoch_milliseconds(&self, _: &str) -> Option<i64> {
        None
    }

    /// keep formatting unused in the explicit gap population
    fn canonical_utc(&self, _: i64) -> Option<String> {
        None
    }

    /// keep calendar timing unused in the explicit gap population
    fn los_angeles_local_date(&self, _: i64) -> Option<String> {
        None
    }
}

impl TemperaturePerformanceTargetValues for FixtureValues {
    /// keep sort timing unused without a supported regional target
    fn compare_js_locale(&self, _: &str, _: &str) -> Ordering {
        unreachable!("fixture has no regional target identities")
    }
}

impl TemperaturePerformanceDataProjectionValues for FixtureValues {
    /// join the single numeric run id with the matching hour id
    fn js_string(&mut self, value: &JsonValue) -> Option<Vec<u16>> {
        (value == &JsonValue::Number(1.0)).then(|| "1".encode_utf16().collect())
    }
}

impl RainPerformancePackageCompositionValues for FixtureValues {
    /// keep edge date conversion unused for the empty freeze
    fn rain_package_local_date(&mut self, _: &str) -> Option<String> {
        None
    }
}

/// run both strict file verifiers and the nonempty temperature projection to stdout
#[test]
fn temperature_performance_native_command_joins_verified_run_and_hour() {
    let forecast_directory = forecast::FixtureDirectory::new();
    let mut forecast_values = forecast::FixtureValues::complete_day(&forecast_directory);
    let mut package_values = forecast_values.clone();
    let (adjustment_directory, mut adjustment_values) = adjustment::temperature_package_fixture();
    let mut packages = RainPerformanceNativePackageValues::new(
        &mut package_values,
        &mut forecast_values,
        &mut adjustment_values,
    );
    let forecast_path = forecast_directory.package_path();
    let mut values = FixtureValues::default();
    let mut stdout = Vec::new();
    let argv = [
        "temperature-data",
        "--forecast-package",
        forecast_path.to_str().expect("fixture forecast path"),
        "--adjustment-package",
        adjustment_directory
            .path()
            .to_str()
            .expect("fixture adjustment path"),
    ]
    .map(str::to_owned);
    let code = run_temperature_performance_data_cli_with_native_packages(
        &argv,
        &mut packages,
        &mut values,
        &mut stdout,
    )
    .expect("temperature fit data from both verified packages");
    assert_eq!(code, 0);
    assert_eq!(stdout, b"fixture-temperature-data\n");
    assert_eq!(forecast_values.effects(), ["snapshot", "verify"]);
    assert_eq!(
        adjustment_values.control_reads(),
        ["manifest.json", "manifest.sha256"].repeat(4)
    );

    let document = values
        .document
        .expect("actual temperature data before encoding");
    let inputs = field(&document, "inputs").expect("verified package inputs");
    assert_eq!(
        text(inputs, "targetCutoffAt").as_deref(),
        Some("2026-10-02T07:00:00.000Z")
    );
    assert_eq!(
        text(inputs, "forecastTrainingManifestSha256").as_deref(),
        Some(forecast::MANIFEST_DIGEST)
    );
    assert_eq!(
        text(inputs, "adjustmentEvidenceManifestSha256").as_deref(),
        Some(adjustment::MANIFEST_DIGEST)
    );
    assert_eq!(
        field(&document, "actualServingFallbacks"),
        Some(&JsonValue::Array(Vec::new()))
    );
    let Some(JsonValue::Array(rows)) = field(&document, "rows") else {
        panic!("expected fit rows");
    };
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(
        text(row, "key").as_deref(),
        Some("ecmwf-temperature-canary-hour:1:7")
    );
    assert_eq!(
        field(row, "rawTemperatureC"),
        Some(&JsonValue::Number(12.5))
    );
    assert_eq!(
        field(row, "operationalHorizonHours"),
        Some(&JsonValue::Number(1.0))
    );
    assert_eq!(field(row, "scoreEligible"), Some(&JsonValue::Boolean(true)));
    assert_eq!(field(row, "actualTemperatureC"), Some(&JsonValue::Null));
    assert_eq!(field(row, "recordedRuntimeResult"), Some(&JsonValue::Null));
    let pair = field(row, "scoredPairMetadata").expect("native score metadata");
    assert_eq!(
        text(pair, "evidenceClass").as_deref(),
        Some("retrospective_counterfactual")
    );
    assert_eq!(
        text(pair, "targetKey").as_deref(),
        Some("missing:2026-10-01T07:00:00.000Z")
    );
}
