use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    ForecastObservationHourlyMetrics, ForecastObservationHourlyStationRow,
    ForecastObservationProviderFamily, ForecastObservationStationKey, Nullable,
};
use weather_database::{
    AdmittedCausalEcmwfPriorHoursQuery, AdmittedCausalForecastObservationQuery,
    CAUSAL_ECMWF_TEMPERATURE_PRIOR_HOURS_SQL, CAUSAL_FORECAST_OBSERVATION_HOURLY_STATIONS_SQL,
    CausalEcmwfPriorHourDbRow, CausalTemperatureReadBind, CausalTemperatureReadPlanError,
    causal_ecmwf_prior_hours_read_plan, causal_forecast_observation_read_plan,
    project_causal_ecmwf_prior_hours, project_causal_temperature_station_rows,
};

const FROM: &str = "2026-09-04T18:00:00.000Z";
const OBSERVATION_TO: &str = "2026-09-07T18:00:00.000Z";
const PRIOR_TO_INCLUSIVE: &str = "2026-09-07T17:00:00.000Z";
const TARGET: &str = "2026-09-08T00:00:00.000Z";

/// Compare both SQL plans, binds, and nullable projections to compiled Node.
#[test]
fn matches_compiled_causal_temperature_reads() {
    let lines = node_oracle();
    let epochs = lines[6]
        .strip_prefix("epochs=")
        .expect("epoch prefix")
        .split('=')
        .map(|value| value.parse::<i64>().expect("epoch value"))
        .collect::<Vec<_>>();
    let observation =
        causal_forecast_observation_read_plan(&AdmittedCausalForecastObservationQuery::admitted(
            "ballydidean".to_owned(),
            FROM.to_owned(),
            epochs[0],
            OBSERVATION_TO.to_owned(),
            epochs[1],
            TARGET.to_owned(),
        ))
        .expect("causal observation plan");
    let prior = causal_ecmwf_prior_hours_read_plan(&AdmittedCausalEcmwfPriorHoursQuery::admitted(
        "ballydidean".to_owned(),
        FROM.to_owned(),
        epochs[0],
        PRIOR_TO_INCLUSIVE.to_owned(),
        epochs[2],
        TARGET.to_owned(),
    ))
    .expect("causal prior plan");

    assert_eq!(
        lines[0],
        format!("observation-sql={}", normalize_sql(observation.sql))
    );
    assert_eq!(
        observation.sql,
        CAUSAL_FORECAST_OBSERVATION_HOURLY_STATIONS_SQL
    );
    assert_eq!(
        lines[1],
        format!("observation-binds={}", render_binds(&observation.binds))
    );
    assert_eq!(lines[3], format!("prior-sql={}", normalize_sql(prior.sql)));
    assert_eq!(prior.sql, CAUSAL_ECMWF_TEMPERATURE_PRIOR_HOURS_SQL);
    assert_eq!(
        lines[4],
        format!("prior-binds={}", render_binds(&prior.binds))
    );

    let projected_prior = project_causal_ecmwf_prior_hours(vec![prior_row()]);
    assert_eq!(
        lines[5],
        format!(
            "prior-row={}={}={}={}={}={}={}={}",
            projected_prior[0].key,
            projected_prior[0].contentHash,
            projected_prior[0].modelLeadHours,
            render_nullable(&projected_prior[0].rawRelativeHumidityPercent),
            projected_prior[0].rawTemperatureC,
            render_nullable(&projected_prior[0].rawWindSpeedMps),
            projected_prior[0].runInitializedAt,
            projected_prior[0].validAt
        )
    );

    let station_rows = project_causal_temperature_station_rows(vec![station_row(
        ForecastObservationStationKey::AmbientMaxweather,
        ForecastObservationProviderFamily::Ambient,
        FROM,
    )]);
    assert_eq!(station_rows.len(), 1);
    assert_eq!(
        station_rows[0].physical_station_key,
        ForecastObservationStationKey::AmbientMaxweather
    );
    assert_eq!(station_rows[0].temperature_c, Nullable::Null);
    assert_eq!(station_rows[0].valid_at, FROM);
    assert_eq!(
        lines[2],
        "observation-rows=792=ambient-maxweather=null=2026-09-04T18:00:00.000Z=tempest-64255=null=2026-09-07T17:00:00.000Z"
    );
}

/// Preserve the source's hourly and inclusive-window admission boundaries.
#[test]
fn rejects_invalid_causal_temperature_windows() {
    let misaligned = AdmittedCausalForecastObservationQuery::admitted(
        "ballydidean".to_owned(),
        FROM.to_owned(),
        1,
        OBSERVATION_TO.to_owned(),
        3_600_001,
        TARGET.to_owned(),
    );
    assert_eq!(
        causal_forecast_observation_read_plan(&misaligned),
        Err(CausalTemperatureReadPlanError::ObservationWindowNotHourly)
    );
    let excessive = AdmittedCausalEcmwfPriorHoursQuery::admitted(
        "ballydidean".to_owned(),
        FROM.to_owned(),
        0,
        PRIOR_TO_INCLUSIVE.to_owned(),
        72 * 3_600_000,
        TARGET.to_owned(),
    );
    assert_eq!(
        causal_ecmwf_prior_hours_read_plan(&excessive),
        Err(CausalTemperatureReadPlanError::InvalidPriorHourWindow)
    );
}

/// Invoke the ordinary compiled repository functions over one fake queryable.
fn node_oracle() -> Vec<String> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/causal-temperature-reads-node-oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .output()
        .expect("Node oracle must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Node oracle must emit UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Collapse irrelevant SQL whitespace while retaining every token.
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Render typed binds through the same fake-queryable scalar encoding.
fn render_binds(binds: &[CausalTemperatureReadBind]) -> String {
    binds
        .iter()
        .map(|bind| {
            // preserve scalar and array placeholder values without a codec
            match bind {
                CausalTemperatureReadBind::Text(value)
                | CausalTemperatureReadBind::UtcInstant(value) => value.clone(),
                CausalTemperatureReadBind::TextArray(values) => values.join(","),
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Build one representative nullable decoded prior-hour row.
fn prior_row() -> CausalEcmwfPriorHourDbRow {
    CausalEcmwfPriorHourDbRow {
        content_hash: "a".repeat(64),
        model_lead_hours: 11,
        raw_relative_humidity_percent: Nullable::Null,
        raw_temperature_c: 10.25,
        raw_wind_speed_mps: Nullable::Value(3.5),
        run_initialized_at: "2026-09-07T06:00:00.000Z".to_owned(),
        valid_at: PRIOR_TO_INCLUSIVE.to_owned(),
    }
}

/// Build one full canonical hourly station row for the reducer projection.
fn station_row(
    physical_station_key: ForecastObservationStationKey,
    provider_family: ForecastObservationProviderFamily,
    valid_at: &str,
) -> ForecastObservationHourlyStationRow {
    ForecastObservationHourlyStationRow {
        metrics: ForecastObservationHourlyMetrics {
            relativeHumidityPercent: Nullable::Null,
            temperatureC: Nullable::Null,
            windDirectionDegrees: Nullable::Null,
            windGustMps: Nullable::Null,
            windSpeedMps: Nullable::Null,
        },
        physicalStationKey: physical_station_key,
        providerFamily: provider_family,
        sourceKeys: Vec::new(),
        validAt: valid_at.to_owned(),
    }
}

/// Render one nullable metric like the Node oracle.
fn render_nullable(value: &Nullable<f64>) -> String {
    match value {
        Nullable::Value(value) => value.to_string(),
        Nullable::Null => "null".to_owned(),
    }
}
