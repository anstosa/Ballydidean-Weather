use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    CanonicalWeatherMetrics, DeviceMetadata, FieldPresence, IngestionError,
    IngestionErrorClassification, NormalizedWeatherRecord, Nullable, SourceKind,
    WeatherRecordMetadata,
};
use weather_database::{
    ASSERT_TIDE_BACKFILL_RUN_SQL, AdmittedIngestionError, AdmittedTideBackfillIdentity,
    CompleteTideBackfill, FINALIZE_FAILED_TIDE_BACKFILL_RUN_SQL,
    FINALIZE_SUCCESSFUL_TIDE_BACKFILL_RUN_SQL, FailTideBackfill, PreparedWeatherRecord,
    RetainedSourceSession, SerializedJson, Sha256Digest, StartedTideBackfillRun,
    TideBackfillAbandonExpiredQuery, TideBackfillCommandPort, TideBackfillExecutionError,
    TideBackfillFailureBind, TideBackfillFindRunningQuery, TideBackfillIdentityBind,
    TideBackfillOutcome, TideBackfillOutcomeBind, TideBackfillOutcomeQuery,
    TideBackfillRunIdentityQuery, TideBackfillRunStartQuery, TideBackfillRunningLookup,
    TideBackfillSuccessBind, TideBackfillSuccessQuery, UPSERT_TIDE_BACKFILL_OUTCOME_SQL,
    WeatherRecordBind, WeatherRecordStorageEvidence, WeatherRecordUpsertQuery,
    complete_tide_backfill, fail_tide_backfill,
};

const FINGERPRINT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CONTENT_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CHECKSUM: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const INTERVAL_START: &str = "2026-09-01T00:00:00.000Z";
const INTERVAL_END: &str = "2026-09-02T00:00:00.000Z";

#[derive(Clone, Debug, Eq, PartialEq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// Render one injected database error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

struct FixturePort {
    failure: bool,
    source_id: String,
    trace: Vec<String>,
}

impl RetainedSourceSession for FixturePort {
    /// Return the immutable retained source identity.
    fn source_id(&self) -> &str {
        &self.source_id
    }
}

impl TideBackfillCommandPort for FixturePort {
    type Error = FixtureError;

    /// Reject unused recovery in the completion fixture.
    fn abandon_expired_runs(
        &mut self,
        _query: &TideBackfillAbandonExpiredQuery,
    ) -> Result<Vec<String>, Self::Error> {
        unreachable!("completion fixture does not recover rows")
    }

    /// Reject unused start lookup in the completion fixture.
    fn find_running_run(
        &mut self,
        _query: &TideBackfillFindRunningQuery,
    ) -> Result<TideBackfillRunningLookup, Self::Error> {
        unreachable!("completion fixture does not start rows")
    }

    /// Reject unused start insert in the completion fixture.
    fn insert_running_run(
        &mut self,
        _query: &TideBackfillRunStartQuery,
    ) -> Result<Option<StartedTideBackfillRun>, Self::Error> {
        unreachable!("completion fixture does not start rows")
    }

