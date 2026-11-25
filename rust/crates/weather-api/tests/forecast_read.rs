use std::path::PathBuf;
use std::process::Command;

use weather_api::{
    CanonicalInstant, ForecastReadError, ForecastSelection, ResolvedForecastReadWindow,
    forecast_endpoint_read_plan, group_active_sites,
};
use weather_contracts::{SourceKind, StationKind};
use weather_database::{ActiveSiteRow, ForecastReadBind};

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

/// build one already-resolved forecast read window
fn window(
    as_of_epoch_milliseconds: i64,
    as_of: &str,
    end_epoch_milliseconds: i64,
    end_exclusive: &str,
    hours: u64,
) -> ResolvedForecastReadWindow {
    ResolvedForecastReadWindow {
        as_of: instant(as_of_epoch_milliseconds, as_of),
        end_exclusive: instant(end_epoch_milliseconds, end_exclusive),
        hours,
    }
}

// verifies compiled Node daily and overnight selections link to exact DB binds
#[test]
fn matches_compiled_node_forecast_read_selection() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/forecast-read-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run forecast endpoint Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let daily = forecast_endpoint_read_plan(
        &site(),
        ForecastSelection::Daily { days: 1 },
        &window(
            1_787_252_400_000,
            "2026-08-20T19:00:00.000Z",
            1_787_385_600_000,
            "2026-08-22T08:00:00.000Z",
            37,
        ),
    )
    .expect("daily forecast read plan");
    assert_eq!(
        daily.binds,
        [
            ForecastReadBind::Text("ballydidean".to_owned()),
            ForecastReadBind::UtcInstant("2026-08-20T19:00:00.000Z".to_owned()),
            ForecastReadBind::UtcInstant("2026-08-22T08:00:00.000Z".to_owned()),
        ]
    );
    assert!(daily.sql.contains("FROM public.forecast_products product"));

    let overnight = forecast_endpoint_read_plan(
        &site(),
        ForecastSelection::Overnight,
        &window(
            1_787_295_600_000,
            "2026-08-21T07:00:00.000Z",
            1_787_407_200_000,
            "2026-08-22T14:00:00.000Z",
            31,
        ),
    )
    .expect("overnight forecast read plan");
    assert!(overnight.sql.contains("SELECT MAX(product.product_run_at)"));
}

// verifies the ten-day compatibility read retains its expanded AQ horizon
#[test]
fn preserves_extended_daily_compatibility_window() {
    let extended = forecast_endpoint_read_plan(
        &site(),
        ForecastSelection::Daily { days: 10 },
        &window(
            1_787_252_400_000,
            "2026-08-20T19:00:00.000Z",
            1_788_163_200_000,
            "2026-08-31T08:00:00.000Z",
            253,
        ),
    )
    .expect("extended daily forecast read plan");
    assert_eq!(
        extended.binds[2],
        ForecastReadBind::UtcInstant("2026-08-31T08:00:00.000Z".to_owned())
    );
}

// verifies endpoint geometry rejects incompatible admitted durations
#[test]
fn rejects_incompatible_resolved_forecast_windows() {
    let daily = forecast_endpoint_read_plan(
        &site(),
        ForecastSelection::Daily { days: 1 },
        &window(
            1_787_252_400_000,
            "2026-08-20T19:00:00.000Z",
            1_787_342_400_000,
            "2026-08-21T20:00:00.000Z",
            25,
        ),
    );
    assert_eq!(daily, Err(ForecastReadError::IncompatibleResolvedWindow));

    let overnight = forecast_endpoint_read_plan(
        &site(),
        ForecastSelection::Overnight,
        &window(
            1_787_295_600_000,
            "2026-08-21T07:00:00.000Z",
            1_787_382_000_000,
            "2026-08-22T07:00:00.000Z",
            24,
        ),
    );
    assert_eq!(
        overnight,
        Err(ForecastReadError::IncompatibleResolvedWindow)
    );
}
