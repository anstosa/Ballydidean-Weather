use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{DailyPrecipitationQuery, DailyPrecipitationRow, JsDateOrString};
use weather_database::{
    AdmittedDailyPrecipitationQuery, DAILY_PRECIPITATION_SQL, DailyPrecipitationBind,
    DailyPrecipitationReadPlanError, daily_precipitation_read_plan,
    project_daily_precipitation_row,
};

/// encode one generated UTF-16 string
fn text(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// build one admitted local-day query
fn admitted(
    from_epoch_milliseconds: i64,
    to_epoch_milliseconds: i64,
) -> AdmittedDailyPrecipitationQuery {
    AdmittedDailyPrecipitationQuery::admitted(
        DailyPrecipitationQuery {
            from: "2026-08-21T07:00:00.000Z".to_owned(),
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
fn render_binds(binds: &[DailyPrecipitationBind]) -> String {
    binds
        .iter()
        .map(|bind| {
            // preserve each exact PostgreSQL scalar value
            match bind {
                DailyPrecipitationBind::Text(value) | DailyPrecipitationBind::UtcInstant(value) => {
                    value.clone()
                }
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// build one typed decoded daily row
fn row(source_id: &str) -> DailyPrecipitationRow {
    DailyPrecipitationRow {
        accumulationMm: 2.54,
        sourceId: source_id.to_owned(),
        stationSlug: "ballydidean-ecowitt".to_owned(),
        validThrough: JsDateOrString::String(text("2026-08-22T04:59:00.000Z")),
    }
}

// verifies complete SQL, bind, and first-row parity with the compiled Node repository
#[test]
fn matches_compiled_node_daily_precipitation_plan() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/daily-precipitation-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run daily precipitation repository Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let lines = stdout.lines().collect::<Vec<_>>();
    let plan = daily_precipitation_read_plan(&admitted(1_787_295_600_000, 1_787_374_800_000))
        .expect("daily precipitation read plan");

    assert_eq!(plan.sql, DAILY_PRECIPITATION_SQL);
    assert_eq!(lines[0], format!("sql={}", normalize_sql(plan.sql)));
    assert_eq!(lines[1], format!("binds={}", render_binds(&plan.binds)));
    assert_eq!(lines[2], "row=13|2.54");
}

// verifies the exact source repository ordered-window guard
#[test]
fn rejects_unordered_daily_precipitation_window() {
    let unordered = admitted(1_787_374_800_000, 1_787_374_800_000);
    assert_eq!(
        daily_precipitation_read_plan(&unordered),
        Err(DailyPrecipitationReadPlanError)
    );
}

// verifies first-row-or-null projection without reordering decoded rows
#[test]
fn preserves_first_daily_precipitation_row() {
    let selected = project_daily_precipitation_row(vec![row("13"), row("14")]);
    assert_eq!(selected.row.expect("first row").sourceId, "13");
    assert_eq!(project_daily_precipitation_row(Vec::new()).row, None);
}
