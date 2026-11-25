use std::cell::RefCell;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::rc::Rc;

use weather_contracts::{
    FieldPresence, IngestionError, IngestionErrorClassification, IngestionState, JsonValue,
    Nullable, ScheduledCheckpointState, SourceKind, TempestConfiguration,
    TempestConfigurationProvider, TempestStationConfiguration,
};
use weather_database::{
    AssertRunningScheduledBind, AssertRunningScheduledQuery, CheckpointUpdateBind,
    FinalizeFailedIngestionBind, FinalizeFailedIngestionQuery, FinalizeScheduledRunBind,
    FinalizeScheduledRunQuery, FindRunningIngestionBind, FindRunningIngestionQuery,
    GetScheduledCheckpointBind, GetScheduledCheckpointQuery, InsertScheduledIngestionBind,
    InsertScheduledIngestionQuery, RetainedSourceSession, RunningIngestionLookup,
    ScheduledCheckpointDbRow, ScheduledCheckpointQuery, ScheduledCheckpointReadPort,
    ScheduledIngestionCommandPort, ScheduledRunFailureCommandPort, ScheduledRunIdentityBind,
    ScheduledRunIdentityQuery, ScheduledRunStartCommandPort, SerializedJson, Sha256Digest,
    StartedScheduledIngestion, WeatherRecordBind, WeatherRecordUpsertQuery,
};
use weather_providers::{
    ContentBudget, MAX_PROVIDER_BODY_BYTES, ProviderAttempt, ProviderAttemptError,
    ProviderAttemptResponse, ProviderControls, ProviderIo, ProviderSleep, ProviderSleepError,
};
use weather_runtime::{
    OpenMeteoDatabaseBoundary, OpenMeteoRuntimeValues, RetryingTempestProvider,
    RuntimePersistenceOperation, RuntimeTimeValue, TempestAttemptPayload,
    TempestDatabaseRepository, TempestRuntimeValues, WorkerIterationDatabaseRepository,
    WorkerSourceMaterialIdentity, WorkerTempestDispatcher, WorkerTempestTransport,
};
use weather_worker::{
    OpenMeteoCursor, ScheduledSourceMaterialComparison, SourceRunStatus,
    TEMPEST_OBSERVATION_ADAPTER_VERSION, TEMPEST_OBSERVATION_ENDPOINT,
    TEMPEST_SOURCE_CONTRACT_VERSION, TaskFailure, TempestObservation, TempestObservationEnvelope,
    TempestProviderCursor, TempestResponseType, TempestRuntimeConfiguration,
    TempestScheduledExecution, TempestScheduledSource, TempestSourceKind, WorkerDueSource,
    WorkerScheduledSourceDispatcher, run_scheduled_tempest_source,
};

const SOURCE_ID: &str = "source-tempest";
const FINGERPRINT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const RESPONSE_CHECKSUM: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CONTENT_HASH: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const CHECKPOINT_VALID_AT_MS: i64 = 1_787_371_140_000;
const WINDOW_START_MS: i64 = 1_787_371_200_000;
const WINDOW_END_MS: i64 = 1_787_374_800_000;
const RECEIVED_AT_MS: i64 = 1_787_376_000_000;
const DEADLINE_AT_MS: i64 = 1_787_376_600_000;
const DISPATCHER_DEADLINE_AT_MS: i64 = RECEIVED_AT_MS + 120_000;
const CHECKPOINT_VALID_AT: &str = "2026-08-22T03:59:00.000Z";
const WINDOW_START: &str = "2026-08-22T04:00:00.000Z";
const WINDOW_END: &str = "2026-08-22T05:00:00.000Z";
const RECEIVED_AT: &str = "2026-08-22T05:20:00.000Z";
const DEADLINE_AT: &str = "2026-08-22T05:30:00.000Z";
const DISPATCHER_DEADLINE_AT: &str = "2026-08-22T05:22:00.000Z";
const PROVIDER_JSON: &str = r#"{"battery_volts":2.7,"device_id":470937,"illuminance_lux":20.0,"lightning_average_distance_km":4.0,"lightning_strike_count":0.0,"location_id":203055,"precipitation_type":0.0,"rain_accumulation_nc_mm":0.0,"report_interval_minutes":1.0,"wind_lull_mps":0.5,"wind_sample_interval_seconds":3.0}"#;
const QUALITY_JSON: &str = r#"{"sampling":"every_distinct_provider_observation"}"#;
const RESPONSE_JSON: &str = r#"{"device_id":470937,"minute_record_count":1,"raw_observation_count":1,"bucket_step_minutes":1.0}"#;
const CURSOR_JSON: &str = r#"{"valid_at":"2026-08-22T04:00:00.000Z"}"#;
const CHECKPOINT_CURSOR_JSON: &str = r#"{"valid_at":"2026-08-22T03:59:00.000Z"}"#;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// render one fixture database failure
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

#[derive(Debug)]
struct FixtureSession {
    expected_deadline: String,
    source_id: String,
    trace: Rc<RefCell<Vec<String>>>,
}

impl RetainedSourceSession for FixtureSession {
    /// return the retained Tempest source identity
    fn source_id(&self) -> &str {
        &self.source_id
    }
}

