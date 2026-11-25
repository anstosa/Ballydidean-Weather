use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
    EcmwfTemperatureCanaryRunAdapterVersionLiteral, EcmwfTemperatureCanaryRunModelCycleEnum,
    EcmwfTemperatureCanaryRunStateStatusEnum, EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    Nullable,
};
use weather_database::{
    AdmittedEcmwfCanaryRunPersistence, ECMWF_CANARY_EXISTING_HOURS_SQL,
    ECMWF_CANARY_EXISTING_RUN_SQL, ECMWF_CANARY_INSERT_HOUR_SQL, ECMWF_CANARY_INSERT_RUN_SQL,
    ECMWF_CANARY_SITE_SQL, ECMWF_CANARY_UPDATE_RECEIPT_SQL, EcmwfCanaryExistingHoursQuery,
    EcmwfCanaryExistingRunQuery, EcmwfCanaryHourInsertQuery, EcmwfCanaryPersistenceBind,
    EcmwfCanaryPersistenceError, EcmwfCanaryPersistencePort, EcmwfCanaryReceiptUpdateQuery,
    EcmwfCanaryRunInsertQuery, EcmwfCanarySiteQuery, EcmwfCanarySiteRow, PreparedEcmwfCanaryHour,
    SerializedJson, Sha256Digest, StoredEcmwfCanaryHourIdentity, StoredEcmwfCanaryRun,
    persist_ecmwf_temperature_canary_run,
};

const RUN_AT: &str = "2026-09-07T12:00:00.000Z";
const RECEIVED_AT: &str = "2026-09-07T18:03:00.000Z";
const PROVIDER_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixtureMode {
    Insert,
    Retry,
    Revision,
    CommitFailure,
}

#[derive(Debug, Clone, PartialEq)]
struct FixturePort {
    existing_hours: Vec<StoredEcmwfCanaryHourIdentity>,
    hour_queries: Vec<EcmwfCanaryHourInsertQuery>,
    mode: FixtureMode,
    run_query: Option<EcmwfCanaryRunInsertQuery>,
    stored: StoredEcmwfCanaryRun,
    trace: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// Render one injected command failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

impl EcmwfCanaryPersistencePort for FixturePort {
    type Error = FixtureError;

    /// Record one transaction start.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("begin".to_owned());
        Ok(())
    }

    /// Verify and return the active fixture site.
    fn select_site(
        &mut self,
        query: &EcmwfCanarySiteQuery,
    ) -> Result<Option<EcmwfCanarySiteRow>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_SITE_SQL);
        assert_eq!(
            query.binds,
            [EcmwfCanaryPersistenceBind::Text("ballydidean".to_owned())]
        );
        self.trace.push("site".to_owned());
        Ok(Some(EcmwfCanarySiteRow {
            id: "site-1".to_owned(),
            latitude: 47.95,
            longitude: -122.43,
        }))
    }

    /// Capture the exact immutable run insert.
    fn insert_run(
        &mut self,
        query: &EcmwfCanaryRunInsertQuery,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_INSERT_RUN_SQL);
        self.run_query = Some(query.clone());
        self.trace.push("insert-run".to_owned());
        Ok((self.mode == FixtureMode::Insert).then(|| self.stored.clone()))
    }

    /// Capture one exact newly inserted lead.
    fn insert_hour(&mut self, query: &EcmwfCanaryHourInsertQuery) -> Result<(), Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_INSERT_HOUR_SQL);
        let EcmwfCanaryPersistenceBind::Count(lead) = query.binds[2] else {
            panic!("hour lead bind");
        };
        self.trace.push(format!("hour-{lead}"));
        self.hour_queries.push(query.clone());
        Ok(())
    }

    /// Return the locked stored fixture after a conflict.
    fn select_existing_run(
        &mut self,
        query: &EcmwfCanaryExistingRunQuery,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_EXISTING_RUN_SQL);
        self.trace.push("existing-run".to_owned());
        Ok(Some(self.stored.clone()))
    }

    /// Return exact stored hour hashes in lead order.
    fn select_existing_hours(
        &mut self,
        query: &EcmwfCanaryExistingHoursQuery,
    ) -> Result<Vec<StoredEcmwfCanaryHourIdentity>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_EXISTING_HOURS_SQL);
        self.trace.push("existing-hours".to_owned());
        Ok(self.existing_hours.clone())
    }

    /// Record the monotonic retry receipt update.
    fn update_last_received(
        &mut self,
        query: &EcmwfCanaryReceiptUpdateQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_UPDATE_RECEIPT_SQL);
        self.trace.push("update-receipt".to_owned());
        Ok(())
    }

    /// Commit or inject the compiled-oracle failure point.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("commit".to_owned());
        // fail only the dedicated commit fixture
        if self.mode == FixtureMode::CommitFailure {
            return Err(FixtureError("commit failed"));
        }
        Ok(())
    }

    /// Record rollback after a body or commit failure.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("rollback".to_owned());
        Ok(())
    }

    /// Record release after every attempted retained-client transaction.
    fn release_client(&mut self) {
        self.trace.push("release".to_owned());
    }
}

