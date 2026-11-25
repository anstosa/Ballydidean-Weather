use std::path::PathBuf;
use std::process::Command;

use weather_api::{
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver, QueryParameter,
    TideBoundaryAdapter, TideBoundaryError, TideQueryTime, group_active_sites, parse_tide_query,
    project_tides_response, tides_read_plan,
};
use weather_contracts::{
    ApiTideRecordEventTypeEnum, ApiTideRecordKindEnum, JsDateOrString, Nullable, SourceKind,
    StationKind, TideRecordRow, TideRecordRowSourceKindEnum,
};
use weather_database::{ActiveSiteRow, TideReadBind};

struct FixtureBoundary;

impl TideBoundaryAdapter for FixtureBoundary {
    /// resolve only explicit submitted fixture times
    fn resolve_query_time(
        &self,
        value: &str,
        field: TideQueryTime,
    ) -> Result<CanonicalInstant, TideBoundaryError> {
        // preserve source-derived canonical query pairs
        match (value, field) {
            ("2026-08-20T00:00:00Z", TideQueryTime::From) => {
                Ok(instant(1_787_184_000_000, "2026-08-20T00:00:00.000Z"))
            }
            ("2026-08-25T00:00:00Z", TideQueryTime::To) => {
                Ok(instant(1_787_616_000_000, "2026-08-25T00:00:00.000Z"))
            }
            ("2026-08-01T00:00:00Z", TideQueryTime::From) => {
                Ok(instant(1_785_542_400_000, "2026-08-01T00:00:00.000Z"))
            }
            ("2026-09-02T00:00:00Z", TideQueryTime::To) => {
                Ok(instant(1_788_307_200_000, "2026-09-02T00:00:00.000Z"))
            }
            _ => Err(TideBoundaryError),
        }
    }

    /// resolve only the exact source default offsets
    fn offset_data_time(
        &self,
        data_at: &CanonicalInstant,
        offset_milliseconds: i64,
        field: TideQueryTime,
    ) -> Result<CanonicalInstant, TideBoundaryError> {
        assert_eq!(data_at.utc, "2026-08-22T05:00:00.000Z");

        // admit the one-day history and ten-day forecast defaults
        match (offset_milliseconds, field) {
            (-86_400_000, TideQueryTime::From) => {
                Ok(instant(1_787_288_400_000, "2026-08-21T05:00:00.000Z"))
            }
            (864_000_000, TideQueryTime::To) => {
                Ok(instant(1_788_238_800_000, "2026-09-01T05:00:00.000Z"))
            }
            _ => Err(TideBoundaryError),
        }
    }
}

struct FixtureTimeResolver;

