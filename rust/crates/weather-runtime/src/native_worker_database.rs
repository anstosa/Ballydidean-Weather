//! native PostgreSQL worker boot factory and one-shot health process binding

use std::io::Write;

use weather_contracts::{
    EcowittConfiguration, PublicStationConfiguration, SiteConfiguration, TempestConfiguration,
    TideConfiguration,
};
use weather_database::{
    DatabasePoolConfiguration, DueSourceDiscoveryRow, ProcessMigrationArtifactFilesystem,
};
use weather_worker::{
    ScheduledSourceCatalogs, TaskFailure, WorkerDueSource, WorkerProcessFailure,
    WorkerProcessPhase, WorkerProcessPoolFactory,
};

use crate::{
    ConfiguredWorkerDueSourceProjection, NativePostgresError, NativePostgresPool,
    NativeRuntimeMaterial, NativeWirePrimitives, NativeWorkerCompatibilityOrigin,
    NativeWorkerConfigurationArtifacts, NativeWorkerJsonConfiguration, PostgresWorkerBootFactory,
    PostgresWorkerBootPool, PostgresWorkerIterationDatabase, WorkerBootDatabaseConnector,
    WorkerConfiguration, WorkerDueSourceProjection, WorkerFilesystemBootValues,
    WorkerHealthProcessError, native_worker_health_output, run_process_worker_health_check,
    system_worker_health_milliseconds,
};

/// owned admitted catalogs used after process configuration falls out of scope
pub struct NativeOwnedWorkerDueSourceProjection {
    site: SiteConfiguration,
    ecowitt: Option<EcowittConfiguration>,
    public_stations: Option<PublicStationConfiguration>,
    tempest: Option<TempestConfiguration>,
    tides: Option<TideConfiguration>,
}

impl NativeOwnedWorkerDueSourceProjection {
    /// clone only provider selection catalogs and exclude database or credential material
    pub fn from_configuration(configuration: &WorkerConfiguration) -> Self {
        Self {
            site: configuration.site.clone(),
            ecowitt: configuration.ecowitt.clone(),
            public_stations: configuration.public_stations.clone(),
            tempest: configuration.tempest.clone(),
            tides: configuration.tides.clone(),
        }
    }

    /// borrow the retained catalogs for one source projection
    fn catalogs(&self) -> ScheduledSourceCatalogs<'_> {
        ScheduledSourceCatalogs {
            site: &self.site,
            ecowitt: self.ecowitt.as_ref(),
            public_stations: self.public_stations.as_ref(),
            tempest: self.tempest.as_ref(),
            tides: self.tides.as_ref(),
        }
    }
}

impl WorkerDueSourceProjection for NativeOwnedWorkerDueSourceProjection {
    /// reuse the configured projection with a fresh zero-state material owner for each row
    fn project_due_source(
        &mut self,
        row: &DueSourceDiscoveryRow,
    ) -> Result<Option<WorkerDueSource>, TaskFailure> {
        ConfiguredWorkerDueSourceProjection::from_catalogs(self.catalogs(), NativeRuntimeMaterial)
            .project_due_source(row)
    }
}

/// actual retained iteration database used after worker configuration admission
pub type NativePostgresWorkerDatabase = PostgresWorkerIterationDatabase<
    NativePostgresPool,
    NativeWirePrimitives,
    NativeOwnedWorkerDueSourceProjection,
>;

/// actual filesystem and value owners used during worker database readiness
pub type NativePostgresWorkerBootValues = WorkerFilesystemBootValues<
    ProcessMigrationArtifactFilesystem,
    NativeRuntimeMaterial,
    NativeRuntimeMaterial,
    fn() -> Option<i64>,
    NativeWirePrimitives,
>;

/// retained startup and iteration pool over exactly one native PostgreSQL pool
pub type NativePostgresWorkerPool =
    PostgresWorkerBootPool<NativePostgresWorkerDatabase, NativePostgresWorkerBootValues>;

/// Share the exact open native worker pool for independent sidecar leases.
pub fn native_worker_shared_pool(
    pool: &mut NativePostgresWorkerPool,
) -> Result<NativePostgresPool, TaskFailure> {
    pool.database_mut()
        .map(|database| database.driver_ref().clone())
        .ok_or_else(|| TaskFailure {
            attempts: 0,
            code: "worker_pool_closed".to_owned(),
        })
}

/// local connector that can open only the one configuration-bound worker database
struct NativePostgresWorkerConnector {
    projection: Option<NativeOwnedWorkerDueSourceProjection>,
    instance: String,
    version: String,
}

impl WorkerBootDatabaseConnector for NativePostgresWorkerConnector {
    type Database = NativePostgresWorkerDatabase;
    type Error = NativePostgresError;