/// Match compiled Node SQL, binds, insert/retry behavior, and rollback order.
#[test]
fn matches_compiled_ecmwf_canary_persistence() {
    let oracle = node_oracle();
    assert_sql(&oracle);
    let input = admitted_input(&oracle, RECEIVED_AT, 0);
    let stored = stored_run(&input);
    let existing_hours = input
        .hours
        .iter()
        .map(|hour| StoredEcmwfCanaryHourIdentity {
            content_hash: hour.content_hash.clone(),
        })
        .collect::<Vec<_>>();

    let mut insert = fixture_port(FixtureMode::Insert, &stored, &existing_hours);
    let result =
        persist_ecmwf_temperature_canary_run(&input, &mut insert).expect("new run must persist");
    assert_eq!(trace(&insert.trace), oracle["insert-trace"]);
    assert_eq!(
        render_binds(&insert.run_query.expect("run query").binds),
        oracle["insert-run-binds"]
    );
    assert_eq!(
        render_binds(&insert.hour_queries[0].binds),
        oracle["first-hour-binds"]
    );
    assert_eq!(
        render_binds(&insert.hour_queries[17].binds),
        oracle["last-hour-binds"]
    );
    assert_eq!(
        format!(
            "{}|{}|{}",
            result.id,
            status_wire(result.stateStatus),
            result.firstReceivedAt
        ),
        oracle["result"]
    );

    let retry_input = admitted_input(&oracle, "2026-09-07T18:05:00.000Z", 120_000);
    let mut retry = fixture_port(FixtureMode::Retry, &stored, &existing_hours);
    let retried = persist_ecmwf_temperature_canary_run(&retry_input, &mut retry)
        .expect("identical retry must persist");
    assert_eq!(retried.firstReceivedAt, RECEIVED_AT);
    assert_eq!(trace(&retry.trace), oracle["retry-trace"]);

    let mut revision_input = admitted_input(&oracle, "2026-09-07T18:06:00.000Z", 180_000);
    revision_input.content_hash = Sha256Digest::admitted("b".repeat(64));
    revision_input.hours[7].content_hash = Sha256Digest::admitted("c".repeat(64));
    let mut revision = fixture_port(FixtureMode::Revision, &stored, &existing_hours);
    assert_eq!(
        persist_ecmwf_temperature_canary_run(&revision_input, &mut revision),
        Err(EcmwfCanaryPersistenceError::RevisionRejected)
    );
    assert_eq!(trace(&revision.trace), oracle["revision-trace"]);

    let commit_input = admitted_input(&oracle, "2026-09-07T18:07:00.000Z", 240_000);
    let mut commit = fixture_port(FixtureMode::CommitFailure, &stored, &existing_hours);
    assert_eq!(
        persist_ecmwf_temperature_canary_run(&commit_input, &mut commit),
        Err(EcmwfCanaryPersistenceError::Command(FixtureError(
            "commit failed"
        )))
    );
    assert_eq!(trace(&commit.trace), oracle["commit-trace"]);
}

/// Reject malformed hour geometry before acquiring a transaction.
#[test]
fn rejects_invalid_ecmwf_canary_hours_before_begin() {
    let oracle = node_oracle();
    let mut input = admitted_input(&oracle, RECEIVED_AT, 0);
    input.hours.pop();
    let stored = stored_run(&input);
    let mut port = fixture_port(FixtureMode::Insert, &stored, &[]);
    assert_eq!(
        persist_ecmwf_temperature_canary_run(&input, &mut port),
        Err(EcmwfCanaryPersistenceError::Input(
            weather_database::EcmwfCanaryPersistenceInputError::InvalidHourCount
        ))
    );
    assert!(port.trace.is_empty());
}

/// Invoke the production function over one retained fake client.
fn node_oracle() -> BTreeMap<String, String> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ecmwf-canary-persistence-node-oracle.mjs");
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

/// Match every copied SQL statement to the captured production query.
fn assert_sql(oracle: &BTreeMap<String, String>) {
    for (key, sql) in [
        ("site-sql", ECMWF_CANARY_SITE_SQL),
        ("insert-run-sql", ECMWF_CANARY_INSERT_RUN_SQL),
        ("insert-hour-sql", ECMWF_CANARY_INSERT_HOUR_SQL),
        ("existing-run-sql", ECMWF_CANARY_EXISTING_RUN_SQL),
        ("existing-hours-sql", ECMWF_CANARY_EXISTING_HOURS_SQL),
        ("update-receipt-sql", ECMWF_CANARY_UPDATE_RECEIPT_SQL),
    ] {
        assert_eq!(normalize_sql(sql), oracle[key], "{key}");
    }
}

