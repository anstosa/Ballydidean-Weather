use std::path::PathBuf;
use std::process::Command;

use weather_contracts::ForecastPressureContextQuery;
use weather_database::{
    AdmittedForecastPressureContextQuery, ForecastPressureContextBind,
    ForecastPressureContextPlanError, forecast_pressure_context_plan,
};

/// build one source-derived admitted pressure-context window
fn admitted(context_start_epoch_milliseconds: i64) -> AdmittedForecastPressureContextQuery {
    AdmittedForecastPressureContextQuery::admitted(
        ForecastPressureContextQuery {
            asOf: "2026-08-21T07:00:00.000Z".to_owned(),
            siteSlug: "ballydidean".to_owned(),
        },
        1_787_295_600_000,
        "2026-08-21T04:00:00.000Z".to_owned(),
        context_start_epoch_milliseconds,
        "2026-08-21T10:00:00.000Z".to_owned(),
        1_787_306_400_000,
        "2026-08-19T07:00:00.000Z".to_owned(),
        1_787_122_800_000,
    )
}

/// collapse irrelevant SQL formatting like the Node oracle
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// render typed binds through their Node scalar values
fn render_binds(binds: &[ForecastPressureContextBind]) -> String {
    binds
        .iter()
        .map(|bind| {
            // preserve each exact PostgreSQL scalar value
            match bind {
                ForecastPressureContextBind::Text(value)
                | ForecastPressureContextBind::UtcInstant(value) => value.clone(),
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

// verifies complete SQL and bind parity with the compiled repository and route
#[test]
fn matches_compiled_node_pressure_context_plan() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/forecast-pressure-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run forecast pressure Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let lines = stdout.lines().collect::<Vec<_>>();
    let plan = forecast_pressure_context_plan(&admitted(1_787_284_800_000))
        .expect("forecast pressure context plan");

    assert_eq!(lines[0], format!("sql={}", normalize_sql(&plan.sql)));
    assert_eq!(lines[1], format!("binds={}", render_binds(&plan.binds)));
    assert_eq!(
        lines[2],
        "forecast pressure repository and route Node oracle passed"
    );
}

// verifies all fixed source offsets are required before SQL binding
#[test]
fn rejects_mismatched_pressure_context_offsets() {
    let mismatched_start = admitted(1_787_288_400_000);
    assert_eq!(
        forecast_pressure_context_plan(&mismatched_start),
        Err(ForecastPressureContextPlanError)
    );
}
