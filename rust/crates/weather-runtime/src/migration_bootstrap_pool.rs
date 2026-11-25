use weather_database::{
    BootstrapDriverError, BootstrapError, BootstrapJsonEncoder, BootstrapPool,
    BootstrapPublicStationReceipt, BootstrapSiteReceipt, BootstrapTideReceipt,
    CheckedMigrationError, CheckedMigrationSession, MigrationArtifactLoadError,
    MigrationCommandPort, SensorBootstrapReceipt, bootstrap_ecowitt_configuration,
    bootstrap_public_station_configuration, bootstrap_site_configuration,
    bootstrap_tempest_configuration, bootstrap_tide_configuration,
};

use crate::{
    CheckedMigrationCommandPool, MigrationCommandBootstrapPort, WorkerJsonConfigurationOwner,
};

/// One retained driver pool supporting both migration and typed bootstrap sessions.
pub trait MigrationBootstrapDriverPool: BootstrapPool + Sized {
    type MigrationSession: CheckedMigrationSession<Error = BootstrapDriverError<Self>>;

    /// Acquire the migration session from the same underlying process pool.
    fn checkout_migration(&mut self) -> Result<Self::MigrationSession, BootstrapDriverError<Self>>;

    /// Close the retained process pool after migration and all bootstraps.
    fn close(&mut self) -> Result<(), BootstrapDriverError<Self>>;
}

/// Complete owned migration/bootstrap adapter failure.
#[derive(Debug)]
pub enum MigrationBootstrapPoolError<DriverError, EncoderError, LoaderError> {
    Command(CheckedMigrationError<DriverError, MigrationArtifactLoadError<std::io::Error>>),
    Bootstrap(BootstrapError<DriverError, EncoderError>),
    Load(LoaderError),
    Driver(DriverError),
}

/// Retain one driver pool, generated configuration loader, and JSON owner for the command.
pub struct MigrationBootstrapPool<Driver, Loader, Encoder> {
    driver: Driver,
    loader: Loader,
    encoder: Encoder,
}

impl<Driver, Loader, Encoder> MigrationBootstrapPool<Driver, Loader, Encoder> {
    /// Compose the three approved owners without opening another database pool.
    pub const fn new(driver: Driver, loader: Loader, encoder: Encoder) -> Self {
        Self {
            driver,
            loader,
            encoder,
        }
    }

    /// Return all owners after a command-specific lifecycle has completed.
    pub fn into_parts(self) -> (Driver, Loader, Encoder) {
        (self.driver, self.loader, self.encoder)
    }
}

impl<Driver, Loader, Encoder> MigrationCommandBootstrapPort
    for MigrationBootstrapPool<Driver, Loader, Encoder>
where
    Driver: MigrationBootstrapDriverPool,
    Loader: WorkerJsonConfigurationOwner,
    Encoder: BootstrapJsonEncoder,
{
    type Error =
        MigrationBootstrapPoolError<BootstrapDriverError<Driver>, Encoder::Error, Loader::Error>;
    type SiteBootstrap = BootstrapSiteReceipt;
    type EcowittBootstrap = SensorBootstrapReceipt;
    type TempestBootstrap = SensorBootstrapReceipt;
    type TideBootstrap = BootstrapTideReceipt;
    type PublicStationBootstrap = BootstrapPublicStationReceipt;

    /// Load and bootstrap the base site catalog through the retained pool.
    fn bootstrap_site(
        &mut self,
        configuration_path: &str,
    ) -> Result<Self::SiteBootstrap, Self::Error> {
        let configuration = self
            .loader
            .load_site_configuration(configuration_path)
            .map_err(MigrationBootstrapPoolError::Load)?;
        bootstrap_site_configuration(&mut self.driver, &configuration, &mut self.encoder)
            .map_err(MigrationBootstrapPoolError::Bootstrap)
    }

    /// Load and bootstrap the Ecowitt sensor catalog through the retained pool.
    fn bootstrap_ecowitt(
        &mut self,
        configuration_path: &str,
    ) -> Result<Self::EcowittBootstrap, Self::Error> {
        let configuration = self
            .loader
            .load_ecowitt_configuration(configuration_path)
            .map_err(MigrationBootstrapPoolError::Load)?;
        bootstrap_ecowitt_configuration(&mut self.driver, &configuration, &mut self.encoder)
            .map_err(MigrationBootstrapPoolError::Bootstrap)
    }

    /// Load and bootstrap the Tempest sensor catalog through the retained pool.
    fn bootstrap_tempest(
        &mut self,
        configuration_path: &str,
    ) -> Result<Self::TempestBootstrap, Self::Error> {
        let configuration = self
            .loader
            .load_tempest_configuration(configuration_path)
            .map_err(MigrationBootstrapPoolError::Load)?;
        bootstrap_tempest_configuration(&mut self.driver, &configuration, &mut self.encoder)
            .map_err(MigrationBootstrapPoolError::Bootstrap)
    }

    /// Load and bootstrap the NOAA tide catalog through the retained pool.
    fn bootstrap_tide(
        &mut self,
        configuration_path: &str,
    ) -> Result<Self::TideBootstrap, Self::Error> {
        let configuration = self
            .loader
            .load_tide_configuration(configuration_path)
            .map_err(MigrationBootstrapPoolError::Load)?;
        bootstrap_tide_configuration(&mut self.driver, &configuration, &mut self.encoder)
            .map_err(MigrationBootstrapPoolError::Bootstrap)
    }

    /// Load and bootstrap the public-station catalog through the retained pool.
    fn bootstrap_public_stations(
        &mut self,
        configuration_path: &str,
    ) -> Result<Self::PublicStationBootstrap, Self::Error> {
        let configuration = self
            .loader
            .load_public_station_configuration(configuration_path)
            .map_err(MigrationBootstrapPoolError::Load)?;
        bootstrap_public_station_configuration(&mut self.driver, &configuration, &mut self.encoder)
            .map_err(MigrationBootstrapPoolError::Bootstrap)
    }

    /// Close the one retained driver pool after every command outcome.
    fn close_pool(&mut self) -> Result<(), Self::Error> {
        self.driver
            .close()
            .map_err(MigrationBootstrapPoolError::Driver)
    }
}

impl<Driver, Loader, Encoder> CheckedMigrationCommandPool
    for MigrationBootstrapPool<Driver, Loader, Encoder>
where
    Driver: MigrationBootstrapDriverPool,
    Loader: WorkerJsonConfigurationOwner,
    Encoder: BootstrapJsonEncoder,
{
    type Session = Driver::MigrationSession;

    /// Acquire one migration session from the retained bootstrap driver.
    fn acquire_migration_session(&mut self) -> Result<Self::Session, Self::Error> {
        self.driver
            .checkout_migration()
            .map_err(MigrationBootstrapPoolError::Driver)
    }

    /// Retain the checked migration failure without erasing its stage.
    fn migration_error(
        &mut self,
        error: CheckedMigrationError<
            <Self::Session as MigrationCommandPort>::Error,
            MigrationArtifactLoadError<std::io::Error>,
        >,
    ) -> Self::Error {
        MigrationBootstrapPoolError::Command(error)
    }
}