/// Build one fully prepared input from hashes produced by the owning source.
fn admitted_input(
    oracle: &BTreeMap<String, String>,
    received_at: &str,
    receipt_offset_ms: i64,
) -> AdmittedEcmwfCanaryRunPersistence {
    let epochs = oracle["epochs"]
        .split('|')
        .map(|value| value.parse::<i64>().expect("epoch"))
        .collect::<Vec<_>>();
    let hashes = oracle["hour-hashes"].split(',').collect::<Vec<_>>();
    let valid_times = oracle["valid-times"].split(',').collect::<Vec<_>>();
    let hours = hashes
        .iter()
        .zip(valid_times)
        .enumerate()
        .map(|(index, (hash, valid_at))| PreparedEcmwfCanaryHour {
            content_hash: Sha256Digest::admitted(*hash),
            model_lead_hours: (index + 1) as u64,
            raw_relative_humidity_percent: Nullable::Value(70.0 + index as f64),
            raw_temperature_c: 10.0 + index as f64 / 10.0,
            raw_wind_speed_mps: Nullable::Value(2.0 + index as f64 / 10.0),
            valid_at: valid_at.to_owned(),
            valid_at_epoch_milliseconds: epochs[0] + (index as i64 + 1) * 3_600_000,
        })
        .collect();
    AdmittedEcmwfCanaryRunPersistence {
        adapter_version: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
        content_hash: Sha256Digest::admitted(&oracle["run-content-hash"]),
        hours,
        model_cycle: EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1,
        provider_response_sha256: Sha256Digest::admitted(PROVIDER_HASH),
        received_at: received_at.to_owned(),
        received_at_epoch_milliseconds: epochs[1] + receipt_offset_ms,
        recent_error_state: cold_state(),
        recent_error_state_json: SerializedJson::admitted(&oracle["recent-state-json"]),
        recent_error_state_maximum_run_epoch_milliseconds: None,
        recent_error_state_maximum_valid_epoch_milliseconds: None,
        recent_error_state_sha256: Sha256Digest::admitted(&oracle["recent-state-hash"]),
        recent_error_state_window_end_epoch_milliseconds: epochs[0] - 7 * 3_600_000,
        recent_error_state_window_end_valid_at: "2026-09-07T05:00:00.000Z".to_owned(),
        run_initialized_at: RUN_AT.to_owned(),
        run_initialized_at_epoch_milliseconds: epochs[0],
        site_slug: "ballydidean".to_owned(),
        state_reason: "no_causal_forecast_observation_pairs".to_owned(),
        state_status: EcmwfTemperatureCanaryRunStateStatusEnum::Cold,
        upstream_model: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    }
}

/// Build the exact cold recent-error state serialized by the Node fixture.
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

/// Build one trusted stored row returned by both run statements.
fn stored_run(input: &AdmittedEcmwfCanaryRunPersistence) -> StoredEcmwfCanaryRun {
    StoredEcmwfCanaryRun {
        adapter_version: input.adapter_version,
        content_hash: input.content_hash.clone(),
        first_received_at: RECEIVED_AT.to_owned(),
        id: "run-1".to_owned(),
        model_cycle: input.model_cycle,
        provider_response_sha256: input.provider_response_sha256.clone(),
        recent_error_state: input.recent_error_state.clone(),
        recent_error_state_sha256: input.recent_error_state_sha256.clone(),
        run_initialized_at: input.run_initialized_at.clone(),
        state_reason: input.state_reason.clone(),
        state_status: input.state_status,
        upstream_model: input.upstream_model,
    }
}

/// Build one fake port in a named persistence disposition.
fn fixture_port(
    mode: FixtureMode,
    stored: &StoredEcmwfCanaryRun,
    existing_hours: &[StoredEcmwfCanaryHourIdentity],
) -> FixturePort {
    FixturePort {
        existing_hours: existing_hours.to_vec(),
        hour_queries: Vec::new(),
        mode,
        run_query: None,
        stored: stored.clone(),
        trace: Vec::new(),
    }
}

/// Collapse irrelevant SQL whitespace while retaining every token.
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Render one exact query bind vector like the Node fake client.
fn render_binds<const N: usize>(binds: &[EcmwfCanaryPersistenceBind; N]) -> String {
    binds
        .iter()
        .map(|bind| match bind {
            EcmwfCanaryPersistenceBind::Count(value) => value.to_string(),
            EcmwfCanaryPersistenceBind::Digest(value) => value.as_str().to_owned(),
            EcmwfCanaryPersistenceBind::Json(value) => value.as_str().to_owned(),
            EcmwfCanaryPersistenceBind::NullableNumber(value) => {
                value.map_or_else(|| "".to_owned(), |value| value.to_string())
            }
            EcmwfCanaryPersistenceBind::Number(value) => value.to_string(),
            EcmwfCanaryPersistenceBind::Text(value)
            | EcmwfCanaryPersistenceBind::UtcInstant(value) => value.clone(),
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Join one command trace in source order.
fn trace(values: &[String]) -> String {
    values.join(",")
}

/// Return one generated state-status wire value.
fn status_wire(status: EcmwfTemperatureCanaryRunStateStatusEnum) -> &'static str {
    match status {
        EcmwfTemperatureCanaryRunStateStatusEnum::Supported => "supported",
        EcmwfTemperatureCanaryRunStateStatusEnum::Cold => "cold",
        EcmwfTemperatureCanaryRunStateStatusEnum::Insufficient => "insufficient",
        EcmwfTemperatureCanaryRunStateStatusEnum::Invalid => "invalid",
    }
}
