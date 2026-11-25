use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::rc::Rc;

use weather_contracts::{
    FieldPresence, IngestionError, IngestionErrorClassification, IngestionState, JsonValue,
    Nullable, PublicStationAdapter, PublicStationConfiguration, PublicStationConfigurationStation,
    PublicStationProviderConfiguration, PublicStationSourceConfiguration, ScheduledCheckpointState,
    SourceCapability, SourceKind,
};
use weather_database::{
    AssertRunningScheduledQuery, CheckpointUpdateBind, FinalizeFailedIngestionQuery,
    FinalizeScheduledRunBind, FinalizeScheduledRunQuery, FindRunningIngestionQuery,
    GetScheduledCheckpointQuery, InsertScheduledIngestionBind, InsertScheduledIngestionQuery,
    RetainedSourceSession, RunningIngestionLookup, ScheduledCheckpointDbRow,
    ScheduledCheckpointQuery, ScheduledCheckpointReadPort, ScheduledIngestionCommandPort,
    ScheduledRunFailureCommandPort, ScheduledRunIdentityQuery, ScheduledRunStartCommandPort,
    SerializedJson, Sha256Digest, StartedScheduledIngestion, WeatherRecordBind,
    WeatherRecordUpsertQuery,
};
use weather_providers::{
    ContentBudget, ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse, ProviderIo,
    ProviderSleep, ProviderSleepError,
};
use weather_runtime::{
    AmbientWeatherAttemptPayload, AmbientWeatherDatabaseRepository, AmbientWeatherRuntimeValues,
    OpenMeteoDatabaseBoundary, OpenMeteoRuntimeValues, RetryingAmbientWeatherProvider,
    RuntimePersistenceOperation, RuntimeTimeValue, WorkerAmbientWeatherDispatcher,
    WorkerAmbientWeatherTransport, WorkerIterationDatabasePort, WorkerIterationDatabaseRepository,
    WorkerSourceMaterialIdentity,
};
use weather_worker::{
    AMBIENT_WEATHER_ADAPTER_VERSION, AMBIENT_WEATHER_DEVICE_DATA_ENDPOINT,
    AmbientWeatherCapability, AmbientWeatherDeviceIdentity, AmbientWeatherObservation,
    AmbientWeatherPayload, AmbientWeatherProviderCursor, AmbientWeatherRuntimeConfiguration,
    AmbientWeatherScheduledExecution, AmbientWeatherScheduledSource, AmbientWeatherSourceKind,
    OpenMeteoCursor, ScheduledSourceMaterialComparison, SourceRunResult, SourceRunStatus,
    TaskFailure, WorkerContinuation, WorkerDueSource, WorkerHeartbeat,
    WorkerScheduledSourceDispatcher, run_scheduled_ambient_weather_source, run_worker_iteration,
};

const SOURCE_ID: &str = "source-ambient-weather";
const DEVICE_ID: &str = "5fe7d16a25083b000171f8e5";
const FINGERPRINT: &str = "ambient-weather-fingerprint-1";
const CHECKPOINT_CURSOR_MS: i64 = 1_786_716_000_000;
const WINDOW_START_MS: i64 = 1_786_975_200_000;
const VALID_AT_MS: i64 = 1_787_576_400_000;
const NOW_MS: i64 = 1_787_580_000_000;
const DEADLINE_MS: i64 = 1_787_580_600_000;
const DISPATCH_DEADLINE_MS: i64 = NOW_MS + 120_000;
const RECEIVED_AT_MS: i64 = 1_787_583_600_000;
const CHECKPOINT_CURSOR: &str = "2026-08-14T14:00:00.000Z";
const WINDOW_START: &str = "2026-08-17T14:00:00.000Z";
const VALID_AT: &str = "2026-08-24T13:00:00.000Z";
const NOW: &str = "2026-08-24T14:00:00.000Z";
const DEADLINE: &str = "2026-08-24T14:10:00.000Z";
const DISPATCH_DEADLINE: &str = "2026-08-24T14:02:00.000Z";
const RECEIVED_AT: &str = "2026-08-24T15:00:00.000Z";
const RESPONSE_CHECKSUM: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const CONTENT_HASH: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const PROVIDER_JSON: &str =
    r#"{"dataset":"public-device-data","device_id":"5fe7d16a25083b000171f8e5"}"#;
const QUALITY_JSON: &str = r#"{"sampling":"every_distinct_provider_observation"}"#;
const RESPONSE_JSON: &str = r#"{"normalized_record_count":1,"provider":"ambient-weather-network","raw_observation_count":2}"#;
const CURSOR_JSON: &str = r#"{"valid_at":"2026-08-24T13:00:00.000Z"}"#;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// render one fixture database failure
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

