//! Typed repository plans and row projections for the Weather database.
//!
//! PostgreSQL transport remains outside this crate until the selected database
//! dependency is approved. This layer owns exact SQL, bind shape, and typed row
//! projection without implementing a database protocol or applying schema DDL.

mod ambient_backfill;
mod api_maintenance_prediction_write;
mod atomic_migration;
mod bootstrap;
mod bootstrap_public_tide;
mod bootstrap_sensors;
mod bootstrap_site;
mod causal_temperature;
mod checked_migration_execution;
mod config;
mod daily_precipitation;
mod ecmwf_canary_persistence;
mod forecast;
mod forecast_anchor_backfill;
mod forecast_observation;
mod health_reads;
mod history;
mod maintenance_publication;
mod migration_artifacts;
mod migrations;
mod open_meteo_backfill_source;
mod process_config;
mod rain_capture_claim;
mod rain_capture_receipt;
mod rain_serving;
mod revision_archive;
mod scheduled_ingestion;
mod scheduled_runs;
mod source_session;
mod temperature_canary_reads;
mod tempest_backfill;
mod tide_backfill;
mod tides;
mod trends;
mod worker_iteration_queries;
mod worker_rain_adjustment_append;
mod worker_rain_adjustment_read;
mod worker_rain_adjustment_revision;
mod worker_rain_fixed_gauge_read;
mod worker_rain_fixed_gauge_write;
mod worker_readiness;
mod worker_revision_archive_write;

pub use api_maintenance_prediction_write::*;
pub use atomic_migration::*;
pub use bootstrap::*;
pub use bootstrap_public_tide::*;
pub use bootstrap_sensors::*;
pub use bootstrap_site::*;
pub use checked_migration_execution::*;
pub use migration_artifacts::*;
pub use open_meteo_backfill_source::*;
pub use process_config::*;
pub use worker_iteration_queries::*;
pub use worker_rain_adjustment_append::*;
pub use worker_rain_adjustment_read::*;
pub use worker_rain_adjustment_revision::*;
pub use worker_rain_fixed_gauge_read::*;
pub use worker_rain_fixed_gauge_write::*;
pub use worker_readiness::*;
pub use worker_revision_archive_write::*;