    /// Record transaction start.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("begin".to_owned());
        Ok(())
    }

    /// Verify the exact seven-bind backfill identity guard.
    fn assert_backfill_run(
        &mut self,
        query: &TideBackfillRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.sql, ASSERT_TIDE_BACKFILL_RUN_SQL);
        assert_eq!(query.binds.len(), 7);
        assert_eq!(
            query.binds[4],
            TideBackfillIdentityBind::Fingerprint(Sha256Digest::admitted(FINGERPRINT))
        );
        self.trace.push("assert:7".to_owned());
        Ok(1)
    }

    /// Verify the shared 32-bind normalized weather row.
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.record_count, 1);
        assert_eq!(query.binds.len(), 32);
        assert_eq!(
            query.binds[1],
            WeatherRecordBind::SourceKind(SourceKind::TideObservation)
        );
        assert_eq!(
            query.binds[30],
            WeatherRecordBind::NullableNumber(Some(1.234))
        );
        assert_eq!(
            query.binds[31],
            WeatherRecordBind::ContentHash(Sha256Digest::admitted(CONTENT_HASH))
        );
        self.trace.push("records:32".to_owned());
        Ok(())
    }

    /// Verify the exact successful chunk outcome.
    fn upsert_backfill_outcome(
        &mut self,
        query: &TideBackfillOutcomeQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.sql, UPSERT_TIDE_BACKFILL_OUTCOME_SQL);
        assert_eq!(query.binds.len(), 11);
        // select the transaction's exact durable outcome
        let expected = if self.failure {
            TideBackfillOutcome::Failed
        } else {
            TideBackfillOutcome::Succeeded
        };
        assert_eq!(query.binds[9], TideBackfillOutcomeBind::Outcome(expected));
        self.trace.push("outcome:11".to_owned());
        Ok(())
    }

    /// Verify exact success evidence and checksum.
    fn finalize_successful_run(
        &mut self,
        query: &TideBackfillSuccessQuery,
    ) -> Result<usize, Self::Error> {
        assert!(!self.failure);
        assert_eq!(query.sql, FINALIZE_SUCCESSFUL_TIDE_BACKFILL_RUN_SQL);
        assert_eq!(query.binds.len(), 7);
        assert_eq!(query.binds[2], TideBackfillSuccessBind::Count(2));
        assert_eq!(query.binds[3], TideBackfillSuccessBind::Count(1));
        assert_eq!(
            query.binds[5],
            TideBackfillSuccessBind::NullableChecksum(Some(Sha256Digest::admitted(CHECKSUM)))
        );
        self.trace.push("finalize:7".to_owned());
        Ok(1)
    }

    /// Reject unused failed finalization.
    fn finalize_failed_run(
        &mut self,
        query: &weather_database::TideBackfillFailureQuery,
    ) -> Result<usize, Self::Error> {
        assert!(self.failure);
        assert_eq!(query.sql, FINALIZE_FAILED_TIDE_BACKFILL_RUN_SQL);
        assert_eq!(query.binds.len(), 7);
        assert_eq!(query.binds[2], TideBackfillFailureBind::Count(3));
        self.trace.push("failed:7".to_owned());
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

/// Construct one exact durable tide identity.
fn identity() -> AdmittedTideBackfillIdentity {
    AdmittedTideBackfillIdentity {
        adapter_version: "noaa-water-level/v1".to_owned(),
        chunk_plan_version: "noaa-calendar-range/v1".to_owned(),
        interval_end_exclusive: INTERVAL_END.to_owned(),
        interval_start: INTERVAL_START.to_owned(),
        requested_from_date: "2026-09-01".to_owned(),
        requested_to_date: "2026-09-01".to_owned(),
        source_config_fingerprint: Sha256Digest::admitted(FINGERPRINT),
        source_id: "source-tide".to_owned(),
    }
}

/// Construct the normalized NOAA row used by the compiled Node oracle.
fn prepared_record() -> PreparedWeatherRecord {
    let null = Nullable::Null;
    PreparedWeatherRecord {
        record: NormalizedWeatherRecord {
            metadata: WeatherRecordMetadata {
                device: Nullable::Value(DeviceMetadata {
                    model: FieldPresence::Value("Water Level Station".to_owned()),
                    serial: FieldPresence::Value("9444900".to_owned()),
                    vendor: FieldPresence::Value("NOAA CO-OPS".to_owned()),
                }),
                model: Nullable::Null,
                provider: Nullable::Null,
                quality: Nullable::Null,
                upstreamTimezone: "America/Los_Angeles".to_owned(),
            },
            metrics: CanonicalWeatherMetrics {
                apparentTemperatureC: null.clone(),
                blackGlobeTemperatureC: null.clone(),
                cloudCoverPercent: null.clone(),
                pm25MicrogramsPerCubicMeter: null.clone(),
                precipitationMm: null.clone(),
                precipitationRateMmPerHour: null.clone(),
                pressureHpa: null.clone(),
                relativeHumidityPercent: null.clone(),
                soilElectricalConductivityMicrosiemensPerCm: null.clone(),
                soilMoisturePercent: null.clone(),
                solarRadiationWm2: null.clone(),
                temperatureC: null.clone(),
                uvIndex: null.clone(),
                waterLevelM: Nullable::Value(1.234),
                windDirectionDegrees: null.clone(),
                windGustMps: null.clone(),
                windSpeedMps: null.clone(),
                wetBulbGlobeTemperatureC: null,
            },
            productRunAt: Nullable::Null,
            receivedAt: "2026-09-02T00:01:00.000Z".to_owned(),
            sourceId: "source-tide".to_owned(),
            sourceKind: SourceKind::TideObservation,
            validAt: "2026-09-01T00:06:00.000Z".to_owned(),
        },
        storage: WeatherRecordStorageEvidence {
            content_hash: Sha256Digest::admitted(CONTENT_HASH),
            provider_metadata_json: Some(SerializedJson::admitted(r#"{"product":"water_level"}"#)),
            quality_metadata_json: Some(SerializedJson::admitted(r#"{"status":"v"}"#)),
        },
    }
}

/// Construct one successful completion input.
fn completion() -> CompleteTideBackfill {
    CompleteTideBackfill {
        attempts: 2,
        identity: identity(),
        records: vec![prepared_record()],
        response_metadata_json: Some(SerializedJson::admitted(r#"{"normalized_record_count":1}"#)),
        run_id: "run-1".to_owned(),
        upstream_response_checksum: Some(Sha256Digest::admitted(CHECKSUM)),
    }
}

/// Render one exact Node oracle trace line.
fn rendered_trace(label: &str, trace: &[String]) -> String {
    format!("{label}={}", trace.join("|"))
}

// verifies the actual compiled generic backfill transaction
#[test]
fn matches_compiled_node_tide_completion() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tide-backfill-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run tide backfill Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let lines = stdout.lines().collect::<Vec<_>>();

    let mut success = FixturePort {
        failure: false,
        source_id: "source-tide".to_owned(),
        trace: Vec::new(),
    };
    complete_tide_backfill(&completion(), &mut success).expect("tide completion succeeds");
    assert_eq!(lines[0], rendered_trace("success", &success.trace));

    let mut mismatch_input = completion();
    mismatch_input.records[0].record.sourceId = "source-other".to_owned();
    let mut mismatch = FixturePort {
        failure: false,
        source_id: "source-tide".to_owned(),
        trace: Vec::new(),
    };
    let error = complete_tide_backfill(&mismatch_input, &mut mismatch)
        .expect_err("cross-source row rolls back");
    assert_eq!(lines[1], rendered_trace("source-mismatch", &mismatch.trace));
    assert_eq!(lines[2], format!("source-mismatch-error={error}"));
    assert!(matches!(error, TideBackfillExecutionError::Input(_)));

    let mut failure_port = FixturePort {
        failure: true,
        source_id: "source-tide".to_owned(),
        trace: Vec::new(),
    };
    fail_tide_backfill(
        &FailTideBackfill {
            attempts: 3,
            error: AdmittedIngestionError::admitted(IngestionError {
                classification: IngestionErrorClassification::Retryable,
                code: "provider_unavailable".to_owned(),
                message: "NOAA unavailable".to_owned(),
                metadata: FieldPresence::Missing,
            }),
            identity: identity(),
            run_id: "run-1".to_owned(),
        },
        &mut failure_port,
    )
    .expect("failed tide chunk finalizes");
    assert_eq!(lines[3], rendered_trace("failure", &failure_port.trace));
}
