use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{Nullable, RainAdjustmentHour, RainCollectionStatus};
use weather_database::{
    AdmittedRevisionUtcInstant, RAIN_ADJUSTMENT_RUN_READ_SQL, RAIN_COLLECTION_STATUS_READ_SQL,
    RainAdjustmentRunReadPlan, RainAdjustmentRunReadRequest, RainAdjustmentRunReadRow,
    RainCollectionStatusReadPlan, RainCollectionStatusReadRow, RainServingReadBind,
    RainServingReadError, RainServingReadPort, Sha256Digest, read_rain_adjustment_run,
    read_rain_collection_status,
};

const AS_OF: &str = "2026-09-08T06:00:00.000Z";

#[derive(Debug, Clone, PartialEq)]
struct FixturePort {
    calls: Vec<&'static str>,
    run: Option<RainAdjustmentRunReadRow>,
    run_plan: Option<RainAdjustmentRunReadPlan>,
    status: Option<RainCollectionStatusReadRow>,
    status_plan: Option<RainCollectionStatusReadPlan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// Render one injected query failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

impl RainServingReadPort for FixturePort {
    type Error = FixtureError;

    /// Capture and answer one recent run read.
    fn select_rain_adjustment_run(
        &mut self,
        plan: &RainAdjustmentRunReadPlan,
    ) -> Result<Option<RainAdjustmentRunReadRow>, Self::Error> {
        self.calls.push("run");
        self.run_plan = Some(plan.clone());
        Ok(self.run.clone())
    }

