//! Actual worker file owners compared with the shipped predecessor catalog parser.

use std::{path::PathBuf, process::Command};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;
use weather_runtime::{
    NativeWirePrimitives, NativeWorkerCompatibilityOrigin, NativeWorkerConfigurationArtifacts,
    NativeWorkerJsonConfiguration, WorkerCompatibilityOriginOwner,
    WorkerConfigurationArtifactOwner, WorkerJsonConfigurationOwner,
};

/// Convert one selected generated string without changing its UTF-16 identity.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
/// Preserve the original ordered array projection.
fn array(values: impl IntoIterator<Item = JsonValue>) -> JsonValue {
    JsonValue::Array(values.into_iter().collect())
}
/// Encode only the selected small catalog contract number.
fn version(value: u64) -> JsonValue {
    JsonValue::Number(value as f64)
}
/// Preserve the admitted active flag.
fn active(value: bool) -> JsonValue {
    JsonValue::Boolean(value)
}

/// Drive actual Rust configuration file readers, retaining all normalized source fingerprints.
#[test]
fn native_worker_catalogs_match_original_shipped_material() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let path = |relative: &str| root.join(relative).to_string_lossy().into_owned();
    let source = Command::new("node")
        .arg(root.join("rust/crates/weather-runtime/tests/native-worker-catalogs-node-oracle.mjs"))
        .output()
        .unwrap();
    assert!(
        source.status.success(),
        "{}",
        String::from_utf8_lossy(&source.stderr)
    );
    let mut configs = NativeWorkerJsonConfiguration;
    let site = configs
        .load_site_configuration(&path("config/sites/ballydidean.json"))
        .unwrap();
    let ecowitt = configs
        .load_ecowitt_configuration(&path("config/ecowitt/gateways.json"))
        .unwrap();
    let tempest = configs
        .load_tempest_configuration(&path("config/tempest/stations.json"))
        .unwrap();
    let public = configs
        .load_public_station_configuration(&path("config/public-stations/stations.json"))
        .unwrap();
    let tides = configs
        .load_tide_configuration(&path("config/tides/noaa.json"))
        .unwrap();
    let actual = array([
        array([
            version(site.version),
            text(&site.provider.key),
            text(&site.site.key),
            text(&site.station.key),
            array(site.sources.iter().map(|source| {
                array([
                    text(&source.key),
                    text(&source.fingerprint),
                    active(source.active),
                ])
            })),
        ]),
        array([
            version(ecowitt.version),
            text(&ecowitt.siteKey),
            text(&ecowitt.provider.key),
            array(ecowitt.stations.iter().map(|station| {
                array([
                    text(&station.key),
                    text(&station.sourceKey),
                    text(&station.fingerprint),
                    active(station.active),
                ])
            })),
        ]),
        array([
            version(tempest.version),
            text(&tempest.siteKey),
            text(&tempest.provider.key),
            array(tempest.stations.iter().map(|station| {
                array([
                    text(&station.key),
                    text(&station.sourceKey),
                    text(&station.fingerprint),
                    active(station.active),
                ])
            })),
        ]),
        array([
            version(public.version),
            text(&public.siteKey),
            array(public.providers.iter().map(|provider| text(&provider.key))),
            array(public.stations.iter().map(|station| {
                array([
                    text(&station.key),
                    active(station.active),
                    array(station.sources.iter().map(|source| {
                        array([
                            text(&source.key),
                            text(&source.providerKey),
                            text(&source.fingerprint),
                            active(source.active),
                        ])
                    })),
                ])
            })),
        ]),
        array([
            version(tides.version),
            text(&tides.siteKey),
            text(&tides.provider.key),
            array(tides.stations.iter().map(|station| {
                array([
                    text(&station.key),
                    active(station.active),
                    text(&station.source.key),
                    text(&station.source.fingerprint),
                    active(station.source.active),
                ])
            })),
        ]),
    ]);
    assert_eq!(
        NativeWirePrimitives.decode_json(&source.stdout),
        Some(actual)
    );
}

/// Retain the original origin and immutable migration directory normalization.
#[test]
fn native_worker_process_owners_admit_original_origin_and_artifact() {
    let mut origin = NativeWorkerCompatibilityOrigin;
    assert_eq!(
        origin.parse_open_meteo_compatibility_origin(None).unwrap(),
        None
    );
    assert_eq!(
        origin
            .parse_open_meteo_compatibility_origin(Some("HTTPS://Example.COM:443/?#"))
            .unwrap()
            .as_deref(),
        Some("https://example.com")
    );
    let mut artifact = NativeWorkerConfigurationArtifacts;
    let authorization = artifact
        .admit_migration_authorization("2026.11.25-3", &"a".repeat(64))
        .unwrap();
    assert_eq!(authorization.release, "2026.11.25-3");
    assert_eq!(authorization.history_sha256.as_str(), "a".repeat(64));
    let path = artifact
        .resolve_migration_directory("packages/../packages/database/migrations")
        .unwrap();
    assert_eq!(
        PathBuf::from(path),
        std::env::current_dir()
            .unwrap()
            .join("packages/database/migrations")
    );
}
