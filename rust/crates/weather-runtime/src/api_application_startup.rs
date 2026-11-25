//! Retain one API read pool while binding public and optional maintenance applications.

use std::num::NonZeroU16;
use std::sync::Arc;

use weather_api::{
    WeatherApiMaintenanceStartupPreparation, WeatherApiStartupApplicationPort,
    WeatherApiStartupPortError, WeatherApiStartupSnapshots,
};
use weather_database::to_pool_configuration;

use crate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceCandidateFiles, ApiProcessConfiguration,
    ApiProcessMigrationSource, LoadedApiMaintenanceCandidates, WeatherApiReadPoolConnector,
    WeatherApiSharedPool, load_api_maintenance_candidates,
};

/// Approved application binding that owns listener construction and retained request handles.
pub trait WeatherApiApplicationBinding<Pool> {
    /// Bind the public application to one shared pool handle and migration source.
    fn prepare_public(
        &mut self,
        pool: Pool,
        migration: ApiProcessMigrationSource,
        snapshots: &Arc<WeatherApiStartupSnapshots>,
    ) -> Result<(), WeatherApiStartupPortError>;

    /// Bind the optional maintenance application after its witness and settings are admitted.
    fn prepare_maintenance(
        &mut self,
        pool: Pool,
        candidates: LoadedApiMaintenanceCandidates,
        snapshots: &Arc<WeatherApiStartupSnapshots>,
        preparation: &WeatherApiMaintenanceStartupPreparation,
    ) -> Result<(), WeatherApiStartupPortError>;

    /// Start the already prepared public listener on its admitted port.
    fn listen_public(&mut self, port: NonZeroU16) -> Result<(), WeatherApiStartupPortError>;

    /// Start the already prepared maintenance listener on its admitted port.
    fn listen_maintenance(&mut self, port: NonZeroU16) -> Result<(), WeatherApiStartupPortError>;
}

/// Startup adapter retaining configuration owners, one pool, and both application bindings.
pub struct RetainedWeatherApiStartup<Connector, Files, Codec, Binding>
where
    Connector: WeatherApiReadPoolConnector,
{
    configuration: Option<ApiProcessConfiguration>,
    connector: Connector,
    files: Files,
    codec: Codec,
    binding: Binding,
    pool: Option<Connector::Pool>,
}

impl<Connector, Files, Codec, Binding> RetainedWeatherApiStartup<Connector, Files, Codec, Binding>
where
    Connector: WeatherApiReadPoolConnector,
{
    /// Retain all process owners without constructing a pool or listener early.
    pub fn new(
        configuration: ApiProcessConfiguration,
        connector: Connector,
        files: Files,
        codec: Codec,
        binding: Binding,
    ) -> Self {
        Self {
            configuration: Some(configuration),
            connector,
            files,
            codec,
            binding,
            pool: None,
        }
    }

    /// Return the binding owner after startup for ordinary lifecycle inspection or shutdown.
    pub fn into_binding(self) -> Binding {
        self.binding
    }
}

impl<Connector, Files, Codec, Binding> WeatherApiStartupApplicationPort
    for RetainedWeatherApiStartup<Connector, Files, Codec, Binding>
where
    Connector: WeatherApiReadPoolConnector,
    Connector::Pool: WeatherApiSharedPool,
    Files: ApiMaintenanceCandidateFiles,
    Codec: ApiMaintenanceCandidateCodec,
    Binding: WeatherApiApplicationBinding<Connector::Pool>,
{
    /// Prepare exactly one retained pool before handing a shared handle to the public binder.
    fn prepare_public(
        &mut self,
        snapshots: &Arc<WeatherApiStartupSnapshots>,
    ) -> Result<(), WeatherApiStartupPortError> {
        let configuration = self
            .configuration
            .take()
            .ok_or(WeatherApiStartupPortError)?;
        let (database, migration) = configuration.into_parts();
        let pool = self
            .connector
            .prepare_pool(to_pool_configuration(&database))
            .map_err(|_| WeatherApiStartupPortError)?;
        self.pool = Some(pool);
        let public_pool = self
            .pool
            .as_ref()
            .map(WeatherApiSharedPool::share)
            .ok_or(WeatherApiStartupPortError)?;
        self.binding
            .prepare_public(public_pool, migration, snapshots)
    }

    /// Load immutable candidates only after private witness and listener admission succeeds.
    fn prepare_maintenance(
        &mut self,
        snapshots: &Arc<WeatherApiStartupSnapshots>,
        preparation: &WeatherApiMaintenanceStartupPreparation,
    ) -> Result<(), WeatherApiStartupPortError> {
        let candidates = load_api_maintenance_candidates(&mut self.files, &mut self.codec);
        let maintenance_pool = self
            .pool
            .as_ref()
            .map(WeatherApiSharedPool::share)
            .ok_or(WeatherApiStartupPortError)?;
        self.binding
            .prepare_maintenance(maintenance_pool, candidates, snapshots, preparation)
    }

    /// Delegate public listening only after the public application is prepared.
    fn listen_public(&mut self, port: NonZeroU16) -> Result<(), WeatherApiStartupPortError> {
        self.binding.listen_public(port)
    }

    /// Delegate maintenance listening after Node's public-first listener transition.
    fn listen_maintenance(&mut self, port: NonZeroU16) -> Result<(), WeatherApiStartupPortError> {
        self.binding.listen_maintenance(port)
    }
}