    /// open the lazy native pool and move the admitted catalogs into its iteration projection
    fn open_worker_pool(
        &mut self,
        configuration: DatabasePoolConfiguration,
    ) -> Result<Self::Database, Self::Error> {
        // reject a second connector use before allocating another process pool
        let projection =
            self.projection
                .take()
                .ok_or(NativePostgresError::InvalidConfiguration(
                    "worker_projection",
                ))?;
        let driver = NativePostgresPool::new(configuration)?;
        Ok(PostgresWorkerIterationDatabase::new(
            driver,
            NativeWirePrimitives,
            projection,
            self.instance.clone(),
            self.version.clone(),
        ))
    }
}

/// one-use real worker factory retaining startup values until configuration admission
pub struct NativePostgresWorkerFactory {
    values: Option<NativePostgresWorkerBootValues>,
}

impl NativePostgresWorkerFactory {
    /// bind zero-state filesystem digest clock and time owners without opening PostgreSQL
    pub fn new() -> Self {
        Self {
            values: Some(WorkerFilesystemBootValues::new(
                ProcessMigrationArtifactFilesystem,
                NativeRuntimeMaterial,
                NativeRuntimeMaterial,
                system_worker_health_milliseconds as fn() -> Option<i64>,
                NativeWirePrimitives,
            )),
        }
    }
}

impl Default for NativePostgresWorkerFactory {
    /// create one unopened native worker factory
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerProcessPoolFactory<WorkerConfiguration> for NativePostgresWorkerFactory {
    type Pool = NativePostgresWorkerPool;

    /// clone admitted catalogs then delegate one lazy pool open to the existing boot factory
    fn create_pool(
        &mut self,
        configuration: &WorkerConfiguration,
    ) -> Result<Self::Pool, WorkerProcessFailure> {
        // preserve the existing factory's one retained pool invariant
        let Some(values) = self.values.take() else {
            return Err(WorkerProcessFailure {
                code: "worker_pool_already_created".to_owned(),
                phase: WorkerProcessPhase::Pool,
            });
        };
        let connector = NativePostgresWorkerConnector {
            projection: Some(NativeOwnedWorkerDueSourceProjection::from_configuration(
                configuration,
            )),
            instance: configuration.instance.clone(),
            version: configuration.version.clone(),
        };
        PostgresWorkerBootFactory::new(connector, values).create_pool(configuration)
    }
}

/// run the actual one-shot worker health command over the retained native PostgreSQL pool
pub fn run_native_worker_health_process(
    output: impl Write,
) -> Result<u8, WorkerHealthProcessError> {
    let mut json = NativeWorkerJsonConfiguration;
    let mut url = NativeWorkerCompatibilityOrigin;
    let mut artifacts = NativeWorkerConfigurationArtifacts;
    let mut factory = NativePostgresWorkerFactory::new();
    let mut output = native_worker_health_output(output, NativeWirePrimitives);
    run_process_worker_health_check(
        &mut json,
        &mut url,
        &mut artifacts,
        &mut factory,
        &mut output,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkerJsonConfigurationOwner;
    use std::path::PathBuf;
    use weather_database::DatabaseConfiguration;
    use weather_worker::WorkerProcessPool;

    /// Construct an actual retained native worker pool without connecting before readiness.
    #[test]
    fn native_worker_factory_retains_one_lazy_pool() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let site = NativeWorkerJsonConfiguration
            .load_site_configuration(root.join("config/sites/ballydidean.json").to_str().unwrap())
            .unwrap();
        let configuration = WorkerConfiguration {
            adjustment_maintenance_api_origin: None,
            database: DatabaseConfiguration {
                application_name: "worker-test".to_owned(),
                database: "weather".to_owned(),
                host: "database.invalid".to_owned(),
                lock_timeout_ms: 10_000,
                password: "fixture".to_owned(),
                port: 5432,
                ssl: false,
                statement_timeout_ms: 30_000,
                user: "worker".to_owned(),
            },
            ecowitt: None,
            instance: "fixture-worker".to_owned(),
            migration_authorization: None,
            migration_directory: root
                .join("packages/database/migrations")
                .to_string_lossy()
                .into_owned(),
            open_meteo_compatibility_origin: None,
            public_stations: None,
            rain_collection_enabled: false,
            site,
            site_configuration_path: root
                .join("config/sites/ballydidean.json")
                .to_string_lossy()
                .into_owned(),
            tempest: None,
            tempest_api_key: None,
            temperature_canary_kill_switch: None,
            tides: None,
            version: "fixture-release".to_owned(),
        };
        let mut factory = NativePostgresWorkerFactory::new();
        let mut pool = factory.create_pool(&configuration).unwrap();
        assert!(factory.create_pool(&configuration).is_err());
        assert!(pool.database_mut().is_some());
        WorkerProcessPool::<WorkerConfiguration>::close(&mut pool).unwrap();
        assert!(pool.database_mut().is_none());
    }
}
