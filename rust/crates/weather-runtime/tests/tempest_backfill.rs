use std::cell::RefCell;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

use weather_contracts::{
    FieldPresence, IngestionError, IngestionErrorClassification, JsonValue,
    NormalizedWeatherRecord, Nullable, ScheduledCheckpointState, SourceKind,
};
use weather_database::{
    AdmittedIngestionError, RetainedSourceSession, SerializedJson, Sha256Digest,
    StartedTempestBackfillRun, TempestBackfillAbandonExpiredQuery, TempestBackfillCommandPort,
    TempestBackfillFailureBind, TempestBackfillFailureQuery, TempestBackfillFindRunningQuery,
    TempestBackfillOutcome, TempestBackfillOutcomeBind, TempestBackfillOutcomeQuery,
    TempestBackfillPoolPort, TempestBackfillRunIdentityQuery, TempestBackfillRunStartBind,
    TempestBackfillRunStartQuery, TempestBackfillRunningLookup, TempestBackfillSuccessQuery,
    TempestBackfillSuccessfulChunkQuery, WeatherRecordBind, WeatherRecordUpsertQuery,
};
use weather_runtime::{
    DatabaseTempestBackfillRepository, OpenMeteoRuntimeValues, RuntimePersistenceOperation,
    RuntimeTimeValue, TempestBackfillPersistenceOperation, TempestBackfillRuntimeValues,
    TempestRuntimeValues,
};
use weather_worker::{
    OpenMeteoCursor, ProviderBatch, TEMPEST_SOURCE_CONTRACT_VERSION, TaskFailure,
    TempestBackfillArguments, TempestBackfillDate, TempestBackfillDeadlines,
    TempestBackfillErrorClassification, TempestBackfillOperationError, TempestBackfillProvider,
    TempestBackfillProviderFailure, TempestBackfillSource, TempestBackfillTimeAdapter,
    TempestObservation, TempestObservationEnvelope, TempestObservationRequest,
    TempestProviderCursor, TempestResponseMetadata, TempestResponseType,
    TempestRuntimeConfiguration, TempestScheduledSource, TempestSourceKind,
    execute_tempest_bulk_backfill, normalize_tempest_observations,
};

const START_MS: i64 = 1_785_542_400_000;
const VALID_AT_MS: i64 = START_MS + 60_000;
const END_MS: i64 = 1_785_628_800_000;
const TODAY_MS: i64 = 1_794_009_600_000;
const RECEIVED_MS: i64 = TODAY_MS;
const PROVIDER_DEADLINE_MS: i64 = TODAY_MS + 90_000;
const RUN_DEADLINE_MS: i64 = TODAY_MS + 120_000;
const FINGERPRINT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CHECKSUM: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CONTENT_HASH: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
type Trace = Rc<RefCell<Vec<String>>>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// Render one injected database error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

struct FixtureDatabase {
    trace: Trace,
}

impl TempestBackfillPoolPort for FixtureDatabase {
    type Error = FixtureError;
    type Session = FixtureSession;

    /// Execute the exact successful-chunk resume query before locking.
    fn has_successful_chunk(
        &mut self,
        query: &TempestBackfillSuccessfulChunkQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.binds.len(), 6);
        self.trace.borrow_mut().push("resume:6".to_owned());
        Ok(0)
    }

    /// Acquire one retained Tempest source session.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, Self::Error> {
        assert_eq!(source_id, "source-tempest");
        self.trace.borrow_mut().push("acquire".to_owned());
        Ok(Some(FixtureSession {
            source_id: source_id.to_owned(),
            trace: Rc::clone(&self.trace),
        }))
    }
}

struct FixtureSession {
    source_id: String,
    trace: Trace,
}

impl RetainedSourceSession for FixtureSession {
    /// Return the exact retained source identity.
    fn source_id(&self) -> &str {
        &self.source_id
    }
}

impl TempestBackfillCommandPort for FixtureSession {
    type Error = FixtureError;

