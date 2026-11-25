use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
    EcmwfTemperatureCanaryRunAdapterVersionLiteral, EcmwfTemperatureCanaryRunModelCycleEnum,
    EcmwfTemperatureCanaryRunStateStatusEnum, EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    JsDateOrString, Nullable, PersistedEcmwfTemperatureRevision, SourceKind, WeatherRecordRow,
};
use weather_database::{
    AdmittedEcmwfCanaryBestMatchRow, AdmittedEcmwfCanaryRevisionWindow,
    AdmittedEcmwfCanaryRunPersistence, AdmittedRevisionUtcInstant, ECMWF_CANARY_BEST_MATCH_SQL,
    ECMWF_CANARY_EXISTING_HOURS_SQL, ECMWF_CANARY_EXISTING_RUN_SQL, ECMWF_CANARY_INSERT_HOUR_SQL,
    ECMWF_CANARY_INSERT_RUN_SQL, ECMWF_CANARY_LOGICAL_RECEIPT_SQL, ECMWF_CANARY_SITE_SQL,
    ECMWF_CANARY_UPDATE_RECEIPT_SQL, EcmwfCanaryBestMatchQuery, EcmwfCanaryExistingHoursQuery,
    EcmwfCanaryExistingRunQuery, EcmwfCanaryHourInsertQuery, EcmwfCanaryLogicalReceiptQuery,
    EcmwfCanaryPersistenceBind, EcmwfCanaryPersistencePort, EcmwfCanaryReceiptUpdateQuery,
    EcmwfCanaryRevisionArchivePort, EcmwfCanaryRevisionSavepointCommand, EcmwfCanaryRunInsertQuery,
    EcmwfCanarySiteQuery, EcmwfCanarySiteRow, PreparedEcmwfCanaryHour,
    RELEASE_ECMWF_CANARY_REVISION_SQL, ROLLBACK_ECMWF_CANARY_REVISION_SQL,
    SAVEPOINT_ECMWF_CANARY_REVISION_SQL, SerializedJson, Sha256Digest,
    StoredEcmwfCanaryHourIdentity, StoredEcmwfCanaryRun,
    persist_ecmwf_temperature_canary_run_with_revision_archive,
};

const RUN_AT: &str = "2026-09-07T18:00:00.000Z";
const RUN_EPOCH: i64 = 1_788_804_000_000;
const RECEIVED_AT: &str = "2026-09-08T00:03:00.000Z";
const RECEIVED_EPOCH: i64 = 1_788_825_780_000;
const LOGICAL_RECEIVED_AT: &str = "2026-09-08T00:04:00.000Z";
const WINDOW_FROM: &str = "2026-09-08T01:00:00.000Z";
const WINDOW_FROM_EPOCH: i64 = 1_788_829_200_000;
const WINDOW_TO: &str = "2026-09-08T13:00:00.000Z";
const WINDOW_TO_EPOCH: i64 = 1_788_872_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixtureMode {
    Positive,
    StageFailure,
    Retry,
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

#[derive(Debug, Clone, PartialEq)]
struct FixturePort {
    best_match_query: Option<EcmwfCanaryBestMatchQuery>,
    best_match_rows: Vec<AdmittedEcmwfCanaryBestMatchRow>,
    existing_hours: Vec<StoredEcmwfCanaryHourIdentity>,
    mode: FixtureMode,
    publication_count: usize,
    staged_revision: Option<PersistedEcmwfTemperatureRevision>,
    stored: StoredEcmwfCanaryRun,
    trace: Vec<String>,
}

impl EcmwfCanaryPersistencePort for FixturePort {
    type Error = FixtureError;

    /// Record one transaction start.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("begin".to_owned());
        Ok(())
    }

    /// Return the exact active site identity.
    fn select_site(
        &mut self,
        query: &EcmwfCanarySiteQuery,
    ) -> Result<Option<EcmwfCanarySiteRow>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_SITE_SQL);
        self.trace.push("site".to_owned());
        Ok(Some(EcmwfCanarySiteRow {
            id: "site-1".to_owned(),
            latitude: 47.95,
            longitude: -122.43,
        }))
    }

    /// Claim only the two new-run fixture dispositions.
    fn insert_run(
        &mut self,
        query: &EcmwfCanaryRunInsertQuery,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_INSERT_RUN_SQL);
        self.trace.push("insert-run".to_owned());
        Ok((self.mode != FixtureMode::Retry).then(|| self.stored.clone()))
    }

    /// Record each exact new lead.
    fn insert_hour(&mut self, query: &EcmwfCanaryHourInsertQuery) -> Result<(), Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_INSERT_HOUR_SQL);
        let EcmwfCanaryPersistenceBind::Count(lead) = query.binds[2] else {
            panic!("hour lead bind")
        };
        self.trace.push(format!("hour-{lead}"));
        Ok(())
    }

    /// Return the locked identical run for the retry fixture.
    fn select_existing_run(
        &mut self,
        query: &EcmwfCanaryExistingRunQuery,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_EXISTING_RUN_SQL);
        self.trace.push("existing-run".to_owned());
        Ok(Some(self.stored.clone()))
    }

    /// Return exact stored hour identities in lead order.
    fn select_existing_hours(
        &mut self,
        query: &EcmwfCanaryExistingHoursQuery,
    ) -> Result<Vec<StoredEcmwfCanaryHourIdentity>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_EXISTING_HOURS_SQL);
        self.trace.push("existing-hours".to_owned());
        Ok(self.existing_hours.clone())
    }

    /// Record the identical retry receipt update.
    fn update_last_received(
        &mut self,
        query: &EcmwfCanaryReceiptUpdateQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_UPDATE_RECEIPT_SQL);
        self.trace.push("update-receipt".to_owned());
        Ok(())
    }

    /// Commit the source transaction.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("commit".to_owned());
        Ok(())
    }

    /// Record an outer rollback if the shared engine needs one.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("rollback".to_owned());
        Ok(())
    }

    /// Record retained-client release after publication handling.
    fn release_client(&mut self) {
        self.trace.push("release".to_owned());
    }
}

