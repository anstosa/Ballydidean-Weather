use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::rc::Rc;

use weather_contracts::{
    FieldPresence, IngestionError, IngestionErrorClassification, IngestionState, JsonValue,
    Nullable, ScheduledCheckpointState, SourceCapability, SourceKind, StationKind,
    TideConfiguration, TideProviderConfiguration, TideSourceConfiguration,
    TideSourceConfigurationSourceKindEnum, TideStationConfiguration,
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
    NoaaTideAttemptPayload, NoaaTideDatabaseRepository, NoaaTideRuntimeValues,
    OpenMeteoDatabaseBoundary, OpenMeteoRuntimeValues, RetryingNoaaTideProvider,
    RuntimePersistenceOperation, RuntimeTimeValue, WorkerIterationDatabaseRepository,
    WorkerNoaaTideDispatcher, WorkerNoaaTideTransport, WorkerSourceMaterialIdentity,
};
use weather_worker::{
    NOAA_SCHEDULED_PREDICTION_DAYS, NOAA_TIDE_DATA_ENDPOINT, NOAA_TIDE_OBSERVATION_ADAPTER_VERSION,
    NOAA_TIDE_PREDICTION_ADAPTER_VERSION, NoaaPredictionType, NoaaTideCapability,
    NoaaTideObservation, NoaaTidePayload, NoaaTidePrediction, NoaaTideProviderCursor,
    NoaaTideRuntimeConfiguration, NoaaTideScheduledExecution, NoaaTideScheduledSource,
    NoaaTideSourceKind, OpenMeteoCursor, ScheduledSourceMaterialComparison, SourceRunStatus,
    TaskFailure, WorkerDueSource, WorkerScheduledSourceDispatcher, run_scheduled_noaa_tide_source,
};

const NOW_MS: i64 = 1_788_012_000_000;
const DEADLINE_MS: i64 = 1_788_012_600_000;
const DISPATCHER_DEADLINE_MS: i64 = NOW_MS + 120_000;
const NOW: &str = "2026-08-29T14:00:00.000Z";
const DEADLINE: &str = "2026-08-29T14:10:00.000Z";
const DISPATCHER_DEADLINE: &str = "2026-08-29T14:02:00.000Z";
const OBS_CHECKPOINT_CURSOR_MS: i64 = 1_788_010_560_000;
const OBS_START_MS: i64 = 1_788_010_920_000;
const OBS_END_MS: i64 = 1_788_011_280_000;
const OBS_CHECKPOINT_CURSOR: &str = "2026-08-29T13:36:00.000Z";
const OBS_START: &str = "2026-08-29T13:42:00.000Z";
const OBS_END: &str = "2026-08-29T13:48:00.000Z";
const PRED_START_MS: i64 = 1_787_983_200_000;
const PRED_END_MS: i64 = 1_788_004_800_000;
const PRED_VALID_MS: i64 = 1_788_055_200_000;
const PRED_REQUEST_END_MS: i64 = 1_790_604_000_000;
const PRED_START: &str = "2026-08-29T06:00:00.000Z";
const PRED_END: &str = "2026-08-29T12:00:00.000Z";
const PRED_VALID: &str = "2026-08-30T02:00:00.000Z";
const PRED_REQUEST_END: &str = "2026-09-28T14:00:00.000Z";
const RESPONSE_CHECKSUM: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CONTENT_HASH: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FixtureCase {
    Observation,
    Prediction,
}

impl FixtureCase {
    /// return the source identity for this NOAA product
    const fn source_id(self) -> &'static str {
        // preserve product-specific source identity
        match self {
            Self::Observation => "source-noaa-observation",
            Self::Prediction => "source-noaa-prediction",
        }
    }

    /// return the source kind for this NOAA product
    const fn source_kind(self) -> NoaaTideSourceKind {
        // preserve product-specific source kind
        match self {
            Self::Observation => NoaaTideSourceKind::Observation,
            Self::Prediction => NoaaTideSourceKind::Prediction,
        }
    }

    /// return the checked NOAA station identity
    const fn station_id(self) -> &'static str {
        // preserve product-specific station identity
        match self {
            Self::Observation => "9444900",
            Self::Prediction => "9447814",
        }
    }

    /// return the expected checkpoint version
    const fn checkpoint_version(self) -> u64 {
        // preserve fixture compare-and-set versions
        match self {
            Self::Observation => 4,
            Self::Prediction => 7,
        }
    }
}

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
    case: FixtureCase,
    commands: Rc<RefCell<CapturedCommands>>,
}

impl RetainedSourceSession for FixtureSession {
    /// return the retained NOAA source identity
    fn source_id(&self) -> &str {
        self.case.source_id()
    }
}

impl ScheduledCheckpointReadPort for FixtureSession {
    type Error = FixtureError;

