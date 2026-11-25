//! Coordinate the one-shot migration entrypoint without owning database execution.

use std::path::Path;

use weather_database::{
    CheckedMigrationError, CheckedMigrationResult, CheckedMigrationSession,
    FilesystemCheckedMigrationCatalog, MigrationArtifactLoadError, MigrationArtifactNameOrder,
    MigrationArtifactSha256, MigrationMode, MigrationSessionTimeouts,
    ProcessMigrationArtifactFilesystem, run_checked_migrations,
};

/// environment key selecting the migration catalog directory.
pub const WEATHER_MIGRATION_DIRECTORY: &str = "WEATHER_MIGRATION_DIRECTORY";
/// environment key selecting the site configuration document.
pub const WEATHER_SITE_CONFIG_PATH: &str = "WEATHER_SITE_CONFIG_PATH";
/// environment key selecting the Ecowitt configuration document.
pub const WEATHER_ECOWITT_CONFIG_PATH: &str = "WEATHER_ECOWITT_CONFIG_PATH";
/// environment key selecting the Tempest configuration document.
pub const WEATHER_TEMPEST_CONFIG_PATH: &str = "WEATHER_TEMPEST_CONFIG_PATH";
/// environment key selecting the tide configuration document.
pub const WEATHER_TIDE_CONFIG_PATH: &str = "WEATHER_TIDE_CONFIG_PATH";
/// environment key selecting the public-station configuration document.
pub const WEATHER_PUBLIC_STATIONS_CONFIG_PATH: &str = "WEATHER_PUBLIC_STATIONS_CONFIG_PATH";

/// default migration catalog installed in the release image.
pub const DEFAULT_MIGRATION_DIRECTORY: &str = "/opt/weather/packages/database/migrations";
/// default site configuration installed in the release image.
pub const DEFAULT_SITE_CONFIG_PATH: &str = "/opt/weather/config/sites/ballydidean.json";
/// default Ecowitt configuration installed in the release image.
pub const DEFAULT_ECOWITT_CONFIG_PATH: &str = "/opt/weather/config/ecowitt/gateways.json";
/// default Tempest configuration installed in the release image.
pub const DEFAULT_TEMPEST_CONFIG_PATH: &str = "/opt/weather/config/tempest/stations.json";
/// default tide configuration installed in the release image.
pub const DEFAULT_TIDE_CONFIG_PATH: &str = "/opt/weather/config/tides/noaa.json";
/// default public-station configuration installed in the release image.
pub const DEFAULT_PUBLIC_STATIONS_CONFIG_PATH: &str =
    "/opt/weather/config/public-stations/stations.json";

/// invalid migration entrypoint argument sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidMigrationArguments;

/// parse only the separately reviewed migration bridge arguments.
pub fn parse_migration_command_arguments<Argument: AsRef<str>>(
    arguments: &[Argument],
) -> Result<MigrationMode, InvalidMigrationArguments> {
    match arguments {
        [] => Ok(MigrationMode::Ordinary),
        [argument] => match argument.as_ref() {
            "--atomic-maintenance-v14" => Ok(MigrationMode::AtomicMaintenanceV14),
            "--atomic-normal-21-23" => Ok(MigrationMode::AtomicNormal21To23),
            "--atomic-direct-18-23" => Ok(MigrationMode::AtomicDirect18To23),
            "--atomic-direct-18-23-index-600s" => Ok(MigrationMode::AtomicDirect18To23Index600s),
            _ => Err(InvalidMigrationArguments),
        },
        _ => Err(InvalidMigrationArguments),
    }
}

/// nullable process environment values used by the migration entrypoint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MigrationCommandEnvironment<'a> {
    pub migration_directory: Option<&'a str>,
    pub site_configuration_path: Option<&'a str>,
    pub ecowitt_configuration_path: Option<&'a str>,
    pub tempest_configuration_path: Option<&'a str>,
    pub tide_configuration_path: Option<&'a str>,
    pub public_station_configuration_path: Option<&'a str>,
}

/// resolved paths passed to the ordered migration lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigrationCommandPaths<'a> {
    pub migration_directory: &'a str,
    pub site_configuration_path: &'a str,
    pub ecowitt_configuration_path: &'a str,
    pub tempest_configuration_path: &'a str,
    pub tide_configuration_path: &'a str,
    pub public_station_configuration_path: &'a str,
}

