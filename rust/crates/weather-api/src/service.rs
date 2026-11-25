use weather_contracts::{
    ApiCurrentWeatherResponse, ApiErrorResponse, ApiErrorResponseError,
    ApiForecastAdjustmentRuntime, ApiForecastAdjustmentRuntimeReasonCodeEnum,
    ApiForecastAdjustmentRuntimeStateEnum, ApiForecastResponse,
    ApiForecastTemperatureAdjustmentRuntime, ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum,
    ApiForecastTemperatureAdjustmentRuntimeStateEnum, ApiHealthResponse, ApiHealthResponseData,
    ApiHealthResponseDataDatabaseEnum, ApiHealthResponseDataLiveLiteral,
    ApiHealthResponseDataMigration, ApiHealthResponseDataMigrationStatusEnum,
    ApiHealthResponseDataWorker, ApiHealthResponseDataWorkerFreshnessEnum, ApiHistoryResponse,
    ApiRainCollectionResponse, ApiSitesResponse, ApiTidesResponse, ApiTrendsResponse,
    CurrentWeatherRecordRow, DailyPrecipitationRow, EcmwfTemperatureCanarySidecar,
    EcmwfTemperatureCanaryStatus, FieldPresence, ForecastDays, ForecastObservationInput,
    ForecastTemperatureCanaryDecisionV1, Nullable, RainCollectionStatus, TideRecordRow,
    TrendPointRow, WeatherRecordRow,
};
use weather_database::{
    ActiveSiteRow, AdmittedDailyPrecipitationQuery, AdmittedForecastObservationQuery,
    CurrentWeatherQuery, CurrentWeatherQueryPlan, DailyPrecipitationReadPlan,
    ForecastObservationInputReadPlan, ForecastPressureContextPlan, ForecastReadPlan,
    HistoryReadPlan, ListActiveSitesQuery, TideReadPlan, TrendReadPlan, current_weather_query,
    daily_precipitation_read_plan, forecast_observation_input_read_plan, list_active_sites_query,
};

use crate::ForecastMaintenanceWindAdjustmentBoundary;
use crate::{
    AdmittedRainAdjustmentRun, AdmittedRainForecastRuntime, AdmittedTemperatureCanaryRuntime,
    AdmittedWindCanaryRuntime, AdmittedWindRowOutput, CanonicalInstant, CurrentProjectionError,
    CurrentTimeResolver, DatabaseStatus, ErrorDispatch, ForecastObservationGeometryBoundary,
    ForecastObservationInputResponse, ForecastStartupRefusal, HealthSnapshot,
    HistoryBoundaryAdapter, InactiveForecastDecisionConfig, InactiveForecastServiceRuntime,
    MigrationStatus, QueryParameter, RainCollectionTimestampBoundary,
    ResolvedDailyPrecipitationWindow, ResolvedForecastPressureContextWindow,
    ResolvedForecastReadWindow, Route, TideBoundaryAdapter, TrendTimeBoundary, WorkerFreshness,
    calendar_trend_query, daily_precipitation_query, forecast_endpoint_read_plan,
    forecast_pressure_context_read_plan, group_active_sites, history_read_plan, match_route,
    parse_current_selection, parse_forecast_selection, parse_history_query,
    parse_legacy_trend_range, parse_tide_query, project_current_weather_response,
    project_daily_precipitation_response, project_forecast_observation_input,
    project_forecast_pressure_context, project_forecast_weather_records, project_health,
    project_history_response, project_rain_collection_status, project_temperature_canary_runtime,
    project_tides_response, project_trends_response, project_wind_canary_runtime,
    replace_rain_adjustments, replace_temperature_adjustments, temperature_application_failure,
    temperature_canary_inputs, tides_read_plan, trends_read_plan,
};

/// one decoded request before any HTTP or JSON adaptation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedServiceRequest<'a> {
    pub method: &'a str,
    pub pathname: &'a str,
    pub query: &'a [QueryParameter<'a>],
}

/// one bounded repository execution failure without private database detail
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceRepositoryError;

/// one request-clock failure without transport detail
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceClockError;

/// one process-wiring owner for the separate private service listener
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateServiceListenerOwner {
    ForecastObservationProjection,
}

/// admitted private-route ownership passed only by the separate listener adapter
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedPrivateRouteAuthority {
    owner: PrivateServiceListenerOwner,
}

impl AdmittedPrivateRouteAuthority {
    /// bind one already separated listener owner to private application dispatch
    pub const fn admitted(owner: PrivateServiceListenerOwner) -> Self {
        Self { owner }
    }
}

/// one admitted forecast calendar and repository window
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedServiceForecastWindow {
    pub public_start: CanonicalInstant,
    pub public_end: CanonicalInstant,
    pub repository: ResolvedForecastReadWindow,
    pub pressure_context: ResolvedForecastPressureContextWindow,
}