    /// return one exact typed NOAA checkpoint row
    fn get_scheduled_checkpoint(
        &mut self,
        _query: &GetScheduledCheckpointQuery,
    ) -> Result<Option<ScheduledCheckpointDbRow>, Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("checkpoint".to_owned());
        let (cursor, window_end) = match self.case {
            // select product-specific durable history
            FixtureCase::Observation => (OBS_CHECKPOINT_CURSOR, OBS_START),
            FixtureCase::Prediction => (PRED_START, PRED_START),
        };
        Ok(Some(ScheduledCheckpointDbRow {
            last_committed_at: window_end.to_owned(),
            last_valid_at: cursor.to_owned(),
            provider_cursor: Nullable::Value(vec![(
                utf16("valid_at"),
                JsonValue::String(utf16(cursor)),
            )]),
            source_id: self.case.source_id().to_owned(),
            version: self.case.checkpoint_version() as f64,
            window_end_exclusive: window_end.to_owned(),
            window_start: cursor.to_owned(),
        }))
    }
}

impl ScheduledRunStartCommandPort for FixtureSession {
    type Error = FixtureError;

    /// capture the source-scoped running-row lookup
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

    /// capture one committed NOAA running row
    fn insert_scheduled_ingestion(
        &mut self,
        query: &InsertScheduledIngestionQuery,
    ) -> Result<Option<StartedScheduledIngestion>, Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("start-insert".to_owned());
        commands.insert = Some(query.clone());
        Ok(Some(StartedScheduledIngestion {
            id: "run-noaa".to_owned(),
            started_at: NOW.to_owned(),
            state: IngestionState::Running,
        }))
    }
}

impl ScheduledIngestionCommandPort for FixtureSession {
    type Error = FixtureError;

    /// record one atomic NOAA completion start
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("complete-begin".to_owned());
        Ok(())
    }

    /// capture the running NOAA row assertion
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

    /// capture actual normalized NOAA storage binds
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("records".to_owned());
        commands.records = Some(query.clone());
        Ok(())
    }

    /// capture typed NOAA cursor advancement
    fn advance_scheduled_checkpoint(
        &mut self,
        query: &ScheduledCheckpointQuery,
    ) -> Result<usize, Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("checkpoint-update".to_owned());
        commands.checkpoint = Some(query.clone());
        Ok(1)
    }

    /// capture exact NOAA response finalization
    fn finalize_scheduled_run(
        &mut self,
        query: &FinalizeScheduledRunQuery,
    ) -> Result<usize, Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("finalize-success".to_owned());
        commands.finalize = Some(query.clone());
        Ok(1)
    }

    /// record successful NOAA completion commit
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("complete-commit".to_owned());
        Ok(())
    }

    /// record unexpected NOAA completion rollback
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

    /// record NOAA failure finalization start
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("failure-begin".to_owned());
        Ok(())
    }

    /// capture one failed NOAA run assertion
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

    /// capture typed NOAA provider failure finalization
    fn finalize_failed_ingestion(
        &mut self,
        query: &FinalizeFailedIngestionQuery,
    ) -> Result<usize, Self::Error> {
        let mut commands = self.commands.borrow_mut();
        commands.trace.push("failure-finalize".to_owned());
        commands.failure = Some(query.clone());
        Ok(1)
    }

    /// record NOAA failure commit
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("failure-commit".to_owned());
        Ok(())
    }

    /// record unexpected NOAA failure rollback
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.commands
            .borrow_mut()
            .trace
            .push("failure-rollback".to_owned());
        Ok(())
    }
}

struct FixtureDatabase {
    case: FixtureCase,
    commands: Rc<RefCell<CapturedCommands>>,
}

impl OpenMeteoDatabaseBoundary for FixtureDatabase {
    type Session = FixtureSession;

    /// acquire one retained NOAA source session
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure> {
        assert_eq!(source_id, self.case.source_id());
        self.commands.borrow_mut().trace.push("acquire".to_owned());
        Ok(Some(FixtureSession {
            case: self.case,
            commands: Rc::clone(&self.commands),
        }))
    }

    /// capture stale-run cleanup at the admitted instant
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        as_of: &str,
    ) -> Result<(), TaskFailure> {
        assert_eq!(session.source_id(), self.case.source_id());
        assert_eq!(as_of, NOW);
        self.commands.borrow_mut().trace.push("abandon".to_owned());
        Ok(())
    }

    /// release the exact retained NOAA session
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
        assert_eq!(session.source_id(), self.case.source_id());
        self.commands.borrow_mut().trace.push("release".to_owned());
        Ok(())
    }
}

struct FixtureValues {
    case: FixtureCase,
}

impl OpenMeteoRuntimeValues for FixtureValues {
    /// resolve only admitted NOAA fixture timestamps
    fn utc_instant(
        &mut self,
        milliseconds: i64,
        _value: RuntimeTimeValue,
    ) -> Result<String, TaskFailure> {
        let instant = match milliseconds {
            // map every exact NOAA fixture instant
            NOW_MS => NOW,
            DEADLINE_MS => DEADLINE,
            DISPATCHER_DEADLINE_MS => DISPATCHER_DEADLINE,
            OBS_CHECKPOINT_CURSOR_MS => OBS_CHECKPOINT_CURSOR,
            OBS_START_MS => OBS_START,
            OBS_END_MS => OBS_END,
            PRED_START_MS => PRED_START,
            PRED_END_MS => PRED_END,
            PRED_VALID_MS => PRED_VALID,
            PRED_REQUEST_END_MS => PRED_REQUEST_END,
            _ => panic!("unexpected NOAA UTC boundary: {milliseconds}"),
        };
        Ok(instant.to_owned())
    }

