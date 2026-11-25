//! Pure request dispatch for the Weather API.
//!
//! HTTP parsing, JSON serialization, and I/O stay in the eventual transport
//! adapter. This crate owns the application route, method, and query policy.

mod current_projection;
mod daily_precipitation;
mod forecast_observation;
mod forecast_projection;
mod forecast_read;
mod forecast_startup_refusal;
mod history;
mod maintenance_comparator_body;
mod maintenance_prediction_body;
mod maintenance_projection;
mod maintenance_repository;
mod maintenance_service;
mod maintenance_source_projection;
mod maintenance_temperature_model;
mod maintenance_temperature_model_causality;
mod maintenance_temperature_projection;
mod maintenance_temperature_recent_error_admission;
mod maintenance_wind_model;
mod maintenance_wind_projection;
mod maintenance_wire;
mod rain_collection;
mod rain_forecast;
mod rain_portable_artifact;
#[cfg(test)]
mod rain_portable_artifact_test;
mod service;
mod service_wire;
mod standard_adjustment_startup;
mod startup;
mod temperature_model;
mod temperature_projection;
mod temperature_startup;
mod tides;
mod trends;
mod wind_maintenance_startup;
mod wind_model;
mod wind_repository_admission;
mod wind_startup;

pub use current_projection::{
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver, project_current_weather_response,
};
pub use daily_precipitation::{
    ResolvedDailyPrecipitationWindow, daily_precipitation_query,
    project_daily_precipitation_response,
};
pub use forecast_observation::{
    FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES, ForecastObservationGeometryBoundary,
    ForecastObservationInputResponse, ForecastObservationInputResponseContractVersionLiteral,
    ForecastObservationInputResponseMetrics, ForecastObservationInputResponseProvenance,
    ForecastObservationInputResponseRecord, ForecastObservationInputResponseSite,
    ResolvedForecastObservationGeometry, project_forecast_observation_input,
};
pub use forecast_projection::{
    AdmittedWindCanaryEvidence, AdmittedWindCanaryRuntime, AdmittedWindCorrections,
    AdmittedWindRowOutput, InactiveForecastDecisionConfig, project_forecast_pressure_context,
    project_forecast_weather_records, project_forecast_weather_records_with_wind_adjustments,
    project_wind_canary_runtime,
};
pub use forecast_read::{
    ForecastReadError, ResolvedForecastPressureContextWindow, ResolvedForecastReadWindow,
    forecast_endpoint_read_plan, forecast_pressure_context_read_plan,
};
pub use forecast_startup_refusal::{ForecastStartupRefusal, InactiveForecastServiceRuntime};
pub use history::{
    AdmittedHistoryCursor, HistoryBoundaryAdapter, HistoryBoundaryError, HistoryQueryTime,
    ParsedHistoryQuery, history_read_plan, parse_history_query, project_history_response,
};
pub use maintenance_comparator_body::*;
pub use maintenance_prediction_body::*;
pub use maintenance_projection::{
    MaintenanceScheduleSlotError, maintenance_schedule_slot_reads,
    project_maintenance_schedule_slots,
};
pub use maintenance_repository::{
    DatabaseAdjustmentMaintenanceRepository, ShadowPublishAdmissionError,
};
pub use maintenance_service::{
    AdjustmentMaintenanceAdmissionBoundary, AdjustmentMaintenanceBodyError,
    AdjustmentMaintenanceBodyRead, AdjustmentMaintenanceCaptureCallback,
    AdjustmentMaintenanceCaptureCycle, AdjustmentMaintenanceErrorCode,
    AdjustmentMaintenanceGapCallback, AdjustmentMaintenanceListenerOwner,
    AdjustmentMaintenanceOperationError, AdjustmentMaintenancePublicationRelay,
    AdjustmentMaintenancePublishRepository, AdjustmentMaintenanceRelayPorts,
    AdjustmentMaintenanceRepository, AdjustmentMaintenanceRequestClass,
    AdjustmentMaintenanceResponse, AdjustmentMaintenanceResponseBody,
    AdjustmentMaintenanceScheduleRepository, AdjustmentMaintenanceServicePorts,
    AdjustmentMaintenanceServiceRelay, AdjustmentRevisionPublicationPointer,
    AdjustmentRevisionPublishResult, AdmittedAdjustmentMaintenanceAuthority,
    AdmittedAdjustmentMaintenanceCapture, AdmittedAdjustmentMaintenanceGap,
    AdmittedAdjustmentMaintenancePublish, AdmittedAdjustmentMaintenanceStage,
    AdmittedAdjustmentRevisionGap, AdmittedAdjustmentRevisionPublish,
    AdmittedAdjustmentRevisionStage, AdmittedRainFixedGaugeTargetGap,
    AdmittedRainFixedGaugeTargetGapStatus, AdmittedRainMaintenanceControlStateGap,
    AdmittedRainMaintenanceControlStateStage, DecodedAdjustmentMaintenancePublish,
    DecodedAdjustmentMaintenanceRequest, DecodedAdjustmentMaintenanceStage,
    DecodedAdjustmentRevisionPublish, DecodedAdjustmentRevisionStage,
    DecodedRainMaintenanceControlStateStage, MAINTENANCE_CAPTURE_PATH, MAINTENANCE_GAP_PATH,
    MAINTENANCE_PUBLISH_PATH, MAINTENANCE_RAIN_CONTROL_STATE_GAP_PATH,
    MAINTENANCE_RAIN_CONTROL_STATE_STAGE_PATH, MAINTENANCE_RAIN_FIXED_GAUGE_TARGET_GAP_PATH,
    MAINTENANCE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_PATH, MAINTENANCE_REVISION_GAP_PATH,
    MAINTENANCE_REVISION_PUBLISH_PATH, MAINTENANCE_REVISION_STAGE_PATH,
    MAINTENANCE_SCHEDULE_SLOTS_PATH, MAINTENANCE_SHADOW_STAGE_PATH,
    MAX_ADJUSTMENT_MAINTENANCE_REQUEST_BYTES, PreparedAdjustmentMaintenancePublish,
    PreparedAdjustmentRevisionPublish, UnifiedAdjustmentMaintenanceServicePorts,
    classify_adjustment_maintenance_request, serve_adjustment_maintenance_request,
    serve_adjustment_maintenance_request_with_repository,
};
pub use maintenance_source_projection::*;
pub use maintenance_temperature_model::*;
pub use maintenance_wind_model::*;
pub use maintenance_wire::*;
pub use rain_collection::{RainCollectionTimestampBoundary, project_rain_collection_status};
pub use rain_forecast::{
    AdmittedRainAdjustmentHour, AdmittedRainAdjustmentRun, AdmittedRainForecastRuntime,
    admit_rain_forecast_runtime, project_rain_adjustment_runtime, project_rain_adjustments,
    rain_adjustment_model_enabled, replace_rain_adjustments, valid_rain_adjustment_run,
};
pub use rain_portable_artifact::admit_rain_portable_artifact;
pub use service::{
    AdmittedPrivateRouteAuthority, AdmittedTemperatureCanarySidecar,
    AdmittedTemperatureForecastServiceRuntime, AdmittedWindForecastServiceRuntime,
    DailyPrecipitationServiceBoundary, DecodedServiceRequest, ForecastRainAdjustmentRead,
    ForecastServiceBoundary, ForecastTemperatureAdjustmentBoundary, ForecastTemperatureSidecarRead,
    ForecastTemperatureStatusRead, ForecastWindAdjustmentBoundary, PrivateServiceListenerOwner,
    ResolvedServiceForecastWindow, ServiceClockError, ServiceRepositoryError, WeatherServiceBody,
    WeatherServiceBoundaries, WeatherServiceClock, WeatherServiceRepository,
    WeatherServiceResponse, serve_decoded_request, serve_private_decoded_request,
};
pub use service_wire::*;
pub use standard_adjustment_startup::{
    AdmittedStandardForecastAdjustmentOwner, StandardForecastAdjustmentStartup,
};
pub use startup::{
    AdjustmentRevisionCaptureWitnessLoader, AdmittedAdjustmentRevisionCaptureWitness,
    AdmittedMaintenanceArchiveOrigin, AdmittedMaintenanceListenerConfiguration, StartedWeatherApi,
    WeatherApiMaintenanceStartupPreparation, WeatherApiStartupApplicationPort,
    WeatherApiStartupClock, WeatherApiStartupError, WeatherApiStartupListenerConfiguration,
    WeatherApiStartupLoadError, WeatherApiStartupPortError, WeatherApiStartupRawListenerSettings,
    WeatherApiStartupRuntimeLoader, WeatherApiStartupSnapshots, start_weather_api,
    start_weather_api_with_raw_listener_settings,
};
pub use temperature_model::{
    AdmittedTemperatureMosModelOrigin, AdmittedTemperatureMosModelOwner,
    ForecastTemperatureMosAdmissionBoundary,
};
pub use temperature_projection::{
    AdmittedTemperatureCanaryAuthorization, AdmittedTemperatureCanaryEvidence,
    AdmittedTemperatureCanaryRuntime, project_temperature_canary_runtime,
    replace_temperature_adjustments, temperature_application_failure, temperature_canary_inputs,
};
pub use temperature_startup::{
    LegacyTemperatureForecastStartup, MaintenanceTemperatureForecastStartup,
};
pub use tides::{
    ParsedTideQuery, TideBoundaryAdapter, TideBoundaryError, TideQueryTime, parse_tide_query,
    project_tides_response, tides_read_plan,
};
pub use trends::{
    LegacyTrendRange, ParsedTrendQuery, TrendTimeBoundary, calendar_trend_query,
    parse_legacy_trend_range, project_trends_response, trends_read_plan,
};
pub use weather_adjustment::{
    AdmittedRainHurdleWindFeatureInput, AdmittedRainHurdleWindStations,
    AdmittedTemperatureMosCalendar, AdmittedWindRowCalendar, AdmittedWindRowFeatures,
    AdmittedWindRowSourceIdentity, AdmittedWindV2MetricOrder, BorrowedTemperatureMosServingModel,
    RainHurdleWindPortableModel, WindV2RowOutcome, build_admitted_rain_hurdle_wind_features,
    create_wind_moving_block_start_plan, evaluate_admitted_wind_v2_row,
};
pub use wind_maintenance_startup::{
    AdmittedMaintenanceWindModelOwner, ForecastMaintenanceWindAdjustmentBoundary,
    ForecastMaintenanceWindModelAdmissionBoundary, MaintenanceWindForecastStartup,
};
pub use wind_model::{
    AdmittedWindModelOwner, AdmittedWindModelRow, ForecastWindModelAdmissionBoundary,
    WindForecastRowAdmission,
};
pub use wind_repository_admission::RepositoryWindModelAdmission;
pub use wind_startup::LegacyWindForecastStartup;