impl ScheduledCheckpointReadPort for FixtureSession {
    type Error = FixtureError;

    /// return the exact typed Tempest checkpoint row
    fn get_scheduled_checkpoint(
        &mut self,
        query: &GetScheduledCheckpointQuery,
    ) -> Result<Option<ScheduledCheckpointDbRow>, Self::Error> {
        assert_eq!(
            query.binds,
            [GetScheduledCheckpointBind::BigintText(SOURCE_ID.to_owned())]
        );
        self.trace.borrow_mut().push("checkpoint".to_owned());
        Ok(Some(ScheduledCheckpointDbRow {
            last_committed_at: WINDOW_START.to_owned(),
            last_valid_at: CHECKPOINT_VALID_AT.to_owned(),
            provider_cursor: Nullable::Value(vec![(
                utf16("valid_at"),
                JsonValue::String(utf16(CHECKPOINT_VALID_AT)),
            )]),
            source_id: SOURCE_ID.to_owned(),
            version: 7.0,
            window_end_exclusive: WINDOW_START.to_owned(),
            window_start: CHECKPOINT_VALID_AT.to_owned(),
        }))
    }
}

impl ScheduledRunStartCommandPort for FixtureSession {
    type Error = FixtureError;

    /// verify source-scoped running-row lookup
    fn find_running_ingestion(
        &mut self,
        query: &FindRunningIngestionQuery,
    ) -> Result<RunningIngestionLookup, Self::Error> {
        assert_eq!(
            query.binds,
            [FindRunningIngestionBind::BigintText(SOURCE_ID.to_owned())]
        );
        self.trace.borrow_mut().push("start-lookup".to_owned());
        Ok(RunningIngestionLookup {
            first_run_id: None,
            row_count: 0,
        })
    }

    /// verify exact Tempest run identity and request evidence
    fn insert_scheduled_ingestion(
        &mut self,
        query: &InsertScheduledIngestionQuery,
    ) -> Result<Option<StartedScheduledIngestion>, Self::Error> {
        assert_eq!(
            query.binds,
            [
                InsertScheduledIngestionBind::BigintText(SOURCE_ID.to_owned()),
                InsertScheduledIngestionBind::ScheduledMode,
                InsertScheduledIngestionBind::UtcInstant(WINDOW_START.to_owned()),
                InsertScheduledIngestionBind::UtcInstant(WINDOW_END.to_owned()),
                InsertScheduledIngestionBind::Fingerprint(Sha256Digest::admitted(FINGERPRINT)),
                InsertScheduledIngestionBind::Text(TEMPEST_OBSERVATION_ADAPTER_VERSION.to_owned()),
                InsertScheduledIngestionBind::NullChunkPlanVersion,
                InsertScheduledIngestionBind::UtcInstant(self.expected_deadline.clone()),
                InsertScheduledIngestionBind::Count(0),
                InsertScheduledIngestionBind::NullableJson(Some(SerializedJson::admitted(
                    r#"{"endpoint":"observations/device"}"#,
                ))),
            ]
        );
        self.trace.borrow_mut().push("start-insert".to_owned());
        Ok(Some(StartedScheduledIngestion {
            id: "run-tempest".to_owned(),
            started_at: RECEIVED_AT.to_owned(),
            state: IngestionState::Running,
        }))
    }
}

impl ScheduledIngestionCommandPort for FixtureSession {
    type Error = FixtureError;

