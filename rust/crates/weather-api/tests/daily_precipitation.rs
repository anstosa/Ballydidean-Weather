use weather_api::{
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver,
    ResolvedDailyPrecipitationWindow, daily_precipitation_query, group_active_sites,
    project_daily_precipitation_response,
};
use weather_contracts::{DailyPrecipitationRow, JsDateOrString, Nullable, SourceKind, StationKind};
use weather_database::ActiveSiteRow;

struct FixtureTimeResolver;

impl CurrentTimeResolver for FixtureTimeResolver {
    /// resolve one node-derived fixture instant
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        // accept only the source fixture timestamp
        if value == &JsDateOrString::String(text("2026-08-22T04:59:00.000Z")) {
            return Ok(instant(1_787_374_740_000, "2026-08-22T04:59:00.000Z"));
        }

        Err(CurrentProjectionError {
            code: "daily_precipitation.time",
            detail: "fixture time adapter rejected the source timestamp".to_owned(),
        })
    }
}

/// encode one generated string
fn text(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// build one canonical instant
fn instant(epoch_milliseconds: i64, utc: &str) -> CanonicalInstant {
    CanonicalInstant {
        epoch_milliseconds,
        utc: utc.to_owned(),
    }
}

/// build one active physical precipitation source
fn active_source() -> ActiveSiteRow {
    ActiveSiteRow {
        attribution_label: "Weather data by Ecowitt".to_owned(),
        attribution_url: "https://www.ecowitt.com/".to_owned(),
        latitude: 47.95,
        longitude: -122.42,
        provider_key: "ecowitt-local".to_owned(),
        provider_name: "Ecowitt".to_owned(),
        site_name: "Ballydidean".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_id: "13".to_owned(),
        source_key: "ecowitt-rain".to_owned(),
        source_kind: SourceKind::PhysicalSensor,
        station_kind: StationKind::Physical,
        station_latitude: 47.951,
        station_longitude: -122.421,
        station_name: "Ballydidean Ecowitt".to_owned(),
        station_slug: "ballydidean-ecowitt".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

// verifies the pre-resolved node local-day query
#[test]
fn builds_daily_precipitation_query() {
    let site = group_active_sites(vec![active_source()]).data.remove(0);
    let query = daily_precipitation_query(
        &site,
        &ResolvedDailyPrecipitationWindow {
            from: instant(1_787_295_600_000, "2026-08-21T07:00:00.000Z"),
            to: instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z"),
        },
    )
    .expect("ordered daily window");

    assert_eq!(query.siteSlug, "ballydidean");
    assert_eq!(query.from, "2026-08-21T07:00:00.000Z");
    assert_eq!(query.to, "2026-08-22T05:00:00.000Z");
}

// verifies the canonical nullable endpoint response
#[test]
fn projects_daily_precipitation_response() {
    let site = group_active_sites(vec![active_source()]).data.remove(0);
    let generated_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let response = project_daily_precipitation_response(
        Some(DailyPrecipitationRow {
            accumulationMm: 2.54,
            sourceId: "13".to_owned(),
            stationSlug: "ballydidean-ecowitt".to_owned(),
            validThrough: JsDateOrString::String(text("2026-08-22T04:59:00.000Z")),
        }),
        &site,
        &generated_at,
        &FixtureTimeResolver,
    )
    .expect("daily precipitation row");

    assert_eq!(response.generatedAt, generated_at.utc);
    assert_eq!(response.site.slug, "ballydidean");
    let Nullable::Value(data) = response.data else {
        panic!("daily precipitation must be present");
    };
    assert_eq!(data.accumulationMm, 2.54);
    assert_eq!(data.source.sourceId, "13");
    assert_eq!(data.source.stationSlug, "ballydidean-ecowitt");
    assert_eq!(data.validThrough, "2026-08-22T04:59:00.000Z");

    let absent =
        project_daily_precipitation_response(None, &site, &generated_at, &FixtureTimeResolver)
            .expect("absent daily precipitation");
    assert_eq!(absent.data, Nullable::Null);
}

// verifies ordered windows and active source ownership
#[test]
fn rejects_invalid_daily_precipitation_boundaries() {
    let site = group_active_sites(vec![active_source()]).data.remove(0);
    let instant = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let window_error = daily_precipitation_query(
        &site,
        &ResolvedDailyPrecipitationWindow {
            from: instant.clone(),
            to: instant.clone(),
        },
    )
    .expect_err("unordered daily window");
    assert_eq!(window_error.code, "daily_precipitation.window");

    let source_error = project_daily_precipitation_response(
        Some(DailyPrecipitationRow {
            accumulationMm: 2.54,
            sourceId: "99".to_owned(),
            stationSlug: "unknown".to_owned(),
            validThrough: JsDateOrString::String(text("2026-08-22T04:59:00.000Z")),
        }),
        &site,
        &instant,
        &FixtureTimeResolver,
    )
    .expect_err("unknown daily source");
    assert_eq!(source_error.code, "daily_precipitation.source");
}