use weather_contracts::{
    ApiSite, ApiSitesResponse, ApiSource, ApiSourceAttribution, ApiStation, SourceKind,
};
use weather_database::ActiveSiteRow;

/// One matched Weather API read route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    ForecastObservationInput,
    Health,
    Sites,
    Current { site_slug: String },
    DailyPrecipitation { site_slug: String },
    Forecast { site_slug: String },
    RainCollection { site_slug: String },
    History { site_slug: String },
    Tides { site_slug: String },
    Trends { site_slug: String },
}

/// Whether the eventual response adapter emits its serialized body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyPolicy {
    Include,
    Omit,
}

/// A successfully dispatched read before repository work begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadDispatch {
    pub route: Route,
    pub body_policy: BodyPolicy,
}

/// A bounded application error for the eventual JSON adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorDispatch {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
    pub allow_get_head: bool,
}

/// The pure result of applying route, method, and query-name policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dispatch {
    Read(ReadDispatch),
    Error(ErrorDispatch),
}

/// Database availability exposed by the health projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseStatus {
    Ready,
    Unavailable,
}

/// Schema migration state exposed by the health projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStatus {
    Current,
    Outdated,
    Unavailable,
}

/// Repository health facts before public projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthSnapshot {
    pub database: DatabaseStatus,
    pub migration_status: MigrationStatus,
    pub migration_version: Option<String>,
    pub worker_last_loop_epoch_ms: Option<i64>,
}