#[derive(Default)]
struct CapturedCommands {
    checkpoint: Option<ScheduledCheckpointQuery>,
    failure: Option<FinalizeFailedIngestionQuery>,
    finalize: Option<FinalizeScheduledRunQuery>,
    insert: Option<InsertScheduledIngestionQuery>,
    records: Option<WeatherRecordUpsertQuery>,
    trace: Vec<String>,
}

struct FixtureSession {
    commands: Rc<RefCell<CapturedCommands>>,
}

impl RetainedSourceSession for FixtureSession {
    /// return the retained Ambient source identity
    fn source_id(&self) -> &str {
        SOURCE_ID
    }
}

impl ScheduledCheckpointReadPort for FixtureSession {
    type Error = FixtureError;

    /// return the exact typed Ambient checkpoint row
    fn get_scheduled_checkpoint(
        &mut self,
        _query: &GetScheduledCheckpointQuery,
    ) -> Result<Option<ScheduledCheckpointDbRow>, Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("checkpoint".to_owned());
        Ok(Some(ScheduledCheckpointDbRow {
            last_committed_at: CHECKPOINT_CURSOR.to_owned(),
            last_valid_at: CHECKPOINT_CURSOR.to_owned(),
            provider_cursor: Nullable::Value(vec![(
                utf16("valid_at"),
                JsonValue::String(utf16(CHECKPOINT_CURSOR)),
            )]),
            source_id: SOURCE_ID.to_owned(),
            version: 11.0,
            window_end_exclusive: CHECKPOINT_CURSOR.to_owned(),
            window_start: CHECKPOINT_CURSOR.to_owned(),
        }))
    }
}

impl ScheduledRunStartCommandPort for FixtureSession {
    type Error = FixtureError;

    /// capture the Ambient running-row lookup
    fn find_running_ingestion(
        &mut self,
        _query: &FindRunningIngestionQuery,
    ) -> Result<RunningIngestionLookup, Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("start-lookup".to_owned());
        Ok(RunningIngestionLookup {
            first_run_id: None,
            row_count: 0,
        })
    }

    /// capture one committed Ambient running row
    fn insert_scheduled_ingestion(
        &mut self,
        query: &InsertScheduledIngestionQuery,
    ) -> Result<Option<StartedScheduledIngestion>, Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("start-insert".to_owned());
        commands.insert = Some(query.clone());
        Ok(Some(StartedScheduledIngestion {
            id: "run-ambient".to_owned(),
            started_at: NOW.to_owned(),
            state: IngestionState::Running,
        }))
    }
}

impl ScheduledIngestionCommandPort for FixtureSession {
    type Error = FixtureError;

    /// record one atomic Ambient completion start
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("complete-begin".to_owned());
        Ok(())
    }

    /// capture the running Ambient row assertion
    fn assert_scheduled_run(
        &mut self,
        _query: &ScheduledRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("complete-assert".to_owned());
        Ok(1)
    }

    /// capture actual normalized Ambient storage binds
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("records".to_owned());
        commands.records = Some(query.clone());
        Ok(())
    }

    /// capture typed Ambient cursor advancement
    fn advance_scheduled_checkpoint(
        &mut self,
        query: &ScheduledCheckpointQuery,
    ) -> Result<usize, Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("checkpoint-update".to_owned());
        commands.checkpoint = Some(query.clone());
        Ok(1)
    }

    /// capture exact Ambient response finalization
    fn finalize_scheduled_run(
        &mut self,
        query: &FinalizeScheduledRunQuery,
    ) -> Result<usize, Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("finalize-success".to_owned());
        commands.finalize = Some(query.clone());
        Ok(1)
    }

    /// record successful Ambient completion commit
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("complete-commit".to_owned());
        Ok(())
    }

    /// record unexpected Ambient completion rollback
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("complete-rollback".to_owned());
        Ok(())
    }
}

impl ScheduledRunFailureCommandPort for FixtureSession {
    type Error = FixtureError;

    /// record Ambient failure finalization start
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("failure-begin".to_owned());
        Ok(())
    }

    /// capture one failed Ambient run assertion
    fn assert_running_scheduled(
        &mut self,
        _query: &AssertRunningScheduledQuery,
    ) -> Result<usize, Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("failure-assert".to_owned());
        Ok(1)
    }

    /// capture typed Ambient provider failure finalization
    fn finalize_failed_ingestion(
        &mut self,
        query: &FinalizeFailedIngestionQuery,
    ) -> Result<usize, Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("failure-finalize".to_owned());
        commands.failure = Some(query.clone());
        Ok(1)
    }

    /// record Ambient failure commit
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("failure-commit".to_owned());
        Ok(())
    }

    /// record unexpected Ambient failure rollback
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("failure-rollback".to_owned());
        Ok(())
    }
}

