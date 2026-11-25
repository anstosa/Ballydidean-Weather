//! Focused ownership tests for the native administrator composition.

use std::path::PathBuf;

use weather_edge::{
    DISABLED_FORECAST_ADJUSTMENT_SETTINGS, ForecastAdjustmentSettingsDiagnostic,
    ForecastAdjustmentSettingsStatus, ForecastAdjustmentSettingsStore, PropertySensorLayoutOutcome,
    PropertySensorLayoutRequest, PropertySensorMapCenter,
};

use super::{NativeEdgeAdminOwner, native_edge_adjustment_settings_path};

/// Prove exact sibling-path derivation for absolute and current-directory layout files.
#[test]
fn derives_original_settings_sibling_path() {
    assert_eq!(
        native_edge_adjustment_settings_path(&PathBuf::from(
            "/var/lib/weather/xweather/property-sensor-layout.json"
        )),
        PathBuf::from("/var/lib/weather/xweather/forecast-adjustment-settings.json")
    );
    assert_eq!(
        native_edge_adjustment_settings_path(&PathBuf::from("layout.json")),
        PathBuf::from("./forecast-adjustment-settings.json")
    );
}

/// Retain the genuine first-run settings and empty-layout route behavior without credentials.
#[test]
fn composes_first_run_settings_and_layout_owners() {
    let root = temporary_fixture_root();
    let mut owner = NativeEdgeAdminOwner::new(
        root.join("admin-auth.json"),
        root.join("forecast-adjustment-settings.json"),
        root.join("property-sensor-layout.json"),
        PropertySensorMapCenter {
            latitude: 47.95043,
            longitude: -122.42797,
        },
    )
    .expect("construct native admin owner");

    assert_eq!(
        owner.settings_mut().read_status(),
        ForecastAdjustmentSettingsStatus {
            settings: DISABLED_FORECAST_ADJUSTMENT_SETTINGS,
            error: Some(ForecastAdjustmentSettingsDiagnostic::Unavailable),
        }
    );
    assert_eq!(
        owner.property_sensor_layout(PropertySensorLayoutRequest::PublicRead { omit_body: false }),
        PropertySensorLayoutOutcome::ReadSuccess {
            data: Vec::new(),
            omit_body: false,
        }
    );
}

/// Allocate one unused bounded fixture root without creating persistent state.
fn temporary_fixture_root() -> PathBuf {
    let mut entropy = [0_u8; 8];
    getrandom::fill(&mut entropy).expect("fixture entropy");
    std::env::temp_dir().join(format!(
        "weather-native-edge-admin-owner-{}-{:016x}",
        std::process::id(),
        u64::from_le_bytes(entropy)
    ))
}
