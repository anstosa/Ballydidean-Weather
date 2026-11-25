use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use weather_contracts::{
    EcowittConfiguration, EcowittConfigurationProvider, Nullable, PublicStationConfiguration,
    SiteConfiguration, SiteConfigurationProvider, SiteConfigurationSite, SiteConfigurationStation,
    StationKind, TempestConfiguration, TideConfiguration, TideProviderConfiguration,
};
use weather_database::{AdmittedHealthMigrationAuthorization, DatabaseConfiguration};
use weather_runtime::{
    DatabaseWorkerBootConfiguration, MigrationAuthorizationAdmissionError,
    TemperatureCanaryKillSwitch, WorkerCompatibilityOriginOwner, WorkerConfigurationArtifactOwner,
    WorkerConfigurationEnvironment, WorkerConfigurationEnvironmentKey, WorkerConfigurationOwners,
    WorkerCredentialOwner, WorkerDatabaseConfigurationOwner, WorkerJsonConfigurationOwner,
    WorkerMigrationSource, load_worker_configuration,
};

#[derive(Clone, Default)]
struct Fixture(Rc<RefCell<Vec<&'static str>>>);

impl WorkerConfigurationEnvironment for Fixture {
    type Error = ();

    /// Admit exact configured flags alongside otherwise absent optional settings.
    fn read_environment(
        &mut self,
        key: WorkerConfigurationEnvironmentKey,
    ) -> Result<Option<String>, ()> {
        Ok(match key {
            WorkerConfigurationEnvironmentKey::SiteConfigurationPath => Some("fixture-site.json"),
            WorkerConfigurationEnvironmentKey::RainCollectionEnabled => Some("1"),
            WorkerConfigurationEnvironmentKey::TemperatureCanaryKillSwitch => Some("0"),
            _ => None,
        }
        .map(str::to_owned))
    }
}

impl WorkerDatabaseConfigurationOwner for Fixture {
    type Error = ();

    /// Provide the same retained settings inspected by the startup traits.
    fn load_database_configuration(&mut self) -> Result<DatabaseConfiguration, ()> {
        self.0.borrow_mut().push("database");
        Ok(DatabaseConfiguration {
            application_name: "fixture".to_owned(),
            database: "fixture".to_owned(),
            host: "fixture".to_owned(),
            lock_timeout_ms: 10_000,
            password: "fixture".to_owned(),
            port: 5432,
            ssl: false,
            statement_timeout_ms: 30_000,
            user: "fixture".to_owned(),
        })
    }
}

impl WorkerJsonConfigurationOwner for Fixture {
    type Error = ();

    /// Materialize one generated site representation from the selected path.
    fn load_site_configuration(&mut self, path: &str) -> Result<SiteConfiguration, ()> {
        assert_eq!(path, "fixture-site.json");
        self.0.borrow_mut().push("site");
        Ok(SiteConfiguration {
            provider: SiteConfigurationProvider {
                active: true,
                attributionLabel: String::new(),
                attributionUrl: String::new(),
                displayName: "Fixture".to_owned(),
                key: "fixture".to_owned(),
            },
            site: SiteConfigurationSite {
                active: true,
                displayName: "Fixture".to_owned(),
                key: "fixture".to_owned(),
                latitude: 47.95,
                longitude: -122.43,
                timezone: "America/Los_Angeles".to_owned(),
            },
            station: SiteConfigurationStation {
                active: true,
                displayName: "Fixture".to_owned(),
                key: "fixture".to_owned(),
                kind: StationKind::Virtual,
                latitude: 47.95,
                longitude: -122.43,
                model: Nullable::Null,
                serial: Nullable::Null,
                vendor: Nullable::Null,
            },
            sources: Vec::new(),
            version: 1,
        })
    }

    /// Witness the shipped default Ecowitt path without reading any real catalog.
    fn load_ecowitt_configuration(&mut self, path: &str) -> Result<EcowittConfiguration, ()> {
        assert_eq!(path, "config/ecowitt/gateways.json");
        self.0.borrow_mut().push("ecowitt");
        Ok(EcowittConfiguration {
            siteKey: "fixture".to_owned(),
            version: 1,
            stations: Vec::new(),
            provider: EcowittConfigurationProvider {
                active: true,
                attributionLabel: String::new(),
                attributionUrl: String::new(),
                displayName: "Fixture".to_owned(),
                key: "fixture".to_owned(),
            },
        })
    }

    /// An omitted Tempest pair must not load its catalog.
    fn load_tempest_configuration(&mut self, _: &str) -> Result<TempestConfiguration, ()> {
        panic!("unexpected absent Tempest catalog")
    }

    /// Witness the default station catalog after the compatibility origin.
    fn load_public_station_configuration(
        &mut self,
        path: &str,
    ) -> Result<PublicStationConfiguration, ()> {
        assert_eq!(path, "config/public-stations/stations.json");
        self.0.borrow_mut().push("stations");
        Ok(PublicStationConfiguration {
            siteKey: "fixture".to_owned(),
            providers: Vec::new(),
            stations: Vec::new(),
            version: 1,
        })
    }

    /// Witness the default NOAA catalog after provider inputs.
    fn load_tide_configuration(&mut self, path: &str) -> Result<TideConfiguration, ()> {
        assert_eq!(path, "config/tides/noaa.json");
        self.0.borrow_mut().push("tides");
        Ok(TideConfiguration {
            siteKey: "fixture".to_owned(),
            version: 1,
            stations: Vec::new(),
            provider: TideProviderConfiguration {
                active: true,
                attributionLabel: String::new(),
                attributionUrl: String::new(),
                displayName: "Fixture".to_owned(),
                key: "fixture".to_owned(),
            },
        })
    }
}

impl WorkerCompatibilityOriginOwner for Fixture {
    type Error = ();

    /// Retain omitted compatibility mode without constructing an origin.
    fn parse_open_meteo_compatibility_origin(
        &mut self,
        value: Option<&str>,
    ) -> Result<Option<String>, ()> {
        assert_eq!(value, None);
        self.0.borrow_mut().push("origin");
        Ok(None)
    }
}

impl WorkerCredentialOwner for Fixture {
    type Error = ();

    /// Confirm an omitted optional integration does not read its credential.
    fn read_tempest_api_key(&mut self, _: &str) -> Result<String, ()> {
        panic!("unexpected absent Tempest credential")
    }
}

impl WorkerConfigurationArtifactOwner for Fixture {
    type Error = ();

    /// Omitted migration authorization has no admission workload.
    fn admit_migration_authorization(
        &mut self,
        _: &str,
        _: &str,
    ) -> Result<AdmittedHealthMigrationAuthorization, MigrationAuthorizationAdmissionError> {
        panic!("unexpected absent migration authorization")
    }

    /// Use the default shipped path under the admitted fixture process directory.
    fn resolve_migration_directory(&mut self, path: &str) -> Result<String, ()> {
        assert_eq!(path, "packages/database/migrations");
        self.0.borrow_mut().push("migration-dir");
        Ok(format!("/fixture/{path}"))
    }
}

/// Load one complete worker configuration through real orchestration and its typed boot traits.
#[test]
fn default_paths_flags_and_release_flow_into_worker_readiness() {
    let fixture = Fixture::default();
    let configuration = load_worker_configuration(
        &mut fixture.clone(),
        WorkerConfigurationOwners {
            database: &mut fixture.clone(),
            json: &mut fixture.clone(),
            url: &mut fixture.clone(),
            credential: &mut fixture.clone(),
            artifact: &mut fixture.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        *fixture.0.borrow(),
        [
            "database",
            "ecowitt",
            "migration-dir",
            "origin",
            "stations",
            "site",
            "tides"
        ]
    );
    assert_eq!(configuration.worker_instance(), "weather-worker");
    assert_eq!(configuration.database_configuration().host, "fixture");
    assert_eq!(configuration.release(), "development");
    assert_eq!(
        configuration.migration_directory(),
        Path::new("/fixture/packages/database/migrations")
    );
    assert!(configuration.rain_collection_enabled);
    assert_eq!(
        configuration.temperature_canary_kill_switch,
        Some(TemperatureCanaryKillSwitch::Zero)
    );
    assert!(configuration.tempest.is_none());
    assert!(configuration.tempest_api_key.is_none());
}