struct FixtureDatabase {
    commands: Rc<RefCell<CapturedCommands>>,
    discover_at_ms: Vec<i64>,
    due_sources: Vec<WorkerDueSource>,
    heartbeats: Vec<WorkerHeartbeat>,
}

impl WorkerIterationDatabasePort for FixtureDatabase {
    type Error = FixtureError;

    /// Return due sources from the same database that owns Ambient sessions.
    fn discover_due_sources(&mut self, as_of_ms: i64) -> Result<Vec<WorkerDueSource>, Self::Error> {
        self.commands.borrow_mut().trace.push("discover".to_owned());
        self.discover_at_ms.push(as_of_ms);
        Ok(std::mem::take(&mut self.due_sources))
    }

    /// Retain the ordinary heartbeat on the shared fixture database.
    fn update_worker_heartbeat(&mut self, heartbeat: WorkerHeartbeat) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("heartbeat".to_owned());
        self.heartbeats.push(heartbeat);
        Ok(())
    }
}

impl OpenMeteoDatabaseBoundary for FixtureDatabase {
    type Session = FixtureSession;

    /// acquire one retained Ambient source session
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure> {
        assert_eq!(source_id, SOURCE_ID);
        self.commands.borrow_mut().trace.push("acquire".to_owned());
        Ok(Some(FixtureSession {
            commands: Rc::clone(&self.commands),
        }))
    }

    /// capture Ambient stale-run cleanup
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        as_of: &str,
    ) -> Result<(), TaskFailure> {
        assert_eq!(session.source_id(), SOURCE_ID);
        assert_eq!(as_of, NOW);
        self.commands.borrow_mut().trace.push("abandon".to_owned());
        Ok(())
    }

    /// release the retained Ambient session
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
        assert_eq!(session.source_id(), SOURCE_ID);
        self.commands.borrow_mut().trace.push("release".to_owned());
        Ok(())
    }
}

struct FixtureValues;

impl OpenMeteoRuntimeValues for FixtureValues {
    /// resolve only admitted Ambient fixture timestamps
    fn utc_instant(
        &mut self,
        milliseconds: i64,
        _value: RuntimeTimeValue,
    ) -> Result<String, TaskFailure> {
        let instant = match milliseconds {
            // map every exact Ambient fixture instant
            CHECKPOINT_CURSOR_MS => CHECKPOINT_CURSOR,
            WINDOW_START_MS => WINDOW_START,
            VALID_AT_MS => VALID_AT,
            NOW_MS => NOW,
            DEADLINE_MS => DEADLINE,
            DISPATCH_DEADLINE_MS => DISPATCH_DEADLINE,
            RECEIVED_AT_MS => RECEIVED_AT,
            _ => panic!("unexpected Ambient UTC boundary: {milliseconds}"),
        };
        Ok(instant.to_owned())
    }

    /// decode the exact Ambient checkpoint window
    fn epoch_milliseconds(
        &mut self,
        instant: &str,
        value: RuntimeTimeValue,
    ) -> Result<i64, TaskFailure> {
        assert_eq!(instant, CHECKPOINT_CURSOR);
        assert_eq!(value, RuntimeTimeValue::CheckpointWindowEnd);
        Ok(CHECKPOINT_CURSOR_MS)
    }