/// Coarse worker heartbeat state exposed publicly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerFreshness {
    Fresh,
    Stale,
    Unknown,
}

/// One transport-decoded query field in submission order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryParameter<'a> {
    pub name: &'a str,
    pub value: &'a str,
}

/// The supported forecast product selection before civil-time expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastSelection {
    Daily { days: u8 },
    Overnight,
}

/// One validated current-weather repository selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentSelection {
    pub source_id: Option<String>,
    pub station_slug: Option<String>,
}

/// Public health fields owned by the pure API application layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthProjection {
    pub database: DatabaseStatus,
    pub live: bool,
    pub migration_status: MigrationStatus,
    pub migration_version: Option<String>,
    pub ready: bool,
    pub status: u16,
    pub version: String,
    pub worker_freshness: WorkerFreshness,
}

const WORKER_FRESH_MILLISECONDS: i64 = 1_800_000;

/// Dispatch one transport-decoded request target.
///
/// The transport must supply an already decoded pathname and decoded query
/// names in their submitted order. Query value validation remains with each
/// route handler.
pub fn dispatch_decoded_request(method: &str, pathname: &str, query_names: &[&str]) -> Dispatch {
    let route = match_route(pathname);

    // preserve Node's route-before-method failure order
    let route = match route {
        Ok(route) => route,
        Err(error) => return Dispatch::Error(error),
    };

    // keep the recognized private route outside ordinary public dispatch
    if route == Route::ForecastObservationInput {
        return Dispatch::Error(ErrorDispatch {
            status: 404,
            code: "not_found",
            message: "Endpoint not found".to_owned(),
            allow_get_head: false,
        });
    }

    // preserve the exact read-only method surface
    let body_policy = match method {
        "GET" => BodyPolicy::Include,
        "HEAD" => BodyPolicy::Omit,
        _ => {
            return Dispatch::Error(ErrorDispatch {
                status: 405,
                code: "method_not_allowed",
                message: "Only GET and HEAD are supported".to_owned(),
                allow_get_head: true,
            });
        }
    };

    // reject the first unsupported query name in submission order
    for name in query_names {
        // keep each route's accepted filter names identical to the Node handler
        if !route_accepts_query_name(&route, name) {
            return Dispatch::Error(ErrorDispatch {
                status: 400,
                code: "invalid_query",
                message: format!("Unsupported query parameter: {name}"),
                allow_get_head: false,
            });
        }
    }

    Dispatch::Read(ReadDispatch { route, body_policy })
}

