use std::cell::RefCell;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::rc::Rc;

use weather_contracts::{
    FieldPresence, IngestionError, IngestionErrorClassification, JsonValue,
    NormalizedWeatherRecord, Nullable, ScheduledCheckpointState,
};
use weather_database::{
    AdmittedIngestionError, RetainedSourceSession, SerializedJson, Sha256Digest,
    StartedTideBackfillRun, TideBackfillAbandonExpiredQuery, TideBackfillCommandPort,
    TideBackfillFailureBind, TideBackfillFailureQuery, TideBackfillFindRunningQuery,
    TideBackfillOutcome, TideBackfillOutcomeBind, TideBackfillOutcomeQuery, TideBackfillPoolPort,
    TideBackfillRunIdentityQuery, TideBackfillRunStartBind, TideBackfillRunStartQuery,
    TideBackfillRunningLookup, TideBackfillSuccessQuery, TideBackfillSuccessfulChunkQuery,
    WeatherRecordBind, WeatherRecordUpsertQuery,
};
use weather_runtime::{
    DatabaseTideBackfillRepository, NoaaTideRuntimeValues, OpenMeteoRuntimeValues,
    RuntimePersistenceOperation, RuntimeTimeValue, TideBackfillPersistenceOperation,
    TideBackfillRuntimeValues,
};
use weather_worker::{
    NOAA_TIDE_OBSERVATION_ADAPTER_VERSION, NoaaTideCapability, NoaaTideObservation,
    NoaaTidePayload, NoaaTideProviderCursor, NoaaTideRuntimeConfiguration, NoaaTideScheduledSource,
    NoaaTideSourceKind, OpenMeteoCursor, ProviderBatch, TaskFailure, TideBackfillArguments,
    TideBackfillDate, TideBackfillDeadlines, TideBackfillErrorClassification,
    TideBackfillOperationError, TideBackfillProvider, TideBackfillProviderFailure,
    TideBackfillProviderRequest, TideBackfillSource, TideBackfillTimeAdapter,
    execute_tide_backfill, normalize_noaa_tide_range,
};

const START_MS: i64 = 1_788_220_800_000;
const END_MS: i64 = 1_788_307_200_000;
const TODAY_MS: i64 = 1_788_393_600_000;
const RECEIVED_MS: i64 = 1_788_397_200_000;
const PROVIDER_DEADLINE_MS: i64 = 1_788_397_290_000;
const RUN_DEADLINE_MS: i64 = 1_788_397_320_000;
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

impl TideBackfillPoolPort for FixtureDatabase {
    type Error = FixtureError;
    type Session = FixtureSession;

    /// Execute the exact chunk resume query before locking.
    fn has_successful_chunk(
        &mut self,
        query: &TideBackfillSuccessfulChunkQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.binds.len(), 6);
        self.trace.borrow_mut().push("resume:6".to_owned());
        Ok(0)
    }

    /// Acquire one exact source-scoped session.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, Self::Error> {
        assert_eq!(source_id, "source-tide");
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
    /// Return the retained source identity.
    fn source_id(&self) -> &str {
        &self.source_id
    }
}

impl TideBackfillCommandPort for FixtureSession {
    type Error = FixtureError;

    /// Capture exact expired-row recovery.
    fn abandon_expired_runs(
        &mut self,
        query: &TideBackfillAbandonExpiredQuery,
    ) -> Result<Vec<String>, Self::Error> {
        assert_eq!(query.binds.len(), 2);
        self.trace.borrow_mut().push("abandon:2".to_owned());
        Ok(Vec::new())
    }

    /// Capture the running-row lookup before insert.
    fn find_running_run(
        &mut self,
        query: &TideBackfillFindRunningQuery,
    ) -> Result<TideBackfillRunningLookup, Self::Error> {
        assert_eq!(query.binds.len(), 1);
        self.trace.borrow_mut().push("find:1".to_owned());
        Ok(TideBackfillRunningLookup {
            first_run_id: None,
            row_count: 0,
        })
    }

