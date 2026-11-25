//! Construct the real API handlers before handing them to the admitted listeners.

use std::num::NonZeroU16;
use std::sync::Arc;

use weather_api::{
    CurrentTimeResolver, ForecastMaintenanceWindModelAdmissionBoundary,
    ForecastTemperatureMosAdmissionBoundary, ForecastWindModelAdmissionBoundary,
    MaintenanceWindModelCalendar, RepositoryWindModelAdmission,
    WeatherApiMaintenanceStartupPreparation, WeatherApiStartupPortError,
    WeatherApiStartupSnapshots,
};

use crate::{
    ApiProcessMigrationSource, LoadedApiMaintenanceCandidates, PooledWeatherApiReads,
    PostgresWeatherServiceRepository, PreparedPrivateMaintenanceApi, PreparedPublicWeatherApi,
    RuntimeMigrationSource, WeatherApiApplicationBinding, WeatherApiMaintenancePool,
};

/// one indivisible process opt-in for cached legacy-wind request admission
struct LegacyWindProcessAdmission {
    time: Arc<dyn CurrentTimeResolver + Send + Sync>,
    rows: Arc<dyn ForecastWindModelAdmissionBoundary + Send + Sync>,
}

/// one process opt-in for the authoritative installed wind startup grammar
struct WindProcessAdmission {
    time: Arc<dyn CurrentTimeResolver + Send + Sync>,
    legacy_rows: Arc<dyn ForecastWindModelAdmissionBoundary + Send + Sync>,
    v2_rows: Arc<dyn ForecastMaintenanceWindModelAdmissionBoundary + Send + Sync>,
}

/// installed startup grammar admitted for public temperature requests
#[derive(Clone, Copy)]
enum TemperatureStartupSelection {
    Legacy,
    Installed,
}

/// one indivisible process opt-in for cached temperature request admission
struct TemperatureProcessAdmission {
    time: Arc<dyn CurrentTimeResolver + Send + Sync>,
    rows: Arc<dyn ForecastTemperatureMosAdmissionBoundary + Send + Sync>,
    selection: TemperatureStartupSelection,
}

/// Listener owner receives only the application admitted for that listening endpoint.
pub trait WeatherApiPreparedListeners<Pool, External> {
    /// Listen for public requests with the prepared public handler.
    fn listen_public(
        &mut self,
        port: NonZeroU16,
        application: PreparedPublicWeatherApi<Pool, External>,
    ) -> Result<(), WeatherApiStartupPortError>;

    /// Listen for private maintenance with its own admitted handler.
    fn listen_maintenance(
        &mut self,
        port: NonZeroU16,
        application: PreparedPrivateMaintenanceApi<Pool>,
    ) -> Result<(), WeatherApiStartupPortError>;
}

/// Retain ready handlers until startup transfers each to its approved listener.
pub struct WeatherApiProcessApplications<Pool, External, ExternalFactory, Listeners> {
    build_external: ExternalFactory,
    listeners: Listeners,
    public: Option<PreparedPublicWeatherApi<Pool, External>>,
    maintenance: Option<PreparedPrivateMaintenanceApi<Pool>>,
    legacy_wind: Option<LegacyWindProcessAdmission>,
    wind: Option<WindProcessAdmission>,
    temperature: Option<TemperatureProcessAdmission>,
}

impl<Pool, External, ExternalFactory, Listeners>
    WeatherApiProcessApplications<Pool, External, ExternalFactory, Listeners>
{
    /// Bind the process's migration/time values and listener owners without opening sockets.
    pub const fn new(build_external: ExternalFactory, listeners: Listeners) -> Self
    where
        ExternalFactory:
            FnMut(ApiProcessMigrationSource) -> Result<External, WeatherApiStartupPortError>,
        Listeners: WeatherApiPreparedListeners<Pool, External>,
    {
        Self {
            build_external,
            listeners,
            public: None,
            maintenance: None,
            legacy_wind: None,
            wind: None,
            temperature: None,
        }
    }

    /// opt into startup-cached legacy-wind admission for public forecast requests
    pub fn with_legacy_wind_admission(
        mut self,
        time: Arc<dyn CurrentTimeResolver + Send + Sync>,
        rows: Arc<dyn ForecastWindModelAdmissionBoundary + Send + Sync>,
    ) -> Self {
        self.legacy_wind = Some(LegacyWindProcessAdmission { time, rows });
        self
    }

    /// opt into the installed authoritative adjustment startup for public requests
    pub fn with_wind_admission(
        mut self,
        time: Arc<dyn CurrentTimeResolver + Send + Sync>,
        legacy_rows: Arc<dyn ForecastWindModelAdmissionBoundary + Send + Sync>,
        v2_rows: Arc<dyn ForecastMaintenanceWindModelAdmissionBoundary + Send + Sync>,
    ) -> Self {
        self.wind = Some(WindProcessAdmission {
            time,
            legacy_rows,
            v2_rows,
        });
        self
    }

    /// derive every installed adjustment grammar from decoded repository rows
    pub fn with_repository_wind_admission(
        self,
        time: Arc<dyn CurrentTimeResolver + Send + Sync>,
        calendar: Arc<dyn MaintenanceWindModelCalendar + Send + Sync>,
    ) -> Self {
        let rows = Arc::new(RepositoryWindModelAdmission::new(
            Arc::clone(&time),
            calendar,
        ));
        self.with_wind_admission(time, rows.clone(), rows)
    }

    /// opt into startup-cached legacy-temperature admission for public forecast requests
    pub fn with_legacy_temperature_admission(
        mut self,
        time: Arc<dyn CurrentTimeResolver + Send + Sync>,
        rows: Arc<dyn ForecastTemperatureMosAdmissionBoundary + Send + Sync>,
    ) -> Self {
        self.temperature = Some(TemperatureProcessAdmission {
            time,
            rows,
            selection: TemperatureStartupSelection::Legacy,
        });
        self
    }

    /// opt into the installed authoritative temperature startup for public requests
    pub fn with_temperature_admission(
        mut self,
        time: Arc<dyn CurrentTimeResolver + Send + Sync>,
        rows: Arc<dyn ForecastTemperatureMosAdmissionBoundary + Send + Sync>,
    ) -> Self {
        self.temperature = Some(TemperatureProcessAdmission {
            time,
            rows,
            selection: TemperatureStartupSelection::Installed,
        });
        self
    }

    /// Return the listener owner, which owns final connection draining.
    pub fn into_listeners(self) -> Listeners {
        self.listeners
    }
}