impl EcmwfCanaryRevisionArchivePort for FixturePort {
    type Publication = &'static str;

    /// Return the admitted database logical clock.
    fn select_logical_receipt(
        &mut self,
        query: &EcmwfCanaryLogicalReceiptQuery,
    ) -> Result<Option<AdmittedRevisionUtcInstant>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_LOGICAL_RECEIPT_SQL);
        self.trace.push("clock".to_owned());
        Ok(Some(AdmittedRevisionUtcInstant::admitted(
            LOGICAL_RECEIVED_AT,
        )))
    }

    /// Record exact savepoint and recovery commands.
    fn execute_revision_savepoint(
        &mut self,
        command: EcmwfCanaryRevisionSavepointCommand,
    ) -> Result<(), Self::Error> {
        let (expected, label) = match command {
            EcmwfCanaryRevisionSavepointCommand::Savepoint => {
                (SAVEPOINT_ECMWF_CANARY_REVISION_SQL, "savepoint")
            }
            EcmwfCanaryRevisionSavepointCommand::RollbackTo => {
                (ROLLBACK_ECMWF_CANARY_REVISION_SQL, "rollback-savepoint")
            }
            EcmwfCanaryRevisionSavepointCommand::Release => {
                (RELEASE_ECMWF_CANARY_REVISION_SQL, "release-savepoint")
            }
        };
        assert_eq!(command.sql(), expected);
        self.trace.push(label.to_owned());
        Ok(())
    }

    /// Capture and answer the exact Best Match lookup.
    fn select_best_match_rows(
        &mut self,
        query: &EcmwfCanaryBestMatchQuery,
    ) -> Result<Vec<AdmittedEcmwfCanaryBestMatchRow>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_BEST_MATCH_SQL);
        self.best_match_query = Some(query.clone());
        self.trace.push("best-match".to_owned());
        Ok(self.best_match_rows.clone())
    }

    /// Capture the canonical revision or inject the ordinary archive failure.
    fn stage_ecmwf_temperature_revision(
        &mut self,
        revision: &PersistedEcmwfTemperatureRevision,
    ) -> Result<Option<Self::Publication>, Self::Error> {
        self.trace.push("stage".to_owned());
        self.staged_revision = Some(revision.clone());
        // reproduce one ordinary cold archive failure inside the savepoint
        if self.mode == FixtureMode::StageFailure {
            return Err(FixtureError("archive unavailable"));
        }
        Ok(Some("publication"))
    }

    /// Count publication only after the source commit.
    fn publish_ecmwf_temperature_revision(
        &mut self,
        _publication: Self::Publication,
    ) -> Result<(), Self::Error> {
        self.trace.push("publish".to_owned());
        self.publication_count += 1;
        Ok(())
    }
}

