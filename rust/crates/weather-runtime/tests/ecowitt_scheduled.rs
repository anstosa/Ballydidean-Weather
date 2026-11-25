use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::rc::Rc;

use weather_contracts::{
    EcowittConfiguration, EcowittConfigurationProvider, EcowittStationConfiguration, FieldPresence,
    IngestionError, IngestionErrorClassification, IngestionState, JsonValue, Nullable,
    ScheduledCheckpointState, SourceKind,
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
    EcowittAttemptPayload, EcowittDatabaseRepository, EcowittParsedProduct, EcowittProviderIo,
    EcowittProviderStage, EcowittRuntimeValues, OpenMeteoDatabaseBoundary, OpenMeteoRuntimeValues,
    RetryingEcowittProvider, RuntimePersistenceOperation, RuntimeTimeValue,
    WorkerEcowittBindResult, WorkerEcowittDispatcher, WorkerEcowittExchange,
    WorkerEcowittTransport, WorkerIterationDatabasePort, WorkerIterationDatabaseRepository,
    WorkerSourceMaterialIdentity,
};
use weather_worker::{
    ECOWITT_LOCAL_LIVE_ADAPTER_VERSION, ECOWITT_LOCAL_LIVE_ENDPOINT, EcowittAdmittedLocalDate,
    EcowittCanonicalMetadataAdapter, EcowittCanonicalMetadataMeasurement, EcowittCapability,
    EcowittLivePayload, EcowittMeasurement, EcowittProviderCursor, EcowittProviderMetadata,
    EcowittRainReadings, EcowittRuntimeConfiguration, EcowittScheduledExecution,
    EcowittScheduledRequest, EcowittScheduledSource, EcowittSourceKind, EcowittTimeAdapter,
    EcowittUnit, OpenMeteoCursor, ScheduledSourceMaterialComparison, SourceRunResult,
    SourceRunStatus, TaskFailure, WorkerContinuation, WorkerDueSource, WorkerHeartbeat,
    WorkerScheduledSourceDispatcher, run_scheduled_ecowitt_source, run_worker_iteration,
};

const SOURCE_ID: &str = "source-ecowitt";
const MAC: &str = "88:F1:55:05:D8:9F";
const START_MS: i64 = 1_788_011_940_000;
const NOW_MS: i64 = 1_788_012_000_000;
const DEADLINE_MS: i64 = 1_788_012_600_000;
const DISPATCH_DEADLINE_MS: i64 = NOW_MS + 120_000;
const START: &str = "2026-08-29T13:59:00.000Z";
const NOW: &str = "2026-08-29T14:00:00.000Z";
const DEADLINE: &str = "2026-08-29T14:10:00.000Z";
const DISPATCH_DEADLINE: &str = "2026-08-29T14:02:00.000Z";
const RESPONSE_CHECKSUM: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CONTENT_HASH: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PROVIDER_JSON: &str = r#"{"dataset":"get_livedata_info","device_id":"88:F1:55:05:D8:9F","property_sensors":[{"channel":null,"key":"weather-array","model":"WS90","readings":{"dailyPrecipitationMm":3.8099999999999996}}]}"#;
const QUALITY_JSON: &str = r#"{"sampling":"local_gateway_poll"}"#;
const RESPONSE_JSON: &str = r#"{"gateway_mac":"88:F1:55:05:D8:9F","group_count":2,"rain_gauge":"piezo","soil_channel_count":0,"temperature_humidity_channel_count":0}"#;
const CURSOR_JSON: &str = r#"{"rain_daily_total_mm":3.8099999999999996,"rain_day":"2026-08-29","valid_at":"2026-08-29T14:00:00.000Z"}"#;

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
    /// return the retained Ecowitt source identity
    fn source_id(&self) -> &str {
        SOURCE_ID
    }
}

impl ScheduledCheckpointReadPort for FixtureSession {
    type Error = FixtureError;

    /// return one exact Ecowitt rain-counter checkpoint
    fn get_scheduled_checkpoint(
        &mut self,
        _query: &GetScheduledCheckpointQuery,
    ) -> Result<Option<ScheduledCheckpointDbRow>, Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("checkpoint".to_owned());
        Ok(Some(ScheduledCheckpointDbRow {
            last_committed_at: START.to_owned(),
            last_valid_at: START.to_owned(),
            provider_cursor: Nullable::Value(vec![
                (utf16("rain_daily_total_mm"), JsonValue::Number(2.54)),
                (utf16("rain_day"), json_string("2026-08-29")),
                (utf16("valid_at"), json_string(START)),
            ]),
            source_id: SOURCE_ID.to_owned(),
            version: 9.0,
            window_end_exclusive: START.to_owned(),
            window_start: START.to_owned(),
        }))
    }
}