    /// record one atomic Tempest completion start
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("complete-begin".to_owned());
        Ok(())
    }

    /// verify the running Tempest row identity
    fn assert_scheduled_run(
        &mut self,
        query: &ScheduledRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(
            query.binds,
            [
                ScheduledRunIdentityBind::BigintText("run-tempest".to_owned()),
                ScheduledRunIdentityBind::BigintText(SOURCE_ID.to_owned()),
                ScheduledRunIdentityBind::UtcInstant(WINDOW_START.to_owned()),
                ScheduledRunIdentityBind::UtcInstant(WINDOW_END.to_owned()),
            ]
        );
        self.trace.borrow_mut().push("complete-assert".to_owned());
        Ok(1)
    }

    /// verify actual normalized Tempest provenance reaches storage binds
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.record_count, 1);
        assert_eq!(query.binds.len(), 32);
        assert_eq!(
            query.binds[0],
            WeatherRecordBind::BigintText(SOURCE_ID.to_owned())
        );
        assert_eq!(
            query.binds[1],
            WeatherRecordBind::SourceKind(SourceKind::PhysicalSensor)
        );
        assert_eq!(
            query.binds[2],
            WeatherRecordBind::UtcInstant(WINDOW_START.to_owned())
        );
        assert_eq!(query.binds[3], WeatherRecordBind::NullableUtcInstant(None));
        assert_eq!(
            query.binds[4],
            WeatherRecordBind::BigintText("run-tempest".to_owned())
        );
        assert_eq!(
            query.binds[5],
            WeatherRecordBind::UtcInstant(RECEIVED_AT.to_owned())
        );
        assert_eq!(
            query.binds[6],
            WeatherRecordBind::Text("America/Los_Angeles".to_owned())
        );
        assert_eq!(query.binds[7], WeatherRecordBind::NullableText(None));
        assert_eq!(
            query.binds[8],
            WeatherRecordBind::NullableText(Some("WeatherFlow".to_owned()))
        );
        assert_eq!(
            query.binds[9],
            WeatherRecordBind::NullableText(Some("Tempest".to_owned()))
        );
        assert_eq!(
            query.binds[10],
            WeatherRecordBind::NullableText(Some("ST-00198967".to_owned()))
        );
        assert_eq!(
            query.binds[11],
            WeatherRecordBind::NullableJson(Some(SerializedJson::admitted(QUALITY_JSON)))
        );
        assert_eq!(
            query.binds[12],
            WeatherRecordBind::NullableJson(Some(SerializedJson::admitted(PROVIDER_JSON)))
        );
        assert_eq!(
            query.binds[15],
            WeatherRecordBind::NullableNumber(Some(0.1))
        );
        assert_eq!(
            query.binds[24],
            WeatherRecordBind::NullableNumber(Some(6.0))
        );
        assert_eq!(
            query.binds[13],
            WeatherRecordBind::NullableNumber(Some(10.0))
        );
        assert_eq!(
            query.binds[31],
            WeatherRecordBind::ContentHash(Sha256Digest::admitted(CONTENT_HASH))
        );
        self.trace.borrow_mut().push("records".to_owned());
        Ok(())
    }

    /// verify typed Tempest cursor round-trip and checkpoint compare-and-set
    fn advance_scheduled_checkpoint(
        &mut self,
        query: &ScheduledCheckpointQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(
            query,
            &ScheduledCheckpointQuery::Update {
                sql: weather_database::UPDATE_SCHEDULED_CHECKPOINT_SQL,
                binds: [
                    CheckpointUpdateBind::BigintText(SOURCE_ID.to_owned()),
                    CheckpointUpdateBind::Version(7),
                    CheckpointUpdateBind::UtcInstant(WINDOW_START.to_owned()),
                    CheckpointUpdateBind::UtcInstant(WINDOW_START.to_owned()),
                    CheckpointUpdateBind::UtcInstant(WINDOW_END.to_owned()),
                    CheckpointUpdateBind::NullableJson(Some(SerializedJson::admitted(CURSOR_JSON))),
                ],
            }
        );
        self.trace.borrow_mut().push("checkpoint-update".to_owned());
        Ok(1)
    }

    /// verify exact Tempest response metadata finalization
    fn finalize_scheduled_run(
        &mut self,
        query: &FinalizeScheduledRunQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(
            query.binds,
            [
                FinalizeScheduledRunBind::BigintText("run-tempest".to_owned()),
                FinalizeScheduledRunBind::BigintText(SOURCE_ID.to_owned()),
                FinalizeScheduledRunBind::Count(2),
                FinalizeScheduledRunBind::Count(1),
                FinalizeScheduledRunBind::NullableJson(Some(SerializedJson::admitted(
                    RESPONSE_JSON,
                ))),
                FinalizeScheduledRunBind::NullableChecksum(Some(Sha256Digest::admitted(
                    RESPONSE_CHECKSUM,
                ))),
                FinalizeScheduledRunBind::ScheduledMode,
            ]
        );
        self.trace.borrow_mut().push("finalize-success".to_owned());
        Ok(1)
    }

    /// record successful Tempest completion commit
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("complete-commit".to_owned());
        Ok(())
    }

    /// record unexpected Tempest completion rollback
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("complete-rollback".to_owned());
        Ok(())
    }
}

impl ScheduledRunFailureCommandPort for FixtureSession {
    type Error = FixtureError;

    /// record Tempest failure finalization start
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("failure-begin".to_owned());
        Ok(())
    }

    /// verify one failed Tempest run identity
    fn assert_running_scheduled(
        &mut self,
        query: &AssertRunningScheduledQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(
            query.binds,
            [
                AssertRunningScheduledBind::BigintText("run-tempest".to_owned()),
                AssertRunningScheduledBind::BigintText(SOURCE_ID.to_owned()),
                AssertRunningScheduledBind::ScheduledMode,
            ]
        );
        self.trace.borrow_mut().push("failure-assert".to_owned());
        Ok(1)
    }

    /// verify typed Tempest provider failure finalization
    fn finalize_failed_ingestion(
        &mut self,
        query: &FinalizeFailedIngestionQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(
            query.binds,
            [
                FinalizeFailedIngestionBind::BigintText("run-tempest".to_owned()),
                FinalizeFailedIngestionBind::BigintText(SOURCE_ID.to_owned()),
                FinalizeFailedIngestionBind::Count(3),
                FinalizeFailedIngestionBind::NullableJson(None),
                FinalizeFailedIngestionBind::Classification(
                    IngestionErrorClassification::Retryable,
                ),
                FinalizeFailedIngestionBind::Text("provider_server_error".to_owned()),
                FinalizeFailedIngestionBind::Text("provider_server_error".to_owned()),
            ]
        );
        self.trace.borrow_mut().push("failure-finalize".to_owned());
        Ok(1)
    }

    /// record Tempest failure commit
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("failure-commit".to_owned());
        Ok(())
    }

    /// record unexpected Tempest failure rollback
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("failure-rollback".to_owned());
        Ok(())
    }
}

