use weather_api::{
    BodyPolicy, DatabaseStatus, Dispatch, ErrorDispatch, ForecastSelection, HealthSnapshot,
    MigrationStatus, QueryParameter, Route, WorkerFreshness, dispatch_decoded_request,
    parse_forecast_selection, project_health,
};

// unwrap one successful dispatch for focused route assertions
fn read(method: &str, path: &str, query_names: &[&str]) -> weather_api::ReadDispatch {
    match dispatch_decoded_request(method, path, query_names) {
        Dispatch::Read(dispatch) => dispatch,
        Dispatch::Error(error) => panic!("unexpected dispatch error: {error:?}"),
    }
}

// unwrap one failed dispatch for focused error assertions
fn error(method: &str, path: &str, query_names: &[&str]) -> ErrorDispatch {
    match dispatch_decoded_request(method, path, query_names) {
        Dispatch::Error(error) => error,
        Dispatch::Read(dispatch) => panic!("unexpected read dispatch: {dispatch:?}"),
    }
}

#[test]
// verify the exact documented public path taxonomy
fn matches_every_public_read_route() {
    let cases = [
        ("/api/v1/sites", Route::Sites),
        ("/api/v1/health", Route::Health),
        (
            "/api/v1/sites/ballydidean/current",
            Route::Current {
                site_slug: "ballydidean".to_owned(),
            },
        ),
        (
            "/api/v1/sites/ballydidean/daily-precipitation",
            Route::DailyPrecipitation {
                site_slug: "ballydidean".to_owned(),
            },
        ),
        (
            "/api/v1/sites/ballydidean/forecast",
            Route::Forecast {
                site_slug: "ballydidean".to_owned(),
            },
        ),
        (
            "/api/v1/sites/ballydidean/rain-collection",
            Route::RainCollection {
                site_slug: "ballydidean".to_owned(),
            },
        ),
        (
            "/api/v1/sites/ballydidean/history",
            Route::History {
                site_slug: "ballydidean".to_owned(),
            },
        ),
        (
            "/api/v1/sites/ballydidean/tides",
            Route::Tides {
                site_slug: "ballydidean".to_owned(),
            },
        ),
        (
            "/api/v1/sites/ballydidean/trends",
            Route::Trends {
                site_slug: "ballydidean".to_owned(),
            },
        ),
    ];

    // verify every source-owned route
    for (path, expected) in cases {
        let dispatch = read("GET", path, &[]);
        assert_eq!(dispatch.route, expected);
        assert_eq!(dispatch.body_policy, BodyPolicy::Include);
    }
}

#[test]
// keep the fixed private read outside public application dispatch
fn matches_the_fixed_observation_input() {
    let Dispatch::Error(error) =
        dispatch_decoded_request("GET", "/internal/v1/forecast-observation-input", &[])
    else {
        panic!("private route must not enter public dispatch");
    };
    assert_eq!(error.status, 404);
    assert_eq!(error.code, "not_found");
}

#[test]
// preserve bodyless HEAD semantics without changing route selection
fn omits_the_body_for_head() {
    let dispatch = read("HEAD", "/api/v1/sites/ballydidean/current", &[]);
    assert_eq!(dispatch.body_policy, BodyPolicy::Omit);
    assert_eq!(
        dispatch.route,
        Route::Current {
            site_slug: "ballydidean".to_owned()
        },
    );
}

#[test]
// preserve the shared read-only method error and Allow contract
fn rejects_mutation_methods_after_matching_the_route() {
    let failure = error("POST", "/api/v1/sites", &[]);
    assert_eq!(failure.status, 405);
    assert_eq!(failure.code, "method_not_allowed");
    assert_eq!(failure.message, "Only GET and HEAD are supported");
    assert!(failure.allow_get_head);
}

#[test]
// preserve route-before-method error ordering
fn rejects_unknown_mutation_paths_as_not_found() {
    let failure = error("POST", "/api/v1/unknown", &[]);
    assert_eq!(failure.status, 404);
    assert_eq!(failure.code, "not_found");
    assert!(!failure.allow_get_head);
}

