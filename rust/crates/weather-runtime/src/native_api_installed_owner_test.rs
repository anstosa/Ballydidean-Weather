//! Positive fixture proof for the concrete native installed-model owner.

use std::path::Path;

use weather_api::WeatherApiStartupRuntimeLoader;
use weather_contracts::{
    FieldPresence, ForecastAdjustmentTemperatureCanaryRuntimeBundle,
    ForecastAdjustmentWindCanaryRuntimeBundle, LoadedForecastAdjustmentRuntimeV1,
    LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentTemperatureCanaryRuntime,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1, LoadedForecastAdjustmentWindCanaryRuntime,
    LoadedForecastAdjustmentWindCanaryRuntimeV1,
};

use crate::{
    InstalledApiCanarySwitches, NativeApiInstalledOwner, NativeApiMaintenanceCandidateFiles,
    NativeForecastRuntimeFiles, NativeInstalledApiRuntimeLoader,
    NativeInstalledQualifiedCanaryLoaders, NativeModelEvidenceValues, NativeWirePrimitives,
};

const WIND_BUNDLE_SHA256: &str = "51f8efd63bef678a7f02d11bdab91405ec48f19808f64d3fe8354036c9b302a2";
const TEMPERATURE_BUNDLE_SHA256: &str =
    "4d4e229b42823e53d2db062ec18c625bb2d2378a8a46d641fa95fabb59501b0e";

/// Return the activation clock carried by both checked-in permanent model fixtures.
fn fixture_now() -> Option<String> {
    Some("2026-10-01T16:33:20.936Z".to_owned())
}

/// Load the checked-in registries through the exact production owner and native value graph.
#[test]
fn native_installed_owner_loads_checked_in_model_registries() {
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("canonical repository fixture root");
    let runtime_root = repository_root.join("config/forecast-adjustments");
    let files = NativeForecastRuntimeFiles::new(runtime_root)
        .expect("bind checked-in forecast runtime fixtures");
    let candidate_files = NativeApiMaintenanceCandidateFiles::new(repository_root)
        .expect("bind checked-in candidate fixture root");
    let values = NativeModelEvidenceValues::new();
    let qualified = NativeInstalledQualifiedCanaryLoaders::new(
        candidate_files.clone(),
        NativeWirePrimitives::new(),
        values,
    );
    let mut owner: NativeApiInstalledOwner = NativeInstalledApiRuntimeLoader::new(
        files,
        values,
        NativeWirePrimitives::new(),
        candidate_files,
        qualified,
        fixture_now as fn() -> Option<String>,
        InstalledApiCanarySwitches {
            wind: None,
            temperature: Some("0".to_owned()),
        },
    );

    let wind = owner.load_wind_canary().expect("load wind fixture");
    let LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
        value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Active(wind),
        comparatorAuthority: FieldPresence::Value(_),
    } = wind
    else {
        panic!("checked-in wind fixture did not activate: {wind:?}");
    };
    let ForecastAdjustmentWindCanaryRuntimeBundle::Permanent(wind) = wind.bundle else {
        panic!("checked-in wind fixture was not permanent");
    };
    assert_eq!(wind.bundleSha256, WIND_BUNDLE_SHA256);
    assert_eq!(wind.candidate.runtimeFingerprint.icuVersion, "78.3");
    assert_eq!(wind.candidate.runtimeFingerprint.tzdataVersion, "2026b");

    let temperature = owner.load_temperature().expect("load temperature fixture");
    let LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy {
        value: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Active(temperature),
        comparatorAuthority: FieldPresence::Value(_),
    } = temperature
    else {
        panic!("checked-in temperature fixture did not activate: {temperature:?}");
    };
    let ForecastAdjustmentTemperatureCanaryRuntimeBundle::Permanent(temperature) =
        temperature.bundle
    else {
        panic!("checked-in temperature fixture was not permanent");
    };
    assert_eq!(temperature.bundleSha256, TEMPERATURE_BUNDLE_SHA256);

    let standard = owner
        .load_qualified_wind()
        .expect("load inactive standard fixture");
    let LoadedForecastAdjustmentRuntimeV1::Disabled(standard) = standard else {
        panic!("checked-in standard fixture unexpectedly activated");
    };
    assert_eq!(
        standard.reasonCode,
        LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::RegistryInactive
    );
}