impl<'a> MigrationCommandEnvironment<'a> {
    /// apply release-image defaults only when an environment value is absent.
    pub fn resolve(self) -> MigrationCommandPaths<'a> {
        MigrationCommandPaths {
            migration_directory: self
                .migration_directory
                .unwrap_or(DEFAULT_MIGRATION_DIRECTORY),
            site_configuration_path: self
                .site_configuration_path
                .unwrap_or(DEFAULT_SITE_CONFIG_PATH),
            ecowitt_configuration_path: self
                .ecowitt_configuration_path
                .unwrap_or(DEFAULT_ECOWITT_CONFIG_PATH),
            tempest_configuration_path: self
                .tempest_configuration_path
                .unwrap_or(DEFAULT_TEMPEST_CONFIG_PATH),
            tide_configuration_path: self
                .tide_configuration_path
                .unwrap_or(DEFAULT_TIDE_CONFIG_PATH),
            public_station_configuration_path: self
                .public_station_configuration_path
                .unwrap_or(DEFAULT_PUBLIC_STATIONS_CONFIG_PATH),
        }
    }
}

/// Source-ordered bootstrap operations using the same process-owned pool.
pub trait MigrationCommandBootstrapPort {
    type Error;
    type SiteBootstrap;
    type EcowittBootstrap;
    type TempestBootstrap;
    type TideBootstrap;
    type PublicStationBootstrap;

    /// load and bootstrap the configured site.
    fn bootstrap_site(
        &mut self,
        configuration_path: &str,
    ) -> Result<Self::SiteBootstrap, Self::Error>;

    /// load and bootstrap the configured Ecowitt catalog.
    fn bootstrap_ecowitt(
        &mut self,
        configuration_path: &str,
    ) -> Result<Self::EcowittBootstrap, Self::Error>;

    /// load and bootstrap the configured Tempest catalog.
    fn bootstrap_tempest(
        &mut self,
        configuration_path: &str,
    ) -> Result<Self::TempestBootstrap, Self::Error>;

    /// load and bootstrap the configured tide catalog.
    fn bootstrap_tide(
        &mut self,
        configuration_path: &str,
    ) -> Result<Self::TideBootstrap, Self::Error>;

    /// load and bootstrap the configured public-station catalog.
    fn bootstrap_public_stations(
        &mut self,
        configuration_path: &str,
    ) -> Result<Self::PublicStationBootstrap, Self::Error>;

    /// close the process-owned database pool after every outcome.
    fn close_pool(&mut self) -> Result<(), Self::Error>;
}

/// Compatibility boundary for callers retaining their own migration implementation.
pub trait MigrationCommandLifecyclePort: MigrationCommandBootstrapPort {
    type Migrations;

    /// Run the selected migration mode against the configured artifact directory.
    fn run_migrations(
        &mut self,
        migration_directory: &str,
        mode: MigrationMode,
    ) -> Result<Self::Migrations, Self::Error>;
}

/// Connect the command to an acquired session from the same pool as the bootstraps.
pub trait CheckedMigrationCommandPool: MigrationCommandBootstrapPort {
    type Session: CheckedMigrationSession;

    /// Acquire the single driver session that holds the migration lock.
    fn acquire_migration_session(&mut self) -> Result<Self::Session, Self::Error>;

    /// Retain the owning migration failure category through the command error boundary.
    fn migration_error(
        &mut self,
        error: CheckedMigrationError<
            <Self::Session as weather_database::MigrationCommandPort>::Error,
            MigrationArtifactLoadError<std::io::Error>,
        >,
    ) -> Self::Error;
}

/// typed completion data projected by the process-owned JSON encoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationCommandCompletion<Migrations, Site, Ecowitt, Tempest, Tide, PublicStations> {
    pub migrations: Migrations,
    pub bootstrap: Site,
    pub ecowitt_bootstrap: Ecowitt,
    pub tempest_bootstrap: Tempest,
    pub tide_bootstrap: Tide,
    pub public_station_bootstrap: PublicStations,
}

/// result type for one port-specific migration command execution.
pub type MigrationCommandResult<Port> =
    MigrationCommandResultFor<Port, <Port as MigrationCommandLifecyclePort>::Migrations>;

/// Result of the command with either its retained or checked migration execution.
pub type MigrationCommandResultFor<Port, Migrations> = Result<
    MigrationCommandCompletion<
        Migrations,
        <Port as MigrationCommandBootstrapPort>::SiteBootstrap,
        <Port as MigrationCommandBootstrapPort>::EcowittBootstrap,
        <Port as MigrationCommandBootstrapPort>::TempestBootstrap,
        <Port as MigrationCommandBootstrapPort>::TideBootstrap,
        <Port as MigrationCommandBootstrapPort>::PublicStationBootstrap,
    >,
    <Port as MigrationCommandBootstrapPort>::Error,
>;

