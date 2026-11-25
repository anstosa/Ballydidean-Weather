use weather_api::{CurrentSelection, QueryParameter, group_active_sites, parse_current_selection};
use weather_contracts::{SourceKind, StationKind};
use weather_database::ActiveSiteRow;

// build one active source row for current selection fixtures
fn row(station_slug: &str, source_id: &str) -> ActiveSiteRow {
    ActiveSiteRow {
        attribution_label: "Provider".to_owned(),
        attribution_url: "https://provider.test/".to_owned(),
        latitude: 47.95,
        longitude: -122.42,
        provider_key: "provider".to_owned(),
        provider_name: "Provider".to_owned(),
        site_name: "Ballydidean".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_id: source_id.to_owned(),
        source_key: format!("source-{source_id}"),
        source_kind: SourceKind::ModelCurrent,
        station_kind: StationKind::Virtual,
        station_latitude: 47.96,
        station_longitude: -122.43,
        station_name: station_slug.to_owned(),
        station_slug: station_slug.to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

// build one site with disjoint active station sources
fn site() -> weather_contracts::ApiSite {
    group_active_sites(vec![row("virtual", "10"), row("physical", "12")])
        .data
        .remove(0)
}

#[test]
// preserve absent and explicitly empty filters as an unfiltered selection
fn parses_unfiltered_current_selection() {
    assert_eq!(
        parse_current_selection(&[], &site()),
        Ok(CurrentSelection {
            source_id: None,
            station_slug: None,
        }),
    );
    assert_eq!(
        parse_current_selection(
            &[
                QueryParameter {
                    name: "station",
                    value: "",
                },
                QueryParameter {
                    name: "source",
                    value: "",
                },
            ],
            &site(),
        ),
        Ok(CurrentSelection {
            source_id: None,
            station_slug: None,
        }),
    );
}

#[test]
// select one active station and one source belonging to that station
fn parses_active_current_selection() {
    assert_eq!(
        parse_current_selection(
            &[
                QueryParameter {
                    name: "station",
                    value: "virtual",
                },
                QueryParameter {
                    name: "source",
                    value: "10",
                },
            ],
            &site(),
        ),
        Ok(CurrentSelection {
            source_id: Some("10".to_owned()),
            station_slug: Some("virtual".to_owned()),
        }),
    );
    assert_eq!(
        parse_current_selection(
            &[QueryParameter {
                name: "source",
                value: "12",
            }],
            &site(),
        ),
        Ok(CurrentSelection {
            source_id: Some("12".to_owned()),
            station_slug: None,
        }),
    );
}

#[test]
// reject unknown stations and sources outside the selected station
fn rejects_unknown_current_selection() {
    let missing_station = parse_current_selection(
        &[QueryParameter {
            name: "station",
            value: "missing",
        }],
        &site(),
    )
    .expect_err("station must be active");
    assert_eq!(missing_station.status, 404);
    assert_eq!(missing_station.code, "station_not_found");
    assert_eq!(missing_station.message, "Unknown station: missing");

    let unrelated_source = parse_current_selection(
        &[
            QueryParameter {
                name: "station",
                value: "virtual",
            },
            QueryParameter {
                name: "source",
                value: "12",
            },
        ],
        &site(),
    )
    .expect_err("source must belong to selected station");
    assert_eq!(unrelated_source.status, 404);
    assert_eq!(unrelated_source.code, "source_not_found");
    assert_eq!(unrelated_source.message, "Unknown source: 12");
}

#[test]
// preserve shared stable-key and positive source identifier failures
fn rejects_invalid_current_filter_values() {
    let simultaneous_invalid = parse_current_selection(
        &[
            QueryParameter {
                name: "source",
                value: "not-positive",
            },
            QueryParameter {
                name: "station",
                value: "Bad",
            },
        ],
        &site(),
    )
    .expect_err("current station syntax must retain precedence");
    assert_eq!(simultaneous_invalid.message, "Request query is invalid");

    let invalid_station = parse_current_selection(
        &[QueryParameter {
            name: "station",
            value: "Bad",
        }],
        &site(),
    )
    .expect_err("invalid station key must fail");
    assert_eq!(invalid_station.status, 400);
    assert_eq!(invalid_station.code, "invalid_query");
    assert_eq!(invalid_station.message, "Request query is invalid");

    // verify every non-positive or non-decimal source value
    for value in ["0", "01", "-1", "+1", "1x"] {
        let invalid_source = parse_current_selection(
            &[QueryParameter {
                name: "source",
                value,
            }],
            &site(),
        )
        .expect_err("invalid source id must fail");
        assert_eq!(invalid_source.status, 400);
        assert_eq!(invalid_source.message, "source must be a positive integer");
    }
}

#[test]
// preserve duplicate and unsupported field diagnostics
fn rejects_duplicate_and_unsupported_current_filters() {
    let duplicate = parse_current_selection(
        &[
            QueryParameter {
                name: "source",
                value: "10",
            },
            QueryParameter {
                name: "source",
                value: "10",
            },
        ],
        &site(),
    )
    .expect_err("duplicate must fail");
    assert_eq!(duplicate.message, "source may be provided once");

    let unsupported = parse_current_selection(
        &[QueryParameter {
            name: "other",
            value: "value",
        }],
        &site(),
    )
    .expect_err("unsupported field must fail");
    assert_eq!(unsupported.message, "Unsupported query parameter: other");
}