pub use ambient_backfill::*;
pub use causal_temperature::{
    AdmittedCausalEcmwfPriorHoursQuery, AdmittedCausalForecastObservationQuery,
    CAUSAL_ECMWF_TEMPERATURE_PRIOR_HOURS_SQL, CAUSAL_FORECAST_OBSERVATION_HOURLY_STATIONS_SQL,
    CAUSAL_FORECAST_OBSERVATION_SOURCE_KEYS, CausalEcmwfPriorHourDbRow,
    CausalEcmwfPriorHoursReadPlan, CausalForecastObservationReadPlan, CausalTemperatureReadBind,
    CausalTemperatureReadPlanError, CausalTemperatureStationReadRow,
    causal_ecmwf_prior_hours_read_plan, causal_forecast_observation_read_plan,
    project_causal_ecmwf_prior_hours, project_causal_temperature_station_rows,
};
pub use config::{
    DatabaseConfiguration, DatabaseConfigurationError, DatabaseConfigurationInput,
    DatabasePasswordLoader, DatabasePoolConfiguration, load_database_configuration,
    to_pool_configuration,
};
pub use daily_precipitation::{
    AdmittedDailyPrecipitationQuery, DAILY_PRECIPITATION_SQL, DailyPrecipitationBind,
    DailyPrecipitationReadPlan, DailyPrecipitationReadPlanError, DailyPrecipitationResult,
    daily_precipitation_read_plan, project_daily_precipitation_row,
};
pub use ecmwf_canary_persistence::{
    AdmittedEcmwfCanaryBestMatchRow, AdmittedEcmwfCanaryRevisionWindow,
    AdmittedEcmwfCanaryRunPersistence, ECMWF_CANARY_BEST_MATCH_SQL,
    ECMWF_CANARY_EXISTING_HOURS_SQL, ECMWF_CANARY_EXISTING_RUN_SQL, ECMWF_CANARY_INSERT_HOUR_SQL,
    ECMWF_CANARY_INSERT_RUN_SQL, ECMWF_CANARY_LOGICAL_RECEIPT_SQL, ECMWF_CANARY_SITE_SQL,
    ECMWF_CANARY_UPDATE_RECEIPT_SQL, EcmwfCanaryBestMatchQuery, EcmwfCanaryExistingHoursQuery,
    EcmwfCanaryExistingRunQuery, EcmwfCanaryHourInsertQuery, EcmwfCanaryLogicalReceiptQuery,
    EcmwfCanaryPersistenceBind, EcmwfCanaryPersistenceError, EcmwfCanaryPersistenceInputError,
    EcmwfCanaryPersistencePort, EcmwfCanaryReceiptUpdateQuery, EcmwfCanaryRevisionArchivePort,
    EcmwfCanaryRevisionSavepointCommand, EcmwfCanaryRunInsertQuery, EcmwfCanarySiteQuery,
    EcmwfCanarySiteRow, PreparedEcmwfCanaryHour, RELEASE_ECMWF_CANARY_REVISION_SQL,
    ROLLBACK_ECMWF_CANARY_REVISION_SQL, SAVEPOINT_ECMWF_CANARY_REVISION_SQL,
    StoredEcmwfCanaryHourIdentity, StoredEcmwfCanaryRun, persist_ecmwf_temperature_canary_run,
    persist_ecmwf_temperature_canary_run_with_revision_archive,
};
pub use forecast::{
    AdmittedForecastPressureContextQuery, AdmittedForecastQuery, FORECAST_PROVENANCE_SQL,
    ForecastPressureContextBind, ForecastPressureContextPlan, ForecastPressureContextPlanError,
    ForecastPressureContextResult, ForecastProvenanceBind, ForecastProvenancePlan,
    ForecastReadBind, ForecastReadPlan, ForecastReadPlanError, ForecastRuntimeProvenanceRow,
    decorate_forecast_provenance, forecast_pressure_context_plan, forecast_provenance_plan,
    forecast_read_plan, project_forecast_pressure_context_rows,
};
pub use forecast_anchor_backfill::{
    ABANDON_EXPIRED_FORECAST_ANCHOR_RUNS_SQL, ASSERT_FORECAST_ANCHOR_BACKFILL_RUN_SQL,
    AdmittedForecastAnchorBackfillIdentity, CompleteForecastAnchorBackfill,
    FINALIZE_FAILED_FORECAST_ANCHOR_RUN_SQL, FINALIZE_SUCCESSFUL_FORECAST_ANCHOR_RUN_SQL,
    FIND_RUNNING_FORECAST_ANCHOR_RUN_SQL, FailForecastAnchorBackfill,
    ForecastAnchorAbandonExpiredQuery, ForecastAnchorBackfillCommandPort,
    ForecastAnchorBackfillExecutionError, ForecastAnchorBackfillGuard,
    ForecastAnchorBackfillInputError, ForecastAnchorBackfillPoolPort, ForecastAnchorFailureBind,
    ForecastAnchorFailureQuery, ForecastAnchorFindRunningBind, ForecastAnchorFindRunningQuery,
    ForecastAnchorIdentityBind, ForecastAnchorOutcome, ForecastAnchorOutcomeBind,
    ForecastAnchorOutcomeQuery, ForecastAnchorRecordBind, ForecastAnchorRecordStorageEvidence,
    ForecastAnchorRecordUpsertQuery, ForecastAnchorRunIdentityQuery, ForecastAnchorRunStartBind,
    ForecastAnchorRunStartQuery, ForecastAnchorRunningLookup, ForecastAnchorSourceKindBind,
    ForecastAnchorSuccessBind, ForecastAnchorSuccessQuery, ForecastAnchorSuccessfulChunkQuery,
    HAS_SUCCESSFUL_FORECAST_ANCHOR_CHUNK_SQL, INSERT_FORECAST_ANCHOR_BACKFILL_RUN_SQL,
    PreparedForecastAnchorRecord, StartForecastAnchorBackfillRun, StartedForecastAnchorBackfillRun,
    UPSERT_FORECAST_ANCHOR_OUTCOME_SQL, complete_forecast_anchor_backfill,
    fail_forecast_anchor_backfill, forecast_anchor_abandon_expired_query,
    forecast_anchor_failure_query, forecast_anchor_outcome_query,
    forecast_anchor_record_upsert_query, forecast_anchor_run_identity_query,
    forecast_anchor_run_start_query, forecast_anchor_success_query,
    forecast_anchor_successful_chunk_query, start_forecast_anchor_backfill_run,
};
pub use forecast_observation::{
    AdmittedForecastObservationQuery, FORECAST_OBSERVATION_INPUT_SQL, ForecastObservationInputBind,
    ForecastObservationInputDbRow, ForecastObservationInputProjectionError,
    ForecastObservationInputReadPlan, forecast_observation_input_read_plan,
    project_forecast_observation_input_rows,
};
pub use health_reads::{
    AdmittedHealthMigrationAuthorization, AdmittedHealthMigrationIdentity,
    HEALTH_LATEST_WORKER_HEARTBEAT_SQL, HEALTH_MIGRATION_LEDGER_SQL, HEALTH_PING_SQL,
    HealthLatestWorkerHeartbeatReadPlan, HealthLatestWorkerHeartbeatRow,
    HealthMigrationLedgerReadPlan, HealthMigrationReadiness, HealthMigrationReadinessError,
    HealthPingReadPlan, HealthReadBind, health_latest_worker_heartbeat_read_plan,
    health_migration_ledger_read_plan, health_ping_read_plan, verify_health_migration_readiness,
};
pub use history::{
    AdmittedHistoryQuery, HistoryReadBind, HistoryReadPlan, HistoryReadPlanError,
    WeatherHistoryResult, project_weather_history_rows, weather_history_read_plan,
};
pub use maintenance_publication::{
    AdjustmentMaintenanceDatabasePort, AdjustmentRevisionAdmissionKey, AdjustmentRevisionGapMarker,
    MaintenancePublicationBind, MaintenancePublicationPlan, adjustment_revision_gap_plan,
    adjustment_revision_publication_plan, adjustment_shadow_publication_plans,
};
pub use migrations::{
    AppliedMigration, MIGRATION_ADVISORY_LOCK_KEY, MigrationCommandPort, MigrationDigest,
    MigrationExecutionError, MigrationExecutionResult, MigrationPlan, MigrationPlanningError,
    MigrationScript, execute_migrations, plan_migrations,
};
pub use rain_capture_claim::*;
pub use rain_capture_receipt::*;
pub use rain_serving::{
    RAIN_ADJUSTMENT_RUN_READ_SQL, RAIN_COLLECTION_STATUS_READ_SQL, RainAdjustmentRunReadPlan,
    RainAdjustmentRunReadRequest, RainAdjustmentRunReadRow, RainCollectionStatusReadPlan,
    RainCollectionStatusReadRow, RainServingReadBind, RainServingReadError, RainServingReadPort,
    rain_adjustment_run_read_plan, rain_collection_status_read_plan, read_rain_adjustment_run,
    read_rain_collection_status,
};
pub use revision_archive::{
    AdmittedRevisionUtcInstant, ChangedWeatherRevisionInput, CommittedWeatherRevisionPublications,
    ExistingWeatherRevisionIdentity, PersistedWeatherAdjustmentRevision,
    RELEASE_WEATHER_REVISION_ARCHIVE_SQL, ROLLBACK_WEATHER_REVISION_ARCHIVE_SQL,
    SAVEPOINT_WEATHER_REVISION_ARCHIVE_SQL, SELECT_EXISTING_WEATHER_REVISIONS_SQL,
    StagedWeatherRevisionPublications, WEATHER_REVISION_SOURCE_CONTEXT_SQL,
    WeatherRevisionArchiveError, WeatherRevisionArchivePort, WeatherRevisionArchiveSelection,
    WeatherRevisionContextBind, WeatherRevisionContextQuery, WeatherRevisionLookupRequest,
    WeatherRevisionSavepointCommand, WeatherRevisionSelectionBind, WeatherRevisionSelectionQuery,
    WeatherRevisionSourceContext, publish_committed_weather_revisions,
    select_changed_weather_revision_inputs, stage_changed_weather_revisions,
    weather_revision_context_query, weather_revision_selection_query,
};
pub use scheduled_ingestion::{
    ASSERT_SCHEDULED_RUN_SQL, CheckpointUpdateBind, CompleteScheduledIngestion,
    FINALIZE_SCHEDULED_RUN_SQL, FinalizeScheduledRunBind, FinalizeScheduledRunQuery,
    INSERT_SCHEDULED_CHECKPOINT_SQL, InitialCheckpointBind, PreparedWeatherRecord,
    ScheduledCheckpointQuery, ScheduledIngestionCommandPort, ScheduledIngestionExecutionError,
    ScheduledIngestionGuard, ScheduledIngestionInputError, ScheduledRunIdentityBind,
    ScheduledRunIdentityQuery, SerializedJson, Sha256Digest, UPDATE_SCHEDULED_CHECKPOINT_SQL,
    WeatherRecordBind, WeatherRecordStorageEvidence, WeatherRecordUpsertQuery,
    complete_scheduled_ingestion, complete_scheduled_ingestion_with_revision_archive,
    finalize_scheduled_run_query, scheduled_checkpoint_query, scheduled_run_identity_query,
    weather_record_upsert_query,
};
pub use scheduled_runs::{
    ASSERT_RUNNING_SCHEDULED_MODE_SQL, AdmittedAdapterVersion, AdmittedIngestionError,
    AdmittedScheduledRunTimes, AssertRunningScheduledBind, AssertRunningScheduledQuery,
    FINALIZE_FAILED_INGESTION_SQL, FIND_RUNNING_INGESTION_SQL, FailScheduledIngestion,
    FinalizeFailedIngestionBind, FinalizeFailedIngestionQuery, FindRunningIngestionBind,
    FindRunningIngestionQuery, GET_SCHEDULED_CHECKPOINT_SQL, GetScheduledCheckpointBind,
    GetScheduledCheckpointQuery, INSERT_SCHEDULED_INGESTION_RUN_SQL, InsertScheduledIngestionBind,
    InsertScheduledIngestionQuery, RunningIngestionLookup, ScheduledCheckpointDbRow,
    ScheduledCheckpointReadError, ScheduledCheckpointReadPort, ScheduledRunFailureCommandPort,
    ScheduledRunFailureError, ScheduledRunFailureGuard, ScheduledRunStartCommandPort,
    ScheduledRunStartError, StartScheduledIngestion, StartedScheduledIngestion,
    assert_running_scheduled_query, fail_scheduled_ingestion, finalize_failed_ingestion_query,
    find_running_ingestion_query, get_scheduled_checkpoint, insert_scheduled_ingestion_query,
    start_scheduled_ingestion,
};
pub use source_session::RetainedSourceSession;
pub use temperature_canary_reads::{
    AdmittedEcmwfTemperatureCanarySidecarQuery, AdmittedEcmwfTemperatureCanaryStatusQuery,
    ECMWF_TEMPERATURE_CANARY_RUN_INITIALIZATIONS_SQL, ECMWF_TEMPERATURE_CANARY_SIDECAR_HOURS_SQL,
    ECMWF_TEMPERATURE_CANARY_SIDECAR_RUN_SQL, ECMWF_TEMPERATURE_CANARY_STATUS_SQL,
    EcmwfTemperatureCanaryHourReadRow, EcmwfTemperatureCanaryReadBind,
    EcmwfTemperatureCanaryReadError, EcmwfTemperatureCanaryReadPlanError,
    EcmwfTemperatureCanaryReadPort, EcmwfTemperatureCanaryRunInitializationsReadPlan,
    EcmwfTemperatureCanarySidecarHoursReadPlan, EcmwfTemperatureCanarySidecarRunReadPlan,
    EcmwfTemperatureCanaryStatusReadPlan, EcmwfTemperatureCanaryStatusReadRow,
    ecmwf_temperature_canary_run_initializations_read_plan,
    ecmwf_temperature_canary_sidecar_hours_read_plan,
    ecmwf_temperature_canary_sidecar_run_read_plan, ecmwf_temperature_canary_status_read_plan,
    get_ecmwf_temperature_canary_sidecar, get_ecmwf_temperature_canary_status,
};
pub use tempest_backfill::*;
pub use tide_backfill::{
    ABANDON_EXPIRED_TIDE_BACKFILL_RUNS_SQL, ASSERT_TIDE_BACKFILL_RUN_SQL,
    AdmittedTideBackfillIdentity, CompleteTideBackfill, FINALIZE_FAILED_TIDE_BACKFILL_RUN_SQL,
    FINALIZE_SUCCESSFUL_TIDE_BACKFILL_RUN_SQL, FIND_RUNNING_TIDE_BACKFILL_RUN_SQL,
    FailTideBackfill, HAS_SUCCESSFUL_TIDE_BACKFILL_CHUNK_SQL, INSERT_TIDE_BACKFILL_RUN_SQL,
    StartTideBackfillRun, StartedTideBackfillRun, TideBackfillAbandonExpiredQuery,
    TideBackfillCommandPort, TideBackfillExecutionError, TideBackfillFailureBind,
    TideBackfillFailureQuery, TideBackfillFindRunningBind, TideBackfillFindRunningQuery,
    TideBackfillGuard, TideBackfillIdentityBind, TideBackfillInputError, TideBackfillOutcome,
    TideBackfillOutcomeBind, TideBackfillOutcomeQuery, TideBackfillPoolPort,
    TideBackfillRunIdentityQuery, TideBackfillRunStartBind, TideBackfillRunStartQuery,
    TideBackfillRunningLookup, TideBackfillSuccessBind, TideBackfillSuccessQuery,
    TideBackfillSuccessfulChunkQuery, UPSERT_TIDE_BACKFILL_OUTCOME_SQL, complete_tide_backfill,
    fail_tide_backfill, start_tide_backfill_run, tide_backfill_abandon_expired_query,
    tide_backfill_failure_query, tide_backfill_outcome_query, tide_backfill_run_identity_query,
    tide_backfill_run_start_query, tide_backfill_success_query,
    tide_backfill_successful_chunk_query,
};
pub use tides::{
    AdmittedTideQuery, TIDE_RECORDS_SQL, TideReadBind, TideReadPlan, TideReadPlanError,
    TideRecordResult, project_tide_record_rows, tide_read_plan,
};
pub use trends::{
    AdmittedTrendQuery, TREND_POINTS_SQL, TrendPointResult, TrendReadBind, TrendReadPlan,
    TrendReadPlanError, project_trend_point_rows, trend_read_plan,
};