impl ScheduledRunStartCommandPort for FixtureSession {
    type Error = FixtureError;

    /// capture the Ecowitt running-row lookup
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

    /// capture one committed Ecowitt running row
    fn insert_scheduled_ingestion(
        &mut self,
        query: &InsertScheduledIngestionQuery,
    ) -> Result<Option<StartedScheduledIngestion>, Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("start-insert".to_owned());
        commands.insert = Some(query.clone());
        Ok(Some(StartedScheduledIngestion {
            id: "run-ecowitt".to_owned(),
            started_at: NOW.to_owned(),
            state: IngestionState::Running,
        }))
    }
}

impl ScheduledIngestionCommandPort for FixtureSession {
    type Error = FixtureError;

    /// record one atomic Ecowitt completion start
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("complete-begin".to_owned());
        Ok(())
    }

    /// capture the running Ecowitt row assertion
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

    /// capture the actual normalized Ecowitt record binds
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("records".to_owned());
        commands.records = Some(query.clone());
        Ok(())
    }

    /// capture typed Ecowitt cursor advancement
    fn advance_scheduled_checkpoint(
        &mut self,
        query: &ScheduledCheckpointQuery,
    ) -> Result<usize, Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("checkpoint-update".to_owned());
        commands.checkpoint = Some(query.clone());
        Ok(1)
    }

    /// capture exact Ecowitt response finalization
    fn finalize_scheduled_run(
        &mut self,
        query: &FinalizeScheduledRunQuery,
    ) -> Result<usize, Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("finalize-success".to_owned());
        commands.finalize = Some(query.clone());
        Ok(1)
    }

    /// record successful Ecowitt completion commit
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("complete-commit".to_owned());
        Ok(())
    }

    /// record unexpected Ecowitt completion rollback
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

    /// record Ecowitt failure transaction start
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("failure-begin".to_owned());
        Ok(())
    }

    /// record Ecowitt running-row failure assertion
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

    /// capture durable Ecowitt provider failure
    fn finalize_failed_ingestion(
        &mut self,
        query: &FinalizeFailedIngestionQuery,
    ) -> Result<usize, Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("failure-finalize".to_owned());
        commands.failure = Some(query.clone());
        Ok(1)
    }

    /// record Ecowitt failure commit
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("failure-commit".to_owned());
        Ok(())
    }

    /// record Ecowitt failure rollback
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

    /// Return due rows from the same database that owns scheduled Ecowitt sessions.
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

    /// acquire one retained Ecowitt source session
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

    /// capture Ecowitt stale-run cleanup
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

    /// release the retained Ecowitt session
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
        assert_eq!(session.source_id(), SOURCE_ID);
        self.commands.borrow_mut().trace.push("release".to_owned());
        Ok(())
    }
}

struct FixtureValues;

impl OpenMeteoRuntimeValues for FixtureValues {
    /// resolve only admitted Ecowitt fixture timestamps
    fn utc_instant(
        &mut self,
        milliseconds: i64,
        _value: RuntimeTimeValue,
    ) -> Result<String, TaskFailure> {
        let instant = match milliseconds {
            // map every exact Ecowitt fixture instant
            START_MS => START,
            NOW_MS => NOW,
            DEADLINE_MS => DEADLINE,
            DISPATCH_DEADLINE_MS => DISPATCH_DEADLINE,
            _ => panic!("unexpected Ecowitt UTC boundary: {milliseconds}"),
        };
        Ok(instant.to_owned())
    }

    /// decode the exact Ecowitt checkpoint window
    fn epoch_milliseconds(
        &mut self,
        instant: &str,
        value: RuntimeTimeValue,
    ) -> Result<i64, TaskFailure> {
        assert_eq!(instant, START);
        assert_eq!(value, RuntimeTimeValue::CheckpointWindowEnd);
        Ok(START_MS)
    }