struct FixtureDatabase {
    expected_deadline: String,
    trace: Rc<RefCell<Vec<String>>>,
}

impl OpenMeteoDatabaseBoundary for FixtureDatabase {
    type Session = FixtureSession;

    /// acquire one retained Tempest source session
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure> {
        assert_eq!(source_id, SOURCE_ID);
        self.trace.borrow_mut().push("acquire".to_owned());
        Ok(Some(FixtureSession {
            expected_deadline: self.expected_deadline.clone(),
            source_id: source_id.to_owned(),
            trace: Rc::clone(&self.trace),
        }))
    }

    /// record stale-run cleanup at the admitted instant
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        as_of: &str,
    ) -> Result<(), TaskFailure> {
        assert_eq!(session.source_id(), SOURCE_ID);
        assert_eq!(as_of, RECEIVED_AT);
        self.trace.borrow_mut().push("abandon".to_owned());
        Ok(())
    }

    /// release the exact retained Tempest session
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
        assert_eq!(session.source_id(), SOURCE_ID);
        self.trace.borrow_mut().push("release".to_owned());
        Ok(())
    }
}

#[derive(Default)]
struct FixtureValues;

impl OpenMeteoRuntimeValues for FixtureValues {
    /// resolve only admitted Tempest fixture timestamps
    fn utc_instant(
        &mut self,
        milliseconds: i64,
        value: RuntimeTimeValue,
    ) -> Result<String, TaskFailure> {
        let instant = match (milliseconds, value) {
            // map the exact scheduled window and provider instants
            (
                WINDOW_START_MS,
                RuntimeTimeValue::RequestedStart
                | RuntimeTimeValue::LastValidAt
                | RuntimeTimeValue::ValidAt,
            ) => WINDOW_START,
            (WINDOW_END_MS, RuntimeTimeValue::RequestedEndExclusive) => WINDOW_END,
            (RECEIVED_AT_MS, RuntimeTimeValue::AbandonAsOf | RuntimeTimeValue::ReceivedAt) => {
                RECEIVED_AT
            }
            (DEADLINE_AT_MS, RuntimeTimeValue::Deadline) => DEADLINE_AT,
            (DISPATCHER_DEADLINE_AT_MS, RuntimeTimeValue::Deadline) => DISPATCHER_DEADLINE_AT,
            _ => panic!("unexpected UTC boundary: {milliseconds:?} {value:?}"),
        };
        Ok(instant.to_owned())
    }

    /// decode the checkpoint window through the admitted time seam
    fn epoch_milliseconds(
        &mut self,
        instant: &str,
        value: RuntimeTimeValue,
    ) -> Result<i64, TaskFailure> {
        assert_eq!(
            (instant, value),
            (WINDOW_START, RuntimeTimeValue::CheckpointWindowEnd)
        );
        Ok(WINDOW_START_MS)
    }