use weather_contracts::{AdjustmentShadowFamily, CurrentWeatherRecordRow, SourceKind, StationKind};

/// The exact source-owned active-site discovery statement.
pub const LIST_ACTIVE_SITES_SQL: &str = include_str!("../fixtures/list-active-sites.sql");
/// The exact source-owned current-weather statement.
pub const CURRENT_WEATHER_SQL: &str = include_str!("../fixtures/current-weather.sql");
/// exact source-owned schedule-slot statement
pub const ADJUSTMENT_SHADOW_REGISTRATION_SLOT_SQL: &str =
    "SELECT adjustment_shadow_registration_slot_v3($1::text) AS value";

/// One zero-bind active-site discovery query plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListActiveSitesQuery {
    pub sql: &'static str,
    pub binds: [ListActiveSitesBind; 0],
}

/// The closed bind domain for the intentionally zero-bind discovery query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListActiveSitesBind {}

/// One typed current-weather input after application validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentWeatherQuery {
    pub site_slug: String,
    pub station_slug: Option<String>,
    pub source_id: Option<String>,
    pub as_of: String,
}

/// The four PostgreSQL values in exact placeholder order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurrentWeatherBind {
    Text(String),
    NullableText(Option<String>),
    UtcInstant(String),
}

/// One exact current-weather query and its typed binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentWeatherQueryPlan {
    pub sql: &'static str,
    pub binds: [CurrentWeatherBind; 4],
}