impl<Migrations, Site, Ecowitt, Tempest, Tide, PublicStations>
    MigrationCommandCompletion<Migrations, Site, Ecowitt, Tempest, Tide, PublicStations>
{
    /// expose the fixed completion event without owning JSON serialization.
    pub const EVENT: &'static str = "migrations_complete";
}

/// run one ordered migration lifecycle and close the pool like a JavaScript finally block.
pub fn run_migration_command<Port: MigrationCommandLifecyclePort>(
    port: &mut Port,
    mode: MigrationMode,
    paths: MigrationCommandPaths<'_>,
) -> MigrationCommandResult<Port> {
    run_migration_steps(port, mode, paths, Port::run_migrations)
}

/// Run the checked Rust migration engine over the selected original catalog, then bootstrap.
pub fn run_checked_migration_command<Pool, Order, Sha256>(
    pool: &mut Pool,
    mode: MigrationMode,
    paths: MigrationCommandPaths<'_>,
    order: &Order,
    sha256: &mut Sha256,
    timeouts: MigrationSessionTimeouts,
) -> MigrationCommandResultFor<Pool, CheckedMigrationResult>
where
    Pool: CheckedMigrationCommandPool,
    Order: MigrationArtifactNameOrder,
    Sha256: MigrationArtifactSha256,
{
    run_migration_steps(pool, mode, paths, |pool, directory, mode| {
        let mut session = pool.acquire_migration_session()?;
        let mut catalog = FilesystemCheckedMigrationCatalog {
            directory: Path::new(directory),
            filesystem: &ProcessMigrationArtifactFilesystem,
            order,
        };
        run_checked_migrations(&mut session, &mut catalog, sha256, mode, timeouts)
            .map_err(|error| pool.migration_error(error))
    })
}

