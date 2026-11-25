use std::path::PathBuf;
use std::process::Command;

use weather_api::{
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver,
    ResolvedForecastPressureContextWindow, forecast_pressure_context_read_plan, group_active_sites,
    project_forecast_pressure_context,
};
use weather_contracts::{JsDateOrString, Nullable, SourceKind, StationKind, WeatherRecordRow};
use weather_database::{ActiveSiteRow, ForecastPressureContextBind};

struct FixtureTimeResolver;

impl CurrentTimeResolver for FixtureTimeResolver {
    /// resolve only source-derived pressure-context instants
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        // preserve every explicit database epoch and UTC pair
        match value {
            JsDateOrString::DateEpochMilliseconds(1_787_284_800_000) => {
                Ok(instant(1_787_284_800_000, "2026-08-21T04:00:00.000Z"))
            }
            JsDateOrString::DateEpochMilliseconds(1_787_288_400_000) => {
                Ok(instant(1_787_288_400_000, "2026-08-21T05:00:00.000Z"))
            }
            JsDateOrString::DateEpochMilliseconds(1_787_292_000_000) => {
                Ok(instant(1_787_292_000_000, "2026-08-21T06:00:00.000Z"))
            }
            JsDateOrString::DateEpochMilliseconds(1_787_295_600_000) => {
                Ok(instant(1_787_295_600_000, "2026-08-21T07:00:00.000Z"))
            }
            JsDateOrString::DateEpochMilliseconds(1_787_299_200_000) => {
                Ok(instant(1_787_299_200_000, "2026-08-21T08:00:00.000Z"))
            }
            JsDateOrString::DateEpochMilliseconds(1_787_302_800_000) => {
                Ok(instant(1_787_302_800_000, "2026-08-21T09:00:00.000Z"))
            }
            JsDateOrString::DateEpochMilliseconds(1_787_292_300_000) => {
                Ok(instant(1_787_292_300_000, "2026-08-21T06:05:00.000Z"))
            }
            _ => Err(CurrentProjectionError {
                code: "forecast_pressure.time",
                detail: "fixture time resolver rejected the pressure context timestamp".to_owned(),
            }),
        }
    }
}

/// build one admitted canonical instant
fn instant(epoch_milliseconds: i64, utc: &str) -> CanonicalInstant {
    CanonicalInstant {
        epoch_milliseconds,
        utc: utc.to_owned(),
    }
}