    /// serialize exact Ecowitt provider quality and response objects
    fn serialize_json_object(
        &mut self,
        value: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure> {
        let first_key = &value
            .first()
            .expect("serialized Ecowitt object should not be empty")
            .0;

        // select the exact object contract by its first ordered key
        if *first_key == utf16("dataset") {
            assert_eq!(value, expected_provider_metadata());
            return Ok(SerializedJson::admitted(PROVIDER_JSON));
        }

        // preserve exact Ecowitt quality evidence
        if *first_key == utf16("sampling") {
            assert_eq!(
                value,
                [(utf16("sampling"), json_string("local_gateway_poll"))]
            );
            return Ok(SerializedJson::admitted(QUALITY_JSON));
        }

        assert_eq!(value, expected_response_metadata());
        Ok(SerializedJson::admitted(RESPONSE_JSON))
    }

    /// serialize the exact Ecowitt endpoint evidence
    fn request_metadata_json(&mut self, endpoint: &str) -> Result<SerializedJson, TaskFailure> {
        assert_eq!(endpoint, ECOWITT_LOCAL_LIVE_ENDPOINT);
        Ok(SerializedJson::admitted(
            r#"{"endpoint":"get_livedata_info"}"#,
        ))
    }

    /// reject the OpenMeteo string cursor path for Ecowitt
    fn provider_cursor_json(
        &mut self,
        _cursor: Option<&OpenMeteoCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        panic!("Ecowitt must not use the OpenMeteo cursor mapper")
    }

    /// reject the OpenMeteo checkpoint cursor path for Ecowitt
    fn checkpoint_cursor(
        &mut self,
        _checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<OpenMeteoCursor>, TaskFailure> {
        panic!("Ecowitt must not use the OpenMeteo checkpoint mapper")
    }

    /// admit the actual normalized Ecowitt record hash
    fn weather_record_content_hash(
        &mut self,
        record: &weather_contracts::NormalizedWeatherRecord,
    ) -> Result<Sha256Digest, TaskFailure> {
        assert_eq!(record.sourceId, SOURCE_ID);
        assert_eq!(record.sourceKind, SourceKind::PhysicalSensor);
        assert_eq!(record.validAt, NOW);
        assert_eq!(record.receivedAt, NOW);
        let Nullable::Value(precipitation_mm) = record.metrics.precipitationMm else {
            panic!("Ecowitt interval rain should be present");
        };
        assert!((precipitation_mm - 1.27).abs() < 1e-9);
        assert_eq!(
            record.metadata.device,
            Nullable::Value(weather_contracts::DeviceMetadata {
                model: FieldPresence::Value("GW3000".to_owned()),
                serial: FieldPresence::Value(MAC.to_owned()),
                vendor: FieldPresence::Value("Ecowitt".to_owned()),
            })
        );
        Ok(Sha256Digest::admitted(CONTENT_HASH))
    }

    /// admit the Ecowitt fingerprint and response checksum
    fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure> {
        assert!(value == "ecowitt-fingerprint-1" || value == RESPONSE_CHECKSUM);
        Ok(Sha256Digest::admitted(value))
    }

    /// map one Ecowitt provider failure through the shared domain boundary
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

impl EcowittRuntimeValues for FixtureValues {
    /// encode the typed Ecowitt rain-counter cursor
    fn ecowitt_provider_cursor_json(
        &mut self,
        cursor: &EcowittProviderCursor,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        assert_eq!(cursor.rain_day, "2026-08-29");
        assert_eq!(cursor.valid_at_ms, NOW_MS);
        assert!((cursor.rain_daily_total_mm.expect("daily total") - 3.81).abs() < 1e-12);
        Ok(Some(SerializedJson::admitted(CURSOR_JSON)))
    }

    /// decode the exact persisted Ecowitt counter cursor
    fn ecowitt_checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<EcowittProviderCursor, TaskFailure> {
        assert_eq!(checkpoint.sourceId, SOURCE_ID);
        assert_eq!(
            checkpoint.providerCursor,
            Nullable::Value(vec![
                (utf16("rain_daily_total_mm"), JsonValue::Number(2.54)),
                (utf16("rain_day"), json_string("2026-08-29")),
                (utf16("valid_at"), json_string(START)),
            ])
        );
        Ok(EcowittProviderCursor {
            rain_daily_total_mm: Some(2.54),
            rain_day: "2026-08-29".to_owned(),
            valid_at_ms: START_MS,
        })
    }
}

struct FixtureTimeAdapter;

impl EcowittTimeAdapter for FixtureTimeAdapter {
    /// admit the exact received instant and source-local date
    fn local_date(
        &mut self,
        received_at_ms: i64,
        timezone: &str,
    ) -> Result<EcowittAdmittedLocalDate, TaskFailure> {
        assert_eq!(received_at_ms, NOW_MS);
        assert_eq!(timezone, "America/Los_Angeles");
        Ok(EcowittAdmittedLocalDate::admitted(2026, 8, 29))
    }
}

struct FixtureMetadataAdapter;

impl EcowittCanonicalMetadataAdapter for FixtureMetadataAdapter {
    /// admit the actual constructed minimal provider metadata
    fn measure_provider_metadata(
        &mut self,
        metadata: &EcowittProviderMetadata,
    ) -> Result<EcowittCanonicalMetadataMeasurement, TaskFailure> {
        assert_eq!(metadata.dataset, "get_livedata_info");
        assert_eq!(metadata.device_id, MAC);
        assert_eq!(metadata.property_sensors.len(), 1);
        Ok(EcowittCanonicalMetadataMeasurement::admitted(180))
    }
}

#[derive(Clone, Copy)]
enum ProviderOutcome {
    Failure,
    Success,
}

struct FixtureProvider {
    attempts: Vec<(EcowittProviderStage, ProviderAttempt)>,
    commands: Rc<RefCell<CapturedCommands>>,
    current_stage: Option<EcowittProviderStage>,
    now_ms: i64,
    outcomes: VecDeque<(
        EcowittProviderStage,
        Result<ProviderAttemptResponse<EcowittAttemptPayload>, ProviderAttemptError>,
    )>,
    random: VecDeque<f64>,
    sleeps: Vec<(EcowittProviderStage, ProviderSleep)>,
}

impl EcowittProviderIo for FixtureProvider {
    /// select and validate one exact Ecowitt gateway request
    fn select_request(
        &mut self,
        stage: EcowittProviderStage,
        request: &EcowittScheduledRequest,
    ) -> Result<(), TaskFailure> {
        assert_eq!(request.expected_mac, MAC);
        assert_eq!(request.gateway_host, "192.168.11.137");
        assert_eq!(request.model, "GW3000");
        assert_eq!(request.source_id, SOURCE_ID);
        assert_eq!(request.timezone, "America/Los_Angeles");
        self.commands
            .borrow_mut()
            .trace
            .push(format!("provider-{}-select", stage_name(stage)));
        self.current_stage = Some(stage);
        Ok(())
    }
}

impl ProviderIo<EcowittAttemptPayload> for FixtureProvider {
    /// return the deterministic Ecowitt provider clock
    fn now_ms(&mut self) -> i64 {
        self.now_ms
    }

    /// return one deterministic Ecowitt retry jitter input
    fn random_unit(&mut self) -> f64 {
        self.random.pop_front().unwrap_or(0.0)
    }

    /// retain one physical Ecowitt stage attempt
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<EcowittAttemptPayload>, ProviderAttemptError> {
        let stage = self
            .current_stage
            .expect("Ecowitt stage should be selected");
        self.commands.borrow_mut().trace.push(format!(
            "provider-{}-attempt-{}",
            stage_name(stage),
            attempt.attempt,
        ));
        self.attempts.push((stage, attempt));
        let (expected_stage, outcome) = self
            .outcomes
            .pop_front()
            .unwrap_or_else(|| panic!("Ecowitt provider response missing"));
        assert_eq!(stage, expected_stage);
        outcome
    }

    /// retain and apply one exact Ecowitt retry sleep
    fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
        let stage = self
            .current_stage
            .expect("Ecowitt stage should be selected");
        self.commands.borrow_mut().trace.push(format!(
            "provider-{}-sleep-{}",
            stage_name(stage),
            sleep.attempts,
        ));
        self.sleeps.push((stage, sleep));
        self.now_ms += i64::try_from(sleep.delay_ms).expect("Ecowitt delay should fit i64");
        Ok(())
    }
}

/// physical Ecowitt controller evidence after one scheduled run
struct ProviderExecutionEvidence {
    attempts: Vec<(EcowittProviderStage, ProviderAttempt)>,
    sleeps: Vec<(EcowittProviderStage, ProviderSleep)>,
}

/// return one stable Ecowitt stage trace name
const fn stage_name(stage: EcowittProviderStage) -> &'static str {
    // map each exact local gateway endpoint
    match stage {
        EcowittProviderStage::GatewayIdentity => "identity",
        EcowittProviderStage::LiveData => "live",
    }
}

/// build one typed Ecowitt identity response
fn identity_response(status: u16) -> ProviderAttemptResponse<EcowittAttemptPayload> {
    ProviderAttemptResponse {
        body: EcowittAttemptPayload {
            product: EcowittParsedProduct::GatewayIdentity {
                gateway_mac: MAC.to_owned(),
            },
        },
        content: ContentBudget {
            declared_body_bytes: Some(20_000),
            streamed_body_bytes: 20_000,
        },
        // retain a retry reason only for the failed fixture response
        provider_reason: (status != 200).then(|| "identity unavailable".to_owned()),
        retry_after_ms: None,
        status,
    }
}

/// build one typed Ecowitt live-data response
fn live_response(status: u16) -> ProviderAttemptResponse<EcowittAttemptPayload> {
    ProviderAttemptResponse {
        body: EcowittAttemptPayload {
            product: EcowittParsedProduct::LiveData {
                combined_upstream_response_checksum: RESPONSE_CHECKSUM.to_owned(),
                payload: Box::new(ecowitt_payload()),
                received_at_ms: NOW_MS,
            },
        },
        content: ContentBudget {
            declared_body_bytes: Some(200_000),
            streamed_body_bytes: 200_000,
        },
        // retain a retry reason only for the failed fixture response
        provider_reason: (status != 200).then(|| "live data unavailable".to_owned()),
        retry_after_ms: None,
        status,
    }
}

/// build one controller-backed Ecowitt adapter
fn provider_fixture(
    outcome: ProviderOutcome,
    commands: Rc<RefCell<CapturedCommands>>,
) -> RetryingEcowittProvider<FixtureProvider, FixtureTimeAdapter, FixtureMetadataAdapter> {
    RetryingEcowittProvider::new(
        provider_io_fixture(outcome, commands),
        Some(NOW_MS + 90_000),
        FixtureTimeAdapter,
        FixtureMetadataAdapter,
    )
}

/// Build the raw request-selecting IO used by direct and dispatcher fixtures.
fn provider_io_fixture(
    outcome: ProviderOutcome,
    commands: Rc<RefCell<CapturedCommands>>,
) -> FixtureProvider {
    // select exact two-stage success or live exhaustion
    let outcomes = match outcome {
        ProviderOutcome::Failure => VecDeque::from([
            (
                EcowittProviderStage::GatewayIdentity,
                Ok(identity_response(200)),
            ),
            (EcowittProviderStage::LiveData, Ok(live_response(503))),
            (EcowittProviderStage::LiveData, Ok(live_response(503))),
            (EcowittProviderStage::LiveData, Ok(live_response(503))),
        ]),
        ProviderOutcome::Success => VecDeque::from([
            (
                EcowittProviderStage::GatewayIdentity,
                Ok(identity_response(503)),
            ),
            (
                EcowittProviderStage::GatewayIdentity,
                Ok(identity_response(200)),
            ),
            (EcowittProviderStage::LiveData, Ok(live_response(503))),
            (EcowittProviderStage::LiveData, Ok(live_response(200))),
        ]),
    };
    FixtureProvider {
        attempts: Vec::new(),
        commands,
        current_stage: None,
        now_ms: NOW_MS,
        outcomes,
        random: VecDeque::from([0.5, 0.5, 0.5]),
        sleeps: Vec::new(),
    }
}

struct DispatcherMaterial;

impl ScheduledSourceMaterialComparison for DispatcherMaterial {
    type Error = FixtureError;

