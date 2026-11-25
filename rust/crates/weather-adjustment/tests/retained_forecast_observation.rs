//! Source-authentic parity for the compiled forecast-observation station catalog.

use std::{path::PathBuf, process::Command, ptr};

use weather_contracts::{
    ForecastAdjustmentMetric as Metric, ForecastObservationProviderFamily as Provider,
    ForecastObservationStationKey as StationKey, retained_forecast_observation::station_catalog,
};

/// Compare every station field and ordered array to the compiled original TypeScript catalog.
#[test]
fn matches_compiled_forecast_observation_station_catalog() {
    let first = station_catalog();
    let second = station_catalog();
    assert!(
        ptr::eq(first, second),
        "catalog allocation must be retained"
    );

    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/retained_forecast_observation_node_oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("compiled Node station-catalog oracle starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = String::from_utf8(output.stdout).expect("oracle output is UTF-8");
    let actual = first
        .iter()
        .enumerate()
        .map(|(index, station)| {
            [
                "STATION".to_owned(),
                index.to_string(),
                station_name(station.key).to_owned(),
                provider_name(station.providerFamily).to_owned(),
                station.nearestRank.to_string(),
                bits(station.latitude),
                bits(station.longitude),
                bits(station.distanceMeters),
                bits(station.unnormalizedSpatialWeight),
                station
                    .eligibleMetrics
                    .iter()
                    .map(|&metric| metric_name(metric))
                    .collect::<Vec<_>>()
                    .join(","),
                station.acceptedSourceKeys.join(","),
            ]
            .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert_eq!(actual, expected);
}

/// Encode one Rust binary64 value in the oracle's big-endian hexadecimal form.
fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

/// Render one generated metric wire value.
const fn metric_name(value: Metric) -> &'static str {
    match value {
        Metric::RelativeHumidityPercent => "relativeHumidityPercent",
        Metric::TemperatureC => "temperatureC",
        Metric::WindDirectionDegrees => "windDirectionDegrees",
        Metric::WindGustMps => "windGustMps",
        Metric::WindSpeedMps => "windSpeedMps",
    }
}

/// Render one generated provider-family wire value.
const fn provider_name(value: Provider) -> &'static str {
    match value {
        Provider::Ambient => "ambient",
        Provider::Ecowitt => "ecowitt",
        Provider::Netatmo => "netatmo",
        Provider::Tempest => "tempest",
    }
}

/// Render one generated station-key wire value.
const fn station_name(value: StationKey) -> &'static str {
    match value {
        StationKey::AmbientMaxweather => "ambient-maxweather",
        StationKey::AmbientMerlin => "ambient-merlin",
        StationKey::BallydideanEcowitt => "ballydidean-ecowitt",
        StationKey::NetatmoNearby => "netatmo-nearby",
        StationKey::Tempest126537 => "tempest-126537",
        StationKey::Tempest168853 => "tempest-168853",
        StationKey::Tempest201058 => "tempest-201058",
        StationKey::Tempest203055 => "tempest-203055",
        StationKey::Tempest225947 => "tempest-225947",
        StationKey::Tempest38270 => "tempest-38270",
        StationKey::Tempest64255 => "tempest-64255",
    }
}