    /// decode the product-specific checkpoint window
    fn epoch_milliseconds(
        &mut self,
        instant: &str,
        value: RuntimeTimeValue,
    ) -> Result<i64, TaskFailure> {
        assert_eq!(value, RuntimeTimeValue::CheckpointWindowEnd);
        let milliseconds = match self.case {
            // preserve each product's durable scheduling window
            FixtureCase::Observation => {
                assert_eq!(instant, OBS_START);
                OBS_START_MS
            }
            FixtureCase::Prediction => {
                assert_eq!(instant, PRED_START);
                PRED_START_MS
            }
        };
        Ok(milliseconds)
    }

    /// serialize exact NOAA provider quality and response objects
    fn serialize_json_object(
        &mut self,
        value: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure> {
        let first_key = &value
            .first()
            .expect("serialized NOAA object should not be empty")
            .0;

        // select the exact object contract by its first ordered key
        if *first_key == utf16("dataset") {
            assert_eq!(value, expected_provider_metadata(self.case));
            return Ok(SerializedJson::admitted(provider_json(self.case)));
        }

        // preserve observation and prediction quality variants
        if *first_key == utf16("flags") || *first_key == utf16("sampling") {
            assert_eq!(value, expected_quality_metadata(self.case));
            return Ok(SerializedJson::admitted(quality_json(self.case)));
        }

        assert_eq!(value, expected_response_metadata(self.case));
        Ok(SerializedJson::admitted(response_json(self.case)))
    }

    /// serialize the exact NOAA endpoint evidence
    fn request_metadata_json(&mut self, endpoint: &str) -> Result<SerializedJson, TaskFailure> {
        assert_eq!(endpoint, NOAA_TIDE_DATA_ENDPOINT);
        Ok(SerializedJson::admitted(
            r#"{"endpoint":"noaa-co-ops/datagetter"}"#,
        ))
    }

    /// reject the OpenMeteo string cursor path for NOAA
    fn provider_cursor_json(
        &mut self,
        _cursor: Option<&OpenMeteoCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        panic!("NOAA must not use the OpenMeteo cursor mapper")
    }

    /// reject the OpenMeteo checkpoint cursor path for NOAA
    fn checkpoint_cursor(
        &mut self,
        _checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<OpenMeteoCursor>, TaskFailure> {
        panic!("NOAA must not use the OpenMeteo checkpoint mapper")
    }

    /// admit the actual normalized NOAA record hash
    fn weather_record_content_hash(
        &mut self,
        record: &weather_contracts::NormalizedWeatherRecord,
    ) -> Result<Sha256Digest, TaskFailure> {
        let (source_kind, valid_at, model, water_level) = match self.case {
            // require both product-specific projections
            FixtureCase::Observation => (
                SourceKind::TideObservation,
                OBS_START,
                "Water Level Station",
                0.77,
            ),
            FixtureCase::Prediction => (
                SourceKind::TidePrediction,
                PRED_VALID,
                "Tide Prediction Station",
                2.11,
            ),
        };
        assert_eq!(record.sourceId, self.case.source_id());
        assert_eq!(record.sourceKind, source_kind);
        assert_eq!(record.validAt, valid_at);
        assert_eq!(record.receivedAt, NOW);
        assert_eq!(record.metrics.waterLevelM, Nullable::Value(water_level));
        assert_eq!(
            record.metadata.device,
            Nullable::Value(weather_contracts::DeviceMetadata {
                model: FieldPresence::Value(model.to_owned()),
                serial: FieldPresence::Value(self.case.station_id().to_owned()),
                vendor: FieldPresence::Value("NOAA CO-OPS".to_owned()),
            })
        );
        Ok(Sha256Digest::admitted(CONTENT_HASH))
    }

    /// admit fixture source fingerprints and response checksums
    fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure> {
        assert!(value == fingerprint(self.case) || value == RESPONSE_CHECKSUM);
        Ok(Sha256Digest::admitted(value))
    }

    /// map a NOAA provider failure through the shared domain boundary
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

impl NoaaTideRuntimeValues for FixtureValues {
    /// encode the typed NOAA cursor as Node's exact valid_at object
    fn noaa_tide_provider_cursor_json(
        &mut self,
        cursor: Option<&NoaaTideProviderCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        let valid_at_ms = match self.case {
            // select the actual final provider record
            FixtureCase::Observation => OBS_START_MS,
            FixtureCase::Prediction => PRED_VALID_MS,
        };
        assert_eq!(cursor, Some(&NoaaTideProviderCursor { valid_at_ms }));
        Ok(Some(SerializedJson::admitted(cursor_json(self.case))))
    }

    /// decode Node's exact valid_at object into the typed NOAA cursor
    fn noaa_tide_checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<NoaaTideProviderCursor>, TaskFailure> {
        let (valid_at, valid_at_ms) = match self.case {
            // select the exact persisted checkpoint cursor
            FixtureCase::Observation => (OBS_CHECKPOINT_CURSOR, OBS_CHECKPOINT_CURSOR_MS),
            FixtureCase::Prediction => (PRED_START, PRED_START_MS),
        };
        assert_eq!(checkpoint.sourceId, self.case.source_id());
        assert_eq!(
            checkpoint.providerCursor,
            Nullable::Value(vec![(
                utf16("valid_at"),
                JsonValue::String(utf16(valid_at)),
            )])
        );
        Ok(Some(NoaaTideProviderCursor { valid_at_ms }))
    }
}

#[derive(Clone, Copy)]
enum ProviderOutcome {
    Failure,
    Success,
}

struct FixtureProvider {
    attempts: Vec<ProviderAttempt>,
    case: FixtureCase,
    commands: Rc<RefCell<CapturedCommands>>,
    now_ms: i64,
    outcomes:
        VecDeque<Result<ProviderAttemptResponse<NoaaTideAttemptPayload>, ProviderAttemptError>>,
    random: VecDeque<f64>,
    sleeps: Vec<ProviderSleep>,
}

impl ProviderIo<NoaaTideAttemptPayload> for FixtureProvider {
    /// return the deterministic NOAA provider clock
    fn now_ms(&mut self) -> i64 {
        self.now_ms
    }

    /// return one deterministic NOAA retry jitter input
    fn random_unit(&mut self) -> f64 {
        self.random.pop_front().unwrap_or(0.0)
    }

    /// retain one physical NOAA attempt and return its typed product
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<NoaaTideAttemptPayload>, ProviderAttemptError> {
        self.commands
            .borrow_mut()
            .trace
            .push(format!("provider-attempt-{}", attempt.attempt));
        self.attempts.push(attempt);
        self.outcomes
            .pop_front()
            .unwrap_or_else(|| panic!("NOAA provider response missing for {:?}", self.case))
    }

    /// retain and apply one exact NOAA retry sleep
    fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
        self.commands
            .borrow_mut()
            .trace
            .push(format!("provider-sleep-{}", sleep.attempts));
        self.sleeps.push(sleep);
        self.now_ms += i64::try_from(sleep.delay_ms).expect("NOAA delay should fit i64");
        Ok(())
    }
}

/// physical NOAA controller evidence after one scheduled run
struct ProviderExecutionEvidence {
    attempts: Vec<ProviderAttempt>,
    sleeps: Vec<ProviderSleep>,
}

/// build typed NOAA payload after admitted transport parsing
fn noaa_attempt_payload(case: FixtureCase) -> NoaaTideAttemptPayload {
    NoaaTideAttemptPayload {
        payload: payload(case),
        received_at_ms: NOW_MS,
        upstream_response_checksum: RESPONSE_CHECKSUM.to_owned(),
    }
}

/// build NOAA response with ten-megabyte budget evidence
fn provider_response(
    case: FixtureCase,
    status: u16,
    reason: Option<&str>,
) -> ProviderAttemptResponse<NoaaTideAttemptPayload> {
    ProviderAttemptResponse {
        body: noaa_attempt_payload(case),
        content: ContentBudget {
            declared_body_bytes: Some(2_500_000),
            streamed_body_bytes: 2_500_000,
        },
        provider_reason: reason.map(str::to_owned),
        retry_after_ms: None,
        status,
    }
}

/// build controller-backed NOAA adapter
fn provider_fixture(
    case: FixtureCase,
    outcome: ProviderOutcome,
    commands: Rc<RefCell<CapturedCommands>>,
) -> RetryingNoaaTideProvider<FixtureProvider> {
    // select the exact physical attempt sequence
    let outcomes = match outcome {
        ProviderOutcome::Failure => VecDeque::from([
            Ok(provider_response(case, 503, Some("temporary one"))),
            Ok(provider_response(case, 503, Some("temporary two"))),
            Ok(provider_response(case, 503, Some("still unavailable"))),
        ]),
        ProviderOutcome::Success => VecDeque::from([
            Ok(provider_response(case, 503, Some("temporary outage"))),
            Ok(provider_response(case, 200, None)),
        ]),
    };
    RetryingNoaaTideProvider::new(
        FixtureProvider {
            attempts: Vec::new(),
            case,
            commands,
            now_ms: NOW_MS,
            outcomes,
            random: VecDeque::from([0.5, 0.5]),
            sleeps: Vec::new(),
        },
        Some(NOW_MS + 90_000),
    )
}

/// build one exact active NOAA source and runtime identity
fn source_configuration(
    case: FixtureCase,
) -> (NoaaTideScheduledSource, NoaaTideRuntimeConfiguration) {
    let (cadence_seconds, material, source_key, station_slug) = match case {
        // select the checked product catalog identity
        FixtureCase::Observation => (
            360,
            "noaa-observation-material-1",
            "noaa-port-townsend-water-level-v1",
            "port-townsend-tide-gauge",
        ),
        FixtureCase::Prediction => (
            21_600,
            "noaa-prediction-material-1",
            "noaa-glendale-tide-predictions-v1",
            "glendale-tide-predictions",
        ),
    };
    let source = NoaaTideScheduledSource {
        active: true,
        cadence_seconds,
        id: case.source_id().to_owned(),
        material_config_identity: material.to_owned(),
        provider_key: "noaa-co-ops".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_config_fingerprint: fingerprint(case).to_owned(),
        source_key: source_key.to_owned(),
        source_kind: case.source_kind(),
        station_slug: station_slug.to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    };
    let (adapter_contract_version, capabilities, forecast_days, maximum_chunk_days) = match case {
        // select the exact product capability contract
        FixtureCase::Observation => (
            NOAA_TIDE_OBSERVATION_ADAPTER_VERSION,
            BTreeSet::from([NoaaTideCapability::Current, NoaaTideCapability::Historical]),
            None,
            31,
        ),
        FixtureCase::Prediction => (
            NOAA_TIDE_PREDICTION_ADAPTER_VERSION,
            BTreeSet::from([NoaaTideCapability::Forecast, NoaaTideCapability::Historical]),
            Some(NOAA_SCHEDULED_PREDICTION_DAYS),
            366,
        ),
    };
    let configuration = NoaaTideRuntimeConfiguration {
        adapter_contract_version,
        capabilities,
        forecast_days,
        material_config_identity: source.material_config_identity.clone(),
        maximum_chunk_days,
        provider_key: source.provider_key.clone(),
        site_slug: source.site_slug.clone(),
        source_active: true,
        source_config_fingerprint: source.source_config_fingerprint.clone(),
        source_key: source.source_key.clone(),
        source_kind: source.source_kind,
        station_active: true,
        station_id: case.station_id().to_owned(),
        station_slug: source.station_slug.clone(),
        timezone: source.timezone.clone(),
    };
    (source, configuration)
}

/// build one actual typed NOAA payload
fn payload(case: FixtureCase) -> NoaaTidePayload {
    // select the actual provider product family
    match case {
        FixtureCase::Observation => NoaaTidePayload {
            observations: Some(vec![NoaaTideObservation {
                flags: "0,0,0,0".to_owned(),
                quality_status: "v".to_owned(),
                valid_at_ms: OBS_START_MS,
                water_level_m: 0.77,
            }]),
            ..NoaaTidePayload::default()
        },
        FixtureCase::Prediction => NoaaTidePayload {
            predictions: Some(vec![NoaaTidePrediction {
                prediction_type: NoaaPredictionType::High,
                valid_at_ms: PRED_VALID_MS,
                water_level_m: 2.11,
            }]),
            ..NoaaTidePayload::default()
        },
    }
}

/// execute one NOAA product through the shared scheduled database adapter
fn execute(
    case: FixtureCase,
    outcome: ProviderOutcome,
) -> (
    weather_worker::SourceRunResult,
    Rc<RefCell<CapturedCommands>>,
    ProviderExecutionEvidence,
) {
    let commands = Rc::new(RefCell::new(CapturedCommands::default()));
    let mut database = FixtureDatabase {
        case,
        commands: Rc::clone(&commands),
    };
    let mut values = FixtureValues { case };
    let mut repository = NoaaTideDatabaseRepository::new(&mut database, &mut values);
    let mut provider = provider_fixture(case, outcome, Rc::clone(&commands));
    let (source, configuration) = source_configuration(case);
    let result = run_scheduled_noaa_tide_source(
        NoaaTideScheduledExecution {
            configuration: &configuration,
            now_ms: NOW_MS,
            run_deadline_at_ms: DEADLINE_MS,
            source: &source,
        },
        &mut repository,
        &mut provider,
        &mut || NOW_MS + 5_000,
    )
    .expect("NOAA database adapter should return a bounded run result");
    let evidence = ProviderExecutionEvidence {
        attempts: provider.io().attempts.clone(),
        sleeps: provider.io().sleeps.clone(),
    };
    (result, commands, evidence)
}

/// verify one product's exact committed database effects
fn assert_success(case: FixtureCase) {
    let (result, commands, evidence) = execute(case, ProviderOutcome::Success);
    assert_eq!(result.status, SourceRunStatus::Succeeded);
    assert_eq!(result.record_count, 1);
    assert_eq!(result.attempts, 2);
    assert_eq!(result.successful_at_ms, Some(NOW_MS + 5_000));
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
    let (adapter, window_start, window_end) = match case {
        // preserve scheduled identity independently from provider prediction horizon
        FixtureCase::Observation => (NOAA_TIDE_OBSERVATION_ADAPTER_VERSION, OBS_START, OBS_END),
        FixtureCase::Prediction => (NOAA_TIDE_PREDICTION_ADAPTER_VERSION, PRED_START, PRED_END),
    };
    assert_eq!(
        insert.binds[2],
        InsertScheduledIngestionBind::UtcInstant(window_start.to_owned())
    );
    assert_eq!(
        insert.binds[3],
        InsertScheduledIngestionBind::UtcInstant(window_end.to_owned())
    );
    assert_eq!(
        insert.binds[5],
        InsertScheduledIngestionBind::Text(adapter.to_owned())
    );
    assert_eq!(
        insert.binds[7],
        InsertScheduledIngestionBind::UtcInstant(DEADLINE.to_owned())
    );
    assert_eq!(
        insert.binds[9],
        InsertScheduledIngestionBind::NullableJson(Some(SerializedJson::admitted(
            r#"{"endpoint":"noaa-co-ops/datagetter"}"#,
        )))
    );
    let records = commands.records.as_ref().expect("records should persist");
    assert_eq!(records.record_count, 1);
    assert_eq!(
        records.binds[0],
        WeatherRecordBind::BigintText(case.source_id().to_owned())
    );
    let (source_kind, model, water_level) = match case {
        // preserve each product's record projection
        FixtureCase::Observation => (SourceKind::TideObservation, "Water Level Station", 0.77),
        FixtureCase::Prediction => (SourceKind::TidePrediction, "Tide Prediction Station", 2.11),
    };
    assert_eq!(records.binds[1], WeatherRecordBind::SourceKind(source_kind));
    assert_eq!(
        records.binds[8],
        WeatherRecordBind::NullableText(Some("NOAA CO-OPS".to_owned()))
    );
    assert_eq!(
        records.binds[9],
        WeatherRecordBind::NullableText(Some(model.to_owned()))
    );
    assert_eq!(
        records.binds[10],
        WeatherRecordBind::NullableText(Some(case.station_id().to_owned()))
    );
    assert_eq!(
        records.binds[11],
        WeatherRecordBind::NullableJson(Some(SerializedJson::admitted(quality_json(case))))
    );
    assert_eq!(
        records.binds[12],
        WeatherRecordBind::NullableJson(Some(SerializedJson::admitted(provider_json(case))))
    );
    assert_eq!(
        records.binds[30],
        WeatherRecordBind::NullableNumber(Some(water_level))
    );
    let checkpoint = commands
        .checkpoint
        .as_ref()
        .expect("checkpoint should advance");
    let (last_valid_at, checkpoint_cursor) = match case {
        // preserve final-record versus scheduled-window checkpoint policy
        FixtureCase::Observation => (OBS_START, cursor_json(case)),
        FixtureCase::Prediction => (PRED_END, cursor_json(case)),
    };
    match checkpoint {
        // verify the exact compare-and-set checkpoint effects
        ScheduledCheckpointQuery::Update { binds, .. } => {
            assert_eq!(
                binds[1],
                CheckpointUpdateBind::Version(case.checkpoint_version())
            );
            assert_eq!(
                binds[2],
                CheckpointUpdateBind::UtcInstant(last_valid_at.to_owned())
            );
            assert_eq!(
                binds[5],
                CheckpointUpdateBind::NullableJson(Some(SerializedJson::admitted(
                    checkpoint_cursor,
                )))
            );
        }
        ScheduledCheckpointQuery::Insert { .. } => panic!("fixture must update its checkpoint"),
    }
    let finalize = commands.finalize.as_ref().expect("run should finalize");
    assert_eq!(
        finalize.binds[4],
        FinalizeScheduledRunBind::NullableJson(Some(SerializedJson::admitted(response_json(case))))
    );
}

/// persist one real verified NOAA observation with typed cursor and quality
#[test]
fn persists_normalized_noaa_observation_atomically() {
    assert_success(FixtureCase::Observation);
}

/// persist one real NOAA prediction with independent request and checkpoint effects
#[test]
fn persists_normalized_noaa_prediction_atomically() {
    assert_success(FixtureCase::Prediction);
}

/// finalize one NOAA provider failure through the shared database lifecycle
#[test]
fn finalizes_noaa_provider_failure() {
    let (result, commands, evidence) = execute(FixtureCase::Observation, ProviderOutcome::Failure);
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
    let failure = commands.failure.as_ref().expect("failure should finalize");
    assert!(format!("{failure:?}").contains("provider_server_error"));
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

struct DispatcherMaterial;

impl ScheduledSourceMaterialComparison for DispatcherMaterial {
    type Error = FixtureError;

    /// compare decoded tide material through its generated representation.
    fn values_equal(&mut self, left: &JsonValue, right: &JsonValue) -> Result<bool, Self::Error> {
        Ok(left == right)
    }

    /// compare one decoded tide value with its ordered object entries.
    fn value_and_object_equal(
        &mut self,
        value: &JsonValue,
        object: &[(Vec<u16>, JsonValue)],
    ) -> Result<bool, Self::Error> {
        Ok(matches!(value, JsonValue::Object(entries) if entries == object))
    }
}

impl WorkerSourceMaterialIdentity for DispatcherMaterial {
    /// retain the exact generated observation material once at construction.
    fn material_identity(&mut self, value: &JsonValue) -> Result<String, TaskFailure> {
        assert_eq!(value, &JsonValue::Object(observation_adapter_config()));
        Ok("noaa-observation-material-1".to_owned())
    }
}

struct DispatcherTransport {
    commands: Rc<RefCell<CapturedCommands>>,
    requests: Rc<RefCell<Vec<weather_worker::NoaaTideRangeRequest>>>,
}

impl WorkerNoaaTideTransport for DispatcherTransport {
    type Exchange<'a>
        = FixtureProvider
    where
        Self: 'a;

    /// bind the observation exchange only after the running row exists.
    fn bind_request(
        &mut self,
        request: &weather_worker::NoaaTideRangeRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure> {
        self.commands
            .borrow_mut()
            .trace
            .push("provider-bind".to_owned());
        self.requests.borrow_mut().push(request.clone());
        Ok(FixtureProvider {
            attempts: Vec::new(),
            case: FixtureCase::Observation,
            commands: Rc::clone(&self.commands),
            now_ms: NOW_MS,
            outcomes: VecDeque::from([
                Ok(provider_response(
                    FixtureCase::Observation,
                    503,
                    Some("temporary outage"),
                )),
                Ok(provider_response(FixtureCase::Observation, 200, None)),
            ]),
            random: VecDeque::from([0.5]),
            sleeps: Vec::new(),
        })
    }
}

struct RejectOther;

impl<Repository> WorkerScheduledSourceDispatcher<Repository> for RejectOther {
    /// reject accidental fallback routing for the exact NOAA source.
    fn run_source<C: FnMut() -> i64>(
        &mut self,
        _repository: &mut Repository,
        _source: &WorkerDueSource,
        _success_clock: &mut C,
    ) -> Result<weather_worker::SourceRunResult, TaskFailure> {
        panic!("NOAA fixture must not delegate")
    }
}

/// return the exact generated NOAA observation adapter object.
fn observation_adapter_config() -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        (
            utf16("contractVersion"),
            json_string(NOAA_TIDE_OBSERVATION_ADAPTER_VERSION),
        ),
        (utf16("datum"), json_string("MLLW")),
        (utf16("product"), json_string("water_level")),
        (
            utf16("sample"),
            json_string("six-minute-verified-water-level"),
        ),
        (utf16("stationId"), json_string("9444900")),
    ]
}

/// build one generated NOAA observation station catalog.
fn dispatcher_configuration() -> TideConfiguration {
    TideConfiguration {
        provider: TideProviderConfiguration {
            active: true,
            attributionLabel: "NOAA CO-OPS".to_owned(),
            attributionUrl: "https://tidesandcurrents.noaa.gov".to_owned(),
            displayName: "NOAA CO-OPS".to_owned(),
            key: "noaa-co-ops".to_owned(),
        },
        siteKey: "ballydidean".to_owned(),
        stations: vec![TideStationConfiguration {
            active: true,
            displayName: "Port Townsend Tide Gauge".to_owned(),
            key: "port-townsend-tide-gauge".to_owned(),
            kind: StationKind::Physical,
            latitude: 48.11,
            longitude: -122.76,
            model: "Water Level Station".to_owned(),
            serial: "9444900".to_owned(),
            source: TideSourceConfiguration {
                active: true,
                adapterConfig: observation_adapter_config(),
                cadenceSeconds: 360,
                capabilities: vec![SourceCapability::Current, SourceCapability::Historical],
                fingerprint: fingerprint(FixtureCase::Observation).to_owned(),
                historyStartDate: "2026-01-01".to_owned(),
                key: "noaa-port-townsend-water-level-v1".to_owned(),
                // backfill may use smaller chunks without disabling scheduled observations
                maximumChunkDays: 7,
                sourceKind: TideSourceConfigurationSourceKindEnum::TideObservation,
            },
            timezone: "America/Los_Angeles".to_owned(),
            vendor: "NOAA CO-OPS".to_owned(),
        }],
        version: 1,
    }
}

/// dispatch a lagged observation through one shared session and request-bound exchange.
#[test]
fn dispatches_noaa_observation_after_running_row_on_the_shared_database() {
    let commands = Rc::new(RefCell::new(CapturedCommands::default()));
    let requests = Rc::new(RefCell::new(Vec::new()));
    let mut database = FixtureDatabase {
        case: FixtureCase::Observation,
        commands: Rc::clone(&commands),
    };
    let mut values = FixtureValues {
        case: FixtureCase::Observation,
    };
    let mut repository = WorkerIterationDatabaseRepository::new(&mut database, &mut values);
    let mut dispatcher = WorkerNoaaTideDispatcher::from_configuration(
        &dispatcher_configuration(),
        &mut DispatcherMaterial,
        DispatcherTransport {
            commands: Rc::clone(&commands),
            requests: Rc::clone(&requests),
        },
        RejectOther,
    )
    .unwrap();
    let (source, _) = source_configuration(FixtureCase::Observation);
    let mut clock = VecDeque::from([NOW_MS - 1, NOW_MS, NOW_MS + 4_000, NOW_MS + 5_000]);
    let result = dispatcher
        .run_source(
            &mut repository,
            &WorkerDueSource::NoaaTide(source),
            &mut || clock.pop_front().expect("fixture clock exhausted"),
        )
        .unwrap();

    assert_eq!(result.status, SourceRunStatus::Succeeded);
    assert_eq!(result.attempts, 2);
    assert_eq!(result.successful_at_ms, Some(NOW_MS + 5_000));
    assert_eq!(
        requests.borrow().as_slice(),
        [weather_worker::NoaaTideRangeRequest {
            end_exclusive_ms: OBS_END_MS,
            source_id: FixtureCase::Observation.source_id().to_owned(),
            source_kind: NoaaTideSourceKind::Observation,
            start_ms: OBS_START_MS,
            station_id: "9444900".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }]
    );
    let commands = commands.borrow();
    assert_eq!(
        commands.trace,
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
    assert_eq!(
        commands
            .insert
            .as_ref()
            .expect("start should persist")
            .binds[7],
        InsertScheduledIngestionBind::UtcInstant(DISPATCHER_DEADLINE.to_owned())
    );
}

/// return one exact product source fingerprint
const fn fingerprint(case: FixtureCase) -> &'static str {
    // preserve distinct checked source identities
    match case {
        FixtureCase::Observation => {
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }
        FixtureCase::Prediction => {
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        }
    }
}

/// return one exact durable cursor JSON value
const fn cursor_json(case: FixtureCase) -> &'static str {
    // preserve the final valid provider instant
    match case {
        FixtureCase::Observation => r#"{"valid_at":"2026-08-29T13:42:00.000Z"}"#,
        FixtureCase::Prediction => r#"{"valid_at":"2026-08-30T02:00:00.000Z"}"#,
    }
}

/// return one exact NOAA provider metadata JSON value
const fn provider_json(case: FixtureCase) -> &'static str {
    // preserve product-specific provider provenance
    match case {
        FixtureCase::Observation => {
            r#"{"dataset":"water_level","datum":"MLLW","prediction_type":null,"product":"water_level","station_id":"9444900"}"#
        }
        FixtureCase::Prediction => {
            r#"{"dataset":"predictions","datum":"MLLW","prediction_type":"H","product":"predictions","station_id":"9447814"}"#
        }
    }
}