    /// Compare decoded source material without a fixture serializer.
    fn values_equal(&mut self, left: &JsonValue, right: &JsonValue) -> Result<bool, Self::Error> {
        Ok(left == right)
    }

    /// Compare a decoded value with the same admitted object entries.
    fn value_and_object_equal(
        &mut self,
        value: &JsonValue,
        object: &[(Vec<u16>, JsonValue)],
    ) -> Result<bool, Self::Error> {
        Ok(matches!(value, JsonValue::Object(entries) if entries == object))
    }
}

impl WorkerSourceMaterialIdentity for DispatcherMaterial {
    /// Return the exact identity already carried by the due-source fixture.
    fn material_identity(&mut self, value: &JsonValue) -> Result<String, TaskFailure> {
        assert_eq!(
            value,
            &JsonValue::Object(vec![(
                utf16("contractVersion"),
                json_string(ECOWITT_LOCAL_LIVE_ADAPTER_VERSION),
            )])
        );
        Ok("ecowitt-material-1".to_owned())
    }
}

struct DispatcherTransport {
    bindings: Rc<RefCell<Vec<EcowittScheduledRequest>>>,
    commands: Rc<RefCell<CapturedCommands>>,
}

impl WorkerEcowittTransport for DispatcherTransport {
    type Io<'a> = FixtureProvider;
    type Time<'a> = FixtureTimeAdapter;
    type Metadata<'a> = FixtureMetadataAdapter;

