use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use weather_contracts::{
    FieldPresence, IngestionError, IngestionErrorClassification, IngestionState, JsonValue,
    Nullable, RainHurdleWindPortableArtifactV1Heads,
    RainHurdleWindPortableArtifactV1HeadsNativeHead,
    RainHurdleWindPortableArtifactV1HeadsObjectiveEnum as RainObjective,
    RainHurdleWindPortableArtifactV1Rules, ScheduledCheckpointState, SiteConfiguration,
    SiteConfigurationProvider, SiteConfigurationSite, SiteConfigurationSource,
    SiteConfigurationStation, SourceCapability, SourceKind, StationKind,
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
    ContentBudget, ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse,
    ProviderControls, ProviderIo, ProviderResponse, ProviderSleep, ProviderSleepError,
};
use weather_runtime::{
    OpenMeteoAttemptPayload, OpenMeteoDatabaseAdapter, OpenMeteoDatabaseBoundary,
    OpenMeteoDatabaseRepository, OpenMeteoForecastAirQualityProduct,
    OpenMeteoForecastEvidenceValues, OpenMeteoForecastPair, OpenMeteoForecastWeatherProduct,
    OpenMeteoParsedProduct, OpenMeteoRuntimeValues, OpenMeteoWorkerLocation,
    RetainedWorkerSidecarTask, RetryingOpenMeteoProvider, RuntimePersistenceOperation,
    RuntimeTimeValue, ScheduledDatabaseAdapter, WorkerIterationDatabasePort,
    WorkerIterationDatabaseRepository, WorkerOpenMeteoDispatcher, WorkerOpenMeteoForecastTransport,
    WorkerOpenMeteoTransport, WorkerRainAfterServing, WorkerRainInferenceModel,
    WorkerRainInferenceRepositoryBinding, WorkerRainInferenceServing, WorkerRainInferenceTask,
    WorkerSourceMaterialIdentity,
};
use weather_worker::{
    AdmittedRainHurdleWindCalendar, FixedGaugeTargetResult, MaintenanceCaptureDue,
    NonOverlappingGate, OPEN_METEO_CURRENT_ADAPTER_VERSION, OPEN_METEO_FORECAST_ADAPTER_VERSION,
    OpenMeteoAirQualitySample, OpenMeteoCapability, OpenMeteoCursor, OpenMeteoEnvelope,
    OpenMeteoRuntimeConfiguration, OpenMeteoScheduledSource, OpenMeteoSourceKind,
    OpenMeteoWeatherSample, RAIN_COLLECTION_POLICY, RAIN_COLLECTION_STATIONS,
    RAIN_FIXED_GAUGE_TARGET_CATALOG_CONTRACT_VERSION, RAIN_FIXED_GAUGE_TARGET_CATALOG_SHA256,
    RAIN_FIXED_GAUGE_TARGET_RUN_CONTRACT_SHA256, RainAdjustmentAppendTransaction,
    RainAdjustmentClient, RainAdjustmentCommitReceipt, RainAdjustmentEvaluation,
    RainAdjustmentFeatureAdmission, RainAdjustmentGap, RainAdjustmentIdentityAdapter,
    RainAdjustmentModel, RainAdjustmentPendingInput, RainAdjustmentPersistedRevision,
    RainAdjustmentProjection, RainAdjustmentProjectionCodec, RainAdjustmentRepository,
    RainAdjustmentRevisionPlan, RainAdjustmentRun, RainAdjustmentRuntimeSelection,
    RainAdjustmentStageReceipt, RainAdjustmentTimeAdapter, RainCaptureOutcome, RainCaptureReceipt,
    RainCaptureRequest, RainCaptureResult, RainCollectionClock, RainCollectionExecution,
    RainCollectionPacer, RainCollectionProvider, RainCollectionRepository,
    RainCollectionTimeAdapter, RainCollectionTimeValue, RainFixedGaugeBuildResult,
    RainFixedGaugeCapture, RainFixedGaugeClient, RainFixedGaugeCommitReceipt,
    RainFixedGaugeDurableGap, RainFixedGaugeGapReason, RainFixedGaugeGapStatus,
    RainFixedGaugeIdentityAdapter, RainFixedGaugeNormalizedRecord, RainFixedGaugeProjectionBuilder,
    RainFixedGaugeRepository, RainFixedGaugeRevisionGap, RainFixedGaugeRevisionGroup,
    RainFixedGaugeRevisionResult, RainFixedGaugeSource, RainFixedGaugeSourceCatalog,
    RainFixedGaugeStageReceipt, RainFixedGaugeTargetExecution, ScheduledRunExecution,
    ScheduledSourceMaterialComparison, SourceRunResult, SourceRunStatus, TaskFailure,
    TemperatureCanaryBatch, TemperatureCanaryCollectionResult, TemperatureCanaryExecution,
    TemperatureCanaryFetchControls, TemperatureCanaryFetchLimits, TemperatureCanaryHour,
    TemperatureCanaryPersistedRun, TemperatureCanaryProvider, TemperatureCanaryRecentErrorBuilder,
    TemperatureCanaryRecentErrorState, TemperatureCanaryRepository, TemperatureCanaryRequest,
    TemperatureCanarySource, TemperatureCanaryTimeAdapter, VerifiedTemperatureCanaryCollection,
    WorkerActivity, WorkerContinuation, WorkerDueSource, WorkerHeartbeat, WorkerIterationRun,
    WorkerScheduledSourceDispatcher, WorkerSidecarDiagnostic, WorkerSidecarDiagnosticSink,
    WorkerSidecarExecution, WorkerSidecarPlan, WorkerSidecarTasks,
    adjustment_maintenance_capture_due_at, collect_ecmwf_temperature_canary_runs,
    collect_rain_evidence, publish_rain_fixed_gauge_target, run_scheduled_source,
    run_worker_iteration, run_worker_iteration_once_with_sidecars,
};

const SOURCE_ID: &str = "source-1";
const FORECAST_SOURCE_ID: &str = "source-forecast";
const FINGERPRINT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const RESPONSE_CHECKSUM: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CONTENT_HASH: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const WEATHER_CHECKSUM: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const AIR_QUALITY_CHECKSUM: &str =
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const FORECAST_RESPONSE_METADATA: &str = r#"{"air_quality_http_status":206,"air_quality_generation_ms":7.25,"http_status":200,"upstream_response_count":2,"weather_generation_ms":12.5}"#;
const WINDOW_START: &str = "1970-01-01T00:28:00.000Z";
const VALID_AT: &str = "1970-01-01T00:29:50.000Z";
const RECEIVED_AT: &str = "1970-01-01T00:29:55.000Z";
const WINDOW_END: &str = "1970-01-01T00:30:00.000Z";
const DEADLINE_AT: &str = "1970-01-01T00:31:00.000Z";

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
    checkpoint: bool,
    expected_attempts: u64,
    expected_failure_classification: IngestionErrorClassification,
    expected_failure_code: String,
    source_id: String,
    trace: Rc<RefCell<Vec<String>>>,
}

impl RetainedSourceSession for FixtureSession {
    /// return the immutable retained source identity
    fn source_id(&self) -> &str {
        &self.source_id
    }
}

impl ScheduledCheckpointReadPort for FixtureSession {
    type Error = FixtureError;

    /// return the typed checkpoint through its exact source bind
    fn get_scheduled_checkpoint(
        &mut self,
        query: &GetScheduledCheckpointQuery,
    ) -> Result<Option<ScheduledCheckpointDbRow>, Self::Error> {
        assert_eq!(
            query.binds,
            [GetScheduledCheckpointBind::BigintText(
                self.source_id.clone()
            )]
        );
        self.trace.borrow_mut().push("checkpoint".to_owned());

        // model a source with or without durable history
        if !self.checkpoint {
            return Ok(None);
        }

        let provider_cursor = if self.source_id == FORECAST_SOURCE_ID {
            vec![(utf16("product_run_at"), json_string(RECEIVED_AT))]
        } else {
            vec![(utf16("valid_at"), json_string(WINDOW_START))]
        };
        Ok(Some(ScheduledCheckpointDbRow {
            last_committed_at: WINDOW_START.to_owned(),
            last_valid_at: WINDOW_START.to_owned(),
            provider_cursor: Nullable::Value(provider_cursor),
            source_id: self.source_id.clone(),
            version: 7.0,
            window_end_exclusive: WINDOW_START.to_owned(),
            window_start: "1970-01-01T00:27:00.000Z".to_owned(),
        }))
    }
}

impl ScheduledRunStartCommandPort for FixtureSession {
    type Error = FixtureError;

    /// verify the source-scoped running-row lookup
    fn find_running_ingestion(
        &mut self,
        query: &FindRunningIngestionQuery,
    ) -> Result<RunningIngestionLookup, Self::Error> {
        assert_eq!(
            query.binds,
            [FindRunningIngestionBind::BigintText(self.source_id.clone())]
        );
        self.trace.borrow_mut().push("start-lookup".to_owned());
        Ok(RunningIngestionLookup {
            first_run_id: None,
            row_count: 0,
        })
    }

    /// verify and return the committed running row
    fn insert_scheduled_ingestion(
        &mut self,
        query: &InsertScheduledIngestionQuery,
    ) -> Result<Option<StartedScheduledIngestion>, Self::Error> {
        let (adapter_version, endpoint) = if self.source_id == FORECAST_SOURCE_ID {
            (
                OPEN_METEO_FORECAST_ADAPTER_VERSION,
                r#"{"endpoint":"forecast/hourly"}"#,
            )
        } else {
            (
                OPEN_METEO_CURRENT_ADAPTER_VERSION,
                r#"{"endpoint":"forecast/current"}"#,
            )
        };
        assert_eq!(
            query.binds,
            [
                InsertScheduledIngestionBind::BigintText(self.source_id.clone()),
                InsertScheduledIngestionBind::ScheduledMode,
                InsertScheduledIngestionBind::UtcInstant(WINDOW_START.to_owned()),
                InsertScheduledIngestionBind::UtcInstant(WINDOW_END.to_owned()),
                InsertScheduledIngestionBind::Fingerprint(Sha256Digest::admitted(FINGERPRINT)),
                InsertScheduledIngestionBind::Text(adapter_version.to_owned()),
                InsertScheduledIngestionBind::NullChunkPlanVersion,
                InsertScheduledIngestionBind::UtcInstant(DEADLINE_AT.to_owned()),
                InsertScheduledIngestionBind::Count(0),
                InsertScheduledIngestionBind::NullableJson(Some(SerializedJson::admitted(
                    endpoint
                ))),
            ]
        );
        self.trace.borrow_mut().push("start-insert".to_owned());
        Ok(Some(StartedScheduledIngestion {
            id: "run-1".to_owned(),
            started_at: WINDOW_END.to_owned(),
            state: IngestionState::Running,
        }))
    }
}

impl ScheduledIngestionCommandPort for FixtureSession {
    type Error = FixtureError;