    /// serialize exact Tempest provider quality and response objects
    fn serialize_json_object(
        &mut self,
        value: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure> {
        let first_key = value
            .first()
            .expect("serialized object should not be empty")
            .0
            .clone();

        // select the exact admitted fixture object by its first ordered key
        if first_key == utf16("battery_volts") {
            assert_eq!(value, expected_provider_metadata());
            return Ok(SerializedJson::admitted(PROVIDER_JSON));
        }

        // retain exact quality field order
        if first_key == utf16("sampling") {
            assert_eq!(value, expected_quality_metadata());
            return Ok(SerializedJson::admitted(QUALITY_JSON));
        }

        assert_eq!(value, expected_response_metadata());
        Ok(SerializedJson::admitted(RESPONSE_JSON))
    }

    /// serialize the exact Tempest endpoint evidence
    fn request_metadata_json(&mut self, endpoint: &str) -> Result<SerializedJson, TaskFailure> {
        assert_eq!(endpoint, TEMPEST_OBSERVATION_ENDPOINT);
        Ok(SerializedJson::admitted(
            r#"{"endpoint":"observations/device"}"#,
        ))
    }

    /// reject the OpenMeteo string cursor path for Tempest
    fn provider_cursor_json(
        &mut self,
        _cursor: Option<&OpenMeteoCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        panic!("Tempest must not use the OpenMeteo cursor mapper")
    }

    /// reject the OpenMeteo checkpoint cursor path for Tempest
    fn checkpoint_cursor(
        &mut self,
        _checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<OpenMeteoCursor>, TaskFailure> {
        panic!("Tempest must not use the OpenMeteo checkpoint mapper")
    }

    /// admit the actual normalized Tempest record hash
    fn weather_record_content_hash(
        &mut self,
        record: &weather_contracts::NormalizedWeatherRecord,
    ) -> Result<Sha256Digest, TaskFailure> {
        assert_eq!(record.sourceId, SOURCE_ID);
        assert_eq!(record.sourceKind, SourceKind::PhysicalSensor);
        assert_eq!(record.validAt, WINDOW_START);
        assert_eq!(record.receivedAt, RECEIVED_AT);
        assert_eq!(record.metrics.temperatureC, Nullable::Value(10.0));
        assert_eq!(
            record.metadata.device,
            Nullable::Value(weather_contracts::DeviceMetadata {
                model: FieldPresence::Value("Tempest".to_owned()),
                serial: FieldPresence::Value("ST-00198967".to_owned()),
                vendor: FieldPresence::Value("WeatherFlow".to_owned()),
            })
        );
        Ok(Sha256Digest::admitted(CONTENT_HASH))
    }

    /// admit the exact source fingerprint and response checksum
    fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure> {
        assert!(value == FINGERPRINT || value == RESPONSE_CHECKSUM);
        Ok(Sha256Digest::admitted(value))
    }

    /// map a Tempest provider failure through the shared domain boundary
    fn ingestion_error(
        &mut self,
        failure: &TaskFailure,
    ) -> Result<weather_database::AdmittedIngestionError, TaskFailure> {
        Ok(weather_database::AdmittedIngestionError::admitted(
            IngestionError {
                classification: IngestionErrorClassification::Retryable,
                code: failure.code.clone(),
                message: failure.code.clone(),
                metadata: FieldPresence::Missing,
            },
        ))
    }

    /// map one persistence error to a bounded worker failure
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
    /// encode the typed cursor as Node's exact valid_at JSON object
    fn tempest_provider_cursor_json(
        &mut self,
        cursor: Option<&TempestProviderCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        assert_eq!(
            cursor,
            Some(&TempestProviderCursor {
                valid_at_ms: WINDOW_START_MS,
            })
        );
        Ok(Some(SerializedJson::admitted(CURSOR_JSON)))
    }

    /// decode Node's exact valid_at cursor into the typed Tempest cursor
    fn tempest_checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<TempestProviderCursor>, TaskFailure> {
        assert_eq!(checkpoint.sourceId, SOURCE_ID);
        assert_eq!(
            checkpoint.providerCursor,
            Nullable::Value(vec![(
                utf16("valid_at"),
                JsonValue::String(utf16(CHECKPOINT_VALID_AT)),
            )])
        );
        assert_eq!(
            CHECKPOINT_CURSOR_JSON,
            r#"{"valid_at":"2026-08-22T03:59:00.000Z"}"#
        );
        Ok(Some(TempestProviderCursor {
            valid_at_ms: CHECKPOINT_VALID_AT_MS,
        }))
    }
}

#[derive(Clone, Copy)]
enum ProviderOutcome {
    Failure,
    Success,
}

struct FixtureProvider {
    attempts: Vec<ProviderAttempt>,
    now_ms: i64,
    outcomes:
        VecDeque<Result<ProviderAttemptResponse<TempestAttemptPayload>, ProviderAttemptError>>,
    random: VecDeque<f64>,
    sleeps: Vec<ProviderSleep>,
    trace: Rc<RefCell<Vec<String>>>,
}

impl ProviderIo<TempestAttemptPayload> for FixtureProvider {
    /// return the deterministic Tempest provider clock
    fn now_ms(&mut self) -> i64 {
        self.now_ms
    }

    /// return one deterministic Tempest retry jitter input
    fn random_unit(&mut self) -> f64 {
        self.random.pop_front().unwrap_or(0.0)
    }

    /// retain one physical Tempest attempt and return its typed outcome
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<TempestAttemptPayload>, ProviderAttemptError> {
        self.trace
            .borrow_mut()
            .push(format!("provider-attempt-{}", attempt.attempt));
        self.attempts.push(attempt);
        self.outcomes
            .pop_front()
            .unwrap_or_else(|| panic!("Tempest provider response missing"))
    }

    /// retain and apply one exact Tempest retry sleep
    fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
        self.trace
            .borrow_mut()
            .push(format!("provider-sleep-{}", sleep.attempts));
        self.sleeps.push(sleep);
        self.now_ms += i64::try_from(sleep.delay_ms).expect("Tempest delay should fit i64");
        Ok(())
    }
}

/// Physical Tempest controller evidence retained after one scheduled run.
struct ProviderExecutionEvidence {
    attempts: Vec<ProviderAttempt>,
    sleeps: Vec<ProviderSleep>,
    trace: Vec<String>,
}

/// Build one typed Tempest payload after admitted transport parsing.
fn tempest_attempt_payload() -> TempestAttemptPayload {
    TempestAttemptPayload {
        envelope: tempest_envelope(),
        received_at_ms: RECEIVED_AT_MS,
        upstream_response_checksum: RESPONSE_CHECKSUM.to_owned(),
    }
}

/// Build one physical Tempest response with its source-specific byte evidence.
fn provider_response(
    status: u16,
    reason: Option<&str>,
) -> ProviderAttemptResponse<TempestAttemptPayload> {
    ProviderAttemptResponse {
        body: tempest_attempt_payload(),
        content: ContentBudget {
            declared_body_bytes: Some(2_500_000),
            streamed_body_bytes: 2_500_000,
        },
        provider_reason: reason.map(str::to_owned),
        retry_after_ms: None,
        status,
    }
}

/// Build one controller-backed Tempest adapter for success or exhaustion.
fn provider_fixture(
    outcome: ProviderOutcome,
    trace: Rc<RefCell<Vec<String>>>,
) -> RetryingTempestProvider<FixtureProvider> {
    // select the exact physical attempt sequence
    let outcomes = match outcome {
        ProviderOutcome::Failure => VecDeque::from([
            Ok(provider_response(503, Some("temporary one"))),
            Ok(provider_response(503, Some("temporary two"))),
            Ok(provider_response(503, Some("still unavailable"))),
        ]),
        ProviderOutcome::Success => VecDeque::from([
            Ok(provider_response(503, Some("temporary outage"))),
            Ok(provider_response(200, None)),
        ]),
    };
    RetryingTempestProvider::new(
        FixtureProvider {
            attempts: Vec::new(),
            now_ms: RECEIVED_AT_MS,
            outcomes,
            random: VecDeque::from([0.5, 0.5]),
            sleeps: Vec::new(),
            trace,
        },
        ProviderControls {
            max_body_bytes: MAX_PROVIDER_BODY_BYTES,
            ..ProviderControls::default()
        },
        Some(RECEIVED_AT_MS + 90_000),
    )
}

/// build one exact active Tempest source and runtime identity
fn source_configuration() -> (TempestScheduledSource, TempestRuntimeConfiguration) {
    let source = TempestScheduledSource {
        active: true,
        cadence_seconds: 3_600,
        id: SOURCE_ID.to_owned(),
        material_config_identity: "tempest-material-1".to_owned(),
        provider_key: "tempest".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_config_fingerprint: FINGERPRINT.to_owned(),
        source_key: "tempest-minute".to_owned(),
        source_kind: TempestSourceKind::PhysicalSensor,
        station_slug: "garden".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    };
    let configuration = TempestRuntimeConfiguration {
        adapter_contract_version: TEMPEST_SOURCE_CONTRACT_VERSION,
        device_id: 470_937,
        location_id: 203_055,
        material_config_identity: source.material_config_identity.clone(),
        provider_key: source.provider_key.clone(),
        serial: "ST-00198967".to_owned(),
        site_slug: source.site_slug.clone(),
        source_config_fingerprint: source.source_config_fingerprint.clone(),
        source_key: source.source_key.clone(),
        source_kind: source.source_kind,
        station_slug: source.station_slug.clone(),
        timezone: source.timezone.clone(),
    };
    (source, configuration)
}

/// build one real typed Tempest observation response
fn tempest_envelope() -> TempestObservationEnvelope {
    TempestObservationEnvelope {
        bucket_step_minutes: Some(1.0),
        device_id: 470_937,
        observations: Some(vec![TempestObservation {
            air_temperature_c: Some(10.0),
            battery_volts: Some(2.7),
            epoch_seconds: WINDOW_START_MS / 1_000,
            illuminance_lux: Some(20.0),
            lightning_average_distance_km: Some(4.0),
            lightning_strike_count: Some(0.0),
            local_daily_rain_accumulation_final_mm: Some(0.0),
            local_daily_rain_accumulation_mm: Some(0.0),
            precipitation_analysis_type: Some(0.0),
            precipitation_mm: Some(0.1),
            precipitation_type: Some(0.0),
            pressure_hpa: Some(1_000.0),
            rain_accumulation_nc_mm: Some(0.0),
            relative_humidity_percent: Some(70.0),
            report_interval_minutes: Some(1.0),
            solar_radiation_wm2: Some(20.0),
            uv_index: Some(0.2),
            wind_direction_degrees: Some(180.0),
            wind_gust_mps: Some(3.0),
            wind_lull_mps: Some(0.5),
            wind_sample_interval_seconds: Some(3.0),
            wind_speed_mps: Some(1.5),
        }]),
        response_type: TempestResponseType::StationObservation,
        status_code: 0,
    }
}

/// build exact expected Tempest provider metadata
fn expected_provider_metadata() -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        (utf16("battery_volts"), JsonValue::Number(2.7)),
        (utf16("device_id"), JsonValue::Number(470_937.0)),
        (utf16("illuminance_lux"), JsonValue::Number(20.0)),
        (
            utf16("lightning_average_distance_km"),
            JsonValue::Number(4.0),
        ),
        (utf16("lightning_strike_count"), JsonValue::Number(0.0)),
        (utf16("location_id"), JsonValue::Number(203_055.0)),
        (utf16("precipitation_type"), JsonValue::Number(0.0)),
        (utf16("rain_accumulation_nc_mm"), JsonValue::Number(0.0)),
        (utf16("report_interval_minutes"), JsonValue::Number(1.0)),
        (utf16("wind_lull_mps"), JsonValue::Number(0.5)),
        (
            utf16("wind_sample_interval_seconds"),
            JsonValue::Number(3.0),
        ),
    ]
}

