//! Compose the native API's installed models, one PostgreSQL pool, and concrete listener bindings.

use std::sync::Arc;

use weather_api::{
    StartedWeatherApi, WeatherApiStartupError, WeatherApiStartupPortError,
    WeatherApiStartupRuntimeLoader, start_weather_api_with_raw_listener_settings,
};

use crate::{
    ApiProcessConfigurationError, ApiProcessMigrationSource, CurrentApiProcessEnvironment,
    NativeApiMaintenanceCandidateFiles, NativeApiModelAdmission, NativeApiPreparedListeners,
    NativeEdgeOrigins, NativeHttpListenerConfiguration, NativePostgresApiConnector,
    NativePostgresApiExternal, NativePostgresPool, NativePostgresPublicHandler,
    NativeWirePrimitives, RetainedCaptureEpochWitnessLoader, RetainedWeatherApiStartup,
    WeatherApiProcessApplications, WeatherApiProcessClock, load_api_process_listener_settings,
    load_current_api_process_configuration, native_postgres_api_external,
};

const INSTALLED_APPLICATION_ROOT: &str = "/opt/weather";

/// The native API's concrete listeners retain the single shared PostgreSQL process pool.
pub type NativePostgresApiListeners =
    NativeApiPreparedListeners<NativePostgresPool, NativePostgresApiExternal>;

/// Distinguish process configuration and model preparation failures from the original startup phases.
#[derive(Debug)]
pub enum NativePostgresApiStartupError {
    Configuration(ApiProcessConfigurationError),
    InstalledModelRoot,
    ModelCalendar,
    Startup(WeatherApiStartupError),
}

/// The actually prepared application and HTTP handler after the original startup sequence.
pub struct StartedNativePostgresApi {
    pub started: StartedWeatherApi,
    pub listeners: NativePostgresApiListeners,
    pub public_handler: NativePostgresPublicHandler,
    pub models: Arc<NativeApiModelAdmission>,
}

/// Prepare the concrete external read values over the already admitted migration source.
fn build_external(
    source: ApiProcessMigrationSource,
) -> Result<NativePostgresApiExternal, WeatherApiStartupPortError> {
    Ok(native_postgres_api_external(source))
}

/// Load the actual environment, bind shared runtime model owners, and prepare the native listeners.
///
/// The caller supplies the installed runtime loader with its independently admitted calendar
/// fingerprint authority. The startup never substitutes a raw runtime for an admitted active model.
pub fn prepare_native_postgres_api(
    loader: &mut dyn WeatherApiStartupRuntimeLoader,
    public_limits: NativeHttpListenerConfiguration,
    maintenance_limits: NativeHttpListenerConfiguration,
) -> Result<StartedNativePostgresApi, NativePostgresApiStartupError> {
    let configuration = load_current_api_process_configuration()
        .map_err(NativePostgresApiStartupError::Configuration)?;
    let migration = configuration.migration.clone();
    let files = NativeApiMaintenanceCandidateFiles::new(INSTALLED_APPLICATION_ROOT)
        .map_err(|_| NativePostgresApiStartupError::InstalledModelRoot)?;
    let models = Arc::new(
        NativeApiModelAdmission::production()
            .map_err(|_| NativePostgresApiStartupError::ModelCalendar)?,
    );
    let listeners = NativeApiPreparedListeners::production(public_limits, maintenance_limits);
    let applications = WeatherApiProcessApplications::new(build_external, listeners)
        .with_repository_wind_admission(models.clone(), models.clone())
        .with_temperature_admission(models.clone(), models.clone());
    let mut application = RetainedWeatherApiStartup::new(
        configuration,
        NativePostgresApiConnector,
        files,
        NativeWirePrimitives,
        applications,
    );
    let mut settings =
        load_api_process_listener_settings(&CurrentApiProcessEnvironment, NativeEdgeOrigins)
            .map_err(NativePostgresApiStartupError::Configuration)?;
    let mut clock = WeatherApiProcessClock::current_process(NativeWirePrimitives);
    let mut witness_loader =
        RetainedCaptureEpochWitnessLoader::native(NativeWirePrimitives, NativeWirePrimitives);
    let started = start_weather_api_with_raw_listener_settings(
        loader,
        &mut clock,
        &mut witness_loader,
        &mut application,
        &mut settings,
    )
    .map_err(NativePostgresApiStartupError::Startup)?;
    Ok(StartedNativePostgresApi {
        started,
        listeners: application.into_binding().into_listeners(),
        public_handler: NativePostgresPublicHandler::with_time_values(
            migration,
            models.service_time_values().clone(),
        ),
        models,
    })
}