    /// record the atomic completion start
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("complete-begin".to_owned());
        Ok(())
    }

    /// verify the running row uses only the retained source
    fn assert_scheduled_run(
        &mut self,
        query: &ScheduledRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(
            query.binds,
            [
                ScheduledRunIdentityBind::BigintText("run-1".to_owned()),
                ScheduledRunIdentityBind::BigintText(self.source_id.clone()),
                ScheduledRunIdentityBind::UtcInstant(WINDOW_START.to_owned()),
                ScheduledRunIdentityBind::UtcInstant(WINDOW_END.to_owned()),
            ]
        );
        self.trace.borrow_mut().push("complete-assert".to_owned());
        Ok(1)
    }

    /// verify the normalized record reaches the exact database bind plan
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.record_count, 1);
        assert_eq!(query.binds.len(), 32);
        // retain an independent exact storage projection for the paired forecast result
        if self.source_id == FORECAST_SOURCE_ID {
            assert_eq!(
                query.binds,
                [
                    WeatherRecordBind::BigintText(FORECAST_SOURCE_ID.to_owned()),
                    WeatherRecordBind::SourceKind(SourceKind::Forecast),
                    WeatherRecordBind::UtcInstant(VALID_AT.to_owned()),
                    WeatherRecordBind::NullableUtcInstant(Some(RECEIVED_AT.to_owned())),
                    WeatherRecordBind::BigintText("run-1".to_owned()),
                    WeatherRecordBind::UtcInstant(RECEIVED_AT.to_owned()),
                    WeatherRecordBind::Text("Europe/London".to_owned()),
                    WeatherRecordBind::NullableText(Some("best_match".to_owned())),
                    WeatherRecordBind::NullableText(None),
                    WeatherRecordBind::NullableText(None),
                    WeatherRecordBind::NullableText(None),
                    WeatherRecordBind::NullableJson(None),
                    WeatherRecordBind::NullableJson(Some(SerializedJson::admitted(
                        r#"{"dataset":"forecast","elevation_m":17.0,"grid_cell":"54,-4"}"#,
                    ))),
                    WeatherRecordBind::NullableNumber(Some(16.4)),
                    WeatherRecordBind::NullableNumber(Some(15.8)),
                    WeatherRecordBind::NullableNumber(Some(0.2)),
                    WeatherRecordBind::NullableNumber(Some(4.3)),
                    WeatherRecordBind::NullableNumber(Some(7.1)),
                    WeatherRecordBind::NullableNumber(Some(1_012.7)),
                    WeatherRecordBind::NullableNumber(Some(81.0)),
                    WeatherRecordBind::NullableNumber(Some(65.0)),
                    WeatherRecordBind::NullableNumber(Some(270.0)),
                    WeatherRecordBind::NullableNumber(None),
                    WeatherRecordBind::NullableNumber(Some(5.5)),
                    WeatherRecordBind::NullableNumber(Some(0.2)),
                    WeatherRecordBind::NullableNumber(None),
                    WeatherRecordBind::NullableNumber(None),
                    WeatherRecordBind::NullableNumber(None),
                    WeatherRecordBind::NullableNumber(Some(4.0)),
                    WeatherRecordBind::NullableNumber(None),
                    WeatherRecordBind::NullableNumber(None),
                    WeatherRecordBind::ContentHash(Sha256Digest::admitted(CONTENT_HASH)),
                ]
            );
            self.trace.borrow_mut().push("records".to_owned());
            return Ok(());
        }
        assert_eq!(
            query.binds,
            [
                WeatherRecordBind::BigintText(SOURCE_ID.to_owned()),
                WeatherRecordBind::SourceKind(SourceKind::ModelCurrent),
                WeatherRecordBind::UtcInstant(VALID_AT.to_owned()),
                WeatherRecordBind::NullableUtcInstant(None),
                WeatherRecordBind::BigintText("run-1".to_owned()),
                WeatherRecordBind::UtcInstant(RECEIVED_AT.to_owned()),
                WeatherRecordBind::Text("Europe/London".to_owned()),
                WeatherRecordBind::NullableText(Some("best_match".to_owned())),
                WeatherRecordBind::NullableText(None),
                WeatherRecordBind::NullableText(None),
                WeatherRecordBind::NullableText(None),
                WeatherRecordBind::NullableJson(None),
                WeatherRecordBind::NullableJson(Some(SerializedJson::admitted(
                    r#"{"dataset":"forecast","elevation_m":17.0,"grid_cell":"54,-4"}"#,
                ))),
                WeatherRecordBind::NullableNumber(Some(16.4)),
                WeatherRecordBind::NullableNumber(Some(15.8)),
                WeatherRecordBind::NullableNumber(Some(0.2)),
                WeatherRecordBind::NullableNumber(Some(4.3)),
                WeatherRecordBind::NullableNumber(Some(7.1)),
                WeatherRecordBind::NullableNumber(Some(1_012.7)),
                WeatherRecordBind::NullableNumber(Some(81.0)),
                WeatherRecordBind::NullableNumber(Some(65.0)),
                WeatherRecordBind::NullableNumber(Some(270.0)),
                WeatherRecordBind::NullableNumber(None),
                WeatherRecordBind::NullableNumber(None),
                WeatherRecordBind::NullableNumber(None),
                WeatherRecordBind::NullableNumber(None),
                WeatherRecordBind::NullableNumber(None),
                WeatherRecordBind::NullableNumber(None),
                WeatherRecordBind::NullableNumber(None),
                WeatherRecordBind::NullableNumber(None),
                WeatherRecordBind::NullableNumber(None),
                WeatherRecordBind::ContentHash(Sha256Digest::admitted(CONTENT_HASH)),
            ]
        );
        self.trace.borrow_mut().push("records".to_owned());
        Ok(())
    }

    /// verify the checkpoint compare-and-set and cursor evidence
    fn advance_scheduled_checkpoint(
        &mut self,
        query: &ScheduledCheckpointQuery,
    ) -> Result<usize, Self::Error> {
        // use the selected window end and product-run cursor for forecast durability
        if self.source_id == FORECAST_SOURCE_ID {
            assert_eq!(
                query,
                &ScheduledCheckpointQuery::Update {
                    sql: weather_database::UPDATE_SCHEDULED_CHECKPOINT_SQL,
                    binds: [
                        CheckpointUpdateBind::BigintText(FORECAST_SOURCE_ID.to_owned()),
                        CheckpointUpdateBind::Version(7),
                        CheckpointUpdateBind::UtcInstant(WINDOW_END.to_owned()),
                        CheckpointUpdateBind::UtcInstant(WINDOW_START.to_owned()),
                        CheckpointUpdateBind::UtcInstant(WINDOW_END.to_owned()),
                        CheckpointUpdateBind::NullableJson(Some(SerializedJson::admitted(
                            r#"{"product_run_at":"1970-01-01T00:29:55.000Z"}"#,
                        ))),
                    ],
                }
            );
            self.trace.borrow_mut().push("checkpoint-update".to_owned());
            return Ok(1);
        }
        assert_eq!(
            query,
            &ScheduledCheckpointQuery::Update {
                sql: weather_database::UPDATE_SCHEDULED_CHECKPOINT_SQL,
                binds: [
                    CheckpointUpdateBind::BigintText(SOURCE_ID.to_owned()),
                    CheckpointUpdateBind::Version(7),
                    CheckpointUpdateBind::UtcInstant(VALID_AT.to_owned()),
                    CheckpointUpdateBind::UtcInstant(WINDOW_START.to_owned()),
                    CheckpointUpdateBind::UtcInstant(WINDOW_END.to_owned()),
                    CheckpointUpdateBind::NullableJson(Some(SerializedJson::admitted(
                        r#"{"valid_at":"1970-01-01T00:29:50.000Z"}"#,
                    ))),
                ],
            }
        );
        self.trace.borrow_mut().push("checkpoint-update".to_owned());
        Ok(1)
    }

    /// verify successful finalization evidence
    fn finalize_scheduled_run(
        &mut self,
        query: &FinalizeScheduledRunQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(
            query.binds,
            [
                FinalizeScheduledRunBind::BigintText("run-1".to_owned()),
                FinalizeScheduledRunBind::BigintText(self.source_id.clone()),
                FinalizeScheduledRunBind::Count(self.expected_attempts),
                FinalizeScheduledRunBind::Count(1),
                FinalizeScheduledRunBind::NullableJson(Some(SerializedJson::admitted(
                    if self.source_id == FORECAST_SOURCE_ID {
                        FORECAST_RESPONSE_METADATA
                    } else {
                        r#"{"request_id":"response-1"}"#
                    },
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

    /// record the successful completion commit
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("complete-commit".to_owned());
        Ok(())
    }

    /// record an unexpected completion rollback
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("complete-rollback".to_owned());
        Ok(())
    }
}

impl ScheduledRunFailureCommandPort for FixtureSession {
    type Error = FixtureError;

    /// record failure finalization start
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("failure-begin".to_owned());
        Ok(())
    }

    /// verify the failed run belongs to the retained source
    fn assert_running_scheduled(
        &mut self,
        query: &AssertRunningScheduledQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(
            query.binds,
            [
                AssertRunningScheduledBind::BigintText("run-1".to_owned()),
                AssertRunningScheduledBind::BigintText(self.source_id.clone()),
                AssertRunningScheduledBind::ScheduledMode,
            ]
        );
        self.trace.borrow_mut().push("failure-assert".to_owned());
        Ok(1)
    }

    /// verify provider failure classification and attempts
    fn finalize_failed_ingestion(
        &mut self,
        query: &FinalizeFailedIngestionQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(
            query.binds,
            [
                FinalizeFailedIngestionBind::BigintText("run-1".to_owned()),
                FinalizeFailedIngestionBind::BigintText(self.source_id.clone()),
                FinalizeFailedIngestionBind::Count(self.expected_attempts),
                FinalizeFailedIngestionBind::NullableJson(None),
                FinalizeFailedIngestionBind::Classification(self.expected_failure_classification,),
                FinalizeFailedIngestionBind::Text(self.expected_failure_code.clone()),
                FinalizeFailedIngestionBind::Text(self.expected_failure_code.clone()),
            ]
        );
        self.trace.borrow_mut().push("failure-finalize".to_owned());
        Ok(1)
    }

    /// record the failure finalization commit
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("failure-commit".to_owned());
        Ok(())
    }

    /// record an unexpected failure finalization rollback
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("failure-rollback".to_owned());
        Ok(())
    }
}

struct FixtureDatabase {
    checkpoint: bool,
    discover_at_ms: Vec<i64>,
    due_sources: Vec<WorkerDueSource>,
    expected_attempts: u64,
    expected_failure_classification: IngestionErrorClassification,
    expected_failure_code: String,
    heartbeats: Vec<WorkerHeartbeat>,
    retained_source_id: String,
    trace: Rc<RefCell<Vec<String>>>,
}

impl WorkerIterationDatabasePort for FixtureDatabase {
    type Error = FixtureError;

    /// Return due rows from the same database used by the scheduled transaction.
    fn discover_due_sources(&mut self, as_of_ms: i64) -> Result<Vec<WorkerDueSource>, Self::Error> {
        self.trace.borrow_mut().push("discover".to_owned());
        self.discover_at_ms.push(as_of_ms);
        Ok(std::mem::take(&mut self.due_sources))
    }

    /// Retain the final heartbeat on the same fixture database.
    fn update_worker_heartbeat(&mut self, heartbeat: WorkerHeartbeat) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("heartbeat".to_owned());
        self.heartbeats.push(heartbeat);
        Ok(())
    }
}

impl OpenMeteoDatabaseBoundary for FixtureDatabase {
    type Session = FixtureSession;

    /// acquire one fixture retained source session
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure> {
        assert!(source_id == SOURCE_ID || source_id == FORECAST_SOURCE_ID);
        self.trace.borrow_mut().push("acquire".to_owned());
        Ok(Some(FixtureSession {
            checkpoint: self.checkpoint,
            expected_attempts: self.expected_attempts,
            expected_failure_classification: self.expected_failure_classification,
            expected_failure_code: self.expected_failure_code.clone(),
            source_id: self.retained_source_id.clone(),
            trace: Rc::clone(&self.trace),
        }))
    }

    /// record expired-run cleanup at the injected instant
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        as_of: &str,
    ) -> Result<(), TaskFailure> {
        assert_eq!(session.source_id(), self.retained_source_id);
        assert_eq!(as_of, WINDOW_END);
        self.trace.borrow_mut().push("abandon".to_owned());
        Ok(())
    }

    /// release the exact acquired session
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
        assert!(!session.source_id().is_empty());
        self.trace.borrow_mut().push("release".to_owned());
        Ok(())
    }
}

#[derive(Default)]
struct FixtureValues;

impl OpenMeteoRuntimeValues for FixtureValues {
    /// resolve only explicit source-derived timestamp pairs
    fn utc_instant(
        &mut self,
        milliseconds: i64,
        value: RuntimeTimeValue,
    ) -> Result<String, TaskFailure> {
        let instant = match (milliseconds, value) {
            (1_680_000, RuntimeTimeValue::RequestedStart) => WINDOW_START,
            (1_790_000, RuntimeTimeValue::LastValidAt | RuntimeTimeValue::ValidAt) => VALID_AT,
            (1_795_000, RuntimeTimeValue::ProductRunAt | RuntimeTimeValue::ReceivedAt) => {
                RECEIVED_AT
            }
            (
                1_800_000,
                RuntimeTimeValue::AbandonAsOf
                | RuntimeTimeValue::LastValidAt
                | RuntimeTimeValue::RequestedEndExclusive,
            ) => WINDOW_END,
            (1_860_000, RuntimeTimeValue::Deadline) => DEADLINE_AT,
            _ => panic!("unexpected UTC boundary: {milliseconds:?} {value:?}"),
        };
        Ok(instant.to_owned())
    }

    /// resolve the one explicit checkpoint instant pair
    fn epoch_milliseconds(
        &mut self,
        instant: &str,
        value: RuntimeTimeValue,
    ) -> Result<i64, TaskFailure> {
        // admit the checkpoint window and typed current cursor independently
        match (instant, value) {
            (WINDOW_START, RuntimeTimeValue::CheckpointWindowEnd | RuntimeTimeValue::ValidAt) => {
                Ok(1_680_000)
            }
            (RECEIVED_AT, RuntimeTimeValue::ProductRunAt) => Ok(1_795_000),
            _ => panic!("unexpected epoch boundary: {instant:?} {value:?}"),
        }
    }