impl CurrentTimeResolver for FixtureTimeResolver {
    /// resolve only explicit decoded tide row instants
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        // preserve source-derived repository time pairs
        match value {
            JsDateOrString::String(value) if value == &text("2026-08-22T11:27:00.000Z") => {
                Ok(instant(1_787_398_020_000, "2026-08-22T11:27:00.000Z"))
            }
            JsDateOrString::String(value) if value == &text("2026-08-22T11:30:00.000Z") => {
                Ok(instant(1_787_398_200_000, "2026-08-22T11:30:00.000Z"))
            }
            JsDateOrString::String(value) if value == &text("2026-08-22T11:33:00.000Z") => {
                Ok(instant(1_787_398_380_000, "2026-08-22T11:33:00.000Z"))
            }
            JsDateOrString::String(value) if value == &text("2026-08-22T11:36:00.000Z") => {
                Ok(instant(1_787_398_560_000, "2026-08-22T11:36:00.000Z"))
            }
            _ => Err(CurrentProjectionError {
                code: "tides_projection.time",
                detail: "fixture time adapter rejected the tide timestamp".to_owned(),
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

/// build one active tide source row
fn active_source() -> ActiveSiteRow {
    ActiveSiteRow {
        attribution_label: "NOAA Tides & Currents".to_owned(),
        attribution_url: "https://tidesandcurrents.noaa.gov/".to_owned(),
        latitude: 47.95,
        longitude: -122.42,
        provider_key: "noaa-co-ops".to_owned(),
        provider_name: "NOAA CO-OPS".to_owned(),
        site_name: "Ballydidean".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_id: "42".to_owned(),
        source_key: "glendale-tide-predictions".to_owned(),
        source_kind: SourceKind::TidePrediction,
        station_kind: StationKind::Virtual,
        station_latitude: 48.12,
        station_longitude: -122.58,
        station_name: "Glendale Tide Predictions".to_owned(),
        station_slug: "glendale-tide-predictions".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

/// build the one active site fixture
fn site() -> weather_contracts::ApiSite {
    group_active_sites(vec![active_source()]).data.remove(0)
}

/// build one typed tide row
fn row(
    source_kind: TideRecordRowSourceKindEnum,
    prediction_type: Nullable<&str>,
    station_name: &str,
    station_slug: &str,
    valid_at: &str,
    water_level_m: f64,
) -> TideRecordRow {
    // preserve null versus submitted provider event markers
    let prediction_type = match prediction_type {
        Nullable::Null => Nullable::Null,
        Nullable::Value(value) => Nullable::Value(value.to_owned()),
    };

    TideRecordRow {
        attributionLabel: "NOAA Tides & Currents".to_owned(),
        attributionUrl: "https://tidesandcurrents.noaa.gov/".to_owned(),
        predictionType: prediction_type,
        providerKey: "noaa-co-ops".to_owned(),
        sourceId: "42".to_owned(),
        sourceKind: source_kind,
        stationName: station_name.to_owned(),
        stationSlug: station_slug.to_owned(),
        validAt: JsDateOrString::String(text(valid_at)),
        waterLevelM: water_level_m,
    }
}

// verifies compiled Node query and projection semantics before Rust parity assertions
#[test]
fn matches_compiled_node_tides_query_and_projection() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tides-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run tides API Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let parsed =
        parse_tide_query(&[], &site(), &data_at, &FixtureBoundary).expect("default tide query");
    assert_eq!(parsed.query.from, "2026-08-21T05:00:00.000Z");
    assert_eq!(parsed.query.to, "2026-09-01T05:00:00.000Z");
    assert_eq!(
        parsed.query.limit,
        weather_contracts::FieldPresence::Missing
    );
    let plan = tides_read_plan(&parsed).expect("linked tide database plan");
    assert_eq!(
        plan.binds,
        [
            TideReadBind::Text("ballydidean".to_owned()),
            TideReadBind::UtcInstant("2026-08-21T05:00:00.000Z".to_owned()),
            TideReadBind::UtcInstant("2026-09-01T05:00:00.000Z".to_owned()),
            TideReadBind::Limit(10_000),
        ]
    );

    let response = project_tides_response(
        vec![
            row(
                TideRecordRowSourceKindEnum::TidePrediction,
                Nullable::Value("H"),
                "Glendale Tide Predictions",
                "glendale-tide-predictions",
                "2026-08-22T11:27:00.000Z",
                3.271,
            ),
            row(
                TideRecordRowSourceKindEnum::TideObservation,
                Nullable::Null,
                "Glendale Tide Observations",
                "glendale-tide-observations",
                "2026-08-22T11:30:00.000Z",
                3.2,
            ),
            row(
                TideRecordRowSourceKindEnum::TidePrediction,
                Nullable::Value("M"),
                "Glendale Tide Predictions",
                "glendale-tide-predictions",
                "2026-08-22T11:33:00.000Z",
                3.1,
            ),
            row(
                TideRecordRowSourceKindEnum::TidePrediction,
                Nullable::Value("L"),
                "Glendale Tide Predictions",
                "glendale-tide-predictions",
                "2026-08-22T11:36:00.000Z",
                0.4,
            ),
        ],
        site(),
        &data_at,
        &FixtureTimeResolver,
    )
    .expect("tides response");
    assert_eq!(response.generatedAt, data_at.utc);
    assert_eq!(response.data.len(), 4);
    assert_eq!(
        response.data[0].eventType,
        Nullable::Value(ApiTideRecordEventTypeEnum::High)
    );
    assert_eq!(response.data[0].kind, ApiTideRecordKindEnum::Prediction);
    assert_eq!(
        response.data[0].source.stationSlug,
        "glendale-tide-predictions"
    );
    assert_eq!(response.data[0].validAt, "2026-08-22T11:27:00.000Z");
    assert_eq!(response.data[1].eventType, Nullable::Null);
    assert_eq!(response.data[1].kind, ApiTideRecordKindEnum::Observation);
    assert_eq!(response.data[2].eventType, Nullable::Null);
    assert_eq!(
        response.data[3].eventType,
        Nullable::Value(ApiTideRecordEventTypeEnum::Low)
    );
}

// verifies custom admitted range values replace both defaults
#[test]
fn parses_custom_tide_range() {
    let data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let parsed = parse_tide_query(
        &[
            QueryParameter {
                name: "from",
                value: "2026-08-20T00:00:00Z",
            },
            QueryParameter {
                name: "to",
                value: "2026-08-25T00:00:00Z",
            },
        ],
        &site(),
        &data_at,
        &FixtureBoundary,
    )
    .expect("custom tide range");
    assert_eq!(parsed.query.from, "2026-08-20T00:00:00.000Z");
    assert_eq!(parsed.query.to, "2026-08-25T00:00:00.000Z");
}

// verifies excessive, duplicate, and unsupported tide filters fail closed
#[test]
fn rejects_invalid_tide_ranges() {
    let data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let excessive = parse_tide_query(
        &[
            QueryParameter {
                name: "from",
                value: "2026-08-01T00:00:00Z",
            },
            QueryParameter {
                name: "to",
                value: "2026-09-02T00:00:00Z",
            },
        ],
        &site(),
        &data_at,
        &FixtureBoundary,
    )
    .expect_err("excessive tide range");
    assert_eq!(excessive.message, "Request query is invalid");

    let duplicate = parse_tide_query(
        &[
            QueryParameter {
                name: "from",
                value: "2026-08-20T00:00:00Z",
            },
            QueryParameter {
                name: "from",
                value: "2026-08-20T00:00:00Z",
            },
        ],
        &site(),
        &data_at,
        &FixtureBoundary,
    )
    .expect_err("duplicate tide bound");
    assert_eq!(duplicate.message, "from may be provided once");

    let unsupported = parse_tide_query(
        &[QueryParameter {
            name: "station",
            value: "glendale",
        }],
        &site(),
        &data_at,
        &FixtureBoundary,
    )
    .expect_err("unsupported tide filter");
    assert_eq!(unsupported.message, "Unsupported query parameter: station");
}