/// build one active forecast source row
fn active_source() -> ActiveSiteRow {
    ActiveSiteRow {
        attribution_label: "Weather data by Open-Meteo".to_owned(),
        attribution_url: "https://open-meteo.com/".to_owned(),
        latitude: 47.95,
        longitude: -122.42,
        provider_key: "open-meteo".to_owned(),
        provider_name: "Open-Meteo".to_owned(),
        site_name: "Ballydidean".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_id: "12".to_owned(),
        source_key: "open-meteo-forecast-v4".to_owned(),
        source_kind: SourceKind::Forecast,
        station_kind: StationKind::Virtual,
        station_latitude: 47.95,
        station_longitude: -122.42,
        station_name: "Open-Meteo Virtual".to_owned(),
        station_slug: "open-meteo-virtual".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

/// build the one active forecast site fixture
fn site() -> weather_contracts::ApiSite {
    group_active_sites(vec![active_source()]).data.remove(0)
}

/// return one fixed hourly context epoch
fn valid_at(index: usize) -> i64 {
    // retain the six complete hourly slots in repository order
    match index {
        0 => 1_787_284_800_000,
        1 => 1_787_288_400_000,
        2 => 1_787_292_000_000,
        3 => 1_787_295_600_000,
        4 => 1_787_299_200_000,
        5 => 1_787_302_800_000,
        _ => panic!("pressure context fixture index is out of range"),
    }
}

/// build one complete raw pressure-context row
fn context_row(index: usize) -> WeatherRecordRow {
    WeatherRecordRow {
        adapterVersion: Nullable::Null,
        apparentTemperatureC: Nullable::Value(8.0 + index as f64),
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Value(40.0 + index as f64),
        contentHash: format!("context-{index}"),
        contractEpoch: Nullable::Null,
        deviceModel: Nullable::Null,
        deviceSerial: Nullable::Null,
        deviceVendor: Nullable::Null,
        firstReceivedAt: JsDateOrString::DateEpochMilliseconds(1_787_292_300_000),
        id: (300 + index).to_string(),
        lastReceivedAt: JsDateOrString::DateEpochMilliseconds(1_787_292_300_000),
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Value(0.1 * index as f64),
        precipitationRateMmPerHour: Nullable::Value(0.1 * index as f64),
        pressureHpa: Nullable::Value(1010.0 + index as f64),
        productRunAt: Nullable::Value(JsDateOrString::DateEpochMilliseconds(1_787_292_000_000)),
        providerKey: "open-meteo".to_owned(),
        providerMetadata: Nullable::Null,
        qualityMetadata: Nullable::Null,
        relativeHumidityPercent: Nullable::Value(80.0 - index as f64),
        revisionCount: 0,
        siteSlug: "ballydidean".to_owned(),
        sourceId: "12".to_owned(),
        sourceConfigFingerprint: Nullable::Null,
        sourceKey: "open-meteo-forecast-v4".to_owned(),
        sourceKind: SourceKind::Forecast,
        stationSlug: "open-meteo-virtual".to_owned(),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Value(10.0 + index as f64),
        upstreamModel: Nullable::Value("best_match".to_owned()),
        upstreamTimezone: "GMT".to_owned(),
        uvIndex: Nullable::Null,
        validAt: JsDateOrString::DateEpochMilliseconds(valid_at(index)),
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Value(180.0),
        windGustMps: Nullable::Value(3.0 + index as f64),
        windSpeedMps: Nullable::Value(2.0 + index as f64),
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

/// build the exact admitted pressure-context offsets
fn window() -> ResolvedForecastPressureContextWindow {
    ResolvedForecastPressureContextWindow {
        as_of: instant(1_787_295_600_000, "2026-08-21T07:00:00.000Z"),
        context_start: instant(1_787_284_800_000, "2026-08-21T04:00:00.000Z"),
        context_end: instant(1_787_306_400_000, "2026-08-21T10:00:00.000Z"),
        oldest_run: instant(1_787_122_800_000, "2026-08-19T07:00:00.000Z"),
    }
}

// verifies compiled Node SQL/route behavior and the linked API read binds
#[test]
fn matches_compiled_node_pressure_context_path() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../weather-database/tests/forecast-pressure-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run forecast pressure Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let plan = forecast_pressure_context_read_plan(&site(), &window())
        .expect("linked pressure context plan");
    assert_eq!(
        plan.binds,
        [
            ForecastPressureContextBind::Text("ballydidean".to_owned()),
            ForecastPressureContextBind::UtcInstant("2026-08-21T04:00:00.000Z".to_owned()),
            ForecastPressureContextBind::UtcInstant("2026-08-21T10:00:00.000Z".to_owned()),
            ForecastPressureContextBind::UtcInstant("2026-08-19T07:00:00.000Z".to_owned()),
            ForecastPressureContextBind::UtcInstant("2026-08-21T07:00:00.000Z".to_owned()),
        ]
    );
}

// verifies every raw numerical value and repository row order remain unchanged
#[test]
fn projects_unadjusted_pressure_context_numbers() {
    let rows = (0..6).map(context_row).collect::<Vec<_>>();
    let generated_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let records =
        project_forecast_pressure_context(rows, &site(), &generated_at, &FixtureTimeResolver);

    assert_eq!(records.len(), 6);
    assert_eq!(records[0].id, "300");
    assert_eq!(records[5].id, "305");
    assert_eq!(records[0].metrics.pressureHpa, Nullable::Value(1010.0));
    assert_eq!(records[5].metrics.pressureHpa, Nullable::Value(1015.0));
    assert_eq!(records[0].metrics.temperatureC, Nullable::Value(10.0));
    assert_eq!(records[0].metrics.windSpeedMps, Nullable::Value(2.0));
    assert_eq!(
        records[0].productRunAt,
        Nullable::Value("2026-08-21T06:00:00.000Z".to_owned())
    );
    assert_eq!(records[5].validAt, "2026-08-21T09:00:00.000Z");
}

// verifies malformed optional source context collapses to an empty result
#[test]
fn isolates_invalid_pressure_context_projection() {
    let mut row = context_row(0);
    row.sourceId = "99".to_owned();
    let generated_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let records =
        project_forecast_pressure_context(vec![row], &site(), &generated_at, &FixtureTimeResolver);
    assert!(records.is_empty());
}