    /// serialize only the exact provider metadata fixture
    fn serialize_json_object(
        &mut self,
        value: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure> {
        assert_eq!(
            value,
            [
                (utf16("dataset"), json_string("forecast")),
                (utf16("elevation_m"), JsonValue::Number(17.0)),
                (utf16("grid_cell"), json_string("54,-4")),
            ]
        );
        Ok(SerializedJson::admitted(
            r#"{"dataset":"forecast","elevation_m":17.0,"grid_cell":"54,-4"}"#,
        ))
    }

    /// serialize the exact provider endpoint evidence
    fn request_metadata_json(&mut self, endpoint: &str) -> Result<SerializedJson, TaskFailure> {
        // preserve the product-specific endpoint committed before provider traffic
        match endpoint {
            "forecast/current" => Ok(SerializedJson::admitted(
                r#"{"endpoint":"forecast/current"}"#,
            )),
            "forecast/hourly" => Ok(SerializedJson::admitted(
                r#"{"endpoint":"forecast/hourly"}"#,
            )),
            _ => panic!("unexpected Open-Meteo endpoint: {endpoint}"),
        }
    }

    /// serialize the exact worker completion cursor
    fn provider_cursor_json(
        &mut self,
        cursor: Option<&OpenMeteoCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        // preserve null and select the exact cursor-specific timestamp field
        match cursor {
            None => Ok(None),
            Some(OpenMeteoCursor::ValidAt(valid_at_ms)) => {
                let instant = self.utc_instant(*valid_at_ms, RuntimeTimeValue::ValidAt)?;
                assert_eq!(instant, VALID_AT);
                Ok(Some(SerializedJson::admitted(
                    r#"{"valid_at":"1970-01-01T00:29:50.000Z"}"#,
                )))
            }
            Some(OpenMeteoCursor::ProductRunAt(product_run_at_ms)) => {
                let instant =
                    self.utc_instant(*product_run_at_ms, RuntimeTimeValue::ProductRunAt)?;
                assert_eq!(instant, RECEIVED_AT);
                Ok(Some(SerializedJson::admitted(
                    r#"{"product_run_at":"1970-01-01T00:29:55.000Z"}"#,
                )))
            }
        }
    }

    /// decode the exact durable checkpoint cursor
    fn checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<OpenMeteoCursor>, TaskFailure> {
        assert!(checkpoint.sourceId == SOURCE_ID || checkpoint.sourceId == FORECAST_SOURCE_ID);

        // decode only the exact admitted Node cursor variants
        match &checkpoint.providerCursor {
            Nullable::Null => Ok(None),
            Nullable::Value(fields)
                if fields == &[(utf16("valid_at"), json_string(WINDOW_START))] =>
            {
                let valid_at_ms =
                    self.epoch_milliseconds(WINDOW_START, RuntimeTimeValue::ValidAt)?;
                Ok(Some(OpenMeteoCursor::ValidAt(valid_at_ms)))
            }
            Nullable::Value(fields)
                if fields == &[(utf16("product_run_at"), json_string(RECEIVED_AT))] =>
            {
                let product_run_at_ms =
                    self.epoch_milliseconds(RECEIVED_AT, RuntimeTimeValue::ProductRunAt)?;
                Ok(Some(OpenMeteoCursor::ProductRunAt(product_run_at_ms)))
            }
            Nullable::Value(fields) => panic!("unexpected OpenMeteo cursor fields: {fields:?}"),
        }
    }

    /// admit the exact normalized current-record hash
    fn weather_record_content_hash(
        &mut self,
        record: &weather_contracts::NormalizedWeatherRecord,
    ) -> Result<Sha256Digest, TaskFailure> {
        assert!(record.sourceId == SOURCE_ID || record.sourceId == FORECAST_SOURCE_ID);
        // bind the generated source kind to the selected fixture product
        if record.sourceId == FORECAST_SOURCE_ID {
            assert_eq!(record.sourceKind, SourceKind::Forecast);
            assert_eq!(record.productRunAt, Nullable::Value(RECEIVED_AT.to_owned()));
            assert_eq!(
                record.metrics.pm25MicrogramsPerCubicMeter,
                Nullable::Value(5.5)
            );
            assert_eq!(record.metrics.uvIndex, Nullable::Value(4.0));
        } else {
            assert_eq!(record.sourceKind, SourceKind::ModelCurrent);
        }
        assert_eq!(record.validAt, VALID_AT);
        assert_eq!(record.receivedAt, RECEIVED_AT);
        assert_eq!(record.metrics.temperatureC, Nullable::Value(16.4));
        Ok(Sha256Digest::admitted(CONTENT_HASH))
    }

    /// admit only the fixture's existing SHA-256 values
    fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure> {
        assert!(value == FINGERPRINT || value == RESPONSE_CHECKSUM);
        Ok(Sha256Digest::admitted(value))
    }

    /// map provider failure through the shared generated error DTO
    fn ingestion_error(
        &mut self,
        failure: &TaskFailure,
    ) -> Result<weather_database::AdmittedIngestionError, TaskFailure> {
        // retain the controller's durable content-budget classification
        let classification = if failure.code == "provider_response_too_large" {
            IngestionErrorClassification::InvalidPayload
        } else {
            IngestionErrorClassification::Retryable
        };
        Ok(weather_database::AdmittedIngestionError::admitted(
            IngestionError {
                classification,
                code: failure.code.clone(),
                message: failure.code.clone(),
                metadata: FieldPresence::Missing,
            },
        ))
    }

    /// map a typed persistence error to one worker failure code
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

#[derive(Clone, Copy)]
enum ProviderOutcome {
    ContentBudgetFailure,
    DeadlineBoundedSuccess,
    Failure,
    Success,
}

struct FixtureProviderIo {
    attempts: Vec<ProviderAttempt>,
    now_ms: i64,
    outcomes:
        VecDeque<Result<ProviderAttemptResponse<OpenMeteoAttemptPayload>, ProviderAttemptError>>,
    random: VecDeque<f64>,
    sleeps: Vec<ProviderSleep>,
    trace: Rc<RefCell<Vec<String>>>,
}

impl ProviderIo<OpenMeteoAttemptPayload> for FixtureProviderIo {
    /// return the deterministic fake wall clock
    fn now_ms(&mut self) -> i64 {
        self.now_ms
    }

    /// return one deterministic jitter input
    fn random_unit(&mut self) -> f64 {
        self.random.pop_front().unwrap_or(0.0)
    }

    /// retain one exact bounded physical attempt and return its typed response
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<OpenMeteoAttemptPayload>, ProviderAttemptError> {
        self.trace
            .borrow_mut()
            .push(format!("provider-attempt-{}", attempt.attempt));
        self.attempts.push(attempt);
        self.outcomes
            .pop_front()
            .unwrap_or_else(|| panic!("fixture provider response missing"))
    }

    /// retain and apply one exact retry sleep
    fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
        self.trace
            .borrow_mut()
            .push(format!("provider-sleep-{}", sleep.attempts));
        self.sleeps.push(sleep);
        self.now_ms += i64::try_from(sleep.delay_ms).expect("fixture delay should fit i64");
        Ok(())
    }
}

/// Physical provider-controller evidence retained after one worker run.
struct ProviderExecutionEvidence {
    attempts: Vec<ProviderAttempt>,
    sleeps: Vec<ProviderSleep>,
    trace: Vec<String>,
}

/// Build one real typed current product for normalization after transport policy.
fn current_attempt_payload() -> OpenMeteoAttemptPayload {
    OpenMeteoAttemptPayload {
        product: OpenMeteoParsedProduct {
            envelope: OpenMeteoEnvelope {
                elevation_m: Some(17.0),
                grid_cell: "54,-4".to_owned(),
                latitude: 54.0,
                longitude: -4.0,
                upstream_timezone: "Europe/London".to_owned(),
            },
            sample: OpenMeteoWeatherSample {
                apparent_temperature_c: Some(15.8),
                cloud_cover_percent: Some(65.0),
                precipitation_mm: Some(0.2),
                pressure_hpa: Some(1_012.7),
                relative_humidity_percent: Some(81.0),
                temperature_c: Some(16.4),
                uv_index: Some(4.0),
                valid_at_ms: 1_790_000,
                wind_direction_degrees: Some(270.0),
                wind_gust_mps: Some(7.1),
                wind_speed_mps: Some(4.3),
            },
        },
        received_at_ms: 1_795_000,
        response_metadata: Some(SerializedJson::admitted(r#"{"request_id":"response-1"}"#)),
        upstream_response_checksum: RESPONSE_CHECKSUM.to_owned(),
    }
}

/// Build one typed physical response with admitted body-budget evidence.
fn provider_response(
    status: u16,
    reason: Option<&str>,
) -> ProviderAttemptResponse<OpenMeteoAttemptPayload> {
    ProviderAttemptResponse {
        body: current_attempt_payload(),
        content: ContentBudget {
            declared_body_bytes: Some(1_024),
            streamed_body_bytes: 1_024,
        },
        provider_reason: reason.map(str::to_owned),
        retry_after_ms: None,
        status,
    }
}

/// Build one successful-status response rejected by the declared content budget.
fn oversized_provider_response() -> ProviderAttemptResponse<OpenMeteoAttemptPayload> {
    let mut response = provider_response(200, None);
    response.content.declared_body_bytes = Some(2_000_001);
    response
}

/// Build one controller-backed provider with success or exhaustion outcomes.
fn provider_fixture(
    outcome: ProviderOutcome,
    trace: Rc<RefCell<Vec<String>>>,
) -> RetryingOpenMeteoProvider<FixtureProviderIo> {
    let outcomes = match outcome {
        ProviderOutcome::ContentBudgetFailure => {
            VecDeque::from([Ok(oversized_provider_response())])
        }
        ProviderOutcome::DeadlineBoundedSuccess => {
            VecDeque::from([Ok(provider_response(200, None))])
        }
        ProviderOutcome::Success => VecDeque::from([
            Ok(provider_response(503, Some("temporary outage"))),
            Ok(provider_response(200, None)),
        ]),
        ProviderOutcome::Failure => VecDeque::from([
            Ok(provider_response(503, Some("temporary outage one"))),
            Ok(provider_response(503, Some("temporary outage two"))),
            Ok(provider_response(503, Some("still unavailable"))),
        ]),
    };
    // start near the deadline only for the bounded-timeout branch
    let now_ms = match outcome {
        ProviderOutcome::DeadlineBoundedSuccess => 1_829_500,
        _ => 1_800_000,
    };
    RetryingOpenMeteoProvider::new(
        FixtureProviderIo {
            attempts: Vec::new(),
            now_ms,
            outcomes,
            random: VecDeque::from([0.5, 0.5]),
            sleeps: Vec::new(),
            trace,
        },
        ProviderControls::default(),
        Some(1_830_000),
    )
}

struct DispatcherMaterial;

impl ScheduledSourceMaterialComparison for DispatcherMaterial {
    type Error = FixtureError;

    /// Compare decoded material without introducing a fixture serializer.
    fn values_equal(&mut self, left: &JsonValue, right: &JsonValue) -> Result<bool, Self::Error> {
        Ok(left == right)
    }

    /// Compare one decoded value with its admitted object projection.
    fn value_and_object_equal(
        &mut self,
        value: &JsonValue,
        object: &[(Vec<u16>, JsonValue)],
    ) -> Result<bool, Self::Error> {
        Ok(matches!(value, JsonValue::Object(entries) if entries == object))
    }
}

impl WorkerSourceMaterialIdentity for DispatcherMaterial {
    /// Admit the exact source material used by both discovery and dispatcher configuration.
    fn material_identity(&mut self, value: &JsonValue) -> Result<String, TaskFailure> {
        assert_eq!(
            value,
            &JsonValue::Object(vec![(
                utf16("contractVersion"),
                json_string("forecast-current/v1"),
            )])
        );
        Ok("material-1".to_owned())
    }
}

#[derive(Debug, Default)]
struct DispatcherTransportEvidence {
    attempts: Vec<ProviderAttempt>,
    bindings: Vec<(OpenMeteoWorkerLocation, weather_worker::ProviderRequest)>,
}

struct DispatcherTransport {
    evidence: Rc<RefCell<DispatcherTransportEvidence>>,
    trace: Rc<RefCell<Vec<String>>>,
}

struct DispatcherProviderIo {
    evidence: Rc<RefCell<DispatcherTransportEvidence>>,
    trace: Rc<RefCell<Vec<String>>>,
}

impl ProviderIo<OpenMeteoAttemptPayload> for DispatcherProviderIo {
    /// Return the locked provider clock used to bound the single physical attempt.
    fn now_ms(&mut self) -> i64 {
        1_800_000
    }

    /// Return deterministic jitter even though the successful fixture never retries.
    fn random_unit(&mut self) -> f64 {
        0.0
    }

    /// Retain the bounded attempt before returning the parsed current payload.
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<OpenMeteoAttemptPayload>, ProviderAttemptError> {
        self.trace
            .borrow_mut()
            .push(format!("provider-attempt-{}", attempt.attempt));
        self.evidence.borrow_mut().attempts.push(attempt);
        Ok(provider_response(200, None))
    }

    /// Reject an impossible retry sleep in the one-attempt fixture.
    fn sleep(&mut self, _sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
        panic!("successful one-attempt dispatcher fixture must not sleep")
    }
}

impl WorkerOpenMeteoTransport for DispatcherTransport {
    type Exchange<'a>
        = DispatcherProviderIo
    where
        Self: 'a;

    /// Bind the provider request only after the durable run row exists.
    fn bind_request(
        &mut self,
        location: OpenMeteoWorkerLocation,
        request: &weather_worker::ProviderRequest,
    ) -> Result<Self::Exchange<'_>, TaskFailure> {
        self.trace.borrow_mut().push("provider-bind".to_owned());
        self.evidence
            .borrow_mut()
            .bindings
            .push((location, request.clone()));
        Ok(DispatcherProviderIo {
            evidence: Rc::clone(&self.evidence),
            trace: Rc::clone(&self.trace),
        })
    }
}

struct ForecastExchange<Product> {
    attempts: Arc<Mutex<Vec<ProviderAttempt>>>,
    product: Option<Product>,
    status: u16,
}

impl<Product> ProviderIo<Product> for ForecastExchange<Product> {
    /// Return the locked clock used by both first forecast attempts.
    fn now_ms(&mut self) -> i64 {
        1_800_000
    }

    /// Return deterministic unused retry jitter.
    fn random_unit(&mut self) -> f64 {
        0.0
    }

    /// Retain one bounded attempt and return its typed parsed product.
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<Product>, ProviderAttemptError> {
        self.attempts.lock().expect("attempt lock").push(attempt);
        Ok(ProviderAttemptResponse {
            body: self.product.take().expect("forecast product should exist"),
            content: ContentBudget {
                declared_body_bytes: Some(1_024),
                streamed_body_bytes: 1_024,
            },
            provider_reason: None,
            retry_after_ms: None,
            status: self.status,
        })
    }

    /// Reject an impossible retry sleep in the successful fixture.
    fn sleep(&mut self, _sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
        panic!("successful forecast exchange must not sleep")
    }
}

#[derive(Debug, Default)]
struct ForecastTransportEvidence {
    bindings: Vec<(OpenMeteoWorkerLocation, weather_worker::ProviderRequest)>,
    receipt_clock_calls: usize,
}

struct ForecastTransport {
    air_quality: ForecastExchange<OpenMeteoForecastAirQualityProduct>,
    evidence: Arc<Mutex<ForecastTransportEvidence>>,
    weather: ForecastExchange<OpenMeteoForecastWeatherProduct>,
}

impl WorkerOpenMeteoForecastTransport for ForecastTransport {
    type WeatherExchange = ForecastExchange<OpenMeteoForecastWeatherProduct>;
    type AirQualityExchange = ForecastExchange<OpenMeteoForecastAirQualityProduct>;

    /// Bind both controllers to one exact committed forecast request.
    fn run_pair<Weather, AirQuality>(
        &mut self,
        location: OpenMeteoWorkerLocation,
        request: &weather_worker::ProviderRequest,
        weather: Weather,
        air_quality: AirQuality,
    ) -> Result<OpenMeteoForecastPair, TaskFailure>
    where
        Weather: FnOnce(
                &mut Self::WeatherExchange,
            )
                -> Result<ProviderResponse<OpenMeteoForecastWeatherProduct>, TaskFailure>
            + Send
            + 'static,
        AirQuality: FnOnce(
                &mut Self::AirQualityExchange,
            )
                -> Result<ProviderResponse<OpenMeteoForecastAirQualityProduct>, TaskFailure>
            + Send
            + 'static,
    {
        self.evidence
            .lock()
            .expect("transport evidence lock")
            .bindings
            .push((location, request.clone()));
        let weather = weather(&mut self.weather)?;
        let air_quality = air_quality(&mut self.air_quality)?;
        Ok(OpenMeteoForecastPair {
            weather,
            air_quality,
        })
    }

    /// Capture one receipt instant after both products succeed.
    fn forecast_received_at_ms(&mut self) -> i64 {
        self.evidence
            .lock()
            .expect("transport evidence lock")
            .receipt_clock_calls += 1;
        1_795_000
    }
}

#[derive(Debug, Default)]
struct ForecastEvidenceInputs {
    checksum_inputs: Vec<Vec<u8>>,
    metadata_inputs: Vec<Vec<(Vec<u16>, JsonValue)>>,
}

struct ForecastEvidence {
    inputs: Arc<Mutex<ForecastEvidenceInputs>>,
}

impl OpenMeteoForecastEvidenceValues for ForecastEvidence {
    /// Retain paired checksum framing and return the admitted combined digest.
    fn forecast_sha256(&mut self, bytes: &[u8]) -> Result<Sha256Digest, TaskFailure> {
        self.inputs
            .lock()
            .expect("forecast evidence lock")
            .checksum_inputs
            .push(bytes.to_vec());
        Ok(Sha256Digest::admitted(RESPONSE_CHECKSUM))
    }

    /// Retain ordered paired metadata and return its admitted serialization.
    fn forecast_metadata_json(
        &mut self,
        entries: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure> {
        self.inputs
            .lock()
            .expect("forecast evidence lock")
            .metadata_inputs
            .push(entries.to_vec());
        Ok(SerializedJson::admitted(FORECAST_RESPONSE_METADATA))
    }
}

struct DelegatedDispatcher {
    sources: Rc<RefCell<Vec<(String, OpenMeteoSourceKind)>>>,
}

impl<Repository> WorkerScheduledSourceDispatcher<Repository> for DelegatedDispatcher {
    /// Retain the forecast routed around the current-only transport.
    fn run_source<C: FnMut() -> i64>(
        &mut self,
        _repository: &mut Repository,
        source: &WorkerDueSource,
        _clock: &mut C,
    ) -> Result<SourceRunResult, TaskFailure> {
        let WorkerDueSource::OpenMeteo(source) = source else {
            panic!("unexpected delegated source: {}", source.source_id())
        };
        assert_eq!(source.source_kind, OpenMeteoSourceKind::Forecast);
        self.sources
            .borrow_mut()
            .push((source.id.clone(), source.source_kind));
        Ok(SourceRunResult {
            attempts: 0,
            reason: Some("delegated forecast fixture".to_owned()),
            record_count: 0,
            run_id: None,
            secondary_error_codes: Vec::new(),
            source_id: source.id.clone(),
            status: SourceRunStatus::Skipped,
            successful_at_ms: None,
        })
    }
}

/// Build the original generated site catalog with coordinates distinct from its station.
fn dispatcher_site_configuration() -> SiteConfiguration {
    SiteConfiguration {
        provider: SiteConfigurationProvider {
            active: true,
            attributionLabel: "Open-Meteo".to_owned(),
            attributionUrl: "https://open-meteo.com".to_owned(),
            displayName: "Open-Meteo".to_owned(),
            key: "open-meteo".to_owned(),
        },
        site: SiteConfigurationSite {
            active: true,
            displayName: "Home".to_owned(),
            key: "home".to_owned(),
            latitude: 54.0,
            longitude: -4.0,
            timezone: "Europe/London".to_owned(),
        },
        station: SiteConfigurationStation {
            active: true,
            displayName: "Model".to_owned(),
            key: "model".to_owned(),
            latitude: 53.25,
            longitude: -3.75,
            kind: StationKind::Virtual,
            model: Nullable::Null,
            serial: Nullable::Null,
            vendor: Nullable::Null,
        },
        sources: vec![
            SiteConfigurationSource {
                active: true,
                adapterConfig: JsonValue::Object(vec![(
                    utf16("contractVersion"),
                    json_string("forecast-current/v1"),
                )]),
                cadenceSeconds: Nullable::Value(60),
                capabilities: vec![SourceCapability::Current],
                fingerprint: FINGERPRINT.to_owned(),
                key: "current".to_owned(),
                sourceKind: SourceKind::ModelCurrent,
            },
            SiteConfigurationSource {
                active: true,
                adapterConfig: JsonValue::Object(vec![(
                    utf16("contractVersion"),
                    json_string("forecast-current/v1"),
                )]),
                cadenceSeconds: Nullable::Value(60),
                capabilities: vec![SourceCapability::Forecast],
                fingerprint: FINGERPRINT.to_owned(),
                key: "forecast".to_owned(),
                sourceKind: SourceKind::Forecast,
            },
        ],
        version: 1,
    }
}

/// encode one generated UTF-16 key or string
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// encode one generated JSON string
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(utf16(value))
}

/// build one generated checkpoint state around admitted cursor JSON
fn checkpoint_state(
    provider_cursor: Nullable<Vec<(Vec<u16>, JsonValue)>>,
) -> ScheduledCheckpointState {
    ScheduledCheckpointState {
        lastCommittedAt: WINDOW_START.to_owned(),
        lastValidAt: WINDOW_START.to_owned(),
        providerCursor: provider_cursor,
        sourceId: SOURCE_ID.to_owned(),
        version: 7.0,
        windowEndExclusive: WINDOW_START.to_owned(),
        windowStart: "1970-01-01T00:27:00.000Z".to_owned(),
    }
}

/// build one matching source and runtime configuration
fn source_configuration() -> (OpenMeteoScheduledSource, OpenMeteoRuntimeConfiguration) {
    let source = OpenMeteoScheduledSource {
        active: true,
        adapter_version: OPEN_METEO_CURRENT_ADAPTER_VERSION,
        cadence_seconds: 60,
        id: SOURCE_ID.to_owned(),
        material_config_identity: "material-1".to_owned(),
        provider_key: "open-meteo".to_owned(),
        site_slug: "home".to_owned(),
        source_config_fingerprint: FINGERPRINT.to_owned(),
        source_key: "current".to_owned(),
        source_kind: OpenMeteoSourceKind::ModelCurrent,
        station_slug: "model".to_owned(),
        timezone: "Europe/London".to_owned(),
    };
    let configuration = OpenMeteoRuntimeConfiguration {
        active: true,
        adapter_version: OPEN_METEO_CURRENT_ADAPTER_VERSION,
        capabilities: BTreeSet::from([OpenMeteoCapability::Current]),
        material_config_identity: source.material_config_identity.clone(),
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

/// Build the generated forecast source selected by the paired dispatcher.
fn forecast_source_configuration() -> OpenMeteoScheduledSource {
    let (mut source, _) = source_configuration();
    source.adapter_version = OPEN_METEO_FORECAST_ADAPTER_VERSION;
    source.id = FORECAST_SOURCE_ID.to_owned();
    source.source_key = "forecast".to_owned();
    source.source_kind = OpenMeteoSourceKind::Forecast;
    source
}

/// Build the typed weather half of one paired forecast response.
fn forecast_weather_product() -> OpenMeteoForecastWeatherProduct {
    OpenMeteoForecastWeatherProduct {
        checksum: Sha256Digest::admitted(WEATHER_CHECKSUM),
        generation_ms: Some(12.5),
        envelope: OpenMeteoEnvelope {
            elevation_m: Some(17.0),
            grid_cell: "54,-4".to_owned(),
            latitude: 54.0,
            longitude: -4.0,
            upstream_timezone: "Europe/London".to_owned(),
        },
        samples: vec![OpenMeteoWeatherSample {
            apparent_temperature_c: Some(15.8),
            cloud_cover_percent: Some(65.0),
            precipitation_mm: Some(0.2),
            pressure_hpa: Some(1_012.7),
            relative_humidity_percent: Some(81.0),
            temperature_c: Some(16.4),
            uv_index: Some(4.0),
            valid_at_ms: 1_790_000,
            wind_direction_degrees: Some(270.0),
            wind_gust_mps: Some(7.1),
            wind_speed_mps: Some(4.3),
        }],
    }
}

/// Build the typed air-quality half matched to the weather validity instant.
fn forecast_air_quality_product() -> OpenMeteoForecastAirQualityProduct {
    OpenMeteoForecastAirQualityProduct {
        checksum: Sha256Digest::admitted(AIR_QUALITY_CHECKSUM),
        generation_ms: Some(7.25),
        samples: vec![OpenMeteoAirQualitySample {
            pm25_micrograms_per_cubic_meter: Some(5.5),
            valid_at_ms: 1_790_000,
        }],
    }
}

/// execute one source against the real worker lifecycle and runtime adapter
fn execute(
    outcome: ProviderOutcome,
) -> (weather_worker::SourceRunResult, ProviderExecutionEvidence) {
    let trace = Rc::new(RefCell::new(Vec::new()));
    // retain the attempts and durable code expected from the selected controller branch
    let (expected_attempts, expected_failure_classification, expected_failure_code) = match outcome
    {
        ProviderOutcome::ContentBudgetFailure => (
            1,
            IngestionErrorClassification::InvalidPayload,
            "provider_response_too_large",
        ),
        ProviderOutcome::DeadlineBoundedSuccess => {
            (1, IngestionErrorClassification::Retryable, "unused_success")
        }
        ProviderOutcome::Failure => (
            3,
            IngestionErrorClassification::Retryable,
            "provider_server_error",
        ),
        ProviderOutcome::Success => (2, IngestionErrorClassification::Retryable, "unused_success"),
    };
    let mut database = FixtureDatabase {
        checkpoint: true,
        discover_at_ms: Vec::new(),
        due_sources: Vec::new(),
        expected_attempts,
        expected_failure_classification,
        expected_failure_code: expected_failure_code.to_owned(),
        heartbeats: Vec::new(),
        retained_source_id: SOURCE_ID.to_owned(),
        trace: Rc::clone(&trace),
    };
    let mut values = FixtureValues;
    let mut repository = OpenMeteoDatabaseRepository::new(&mut database, &mut values);
    let mut provider = provider_fixture(outcome, Rc::clone(&trace));
    let (source, configuration) = source_configuration();
    let result = run_scheduled_source(
        ScheduledRunExecution {
            configuration: &configuration,
            maximum_range_seconds: None,
            now_ms: 1_800_000,
            run_deadline_at_ms: 1_860_000,
            source: &source,
        },
        &mut repository,
        &mut provider,
        &mut || 1_801_000,
    )
    .expect("scheduled source result");
    let evidence = ProviderExecutionEvidence {
        attempts: provider.io().attempts.clone(),
        sleeps: provider.io().sleeps.clone(),
        trace: trace.borrow().clone(),
    };
    (result, evidence)
}

#[derive(Default)]
struct IterationPg {
    discover_at_ms: Vec<i64>,
    due_sources: Vec<WorkerDueSource>,
    heartbeats: Vec<WorkerHeartbeat>,
}

impl WorkerIterationDatabasePort for IterationPg {
    type Error = FixtureError;

    /// return ready sources in injected database order
    fn discover_due_sources(&mut self, as_of_ms: i64) -> Result<Vec<WorkerDueSource>, Self::Error> {
        self.discover_at_ms.push(as_of_ms);
        Ok(std::mem::take(&mut self.due_sources))
    }

    /// retain one typed fake PostgreSQL heartbeat command
    fn update_worker_heartbeat(&mut self, heartbeat: WorkerHeartbeat) -> Result<(), Self::Error> {
        self.heartbeats.push(heartbeat);
        Ok(())
    }
}

#[derive(Default)]
struct IterationDispatcher {
    actual_attempts: Vec<ProviderAttempt>,
    actual_trace: Vec<String>,
    visited: Vec<String>,
}

impl<Repository> WorkerScheduledSourceDispatcher<Repository> for IterationDispatcher {
    /// dispatch one actual provider plus bounded continuation fixtures
    fn run_source<C: FnMut() -> i64>(
        &mut self,
        _repository: &mut Repository,
        source: &WorkerDueSource,
        success_clock: &mut C,
    ) -> Result<SourceRunResult, TaskFailure> {
        self.visited.push(source.source_id().to_owned());

        // run the first source through the production provider and database adapters
        if source.source_id() == SOURCE_ID {
            let trace = Rc::new(RefCell::new(Vec::new()));
            let mut database = FixtureDatabase {
                checkpoint: true,
                discover_at_ms: Vec::new(),
                due_sources: Vec::new(),
                expected_attempts: 2,
                expected_failure_classification: IngestionErrorClassification::Retryable,
                expected_failure_code: "unused_success".to_owned(),
                heartbeats: Vec::new(),
                retained_source_id: SOURCE_ID.to_owned(),
                trace: Rc::clone(&trace),
            };
            let mut values = FixtureValues;
            let mut repository = OpenMeteoDatabaseRepository::new(&mut database, &mut values);
            let mut provider = provider_fixture(ProviderOutcome::Success, Rc::clone(&trace));
            let (configured_source, configuration) = source_configuration();
            let result = run_scheduled_source(
                ScheduledRunExecution {
                    configuration: &configuration,
                    maximum_range_seconds: None,
                    now_ms: 1_800_000,
                    run_deadline_at_ms: 1_860_000,
                    source: &configured_source,
                },
                &mut repository,
                &mut provider,
                success_clock,
            )?;
            self.actual_attempts = provider.io().attempts.clone();
            self.actual_trace = trace.borrow().clone();
            return Ok(result);
        }

        // isolate one unexpected dispatch failure without stopping later sources
        if source.source_id() == "source-dispatch-error" {
            return Err(TaskFailure {
                attempts: 0,
                code: "dispatcher_unavailable".to_owned(),
            });
        }

        // represent only the explicitly completed following source
        let successful_at_ms = success_clock();
        Ok(SourceRunResult {
            attempts: 1,
            reason: None,
            record_count: 1,
            run_id: Some("run-after".to_owned()),
            secondary_error_codes: Vec::new(),
            source_id: source.source_id().to_owned(),
            status: SourceRunStatus::Succeeded,
            successful_at_ms: Some(successful_at_ms),
        })
    }
}

struct IterationRainClock;

impl RainCollectionClock for IterationRainClock {
    /// return the compiled Node planner fixture instant
    fn now(&mut self) -> RainCollectionTimeValue {
        RainCollectionTimeValue::EpochMilliseconds(1_789_387_800_000)
    }
}

struct IterationRainTimeAdapter;

impl RainCollectionTimeAdapter for IterationRainTimeAdapter {
    /// admit only exact timestamps selected by the compiled Node fixture
    fn canonical_utc_iso(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        let value = match epoch_ms {
            1_789_365_600_000 => "2026-09-14T06:00:00.000Z",
            1_789_380_001_000 => "2026-09-14T10:00:01.000Z",
            1_789_387_201_000 => "2026-09-14T12:00:01.000Z",
            _ => {
                return Err(TaskFailure {
                    attempts: 0,
                    code: "unexpected rain fixture timestamp".to_owned(),
                });
            }
        };
        Ok(value.to_owned())
    }
}

struct IterationRainRepository {
    claim_count: usize,
    trace: Rc<RefCell<Vec<String>>>,
}

impl RainCollectionRepository for IterationRainRepository {
    /// claim each selected request before any provider call
    fn claim_rain_capture_slot(
        &mut self,
        request: &RainCaptureRequest,
        release: &str,
    ) -> Result<Option<String>, TaskFailure> {
        self.claim_count += 1;
        self.trace
            .borrow_mut()
            .push(format!("rain-claim:{}:{release}", request.slot_key()));
        Ok(Some(format!("claim-{}", self.claim_count)))
    }

    /// retain each complete raw receipt after its provider call
    fn append_rain_capture_receipt(
        &mut self,
        claim_id: &str,
        receipt: RainCaptureReceipt,
    ) -> Result<(), TaskFailure> {
        self.trace
            .borrow_mut()
            .push(format!("rain-receipt:{claim_id}:{}", receipt.row_count));
        Ok(())
    }
}

struct IterationRainProvider {
    trace: Rc<RefCell<Vec<String>>>,
}

impl RainCollectionProvider for IterationRainProvider {
    /// return one actual typed raw capture receipt without provider traffic
    fn fetch_rain_capture(
        &mut self,
        request: &RainCaptureRequest,
    ) -> Result<RainCaptureReceipt, TaskFailure> {
        self.trace
            .borrow_mut()
            .push(format!("rain-fetch:{}", request.slot_key()));
        Ok(RainCaptureReceipt {
            available_by_decision: Some(true),
            body: Some(b"{}".to_vec()),
            body_sha256: Some("fixture".to_owned()),
            completed_at: "2026-09-14T12:10:00.000Z".to_owned(),
            error_code: None,
            http_status: Some(200),
            metadata: BTreeMap::new(),
            outcome: RainCaptureOutcome::Valid,
            parser_version: RAIN_COLLECTION_POLICY.contract_version,
            row_count: 48,
            started_at: "2026-09-14T12:10:00.000Z".to_owned(),
        })
    }
}

struct IterationRainPacer {
    trace: Rc<RefCell<Vec<String>>>,
}

impl RainCollectionPacer for IterationRainPacer {
    /// retain exact pacing without waiting in the runtime fixture
    fn sleep(&mut self, milliseconds: u64) -> Result<(), TaskFailure> {
        self.trace
            .borrow_mut()
            .push(format!("rain-sleep:{milliseconds}"));
        Ok(())
    }
}

struct IterationFixedGaugeRepository {
    trace: Rc<RefCell<Vec<String>>>,
}

impl RainFixedGaugeRepository for IterationFixedGaugeRepository {
    /// return the exact admitted catalog before pending-hour selection
    fn read_source_catalog(&mut self) -> Result<RainFixedGaugeSourceCatalog, TaskFailure> {
        self.trace.borrow_mut().push("fixed-catalog".to_owned());
        Ok(RainFixedGaugeSourceCatalog {
            catalog_sha256: RAIN_FIXED_GAUGE_TARGET_CATALOG_SHA256.to_owned(),
            contract_version: RAIN_FIXED_GAUGE_TARGET_CATALOG_CONTRACT_VERSION.to_owned(),
            run_contract_sha256: RAIN_FIXED_GAUGE_TARGET_RUN_CONTRACT_SHA256.to_owned(),
            sources: RAIN_COLLECTION_STATIONS
                .iter()
                .enumerate()
                .map(|(index, station)| RainFixedGaugeSource {
                    location_id: station.location_id,
                    source_config_fingerprint: format!("{:064x}", 100 + index),
                    source_id: (index + 1).to_string(),
                    source_key: format!("rain-target-tempest-{}", station.location_id),
                })
                .collect(),
        })
    }

    /// return normal idle without inflating raw target bodies
    fn read_pending_hour(&mut self, capture_epoch_at: &str) -> Result<Option<String>, TaskFailure> {
        self.trace
            .borrow_mut()
            .push(format!("fixed-pending:{capture_epoch_at}"));
        Ok(None)
    }

    /// reject raw-body work after an idle pending-hour result
    fn read_captures_for_hour(
        &mut self,
        _capture_epoch_at: &str,
        _valid_at: &str,
    ) -> Result<Vec<RainFixedGaugeCapture>, TaskFailure> {
        panic!("idle fixed-gauge fixture inflated raw captures")
    }

    /// reject database mutation after an idle pending-hour result
    fn commit_revision_group(
        &mut self,
        _group: &RainFixedGaugeRevisionGroup,
    ) -> Result<RainFixedGaugeRevisionResult, TaskFailure> {
        panic!("idle fixed-gauge fixture committed a revision group")
    }

    /// reject gap mutation after an idle pending-hour result
    fn mark_revision_group_gap(
        &mut self,
        _group: &RainFixedGaugeRevisionGroup,
        _reason: RainFixedGaugeGapReason,
    ) -> Result<RainFixedGaugeRevisionGap, TaskFailure> {
        panic!("idle fixed-gauge fixture marked a revision gap")
    }
}

struct IterationFixedGaugeBuilder;

impl RainFixedGaugeProjectionBuilder for IterationFixedGaugeBuilder {
    /// reject replay and encoding after an idle pending-hour result
    fn build_projection(
        &mut self,
        _captures: &[RainFixedGaugeCapture],
        _sources: &[RainFixedGaugeSource],
        _target_cutoff_at: &str,
        _valid_at: &str,
    ) -> Result<RainFixedGaugeBuildResult, TaskFailure> {
        panic!("idle fixed-gauge fixture built a projection")
    }
}

struct IterationFixedGaugeIdentity;

impl RainFixedGaugeIdentityAdapter for IterationFixedGaugeIdentity {
    /// reject logical identity work after an idle pending-hour result
    fn logical_key_sha256(
        &mut self,
        _source_ids: &[String],
        _valid_at: &str,
    ) -> Result<String, TaskFailure> {
        panic!("idle fixed-gauge fixture derived a logical identity")
    }

    /// reject body verification after an idle pending-hour result
    fn capture_body_matches(&mut self, _body: &[u8], _expected_sha256: &str) -> bool {
        panic!("idle fixed-gauge fixture verified a raw body")
    }

    /// reject projection identity work after an idle pending-hour result
    fn projection_identity_sha256(&mut self, _bytes: &[u8]) -> Result<String, TaskFailure> {
        panic!("idle fixed-gauge fixture derived a projection identity")
    }

    /// reject record identity work after an idle pending-hour result
    fn weather_record_sha256(
        &mut self,
        _station_id: u64,
        _record: &RainFixedGaugeNormalizedRecord,
    ) -> Result<String, TaskFailure> {
        panic!("idle fixed-gauge fixture derived a record identity")
    }

    /// reject durable gap verification after an idle pending-hour result
    fn durable_gap_receipt_is_authentic(&mut self, _gap: &RainFixedGaugeDurableGap) -> bool {
        panic!("idle fixed-gauge fixture verified a durable gap")
    }

    /// reject stage verification after an idle pending-hour result
    fn stage_receipt_is_authentic(&mut self, _receipt: &RainFixedGaugeStageReceipt) -> bool {
        panic!("idle fixed-gauge fixture verified a stage receipt")
    }
}

struct IterationFixedGaugeClient;

impl RainFixedGaugeClient for IterationFixedGaugeClient {
    /// reject gap reads after an idle pending-hour result
    fn read_oversized_gap(
        &mut self,
        _logical_hour_at: &str,
        _logical_key_sha256: &str,
    ) -> Result<RainFixedGaugeGapStatus, TaskFailure> {
        panic!("idle fixed-gauge fixture read an oversized gap")
    }

    /// reject oversized gap mutation after an idle pending-hour result
    fn record_oversized_gap(
        &mut self,
        _logical_hour_at: &str,
        _logical_key_sha256: &str,
    ) -> Result<RainFixedGaugeDurableGap, TaskFailure> {
        panic!("idle fixed-gauge fixture recorded an oversized gap")
    }

    /// reject stage mutation after an idle pending-hour result
    fn stage_revision(&mut self, _bytes: &[u8]) -> Result<RainFixedGaugeStageReceipt, TaskFailure> {
        panic!("idle fixed-gauge fixture staged a body")
    }

    /// reject database gap archival after an idle pending-hour result
    fn record_revision_gap(&mut self, _gap: &RainFixedGaugeRevisionGap) -> Result<(), TaskFailure> {
        panic!("idle fixed-gauge fixture archived a revision gap")
    }

    /// reject publication after an idle pending-hour result
    fn publish_revision_batch(
        &mut self,
        _bytes: &[u8],
        _stage_receipt: &RainFixedGaugeStageReceipt,
        _commit_receipts: &[RainFixedGaugeCommitReceipt],
    ) -> Result<(), TaskFailure> {
        panic!("idle fixed-gauge fixture published a revision batch")
    }
}

struct IterationRainAdjustmentRepository {
    trace: Rc<RefCell<Vec<String>>>,
}

struct IterationRainInferenceBinding {
    trace: Rc<RefCell<Vec<String>>>,
}

impl<Database, Values> WorkerRainInferenceRepositoryBinding<Database, Values>
    for IterationRainInferenceBinding
{
    type Repository<'a>
        = IterationRainAdjustmentRepository
    where
        Self: 'a,
        Database: 'a,
        Values: 'a;

    /// Bind the scheduled task to the same idle repository trace used by the iteration fixture.
    fn bind<'a>(
        &'a mut self,
        _database: &'a mut Database,
        _values: &'a mut Values,
    ) -> Self::Repository<'a> {
        IterationRainAdjustmentRepository {
            trace: Rc::clone(&self.trace),
        }
    }
}

struct IterationRainAfterServing;

impl<Database, Values> WorkerRainAfterServing<Database, Values> for IterationRainAfterServing {
    /// Reject shadow execution because the serving repository has no mature source.
    fn run_shadow(
        &mut self,
        _pending: &RainAdjustmentPendingInput,
        _evaluation: &RainAdjustmentEvaluation,
        _database: &mut Database,
        _values: &mut Values,
    ) {
        panic!("idle inference fixture attempted shadow publication")
    }
}

impl RainAdjustmentRepository for IterationRainAdjustmentRepository {
    type Transaction = ();

    /// execute the real publisher's pending-input boundary with no mature run
    fn read_pending(
        &mut self,
        model_sha256: &str,
    ) -> Result<RainAdjustmentPendingInput, TaskFailure> {
        self.trace
            .borrow_mut()
            .push(format!("inference-read:{model_sha256}"));
        Ok(RainAdjustmentPendingInput {
            captures: Vec::new(),
            station_hours: Vec::new(),
        })
    }

    /// reject serving mutation for the no-ready fixture
    fn begin_append(
        &mut self,
        _run: &RainAdjustmentRun,
    ) -> Result<Option<RainAdjustmentAppendTransaction<Self::Transaction>>, TaskFailure> {
        panic!("no-ready inference attempted a serving insert")
    }

    /// reject revision binding for the no-ready fixture
    fn bind_revision(
        &mut self,
        _transaction: &mut Self::Transaction,
        _persisted: &RainAdjustmentPersistedRevision,
        _plan: &RainAdjustmentRevisionPlan,
        _stage_receipt: &RainAdjustmentStageReceipt,
    ) -> Result<RainAdjustmentCommitReceipt, TaskFailure> {
        panic!("no-ready inference attempted revision binding")
    }

    /// reject transactional gaps for the no-ready fixture
    fn mark_revision_gap_in_transaction(
        &mut self,
        _transaction: &mut Self::Transaction,
        _persisted: &RainAdjustmentPersistedRevision,
        _gap: &RainAdjustmentGap,
    ) -> Result<(), TaskFailure> {
        panic!("no-ready inference attempted a transactional gap")
    }

    /// reject serving commit for the no-ready fixture
    fn commit_append(&mut self, _transaction: &mut Self::Transaction) -> Result<(), TaskFailure> {
        panic!("no-ready inference attempted a serving commit")
    }

    /// reject serving rollback for the no-ready fixture
    fn rollback_append(&mut self, _transaction: &mut Self::Transaction) {
        panic!("no-ready inference attempted a serving rollback")
    }

    /// reject post-commit gaps for the no-ready fixture
    fn mark_revision_gap_after_commit(
        &mut self,
        _persisted: &RainAdjustmentPersistedRevision,
        _gap: &RainAdjustmentGap,
    ) -> Result<(), TaskFailure> {
        panic!("no-ready inference attempted a post-commit gap")
    }
}

struct IterationRainAdjustmentAdmission;

impl RainAdjustmentFeatureAdmission for IterationRainAdjustmentAdmission {
    /// reject calendar work when the repository returned no mature source
    fn calendar(
        &mut self,
        _valid_at_ms: i64,
    ) -> Result<AdmittedRainHurdleWindCalendar, TaskFailure> {
        panic!("no-ready inference attempted calendar admission")
    }

    /// reject station geometry when the repository returned no mature source
    fn station_weights(&mut self) -> Result<[f64; 12], TaskFailure> {
        panic!("no-ready inference attempted station admission")
    }
}

struct IterationRainAdjustmentTime;

impl RainAdjustmentTimeAdapter for IterationRainAdjustmentTime {
    /// reject date formatting when the repository returned no mature source
    fn utc_string(&mut self, _epoch_ms: i64) -> Result<String, TaskFailure> {
        panic!("no-ready inference attempted date formatting")
    }
}

struct IterationRainAdjustmentIdentity;

impl RainAdjustmentIdentityAdapter for IterationRainAdjustmentIdentity {
    /// reject input hashing when the repository returned no mature source
    fn input_sha256(
        &mut self,
        _captures: &[weather_worker::RainAdjustmentCaptureIdentity],
    ) -> Result<String, TaskFailure> {
        panic!("no-ready inference attempted input hashing")
    }

    /// reject stage verification when revision work is absent
    fn stage_receipt_is_authentic(&mut self, _receipt: &RainAdjustmentStageReceipt) -> bool {
        panic!("no-ready inference verified a stage receipt")
    }

    /// reject commit verification when revision work is absent
    fn commit_receipt_is_authentic(&mut self, _receipt: &RainAdjustmentCommitReceipt) -> bool {
        panic!("no-ready inference verified a commit receipt")
    }
}

struct IterationRainAdjustmentCodec;

struct IterationRainInferenceModel<'a>(RainAdjustmentModel<'a>);

impl WorkerRainInferenceModel for IterationRainInferenceModel<'_> {
    /// Bind the test's retained model to the active branch without reading a startup file.
    fn serving_model(&self) -> Option<(RainAdjustmentRuntimeSelection, RainAdjustmentModel<'_>)> {
        Some((RainAdjustmentRuntimeSelection::Active, self.0))
    }
}