    /// Bind one request only after the durable scheduled run has started.
    fn bind_request(
        &mut self,
        request: &EcowittScheduledRequest,
    ) -> WorkerEcowittBindResult<Self::Io<'_>, Self::Time<'_>, Self::Metadata<'_>> {
        self.commands
            .borrow_mut()
            .trace
            .push("provider-bind".to_owned());
        self.bindings.borrow_mut().push(request.clone());
        Ok(WorkerEcowittExchange {
            io: provider_io_fixture(ProviderOutcome::Success, Rc::clone(&self.commands)),
            metadata: FixtureMetadataAdapter,
            time: FixtureTimeAdapter,
        })
    }
}

struct NoOtherDispatcher;

impl<Repository> WorkerScheduledSourceDispatcher<Repository> for NoOtherDispatcher {
    /// Reject source families outside the Ecowitt fixture catalog.
    fn run_source<Clock: FnMut() -> i64>(
        &mut self,
        _repository: &mut Repository,
        source: &WorkerDueSource,
        _clock: &mut Clock,
    ) -> Result<SourceRunResult, TaskFailure> {
        panic!("unexpected delegated source: {}", source.source_id())
    }
}

/// Build the generated Ecowitt configuration consumed by the dispatcher.
fn generated_configuration() -> EcowittConfiguration {
    EcowittConfiguration {
        provider: EcowittConfigurationProvider {
            active: false,
            attributionLabel: "Ecowitt".to_owned(),
            attributionUrl: "https://www.ecowitt.com".to_owned(),
            displayName: "Ecowitt Local".to_owned(),
            key: "ecowitt-local".to_owned(),
        },
        siteKey: "ballydidean".to_owned(),
        stations: vec![EcowittStationConfiguration {
            active: false,
            adapterConfig: JsonValue::Object(vec![(
                utf16("contractVersion"),
                json_string(ECOWITT_LOCAL_LIVE_ADAPTER_VERSION),
            )]),
            cadenceSeconds: 60,
            displayName: "Ecowitt Gateway".to_owned(),
            expectedMac: MAC.to_owned(),
            fingerprint: "ecowitt-fingerprint-1".to_owned(),
            gatewayHost: "192.168.11.137".to_owned(),
            key: "ecowitt-gateway".to_owned(),
            latitude: 37.0,
            longitude: -122.0,
            model: "GW3000".to_owned(),
            sourceKey: "ecowitt-gateway-live-v1".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }],
        version: 1,
    }
}