    /// serialize exact Ambient provider quality and response objects
    fn serialize_json_object(
        &mut self,
        value: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure> {
        let first_key = &value
            .first()
            .expect("serialized Ambient object should not be empty")
            .0;

        // select the exact object contract by its first ordered key
        if *first_key == utf16("dataset") {
            assert_eq!(
                value,
                [
                    (utf16("dataset"), json_string("public-device-data")),
                    (utf16("device_id"), json_string(DEVICE_ID)),
                ]
            );
            return Ok(SerializedJson::admitted(PROVIDER_JSON));
        }

        // preserve exact Ambient quality evidence
        if *first_key == utf16("sampling") {
            assert_eq!(
                value,
                [(
                    utf16("sampling"),
                    json_string("every_distinct_provider_observation")
                )]
            );
            return Ok(SerializedJson::admitted(QUALITY_JSON));
        }

        assert_eq!(
            value,
            [
                (utf16("normalized_record_count"), JsonValue::Number(1.0)),
                (utf16("provider"), json_string("ambient-weather-network")),
                (utf16("raw_observation_count"), JsonValue::Number(2.0)),
            ]
        );
        Ok(SerializedJson::admitted(RESPONSE_JSON))
    }

    /// serialize the exact Ambient endpoint evidence
    fn request_metadata_json(&mut self, endpoint: &str) -> Result<SerializedJson, TaskFailure> {
        assert_eq!(endpoint, AMBIENT_WEATHER_DEVICE_DATA_ENDPOINT);
        Ok(SerializedJson::admitted(r#"{"endpoint":"device-data"}"#))
    }

    /// reject the OpenMeteo string cursor path for Ambient
    fn provider_cursor_json(
        &mut self,
        _cursor: Option<&OpenMeteoCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        panic!("Ambient must not use the OpenMeteo cursor mapper")
    }

    /// reject the OpenMeteo checkpoint cursor path for Ambient
    fn checkpoint_cursor(
        &mut self,
        _checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<OpenMeteoCursor>, TaskFailure> {
        panic!("Ambient must not use the OpenMeteo checkpoint mapper")
    }

    /// admit the actual normalized Ambient record hash
    fn weather_record_content_hash(
        &mut self,
        record: &weather_contracts::NormalizedWeatherRecord,
    ) -> Result<Sha256Digest, TaskFailure> {
        assert_eq!(record.sourceId, SOURCE_ID);
        assert_eq!(record.sourceKind, SourceKind::PhysicalSensor);
        assert_eq!(record.validAt, VALID_AT);
        assert_eq!(record.receivedAt, RECEIVED_AT);
        // require the normalized converted temperature
        let Nullable::Value(temperature_c) = record.metrics.temperatureC else {
            panic!("Ambient temperature should be present");
        };
        assert!((temperature_c - 12.0).abs() < 1e-9);
        assert_eq!(
            record.metadata.device,
            Nullable::Value(weather_contracts::DeviceMetadata {
                model: FieldPresence::Value("AMBWeatherV4.3.6".to_owned()),
                serial: FieldPresence::Value(DEVICE_ID.to_owned()),
                vendor: FieldPresence::Value("Ambient Weather".to_owned()),
            })
        );
        Ok(Sha256Digest::admitted(CONTENT_HASH))
    }

    /// admit the Ambient fingerprint and response checksum
    fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure> {
        assert!(value == FINGERPRINT || value == RESPONSE_CHECKSUM);
        Ok(Sha256Digest::admitted(value))
    }

    /// map one Ambient provider failure through the shared domain boundary
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

impl AmbientWeatherRuntimeValues for FixtureValues {
    /// encode the typed Ambient cursor as Node's exact valid_at object
    fn ambient_weather_provider_cursor_json(
        &mut self,
        cursor: Option<&AmbientWeatherProviderCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        assert_eq!(
            cursor,
            Some(&AmbientWeatherProviderCursor {
                valid_at_ms: VALID_AT_MS,
            })
        );
        Ok(Some(SerializedJson::admitted(CURSOR_JSON)))
    }

    /// decode Node's exact valid_at object into the typed Ambient cursor
    fn ambient_weather_checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<AmbientWeatherProviderCursor>, TaskFailure> {
        assert_eq!(checkpoint.sourceId, SOURCE_ID);
        assert_eq!(
            checkpoint.providerCursor,
            Nullable::Value(vec![(
                utf16("valid_at"),
                JsonValue::String(utf16(CHECKPOINT_CURSOR)),
            )])
        );
        Ok(Some(AmbientWeatherProviderCursor {
            valid_at_ms: CHECKPOINT_CURSOR_MS,
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
    commands: Rc<RefCell<CapturedCommands>>,
    now_ms: i64,
    outcomes: VecDeque<
        Result<ProviderAttemptResponse<AmbientWeatherAttemptPayload>, ProviderAttemptError>,
    >,
    random: VecDeque<f64>,
    sleeps: Vec<ProviderSleep>,
}

impl ProviderIo<AmbientWeatherAttemptPayload> for FixtureProvider {
    /// return the deterministic Ambient provider clock
    fn now_ms(&mut self) -> i64 {
        self.now_ms
    }

    /// return one deterministic Ambient retry jitter input
    fn random_unit(&mut self) -> f64 {
        self.random.pop_front().unwrap_or(0.0)
    }

    /// retain one physical Ambient attempt and return its typed response
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<AmbientWeatherAttemptPayload>, ProviderAttemptError> {
        self.commands
            .borrow_mut()
            .trace
            .push(format!("provider-attempt-{}", attempt.attempt));
        self.attempts.push(attempt);
        self.outcomes
            .pop_front()
            .unwrap_or_else(|| panic!("Ambient provider response missing"))
    }

    /// retain and apply one exact Ambient retry sleep
    fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
        self.commands
            .borrow_mut()
            .trace
            .push(format!("provider-sleep-{}", sleep.attempts));
        self.sleeps.push(sleep);
        self.now_ms += i64::try_from(sleep.delay_ms).expect("Ambient delay should fit i64");
        Ok(())
    }
}

/// physical Ambient controller evidence after one scheduled run
struct ProviderExecutionEvidence {
    attempts: Vec<ProviderAttempt>,
    sleeps: Vec<ProviderSleep>,
}

/// build typed Ambient payload after admitted transport parsing
fn ambient_attempt_payload() -> AmbientWeatherAttemptPayload {
    AmbientWeatherAttemptPayload {
        payload: ambient_payload(),
        received_at_ms: RECEIVED_AT_MS,
        upstream_response_checksum: RESPONSE_CHECKSUM.to_owned(),
    }
}

/// build Ambient response with ten-megabyte budget evidence
fn provider_response(
    status: u16,
    reason: Option<&str>,
) -> ProviderAttemptResponse<AmbientWeatherAttemptPayload> {
    ProviderAttemptResponse {
        body: ambient_attempt_payload(),
        content: ContentBudget {
            declared_body_bytes: Some(2_500_000),
            streamed_body_bytes: 2_500_000,
        },
        provider_reason: reason.map(str::to_owned),
        retry_after_ms: None,
        status,
    }
}

/// build controller-backed Ambient adapter
fn provider_fixture(
    outcome: ProviderOutcome,
    commands: Rc<RefCell<CapturedCommands>>,
) -> RetryingAmbientWeatherProvider<FixtureProvider> {
    RetryingAmbientWeatherProvider::new(
        provider_io_fixture(outcome, commands),
        Some(NOW_MS + 90_000),
    )
}

/// Build raw Ambient IO shared by direct and dispatcher fixtures.
fn provider_io_fixture(
    outcome: ProviderOutcome,
    commands: Rc<RefCell<CapturedCommands>>,
) -> FixtureProvider {
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
    FixtureProvider {
        attempts: Vec::new(),
        commands,
        now_ms: NOW_MS,
        outcomes,
        random: VecDeque::from([0.5, 0.5]),
        sleeps: Vec::new(),
    }
}

struct DispatcherMaterial;

impl ScheduledSourceMaterialComparison for DispatcherMaterial {
    type Error = FixtureError;

    /// Compare decoded material without introducing a fixture JSON codec.
    fn values_equal(&mut self, left: &JsonValue, right: &JsonValue) -> Result<bool, Self::Error> {
        Ok(left == right)
    }

    /// Compare one decoded value with its generated ordered object.
    fn value_and_object_equal(
        &mut self,
        value: &JsonValue,
        object: &[(Vec<u16>, JsonValue)],
    ) -> Result<bool, Self::Error> {
        Ok(matches!(value, JsonValue::Object(entries) if entries == object))
    }
}

impl WorkerSourceMaterialIdentity for DispatcherMaterial {
    /// Return the exact identity shared with the active due source.
    fn material_identity(&mut self, value: &JsonValue) -> Result<String, TaskFailure> {
        assert_eq!(value, &JsonValue::Object(ambient_adapter_config()));
        Ok("ambient-weather-material-1".to_owned())
    }
}

struct DispatcherTransport {
    bindings: Rc<RefCell<Vec<weather_worker::AmbientWeatherRangeRequest>>>,
    commands: Rc<RefCell<CapturedCommands>>,
}

impl WorkerAmbientWeatherTransport for DispatcherTransport {
    type Exchange<'a> = FixtureProvider;

    /// Bind the typed request after the durable Ambient row exists.
    fn bind_request(
        &mut self,
        request: &weather_worker::AmbientWeatherRangeRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure> {
        self.commands
            .borrow_mut()
            .trace
            .push("provider-bind".to_owned());
        self.bindings.borrow_mut().push(request.clone());
        Ok(provider_io_fixture(
            ProviderOutcome::Success,
            Rc::clone(&self.commands),
        ))
    }
}

struct NoOtherDispatcher;

impl<Repository> WorkerScheduledSourceDispatcher<Repository> for NoOtherDispatcher {
    /// Reject source families outside this Ambient fixture.
    fn run_source<Clock: FnMut() -> i64>(
        &mut self,
        _repository: &mut Repository,
        source: &WorkerDueSource,
        _clock: &mut Clock,
    ) -> Result<SourceRunResult, TaskFailure> {
        panic!("unexpected delegated source: {}", source.source_id())
    }
}

/// Return the exact generated ordered Ambient adapter material.
fn ambient_adapter_config() -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        (utf16("deviceId"), JsonValue::String(utf16(DEVICE_ID))),
        (
            utf16("macAddress"),
            JsonValue::String(utf16("F0:08:D1:07:B3:9E")),
        ),
    ]
}

/// Build the generated public-station catalog consumed by dispatch.
fn generated_configuration() -> PublicStationConfiguration {
    PublicStationConfiguration {
        siteKey: "ballydidean".to_owned(),
        providers: vec![PublicStationProviderConfiguration {
            active: false,
            attributionLabel: "Ambient".to_owned(),
            attributionUrl: "https://ambientweather.net".to_owned(),
            displayName: "Ambient Weather Network".to_owned(),
            key: "ambient-weather-network".to_owned(),
        }],
        stations: vec![PublicStationConfigurationStation {
            active: false,
            displayName: "Ambient Merlin".to_owned(),
            key: "ambient-merlin".to_owned(),
            latitude: 37.0,
            longitude: -122.0,
            model: Nullable::Value("AMBWeatherV4.3.6".to_owned()),
            serial: Nullable::Value(DEVICE_ID.to_owned()),
            sources: vec![PublicStationSourceConfiguration {
                active: true,
                adapter: PublicStationAdapter::AmbientWeather,
                adapterConfig: ambient_adapter_config(),
                cadenceSeconds: Nullable::Value(3_600),
                capabilities: vec![SourceCapability::Current, SourceCapability::Historical],
                fingerprint: FINGERPRINT.to_owned(),
                historyEndDate: Nullable::Null,
                historyStartDate: Nullable::Value("2026-01-01".to_owned()),
                key: "ambient-merlin-observations-v1".to_owned(),
                maximumChunkDays: 7,
                providerKey: "ambient-weather-network".to_owned(),
            }],
            timezone: "America/Los_Angeles".to_owned(),
            vendor: Nullable::Value("Ambient Weather".to_owned()),
        }],
        version: 1,
    }
}

/// build the exact active Ambient source and configuration
fn source_configuration() -> (
    AmbientWeatherScheduledSource,
    AmbientWeatherRuntimeConfiguration,
) {
    let source = AmbientWeatherScheduledSource {
        active: true,
        cadence_seconds: 3_600,
        id: SOURCE_ID.to_owned(),
        material_config_identity: "ambient-weather-material-1".to_owned(),
        provider_key: "ambient-weather-network".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_config_fingerprint: FINGERPRINT.to_owned(),
        source_key: "ambient-merlin-observations-v1".to_owned(),
        source_kind: AmbientWeatherSourceKind::PhysicalSensor,
        station_slug: "ambient-merlin".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    };
    let configuration = AmbientWeatherRuntimeConfiguration {
        adapter_contract_version: AMBIENT_WEATHER_ADAPTER_VERSION,
        capabilities: BTreeSet::from([
            AmbientWeatherCapability::Current,
            AmbientWeatherCapability::Historical,
        ]),
        device_id: DEVICE_ID.to_owned(),
        mac_address: "F0:08:D1:07:B3:9E".to_owned(),
        material_config_identity: source.material_config_identity.clone(),
        maximum_chunk_days: 7,
        model: Some("AMBWeatherV4.3.6".to_owned()),
        provider_key: source.provider_key.clone(),
        serial: Some(DEVICE_ID.to_owned()),
        site_slug: source.site_slug.clone(),
        source_active: true,
        source_config_fingerprint: source.source_config_fingerprint.clone(),
        source_key: source.source_key.clone(),
        source_kind: source.source_kind,
        station_slug: source.station_slug.clone(),
        timezone: source.timezone.clone(),
    };
    (source, configuration)
}

/// build one real typed Ambient payload with an exclusive-end row
fn ambient_payload() -> AmbientWeatherPayload {
    let observation = |date_utc_ms| AmbientWeatherObservation {
        apparent_temperature_f: Some(54.0),
        date_utc_ms,
        device_identity: AmbientWeatherDeviceIdentity::Value(DEVICE_ID.to_owned()),
        hourly_rain_in: Some(0.1),
        pressure_in_hg: Some(30.0),
        relative_humidity_percent: Some(90.0),
        solar_radiation_wm2: Some(100.0),
        temperature_f: Some(53.6),
        uv_index: Some(1.0),
        wind_direction_degrees: Some(360.0),
        wind_gust_mph: Some(10.0),
        wind_speed_mph: Some(5.0),
    };
    AmbientWeatherPayload {
        observations: vec![observation(VALID_AT_MS), observation(NOW_MS)],
    }
}

/// execute one Ambient run through the shared database adapter
fn execute(
    outcome: ProviderOutcome,
) -> (
    weather_worker::SourceRunResult,
    Rc<RefCell<CapturedCommands>>,
    ProviderExecutionEvidence,
) {
    let commands = Rc::new(RefCell::new(CapturedCommands::default()));
    let mut database = FixtureDatabase {
        commands: Rc::clone(&commands),
        discover_at_ms: Vec::new(),
        due_sources: Vec::new(),
        heartbeats: Vec::new(),
    };
    let mut values = FixtureValues;
    let mut repository = AmbientWeatherDatabaseRepository::new(&mut database, &mut values);
    let mut provider = provider_fixture(outcome, Rc::clone(&commands));
    let (source, configuration) = source_configuration();
    let result = run_scheduled_ambient_weather_source(
        AmbientWeatherScheduledExecution {
            configuration: &configuration,
            now_ms: NOW_MS,
            run_deadline_at_ms: DEADLINE_MS,
            source: &source,
        },
        &mut repository,
        &mut provider,
        &mut || NOW_MS + 5_000,
    )
    .expect("Ambient database adapter should return a bounded run result");
    let evidence = ProviderExecutionEvidence {
        attempts: provider.io().attempts.clone(),
        sleeps: provider.io().sleeps.clone(),
    };
    (result, commands, evidence)
}

/// Run Ambient discovery, durable ingestion, and heartbeat through one database.
#[test]
fn dispatches_ambient_through_shared_iteration_database() {
    let commands = Rc::new(RefCell::new(CapturedCommands::default()));
    let bindings = Rc::new(RefCell::new(Vec::new()));
    let (source, _) = source_configuration();
    let mut database = FixtureDatabase {
        commands: Rc::clone(&commands),
        discover_at_ms: Vec::new(),
        due_sources: vec![WorkerDueSource::Ambient(source)],
        heartbeats: Vec::new(),
    };
    let mut values = FixtureValues;
    let mut material = DispatcherMaterial;
    let mut dispatcher = WorkerAmbientWeatherDispatcher::from_configuration(
        &generated_configuration(),
        &mut material,
        Some(DispatcherTransport {
            bindings: Rc::clone(&bindings),
            commands: Rc::clone(&commands),
        }),
        NoOtherDispatcher,
    )
    .expect("Ambient dispatcher configuration should load");
    let mut continuation = WorkerContinuation::new(None);
    let mut clocks = VecDeque::from([
        NOW_MS - 3_000,
        NOW_MS - 2_000,
        NOW_MS - 1_000,
        NOW_MS,
        NOW_MS + 5_000,
        NOW_MS + 6_000,
        NOW_MS + 7_000,
    ]);
    let result = {
        let mut repository = WorkerIterationDatabaseRepository::new(&mut database, &mut values);
        run_worker_iteration(
            &mut continuation,
            &mut repository,
            &mut dispatcher,
            &mut || {
                clocks
                    .pop_front()
                    .expect("Ambient dispatcher clock should exist")
            },
        )
        .expect("Ambient iteration should complete")
    };

    assert!(clocks.is_empty());
    assert_eq!(database.discover_at_ms, [NOW_MS - 2_000]);
    assert_eq!(result.sources.len(), 1);
    assert_eq!(result.sources[0].status, SourceRunStatus::Succeeded);
    assert_eq!(result.sources[0].attempts, 2);
    assert_eq!(result.sources[0].record_count, 1);
    assert_eq!(result.sources[0].successful_at_ms, Some(NOW_MS + 6_000));
    assert_eq!(result.last_success_at_ms, Some(NOW_MS + 6_000));
    assert_eq!(result.completed_at_ms, NOW_MS + 7_000);
    assert_eq!(
        database.heartbeats,
        [WorkerHeartbeat {
            activity: None,
            last_loop_at_ms: NOW_MS + 7_000,
            last_success_at_ms: Some(NOW_MS + 6_000),
        }]
    );
    let bindings = bindings.borrow();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].device_id, DEVICE_ID);
    assert_eq!(bindings[0].mac_address, "F0:08:D1:07:B3:9E");
    assert_eq!(bindings[0].model.as_deref(), Some("AMBWeatherV4.3.6"));
    assert_eq!(bindings[0].serial.as_deref(), Some(DEVICE_ID));
    assert_eq!(bindings[0].source_id, SOURCE_ID);
    assert_eq!(bindings[0].start_ms, WINDOW_START_MS);
    assert_eq!(bindings[0].end_exclusive_ms, NOW_MS);
    drop(bindings);
    let commands = commands.borrow();
    let insert = commands.insert.as_ref().expect("run should start durably");
    assert_eq!(
        insert.binds[7],
        InsertScheduledIngestionBind::UtcInstant(DISPATCH_DEADLINE.to_owned())
    );
    assert_eq!(
        commands.trace,
        [
            "discover",
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
            "heartbeat",
        ]
    );
}

/// persist one real Ambient normalization with typed cursor and exact evidence
#[test]
fn persists_normalized_ambient_atomically() {
    let (result, commands, evidence) = execute(ProviderOutcome::Success);
    assert_eq!(result.status, SourceRunStatus::Succeeded);
    assert_eq!(result.record_count, 1);
    assert_eq!(result.attempts, 2);
    let commands = commands.borrow();
    assert_eq!(
        commands.trace,
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
    let insert = commands.insert.as_ref().expect("start should persist");
    assert_eq!(
        insert.binds[2],
        InsertScheduledIngestionBind::UtcInstant(WINDOW_START.to_owned())
    );
    assert_eq!(
        insert.binds[3],
        InsertScheduledIngestionBind::UtcInstant(NOW.to_owned())
    );
    assert_eq!(
        insert.binds[5],
        InsertScheduledIngestionBind::Text(AMBIENT_WEATHER_ADAPTER_VERSION.to_owned())
    );
    assert_eq!(
        insert.binds[7],
        InsertScheduledIngestionBind::UtcInstant(DEADLINE.to_owned())
    );
    let records = commands.records.as_ref().expect("record should persist");
    assert_eq!(records.record_count, 1);
    assert_eq!(
        records.binds[1],
        WeatherRecordBind::SourceKind(SourceKind::PhysicalSensor)
    );
    assert_eq!(
        records.binds[8],
        WeatherRecordBind::NullableText(Some("Ambient Weather".to_owned()))
    );
    assert_eq!(
        records.binds[9],
        WeatherRecordBind::NullableText(Some("AMBWeatherV4.3.6".to_owned()))
    );
    assert_eq!(
        records.binds[10],
        WeatherRecordBind::NullableText(Some(DEVICE_ID.to_owned()))
    );
    assert_eq!(
        records.binds[11],
        WeatherRecordBind::NullableJson(Some(SerializedJson::admitted(QUALITY_JSON)))
    );
    assert_eq!(
        records.binds[12],
        WeatherRecordBind::NullableJson(Some(SerializedJson::admitted(PROVIDER_JSON)))
    );
    // require the exact converted temperature bind
    let WeatherRecordBind::NullableNumber(Some(temperature_c)) = records.binds[13] else {
        panic!("Ambient temperature bind should be present");
    };
    assert!((temperature_c - 12.0).abs() < 1e-9);
    let checkpoint = commands
        .checkpoint
        .as_ref()
        .expect("checkpoint should advance");
    match checkpoint {
        // verify the exact typed cursor and final-record checkpoint
        ScheduledCheckpointQuery::Update { binds, .. } => {
            assert_eq!(binds[1], CheckpointUpdateBind::Version(11));
            assert_eq!(
                binds[2],
                CheckpointUpdateBind::UtcInstant(VALID_AT.to_owned())
            );
            assert_eq!(
                binds[5],
                CheckpointUpdateBind::NullableJson(Some(SerializedJson::admitted(CURSOR_JSON)))
            );
        }
        ScheduledCheckpointQuery::Insert { .. } => panic!("fixture must update its checkpoint"),
    }
    let finalize = commands.finalize.as_ref().expect("run should finalize");
    assert_eq!(
        finalize.binds[4],
        FinalizeScheduledRunBind::NullableJson(Some(SerializedJson::admitted(RESPONSE_JSON)))
    );
}

/// finalize one Ambient provider failure through the shared lifecycle
#[test]
fn finalizes_ambient_provider_failure() {
    let (result, commands, evidence) = execute(ProviderOutcome::Failure);
    assert_eq!(result.status, SourceRunStatus::Failed);
    assert_eq!(result.reason.as_deref(), Some("provider_server_error"));
    assert_eq!(result.attempts, 3);
    let commands = commands.borrow();
    assert!(commands.failure.is_some());
    assert_eq!(commands.trace.last().map(String::as_str), Some("release"));
    assert_eq!(
        evidence
            .attempts
            .iter()
            .map(|attempt| (attempt.attempt, attempt.max_body_bytes, attempt.timeout_ms))
            .collect::<Vec<_>>(),
        [
            (1, 10_000_000, 10_000),
            (2, 10_000_000, 10_000),
            (3, 10_000_000, 10_000),
        ]
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

/// encode one JSON fixture string
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(utf16(value))
}

/// encode one fixture string as JavaScript UTF-16 code units
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}