    /// Capture the exact ten-bind committed running row.
    fn insert_running_run(
        &mut self,
        query: &TideBackfillRunStartQuery,
    ) -> Result<Option<StartedTideBackfillRun>, Self::Error> {
        assert_eq!(query.binds.len(), 10);
        assert_eq!(
            query.binds[9],
            TideBackfillRunStartBind::NullableJson(Some(SerializedJson::admitted(
                r#"{"endpoint":"noaa-co-ops/datagetter","station_id":"9444900"}"#,
            )))
        );
        self.trace.borrow_mut().push("start:10".to_owned());
        Ok(Some(StartedTideBackfillRun {
            id: "run-tide".to_owned(),
            started_at: "2026-09-03T01:00:00.000Z".to_owned(),
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
        query: &TideBackfillRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.binds.len(), 7);
        self.trace.borrow_mut().push("assert:7".to_owned());
        Ok(1)
    }

    /// Capture the actual normalized NOAA storage row.
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.record_count, 1);
        assert_eq!(query.binds.len(), 32);
        assert_eq!(
            query.binds[30],
            WeatherRecordBind::NullableNumber(Some(1.234))
        );
        self.trace.borrow_mut().push("records:32".to_owned());
        Ok(())
    }

    /// Capture the successful or failed durable chunk outcome.
    fn upsert_backfill_outcome(
        &mut self,
        query: &TideBackfillOutcomeQuery,
    ) -> Result<(), Self::Error> {
        // preserve the exact outcome in the trace
        let outcome = match query.binds[9] {
            TideBackfillOutcomeBind::Outcome(TideBackfillOutcome::Succeeded) => "succeeded",
            TideBackfillOutcomeBind::Outcome(TideBackfillOutcome::Failed) => "failed",
            _ => panic!("unexpected outcome bind"),
        };
        self.trace.borrow_mut().push(format!("outcome:{outcome}"));
        Ok(())
    }

