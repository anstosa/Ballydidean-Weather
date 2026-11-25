use std::path::PathBuf;
use std::process::Command;

use weather_api::{
    CanonicalInstant, CurrentProjectionError, LegacyTrendRange, QueryParameter, TrendTimeBoundary,
    calendar_trend_query, group_active_sites, parse_legacy_trend_range, project_trends_response,
    trends_read_plan,
};
use weather_contracts::{JsDate, Nullable, SourceKind, StationKind, TrendPointRow};
use weather_database::{ActiveSiteRow, TrendReadBind};

struct FixtureTimeBoundary;

impl TrendTimeBoundary for FixtureTimeBoundary {
    /// resolve only the source-derived fixed calendar start
    fn calendar_start(
        &self,
        generated_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<CanonicalInstant, CurrentProjectionError> {
        assert_eq!(generated_at.utc, "2026-08-22T05:00:00.000Z");
        assert_eq!(timezone, "America/Los_Angeles");
        Ok(instant(1_546_329_600_000, "2019-01-01T08:00:00.000Z"))
    }

    /// resolve only the decoded source bucket dates
    fn resolve_row_time(&self, value: &JsDate) -> Result<CanonicalInstant, CurrentProjectionError> {
        // preserve the two source-derived date and UTC pairs
        match value {
            JsDate::EpochMilliseconds(1_546_329_600_000) => {
                Ok(instant(1_546_329_600_000, "2019-01-01T08:00:00.000Z"))
            }
            JsDate::EpochMilliseconds(1_787_295_600_000) => {
                Ok(instant(1_787_295_600_000, "2026-08-21T07:00:00.000Z"))
            }
            _ => Err(CurrentProjectionError {
                code: "trends.time",
                detail: "fixture time boundary rejected the trend bucket".to_owned(),
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

/// build one active reanalysis source row
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
        source_id: "11".to_owned(),
        source_key: "open-meteo-reanalysis-v1".to_owned(),
        source_kind: SourceKind::Reanalysis,
        station_kind: StationKind::Virtual,
        station_latitude: 47.95,
        station_longitude: -122.42,
        station_name: "Open-Meteo Virtual".to_owned(),
        station_slug: "open-meteo-virtual".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

/// build the one active trend site fixture
fn site() -> weather_contracts::ApiSite {
    group_active_sites(vec![active_source()]).data.remove(0)
}

/// build one complete typed trend bucket
fn row(valid_at: i64, apparent_temperature_c: Nullable<f64>, temperature_c: f64) -> TrendPointRow {
    TrendPointRow {
        apparentTemperatureC: apparent_temperature_c,
        pm25MicrogramsPerCubicMeter: Nullable::Value(12.3),
        precipitationMm: Nullable::Value(1.2),
        pressureHpa: Nullable::Value(1012.4),
        relativeHumidityPercent: Nullable::Value(82.0),
        temperatureC: Nullable::Value(temperature_c),
        temperatureMaximumC: Nullable::Value(19.4),
        temperatureMinimumC: Nullable::Value(8.2),
        validAt: JsDate::EpochMilliseconds(valid_at),
        windDirectionDegrees: Nullable::Value(-135.0),
        windGustMps: Nullable::Value(7.2),
        windSpeedMps: Nullable::Value(4.1),
    }
}

// verifies the compiled Node calendar route, exact read plan, and DTO projection
#[test]
fn matches_compiled_node_trends_query_and_projection() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/trends-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run trends API Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let generated_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let parsed = calendar_trend_query(&site(), &generated_at, &FixtureTimeBoundary)
        .expect("calendar trend query");
    assert_eq!(parsed.query.from, "2019-01-01T08:00:00.000Z");
    assert_eq!(parsed.query.siteSlug, "ballydidean");
    assert_eq!(parsed.query.to, generated_at.utc);
    let plan = trends_read_plan(&parsed).expect("linked trend database plan");
    assert_eq!(
        plan.binds,
        [
            TrendReadBind::Text("ballydidean".to_owned()),
            TrendReadBind::UtcInstant("2019-01-01T08:00:00.000Z".to_owned()),
            TrendReadBind::UtcInstant("2026-08-22T05:00:00.000Z".to_owned()),
        ]
    );

    let response = project_trends_response(
        vec![
            row(1_546_329_600_000, Nullable::Value(10.1), 11.5),
            row(1_787_295_600_000, Nullable::Value(f64::INFINITY), 16.2),
        ],
        site(),
        &generated_at,
        &FixtureTimeBoundary,
    )
    .expect("trends response");
    assert_eq!(response.generatedAt, generated_at.utc);
    assert_eq!(response.data.len(), 2);
    assert_eq!(response.data[0].validAt, "2019-01-01T08:00:00.000Z");
    assert_eq!(response.data[0].metrics.temperatureC, Nullable::Value(11.5));
    assert_eq!(
        response.data[0].metrics.windDirectionDegrees,
        Nullable::Value(-135.0)
    );
    assert_eq!(
        response.data[1].metrics.apparentTemperatureC,
        Nullable::Null
    );
}

// verifies all legacy selectors remain accepted but do not choose the calendar window
#[test]
fn parses_legacy_trend_ranges() {
    assert_eq!(parse_legacy_trend_range(&[]), Ok(None));
    assert_eq!(
        parse_legacy_trend_range(&[QueryParameter {
            name: "range",
            value: "",
        }]),
        Ok(None)
    );

    // preserve every accepted cached-client selector
    for (value, expected) in [
        ("24h", LegacyTrendRange::TwentyFourHours),
        ("7d", LegacyTrendRange::SevenDays),
        ("30d", LegacyTrendRange::ThirtyDays),
    ] {
        assert_eq!(
            parse_legacy_trend_range(&[QueryParameter {
                name: "range",
                value,
            }]),
            Ok(Some(expected))
        );
    }
}

// verifies invalid, duplicate, station, and source filters fail closed
#[test]
fn rejects_invalid_trend_filters() {
    let invalid = parse_legacy_trend_range(&[QueryParameter {
        name: "range",
        value: "90d",
    }])
    .expect_err("unsupported legacy range");
    assert_eq!(invalid.message, "Request query is invalid");

    let duplicate = parse_legacy_trend_range(&[
        QueryParameter {
            name: "range",
            value: "24h",
        },
        QueryParameter {
            name: "range",
            value: "7d",
        },
    ])
    .expect_err("duplicate legacy range");
    assert_eq!(duplicate.message, "range may be provided once");

    // preserve the trends route's lack of station and source selectors
    for name in ["station", "source"] {
        let rejected = parse_legacy_trend_range(&[QueryParameter {
            name,
            value: "selected",
        }])
        .expect_err("unsupported trend filter");
        assert_eq!(
            rejected.message,
            format!("Unsupported query parameter: {name}")
        );
    }
}