/// return one exact NOAA quality JSON value
const fn quality_json(case: FixtureCase) -> &'static str {
    // preserve product-specific quality evidence
    match case {
        FixtureCase::Observation => r#"{"flags":["0","0","0","0"],"status":"v"}"#,
        FixtureCase::Prediction => r#"{"sampling":"high_low_prediction"}"#,
    }
}

/// return one exact NOAA response metadata JSON value
const fn response_json(case: FixtureCase) -> &'static str {
    // preserve product-specific response evidence
    match case {
        FixtureCase::Observation => {
            r#"{"datum":"MLLW","normalized_record_count":1,"product":"water_level","provider":"noaa-co-ops","station_id":"9444900"}"#
        }
        FixtureCase::Prediction => {
            r#"{"datum":"MLLW","normalized_record_count":1,"product":"predictions","provider":"noaa-co-ops","station_id":"9447814"}"#
        }
    }
}

/// build exact expected NOAA provider metadata
fn expected_provider_metadata(case: FixtureCase) -> Vec<(Vec<u16>, JsonValue)> {
    let (product, prediction_type) = match case {
        // preserve observation null and prediction event labels
        FixtureCase::Observation => ("water_level", JsonValue::Null),
        FixtureCase::Prediction => ("predictions", json_string("H")),
    };
    vec![
        (utf16("dataset"), json_string(product)),
        (utf16("datum"), json_string("MLLW")),
        (utf16("prediction_type"), prediction_type),
        (utf16("product"), json_string(product)),
        (utf16("station_id"), json_string(case.station_id())),
    ]
}

