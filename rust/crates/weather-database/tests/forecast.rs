use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    FieldPresence, ForecastProductSelection, ForecastQuery, JsDateOrString, Nullable, SourceKind,
    WeatherRecordRow,
};
use weather_database::{
    AdmittedForecastQuery, FORECAST_PROVENANCE_SQL, ForecastProvenanceBind, ForecastReadBind,
    ForecastReadPlanError, ForecastRuntimeProvenanceRow, decorate_forecast_provenance,
    forecast_provenance_plan, forecast_read_plan,
};

/// encode one generated UTF-16 string
fn text(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// build one admitted forecast query
fn admitted(
    as_of: &str,
    as_of_epoch_milliseconds: i64,
    end_exclusive: &str,
    end_exclusive_epoch_milliseconds: i64,
    hours: u64,
    product_selection: FieldPresence<ForecastProductSelection>,
) -> AdmittedForecastQuery {
    AdmittedForecastQuery::admitted(
        ForecastQuery {
            asOf: as_of.to_owned(),
            hours,
            productSelection: product_selection,
            siteSlug: "ballydidean".to_owned(),
        },
        as_of_epoch_milliseconds,
        end_exclusive.to_owned(),
        end_exclusive_epoch_milliseconds,
    )
}

/// collapse irrelevant SQL formatting like the Node oracle
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// render typed raw-read binds through their Node scalar values
fn render_binds(binds: &[ForecastReadBind]) -> String {
    binds
        .iter()
        .map(|bind| {
            // preserve each exact PostgreSQL scalar value
            match bind {
                ForecastReadBind::Text(value) | ForecastReadBind::UtcInstant(value) => {
                    value.clone()
                }
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// build one representative decoded raw forecast row
fn raw_row(id: &str) -> WeatherRecordRow {
    WeatherRecordRow {
        adapterVersion: Nullable::Null,
        apparentTemperatureC: Nullable::Value(11.7),
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Value(72.0),
        contentHash: format!("content-{id}"),
        contractEpoch: Nullable::Null,
        deviceModel: Nullable::Null,
        deviceSerial: Nullable::Null,
        deviceVendor: Nullable::Null,
        firstReceivedAt: JsDateOrString::String(text("2026-08-20T18:00:00.000Z")),
        id: id.to_owned(),
        lastReceivedAt: JsDateOrString::String(text("2026-08-20T18:00:00.000Z")),
        pm25MicrogramsPerCubicMeter: Nullable::Value(5.0),
        precipitationMm: Nullable::Value(0.1),
        precipitationRateMmPerHour: Nullable::Value(0.1),
        pressureHpa: Nullable::Value(1018.1),
        productRunAt: Nullable::Value(JsDateOrString::String(text("2026-08-20T18:00:00.000Z"))),
        providerKey: "open-meteo".to_owned(),
        providerMetadata: Nullable::Null,
        qualityMetadata: Nullable::Null,
        relativeHumidityPercent: Nullable::Value(83.0),
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
        temperatureC: Nullable::Value(12.1),
        upstreamModel: Nullable::Value("best_match".to_owned()),
        upstreamTimezone: "GMT".to_owned(),
        uvIndex: Nullable::Value(0.2),
        validAt: JsDateOrString::String(text("2026-08-21T07:00:00.000Z")),
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Value(180.0),
        windGustMps: Nullable::Value(3.1),
        windSpeedMps: Nullable::Value(1.8),
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

/// build one matching optional provenance row
fn provenance(weather_record_id: &str) -> ForecastRuntimeProvenanceRow {
    ForecastRuntimeProvenanceRow {
        adapter_version: "open-meteo/v4".to_owned(),
        contract_epoch: "forecast-contract/v4".to_owned(),
        source_config_fingerprint: "f".repeat(64),
        source_id: "12".to_owned(),
        source_key: "open-meteo-forecast-v4".to_owned(),
        weather_record_id: weather_record_id.to_owned(),
    }
}

// verifies both product SQL branches, binds, and provenance against compiled Node
#[test]
fn matches_compiled_node_forecast_plans() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/forecast-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run forecast repository Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let lines = stdout.lines().collect::<Vec<_>>();
    let latest = forecast_read_plan(&admitted(
        "2026-08-20T19:00:00.000Z",
        1_787_252_400_000,
        "2026-08-22T08:00:00.000Z",
        1_787_385_600_000,
        37,
        FieldPresence::Missing,
    ))
    .expect("latest forecast plan");
    let anchor = forecast_read_plan(&admitted(
        "2026-08-21T07:00:00.000Z",
        1_787_295_600_000,
        "2026-08-22T14:00:00.000Z",
        1_787_407_200_000,
        31,
        FieldPresence::Value(ForecastProductSelection::AnchorContaining),
    ))
    .expect("anchor forecast plan");
    let raw_rows = vec![raw_row("201"), raw_row("202")];
    let provenance_plan = forecast_provenance_plan(&raw_rows).expect("provenance plan");

    assert_eq!(
        lines[0],
        format!("latest-sql={}", normalize_sql(&latest.sql))
    );
    assert_eq!(
        lines[1],
        format!("latest-binds={}", render_binds(&latest.binds))
    );
    assert_eq!(
        lines[2],
        format!("provenance-sql={}", normalize_sql(FORECAST_PROVENANCE_SQL))
    );
    assert_eq!(provenance_plan.sql, FORECAST_PROVENANCE_SQL);
    assert_eq!(
        provenance_plan.bind,
        ForecastProvenanceBind(vec!["201".to_owned(), "202".to_owned()])
    );
    assert_eq!(lines[3], "provenance-binds=201|202");
    assert_eq!(
        lines[4],
        format!("anchor-sql={}", normalize_sql(&anchor.sql))
    );
    assert_eq!(
        lines[5],
        format!("anchor-binds={}", render_binds(&anchor.binds))
    );
    assert_eq!(lines[6], "decorated=201|202");
}

// verifies repository hours and the admitted exact end instant remain bounded
#[test]
fn rejects_invalid_forecast_windows() {
    let zero = admitted(
        "2026-08-20T19:00:00.000Z",
        1_787_252_400_000,
        "2026-08-20T19:00:00.000Z",
        1_787_252_400_000,
        0,
        FieldPresence::Missing,
    );
    assert_eq!(
        forecast_read_plan(&zero),
        Err(ForecastReadPlanError::InvalidHours)
    );

    let excessive = admitted(
        "2026-08-20T19:00:00.000Z",
        1_787_252_400_000,
        "2026-08-31T20:00:00.000Z",
        1_788_206_400_000,
        265,
        FieldPresence::Missing,
    );
    assert_eq!(
        forecast_read_plan(&excessive),
        Err(ForecastReadPlanError::InvalidHours)
    );

    let mismatched_end = admitted(
        "2026-08-20T19:00:00.000Z",
        1_787_252_400_000,
        "2026-08-22T07:00:00.000Z",
        1_787_382_000_000,
        37,
        FieldPresence::Missing,
    );
    assert_eq!(
        forecast_read_plan(&mismatched_end),
        Err(ForecastReadPlanError::InvalidResolvedWindow)
    );

    let latest = forecast_read_plan(&admitted(
        "2026-08-20T19:00:00.000Z",
        1_787_252_400_000,
        "2026-08-22T08:00:00.000Z",
        1_787_385_600_000,
        37,
        FieldPresence::Value(ForecastProductSelection::Latest),
    ))
    .expect("explicit latest forecast plan");
    assert!(latest.sql.contains("FROM public.forecast_products product"));
}

// verifies optional provenance decorates matches and fails raw for invalid linkage
#[test]
fn decorates_valid_provenance_and_fails_raw() {
    let raw_rows = vec![raw_row("201"), raw_row("202")];
    let decorated = decorate_forecast_provenance(raw_rows.clone(), vec![provenance("201")]);
    assert_eq!(
        decorated[0].adapterVersion,
        Nullable::Value("open-meteo/v4".to_owned())
    );
    assert_eq!(
        decorated[0].contractEpoch,
        Nullable::Value("forecast-contract/v4".to_owned())
    );
    assert_eq!(
        decorated[0].sourceConfigFingerprint,
        Nullable::Value("f".repeat(64))
    );
    assert_eq!(decorated[1].adapterVersion, Nullable::Null);

    let mut invalid = provenance("201");
    invalid.source_id = "99".to_owned();
    assert_eq!(
        decorate_forecast_provenance(raw_rows.clone(), vec![invalid]),
        raw_rows
    );
    assert!(forecast_provenance_plan(&[]).is_none());
}
