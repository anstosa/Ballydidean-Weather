//! Read the real migration catalog and project retained worker readiness values.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsDateOrString;
use weather_database::{
    AdmittedHealthMigrationAuthorization, AdmittedHealthMigrationIdentity, AppliedMigration,
    MigrationArtifactFilesystem, MigrationArtifactLoadError, MigrationArtifactNameOrder,
    MigrationArtifactSha256, digest_applied_migration_history, load_migration_artifacts,
};

use crate::{AdmittedHealthMigrationArtifacts, WorkerBootValues};

/// Admitted local artifact root and immutable running-release material.
pub trait RuntimeMigrationSource {
    /// Borrow the configured artifact path without resolving or substituting it.
    fn migration_directory(&self) -> &Path;

    /// Borrow any already admitted trailing-ledger authorization.
    fn migration_authorization(&self) -> Option<&AdmittedHealthMigrationAuthorization>;

    /// Borrow the running release to bind to the readiness check.
    fn release(&self) -> &str;
}

/// Existing worker-facing name for the shared migration source.
pub use RuntimeMigrationSource as WorkerMigrationSource;

/// Read-only artifact, digest, wall-clock and UTC timestamp owners.
pub struct WorkerFilesystemBootValues<Filesystem, Order, Digest, Clock, Time> {
    filesystem: Filesystem,
    order: Order,
    digest: Digest,
    clock: Clock,
    time: Time,
}

impl<Filesystem, Order, Digest, Clock, Time>
    WorkerFilesystemBootValues<Filesystem, Order, Digest, Clock, Time>
{
    /// Bind the approved owners without reading the host until requested by startup.
    pub const fn new(
        filesystem: Filesystem,
        order: Order,
        digest: Digest,
        clock: Clock,
        time: Time,
    ) -> Self {
        Self {
            filesystem,
            order,
            digest,
            clock,
            time,
        }
    }
}

/// Bounded stage failure while assembling ordinary worker readiness values.
#[derive(Debug)]
pub enum WorkerFilesystemBootError<FilesystemError> {
    Artifacts(MigrationArtifactLoadError<FilesystemError>),
    Clock,
    HeartbeatTimestamp,
}

impl<Config, Filesystem, Order, Digest, Clock, Time> WorkerBootValues<Config>
    for WorkerFilesystemBootValues<Filesystem, Order, Digest, Clock, Time>
where
    Config: WorkerMigrationSource,
    Filesystem: MigrationArtifactFilesystem,
    Order: MigrationArtifactNameOrder,
    Digest: MigrationArtifactSha256,
    Clock: FnMut() -> Option<i64>,
    Time: CurrentTimeResolver,
{
    type Error = WorkerFilesystemBootError<Filesystem::Error>;

    /// Load the ordered catalog before reading any applied ledger from PostgreSQL.
    fn load_migration_artifacts(
        &mut self,
        configuration: &Config,
    ) -> Result<AdmittedHealthMigrationArtifacts, Self::Error> {
        let artifacts = load_migration_artifacts(
            configuration.migration_directory(),
            &self.filesystem,
            &self.order,
            &mut self.digest,
        )
        .map_err(WorkerFilesystemBootError::Artifacts)?;
        Ok(AdmittedHealthMigrationArtifacts {
            artifacts,
            authorization: configuration.migration_authorization().cloned(),
            release: configuration.release().to_owned(),
        })
    }

    /// Bind the exact observed ordered ledger to the already loaded catalog and release.
    fn admit_migration_identity(
        &mut self,
        artifacts: AdmittedHealthMigrationArtifacts,
        applied: &[AppliedMigration],
    ) -> Result<AdmittedHealthMigrationIdentity, Self::Error> {
        Ok(AdmittedHealthMigrationIdentity {
            applied_history_sha256: digest_applied_migration_history(applied, &mut self.digest),
            artifacts: artifacts.artifacts,
            authorization: artifacts.authorization,
            release: artifacts.release,
        })
    }

    /// Sample the process clock once before reading the heartbeat row.
    fn health_now_ms(&mut self) -> Result<i64, Self::Error> {
        (self.clock)().ok_or(WorkerFilesystemBootError::Clock)
    }

    /// Resolve the same canonical timestamp representation used by the API repositories.
    fn heartbeat_epoch_ms(&mut self, value: &JsDateOrString) -> Result<i64, Self::Error> {
        self.time
            .resolve(value)
            .map(|instant| instant.epoch_milliseconds)
            .map_err(|_| WorkerFilesystemBootError::HeartbeatTimestamp)
    }
}

/// Standard wall clock for health freshness, without deriving civil-time representations.
pub fn system_worker_health_milliseconds() -> Option<i64> {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).ok(),
        Err(error) => i64::try_from(error.duration().as_millis())
            .ok()?
            .checked_neg(),
    }
}