/// build exact expected NOAA quality metadata
fn expected_quality_metadata(case: FixtureCase) -> Vec<(Vec<u16>, JsonValue)> {
    // preserve each product's quality object order
    match case {
        FixtureCase::Observation => vec![
            (
                utf16("flags"),
                JsonValue::Array(["0", "0", "0", "0"].map(json_string).to_vec()),
            ),
            (utf16("status"), json_string("v")),
        ],
        FixtureCase::Prediction => {
            vec![(utf16("sampling"), json_string("high_low_prediction"))]
        }
    }
}

/// build exact expected NOAA response metadata
fn expected_response_metadata(case: FixtureCase) -> Vec<(Vec<u16>, JsonValue)> {
    let product = match case {
        // preserve the exact product wire value
        FixtureCase::Observation => "water_level",
        FixtureCase::Prediction => "predictions",
    };
    vec![
        (utf16("datum"), json_string("MLLW")),
        (utf16("normalized_record_count"), JsonValue::Number(1.0)),
        (utf16("product"), json_string(product)),
        (utf16("provider"), json_string("noaa-co-ops")),
        (utf16("station_id"), json_string(case.station_id())),
    ]
}

/// encode one JSON fixture string
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(utf16(value))
}

/// encode one fixture string as JavaScript UTF-16 code units
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}