impl RainAdjustmentProjectionCodec for IterationRainAdjustmentCodec {
    /// reject logical-key derivation when revision work is absent
    fn logical_key_sha256(
        &mut self,
        _persisted: &RainAdjustmentPersistedRevision,
    ) -> Result<String, TaskFailure> {
        panic!("no-ready inference derived a revision logical key")
    }

    /// reject projection encoding when revision work is absent
    fn build_revision(
        &mut self,
        _pending: &RainAdjustmentPendingInput,
        _evaluation: &RainAdjustmentEvaluation,
        _persisted: &RainAdjustmentPersistedRevision,
        _logical_key_sha256: &str,
        _time: &mut impl RainAdjustmentTimeAdapter,
    ) -> Result<RainAdjustmentRevisionPlan, TaskFailure> {
        panic!("no-ready inference encoded a revision")
    }
}

struct IterationRainAdjustmentClient;

impl RainAdjustmentClient for IterationRainAdjustmentClient {
    /// reject staging when revision work is absent
    fn stage_revision(
        &mut self,
        _projection: &RainAdjustmentProjection,
    ) -> Result<RainAdjustmentStageReceipt, TaskFailure> {
        panic!("no-ready inference staged a revision")
    }

    /// reject publication when revision work is absent
    fn publish_revision(
        &mut self,
        _projection: &RainAdjustmentProjection,
        _stage_receipt: &RainAdjustmentStageReceipt,
        _commit_receipt: &RainAdjustmentCommitReceipt,
    ) -> Result<(), TaskFailure> {
        panic!("no-ready inference published a revision")
    }

