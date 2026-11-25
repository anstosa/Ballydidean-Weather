use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    FieldPresence, JsDateOrString, Nullable, TideQuery, TideRecordRow, TideRecordRowSourceKindEnum,
};
use weather_database::{
    AdmittedTideQuery, TIDE_RECORDS_SQL, TideReadBind, TideReadPlanError, project_tide_record_rows,
    tide_read_plan,
};

/// build one explicit-limit tide query
fn query(limit: FieldPresence<u64>) -> TideQuery {
    TideQuery {
        from: "2026-08-21T05:00:00.000Z".to_owned(),
        limit,
        siteSlug: "ballydidean".to_owned(),
        to: "2026-09-01T05:00:00.000Z".to_owned(),
    }
}

/// admit the fixture query with its canonical epoch evidence
fn admitted(limit: FieldPresence<u64>) -> AdmittedTideQuery {
    AdmittedTideQuery::admitted(query(limit), 1_787_288_400_000, 1_788_238_800_000)
}

/// collapse irrelevant SQL formatting like the Node oracle
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// render typed binds through their Node scalar values
fn render_binds(binds: &[TideReadBind]) -> String {
    binds
        .iter()
        .map(|bind| {
            // preserve each exact PostgreSQL scalar value
            match bind {
                TideReadBind::Text(value) | TideReadBind::UtcInstant(value) => value.clone(),
                TideReadBind::Limit(value) => value.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// build one typed decoded tide row
fn row(source_id: &str, source_kind: TideRecordRowSourceKindEnum) -> TideRecordRow {
    TideRecordRow {
        attributionLabel: "NOAA Tides & Currents".to_owned(),
        attributionUrl: "https://tidesandcurrents.noaa.gov/".to_owned(),
        predictionType: Nullable::Null,
        providerKey: "noaa-co-ops".to_owned(),
        sourceId: source_id.to_owned(),
        sourceKind: source_kind,
        stationName: "Glendale".to_owned(),
        stationSlug: "glendale".to_owned(),
        validAt: JsDateOrString::String("2026-08-22T11:30:00.000Z".encode_utf16().collect()),
        waterLevelM: 3.2,
    }
}

// verifies complete SQL and bind parity with the compiled Node repository
#[test]
fn matches_compiled_node_tide_plan() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tides-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run tides repository Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let lines = stdout.lines().collect::<Vec<_>>();
    let plan = tide_read_plan(&admitted(FieldPresence::Value(7))).expect("tide read plan");

    assert_eq!(plan.sql, TIDE_RECORDS_SQL);
    assert_eq!(lines[0], format!("sql={}", normalize_sql(plan.sql)));
    assert_eq!(lines[1], format!("binds={}", render_binds(&plan.binds)));
    assert_eq!(lines[2], "default-limit=10000");
    assert_eq!(lines[3], "rows=42|43");
}

// verifies repository defaults and combined range-limit guard
#[test]
fn enforces_tide_repository_bounds() {
    let default = tide_read_plan(&admitted(FieldPresence::Missing)).expect("default tide limit");
    assert_eq!(default.binds[3], TideReadBind::Limit(10_000));

    let invalid_limit = admitted(FieldPresence::Value(10_001));
    assert_eq!(
        tide_read_plan(&invalid_limit),
        Err(TideReadPlanError::InvalidRangeOrLimit)
    );
    let reversed = AdmittedTideQuery::admitted(query(FieldPresence::Missing), 20, 10);
    assert_eq!(
        tide_read_plan(&reversed),
        Err(TideReadPlanError::InvalidRangeOrLimit)
    );
    let excessive =
        AdmittedTideQuery::admitted(query(FieldPresence::Missing), 0, 31 * 86_400_000 + 1);
    assert_eq!(
        tide_read_plan(&excessive),
        Err(TideReadPlanError::InvalidRangeOrLimit)
    );
}

// verifies typed row projection preserves ascending source order and nulls
#[test]
fn preserves_typed_tide_row_order() {
    let result = project_tide_record_rows(vec![
        row("42", TideRecordRowSourceKindEnum::TideObservation),
        row("43", TideRecordRowSourceKindEnum::TidePrediction),
    ]);
    assert_eq!(result.rows[0].sourceId, "42");
    assert_eq!(result.rows[1].sourceId, "43");
    assert_eq!(result.rows[0].predictionType, Nullable::Null);
}