/// build the exact active Ecowitt source and configuration
fn source_configuration() -> (EcowittScheduledSource, EcowittRuntimeConfiguration) {
    let source = EcowittScheduledSource {
        active: true,
        cadence_seconds: 60,
        id: SOURCE_ID.to_owned(),
        material_config_identity: "ecowitt-material-1".to_owned(),
        provider_key: "ecowitt-local".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_config_fingerprint: "ecowitt-fingerprint-1".to_owned(),
        source_key: "ecowitt-gateway-live-v1".to_owned(),
        source_kind: EcowittSourceKind::PhysicalSensor,
        station_slug: "ecowitt-gateway".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    };
    let configuration = EcowittRuntimeConfiguration {
        adapter_contract_version: ECOWITT_LOCAL_LIVE_ADAPTER_VERSION,
        capabilities: BTreeSet::from([EcowittCapability::Current]),
        expected_mac: MAC.to_owned(),
        gateway_host: "192.168.11.137".to_owned(),
        material_config_identity: source.material_config_identity.clone(),
        model: "GW3000".to_owned(),
        provider_key: source.provider_key.clone(),
        site_slug: source.site_slug.clone(),
        source_config_fingerprint: source.source_config_fingerprint.clone(),
        source_key: source.source_key.clone(),
        source_kind: source.source_kind,
        station_slug: source.station_slug.clone(),
        timezone: source.timezone.clone(),
    };
    (source, configuration)
}

/// build one minimal actual Ecowitt piezo-rain payload
fn ecowitt_payload() -> EcowittLivePayload {
    EcowittLivePayload {
        group_count: 2,
        piezo_rain: Some(EcowittRainReadings {
            battery: None,
            daily_total: Some(EcowittMeasurement {
                unit: EcowittUnit::Inch,
                value: 0.15,
            }),
            rate: None,
        }),
        ..EcowittLivePayload::default()
    }
}

/// build exact expected nested Ecowitt provider metadata
fn expected_provider_metadata() -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        (utf16("dataset"), json_string("get_livedata_info")),
        (utf16("device_id"), json_string(MAC)),
        (
            utf16("property_sensors"),
            JsonValue::Array(vec![JsonValue::Object(vec![
                (utf16("channel"), JsonValue::Null),
                (utf16("key"), json_string("weather-array")),
                (utf16("model"), json_string("WS90")),
                (
                    utf16("readings"),
                    JsonValue::Object(vec![(
                        utf16("dailyPrecipitationMm"),
                        JsonValue::Number(3.809_999_999_999_999_6),
                    )]),
                ),
            ])]),
        ),
    ]
}

