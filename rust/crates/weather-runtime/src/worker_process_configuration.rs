//! Compose process-owned worker input with its typed runtime startup consumers.

use std::convert::Infallible;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::Path;

use weather_database::{
    AdmittedHealthMigrationAuthorization, DatabaseConfiguration, DatabaseProcessConfigurationError,
    load_current_process_database_configuration,
};
use weather_worker::{
    AdmittedWorkerProcessConfiguration, AdmittedWorkerStartupTask, WorkerProcessEnvironmentPort,
    WorkerProcessFailure, WorkerProcessPhase, WorkerProcessPoolFactory,
};

use crate::{
    DatabaseWorkerBootConfiguration, PublicStationStartupConfiguration, PublicStationStartupInput,
    WorkerCompatibilityOriginOwner, WorkerConfiguration, WorkerConfigurationArtifactOwner,
    WorkerConfigurationEnvironment, WorkerConfigurationEnvironmentKey,
    WorkerConfigurationLoadError, WorkerConfigurationOwners, WorkerCredentialOwner,
    WorkerDatabaseConfigurationOwner, WorkerHealthOutput, WorkerJsonConfigurationOwner,
    WorkerMigrationSource, load_worker_configuration, run_worker_health_check,
};

impl AdmittedWorkerProcessConfiguration for WorkerConfiguration {
    /// Use the same instance key for scheduling and durable health.
    fn instance(&self) -> &str {
        &self.instance
    }

    /// Retain origin-gated client creation separately from the unconditional witness read.
    fn startup_maintenance(&self) -> AdmittedWorkerStartupTask {
        // a missing private origin leaves optional network clients unavailable
        if self.adjustment_maintenance_api_origin.is_some() {
            AdmittedWorkerStartupTask::Active
        } else {
            AdmittedWorkerStartupTask::Inactive
        }
    }

    /// Skip startup archive preparation when there is no loaded public-station catalog.
    fn startup_backfill(&self) -> AdmittedWorkerStartupTask {
        // the backfill runner retains independent filtering within the admitted catalog
        if self.public_stations.is_some() {
            AdmittedWorkerStartupTask::Active
        } else {
            AdmittedWorkerStartupTask::Inactive
        }
    }
}

impl PublicStationStartupConfiguration for WorkerConfiguration {
    /// Pass the actual process catalog and release to the retained startup runner.
    fn public_station_startup_input(&self) -> Option<PublicStationStartupInput> {
        Some(PublicStationStartupInput {
            configuration: self.public_stations.clone()?,
            release: self.version.clone(),
        })
    }
}

impl DatabaseWorkerBootConfiguration for WorkerConfiguration {
    /// Use the database settings retained during process configuration.
    fn database_configuration(&self) -> &DatabaseConfiguration {
        &self.database
    }

    /// Select the configured instance for durable health and continuation.
    fn worker_instance(&self) -> &str {
        &self.instance
    }
}

impl WorkerMigrationSource for WorkerConfiguration {
    /// Borrow the already resolved migration artifact path.
    fn migration_directory(&self) -> &Path {
        Path::new(&self.migration_directory)
    }

    /// Retain the exact admitted migration authorization for the configured release.
    fn migration_authorization(&self) -> Option<&AdmittedHealthMigrationAuthorization> {
        self.migration_authorization.as_ref()
    }

    /// Verify applied ledger readiness against the same running release.
    fn release(&self) -> &str {
        &self.version
    }
}

struct ProcessWorkerEnvironment;

impl WorkerConfigurationEnvironment for ProcessWorkerEnvironment {
    type Error = Infallible;

    /// Read only a named process setting with the source string-decoding boundary.
    fn read_environment(
        &mut self,
        key: WorkerConfigurationEnvironmentKey,
    ) -> Result<Option<String>, Infallible> {
        Ok(std::env::var_os(key.as_str()).map(
            // preserve absence while converting process text in the same way as node
            |value| value.to_string_lossy().into_owned(),
        ))
    }
}

struct ProcessWorkerDatabase;

impl WorkerDatabaseConfigurationOwner for ProcessWorkerDatabase {
    type Error = DatabaseProcessConfigurationError;

    /// Use the shared database loader at the point selected by the worker source.
    fn load_database_configuration(&mut self) -> Result<DatabaseConfiguration, Self::Error> {
        load_current_process_database_configuration()
    }
}