#[test]
// reject every non-canonical or undocumented route shape
fn rejects_paths_outside_the_frozen_surface() {
    let rejected = [
        "/",
        "/sites",
        "/api/sites",
        "/api/v1",
        "/api/v1/sites/",
        "/api/v1/sites/ballydidean",
        "/api/v1/sites/ballydidean/current/extra",
        "/api/v1/sites/Bad/current",
        "/api/v1/sites/bad--key/current",
        "/api/v2/sites",
    ];

    // verify the complete rejected path fixture
    for path in rejected {
        assert_eq!(error("GET", path, &[]).status, 404, "{path}");
    }
}

#[test]
// preserve every route's query-name allowlist before value parsing
fn accepts_route_specific_query_names() {
    assert!(matches!(
        dispatch_decoded_request("GET", "/api/v1/sites/x/current", &["station", "source"]),
        Dispatch::Read(_),
    ));
    assert!(matches!(
        dispatch_decoded_request("GET", "/api/v1/sites/x/forecast", &["days", "window"]),
        Dispatch::Read(_),
    ));
    assert!(matches!(
        dispatch_decoded_request(
            "GET",
            "/api/v1/sites/x/history",
            &[
                "cursor",
                "from",
                "limit",
                "source",
                "sourceKind",
                "station",
                "to"
            ],
        ),
        Dispatch::Read(_),
    ));
    assert!(matches!(
        dispatch_decoded_request("GET", "/api/v1/sites/x/tides", &["from", "to"]),
        Dispatch::Read(_),
    ));
    assert!(matches!(
        dispatch_decoded_request("GET", "/api/v1/sites/x/trends", &["range"]),
        Dispatch::Read(_),
    ));
}

#[test]
// retain the first unsupported query name in the public response
fn rejects_unsupported_query_names() {
    let failure = error(
        "GET",
        "/api/v1/sites/x/forecast",
        &["days", "other", "later"],
    );
    assert_eq!(failure.status, 400);
    assert_eq!(failure.code, "invalid_query");
    assert_eq!(failure.message, "Unsupported query parameter: other");
    assert!(!failure.allow_get_head);
}

#[test]
// preserve no-query routes as closed request shapes
fn rejects_query_names_on_fixed_routes() {
    for path in [
        "/api/v1/sites",
        "/api/v1/health",
        "/api/v1/sites/x/daily-precipitation",
        "/api/v1/sites/x/rain-collection",
    ] {
        assert_eq!(error("GET", path, &["from"]).status, 400, "{path}");
    }
}

#[test]
// project ready database and migration state with one fresh worker heartbeat
fn projects_ready_health() {
    let projection = project_health(
        HealthSnapshot {
            database: DatabaseStatus::Ready,
            migration_status: MigrationStatus::Current,
            migration_version: Some("0001_initial_weather.sql".to_owned()),
            worker_last_loop_epoch_ms: Some(10_000),
        },
        10_000 + 1_800_000,
        "2026.08.22-1",
    );

    assert_eq!(projection.database, DatabaseStatus::Ready);
    assert!(projection.live);
    assert!(projection.ready);
    assert_eq!(projection.status, 200);
    assert_eq!(projection.worker_freshness, WorkerFreshness::Fresh);
    assert_eq!(projection.version, "2026.08.22-1");
}

#[test]
// match node's whole-second freshness boundary on both sides
fn classifies_fractional_second_worker_heartbeat_boundary() {
    let safety = 2_000_000;

    // preserve a fresh heartbeat through the last millisecond of the threshold second
    for (age_ms, expected) in [
        (-1, WorkerFreshness::Stale),
        (0, WorkerFreshness::Fresh),
        (1_800_000, WorkerFreshness::Fresh),
        (1_800_001, WorkerFreshness::Fresh),
        (1_800_999, WorkerFreshness::Fresh),
        (1_801_000, WorkerFreshness::Stale),
    ] {
        let projection = project_health(
            HealthSnapshot {
                database: DatabaseStatus::Ready,
                migration_status: MigrationStatus::Current,
                migration_version: Some("current".to_owned()),
                worker_last_loop_epoch_ms: Some(safety - age_ms),
            },
            safety,
            "development",
        );
        assert_eq!(projection.worker_freshness, expected, "age {age_ms}");
    }
}

