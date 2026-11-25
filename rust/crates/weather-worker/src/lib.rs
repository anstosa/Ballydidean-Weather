//! Dependency-free worker scheduling policy shared by native worker entrypoints.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::atomic::{AtomicBool, Ordering};

pub use historical_backfill_command::*;
pub use open_meteo_backfill_arguments::*;
pub use weather_adjustment::{
    AdmittedRainHurdleWindCalendar, AdmittedTemperatureRecentErrorForecast,
    AdmittedTemperatureRecentErrorHour, AdmittedTemperatureRecentErrorInput,
    AdmittedTemperatureRecentErrorStation, ForecastAdjustmentRainDiagnosticCalendar,
    ForecastAdjustmentScorecardCalendar, build_admitted_temperature_recent_error_state,
};

/// Numerical owners used by the worker's pre-holdout wind maintenance command.
pub mod wind_maintenance_development {
    pub use weather_adjustment::{
        AdmittedWindDevelopmentDate, AdmittedWindForecastTrainingRow, AdmittedWindRowCalendar,
        AdmittedWindStationTrainingRow, AdmittedWindTrainingRow, WindAuxiliaryModel,
        WindDevelopmentQualification, WindDevelopmentQualifiedPair, WindDevelopmentRunError,
        WindMaintenanceTrainingPreparation, WindTrainingEventError,
        forecast_adjustment_metric_policies, prepare_wind_maintenance_training,
        qualify_wind_maintenance_development, validate_wind_development_fold,
        wind_development_pair_passes,
    };
}

/// Numerical owners used by the offline wind requalification command.
pub mod wind_performance_numerics {
    pub use weather_adjustment::{
        AdmittedWindBundleRow, AdmittedWindRowFeatures, AdmittedWindV2MetricOrder,
        evaluate_admitted_wind_bundle_row, evaluate_admitted_wind_v2_row,
    };
}

mod ecowitt;
mod forecast_anchor_backfill;
mod historical_backfill_command;
mod noaa_tides;
mod open_meteo_archive_backfill;
mod open_meteo_backfill_arguments;
mod public_station_backfill;
mod public_station_netatmo;
mod public_station_purpleair;
mod public_station_wunderground;
mod public_stations;
mod rain_adjustment;
mod rain_collection;
mod rain_fixed_gauge;
mod rain_performance_evidence;
mod rain_performance_report;
mod rain_portable_replay;
mod rain_shadow_source;
mod scheduled_source_identity;
mod scheduler_timing;
mod startup;
mod supplemental_public_station;
mod temperature_canary;
mod tempest;
mod tempest_backfill;
mod tide_backfill;
mod wind_performance_report;

pub use wind_performance_report::{
    WindPerformanceNonActiveCounts, WindPerformanceReportError, create_wind_performance_report,
};

pub use ecowitt::{
    ECOWITT_LOCAL_LIVE_ADAPTER_VERSION, ECOWITT_LOCAL_LIVE_ENDPOINT,
    ECOWITT_MAXIMUM_CADENCE_SECONDS, ECOWITT_MINIMUM_CADENCE_SECONDS, EcowittAdmittedLocalDate,
    EcowittBatteryState, EcowittCanonicalMetadataAdapter, EcowittCanonicalMetadataMeasurement,
    EcowittCapability, EcowittCommonReadings, EcowittDeviceMetadata, EcowittGatewayReadings,
    EcowittLivePayload, EcowittLiveRequest, EcowittMeasurement, EcowittParticulateSensor,
    EcowittPreviousCursor, EcowittProviderCursor, EcowittProviderMetadata, EcowittQualityMetadata,
    EcowittRainGaugeKind, EcowittRainReadings, EcowittResponseMetadata,
    EcowittRuntimeConfiguration, EcowittScheduledExecution, EcowittScheduledRequest,
    EcowittScheduledSource, EcowittSensorReadings, EcowittSensorSnapshot, EcowittSoilSensor,
    EcowittSourceKind, EcowittTemperatureSensor, EcowittTimeAdapter, EcowittUnit,
    MAXIMUM_ECOWITT_PROVIDER_METADATA_JS_STRING_UNITS, NormalizedEcowittBatch,
    NormalizedEcowittRecord, normalize_ecowitt_live, require_ecowitt_gateway_identity,
};
pub use forecast_anchor_backfill::{
    FORECAST_ANCHOR_ADAPTER_VERSION, FORECAST_ANCHOR_CHUNK_PLAN_VERSION,
    FORECAST_ANCHOR_CONTRACT_EPOCH, FORECAST_ANCHOR_CONTRACT_VERSION, FORECAST_ANCHOR_DATASET,
    FORECAST_ANCHOR_ENDPOINT, FORECAST_ANCHOR_MAXIMUM_CHUNK_DAYS, FORECAST_ANCHOR_MODEL,
    ForecastAnchorAdapterConfiguration, ForecastAnchorAttemptPayload,
    ForecastAnchorBackfillArguments, ForecastAnchorBackfillChunkIdentity,
    ForecastAnchorBackfillChunkReport, ForecastAnchorBackfillChunkStatus,
    ForecastAnchorBackfillCompletion, ForecastAnchorBackfillDate, ForecastAnchorBackfillDeadlines,
    ForecastAnchorBackfillOperationError, ForecastAnchorBackfillProvider,
    ForecastAnchorBackfillProviderFailure, ForecastAnchorBackfillReport,
    ForecastAnchorBackfillRepository, ForecastAnchorBackfillRunStart, ForecastAnchorBackfillSource,
    ForecastAnchorBackfillTimeAdapter, ForecastAnchorCapability, ForecastAnchorEnvelope,
    ForecastAnchorIngestionMode, ForecastAnchorLeadCounts, ForecastAnchorLocation,
    ForecastAnchorMetricCells, ForecastAnchorParsedHour, ForecastAnchorParsedLocation,
    ForecastAnchorProviderRequest, ForecastAnchorResponseMetadata,
    ForecastAnchorRuntimeConfiguration, ForecastAnchorSourceKind, ForecastAnchorTimezone,
    PlannedForecastAnchorBackfillChunk, execute_forecast_anchor_backfill,
    normalize_forecast_anchor_payload, plan_forecast_anchor_backfill_chunks,
    validate_forecast_anchor_backfill_source,
};
pub use noaa_tides::{
    MAXIMUM_NOAA_OBSERVATION_RANGE_SECONDS, MAXIMUM_NOAA_PREDICTION_RANGE_SECONDS,
    NOAA_SCHEDULED_MAXIMUM_CADENCE_SECONDS, NOAA_SCHEDULED_MINIMUM_CADENCE_SECONDS,
    NOAA_SCHEDULED_OBSERVATION_LAG_MILLISECONDS, NOAA_SCHEDULED_PREDICTION_DAYS,
    NOAA_TIDE_CHUNK_PLAN_VERSION, NOAA_TIDE_DATA_ENDPOINT, NOAA_TIDE_OBSERVATION_ADAPTER_VERSION,
    NOAA_TIDE_PREDICTION_ADAPTER_VERSION, NoaaPredictionType, NoaaTideCapability,
    NoaaTideDeviceMetadata, NoaaTideObservation, NoaaTidePayload, NoaaTidePrediction,
    NoaaTideProduct, NoaaTideProviderCursor, NoaaTideProviderMetadata, NoaaTideQualityMetadata,
    NoaaTideRangeRequest, NoaaTideResponseMetadata, NoaaTideRuntimeConfiguration,
    NoaaTideScheduledExecution, NoaaTideScheduledSource, NoaaTideSourceKind,
    NormalizedNoaaTideBatch, NormalizedNoaaTideRecord, noaa_tide_adapter_version,
    normalize_noaa_tide_range,
};
pub use open_meteo_archive_backfill::{
    OPEN_METEO_ARCHIVE_ADAPTER_VERSION, OPEN_METEO_ARCHIVE_CHUNK_PLAN_VERSION,
    OPEN_METEO_ARCHIVE_CONTRACT_VERSION, OPEN_METEO_ARCHIVE_ENDPOINT,
    OPEN_METEO_ARCHIVE_MAXIMUM_CHUNK_DAYS, OPEN_METEO_ARCHIVE_SOURCE_KEY,
    OpenMeteoArchiveCleanupFailure, OpenMeteoArchiveCompletion, OpenMeteoArchiveInstant,
    OpenMeteoArchiveProvider, OpenMeteoArchiveRepository, OpenMeteoArchiveRequest,
    OpenMeteoArchiveRunStart, OpenMeteoArchiveSource, OpenMeteoArchiveTimeAdapter,
    PlannedOpenMeteoArchiveChunk, execute_open_meteo_archive_backfill,
    plan_open_meteo_archive_chunks,
};
pub use public_station_backfill::{
    AMBIENT_WEATHER_BACKFILL_ENDPOINT, AmbientWeatherBackfillProvider,
    AmbientWeatherBackfillSource, PUBLIC_STATION_CHUNK_PLAN_VERSION,
    PlannedPublicStationBackfillChunk, PublicStationArchiveBackfillBatch,
    PublicStationArchiveBackfillProvider, PublicStationArchiveBackfillSource,
    PublicStationBackfillArguments, PublicStationBackfillChunkIdentity,
    PublicStationBackfillCompletion, PublicStationBackfillDate, PublicStationBackfillDeadlines,
    PublicStationBackfillErrorClassification, PublicStationBackfillFailedChunk,
    PublicStationBackfillIngestionError, PublicStationBackfillOperationError,
    PublicStationBackfillPlanningSource, PublicStationBackfillProviderFailure,
    PublicStationBackfillReport, PublicStationBackfillRepository, PublicStationBackfillRunStart,
    PublicStationBackfillSourceReport, PublicStationBackfillTimeAdapter,
    SupplementalPublicStationBackfillRepository, execute_ambient_weather_backfill,
    execute_public_station_archive_backfill, plan_ambient_weather_backfill_chunks,
    plan_public_station_backfill_dry_run, select_ambient_weather_backfill_sources,
};
pub use public_station_netatmo::*;
pub use public_station_purpleair::*;
pub use public_station_wunderground::*;
pub use public_stations::{
    AMBIENT_WEATHER_ADAPTER_VERSION, AMBIENT_WEATHER_DEVICE_DATA_ENDPOINT,
    AMBIENT_WEATHER_MAXIMUM_CADENCE_SECONDS, AMBIENT_WEATHER_MINIMUM_CADENCE_SECONDS,
    AmbientWeatherCapability, AmbientWeatherDeviceIdentity, AmbientWeatherDeviceMetadata,
    AmbientWeatherObservation, AmbientWeatherPayload, AmbientWeatherProviderCursor,
    AmbientWeatherProviderMetadata, AmbientWeatherQualityMetadata, AmbientWeatherRangeRequest,
    AmbientWeatherResponseMetadata, AmbientWeatherRuntimeConfiguration,
    AmbientWeatherScheduledExecution, AmbientWeatherScheduledSource, AmbientWeatherSourceKind,
    MAXIMUM_AMBIENT_WEATHER_RANGE_SECONDS, NormalizedAmbientWeatherBatch,
    NormalizedAmbientWeatherRecord, normalize_ambient_weather_range,
};
pub use rain_adjustment::{
    RainAdjustmentAppendTransaction, RainAdjustmentCapture, RainAdjustmentCaptureIdentity,
    RainAdjustmentCaptureKind, RainAdjustmentClient, RainAdjustmentCommitReceipt,
    RainAdjustmentEvaluation, RainAdjustmentExecution, RainAdjustmentFeatureAdmission,
    RainAdjustmentGap, RainAdjustmentGapReason, RainAdjustmentHour, RainAdjustmentHourReason,
    RainAdjustmentIdentityAdapter, RainAdjustmentModel, RainAdjustmentPendingInput,
    RainAdjustmentPersistedRevision, RainAdjustmentProjection, RainAdjustmentProjectionCodec,
    RainAdjustmentRepository, RainAdjustmentRevisionExecution, RainAdjustmentRevisionPlan,
    RainAdjustmentRun, RainAdjustmentStageReceipt, RainAdjustmentStationHour,
    RainAdjustmentTimeAdapter, publish_rain_adjustment, publish_rain_adjustment_with_shadow,
    replay_retained_rain_performance,
};
pub use rain_collection::{
    RAIN_COLLECTION_POLICY, RAIN_COLLECTION_STATIONS, RainCaptureMetadataValue, RainCaptureOutcome,
    RainCaptureReceipt, RainCaptureRequest, RainCollectionClock, RainCollectionConfiguration,
    RainCollectionExecution, RainCollectionPacer, RainCollectionPolicy, RainCollectionProvider,
    RainCollectionRepository, RainCollectionResult, RainCollectionStation,
    RainCollectionTimeAdapter, RainCollectionTimeValue, collect_rain_evidence,
    is_rain_collection_enabled, plan_rain_capture_requests,
};
pub use rain_fixed_gauge::{
    RAIN_FIXED_GAUGE_TARGET_CATALOG_CONTRACT_VERSION, RAIN_FIXED_GAUGE_TARGET_CATALOG_SHA256,
    RAIN_FIXED_GAUGE_TARGET_RUN_CONTRACT_SHA256, RainFixedGaugeBuildResult, RainFixedGaugeCapture,
    RainFixedGaugeCaptureMember, RainFixedGaugeClient, RainFixedGaugeCommitReceipt,
    RainFixedGaugeDurableGap, RainFixedGaugeGapReason, RainFixedGaugeGapStatus,
    RainFixedGaugeIdentityAdapter, RainFixedGaugeNormalizedRecord, RainFixedGaugeProjection,
    RainFixedGaugeProjectionBuilder, RainFixedGaugeProjectionRow, RainFixedGaugeRepository,
    RainFixedGaugeRevisionGap, RainFixedGaugeRevisionGroup, RainFixedGaugeRevisionResult,
    RainFixedGaugeRevisionRow, RainFixedGaugeSource, RainFixedGaugeSourceCatalog,
    RainFixedGaugeStageReceipt, RainFixedGaugeTargetExecution, publish_rain_fixed_gauge_target,
};
pub use rain_performance_evidence::*;
pub use rain_performance_report::*;
pub use rain_portable_replay::{
    RainPortableReplayCalendar, replay_rain_maintenance_candidate, score_rain_control_ordinals,
};
pub use rain_shadow_source::{
    RAIN_SOURCE_CONFIG_FINGERPRINT, RainShadowSourceHeader, project_rain_shadow_source,
};
pub use scheduled_source_identity::*;
pub use scheduler_timing::{WorkerSchedulerTimingError, next_worker_scheduled_at_ms};
pub use startup::{
    AdmittedWorkerProcessConfiguration, AdmittedWorkerStartupTask, RetainedWorkerProcess,
    RunningWorkerProcess, WORKER_PROCESS_CADENCE_MS, WorkerProcessArgumentsError,
    WorkerProcessBackfillPort, WorkerProcessEnvironmentPort, WorkerProcessFailure,
    WorkerProcessIteration, WorkerProcessIterationFactory, WorkerProcessOptions,
    WorkerProcessPhase, WorkerProcessPoll, WorkerProcessPool, WorkerProcessPoolFactory,
    WorkerProcessPorts, WorkerProcessRuntimeLoader, WorkerProcessSchedule,
    WorkerProcessSchedulerPort, WorkerProcessStartResult, WorkerProcessStartupResult,
    WorkerProcessTrigger, parse_worker_process_arguments, start_worker_process,
};
pub use supplemental_public_station::{
    PublicStationNormalizationError, PublicStationNormalizedSample, PublicStationNumber,
    SupplementalPublicStationBatch, SupplementalPublicStationIdentity,
    SupplementalPublicStationInputs, SupplementalPublicStationProduct,
    SupplementalPublicStationRecord, SupplementalPublicStationResponseMetadata,
    SupplementalPublicStationScheduledConfiguration, SupplementalPublicStationScheduledRequest,
    normalize_supplemental_public_station_batch,
    normalize_supplemental_public_station_batch_with_diagnostics,
    run_scheduled_supplemental_public_station_source_after_lock,
    supplemental_public_station_contract, validate_supplemental_public_station_range,
};
pub use temperature_canary::{
    OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION, OPEN_METEO_ECMWF_SINGLE_RUN_MODEL,
    TemperatureCanaryAuthorization, TemperatureCanaryBatch, TemperatureCanaryCollection,
    TemperatureCanaryExecution, TemperatureCanaryFetchControls, TemperatureCanaryFetchLimits,
    TemperatureCanaryHour, TemperatureCanaryPersistedRun, TemperatureCanaryProvider,
    TemperatureCanaryRecentErrorBuilder, TemperatureCanaryRecentErrorState,
    TemperatureCanaryRepository, TemperatureCanaryRequest, TemperatureCanarySource,
    TemperatureCanaryStateStatus, TemperatureCanaryTimeAdapter,
    collect_ecmwf_temperature_canary_runs, temperature_canary_collection_is_active,
};
pub use tempest::{
    NormalizedTempestBatch, NormalizedTempestRecord, TEMPEST_MAXIMUM_RANGE_SECONDS,
    TEMPEST_OBSERVATION_ADAPTER_VERSION, TEMPEST_OBSERVATION_CHUNK_PLAN_VERSION,
    TEMPEST_OBSERVATION_ENDPOINT, TEMPEST_SOURCE_CONTRACT_VERSION, TempestDeviceMetadata,
    TempestObservation, TempestObservationEnvelope, TempestObservationRequest,
    TempestProviderCursor, TempestProviderMetadata, TempestQualityFlag, TempestQualityMetadata,
    TempestResponseMetadata, TempestResponseType, TempestRuntimeConfiguration,
    TempestScheduledExecution, TempestScheduledSource, TempestSourceKind,
    normalize_tempest_observations,
};
pub use tempest_backfill::{
    PlannedTempestBackfillChunk, TempestBackfillArguments, TempestBackfillChunkIdentity,
    TempestBackfillChunkReport, TempestBackfillChunkStatus, TempestBackfillCompletion,
    TempestBackfillDate, TempestBackfillDeadlines, TempestBackfillErrorClassification,
    TempestBackfillIngestionError, TempestBackfillOperationError, TempestBackfillProvider,
    TempestBackfillProviderFailure, TempestBackfillRepository, TempestBackfillRunStart,
    TempestBackfillSource, TempestBackfillTimeAdapter, TempestBulkBackfillReport,
    TempestStationBackfillReport, execute_tempest_bulk_backfill, plan_tempest_backfill_chunks,
    select_tempest_backfill_sources,
};
pub use tide_backfill::{
    PlannedTideBackfillChunk, TideBackfillArguments, TideBackfillChunkIdentity,
    TideBackfillCompletion, TideBackfillDate, TideBackfillDeadlines,
    TideBackfillErrorClassification, TideBackfillFailedChunk, TideBackfillIngestionError,
    TideBackfillOperationError, TideBackfillProvider, TideBackfillProviderFailure,
    TideBackfillProviderRequest, TideBackfillReport, TideBackfillRepository, TideBackfillRunStart,
    TideBackfillSource, TideBackfillSourceReport, TideBackfillTimeAdapter, execute_tide_backfill,
    plan_tide_backfill_chunks, select_tide_backfill_sources,
};

const MILLISECONDS_PER_SECOND: i64 = 1_000;
const MILLISECONDS_PER_DAY: i64 = 86_400_000;
const MINIMUM_CADENCE_MILLISECONDS: i64 = 60_000;
const MAINTENANCE_CYCLE_MILLISECONDS: i64 = 6 * 3_600_000;
const MAINTENANCE_DUE_OFFSET_MILLISECONDS: i64 = 35 * 60_000;
const ECMWF_AVAILABILITY_DELAY_MILLISECONDS: i64 = 6 * 3_600_000;

pub const OPEN_METEO_CURRENT_ADAPTER_VERSION: &str = "open-meteo-forecast-current/v1";
pub const OPEN_METEO_FORECAST_ADAPTER_VERSION: &str = "open-meteo-forecast-daily/v4";

/// One anchored half-open scheduled interval, expressed as Unix milliseconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduledWindow {
    pub end_exclusive_ms: i64,
    pub start_ms: i64,
}

/// One fixed maintenance capture decision, expressed as Unix milliseconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaintenanceCaptureDue {
    pub issued_at_ms: i64,
}

/// One allowlisted worker health result, expressed as Unix milliseconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerHealth {
    pub last_loop_at_ms: Option<i64>,
    pub last_success_at_ms: Option<i64>,
    pub live: bool,
    pub ready: bool,
    pub stale: bool,
}

/// The worker's in-process non-overlap guard.
#[derive(Debug, Default)]
pub struct NonOverlappingGate {
    running: AtomicBool,
}

/// One active run permit that releases the gate when dropped.
#[derive(Debug)]
pub struct RunPermit<'a> {
    gate: &'a NonOverlappingGate,
}

/// One durable scheduled checkpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduledCheckpoint<Cursor = Option<String>> {
    pub provider_cursor: Cursor,
    pub version: u64,
    pub window_end_exclusive_ms: i64,
}

/// One record returned by a scheduled provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduledRecord {
    pub valid_at_ms: i64,
}

/// One successful provider batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderBatch<Record = ScheduledRecord, ResponseMetadata = (), Cursor = Option<String>> {
    pub attempts: u32,
    pub provider_cursor: Cursor,
    pub records: Vec<Record>,
    pub response_metadata: ResponseMetadata,
    pub upstream_response_checksum: String,
}

/// One provider request derived from a durable checkpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderRequest {
    pub endpoint: &'static str,
    pub previous_cursor: Option<OpenMeteoCursor>,
    pub source_id: String,
    pub source_kind: OpenMeteoSourceKind,
    pub timezone: String,
    pub window: ScheduledWindow,
}

/// Metadata committed before one provider request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduledRunStart {
    pub adapter_version: &'static str,
    pub deadline_at_ms: i64,
    pub endpoint: &'static str,
    pub requested_window: ScheduledWindow,
    pub source_config_fingerprint: String,
}

/// Inputs for one admitted scheduled source execution.
#[derive(Clone, Copy, Debug)]
pub struct ScheduledRunExecution<'a> {
    pub configuration: &'a OpenMeteoRuntimeConfiguration,
    pub maximum_range_seconds: Option<u64>,
    pub now_ms: i64,
    pub run_deadline_at_ms: i64,
    pub source: &'a OpenMeteoScheduledSource,
}

/// One committed ingestion completion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduledCompletion<
    Record = ScheduledRecord,
    ResponseMetadata = (),
    Cursor = Option<String>,
> {
    pub attempts: u32,
    pub expected_checkpoint_version: Option<u64>,
    pub last_valid_at_ms: i64,
    pub provider_cursor: Cursor,
    pub record_count: usize,
    pub records: Vec<Record>,
    pub response_metadata: ResponseMetadata,
    pub run_id: String,
    pub upstream_response_checksum: String,
    pub window: ScheduledWindow,
}

/// One scheduled source run outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceRunStatus {
    Failed,
    Skipped,
    Succeeded,
}

/// One scheduled source run result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRunResult {
    pub attempts: u32,
    pub reason: Option<String>,
    pub record_count: usize,
    pub run_id: Option<String>,
    pub secondary_error_codes: Vec<String>,
    pub source_id: String,
    pub status: SourceRunStatus,
    pub successful_at_ms: Option<i64>,
}

/// One bounded worker task failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskFailure {
    pub attempts: u32,
    pub code: String,
}

/// One worker heartbeat activity classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerActivity {
    Degraded,
}

/// One heartbeat update after an iteration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerHeartbeat {
    pub activity: Option<WorkerActivity>,
    pub last_loop_at_ms: i64,
    pub last_success_at_ms: Option<i64>,
}

/// One completed worker iteration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerIterationResult {
    pub completed_at_ms: i64,
    pub last_success_at_ms: Option<i64>,
    pub sources: Vec<SourceRunResult>,
}

/// Admitted public-station source for the Weather Underground, Netatmo or PurpleAir lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicStationScheduledSource {
    pub adapter: weather_contracts::PublicStationAdapter,
    pub active: bool,
    pub cadence_seconds: u64,
    pub id: String,
    pub material_config_identity: String,
    pub provider_key: String,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub station_slug: String,
    pub timezone: String,
}

/// one ordered source discovered for an ordinary worker iteration
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkerDueSource {
    Ambient(AmbientWeatherScheduledSource),
    Ecowitt(EcowittScheduledSource),
    NoaaTide(NoaaTideScheduledSource),
    OpenMeteo(OpenMeteoScheduledSource),
    PublicStation(PublicStationScheduledSource),
    Tempest(TempestScheduledSource),
    Unconfigured(String),
    InvalidConfiguration {
        source_id: String,
        failure: TaskFailure,
    },
}

impl WorkerDueSource {
    /// return the durable source identity shared by every provider family
    pub fn source_id(&self) -> &str {
        // project the selected provider-specific source
        match self {
            Self::Ambient(source) => &source.id,
            Self::Ecowitt(source) => &source.id,
            Self::NoaaTide(source) => &source.id,
            Self::OpenMeteo(source) => &source.id,
            Self::PublicStation(source) => &source.id,
            Self::Tempest(source) => &source.id,
            Self::Unconfigured(id) => id,
            Self::InvalidConfiguration { source_id, .. } => source_id,
        }
    }
}

/// one gated worker iteration attempt
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkerIterationRun {
    Busy,
    Completed(WorkerIterationResult),
}

/// One temperature-canary admission decision already verified against runtime policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifiedTemperatureCanaryCollection {
    Inactive,
    Active,
}

/// One admitted rain-adjustment runtime selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RainAdjustmentRuntimeSelection {
    Unconfigured,
    Active,
    DisabledPolicyRawWithComparatorAuthority,
    Disabled,
}

/// Optional sidecars admitted for one ordinary worker iteration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerSidecarPlan {
    pub adjustment_capture_epoch_configured: bool,
    pub maintenance_capture_configured: bool,
    pub rain_adjustment_revision_client_configured: bool,
    pub rain_capture_configured: bool,
    pub rain_inference_legacy_enabled: bool,
    pub rain_inference_runtime: RainAdjustmentRuntimeSelection,
    pub temperature_canary: VerifiedTemperatureCanaryCollection,
}

/// One cohesive sidecar phase bound to its admitted plan and output ports.
pub struct WorkerSidecarExecution<'a, Tasks, Diagnostics> {
    pub diagnostics: &'a mut Diagnostics,
    pub plan: WorkerSidecarPlan,
    pub tasks: &'a mut Tasks,
}

impl<'a, Tasks, Diagnostics> WorkerSidecarExecution<'a, Tasks, Diagnostics> {
    /// Bind one admitted sidecar plan to its task and diagnostic ports.
    pub const fn new(
        plan: WorkerSidecarPlan,
        tasks: &'a mut Tasks,
        diagnostics: &'a mut Diagnostics,
    ) -> Self {
        Self {
            diagnostics,
            plan,
            tasks,
        }
    }
}

impl Default for WorkerSidecarPlan {
    // keep every optional sidecar absent unless its owning runtime admits it
    fn default() -> Self {
        Self {
            adjustment_capture_epoch_configured: false,
            maintenance_capture_configured: false,
            rain_adjustment_revision_client_configured: false,
            rain_capture_configured: false,
            rain_inference_legacy_enabled: false,
            rain_inference_runtime: RainAdjustmentRuntimeSelection::Unconfigured,
            temperature_canary: VerifiedTemperatureCanaryCollection::Inactive,
        }
    }
}

/// One completed temperature-canary collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemperatureCanaryCollectionResult {
    pub failed_runs: usize,
    pub persisted_runs: usize,
}

/// One completed prospective rain collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RainCaptureResult {
    pub failed: usize,
    pub valid: usize,
}

/// One fixed-gauge target publication decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FixedGaugeTargetResult {
    Idle,
    Published { revision_count: usize },
    Gap { reason: String },
}

/// One bounded optional-sidecar diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerSidecarDiagnostic {
    pub count: usize,
    pub duration_ms: i64,
    pub error_code: Option<String>,
    pub source_id: &'static str,
}

