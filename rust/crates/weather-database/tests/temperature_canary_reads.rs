use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
    EcmwfTemperatureCanaryRunAdapterVersionLiteral, EcmwfTemperatureCanaryRunModelCycleEnum,
    EcmwfTemperatureCanaryRunStateStatusEnum, EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    EcmwfTemperatureCanaryStatusStateStatusEnum, Nullable,
};
use weather_database::{
    AdmittedEcmwfTemperatureCanarySidecarQuery, AdmittedEcmwfTemperatureCanaryStatusQuery,
    ECMWF_TEMPERATURE_CANARY_RUN_INITIALIZATIONS_SQL, ECMWF_TEMPERATURE_CANARY_SIDECAR_HOURS_SQL,
    ECMWF_TEMPERATURE_CANARY_SIDECAR_RUN_SQL, ECMWF_TEMPERATURE_CANARY_STATUS_SQL,
    EcmwfTemperatureCanaryHourReadRow, EcmwfTemperatureCanaryReadBind,
    EcmwfTemperatureCanaryReadError, EcmwfTemperatureCanaryReadPlanError,
    EcmwfTemperatureCanaryReadPort, EcmwfTemperatureCanarySidecarHoursReadPlan,
    EcmwfTemperatureCanarySidecarRunReadPlan, EcmwfTemperatureCanaryStatusReadPlan,
    EcmwfTemperatureCanaryStatusReadRow, Sha256Digest, StoredEcmwfCanaryRun,
    ecmwf_temperature_canary_run_initializations_read_plan,
    ecmwf_temperature_canary_sidecar_run_read_plan, get_ecmwf_temperature_canary_sidecar,
    get_ecmwf_temperature_canary_status,
};

const RUN_AT: &str = "2026-09-07T12:00:00.000Z";
const SIDECAR_AS_OF: &str = "2026-09-07T18:03:00.000Z";
const STATUS_AS_OF: &str = "2026-09-08T00:03:00.000Z";
const FROM: &str = "2026-09-07T12:00:00.000Z";
const TO: &str = "2026-09-09T00:00:00.000Z";