/// Run migration before the source-ordered bootstraps and always close the pool.
fn run_migration_steps<Port: MigrationCommandBootstrapPort, Migrations>(
    port: &mut Port,
    mode: MigrationMode,
    paths: MigrationCommandPaths<'_>,
    migrate: impl FnOnce(&mut Port, &str, MigrationMode) -> Result<Migrations, Port::Error>,
) -> MigrationCommandResultFor<Port, Migrations> {
    let result = (|| {
        let migrations = migrate(port, paths.migration_directory, mode)?;
        let bootstrap = port.bootstrap_site(paths.site_configuration_path)?;
        let ecowitt_bootstrap = port.bootstrap_ecowitt(paths.ecowitt_configuration_path)?;
        let tempest_bootstrap = port.bootstrap_tempest(paths.tempest_configuration_path)?;
        let tide_bootstrap = port.bootstrap_tide(paths.tide_configuration_path)?;
        let public_station_bootstrap =
            port.bootstrap_public_stations(paths.public_station_configuration_path)?;
        Ok(MigrationCommandCompletion {
            migrations,
            bootstrap,
            ecowitt_bootstrap,
            tempest_bootstrap,
            tide_bootstrap,
            public_station_bootstrap,
        })
    })();
    // preserve finally semantics when both the command and pool close report failure
    port.close_pool()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Error {
        Site,
        Close,
    }

    #[derive(Default)]
    struct Port {
        events: Vec<String>,
        fail_site: bool,
        fail_close: bool,
    }

    impl MigrationCommandLifecyclePort for Port {
        type Migrations = &'static str;

        /// retain the selected mode and migration directory.
        fn run_migrations(
            &mut self,
            migration_directory: &str,
            mode: MigrationMode,
        ) -> Result<Self::Migrations, Self::Error> {
            self.events
                .push(format!("migrate:{migration_directory}:{mode:?}"));
            Ok("migrations")
        }
    }

    impl MigrationCommandBootstrapPort for Port {
        type Error = Error;
        type SiteBootstrap = &'static str;
        type EcowittBootstrap = &'static str;
        type TempestBootstrap = &'static str;
        type TideBootstrap = &'static str;
        type PublicStationBootstrap = &'static str;

        /// retain the site path and optionally fail this lifecycle step.
        fn bootstrap_site(
            &mut self,
            configuration_path: &str,
        ) -> Result<Self::SiteBootstrap, Self::Error> {
            self.events.push(format!("site:{configuration_path}"));
            // inject one ordinary site bootstrap failure
            if self.fail_site {
                return Err(Error::Site);
            }
            Ok("site")
        }

        /// retain the Ecowitt path.
        fn bootstrap_ecowitt(
            &mut self,
            configuration_path: &str,
        ) -> Result<Self::EcowittBootstrap, Self::Error> {
            self.events.push(format!("ecowitt:{configuration_path}"));
            Ok("ecowitt")
        }

        /// retain the Tempest path.
        fn bootstrap_tempest(
            &mut self,
            configuration_path: &str,
        ) -> Result<Self::TempestBootstrap, Self::Error> {
            self.events.push(format!("tempest:{configuration_path}"));
            Ok("tempest")
        }

        /// retain the tide path.
        fn bootstrap_tide(
            &mut self,
            configuration_path: &str,
        ) -> Result<Self::TideBootstrap, Self::Error> {
            self.events.push(format!("tide:{configuration_path}"));
            Ok("tide")
        }

        /// retain the public-station path.
        fn bootstrap_public_stations(
            &mut self,
            configuration_path: &str,
        ) -> Result<Self::PublicStationBootstrap, Self::Error> {
            self.events.push(format!("stations:{configuration_path}"));
            Ok("stations")
        }

        /// retain the final pool close and optionally fail it.
        fn close_pool(&mut self) -> Result<(), Self::Error> {
            self.events.push("close".to_owned());
            // inject one ordinary close failure
            if self.fail_close {
                return Err(Error::Close);
            }
            Ok(())
        }
    }

    /// map all five modes and run the complete default-path lifecycle in source order.
    #[test]
    fn maps_modes_and_runs_default_lifecycle() {
        let no_arguments: [&str; 0] = [];
        assert_eq!(
            parse_migration_command_arguments(&no_arguments),
            Ok(MigrationMode::Ordinary)
        );
        assert_eq!(
            parse_migration_command_arguments(&["--atomic-maintenance-v14"]),
            Ok(MigrationMode::AtomicMaintenanceV14)
        );
        assert_eq!(
            parse_migration_command_arguments(&["--atomic-normal-21-23"]),
            Ok(MigrationMode::AtomicNormal21To23)
        );
        assert_eq!(
            parse_migration_command_arguments(&["--atomic-direct-18-23"]),
            Ok(MigrationMode::AtomicDirect18To23)
        );
        assert_eq!(
            parse_migration_command_arguments(&["--atomic-direct-18-23-index-600s"]),
            Ok(MigrationMode::AtomicDirect18To23Index600s)
        );
        assert_eq!(
            parse_migration_command_arguments(&["--atomic"]),
            Err(InvalidMigrationArguments)
        );
        assert_eq!(
            parse_migration_command_arguments(&[
                "--atomic-maintenance-v14",
                "--atomic-normal-21-23",
            ]),
            Err(InvalidMigrationArguments)
        );

        let mut port = Port::default();
        let result = run_migration_command(
            &mut port,
            MigrationMode::AtomicNormal21To23,
            MigrationCommandEnvironment::default().resolve(),
        )
        .unwrap();
        assert_eq!(
            result,
            MigrationCommandCompletion {
                migrations: "migrations",
                bootstrap: "site",
                ecowitt_bootstrap: "ecowitt",
                tempest_bootstrap: "tempest",
                tide_bootstrap: "tide",
                public_station_bootstrap: "stations",
            }
        );
        assert_eq!(
            port.events,
            [
                format!("migrate:{DEFAULT_MIGRATION_DIRECTORY}:AtomicNormal21To23"),
                format!("site:{DEFAULT_SITE_CONFIG_PATH}"),
                format!("ecowitt:{DEFAULT_ECOWITT_CONFIG_PATH}"),
                format!("tempest:{DEFAULT_TEMPEST_CONFIG_PATH}"),
                format!("tide:{DEFAULT_TIDE_CONFIG_PATH}"),
                format!("stations:{DEFAULT_PUBLIC_STATIONS_CONFIG_PATH}"),
                "close".to_owned(),
            ]
        );
    }

    /// preserve empty overrides, stop after failure, and always close the pool.
    #[test]
    fn closes_after_failure_without_replacing_empty_environment_values() {
        let mut port = Port {
            fail_site: true,
            ..Port::default()
        };
        let result = run_migration_command(
            &mut port,
            MigrationMode::Ordinary,
            MigrationCommandEnvironment {
                migration_directory: Some(""),
                site_configuration_path: Some("/site.json"),
                ..MigrationCommandEnvironment::default()
            }
            .resolve(),
        );
        assert_eq!(result, Err(Error::Site));
        assert_eq!(
            port.events,
            [
                "migrate::Ordinary".to_owned(),
                "site:/site.json".to_owned(),
                "close".to_owned(),
            ]
        );

        let mut closing_port = Port {
            fail_site: true,
            fail_close: true,
            ..Port::default()
        };
        assert_eq!(
            run_migration_command(
                &mut closing_port,
                MigrationMode::Ordinary,
                MigrationCommandEnvironment::default().resolve(),
            ),
            Err(Error::Close)
        );
    }
}