/// one typed schedule-slot bind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentShadowRegistrationSlotBind {
    Family(AdjustmentShadowFamily),
}

/// exact schedule-slot query plan
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdjustmentShadowRegistrationSlotQueryPlan {
    pub sql: &'static str,
    pub binds: [AdjustmentShadowRegistrationSlotBind; 1],
}

/// The generated TypeScript-canonical result boundary for current rows.
#[derive(Debug, Clone, PartialEq)]
pub struct CurrentWeatherResult {
    pub rows: Vec<CurrentWeatherRecordRow>,
}

/// One raw row after PostgreSQL alias and scalar decoding.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveSiteDbRow {
    pub attribution_label: String,
    pub attribution_url: String,
    pub latitude: f64,
    pub longitude: f64,
    pub provider_key: String,
    pub provider_name: String,
    pub site_name: String,
    pub site_slug: String,
    pub source_id: String,
    pub source_key: String,
    pub source_kind: String,
    pub station_kind: String,
    pub station_latitude: f64,
    pub station_longitude: f64,
    pub station_name: String,
    pub station_slug: String,
    pub timezone: String,
}

/// One typed active-site row returned to the application repository boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveSiteRow {
    pub attribution_label: String,
    pub attribution_url: String,
    pub latitude: f64,
    pub longitude: f64,
    pub provider_key: String,
    pub provider_name: String,
    pub site_name: String,
    pub site_slug: String,
    pub source_id: String,
    pub source_key: String,
    pub source_kind: SourceKind,
    pub station_kind: StationKind,
    pub station_latitude: f64,
    pub station_longitude: f64,
    pub station_name: String,
    pub station_slug: String,
    pub timezone: String,
}