/// Match the original inclusive inventory window and exact query binds.
#[test]
fn admits_equal_and_exactly_seven_day_inventory_bounds() {
    use EcmwfTemperatureCanaryReadBind::{Text, UtcInstant};
    let original = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ecmwf-canary-run-inventory-node-oracle.mjs");
    let output = Command::new("node")
        .arg(original)
        .output()
        .expect("run Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let start = "2026-09-04T18:00:00.000Z";
    let start_ms = 1_788_544_800_000;
    let seven_days = 7 * 86_400_000;

    // keep both the point lookup and the full warmup envelope inclusive
    for (end, end_ms) in [
        (start, start_ms),
        ("2026-09-11T18:00:00.000Z", start_ms + seven_days),
    ] {
        let plan = ecmwf_temperature_canary_run_initializations_read_plan(
            "ballydidean",
            start.to_owned(),
            start_ms,
            end.to_owned(),
            end_ms,
        )
        .expect("valid warmup interval");
        assert_eq!(plan.sql, ECMWF_TEMPERATURE_CANARY_RUN_INITIALIZATIONS_SQL);
        assert_eq!(
            plan.binds,
            [
                Text("ballydidean".into()),
                UtcInstant(start.into()),
                UtcInstant(end.into())
            ]
        );
    }
    assert_eq!(
        ecmwf_temperature_canary_run_initializations_read_plan(
            "ballydidean",
            start.to_owned(),
            start_ms,
            "2026-09-11T18:00:00.001Z".to_owned(),
            start_ms + seven_days + 1,
        ),
        Err(EcmwfTemperatureCanaryReadPlanError::InvalidRunInventoryInterval)
    );
}

#[derive(Debug, Clone, PartialEq)]
struct FixturePort {
    calls: Vec<&'static str>,
    hours: Vec<EcmwfTemperatureCanaryHourReadRow>,
    hours_plan: Option<EcmwfTemperatureCanarySidecarHoursReadPlan>,
    run: Option<StoredEcmwfCanaryRun>,
    run_plan: Option<EcmwfTemperatureCanarySidecarRunReadPlan>,
    status: Option<EcmwfTemperatureCanaryStatusReadRow>,
    status_plan: Option<EcmwfTemperatureCanaryStatusReadPlan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// Render one injected fake-query failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

impl EcmwfTemperatureCanaryReadPort for FixturePort {
    type Error = FixtureError;

    /// Capture and answer one sidecar run query.
    fn select_sidecar_run(
        &mut self,
        plan: &EcmwfTemperatureCanarySidecarRunReadPlan,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        self.calls.push("run");
        self.run_plan = Some(plan.clone());
        Ok(self.run.clone())
    }

    /// Capture and answer one selected-run hour query.
    fn select_sidecar_hours(
        &mut self,
        plan: &EcmwfTemperatureCanarySidecarHoursReadPlan,
    ) -> Result<Vec<EcmwfTemperatureCanaryHourReadRow>, Self::Error> {
        self.calls.push("hours");
        self.hours_plan = Some(plan.clone());
        Ok(self.hours.clone())
    }

    /// Capture and answer one independent collector-status query.
    fn select_status(
        &mut self,
        plan: &EcmwfTemperatureCanaryStatusReadPlan,
    ) -> Result<Option<EcmwfTemperatureCanaryStatusReadRow>, Self::Error> {
        self.calls.push("status");
        self.status_plan = Some(plan.clone());
        Ok(self.status.clone())
    }
}

/// Match compiled Node SQL, binds, two-stage ordering, and nullable projections.
#[test]
fn matches_compiled_temperature_canary_serving_reads() {
    let oracle = node_oracle();
    let epochs = oracle["epochs"]
        .split('=')
        .map(|value| value.parse::<i64>().expect("epoch"))
        .collect::<Vec<_>>();
    let mut port = fixture_port();
    let sidecar =
        get_ecmwf_temperature_canary_sidecar(&mut port, &sidecar_query(epochs[0], epochs[1]))
            .expect("sidecar query")
            .expect("sidecar row");
    let status = get_ecmwf_temperature_canary_status(
        &mut port,
        &AdmittedEcmwfTemperatureCanaryStatusQuery::admitted(
            STATUS_AS_OF.to_owned(),
            "ballydidean".to_owned(),
        ),
    )
    .expect("status query")
    .expect("status row");

    assert_eq!(port.calls, ["run", "hours", "status"]);
    let run_plan = port.run_plan.expect("run plan");
    let hours_plan = port.hours_plan.expect("hours plan");
    let status_plan = port.status_plan.expect("status plan");
    assert_eq!(run_plan.sql, ECMWF_TEMPERATURE_CANARY_SIDECAR_RUN_SQL);
    assert_eq!(hours_plan.sql, ECMWF_TEMPERATURE_CANARY_SIDECAR_HOURS_SQL);
    assert_eq!(status_plan.sql, ECMWF_TEMPERATURE_CANARY_STATUS_SQL);
    assert_eq!(normalize_sql(run_plan.sql), oracle["sidecar-run-sql"]);
    assert_eq!(render_binds(&run_plan.binds), oracle["sidecar-run-binds"]);
    assert_eq!(normalize_sql(hours_plan.sql), oracle["sidecar-hours-sql"]);
    assert_eq!(
        render_binds(&hours_plan.binds),
        oracle["sidecar-hours-binds"]
    );
    assert_eq!(normalize_sql(status_plan.sql), oracle["status-sql"]);
    assert_eq!(render_binds(&status_plan.binds), oracle["status-binds"]);
    assert_eq!(
        render_sidecar(&sidecar),
        oracle["sidecar"],
        "nullable metrics and lead ordering"
    );
    assert_eq!(render_status(&status), oracle["status"]);
}

/// Preserve null results and skip the dependent hour read when no run exists.
#[test]
fn preserves_missing_temperature_canary_results() {
    let oracle = node_oracle();
    let epochs = oracle["epochs"]
        .split('=')
        .map(|value| value.parse::<i64>().expect("epoch"))
        .collect::<Vec<_>>();
    let mut port = FixturePort {
        calls: Vec::new(),
        hours: Vec::new(),
        hours_plan: None,
        run: None,
        run_plan: None,
        status: None,
        status_plan: None,
    };

    assert_eq!(
        get_ecmwf_temperature_canary_sidecar(&mut port, &sidecar_query(epochs[0], epochs[1])),
        Ok(None)
    );
    assert_eq!(port.calls, ["run"]);
    assert_eq!(
        get_ecmwf_temperature_canary_status(
            &mut port,
            &AdmittedEcmwfTemperatureCanaryStatusQuery::admitted(
                STATUS_AS_OF.to_owned(),
                "missing".to_owned(),
            )
        ),
        Ok(None)
    );
    assert_eq!(port.calls, ["run", "status"]);
    assert_eq!(oracle["missing"], "true=true=1=1");
}

/// Reject empty or excessive sidecar horizons before issuing the run query.
#[test]
fn enforces_node_sidecar_horizon_boundary() {
    let mut port = fixture_port();
    let empty = sidecar_query(10, 10);
    assert_eq!(
        get_ecmwf_temperature_canary_sidecar(&mut port, &empty),
        Err(EcmwfTemperatureCanaryReadError::Input(
            EcmwfTemperatureCanaryReadPlanError::InvalidHorizon
        ))
    );
    assert!(port.calls.is_empty());

    let excessive = sidecar_query(0, 264 * 3_600_000 + 1);
    assert_eq!(
        ecmwf_temperature_canary_sidecar_run_read_plan(&excessive),
        Err(EcmwfTemperatureCanaryReadPlanError::InvalidHorizon)
    );
    let exact = sidecar_query(0, 264 * 3_600_000);
    assert!(ecmwf_temperature_canary_sidecar_run_read_plan(&exact).is_ok());
}

/// Invoke the ordinary compiled repository functions over a fake queryable.
fn node_oracle() -> BTreeMap<String, String> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/temperature-canary-reads-node-oracle.mjs");
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
        .map(|line| {
            let (key, value) = line.split_once('=').expect("oracle field");
            (key.to_owned(), value.to_owned())
        })
        .collect()
}

/// Build one admitted sidecar request with caller-supplied boundary epochs.
fn sidecar_query(
    from_epoch_milliseconds: i64,
    to_epoch_milliseconds: i64,
) -> AdmittedEcmwfTemperatureCanarySidecarQuery {
    AdmittedEcmwfTemperatureCanarySidecarQuery::admitted(
        SIDECAR_AS_OF.to_owned(),
        FROM.to_owned(),
        from_epoch_milliseconds,
        "ballydidean".to_owned(),
        TO.to_owned(),
        to_epoch_milliseconds,
    )
}

/// Build representative rows returned by the three fake database calls.
fn fixture_port() -> FixturePort {
    FixturePort {
        calls: Vec::new(),
        hours: vec![
            EcmwfTemperatureCanaryHourReadRow {
                content_hash: Sha256Digest::admitted("d".repeat(64)),
                model_lead_hours: 7,
                raw_relative_humidity_percent: Nullable::Null,
                raw_temperature_c: 10.5,
                raw_wind_speed_mps: Nullable::Value(3.5),
                valid_at: "2026-09-07T19:00:00.000Z".to_owned(),
            },
            EcmwfTemperatureCanaryHourReadRow {
                content_hash: Sha256Digest::admitted("e".repeat(64)),
                model_lead_hours: 8,
                raw_relative_humidity_percent: Nullable::Value(80.0),
                raw_temperature_c: 11.0,
                raw_wind_speed_mps: Nullable::Null,
                valid_at: "2026-09-07T20:00:00.000Z".to_owned(),
            },
        ],
        hours_plan: None,
        run: Some(stored_run()),
        run_plan: None,
        status: Some(EcmwfTemperatureCanaryStatusReadRow {
            first_received_at: "2026-09-07T17:59:00.000Z".to_owned(),
            hour_count: 18,
            run_initialized_at: RUN_AT.to_owned(),
            state_reason: "no_causal_forecast_observation_pairs".to_owned(),
            state_status: EcmwfTemperatureCanaryRunStateStatusEnum::Cold,
        }),
        status_plan: None,
    }
}

/// Build one trusted run row equivalent to the compiled Node fixture.
fn stored_run() -> StoredEcmwfCanaryRun {
    StoredEcmwfCanaryRun {
        adapter_version: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
        content_hash: Sha256Digest::admitted("b".repeat(64)),
        first_received_at: "2026-09-07T17:59:00.000Z".to_owned(),
        id: "run-1".to_owned(),
        model_cycle: EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1,
        provider_response_sha256: Sha256Digest::admitted("a".repeat(64)),
        recent_error_state: cold_state(),
        recent_error_state_sha256: Sha256Digest::admitted("c".repeat(64)),
        run_initialized_at: RUN_AT.to_owned(),
        state_reason: "no_causal_forecast_observation_pairs".to_owned(),
        state_status: EcmwfTemperatureCanaryRunStateStatusEnum::Cold,
        upstream_model: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    }
}

/// Build the exact cold recent-error state returned by the Node fixture.
fn cold_state() -> EcmwfTemperatureCanaryRecentErrorState {
    EcmwfTemperatureCanaryRecentErrorState {
        b24C: Nullable::Null,
        b72C: Nullable::Null,
        cohort: EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
        localDates: 0,
        mad72C: Nullable::Null,
        maximumSourceRunInitializedAt: Nullable::Null,
        maximumSourceValidAt: Nullable::Null,
        n24: 0,
        n72: 0,
        sourceKeys: Vec::new(),
        supported: false,
        targetRunInitializedAt: RUN_AT.to_owned(),
        windowEndValidAt: "2026-09-07T05:00:00.000Z".to_owned(),
    }
}

/// Collapse irrelevant SQL whitespace while retaining every token.
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Render typed scalar binds through the fake-queryable representation.
fn render_binds(binds: &[EcmwfTemperatureCanaryReadBind]) -> String {
    binds
        .iter()
        .map(|bind| match bind {
            EcmwfTemperatureCanaryReadBind::Text(value)
            | EcmwfTemperatureCanaryReadBind::UtcInstant(value) => value.clone(),
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Render the selected fields used by the compiled projection oracle.
fn render_sidecar(sidecar: &weather_contracts::EcmwfTemperatureCanarySidecar) -> String {
    format!(
        "{}={}={}={}={}={}={}={}={}",
        sidecar.run.id,
        run_status_wire(sidecar.run.stateStatus),
        sidecar.hours.len(),
        sidecar.hours[0].modelLeadHours,
        nullable_wire(&sidecar.hours[0].rawRelativeHumidityPercent),
        nullable_wire(&sidecar.hours[0].rawWindSpeedMps),
        sidecar.hours[1].modelLeadHours,
        nullable_wire(&sidecar.hours[1].rawRelativeHumidityPercent),
        nullable_wire(&sidecar.hours[1].rawWindSpeedMps)
    )
}

/// Render the collector status fields used by the compiled oracle.
fn render_status(status: &weather_contracts::EcmwfTemperatureCanaryStatus) -> String {
    format!(
        "{}={}={}={}={}",
        status.firstReceivedAt,
        status.hourCount,
        status.latestRunInitializedAt,
        status_status_wire(status.stateStatus),
        status.stateReason
    )
}

/// Render one nullable metric like JavaScript's nullish display.
fn nullable_wire(value: &Nullable<f64>) -> String {
    match value {
        Nullable::Null => "null".to_owned(),
        Nullable::Value(value) => value.to_string(),
    }
}

/// Render the fixture run-status literal.
fn run_status_wire(status: EcmwfTemperatureCanaryRunStateStatusEnum) -> &'static str {
    match status {
        EcmwfTemperatureCanaryRunStateStatusEnum::Supported => "supported",
        EcmwfTemperatureCanaryRunStateStatusEnum::Cold => "cold",
        EcmwfTemperatureCanaryRunStateStatusEnum::Insufficient => "insufficient",
        EcmwfTemperatureCanaryRunStateStatusEnum::Invalid => "invalid",
    }
}

/// Render the fixture status-projection literal.
fn status_status_wire(status: EcmwfTemperatureCanaryStatusStateStatusEnum) -> &'static str {
    match status {
        EcmwfTemperatureCanaryStatusStateStatusEnum::Supported => "supported",
        EcmwfTemperatureCanaryStatusStateStatusEnum::Cold => "cold",
        EcmwfTemperatureCanaryStatusStateStatusEnum::Insufficient => "insufficient",
        EcmwfTemperatureCanaryStatusStateStatusEnum::Invalid => "invalid",
    }
}