struct ProcessWorkerCredential;

impl WorkerCredentialOwner for ProcessWorkerCredential {
    type Error = std::io::Error;

    /// Read only the selected Tempest credential; the typed owner admits the line.
    fn read_tempest_api_key(&mut self, path: &str) -> Result<String, Self::Error> {
        let bytes = std::fs::read(path)?;
        Ok(String::from_utf8(bytes).unwrap_or_else(
            // reuse valid text and preserve the source utf8 replacement fallback
            |error| String::from_utf8_lossy(error.as_bytes()).into_owned(),
        ))
    }
}

/// Load the current worker process using its approved configuration decoders and URL owner.
pub fn load_process_worker_configuration<Json, Url, Artifact>(
    json: &mut Json,
    url: &mut Url,
    artifact: &mut Artifact,
) -> Result<WorkerConfiguration, WorkerConfigurationLoadError>
where
    Json: WorkerJsonConfigurationOwner,
    Url: WorkerCompatibilityOriginOwner,
    Artifact: WorkerConfigurationArtifactOwner,
{
    load_worker_configuration(
        &mut ProcessWorkerEnvironment,
        WorkerConfigurationOwners {
            database: &mut ProcessWorkerDatabase,
            json,
            url,
            credential: &mut ProcessWorkerCredential,
            artifact,
        },
    )
}

/// Bind the original configuration loader to the long-running Rust worker startup.
pub struct ProcessWorkerConfigurationLoader<Json, Url, Artifact> {
    json: Json,
    url: Url,
    artifact: Artifact,
}

impl<Json, Url, Artifact> ProcessWorkerConfigurationLoader<Json, Url, Artifact> {
    /// Accept the approved decoding and immutable-selection owners before startup.
    pub const fn new(json: Json, url: Url, artifact: Artifact) -> Self {
        Self {
            json,
            url,
            artifact,
        }
    }
}

impl<Json, Url, Artifact> WorkerProcessEnvironmentPort
    for ProcessWorkerConfigurationLoader<Json, Url, Artifact>
where
    Json: WorkerJsonConfigurationOwner,
    Url: WorkerCompatibilityOriginOwner,
    Artifact: WorkerConfigurationArtifactOwner,
{
    type Configuration = WorkerConfiguration;

    /// Load the real process configuration before retaining any pool or optional witness.
    fn load_configuration(&mut self) -> Result<WorkerConfiguration, WorkerProcessFailure> {
        load_process_worker_configuration(&mut self.json, &mut self.url, &mut self.artifact)
            .map_err(|_| WorkerProcessFailure {
                code: "worker_configuration_failed".to_owned(),
                phase: WorkerProcessPhase::Configuration,
            })
    }
}

/// Distinguish process configuration from database, heartbeat, and output failures.
#[derive(Debug)]
pub enum WorkerHealthProcessError {
    Configuration(WorkerConfigurationLoadError),
    Runtime(WorkerProcessFailure),
}

impl Display for WorkerHealthProcessError {
    /// Report only the bounded owning failure from configuration or runtime.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration(error) => Display::fmt(error, formatter),
            Self::Runtime(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for WorkerHealthProcessError {
    /// Retain the typed root cause for the command's diagnostic owner.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Configuration(error) => Some(error),
            Self::Runtime(error) => Some(error),
        }
    }
}

/// Execute the real worker health command lifecycle over approved external adapters.
pub fn run_process_worker_health_check<Json, Url, Artifact, Factory, Output>(
    json: &mut Json,
    url: &mut Url,
    artifact: &mut Artifact,
    factory: &mut Factory,
    output: &mut Output,
) -> Result<u8, WorkerHealthProcessError>
where
    Json: WorkerJsonConfigurationOwner,
    Url: WorkerCompatibilityOriginOwner,
    Artifact: WorkerConfigurationArtifactOwner,
    Factory: WorkerProcessPoolFactory<WorkerConfiguration>,
    Output: WorkerHealthOutput,
{
    let configuration = load_process_worker_configuration(json, url, artifact)
        .map_err(WorkerHealthProcessError::Configuration)?;
    run_worker_health_check(&configuration, factory, output)
        .map_err(WorkerHealthProcessError::Runtime)
}