/// A bounded typed-row projection failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionError {
    pub field: &'static str,
    pub value: String,
}

/// Return the exact no-parameter discovery query.
pub const fn list_active_sites_query() -> ListActiveSitesQuery {
    ListActiveSitesQuery {
        sql: LIST_ACTIVE_SITES_SQL,
        binds: [],
    }
}

/// Build the exact current-weather statement and bind order.
///
/// `as_of` is the canonical UTC instant already normalized by the shared time
/// validator; the eventual database adapter binds it as `timestamptz`.
pub fn current_weather_query(query: CurrentWeatherQuery) -> CurrentWeatherQueryPlan {
    CurrentWeatherQueryPlan {
        sql: CURRENT_WEATHER_SQL,
        binds: [
            CurrentWeatherBind::Text(query.site_slug),
            CurrentWeatherBind::NullableText(query.station_slug),
            CurrentWeatherBind::NullableText(query.source_id),
            CurrentWeatherBind::UtcInstant(query.as_of),
        ],
    }
}

/// build one schedule-slot read
pub const fn adjustment_shadow_registration_slot_query(
    family: AdjustmentShadowFamily,
) -> AdjustmentShadowRegistrationSlotQueryPlan {
    AdjustmentShadowRegistrationSlotQueryPlan {
        sql: ADJUSTMENT_SHADOW_REGISTRATION_SLOT_SQL,
        binds: [AdjustmentShadowRegistrationSlotBind::Family(family)],
    }
}