/// build exact expected Tempest quality metadata
fn expected_quality_metadata() -> Vec<(Vec<u16>, JsonValue)> {
    vec![(
        utf16("sampling"),
        JsonValue::String(utf16("every_distinct_provider_observation")),
    )]
}

/// build exact expected Tempest response metadata
fn expected_response_metadata() -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        (utf16("device_id"), JsonValue::Number(470_937.0)),
        (utf16("minute_record_count"), JsonValue::Number(1.0)),
        (utf16("raw_observation_count"), JsonValue::Number(1.0)),
        (utf16("bucket_step_minutes"), JsonValue::Number(1.0)),
    ]
}

/// encode one fixture string as JavaScript UTF-16 code units
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// execute one Tempest run through the real scheduled database adapter
fn execute(
    outcome: ProviderOutcome,
) -> (weather_worker::SourceRunResult, ProviderExecutionEvidence) {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut database = FixtureDatabase {
        expected_deadline: DEADLINE_AT.to_owned(),
        trace: Rc::clone(&trace),
    };
    let mut values = FixtureValues;
    let mut repository = TempestDatabaseRepository::new(&mut database, &mut values);
    let mut provider = provider_fixture(outcome, Rc::clone(&trace));
    let (source, configuration) = source_configuration();
    let result = run_scheduled_tempest_source(
        TempestScheduledExecution {
            configuration: &configuration,
            now_ms: RECEIVED_AT_MS,
            run_deadline_at_ms: DEADLINE_AT_MS,
            source: &source,
        },
        &mut repository,
        &mut provider,
        &mut || RECEIVED_AT_MS + 5_000,
    )
    .expect("Tempest database adapter should return a bounded run result");
    let evidence = ProviderExecutionEvidence {
        attempts: provider.io().attempts.clone(),
        sleeps: provider.io().sleeps.clone(),
        trace: trace.borrow().clone(),
    };
    (result, evidence)
}