    /// Capture exact expired-row recovery.
    fn abandon_expired_runs(
        &mut self,
        query: &TempestBackfillAbandonExpiredQuery,
    ) -> Result<Vec<String>, Self::Error> {
        assert_eq!(query.binds.len(), 2);
        self.trace.borrow_mut().push("abandon:2".to_owned());
        Ok(Vec::new())
    }

    /// Capture the running-row lookup before insertion.
    fn find_running_run(
        &mut self,
        query: &TempestBackfillFindRunningQuery,
    ) -> Result<TempestBackfillRunningLookup, Self::Error> {
        assert_eq!(query.binds.len(), 1);
        self.trace.borrow_mut().push("find:1".to_owned());
        Ok(TempestBackfillRunningLookup {
            first_run_id: None,
            row_count: 0,
        })
    }

    /// Capture the exact running Tempest row and request evidence.
    fn insert_running_run(
        &mut self,
        query: &TempestBackfillRunStartQuery,
    ) -> Result<Option<StartedTempestBackfillRun>, Self::Error> {
        assert_eq!(query.binds.len(), 10);
        assert_eq!(
            query.binds[9],
            TempestBackfillRunStartBind::NullableJson(Some(SerializedJson::admitted(
                r#"{"device_id":470937,"endpoint":"observations/device","location_id":203055}"#,
            )))
        );
        self.trace.borrow_mut().push("start:10".to_owned());
        Ok(Some(StartedTempestBackfillRun {
            id: "run-tempest".to_owned(),
            started_at: "2026-11-07T00:00:00.000Z".to_owned(),
        }))
    }

    /// Capture one completion or failure transaction start.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("begin".to_owned());
        Ok(())
    }

    /// Capture the exact committed backfill identity.
    fn assert_backfill_run(
        &mut self,
        query: &TempestBackfillRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.binds.len(), 7);
        self.trace.borrow_mut().push("assert:7".to_owned());
        Ok(1)
    }

    /// Capture one actual normalized Tempest storage row.
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.record_count, 1);
        assert_eq!(query.binds.len(), 32);
        assert_eq!(
            query.binds[1],
            WeatherRecordBind::SourceKind(SourceKind::PhysicalSensor)
        );
        assert_eq!(
            query.binds[2],
            WeatherRecordBind::UtcInstant("2026-08-01T00:01:00.000Z".to_owned())
        );
        assert_eq!(
            query.binds[13],
            WeatherRecordBind::NullableNumber(Some(12.5))
        );
        assert_eq!(
            query.binds[31],
            WeatherRecordBind::ContentHash(Sha256Digest::admitted(CONTENT_HASH))
        );
        self.trace.borrow_mut().push("records:32".to_owned());
        Ok(())
    }

    /// Capture the exact successful or failed durable chunk outcome.
    fn upsert_backfill_outcome(
        &mut self,
        query: &TempestBackfillOutcomeQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.binds.len(), 11);
        // retain the exact outcome name in the execution trace
        let outcome = match query.binds[9] {
            TempestBackfillOutcomeBind::Outcome(TempestBackfillOutcome::Succeeded) => "succeeded",
            TempestBackfillOutcomeBind::Outcome(TempestBackfillOutcome::Failed) => "failed",
            _ => panic!("unexpected Tempest outcome bind"),
        };
        self.trace.borrow_mut().push(format!("outcome:{outcome}"));
        Ok(())
    }

    /// Capture successful response evidence.
    fn finalize_successful_run(
        &mut self,
        query: &TempestBackfillSuccessQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.binds.len(), 7);
        self.trace.borrow_mut().push("finalize:success".to_owned());
        Ok(1)
    }

    /// Capture exact classified provider failure fields.
    fn finalize_failed_run(
        &mut self,
        query: &TempestBackfillFailureQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.binds.len(), 7);
        assert_eq!(query.binds[2], TempestBackfillFailureBind::Count(3));
        assert_eq!(
            query.binds[4],
            TempestBackfillFailureBind::Classification(IngestionErrorClassification::Retryable,)
        );
        assert_eq!(
            query.binds[5],
            TempestBackfillFailureBind::Text("provider_unavailable".to_owned())
        );
        assert_eq!(
            query.binds[6],
            TempestBackfillFailureBind::Text("Tempest unavailable".to_owned())
        );
        self.trace.borrow_mut().push("finalize:failed".to_owned());
        Ok(1)
    }

    /// Record transaction commit.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("commit".to_owned());
        Ok(())
    }

    /// Record unexpected transaction rollback.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("rollback".to_owned());
        Ok(())
    }

    /// Release the retained source session last.
    fn release(self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("release".to_owned());
        Ok(())
    }
}