    /// Capture and answer one aggregate-only status read.
    fn select_rain_collection_status(
        &mut self,
        plan: &RainCollectionStatusReadPlan,
    ) -> Result<Option<RainCollectionStatusReadRow>, Self::Error> {
        self.calls.push("status");
        self.status_plan = Some(plan.clone());
        Ok(self.status.clone())
    }
}

/// Match compiled Node SQL, admission, row metadata, order, and nullability.
#[test]
fn matches_compiled_rain_serving_reads() {
    let oracle = node_oracle();
    let as_of_epoch = oracle["epochs"].parse::<i64>().expect("as-of epoch");
    let mut port = fixture_port();
    let run = read_rain_adjustment_run(
        &mut port,
        &RainAdjustmentRunReadRequest {
            as_of: AS_OF.to_owned(),
            as_of_epoch_milliseconds: Some(as_of_epoch),
            site_slug: "ballydidean".to_owned(),
        },
    )
    .expect("run query")
    .expect("run row");
    let status = read_rain_collection_status(&mut port).expect("status row");

    assert_eq!(port.calls, ["run", "status"]);
    let run_plan = port.run_plan.expect("run plan");
    let status_plan = port.status_plan.expect("status plan");
    assert_eq!(run_plan.sql, RAIN_ADJUSTMENT_RUN_READ_SQL);
    assert_eq!(status_plan.sql, RAIN_COLLECTION_STATUS_READ_SQL);
    assert_eq!(normalize_sql(run_plan.sql), oracle["run-sql"]);
    assert_eq!(render_binds(&run_plan.binds), oracle["run-binds"]);
    assert_eq!(normalize_sql(status_plan.sql), oracle["status-sql"]);
    assert_eq!(render_run(&run), oracle["run"]);
    assert_eq!(render_status(&status), oracle["status"]);
}

/// Preserve pre-query null admission and ordinary missing rows.
#[test]
fn preserves_invalid_and_missing_rain_run_results() {
    let mut port = fixture_port();
    for request in [
        RainAdjustmentRunReadRequest {
            as_of: AS_OF.to_owned(),
            as_of_epoch_milliseconds: Some(1),
            site_slug: "elsewhere".to_owned(),
        },
        RainAdjustmentRunReadRequest {
            as_of: "not-a-date".to_owned(),
            as_of_epoch_milliseconds: None,
            site_slug: "ballydidean".to_owned(),
        },
    ] {
        assert_eq!(read_rain_adjustment_run(&mut port, &request), Ok(None));
    }
    assert!(port.calls.is_empty());

    port.run = None;
    assert_eq!(
        read_rain_adjustment_run(
            &mut port,
            &RainAdjustmentRunReadRequest {
                as_of: AS_OF.to_owned(),
                as_of_epoch_milliseconds: Some(1_788_847_200_000),
                site_slug: "ballydidean".to_owned(),
            }
        ),
        Ok(None)
    );
    assert_eq!(port.calls, ["run"]);
}

/// Reject a status view that differs from the frozen non-serving contract.
#[test]
fn rejects_rain_collection_status_contract_drift() {
    let mut port = fixture_port();
    port.status.as_mut().expect("status").model_enabled = true;
    assert_eq!(
        read_rain_collection_status(&mut port),
        Err(RainServingReadError::StatusContractMismatch)
    );
    assert_eq!(
        RainServingReadError::<FixtureError>::StatusContractMismatch.to_string(),
        "rain collection status contract mismatch"
    );
}

/// Invoke both actual compiled repository functions over fake queryables.
fn node_oracle() -> BTreeMap<String, String> {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/rain-serving-node-oracle.mjs");
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

/// Build representative typed rows returned by the two database reads.
fn fixture_port() -> FixturePort {
    FixturePort {
        calls: Vec::new(),
        run: Some(RainAdjustmentRunReadRow {
            decision_at: instant("2026-09-08T02:00:00.000Z"),
            first_received_at: instant("2026-09-07T18:03:00.000Z"),
            forecast_claim_id: "claim-1".to_owned(),
            generated_at: instant("2026-09-08T02:05:00.000Z"),
            hours: vec![
                RainAdjustmentHour {
                    validAt: "2026-09-08T01:00:00.000Z".to_owned(),
                    modelLeadHours: 7,
                    rawPrecipitationMm: 0.0,
                    correctedPrecipitationMm: 0.0,
                    applied: false,
                    reasonCode: Nullable::Null,
                },
                RainAdjustmentHour {
                    validAt: "2026-09-08T02:00:00.000Z".to_owned(),
                    modelLeadHours: 8,
                    rawPrecipitationMm: 1.5,
                    correctedPrecipitationMm: 1.25,
                    applied: true,
                    reasonCode: Nullable::Value("hurdle_active".to_owned()),
                },
            ],
            input_sha256: Sha256Digest::admitted("b".repeat(64)),
            model_sha256: Sha256Digest::admitted("a".repeat(64)),
            run_initialized_at: instant("2026-09-07T18:00:00.000Z"),
        }),
        run_plan: None,
        status: Some(RainCollectionStatusReadRow {
            claims: 14,
            compressed_bytes: 4096,
            contract_version: "rain-prospective-capture/v1".to_owned(),
            failed_requests: 2,
            last_claim_at: Some(instant("2026-09-08T05:00:00.000Z")),
            last_forecast_receipt_at: Some(instant("2026-09-08T04:00:00.000Z")),
            last_receipt_at: Some(instant("2026-09-08T05:05:00.000Z")),
            last_station_receipt_at: None,
            model_enabled: false,
            paused_until: None,
            pending_requests: 1,
            qualification_enabled: false,
            receipts: 12,
            stations_seen: 9,
            timely_forecasts: 3,
            unknown_requests: 1,
            valid_forecasts: 4,
            valid_station_windows: 5,
        }),
        status_plan: None,
    }
}

/// Admit one canonical fixture instant through the existing typed boundary.
fn instant(value: &str) -> AdmittedRevisionUtcInstant {
    AdmittedRevisionUtcInstant::admitted(value)
}

/// Collapse irrelevant SQL whitespace while retaining every token.
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Render the one exact run-query bind.
fn render_binds(binds: &[RainServingReadBind; 1]) -> String {
    binds
        .iter()
        .map(|bind| match bind {
            RainServingReadBind::UtcInstant(value) => value.clone(),
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Render ordered run metadata and nullable hour reasons like the Node oracle.
fn render_run(run: &weather_contracts::RainAdjustmentRun) -> String {
    let hours = run
        .hours
        .iter()
        .map(|hour| {
            format!(
                "{}~{}~{}~{}~{}~{}",
                hour.validAt,
                hour.modelLeadHours,
                hour.rawPrecipitationMm,
                hour.correctedPrecipitationMm,
                hour.applied,
                nullable_string(&hour.reasonCode)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        run.runInitializedAt,
        run.modelSha256,
        run.inputSha256,
        run.forecastClaimId,
        run.firstReceivedAt,
        run.decisionAt,
        run.generatedAt,
        hours
    )
}

/// Render aggregate counts and nullable admitted timestamps.
fn render_status(status: &RainCollectionStatus) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        status.claims,
        status.receipts,
        status.validForecasts,
        status.timelyForecasts,
        status.validStationWindows,
        status.stationsSeen,
        status.failedRequests,
        status.pendingRequests,
        status.unknownRequests,
        nullable_string(&status.lastClaimAt),
        nullable_string(&status.lastReceiptAt),
        nullable_string(&status.lastForecastReceiptAt),
        nullable_string(&status.lastStationReceiptAt),
        nullable_string(&status.pausedUntil),
        status.compressedBytes
    )
}

/// Render one generated nullable string.
fn nullable_string(value: &Nullable<String>) -> &str {
    match value {
        Nullable::Null => "null",
        Nullable::Value(value) => value,
    }
}