    /// reject gap archival when revision work is absent
    fn record_revision_gap(&mut self, _gap: &RainAdjustmentGap) -> Result<(), TaskFailure> {
        panic!("no-ready inference archived a revision gap")
    }
}

// build one unused typed model head for the no-ready publisher boundary
fn idle_rain_model_head(
    objective: RainObjective,
) -> RainHurdleWindPortableArtifactV1HeadsNativeHead {
    RainHurdleWindPortableArtifactV1HeadsNativeHead {
        objective,
        baseScore: 0.5,
        trees: Vec::new(),
    }
}

const ITERATION_TEMPERATURE_NOW_MS: i64 = 1_788_827_700_000;
const ITERATION_TEMPERATURE_LATEST_MS: i64 = 1_788_804_000_000;

struct IterationTemperatureTime;

impl TemperatureCanaryTimeAdapter for IterationTemperatureTime {
    /// format the exact canary warmup cycles without implementing a date codec
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        let hours = (ITERATION_TEMPERATURE_LATEST_MS - epoch_ms) / 3_600_000;
        Ok(match hours {
            0 => "2026-09-07T18:00:00.000Z",
            6 => "2026-09-07T12:00:00.000Z",
            7 => "2026-09-07T11:00:00.000Z",
            12 => "2026-09-07T06:00:00.000Z",
            13 => "2026-09-07T05:00:00.000Z",
            18 => "2026-09-07T00:00:00.000Z",
            24 => "2026-09-06T18:00:00.000Z",
            30 => "2026-09-06T12:00:00.000Z",
            36 => "2026-09-06T06:00:00.000Z",
            42 => "2026-09-06T00:00:00.000Z",
            48 => "2026-09-05T18:00:00.000Z",
            54 => "2026-09-05T12:00:00.000Z",
            60 => "2026-09-05T06:00:00.000Z",
            66 => "2026-09-05T00:00:00.000Z",
            72 => "2026-09-04T18:00:00.000Z",
            _ => return Ok(format!("temperature-valid-{epoch_ms}")),
        }
        .to_owned())
    }
}