/// Match compiled Node optional archive, recovery, and retry behavior.
#[test]
fn matches_compiled_ecmwf_revision_archive_path() {
    let oracle = node_oracle();
    assert_eq!(
        normalize_sql(ECMWF_CANARY_BEST_MATCH_SQL),
        oracle["best-match-sql"]
    );
    assert_eq!(ECMWF_CANARY_LOGICAL_RECEIPT_SQL, oracle["clock-sql"]);
    assert_eq!(SAVEPOINT_ECMWF_CANARY_REVISION_SQL, oracle["savepoint-sql"]);
    assert_eq!(
        ROLLBACK_ECMWF_CANARY_REVISION_SQL,
        oracle["rollback-savepoint-sql"]
    );
    assert_eq!(
        RELEASE_ECMWF_CANARY_REVISION_SQL,
        oracle["release-savepoint-sql"]
    );

    let input = admitted_input(&oracle);
    let window = AdmittedEcmwfCanaryRevisionWindow::admitted(
        WINDOW_FROM,
        WINDOW_FROM_EPOCH,
        WINDOW_TO,
        WINDOW_TO_EPOCH,
    );
    let existing_hours = input
        .hours
        .iter()
        .map(|hour| StoredEcmwfCanaryHourIdentity {
            content_hash: hour.content_hash.clone(),
        })
        .collect::<Vec<_>>();

    let mut positive = fixture_port(FixtureMode::Positive, &input, &existing_hours);
    let result =
        persist_ecmwf_temperature_canary_run_with_revision_archive(&input, &window, &mut positive)
            .expect("optional archive path must preserve the run");
    assert_eq!(trace(&positive.trace), oracle["positive-trace"]);
    assert_eq!(result.id, "run-positive");
    assert_eq!(positive.publication_count, 1);
    assert_eq!(
        render_binds(&positive.best_match_query.expect("best match query").binds),
        oracle["best-match-binds"]
    );
    assert_eq!(
        render_revision(positive.staged_revision.as_ref().expect("revision")),
        oracle["revision"]
    );

    let mut failure = fixture_port(FixtureMode::StageFailure, &input, &existing_hours);
    failure.stored.id = "run-failure".to_owned();
    let failed_archive =
        persist_ecmwf_temperature_canary_run_with_revision_archive(&input, &window, &mut failure)
            .expect("archive stage failure must preserve serving persistence");
    assert_eq!(trace(&failure.trace), oracle["failure-trace"]);
    assert_eq!(failed_archive.id, "run-failure");
    assert_eq!(failure.publication_count, 0);

    let mut ineligible = fixture_port(FixtureMode::Positive, &input, &existing_hours);
    ineligible.best_match_rows[5].content_hash = Sha256Digest::admitted("d".repeat(64));
    persist_ecmwf_temperature_canary_run_with_revision_archive(&input, &window, &mut ineligible)
        .expect("ineligible Best Match geometry must stage an empty matrix");
    assert_eq!(trace(&ineligible.trace), oracle["ineligible-trace"]);
    assert!(
        ineligible
            .staged_revision
            .as_ref()
            .expect("ineligible revision")
            .bestMatchRows
            .is_empty()
    );

    let mut retry = fixture_port(FixtureMode::Retry, &input, &existing_hours);
    let retried =
        persist_ecmwf_temperature_canary_run_with_revision_archive(&input, &window, &mut retry)
            .expect("identical retry must skip archival");
    assert_eq!(trace(&retry.trace), oracle["retry-trace"]);
    assert_eq!(retried.id, "run-positive");
    assert!(retry.best_match_query.is_none());
    assert!(retry.staged_revision.is_none());
}

/// Invoke the actual compiled repository owner with fake ports.
fn node_oracle() -> BTreeMap<String, String> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ecmwf-canary-revision-node-oracle.mjs");
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