struct FixtureValues;

impl OpenMeteoRuntimeValues for FixtureValues {
    /// Format only the admitted Tempest fixture instants.
    fn utc_instant(
        &mut self,
        milliseconds: i64,
        _value: RuntimeTimeValue,
    ) -> Result<String, TaskFailure> {
        // map every admitted fixture instant explicitly
        let value = match milliseconds {
            START_MS => "2026-08-01T00:00:00.000Z",
            VALID_AT_MS => "2026-08-01T00:01:00.000Z",
            END_MS => "2026-08-02T00:00:00.000Z",
            TODAY_MS => "2026-11-07T00:00:00.000Z",
            RUN_DEADLINE_MS => "2026-11-07T00:02:00.000Z",
            _ => panic!("unexpected Tempest time {milliseconds}"),
        };
        Ok(value.to_owned())
    }

    /// Reject unused durable checkpoint decoding.
    fn epoch_milliseconds(
        &mut self,
        _instant: &str,
        _value: RuntimeTimeValue,
    ) -> Result<i64, TaskFailure> {
        unreachable!("backfill does not decode scheduled checkpoints")
    }

    /// Serialize admitted provider, quality, and response objects.
    fn serialize_json_object(
        &mut self,
        value: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure> {
        assert!(!value.is_empty());
        Ok(SerializedJson::admitted("admitted-tempest-json"))
    }

    /// Reject the scheduled request metadata path.
    fn request_metadata_json(&mut self, _endpoint: &str) -> Result<SerializedJson, TaskFailure> {
        unreachable!("backfill has distinct request metadata")
    }

    /// Reject the Open-Meteo cursor path.
    fn provider_cursor_json(
        &mut self,
        _cursor: Option<&OpenMeteoCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        unreachable!("Tempest backfill does not persist a scheduled cursor")
    }

    /// Reject the Open-Meteo checkpoint path.
    fn checkpoint_cursor(
        &mut self,
        _checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<OpenMeteoCursor>, TaskFailure> {
        unreachable!("Tempest backfill does not read a scheduled checkpoint")
    }

    /// Admit one actual normalized Tempest row hash.
    fn weather_record_content_hash(
        &mut self,
        record: &NormalizedWeatherRecord,
    ) -> Result<Sha256Digest, TaskFailure> {
        assert_eq!(record.sourceId, "source-tempest");
        assert_eq!(record.validAt, "2026-08-01T00:01:00.000Z");
        assert_eq!(record.metrics.temperatureC, Nullable::Value(12.5));
        Ok(Sha256Digest::admitted(CONTENT_HASH))
    }

    /// Admit source fingerprints and response checksums.
    fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure> {
        assert!(value == FINGERPRINT || value == CHECKSUM);
        Ok(Sha256Digest::admitted(value))
    }

    /// Reject the scheduled failure admission path.
    fn ingestion_error(
        &mut self,
        _failure: &TaskFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure> {
        unreachable!("Tempest backfill admits exact classified failures")
    }

    /// Preserve the existing scheduled persistence classifier contract.
    fn persistence_failure(
        &mut self,
        operation: RuntimePersistenceOperation,
        _message: &str,
    ) -> TaskFailure {
        TaskFailure {
            attempts: 0,
            code: format!("persistence_{operation:?}"),
        }
    }
}

impl TempestRuntimeValues for FixtureValues {
    /// Reject scheduled Tempest cursor persistence.
    fn tempest_provider_cursor_json(
        &mut self,
        _cursor: Option<&TempestProviderCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        unreachable!("Tempest backfill resume is chunk-identity based")
    }

    /// Reject scheduled Tempest checkpoint decoding.
    fn tempest_checkpoint_cursor(
        &mut self,
        _checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<TempestProviderCursor>, TaskFailure> {
        unreachable!("Tempest backfill resume is chunk-identity based")
    }
}

impl TempestBackfillRuntimeValues for FixtureValues {
    /// Serialize exact endpoint and station request evidence.
    fn tempest_backfill_request_metadata_json(
        &mut self,
        endpoint: &str,
        device_id: u64,
        location_id: u64,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        assert_eq!(endpoint, "observations/device");
        assert_eq!(device_id, 470937);
        assert_eq!(location_id, 203055);
        Ok(Some(SerializedJson::admitted(
            r#"{"device_id":470937,"endpoint":"observations/device","location_id":203055}"#,
        )))
    }

    /// Admit exact classified Tempest provider failure fields.
    fn tempest_backfill_ingestion_error(
        &mut self,
        failure: &TempestBackfillProviderFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure> {
        // preserve the worker's exact durable classification
        let classification = match failure.error.classification {
            TempestBackfillErrorClassification::Retryable => {
                IngestionErrorClassification::Retryable
            }
            TempestBackfillErrorClassification::RateLimited => {
                IngestionErrorClassification::RateLimited
            }
            TempestBackfillErrorClassification::Permanent => {
                IngestionErrorClassification::Permanent
            }
            TempestBackfillErrorClassification::InvalidPayload => {
                IngestionErrorClassification::InvalidPayload
            }
        };
        Ok(AdmittedIngestionError::admitted(IngestionError {
            classification,
            code: failure.error.code.clone(),
            message: failure.error.message.clone(),
            metadata: FieldPresence::Missing,
        }))
    }

    /// Classify one task-level Tempest database error.
    fn tempest_backfill_task_failure(
        &mut self,
        operation: TempestBackfillPersistenceOperation,
        _message: &str,
    ) -> TaskFailure {
        TaskFailure {
            attempts: 0,
            code: format!("tempest_{operation:?}"),
        }
    }

    /// Classify one committed Tempest database error.
    fn tempest_backfill_operation_failure(
        &mut self,
        operation: TempestBackfillPersistenceOperation,
        message: &str,
    ) -> TempestBackfillOperationError {
        TempestBackfillOperationError::unexpected(format!("{operation:?}: {message}"))
    }
}

struct FixtureTime;

impl TempestBackfillTimeAdapter for FixtureTime {
    /// Return the first UTC date after the requested archive.
    fn today_utc(&mut self) -> Result<TempestBackfillDate, TaskFailure> {
        Ok(TempestBackfillDate::admitted("2026-11-07", TODAY_MS))
    }

    /// Add exact UTC fixture days.
    fn add_utc_days(
        &mut self,
        date: &TempestBackfillDate,
        days: i64,
    ) -> Result<TempestBackfillDate, TaskFailure> {
        let milliseconds = date.start_ms() + days * 86_400_000;
        // map every admitted fixture date explicitly
        let label = match milliseconds {
            START_MS => "2026-08-01",
            END_MS => "2026-08-02",
            TODAY_MS => "2026-11-07",
            _ => panic!("unexpected Tempest date {milliseconds}"),
        };
        Ok(TempestBackfillDate::admitted(label, milliseconds))
    }

    /// Return admitted provider and durable-run deadlines.
    fn deadlines(&mut self) -> Result<TempestBackfillDeadlines, TempestBackfillOperationError> {
        Ok(TempestBackfillDeadlines {
            provider_deadline_at_ms: PROVIDER_DEADLINE_MS,
            run_deadline_at_ms: RUN_DEADLINE_MS,
            started_at_ms: RECEIVED_MS,
        })
    }
}

#[derive(Clone, Copy)]
enum ProviderOutcome {
    Failure,
    Success,
}

struct FixtureProvider {
    outcome: ProviderOutcome,
    trace: Trace,
}

impl TempestBackfillProvider for FixtureProvider {
    /// Return one real normalized observation or one classified provider failure.
    fn fetch(
        &mut self,
        request: TempestObservationRequest,
        deadline_at_ms: i64,
    ) -> Result<
        ProviderBatch<
            weather_worker::NormalizedTempestRecord,
            TempestResponseMetadata,
            Option<i64>,
        >,
        TempestBackfillOperationError,
    > {
        assert_eq!(deadline_at_ms, PROVIDER_DEADLINE_MS);
        self.trace.borrow_mut().push("provider".to_owned());

        // exercise durable failure after the running row commits
        if matches!(self.outcome, ProviderOutcome::Failure) {
            return Err(TempestBackfillOperationError::provider(
                3,
                TempestBackfillErrorClassification::Retryable,
                "provider_unavailable",
                "Tempest unavailable",
            ));
        }
        let normalized = normalize_tempest_observations(
            TempestObservationEnvelope {
                bucket_step_minutes: Some(1.0),
                device_id: request.device_id,
                observations: Some(vec![fixture_observation(VALID_AT_MS / 1_000)]),
                response_type: TempestResponseType::StationObservation,
                status_code: 0,
            },
            &request,
            RECEIVED_MS,
            2,
        )
        .expect("normalize Tempest fixture");
        Ok(ProviderBatch {
            attempts: normalized.attempts,
            provider_cursor: normalized.provider_cursor_valid_at_ms,
            records: normalized.records,
            response_metadata: normalized.response_metadata,
            upstream_response_checksum: CHECKSUM.to_owned(),
        })
    }
}

/// Build one typed Tempest minute observation.
fn fixture_observation(epoch_seconds: i64) -> TempestObservation {
    TempestObservation {
        air_temperature_c: Some(12.5),
        battery_volts: Some(2.6),
        epoch_seconds,
        illuminance_lux: Some(100.0),
        lightning_average_distance_km: None,
        lightning_strike_count: Some(0.0),
        local_daily_rain_accumulation_final_mm: None,
        local_daily_rain_accumulation_mm: None,
        precipitation_analysis_type: None,
        precipitation_mm: Some(0.1),
        precipitation_type: Some(0.0),
        pressure_hpa: Some(1_012.0),
        rain_accumulation_nc_mm: Some(0.1),
        relative_humidity_percent: Some(80.0),
        report_interval_minutes: Some(1.0),
        solar_radiation_wm2: Some(50.0),
        uv_index: Some(1.0),
        wind_direction_degrees: Some(360.0),
        wind_gust_mps: Some(3.0),
        wind_lull_mps: Some(1.0),
        wind_sample_interval_seconds: Some(3.0),
        wind_speed_mps: Some(2.0),
    }
}

/// Construct the exact active retained Tempest source.
fn source() -> TempestBackfillSource {
    let scheduled = TempestScheduledSource {
        active: true,
        cadence_seconds: 3_600,
        id: "source-tempest".to_owned(),
        material_config_identity: "material-tempest".to_owned(),
        provider_key: "weatherflow-tempest".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_config_fingerprint: FINGERPRINT.to_owned(),
        source_key: "tempest-203055-observations-v2".to_owned(),
        source_kind: TempestSourceKind::PhysicalSensor,
        station_slug: "ballydidean-tempest".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    };
    TempestBackfillSource {
        configuration: TempestRuntimeConfiguration {
            adapter_contract_version: TEMPEST_SOURCE_CONTRACT_VERSION,
            device_id: 470937,
            location_id: 203055,
            material_config_identity: scheduled.material_config_identity.clone(),
            provider_key: scheduled.provider_key.clone(),
            serial: "ST-00198967".to_owned(),
            site_slug: scheduled.site_slug.clone(),
            source_config_fingerprint: FINGERPRINT.to_owned(),
            source_key: scheduled.source_key.clone(),
            source_kind: scheduled.source_kind,
            station_slug: scheduled.station_slug.clone(),
            timezone: scheduled.timezone.clone(),
        },
        history_start: TempestBackfillDate::admitted("2026-08-01", START_MS),
        provider_active: true,
        source: scheduled,
        station_active: true,
        storage_active: true,
    }
}

/// Construct the one-day exact one-shot request.
fn arguments() -> TempestBackfillArguments {
    TempestBackfillArguments {
        chunk_days: 1,
        dry_run: false,
        from: Some(TempestBackfillDate::admitted("2026-08-01", START_MS)),
        resume: true,
        site: "ballydidean".to_owned(),
        station_ids: vec![203055],
        to: TempestBackfillDate::admitted("2026-08-01", START_MS),
    }
}

// verifies the existing executor reaches actual normalized SQL completion
#[test]
fn persists_real_normalized_tempest_chunk_atomically() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let database = FixtureDatabase {
        trace: Rc::clone(&trace),
    };
    let mut repository = DatabaseTempestBackfillRepository::new(database, FixtureValues);
    let mut provider = FixtureProvider {
        outcome: ProviderOutcome::Success,
        trace: Rc::clone(&trace),
    };
    let mut time = FixtureTime;
    let report = execute_tempest_bulk_backfill(
        &arguments(),
        "ballydidean",
        &[source()],
        &mut repository,
        &mut provider,
        &mut time,
    )
    .expect("execute Tempest backfill");
    assert_eq!(report.exit_code, 0);
    assert_eq!(report.stations[0].chunks[0].record_count, 1);
    assert_eq!(
        trace.borrow().as_slice(),
        [
            "resume:6",
            "acquire",
            "abandon:2",
            "find:1",
            "start:10",
            "provider",
            "begin",
            "assert:7",
            "records:32",
            "outcome:succeeded",
            "finalize:success",
            "commit",
            "release",
        ]
    );
}

// verifies classified provider failure is durably finalized after start
#[test]
fn finalizes_exact_tempest_provider_failure() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let database = FixtureDatabase {
        trace: Rc::clone(&trace),
    };
    let mut repository = DatabaseTempestBackfillRepository::new(database, FixtureValues);
    let mut provider = FixtureProvider {
        outcome: ProviderOutcome::Failure,
        trace: Rc::clone(&trace),
    };
    let mut time = FixtureTime;
    let report = execute_tempest_bulk_backfill(
        &arguments(),
        "ballydidean",
        &[source()],
        &mut repository,
        &mut provider,
        &mut time,
    )
    .expect("execute failed Tempest backfill");
    assert_eq!(report.exit_code, 1);
    assert_eq!(
        report.stations[0].chunks[0].error_code.as_deref(),
        Some("provider_unavailable")
    );
    assert_eq!(
        trace.borrow().as_slice(),
        [
            "resume:6",
            "acquire",
            "abandon:2",
            "find:1",
            "start:10",
            "provider",
            "begin",
            "assert:7",
            "outcome:failed",
            "finalize:failed",
            "commit",
            "release",
        ]
    );
}

// verifies compiled Node normalization and transaction ordering remain identical
#[test]
fn matches_compiled_node_tempest_backfill_oracle() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tempest-backfill-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("execute compiled Tempest backfill oracle");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 oracle output");
    assert!(stdout.contains("normalized=1:2026-08-01T00:01:00.000Z"));
    assert!(stdout.contains("success=begin|assert:7|records:32|outcome:11|finalize:7|commit"));
    assert!(stdout.contains("failure=begin|assert:7|outcome:11|failed:7|commit"));
}