/// build exact expected Ecowitt response metadata
fn expected_response_metadata() -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        (utf16("gateway_mac"), json_string(MAC)),
        (utf16("group_count"), JsonValue::Number(2.0)),
        (utf16("rain_gauge"), json_string("piezo")),
        (utf16("soil_channel_count"), JsonValue::Number(0.0)),
        (
            utf16("temperature_humidity_channel_count"),
            JsonValue::Number(0.0),
        ),
    ]
}

/// execute one Ecowitt run through the shared database adapter
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
    let mut repository = EcowittDatabaseRepository::new(&mut database, &mut values);
    let mut provider = provider_fixture(outcome, Rc::clone(&commands));
    let (source, configuration) = source_configuration();
    let result = run_scheduled_ecowitt_source(
        EcowittScheduledExecution {
            configuration: &configuration,
            now_ms: NOW_MS,
            run_deadline_at_ms: DEADLINE_MS,
            source: &source,
        },
        &mut repository,
        &mut provider,
        &mut || NOW_MS + 5_000,
    )
    .expect("Ecowitt database adapter should return a bounded result");
    let evidence = ProviderExecutionEvidence {
        attempts: provider.io().attempts.clone(),
        sleeps: provider.io().sleeps.clone(),
    };
    (result, commands, evidence)
}

/// Run one configured Ecowitt source through discovery, shared session, and heartbeat.
#[test]
fn dispatches_ecowitt_through_shared_iteration_database() {
    let commands = Rc::new(RefCell::new(CapturedCommands::default()));
    let bindings = Rc::new(RefCell::new(Vec::new()));
    let (source, _) = source_configuration();
    let mut database = FixtureDatabase {
        commands: Rc::clone(&commands),
        discover_at_ms: Vec::new(),
        due_sources: vec![WorkerDueSource::Ecowitt(source)],
        heartbeats: Vec::new(),
    };
    let mut values = FixtureValues;
    let mut material = DispatcherMaterial;
    let mut dispatcher = WorkerEcowittDispatcher::from_configuration(
        &generated_configuration(),
        &mut material,
        DispatcherTransport {
            bindings: Rc::clone(&bindings),
            commands: Rc::clone(&commands),
        },
        NoOtherDispatcher,
    )
    .expect("Ecowitt dispatcher configuration should load");
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
            &mut || clocks.pop_front().expect("dispatcher clock should exist"),
        )
        .expect("Ecowitt iteration should complete")
    };

    assert!(clocks.is_empty());
    assert_eq!(database.discover_at_ms, [NOW_MS - 2_000]);
    assert_eq!(result.sources.len(), 1);
    assert_eq!(result.sources[0].status, SourceRunStatus::Succeeded);
    assert_eq!(result.sources[0].attempts, 4);
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
    assert_eq!(bindings.borrow().len(), 1);
    let binding = bindings.borrow();
    assert_eq!(binding[0].expected_mac, MAC);
    assert_eq!(binding[0].gateway_host, "192.168.11.137");
    assert_eq!(binding[0].source_id, SOURCE_ID);
    assert_eq!(binding[0].timezone, "America/Los_Angeles");
    drop(binding);
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
            "provider-identity-select",
            "provider-identity-attempt-1",
            "provider-identity-sleep-1",
            "provider-identity-attempt-2",
            "provider-live-select",
            "provider-live-attempt-1",
            "provider-live-sleep-1",
            "provider-live-attempt-2",
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