struct IterationTemperatureProvider {
    trace: Rc<RefCell<Vec<String>>>,
}

impl TemperatureCanaryProvider for IterationTemperatureProvider {
    /// return one complete normalized single-run response
    fn fetch_single_run(
        &mut self,
        request: &TemperatureCanaryRequest,
        controls: TemperatureCanaryFetchControls,
    ) -> Result<TemperatureCanaryBatch, TaskFailure> {
        self.trace.borrow_mut().push(format!(
            "temperature-fetch:{}:{}:{}:{}",
            request.run_initialized_at,
            controls.max_attempts,
            controls.timeout_ms,
            controls.max_body_bytes
        ));
        let initialized_at_ms = if request.run_initialized_at == "2026-09-07T18:00:00.000Z" {
            ITERATION_TEMPERATURE_LATEST_MS
        } else {
            ITERATION_TEMPERATURE_LATEST_MS - 6 * 3_600_000
        };
        let mut time = IterationTemperatureTime;
        Ok(TemperatureCanaryBatch {
            adapter_version: "open-meteo-ecmwf-single-run/v1".to_owned(),
            attempts: 1,
            hours: (1..=18)
                .map(|lead| {
                    let valid_at_ms = initialized_at_ms + i64::from(lead) * 3_600_000;
                    TemperatureCanaryHour {
                        model_lead_hours: lead,
                        raw_relative_humidity_percent: Some(75.0),
                        raw_temperature_c: 10.0,
                        raw_wind_speed_mps: Some(3.0),
                        valid_at: time
                            .utc_string(valid_at_ms)
                            .expect("fixture hour should format"),
                        valid_at_ms,
                    }
                })
                .collect(),
            model_cycle: "50r1".to_owned(),
            provider_response_sha256: "a".repeat(64),
            received_at: "2026-09-08T00:35:00.000Z".to_owned(),
            run_initialized_at: request.run_initialized_at.clone(),
            upstream_model: "ecmwf_ifs".to_owned(),
        })
    }
}

struct IterationTemperatureRepository {
    trace: Rc<RefCell<Vec<String>>>,
}

impl TemperatureCanaryRepository for IterationTemperatureRepository {
    /// select a cold inventory so two current runs are requested
    fn list_run_initializations(
        &mut self,
        site_slug: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<String>, TaskFailure> {
        self.trace
            .borrow_mut()
            .push(format!("temperature-list:{site_slug}:{from}:{to}"));
        Ok(Vec::new())
    }

    /// retain one actual normalized run persistence effect
    fn persist_run(&mut self, run: &TemperatureCanaryPersistedRun) -> Result<(), TaskFailure> {
        self.trace.borrow_mut().push(format!(
            "temperature-persist:{}:{}",
            run.batch.run_initialized_at,
            run.state_status.code()
        ));
        Ok(())
    }
}

struct IterationTemperatureRecentError {
    trace: Rc<RefCell<Vec<String>>>,
}

impl TemperatureCanaryRecentErrorBuilder for IterationTemperatureRecentError {
    /// return truthful cold causal state for the newest run
    fn build_recent_error_state(
        &mut self,
        site_slug: &str,
        target_run_initialized_at: &str,
        target_run_initialized_at_ms: i64,
    ) -> Result<TemperatureCanaryRecentErrorState, TaskFailure> {
        assert_eq!(
            target_run_initialized_at_ms,
            ITERATION_TEMPERATURE_LATEST_MS
        );
        self.trace.borrow_mut().push(format!(
            "temperature-state:{site_slug}:{target_run_initialized_at}"
        ));
        Ok(TemperatureCanaryRecentErrorState {
            b24_c: None,
            b72_c: None,
            local_dates: 0,
            mad72_c: None,
            maximum_source_run_initialized_at: None,
            maximum_source_valid_at: None,
            n24: 0,
            n72: 0,
            source_keys: Vec::new(),
            supported: false,
            target_run_initialized_at: target_run_initialized_at.to_owned(),
            window_end_valid_at: "2026-09-07T11:00:00.000Z".to_owned(),
        })
    }
}

// return the fixed admitted source identity used by the coordinator fixture
fn iteration_temperature_source() -> TemperatureCanarySource {
    TemperatureCanarySource {
        active: true,
        adapter_version: "open-meteo-ecmwf-single-run/v1".to_owned(),
        capability: "forecast".to_owned(),
        dataset: "single_run".to_owned(),
        latitude: 56.0,
        longitude: -3.0,
        provider_key: "open-meteo".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_kind: "forecast".to_owned(),
        upstream_model: "ecmwf_ifs".to_owned(),
    }
}

#[derive(Default)]
struct IterationSidecars {
    events: Vec<&'static str>,
    fixed_gauge_trace: Rc<RefCell<Vec<String>>>,
    inference_trace: Rc<RefCell<Vec<String>>>,
    inference_now_ms: Vec<i64>,
    maintenance_due: Vec<MaintenanceCaptureDue>,
    rain_trace: Rc<RefCell<Vec<String>>>,
    temperature_trace: Rc<RefCell<Vec<String>>>,
}

impl<Repository> WorkerSidecarTasks<Repository> for IterationSidecars {
    /// execute the actual bounded temperature-canary collector
    fn collect_temperature_canary(
        &mut self,
        _repository: &mut Repository,
    ) -> Result<TemperatureCanaryCollectionResult, TaskFailure> {
        self.events.push("temperature");
        let mut repository = IterationTemperatureRepository {
            trace: Rc::clone(&self.temperature_trace),
        };
        let mut provider = IterationTemperatureProvider {
            trace: Rc::clone(&self.temperature_trace),
        };
        let mut recent_error = IterationTemperatureRecentError {
            trace: Rc::clone(&self.temperature_trace),
        };
        let mut time = IterationTemperatureTime;
        let result = collect_ecmwf_temperature_canary_runs(
            ITERATION_TEMPERATURE_NOW_MS,
            &mut TemperatureCanaryExecution {
                fetch_limits: TemperatureCanaryFetchLimits::default(),
                provider: &mut provider,
                recent_error: &mut recent_error,
                repository: &mut repository,
                source: &iteration_temperature_source(),
                time: &mut time,
            },
        )?;
        Ok((&result).into())
    }

    /// return one partially failed prospective rain collection
    fn collect_rain_capture(
        &mut self,
        _repository: &mut Repository,
    ) -> Result<RainCaptureResult, TaskFailure> {
        self.events.push("rain");
        let mut clock = IterationRainClock;
        let mut time_adapter = IterationRainTimeAdapter;
        let mut repository = IterationRainRepository {
            claim_count: 0,
            trace: Rc::clone(&self.rain_trace),
        };
        let mut provider = IterationRainProvider {
            trace: Rc::clone(&self.rain_trace),
        };
        let mut pacer = IterationRainPacer {
            trace: Rc::clone(&self.rain_trace),
        };
        let result = collect_rain_evidence(&mut RainCollectionExecution {
            catalog: RAIN_COLLECTION_STATIONS,
            clock: &mut clock,
            pacer: &mut pacer,
            policy: &RAIN_COLLECTION_POLICY,
            provider: &mut provider,
            release: "2026.09.14-1",
            repository: &mut repository,
            stations_authorized: true,
            time_adapter: &mut time_adapter,
        })?;
        Ok(RainCaptureResult {
            failed: result.failed,
            valid: result.valid,
        })
    }