/// persist one real Tempest normalization with typed cursor and exact metadata
#[test]
fn persists_normalized_tempest_atomically() {
    let (result, evidence) = execute(ProviderOutcome::Success);

    assert_eq!(result.status, SourceRunStatus::Succeeded);
    assert_eq!(result.record_count, 1);
    assert_eq!(result.attempts, 2);
    assert_eq!(result.successful_at_ms, Some(RECEIVED_AT_MS + 5_000));
    assert_eq!(
        evidence.trace,
        [
            "acquire",
            "abandon",
            "checkpoint",
            "start-lookup",
            "start-insert",
            "provider-attempt-1",
            "provider-sleep-1",
            "provider-attempt-2",
            "complete-begin",
            "complete-assert",
            "records",
            "checkpoint-update",
            "finalize-success",
            "complete-commit",
            "release",
        ]
    );
    assert_eq!(
        evidence.attempts,
        [
            ProviderAttempt {
                attempt: 1,
                max_body_bytes: 10_000_000,
                timeout_ms: 10_000,
            },
            ProviderAttempt {
                attempt: 2,
                max_body_bytes: 10_000_000,
                timeout_ms: 10_000,
            },
        ]
    );
    assert_eq!(
        evidence.sleeps,
        [ProviderSleep {
            attempts: 1,
            deadline_remaining_ms: Some(90_000),
            delay_ms: 300,
        }]
    );
}

/// finalize one Tempest provider failure before releasing the retained session
#[test]
fn finalizes_tempest_provider_failure() {
    let (result, evidence) = execute(ProviderOutcome::Failure);

    assert_eq!(result.status, SourceRunStatus::Failed);
    assert_eq!(result.reason.as_deref(), Some("provider_server_error"));
    assert_eq!(result.attempts, 3);
    assert_eq!(
        evidence.trace,
        [
            "acquire",
            "abandon",
            "checkpoint",
            "start-lookup",
            "start-insert",
            "provider-attempt-1",
            "provider-sleep-1",
            "provider-attempt-2",
            "provider-sleep-2",
            "provider-attempt-3",
            "failure-begin",
            "failure-assert",
            "failure-finalize",
            "failure-commit",
            "release",
        ]
    );
    assert_eq!(
        evidence
            .attempts
            .iter()
            .map(|attempt| (attempt.attempt, attempt.timeout_ms))
            .collect::<Vec<_>>(),
        [(1, 10_000), (2, 10_000), (3, 10_000)]
    );
    assert_eq!(
        evidence
            .sleeps
            .iter()
            .map(|sleep| sleep.delay_ms)
            .collect::<Vec<_>>(),
        [300, 550]
    );
}

struct DispatcherMaterial;

impl ScheduledSourceMaterialComparison for DispatcherMaterial {
    type Error = FixtureError;

    /// compare decoded material through the fixture's admitted representation.
    fn values_equal(&mut self, left: &JsonValue, right: &JsonValue) -> Result<bool, Self::Error> {
        Ok(left == right)
    }

    /// compare one decoded value with its ordered object projection.
    fn value_and_object_equal(
        &mut self,
        value: &JsonValue,
        object: &[(Vec<u16>, JsonValue)],
    ) -> Result<bool, Self::Error> {
        Ok(matches!(value, JsonValue::Object(entries) if entries == object))
    }
}

impl WorkerSourceMaterialIdentity for DispatcherMaterial {
    /// retain the generated Tempest adapter material identity once at construction.
    fn material_identity(&mut self, value: &JsonValue) -> Result<String, TaskFailure> {
        assert_eq!(value, &JsonValue::Object(Vec::new()));
        Ok("tempest-material-1".to_owned())
    }
}