/// Project bounded repository health into the public readiness contract.
pub fn project_health(
    snapshot: HealthSnapshot,
    safety_epoch_ms: i64,
    version: &str,
) -> HealthProjection {
    let ready = snapshot.database == DatabaseStatus::Ready
        && snapshot.migration_status == MigrationStatus::Current;
    let worker_freshness =
        describe_worker_freshness(snapshot.worker_last_loop_epoch_ms, safety_epoch_ms);

    HealthProjection {
        database: snapshot.database,
        live: true,
        migration_status: snapshot.migration_status,
        migration_version: snapshot.migration_version,
        ready,
        // align the response status with database and schema readiness
        status: if ready { 200 } else { 503 },
        version: version.to_owned(),
        worker_freshness,
    }
}

/// Group ordered active-site repository rows into the canonical API DTO.
pub fn group_active_sites(rows: Vec<ActiveSiteRow>) -> ApiSitesResponse {
    let mut data = Vec::<ApiSite>::new();

    // group every repository metadata row without changing first-seen order
    for row in rows {
        let ActiveSiteRow {
            attribution_label,
            attribution_url,
            latitude,
            longitude,
            provider_key,
            provider_name,
            site_name,
            site_slug,
            source_id,
            source_key,
            source_kind,
            station_kind,
            station_latitude,
            station_longitude,
            station_name,
            station_slug,
            timezone,
        } = row;
        // locate an already-created site by stable slug
        let site_index = data.iter().position(|site| site.slug == site_slug);

        // create each site from its first repository row
        let site_index = match site_index {
            Some(index) => index,
            None => {
                data.push(ApiSite {
                    latitude,
                    longitude,
                    name: site_name,
                    slug: site_slug,
                    stations: Vec::new(),
                    timezone,
                });
                data.len() - 1
            }
        };
        let site = &mut data[site_index];
        // locate an already-created station within the selected site
        let station_index = site
            .stations
            .iter()
            .position(|station| station.slug == station_slug);

        // create each station from its first repository row
        let station_index = match station_index {
            Some(index) => index,
            None => {
                site.stations.push(ApiStation {
                    kind: station_kind,
                    latitude: station_latitude,
                    longitude: station_longitude,
                    name: station_name,
                    slug: station_slug,
                    sources: Vec::new(),
                });
                site.stations.len() - 1
            }
        };

        site.stations[station_index].sources.push(ApiSource {
            attribution: ApiSourceAttribution {
                label: attribution_label,
                url: attribution_url,
            },
            id: source_id,
            key: source_key,
            kind: source_kind,
            providerKey: provider_key,
            providerName: provider_name,
            provenanceLabel: source_kind_label(source_kind).to_owned(),
        });
    }

    ApiSitesResponse { data }
}