/// persist one real Ecowitt rain counter and nested sensor metadata atomically
#[test]
fn persists_normalized_ecowitt_atomically() {
    let (result, commands, evidence) = execute(ProviderOutcome::Success);
    assert_eq!(result.status, SourceRunStatus::Succeeded);
    assert_eq!(result.record_count, 1);
    assert_eq!(result.attempts, 4);
    let commands = commands.borrow();
    assert_eq!(
        commands.trace,
        [
            "acquire",
            "abandon",
            "checkpoint",
            "start-lookup",
            "start-insert",
            "provider-identity-select",
            "provider-identity-attempt-1",
            "provider-identity-sleep-1",
            "provider-identity-attempt-2",
            "provider-live-select",
            "provider-live-attempt-1",
            "provider-live-sleep-1",
            "provider-live-attempt-2",
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
            (
                EcowittProviderStage::GatewayIdentity,
                ProviderAttempt {
                    attempt: 1,
                    max_body_bytes: 32_768,
                    timeout_ms: 5_000,
                },
            ),
            (
                EcowittProviderStage::GatewayIdentity,
                ProviderAttempt {
                    attempt: 2,
                    max_body_bytes: 32_768,
                    timeout_ms: 5_000,
                },
            ),
            (
                EcowittProviderStage::LiveData,
                ProviderAttempt {
                    attempt: 1,
                    max_body_bytes: 262_144,
                    timeout_ms: 5_000,
                },
            ),
            (
                EcowittProviderStage::LiveData,
                ProviderAttempt {
                    attempt: 2,
                    max_body_bytes: 262_144,
                    timeout_ms: 5_000,
                },
            ),
        ]
    );
    assert_eq!(
        evidence.sleeps,
        [
            (
                EcowittProviderStage::GatewayIdentity,
                ProviderSleep {
                    attempts: 1,
                    deadline_remaining_ms: Some(90_000),
                    delay_ms: 300,
                },
            ),
            (
                EcowittProviderStage::LiveData,
                ProviderSleep {
                    attempts: 1,
                    deadline_remaining_ms: Some(89_700),
                    delay_ms: 300,
                },
            ),
        ]
    );
    let insert = commands.insert.as_ref().expect("start should persist");
    assert_eq!(
        insert.binds[2],
        InsertScheduledIngestionBind::UtcInstant(START.to_owned())
    );
    assert_eq!(
        insert.binds[3],
        InsertScheduledIngestionBind::UtcInstant(NOW.to_owned())
    );
    assert_eq!(
        insert.binds[5],
        InsertScheduledIngestionBind::Text(ECOWITT_LOCAL_LIVE_ADAPTER_VERSION.to_owned())
    );
    let records = commands.records.as_ref().expect("record should persist");
    assert_eq!(
        records.binds[1],
        WeatherRecordBind::SourceKind(SourceKind::PhysicalSensor)
    );
    assert_eq!(
        records.binds[11],
        WeatherRecordBind::NullableJson(Some(SerializedJson::admitted(QUALITY_JSON)))
    );
    assert_eq!(
        records.binds[12],
        WeatherRecordBind::NullableJson(Some(SerializedJson::admitted(PROVIDER_JSON)))
    );
    let checkpoint = commands
        .checkpoint
        .as_ref()
        .expect("checkpoint should advance");
    match checkpoint {
        // verify the exact typed rain-counter cursor
        ScheduledCheckpointQuery::Update { binds, .. } => {
            assert_eq!(binds[1], CheckpointUpdateBind::Version(9));
            assert_eq!(binds[2], CheckpointUpdateBind::UtcInstant(NOW.to_owned()));
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

/// finalize one exhausted Ecowitt live request with exact durable attempts
#[test]
fn finalizes_ecowitt_live_provider_failure() {
    let (result, commands, evidence) = execute(ProviderOutcome::Failure);
    assert_eq!(result.status, SourceRunStatus::Failed);
    assert_eq!(result.reason.as_deref(), Some("provider_server_error"));
    assert_eq!(result.attempts, 3);
    let commands = commands.borrow();
    assert_eq!(
        commands.trace,
        [
            "acquire",
            "abandon",
            "checkpoint",
            "start-lookup",
            "start-insert",
            "provider-identity-select",
            "provider-identity-attempt-1",
            "provider-live-select",
            "provider-live-attempt-1",
            "provider-live-sleep-1",
            "provider-live-attempt-2",
            "provider-live-sleep-2",
            "provider-live-attempt-3",
            "failure-begin",
            "failure-assert",
            "failure-finalize",
            "failure-commit",
            "release",
        ]
    );
    let failure = commands.failure.as_ref().expect("failure should finalize");
    assert!(format!("{failure:?}").contains("provider_server_error"));
    assert_eq!(
        evidence
            .attempts
            .iter()
            .map(|(stage, attempt)| (*stage, attempt.attempt, attempt.max_body_bytes))
            .collect::<Vec<_>>(),
        [
            (EcowittProviderStage::GatewayIdentity, 1, 32_768),
            (EcowittProviderStage::LiveData, 1, 262_144),
            (EcowittProviderStage::LiveData, 2, 262_144),
            (EcowittProviderStage::LiveData, 3, 262_144),
        ]
    );
    assert_eq!(
        evidence
            .sleeps
            .iter()
            .map(|(stage, sleep)| (*stage, sleep.delay_ms))
            .collect::<Vec<_>>(),
        [
            (EcowittProviderStage::LiveData, 300),
            (EcowittProviderStage::LiveData, 550),
        ]
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
