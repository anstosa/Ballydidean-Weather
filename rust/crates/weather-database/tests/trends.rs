use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{JsDate, Nullable, TrendPointRow, TrendQuery};
use weather_database::{
    AdmittedTrendQuery, TREND_POINTS_SQL, TrendReadBind, TrendReadPlanError,
    project_trend_point_rows, trend_read_plan,
};

/// build one admitted calendar trend query
fn admitted(from_epoch_milliseconds: i64, to_epoch_milliseconds: i64) -> AdmittedTrendQuery {
    AdmittedTrendQuery::admitted(
        TrendQuery {
            from: "2019-01-01T08:00:00.000Z".to_owned(),
            siteSlug: "ballydidean".to_owned(),
            to: "2026-08-22T05:00:00.000Z".to_owned(),
        },
        from_epoch_milliseconds,
        to_epoch_milliseconds,
    )
}

/// collapse irrelevant SQL formatting like the Node oracle
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// render typed binds through their Node scalar values
fn render_binds(binds: &[TrendReadBind]) -> String {
    binds
        .iter()
        .map(|bind| {
            // preserve each exact PostgreSQL scalar value
            match bind {
                TrendReadBind::Text(value) | TrendReadBind::UtcInstant(value) => value.clone(),
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// build one typed decoded trend bucket
fn row(valid_at: i64, temperature_c: f64) -> TrendPointRow {
    TrendPointRow {
        apparentTemperatureC: Nullable::Null,
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Null,
        pressureHpa: Nullable::Null,
        relativeHumidityPercent: Nullable::Null,
        temperatureC: Nullable::Value(temperature_c),
        temperatureMaximumC: Nullable::Null,
        temperatureMinimumC: Nullable::Null,
        validAt: JsDate::EpochMilliseconds(valid_at),
        windDirectionDegrees: Nullable::Null,
        windGustMps: Nullable::Null,
        windSpeedMps: Nullable::Null,
    }
}

// verifies complete SQL and bind parity with the compiled Node repository
#[test]
fn matches_compiled_node_trend_plan() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/trends-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run trends repository Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let lines = stdout.lines().collect::<Vec<_>>();
    let plan =
        trend_read_plan(&admitted(1_546_329_600_000, 1_787_374_800_000)).expect("trend read plan");

    assert_eq!(plan.sql, TREND_POINTS_SQL);
    assert_eq!(lines[0], format!("sql={}", normalize_sql(plan.sql)));
    assert_eq!(lines[1], format!("binds={}", render_binds(&plan.binds)));
    assert_eq!(lines[2], "rows=11.5|16.2");
}

// verifies the exact source repository ordered-window guard
#[test]
fn rejects_unordered_trend_window() {
    let unordered = admitted(1_787_374_800_000, 1_787_374_800_000);
    assert_eq!(trend_read_plan(&unordered), Err(TrendReadPlanError));
}

// verifies typed row projection preserves ascending bucket order and nulls
#[test]
fn preserves_typed_trend_row_order() {
    let result = project_trend_point_rows(vec![
        row(1_546_329_600_000, 11.5),
        row(1_787_295_600_000, 16.2),
    ]);
    assert_eq!(
        result.rows[0].validAt,
        JsDate::EpochMilliseconds(1_546_329_600_000)
    );
    assert_eq!(result.rows[1].temperatureC, Nullable::Value(16.2));
    assert_eq!(result.rows[1].windSpeedMps, Nullable::Null);
}