/// admitted site-calendar calculations for one forecast request
pub trait ForecastServiceBoundary {
    /// resolve public, expanded repository, and optional pressure windows
    fn resolve_window(
        &self,
        selection: crate::ForecastSelection,
        data_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<ResolvedServiceForecastWindow, crate::CurrentProjectionError>;
}

/// admitted site-calendar calculation for the current precipitation day
pub trait DailyPrecipitationServiceBoundary {
    /// resolve the local-day start and nominal data-time end
    fn resolve_window(
        &self,
        data_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<ResolvedDailyPrecipitationWindow, crate::CurrentProjectionError>;
}

/// admitted row-application boundary for one constructor-snapshotted wind runtime
pub trait ForecastWindAdjustmentBoundary {
    /// derive one explicit active or fail-raw output for one selected repository row
    fn apply(
        &self,
        runtime: &AdmittedWindCanaryRuntime,
        row: &WeatherRecordRow,
    ) -> Result<AdmittedWindRowOutput, CurrentProjectionError>;
}

/// one prevalidated wind runtime and its application owner captured at construction
#[derive(Clone, Copy)]
pub enum AdmittedWindForecastServiceRuntime<'a> {
    Legacy {
        loaded_at: &'a CanonicalInstant,
        runtime: &'a AdmittedWindCanaryRuntime,
        adjustment: &'a dyn ForecastWindAdjustmentBoundary,
    },
    Maintenance {
        loaded_at: &'a CanonicalInstant,
        adjustment: &'a dyn ForecastMaintenanceWindAdjustmentBoundary,
    },
    Standard {
        loaded_at: &'a CanonicalInstant,
        adjustment: &'a dyn ForecastMaintenanceWindAdjustmentBoundary,
    },
}

/// one exact optional sidecar read derived from data-time forecast geometry
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastTemperatureSidecarRead {
    pub site_slug: String,
    pub as_of: String,
    pub from: String,
    pub to: String,
}

/// one decoded sidecar with its admitted causal-state material digest
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedTemperatureCanarySidecar {
    pub sidecar: EcmwfTemperatureCanarySidecar,
    pub recent_error_state_sha256: String,
}

/// one exact optional collector-status read at the request safety clock
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastTemperatureStatusRead {
    pub site_slug: String,
    pub as_of: String,
}

/// one exact optional retained-rain read at the nominal data clock
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastRainAdjustmentRead {
    pub site_slug: String,
    pub as_of: String,
}

/// admitted row application boundary for one validated temperature runtime
pub trait ForecastTemperatureAdjustmentBoundary {
    /// apply one validated runtime to one exact generated row input
    fn apply(
        &self,
        runtime: &AdmittedTemperatureCanaryRuntime,
        input: &weather_contracts::ApplyTemperatureCanaryInputV1,
        recent_error_state_sha256: Option<&str>,
    ) -> Result<ForecastTemperatureCanaryDecisionV1, CurrentProjectionError>;
}

/// one validated temperature runtime and application owner captured at construction
#[derive(Clone, Copy)]
pub struct AdmittedTemperatureForecastServiceRuntime<'a> {
    pub loaded_at: &'a CanonicalInstant,
    pub runtime: &'a AdmittedTemperatureCanaryRuntime,
    pub adjustment: &'a dyn ForecastTemperatureAdjustmentBoundary,
}

/// one request-scoped canonical clock
pub trait WeatherServiceClock {
    /// read one canonical request instant
    fn request_time(&mut self) -> Result<CanonicalInstant, ServiceClockError>;
}

/// exact typed repository plan execution used by integrated public reads
pub trait WeatherServiceRepository {
    /// execute active-site discovery
    fn active_sites(
        &mut self,
        plan: ListActiveSitesQuery,
    ) -> Result<Vec<ActiveSiteRow>, ServiceRepositoryError>;

    /// execute one current-weather plan
    fn current(
        &mut self,
        plan: CurrentWeatherQueryPlan,
    ) -> Result<Vec<CurrentWeatherRecordRow>, ServiceRepositoryError>;