impl<Pool, External, ExternalFactory, Listeners> WeatherApiApplicationBinding<Pool>
    for WeatherApiProcessApplications<Pool, External, ExternalFactory, Listeners>
where
    Pool: WeatherApiMaintenancePool,
    ExternalFactory:
        FnMut(ApiProcessMigrationSource) -> Result<External, WeatherApiStartupPortError>,
    Listeners: WeatherApiPreparedListeners<Pool, External>,
{
    /// Assemble the actual SQL repository and public app over the only process pool.
    fn prepare_public(
        &mut self,
        pool: Pool,
        migration: ApiProcessMigrationSource,
        snapshots: &Arc<WeatherApiStartupSnapshots>,
    ) -> Result<(), WeatherApiStartupPortError> {
        let release = migration.release().to_owned();
        let external = (self.build_external)(migration)?;
        let repository =
            PostgresWeatherServiceRepository::new(PooledWeatherApiReads::new(pool), external);
        let mut application =
            PreparedPublicWeatherApi::new(repository, Arc::clone(snapshots), release);
        // prefer the combined installed-grammar binding when explicitly configured
        if let Some(binding) = &self.wind {
            application
                .bind_wind_startup(
                    binding.time.as_ref(),
                    Arc::clone(&binding.legacy_rows),
                    Arc::clone(&binding.v2_rows),
                )
                .map_err(|_| WeatherApiStartupPortError)?;
        } else if let Some(binding) = &self.legacy_wind {
            // preserve the predecessor's legacy-only opt-in
            application
                .bind_legacy_wind_startup(binding.time.as_ref(), Arc::clone(&binding.rows))
                .map_err(|_| WeatherApiStartupPortError)?;
        }
        // bind only when the process explicitly supplies both production boundaries
        if let Some(binding) = &self.temperature {
            // prepare exactly the startup grammar selected by the process builder
            match binding.selection {
                TemperatureStartupSelection::Legacy => application
                    .bind_legacy_temperature_startup(
                        binding.time.as_ref(),
                        Arc::clone(&binding.rows),
                    )
                    .map_err(|_| WeatherApiStartupPortError)?,
                TemperatureStartupSelection::Installed => application
                    .bind_temperature_startup(binding.time.as_ref(), Arc::clone(&binding.rows))
                    .map_err(|_| WeatherApiStartupPortError)?,
            }
        }
        self.public = Some(application);
        Ok(())
    }

    /// Assemble the actual private handler with the same pool and admitted capture material.
    fn prepare_maintenance(
        &mut self,
        pool: Pool,
        candidates: LoadedApiMaintenanceCandidates,
        snapshots: &Arc<WeatherApiStartupSnapshots>,
        preparation: &WeatherApiMaintenanceStartupPreparation,
    ) -> Result<(), WeatherApiStartupPortError> {
        // preserve public-before-private preparation without allocating another model pack
        if self.public.is_none() {
            return Err(WeatherApiStartupPortError);
        }
        self.maintenance = Some(PreparedPrivateMaintenanceApi::new(
            pool,
            preparation.clone(),
            Arc::clone(snapshots),
            candidates,
        ));
        Ok(())
    }

    /// Transfer the already prepared public application exactly once.
    fn listen_public(&mut self, port: NonZeroU16) -> Result<(), WeatherApiStartupPortError> {
        let application = self.public.take().ok_or(WeatherApiStartupPortError)?;
        self.listeners.listen_public(port, application)
    }

    /// Transfer the admitted private application without exposing it to the public owner.
    fn listen_maintenance(&mut self, port: NonZeroU16) -> Result<(), WeatherApiStartupPortError> {
        let application = self.maintenance.take().ok_or(WeatherApiStartupPortError)?;
        self.listeners.listen_maintenance(port, application)
    }
}
