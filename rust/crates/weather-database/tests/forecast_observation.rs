use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    ForecastObservationInputRow, ForecastObservationInputRowSourceKindEnum, JsDateOrString,
    Nullable,
};
use weather_database::{
    AdmittedForecastObservationQuery, FORECAST_OBSERVATION_INPUT_SQL, ForecastObservationInputBind,
    ForecastObservationInputDbRow, ForecastObservationInputProjectionError,
    forecast_observation_input_read_plan, project_forecast_observation_input_rows,
};

/// encode one generated UTF-16 string
fn text(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// build one typed observation row
fn record() -> ForecastObservationInputRow {
    ForecastObservationInputRow {
        apparentTemperatureC: Nullable::Value(9.0),
        cloudCoverPercent: Nullable::Value(20.0),
        id: "100".to_owned(),
        lastReceivedAt: JsDateOrString::String(text("2026-08-22T04:00:00.000Z")),
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationRateMmPerHour: Nullable::Value(0.0),
        pressureHpa: Nullable::Value(1010.0),
        providerKey: "ecowitt-local".to_owned(),
        relativeHumidityPercent: Nullable::Value(80.0),
        solarRadiationWm2: Nullable::Value(0.0),
        sourceId: "1".to_owned(),
        sourceKey: "ecowitt-local-live-v1".to_owned(),
        sourceKind: ForecastObservationInputRowSourceKindEnum::PhysicalSensor,
        stationSlug: "ballydidean-ecowitt".to_owned(),
        temperatureC: Nullable::Value(10.0),
        uvIndex: Nullable::Value(0.0),
        validAt: JsDateOrString::String(text("2026-08-22T04:00:00.000Z")),
        windDirectionDegrees: Nullable::Value(90.0),
        windGustMps: Nullable::Value(3.0),
        windSpeedMps: Nullable::Value(2.0),
    }
}

/// build one decoded repository row
fn row(
    site_slug: &str,
    record: Option<ForecastObservationInputRow>,
) -> ForecastObservationInputDbRow {
    ForecastObservationInputDbRow {
        projection_from: JsDateOrString::String(text("2026-08-21T07:00:00.000Z")),
        query_from: JsDateOrString::String(text("2026-08-21T04:00:00.000Z")),
        record,
        site_slug: site_slug.to_owned(),
        through: JsDateOrString::String(text("2026-08-22T05:00:00.000Z")),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

/// collapse irrelevant SQL formatting like the Node oracle
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

// verifies exact SQL, bind, and typed projection parity with compiled Node
#[test]
fn matches_compiled_node_forecast_observation_read() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/forecast-observation-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run forecast observation repository Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let lines = stdout.lines().collect::<Vec<_>>();
    let plan = forecast_observation_input_read_plan(&AdmittedForecastObservationQuery::admitted(
        "2026-08-22T05:00:00.000Z".to_owned(),
    ));

    assert_eq!(plan.sql, FORECAST_OBSERVATION_INPUT_SQL);
    assert_eq!(lines[0], format!("sql={}", normalize_sql(plan.sql)));
    assert_eq!(
        plan.binds,
        [ForecastObservationInputBind::UtcInstant(
            "2026-08-22T05:00:00.000Z".to_owned()
        )]
    );
    assert_eq!(lines[1], "binds=2026-08-22T05:00:00.000Z");
    assert_eq!(lines[2], "input=ballydidean|America/Los_Angeles|100");

    let input = project_forecast_observation_input_rows(vec![row("ballydidean", Some(record()))])
        .expect("typed projection")
        .expect("available input");
    assert_eq!(input.records.len(), 1);
    assert_eq!(input.records[0].id, "100");
}

// verifies absent sites, empty lateral rows, and fixed identity validation
#[test]
fn preserves_fixed_site_projection_guards() {
    assert_eq!(
        project_forecast_observation_input_rows(Vec::new()).expect("absent site"),
        None
    );
    let empty = project_forecast_observation_input_rows(vec![row("ballydidean", None)])
        .expect("empty archive")
        .expect("active site");
    assert!(empty.records.is_empty());
    assert_eq!(
        project_forecast_observation_input_rows(vec![row("elsewhere", None)]),
        Err(ForecastObservationInputProjectionError::IdentityMismatch)
    );
}