    /// execute one history plan
    fn history(
        &mut self,
        plan: HistoryReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, ServiceRepositoryError>;

    /// execute one tide plan
    fn tides(&mut self, plan: TideReadPlan) -> Result<Vec<TideRecordRow>, ServiceRepositoryError>;

    /// execute one raw forecast plan
    fn forecast(
        &mut self,
        plan: ForecastReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, ServiceRepositoryError>;

    /// execute one optional pressure-context plan
    fn forecast_pressure_context(
        &mut self,
        plan: ForecastPressureContextPlan,
    ) -> Result<Vec<WeatherRecordRow>, ServiceRepositoryError>;

    /// read one optional active temperature sidecar
    fn temperature_canary_sidecar(
        &mut self,
        _read: ForecastTemperatureSidecarRead,
    ) -> Result<Option<AdmittedTemperatureCanarySidecar>, ServiceRepositoryError> {
        Ok(None)
    }

    /// read one optional bounded temperature collector status
    fn temperature_canary_status(
        &mut self,
        _read: ForecastTemperatureStatusRead,
    ) -> Result<Option<EcmwfTemperatureCanaryStatus>, ServiceRepositoryError> {
        Ok(None)
    }

    /// read one optional retained public-safe rain adjustment run
    fn rain_adjustment_run(
        &mut self,
        _read: ForecastRainAdjustmentRead,
    ) -> Result<Option<AdmittedRainAdjustmentRun>, ServiceRepositoryError> {
        Ok(None)
    }

    /// execute one daily trend plan
    fn trends(&mut self, plan: TrendReadPlan)
    -> Result<Vec<TrendPointRow>, ServiceRepositoryError>;

    /// execute one nearest-gauge local-day precipitation plan
    fn daily_precipitation(
        &mut self,
        plan: DailyPrecipitationReadPlan,
    ) -> Result<Option<DailyPrecipitationRow>, ServiceRepositoryError>;

    /// read one bounded readiness snapshot
    fn health(&mut self) -> Result<HealthSnapshot, ServiceRepositoryError>;

    /// read one sanitized fixed-site rain collection status
    fn rain_collection_status(
        &mut self,
    ) -> Result<Option<RainCollectionStatus>, ServiceRepositoryError>;

    /// execute one fixed private observation-input plan
    fn forecast_observation_input(
        &mut self,
        plan: ForecastObservationInputReadPlan,
    ) -> Result<Option<ForecastObservationInput>, ServiceRepositoryError>;
}

/// admitted time and cursor boundaries shared by integrated routes
pub struct WeatherServiceBoundaries<'a> {
    pub nominal_data_at: Option<&'a CanonicalInstant>,
    pub time_resolver: &'a dyn CurrentTimeResolver,
    pub history: &'a dyn HistoryBoundaryAdapter,
    pub tides: &'a dyn TideBoundaryAdapter,
    pub forecast: &'a dyn ForecastServiceBoundary,
    pub forecast_runtime: &'a InactiveForecastServiceRuntime,
    pub forecast_wind_runtime: Option<AdmittedWindForecastServiceRuntime<'a>>,
    pub forecast_temperature_runtime: Option<AdmittedTemperatureForecastServiceRuntime<'a>>,
    pub forecast_rain_runtime: Option<&'a AdmittedRainForecastRuntime>,
    pub trends: &'a dyn TrendTimeBoundary,
    pub daily_precipitation: &'a dyn DailyPrecipitationServiceBoundary,
    pub rain_collection: &'a dyn RainCollectionTimestampBoundary,
    pub forecast_observation: &'a dyn ForecastObservationGeometryBoundary,
    pub version: Option<&'a str>,
}

/// one typed response body before JSON serialization
#[derive(Debug, Clone, PartialEq)]
pub enum WeatherServiceBody {
    Sites(ApiSitesResponse),
    Current(ApiCurrentWeatherResponse),
    History(ApiHistoryResponse),
    Tides(ApiTidesResponse),
    Forecast(Box<ApiForecastResponse>),
    Trends(ApiTrendsResponse),
    DailyPrecipitation(weather_contracts::ApiDailyPrecipitationResponse),
    RainCollection(ApiRainCollectionResponse),
    ForecastObservationInput(Box<ForecastObservationInputResponse>),
    Health(Box<ApiHealthResponse>),
    Error(ApiErrorResponse),
}

/// one transport-independent service response
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherServiceResponse {
    pub status: u16,
    pub allow_get_head: bool,
    pub body: Option<WeatherServiceBody>,
}

/// execute one decoded public request through real query plans and DTO projections
pub fn serve_decoded_request(
    repository: &mut dyn WeatherServiceRepository,
    clock: &mut dyn WeatherServiceClock,
    boundaries: &WeatherServiceBoundaries<'_>,
    request: DecodedServiceRequest<'_>,
) -> WeatherServiceResponse {
    let omit_body = request.method == "HEAD";
    let response = serve_decoded_request_representation(repository, clock, boundaries, request);
    suppress_head_body(response, omit_body)
}

/// execute one decoded public request while retaining its typed HEAD representation
pub(crate) fn serve_decoded_request_representation(
    repository: &mut dyn WeatherServiceRepository,
    clock: &mut dyn WeatherServiceClock,
    boundaries: &WeatherServiceBoundaries<'_>,
    request: DecodedServiceRequest<'_>,
) -> WeatherServiceResponse {
    serve_decoded_request_representation_reporting(repository, clock, boundaries, request, None)
}

/// execute one decoded public request with optional private rain-read failure reporting
pub(crate) fn serve_decoded_request_representation_reporting(
    repository: &mut dyn WeatherServiceRepository,
    clock: &mut dyn WeatherServiceClock,
    boundaries: &WeatherServiceBoundaries<'_>,
    request: DecodedServiceRequest<'_>,
    rain_read_failed: Option<&mut bool>,
) -> WeatherServiceResponse {
    let route = match match_route(request.pathname) {
        Ok(route) => route,
        Err(error) => return public_error(error),
    };

    // never admit the recognized private path through public service dispatch
    if route == Route::ForecastObservationInput {
        return public_error(not_found());
    }
    let safety_at = match clock.request_time() {
        Ok(safety_at) => safety_at,
        Err(_) => return internal_error(),
    };
    let data_at = boundaries
        .nominal_data_at
        .cloned()
        .unwrap_or_else(|| safety_at.clone());

    // preserve Node's request-clock-before-method order on matched routes
    if !matches!(request.method, "GET" | "HEAD") {
        return public_error(ErrorDispatch {
            status: 405,
            code: "method_not_allowed",
            message: "Only GET and HEAD are supported".to_owned(),
            allow_get_head: true,
        });
    }

    // serve health before active-site discovery like the Node handler
    if route == Route::Health {
        // reject the first unexpected fixed-route parameter
        if let Some(parameter) = request.query.first() {
            return public_error(unsupported_query(parameter.name));
        }
        return serve_health(repository, boundaries, &safety_at);
    }

    // serve discovery without a second site lookup
    if route == Route::Sites {
        // reject the first unexpected discovery parameter
        if let Some(parameter) = request.query.first() {
            return public_error(unsupported_query(parameter.name));
        }
        let rows = match repository.active_sites(list_active_sites_query()) {
            Ok(rows) => rows,
            Err(_) => return internal_error(),
        };
        return success(WeatherServiceBody::Sites(group_active_sites(rows)));
    }

    let Some(site_slug) = route_site_slug(&route) else {
        return unsupported_route();
    };
    let rows = match repository.active_sites(list_active_sites_query()) {
        Ok(rows) => rows,
        Err(_) => return internal_error(),
    };
    let sites = group_active_sites(rows);
    let site = sites.data.into_iter().find(|site| site.slug == site_slug);
    let Some(site) = site else {
        return public_error(ErrorDispatch {
            status: 404,
            code: "site_not_found",
            message: format!("Unknown site: {site_slug}"),
            allow_get_head: false,
        });
    };

    // execute only routes with complete typed plan and projection ownership
    match route {
        Route::Current { .. } => {
            serve_current(repository, boundaries, request.query, site, data_at)
        }
        Route::History { .. } => {
            serve_history(repository, boundaries, request.query, site, data_at)
        }
        Route::Tides { .. } => serve_tides(repository, boundaries, request.query, site, data_at),
        Route::Forecast { .. } => serve_forecast(
            repository,
            boundaries,
            request.query,
            site,
            data_at,
            safety_at,
            rain_read_failed,
        ),
        Route::Trends { .. } => serve_trends(repository, boundaries, request.query, site, data_at),
        Route::DailyPrecipitation { .. } => {
            serve_daily_precipitation(repository, boundaries, request.query, site, data_at)
        }
        Route::RainCollection { .. } => serve_rain_collection(
            repository,
            boundaries,
            request.query,
            site,
            data_at,
            safety_at,
            rain_read_failed,
        ),
        _ => unsupported_route(),
    }
}

/// execute one decoded request owned by the separate private listener
pub fn serve_private_decoded_request(
    authority: AdmittedPrivateRouteAuthority,
    repository: &mut dyn WeatherServiceRepository,
    clock: &mut dyn WeatherServiceClock,
    boundaries: &WeatherServiceBoundaries<'_>,
    request: DecodedServiceRequest<'_>,
) -> WeatherServiceResponse {
    let omit_body = request.method == "HEAD";
    let response = serve_private_decoded_request_representation(
        authority, repository, clock, boundaries, request,
    );
    suppress_head_body(response, omit_body)
}

/// execute one private observation request while retaining its typed HEAD representation
pub(crate) fn serve_private_decoded_request_representation(
    authority: AdmittedPrivateRouteAuthority,
    repository: &mut dyn WeatherServiceRepository,
    clock: &mut dyn WeatherServiceClock,
    boundaries: &WeatherServiceBoundaries<'_>,
    request: DecodedServiceRequest<'_>,
) -> WeatherServiceResponse {
    let route = match match_route(request.pathname) {
        Ok(route) => route,
        Err(_) => return public_error(not_found()),
    };

    // keep the private owner token bound to its one recognized route
    if authority.owner != PrivateServiceListenerOwner::ForecastObservationProjection
        || route != Route::ForecastObservationInput
    {
        return public_error(not_found());
    }
    let safety_at = match clock.request_time() {
        Ok(safety_at) => safety_at,
        Err(_) => return internal_error(),
    };
    let data_at = boundaries.nominal_data_at.cloned().unwrap_or(safety_at);

    // preserve the exact read-only private method surface
    if !matches!(request.method, "GET" | "HEAD") {
        return public_error(ErrorDispatch {
            status: 405,
            code: "method_not_allowed",
            message: "Only GET and HEAD are supported".to_owned(),
            allow_get_head: true,
        });
    }

    // preserve the fixed private route's empty query surface
    if let Some(parameter) = request.query.first() {
        return public_error(unsupported_query(parameter.name));
    }
    let plan = forecast_observation_input_read_plan(&AdmittedForecastObservationQuery::admitted(
        data_at.utc.clone(),
    ));
    let input = match repository.forecast_observation_input(plan) {
        Ok(Some(input)) => input,
        Ok(None) => return public_error(not_found()),
        Err(_) => return internal_error(),
    };

    // fail closed when a non-database adapter exceeds the fixed archive bound
    if input.records.len() > 20_000 {
        return public_error(not_found());
    }
    let response = match project_forecast_observation_input(
        input,
        &data_at,
        boundaries.forecast_observation,
        boundaries.time_resolver,
    ) {
        Ok(response) => response,
        Err(_) => return internal_error(),
    };
    success(WeatherServiceBody::ForecastObservationInput(Box::new(
        response,
    )))
}

/// execute one selected current read and canonical projection
fn serve_current(
    repository: &mut dyn WeatherServiceRepository,
    boundaries: &WeatherServiceBoundaries<'_>,
    query: &[QueryParameter<'_>],
    site: weather_contracts::ApiSite,
    data_at: CanonicalInstant,
) -> WeatherServiceResponse {
    let selection = match parse_current_selection(query, &site) {
        Ok(selection) => selection,
        Err(error) => return public_error(error),
    };
    let plan = current_weather_query(CurrentWeatherQuery {
        site_slug: site.slug.clone(),
        station_slug: selection.station_slug,
        source_id: selection.source_id,
        as_of: data_at.utc.clone(),
    });
    let rows = match repository.current(plan) {
        Ok(rows) => rows,
        Err(_) => return internal_error(),
    };
    let response =
        match project_current_weather_response(rows, site, data_at, boundaries.time_resolver) {
            Ok(response) => response,
            Err(_) => return internal_error(),
        };
    success(WeatherServiceBody::Current(response))
}

/// execute one history read and canonical page projection
fn serve_history(
    repository: &mut dyn WeatherServiceRepository,
    boundaries: &WeatherServiceBoundaries<'_>,
    query: &[QueryParameter<'_>],
    site: weather_contracts::ApiSite,
    data_at: CanonicalInstant,
) -> WeatherServiceResponse {
    let parsed = match parse_history_query(query, &site, boundaries.history) {
        Ok(parsed) => parsed,
        Err(error) => return public_error(error),
    };
    let plan = match history_read_plan(&parsed) {
        Ok(plan) => plan,
        Err(_) => return internal_error(),
    };
    let rows = match repository.history(plan) {
        Ok(rows) => rows,
        Err(_) => return internal_error(),
    };
    let response = match project_history_response(
        rows,
        site,
        data_at,
        parsed.requested_limit,
        boundaries.time_resolver,
        boundaries.history,
    ) {
        Ok(response) => response,
        Err(_) => return internal_error(),
    };
    success(WeatherServiceBody::History(response))
}

/// execute one tide read and canonical response projection
fn serve_tides(
    repository: &mut dyn WeatherServiceRepository,
    boundaries: &WeatherServiceBoundaries<'_>,
    query: &[QueryParameter<'_>],
    site: weather_contracts::ApiSite,
    data_at: CanonicalInstant,
) -> WeatherServiceResponse {
    let parsed = match parse_tide_query(query, &site, &data_at, boundaries.tides) {
        Ok(parsed) => parsed,
        Err(error) => return public_error(error),
    };
    let plan = match tides_read_plan(&parsed) {
        Ok(plan) => plan,
        Err(_) => return internal_error(),
    };
    let rows = match repository.tides(plan) {
        Ok(rows) => rows,
        Err(_) => return internal_error(),
    };
    let response = match project_tides_response(rows, site, &data_at, boundaries.time_resolver) {
        Ok(response) => response,
        Err(_) => return internal_error(),
    };
    success(WeatherServiceBody::Tides(response))
}

/// execute one forecast read, optional context read, and fail-raw projection
fn serve_forecast(
    repository: &mut dyn WeatherServiceRepository,
    boundaries: &WeatherServiceBoundaries<'_>,
    query: &[QueryParameter<'_>],
    site: weather_contracts::ApiSite,
    data_at: CanonicalInstant,
    safety_at: CanonicalInstant,
    rain_read_failed: Option<&mut bool>,
) -> WeatherServiceResponse {
    let selection = match parse_forecast_selection(query) {
        Ok(selection) => selection,
        Err(error) => return public_error(error),
    };
    let window = match boundaries
        .forecast
        .resolve_window(selection, &data_at, &site.timezone)
    {
        Ok(window) => window,
        Err(_) => return internal_error(),
    };
    let plan = match forecast_endpoint_read_plan(&site, selection, &window.repository) {
        Ok(plan) => plan,
        Err(_) => return internal_error(),
    };
    let pressure_plan = match forecast_pressure_context_read_plan(&site, &window.pressure_context) {
        Ok(plan) => plan,
        Err(_) => return internal_error(),
    };
    let rows = match repository.forecast(plan) {
        Ok(rows) => rows,
        Err(_) => return internal_error(),
    };
    let pressure_rows = repository
        .forecast_pressure_context(pressure_plan)
        .unwrap_or_default();
    let adjustment_refusal = boundaries.forecast_runtime.adjustment_refusal();
    let temperature_refusal = boundaries.forecast_runtime.temperature_refusal();
    // prevent an explicitly refused startup family from using an injected active runtime
    let wind_runtime = if adjustment_refusal.is_none() {
        boundaries.forecast_wind_runtime
    } else {
        None
    };
    // keep temperature startup refusal independent from wind admission
    let temperature_runtime = if temperature_refusal.is_none() {
        boundaries.forecast_temperature_runtime
    } else {
        None
    };
    let default_refusal = ForecastStartupRefusal::RegistryInactive;
    let inactive_decisions = InactiveForecastDecisionConfig {
        rain_reason_code: InactiveForecastDecisionConfig::default().rain_reason_code,
        adjustment_reason_code: adjustment_refusal
            .unwrap_or(default_refusal)
            .adjustment_decision_reason(),
        temperature_reason_code: temperature_refusal
            .unwrap_or(default_refusal)
            .temperature_decision_reason(),
    };
    let temperature_runtime_is_active = temperature_runtime
        .map(|active| {
            active
                .runtime
                .authorization
                .is_active_at(safety_at.epoch_milliseconds)
        })
        .unwrap_or(false);

    // query optional sidecar data only while the safety-projected runtime is active
    let (temperature_sidecar, temperature_status) = if temperature_runtime_is_active {
        let sidecar = repository
            .temperature_canary_sidecar(ForecastTemperatureSidecarRead {
                site_slug: site.slug.clone(),
                as_of: data_at.utc.clone(),
                from: window.public_start.utc.clone(),
                to: window.public_end.utc.clone(),
            })
            .unwrap_or(None);
        let status = repository
            .temperature_canary_status(ForecastTemperatureStatusRead {
                site_slug: site.slug.clone(),
                as_of: safety_at.utc.clone(),
            })
            .unwrap_or(None);
        (sidecar, status)
    } else {
        (None, None)
    };
    let rain_run = read_rain_adjustment_run(
        repository,
        boundaries,
        &site.slug,
        &data_at,
        &safety_at,
        rain_read_failed,
    );
    let rain_decisions = match crate::rain_forecast::project_admitted_rain_adjustments(
        &rows,
        rain_run.as_ref(),
        boundaries.forecast_rain_runtime,
        boundaries.time_resolver,
    ) {
        Ok(decisions) => decisions,
        Err(_) => return internal_error(),
    };
    let temperature_decisions = match temperature_runtime {
        Some(active) => {
            let inputs = match temperature_canary_inputs(
                &rows,
                temperature_sidecar
                    .as_ref()
                    .map(|material| &material.sidecar),
                &safety_at,
                boundaries.time_resolver,
            ) {
                Ok(inputs) => inputs,
                Err(_) => return internal_error(),
            };
            let decisions = rows
                .iter()
                .zip(inputs)
                .map(|(row, input)| {
                    active
                        .adjustment
                        .apply(
                            active.runtime,
                            &input,
                            temperature_sidecar
                                .as_ref()
                                .map(|material| material.recent_error_state_sha256.as_str()),
                        )
                        .unwrap_or_else(|_| {
                            temperature_application_failure(row.temperatureC.clone())
                        })
                })
                .collect();
            Some(decisions)
        }
        None => None,
    };
    let visibility = match forecast_row_visibility(
        &rows,
        &window.public_start,
        &window.public_end,
        boundaries.time_resolver,
    ) {
        Ok(visibility) => visibility,
        Err(_) => return internal_error(),
    };
    // use the constructor snapshot when configured and preserve inactive defaults otherwise
    let (mut records, adjustment_runtime) = match wind_runtime {
        Some(AdmittedWindForecastServiceRuntime::Legacy {
            loaded_at,
            runtime,
            adjustment,
        }) => {
            let mut evaluated_runtime = runtime.clone();
            evaluated_runtime.evaluated_at_epoch_ms = safety_at.epoch_milliseconds;

            // append active or fail-raw decisions without modifying raw weather metrics
            let records = match crate::forecast_projection::project_forecast_weather_records_with_adjustment_decisions(
                rows,
                &site,
                &data_at,
                boundaries.time_resolver,
                inactive_decisions,
                |row| {
                    // contain optional adjustment failures at their exact row boundary
                    crate::forecast_projection::project_wind_adjustment(
                        row,
                        &evaluated_runtime,
                        || {
                            adjustment.apply(&evaluated_runtime, row).unwrap_or(
                                AdmittedWindRowOutput::FailRaw(
                                    weather_contracts::ForecastAdjustmentReasonCode::AdjustmentError,
                                ),
                            )
                        },
                    )
                },
            ) {
                Ok(records) => records,
                Err(_) => return internal_error(),
            };

            // project lifecycle state through the same request safety clock
            let runtime = match project_wind_canary_runtime(loaded_at, &evaluated_runtime) {
                Ok(runtime) => runtime,
                Err(_) => return internal_error(),
            };
            (records, runtime)
        }
        Some(
            AdmittedWindForecastServiceRuntime::Maintenance {
                loaded_at,
                adjustment,
            }
            | AdmittedWindForecastServiceRuntime::Standard {
                loaded_at,
                adjustment,
            },
        ) => {
            let records = match crate::forecast_projection::project_forecast_weather_records_with_adjustment_decisions(
                rows,
                &site,
                &data_at,
                boundaries.time_resolver,
                inactive_decisions,
                |row| {
                    // contain a v2 application failure at its exact row boundary
                    adjustment.apply(row).unwrap_or_else(|_| {
                        crate::forecast_projection::forecast_adjustment_application_failure()
                    })
                },
            ) {
                Ok(records) => records,
                Err(_) => return internal_error(),
            };
            (records, adjustment.project_runtime(loaded_at))
        }
        None => {
            // project the exact startup fallback without applying a refused wind runtime
            let records = match project_forecast_weather_records(
                rows,
                &site,
                &data_at,
                boundaries.time_resolver,
                inactive_decisions,
            ) {
                Ok(records) => records,
                Err(_) => return internal_error(),
            };
            (
                records,
                inactive_adjustment_runtime(
                    boundaries.forecast_runtime.adjustment_loaded_at(),
                    adjustment_refusal
                        .unwrap_or(default_refusal)
                        .adjustment_runtime_reason(),
                ),
            )
        }
    };

    // replace only temperature decisions after raw and wind projection succeeds
    if let Some(decisions) = temperature_decisions {
        records = match replace_temperature_adjustments(records, decisions) {
            Ok(records) => records,
            Err(_) => return internal_error(),
        };
    }
    // replace rain decisions after raw, wind, and temperature projections succeed
    records = match replace_rain_adjustments(records, rain_decisions) {
        Ok(records) => records,
        Err(_) => return internal_error(),
    };
    let include_air_quality_context = matches!(selection, crate::ForecastSelection::Daily { .. });
    let mut data = Vec::with_capacity(records.len());
    let mut air_quality_context = Vec::new();

    // retain repository order while splitting the expanded daily read
    for (record, is_public) in records.into_iter().zip(visibility) {
        // expose only public rows in data and only expanded rows in AQ context
        if is_public {
            data.push(record);
        } else if include_air_quality_context {
            air_quality_context.push(record);
        }
    }

    let temperature_runtime = match temperature_runtime {
        Some(active) => project_temperature_canary_runtime(
            active.loaded_at,
            active.runtime,
            &safety_at,
            temperature_status,
        ),
        None => inactive_temperature_runtime(
            boundaries.forecast_runtime.temperature_loaded_at(),
            temperature_refusal
                .unwrap_or(default_refusal)
                .temperature_runtime_reason(),
        ),
    };
    let rain_runtime = crate::rain_forecast::project_admitted_rain_adjustment_runtime(
        rain_run.as_ref(),
        &safety_at,
        boundaries.forecast_rain_runtime,
    );
    let response = ApiForecastResponse {
        adjustmentRuntime: adjustment_runtime,
        airQualityContext: air_quality_context,
        rainAdjustmentRuntime: rain_runtime,
        data,
        days: forecast_days(selection),
        generatedAt: data_at.utc.clone(),
        pressureContext: project_forecast_pressure_context(
            pressure_rows,
            &site,
            &data_at,
            boundaries.time_resolver,
        ),
        site,
        temperatureAdjustmentRuntime: temperature_runtime,
    };
    success(WeatherServiceBody::Forecast(Box::new(response)))
}

/// execute one fixed-calendar trend read and canonical response projection
fn serve_trends(
    repository: &mut dyn WeatherServiceRepository,
    boundaries: &WeatherServiceBoundaries<'_>,
    query: &[QueryParameter<'_>],
    site: weather_contracts::ApiSite,
    data_at: CanonicalInstant,
) -> WeatherServiceResponse {
    // preserve legacy validation without changing the fixed calendar window
    if let Err(error) = parse_legacy_trend_range(query) {
        return public_error(error);
    }
    let parsed = match calendar_trend_query(&site, &data_at, boundaries.trends) {
        Ok(parsed) => parsed,
        Err(_) => return internal_error(),
    };
    let plan = match trends_read_plan(&parsed) {
        Ok(plan) => plan,
        Err(_) => return internal_error(),
    };
    let rows = match repository.trends(plan) {
        Ok(rows) => rows,
        Err(_) => return internal_error(),
    };
    let response = match project_trends_response(rows, site, &data_at, boundaries.trends) {
        Ok(response) => response,
        Err(_) => return internal_error(),
    };
    success(WeatherServiceBody::Trends(response))
}

/// execute one nearest-gauge local-day precipitation read and projection
fn serve_daily_precipitation(
    repository: &mut dyn WeatherServiceRepository,
    boundaries: &WeatherServiceBoundaries<'_>,
    query: &[QueryParameter<'_>],
    site: weather_contracts::ApiSite,
    data_at: CanonicalInstant,
) -> WeatherServiceResponse {
    // preserve the fixed query surface after active-site lookup
    if let Some(parameter) = query.first() {
        return public_error(unsupported_query(parameter.name));
    }
    let window = match boundaries
        .daily_precipitation
        .resolve_window(&data_at, &site.timezone)
    {
        Ok(window) => window,
        Err(_) => return internal_error(),
    };

    // bind the admitted local-day window to this request's nominal data instant
    if window.to != data_at {
        return internal_error();
    }
    let query = match daily_precipitation_query(&site, &window) {
        Ok(query) => query,
        Err(_) => return internal_error(),
    };
    let plan = match daily_precipitation_read_plan(&AdmittedDailyPrecipitationQuery::admitted(
        query,
        window.from.epoch_milliseconds,
        window.to.epoch_milliseconds,
    )) {
        Ok(plan) => plan,
        Err(_) => return internal_error(),
    };
    let row = match repository.daily_precipitation(plan) {
        Ok(row) => row,
        Err(_) => return internal_error(),
    };
    let response = match project_daily_precipitation_response(
        row,
        &site,
        &data_at,
        boundaries.time_resolver,
    ) {
        Ok(response) => response,
        Err(_) => return internal_error(),
    };
    success(WeatherServiceBody::DailyPrecipitation(response))
}

/// execute one fixed-site sanitized rain collection status read
fn serve_rain_collection(
    repository: &mut dyn WeatherServiceRepository,
    boundaries: &WeatherServiceBoundaries<'_>,
    query: &[QueryParameter<'_>],
    site: weather_contracts::ApiSite,
    data_at: CanonicalInstant,
    safety_at: CanonicalInstant,
    rain_read_failed: Option<&mut bool>,
) -> WeatherServiceResponse {
    // preserve the fixed route's empty query surface after site discovery
    if let Some(parameter) = query.first() {
        return public_error(unsupported_query(parameter.name));
    }

    // refuse other active sites without implying collection support
    if site.slug != "ballydidean" {
        return public_error(not_found());
    }
    let rain_run = read_rain_adjustment_run(
        repository,
        boundaries,
        &site.slug,
        &data_at,
        &safety_at,
        rain_read_failed,
    );
    let status = match repository.rain_collection_status() {
        Ok(Some(status)) => status,
        Ok(None) => return public_error(not_found()),
        Err(_) => return internal_error(),
    };
    let model_enabled = crate::rain_forecast::admitted_rain_adjustment_model_enabled(
        rain_run.as_ref(),
        boundaries.forecast_rain_runtime,
    );
    let mut data = match project_rain_collection_status(status, boundaries.rain_collection) {
        Ok(data) => data,
        Err(_) => return internal_error(),
    };
    data.modelEnabled = model_enabled;
    success(WeatherServiceBody::RainCollection(
        ApiRainCollectionResponse {
            data,
            generatedAt: data_at.utc,
            version: boundaries.version.unwrap_or("development").to_owned(),
        },
    ))
}

/// Read optional retained rain output only when startup policy permits model serving.
fn read_rain_adjustment_run(
    repository: &mut dyn WeatherServiceRepository,
    boundaries: &WeatherServiceBoundaries<'_>,
    site_slug: &str,
    data_at: &CanonicalInstant,
    safety_at: &CanonicalInstant,
    rain_read_failed: Option<&mut bool>,
) -> Option<AdmittedRainAdjustmentRun> {
    // skip all retained-output effects after an explicit startup refusal
    if matches!(
        boundaries.forecast_rain_runtime,
        Some(AdmittedRainForecastRuntime::Disabled { .. })
    ) {
        return None;
    }
    let run = match repository.rain_adjustment_run(ForecastRainAdjustmentRead {
        site_slug: site_slug.to_owned(),
        as_of: data_at.utc.clone(),
    }) {
        Ok(run) => run,
        Err(_) => {
            // report one contained sidecar read or decode failure to the request owner
            if let Some(rain_read_failed) = rain_read_failed {
                *rain_read_failed = true;
            }
            return None;
        }
    };
    let run = run?;

    // treat an invalid or expired retained run like the source-sidecar fallback
    if !crate::rain_forecast::valid_rain_adjustment_run(&run, safety_at) {
        // collapse every rain-sidecar failure into the request's single marker
        if let Some(rain_read_failed) = rain_read_failed {
            *rain_read_failed = true;
        }
        return None;
    }
    Some(run)
}

/// project one readiness response without active-site discovery
fn serve_health(
    repository: &mut dyn WeatherServiceRepository,
    boundaries: &WeatherServiceBoundaries<'_>,
    safety_at: &CanonicalInstant,
) -> WeatherServiceResponse {
    let adjustment_refusal = boundaries.forecast_runtime.adjustment_refusal();
    let temperature_refusal = boundaries.forecast_runtime.temperature_refusal();
    let default_refusal = ForecastStartupRefusal::RegistryInactive;
    // preserve the admitted wind snapshot while evaluating its request-time lifecycle
    let adjustment_runtime = match (
        adjustment_refusal.is_none(),
        boundaries.forecast_wind_runtime,
    ) {
        (
            true,
            Some(AdmittedWindForecastServiceRuntime::Legacy {
                loaded_at, runtime, ..
            }),
        ) => match crate::forecast_projection::project_wind_canary_runtime_at(
            loaded_at,
            runtime,
            safety_at.epoch_milliseconds,
        ) {
            Ok(runtime) => runtime,
            Err(_) => return internal_error(),
        },
        (
            true,
            Some(
                AdmittedWindForecastServiceRuntime::Maintenance {
                    loaded_at,
                    adjustment,
                }
                | AdmittedWindForecastServiceRuntime::Standard {
                    loaded_at,
                    adjustment,
                },
            ),
        ) => adjustment.project_runtime(loaded_at),
        // prefer the startup refusal over any supplied active runtime
        _ => inactive_adjustment_runtime(
            boundaries.forecast_runtime.adjustment_loaded_at(),
            adjustment_refusal
                .unwrap_or(default_refusal)
                .adjustment_runtime_reason(),
        ),
    };
    let temperature_runtime = match (
        temperature_refusal.is_none(),
        boundaries.forecast_temperature_runtime,
    ) {
        (true, Some(active)) => {
            // read fixed-site collector status only while request-time authorization stays active
            let status = if active
                .runtime
                .authorization
                .is_active_at(safety_at.epoch_milliseconds)
            {
                repository
                    .temperature_canary_status(ForecastTemperatureStatusRead {
                        site_slug: "ballydidean".to_owned(),
                        as_of: safety_at.utc.clone(),
                    })
                    .unwrap_or(None)
            } else {
                None
            };
            project_temperature_canary_runtime(active.loaded_at, active.runtime, safety_at, status)
        }
        // skip collector status whenever startup rejected temperature activation
        _ => inactive_temperature_runtime(
            boundaries.forecast_runtime.temperature_loaded_at(),
            temperature_refusal
                .unwrap_or(default_refusal)
                .temperature_runtime_reason(),
        ),
    };
    let snapshot = repository.health().unwrap_or(HealthSnapshot {
        database: DatabaseStatus::Unavailable,
        migration_status: MigrationStatus::Unavailable,
        migration_version: None,
        worker_last_loop_epoch_ms: None,
    });
    let projection = project_health(
        snapshot,
        safety_at.epoch_milliseconds,
        boundaries.version.unwrap_or("development"),
    );
    let body = ApiHealthResponse {
        data: ApiHealthResponseData {
            adjustmentRuntime: adjustment_runtime,
            temperatureAdjustmentRuntime: temperature_runtime,
            database: health_database(projection.database),
            live: ApiHealthResponseDataLiveLiteral,
            migration: ApiHealthResponseDataMigration {
                status: health_migration(projection.migration_status),
                version: match projection.migration_version {
                    Some(version) => Nullable::Value(version),
                    None => Nullable::Null,
                },
            },
            ready: projection.ready,
            version: projection.version,
            worker: ApiHealthResponseDataWorker {
                freshness: health_worker_freshness(projection.worker_freshness),
            },
        },
    };
    WeatherServiceResponse {
        status: projection.status,
        allow_get_head: false,
        body: Some(WeatherServiceBody::Health(Box::new(body))),
    }
}

/// map one internal database status to the canonical DTO enum
fn health_database(status: DatabaseStatus) -> ApiHealthResponseDataDatabaseEnum {
    // preserve the two public database states
    match status {
        DatabaseStatus::Ready => ApiHealthResponseDataDatabaseEnum::Ready,
        DatabaseStatus::Unavailable => ApiHealthResponseDataDatabaseEnum::Unavailable,
    }
}

/// map one internal migration status to the canonical DTO enum
fn health_migration(status: MigrationStatus) -> ApiHealthResponseDataMigrationStatusEnum {
    // preserve every public migration state
    match status {
        MigrationStatus::Current => ApiHealthResponseDataMigrationStatusEnum::Current,
        MigrationStatus::Outdated => ApiHealthResponseDataMigrationStatusEnum::Outdated,
        MigrationStatus::Unavailable => ApiHealthResponseDataMigrationStatusEnum::Unavailable,
    }
}

/// map one internal heartbeat state to the canonical DTO enum
fn health_worker_freshness(freshness: WorkerFreshness) -> ApiHealthResponseDataWorkerFreshnessEnum {
    // preserve every public coarse heartbeat state
    match freshness {
        WorkerFreshness::Fresh => ApiHealthResponseDataWorkerFreshnessEnum::Fresh,
        WorkerFreshness::Stale => ApiHealthResponseDataWorkerFreshnessEnum::Stale,
        WorkerFreshness::Unknown => ApiHealthResponseDataWorkerFreshnessEnum::Unknown,
    }
}

/// classify raw forecast rows against the admitted public window
fn forecast_row_visibility(
    rows: &[WeatherRecordRow],
    public_start: &CanonicalInstant,
    public_end: &CanonicalInstant,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<Vec<bool>, crate::CurrentProjectionError> {
    // reject a malformed admitted public window before row projection
    if public_start.epoch_milliseconds >= public_end.epoch_milliseconds {
        return Err(crate::CurrentProjectionError {
            code: "service.forecast_window",
            detail: "forecast public window must be ordered".to_owned(),
        });
    }
    let mut visibility = Vec::with_capacity(rows.len());

    // classify every repository row without parsing serialized UTC text
    for row in rows {
        let valid_at = time_resolver.resolve(&row.validAt)?;
        visibility.push(
            valid_at.epoch_milliseconds >= public_start.epoch_milliseconds
                && valid_at.epoch_milliseconds < public_end.epoch_milliseconds,
        );
    }
    Ok(visibility)
}

/// map one source selector to its canonical response literal
fn forecast_days(selection: crate::ForecastSelection) -> ForecastDays {
    // preserve the three source-owned numeric alternatives
    match selection {
        crate::ForecastSelection::Daily { days: 1 } | crate::ForecastSelection::Overnight => {
            ForecastDays::Value1
        }
        crate::ForecastSelection::Daily { days: 5 } => ForecastDays::Value5,
        crate::ForecastSelection::Daily { days: 10 } => ForecastDays::Value10,
        crate::ForecastSelection::Daily { .. } => {
            unreachable!("forecast parser admits only 1, 5, or 10 days")
        }
    }
}

/// project one inactive adjustment lifecycle snapshot
fn inactive_adjustment_runtime(
    loaded_at: &str,
    reason_code: ApiForecastAdjustmentRuntimeReasonCodeEnum,
) -> ApiForecastAdjustmentRuntime {
    ApiForecastAdjustmentRuntime {
        actionSha256: FieldPresence::Missing,
        activationMode: Nullable::Null,
        activeBundle: Nullable::Null,
        authorizationSha256: Nullable::Null,
        candidateArtifactSha256: Nullable::Null,
        enabledMetrics: Vec::new(),
        evaluationReportSha256: Nullable::Null,
        expiresAt: Nullable::Null,
        fullMemberRootSha256: FieldPresence::Missing,
        loadedAt: loaded_at.to_owned(),
        qualificationReceiptSha256: Nullable::Null,
        reasonCode: Nullable::Value(reason_code),
        state: ApiForecastAdjustmentRuntimeStateEnum::Disabled,
        policyReportSha256: FieldPresence::Missing,
        transferReportSha256: Nullable::Null,
    }
}

/// project one inactive temperature lifecycle snapshot
fn inactive_temperature_runtime(
    loaded_at: &str,
    reason_code: ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum,
) -> ApiForecastTemperatureAdjustmentRuntime {
    ApiForecastTemperatureAdjustmentRuntime {
        actionSha256: Nullable::Null,
        activationMode: Nullable::Null,
        activeBundle: Nullable::Null,
        authorizationSha256: Nullable::Null,
        expiresAt: Nullable::Null,
        fullMemberRootSha256: Nullable::Null,
        loadedAt: loaded_at.to_owned(),
        policyReportSha256: Nullable::Null,
        reasonCode: Nullable::Value(reason_code),
        source: Nullable::Null,
        state: ApiForecastTemperatureAdjustmentRuntimeStateEnum::Disabled,
    }
}

/// extract one site slug only from site-scoped routes
fn route_site_slug(route: &Route) -> Option<&str> {
    // preserve the complete site-scoped route set before support selection
    match route {
        Route::Current { site_slug }
        | Route::DailyPrecipitation { site_slug }
        | Route::Forecast { site_slug }
        | Route::RainCollection { site_slug }
        | Route::History { site_slug }
        | Route::Tides { site_slug }
        | Route::Trends { site_slug } => Some(site_slug),
        Route::ForecastObservationInput | Route::Health | Route::Sites => None,
    }
}

/// build one successful typed response before outer HEAD suppression
fn success(body: WeatherServiceBody) -> WeatherServiceResponse {
    WeatherServiceResponse {
        status: 200,
        allow_get_head: false,
        body: Some(body),
    }
}

/// map one bounded public application error
fn public_error(error: ErrorDispatch) -> WeatherServiceResponse {
    let body = WeatherServiceBody::Error(ApiErrorResponse {
        error: ApiErrorResponseError {
            code: error.code.to_owned(),
            message: error.message,
        },
    });
    WeatherServiceResponse {
        status: error.status,
        allow_get_head: error.allow_get_head,
        body: Some(body),
    }
}

/// contain ordinary repository, clock, plan, and projection failures
fn internal_error() -> WeatherServiceResponse {
    public_error(ErrorDispatch {
        status: 500,
        code: "internal_error",
        message: "Unexpected server error".to_owned(),
        allow_get_head: false,
    })
}

/// identify a matched route that this service slice does not yet implement
fn unsupported_route() -> WeatherServiceResponse {
    public_error(ErrorDispatch {
        status: 501,
        code: "route_not_implemented",
        message: "Matched route is not implemented by the Rust service".to_owned(),
        allow_get_head: false,
    })
}

/// hide unsupported fixed-site public routes behind the Node not-found response
fn not_found() -> ErrorDispatch {
    ErrorDispatch {
        status: 404,
        code: "not_found",
        message: "Endpoint not found".to_owned(),
        allow_get_head: false,
    }
}

/// reject one decoded query name outside a fixed route surface
fn unsupported_query(name: &str) -> ErrorDispatch {
    ErrorDispatch {
        status: 400,
        code: "invalid_query",
        message: format!("Unsupported query parameter: {name}"),
        allow_get_head: false,
    }
}

/// preserve the existing public APIs' final response-wide HEAD stripping
fn suppress_head_body(
    mut response: WeatherServiceResponse,
    omit_body: bool,
) -> WeatherServiceResponse {
    // Strip only after the typed representation has completed.
    if omit_body {
        response.body = None;
    }
    response
}
