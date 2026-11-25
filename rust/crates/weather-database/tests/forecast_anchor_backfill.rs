use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    CanonicalWeatherMetrics, FieldPresence, FixedForecastLeadHours, IngestionError,
    IngestionErrorClassification, NormalizedForecastAnchorRecord,
    NormalizedForecastAnchorRecordContractVersionLiteral,
    NormalizedForecastAnchorRecordDatasetLiteral, NormalizedForecastAnchorRecordSourceKindLiteral,
    NormalizedForecastAnchorRecordUpstreamModelLiteral, Nullable, WeatherRecordMetadata,
};
use weather_database::{
    ASSERT_FORECAST_ANCHOR_BACKFILL_RUN_SQL, AdmittedForecastAnchorBackfillIdentity,
    AdmittedIngestionError, CompleteForecastAnchorBackfill,
    FINALIZE_FAILED_FORECAST_ANCHOR_RUN_SQL, FINALIZE_SUCCESSFUL_FORECAST_ANCHOR_RUN_SQL,
    FailForecastAnchorBackfill, ForecastAnchorAbandonExpiredQuery,
    ForecastAnchorBackfillCommandPort, ForecastAnchorBackfillExecutionError,
    ForecastAnchorBackfillGuard, ForecastAnchorFailureBind, ForecastAnchorFailureQuery,
    ForecastAnchorFindRunningQuery, ForecastAnchorIdentityBind, ForecastAnchorOutcome,
    ForecastAnchorOutcomeBind, ForecastAnchorOutcomeQuery, ForecastAnchorRecordBind,
    ForecastAnchorRecordStorageEvidence, ForecastAnchorRecordUpsertQuery,
    ForecastAnchorRunIdentityQuery, ForecastAnchorRunningLookup, ForecastAnchorSuccessBind,
    ForecastAnchorSuccessQuery, PreparedForecastAnchorRecord, RetainedSourceSession,
    SerializedJson, Sha256Digest, StartedForecastAnchorBackfillRun,
    UPSERT_FORECAST_ANCHOR_OUTCOME_SQL, complete_forecast_anchor_backfill,
    fail_forecast_anchor_backfill,
};

const FINGERPRINT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CONTENT_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CHECKSUM: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const INTERVAL_START: &str = "2026-09-01T00:00:00.000Z";
const INTERVAL_END: &str = "2026-09-03T00:00:00.000Z";

#[derive(Clone, Debug, PartialEq, Eq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// Render one injected command failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FixturePort {
    anchor_row_count: usize,
    source_id: String,
    trace: Vec<String>,
}

impl RetainedSourceSession for FixturePort {
    /// Return the immutable retained source identity.
    fn source_id(&self) -> &str {
        &self.source_id
    }
}

impl ForecastAnchorBackfillCommandPort for FixturePort {
    type Error = FixtureError;

    /// Reject the unused recovery boundary in this transaction fixture.
    fn abandon_expired_runs(
        &mut self,
        _query: &ForecastAnchorAbandonExpiredQuery,
    ) -> Result<Vec<String>, Self::Error> {
        unreachable!("completion fixture does not recover rows")
    }

    /// Reject the unused start lookup in this transaction fixture.
    fn find_running_run(
        &mut self,
        _query: &ForecastAnchorFindRunningQuery,
    ) -> Result<ForecastAnchorRunningLookup, Self::Error> {
        unreachable!("completion fixture does not start rows")
    }

    /// Reject the unused start insert in this transaction fixture.
    fn insert_running_run(
        &mut self,
        _query: &weather_database::ForecastAnchorRunStartQuery,
    ) -> Result<Option<StartedForecastAnchorBackfillRun>, Self::Error> {
        unreachable!("completion fixture does not start rows")
    }