    /// Capture successful response evidence.
    fn finalize_successful_run(
        &mut self,
        query: &TideBackfillSuccessQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.binds.len(), 7);
        self.trace.borrow_mut().push("finalize:success".to_owned());
        Ok(1)
    }

    /// Capture exact durable provider failure fields.
    fn finalize_failed_run(
        &mut self,
        query: &TideBackfillFailureQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.binds.len(), 7);
        assert_eq!(query.binds[2], TideBackfillFailureBind::Count(3));
        assert_eq!(
            query.binds[4],
            TideBackfillFailureBind::Classification(IngestionErrorClassification::Retryable)
        );
        assert_eq!(
            query.binds[5],
            TideBackfillFailureBind::Text("provider_unavailable".to_owned())
        );
        assert_eq!(
            query.binds[6],
            TideBackfillFailureBind::Text("NOAA unavailable".to_owned())
        );
        self.trace.borrow_mut().push("finalize:failed".to_owned());
        Ok(1)
    }

    /// Record transaction commit.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("commit".to_owned());
        Ok(())
    }

    /// Record unexpected rollback.
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
    /// Format only the admitted tide fixture instants.
    fn utc_instant(
        &mut self,
        milliseconds: i64,
        _value: RuntimeTimeValue,
    ) -> Result<String, TaskFailure> {
        // map every admitted fixture instant explicitly
        let value = match milliseconds {
            START_MS => "2026-09-01T00:00:00.000Z",
            END_MS => "2026-09-02T00:00:00.000Z",
            RECEIVED_MS => "2026-09-03T01:00:00.000Z",
            PROVIDER_DEADLINE_MS => "2026-09-03T01:01:30.000Z",
            RUN_DEADLINE_MS => "2026-09-03T01:02:00.000Z",
            value if value == START_MS + 360_000 => "2026-09-01T00:06:00.000Z",
            _ => panic!("unexpected tide time {milliseconds}"),
        };
        Ok(value.to_owned())
    }

    /// Reject unused durable timestamp decoding.
    fn epoch_milliseconds(
        &mut self,
        _instant: &str,
        _value: RuntimeTimeValue,
    ) -> Result<i64, TaskFailure> {
        unreachable!("backfill does not decode scheduled checkpoints")
    }

    /// Serialize provider, quality, and response objects through the admitted owner.
    fn serialize_json_object(
        &mut self,
        value: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure> {
        assert!(!value.is_empty());
        Ok(SerializedJson::admitted("admitted-noaa-json"))
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
        unreachable!("tide backfill does not persist a scheduled cursor")
    }

    /// Reject the Open-Meteo checkpoint path.
    fn checkpoint_cursor(
        &mut self,
        _checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<OpenMeteoCursor>, TaskFailure> {
        unreachable!("tide backfill does not read a scheduled checkpoint")
    }

    /// Admit the actual normalized NOAA row hash.
    fn weather_record_content_hash(
        &mut self,
        record: &NormalizedWeatherRecord,
    ) -> Result<Sha256Digest, TaskFailure> {
        assert_eq!(record.sourceId, "source-tide");
        assert_eq!(record.validAt, "2026-09-01T00:06:00.000Z");
        assert_eq!(record.metrics.waterLevelM, Nullable::Value(1.234));
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
        unreachable!("tide backfill has an exact classified failure")
    }

    /// Retain the existing scheduled persistence classifier contract.
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

impl NoaaTideRuntimeValues for FixtureValues {
    /// Reject scheduled cursor persistence.
    fn noaa_tide_provider_cursor_json(
        &mut self,
        _cursor: Option<&NoaaTideProviderCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        unreachable!("tide backfill resume is chunk-identity based")
    }

    /// Reject scheduled checkpoint decoding.
    fn noaa_tide_checkpoint_cursor(
        &mut self,
        _checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<NoaaTideProviderCursor>, TaskFailure> {
        unreachable!("tide backfill resume is chunk-identity based")
    }
}

impl TideBackfillRuntimeValues for FixtureValues {
    /// Serialize exact endpoint and station evidence.
    fn tide_backfill_request_metadata_json(
        &mut self,
        endpoint: &str,
        station_id: &str,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        assert_eq!(endpoint, "noaa-co-ops/datagetter");
        assert_eq!(station_id, "9444900");
        Ok(Some(SerializedJson::admitted(
            r#"{"endpoint":"noaa-co-ops/datagetter","station_id":"9444900"}"#,
        )))
    }

    /// Admit exact classified provider failure fields.
    fn tide_backfill_ingestion_error(
        &mut self,
        failure: &TideBackfillProviderFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure> {
        // preserve the worker's exact durable classification
        let classification = match failure.error.classification {
            TideBackfillErrorClassification::Retryable => IngestionErrorClassification::Retryable,
            TideBackfillErrorClassification::RateLimited => {
                IngestionErrorClassification::RateLimited
            }
            TideBackfillErrorClassification::Permanent => IngestionErrorClassification::Permanent,
            TideBackfillErrorClassification::InvalidPayload => {
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

    /// Classify a task-level tide database error.
    fn tide_backfill_task_failure(
        &mut self,
        operation: TideBackfillPersistenceOperation,
        _message: &str,
    ) -> TaskFailure {
        TaskFailure {
            attempts: 0,
            code: format!("tide_{operation:?}"),
        }
    }

    /// Classify a committed tide database error.
    fn tide_backfill_operation_failure(
        &mut self,
        operation: TideBackfillPersistenceOperation,
        message: &str,
    ) -> TideBackfillOperationError {
        TideBackfillOperationError::unexpected(format!("{operation:?}: {message}"))
    }
}

struct FixtureTime;

impl TideBackfillTimeAdapter for FixtureTime {
    /// Return the first UTC date after the requested archive.
    fn today_utc(&mut self) -> Result<TideBackfillDate, TaskFailure> {
        Ok(TideBackfillDate::admitted("2026-09-03", TODAY_MS))
    }

    /// Add exact UTC fixture days.
    fn add_utc_days(
        &mut self,
        date: &TideBackfillDate,
        days: i64,
    ) -> Result<TideBackfillDate, TaskFailure> {
        let milliseconds = date.start_ms() + days * 86_400_000;
        // map every admitted fixture date explicitly
        let label = match milliseconds {
            START_MS => "2026-09-01",
            END_MS => "2026-09-02",
            TODAY_MS => "2026-09-03",
            1_790_812_800_000 => "2026-10-01",
            _ => panic!("unexpected tide date {milliseconds}"),
        };
        Ok(TideBackfillDate::admitted(label, milliseconds))
    }

    /// Return the admitted provider and durable-run deadlines.
    fn deadlines(&mut self) -> Result<TideBackfillDeadlines, TideBackfillOperationError> {
        Ok(TideBackfillDeadlines {
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

impl TideBackfillProvider for FixtureProvider {
    /// Return one real normalized observation or one classified provider failure.
    fn fetch(
        &mut self,
        request: TideBackfillProviderRequest,
    ) -> Result<
        ProviderBatch<
            weather_worker::NormalizedNoaaTideRecord,
            weather_worker::NoaaTideResponseMetadata,
            Option<NoaaTideProviderCursor>,
        >,
        TideBackfillOperationError,
    > {
        self.trace.borrow_mut().push("provider".to_owned());

        // exercise durable failure after the running row commits
        if matches!(self.outcome, ProviderOutcome::Failure) {
            return Err(TideBackfillOperationError::provider(
                3,
                TideBackfillErrorClassification::Retryable,
                "provider_unavailable",
                "NOAA unavailable",
            ));
        }
        let payload = NoaaTidePayload {
            error_message: None,
            observations: Some(vec![NoaaTideObservation {
                flags: "0,0,0,0".to_owned(),
                quality_status: "v".to_owned(),
                valid_at_ms: START_MS + 360_000,
                water_level_m: 1.234,
            }]),
            predictions: None,
        };
        let normalized = normalize_noaa_tide_range(&payload, &request.range, RECEIVED_MS, 2)
            .expect("normalize NOAA fixture");
        Ok(ProviderBatch {
            attempts: normalized.attempts,
            provider_cursor: normalized
                .provider_cursor_valid_at_ms
                .map(|valid_at_ms| NoaaTideProviderCursor { valid_at_ms }),
            records: normalized.records,
            response_metadata: normalized.response_metadata,
            upstream_response_checksum: CHECKSUM.to_owned(),
        })
    }
}

/// Construct the exact active retained observation source.
fn source() -> TideBackfillSource {
    let scheduled = NoaaTideScheduledSource {
        active: true,
        cadence_seconds: 360,
        id: "source-tide".to_owned(),
        material_config_identity: "material-tide".to_owned(),
        provider_key: "noaa-co-ops".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_config_fingerprint: FINGERPRINT.to_owned(),
        source_key: "noaa-port-townsend-water-level-v1".to_owned(),
        source_kind: NoaaTideSourceKind::Observation,
        station_slug: "port-townsend-tide-gauge".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    };
    TideBackfillSource {
        configuration: NoaaTideRuntimeConfiguration {
            adapter_contract_version: NOAA_TIDE_OBSERVATION_ADAPTER_VERSION,
            capabilities: BTreeSet::from([
                NoaaTideCapability::Current,
                NoaaTideCapability::Historical,
            ]),
            forecast_days: None,
            material_config_identity: scheduled.material_config_identity.clone(),
            maximum_chunk_days: 31,
            provider_key: scheduled.provider_key.clone(),
            site_slug: scheduled.site_slug.clone(),
            source_active: true,
            source_config_fingerprint: FINGERPRINT.to_owned(),
            source_key: scheduled.source_key.clone(),
            source_kind: NoaaTideSourceKind::Observation,
            station_active: true,
            station_id: "9444900".to_owned(),
            station_slug: scheduled.station_slug.clone(),
            timezone: scheduled.timezone.clone(),
        },
        history_start: TideBackfillDate::admitted("2026-09-01", START_MS),
        source: scheduled,
        storage_active: true,
    }
}

/// Construct the one-day exact one-shot request.
fn arguments() -> TideBackfillArguments {
    TideBackfillArguments {
        dry_run: false,
        from: TideBackfillDate::admitted("2026-09-01", START_MS),
        resume: true,
        site: "ballydidean".to_owned(),
        source_keys: vec!["noaa-port-townsend-water-level-v1".to_owned()],
        to: TideBackfillDate::admitted("2026-09-01", START_MS),
    }
}

// verifies the actual typed executor reaches normalized SQL completion
#[test]
fn persists_real_normalized_tide_chunk_atomically() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let database = FixtureDatabase {
        trace: Rc::clone(&trace),
    };
    let mut repository = DatabaseTideBackfillRepository::new(database, FixtureValues);
    let mut provider = FixtureProvider {
        outcome: ProviderOutcome::Success,
        trace: Rc::clone(&trace),
    };
    let mut time = FixtureTime;
    let report = execute_tide_backfill(
        &arguments(),
        "ballydidean",
        &[source()],
        &mut repository,
        &mut provider,
        &mut time,
    )
    .expect("execute tide backfill");
    assert_eq!(report.exit_code, 0);
    assert_eq!(report.sources[0].records, 1);
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
fn finalizes_exact_tide_provider_failure() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let database = FixtureDatabase {
        trace: Rc::clone(&trace),
    };
    let mut repository = DatabaseTideBackfillRepository::new(database, FixtureValues);
    let mut provider = FixtureProvider {
        outcome: ProviderOutcome::Failure,
        trace: Rc::clone(&trace),
    };
    let mut time = FixtureTime;
    let report = execute_tide_backfill(
        &arguments(),
        "ballydidean",
        &[source()],
        &mut repository,
        &mut provider,
        &mut time,
    )
    .expect("execute failed tide backfill");
    assert_eq!(report.exit_code, 1);
    assert_eq!(
        report.sources[0]
            .failed_chunk
            .as_ref()
            .expect("failed chunk")
            .error_code,
        "provider_unavailable"
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
