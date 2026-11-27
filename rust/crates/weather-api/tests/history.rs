use std::path::PathBuf;
use std::process::Command;

use weather_api::{
    AdmittedHistoryCursor, CanonicalInstant, CurrentProjectionError, CurrentTimeResolver,
    HistoryBoundaryAdapter, HistoryBoundaryError, HistoryQueryTime, ParsedHistoryQuery,
    QueryParameter, group_active_sites, history_read_plan, parse_history_query,
    project_history_response,
};
use weather_contracts::{
    FieldPresence, HistoryQueryCursor, JsDateOrString, JsonValue, Nullable, SourceKind,
    StationKind, WeatherRecordRow,
};
use weather_database::{ActiveSiteRow, HistoryReadBind};

struct FixtureBoundary;

impl HistoryBoundaryAdapter for FixtureBoundary {
    /// resolve only explicit query fixture instants
    fn resolve_query_time(
        &self,
        value: &str,
        field: HistoryQueryTime,
    ) -> Result<CanonicalInstant, HistoryBoundaryError> {
        // preserve source-derived canonical query pairs
        match (value, field) {
            ("2026-08-01T01:00:00+01:00", HistoryQueryTime::From) => {
                Ok(instant(1_785_542_400_000, "2026-08-01T00:00:00.000Z"))
            }
            ("2026-08-22T01:00:00+01:00", HistoryQueryTime::To) => {
                Ok(instant(1_787_356_800_000, "2026-08-22T00:00:00.000Z"))
            }
            ("2026-08-23T00:00:00Z", HistoryQueryTime::From) => {
                Ok(instant(1_787_443_200_000, "2026-08-23T00:00:00.000Z"))
            }
            ("2026-08-22T00:00:00Z", HistoryQueryTime::To) => {
                Ok(instant(1_787_356_800_000, "2026-08-22T00:00:00.000Z"))
            }
            ("2026-08-01T00:30:00-01:00", HistoryQueryTime::From) => {
                Ok(instant(1_785_547_800_000, "2026-08-01T01:30:00.000Z"))
            }
            ("2026-08-01T01:00:00Z", HistoryQueryTime::To) => {
                Ok(instant(1_785_546_000_000, "2026-08-01T01:00:00.000Z"))
            }
            _ => Err(HistoryBoundaryError),
        }
    }

    /// decode one admitted opaque fixture cursor
    fn decode_cursor(&self, value: &str) -> Result<AdmittedHistoryCursor, HistoryBoundaryError> {
        // accept only the fixture cursor without implementing its codec
        if value != "opaque-input" {
            return Err(HistoryBoundaryError);
        }

        Ok(AdmittedHistoryCursor {
            id: "200".to_owned(),
            valid_at: instant(1_787_277_600_000, "2026-08-21T02:00:00.000Z"),
        })
    }

    /// encode the expected canonical continuation cursor
    fn encode_cursor(&self, cursor: &HistoryQueryCursor) -> Result<String, CurrentProjectionError> {
        assert_eq!(cursor.id, "102");
        assert_eq!(cursor.validAt, "2026-08-21T02:00:00.000Z");
        Ok("opaque-next".to_owned())
    }
}

struct FixtureTimeResolver;