#[test]
// keep liveness true while database readiness is unavailable
fn projects_unavailable_health() {
    let projection = project_health(
        HealthSnapshot {
            database: DatabaseStatus::Unavailable,
            migration_status: MigrationStatus::Unavailable,
            migration_version: None,
            worker_last_loop_epoch_ms: None,
        },
        10_000,
        "development",
    );

    assert!(projection.live);
    assert!(!projection.ready);
    assert_eq!(projection.status, 503);
    assert_eq!(projection.worker_freshness, WorkerFreshness::Unknown);
}

#[test]
// keep worker freshness independent from database readiness
fn classifies_stale_and_future_worker_heartbeats() {
    // verify an expired and a future heartbeat
    for heartbeat in [Some(10_000), Some(4_000_001)] {
        let projection = project_health(
            HealthSnapshot {
                database: DatabaseStatus::Ready,
                migration_status: MigrationStatus::Current,
                migration_version: Some("current".to_owned()),
                worker_last_loop_epoch_ms: heartbeat,
            },
            2_000_000,
            "development",
        );

        assert!(projection.ready);
        assert_eq!(projection.status, 200);
        assert_eq!(projection.worker_freshness, WorkerFreshness::Stale);
    }
}

#[test]
// retain the parameter-free and supported daily forecast horizons
fn parses_daily_forecast_selection() {
    assert_eq!(
        parse_forecast_selection(&[]),
        Ok(ForecastSelection::Daily { days: 1 }),
    );

    // verify every supported explicit horizon
    for (value, days) in [("1", 1), ("5", 5), ("10", 10)] {
        assert_eq!(
            parse_forecast_selection(&[QueryParameter {
                name: "days",
                value,
            }]),
            Ok(ForecastSelection::Daily { days }),
        );
    }

    assert_eq!(
        parse_forecast_selection(&[QueryParameter {
            name: "days",
            value: "",
        }]),
        Ok(ForecastSelection::Daily { days: 1 }),
    );
}

#[test]
// select the one overnight product without a competing horizon
fn parses_overnight_forecast_selection() {
    assert_eq!(
        parse_forecast_selection(&[QueryParameter {
            name: "window",
            value: "overnight",
        }]),
        Ok(ForecastSelection::Overnight),
    );
}

#[test]
// reject unsupported and ambiguous forecast selector values
fn rejects_invalid_forecast_selection() {
    let fixtures = [
        vec![QueryParameter {
            name: "days",
            value: "7",
        }],
        vec![QueryParameter {
            name: "window",
            value: "other",
        }],
        vec![QueryParameter {
            name: "window",
            value: "",
        }],
        vec![
            QueryParameter {
                name: "window",
                value: "overnight",
            },
            QueryParameter {
                name: "days",
                value: "1",
            },
        ],
        vec![
            QueryParameter {
                name: "window",
                value: "overnight",
            },
            QueryParameter {
                name: "days",
                value: "",
            },
        ],
    ];

    // verify every invalid value maps to the same public query error
    for parameters in fixtures {
        let failure = parse_forecast_selection(&parameters).expect_err("query must fail");
        assert_eq!(failure.status, 400);
        assert_eq!(failure.code, "invalid_query");
        assert_eq!(failure.message, "Request query is invalid");
    }
}

#[test]
// preserve exact duplicate and unsupported field diagnostics
fn rejects_duplicate_and_unsupported_forecast_fields() {
    let duplicate = parse_forecast_selection(&[
        QueryParameter {
            name: "window",
            value: "overnight",
        },
        QueryParameter {
            name: "window",
            value: "overnight",
        },
    ])
    .expect_err("duplicate must fail");
    assert_eq!(duplicate.message, "window may be provided once");

    let unsupported = parse_forecast_selection(&[QueryParameter {
        name: "other",
        value: "value",
    }])
    .expect_err("unsupported field must fail");
    assert_eq!(unsupported.message, "Unsupported query parameter: other");
}
