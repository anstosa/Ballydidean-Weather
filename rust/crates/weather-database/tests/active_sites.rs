use weather_contracts::{SourceKind, StationKind};
use weather_database::{
    ActiveSiteDbRow, LIST_ACTIVE_SITES_SQL, list_active_sites_query, project_active_site_row,
    project_active_site_rows,
};

// build one representative database row for projection tests
fn row(source_key: &str, source_kind: &str, station_kind: &str) -> ActiveSiteDbRow {
    ActiveSiteDbRow {
        attribution_label: "Open-Meteo".to_owned(),
        attribution_url: "https://open-meteo.com/".to_owned(),
        latitude: 47.950429954185445,
        longitude: -122.42797012608193,
        provider_key: "open-meteo".to_owned(),
        provider_name: "Open-Meteo".to_owned(),
        site_name: "Ballydidean".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_id: "10".to_owned(),
        source_key: source_key.to_owned(),
        source_kind: source_kind.to_owned(),
        station_kind: station_kind.to_owned(),
        station_latitude: 47.950429954185445,
        station_longitude: -122.42797012608193,
        station_name: "Ballydidean virtual station".to_owned(),
        station_slug: "open-meteo-virtual".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

#[test]
// bind the repository plan to the exact SQL fixture and zero parameters
fn builds_the_exact_zero_bind_query() {
    let query = list_active_sites_query();
    assert_eq!(query.sql, LIST_ACTIVE_SITES_SQL);
    assert_eq!(query.binds, []);
    assert!(query.sql.starts_with("\n      SELECT\n"));
    assert!(
        query
            .sql
            .ends_with("      ORDER BY si.slug, st.slug, s.source_key\n    ")
    );
}

#[test]
// retain the source-owned active and capability predicates
fn retains_discovery_predicates_and_order() {
    let sql = list_active_sites_query().sql;
    assert!(sql.contains("JOIN stations st ON st.site_id = si.id AND st.active"));
    assert!(sql.contains("JOIN sources s ON s.station_id = st.id AND s.active"));
    assert!(sql.contains("JOIN providers p ON p.id = s.provider_id AND p.active"));
    assert!(sql.contains("WHERE si.active"));
    assert!(sql.contains("s.source_kind <> 'forecast'"));
    assert!(sql.contains("s.capabilities @> '[\"forecast\"]'::jsonb"));
    assert!(sql.contains("weather_source_is_current(s.id)"));
    assert!(sql.contains("ORDER BY si.slug, st.slug, s.source_key"));
}

#[test]
// project scalar fields and generated enum types without changing values
fn projects_one_active_site_row() {
    let projected = project_active_site_row(row("open-meteo-forecast-v4", "forecast", "virtual"))
        .expect("row should project");

    assert_eq!(projected.source_kind, SourceKind::Forecast);
    assert_eq!(projected.station_kind, StationKind::Virtual);
    assert_eq!(projected.source_key, "open-meteo-forecast-v4");
    assert_eq!(projected.site_slug, "ballydidean");
    assert_eq!(projected.latitude, 47.950429954185445);
    assert_eq!(projected.longitude, -122.42797012608193);
}

#[test]
// preserve the database statement's row order through projection
fn preserves_ordered_rows() {
    let rows = vec![
        row("open-meteo-current-v1", "model_current", "virtual"),
        row("open-meteo-forecast-v4", "forecast", "virtual"),
        row("open-meteo-reanalysis-v1", "reanalysis", "virtual"),
    ];
    let projected = project_active_site_rows(rows).expect("rows should project");
    let keys = projected
        .iter()
        .map(|row| row.source_key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "open-meteo-current-v1",
            "open-meteo-forecast-v4",
            "open-meteo-reanalysis-v1",
        ],
    );
}

#[test]
// reject database strings outside the generated contract enums
fn rejects_unknown_generated_enum_values() {
    let source_error = project_active_site_row(row("source", "unknown", "virtual"))
        .expect_err("unknown source kind must fail");
    assert_eq!(source_error.field, "sourceKind");
    assert_eq!(source_error.value, "unknown");

    let station_error = project_active_site_row(row("source", "forecast", "unknown"))
        .expect_err("unknown station kind must fail");
    assert_eq!(station_error.field, "stationKind");
    assert_eq!(station_error.value, "unknown");
}