/// Describe one canonical generated source kind for public provenance.
fn source_kind_label(source_kind: SourceKind) -> &'static str {
    // preserve the TypeScript source label table exactly
    match source_kind {
        SourceKind::Forecast => "forecast model product",
        SourceKind::ModelCurrent => "model-derived current conditions",
        SourceKind::PhysicalSensor => "physical sensor measurement",
        SourceKind::Reanalysis => "gridded historical reanalysis",
        SourceKind::TideObservation => "observed tide level",
        SourceKind::TidePrediction => "tide prediction",
    }
}

/// Parse the source-owned forecast selector without calculating civil time.
pub fn parse_forecast_selection(
    parameters: &[QueryParameter<'_>],
) -> Result<ForecastSelection, ErrorDispatch> {
    // reject fields outside the forecast query surface
    for parameter in parameters {
        // preserve the first unsupported query response
        if !matches!(parameter.name, "days" | "window") {
            return Err(ErrorDispatch {
                status: 400,
                code: "invalid_query",
                message: format!("Unsupported query parameter: {}", parameter.name),
                allow_get_head: false,
            });
        }
    }

    let window = optional_query_value(parameters, "window")?;
    let has_window = parameters
        .iter()
        .any(|parameter| parameter.name == "window");
    let has_days = parameters.iter().any(|parameter| parameter.name == "days");

    // preserve the explicit-empty window failure
    if window.is_none() && has_window {
        return Err(invalid_query());
    }

    // retain the parameter-free and days-based products
    let Some(window) = window else {
        let days = optional_query_value(parameters, "days")?.unwrap_or("1");

        // accept only product-supported horizons
        let days = match days {
            "1" => 1,
            "5" => 5,
            "10" => 10,
            _ => return Err(invalid_query()),
        };
        return Ok(ForecastSelection::Daily { days });
    };

    // require the one overnight selector without a days field, including empty days
    if window != "overnight" || has_days {
        return Err(invalid_query());
    }

    Ok(ForecastSelection::Overnight)
}

/// Parse and validate one current-weather selection against active discovery.
pub fn parse_current_selection(
    parameters: &[QueryParameter<'_>],
    site: &ApiSite,
) -> Result<CurrentSelection, ErrorDispatch> {
    // reject fields outside the current-weather query surface
    for parameter in parameters {
        // preserve the first unsupported query response
        if !matches!(parameter.name, "source" | "station") {
            return Err(ErrorDispatch {
                status: 400,
                code: "invalid_query",
                message: format!("Unsupported query parameter: {}", parameter.name),
                allow_get_head: false,
            });
        }
    }

    let station_slug = optional_query_value(parameters, "station")?;

    // require the shared stable-key grammar for a submitted station
    if station_slug.is_some_and(|value| !is_stable_key(value)) {
        return Err(invalid_query());
    }

    let source_id = optional_query_value(parameters, "source")?;

    // require one canonical positive decimal source identifier
    if source_id.is_some_and(|value| !is_positive_decimal(value)) {
        return Err(ErrorDispatch {
            status: 400,
            code: "invalid_query",
            message: "source must be a positive integer".to_owned(),
            allow_get_head: false,
        });
    }

    // select all stations or the one requested active station
    let stations = site
        .stations
        .iter()
        .filter(|station| station_slug.is_none_or(|slug| station.slug == slug))
        .collect::<Vec<_>>();

    // distinguish an unknown station from an empty source result
    if let Some(station_slug) = station_slug
        && stations.is_empty()
    {
        return Err(ErrorDispatch {
            status: 404,
            code: "station_not_found",
            message: format!("Unknown station: {station_slug}"),
            allow_get_head: false,
        });
    }

    // require the source to belong to one selected active station
    if let Some(source_id) = source_id {
        let source_exists = stations
            .iter()
            .any(|station| station.sources.iter().any(|source| source.id == source_id));

        // distinguish inactive or unrelated sources
        if !source_exists {
            return Err(ErrorDispatch {
                status: 404,
                code: "source_not_found",
                message: format!("Unknown source: {source_id}"),
                allow_get_head: false,
            });
        }
    }

    Ok(CurrentSelection {
        source_id: source_id.map(str::to_owned),
        station_slug: station_slug.map(str::to_owned),
    })
}

/// Recognize the positive decimal identifier grammar used by PostgreSQL bigint IDs.
fn is_positive_decimal(value: &str) -> bool {
    let mut bytes = value.bytes();

    // require a non-zero leading digit
    let Some(first) = bytes.next() else {
        return false;
    };

    // reject zero, signs, and non-decimal leading characters
    if !(b'1'..=b'9').contains(&first) {
        return false;
    }

    // require every remaining byte to be one decimal digit
    bytes.all(|byte| byte.is_ascii_digit())
}

/// Read one optional query field while preserving duplicate and empty semantics.
fn optional_query_value<'a>(
    parameters: &'a [QueryParameter<'a>],
    name: &str,
) -> Result<Option<&'a str>, ErrorDispatch> {
    let mut values = parameters
        .iter()
        .filter(|parameter| parameter.name == name)
        .map(|parameter| parameter.value);
    let value = values.next();

    // reject ambiguous duplicate fields
    if values.next().is_some() {
        return Err(ErrorDispatch {
            status: 400,
            code: "invalid_query",
            message: format!("{name} may be provided once"),
            allow_get_head: false,
        });
    }

    // treat an empty value as absent
    if value == Some("") {
        return Ok(None);
    }

    Ok(value)
}