    /// Record transaction start.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("begin".to_owned());
        Ok(())
    }

    /// Verify the exact seven-value committed identity guard.
    fn assert_backfill_run(
        &mut self,
        query: &ForecastAnchorRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.sql, ASSERT_FORECAST_ANCHOR_BACKFILL_RUN_SQL);
        assert_eq!(query.binds.len(), 7);
        assert_eq!(
            query.binds[0],
            ForecastAnchorIdentityBind::Text("run-1".to_owned())
        );
        assert_eq!(
            query.binds[4],
            ForecastAnchorIdentityBind::Fingerprint(Sha256Digest::admitted(FINGERPRINT))
        );
        self.trace.push("assert:7".to_owned());
        Ok(1)
    }

    /// Verify the exact 24-value normalized anchor bind group.
    fn upsert_forecast_anchor_records(
        &mut self,
        query: &ForecastAnchorRecordUpsertQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.record_count, 1);
        assert_eq!(query.binds.len(), 24);
        assert!(query.sql.contains("INSERT INTO forecast_anchor_records"));
        assert!(query.sql.contains("$10, $10, $11, $11"));
        assert_eq!(query.binds[4], ForecastAnchorRecordBind::LeadHours(24));
        assert_eq!(
            query.binds[12],
            ForecastAnchorRecordBind::NullableJson(Some(SerializedJson::admitted(
                r#"{"accepted":true}"#,
            )))
        );
        assert_eq!(
            query.binds[13],
            ForecastAnchorRecordBind::NullableJson(Some(SerializedJson::admitted(
                r#"{"dataset":"previous_runs"}"#,
            )))
        );
        assert_eq!(
            query.binds[23],
            ForecastAnchorRecordBind::ContentHash(Sha256Digest::admitted(CONTENT_HASH))
        );
        self.trace.push("records:24".to_owned());
        Ok(self.anchor_row_count)
    }

    /// Verify the exact eleven-value durable chunk outcome.
    fn upsert_backfill_outcome(
        &mut self,
        query: &ForecastAnchorOutcomeQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.sql, UPSERT_FORECAST_ANCHOR_OUTCOME_SQL);
        assert_eq!(query.binds.len(), 11);
        assert_eq!(
            query.binds[9],
            ForecastAnchorOutcomeBind::Outcome(ForecastAnchorOutcome::Succeeded)
        );
        self.trace.push("outcome:11".to_owned());
        Ok(())
    }

    /// Verify exact successful run evidence.
    fn finalize_successful_run(
        &mut self,
        query: &ForecastAnchorSuccessQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.sql, FINALIZE_SUCCESSFUL_FORECAST_ANCHOR_RUN_SQL);
        assert_eq!(query.binds.len(), 7);
        assert_eq!(query.binds[2], ForecastAnchorSuccessBind::Count(2));
        assert_eq!(query.binds[3], ForecastAnchorSuccessBind::Count(1));
        assert_eq!(
            query.binds[5],
            ForecastAnchorSuccessBind::NullableChecksum(Some(Sha256Digest::admitted(CHECKSUM)))
        );
        self.trace.push("finalize:7".to_owned());
        Ok(1)
    }

    /// Reject the unused failed finalization in this fixture.
    fn finalize_failed_run(
        &mut self,
        _query: &ForecastAnchorFailureQuery,
    ) -> Result<usize, Self::Error> {
        unreachable!("success fixture does not finalize failure")
    }

    /// Record transaction commit.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("commit".to_owned());
        Ok(())
    }

    /// Record transaction rollback.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("rollback".to_owned());
        Ok(())
    }

    /// Consume the unused direct fixture session.
    fn release(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Construct the exact durable chunk identity.
fn identity() -> AdmittedForecastAnchorBackfillIdentity {
    AdmittedForecastAnchorBackfillIdentity {
        adapter_version: "open-meteo-previous-runs/v1".to_owned(),
        chunk_plan_version: "open-meteo-previous-runs/v1".to_owned(),
        interval_end_exclusive: INTERVAL_END.to_owned(),
        interval_start: INTERVAL_START.to_owned(),
        requested_from_date: "2026-09-01".to_owned(),
        requested_to_date: "2026-09-02".to_owned(),
        source_config_fingerprint: Sha256Digest::admitted(FINGERPRINT),
        source_id: "source-1".to_owned(),
    }
}

/// Construct one normalized generated anchor with admitted storage evidence.
fn prepared_record() -> PreparedForecastAnchorRecord {
    let null = Nullable::Null;
    PreparedForecastAnchorRecord {
        record: NormalizedForecastAnchorRecord {
            adapterVersion: "open-meteo-previous-runs/v1".to_owned(),
            contractEpoch: "open-meteo-previous-runs-best-match/2026-09".to_owned(),
            contractVersion: NormalizedForecastAnchorRecordContractVersionLiteral,
            dataset: NormalizedForecastAnchorRecordDatasetLiteral,
            leadHours: FixedForecastLeadHours::Value24,
            metadata: WeatherRecordMetadata {
                device: Nullable::Null,
                model: Nullable::Value("best_match".to_owned()),
                provider: Nullable::Null,
                quality: Nullable::Null,
                upstreamTimezone: "UTC".to_owned(),
            },
            metrics: CanonicalWeatherMetrics {
                apparentTemperatureC: Nullable::Value(9.0),
                blackGlobeTemperatureC: null.clone(),
                cloudCoverPercent: Nullable::Value(50.0),
                pm25MicrogramsPerCubicMeter: null.clone(),
                precipitationMm: Nullable::Value(0.0),
                precipitationRateMmPerHour: null.clone(),
                pressureHpa: Nullable::Value(1012.0),
                relativeHumidityPercent: Nullable::Value(75.0),
                soilElectricalConductivityMicrosiemensPerCm: null.clone(),
                soilMoisturePercent: null.clone(),
                solarRadiationWm2: null.clone(),
                temperatureC: Nullable::Value(10.0),
                uvIndex: null.clone(),
                waterLevelM: null.clone(),
                windDirectionDegrees: Nullable::Value(0.0),
                windGustMps: Nullable::Value(4.0),
                windSpeedMps: Nullable::Value(2.0),
                wetBulbGlobeTemperatureC: null,
            },
            receivedAt: "2026-09-01T00:10:00.000Z".to_owned(),
            sourceConfigFingerprint: FINGERPRINT.to_owned(),
            sourceId: "source-1".to_owned(),
            sourceKind: NormalizedForecastAnchorRecordSourceKindLiteral,
            upstreamModel: NormalizedForecastAnchorRecordUpstreamModelLiteral,
            validAt: "2026-09-02T00:00:00.000Z".to_owned(),
        },
        storage: ForecastAnchorRecordStorageEvidence {
            content_hash: Sha256Digest::admitted(CONTENT_HASH),
            provider_metadata_json: Some(SerializedJson::admitted(
                r#"{"dataset":"previous_runs"}"#,
            )),
            quality_metadata_json: Some(SerializedJson::admitted(r#"{"accepted":true}"#)),
        },
    }
}

/// Construct one exact successful completion input.
fn completion() -> CompleteForecastAnchorBackfill {
    CompleteForecastAnchorBackfill {
        attempts: 2,
        identity: identity(),
        records: vec![prepared_record()],
        response_metadata_json: Some(SerializedJson::admitted(r#"{"requested":168}"#)),
        run_id: "run-1".to_owned(),
        upstream_response_checksum: Some(Sha256Digest::admitted(CHECKSUM)),
    }
}

/// Render one transaction trace with the Node oracle label.
fn rendered_trace(label: &str, trace: &[String]) -> String {
    format!("{label}={}", trace.join("|"))
}

// verifies actual compiled repository ordering and immutable provenance behavior
#[test]
fn matches_compiled_node_completion_transaction() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/forecast-anchor-backfill-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run forecast-anchor Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let lines = stdout.lines().collect::<Vec<_>>();

    let mut success = FixturePort {
        anchor_row_count: 1,
        source_id: "source-1".to_owned(),
        trace: Vec::new(),
    };
    complete_forecast_anchor_backfill(&completion(), &mut success).expect("completion succeeds");
    assert_eq!(lines[0], rendered_trace("success", &success.trace));

    let mut mismatch = FixturePort {
        anchor_row_count: 0,
        source_id: "source-1".to_owned(),
        trace: Vec::new(),
    };
    let error = complete_forecast_anchor_backfill(&completion(), &mut mismatch)
        .expect_err("immutable provenance mismatch rolls back");
    assert_eq!(lines[1], rendered_trace("mismatch", &mismatch.trace));
    assert_eq!(lines[2], format!("mismatch-error={error}"));
    assert_eq!(
        error,
        ForecastAnchorBackfillExecutionError::GuardedWrite {
            guard: ForecastAnchorBackfillGuard::AnchorRecordUpsert,
            row_count: 0,
        }
    );

    let mut cross_source_input = completion();
    cross_source_input.records[0].record.sourceId = "source-2".to_owned();
    let mut cross_source = FixturePort {
        anchor_row_count: 1,
        source_id: "source-1".to_owned(),
        trace: Vec::new(),
    };
    let error = complete_forecast_anchor_backfill(&cross_source_input, &mut cross_source)
        .expect_err("cross-source record rolls back");
    assert_eq!(
        lines[3],
        rendered_trace("source-mismatch", &cross_source.trace)
    );
    assert_eq!(lines[4], format!("source-mismatch-error={error}"));
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FailurePort {
    source_id: String,
    trace: Vec<String>,
}

impl RetainedSourceSession for FailurePort {
    /// Return the immutable retained source identity.
    fn source_id(&self) -> &str {
        &self.source_id
    }
}

impl ForecastAnchorBackfillCommandPort for FailurePort {
    type Error = FixtureError;

    /// Reject the unused recovery command.
    fn abandon_expired_runs(
        &mut self,
        _query: &ForecastAnchorAbandonExpiredQuery,
    ) -> Result<Vec<String>, Self::Error> {
        unreachable!("failure fixture does not recover rows")
    }

    /// Reject the unused start lookup.
    fn find_running_run(
        &mut self,
        _query: &ForecastAnchorFindRunningQuery,
    ) -> Result<ForecastAnchorRunningLookup, Self::Error> {
        unreachable!("failure fixture does not start rows")
    }

    /// Reject the unused start insert.
    fn insert_running_run(
        &mut self,
        _query: &weather_database::ForecastAnchorRunStartQuery,
    ) -> Result<Option<StartedForecastAnchorBackfillRun>, Self::Error> {
        unreachable!("failure fixture does not start rows")
    }

    /// Record transaction start.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("begin".to_owned());
        Ok(())
    }

    /// Retain the exact failed run identity.
    fn assert_backfill_run(
        &mut self,
        _query: &ForecastAnchorRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        self.trace.push("assert".to_owned());
        Ok(1)
    }

    /// Reject records in a failed finalization.
    fn upsert_forecast_anchor_records(
        &mut self,
        _query: &ForecastAnchorRecordUpsertQuery,
    ) -> Result<usize, Self::Error> {
        unreachable!("failure fixture does not store records")
    }

    /// Verify the exact failed chunk outcome.
    fn upsert_backfill_outcome(
        &mut self,
        query: &ForecastAnchorOutcomeQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(
            query.binds[9],
            ForecastAnchorOutcomeBind::Outcome(ForecastAnchorOutcome::Failed)
        );
        assert_eq!(
            query.binds[10],
            ForecastAnchorOutcomeBind::NullableText(Some("provider_unavailable".to_owned()))
        );
        self.trace.push("outcome:failed".to_owned());
        Ok(())
    }

    /// Reject successful finalization in a failed transaction.
    fn finalize_successful_run(
        &mut self,
        _query: &ForecastAnchorSuccessQuery,
    ) -> Result<usize, Self::Error> {
        unreachable!("failure fixture does not finalize success")
    }

    /// Verify exact durable failure classification, code, and message.
    fn finalize_failed_run(
        &mut self,
        query: &ForecastAnchorFailureQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.sql, FINALIZE_FAILED_FORECAST_ANCHOR_RUN_SQL);
        assert_eq!(query.binds[2], ForecastAnchorFailureBind::Count(3));
        assert_eq!(
            query.binds[4],
            ForecastAnchorFailureBind::Classification(IngestionErrorClassification::Retryable)
        );
        assert_eq!(
            query.binds[5],
            ForecastAnchorFailureBind::Text("provider_unavailable".to_owned())
        );
        self.trace.push("finalize:failed".to_owned());
        Ok(1)
    }

    /// Record transaction commit.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("commit".to_owned());
        Ok(())
    }

    /// Record transaction rollback.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("rollback".to_owned());
        Ok(())
    }

    /// Consume the direct fixture session.
    fn release(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

// verifies typed failure fields remain atomic with the chunk outcome
#[test]
fn finalizes_typed_failure_atomically() {
    let mut port = FailurePort {
        source_id: "source-1".to_owned(),
        trace: Vec::new(),
    };
    let failure = FailForecastAnchorBackfill {
        attempts: 3,
        error: AdmittedIngestionError::admitted(IngestionError {
            classification: IngestionErrorClassification::Retryable,
            code: "provider_unavailable".to_owned(),
            message: "upstream unavailable".to_owned(),
            metadata: FieldPresence::Missing,
        }),
        identity: identity(),
        run_id: "run-1".to_owned(),
    };
    fail_forecast_anchor_backfill(&failure, &mut port).expect("failure finalizes");
    assert_eq!(
        port.trace,
        [
            "begin",
            "assert",
            "outcome:failed",
            "finalize:failed",
            "commit"
        ]
    );
}
