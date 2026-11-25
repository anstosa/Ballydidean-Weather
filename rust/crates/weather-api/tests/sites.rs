use weather_api::group_active_sites;
use weather_contracts::{SourceKind, StationKind};
use weather_database::ActiveSiteRow;

// build one typed active-site repository row
fn row(
    site_slug: &str,
    station_slug: &str,
    source_id: &str,
    source_key: &str,
    source_kind: SourceKind,
) -> ActiveSiteRow {
    ActiveSiteRow {
        attribution_label: "Weather data by provider".to_owned(),
        attribution_url: "https://provider.test/".to_owned(),
        latitude: 47.95,
        longitude: -122.42,
        provider_key: "provider".to_owned(),
        provider_name: "Provider".to_owned(),
        site_name: format!("Site {site_slug}"),
        site_slug: site_slug.to_owned(),
        source_id: source_id.to_owned(),
        source_key: source_key.to_owned(),
        source_kind,
        station_kind: StationKind::Virtual,
        station_latitude: 47.96,
        station_longitude: -122.43,
        station_name: format!("Station {station_slug}"),
        station_slug: station_slug.to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

#[test]
// group ordered repository rows into first-seen sites stations and sources
fn groups_active_site_rows_in_source_order() {
    let response = group_active_sites(vec![
        row(
            "ballydidean",
            "virtual",
            "10",
            "current",
            SourceKind::ModelCurrent,
        ),
        row(
            "ballydidean",
            "virtual",
            "11",
            "forecast",
            SourceKind::Forecast,
        ),
        row(
            "ballydidean",
            "physical",
            "12",
            "sensor",
            SourceKind::PhysicalSensor,
        ),
        row("second", "virtual", "13", "history", SourceKind::Reanalysis),
    ]);

    assert_eq!(response.data.len(), 2);
    assert_eq!(response.data[0].slug, "ballydidean");
    assert_eq!(response.data[1].slug, "second");
    assert_eq!(response.data[0].stations.len(), 2);
    assert_eq!(response.data[0].stations[0].slug, "virtual");
    assert_eq!(response.data[0].stations[1].slug, "physical");
    assert_eq!(response.data[0].stations[0].sources.len(), 2);
    assert_eq!(response.data[0].stations[0].sources[0].key, "current");
    assert_eq!(response.data[0].stations[0].sources[1].key, "forecast");
}

#[test]
// project exact attribution identity generated kinds and provenance labels
fn projects_canonical_source_fields() {
    let response = group_active_sites(vec![row(
        "ballydidean",
        "virtual",
        "12",
        "open-meteo-forecast-v4",
        SourceKind::Forecast,
    )]);
    let site = &response.data[0];
    let station = &site.stations[0];
    let source = &station.sources[0];

    assert_eq!(site.latitude, 47.95);
    assert_eq!(site.longitude, -122.42);
    assert_eq!(site.timezone, "America/Los_Angeles");
    assert_eq!(station.kind, StationKind::Virtual);
    assert_eq!(station.latitude, 47.96);
    assert_eq!(station.longitude, -122.43);
    assert_eq!(source.id, "12");
    assert_eq!(source.kind, SourceKind::Forecast);
    assert_eq!(source.providerKey, "provider");
    assert_eq!(source.providerName, "Provider");
    assert_eq!(source.attribution.label, "Weather data by provider");
    assert_eq!(source.attribution.url, "https://provider.test/");
    assert_eq!(source.provenanceLabel, "forecast model product");
}

#[test]
// preserve first-row metadata just like the map-backed Node projection
fn preserves_first_site_and_station_metadata() {
    let first = row(
        "ballydidean",
        "virtual",
        "10",
        "current",
        SourceKind::ModelCurrent,
    );
    let mut second = row(
        "ballydidean",
        "virtual",
        "11",
        "history",
        SourceKind::Reanalysis,
    );
    second.site_name = "Ignored later site name".to_owned();
    second.station_name = "Ignored later station name".to_owned();
    second.latitude = 0.0;
    second.station_latitude = 0.0;

    let response = group_active_sites(vec![first, second]);
    assert_eq!(response.data[0].name, "Site ballydidean");
    assert_eq!(response.data[0].latitude, 47.95);
    assert_eq!(response.data[0].stations[0].name, "Station virtual");
    assert_eq!(response.data[0].stations[0].latitude, 47.96);
}

#[test]
// represent an empty repository result without nullable or placeholder sites
fn keeps_empty_discovery_empty() {
    let response = group_active_sites(Vec::new());
    assert!(response.data.is_empty());
}

#[test]
// map every generated source kind to the canonical public provenance label
fn labels_every_source_kind() {
    let kinds = [
        (SourceKind::PhysicalSensor, "physical sensor measurement"),
        (SourceKind::ModelCurrent, "model-derived current conditions"),
        (SourceKind::Reanalysis, "gridded historical reanalysis"),
        (SourceKind::Forecast, "forecast model product"),
        (SourceKind::TideObservation, "observed tide level"),
        (SourceKind::TidePrediction, "tide prediction"),
    ];

    // verify every generated source kind remains exhaustively labeled
    for (index, (kind, expected)) in kinds.into_iter().enumerate() {
        let response = group_active_sites(vec![row(
            "ballydidean",
            "station",
            &index.to_string(),
            &format!("source-{index}"),
            kind,
        )]);
        assert_eq!(
            response.data[0].stations[0].sources[0].provenanceLabel,
            expected,
        );
    }
}