/// Classify one worker heartbeat using the production thirty-minute window.
fn describe_worker_freshness(
    last_loop_epoch_ms: Option<i64>,
    safety_epoch_ms: i64,
) -> WorkerFreshness {
    // preserve absent heartbeat state
    let Some(last_loop_epoch_ms) = last_loop_epoch_ms else {
        return WorkerFreshness::Unknown;
    };
    let age_ms = safety_epoch_ms.checked_sub(last_loop_epoch_ms);

    // fail closed on arithmetic overflow or future heartbeats
    let Some(age_ms) = age_ms else {
        return WorkerFreshness::Stale;
    };

    // fail closed on future heartbeats
    if age_ms < 0 {
        return WorkerFreshness::Stale;
    }

    // retain the inclusive production freshness boundary
    if age_ms <= WORKER_FRESH_MILLISECONDS {
        return WorkerFreshness::Fresh;
    }

    WorkerFreshness::Stale
}

/// Match only the frozen decoded application path surface.
fn match_route(pathname: &str) -> Result<Route, ErrorDispatch> {
    // keep the fixed private read outside the public site hierarchy
    if pathname == "/internal/v1/forecast-observation-input" {
        return Ok(Route::ForecastObservationInput);
    }

    let segments = canonical_segments(pathname)?;

    // match the two top-level read routes
    if segments.len() == 3 && segments[0] == "api" && segments[1] == "v1" {
        // match active site discovery
        if segments[2] == "sites" {
            return Ok(Route::Sites);
        }

        // match liveness and readiness
        if segments[2] == "health" {
            return Ok(Route::Health);
        }
    }

    // match one site-specific resource
    if segments.len() == 5 && segments[0] == "api" && segments[1] == "v1" && segments[2] == "sites"
    {
        let site_slug = segments[3];

        // keep invalid site path segments outside the public surface
        if !is_stable_key(site_slug) {
            return Err(not_found());
        }

        let site_slug = site_slug.to_owned();

        // map the exact final resource segment
        return match segments[4] {
            "current" => Ok(Route::Current { site_slug }),
            "daily-precipitation" => Ok(Route::DailyPrecipitation { site_slug }),
            "forecast" => Ok(Route::Forecast { site_slug }),
            "rain-collection" => Ok(Route::RainCollection { site_slug }),
            "history" => Ok(Route::History { site_slug }),
            "tides" => Ok(Route::Tides { site_slug }),
            "trends" => Ok(Route::Trends { site_slug }),
            _ => Err(not_found()),
        };
    }

    Err(not_found())
}