/// In-memory success state retained by a running worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerContinuation {
    last_success_at_ms: Option<i64>,
}

/// Typed envelope parsed from an Open-Meteo response.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoEnvelope {
    pub elevation_m: Option<f64>,
    pub latitude: f64,
    pub longitude: f64,
    pub upstream_timezone: String,
}

/// One scheduled Open-Meteo database identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenMeteoScheduledSource {
    pub active: bool,
    pub adapter_version: &'static str,
    pub cadence_seconds: u64,
    pub id: String,
    pub material_config_identity: String,
    pub provider_key: String,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub source_kind: OpenMeteoSourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// One loaded Open-Meteo runtime configuration identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenMeteoRuntimeConfiguration {
    pub active: bool,
    pub adapter_version: &'static str,
    pub capabilities: BTreeSet<OpenMeteoCapability>,
    pub material_config_identity: String,
    pub provider_key: String,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub source_kind: OpenMeteoSourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// One parsed configured source capability.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum OpenMeteoCapability {
    Current,
    Forecast,
    Historical,
    Stream,
}

/// Typed weather sample parsed from Open-Meteo arrays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpenMeteoWeatherSample {
    pub apparent_temperature_c: Option<f64>,
    pub cloud_cover_percent: Option<f64>,
    pub precipitation_mm: Option<f64>,
    pub pressure_hpa: Option<f64>,
    pub relative_humidity_percent: Option<f64>,
    pub temperature_c: Option<f64>,
    pub uv_index: Option<f64>,
    pub valid_at_ms: i64,
    pub wind_direction_degrees: Option<f64>,
    pub wind_gust_mps: Option<f64>,
    pub wind_speed_mps: Option<f64>,
}

/// Typed air-quality sample parsed from Open-Meteo arrays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpenMeteoAirQualitySample {
    pub pm25_micrograms_per_cubic_meter: Option<f64>,
    pub valid_at_ms: i64,
}

/// Canonical weather metrics emitted by provider normalization.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanonicalWeatherMetrics {
    pub apparent_temperature_c: Option<f64>,
    pub black_globe_temperature_c: Option<f64>,
    pub cloud_cover_percent: Option<f64>,
    pub pm25_micrograms_per_cubic_meter: Option<f64>,
    pub precipitation_mm: Option<f64>,
    pub precipitation_rate_mm_per_hour: Option<f64>,
    pub pressure_hpa: Option<f64>,
    pub relative_humidity_percent: Option<f64>,
    pub soil_electrical_conductivity_microsiemens_per_cm: Option<f64>,
    pub soil_moisture_percent: Option<f64>,
    pub solar_radiation_wm2: Option<f64>,
    pub temperature_c: Option<f64>,
    pub uv_index: Option<f64>,
    pub water_level_m: Option<f64>,
    pub wind_direction_degrees: Option<f64>,
    pub wind_gust_mps: Option<f64>,
    pub wind_speed_mps: Option<f64>,
    pub wet_bulb_globe_temperature_c: Option<f64>,
}

/// Open-Meteo provenance retained on one normalized record.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoProvenance {
    pub dataset: &'static str,
    pub elevation_m: Option<f64>,
    pub grid_cell: String,
    pub model: &'static str,
    pub upstream_timezone: String,
}

/// One canonical record normalized from typed Open-Meteo input.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedOpenMeteoRecord {
    pub metrics: CanonicalWeatherMetrics,
    pub product_run_at_ms: Option<i64>,
    pub provenance: OpenMeteoProvenance,
    pub received_at_ms: i64,
    pub source_id: String,
    pub source_kind: OpenMeteoSourceKind,
    pub valid_at_ms: i64,
}

/// Supported scheduled Open-Meteo source kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenMeteoSourceKind {
    Forecast,
    ModelCurrent,
}

/// Durable cursor emitted by Open-Meteo normalization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenMeteoCursor {
    ProductRunAt(i64),
    ValidAt(i64),
}

/// One normalized Open-Meteo provider batch.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedOpenMeteoBatch {
    pub adapter_version: &'static str,
    pub attempts: u32,
    pub provider_cursor: OpenMeteoCursor,
    pub records: Vec<NormalizedOpenMeteoRecord>,
}

/// One provider record with a durable validity timestamp.
pub trait ScheduledProviderRecord {
    /// Return the record's UTC validity timestamp.
    fn valid_at_ms(&self) -> i64;
}

impl ScheduledProviderRecord for ScheduledRecord {
    // expose the simple test record timestamp
    fn valid_at_ms(&self) -> i64 {
        self.valid_at_ms
    }
}

impl ScheduledProviderRecord for NormalizedOpenMeteoRecord {
    // expose the normalized provider timestamp
    fn valid_at_ms(&self) -> i64 {
        self.valid_at_ms
    }
}

/// Persistence required by one scheduled source run.
pub trait ScheduledRunRepository<Record, ResponseMetadata, Cursor = Option<String>> {
    type Session;

    /// Acquire one source-scoped session.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure>;

    /// Abandon runs whose deadline has elapsed.
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        now_ms: i64,
    ) -> Result<(), TaskFailure>;

    /// Read the durable scheduled checkpoint.
    fn scheduled_checkpoint(
        &mut self,
        session: &mut Self::Session,
    ) -> Result<Option<ScheduledCheckpoint<Cursor>>, TaskFailure>;

    /// Commit the running row before provider traffic.
    fn start_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        start: ScheduledRunStart,
    ) -> Result<String, TaskFailure>;

    /// Commit records and advance the checkpoint atomically.
    fn complete_scheduled_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: ScheduledCompletion<Record, ResponseMetadata, Cursor>,
    ) -> Result<(), TaskFailure>;

    /// Finalize one committed failed run.
    fn fail_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        run_id: &str,
        failure: &TaskFailure,
    ) -> Result<(), TaskFailure>;

    /// Release the source-scoped session.
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure>;
}

/// One bounded scheduled provider operation result.
pub type ScheduledProviderResult<Record, ResponseMetadata, Cursor> =
    Result<ProviderBatch<Record, ResponseMetadata, Cursor>, TaskFailure>;

/// Provider operation required by one scheduled source run.
pub trait ScheduledProvider {
    type Cursor;
    type Record: ScheduledProviderRecord;
    type Request;
    type ResponseMetadata;

    /// Fetch one provider batch for the committed run.
    fn fetch(
        &mut self,
        request: Self::Request,
    ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor>;
}

/// Persistence required at the end of a worker iteration.
pub trait HeartbeatRepository {
    /// Persist one worker heartbeat.
    fn update_worker_heartbeat(&mut self, heartbeat: WorkerHeartbeat) -> Result<(), TaskFailure>;
}

/// discovery required by one ordinary worker iteration
pub trait WorkerIterationRepository: HeartbeatRepository {
    /// discover ready sources in durable repository order
    fn discover_due_sources(&mut self, as_of_ms: i64) -> Result<Vec<WorkerDueSource>, TaskFailure>;
}

/// injected provider-family dispatch for one discovered source
pub trait WorkerScheduledSourceDispatcher<Repository> {
    /// run one source while retaining the shared captured success clock
    fn run_source<C: FnMut() -> i64>(
        &mut self,
        repository: &mut Repository,
        source: &WorkerDueSource,
        success_clock: &mut C,
    ) -> Result<SourceRunResult, TaskFailure>;
}

/// Admitted optional tasks executed after the ordinary scheduled-source drain.
pub trait WorkerSidecarTasks<Repository> {
    /// Collect one verified temperature-canary batch.
    fn collect_temperature_canary(
        &mut self,
        repository: &mut Repository,
    ) -> Result<TemperatureCanaryCollectionResult, TaskFailure>;

    /// Collect one prospective rain-evidence batch.
    fn collect_rain_capture(
        &mut self,
        repository: &mut Repository,
    ) -> Result<RainCaptureResult, TaskFailure>;

    /// Publish one fixed-gauge target after rain capture.
    fn publish_fixed_gauge_target(
        &mut self,
        repository: &mut Repository,
    ) -> Result<FixedGaugeTargetResult, TaskFailure>;

    /// Publish one rain-adjustment inference at the captured task clock.
    fn publish_rain_inference(
        &mut self,
        repository: &mut Repository,
        now_ms: i64,
    ) -> Result<bool, TaskFailure>;

    /// Capture one maintenance graph at the stable due clock.
    fn capture_adjustment_maintenance(
        &mut self,
        repository: &mut Repository,
        due: MaintenanceCaptureDue,
    ) -> Result<(), TaskFailure>;
}

/// The sidecar phase reuses the iteration repository and the exact discovery time.
pub trait WorkerSidecarPhase<Repository> {
    /// Recheck loaded admission for this loop and execute admitted work on the retained pool.
    fn run<Clock: FnMut() -> i64>(
        &mut self,
        repository: &mut Repository,
        loop_at_ms: i64,
        clock: &mut Clock,
    );
}

/// Diagnostic output required by the optional sidecar phase.
pub trait WorkerSidecarDiagnosticSink {
    /// Persist one bounded sidecar result without task error details.
    fn record_sidecar_diagnostic(&mut self, diagnostic: WorkerSidecarDiagnostic);
}

/// A rejected scheduling input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleError {
    ArithmeticOverflow,
    CadenceTooShort,
    MaximumRangeTooShort,
}

// format one scheduling error
impl Display for ScheduleError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // retain stable error text
        match self {
            Self::ArithmeticOverflow => formatter.write_str("scheduler arithmetic overflowed"),
            Self::CadenceTooShort => {
                formatter.write_str("source cadence must be at least one minute")
            }
            Self::MaximumRangeTooShort => {
                formatter.write_str("scheduled maximum range must cover one cadence")
            }
        }
    }
}

impl Error for ScheduleError {}

impl WorkerContinuation {
    /// Construct retained success state from the previous heartbeat.
    pub const fn new(last_success_at_ms: Option<i64>) -> Self {
        Self { last_success_at_ms }
    }

    /// Report the success time retained across iterations.
    pub const fn last_success_at_ms(&self) -> Option<i64> {
        self.last_success_at_ms
    }

    /// Persist one iteration heartbeat after updating retained success.
    pub fn finish_iteration<H: HeartbeatRepository>(
        &mut self,
        completed_at_ms: i64,
        sources: Vec<SourceRunResult>,
        heartbeats: &mut H,
    ) -> Result<WorkerIterationResult, TaskFailure> {
        let successful_at_ms = sources
            .iter()
            .filter(|result| result.status == SourceRunStatus::Succeeded)
            .filter_map(|result| result.successful_at_ms)
            .next_back();

        // retain the latest actual source completion
        if let Some(successful_at_ms) = successful_at_ms {
            self.last_success_at_ms = Some(successful_at_ms);
        }

        let activity = sources
            .iter()
            .any(|result| result.status == SourceRunStatus::Failed)
            .then_some(WorkerActivity::Degraded);
        heartbeats.update_worker_heartbeat(WorkerHeartbeat {
            activity,
            last_loop_at_ms: completed_at_ms,
            last_success_at_ms: self.last_success_at_ms,
        })?;

        Ok(WorkerIterationResult {
            completed_at_ms,
            last_success_at_ms: self.last_success_at_ms,
            sources,
        })
    }
}

/// Report whether the admitted rain runtime preserves the legacy inference task.
pub const fn rain_adjustment_inference_is_active(
    runtime: RainAdjustmentRuntimeSelection,
    legacy_enabled: bool,
) -> bool {
    // require the existing explicit enablement before considering runtime selection
    if !legacy_enabled {
        return false;
    }

    matches!(
        runtime,
        RainAdjustmentRuntimeSelection::Unconfigured
            | RainAdjustmentRuntimeSelection::Active
            | RainAdjustmentRuntimeSelection::DisabledPolicyRawWithComparatorAuthority
    )
}

// calculate the same bounded non-negative millisecond duration as the Node worker
fn sidecar_elapsed_milliseconds(started_at_ms: i64, completed_at_ms: i64) -> i64 {
    completed_at_ms.saturating_sub(started_at_ms).max(0)
}

// execute admitted optional tasks after scheduled sources and before heartbeat
fn run_worker_sidecars<R, T, S, C>(
    plan: WorkerSidecarPlan,
    tasks: &mut T,
    diagnostics: &mut S,
    repository: &mut R,
    loop_at_ms: i64,
    clock: &mut C,
) where
    C: FnMut() -> i64,
    S: WorkerSidecarDiagnosticSink,
    T: WorkerSidecarTasks<R>,
{
    // collect only after the temperature-canary runtime has independently admitted work
    if plan.temperature_canary == VerifiedTemperatureCanaryCollection::Active {
        let started_at_ms = clock();
        let (count, error_code) = match tasks.collect_temperature_canary(repository) {
            Ok(result) => (
                result.persisted_runs,
                (result.failed_runs != 0)
                    .then(|| "temperature_canary_collection_failed".to_owned()),
            ),
            Err(_) => (0, Some("temperature_canary_collection_failed".to_owned())),
        };
        let completed_at_ms = clock();
        diagnostics.record_sidecar_diagnostic(WorkerSidecarDiagnostic {
            count,
            duration_ms: sidecar_elapsed_milliseconds(started_at_ms, completed_at_ms),
            error_code,
            source_id: "ecmwf-temperature-canary",
        });
    }

    // keep prospective rain capture independent from inference policy
    if plan.rain_capture_configured {
        let started_at_ms = clock();
        let (count, error_code) = match tasks.collect_rain_capture(repository) {
            Ok(result) => (
                result.valid,
                (result.failed != 0).then(|| "rain_collection_failed".to_owned()),
            ),
            Err(_) => (0, Some("rain_collection_failed".to_owned())),
        };
        let completed_at_ms = clock();
        diagnostics.record_sidecar_diagnostic(WorkerSidecarDiagnostic {
            count,
            duration_ms: sidecar_elapsed_milliseconds(started_at_ms, completed_at_ms),
            error_code,
            source_id: "rain-prospective-capture",
        });
    }

    // require every Node dependency before materializing a fixed-gauge target
    if plan.rain_capture_configured
        && plan.rain_adjustment_revision_client_configured
        && plan.adjustment_capture_epoch_configured
    {
        let started_at_ms = clock();
        let (count, error_code) = match tasks.publish_fixed_gauge_target(repository) {
            Ok(FixedGaugeTargetResult::Idle) => (0, None),
            Ok(FixedGaugeTargetResult::Published { revision_count }) => (revision_count, None),
            Ok(FixedGaugeTargetResult::Gap { reason }) => (0, Some(reason)),
            Err(_) => (0, Some("rain_fixed_gauge_target_failed".to_owned())),
        };
        let completed_at_ms = clock();
        diagnostics.record_sidecar_diagnostic(WorkerSidecarDiagnostic {
            count,
            duration_ms: sidecar_elapsed_milliseconds(started_at_ms, completed_at_ms),
            error_code,
            source_id: "rain-fixed-gauge-target",
        });
    }

    // preserve active and root-selected-raw inference semantics from the Node runtime
    if rain_adjustment_inference_is_active(
        plan.rain_inference_runtime,
        plan.rain_inference_legacy_enabled,
    ) {
        let started_at_ms = clock();
        let task_now_ms = clock();
        let (count, error_code) = match tasks.publish_rain_inference(repository, task_now_ms) {
            Ok(published) => (usize::from(published), None),
            Err(_) => (0, Some("rain_adjustment_failed".to_owned())),
        };
        let completed_at_ms = clock();
        diagnostics.record_sidecar_diagnostic(WorkerSidecarDiagnostic {
            count,
            duration_ms: sidecar_elapsed_milliseconds(started_at_ms, completed_at_ms),
            error_code,
            source_id: "rain-adjustment",
        });
    }

    // run maintenance last against the iteration's stable discovery clock
    if plan.maintenance_capture_configured {
        let started_at_ms = clock();
        let capture_failed = match adjustment_maintenance_capture_due_at(loop_at_ms) {
            Ok(due) => tasks
                .capture_adjustment_maintenance(repository, due)
                .is_err(),
            Err(_) => true,
        };

        // emit only failed maintenance captures, matching the Node worker
        if capture_failed {
            let completed_at_ms = clock();
            diagnostics.record_sidecar_diagnostic(WorkerSidecarDiagnostic {
                count: 0,
                duration_ms: sidecar_elapsed_milliseconds(started_at_ms, completed_at_ms),
                error_code: Some("adjustment_maintenance_capture_failed".to_owned()),
                source_id: "adjustment-maintenance-capture",
            });
        }
    }
}

/// An ordinary process with no admitted sidecars performs no extra clock or pool work.
pub struct NoWorkerSidecars;

impl<R> WorkerSidecarPhase<R> for NoWorkerSidecars {
    /// Keep the inactive sidecar path completely idle.
    fn run<Clock: FnMut() -> i64>(
        &mut self,
        _repository: &mut R,
        _loop_at_ms: i64,
        _clock: &mut Clock,
    ) {
    }
}

impl<R, T: WorkerSidecarTasks<R>, S: WorkerSidecarDiagnosticSink> WorkerSidecarPhase<R>
    for WorkerSidecarExecution<'_, T, S>
{
    /// Run the existing ordered task and diagnostic policy on the retained repository.
    fn run<Clock: FnMut() -> i64>(
        &mut self,
        repository: &mut R,
        loop_at_ms: i64,
        clock: &mut Clock,
    ) {
        run_worker_sidecars(
            self.plan,
            self.tasks,
            self.diagnostics,
            repository,
            loop_at_ms,
            clock,
        );
    }
}

/// Drain ordered scheduled sources, execute a fixed admitted sidecar plan, and persist heartbeat.
pub fn run_worker_iteration_with_sidecars<R, D, T, S, C>(
    continuation: &mut WorkerContinuation,
    repository: &mut R,
    dispatcher: &mut D,
    mut sidecars: WorkerSidecarExecution<'_, T, S>,
    clock: &mut C,
) -> Result<WorkerIterationResult, TaskFailure>
where
    C: FnMut() -> i64,
    D: WorkerScheduledSourceDispatcher<R>,
    R: WorkerIterationRepository,
    S: WorkerSidecarDiagnosticSink,
    T: WorkerSidecarTasks<R>,
{
    run_worker_iteration_with_phase(continuation, repository, dispatcher, &mut sidecars, clock)
}

/// Run scheduled sources, the admitted process sidecar phase, and the same durable heartbeat.
pub fn run_worker_iteration_with_phase<R, D, P, C>(
    continuation: &mut WorkerContinuation,
    repository: &mut R,
    dispatcher: &mut D,
    sidecars: &mut P,
    clock: &mut C,
) -> Result<WorkerIterationResult, TaskFailure>
where
    C: FnMut() -> i64,
    D: WorkerScheduledSourceDispatcher<R>,
    R: WorkerIterationRepository,
    P: WorkerSidecarPhase<R>,
{
    // capture Node's iteration-duration origin before discovery time
    let _iteration_started_at_ms = clock();
    let loop_at_ms = clock();
    let due_sources = repository.discover_due_sources(loop_at_ms)?;
    let mut results = Vec::with_capacity(due_sources.len());

    // isolate each ready source without changing repository order
    for source in &due_sources {
        // preserve ordinary source failures while continuing the drain
        let outcome = match source {
            WorkerDueSource::Unconfigured(id) => {
                // sample the source start and skip instants without opening a provider session
                let _started_at_ms = clock();
                let _skipped_at_ms = clock();
                Ok(skipped_source_result(
                    id,
                    "source is not an active configured scheduled source",
                ))
            }
            WorkerDueSource::InvalidConfiguration { failure, .. } => {
                // retain the source start clock before the per-source failure boundary
                let _started_at_ms = clock();
                Err(failure.clone())
            }
            _ => dispatcher.run_source(repository, source, clock),
        };
        match outcome {
            Ok(result) => results.push(result),
            Err(failure) => {
                // capture the same failure clock used by Node diagnostics
                let _failed_at_ms = clock();
                results.push(SourceRunResult {
                    attempts: failure.attempts,
                    reason: Some("worker_source_failure".to_owned()),
                    record_count: 0,
                    run_id: None,
                    secondary_error_codes: vec![failure.code],
                    source_id: source.source_id().to_owned(),
                    status: SourceRunStatus::Failed,
                    successful_at_ms: None,
                });
            }
        }
    }

    sidecars.run(repository, loop_at_ms, clock);
    let completed_at_ms = clock();
    continuation.finish_iteration(completed_at_ms, results, repository)
}

/// Drain one ordinary iteration with every optional sidecar absent.
pub fn run_worker_iteration<R, D, C>(
    continuation: &mut WorkerContinuation,
    repository: &mut R,
    dispatcher: &mut D,
    clock: &mut C,
) -> Result<WorkerIterationResult, TaskFailure>
where
    C: FnMut() -> i64,
    D: WorkerScheduledSourceDispatcher<R>,
    R: WorkerIterationRepository,
{
    run_worker_iteration_with_phase(
        continuation,
        repository,
        dispatcher,
        &mut NoWorkerSidecars,
        clock,
    )
}

/// Attempt one sidecar-aware iteration while holding the in-process non-overlap gate.
pub fn run_worker_iteration_once_with_sidecars<R, D, T, S, C>(
    gate: &NonOverlappingGate,
    continuation: &mut WorkerContinuation,
    repository: &mut R,
    dispatcher: &mut D,
    sidecars: WorkerSidecarExecution<'_, T, S>,
    clock: &mut C,
) -> Result<WorkerIterationRun, TaskFailure>
where
    C: FnMut() -> i64,
    D: WorkerScheduledSourceDispatcher<R>,
    R: WorkerIterationRepository,
    S: WorkerSidecarDiagnosticSink,
    T: WorkerSidecarTasks<R>,
{
    // skip overlap without executing sources, sidecars, clocks, or heartbeat
    let Some(_permit) = gate.try_enter() else {
        return Ok(WorkerIterationRun::Busy);
    };
    let completed =
        run_worker_iteration_with_sidecars(continuation, repository, dispatcher, sidecars, clock)?;

    Ok(WorkerIterationRun::Completed(completed))
}

/// attempt one iteration while holding the in-process non-overlap gate
pub fn run_worker_iteration_once<R, D, C>(
    gate: &NonOverlappingGate,
    continuation: &mut WorkerContinuation,
    repository: &mut R,
    dispatcher: &mut D,
    clock: &mut C,
) -> Result<WorkerIterationRun, TaskFailure>
where
    C: FnMut() -> i64,
    D: WorkerScheduledSourceDispatcher<R>,
    R: WorkerIterationRepository,
{
    // skip overlap without opening a repository or consuming a phase clock
    let Some(_permit) = gate.try_enter() else {
        return Ok(WorkerIterationRun::Busy);
    };
    run_worker_iteration(continuation, repository, dispatcher, clock)
        .map(WorkerIterationRun::Completed)
}

impl NonOverlappingGate {
    /// Construct one idle guard.
    pub const fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
        }
    }

    /// Report whether one run currently owns the guard.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// Acquire the guard only when no run is active.
    pub fn try_enter(&self) -> Option<RunPermit<'_>> {
        self.running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| RunPermit { gate: self })
    }
}

// release one active run
impl Drop for RunPermit<'_> {
    fn drop(&mut self) {
        self.gate.running.store(false, Ordering::Release);
    }
}

/// Calculate the worker's anchored half-open ingestion interval.
pub fn scheduled_window(
    now_ms: i64,
    cadence_seconds: u64,
    checkpoint_end_ms: Option<i64>,
    maximum_range_seconds: Option<u64>,
) -> Result<ScheduledWindow, ScheduleError> {
    let cadence_ms = seconds_to_milliseconds(cadence_seconds)?;

    // enforce the source cadence floor
    if cadence_ms < MINIMUM_CADENCE_MILLISECONDS {
        return Err(ScheduleError::CadenceTooShort);
    }

    let maximum_range_ms = maximum_range_seconds
        .map(seconds_to_milliseconds)
        .transpose()?;

    // require one complete cadence
    if maximum_range_ms.is_some_and(|maximum| maximum < cadence_ms) {
        return Err(ScheduleError::MaximumRangeTooShort);
    }

    let end_exclusive_ms = now_ms.div_euclid(cadence_ms) * cadence_ms;
    let previous_slot_ms = end_exclusive_ms
        .checked_sub(cadence_ms)
        .ok_or(ScheduleError::ArithmeticOverflow)?;
    let checkpoint_start_ms = checkpoint_end_ms.map_or(previous_slot_ms, |checkpoint| {
        checkpoint.min(previous_slot_ms)
    });
    let maximum_start_ms = maximum_range_ms
        .map(|maximum| {
            end_exclusive_ms
                .checked_sub(maximum)
                .ok_or(ScheduleError::ArithmeticOverflow)
        })
        .transpose()?
        .unwrap_or(checkpoint_start_ms);

    Ok(ScheduledWindow {
        end_exclusive_ms,
        start_ms: checkpoint_start_ms.max(maximum_start_ms),
    })
}

/// Derive the latest fixed six-hour maintenance capture clock.
pub fn adjustment_maintenance_capture_due_at(
    now_ms: i64,
) -> Result<MaintenanceCaptureDue, ScheduleError> {
    let cycle_ms =
        now_ms.div_euclid(MAINTENANCE_CYCLE_MILLISECONDS) * MAINTENANCE_CYCLE_MILLISECONDS;
    let current_due_ms = cycle_ms
        .checked_add(MAINTENANCE_DUE_OFFSET_MILLISECONDS)
        .ok_or(ScheduleError::ArithmeticOverflow)?;

    // select the previous due clock before minute thirty-five
    let issued_at_ms = if current_due_ms > now_ms {
        current_due_ms
            .checked_sub(MAINTENANCE_CYCLE_MILLISECONDS)
            .ok_or(ScheduleError::ArithmeticOverflow)?
    } else {
        current_due_ms
    };

    Ok(MaintenanceCaptureDue { issued_at_ms })
}

/// Choose the newest ECMWF cycle beyond the conservative six-hour delay.
pub fn latest_available_ecmwf_initialization(now_ms: i64) -> Result<i64, ScheduleError> {
    let eligible_ms = now_ms
        .checked_sub(ECMWF_AVAILABILITY_DELAY_MILLISECONDS)
        .ok_or(ScheduleError::ArithmeticOverflow)?;

    Ok(eligible_ms.div_euclid(MAINTENANCE_CYCLE_MILLISECONDS) * MAINTENANCE_CYCLE_MILLISECONDS)
}

/// Derive worker health from one durable heartbeat snapshot.
pub fn worker_health(
    now_ms: i64,
    last_loop_at_ms: Option<i64>,
    last_success_at_ms: Option<i64>,
    database_ready: bool,
    stale_after_ms: i64,
) -> WorkerHealth {
    let heartbeat_age_ms = last_loop_at_ms.and_then(|last_loop| now_ms.checked_sub(last_loop));
    let stale = heartbeat_age_ms.is_none_or(|age| age < 0 || age > stale_after_ms);

    WorkerHealth {
        last_loop_at_ms,
        last_success_at_ms,
        live: true,
        ready: database_ready && !stale,
        stale,
    }
}

/// Normalize one typed Open-Meteo current sample.
pub fn normalize_open_meteo_current(
    envelope: OpenMeteoEnvelope,
    sample: OpenMeteoWeatherSample,
    source_id: &str,
    received_at_ms: i64,
    attempts: u32,
) -> Result<NormalizedOpenMeteoBatch, TaskFailure> {
    validate_open_meteo_identity(&envelope, source_id)?;
    let provenance = open_meteo_provenance(&envelope);
    let record = NormalizedOpenMeteoRecord {
        metrics: normalize_open_meteo_metrics(sample, None, false)?,
        product_run_at_ms: None,
        provenance,
        received_at_ms,
        source_id: source_id.to_owned(),
        source_kind: OpenMeteoSourceKind::ModelCurrent,
        valid_at_ms: sample.valid_at_ms,
    };

    Ok(NormalizedOpenMeteoBatch {
        adapter_version: OPEN_METEO_CURRENT_ADAPTER_VERSION,
        attempts,
        provider_cursor: OpenMeteoCursor::ValidAt(sample.valid_at_ms),
        records: vec![record],
    })
}