    /// execute the actual fixed-gauge producer through its normal idle boundary
    fn publish_fixed_gauge_target(
        &mut self,
        _repository: &mut Repository,
    ) -> Result<FixedGaugeTargetResult, TaskFailure> {
        self.events.push("fixed-gauge");
        let mut repository = IterationFixedGaugeRepository {
            trace: Rc::clone(&self.fixed_gauge_trace),
        };
        let mut builder = IterationFixedGaugeBuilder;
        let mut identity = IterationFixedGaugeIdentity;
        let mut client = IterationFixedGaugeClient;
        publish_rain_fixed_gauge_target(&mut RainFixedGaugeTargetExecution {
            builder: &mut builder,
            capture_epoch_at: "2000-01-01T00:00:00.000Z",
            client: &mut client,
            identity: &mut identity,
            repository: &mut repository,
        })
    }

    /// execute the actual publisher through its normal no-ready source boundary
    fn publish_rain_inference(
        &mut self,
        _repository: &mut Repository,
        now_ms: i64,
    ) -> Result<bool, TaskFailure> {
        self.events.push("inference");
        self.inference_now_ms.push(now_ms);
        let binding = IterationRainInferenceBinding {
            trace: Rc::clone(&self.inference_trace),
        };
        let heads = RainHurdleWindPortableArtifactV1Heads {
            atLeast0_1: idle_rain_model_head(RainObjective::BinaryLogistic),
            atLeast1_0: idle_rain_model_head(RainObjective::BinaryLogistic),
            atLeast2_5: idle_rain_model_head(RainObjective::BinaryLogistic),
            amount: idle_rain_model_head(RainObjective::RegGamma),
        };
        let rules: Vec<RainHurdleWindPortableArtifactV1Rules> = Vec::new();
        let scales: Vec<f64> = Vec::new();
        let serving = WorkerRainInferenceServing {
            admission: IterationRainAdjustmentAdmission,
            identity: IterationRainAdjustmentIdentity,
            model: IterationRainInferenceModel(RainAdjustmentModel::from_typed(
                &heads,
                &rules,
                &scales,
                "2026-08",
                FINGERPRINT,
            )),
            revision: Some((IterationRainAdjustmentCodec, IterationRainAdjustmentClient)),
            time: IterationRainAdjustmentTime,
        };
        let mut task =
            WorkerRainInferenceTask::new(binding, serving, Some(IterationRainAfterServing));
        RetainedWorkerSidecarTask::run(&mut task, now_ms, _repository, &mut ())
    }

    /// retain the real worker maintenance due calculation
    fn capture_adjustment_maintenance(
        &mut self,
        _repository: &mut Repository,
        due: MaintenanceCaptureDue,
    ) -> Result<(), TaskFailure> {
        self.events.push("maintenance");
        self.maintenance_due.push(due);
        Ok(())
    }
}

#[derive(Default)]
struct IterationDiagnostics(Vec<WorkerSidecarDiagnostic>);

impl WorkerSidecarDiagnosticSink for IterationDiagnostics {
    /// retain the exact bounded sidecar diagnostic sequence
    fn record_sidecar_diagnostic(&mut self, diagnostic: WorkerSidecarDiagnostic) {
        self.0.push(diagnostic);
    }
}

/// Run discovery, provider work, durable completion, and heartbeat through one database.
#[test]
fn dispatches_discovered_open_meteo_current_through_shared_iteration_database() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let transport_evidence = Rc::new(RefCell::new(DispatcherTransportEvidence::default()));
    let (source, _) = source_configuration();
    let mut forecast = source.clone();
    forecast.adapter_version = OPEN_METEO_FORECAST_ADAPTER_VERSION;
    forecast.id = FORECAST_SOURCE_ID.to_owned();
    forecast.source_key = "forecast".to_owned();
    forecast.source_kind = OpenMeteoSourceKind::Forecast;
    let delegated_sources = Rc::new(RefCell::new(Vec::new()));
    let mut database = FixtureDatabase {
        checkpoint: true,
        discover_at_ms: Vec::new(),
        due_sources: vec![
            WorkerDueSource::OpenMeteo(source),
            WorkerDueSource::OpenMeteo(forecast),
        ],
        expected_attempts: 1,
        expected_failure_classification: IngestionErrorClassification::Retryable,
        expected_failure_code: "unused_success".to_owned(),
        heartbeats: Vec::new(),
        retained_source_id: SOURCE_ID.to_owned(),
        trace: Rc::clone(&trace),
    };
    let mut values = FixtureValues;
    let site = dispatcher_site_configuration();
    let mut material = DispatcherMaterial;
    let transport = DispatcherTransport {
        evidence: Rc::clone(&transport_evidence),
        trace: Rc::clone(&trace),
    };
    let mut dispatcher = WorkerOpenMeteoDispatcher::from_site(
        &site,
        &mut material,
        transport,
        DelegatedDispatcher {
            sources: Rc::clone(&delegated_sources),
        },
        ProviderControls {
            max_attempts: 1,
            max_body_bytes: 2_000_000,
            timeout_ms: 30_000,
        },
    )
    .expect("site configuration should admit the dispatcher");
    let mut continuation = WorkerContinuation::new(None);
    let mut clocks = VecDeque::from([
        1_600_000, 1_700_000, 1_799_000, 1_800_000, 1_801_000, 1_802_000, 1_803_000,
    ]);
    let result = {
        let mut repository = WorkerIterationDatabaseRepository::new(&mut database, &mut values);
        run_worker_iteration(
            &mut continuation,
            &mut repository,
            &mut dispatcher,
            &mut || {
                let clock = clocks.pop_front().expect("iteration clock should exist");
                trace.borrow_mut().push(format!("clock:{clock}"));
                clock
            },
        )
        .expect("shared worker iteration should complete")
    };

    assert!(clocks.is_empty());
    assert_eq!(database.discover_at_ms, [1_700_000]);
    assert_eq!(result.sources.len(), 2);
    assert_eq!(result.sources[0].status, SourceRunStatus::Succeeded);
    assert_eq!(result.sources[0].successful_at_ms, Some(1_802_000));
    assert_eq!(result.sources[1].status, SourceRunStatus::Skipped);
    assert_eq!(result.last_success_at_ms, Some(1_802_000));
    assert_eq!(result.completed_at_ms, 1_803_000);
    assert_eq!(
        delegated_sources.borrow().as_slice(),
        [(FORECAST_SOURCE_ID.to_owned(), OpenMeteoSourceKind::Forecast)]
    );
    assert_eq!(
        database.heartbeats,
        [WorkerHeartbeat {
            activity: None,
            last_loop_at_ms: 1_803_000,
            last_success_at_ms: Some(1_802_000),
        }]
    );
    let evidence = transport_evidence.borrow();
    assert_eq!(
        evidence.attempts,
        [ProviderAttempt {
            attempt: 1,
            max_body_bytes: 2_000_000,
            timeout_ms: 30_000,
        }]
    );
    assert_eq!(evidence.bindings.len(), 1);
    let (location, request) = &evidence.bindings[0];
    assert_eq!(
        *location,
        OpenMeteoWorkerLocation {
            latitude: site.site.latitude,
            longitude: site.site.longitude,
        }
    );
    assert_ne!(location.latitude, site.station.latitude);
    assert_ne!(location.longitude, site.station.longitude);
    assert_eq!(request.endpoint, "forecast/current");
    assert_eq!(
        request.previous_cursor,
        Some(OpenMeteoCursor::ValidAt(1_680_000))
    );
    assert_eq!(request.source_id, SOURCE_ID);
    assert_eq!(request.source_kind, OpenMeteoSourceKind::ModelCurrent);
    assert_eq!(request.timezone, "Europe/London");
    assert_eq!(request.window.start_ms, 1_680_000);
    assert_eq!(request.window.end_exclusive_ms, 1_800_000);
    assert_eq!(
        trace.borrow().as_slice(),
        [
            "clock:1600000",
            "clock:1700000",
            "discover",
            "clock:1799000",
            "acquire",
            "clock:1800000",
            "abandon",
            "checkpoint",
            "start-lookup",
            "start-insert",
            "provider-bind",
            "provider-attempt-1",
            "complete-begin",
            "complete-assert",
            "records",
            "checkpoint-update",
            "finalize-success",
            "complete-commit",
            "release",
            "clock:1801000",
            "clock:1802000",
            "clock:1803000",
            "heartbeat",
        ]
    );
}

/// Run the paired forecast dispatcher through the same discovery and heartbeat database.
#[test]
fn dispatches_discovered_open_meteo_forecast_through_shared_iteration_database() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let current_transport_evidence = Rc::new(RefCell::new(DispatcherTransportEvidence::default()));
    let weather_attempts = Arc::new(Mutex::new(Vec::new()));
    let air_quality_attempts = Arc::new(Mutex::new(Vec::new()));
    let transport_evidence = Arc::new(Mutex::new(ForecastTransportEvidence::default()));
    let evidence_inputs = Arc::new(Mutex::new(ForecastEvidenceInputs::default()));
    let delegated_sources = Rc::new(RefCell::new(Vec::new()));
    let mut database = FixtureDatabase {
        checkpoint: true,
        discover_at_ms: Vec::new(),
        due_sources: vec![WorkerDueSource::OpenMeteo(forecast_source_configuration())],
        expected_attempts: 2,
        expected_failure_classification: IngestionErrorClassification::Retryable,
        expected_failure_code: "unused_success".to_owned(),
        heartbeats: Vec::new(),
        retained_source_id: FORECAST_SOURCE_ID.to_owned(),
        trace: Rc::clone(&trace),
    };
    let mut values = FixtureValues;
    let site = dispatcher_site_configuration();
    let mut material = DispatcherMaterial;
    let current = WorkerOpenMeteoDispatcher::from_site(
        &site,
        &mut material,
        DispatcherTransport {
            evidence: Rc::clone(&current_transport_evidence),
            trace: Rc::clone(&trace),
        },
        DelegatedDispatcher {
            sources: Rc::clone(&delegated_sources),
        },
        ProviderControls {
            max_attempts: 1,
            max_body_bytes: 2_000_000,
            timeout_ms: 30_000,
        },
    )
    .expect("current fallback should load");
    let mut dispatcher = current
        .with_forecast(
            &site,
            &mut material,
            ForecastTransport {
                air_quality: ForecastExchange {
                    attempts: Arc::clone(&air_quality_attempts),
                    product: Some(forecast_air_quality_product()),
                    status: 206,
                },
                evidence: Arc::clone(&transport_evidence),
                weather: ForecastExchange {
                    attempts: Arc::clone(&weather_attempts),
                    product: Some(forecast_weather_product()),
                    status: 200,
                },
            },
            ForecastEvidence {
                inputs: Arc::clone(&evidence_inputs),
            },
        )
        .expect("forecast dispatcher should load");
    let mut continuation = WorkerContinuation::new(None);
    let mut clocks = VecDeque::from([
        1_600_000, 1_700_000, 1_799_000, 1_800_000, 1_801_000, 1_802_000, 1_803_000,
    ]);
    let result = {
        let mut repository = WorkerIterationDatabaseRepository::new(&mut database, &mut values);
        run_worker_iteration(
            &mut continuation,
            &mut repository,
            &mut dispatcher,
            &mut || {
                let clock = clocks.pop_front().expect("forecast clock should exist");
                trace.borrow_mut().push(format!("clock:{clock}"));
                clock
            },
        )
        .expect("forecast iteration should complete")
    };

    assert!(clocks.is_empty());
    assert_eq!(result.sources.len(), 1);
    assert_eq!(result.sources[0].attempts, 2);
    assert_eq!(result.sources[0].record_count, 1);
    assert_eq!(result.sources[0].status, SourceRunStatus::Succeeded);
    assert_eq!(result.sources[0].successful_at_ms, Some(1_802_000));
    assert_eq!(result.last_success_at_ms, Some(1_802_000));
    assert_eq!(result.completed_at_ms, 1_803_000);
    assert!(current_transport_evidence.borrow().bindings.is_empty());
    assert!(delegated_sources.borrow().is_empty());
    assert_eq!(
        *weather_attempts.lock().expect("weather attempts lock"),
        [ProviderAttempt {
            attempt: 1,
            max_body_bytes: 2_000_000,
            timeout_ms: 30_000,
        }]
    );
    assert_eq!(
        *air_quality_attempts
            .lock()
            .expect("air-quality attempts lock"),
        [ProviderAttempt {
            attempt: 1,
            max_body_bytes: 2_000_000,
            timeout_ms: 30_000,
        }]
    );
    let transport = transport_evidence.lock().expect("transport evidence lock");
    assert_eq!(transport.bindings.len(), 1);
    assert_eq!(transport.receipt_clock_calls, 1);
    let (location, request) = &transport.bindings[0];
    assert_eq!(
        *location,
        OpenMeteoWorkerLocation {
            latitude: 54.0,
            longitude: -4.0,
        }
    );
    assert_eq!(request.endpoint, "forecast/hourly");
    assert_eq!(
        request.previous_cursor,
        Some(OpenMeteoCursor::ProductRunAt(1_795_000))
    );
    assert_eq!(request.source_id, FORECAST_SOURCE_ID);
    assert_eq!(request.source_kind, OpenMeteoSourceKind::Forecast);
    assert_eq!(request.timezone, "Europe/London");
    assert_eq!(request.window.start_ms, 1_680_000);
    assert_eq!(request.window.end_exclusive_ms, 1_800_000);
    drop(transport);
    let evidence = evidence_inputs.lock().expect("forecast evidence lock");
    assert_eq!(
        evidence.checksum_inputs,
        [format!("{WEATHER_CHECKSUM}\n{AIR_QUALITY_CHECKSUM}\n").into_bytes()]
    );
    assert_eq!(
        evidence.metadata_inputs,
        [vec![
            (utf16("air_quality_http_status"), JsonValue::Number(206.0)),
            (utf16("air_quality_generation_ms"), JsonValue::Number(7.25)),
            (utf16("http_status"), JsonValue::Number(200.0)),
            (utf16("upstream_response_count"), JsonValue::Number(2.0)),
            (utf16("weather_generation_ms"), JsonValue::Number(12.5)),
        ]]
    );
    assert_eq!(
        database.heartbeats,
        [WorkerHeartbeat {
            activity: None,
            last_loop_at_ms: 1_803_000,
            last_success_at_ms: Some(1_802_000),
        }]
    );
    assert_eq!(
        trace.borrow().as_slice(),
        [
            "clock:1600000",
            "clock:1700000",
            "discover",
            "clock:1799000",
            "acquire",
            "clock:1800000",
            "abandon",
            "checkpoint",
            "start-lookup",
            "start-insert",
            "complete-begin",
            "complete-assert",
            "records",
            "checkpoint-update",
            "finalize-success",
            "complete-commit",
            "release",
            "clock:1801000",
            "clock:1802000",
            "clock:1803000",
            "heartbeat",
        ]
    );
}