/// Split one canonical decoded pathname without performing transport decoding.
fn canonical_segments(pathname: &str) -> Result<Vec<&str>, ErrorDispatch> {
    // reject non-canonical separators and trailing slashes
    if !pathname.starts_with('/') || pathname.ends_with('/') || pathname.contains("//") {
        return Err(not_found());
    }

    let segments = pathname[1..].split('/').collect::<Vec<_>>();

    // reject empty decoded segments
    if segments
        .iter()
        .any(|segment| segment.is_empty() || segment.contains('/'))
    {
        return Err(not_found());
    }

    Ok(segments)
}

/// Validate the lowercase kebab-case key grammar shared with TypeScript.
fn is_stable_key(value: &str) -> bool {
    // preserve the TypeScript maximum and reject empty keys
    if value.is_empty() || value.len() > 80 {
        return false;
    }

    // require every hyphen-separated component to be non-empty ASCII lowercase
    for component in value.split('-') {
        // reject repeated or edge hyphens
        if component.is_empty() {
            return false;
        }

        // reject characters outside the portable source key alphabet
        if !component
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return false;
        }
    }

    true
}

/// Check the query-name allowlist owned by one route.
fn route_accepts_query_name(route: &Route, name: &str) -> bool {
    // keep the route-specific allowlists closed
    match route {
        Route::Current { .. } => matches!(name, "source" | "station"),
        Route::Forecast { .. } => matches!(name, "days" | "window"),
        Route::History { .. } => matches!(
            name,
            "cursor" | "from" | "limit" | "source" | "sourceKind" | "station" | "to"
        ),
        Route::Tides { .. } => matches!(name, "from" | "to"),
        Route::Trends { .. } => name == "range",
        Route::ForecastObservationInput
        | Route::Health
        | Route::Sites
        | Route::DailyPrecipitation { .. }
        | Route::RainCollection { .. } => false,
    }
}

/// Build the shared public not-found response plan.
fn not_found() -> ErrorDispatch {
    ErrorDispatch {
        status: 404,
        code: "not_found",
        message: "Endpoint not found".to_owned(),
        allow_get_head: false,
    }
}

/// Build the bounded public error used for invalid forecast selector values.
fn invalid_query() -> ErrorDispatch {
    ErrorDispatch {
        status: 400,
        code: "invalid_query",
        message: "Request query is invalid".to_owned(),
        allow_get_head: false,
    }
}

mod temperature_fit_evaluator;
pub use temperature_fit_evaluator::*;