/// Normalize ordered typed Open-Meteo weather and air-quality forecasts.
pub fn normalize_open_meteo_forecast(
    envelope: OpenMeteoEnvelope,
    weather: &[OpenMeteoWeatherSample],
    air_quality: &[OpenMeteoAirQualitySample],
    source_id: &str,
    received_at_ms: i64,
    attempts: u32,
) -> Result<NormalizedOpenMeteoBatch, TaskFailure> {
    validate_open_meteo_identity(&envelope, source_id)?;

    // require both successful provider products
    if weather.is_empty() || air_quality.is_empty() {
        return Err(invalid_provider_payload());
    }

    let mut air_quality_by_valid_at = BTreeMap::new();
    let mut previous_air_quality_at = None;

    // index ordered air-quality values
    for sample in air_quality {
        // reject unordered or duplicate instants
        if previous_air_quality_at.is_some_and(|previous| sample.valid_at_ms <= previous) {
            return Err(invalid_provider_payload());
        }
        let value = normalize_metric(sample.pm25_micrograms_per_cubic_meter, 0.0, 999.0, false)?;
        air_quality_by_valid_at.insert(sample.valid_at_ms, value);
        previous_air_quality_at = Some(sample.valid_at_ms);
    }

    let provenance = open_meteo_provenance(&envelope);
    let mut records = Vec::with_capacity(weather.len());
    let mut previous_weather_at = None;

    // retain every ordered weather hour
    for sample in weather {
        // reject unordered or duplicate instants
        if previous_weather_at.is_some_and(|previous| sample.valid_at_ms <= previous) {
            return Err(invalid_provider_payload());
        }
        records.push(NormalizedOpenMeteoRecord {
            metrics: normalize_open_meteo_metrics(
                *sample,
                air_quality_by_valid_at
                    .get(&sample.valid_at_ms)
                    .copied()
                    .flatten(),
                true,
            )?,
            product_run_at_ms: Some(received_at_ms),
            provenance: provenance.clone(),
            received_at_ms,
            source_id: source_id.to_owned(),
            source_kind: OpenMeteoSourceKind::Forecast,
            valid_at_ms: sample.valid_at_ms,
        });
        previous_weather_at = Some(sample.valid_at_ms);
    }

    Ok(NormalizedOpenMeteoBatch {
        adapter_version: OPEN_METEO_FORECAST_ADAPTER_VERSION,
        attempts,
        provider_cursor: OpenMeteoCursor::ProductRunAt(received_at_ms),
        records,
    })
}

// validate provider identity fields
fn validate_open_meteo_identity(
    envelope: &OpenMeteoEnvelope,
    source_id: &str,
) -> Result<(), TaskFailure> {
    // require bounded coordinates
    if !envelope.latitude.is_finite()
        || !envelope.longitude.is_finite()
        || !(-90.0..=90.0).contains(&envelope.latitude)
        || !(-180.0..=180.0).contains(&envelope.longitude)
    {
        return Err(invalid_provider_payload());
    }

    // require finite optional elevation
    if envelope.elevation_m.is_some_and(|value| !value.is_finite()) {
        return Err(invalid_provider_payload());
    }

    // require parsed timezone and source identity
    if envelope.upstream_timezone.trim().is_empty()
        || source_id.trim().is_empty()
        || source_id.len() > 128
    {
        return Err(invalid_provider_payload());
    }

    Ok(())
}

// build stable provider provenance
fn open_meteo_provenance(envelope: &OpenMeteoEnvelope) -> OpenMeteoProvenance {
    OpenMeteoProvenance {
        dataset: "forecast",
        elevation_m: envelope.elevation_m,
        grid_cell: format!("{},{}", envelope.latitude, envelope.longitude),
        model: "best_match",
        upstream_timezone: envelope.upstream_timezone.clone(),
    }
}

// normalize the complete canonical metric shape
fn normalize_open_meteo_metrics(
    sample: OpenMeteoWeatherSample,
    pm25_micrograms_per_cubic_meter: Option<f64>,
    forecast: bool,
) -> Result<CanonicalWeatherMetrics, TaskFailure> {
    let precipitation_mm = normalize_metric(sample.precipitation_mm, 0.0, 2_000.0, false)?;
    // collapse the provider's circular north duplicate
    let wind_direction_degrees = match sample.wind_direction_degrees {
        Some(360.0) => Some(0.0),
        value => value,
    };
    // include forecast-only ultraviolet measurements
    let uv_index = if forecast {
        normalize_metric(sample.uv_index, 0.0, 20.0, false)?
    } else {
        None
    };

    Ok(CanonicalWeatherMetrics {
        apparent_temperature_c: normalize_metric(
            sample.apparent_temperature_c,
            -100.0,
            70.0,
            false,
        )?,
        black_globe_temperature_c: None,
        cloud_cover_percent: normalize_metric(sample.cloud_cover_percent, 0.0, 100.0, false)?,
        pm25_micrograms_per_cubic_meter: normalize_metric(
            pm25_micrograms_per_cubic_meter,
            0.0,
            999.0,
            false,
        )?,
        precipitation_mm,
        precipitation_rate_mm_per_hour: forecast.then_some(precipitation_mm).flatten(),
        pressure_hpa: normalize_metric(sample.pressure_hpa, 100.0, 1_200.0, false)?,
        relative_humidity_percent: normalize_metric(
            sample.relative_humidity_percent,
            0.0,
            100.0,
            false,
        )?,
        soil_electrical_conductivity_microsiemens_per_cm: None,
        soil_moisture_percent: None,
        solar_radiation_wm2: None,
        temperature_c: normalize_metric(sample.temperature_c, -100.0, 70.0, false)?,
        uv_index,
        water_level_m: None,
        wind_direction_degrees: normalize_metric(wind_direction_degrees, 0.0, 360.0, true)?,
        wind_gust_mps: normalize_metric(sample.wind_gust_mps, 0.0, 150.0, false)?,
        wind_speed_mps: normalize_metric(sample.wind_speed_mps, 0.0, 150.0, false)?,
        wet_bulb_globe_temperature_c: None,
    })
}

// validate one canonical metric
fn normalize_metric(
    value: Option<f64>,
    minimum: f64,
    maximum: f64,
    maximum_exclusive: bool,
) -> Result<Option<f64>, TaskFailure> {
    let Some(value) = value else {
        return Ok(None);
    };

    // require finite in-range values
    if !value.is_finite()
        || value < minimum
        || value > maximum
        || (maximum_exclusive && value == maximum)
    {
        return Err(invalid_provider_payload());
    }

    Ok(Some(value))
}

// create one provider payload failure
fn invalid_provider_payload() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "invalid_payload".to_owned(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScheduledWindowHistory {
    Checkpoint,
    OneCadence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScheduledLastValidPolicy {
    FinalRecord,
    WindowEnd,
}

#[derive(Clone, Copy, Debug)]
struct PreparedScheduledRun<'a> {
    adapter_version: &'static str,
    cadence_seconds: u64,
    endpoint: &'static str,
    last_valid_policy: ScheduledLastValidPolicy,
    maximum_range_seconds: Option<u64>,
    precomputed_window: Option<ScheduledWindow>,
    source_config_fingerprint: &'a str,
    source_id: &'a str,
    window_history: ScheduledWindowHistory,
}

#[derive(Clone, Copy)]
struct ScheduledTiming {
    now_ms: i64,
    run_deadline_at_ms: i64,
    window_now_ms: i64,
    provider_window: Option<ScheduledWindow>,
}

impl ScheduledTiming {
    /// Keep ordinary source windows on the same clock as their run start.
    const fn one_clock(now_ms: i64, run_deadline_at_ms: i64) -> Self {
        Self {
            now_ms,
            run_deadline_at_ms,
            window_now_ms: now_ms,
            provider_window: None,
        }
    }
}

struct OpenMeteoSchedule<'a> {
    configuration: &'a OpenMeteoRuntimeConfiguration,
    fixed_timing: Option<ScheduledTiming>,
    maximum_range_seconds: Option<u64>,
    source: &'a OpenMeteoScheduledSource,
}