impl CurrentTimeResolver for FixtureTimeResolver {
    /// resolve only explicit repository fixture instants
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        // preserve source-derived repository time pairs
        match value {
            JsDateOrString::String(value) if value == &text("2026-08-21T03:00:00.000Z") => {
                Ok(instant(1_787_281_200_000, "2026-08-21T03:00:00.000Z"))
            }
            JsDateOrString::String(value) if value == &text("2026-08-21T02:00:00.000Z") => {
                Ok(instant(1_787_277_600_000, "2026-08-21T02:00:00.000Z"))
            }
            JsDateOrString::String(value) if value == &text("2026-08-21T01:00:00.000Z") => {
                Ok(instant(1_787_274_000_000, "2026-08-21T01:00:00.000Z"))
            }
            JsDateOrString::String(value) if value == &text("2026-08-22T04:51:00.000Z") => {
                Ok(instant(1_787_374_260_000, "2026-08-22T04:51:00.000Z"))
            }
            _ => Err(CurrentProjectionError {
                code: "history_projection.time",
                detail: "fixture time adapter rejected the repository timestamp".to_owned(),
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

/// encode one generated UTF-16 string
fn text(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// build one generated JSON object entry
fn entry(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (text(key), value)
}

/// build one active historical source row
fn active_source() -> ActiveSiteRow {
    ActiveSiteRow {
        attribution_label: "Open-Meteo historical weather data".to_owned(),
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
        station_name: "Open-Meteo virtual station".to_owned(),
        station_slug: "open-meteo-virtual".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

/// build the one active site fixture
fn site() -> weather_contracts::ApiSite {
    group_active_sites(vec![active_source()]).data.remove(0)
}

/// build one representative ordered history row
fn record(id: &str, valid_at: &str) -> WeatherRecordRow {
    WeatherRecordRow {
        adapterVersion: Nullable::Value("open-meteo-reanalysis/v1".to_owned()),
        apparentTemperatureC: Nullable::Value(15.5),
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Value(42.0),
        contentHash: format!("hash-{id}"),
        contractEpoch: Nullable::Null,
        deviceModel: Nullable::Null,
        deviceSerial: Nullable::Null,
        deviceVendor: Nullable::Null,
        firstReceivedAt: JsDateOrString::String(text(valid_at)),
        id: id.to_owned(),
        lastReceivedAt: JsDateOrString::String(text("2026-08-22T04:51:00.000Z")),
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Value(0.2),
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Value(1_014.2),
        productRunAt: Nullable::Null,
        providerKey: "ignored-row-provider".to_owned(),
        providerMetadata: Nullable::Value(vec![
            entry("dataset", JsonValue::String(text("era5"))),
            entry("elevation_m", JsonValue::Number(17.0)),
            entry("grid_cell", JsonValue::String(text("47.95,-122.42"))),
            entry("private_request_id", JsonValue::String(text("hidden"))),
        ]),
        qualityMetadata: Nullable::Null,
        relativeHumidityPercent: Nullable::Value(78.0),
        revisionCount: 0,
        siteSlug: "ballydidean".to_owned(),
        sourceId: "11".to_owned(),
        sourceConfigFingerprint: Nullable::Null,
        sourceKey: "ignored-row-source".to_owned(),
        sourceKind: SourceKind::Reanalysis,
        stationSlug: "open-meteo-virtual".to_owned(),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Value(320.0),
        temperatureC: Nullable::Value(16.2),
        upstreamModel: Nullable::Value("era5".to_owned()),
        upstreamTimezone: "UTC".to_owned(),
        uvIndex: Nullable::Null,
        validAt: JsDateOrString::String(text(valid_at)),
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Value(225.0),
        windGustMps: Nullable::Null,
        windSpeedMps: Nullable::Value(4.1),
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

// verifies compiled Node query and response semantics before Rust parity assertions
#[test]
fn matches_compiled_node_history_query_and_projection() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/history-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run history Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parameters = [
        QueryParameter {
            name: "sourceKind",
            value: "reanalysis",
        },
        QueryParameter {
            name: "station",
            value: "open-meteo-virtual",
        },
        QueryParameter {
            name: "source",
            value: "11",
        },
        QueryParameter {
            name: "from",
            value: "2026-08-01T01:00:00+01:00",
        },
        QueryParameter {
            name: "to",
            value: "2026-08-22T01:00:00+01:00",
        },
        QueryParameter {
            name: "cursor",
            value: "opaque-input",
        },
        QueryParameter {
            name: "limit",
            value: "2",
        },
    ];
    let parsed =
        parse_history_query(&parameters, &site(), &FixtureBoundary).expect("history query");
    let read_plan = history_read_plan(&parsed).expect("linked database history plan");
    assert!(
        read_plan
            .sql
            .contains("(candidate.valid_at, candidate.id) < ($7::timestamptz, $8::bigint)")
    );
    assert_eq!(
        read_plan.binds,
        [
            HistoryReadBind::Text("ballydidean".to_owned()),
            HistoryReadBind::Text("open-meteo-virtual".to_owned()),
            HistoryReadBind::BigintText("11".to_owned()),
            HistoryReadBind::SourceKind(SourceKind::Reanalysis),
            HistoryReadBind::UtcInstant("2026-08-01T00:00:00.000Z".to_owned()),
            HistoryReadBind::UtcInstant("2026-08-22T00:00:00.000Z".to_owned()),
            HistoryReadBind::UtcInstant("2026-08-21T02:00:00.000Z".to_owned()),
            HistoryReadBind::BigintText("200".to_owned()),
            HistoryReadBind::Limit(3),
        ]
    );
    let ParsedHistoryQuery {
        query,
        requested_limit,
        ..
    } = parsed;
    assert_eq!(requested_limit, 2);
    assert_eq!(query.limit, FieldPresence::Value(3));
    assert_eq!(query.siteSlug, "ballydidean");
    assert_eq!(query.sourceId, FieldPresence::Value("11".to_owned()));
    assert_eq!(
        query.sourceKind,
        FieldPresence::Value(SourceKind::Reanalysis)
    );
    assert_eq!(
        query.stationSlug,
        FieldPresence::Value("open-meteo-virtual".to_owned())
    );
    assert_eq!(
        query.from,
        FieldPresence::Value("2026-08-01T00:00:00.000Z".to_owned())
    );
    assert_eq!(
        query.to,
        FieldPresence::Value("2026-08-22T00:00:00.000Z".to_owned())
    );
    assert_eq!(
        query.cursor,
        FieldPresence::Value(HistoryQueryCursor {
            id: "200".to_owned(),
            validAt: "2026-08-21T02:00:00.000Z".to_owned(),
        })
    );

    let response = project_history_response(
        vec![
            record("103", "2026-08-21T03:00:00.000Z"),
            record("102", "2026-08-21T02:00:00.000Z"),
            record("101", "2026-08-21T01:00:00.000Z"),
        ],
        site(),
        instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z"),
        2,
        &FixtureTimeResolver,
        &FixtureBoundary,
    )
    .expect("history response");
    assert_eq!(response.generatedAt, "2026-08-22T05:00:00.000Z");
    assert_eq!(response.data.len(), 2);
    assert_eq!(response.data[0].id, "103");
    assert_eq!(response.data[1].id, "102");
    assert_eq!(response.data[0].metadata.device, Nullable::Null);
    assert_eq!(response.data[0].productRunAt, Nullable::Null);
    assert_eq!(
        response.data[0].metrics.blackGlobeTemperatureC,
        Nullable::Null
    );
    assert_eq!(
        response.data[0].provenance.sourceKey,
        "open-meteo-reanalysis-v1"
    );
    assert_eq!(response.page.limit, 2);
    assert_eq!(
        response.page.nextCursor,
        Nullable::Value("opaque-next".to_owned())
    );
}

// verifies defaults, lookahead status, and nullable terminal cursor
#[test]
fn preserves_history_defaults_and_terminal_page() {
    let parsed = parse_history_query(&[], &site(), &FixtureBoundary).expect("default history");
    assert_eq!(parsed.requested_limit, 100);
    assert_eq!(parsed.query.limit, FieldPresence::Value(101));
    assert_eq!(parsed.query.cursor, FieldPresence::Missing);
    assert_eq!(parsed.query.from, FieldPresence::Missing);
    assert_eq!(parsed.query.sourceId, FieldPresence::Missing);
    assert_eq!(parsed.query.sourceKind, FieldPresence::Missing);
    assert_eq!(parsed.query.stationSlug, FieldPresence::Missing);
    assert_eq!(parsed.query.to, FieldPresence::Missing);

    let response = project_history_response(
        vec![record("103", "2026-08-21T03:00:00.000Z")],
        site(),
        instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z"),
        2,
        &FixtureTimeResolver,
        &FixtureBoundary,
    )
    .expect("terminal page");
    assert_eq!(response.page.nextCursor, Nullable::Null);
    assert_eq!(response.data.len(), 1);
}

// verifies bounded query errors and exact failure precedence
#[test]
fn rejects_invalid_history_queries() {
    let simultaneous_selection_syntax = parse_history_query(
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
        &FixtureBoundary,
    )
    .expect_err("source syntax must precede station syntax");
    assert_eq!(simultaneous_selection_syntax.status, 400);
    assert_eq!(simultaneous_selection_syntax.code, "invalid_query");
    assert_eq!(
        simultaneous_selection_syntax.message,
        "source must be a positive integer"
    );

    let reversed = parse_history_query(
        &[
            QueryParameter {
                name: "from",
                value: "2026-08-23T00:00:00Z",
            },
            QueryParameter {
                name: "to",
                value: "2026-08-22T00:00:00Z",
            },
        ],
        &site(),
        &FixtureBoundary,
    )
    .expect_err("reversed history range");
    assert_eq!(reversed.message, "from must be earlier than to");

    let offset_reversed = parse_history_query(
        &[
            QueryParameter {
                name: "from",
                value: "2026-08-01T00:30:00-01:00",
            },
            QueryParameter {
                name: "to",
                value: "2026-08-01T01:00:00Z",
            },
        ],
        &site(),
        &FixtureBoundary,
    )
    .expect_err("offset history range reverses by instant");
    assert_eq!(offset_reversed.message, "from must be earlier than to");

    let cursor = parse_history_query(
        &[QueryParameter {
            name: "cursor",
            value: "invalid",
        }],
        &site(),
        &FixtureBoundary,
    )
    .expect_err("invalid history cursor");
    assert_eq!(cursor.message, "cursor is invalid");

    let duplicate = parse_history_query(
        &[
            QueryParameter {
                name: "source",
                value: "11",
            },
            QueryParameter {
                name: "source",
                value: "11",
            },
        ],
        &site(),
        &FixtureBoundary,
    )
    .expect_err("duplicate history source");
    assert_eq!(duplicate.message, "source may be provided once");

    // reject invalid integers separately from out-of-range integers
    for (value, message) in [
        ("2.5", "limit must be an integer"),
        ("0", "limit must be between 1 and 250"),
        ("251", "limit must be between 1 and 250"),
    ] {
        let error = parse_history_query(
            &[QueryParameter {
                name: "limit",
                value,
            }],
            &site(),
            &FixtureBoundary,
        )
        .expect_err("invalid history limit");
        assert_eq!(error.message, message);
    }
}