struct DispatcherTransport {
    requests: Rc<RefCell<Vec<weather_worker::TempestObservationRequest>>>,
    trace: Rc<RefCell<Vec<String>>>,
}

impl WorkerTempestTransport for DispatcherTransport {
    type Exchange<'a>
        = FixtureProvider
    where
        Self: 'a;

    /// bind one provider exchange only after the running row is committed.
    fn bind_request(
        &mut self,
        request: &weather_worker::TempestObservationRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure> {
        self.trace.borrow_mut().push("provider-bind".to_owned());
        self.requests.borrow_mut().push(request.clone());
        Ok(FixtureProvider {
            attempts: Vec::new(),
            now_ms: RECEIVED_AT_MS,
            outcomes: VecDeque::from([
                Ok(provider_response(503, Some("temporary outage"))),
                Ok(provider_response(200, None)),
            ]),
            random: VecDeque::from([0.5]),
            sleeps: Vec::new(),
            trace: Rc::clone(&self.trace),
        })
    }
}

struct RejectOther;

impl<Repository> WorkerScheduledSourceDispatcher<Repository> for RejectOther {
    /// reject accidental fallback routing for the exact Tempest source.
    fn run_source<C: FnMut() -> i64>(
        &mut self,
        _repository: &mut Repository,
        _source: &WorkerDueSource,
        _success_clock: &mut C,
    ) -> Result<weather_worker::SourceRunResult, TaskFailure> {
        panic!("Tempest fixture must not delegate")
    }
}

/// build the generated one-station Tempest catalog used by the dispatcher.
fn dispatcher_configuration() -> TempestConfiguration {
    TempestConfiguration {
        provider: TempestConfigurationProvider {
            active: true,
            attributionLabel: "WeatherFlow".to_owned(),
            attributionUrl: "https://tempest.earth".to_owned(),
            displayName: "Tempest".to_owned(),
            key: "tempest".to_owned(),
        },
        siteKey: "ballydidean".to_owned(),
        stations: vec![TempestStationConfiguration {
            active: true,
            adapterConfig: JsonValue::Object(Vec::new()),
            cadenceSeconds: 3_600,
            deviceId: 470_937,
            displayName: "Garden".to_owned(),
            fingerprint: FINGERPRINT.to_owned(),
            historyStartDate: "2026-01-01".to_owned(),
            key: "garden".to_owned(),
            latitude: 47.95,
            locationId: 203_055,
            longitude: -122.43,
            serial: "ST-00198967".to_owned(),
            sourceKey: "tempest-minute".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }],
        version: 1,
    }
}

/// dispatch Tempest through the shared database reborrow and post-lock provider binding.
#[test]
fn dispatches_tempest_after_running_row_on_the_shared_database() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let requests = Rc::new(RefCell::new(Vec::new()));
    let mut database = FixtureDatabase {
        expected_deadline: DISPATCHER_DEADLINE_AT.to_owned(),
        trace: Rc::clone(&trace),
    };
    let mut values = FixtureValues;
    let mut repository = WorkerIterationDatabaseRepository::new(&mut database, &mut values);
    let mut dispatcher = WorkerTempestDispatcher::from_configuration(
        &dispatcher_configuration(),
        &mut DispatcherMaterial,
        Some(DispatcherTransport {
            requests: Rc::clone(&requests),
            trace: Rc::clone(&trace),
        }),
        RejectOther,
    )
    .unwrap();
    let (source, _) = source_configuration();
    let mut clock = VecDeque::from([
        RECEIVED_AT_MS - 1,
        RECEIVED_AT_MS,
        RECEIVED_AT_MS + 4_000,
        RECEIVED_AT_MS + 5_000,
    ]);
    let result = dispatcher
        .run_source(
            &mut repository,
            &WorkerDueSource::Tempest(source),
            &mut || clock.pop_front().expect("fixture clock exhausted"),
        )
        .unwrap();

    assert_eq!(result.status, SourceRunStatus::Succeeded);
    assert_eq!(result.attempts, 2);
    assert_eq!(result.successful_at_ms, Some(RECEIVED_AT_MS + 5_000));
    assert_eq!(
        requests.borrow().as_slice(),
        [weather_worker::TempestObservationRequest {
            device_id: 470_937,
            end_exclusive_ms: WINDOW_END_MS,
            location_id: 203_055,
            serial: "ST-00198967".to_owned(),
            source_id: SOURCE_ID.to_owned(),
            start_ms: WINDOW_START_MS,
            timezone: "America/Los_Angeles".to_owned(),
        }]
    );
    assert_eq!(
        trace.borrow().as_slice(),
        [
            "acquire",
            "abandon",
            "checkpoint",
            "start-lookup",
            "start-insert",
            "provider-bind",
            "provider-attempt-1",
            "provider-sleep-1",
            "provider-attempt-2",
            "complete-begin",
            "complete-assert",
            "records",
            "checkpoint-update",
            "finalize-success",
            "complete-commit",
            "release",
        ]
    );
}