/// Build one fully admitted cold run from source-produced hour hashes.
fn admitted_input(oracle: &BTreeMap<String, String>) -> AdmittedEcmwfCanaryRunPersistence {
    let valid_times = [
        "2026-09-07T19:00:00.000Z",
        "2026-09-07T20:00:00.000Z",
        "2026-09-07T21:00:00.000Z",
        "2026-09-07T22:00:00.000Z",
        "2026-09-07T23:00:00.000Z",
        "2026-09-08T00:00:00.000Z",
        "2026-09-08T01:00:00.000Z",
        "2026-09-08T02:00:00.000Z",
        "2026-09-08T03:00:00.000Z",
        "2026-09-08T04:00:00.000Z",
        "2026-09-08T05:00:00.000Z",
        "2026-09-08T06:00:00.000Z",
        "2026-09-08T07:00:00.000Z",
        "2026-09-08T08:00:00.000Z",
        "2026-09-08T09:00:00.000Z",
        "2026-09-08T10:00:00.000Z",
        "2026-09-08T11:00:00.000Z",
        "2026-09-08T12:00:00.000Z",
    ];
    let hashes = oracle["hour-hashes"].split(',').collect::<Vec<_>>();
    let hours = valid_times
        .iter()
        .zip(hashes)
        .enumerate()
        .map(|(index, (valid_at, hash))| PreparedEcmwfCanaryHour {
            content_hash: Sha256Digest::admitted(hash),
            model_lead_hours: (index + 1) as u64,
            raw_relative_humidity_percent: Nullable::Value(70.0 + index as f64),
            raw_temperature_c: 10.0 + index as f64 / 10.0,
            raw_wind_speed_mps: Nullable::Value(2.0 + index as f64 / 10.0),
            valid_at: (*valid_at).to_owned(),
            valid_at_epoch_milliseconds: RUN_EPOCH + (index as i64 + 1) * 3_600_000,
        })
        .collect();
    let content_hash = oracle["revision"]
        .split('|')
        .nth(7)
        .expect("revision content hash");
    AdmittedEcmwfCanaryRunPersistence {
        adapter_version: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
        content_hash: Sha256Digest::admitted(content_hash),
        hours,
        model_cycle: EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1,
        provider_response_sha256: Sha256Digest::admitted("b".repeat(64)),
        received_at: RECEIVED_AT.to_owned(),
        received_at_epoch_milliseconds: RECEIVED_EPOCH,
        recent_error_state: cold_state(),
        recent_error_state_json: SerializedJson::admitted("{}"),
        recent_error_state_maximum_run_epoch_milliseconds: None,
        recent_error_state_maximum_valid_epoch_milliseconds: None,
        recent_error_state_sha256: Sha256Digest::admitted("e".repeat(64)),
        recent_error_state_window_end_epoch_milliseconds: RUN_EPOCH - 7 * 3_600_000,
        recent_error_state_window_end_valid_at: "2026-09-07T11:00:00.000Z".to_owned(),
        run_initialized_at: RUN_AT.to_owned(),
        run_initialized_at_epoch_milliseconds: RUN_EPOCH,
        site_slug: "ballydidean".to_owned(),
        state_reason: "no_causal_forecast_observation_pairs".to_owned(),
        state_status: EcmwfTemperatureCanaryRunStateStatusEnum::Cold,
        upstream_model: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    }
}

/// Build the exact cold causal state accepted by the source.
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
        windowEndValidAt: "2026-09-07T11:00:00.000Z".to_owned(),
    }
}

/// Build one fixture port for a named archive disposition.
fn fixture_port(
    mode: FixtureMode,
    input: &AdmittedEcmwfCanaryRunPersistence,
    existing_hours: &[StoredEcmwfCanaryHourIdentity],
) -> FixturePort {
    FixturePort {
        best_match_query: None,
        best_match_rows: best_match_rows(),
        existing_hours: existing_hours.to_vec(),
        mode,
        publication_count: 0,
        staged_revision: None,
        stored: StoredEcmwfCanaryRun {
            adapter_version: input.adapter_version,
            content_hash: input.content_hash.clone(),
            first_received_at: RECEIVED_AT.to_owned(),
            id: "run-positive".to_owned(),
            model_cycle: input.model_cycle,
            provider_response_sha256: input.provider_response_sha256.clone(),
            recent_error_state: input.recent_error_state.clone(),
            recent_error_state_sha256: input.recent_error_state_sha256.clone(),
            run_initialized_at: input.run_initialized_at.clone(),
            state_reason: input.state_reason.clone(),
            state_status: input.state_status,
            upstream_model: input.upstream_model,
        },
        trace: Vec::new(),
    }
}