/// Preserve generated nullable, decimal, and timing row values after decoding.
pub fn project_current_weather_rows(rows: Vec<CurrentWeatherRecordRow>) -> CurrentWeatherResult {
    CurrentWeatherResult { rows }
}

/// Project one decoded row through the generated TypeScript contract enums.
pub fn project_active_site_row(raw: ActiveSiteDbRow) -> Result<ActiveSiteRow, ProjectionError> {
    let source_kind = parse_source_kind(&raw.source_kind)?;
    let station_kind = parse_station_kind(&raw.station_kind)?;

    Ok(ActiveSiteRow {
        attribution_label: raw.attribution_label,
        attribution_url: raw.attribution_url,
        latitude: raw.latitude,
        longitude: raw.longitude,
        provider_key: raw.provider_key,
        provider_name: raw.provider_name,
        site_name: raw.site_name,
        site_slug: raw.site_slug,
        source_id: raw.source_id,
        source_key: raw.source_key,
        source_kind,
        station_kind,
        station_latitude: raw.station_latitude,
        station_longitude: raw.station_longitude,
        station_name: raw.station_name,
        station_slug: raw.station_slug,
        timezone: raw.timezone,
    })
}

/// Project ordered decoded rows without reordering repository output.
pub fn project_active_site_rows(
    rows: Vec<ActiveSiteDbRow>,
) -> Result<Vec<ActiveSiteRow>, ProjectionError> {
    let mut projected = Vec::with_capacity(rows.len());

    // preserve the SQL order while projecting every row
    for row in rows {
        projected.push(project_active_site_row(row)?);
    }

    Ok(projected)
}

/// Parse one source kind from the canonical generated wire values.
fn parse_source_kind(value: &str) -> Result<SourceKind, ProjectionError> {
    // reuse the generated TypeScript-to-Rust contract table
    for (wire_value, source_kind) in SourceKind::WIRE_VALUES {
        // return the exact generated enum variant
        if value == *wire_value {
            return Ok(*source_kind);
        }
    }

    Err(ProjectionError {
        field: "sourceKind",
        value: value.to_owned(),
    })
}

/// Parse one station kind from the canonical generated wire values.
fn parse_station_kind(value: &str) -> Result<StationKind, ProjectionError> {
    // reuse the generated TypeScript-to-Rust contract table
    for (wire_value, station_kind) in StationKind::WIRE_VALUES {
        // return the exact generated enum variant
        if value == *wire_value {
            return Ok(*station_kind);
        }
    }

    Err(ProjectionError {
        field: "stationKind",
        value: value.to_owned(),
    })
}
