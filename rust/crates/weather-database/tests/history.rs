use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    FieldPresence, HistoryQuery, HistoryQueryCursor, JsDateOrString, Nullable, SourceKind,
    WeatherRecordRow,
};
use weather_database::{
    AdmittedHistoryQuery, HistoryReadBind, HistoryReadPlanError, project_weather_history_rows,
    weather_history_read_plan,
};

/// build one full optional-filter query matching the Node oracle
fn query() -> HistoryQuery {
    HistoryQuery {
        cursor: FieldPresence::Value(HistoryQueryCursor {
            id: "90".to_owned(),
            validAt: "2026-08-10T00:00:00.000Z".to_owned(),
        }),
        from: FieldPresence::Value("2026-08-01T00:00:00.000Z".to_owned()),
        limit: FieldPresence::Value(251),
        siteSlug: "ballydidean".to_owned(),
        sourceId: FieldPresence::Value("11".to_owned()),
        sourceKind: FieldPresence::Value(SourceKind::Reanalysis),
        stationSlug: FieldPresence::Value("open-meteo-virtual".to_owned()),
        to: FieldPresence::Value("2026-08-22T00:00:00.000Z".to_owned()),
    }
}

/// build one repository-default history query
fn default_query() -> HistoryQuery {
    HistoryQuery {
        cursor: FieldPresence::Missing,
        from: FieldPresence::Missing,
        limit: FieldPresence::Missing,
        siteSlug: "ballydidean".to_owned(),
        sourceId: FieldPresence::Missing,
        sourceKind: FieldPresence::Missing,
        stationSlug: FieldPresence::Missing,
        to: FieldPresence::Missing,
    }
}

/// admit the full fixture query with canonical epoch evidence
fn admitted_query() -> AdmittedHistoryQuery {
    AdmittedHistoryQuery::admitted(query(), Some(1_785_542_400_000), Some(1_787_356_800_000))
}

/// admit the default fixture without optional time bounds
fn admitted_default_query() -> AdmittedHistoryQuery {
    AdmittedHistoryQuery::admitted(default_query(), None, None)
}

/// build one minimally populated typed history row
fn row(id: &str, valid_at: &str) -> WeatherRecordRow {
    WeatherRecordRow {
        adapterVersion: Nullable::Null,
        apparentTemperatureC: Nullable::Null,
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Null,
        contentHash: format!("hash-{id}"),
        contractEpoch: Nullable::Null,
        deviceModel: Nullable::Null,
        deviceSerial: Nullable::Null,
        deviceVendor: Nullable::Null,
        firstReceivedAt: JsDateOrString::String(text(valid_at)),
        id: id.to_owned(),
        lastReceivedAt: JsDateOrString::String(text(valid_at)),
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Null,
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Null,
        productRunAt: Nullable::Null,
        providerKey: "open-meteo".to_owned(),
        providerMetadata: Nullable::Null,
        qualityMetadata: Nullable::Null,
        relativeHumidityPercent: Nullable::Null,
        revisionCount: 0,
        siteSlug: "ballydidean".to_owned(),
        sourceId: "11".to_owned(),
        sourceConfigFingerprint: Nullable::Null,
        sourceKey: "open-meteo-reanalysis-v1".to_owned(),
        sourceKind: SourceKind::Reanalysis,
        stationSlug: "open-meteo-virtual".to_owned(),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Null,
        upstreamModel: Nullable::Null,
        upstreamTimezone: "UTC".to_owned(),
        uvIndex: Nullable::Null,
        validAt: JsDateOrString::String(text(valid_at)),
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: Nullable::Null,
        windSpeedMps: Nullable::Null,
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

/// encode one generated UTF-16 fixture string
fn text(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// collapse irrelevant SQL formatting like the Node oracle
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// render typed binds through their Node scalar values
fn render_binds(binds: &[HistoryReadBind]) -> String {
    binds
        .iter()
        .map(|bind| {
            // preserve each exact PostgreSQL scalar value
            match bind {
                HistoryReadBind::Text(value)
                | HistoryReadBind::UtcInstant(value)
                | HistoryReadBind::BigintText(value) => value.clone(),
                HistoryReadBind::SourceKind(SourceKind::Reanalysis) => "reanalysis".to_owned(),
                HistoryReadBind::SourceKind(source_kind) => {
                    panic!("unexpected history source kind: {source_kind:?}")
                }
                HistoryReadBind::Limit(value) => value.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

// verifies complete SQL and bind parity with the compiled Node repository
#[test]
fn matches_compiled_node_history_plan() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/history-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run history repository Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let lines = stdout.lines().collect::<Vec<_>>();
    let plan = weather_history_read_plan(&admitted_query()).expect("history read plan");

    assert_eq!(lines[0], format!("sql={}", normalize_sql(&plan.sql)));
    assert_eq!(lines[1], format!("binds={}", render_binds(&plan.binds)));
    assert_eq!(lines[2], "rows=103|102");
    assert_eq!(plan.sql.matches("LIMIT $9").count(), 2);
    assert!(
        plan.sql
            .contains("(candidate.valid_at, candidate.id) < ($7::timestamptz, $8::bigint)")
    );
}

// verifies repository limits and half-open interval ordering
#[test]
fn rejects_invalid_history_bounds() {
    let mut invalid_limit = query();
    invalid_limit.limit = FieldPresence::Value(252);
    assert_eq!(
        weather_history_read_plan(&AdmittedHistoryQuery::admitted(
            invalid_limit,
            Some(1_785_542_400_000),
            Some(1_787_356_800_000),
        )),
        Err(HistoryReadPlanError::LimitOutsideRepositoryRange)
    );

    let mut reversed = query();
    reversed.from = FieldPresence::Value("2026-08-23T00:00:00.000Z".to_owned());
    assert_eq!(
        weather_history_read_plan(&AdmittedHistoryQuery::admitted(
            reversed,
            Some(1_787_443_200_000),
            Some(1_787_356_800_000),
        )),
        Err(HistoryReadPlanError::ReversedRange)
    );
}

// verifies every optional predicate stays absent from the default query
#[test]
fn builds_default_history_plan() {
    let plan = weather_history_read_plan(&admitted_default_query()).expect("default history plan");
    assert_eq!(
        plan.binds,
        [
            HistoryReadBind::Text("ballydidean".to_owned()),
            HistoryReadBind::Limit(100),
        ]
    );
    assert_eq!(plan.sql.matches("LIMIT $2").count(), 2);
    assert!(!plan.sql.contains("st.slug = $"));
    assert!(!plan.sql.contains("s.id = $"));
    assert!(!plan.sql.contains("s.source_kind = $"));
    assert!(!plan.sql.contains("candidate.valid_at >= $"));
    assert!(!plan.sql.contains("candidate.valid_at < $"));
    assert!(!plan.sql.contains("candidate.id) <"));
}

// verifies typed row projection preserves database order and nulls
#[test]
fn preserves_typed_history_row_order() {
    let result = project_weather_history_rows(vec![
        row("103", "2026-08-21T03:00:00.000Z"),
        row("102", "2026-08-21T02:00:00.000Z"),
    ]);
    assert_eq!(result.rows[0].id, "103");
    assert_eq!(result.rows[1].id, "102");
    assert_eq!(result.rows[0].productRunAt, Nullable::Null);
    assert_eq!(result.rows[0].temperatureC, Nullable::Null);
}