/// Execute one admitted committed Open-Meteo scheduled source run with fixed fixture time.
pub fn run_scheduled_source<R, P, C>(
    execution: ScheduledRunExecution<'_>,
    repository: &mut R,
    provider: &mut P,
    success_clock: &mut C,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<Cursor = Option<OpenMeteoCursor>, Request = ProviderRequest>,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
{
    let fixed_timing = ScheduledTiming::one_clock(execution.now_ms, execution.run_deadline_at_ms);
    run_open_meteo_schedule(
        OpenMeteoSchedule {
            configuration: execution.configuration,
            fixed_timing: Some(fixed_timing),
            maximum_range_seconds: execution.maximum_range_seconds,
            source: execution.source,
        },
        repository,
        provider,
        success_clock,
        // retain the fixed deadline given by the caller
        |_, _| Ok(fixed_timing),
    )
}

/// Execute the same run, sampling its start and arming provider deadlines only once locked.
pub fn run_scheduled_source_after_lock<R, P, C, T>(
    source: &OpenMeteoScheduledSource,
    configuration: &OpenMeteoRuntimeConfiguration,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
    timing: T,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<Cursor = Option<OpenMeteoCursor>, Request = ProviderRequest>,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
    T: FnOnce(&mut P, i64) -> Result<i64, TaskFailure>,
{
    run_open_meteo_schedule(
        OpenMeteoSchedule {
            configuration,
            fixed_timing: None,
            maximum_range_seconds: None,
            source,
        },
        repository,
        provider,
        clock,
        // sample the run clock only after the source lock succeeds
        |provider, clock| read_locked_schedule_timing(provider, clock, timing),
    )
}

/// Plan a one-clock provider and durable budget only after a source session is retained.
fn read_locked_schedule_timing<P, C, T>(
    provider: &mut P,
    clock: &mut C,
    timing: T,
) -> Result<ScheduledTiming, TaskFailure>
where
    C: FnMut() -> i64,
    T: FnOnce(&mut P, i64) -> Result<i64, TaskFailure>,
{
    let now_ms = clock();
    Ok(ScheduledTiming::one_clock(
        now_ms,
        timing(provider, now_ms)?,
    ))
}

// join both Open-Meteo timing modes to one admission and commit path
fn run_open_meteo_schedule<R, P, C, T>(
    schedule: OpenMeteoSchedule<'_>,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
    timing: T,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<Cursor = Option<OpenMeteoCursor>, Request = ProviderRequest>,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
    T: FnOnce(&mut P, &mut C) -> Result<ScheduledTiming, TaskFailure>,
{
    let OpenMeteoSchedule {
        configuration,
        fixed_timing,
        maximum_range_seconds,
        source,
    } = schedule;

    // skip identities outside the loaded runtime contract
    if !open_meteo_source_matches_configuration(source, configuration) {
        // take the separate completion sample for an ordinary process skip
        if fixed_timing.is_none() {
            let _source_completed_at_ms = clock();
        }
        return Ok(skipped_source_result(
            &source.id,
            "source is not an active configured scheduled source",
        ));
    }
    let adapter_version = open_meteo_adapter_version(source.source_kind);

    // require the exact adapter before taking a source lock
    if source.adapter_version != adapter_version || configuration.adapter_version != adapter_version
    {
        return Err(failure_with_code(
            "source adapter contract must match runtime",
        ));
    }

    // preserve pre-lock validation when fixed timing was provided
    if fixed_timing.is_some_and(|timing| timing.run_deadline_at_ms < timing.now_ms) {
        return Err(failure_with_code("run deadline must not precede start"));
    }

    let endpoint = open_meteo_endpoint(source.source_kind);

    // select Open-Meteo checkpoint semantics
    let last_valid_policy = match source.source_kind {
        OpenMeteoSourceKind::Forecast => ScheduledLastValidPolicy::WindowEnd,
        OpenMeteoSourceKind::ModelCurrent => ScheduledLastValidPolicy::FinalRecord,
    };
    // build the Open-Meteo request entirely from checked configuration
    run_prepared_scheduled_source_with_timing(
        PreparedScheduledRun {
            adapter_version,
            cadence_seconds: source.cadence_seconds,
            endpoint,
            last_valid_policy,
            maximum_range_seconds,
            precomputed_window: None,
            source_config_fingerprint: &source.source_config_fingerprint,
            source_id: &source.id,
            window_history: ScheduledWindowHistory::Checkpoint,
        },
        repository,
        provider,
        clock,
        timing,
        fixed_timing.is_none(),
        // preserve the checkpoint cursor for the request
        |checkpoint, window| ProviderRequest {
            endpoint,
            previous_cursor: checkpoint.and_then(|value| value.provider_cursor),
            source_id: source.id.clone(),
            source_kind: source.source_kind,
            timezone: source.timezone.clone(),
            window,
        },
    )
}

/// Execute one admitted committed Tempest scheduled source run with fixed time.
pub fn run_scheduled_tempest_source<R, P, C>(
    execution: TempestScheduledExecution<'_>,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = Option<TempestProviderCursor>,
            Request = TempestObservationRequest,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
{
    let fixed = ScheduledTiming::one_clock(execution.now_ms, execution.run_deadline_at_ms);
    run_tempest_schedule(
        execution.source,
        execution.configuration,
        repository,
        provider,
        clock,
        Some(fixed),
        // preserve the pre-admitted source timing
        |_, _| Ok(fixed),
    )
}

/// Execute a scheduled Tempest source with deadlines derived after acquiring its lock.
pub fn run_scheduled_tempest_source_after_lock<R, P, C, T>(
    source: &TempestScheduledSource,
    configuration: &TempestRuntimeConfiguration,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
    timing: T,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = Option<TempestProviderCursor>,
            Request = TempestObservationRequest,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
    T: FnOnce(&mut P, i64) -> Result<i64, TaskFailure>,
{
    run_tempest_schedule(
        source,
        configuration,
        repository,
        provider,
        clock,
        None,
        // sample the start through the retained source session
        |provider, clock| read_locked_schedule_timing(provider, clock, timing),
    )
}

/// Reuse one Tempest scheduling recipe for fixed tests and live worker clocks.
fn run_tempest_schedule<R, P, C, T>(
    source: &TempestScheduledSource,
    configuration: &TempestRuntimeConfiguration,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
    fixed_timing: Option<ScheduledTiming>,
    timing: T,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = Option<TempestProviderCursor>,
            Request = TempestObservationRequest,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
    T: FnOnce(&mut P, &mut C) -> Result<ScheduledTiming, TaskFailure>,
{
    // skip identities outside the loaded Tempest contract
    if !tempest_source_matches_configuration(source, configuration) {
        // skipped live-source matches still have a completion time
        if fixed_timing.is_none() {
            let _source_completed_at_ms = clock();
        }
        return Ok(skipped_source_result(
            &source.id,
            "source is not an active configured scheduled source",
        ));
    }

    // require the exact configured source contract
    if configuration.adapter_contract_version != TEMPEST_SOURCE_CONTRACT_VERSION {
        return Err(failure_with_code(
            "source adapter contract must match runtime",
        ));
    }

    // keep one scheduled range within the provider contract
    if source.cadence_seconds > TEMPEST_MAXIMUM_RANGE_SECONDS {
        return Err(failure_with_code("tempest cadence exceeds provider range"));
    }

    // keep existing fixed-time validation before acquiring the source lock
    if fixed_timing.is_some_and(|fixed| fixed.run_deadline_at_ms < fixed.now_ms) {
        return Err(failure_with_code("run deadline must not precede start"));
    }
    run_prepared_scheduled_source_with_timing(
        PreparedScheduledRun {
            adapter_version: TEMPEST_OBSERVATION_ADAPTER_VERSION,
            cadence_seconds: source.cadence_seconds,
            endpoint: TEMPEST_OBSERVATION_ENDPOINT,
            last_valid_policy: ScheduledLastValidPolicy::FinalRecord,
            maximum_range_seconds: None,
            precomputed_window: None,
            source_config_fingerprint: &source.source_config_fingerprint,
            source_id: &source.id,
            window_history: ScheduledWindowHistory::OneCadence,
        },
        repository,
        provider,
        clock,
        timing,
        fixed_timing.is_none(),
        |_checkpoint, window| TempestObservationRequest {
            device_id: configuration.device_id,
            end_exclusive_ms: window.end_exclusive_ms,
            location_id: configuration.location_id,
            serial: configuration.serial.clone(),
            source_id: source.id.clone(),
            start_ms: window.start_ms,
            timezone: configuration.timezone.clone(),
        },
    )
}

/// Execute one admitted committed Ecowitt scheduled source run with fixed time.
pub fn run_scheduled_ecowitt_source<R, P, C>(
    execution: EcowittScheduledExecution<'_>,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = EcowittProviderCursor,
            Record = NormalizedEcowittRecord,
            Request = EcowittScheduledRequest,
            ResponseMetadata = EcowittResponseMetadata,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
{
    let fixed = ScheduledTiming::one_clock(execution.now_ms, execution.run_deadline_at_ms);
    run_ecowitt_schedule(
        execution.source,
        execution.configuration,
        repository,
        provider,
        clock,
        Some(fixed),
        // preserve the pre-admitted source timing
        |_, _| Ok(fixed),
    )
}

/// Execute a scheduled Ecowitt source with deadlines derived after acquiring its lock.
pub fn run_scheduled_ecowitt_source_after_lock<R, P, C, T>(
    source: &EcowittScheduledSource,
    configuration: &EcowittRuntimeConfiguration,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
    timing: T,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = EcowittProviderCursor,
            Record = NormalizedEcowittRecord,
            Request = EcowittScheduledRequest,
            ResponseMetadata = EcowittResponseMetadata,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
    T: FnOnce(&mut P, i64) -> Result<i64, TaskFailure>,
{
    run_ecowitt_schedule(
        source,
        configuration,
        repository,
        provider,
        clock,
        None,
        // sample the start through the retained source session
        |provider, clock| read_locked_schedule_timing(provider, clock, timing),
    )
}

/// Reuse one Ecowitt scheduling recipe for fixed tests and live worker clocks.
fn run_ecowitt_schedule<R, P, C, T>(
    source: &EcowittScheduledSource,
    configuration: &EcowittRuntimeConfiguration,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
    fixed_timing: Option<ScheduledTiming>,
    timing: T,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = EcowittProviderCursor,
            Record = NormalizedEcowittRecord,
            Request = EcowittScheduledRequest,
            ResponseMetadata = EcowittResponseMetadata,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
    T: FnOnce(&mut P, &mut C) -> Result<ScheduledTiming, TaskFailure>,
{
    // skip identities outside the loaded Ecowitt contract
    if !ecowitt_source_matches_configuration(source, configuration) {
        // skipped live-source matches still have a completion time
        if fixed_timing.is_none() {
            let _source_completed_at_ms = clock();
        }
        return Ok(skipped_source_result(
            &source.id,
            "source is not an active configured scheduled source",
        ));
    }

    // require the exact configured source contract before locking
    if configuration.adapter_contract_version != ECOWITT_LOCAL_LIVE_ADAPTER_VERSION {
        return Err(failure_with_code(
            "source adapter contract must match runtime",
        ));
    }

    // keep the checked local-live cadence within its configuration boundary
    if !(ECOWITT_MINIMUM_CADENCE_SECONDS..=ECOWITT_MAXIMUM_CADENCE_SECONDS)
        .contains(&source.cadence_seconds)
    {
        return Err(failure_with_code("ecowitt cadence outside provider range"));
    }
    let window = fixed_timing
        .map(|timing| scheduled_window(timing.now_ms, source.cadence_seconds, None, None))
        .transpose()
        .map_err(schedule_task_failure)?;

    // keep existing fixed-time validation before acquiring the source lock
    if fixed_timing.is_some_and(|fixed| fixed.run_deadline_at_ms < fixed.now_ms) {
        return Err(failure_with_code("run deadline must not precede start"));
    }
    run_prepared_scheduled_source_with_timing(
        PreparedScheduledRun {
            adapter_version: ECOWITT_LOCAL_LIVE_ADAPTER_VERSION,
            cadence_seconds: source.cadence_seconds,
            endpoint: ECOWITT_LOCAL_LIVE_ENDPOINT,
            last_valid_policy: ScheduledLastValidPolicy::FinalRecord,
            maximum_range_seconds: None,
            precomputed_window: window,
            source_config_fingerprint: &source.source_config_fingerprint,
            source_id: &source.id,
            window_history: ScheduledWindowHistory::OneCadence,
        },
        repository,
        provider,
        clock,
        timing,
        fixed_timing.is_none(),
        |checkpoint, _window| EcowittScheduledRequest {
            expected_mac: configuration.expected_mac.clone(),
            gateway_host: configuration.gateway_host.clone(),
            model: configuration.model.clone(),
            previous_cursor: checkpoint.map(|value| value.provider_cursor.clone()),
            source_id: source.id.clone(),
            timezone: configuration.timezone.clone(),
        },
    )
}

/// Execute one admitted committed Ambient Weather scheduled source run with fixed time.
pub fn run_scheduled_ambient_weather_source<R, P, C>(
    execution: AmbientWeatherScheduledExecution<'_>,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = Option<AmbientWeatherProviderCursor>,
            Record = NormalizedAmbientWeatherRecord,
            Request = AmbientWeatherRangeRequest,
            ResponseMetadata = AmbientWeatherResponseMetadata,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
{
    let fixed = ScheduledTiming::one_clock(execution.now_ms, execution.run_deadline_at_ms);
    run_ambient_weather_schedule(
        execution.source,
        execution.configuration,
        repository,
        provider,
        clock,
        Some(fixed),
        // preserve the pre-admitted timing in existing callers
        |_, _| Ok(fixed),
    )
}

/// Execute one scheduled Ambient Weather source with deadlines sampled after locking.
pub fn run_scheduled_ambient_weather_source_after_lock<R, P, C, T>(
    source: &AmbientWeatherScheduledSource,
    configuration: &AmbientWeatherRuntimeConfiguration,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
    timing: T,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = Option<AmbientWeatherProviderCursor>,
            Record = NormalizedAmbientWeatherRecord,
            Request = AmbientWeatherRangeRequest,
            ResponseMetadata = AmbientWeatherResponseMetadata,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
    T: FnOnce(&mut P, i64) -> Result<i64, TaskFailure>,
{
    run_ambient_weather_schedule(
        source,
        configuration,
        repository,
        provider,
        clock,
        None,
        // sample the start only within the retained source session
        |provider, clock| read_locked_schedule_timing(provider, clock, timing),
    )
}

/// Reuse one Ambient Weather recipe for fixed and process-owned timing.
fn run_ambient_weather_schedule<R, P, C, T>(
    source: &AmbientWeatherScheduledSource,
    configuration: &AmbientWeatherRuntimeConfiguration,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
    fixed_timing: Option<ScheduledTiming>,
    timing: T,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = Option<AmbientWeatherProviderCursor>,
            Record = NormalizedAmbientWeatherRecord,
            Request = AmbientWeatherRangeRequest,
            ResponseMetadata = AmbientWeatherResponseMetadata,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
    T: FnOnce(&mut P, &mut C) -> Result<ScheduledTiming, TaskFailure>,
{
    // skip identities outside the loaded Ambient contract
    if !public_stations::ambient_weather_source_matches_configuration(source, configuration) {
        // sample normal completion only for the process-owned live clock
        if fixed_timing.is_none() {
            let _source_completed_at_ms = clock();
        }
        return Ok(skipped_source_result(
            &source.id,
            "source is not an active configured scheduled source",
        ));
    }

    // require the exact configured adapter before locking
    if configuration.adapter_contract_version != AMBIENT_WEATHER_ADAPTER_VERSION {
        return Err(failure_with_code(
            "source adapter contract must match runtime",
        ));
    }

    // keep the checked current cadence within its catalog boundary
    if !(AMBIENT_WEATHER_MINIMUM_CADENCE_SECONDS..=AMBIENT_WEATHER_MAXIMUM_CADENCE_SECONDS)
        .contains(&source.cadence_seconds)
    {
        return Err(failure_with_code(
            "ambient weather cadence outside provider range",
        ));
    }
    let maximum_range_seconds = u64::from(configuration.maximum_chunk_days)
        .checked_mul(86_400)
        .ok_or_else(|| failure_with_code("ambient weather range overflow"))?;

    // keep fixed-time validation outside the session
    if fixed_timing.is_some_and(|fixed| fixed.run_deadline_at_ms < fixed.now_ms) {
        return Err(failure_with_code("run deadline must not precede start"));
    }

    // build the Ambient request entirely from checked configuration
    run_prepared_scheduled_source_with_timing(
        PreparedScheduledRun {
            adapter_version: AMBIENT_WEATHER_ADAPTER_VERSION,
            cadence_seconds: source.cadence_seconds,
            endpoint: AMBIENT_WEATHER_DEVICE_DATA_ENDPOINT,
            last_valid_policy: ScheduledLastValidPolicy::FinalRecord,
            maximum_range_seconds: Some(maximum_range_seconds),
            precomputed_window: None,
            source_config_fingerprint: &source.source_config_fingerprint,
            source_id: &source.id,
            window_history: ScheduledWindowHistory::Checkpoint,
        },
        repository,
        provider,
        clock,
        timing,
        fixed_timing.is_none(),
        |_checkpoint, window| AmbientWeatherRangeRequest {
            device_id: configuration.device_id.clone(),
            end_exclusive_ms: window.end_exclusive_ms,
            mac_address: configuration.mac_address.clone(),
            model: configuration.model.clone(),
            serial: configuration.serial.clone(),
            source_id: source.id.clone(),
            start_ms: window.start_ms,
            timezone: configuration.timezone.clone(),
        },
    )
}

/// Execute one admitted committed NOAA tide scheduled source run with fixed time.
pub fn run_scheduled_noaa_tide_source<R, P, C>(
    execution: NoaaTideScheduledExecution<'_>,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = Option<NoaaTideProviderCursor>,
            Record = NormalizedNoaaTideRecord,
            Request = NoaaTideRangeRequest,
            ResponseMetadata = NoaaTideResponseMetadata,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
{
    let fixed = ScheduledTiming::one_clock(execution.now_ms, execution.run_deadline_at_ms);
    run_noaa_tide_schedule(
        execution.source,
        execution.configuration,
        repository,
        provider,
        clock,
        Some(fixed),
        // preserve the pre-admitted timing in existing callers
        |_, _| Ok(fixed),
    )
}

/// Execute one scheduled NOAA tide source with deadlines sampled after locking.
pub fn run_scheduled_noaa_tide_source_after_lock<R, P, C, T>(
    source: &NoaaTideScheduledSource,
    configuration: &NoaaTideRuntimeConfiguration,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
    timing: T,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = Option<NoaaTideProviderCursor>,
            Record = NormalizedNoaaTideRecord,
            Request = NoaaTideRangeRequest,
            ResponseMetadata = NoaaTideResponseMetadata,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
    T: FnOnce(&mut P, i64) -> Result<i64, TaskFailure>,
{
    run_noaa_tide_schedule(
        source,
        configuration,
        repository,
        provider,
        clock,
        None,
        // sample the start only within the retained source session
        |provider, clock| read_locked_schedule_timing(provider, clock, timing),
    )
}

/// Reuse one NOAA tide recipe for fixed and process-owned timing.
fn run_noaa_tide_schedule<R, P, C, T>(
    source: &NoaaTideScheduledSource,
    configuration: &NoaaTideRuntimeConfiguration,
    repository: &mut R,
    provider: &mut P,
    clock: &mut C,
    fixed_timing: Option<ScheduledTiming>,
    timing: T,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    P: ScheduledProvider<
            Cursor = Option<NoaaTideProviderCursor>,
            Record = NormalizedNoaaTideRecord,
            Request = NoaaTideRangeRequest,
            ResponseMetadata = NoaaTideResponseMetadata,
        >,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
    T: FnOnce(&mut P, &mut C) -> Result<ScheduledTiming, TaskFailure>,
{
    // skip identities outside the loaded NOAA tide contract
    if !noaa_tide_source_matches_configuration(source, configuration) {
        // sample normal completion only for the process-owned live clock
        if fixed_timing.is_none() {
            let _source_completed_at_ms = clock();
        }
        return Ok(skipped_source_result(
            &source.id,
            "source is not an active configured scheduled source",
        ));
    }
    let adapter_version = noaa_tide_adapter_version(source.source_kind);

    // require the exact configured adapter before locking
    if configuration.adapter_contract_version != adapter_version {
        return Err(failure_with_code(
            "source adapter contract must match runtime",
        ));
    }

    // keep the checked tide cadence within its configuration boundary
    if !(NOAA_SCHEDULED_MINIMUM_CADENCE_SECONDS..=NOAA_SCHEDULED_MAXIMUM_CADENCE_SECONDS)
        .contains(&source.cadence_seconds)
    {
        return Err(failure_with_code("noaa cadence outside provider range"));
    }

    // keep observation catch-up and the prediction checkpoint on their own policies
    let (maximum_range_seconds, last_valid_policy) = match source.source_kind {
        NoaaTideSourceKind::Observation => (
            Some(MAXIMUM_NOAA_OBSERVATION_RANGE_SECONDS),
            ScheduledLastValidPolicy::FinalRecord,
        ),
        NoaaTideSourceKind::Prediction => (None, ScheduledLastValidPolicy::WindowEnd),
    };
    let fixed_timing = fixed_timing
        .map(|fixed| noaa_schedule_timing(fixed, source.source_kind))
        .transpose()?;
    // retain fixed-time validation before the source lock
    if fixed_timing.is_some_and(|fixed| fixed.run_deadline_at_ms < fixed.now_ms) {
        return Err(failure_with_code("run deadline must not precede start"));
    }

    run_prepared_scheduled_source_with_timing(
        PreparedScheduledRun {
            adapter_version,
            cadence_seconds: source.cadence_seconds,
            endpoint: NOAA_TIDE_DATA_ENDPOINT,
            last_valid_policy,
            maximum_range_seconds,
            precomputed_window: None,
            source_config_fingerprint: &source.source_config_fingerprint,
            source_id: &source.id,
            window_history: ScheduledWindowHistory::Checkpoint,
        },
        repository,
        provider,
        clock,
        // keep the provider horizon and durable observation lag on the locked time
        |provider, clock| match fixed_timing {
            Some(fixed) => Ok(fixed),
            None => noaa_schedule_timing(timing(provider, clock)?, source.source_kind),
        },
        fixed_timing.is_none(),
        // use the admitted provider range independently of the durable checkpoint window
        |_checkpoint, provider_window| NoaaTideRangeRequest {
            end_exclusive_ms: provider_window.end_exclusive_ms,
            source_id: source.id.clone(),
            source_kind: source.source_kind,
            start_ms: provider_window.start_ms,
            station_id: configuration.station_id.clone(),
            timezone: configuration.timezone.clone(),
        },
    )
}

/// Derive NOAA's observation lag or prediction horizon from the single start clock.
fn noaa_schedule_timing(
    mut timing: ScheduledTiming,
    kind: NoaaTideSourceKind,
) -> Result<ScheduledTiming, TaskFailure> {
    match kind {
        NoaaTideSourceKind::Observation => {
            timing.window_now_ms = timing
                .now_ms
                .checked_sub(NOAA_SCHEDULED_OBSERVATION_LAG_MILLISECONDS)
                .ok_or_else(|| failure_with_code("noaa observation clock underflow"))?;
        }
        NoaaTideSourceKind::Prediction => {
            timing.provider_window = Some(noaa_prediction_request_window(timing.now_ms)?);
        }
    }
    Ok(timing)
}

// finalize every provider through the same retained session and timing policy
fn run_prepared_scheduled_source_with_timing<R, P, C, F, T>(
    prepared: PreparedScheduledRun<'_>,
    repository: &mut R,
    provider: &mut P,
    success_clock: &mut C,
    timing: T,
    completion_after_release: bool,
    request: F,
) -> Result<SourceRunResult, TaskFailure>
where
    C: FnMut() -> i64,
    F: FnOnce(Option<&ScheduledCheckpoint<P::Cursor>>, ScheduledWindow) -> P::Request,
    P: ScheduledProvider,
    R: ScheduledRunRepository<P::Record, P::ResponseMetadata, P::Cursor>,
    T: FnOnce(&mut P, &mut C) -> Result<ScheduledTiming, TaskFailure>,
{
    // acquire only after provider-specific identity validation
    let Some(mut session) = repository.acquire_source_session(prepared.source_id)? else {
        // retain the completion sample without treating the skipped lock as a successful run
        if completion_after_release {
            let _source_completed_at_ms = success_clock();
        }
        return Ok(skipped_source_result(
            prepared.source_id,
            "source lock is held",
        ));
    };
    let mut run_id = None;
    let mut attempts = 0;
    let mut record_count = 0;
    let mut successful_at_ms = None;

    // execute provider work inside the held session
    let execution = (|| {
        let timing = timing(provider, success_clock)?;
        // never commit a run whose durable expiry is already past
        if timing.run_deadline_at_ms < timing.now_ms {
            return Err(failure_with_code("run deadline must not precede start"));
        }
        repository.abandon_expired_runs(&mut session, timing.now_ms)?;
        let checkpoint = repository.scheduled_checkpoint(&mut session)?;

        // select whether durable history controls this range
        let checkpoint_end_ms = match prepared.window_history {
            ScheduledWindowHistory::Checkpoint => checkpoint
                .as_ref()
                .map(|value| value.window_end_exclusive_ms),
            ScheduledWindowHistory::OneCadence => None,
        };
        // reuse an already admitted provider range when one was required before locking
        let window = prepared.precomputed_window.map_or_else(
            || {
                scheduled_window(
                    timing.window_now_ms,
                    prepared.cadence_seconds,
                    checkpoint_end_ms,
                    prepared.maximum_range_seconds,
                )
                .map_err(schedule_task_failure)
            },
            Ok,
        )?;
        let started_id = repository.start_ingestion_run(
            &mut session,
            ScheduledRunStart {
                adapter_version: prepared.adapter_version,
                deadline_at_ms: timing.run_deadline_at_ms,
                endpoint: prepared.endpoint,
                requested_window: window,
                source_config_fingerprint: prepared.source_config_fingerprint.to_owned(),
            },
        )?;
        run_id = Some(started_id.clone());
        // a provider horizon may differ from the durable checkpoint window
        let provider_window = timing.provider_window.unwrap_or(window);
        let batch = provider.fetch(request(checkpoint.as_ref(), provider_window))?;
        attempts = batch.attempts;
        record_count = batch.records.len();
        let final_record_valid_at_ms = batch
            .records
            .last()
            .map(ScheduledProviderRecord::valid_at_ms)
            .ok_or_else(|| TaskFailure {
                attempts: attempts.max(1),
                code: "empty_payload".to_owned(),
            })?;

        // select provider-specific durable checkpoint semantics
        let last_valid_at_ms = match prepared.last_valid_policy {
            ScheduledLastValidPolicy::FinalRecord => final_record_valid_at_ms,
            ScheduledLastValidPolicy::WindowEnd => window.end_exclusive_ms,
        };
        repository.complete_scheduled_ingestion(
            &mut session,
            ScheduledCompletion {
                attempts,
                expected_checkpoint_version: checkpoint.as_ref().map(|value| value.version),
                last_valid_at_ms,
                provider_cursor: batch.provider_cursor,
                record_count,
                records: batch.records,
                response_metadata: batch.response_metadata,
                run_id: started_id,
                upstream_response_checksum: batch.upstream_response_checksum,
                window,
            },
        )?;

        // keep the preexisting fixed-time behavior for legacy provider adapters
        if !completion_after_release {
            successful_at_ms = Some(success_clock());
        }

        Ok::<(), TaskFailure>(())
    })();

    // build the bounded primary result
    let mut result = match execution {
        Ok(()) => SourceRunResult {
            attempts,
            reason: None,
            record_count,
            run_id: run_id.clone(),
            secondary_error_codes: Vec::new(),
            source_id: prepared.source_id.to_owned(),
            status: SourceRunStatus::Succeeded,
            successful_at_ms,
        },
        Err(failure) => {
            let primary_failure = TaskFailure {
                attempts: attempts.max(failure.attempts).max(1),
                code: failure.code,
            };
            attempts = primary_failure.attempts;
            let mut secondary_error_codes = Vec::new();

            // finalize only committed running rows
            if let Some(started_id) = run_id.as_deref() {
                // retain the primary failure
                if let Err(finalization) =
                    repository.fail_ingestion_run(&mut session, started_id, &primary_failure)
                {
                    secondary_error_codes.push(format!("finalization:{}", finalization.code));
                }
            }

            SourceRunResult {
                attempts,
                reason: Some(primary_failure.code),
                record_count,
                run_id: run_id.clone(),
                secondary_error_codes,
                source_id: prepared.source_id.to_owned(),
                status: SourceRunStatus::Failed,
                successful_at_ms: None,
            }
        }
    };

    // retain the primary outcome across cleanup failure
    if let Err(release) = repository.release_source_session(session) {
        // replace only a successful primary result
        if result.status == SourceRunStatus::Succeeded {
            result.reason = Some("session_release_failed".to_owned());
        }
        result
            .secondary_error_codes
            .push(format!("release:{}", release.code));
        result.status = SourceRunStatus::Failed;
        result.successful_at_ms = None;
    }

    // report elapsed source work before updating the iteration's success clock
    if completion_after_release {
        let _source_completed_at_ms = success_clock();
        // a release failure cannot advance the durable success time
        if result.status == SourceRunStatus::Succeeded {
            result.successful_at_ms = Some(success_clock());
        }
    }

    Ok(result)
}

// compare one durable identity to runtime configuration
fn open_meteo_source_matches_configuration(
    source: &OpenMeteoScheduledSource,
    configuration: &OpenMeteoRuntimeConfiguration,
) -> bool {
    source.active
        && configuration.active
        && source.provider_key == "open-meteo"
        && configuration.provider_key == "open-meteo"
        && !source.id.trim().is_empty()
        && source.id.len() <= 128
        && !source.site_slug.trim().is_empty()
        && !source.station_slug.trim().is_empty()
        && !source.source_key.trim().is_empty()
        && !source.source_config_fingerprint.trim().is_empty()
        && !source.material_config_identity.trim().is_empty()
        && !source.timezone.trim().is_empty()
        && source.provider_key == configuration.provider_key
        && source.site_slug == configuration.site_slug
        && source.station_slug == configuration.station_slug
        && source.source_key == configuration.source_key
        && source.source_kind == configuration.source_kind
        && open_meteo_configuration_supports_kind(configuration)
        && source.source_config_fingerprint == configuration.source_config_fingerprint
        && source.timezone == configuration.timezone
        && source.material_config_identity == configuration.material_config_identity
}

// compare one durable Tempest identity to runtime configuration
fn tempest_source_matches_configuration(
    source: &TempestScheduledSource,
    configuration: &TempestRuntimeConfiguration,
) -> bool {
    source.active
        && !source.id.trim().is_empty()
        && source.id.len() <= 128
        && !source.provider_key.trim().is_empty()
        && !source.site_slug.trim().is_empty()
        && !source.station_slug.trim().is_empty()
        && !source.source_key.trim().is_empty()
        && !source.source_config_fingerprint.trim().is_empty()
        && !source.material_config_identity.trim().is_empty()
        && !source.timezone.trim().is_empty()
        && source.provider_key == configuration.provider_key
        && source.site_slug == configuration.site_slug
        && source.station_slug == configuration.station_slug
        && source.source_key == configuration.source_key
        && source.source_kind == TempestSourceKind::PhysicalSensor
        && source.source_kind == configuration.source_kind
        && source.source_config_fingerprint == configuration.source_config_fingerprint
        && source.timezone == configuration.timezone
        && source.material_config_identity == configuration.material_config_identity
}

// compare one durable Ecowitt identity to runtime configuration
fn ecowitt_source_matches_configuration(
    source: &EcowittScheduledSource,
    configuration: &EcowittRuntimeConfiguration,
) -> bool {
    source.active
        && !source.id.trim().is_empty()
        && source.id.len() <= 128
        && !source.provider_key.trim().is_empty()
        && !source.site_slug.trim().is_empty()
        && !source.station_slug.trim().is_empty()
        && !source.source_key.trim().is_empty()
        && !source.source_config_fingerprint.trim().is_empty()
        && !source.material_config_identity.trim().is_empty()
        && !source.timezone.trim().is_empty()
        && !configuration.expected_mac.trim().is_empty()
        && configuration.expected_mac.len() <= 17
        && !configuration.gateway_host.trim().is_empty()
        && configuration.gateway_host.len() <= 64
        && !configuration.model.trim().is_empty()
        && configuration.model.len() <= 128
        && configuration
            .capabilities
            .contains(&EcowittCapability::Current)
        && source.provider_key == configuration.provider_key
        && source.site_slug == configuration.site_slug
        && source.station_slug == configuration.station_slug
        && source.source_key == configuration.source_key
        && source.source_kind == EcowittSourceKind::PhysicalSensor
        && source.source_kind == configuration.source_kind
        && source.source_config_fingerprint == configuration.source_config_fingerprint
        && source.timezone == configuration.timezone
        && source.material_config_identity == configuration.material_config_identity
}

// compare one durable NOAA identity to runtime configuration
fn noaa_tide_source_matches_configuration(
    source: &NoaaTideScheduledSource,
    configuration: &NoaaTideRuntimeConfiguration,
) -> bool {
    source.active
        && configuration.station_active
        && configuration.source_active
        && !source.id.trim().is_empty()
        && source.id.len() <= 128
        && !source.provider_key.trim().is_empty()
        && !source.site_slug.trim().is_empty()
        && !source.station_slug.trim().is_empty()
        && !source.source_key.trim().is_empty()
        && !source.source_config_fingerprint.trim().is_empty()
        && !source.material_config_identity.trim().is_empty()
        && !source.timezone.trim().is_empty()
        && noaa_station_id_is_valid(&configuration.station_id)
        && noaa_tide_configuration_supports_kind(configuration)
        && source.provider_key == configuration.provider_key
        && source.site_slug == configuration.site_slug
        && source.station_slug == configuration.station_slug
        && source.source_key == configuration.source_key
        && source.source_kind == configuration.source_kind
        && source.source_config_fingerprint == configuration.source_config_fingerprint
        && source.timezone == configuration.timezone
        && source.material_config_identity == configuration.material_config_identity
}

// require the fixed capabilities and range controls for one NOAA source kind
fn noaa_tide_configuration_supports_kind(configuration: &NoaaTideRuntimeConfiguration) -> bool {
    // match the checked catalog variants
    match configuration.source_kind {
        NoaaTideSourceKind::Observation => {
            (1..=31).contains(&configuration.maximum_chunk_days)
                && configuration.forecast_days.is_none()
                && configuration
                    .capabilities
                    .contains(&NoaaTideCapability::Current)
                && configuration
                    .capabilities
                    .contains(&NoaaTideCapability::Historical)
        }
        NoaaTideSourceKind::Prediction => {
            (1..=366).contains(&configuration.maximum_chunk_days)
                && configuration.forecast_days == Some(NOAA_SCHEDULED_PREDICTION_DAYS)
                && configuration
                    .capabilities
                    .contains(&NoaaTideCapability::Forecast)
                && configuration
                    .capabilities
                    .contains(&NoaaTideCapability::Historical)
        }
    }
}

// require one documented NOAA station identity
fn noaa_station_id_is_valid(value: &str) -> bool {
    value.len() == 7 && value.bytes().all(|byte| byte.is_ascii_digit())
}

// build the fixed thirty-day prediction request range
fn noaa_prediction_request_window(now_ms: i64) -> Result<ScheduledWindow, TaskFailure> {
    let start_ms = now_ms.div_euclid(60_000) * 60_000;
    let duration_ms = NOAA_SCHEDULED_PREDICTION_DAYS
        .checked_mul(MILLISECONDS_PER_DAY)
        .ok_or_else(|| failure_with_code("noaa prediction range overflow"))?;
    let end_exclusive_ms = start_ms
        .checked_add(duration_ms)
        .ok_or_else(|| failure_with_code("noaa prediction range overflow"))?;

    Ok(ScheduledWindow {
        end_exclusive_ms,
        start_ms,
    })
}

// require the parsed capability selected by source kind
fn open_meteo_configuration_supports_kind(configuration: &OpenMeteoRuntimeConfiguration) -> bool {
    // map each scheduled kind to its configured capability
    match configuration.source_kind {
        OpenMeteoSourceKind::Forecast => configuration
            .capabilities
            .contains(&OpenMeteoCapability::Forecast),
        OpenMeteoSourceKind::ModelCurrent => configuration
            .capabilities
            .contains(&OpenMeteoCapability::Current),
    }
}

// select the frozen adapter contract
const fn open_meteo_adapter_version(source_kind: OpenMeteoSourceKind) -> &'static str {
    // map one exact source kind
    match source_kind {
        OpenMeteoSourceKind::Forecast => OPEN_METEO_FORECAST_ADAPTER_VERSION,
        OpenMeteoSourceKind::ModelCurrent => OPEN_METEO_CURRENT_ADAPTER_VERSION,
    }
}

// select the provider endpoint identity
const fn open_meteo_endpoint(source_kind: OpenMeteoSourceKind) -> &'static str {
    // map one exact source kind
    match source_kind {
        OpenMeteoSourceKind::Forecast => "forecast/hourly",
        OpenMeteoSourceKind::ModelCurrent => "forecast/current",
    }
}

// build one pre-session skipped result
fn skipped_source_result(source_id: &str, reason: &str) -> SourceRunResult {
    SourceRunResult {
        attempts: 0,
        reason: Some(reason.to_owned()),
        record_count: 0,
        run_id: None,
        secondary_error_codes: Vec::new(),
        source_id: source_id.to_owned(),
        status: SourceRunStatus::Skipped,
        successful_at_ms: None,
    }
}

// build one zero-attempt failure
fn failure_with_code(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

// translate schedule validation
fn schedule_task_failure(error: ScheduleError) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: error.to_string(),
    }
}

// convert bounded seconds
fn seconds_to_milliseconds(seconds: u64) -> Result<i64, ScheduleError> {
    i64::try_from(seconds)
        .ok()
        .and_then(|value| value.checked_mul(MILLISECONDS_PER_SECOND))
        .ok_or(ScheduleError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::{
        CanonicalWeatherMetrics, FixedGaugeTargetResult, HeartbeatRepository,
        MaintenanceCaptureDue, NonOverlappingGate, NormalizedEcowittRecord,
        NormalizedOpenMeteoRecord, NormalizedTempestRecord, OPEN_METEO_CURRENT_ADAPTER_VERSION,
        OPEN_METEO_FORECAST_ADAPTER_VERSION, OpenMeteoAirQualitySample, OpenMeteoCapability,
        OpenMeteoCursor, OpenMeteoEnvelope, OpenMeteoRuntimeConfiguration,
        OpenMeteoScheduledSource, OpenMeteoSourceKind, OpenMeteoWeatherSample, ProviderBatch,
        ProviderRequest, RainAdjustmentRuntimeSelection, RainCaptureResult, ScheduleError,
        ScheduledCheckpoint, ScheduledCompletion, ScheduledProvider, ScheduledProviderResult,
        ScheduledRecord, ScheduledRunExecution, ScheduledRunRepository, ScheduledRunStart,
        ScheduledWindow, SourceRunResult, SourceRunStatus, TEMPEST_OBSERVATION_ADAPTER_VERSION,
        TEMPEST_OBSERVATION_ENDPOINT, TEMPEST_SOURCE_CONTRACT_VERSION, TaskFailure,
        TemperatureCanaryCollectionResult, TempestObservation, TempestObservationEnvelope,
        TempestObservationRequest, TempestProviderCursor, TempestResponseMetadata,
        TempestResponseType, TempestRuntimeConfiguration, TempestScheduledExecution,
        TempestScheduledSource, TempestSourceKind, VerifiedTemperatureCanaryCollection,
        WorkerActivity, WorkerContinuation, WorkerDueSource, WorkerHeartbeat,
        WorkerIterationRepository, WorkerIterationRun, WorkerScheduledSourceDispatcher,
        WorkerSidecarDiagnostic, WorkerSidecarDiagnosticSink, WorkerSidecarExecution,
        WorkerSidecarPlan, WorkerSidecarTasks, adjustment_maintenance_capture_due_at,
        latest_available_ecmwf_initialization, normalize_open_meteo_current,
        normalize_open_meteo_forecast, normalize_tempest_observations,
        rain_adjustment_inference_is_active, run_scheduled_source, run_scheduled_tempest_source,
        run_worker_iteration, run_worker_iteration_once, run_worker_iteration_with_sidecars,
        scheduled_window, worker_health,
    };
    use crate::{
        AMBIENT_WEATHER_ADAPTER_VERSION, AMBIENT_WEATHER_DEVICE_DATA_ENDPOINT,
        AmbientWeatherCapability, AmbientWeatherDeviceIdentity, AmbientWeatherObservation,
        AmbientWeatherPayload, AmbientWeatherProviderCursor, AmbientWeatherRangeRequest,
        AmbientWeatherResponseMetadata, AmbientWeatherRuntimeConfiguration,
        AmbientWeatherScheduledExecution, AmbientWeatherScheduledSource, AmbientWeatherSourceKind,
        NormalizedAmbientWeatherRecord, normalize_ambient_weather_range,
        run_scheduled_ambient_weather_source,
    };
    use crate::{
        ECOWITT_LOCAL_LIVE_ADAPTER_VERSION, EcowittAdmittedLocalDate,
        EcowittCanonicalMetadataAdapter, EcowittCanonicalMetadataMeasurement, EcowittCapability,
        EcowittLivePayload, EcowittLiveRequest, EcowittMeasurement, EcowittPreviousCursor,
        EcowittProviderCursor, EcowittProviderMetadata, EcowittRainReadings,
        EcowittResponseMetadata, EcowittRuntimeConfiguration, EcowittScheduledExecution,
        EcowittScheduledRequest, EcowittScheduledSource, EcowittSourceKind, EcowittTimeAdapter,
        EcowittUnit, normalize_ecowitt_live, run_scheduled_ecowitt_source,
    };
    use crate::{
        NOAA_SCHEDULED_PREDICTION_DAYS, NOAA_TIDE_OBSERVATION_ADAPTER_VERSION,
        NOAA_TIDE_PREDICTION_ADAPTER_VERSION, NoaaPredictionType, NoaaTideCapability,
        NoaaTideObservation, NoaaTidePayload, NoaaTidePrediction, NoaaTideProviderCursor,
        NoaaTideRangeRequest, NoaaTideResponseMetadata, NoaaTideRuntimeConfiguration,
        NoaaTideScheduledExecution, NoaaTideScheduledSource, NoaaTideSourceKind,
        NormalizedNoaaTideRecord, normalize_noaa_tide_range, run_scheduled_noaa_tide_source,
    };
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    use std::rc::Rc;

    /// one typed OpenMeteo batch used by scheduled lifecycle fixtures
    type OpenMeteoScheduledRecordBatch =
        ProviderBatch<ScheduledRecord, BTreeMap<String, String>, Option<OpenMeteoCursor>>;

    // build one bounded test failure
    fn failure(code: &str, attempts: u32) -> TaskFailure {
        TaskFailure {
            attempts,
            code: code.to_owned(),
        }
    }

    #[derive(Debug)]
    struct FakeRepository {
        checkpoint: Option<ScheduledCheckpoint<Option<OpenMeteoCursor>>>,
        complete_failure: Option<TaskFailure>,
        completion: Option<
            ScheduledCompletion<ScheduledRecord, BTreeMap<String, String>, Option<OpenMeteoCursor>>,
        >,
        events: Rc<RefCell<Vec<String>>>,
        fail_finalization: bool,
        fail_release: bool,
        failure: Option<TaskFailure>,
        lock_available: bool,
        start: Option<ScheduledRunStart>,
    }

    // create one repository double
    fn fake_repository(events: Rc<RefCell<Vec<String>>>) -> FakeRepository {
        FakeRepository {
            checkpoint: None,
            complete_failure: None,
            completion: None,
            events,
            fail_finalization: false,
            fail_release: false,
            failure: None,
            lock_available: true,
            start: None,
        }
    }

    impl ScheduledRunRepository<ScheduledRecord, BTreeMap<String, String>, Option<OpenMeteoCursor>>
        for FakeRepository
    {
        type Session = ();

        // acquire one fake session
        fn acquire_source_session(
            &mut self,
            _source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            self.events.borrow_mut().push("lock".to_owned());
            Ok(self.lock_available.then_some(()))
        }

        // record stale-run cleanup
        fn abandon_expired_runs(
            &mut self,
            _session: &mut Self::Session,
            _now_ms: i64,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("abandon".to_owned());
            Ok(())
        }

        // return one fake checkpoint
        fn scheduled_checkpoint(
            &mut self,
            _session: &mut Self::Session,
        ) -> Result<Option<ScheduledCheckpoint<Option<OpenMeteoCursor>>>, TaskFailure> {
            self.events.borrow_mut().push("checkpoint".to_owned());
            Ok(self.checkpoint.clone())
        }

        // commit one fake running row
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            start: ScheduledRunStart,
        ) -> Result<String, TaskFailure> {
            self.events.borrow_mut().push("start-committed".to_owned());
            self.start = Some(start);
            Ok("run-1".to_owned())
        }

        // retain one fake completion
        fn complete_scheduled_ingestion(
            &mut self,
            _session: &mut Self::Session,
            completion: ScheduledCompletion<
                ScheduledRecord,
                BTreeMap<String, String>,
                Option<OpenMeteoCursor>,
            >,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("complete".to_owned());

            // inject a post-fetch atomic completion failure
            if let Some(failure) = &self.complete_failure {
                return Err(failure.clone());
            }
            self.completion = Some(completion);
            Ok(())
        }

        // record one fake failed run
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _run_id: &str,
            primary_failure: &TaskFailure,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("fail".to_owned());
            self.failure = Some(primary_failure.clone());

            // inject finalization failure
            if self.fail_finalization {
                return Err(failure("finalization_failed", 0));
            }
            Ok(())
        }

        // release one fake session
        fn release_source_session(&mut self, _session: Self::Session) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("release".to_owned());

            // inject cleanup failure
            if self.fail_release {
                return Err(failure("release_failed", 0));
            }
            Ok(())
        }
    }

    #[derive(Debug)]
    struct FakeProvider {
        events: Rc<RefCell<Vec<String>>>,
        outcome: Result<OpenMeteoScheduledRecordBatch, TaskFailure>,
        request: Option<ProviderRequest>,
    }

    impl ScheduledProvider for FakeProvider {
        type Cursor = Option<OpenMeteoCursor>;
        type Record = ScheduledRecord;
        type Request = ProviderRequest;
        type ResponseMetadata = BTreeMap<String, String>;

        // fetch one fake batch
        fn fetch(
            &mut self,
            request: ProviderRequest,
        ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
            self.events.borrow_mut().push("fetch".to_owned());
            self.request = Some(request);
            self.outcome.clone()
        }
    }

    #[derive(Debug, Default)]
    struct OpenMeteoRecordRepository {
        completion: Option<
            ScheduledCompletion<
                NormalizedOpenMeteoRecord,
                BTreeMap<String, String>,
                Option<OpenMeteoCursor>,
            >,
        >,
    }

    impl
        ScheduledRunRepository<
            NormalizedOpenMeteoRecord,
            BTreeMap<String, String>,
            Option<OpenMeteoCursor>,
        > for OpenMeteoRecordRepository
    {
        type Session = ();

        // acquire one typed-record session
        fn acquire_source_session(
            &mut self,
            _source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            Ok(Some(()))
        }

        // accept typed-record cleanup
        fn abandon_expired_runs(
            &mut self,
            _session: &mut Self::Session,
            _now_ms: i64,
        ) -> Result<(), TaskFailure> {
            Ok(())
        }

        // start without a prior checkpoint
        fn scheduled_checkpoint(
            &mut self,
            _session: &mut Self::Session,
        ) -> Result<Option<ScheduledCheckpoint<Option<OpenMeteoCursor>>>, TaskFailure> {
            Ok(None)
        }

        // commit one typed-record running row
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _start: ScheduledRunStart,
        ) -> Result<String, TaskFailure> {
            Ok("run-open-meteo".to_owned())
        }

        // retain actual normalized records atomically
        fn complete_scheduled_ingestion(
            &mut self,
            _session: &mut Self::Session,
            completion: ScheduledCompletion<
                NormalizedOpenMeteoRecord,
                BTreeMap<String, String>,
                Option<OpenMeteoCursor>,
            >,
        ) -> Result<(), TaskFailure> {
            self.completion = Some(completion);
            Ok(())
        }

        // accept typed-record failure finalization
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _run_id: &str,
            _failure: &TaskFailure,
        ) -> Result<(), TaskFailure> {
            Ok(())
        }

        // release one typed-record session
        fn release_source_session(&mut self, _session: Self::Session) -> Result<(), TaskFailure> {
            Ok(())
        }
    }

    #[derive(Clone, Debug)]
    struct OpenMeteoRecordProvider {
        batch: ProviderBatch<
            NormalizedOpenMeteoRecord,
            BTreeMap<String, String>,
            Option<OpenMeteoCursor>,
        >,
    }

    impl ScheduledProvider for OpenMeteoRecordProvider {
        type Cursor = Option<OpenMeteoCursor>;
        type Record = NormalizedOpenMeteoRecord;
        type Request = ProviderRequest;
        type ResponseMetadata = BTreeMap<String, String>;

        // return one normalized provider batch
        fn fetch(
            &mut self,
            _request: ProviderRequest,
        ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
            Ok(self.batch.clone())
        }
    }

    #[derive(Debug)]
    struct TempestRecordRepository {
        checkpoint: Option<ScheduledCheckpoint<Option<TempestProviderCursor>>>,
        completion: Option<
            ScheduledCompletion<
                NormalizedTempestRecord,
                TempestResponseMetadata,
                Option<TempestProviderCursor>,
            >,
        >,
        events: Rc<RefCell<Vec<String>>>,
        start: Option<ScheduledRunStart>,
    }

    impl
        ScheduledRunRepository<
            NormalizedTempestRecord,
            TempestResponseMetadata,
            Option<TempestProviderCursor>,
        > for TempestRecordRepository
    {
        type Session = ();

        // acquire one Tempest session
        fn acquire_source_session(
            &mut self,
            _source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            self.events.borrow_mut().push("lock".to_owned());
            Ok(Some(()))
        }

        // record Tempest stale-run cleanup
        fn abandon_expired_runs(
            &mut self,
            _session: &mut Self::Session,
            _now_ms: i64,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("abandon".to_owned());
            Ok(())
        }

        // return one Tempest checkpoint
        fn scheduled_checkpoint(
            &mut self,
            _session: &mut Self::Session,
        ) -> Result<Option<ScheduledCheckpoint<Option<TempestProviderCursor>>>, TaskFailure>
        {
            self.events.borrow_mut().push("checkpoint".to_owned());
            Ok(self.checkpoint.clone())
        }

        // commit one Tempest running row
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            start: ScheduledRunStart,
        ) -> Result<String, TaskFailure> {
            self.events.borrow_mut().push("start-committed".to_owned());
            self.start = Some(start);
            Ok("run-tempest".to_owned())
        }

        // retain actual Tempest records atomically
        fn complete_scheduled_ingestion(
            &mut self,
            _session: &mut Self::Session,
            completion: ScheduledCompletion<
                NormalizedTempestRecord,
                TempestResponseMetadata,
                Option<TempestProviderCursor>,
            >,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("complete".to_owned());
            self.completion = Some(completion);
            Ok(())
        }

        // record one Tempest failed run
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _run_id: &str,
            _failure: &TaskFailure,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("fail".to_owned());
            Ok(())
        }

        // release one Tempest session
        fn release_source_session(&mut self, _session: Self::Session) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("release".to_owned());
            Ok(())
        }
    }

    #[derive(Clone, Debug)]
    struct TempestRecordProvider {
        batch: ProviderBatch<
            NormalizedTempestRecord,
            TempestResponseMetadata,
            Option<TempestProviderCursor>,
        >,
        events: Rc<RefCell<Vec<String>>>,
        request: Option<TempestObservationRequest>,
    }

    impl ScheduledProvider for TempestRecordProvider {
        type Cursor = Option<TempestProviderCursor>;
        type Record = NormalizedTempestRecord;
        type Request = TempestObservationRequest;
        type ResponseMetadata = TempestResponseMetadata;

        // return one normalized Tempest provider batch
        fn fetch(
            &mut self,
            request: Self::Request,
        ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
            self.events.borrow_mut().push("fetch".to_owned());
            self.request = Some(request);
            Ok(self.batch.clone())
        }
    }

    #[derive(Debug)]
    struct EcowittRecordRepository {
        checkpoint: Option<ScheduledCheckpoint<EcowittProviderCursor>>,
        completion: Option<
            ScheduledCompletion<
                NormalizedEcowittRecord,
                EcowittResponseMetadata,
                EcowittProviderCursor,
            >,
        >,
        events: Rc<RefCell<Vec<String>>>,
        start: Option<ScheduledRunStart>,
    }

    impl
        ScheduledRunRepository<
            NormalizedEcowittRecord,
            EcowittResponseMetadata,
            EcowittProviderCursor,
        > for EcowittRecordRepository
    {
        type Session = ();

        // acquire one Ecowitt session
        fn acquire_source_session(
            &mut self,
            _source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            self.events.borrow_mut().push("lock".to_owned());
            Ok(Some(()))
        }

        // record Ecowitt stale-run cleanup
        fn abandon_expired_runs(
            &mut self,
            _session: &mut Self::Session,
            _now_ms: i64,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("abandon".to_owned());
            Ok(())
        }

        // return one typed Ecowitt checkpoint
        fn scheduled_checkpoint(
            &mut self,
            _session: &mut Self::Session,
        ) -> Result<Option<ScheduledCheckpoint<EcowittProviderCursor>>, TaskFailure> {
            self.events.borrow_mut().push("checkpoint".to_owned());
            Ok(self.checkpoint.clone())
        }

        // commit one Ecowitt running row
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            start: ScheduledRunStart,
        ) -> Result<String, TaskFailure> {
            self.events.borrow_mut().push("start-committed".to_owned());
            self.start = Some(start);
            Ok("run-ecowitt".to_owned())
        }

        // retain actual Ecowitt records and cursor atomically
        fn complete_scheduled_ingestion(
            &mut self,
            _session: &mut Self::Session,
            completion: ScheduledCompletion<
                NormalizedEcowittRecord,
                EcowittResponseMetadata,
                EcowittProviderCursor,
            >,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("complete".to_owned());
            self.completion = Some(completion);
            Ok(())
        }

        // record one Ecowitt failed run
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _run_id: &str,
            _failure: &TaskFailure,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("fail".to_owned());
            Ok(())
        }

        // release one Ecowitt session
        fn release_source_session(&mut self, _session: Self::Session) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("release".to_owned());
            Ok(())
        }
    }

    #[derive(Debug)]
    struct ScheduledEcowittTimeAdapter {
        calls: usize,
        expected_received_at_ms: i64,
        expected_timezone: String,
        local_date: EcowittAdmittedLocalDate,
    }

    impl EcowittTimeAdapter for ScheduledEcowittTimeAdapter {
        // admit only the exact scheduled provider instant and timezone
        fn local_date(
            &mut self,
            received_at_ms: i64,
            timezone: &str,
        ) -> Result<EcowittAdmittedLocalDate, TaskFailure> {
            self.calls += 1;

            // fail when lifecycle identity drifts before normalization
            if received_at_ms != self.expected_received_at_ms || timezone != self.expected_timezone
            {
                return Err(failure("ecowitt_time_identity_mismatch", 0));
            }

            Ok(self.local_date)
        }
    }

    #[derive(Debug)]
    struct ScheduledEcowittMetadataAdapter {
        calls: usize,
        observed: Vec<EcowittProviderMetadata>,
    }

    impl EcowittCanonicalMetadataAdapter for ScheduledEcowittMetadataAdapter {
        // measure the actual constructed metadata through the admitted serializer seam
        fn measure_provider_metadata(
            &mut self,
            metadata: &EcowittProviderMetadata,
        ) -> Result<EcowittCanonicalMetadataMeasurement, TaskFailure> {
            self.calls += 1;
            self.observed.push(metadata.clone());
            Ok(EcowittCanonicalMetadataMeasurement::admitted(1_163))
        }
    }

    #[derive(Debug)]
    struct EcowittNormalizingProvider {
        events: Rc<RefCell<Vec<String>>>,
        gateway_mac: String,
        metadata_adapter: ScheduledEcowittMetadataAdapter,
        payload: EcowittLivePayload,
        received_at_ms: i64,
        request: Option<EcowittScheduledRequest>,
        time_adapter: ScheduledEcowittTimeAdapter,
    }

    impl ScheduledProvider for EcowittNormalizingProvider {
        type Cursor = EcowittProviderCursor;
        type Record = NormalizedEcowittRecord;
        type Request = EcowittScheduledRequest;
        type ResponseMetadata = EcowittResponseMetadata;

        // normalize one typed Ecowitt response inside the scheduled provider operation
        fn fetch(
            &mut self,
            request: Self::Request,
        ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
            self.events.borrow_mut().push("fetch".to_owned());
            let normalized = normalize_ecowitt_live(
                &self.gateway_mac,
                &self.payload,
                &EcowittLiveRequest {
                    expected_mac: request.expected_mac.clone(),
                    model: request.model.clone(),
                    // retain the rain fields consumed by live normalization
                    previous_cursor: request.previous_cursor.as_ref().map(|cursor| {
                        EcowittPreviousCursor {
                            rain_daily_total_mm: cursor.rain_daily_total_mm,
                            rain_day: Some(cursor.rain_day.clone()),
                        }
                    }),
                    source_id: request.source_id.clone(),
                    timezone: request.timezone.clone(),
                },
                self.received_at_ms,
                2,
                &mut self.time_adapter,
                &mut self.metadata_adapter,
            )?;
            self.request = Some(request);

            Ok(ProviderBatch {
                attempts: normalized.attempts,
                provider_cursor: normalized.provider_cursor,
                records: normalized.records,
                response_metadata: normalized.response_metadata,
                upstream_response_checksum: "ecowitt-checksum".to_owned(),
            })
        }
    }

    #[derive(Debug)]
    struct AmbientWeatherRecordRepository {
        checkpoint: Option<ScheduledCheckpoint<Option<AmbientWeatherProviderCursor>>>,
        completion: Option<
            ScheduledCompletion<
                NormalizedAmbientWeatherRecord,
                AmbientWeatherResponseMetadata,
                Option<AmbientWeatherProviderCursor>,
            >,
        >,
        events: Rc<RefCell<Vec<String>>>,
        start: Option<ScheduledRunStart>,
    }

    impl
        ScheduledRunRepository<
            NormalizedAmbientWeatherRecord,
            AmbientWeatherResponseMetadata,
            Option<AmbientWeatherProviderCursor>,
        > for AmbientWeatherRecordRepository
    {
        type Session = ();

        // acquire one Ambient source session
        fn acquire_source_session(
            &mut self,
            _source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            self.events.borrow_mut().push("lock".to_owned());
            Ok(Some(()))
        }

        // record Ambient stale-run cleanup
        fn abandon_expired_runs(
            &mut self,
            _session: &mut Self::Session,
            _now_ms: i64,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("abandon".to_owned());
            Ok(())
        }

        // return one nullable typed Ambient checkpoint
        fn scheduled_checkpoint(
            &mut self,
            _session: &mut Self::Session,
        ) -> Result<Option<ScheduledCheckpoint<Option<AmbientWeatherProviderCursor>>>, TaskFailure>
        {
            self.events.borrow_mut().push("checkpoint".to_owned());
            Ok(self.checkpoint.clone())
        }

        // commit one Ambient running row
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            start: ScheduledRunStart,
        ) -> Result<String, TaskFailure> {
            self.events.borrow_mut().push("start-committed".to_owned());
            self.start = Some(start);
            Ok("run-ambient-weather".to_owned())
        }

        // retain actual Ambient records and response evidence atomically
        fn complete_scheduled_ingestion(
            &mut self,
            _session: &mut Self::Session,
            completion: ScheduledCompletion<
                NormalizedAmbientWeatherRecord,
                AmbientWeatherResponseMetadata,
                Option<AmbientWeatherProviderCursor>,
            >,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("complete".to_owned());
            self.completion = Some(completion);
            Ok(())
        }

        // record one Ambient failed run
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _run_id: &str,
            _failure: &TaskFailure,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("fail".to_owned());
            Ok(())
        }

        // release one Ambient source session
        fn release_source_session(&mut self, _session: Self::Session) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("release".to_owned());
            Ok(())
        }
    }

    #[derive(Debug)]
    struct AmbientWeatherNormalizingProvider {
        events: Rc<RefCell<Vec<String>>>,
        payload: AmbientWeatherPayload,
        received_at_ms: i64,
        request: Option<AmbientWeatherRangeRequest>,
    }

    impl ScheduledProvider for AmbientWeatherNormalizingProvider {
        type Cursor = Option<AmbientWeatherProviderCursor>;
        type Record = NormalizedAmbientWeatherRecord;
        type Request = AmbientWeatherRangeRequest;
        type ResponseMetadata = AmbientWeatherResponseMetadata;

        // normalize one typed Ambient response inside the scheduled provider operation
        fn fetch(
            &mut self,
            request: Self::Request,
        ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
            self.events.borrow_mut().push("fetch".to_owned());
            let normalized =
                normalize_ambient_weather_range(&self.payload, &request, self.received_at_ms, 1)?;
            self.request = Some(request);

            Ok(ProviderBatch {
                attempts: normalized.attempts,
                provider_cursor: normalized.provider_cursor,
                records: normalized.records,
                response_metadata: normalized.response_metadata,
                upstream_response_checksum: "d".repeat(64),
            })
        }
    }

    #[derive(Debug)]
    struct NoaaTideRecordRepository {
        abandoned_at_ms: Option<i64>,
        checkpoint: Option<ScheduledCheckpoint<Option<NoaaTideProviderCursor>>>,
        completion: Option<
            ScheduledCompletion<
                NormalizedNoaaTideRecord,
                NoaaTideResponseMetadata,
                Option<NoaaTideProviderCursor>,
            >,
        >,
        events: Rc<RefCell<Vec<String>>>,
        start: Option<ScheduledRunStart>,
    }

    impl
        ScheduledRunRepository<
            NormalizedNoaaTideRecord,
            NoaaTideResponseMetadata,
            Option<NoaaTideProviderCursor>,
        > for NoaaTideRecordRepository
    {
        type Session = ();

        // acquire one NOAA tide session
        fn acquire_source_session(
            &mut self,
            _source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            self.events.borrow_mut().push("lock".to_owned());
            Ok(Some(()))
        }

        // record NOAA stale-run cleanup
        fn abandon_expired_runs(
            &mut self,
            _session: &mut Self::Session,
            now_ms: i64,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("abandon".to_owned());
            self.abandoned_at_ms = Some(now_ms);
            Ok(())
        }

        // return one typed NOAA checkpoint
        fn scheduled_checkpoint(
            &mut self,
            _session: &mut Self::Session,
        ) -> Result<Option<ScheduledCheckpoint<Option<NoaaTideProviderCursor>>>, TaskFailure>
        {
            self.events.borrow_mut().push("checkpoint".to_owned());
            Ok(self.checkpoint.clone())
        }

        // commit one NOAA running row
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            start: ScheduledRunStart,
        ) -> Result<String, TaskFailure> {
            self.events.borrow_mut().push("start-committed".to_owned());
            self.start = Some(start);
            Ok("run-noaa-tide".to_owned())
        }

        // retain actual NOAA records and cursor atomically
        fn complete_scheduled_ingestion(
            &mut self,
            _session: &mut Self::Session,
            completion: ScheduledCompletion<
                NormalizedNoaaTideRecord,
                NoaaTideResponseMetadata,
                Option<NoaaTideProviderCursor>,
            >,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("complete".to_owned());
            self.completion = Some(completion);
            Ok(())
        }

        // record one NOAA failed run
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _run_id: &str,
            _failure: &TaskFailure,
        ) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("fail".to_owned());
            Ok(())
        }

        // release one NOAA session
        fn release_source_session(&mut self, _session: Self::Session) -> Result<(), TaskFailure> {
            self.events.borrow_mut().push("release".to_owned());
            Ok(())
        }
    }

    #[derive(Debug)]
    struct NoaaTideNormalizingProvider {
        events: Rc<RefCell<Vec<String>>>,
        payload: NoaaTidePayload,
        received_at_ms: i64,
        request: Option<NoaaTideRangeRequest>,
    }

    impl ScheduledProvider for NoaaTideNormalizingProvider {
        type Cursor = Option<NoaaTideProviderCursor>;
        type Record = NormalizedNoaaTideRecord;
        type Request = NoaaTideRangeRequest;
        type ResponseMetadata = NoaaTideResponseMetadata;

        // normalize one typed NOAA response inside the scheduled provider operation
        fn fetch(
            &mut self,
            request: Self::Request,
        ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
            self.events.borrow_mut().push("fetch".to_owned());
            let normalized =
                normalize_noaa_tide_range(&self.payload, &request, self.received_at_ms, 2)?;
            self.request = Some(request);

            Ok(ProviderBatch {
                attempts: normalized.attempts,
                // retain Node's nullable final-valid provider cursor
                provider_cursor: normalized
                    .provider_cursor_valid_at_ms
                    .map(|valid_at_ms| NoaaTideProviderCursor { valid_at_ms }),
                records: normalized.records,
                response_metadata: normalized.response_metadata,
                upstream_response_checksum: "noaa-tide-checksum".to_owned(),
            })
        }
    }

    #[derive(Debug, Default)]
    struct FakeHeartbeats {
        fail_next: bool,
        updates: Vec<WorkerHeartbeat>,
    }

    impl HeartbeatRepository for FakeHeartbeats {
        // persist one fake heartbeat
        fn update_worker_heartbeat(
            &mut self,
            heartbeat: WorkerHeartbeat,
        ) -> Result<(), TaskFailure> {
            self.updates.push(heartbeat);

            // fail one selected write
            if std::mem::take(&mut self.fail_next) {
                return Err(failure("heartbeat_failed", 0));
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeIterationRepository {
        discover_at_ms: Vec<i64>,
        discovery_failure: Option<TaskFailure>,
        due_sources: Vec<WorkerDueSource>,
        heartbeats: Vec<WorkerHeartbeat>,
    }

    impl HeartbeatRepository for FakeIterationRepository {
        // persist one coordinator heartbeat
        fn update_worker_heartbeat(
            &mut self,
            heartbeat: WorkerHeartbeat,
        ) -> Result<(), TaskFailure> {
            self.heartbeats.push(heartbeat);
            Ok(())
        }
    }

    impl WorkerIterationRepository for FakeIterationRepository {
        // return due sources in their injected durable order
        fn discover_due_sources(
            &mut self,
            as_of_ms: i64,
        ) -> Result<Vec<WorkerDueSource>, TaskFailure> {
            self.discover_at_ms.push(as_of_ms);

            // abort discovery before any source dispatch
            if let Some(failure) = self.discovery_failure.take() {
                return Err(failure);
            }
            Ok(std::mem::take(&mut self.due_sources))
        }
    }

    #[derive(Default)]
    struct FakeIterationDispatcher {
        fail_source_id: Option<String>,
        visited: Vec<String>,
    }

    impl<Repository> WorkerScheduledSourceDispatcher<Repository> for FakeIterationDispatcher {
        // dispatch one source and capture success only for completed work
        fn run_source<C: FnMut() -> i64>(
            &mut self,
            _repository: &mut Repository,
            source: &WorkerDueSource,
            success_clock: &mut C,
        ) -> Result<SourceRunResult, TaskFailure> {
            self.visited.push(source.source_id().to_owned());

            // inject one unexpected source-boundary failure
            if self.fail_source_id.as_deref() == Some(source.source_id()) {
                return Err(failure("dispatcher_failed", 2));
            }
            let successful_at_ms = success_clock();
            Ok(SourceRunResult {
                attempts: 1,
                reason: None,
                record_count: 1,
                run_id: Some(format!("run-{}", source.source_id())),
                secondary_error_codes: Vec::new(),
                source_id: source.source_id().to_owned(),
                status: SourceRunStatus::Succeeded,
                successful_at_ms: Some(successful_at_ms),
            })
        }
    }

    struct FakeIterationSidecars {
        events: Vec<&'static str>,
        fixed_gauge: Result<FixedGaugeTargetResult, TaskFailure>,
        inference: Result<bool, TaskFailure>,
        inference_now_ms: Vec<i64>,
        maintenance: Result<(), TaskFailure>,
        maintenance_due: Vec<MaintenanceCaptureDue>,
        rain: Result<RainCaptureResult, TaskFailure>,
        temperature: Result<TemperatureCanaryCollectionResult, TaskFailure>,
    }

    impl<Repository> WorkerSidecarTasks<Repository> for FakeIterationSidecars {
        // record the temperature-canary position before returning its typed result
        fn collect_temperature_canary(
            &mut self,
            _repository: &mut Repository,
        ) -> Result<TemperatureCanaryCollectionResult, TaskFailure> {
            self.events.push("temperature");
            self.temperature.clone()
        }

        // record the rain-capture position before returning its typed result
        fn collect_rain_capture(
            &mut self,
            _repository: &mut Repository,
        ) -> Result<RainCaptureResult, TaskFailure> {
            self.events.push("rain");
            self.rain.clone()
        }

        // record the fixed-target position before returning its typed result
        fn publish_fixed_gauge_target(
            &mut self,
            _repository: &mut Repository,
        ) -> Result<FixedGaugeTargetResult, TaskFailure> {
            self.events.push("fixed-gauge");
            self.fixed_gauge.clone()
        }

        // retain the separately captured inference clock
        fn publish_rain_inference(
            &mut self,
            _repository: &mut Repository,
            now_ms: i64,
        ) -> Result<bool, TaskFailure> {
            self.events.push("inference");
            self.inference_now_ms.push(now_ms);
            self.inference.clone()
        }

        // retain the exact maintenance due decision supplied by the coordinator
        fn capture_adjustment_maintenance(
            &mut self,
            _repository: &mut Repository,
            due: MaintenanceCaptureDue,
        ) -> Result<(), TaskFailure> {
            self.events.push("maintenance");
            self.maintenance_due.push(due);
            self.maintenance.clone()
        }
    }

    impl WorkerSidecarDiagnosticSink for Vec<WorkerSidecarDiagnostic> {
        // retain sidecar diagnostics in execution order
        fn record_sidecar_diagnostic(&mut self, diagnostic: WorkerSidecarDiagnostic) {
            self.push(diagnostic);
        }
    }

    // build one successful provider batch
    fn successful_batch() -> OpenMeteoScheduledRecordBatch {
        ProviderBatch {
            attempts: 2,
            provider_cursor: Some(OpenMeteoCursor::ValidAt(1_787_613_350_000)),
            records: vec![
                ScheduledRecord {
                    valid_at_ms: 1_787_613_300_000,
                },
                ScheduledRecord {
                    valid_at_ms: 1_787_613_350_000,
                },
            ],
            response_metadata: BTreeMap::from([
                ("http_status".to_owned(), "200".to_owned()),
                ("upstream_response_count".to_owned(), "2".to_owned()),
            ]),
            upstream_response_checksum: "checksum-1".to_owned(),
        }
    }

    // build one successful forecast batch with its product-run cursor
    fn successful_forecast_batch() -> OpenMeteoScheduledRecordBatch {
        ProviderBatch {
            provider_cursor: Some(OpenMeteoCursor::ProductRunAt(1_787_613_420_000)),
            ..successful_batch()
        }
    }

    // build one source result
    fn source_result(status: SourceRunStatus) -> SourceRunResult {
        SourceRunResult {
            attempts: 1,
            reason: (status == SourceRunStatus::Failed).then(|| "provider_failed".to_owned()),
            record_count: usize::from(status == SourceRunStatus::Succeeded),
            run_id: Some("run-1".to_owned()),
            secondary_error_codes: Vec::new(),
            source_id: "source-1".to_owned(),
            status,
            successful_at_ms: (status == SourceRunStatus::Succeeded).then_some(1_787_378_350_000),
        }
    }

    // build one scheduled source identity
    fn scheduled_open_meteo_source() -> OpenMeteoScheduledSource {
        OpenMeteoScheduledSource {
            active: true,
            adapter_version: OPEN_METEO_CURRENT_ADAPTER_VERSION,
            cadence_seconds: 120,
            id: "source-1".to_owned(),
            material_config_identity: "material-1".to_owned(),
            provider_key: "open-meteo".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_config_fingerprint: "fingerprint-1".to_owned(),
            source_key: "open-meteo-current-v1".to_owned(),
            source_kind: OpenMeteoSourceKind::ModelCurrent,
            station_slug: "station-1".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }
    }

    // build one discovered OpenMeteo source identity
    fn iteration_source(source_id: &str) -> WorkerDueSource {
        let mut source = scheduled_open_meteo_source();
        source.id = source_id.to_owned();
        WorkerDueSource::OpenMeteo(source)
    }

    // build one matching runtime configuration
    fn open_meteo_runtime_configuration() -> OpenMeteoRuntimeConfiguration {
        let source = scheduled_open_meteo_source();

        OpenMeteoRuntimeConfiguration {
            active: source.active,
            adapter_version: source.adapter_version,
            capabilities: BTreeSet::from([OpenMeteoCapability::Current]),
            material_config_identity: source.material_config_identity,
            provider_key: source.provider_key,
            site_slug: source.site_slug,
            source_config_fingerprint: source.source_config_fingerprint,
            source_key: source.source_key,
            source_kind: source.source_kind,
            station_slug: source.station_slug,
            timezone: source.timezone,
        }
    }

    // build one deterministic execution input
    fn scheduled_execution<'a>(
        source: &'a OpenMeteoScheduledSource,
        configuration: &'a OpenMeteoRuntimeConfiguration,
        maximum_range_seconds: Option<u64>,
    ) -> ScheduledRunExecution<'a> {
        ScheduledRunExecution {
            configuration,
            maximum_range_seconds,
            now_ms: 1_787_613_420_000,
            run_deadline_at_ms: 1_787_613_540_000,
            source,
        }
    }

    // build one scheduled Tempest source identity
    fn scheduled_tempest_source() -> TempestScheduledSource {
        TempestScheduledSource {
            active: true,
            cadence_seconds: 3_600,
            id: "source-tempest".to_owned(),
            material_config_identity: "tempest-material-1".to_owned(),
            provider_key: "weatherflow-tempest".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_config_fingerprint: "tempest-fingerprint-1".to_owned(),
            source_key: "tempest-203055-observations-v2".to_owned(),
            source_kind: TempestSourceKind::PhysicalSensor,
            station_slug: "tempest-203055".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }
    }

    // build one matching Tempest runtime configuration
    fn tempest_runtime_configuration() -> TempestRuntimeConfiguration {
        let source = scheduled_tempest_source();

        TempestRuntimeConfiguration {
            adapter_contract_version: TEMPEST_SOURCE_CONTRACT_VERSION,
            device_id: 470_937,
            location_id: 203_055,
            material_config_identity: source.material_config_identity,
            provider_key: source.provider_key,
            serial: "ST-00198967".to_owned(),
            site_slug: source.site_slug,
            source_config_fingerprint: source.source_config_fingerprint,
            source_key: source.source_key,
            source_kind: source.source_kind,
            station_slug: source.station_slug,
            timezone: source.timezone,
        }
    }

    // build one scheduled Ecowitt source identity
    fn scheduled_ecowitt_source() -> EcowittScheduledSource {
        EcowittScheduledSource {
            active: true,
            cadence_seconds: 60,
            id: "source-ecowitt".to_owned(),
            material_config_identity: "ecowitt-material-1".to_owned(),
            provider_key: "ecowitt-local".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_config_fingerprint: "ecowitt-fingerprint-1".to_owned(),
            source_key: "ecowitt-88f15505d89f-local-live-v1".to_owned(),
            source_kind: EcowittSourceKind::PhysicalSensor,
            station_slug: "ballydidean-ecowitt".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }
    }

    // build one matching Ecowitt runtime configuration
    fn ecowitt_runtime_configuration() -> EcowittRuntimeConfiguration {
        let source = scheduled_ecowitt_source();

        EcowittRuntimeConfiguration {
            adapter_contract_version: ECOWITT_LOCAL_LIVE_ADAPTER_VERSION,
            capabilities: BTreeSet::from([EcowittCapability::Current]),
            expected_mac: "88:F1:55:05:D8:9F".to_owned(),
            gateway_host: "192.168.11.137".to_owned(),
            material_config_identity: source.material_config_identity,
            model: "GW3000".to_owned(),
            provider_key: source.provider_key,
            site_slug: source.site_slug,
            source_config_fingerprint: source.source_config_fingerprint,
            source_key: source.source_key,
            source_kind: source.source_kind,
            station_slug: source.station_slug,
            timezone: source.timezone,
        }
    }

    // build one normalizing scheduled Ecowitt provider
    fn scheduled_ecowitt_provider(events: Rc<RefCell<Vec<String>>>) -> EcowittNormalizingProvider {
        EcowittNormalizingProvider {
            events,
            gateway_mac: "88:f1:55:05:d8:9f".to_owned(),
            metadata_adapter: ScheduledEcowittMetadataAdapter {
                calls: 0,
                observed: Vec::new(),
            },
            payload: EcowittLivePayload {
                group_count: 1,
                piezo_rain: Some(EcowittRainReadings {
                    battery: None,
                    daily_total: Some(EcowittMeasurement {
                        unit: EcowittUnit::Inch,
                        value: 0.15,
                    }),
                    rate: None,
                }),
                ..EcowittLivePayload::default()
            },
            received_at_ms: 1_788_012_000_000,
            request: None,
            time_adapter: ScheduledEcowittTimeAdapter {
                calls: 0,
                expected_received_at_ms: 1_788_012_000_000,
                expected_timezone: "America/Los_Angeles".to_owned(),
                local_date: EcowittAdmittedLocalDate::admitted(2026, 8, 29),
            },
        }
    }

    // build one scheduled Ambient source identity
    fn scheduled_ambient_weather_source() -> AmbientWeatherScheduledSource {
        AmbientWeatherScheduledSource {
            active: true,
            cadence_seconds: 3_600,
            id: "source-ambient-weather".to_owned(),
            material_config_identity: "ambient-weather-material-1".to_owned(),
            provider_key: "ambient-weather-network".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_config_fingerprint: "ambient-weather-fingerprint-1".to_owned(),
            source_key: "ambient-merlin-observations-v1".to_owned(),
            source_kind: AmbientWeatherSourceKind::PhysicalSensor,
            station_slug: "ambient-merlin".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }
    }

    // build one matching checked Ambient configuration
    fn ambient_weather_runtime_configuration() -> AmbientWeatherRuntimeConfiguration {
        let source = scheduled_ambient_weather_source();

        AmbientWeatherRuntimeConfiguration {
            adapter_contract_version: AMBIENT_WEATHER_ADAPTER_VERSION,
            capabilities: BTreeSet::from([
                AmbientWeatherCapability::Current,
                AmbientWeatherCapability::Historical,
            ]),
            device_id: "5fe7d16a25083b000171f8e5".to_owned(),
            mac_address: "F0:08:D1:07:B3:9E".to_owned(),
            material_config_identity: source.material_config_identity,
            maximum_chunk_days: 7,
            model: Some("AMBWeatherV4.3.6".to_owned()),
            provider_key: source.provider_key,
            serial: Some("5fe7d16a25083b000171f8e5".to_owned()),
            site_slug: source.site_slug,
            source_active: true,
            source_config_fingerprint: source.source_config_fingerprint,
            source_key: source.source_key,
            source_kind: source.source_kind,
            station_slug: source.station_slug,
            timezone: source.timezone,
        }
    }

    // build one compiled-Node-equivalent Ambient scheduled provider
    fn scheduled_ambient_weather_provider(
        events: Rc<RefCell<Vec<String>>>,
    ) -> AmbientWeatherNormalizingProvider {
        // build one typed Ambient observation
        let observation = |date_utc_ms| AmbientWeatherObservation {
            apparent_temperature_f: Some(54.0),
            date_utc_ms,
            device_identity: AmbientWeatherDeviceIdentity::Value(
                "5fe7d16a25083b000171f8e5".to_owned(),
            ),
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

        AmbientWeatherNormalizingProvider {
            events,
            payload: AmbientWeatherPayload {
                observations: vec![
                    observation(1_787_576_400_000),
                    observation(1_787_580_000_000),
                ],
            },
            received_at_ms: 1_787_583_600_000,
            request: None,
        }
    }

    // build one scheduled NOAA source identity
    fn scheduled_noaa_tide_source(source_kind: NoaaTideSourceKind) -> NoaaTideScheduledSource {
        // select the checked catalog identity
        let (cadence_seconds, id, material, fingerprint, source_key, station_slug) =
            match source_kind {
                NoaaTideSourceKind::Observation => (
                    360,
                    "source-noaa-observation",
                    "noaa-observation-material-1",
                    "noaa-observation-fingerprint-1",
                    "noaa-port-townsend-water-level-v1",
                    "port-townsend-tide-gauge",
                ),
                NoaaTideSourceKind::Prediction => (
                    21_600,
                    "source-noaa-prediction",
                    "noaa-prediction-material-1",
                    "noaa-prediction-fingerprint-1",
                    "noaa-glendale-tide-predictions-v1",
                    "glendale-tide-predictions",
                ),
            };

        NoaaTideScheduledSource {
            active: true,
            cadence_seconds,
            id: id.to_owned(),
            material_config_identity: material.to_owned(),
            provider_key: "noaa-co-ops".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_config_fingerprint: fingerprint.to_owned(),
            source_key: source_key.to_owned(),
            source_kind,
            station_slug: station_slug.to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }
    }

    // build one matching NOAA runtime configuration
    fn noaa_tide_runtime_configuration(
        source_kind: NoaaTideSourceKind,
    ) -> NoaaTideRuntimeConfiguration {
        let source = scheduled_noaa_tide_source(source_kind);

        // select the exact checked product contract
        let (adapter_contract_version, capabilities, forecast_days, maximum_chunk_days, station_id) =
            match source_kind {
                NoaaTideSourceKind::Observation => (
                    NOAA_TIDE_OBSERVATION_ADAPTER_VERSION,
                    BTreeSet::from([NoaaTideCapability::Current, NoaaTideCapability::Historical]),
                    None,
                    31,
                    "9444900",
                ),
                NoaaTideSourceKind::Prediction => (
                    NOAA_TIDE_PREDICTION_ADAPTER_VERSION,
                    BTreeSet::from([NoaaTideCapability::Forecast, NoaaTideCapability::Historical]),
                    Some(NOAA_SCHEDULED_PREDICTION_DAYS),
                    366,
                    "9447814",
                ),
            };

        NoaaTideRuntimeConfiguration {
            adapter_contract_version,
            capabilities,
            forecast_days,
            material_config_identity: source.material_config_identity,
            maximum_chunk_days,
            provider_key: source.provider_key,
            site_slug: source.site_slug,
            source_active: true,
            source_config_fingerprint: source.source_config_fingerprint,
            source_key: source.source_key,
            source_kind,
            station_active: true,
            station_id: station_id.to_owned(),
            station_slug: source.station_slug,
            timezone: source.timezone,
        }
    }

    // build one normalizing NOAA observation provider
    fn scheduled_noaa_observation_provider(
        events: Rc<RefCell<Vec<String>>>,
        valid_at_ms: i64,
    ) -> NoaaTideNormalizingProvider {
        NoaaTideNormalizingProvider {
            events,
            payload: NoaaTidePayload {
                observations: Some(vec![NoaaTideObservation {
                    flags: "0,0,0,0".to_owned(),
                    quality_status: "v".to_owned(),
                    valid_at_ms,
                    water_level_m: 0.770,
                }]),
                ..NoaaTidePayload::default()
            },
            received_at_ms: 1_788_012_000_000,
            request: None,
        }
    }

    // build one normalizing NOAA prediction provider
    fn scheduled_noaa_prediction_provider(
        events: Rc<RefCell<Vec<String>>>,
        valid_at_ms: i64,
    ) -> NoaaTideNormalizingProvider {
        NoaaTideNormalizingProvider {
            events,
            payload: NoaaTidePayload {
                predictions: Some(vec![NoaaTidePrediction {
                    prediction_type: NoaaPredictionType::High,
                    valid_at_ms,
                    water_level_m: 2.11,
                }]),
                ..NoaaTidePayload::default()
            },
            received_at_ms: 1_788_012_000_000,
            request: None,
        }
    }

    // build one actual normalized scheduled Tempest batch
    fn scheduled_tempest_batch() -> ProviderBatch<
        NormalizedTempestRecord,
        TempestResponseMetadata,
        Option<TempestProviderCursor>,
    > {
        let normalized = normalize_tempest_observations(
            TempestObservationEnvelope {
                bucket_step_minutes: Some(1.0),
                device_id: 470_937,
                observations: Some(vec![TempestObservation {
                    air_temperature_c: Some(18.2),
                    battery_volts: Some(2.77),
                    epoch_seconds: 1_787_371_200,
                    illuminance_lux: Some(1_000.0),
                    lightning_average_distance_km: Some(0.0),
                    lightning_strike_count: Some(0.0),
                    local_daily_rain_accumulation_final_mm: Some(0.0),
                    local_daily_rain_accumulation_mm: Some(0.0),
                    precipitation_analysis_type: Some(0.0),
                    precipitation_mm: Some(0.0),
                    precipitation_type: Some(0.0),
                    pressure_hpa: Some(1_012.0),
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
            },
            &TempestObservationRequest {
                device_id: 470_937,
                end_exclusive_ms: 1_787_374_800_000,
                location_id: 203_055,
                serial: "ST-00198967".to_owned(),
                source_id: "source-tempest".to_owned(),
                start_ms: 1_787_371_200_000,
                timezone: "America/Los_Angeles".to_owned(),
            },
            1_787_376_000_000,
            1,
        )
        .expect("scheduled Tempest fixture should normalize");

        ProviderBatch {
            attempts: normalized.attempts,
            provider_cursor: normalized
                .provider_cursor_valid_at_ms
                .map(|valid_at_ms| TempestProviderCursor { valid_at_ms }),
            records: normalized.records,
            response_metadata: normalized.response_metadata,
            upstream_response_checksum: "tempest-checksum".to_owned(),
        }
    }

    // build the shared open-meteo fixture envelope
    fn open_meteo_envelope(timezone: &str) -> OpenMeteoEnvelope {
        OpenMeteoEnvelope {
            elevation_m: Some(32.0),
            latitude: 47.941,
            longitude: -122.438,
            upstream_timezone: timezone.to_owned(),
        }
    }

    // build one typed weather fixture sample
    fn open_meteo_sample(valid_at_ms: i64, values: [f64; 10]) -> OpenMeteoWeatherSample {
        OpenMeteoWeatherSample {
            apparent_temperature_c: Some(values[1]),
            cloud_cover_percent: Some(values[4]),
            precipitation_mm: Some(values[3]),
            pressure_hpa: Some(values[9]),
            relative_humidity_percent: Some(values[2]),
            temperature_c: Some(values[0]),
            uv_index: Some(values[5]),
            valid_at_ms,
            wind_direction_degrees: Some(values[8]),
            wind_gust_mps: Some(values[7]),
            wind_speed_mps: Some(values[6]),
        }
    }

    // assert shared open-meteo provenance
    fn assert_open_meteo_provenance(record: &NormalizedOpenMeteoRecord, timezone: &str) {
        assert_eq!(record.provenance.dataset, "forecast");
        assert_eq!(record.provenance.elevation_m, Some(32.0));
        assert_eq!(record.provenance.grid_cell, "47.941,-122.438");
        assert_eq!(record.provenance.model, "best_match");
        assert_eq!(record.provenance.upstream_timezone, timezone);
    }

    // match the stale-checkpoint node oracle
    #[test]
    fn scheduled_window_caps_stale_checkpoints() {
        assert_eq!(
            scheduled_window(
                1_787_613_420_000,
                120,
                Some(1_785_542_400_000),
                Some(172_800),
            ),
            Ok(ScheduledWindow {
                end_exclusive_ms: 1_787_613_360_000,
                start_ms: 1_787_440_560_000,
            }),
        );
    }

    // match the first-run node oracle
    #[test]
    fn scheduled_window_uses_one_cadence_without_checkpoint() {
        assert_eq!(
            scheduled_window(1_787_613_420_000, 120, None, None),
            Ok(ScheduledWindow {
                end_exclusive_ms: 1_787_613_360_000,
                start_ms: 1_787_613_240_000,
            }),
        );
    }

    // match the recent-checkpoint node oracle
    #[test]
    fn scheduled_window_keeps_recent_checkpoint() {
        assert_eq!(
            scheduled_window(1_787_613_420_000, 300, Some(1_787_613_300_000), Some(3_600),),
            Ok(ScheduledWindow {
                end_exclusive_ms: 1_787_613_300_000,
                start_ms: 1_787_613_000_000,
            }),
        );
    }

    // reject unbounded schedule inputs
    #[test]
    fn scheduled_window_rejects_invalid_ranges() {
        assert_eq!(
            scheduled_window(0, 59, None, None),
            Err(ScheduleError::CadenceTooShort),
        );
        assert_eq!(
            scheduled_window(0, 120, None, Some(119)),
            Err(ScheduleError::MaximumRangeTooShort),
        );
        assert_eq!(
            scheduled_window(0, u64::MAX, None, None),
            Err(ScheduleError::ArithmeticOverflow),
        );
    }

    // match the pre-boundary node oracle
    #[test]
    fn maintenance_capture_uses_previous_cycle_before_due_minute() {
        assert_eq!(
            adjustment_maintenance_capture_due_at(1_791_441_299_999),
            Ok(MaintenanceCaptureDue {
                issued_at_ms: 1_791_419_700_000,
            }),
        );
    }

    // match the exact-boundary node oracle
    #[test]
    fn maintenance_capture_uses_current_cycle_at_due_minute() {
        assert_eq!(
            adjustment_maintenance_capture_due_at(1_791_441_300_000),
            Ok(MaintenanceCaptureDue {
                issued_at_ms: 1_791_441_300_000,
            }),
        );
    }

    // match delayed ecmwf node cycles
    #[test]
    fn ecmwf_initialization_observes_six_hour_delay() {
        assert_eq!(
            latest_available_ecmwf_initialization(1_787_378_399_999),
            Ok(1_787_335_200_000),
        );
        assert_eq!(
            latest_available_ecmwf_initialization(1_787_378_400_000),
            Ok(1_787_356_800_000),
        );
    }

    // retain euclidean pre-epoch cycles
    #[test]
    fn ecmwf_initialization_handles_pre_epoch_time() {
        assert_eq!(
            latest_available_ecmwf_initialization(21_599_999),
            Ok(-21_600_000),
        );
    }

    // match heartbeat freshness boundaries
    #[test]
    fn worker_health_keeps_exact_boundary_fresh() {
        let now_ms = 1_787_378_400_000;
        let fresh = worker_health(now_ms, Some(1_787_376_600_000), None, true, 1_800_000);
        let stale = worker_health(now_ms, Some(1_787_376_599_999), None, true, 1_800_000);

        assert!(fresh.ready);
        assert!(!fresh.stale);
        assert!(!stale.ready);
        assert!(stale.stale);
    }

    // fail closed on missing and future heartbeats
    #[test]
    fn worker_health_rejects_invalid_temporal_states() {
        let now_ms = 1_787_378_400_000;
        let missing = worker_health(now_ms, None, None, true, 1_800_000);
        let future = worker_health(now_ms, Some(1_787_378_400_001), None, true, 1_800_000);

        assert!(!missing.ready);
        assert!(missing.stale);
        assert!(!future.ready);
        assert!(future.stale);
    }

    // keep success age independent from readiness
    #[test]
    fn worker_health_ignores_old_success_time() {
        let health = worker_health(
            1_787_378_400_000,
            Some(1_787_378_340_000),
            Some(1_767_225_600_000),
            true,
            1_800_000,
        );

        assert!(health.ready);
        assert!(!health.stale);
        assert_eq!(health.last_success_at_ms, Some(1_767_225_600_000));
    }

    // require database readiness separately
    #[test]
    fn worker_health_preserves_database_readiness() {
        let health = worker_health(1_000, Some(1_000), None, false, 1_800_000);

        assert!(!health.ready);
        assert!(!health.stale);
        assert!(health.live);
    }

    // reject an overlapping trigger
    #[test]
    fn non_overlapping_gate_admits_one_run() {
        let gate = NonOverlappingGate::new();
        let first = gate.try_enter().expect("first run should enter");

        assert!(gate.is_running());
        assert!(gate.try_enter().is_none());
        drop(first);
        assert!(!gate.is_running());
        assert!(gate.try_enter().is_some());
    }

    // release the guard during unwinding
    #[test]
    fn non_overlapping_gate_recovers_after_panic() {
        let gate = NonOverlappingGate::new();
        let result = std::panic::catch_unwind(|| {
            let _permit = gate.try_enter().expect("run should enter");
            panic!("simulated worker failure");
        });

        assert!(result.is_err());
        assert!(!gate.is_running());
        assert!(gate.try_enter().is_some());
    }

    // commit the running row before provider traffic
    #[test]
    fn scheduled_run_commits_fetch_and_checkpoint_in_order() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = fake_repository(Rc::clone(&events));
        repository.checkpoint = Some(ScheduledCheckpoint {
            provider_cursor: Some(OpenMeteoCursor::ValidAt(1_787_613_240_000)),
            version: 3,
            window_end_exclusive_ms: 1_787_613_240_000,
        });
        let mut provider = FakeProvider {
            events: Rc::clone(&events),
            outcome: Ok(successful_batch()),
            request: None,
        };
        let source = scheduled_open_meteo_source();
        let configuration = open_meteo_runtime_configuration();

        let result = run_scheduled_source(
            scheduled_execution(&source, &configuration, Some(172_800)),
            &mut repository,
            &mut provider,
            &mut || 1_787_613_425_000,
        )
        .expect("scheduled run should complete");

        assert_eq!(result.status, SourceRunStatus::Succeeded);
        assert_eq!(result.attempts, 2);
        assert_eq!(result.record_count, 2);
        assert_eq!(result.successful_at_ms, Some(1_787_613_425_000));
        assert_eq!(
            events.borrow().as_slice(),
            [
                "lock",
                "abandon",
                "checkpoint",
                "start-committed",
                "fetch",
                "complete",
                "release"
            ],
        );
        assert_eq!(
            provider
                .request
                .as_ref()
                .and_then(|value| value.previous_cursor),
            Some(OpenMeteoCursor::ValidAt(1_787_613_240_000)),
        );
        let request = provider
            .request
            .as_ref()
            .expect("request should be retained");
        assert_eq!(request.endpoint, "forecast/current");
        assert_eq!(request.source_kind, OpenMeteoSourceKind::ModelCurrent);
        assert_eq!(request.timezone, "America/Los_Angeles");
        let start = repository.start.as_ref().expect("start should be retained");
        assert_eq!(start.adapter_version, OPEN_METEO_CURRENT_ADAPTER_VERSION);
        assert_eq!(start.deadline_at_ms, 1_787_613_540_000);
        assert_eq!(start.endpoint, "forecast/current");
        assert_eq!(start.source_config_fingerprint, "fingerprint-1");
        let completion = repository.completion.expect("completion should persist");
        assert_eq!(completion.expected_checkpoint_version, Some(3));
        assert_eq!(
            completion.provider_cursor,
            Some(OpenMeteoCursor::ValidAt(1_787_613_350_000)),
        );
        assert_eq!(completion.last_valid_at_ms, 1_787_613_350_000);
        assert_eq!(
            completion.records,
            [
                ScheduledRecord {
                    valid_at_ms: 1_787_613_300_000,
                },
                ScheduledRecord {
                    valid_at_ms: 1_787_613_350_000,
                },
            ],
        );
        assert_eq!(
            completion.response_metadata,
            BTreeMap::from([
                ("http_status".to_owned(), "200".to_owned()),
                ("upstream_response_count".to_owned(), "2".to_owned()),
            ]),
        );
        assert_eq!(completion.upstream_response_checksum, "checksum-1");
    }

    // persist actual normalized records with provider evidence
    #[test]
    fn scheduled_run_atomically_carries_open_meteo_records_and_response_evidence() {
        let normalized = normalize_open_meteo_current(
            open_meteo_envelope("America/Los_Angeles"),
            open_meteo_sample(
                1_787_613_360_000,
                [16.4, 15.8, 78.0, 0.0, 42.0, 0.0, 3.2, 5.4, 243.0, 1013.6],
            ),
            "source-1",
            1_787_613_420_000,
            1,
        )
        .expect("current fixture should normalize");
        let expected_records = normalized.records.clone();
        let expected_metadata = BTreeMap::from([
            ("generation_ms".to_owned(), "0.061".to_owned()),
            ("http_status".to_owned(), "200".to_owned()),
        ]);
        let mut provider = OpenMeteoRecordProvider {
            batch: ProviderBatch {
                attempts: normalized.attempts,
                provider_cursor: Some(normalized.provider_cursor),
                records: normalized.records,
                response_metadata: expected_metadata.clone(),
                upstream_response_checksum: "open-meteo-checksum".to_owned(),
            },
        };
        let mut repository = OpenMeteoRecordRepository::default();
        let source = scheduled_open_meteo_source();
        let configuration = open_meteo_runtime_configuration();

        let result = run_scheduled_source(
            scheduled_execution(&source, &configuration, None),
            &mut repository,
            &mut provider,
            &mut || 1_787_613_425_000,
        )
        .expect("typed provider run should complete");

        assert_eq!(result.status, SourceRunStatus::Succeeded);
        let completion = repository.completion.expect("completion should persist");
        assert_eq!(completion.records, expected_records);
        assert_eq!(completion.response_metadata, expected_metadata);
        assert_eq!(completion.upstream_response_checksum, "open-meteo-checksum",);
        assert_eq!(completion.record_count, 1);
        assert_eq!(completion.last_valid_at_ms, 1_787_613_360_000);
    }

    // run actual Tempest records through the committed lifecycle
    #[test]
    fn scheduled_tempest_uses_one_completed_hour_and_atomic_completion() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = TempestRecordRepository {
            checkpoint: Some(ScheduledCheckpoint {
                provider_cursor: Some(TempestProviderCursor {
                    valid_at_ms: 1_787_367_540_000,
                }),
                version: 9,
                window_end_exclusive_ms: 1_787_367_600_000,
            }),
            completion: None,
            events: Rc::clone(&events),
            start: None,
        };
        let batch = scheduled_tempest_batch();
        let expected_records = batch.records.clone();
        let expected_metadata = batch.response_metadata;
        let mut provider = TempestRecordProvider {
            batch,
            events: Rc::clone(&events),
            request: None,
        };
        let source = scheduled_tempest_source();
        let configuration = tempest_runtime_configuration();

        let result = run_scheduled_tempest_source(
            TempestScheduledExecution {
                configuration: &configuration,
                now_ms: 1_787_376_000_000,
                run_deadline_at_ms: 1_787_376_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_787_376_005_000,
        )
        .expect("scheduled Tempest run should complete");

        assert_eq!(result.status, SourceRunStatus::Succeeded);
        assert_eq!(result.successful_at_ms, Some(1_787_376_005_000));
        assert_eq!(
            events.borrow().as_slice(),
            [
                "lock",
                "abandon",
                "checkpoint",
                "start-committed",
                "fetch",
                "complete",
                "release",
            ],
        );
        let request = provider.request.expect("Tempest request should persist");
        assert_eq!(request.device_id, 470_937);
        assert_eq!(request.location_id, 203_055);
        assert_eq!(request.serial, "ST-00198967");
        assert_eq!(request.start_ms, 1_787_371_200_000);
        assert_eq!(request.end_exclusive_ms, 1_787_374_800_000);
        assert_eq!(request.source_id, "source-tempest");
        let start = repository.start.expect("Tempest start should persist");
        assert_eq!(start.adapter_version, TEMPEST_OBSERVATION_ADAPTER_VERSION);
        assert_eq!(start.endpoint, TEMPEST_OBSERVATION_ENDPOINT);
        assert_eq!(start.source_config_fingerprint, "tempest-fingerprint-1");
        assert_eq!(start.requested_window.start_ms, 1_787_371_200_000);
        assert_eq!(start.requested_window.end_exclusive_ms, 1_787_374_800_000);
        let completion = repository
            .completion
            .expect("Tempest completion should persist");
        assert_eq!(completion.expected_checkpoint_version, Some(9));
        assert_eq!(completion.records, expected_records);
        assert_eq!(completion.response_metadata, expected_metadata);
        assert_eq!(completion.last_valid_at_ms, 1_787_371_200_000);
        assert_eq!(
            completion.provider_cursor,
            Some(TempestProviderCursor {
                valid_at_ms: 1_787_371_200_000,
            })
        );
        assert_eq!(completion.upstream_response_checksum, "tempest-checksum");
    }

    // reject Tempest contract drift before acquiring a session
    #[test]
    fn scheduled_tempest_requires_exact_contract_before_session() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = TempestRecordRepository {
            checkpoint: None,
            completion: None,
            events: Rc::clone(&events),
            start: None,
        };
        let mut provider = TempestRecordProvider {
            batch: scheduled_tempest_batch(),
            events: Rc::clone(&events),
            request: None,
        };
        let source = scheduled_tempest_source();
        let mut configuration = tempest_runtime_configuration();
        configuration.adapter_contract_version = "tempest-observations/v999";

        let failure = run_scheduled_tempest_source(
            TempestScheduledExecution {
                configuration: &configuration,
                now_ms: 1_787_376_000_000,
                run_deadline_at_ms: 1_787_376_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_787_376_005_000,
        )
        .expect_err("Tempest contract drift should fail");

        assert_eq!(failure.code, "source adapter contract must match runtime");
        assert!(events.borrow().is_empty());
        assert_eq!(provider.request, None);

        configuration.adapter_contract_version = TEMPEST_SOURCE_CONTRACT_VERSION;
        configuration.material_config_identity = "different-material".to_owned();
        let skipped = run_scheduled_tempest_source(
            TempestScheduledExecution {
                configuration: &configuration,
                now_ms: 1_787_376_000_000,
                run_deadline_at_ms: 1_787_376_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_787_376_005_000,
        )
        .expect("Tempest identity drift should skip");

        assert_eq!(skipped.status, SourceRunStatus::Skipped);
        assert!(events.borrow().is_empty());
        assert_eq!(provider.request, None);
    }

    // run an actual Ecowitt rain record and cursor through atomic completion
    #[test]
    fn scheduled_ecowitt_preserves_exact_request_rain_cursor_and_adapters() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = EcowittRecordRepository {
            checkpoint: Some(ScheduledCheckpoint {
                provider_cursor: EcowittProviderCursor {
                    rain_daily_total_mm: Some(2.54),
                    rain_day: "2026-08-29".to_owned(),
                    valid_at_ms: 1_788_011_940_000,
                },
                version: 9,
                window_end_exclusive_ms: 1_787_000_000_000,
            }),
            completion: None,
            events: Rc::clone(&events),
            start: None,
        };
        let mut provider = scheduled_ecowitt_provider(Rc::clone(&events));
        let source = scheduled_ecowitt_source();
        let configuration = ecowitt_runtime_configuration();

        let result = run_scheduled_ecowitt_source(
            EcowittScheduledExecution {
                configuration: &configuration,
                now_ms: 1_788_012_000_000,
                run_deadline_at_ms: 1_788_012_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_788_012_005_000,
        )
        .expect("scheduled Ecowitt run should complete");

        assert_eq!(result.status, SourceRunStatus::Succeeded);
        assert_eq!(result.successful_at_ms, Some(1_788_012_005_000));
        assert_eq!(
            events.borrow().as_slice(),
            [
                "lock",
                "abandon",
                "checkpoint",
                "start-committed",
                "fetch",
                "complete",
                "release",
            ],
        );
        let request = provider.request.as_ref().expect("request should persist");
        assert_eq!(request.expected_mac, "88:F1:55:05:D8:9F");
        assert_eq!(request.gateway_host, "192.168.11.137");
        assert_eq!(request.model, "GW3000");
        assert_eq!(request.source_id, "source-ecowitt");
        assert_eq!(request.timezone, "America/Los_Angeles");
        assert_eq!(
            request.previous_cursor,
            Some(EcowittProviderCursor {
                rain_daily_total_mm: Some(2.54),
                rain_day: "2026-08-29".to_owned(),
                valid_at_ms: 1_788_011_940_000,
            }),
        );
        assert_eq!(provider.time_adapter.calls, 1);
        assert_eq!(provider.metadata_adapter.calls, 1);
        let start = repository.start.as_ref().expect("start should persist");
        assert_eq!(start.adapter_version, ECOWITT_LOCAL_LIVE_ADAPTER_VERSION);
        assert_eq!(start.endpoint, "get_livedata_info");
        assert_eq!(start.source_config_fingerprint, "ecowitt-fingerprint-1");
        assert_eq!(start.requested_window.start_ms, 1_788_011_940_000);
        assert_eq!(start.requested_window.end_exclusive_ms, 1_788_012_000_000);
        let completion = repository
            .completion
            .as_ref()
            .expect("completion should persist");
        assert_eq!(completion.expected_checkpoint_version, Some(9));
        assert_eq!(completion.record_count, 1);
        assert_eq!(completion.records.len(), 1);
        assert!(
            (completion.records[0]
                .metrics
                .precipitation_mm
                .expect("rain delta")
                - 1.27)
                .abs()
                < 1e-9
        );
        assert_eq!(completion.provider_cursor.rain_day, "2026-08-29");
        assert!(
            (completion
                .provider_cursor
                .rain_daily_total_mm
                .expect("daily total")
                - 3.81)
                .abs()
                < 1e-12
        );
        assert_eq!(completion.last_valid_at_ms, 1_788_012_000_000);
        assert_eq!(
            completion.response_metadata.gateway_mac,
            "88:F1:55:05:D8:9F"
        );
        assert_eq!(completion.upstream_response_checksum, "ecowitt-checksum");
        assert_eq!(
            provider.metadata_adapter.observed[0], completion.records[0].provider,
            "the committed record must retain the metadata admitted during fetch",
        );
    }

    // reject Ecowitt identity capability contract and cadence drift before locking
    #[test]
    fn scheduled_ecowitt_validates_runtime_contract_before_session() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = EcowittRecordRepository {
            checkpoint: None,
            completion: None,
            events: Rc::clone(&events),
            start: None,
        };
        let mut provider = scheduled_ecowitt_provider(Rc::clone(&events));
        let mut source = scheduled_ecowitt_source();
        let mut configuration = ecowitt_runtime_configuration();

        source.material_config_identity = "different-material".to_owned();
        let identity = run_scheduled_ecowitt_source(
            EcowittScheduledExecution {
                configuration: &configuration,
                now_ms: 1_788_012_000_000,
                run_deadline_at_ms: 1_788_012_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_788_012_005_000,
        )
        .expect("identity drift should skip");
        assert_eq!(identity.status, SourceRunStatus::Skipped);

        source = scheduled_ecowitt_source();
        configuration.capabilities.clear();
        let capability = run_scheduled_ecowitt_source(
            EcowittScheduledExecution {
                configuration: &configuration,
                now_ms: 1_788_012_000_000,
                run_deadline_at_ms: 1_788_012_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_788_012_005_000,
        )
        .expect("capability drift should skip");
        assert_eq!(capability.status, SourceRunStatus::Skipped);

        source = scheduled_ecowitt_source();
        configuration = ecowitt_runtime_configuration();
        configuration.adapter_contract_version = "ecowitt-local-live/v999";
        let contract = run_scheduled_ecowitt_source(
            EcowittScheduledExecution {
                configuration: &configuration,
                now_ms: 1_788_012_000_000,
                run_deadline_at_ms: 1_788_012_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_788_012_005_000,
        )
        .expect_err("contract drift should fail");
        assert_eq!(contract.code, "source adapter contract must match runtime");

        configuration = ecowitt_runtime_configuration();
        source.cadence_seconds = 59;
        let short_range = run_scheduled_ecowitt_source(
            EcowittScheduledExecution {
                configuration: &configuration,
                now_ms: 1_788_012_000_000,
                run_deadline_at_ms: 1_788_012_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_788_012_005_000,
        )
        .expect_err("short cadence should fail");
        assert_eq!(short_range.code, "ecowitt cadence outside provider range");

        source.cadence_seconds = 3_601;
        let long_range = run_scheduled_ecowitt_source(
            EcowittScheduledExecution {
                configuration: &configuration,
                now_ms: 1_788_012_000_000,
                run_deadline_at_ms: 1_788_012_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_788_012_005_000,
        )
        .expect_err("long cadence should fail");
        assert_eq!(long_range.code, "ecowitt cadence outside provider range");
        assert!(events.borrow().is_empty());
        assert_eq!(provider.request, None);
    }

    // run an actual Ambient record through the checkpointed shared lifecycle
    #[test]
    fn scheduled_ambient_weather_preserves_cutoff_request_and_atomic_completion() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = AmbientWeatherRecordRepository {
            checkpoint: Some(ScheduledCheckpoint {
                provider_cursor: None,
                version: 11,
                window_end_exclusive_ms: 1_786_716_000_000,
            }),
            completion: None,
            events: Rc::clone(&events),
            start: None,
        };
        let mut provider = scheduled_ambient_weather_provider(Rc::clone(&events));
        let source = scheduled_ambient_weather_source();
        let configuration = ambient_weather_runtime_configuration();

        let result = run_scheduled_ambient_weather_source(
            AmbientWeatherScheduledExecution {
                configuration: &configuration,
                now_ms: 1_787_580_000_000,
                run_deadline_at_ms: 1_787_580_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_787_580_005_000,
        )
        .expect("scheduled Ambient run should complete");

        assert_eq!(result.status, SourceRunStatus::Succeeded);
        assert_eq!(result.successful_at_ms, Some(1_787_580_005_000));
        assert_eq!(result.record_count, 1);
        assert_eq!(
            events.borrow().as_slice(),
            [
                "lock",
                "abandon",
                "checkpoint",
                "start-committed",
                "fetch",
                "complete",
                "release",
            ],
        );
        let request = provider.request.as_ref().expect("request should persist");
        assert_eq!(request.device_id, "5fe7d16a25083b000171f8e5");
        assert_eq!(request.mac_address, "F0:08:D1:07:B3:9E");
        assert_eq!(request.model.as_deref(), Some("AMBWeatherV4.3.6"));
        assert_eq!(request.serial.as_deref(), Some("5fe7d16a25083b000171f8e5"));
        assert_eq!(request.source_id, "source-ambient-weather");
        assert_eq!(request.timezone, "America/Los_Angeles");
        assert_eq!(request.start_ms, 1_786_975_200_000);
        assert_eq!(request.end_exclusive_ms, 1_787_580_000_000);
        let start = repository.start.as_ref().expect("start should persist");
        assert_eq!(start.adapter_version, AMBIENT_WEATHER_ADAPTER_VERSION);
        assert_eq!(start.endpoint, AMBIENT_WEATHER_DEVICE_DATA_ENDPOINT);
        assert_eq!(
            start.source_config_fingerprint,
            "ambient-weather-fingerprint-1"
        );
        assert_eq!(start.requested_window.start_ms, request.start_ms);
        assert_eq!(
            start.requested_window.end_exclusive_ms,
            request.end_exclusive_ms
        );
        let completion = repository
            .completion
            .as_ref()
            .expect("completion should persist");
        assert_eq!(completion.expected_checkpoint_version, Some(11));
        assert_eq!(completion.attempts, 1);
        assert_eq!(completion.record_count, 1);
        assert_eq!(completion.records.len(), 1);
        assert_eq!(completion.records[0].valid_at_ms, 1_787_576_400_000);
        assert!(
            (completion.records[0]
                .metrics
                .temperature_c
                .expect("temperature")
                - 12.0)
                .abs()
                < 1e-9
        );
        assert_eq!(completion.last_valid_at_ms, 1_787_576_400_000);
        assert_eq!(
            completion.provider_cursor,
            Some(AmbientWeatherProviderCursor {
                valid_at_ms: 1_787_576_400_000,
            }),
        );
        assert_eq!(completion.response_metadata.raw_observation_count, 2);
        assert_eq!(completion.response_metadata.normalized_record_count, 1);
        assert_eq!(
            completion.response_metadata.provider,
            "ambient-weather-network"
        );
        assert_eq!(completion.upstream_response_checksum, "d".repeat(64));
        assert_eq!(completion.window, start.requested_window);
    }

    // reject Ambient activity identity capability contract and range drift before locking
    #[test]
    fn scheduled_ambient_weather_validates_runtime_contract_before_session() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = AmbientWeatherRecordRepository {
            checkpoint: None,
            completion: None,
            events: Rc::clone(&events),
            start: None,
        };
        let mut provider = scheduled_ambient_weather_provider(Rc::clone(&events));
        let mut source = scheduled_ambient_weather_source();
        let mut configuration = ambient_weather_runtime_configuration();

        source.material_config_identity = "different-material".to_owned();
        let identity = run_scheduled_ambient_weather_source(
            AmbientWeatherScheduledExecution {
                configuration: &configuration,
                now_ms: 1_787_580_000_000,
                run_deadline_at_ms: 1_787_580_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_787_580_005_000,
        )
        .expect("identity drift should skip");
        assert_eq!(identity.status, SourceRunStatus::Skipped);

        source = scheduled_ambient_weather_source();
        configuration.capabilities = BTreeSet::from([AmbientWeatherCapability::Historical]);
        let capability = run_scheduled_ambient_weather_source(
            AmbientWeatherScheduledExecution {
                configuration: &configuration,
                now_ms: 1_787_580_000_000,
                run_deadline_at_ms: 1_787_580_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_787_580_005_000,
        )
        .expect("capability drift should skip");
        assert_eq!(capability.status, SourceRunStatus::Skipped);

        configuration = ambient_weather_runtime_configuration();
        configuration.source_active = false;
        let inactive = run_scheduled_ambient_weather_source(
            AmbientWeatherScheduledExecution {
                configuration: &configuration,
                now_ms: 1_787_580_000_000,
                run_deadline_at_ms: 1_787_580_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_787_580_005_000,
        )
        .expect("inactive configured source should skip");
        assert_eq!(inactive.status, SourceRunStatus::Skipped);

        configuration = ambient_weather_runtime_configuration();
        configuration.maximum_chunk_days = 0;
        let range = run_scheduled_ambient_weather_source(
            AmbientWeatherScheduledExecution {
                configuration: &configuration,
                now_ms: 1_787_580_000_000,
                run_deadline_at_ms: 1_787_580_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_787_580_005_000,
        )
        .expect("invalid maximum range should skip");
        assert_eq!(range.status, SourceRunStatus::Skipped);

        configuration = ambient_weather_runtime_configuration();
        configuration.adapter_contract_version = "ambient-device-data/v999";
        let contract = run_scheduled_ambient_weather_source(
            AmbientWeatherScheduledExecution {
                configuration: &configuration,
                now_ms: 1_787_580_000_000,
                run_deadline_at_ms: 1_787_580_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_787_580_005_000,
        )
        .expect_err("adapter drift should fail");
        assert_eq!(contract.code, "source adapter contract must match runtime");

        configuration = ambient_weather_runtime_configuration();
        source.cadence_seconds = 59;
        let short_range = run_scheduled_ambient_weather_source(
            AmbientWeatherScheduledExecution {
                configuration: &configuration,
                now_ms: 1_787_580_000_000,
                run_deadline_at_ms: 1_787_580_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_787_580_005_000,
        )
        .expect_err("short cadence should fail");
        assert_eq!(
            short_range.code,
            "ambient weather cadence outside provider range"
        );

        source.cadence_seconds = 86_401;
        let long_range = run_scheduled_ambient_weather_source(
            AmbientWeatherScheduledExecution {
                configuration: &configuration,
                now_ms: 1_787_580_000_000,
                run_deadline_at_ms: 1_787_580_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_787_580_005_000,
        )
        .expect_err("long cadence should fail");
        assert_eq!(
            long_range.code,
            "ambient weather cadence outside provider range"
        );
        assert!(events.borrow().is_empty());
        assert_eq!(provider.request, None);
    }

    // run an actual lagged NOAA observation through atomic completion
    #[test]
    fn scheduled_noaa_observation_preserves_lagged_checkpoint_range_and_cursor() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = NoaaTideRecordRepository {
            abandoned_at_ms: None,
            checkpoint: Some(ScheduledCheckpoint {
                provider_cursor: Some(NoaaTideProviderCursor {
                    valid_at_ms: 1_788_010_560_000,
                }),
                version: 4,
                window_end_exclusive_ms: 1_788_010_920_000,
            }),
            completion: None,
            events: Rc::clone(&events),
            start: None,
        };
        let mut provider =
            scheduled_noaa_observation_provider(Rc::clone(&events), 1_788_010_920_000);
        let source = scheduled_noaa_tide_source(NoaaTideSourceKind::Observation);
        let configuration = noaa_tide_runtime_configuration(NoaaTideSourceKind::Observation);

        let result = run_scheduled_noaa_tide_source(
            NoaaTideScheduledExecution {
                configuration: &configuration,
                now_ms: 1_788_012_000_000,
                run_deadline_at_ms: 1_788_012_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_788_012_005_000,
        )
        .expect("scheduled NOAA observation should complete");

        assert_eq!(result.status, SourceRunStatus::Succeeded);
        assert_eq!(result.successful_at_ms, Some(1_788_012_005_000));
        assert_eq!(repository.abandoned_at_ms, Some(1_788_012_000_000));
        assert_eq!(
            events.borrow().as_slice(),
            [
                "lock",
                "abandon",
                "checkpoint",
                "start-committed",
                "fetch",
                "complete",
                "release",
            ],
        );
        let request = provider.request.as_ref().expect("request should persist");
        assert_eq!(request.source_kind, NoaaTideSourceKind::Observation);
        assert_eq!(request.station_id, "9444900");
        assert_eq!(request.start_ms, 1_788_010_920_000);
        assert_eq!(request.end_exclusive_ms, 1_788_011_280_000);
        let start = repository.start.as_ref().expect("start should persist");
        assert_eq!(start.adapter_version, NOAA_TIDE_OBSERVATION_ADAPTER_VERSION);
        assert_eq!(start.endpoint, "noaa-co-ops/datagetter");
        assert_eq!(start.requested_window.start_ms, request.start_ms);
        assert_eq!(
            start.requested_window.end_exclusive_ms,
            request.end_exclusive_ms
        );
        let completion = repository
            .completion
            .as_ref()
            .expect("completion should persist");
        assert_eq!(completion.expected_checkpoint_version, Some(4));
        assert_eq!(completion.records.len(), 1);
        assert_eq!(completion.records[0].valid_at_ms, 1_788_010_920_000);
        assert_eq!(completion.last_valid_at_ms, 1_788_010_920_000);
        assert_eq!(
            completion.provider_cursor,
            Some(NoaaTideProviderCursor {
                valid_at_ms: 1_788_010_920_000,
            }),
        );
        assert_eq!(completion.response_metadata.station_id, "9444900");
        assert_eq!(completion.upstream_response_checksum, "noaa-tide-checksum");
    }

    // run an actual NOAA prediction through its independent thirty-day request
    #[test]
    fn scheduled_noaa_prediction_preserves_request_and_window_checkpoint_semantics() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = NoaaTideRecordRepository {
            abandoned_at_ms: None,
            checkpoint: Some(ScheduledCheckpoint {
                provider_cursor: Some(NoaaTideProviderCursor {
                    valid_at_ms: 1_787_983_200_000,
                }),
                version: 7,
                window_end_exclusive_ms: 1_787_983_200_000,
            }),
            completion: None,
            events: Rc::clone(&events),
            start: None,
        };
        let mut provider =
            scheduled_noaa_prediction_provider(Rc::clone(&events), 1_788_055_200_000);
        let source = scheduled_noaa_tide_source(NoaaTideSourceKind::Prediction);
        let configuration = noaa_tide_runtime_configuration(NoaaTideSourceKind::Prediction);

        let result = run_scheduled_noaa_tide_source(
            NoaaTideScheduledExecution {
                configuration: &configuration,
                now_ms: 1_788_012_000_000,
                run_deadline_at_ms: 1_788_012_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_788_012_005_000,
        )
        .expect("scheduled NOAA prediction should complete");

        assert_eq!(result.status, SourceRunStatus::Succeeded);
        let request = provider.request.as_ref().expect("request should persist");
        assert_eq!(request.source_kind, NoaaTideSourceKind::Prediction);
        assert_eq!(request.station_id, "9447814");
        assert_eq!(request.start_ms, 1_788_012_000_000);
        assert_eq!(request.end_exclusive_ms, 1_790_604_000_000);
        let start = repository.start.as_ref().expect("start should persist");
        assert_eq!(start.adapter_version, NOAA_TIDE_PREDICTION_ADAPTER_VERSION);
        assert_eq!(start.requested_window.start_ms, 1_787_983_200_000);
        assert_eq!(start.requested_window.end_exclusive_ms, 1_788_004_800_000);
        let completion = repository
            .completion
            .as_ref()
            .expect("completion should persist");
        assert_eq!(completion.expected_checkpoint_version, Some(7));
        assert_eq!(completion.records.len(), 1);
        assert_eq!(completion.records[0].valid_at_ms, 1_788_055_200_000);
        assert_eq!(
            completion.provider_cursor,
            Some(NoaaTideProviderCursor {
                valid_at_ms: 1_788_055_200_000,
            }),
        );
        assert_eq!(
            completion.last_valid_at_ms, 1_788_004_800_000,
            "prediction checkpoints use the scheduled window rather than the future event",
        );
        assert_eq!(completion.window, start.requested_window);
    }

    // reject NOAA identity capability contract and cadence drift before locking
    #[test]
    fn scheduled_noaa_tide_validates_runtime_contract_before_session() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = NoaaTideRecordRepository {
            abandoned_at_ms: None,
            checkpoint: None,
            completion: None,
            events: Rc::clone(&events),
            start: None,
        };
        let mut provider =
            scheduled_noaa_prediction_provider(Rc::clone(&events), 1_788_055_200_000);
        let mut source = scheduled_noaa_tide_source(NoaaTideSourceKind::Prediction);
        let mut configuration = noaa_tide_runtime_configuration(NoaaTideSourceKind::Prediction);

        source.material_config_identity = "different-material".to_owned();
        let identity = run_scheduled_noaa_tide_source(
            NoaaTideScheduledExecution {
                configuration: &configuration,
                now_ms: 1_788_012_000_000,
                run_deadline_at_ms: 1_788_012_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_788_012_005_000,
        )
        .expect("identity drift should skip");
        assert_eq!(identity.status, SourceRunStatus::Skipped);

        source = scheduled_noaa_tide_source(NoaaTideSourceKind::Prediction);
        configuration.capabilities.clear();
        let capability = run_scheduled_noaa_tide_source(
            NoaaTideScheduledExecution {
                configuration: &configuration,
                now_ms: 1_788_012_000_000,
                run_deadline_at_ms: 1_788_012_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_788_012_005_000,
        )
        .expect("capability drift should skip");
        assert_eq!(capability.status, SourceRunStatus::Skipped);

        configuration = noaa_tide_runtime_configuration(NoaaTideSourceKind::Prediction);
        configuration.adapter_contract_version = "noaa-tide-predictions/v999";
        let contract = run_scheduled_noaa_tide_source(
            NoaaTideScheduledExecution {
                configuration: &configuration,
                now_ms: 1_788_012_000_000,
                run_deadline_at_ms: 1_788_012_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_788_012_005_000,
        )
        .expect_err("adapter drift should fail");
        assert_eq!(contract.code, "source adapter contract must match runtime");

        configuration = noaa_tide_runtime_configuration(NoaaTideSourceKind::Prediction);
        source.cadence_seconds = 359;
        let short_range = run_scheduled_noaa_tide_source(
            NoaaTideScheduledExecution {
                configuration: &configuration,
                now_ms: 1_788_012_000_000,
                run_deadline_at_ms: 1_788_012_600_000,
                source: &source,
            },
            &mut repository,
            &mut provider,
            &mut || 1_788_012_005_000,
        )
        .expect_err("short cadence should fail");
        assert_eq!(short_range.code, "noaa cadence outside provider range");
        assert!(events.borrow().is_empty());
        assert_eq!(provider.request, None);
    }

    // checkpoint forecasts at the selected range boundary
    #[test]
    fn scheduled_forecast_uses_window_end_as_last_valid_checkpoint() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = fake_repository(Rc::clone(&events));
        let mut provider = FakeProvider {
            events,
            outcome: Ok(successful_forecast_batch()),
            request: None,
        };
        let mut source = scheduled_open_meteo_source();
        source.adapter_version = OPEN_METEO_FORECAST_ADAPTER_VERSION;
        source.source_key = "open-meteo-forecast-v4".to_owned();
        source.source_kind = OpenMeteoSourceKind::Forecast;
        let mut configuration = open_meteo_runtime_configuration();
        configuration.adapter_version = OPEN_METEO_FORECAST_ADAPTER_VERSION;
        configuration.capabilities = BTreeSet::from([OpenMeteoCapability::Forecast]);
        configuration.source_key = source.source_key.clone();
        configuration.source_kind = OpenMeteoSourceKind::Forecast;

        let result = run_scheduled_source(
            scheduled_execution(&source, &configuration, None),
            &mut repository,
            &mut provider,
            &mut || 1_787_613_425_000,
        )
        .expect("forecast run should complete");

        assert_eq!(result.status, SourceRunStatus::Succeeded);
        let completion = repository.completion.expect("completion should persist");
        assert_eq!(completion.window.end_exclusive_ms, 1_787_613_360_000);
        assert_eq!(completion.last_valid_at_ms, 1_787_613_360_000);
        assert_eq!(
            completion.provider_cursor,
            Some(OpenMeteoCursor::ProductRunAt(1_787_613_420_000)),
        );

        // contrast the final record with the range checkpoint
        assert_eq!(
            completion.records.last().map(|record| record.valid_at_ms),
            Some(1_787_613_350_000),
        );
    }

    // capture success after commit and never on provider failure
    #[test]
    fn scheduled_run_reads_success_clock_only_after_atomic_completion() {
        let success_events = Rc::new(RefCell::new(Vec::new()));
        let mut success_repository = fake_repository(Rc::clone(&success_events));
        let mut success_provider = FakeProvider {
            events: Rc::clone(&success_events),
            outcome: Ok(successful_batch()),
            request: None,
        };
        let source = scheduled_open_meteo_source();
        let configuration = open_meteo_runtime_configuration();
        let clock_events = Rc::clone(&success_events);

        // record the exact success-clock position
        let mut success_clock = || {
            clock_events.borrow_mut().push("clock".to_owned());
            1_787_613_425_000
        };
        let success = run_scheduled_source(
            scheduled_execution(&source, &configuration, None),
            &mut success_repository,
            &mut success_provider,
            &mut success_clock,
        )
        .expect("scheduled run should complete");

        assert_eq!(success.successful_at_ms, Some(1_787_613_425_000));
        assert_eq!(
            success_events.borrow().as_slice(),
            [
                "lock",
                "abandon",
                "checkpoint",
                "start-committed",
                "fetch",
                "complete",
                "clock",
                "release",
            ],
        );

        let failure_events = Rc::new(RefCell::new(Vec::new()));
        let mut failure_repository = fake_repository(Rc::clone(&failure_events));
        let mut failure_provider = FakeProvider {
            events: Rc::clone(&failure_events),
            outcome: Err(failure("provider_primary", 1)),
            request: None,
        };
        let failure_clock_events = Rc::clone(&failure_events);

        // expose any invalid failure-path clock read
        let mut failure_clock = || {
            failure_clock_events.borrow_mut().push("clock".to_owned());
            1_787_613_425_000
        };
        let failed = run_scheduled_source(
            scheduled_execution(&source, &configuration, None),
            &mut failure_repository,
            &mut failure_provider,
            &mut failure_clock,
        )
        .expect("provider failure should be contained");

        assert_eq!(failed.status, SourceRunStatus::Failed);

        // prove failure never reads the success clock
        assert!(!failure_events.borrow().iter().any(|event| event == "clock"));
    }

    // finalize a committed provider failure
    #[test]
    fn scheduled_run_preserves_provider_failure() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = fake_repository(Rc::clone(&events));
        let mut provider = FakeProvider {
            events: Rc::clone(&events),
            outcome: Err(failure("provider_primary", 3)),
            request: None,
        };
        let source = scheduled_open_meteo_source();
        let configuration = open_meteo_runtime_configuration();

        let result = run_scheduled_source(
            scheduled_execution(&source, &configuration, None),
            &mut repository,
            &mut provider,
            &mut || 1_787_613_425_000,
        )
        .expect("provider failure should be contained");

        assert_eq!(result.status, SourceRunStatus::Failed);
        assert_eq!(result.reason.as_deref(), Some("provider_primary"));
        assert_eq!(result.attempts, 3);
        assert_eq!(result.run_id.as_deref(), Some("run-1"));
        assert_eq!(
            events.borrow().as_slice(),
            [
                "lock",
                "abandon",
                "checkpoint",
                "start-committed",
                "fetch",
                "fail",
                "release"
            ],
        );
    }

    // retain fetched attempts when atomic completion fails afterward
    #[test]
    fn scheduled_run_finalizes_completion_failure_with_retained_batch_attempts() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = fake_repository(Rc::clone(&events));
        repository.complete_failure = Some(failure("completion_failed", 0));
        let mut batch = successful_batch();
        batch.attempts = 4;
        let mut provider = FakeProvider {
            events: Rc::clone(&events),
            outcome: Ok(batch),
            request: None,
        };
        let source = scheduled_open_meteo_source();
        let configuration = open_meteo_runtime_configuration();

        let result = run_scheduled_source(
            scheduled_execution(&source, &configuration, None),
            &mut repository,
            &mut provider,
            &mut || 1_787_613_425_000,
        )
        .expect("completion failure should be durably finalized");

        assert_eq!(result.status, SourceRunStatus::Failed);
        assert_eq!(result.reason.as_deref(), Some("completion_failed"));
        assert_eq!(result.attempts, 4);
        assert_eq!(result.record_count, 2);
        assert_eq!(
            repository.failure,
            Some(TaskFailure {
                attempts: 4,
                code: "completion_failed".to_owned(),
            }),
        );
        assert_eq!(
            events.borrow().as_slice(),
            [
                "lock",
                "abandon",
                "checkpoint",
                "start-committed",
                "fetch",
                "complete",
                "fail",
                "release",
            ],
        );
    }

    // retain primary and secondary failures
    #[test]
    fn scheduled_run_preserves_finalization_and_release_failures() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = fake_repository(Rc::clone(&events));
        repository.fail_finalization = true;
        repository.fail_release = true;
        let mut provider = FakeProvider {
            events,
            outcome: Err(failure("provider_primary", 1)),
            request: None,
        };
        let source = scheduled_open_meteo_source();
        let configuration = open_meteo_runtime_configuration();

        let result = run_scheduled_source(
            scheduled_execution(&source, &configuration, None),
            &mut repository,
            &mut provider,
            &mut || 1_787_613_425_000,
        )
        .expect("failures should be contained");

        assert_eq!(result.reason.as_deref(), Some("provider_primary"));
        assert_eq!(
            result.secondary_error_codes,
            ["finalization:finalization_failed", "release:release_failed"],
        );
        assert_eq!(result.status, SourceRunStatus::Failed);
    }

    // reject a successful empty provider batch
    #[test]
    fn scheduled_run_rejects_empty_payload() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = fake_repository(Rc::clone(&events));
        let mut provider = FakeProvider {
            events,
            outcome: Ok(ProviderBatch {
                attempts: 1,
                provider_cursor: None,
                records: Vec::new(),
                response_metadata: BTreeMap::new(),
                upstream_response_checksum: "checksum-empty".to_owned(),
            }),
            request: None,
        };
        let source = scheduled_open_meteo_source();
        let configuration = open_meteo_runtime_configuration();

        let result = run_scheduled_source(
            scheduled_execution(&source, &configuration, None),
            &mut repository,
            &mut provider,
            &mut || 1_787_613_425_000,
        )
        .expect("empty payload should be contained");

        assert_eq!(result.reason.as_deref(), Some("empty_payload"));
        assert_eq!(result.status, SourceRunStatus::Failed);
    }

    // skip a source locked elsewhere
    #[test]
    fn scheduled_run_skips_held_source_lock() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = fake_repository(Rc::clone(&events));
        repository.lock_available = false;
        let mut provider = FakeProvider {
            events,
            outcome: Ok(successful_batch()),
            request: None,
        };
        let source = scheduled_open_meteo_source();
        let configuration = open_meteo_runtime_configuration();

        let result = run_scheduled_source(
            scheduled_execution(&source, &configuration, None),
            &mut repository,
            &mut provider,
            &mut || 1_787_613_425_000,
        )
        .expect("held lock should skip");

        assert_eq!(result.status, SourceRunStatus::Skipped);
        assert_eq!(result.reason.as_deref(), Some("source lock is held"));
        assert_eq!(provider.request, None);
    }

    // reject inactive and mismatched identities before locking
    #[test]
    fn scheduled_run_requires_exact_runtime_identity_before_session() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = fake_repository(Rc::clone(&events));
        let mut provider = FakeProvider {
            events: Rc::clone(&events),
            outcome: Ok(successful_batch()),
            request: None,
        };
        let source = scheduled_open_meteo_source();
        let mut configuration = open_meteo_runtime_configuration();
        configuration.material_config_identity = "different-material".to_owned();

        let result = run_scheduled_source(
            scheduled_execution(&source, &configuration, None),
            &mut repository,
            &mut provider,
            &mut || 1_787_613_425_000,
        )
        .expect("mismatched source should skip");

        assert_eq!(result.status, SourceRunStatus::Skipped);
        assert_eq!(
            result.reason.as_deref(),
            Some("source is not an active configured scheduled source"),
        );
        assert!(events.borrow().is_empty());
        assert_eq!(provider.request, None);
    }

    // reject an exact identity without its kind capability
    #[test]
    fn scheduled_run_requires_kind_specific_configured_capability() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = fake_repository(Rc::clone(&events));
        let mut provider = FakeProvider {
            events: Rc::clone(&events),
            outcome: Ok(successful_batch()),
            request: None,
        };
        let source = scheduled_open_meteo_source();
        let mut configuration = open_meteo_runtime_configuration();
        configuration.capabilities = BTreeSet::from([
            OpenMeteoCapability::Forecast,
            OpenMeteoCapability::Historical,
        ]);

        let result = run_scheduled_source(
            scheduled_execution(&source, &configuration, None),
            &mut repository,
            &mut provider,
            &mut || 1_787_613_425_000,
        )
        .expect("unsupported source capability should skip");

        assert_eq!(result.status, SourceRunStatus::Skipped);
        assert_eq!(
            result.reason.as_deref(),
            Some("source is not an active configured scheduled source"),
        );
        assert!(events.borrow().is_empty());
        assert_eq!(provider.request, None);
    }

    // reject adapter drift before locking
    #[test]
    fn scheduled_run_requires_frozen_adapter_before_session() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut repository = fake_repository(Rc::clone(&events));
        let mut provider = FakeProvider {
            events: Rc::clone(&events),
            outcome: Ok(successful_batch()),
            request: None,
        };
        let mut source = scheduled_open_meteo_source();
        let mut configuration = open_meteo_runtime_configuration();
        source.adapter_version = OPEN_METEO_FORECAST_ADAPTER_VERSION;
        configuration.adapter_version = OPEN_METEO_FORECAST_ADAPTER_VERSION;

        let failure = run_scheduled_source(
            scheduled_execution(&source, &configuration, None),
            &mut repository,
            &mut provider,
            &mut || 1_787_613_425_000,
        )
        .expect_err("adapter drift should fail");

        assert_eq!(failure.code, "source adapter contract must match runtime");
        assert!(events.borrow().is_empty());
        assert_eq!(provider.request, None);
    }

    // drain ordered sources while isolating one unexpected source failure
    #[test]
    fn worker_iteration_preserves_order_continuation_and_clock_capture() {
        let mut repository = FakeIterationRepository {
            due_sources: vec![
                iteration_source("source-first"),
                iteration_source("source-failure"),
                iteration_source("source-after"),
            ],
            ..FakeIterationRepository::default()
        };
        let mut dispatcher = FakeIterationDispatcher {
            fail_source_id: Some("source-failure".to_owned()),
            visited: Vec::new(),
        };
        let mut continuation = WorkerContinuation::new(Some(90));
        let mut clocks = VecDeque::from([100, 110, 120, 130, 140, 150]);
        let mut captured_clocks = Vec::new();
        let result = run_worker_iteration(
            &mut continuation,
            &mut repository,
            &mut dispatcher,
            &mut || {
                let clock = clocks.pop_front().expect("iteration clock should exist");
                captured_clocks.push(clock);
                clock
            },
        )
        .expect("isolated source failure should not abort the worker iteration");

        assert_eq!(
            dispatcher.visited,
            ["source-first", "source-failure", "source-after"]
        );
        assert_eq!(captured_clocks, [100, 110, 120, 130, 140, 150]);
        assert!(clocks.is_empty());
        assert_eq!(repository.discover_at_ms, [110]);
        assert_eq!(result.completed_at_ms, 150);
        assert_eq!(result.last_success_at_ms, Some(140));
        assert_eq!(result.sources[0].status, SourceRunStatus::Succeeded);
        assert_eq!(result.sources[1].status, SourceRunStatus::Failed);
        assert_eq!(
            result.sources[1].reason.as_deref(),
            Some("worker_source_failure")
        );
        assert_eq!(
            result.sources[1].secondary_error_codes,
            ["dispatcher_failed"]
        );
        assert_eq!(result.sources[2].status, SourceRunStatus::Succeeded);
        assert_eq!(
            repository.heartbeats,
            [WorkerHeartbeat {
                activity: Some(WorkerActivity::Degraded),
                last_loop_at_ms: 150,
                last_success_at_ms: Some(140),
            }]
        );
    }

    // contain every optional task failure while preserving Node sidecar order and heartbeat health
    #[test]
    fn worker_iteration_runs_admitted_sidecars_after_sources_in_exact_order() {
        let mut repository = FakeIterationRepository::default();
        let mut dispatcher = FakeIterationDispatcher::default();
        let mut sidecars = FakeIterationSidecars {
            events: Vec::new(),
            fixed_gauge: Ok(FixedGaugeTargetResult::Gap {
                reason: "target_gap".to_owned(),
            }),
            inference: Err(failure("private inference detail", 3)),
            inference_now_ms: Vec::new(),
            maintenance: Err(failure("private maintenance detail", 1)),
            maintenance_due: Vec::new(),
            rain: Err(failure("private rain detail", 2)),
            temperature: Ok(TemperatureCanaryCollectionResult {
                failed_runs: 1,
                persisted_runs: 3,
            }),
        };
        let plan = WorkerSidecarPlan {
            adjustment_capture_epoch_configured: true,
            maintenance_capture_configured: true,
            rain_adjustment_revision_client_configured: true,
            rain_capture_configured: true,
            rain_inference_legacy_enabled: true,
            rain_inference_runtime: RainAdjustmentRuntimeSelection::Active,
            temperature_canary: VerifiedTemperatureCanaryCollection::Active,
        };
        let mut diagnostics = Vec::new();
        let mut continuation = WorkerContinuation::new(Some(90));
        let mut clocks = VecDeque::from([
            100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200, 210, 220, 230,
        ]);
        let result = run_worker_iteration_with_sidecars(
            &mut continuation,
            &mut repository,
            &mut dispatcher,
            WorkerSidecarExecution::new(plan, &mut sidecars, &mut diagnostics),
            &mut || clocks.pop_front().expect("sidecar clock should exist"),
        )
        .expect("optional sidecar failures must not abort the worker iteration");

        assert_eq!(repository.discover_at_ms, [110]);
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
        assert_eq!(sidecars.inference_now_ms, [190]);
        assert_eq!(
            sidecars.maintenance_due,
            [adjustment_maintenance_capture_due_at(110)
                .expect("fixture maintenance clock should be valid")]
        );
        assert_eq!(
            diagnostics,
            [
                WorkerSidecarDiagnostic {
                    count: 3,
                    duration_ms: 10,
                    error_code: Some("temperature_canary_collection_failed".to_owned()),
                    source_id: "ecmwf-temperature-canary",
                },
                WorkerSidecarDiagnostic {
                    count: 0,
                    duration_ms: 10,
                    error_code: Some("rain_collection_failed".to_owned()),
                    source_id: "rain-prospective-capture",
                },
                WorkerSidecarDiagnostic {
                    count: 0,
                    duration_ms: 10,
                    error_code: Some("target_gap".to_owned()),
                    source_id: "rain-fixed-gauge-target",
                },
                WorkerSidecarDiagnostic {
                    count: 0,
                    duration_ms: 20,
                    error_code: Some("rain_adjustment_failed".to_owned()),
                    source_id: "rain-adjustment",
                },
                WorkerSidecarDiagnostic {
                    count: 0,
                    duration_ms: 10,
                    error_code: Some("adjustment_maintenance_capture_failed".to_owned()),
                    source_id: "adjustment-maintenance-capture",
                },
            ]
        );
        assert!(clocks.is_empty());
        assert_eq!(result.completed_at_ms, 230);
        assert_eq!(result.last_success_at_ms, Some(90));
        assert!(result.sources.is_empty());
        assert_eq!(
            repository.heartbeats,
            [WorkerHeartbeat {
                activity: None,
                last_loop_at_ms: 230,
                last_success_at_ms: Some(90),
            }]
        );
    }

    // require the full fixed-target dependency set and exact rain runtime selections
    #[test]
    fn worker_sidecar_gates_do_not_invent_absent_task_success() {
        let mut repository = FakeIterationRepository::default();
        let mut dispatcher = FakeIterationDispatcher::default();
        let mut sidecars = FakeIterationSidecars {
            events: Vec::new(),
            fixed_gauge: Ok(FixedGaugeTargetResult::Published { revision_count: 4 }),
            inference: Ok(true),
            inference_now_ms: Vec::new(),
            maintenance: Ok(()),
            maintenance_due: Vec::new(),
            rain: Ok(RainCaptureResult {
                failed: 0,
                valid: 2,
            }),
            temperature: Ok(TemperatureCanaryCollectionResult {
                failed_runs: 0,
                persisted_runs: 1,
            }),
        };
        let plan = WorkerSidecarPlan {
            adjustment_capture_epoch_configured: true,
            rain_capture_configured: true,
            rain_inference_legacy_enabled: true,
            rain_inference_runtime: RainAdjustmentRuntimeSelection::Disabled,
            ..WorkerSidecarPlan::default()
        };
        let mut diagnostics = Vec::new();
        let mut continuation = WorkerContinuation::new(None);
        let mut clocks = VecDeque::from([100, 110, 120, 130, 140]);
        run_worker_iteration_with_sidecars(
            &mut continuation,
            &mut repository,
            &mut dispatcher,
            WorkerSidecarExecution::new(plan, &mut sidecars, &mut diagnostics),
            &mut || {
                clocks
                    .pop_front()
                    .expect("gated sidecar clock should exist")
            },
        )
        .expect("admitted rain capture should complete");

        assert_eq!(sidecars.events, ["rain"]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source_id, "rain-prospective-capture");
        assert!(clocks.is_empty());
        assert!(rain_adjustment_inference_is_active(
            RainAdjustmentRuntimeSelection::Unconfigured,
            true,
        ));
        assert!(rain_adjustment_inference_is_active(
            RainAdjustmentRuntimeSelection::Active,
            true,
        ));
        assert!(rain_adjustment_inference_is_active(
            RainAdjustmentRuntimeSelection::DisabledPolicyRawWithComparatorAuthority,
            true,
        ));
        assert!(!rain_adjustment_inference_is_active(
            RainAdjustmentRuntimeSelection::Disabled,
            true,
        ));
        assert!(!rain_adjustment_inference_is_active(
            RainAdjustmentRuntimeSelection::Active,
            false,
        ));
    }

    // preserve normal fixed-target idle as a zero-count non-error without changing heartbeat
    #[test]
    fn worker_fixed_gauge_idle_preserves_ticks_and_heartbeat() {
        let mut repository = FakeIterationRepository::default();
        let mut dispatcher = FakeIterationDispatcher::default();
        let mut sidecars = FakeIterationSidecars {
            events: Vec::new(),
            fixed_gauge: Ok(FixedGaugeTargetResult::Idle),
            inference: Ok(false),
            inference_now_ms: Vec::new(),
            maintenance: Ok(()),
            maintenance_due: Vec::new(),
            rain: Ok(RainCaptureResult {
                failed: 0,
                valid: 0,
            }),
            temperature: Ok(TemperatureCanaryCollectionResult {
                failed_runs: 0,
                persisted_runs: 0,
            }),
        };
        let plan = WorkerSidecarPlan {
            adjustment_capture_epoch_configured: true,
            rain_adjustment_revision_client_configured: true,
            rain_capture_configured: true,
            ..WorkerSidecarPlan::default()
        };
        let mut diagnostics = Vec::new();
        let mut continuation = WorkerContinuation::new(Some(90));
        let mut clocks = VecDeque::from([100, 110, 120, 130, 140, 150, 160]);
        let mut captured_clocks = Vec::new();
        let result = run_worker_iteration_with_sidecars(
            &mut continuation,
            &mut repository,
            &mut dispatcher,
            WorkerSidecarExecution::new(plan, &mut sidecars, &mut diagnostics),
            &mut || {
                let clock = clocks.pop_front().expect("idle sidecar clock should exist");
                captured_clocks.push(clock);
                clock
            },
        )
        .expect("normal fixed-target idle should complete the iteration");

        assert_eq!(sidecars.events, ["rain", "fixed-gauge"]);
        assert_eq!(captured_clocks, [100, 110, 120, 130, 140, 150, 160]);
        assert!(clocks.is_empty());
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(
            diagnostics[1],
            WorkerSidecarDiagnostic {
                count: 0,
                duration_ms: 10,
                error_code: None,
                source_id: "rain-fixed-gauge-target",
            }
        );
        assert_eq!(result.completed_at_ms, 160);
        assert_eq!(result.last_success_at_ms, Some(90));
        assert_eq!(
            repository.heartbeats,
            [WorkerHeartbeat {
                activity: None,
                last_loop_at_ms: 160,
                last_success_at_ms: Some(90),
            }]
        );
    }

    // abort before dispatch and heartbeat when discovery itself fails
    #[test]
    fn worker_iteration_aborts_on_discovery_failure() {
        let mut repository = FakeIterationRepository {
            discovery_failure: Some(failure("discovery_failed", 0)),
            due_sources: vec![iteration_source("source-never-runs")],
            ..FakeIterationRepository::default()
        };
        let mut dispatcher = FakeIterationDispatcher::default();
        let mut continuation = WorkerContinuation::new(Some(90));
        let mut clocks = VecDeque::from([100, 110]);
        let failure = run_worker_iteration(
            &mut continuation,
            &mut repository,
            &mut dispatcher,
            &mut || clocks.pop_front().expect("discovery clock should exist"),
        )
        .expect_err("discovery failure should abort the iteration");

        assert_eq!(failure.code, "discovery_failed");
        assert!(dispatcher.visited.is_empty());
        assert!(repository.heartbeats.is_empty());
        assert!(clocks.is_empty());
        assert_eq!(continuation.last_success_at_ms(), Some(90));
    }

    // reject overlapping work without claiming a completed iteration
    #[test]
    fn worker_iteration_gate_reports_busy_without_side_effects() {
        let gate = NonOverlappingGate::new();
        let _permit = gate
            .try_enter()
            .expect("fixture should hold the worker gate");
        let mut repository = FakeIterationRepository {
            due_sources: vec![iteration_source("source-never-runs")],
            ..FakeIterationRepository::default()
        };
        let mut dispatcher = FakeIterationDispatcher::default();
        let mut continuation = WorkerContinuation::new(Some(90));
        let mut clock_calls = 0;
        let result = run_worker_iteration_once(
            &gate,
            &mut continuation,
            &mut repository,
            &mut dispatcher,
            &mut || {
                clock_calls += 1;
                100
            },
        )
        .expect("busy iteration should be a bounded non-error result");

        assert_eq!(result, WorkerIterationRun::Busy);
        assert_eq!(clock_calls, 0);
        assert!(repository.discover_at_ms.is_empty());
        assert!(repository.heartbeats.is_empty());
        assert!(dispatcher.visited.is_empty());
    }

    // retain success before heartbeat persistence
    #[test]
    fn worker_continuation_retains_success_after_heartbeat_failure() {
        let mut continuation = WorkerContinuation::new(Some(1_787_292_000_000));
        let mut heartbeats = FakeHeartbeats {
            fail_next: true,
            updates: Vec::new(),
        };

        let failure = continuation
            .finish_iteration(
                1_787_378_400_000,
                vec![
                    source_result(SourceRunStatus::Succeeded),
                    source_result(SourceRunStatus::Failed),
                ],
                &mut heartbeats,
            )
            .expect_err("first heartbeat should fail");
        assert_eq!(failure.code, "heartbeat_failed");
        assert_eq!(continuation.last_success_at_ms(), Some(1_787_378_350_000));

        let next = continuation
            .finish_iteration(1_787_378_460_000, Vec::new(), &mut heartbeats)
            .expect("idle heartbeat should retry retained success");
        assert_eq!(next.last_success_at_ms, Some(1_787_378_350_000));
        assert_eq!(
            heartbeats.updates[1].last_success_at_ms,
            Some(1_787_378_350_000)
        );
    }

    // classify a failed iteration heartbeat
    #[test]
    fn worker_continuation_marks_failed_sources_degraded() {
        let mut continuation = WorkerContinuation::new(Some(1_787_292_000_000));
        let mut heartbeats = FakeHeartbeats::default();

        let result = continuation
            .finish_iteration(
                1_787_378_400_000,
                vec![source_result(SourceRunStatus::Failed)],
                &mut heartbeats,
            )
            .expect("degraded heartbeat should persist");

        assert_eq!(result.last_success_at_ms, Some(1_787_292_000_000));
        assert_eq!(
            heartbeats.updates[0].activity,
            Some(WorkerActivity::Degraded)
        );
    }

    // match the node current fixture exactly
    #[test]
    fn open_meteo_current_fixture_preserves_metrics_and_provenance() {
        let received_at_ms = 1_787_376_000_000;
        let sample = open_meteo_sample(
            1_787_374_800_000,
            [16.4, 15.8, 78.0, 0.0, 42.0, 999.0, 3.2, 5.4, 243.0, 1013.6],
        );

        let batch = normalize_open_meteo_current(
            open_meteo_envelope("America/Los_Angeles"),
            sample,
            "source-current",
            received_at_ms,
            2,
        )
        .expect("current fixture should normalize");

        assert_eq!(batch.adapter_version, OPEN_METEO_CURRENT_ADAPTER_VERSION);
        assert_eq!(batch.attempts, 2);
        assert_eq!(
            batch.provider_cursor,
            OpenMeteoCursor::ValidAt(1_787_374_800_000)
        );
        assert_eq!(batch.records.len(), 1);
        let record = &batch.records[0];
        assert_eq!(record.source_kind, OpenMeteoSourceKind::ModelCurrent);
        assert_eq!(record.product_run_at_ms, None);
        assert_eq!(record.received_at_ms, received_at_ms);
        assert_eq!(record.valid_at_ms, 1_787_374_800_000);
        assert_eq!(record.metrics.temperature_c, Some(16.4));
        assert_eq!(record.metrics.apparent_temperature_c, Some(15.8));
        assert_eq!(record.metrics.relative_humidity_percent, Some(78.0));
        assert_eq!(record.metrics.precipitation_mm, Some(0.0));
        assert_eq!(record.metrics.precipitation_rate_mm_per_hour, None);
        assert_eq!(record.metrics.cloud_cover_percent, Some(42.0));
        assert_eq!(record.metrics.wind_speed_mps, Some(3.2));
        assert_eq!(record.metrics.wind_gust_mps, Some(5.4));
        assert_eq!(record.metrics.wind_direction_degrees, Some(243.0));
        assert_eq!(record.metrics.pressure_hpa, Some(1013.6));
        assert_eq!(record.metrics.uv_index, None);
        assert_eq!(record.metrics.pm25_micrograms_per_cubic_meter, None);
        assert_open_meteo_provenance(record, "America/Los_Angeles");
    }

    // match the paired node forecast fixtures
    #[test]
    fn open_meteo_forecast_fixture_joins_air_quality_and_preserves_cursor() {
        let received_at_ms = 1_787_376_000_000;
        let weather = [
            open_meteo_sample(
                1_787_374_800_000,
                [12.1, 11.7, 83.0, 0.1, 72.0, 0.2, 1.8, 3.1, 180.0, 1018.1],
            ),
            open_meteo_sample(
                1_787_378_400_000,
                [12.8, 12.4, 80.0, 0.2, 68.0, 0.5, 2.1, 3.6, 185.0, 1018.3],
            ),
            open_meteo_sample(
                1_787_382_000_000,
                [13.4, 13.1, 77.0, 0.3, 61.0, 0.9, 2.4, 4.2, 190.0, 1018.5],
            ),
        ];
        let air_quality = [
            OpenMeteoAirQualitySample {
                pm25_micrograms_per_cubic_meter: Some(99.0),
                valid_at_ms: 1_787_371_200_000,
            },
            OpenMeteoAirQualitySample {
                pm25_micrograms_per_cubic_meter: Some(5.0),
                valid_at_ms: 1_787_374_800_000,
            },
            OpenMeteoAirQualitySample {
                pm25_micrograms_per_cubic_meter: Some(8.0),
                valid_at_ms: 1_787_378_400_000,
            },
            OpenMeteoAirQualitySample {
                pm25_micrograms_per_cubic_meter: Some(12.0),
                valid_at_ms: 1_787_382_000_000,
            },
        ];

        let batch = normalize_open_meteo_forecast(
            open_meteo_envelope("GMT"),
            &weather,
            &air_quality,
            "source-forecast",
            received_at_ms,
            2,
        )
        .expect("forecast fixtures should normalize");

        assert_eq!(batch.adapter_version, OPEN_METEO_FORECAST_ADAPTER_VERSION);
        assert_eq!(batch.attempts, 2);
        assert_eq!(
            batch.provider_cursor,
            OpenMeteoCursor::ProductRunAt(received_at_ms),
        );
        assert_eq!(batch.records.len(), 3);
        assert_eq!(
            batch
                .records
                .iter()
                .map(|record| record.metrics.pm25_micrograms_per_cubic_meter)
                .collect::<Vec<_>>(),
            [Some(5.0), Some(8.0), Some(12.0)],
        );
        assert_eq!(
            batch
                .records
                .iter()
                .map(|record| record.metrics.precipitation_rate_mm_per_hour)
                .collect::<Vec<_>>(),
            [Some(0.1), Some(0.2), Some(0.3)],
        );
        assert_eq!(
            batch.records[2].metrics,
            CanonicalWeatherMetrics {
                apparent_temperature_c: Some(13.1),
                black_globe_temperature_c: None,
                cloud_cover_percent: Some(61.0),
                pm25_micrograms_per_cubic_meter: Some(12.0),
                precipitation_mm: Some(0.3),
                precipitation_rate_mm_per_hour: Some(0.3),
                pressure_hpa: Some(1018.5),
                relative_humidity_percent: Some(77.0),
                soil_electrical_conductivity_microsiemens_per_cm: None,
                soil_moisture_percent: None,
                solar_radiation_wm2: None,
                temperature_c: Some(13.4),
                uv_index: Some(0.9),
                water_level_m: None,
                wind_direction_degrees: Some(190.0),
                wind_gust_mps: Some(4.2),
                wind_speed_mps: Some(2.4),
                wet_bulb_globe_temperature_c: None,
            },
        );
        assert_eq!(batch.records[2].product_run_at_ms, Some(received_at_ms));
        assert_eq!(batch.records[2].source_kind, OpenMeteoSourceKind::Forecast);
        assert_open_meteo_provenance(&batch.records[2], "GMT");
    }

    // collapse the provider north duplicate
    #[test]
    fn open_meteo_normalization_maps_wind_direction_360_to_zero() {
        let sample = open_meteo_sample(
            1_787_374_800_000,
            [16.4, 15.8, 78.0, 0.0, 42.0, 0.0, 3.2, 5.4, 360.0, 1013.6],
        );

        let batch = normalize_open_meteo_current(
            open_meteo_envelope("GMT"),
            sample,
            "source-current",
            1_787_376_000_000,
            1,
        )
        .expect("north duplicate should normalize");

        assert_eq!(batch.records[0].metrics.wind_direction_degrees, Some(0.0));
    }

    // reject malformed typed products
    #[test]
    fn open_meteo_normalization_rejects_empty_unordered_and_out_of_range_products() {
        let envelope = open_meteo_envelope("GMT");
        let mut first = open_meteo_sample(
            1_787_374_800_000,
            [12.1, 11.7, 83.0, 0.1, 72.0, 0.2, 1.8, 3.1, 180.0, 1018.1],
        );
        let air_quality = [OpenMeteoAirQualitySample {
            pm25_micrograms_per_cubic_meter: Some(5.0),
            valid_at_ms: first.valid_at_ms,
        }];

        assert_eq!(
            normalize_open_meteo_forecast(
                envelope.clone(),
                &[],
                &air_quality,
                "source-forecast",
                1_787_376_000_000,
                2,
            )
            .expect_err("empty weather should fail")
            .code,
            "invalid_payload",
        );
        assert_eq!(
            normalize_open_meteo_forecast(
                envelope.clone(),
                &[first, first],
                &air_quality,
                "source-forecast",
                1_787_376_000_000,
                2,
            )
            .expect_err("duplicate weather times should fail")
            .code,
            "invalid_payload",
        );
        first.relative_humidity_percent = Some(101.0);
        assert_eq!(
            normalize_open_meteo_current(envelope, first, "source-current", 1_787_376_000_000, 1,)
                .expect_err("out-of-range metric should fail")
                .code,
            "invalid_payload",
        );
    }
}