/// run the ordered coordinator through one production provider and fake PostgreSQL
#[test]
fn drains_ready_sources_with_production_provider_and_typed_heartbeat() {
    let (source, _) = source_configuration();
    let mut failed_source = source.clone();
    failed_source.id = "source-dispatch-error".to_owned();
    let mut following_source = source.clone();
    following_source.id = "source-after".to_owned();
    let mut database = IterationPg {
        due_sources: vec![
            WorkerDueSource::OpenMeteo(source),
            WorkerDueSource::OpenMeteo(failed_source),
            WorkerDueSource::OpenMeteo(following_source),
        ],
        ..IterationPg::default()
    };
    let mut values = FixtureValues;
    let mut dispatcher = IterationDispatcher::default();
    let mut sidecars = IterationSidecars::default();
    let mut sidecar_diagnostics = IterationDiagnostics::default();
    let mut continuation = WorkerContinuation::new(Some(1_500_000));
    let gate = NonOverlappingGate::new();
    let mut clocks = VecDeque::from([
        1_600_000, 1_700_000, 1_801_000, 1_802_000, 1_803_000, 1_804_000, 1_805_000, 1_806_000,
        1_807_000, 1_808_000, 1_809_000, 1_810_000, 1_811_000, 1_812_000, 1_813_000, 1_814_000,
    ]);
    let mut captured_clocks = Vec::new();
    let run = {
        let mut repository = WorkerIterationDatabaseRepository::new(&mut database, &mut values);
        run_worker_iteration_once_with_sidecars(
            &gate,
            &mut continuation,
            &mut repository,
            &mut dispatcher,
            WorkerSidecarExecution::new(
                WorkerSidecarPlan {
                    adjustment_capture_epoch_configured: true,
                    maintenance_capture_configured: true,
                    rain_adjustment_revision_client_configured: true,
                    rain_capture_configured: true,
                    rain_inference_legacy_enabled: true,
                    rain_inference_runtime: RainAdjustmentRuntimeSelection::Active,
                    temperature_canary: VerifiedTemperatureCanaryCollection::Active,
                },
                &mut sidecars,
                &mut sidecar_diagnostics,
            ),
            &mut || {
                let clock = clocks.pop_front().expect("coordinator clock should exist");
                captured_clocks.push(clock);
                clock
            },
        )
        .expect("typed worker drain should complete")
    };
    // require the idle gate to report a completed iteration
    let WorkerIterationRun::Completed(result) = run else {
        panic!("idle gate should complete the worker iteration");
    };

    assert_eq!(
        dispatcher.visited,
        [SOURCE_ID, "source-dispatch-error", "source-after"]
    );
    assert_eq!(
        captured_clocks,
        [
            1_600_000, 1_700_000, 1_801_000, 1_802_000, 1_803_000, 1_804_000, 1_805_000, 1_806_000,
            1_807_000, 1_808_000, 1_809_000, 1_810_000, 1_811_000, 1_812_000, 1_813_000, 1_814_000,
        ]
    );
    assert!(clocks.is_empty());
    assert_eq!(database.discover_at_ms, [1_700_000]);
    assert_eq!(result.sources.len(), 3);
    assert_eq!(result.sources[0].status, SourceRunStatus::Succeeded);
    assert_eq!(result.sources[0].successful_at_ms, Some(1_801_000));
    assert_eq!(result.sources[1].status, SourceRunStatus::Failed);
    assert_eq!(
        result.sources[1].reason.as_deref(),
        Some("worker_source_failure")
    );
    assert_eq!(result.sources[2].status, SourceRunStatus::Succeeded);
    assert_eq!(result.last_success_at_ms, Some(1_803_000));
    assert_eq!(
        sidecars.events,
        [
            "temperature",
            "rain",
            "fixed-gauge",
            "inference",
            "maintenance"
        ]
    );
    assert_eq!(sidecars.inference_now_ms, [1_811_000]);
    assert_eq!(
        sidecars
            .temperature_trace
            .borrow()
            .iter()
            .filter(|event| event.starts_with("temperature-persist:"))
            .count(),
        2
    );
    assert!(
        sidecars
            .temperature_trace
            .borrow()
            .contains(&"temperature-fetch:2026-09-07T18:00:00.000Z:1:5000:512000".to_owned())
    );
    assert_eq!(
        sidecars.inference_trace.borrow().as_slice(),
        [format!("inference-read:{FINGERPRINT}")]
    );
    assert_eq!(
        sidecars.maintenance_due,
        [adjustment_maintenance_capture_due_at(1_700_000)
            .expect("fixture maintenance due clock should be valid")]
    );
    assert_eq!(sidecar_diagnostics.0.len(), 4);
    assert_eq!(
        sidecar_diagnostics.0[0].source_id,
        "ecmwf-temperature-canary"
    );
    assert_eq!(
        sidecar_diagnostics.0[1].source_id,
        "rain-prospective-capture"
    );
    assert_eq!(sidecar_diagnostics.0[1].count, 2);
    assert_eq!(sidecar_diagnostics.0[1].error_code, None);
    assert_eq!(
        sidecars.rain_trace.borrow().as_slice(),
        [
            "rain-claim:forecast:2026-09-14T06:00:00.000Z:1:2026.09.14-1",
            "rain-fetch:forecast:2026-09-14T06:00:00.000Z:1",
            "rain-receipt:claim-1:48",
            "rain-sleep:1100",
            "rain-claim:station:64255:2026-09-14T12:00:01.000Z:2026.09.14-1",
            "rain-fetch:station:64255:2026-09-14T12:00:01.000Z",
            "rain-receipt:claim-2:48",
        ]
    );
    assert_eq!(
        sidecar_diagnostics.0[2].source_id,
        "rain-fixed-gauge-target"
    );
    assert_eq!(sidecar_diagnostics.0[2].count, 0);
    assert_eq!(sidecar_diagnostics.0[2].error_code, None);
    assert_eq!(
        sidecars.fixed_gauge_trace.borrow().as_slice(),
        ["fixed-catalog", "fixed-pending:2000-01-01T00:00:00.000Z",]
    );
    assert_eq!(sidecar_diagnostics.0[3].source_id, "rain-adjustment");
    assert_eq!(
        database.heartbeats,
        [WorkerHeartbeat {
            activity: Some(WorkerActivity::Degraded),
            last_loop_at_ms: 1_814_000,
            last_success_at_ms: Some(1_803_000),
        }]
    );
    assert_eq!(dispatcher.actual_attempts.len(), 2);
    assert_eq!(
        dispatcher.actual_trace,
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
}

// proves forecast and null cursors retain their exact admitted Node JSON shape
#[test]
fn maps_typed_forecast_and_null_open_meteo_cursors() {
    let mut values = FixtureValues;
    let encoded = <OpenMeteoDatabaseAdapter as ScheduledDatabaseAdapter<FixtureValues>>::
        provider_cursor_json(
            &mut values,
            &Some(OpenMeteoCursor::ProductRunAt(1_795_000)),
        )
        .expect("forecast cursor should encode");
    assert_eq!(
        encoded,
        Some(SerializedJson::admitted(
            r#"{"product_run_at":"1970-01-01T00:29:55.000Z"}"#,
        ))
    );

    let checkpoint = checkpoint_state(Nullable::Value(vec![(
        utf16("product_run_at"),
        json_string(RECEIVED_AT),
    )]));
    let decoded =
        <OpenMeteoDatabaseAdapter as ScheduledDatabaseAdapter<FixtureValues>>::checkpoint_cursor(
            &mut values,
            &checkpoint,
        )
        .expect("forecast cursor should decode");
    assert_eq!(decoded, Some(OpenMeteoCursor::ProductRunAt(1_795_000)));

    let encoded_null =
        <OpenMeteoDatabaseAdapter as ScheduledDatabaseAdapter<FixtureValues>>::
            provider_cursor_json(&mut values, &None)
            .expect("null cursor should encode");
    assert_eq!(encoded_null, None);
    let decoded_null =
        <OpenMeteoDatabaseAdapter as ScheduledDatabaseAdapter<FixtureValues>>::checkpoint_cursor(
            &mut values,
            &checkpoint_state(Nullable::Null),
        )
        .expect("null cursor should decode");
    assert_eq!(decoded_null, None);
}

// verifies a normalized current record reaches atomic storage in exact order
#[test]
fn persists_normalized_open_meteo_current_atomically() {
    let (result, evidence) = execute(ProviderOutcome::Success);
    assert_eq!(result.status, SourceRunStatus::Succeeded);
    assert_eq!(result.attempts, 2);
    assert_eq!(result.record_count, 1);
    assert_eq!(result.run_id.as_deref(), Some("run-1"));
    assert_eq!(result.successful_at_ms, Some(1_801_000));
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
                max_body_bytes: 2_000_000,
                timeout_ms: 10_000,
            },
            ProviderAttempt {
                attempt: 2,
                max_body_bytes: 2_000_000,
                timeout_ms: 10_000,
            },
        ]
    );
    assert_eq!(
        evidence.sleeps,
        [ProviderSleep {
            attempts: 1,
            deadline_remaining_ms: Some(30_000),
            delay_ms: 300,
        }]
    );
}

// verifies the remaining run deadline bounds the actual provider attempt
#[test]
fn bounds_open_meteo_attempt_by_scheduled_deadline() {
    let (result, evidence) = execute(ProviderOutcome::DeadlineBoundedSuccess);
    assert_eq!(result.status, SourceRunStatus::Succeeded);
    assert_eq!(result.attempts, 1);
    assert_eq!(result.record_count, 1);
    assert_eq!(
        evidence.attempts,
        [ProviderAttempt {
            attempt: 1,
            max_body_bytes: 2_000_000,
            timeout_ms: 500,
        }]
    );
    assert!(evidence.sleeps.is_empty());
    assert_eq!(
        evidence.trace,
        [
            "acquire",
            "abandon",
            "checkpoint",
            "start-lookup",
            "start-insert",
            "provider-attempt-1",
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

// verifies provider failure is durably finalized before release
#[test]
fn finalizes_provider_failure_before_releasing_session() {
    let (result, evidence) = execute(ProviderOutcome::Failure);
    assert_eq!(result.status, SourceRunStatus::Failed);
    assert_eq!(result.attempts, 3);
    assert_eq!(result.reason.as_deref(), Some("provider_server_error"));
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

// verifies content-budget rejection is finalized without normalization or retry
#[test]
fn finalizes_open_meteo_content_budget_failure() {
    let (result, evidence) = execute(ProviderOutcome::ContentBudgetFailure);
    assert_eq!(result.status, SourceRunStatus::Failed);
    assert_eq!(result.attempts, 1);
    assert_eq!(
        result.reason.as_deref(),
        Some("provider_response_too_large")
    );
    assert_eq!(evidence.attempts.len(), 1);
    assert!(evidence.sleeps.is_empty());
    assert_eq!(
        evidence.trace,
        [
            "acquire",
            "abandon",
            "checkpoint",
            "start-lookup",
            "start-insert",
            "provider-attempt-1",
            "failure-begin",
            "failure-assert",
            "failure-finalize",
            "failure-commit",
            "release",
        ]
    );
}

// verifies a wrong retained source is rejected before session commands
#[test]
fn rejects_mismatched_retained_source_before_commands() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut database = FixtureDatabase {
        checkpoint: false,
        discover_at_ms: Vec::new(),
        due_sources: Vec::new(),
        expected_attempts: 2,
        expected_failure_classification: IngestionErrorClassification::Retryable,
        expected_failure_code: "unused_success".to_owned(),
        heartbeats: Vec::new(),
        retained_source_id: "source-2".to_owned(),
        trace: Rc::clone(&trace),
    };
    let mut values = FixtureValues;
    let mut repository = OpenMeteoDatabaseRepository::new(&mut database, &mut values);
    let mut provider = provider_fixture(ProviderOutcome::Success, Rc::clone(&trace));
    let (source, configuration) = source_configuration();
    let error = run_scheduled_source(
        ScheduledRunExecution {
            configuration: &configuration,
            maximum_range_seconds: None,
            now_ms: 1_800_000,
            run_deadline_at_ms: 1_860_000,
            source: &source,
        },
        &mut repository,
        &mut provider,
        &mut || 1_801_000,
    )
    .expect_err("mismatched retained source");
    assert_eq!(error.code, "source_session_mismatch");
    assert_eq!(*trace.borrow(), ["acquire", "release"]);
    assert!(provider.io().attempts.is_empty());
}