/// Build the complete admitted Best Match product returned by the fake adapter.
fn best_match_rows() -> Vec<AdmittedEcmwfCanaryBestMatchRow> {
    (0..12)
        .map(|index| {
            let valid_at_epoch = WINDOW_FROM_EPOCH + index * 3_600_000;
            let content_hash = Sha256Digest::admitted("c".repeat(64));
            AdmittedEcmwfCanaryBestMatchRow {
                content_hash: content_hash.clone(),
                product_run_at_epoch_milliseconds: Some(RUN_EPOCH),
                row: weather_record(index + 1, valid_at_epoch, content_hash.as_str()),
                valid_at_epoch_milliseconds: valid_at_epoch,
            }
        })
        .collect()
}

/// Build one canonical generated database row with irrelevant metrics null.
fn weather_record(index: i64, valid_at_epoch: i64, content_hash: &str) -> WeatherRecordRow {
    WeatherRecordRow {
        adapterVersion: Nullable::Null,
        apparentTemperatureC: Nullable::Null,
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Null,
        contentHash: content_hash.to_owned(),
        contractEpoch: Nullable::Null,
        deviceModel: Nullable::Null,
        deviceSerial: Nullable::Null,
        deviceVendor: Nullable::Null,
        firstReceivedAt: JsDateOrString::DateEpochMilliseconds(RECEIVED_EPOCH),
        id: format!("best-{index}"),
        lastReceivedAt: JsDateOrString::DateEpochMilliseconds(RECEIVED_EPOCH),
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Null,
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Null,
        productRunAt: Nullable::Value(JsDateOrString::DateEpochMilliseconds(RUN_EPOCH)),
        providerKey: "open-meteo".to_owned(),
        providerMetadata: Nullable::Null,
        qualityMetadata: Nullable::Null,
        relativeHumidityPercent: Nullable::Null,
        revisionCount: 0,
        siteSlug: "ballydidean".to_owned(),
        sourceId: "best-source-1".to_owned(),
        sourceConfigFingerprint: Nullable::Null,
        sourceKey: "open-meteo-forecast-v4".to_owned(),
        sourceKind: SourceKind::Forecast,
        stationSlug: "forecast".to_owned(),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Null,
        upstreamModel: Nullable::Value("best_match".to_owned()),
        upstreamTimezone: "UTC".to_owned(),
        uvIndex: Nullable::Null,
        validAt: JsDateOrString::DateEpochMilliseconds(valid_at_epoch),
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: Nullable::Null,
        windSpeedMps: Nullable::Null,
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

/// Render the archive revision fields asserted by the compiled owner.
fn render_revision(revision: &PersistedEcmwfTemperatureRevision) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        revision.siteId,
        revision.siteLatitude,
        revision.siteLongitude,
        revision.logicalReceivedAt,
        revision.bestMatchRows.len(),
        revision.hours.len(),
        revision.runInitializedAt,
        revision.contentHash
    )
}

/// Render exact bind order like the Node fake queryable.
fn render_binds<const N: usize>(binds: &[EcmwfCanaryPersistenceBind; N]) -> String {
    binds
        .iter()
        .map(|bind| match bind {
            EcmwfCanaryPersistenceBind::Count(value) => value.to_string(),
            EcmwfCanaryPersistenceBind::Digest(value) => value.as_str().to_owned(),
            EcmwfCanaryPersistenceBind::Json(value) => value.as_str().to_owned(),
            EcmwfCanaryPersistenceBind::NullableNumber(value) => {
                value.map_or_else(String::new, |value| value.to_string())
            }
            EcmwfCanaryPersistenceBind::Number(value) => value.to_string(),
            EcmwfCanaryPersistenceBind::Text(value)
            | EcmwfCanaryPersistenceBind::UtcInstant(value) => value.clone(),
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Collapse irrelevant SQL whitespace while retaining every token.
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Join one transaction trace in source order.
fn trace(values: &[String]) -> String {
    values.join(",")
}
