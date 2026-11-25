//! Process-owned artifact, canonical-time, and digest values for API repository reads.

use weather_api::{
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver, ForecastTemperatureSidecarRead,
};
use weather_contracts::{EcmwfTemperatureCanaryRecentErrorState, JsDateOrString};
use weather_database::{
    AdmittedEcmwfTemperatureCanarySidecarQuery, AdmittedHealthMigrationIdentity, AppliedMigration,
    MigrationArtifactFilesystem, MigrationArtifactLoadError, MigrationArtifactNameOrder,
    MigrationArtifactSha256, ProcessMigrationArtifactFilesystem, Sha256Digest,
    digest_applied_migration_history, load_migration_artifacts,
};
use weather_worker::TaskFailure;

use crate::{
    AdmittedHealthMigrationArtifacts, RainRepositoryTimeBoundary, RuntimeMigrationSource,
    TemperatureCanaryRecentErrorDigestPort, WeatherServiceExternalReadPort,
};

/// Read-only external values used by one concrete PostgreSQL API repository.
pub struct FilesystemWeatherServiceExternal<Source, Filesystem, Order, Digest, Time, RecentDigest> {
    source: Source,
    filesystem: Filesystem,
    order: Order,
    digest: Digest,
    time: Time,
    recent_digest: RecentDigest,
}

impl<Source, Order, Digest, Time, RecentDigest>
    FilesystemWeatherServiceExternal<
        Source,
        ProcessMigrationArtifactFilesystem,
        Order,
        Digest,
        Time,
        RecentDigest,
    >
{
    /// Use the process's read-only migration filesystem without opening any files at startup.
    pub const fn current_process(
        source: Source,
        order: Order,
        digest: Digest,
        time: Time,
        recent_digest: RecentDigest,
    ) -> Self {
        Self::new(
            source,
            ProcessMigrationArtifactFilesystem,
            order,
            digest,
            time,
            recent_digest,
        )
    }
}

impl<Source, Filesystem, Order, Digest, Time, RecentDigest>
    FilesystemWeatherServiceExternal<Source, Filesystem, Order, Digest, Time, RecentDigest>
{
    /// Bind process-owned external values without reading files or parsing timestamps.
    pub const fn new(
        source: Source,
        filesystem: Filesystem,
        order: Order,
        digest: Digest,
        time: Time,
        recent_digest: RecentDigest,
    ) -> Self {
        Self {
            source,
            filesystem,
            order,
            digest,
            time,
            recent_digest,
        }
    }
}

/// Bounded filesystem or canonical-time failure for API external reads.
#[derive(Debug, PartialEq, Eq)]
pub enum FilesystemWeatherServiceExternalError<FilesystemError> {
    Artifacts(MigrationArtifactLoadError<FilesystemError>),
    Time(CurrentProjectionError),
}

impl<Source, Filesystem, Order, Digest, Time, RecentDigest> WeatherServiceExternalReadPort
    for FilesystemWeatherServiceExternal<Source, Filesystem, Order, Digest, Time, RecentDigest>
where
    Source: RuntimeMigrationSource,
    Filesystem: MigrationArtifactFilesystem,
    Order: MigrationArtifactNameOrder,
    Digest: MigrationArtifactSha256,
    Time: CurrentTimeResolver,
{
    type Error = FilesystemWeatherServiceExternalError<Filesystem::Error>;

    /// Admit every sidecar timestamp through the existing canonical time owner.
    fn admit_temperature_sidecar_read(
        &mut self,
        read: ForecastTemperatureSidecarRead,
    ) -> Result<AdmittedEcmwfTemperatureCanarySidecarQuery, Self::Error> {
        let as_of = resolve_text(&self.time, &read.as_of).map_err(Self::Error::Time)?;
        let from = resolve_text(&self.time, &read.from).map_err(Self::Error::Time)?;
        let to = resolve_text(&self.time, &read.to).map_err(Self::Error::Time)?;
        Ok(AdmittedEcmwfTemperatureCanarySidecarQuery::admitted(
            as_of.utc,
            from.utc,
            from.epoch_milliseconds,
            read.site_slug,
            to.utc,
            to.epoch_milliseconds,
        ))
    }

    /// Reload the exact ordered migration catalog for every health request.
    fn admit_health_migration_artifacts(
        &mut self,
    ) -> Result<AdmittedHealthMigrationArtifacts, Self::Error> {
        let artifacts = load_migration_artifacts(
            self.source.migration_directory(),
            &self.filesystem,
            &self.order,
            &mut self.digest,
        )
        .map_err(Self::Error::Artifacts)?;
        Ok(AdmittedHealthMigrationArtifacts {
            artifacts,
            authorization: self.source.migration_authorization().cloned(),
            release: self.source.release().to_owned(),
        })
    }

    /// Bind the observed ledger in its database order through the approved digest owner.
    fn admit_health_migration_identity(
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

    /// Resolve the decoded worker timestamp through the same canonical time owner.
    fn admit_health_worker_last_loop_epoch(
        &mut self,
        value: &JsDateOrString,
    ) -> Result<i64, Self::Error> {
        self.time
            .resolve(value)
            .map(|instant| instant.epoch_milliseconds)
            .map_err(Self::Error::Time)
    }
}

impl<Source, Filesystem, Order, Digest, Time, RecentDigest> RainRepositoryTimeBoundary
    for FilesystemWeatherServiceExternal<Source, Filesystem, Order, Digest, Time, RecentDigest>
where
    Time: CurrentTimeResolver,
{
    type Error = CurrentProjectionError;

    /// Forward retained rain timestamp admission to the shared canonical time owner.
    fn admit_rain_instant(&mut self, value: &str) -> Result<CanonicalInstant, Self::Error> {
        resolve_text(&self.time, value)
    }
}

impl<Source, Filesystem, Order, Digest, Time, RecentDigest> TemperatureCanaryRecentErrorDigestPort
    for FilesystemWeatherServiceExternal<Source, Filesystem, Order, Digest, Time, RecentDigest>
where
    RecentDigest: TemperatureCanaryRecentErrorDigestPort,
{
    /// Forward generated-state hashing to the existing canonical digest owner.
    fn digest_recent_error_state(
        &mut self,
        state: &EcmwfTemperatureCanaryRecentErrorState,
    ) -> Result<Sha256Digest, TaskFailure> {
        self.recent_digest.digest_recent_error_state(state)
    }
}

/// Resolve one Rust string through the generated JavaScript time-value boundary.
fn resolve_text(
    time: &impl CurrentTimeResolver,
    value: &str,
) -> Result<CanonicalInstant, CurrentProjectionError> {
    time.resolve(&JsDateOrString::String(value.encode_utf16().collect()))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::cmp::Ordering;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use weather_contracts::{EcmwfTemperatureCanaryRecentErrorStateCohortLiteral, Nullable};
    use weather_database::{AdmittedHealthMigrationAuthorization, MigrationDigest};

    use super::*;

    struct Source(PathBuf);

    impl RuntimeMigrationSource for Source {
        /// Return the injected read-only migration root.
        fn migration_directory(&self) -> &Path {
            &self.0
        }

        /// Keep the fixture on the ordinary exact-ledger path.
        fn migration_authorization(&self) -> Option<&AdmittedHealthMigrationAuthorization> {
            None
        }

        /// Bind loaded artifacts to one immutable fixture release.
        fn release(&self) -> &str {
            "fixture-release"
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FileError;

    struct Filesystem {
        directory_reads: Rc<RefCell<usize>>,
        files: BTreeMap<PathBuf, Vec<u8>>,
    }

    impl MigrationArtifactFilesystem for Filesystem {
        type Error = FileError;

        /// Return deliberately reversed names and count every health reload.
        fn read_directory(&self, _directory: &Path) -> Result<Vec<String>, Self::Error> {
            *self.directory_reads.borrow_mut() += 1;
            Ok(vec![
                "0002_second.sql".to_owned(),
                "0001_first.sql".to_owned(),
            ])
        }

        /// Return exact injected SQL bytes for the selected catalog entry.
        fn read_file(&self, path: &Path) -> Result<Vec<u8>, Self::Error> {
            self.files.get(path).cloned().ok_or(FileError)
        }
    }

    struct Order;

    impl MigrationArtifactNameOrder for Order {
        /// Reproduce the source filename order for this ASCII fixture.
        fn compare_names(&self, left: &str, right: &str) -> Ordering {
            left.cmp(right)
        }
    }

    struct Digest(Rc<RefCell<Vec<Vec<u8>>>>);

    impl MigrationArtifactSha256 for Digest {
        /// Capture every exact artifact and ledger digest preimage.
        fn sha256(&mut self, decoded_utf8_bytes: &[u8]) -> MigrationDigest {
            self.0.borrow_mut().push(decoded_utf8_bytes.to_vec());
            MigrationDigest::admitted("a".repeat(64))
        }
    }

    struct Time(Rc<RefCell<Vec<JsDateOrString>>>);

    impl CurrentTimeResolver for Time {
        /// Resolve the fixture's canonical string and decoded database timestamp shapes.
        fn resolve(
            &self,
            value: &JsDateOrString,
        ) -> Result<CanonicalInstant, CurrentProjectionError> {
            self.0.borrow_mut().push(value.clone());
            match value {
                JsDateOrString::DateEpochMilliseconds(epoch_milliseconds) => Ok(CanonicalInstant {
                    epoch_milliseconds: *epoch_milliseconds,
                    utc: "2026-08-22T05:00:00.000Z".to_owned(),
                }),
                JsDateOrString::String(value) => {
                    let utc = String::from_utf16(value).map_err(|_| CurrentProjectionError {
                        code: "fixture.time",
                        detail: "invalid utf16".to_owned(),
                    })?;
                    let epoch_milliseconds = match utc.as_str() {
                        "2026-08-22T05:00:00.000Z" => 1_787_374_800_000,
                        "2026-08-22T06:00:00.000Z" => 1_787_378_400_000,
                        "2026-08-22T07:00:00.000Z" => 1_787_382_000_000,
                        _ => {
                            return Err(CurrentProjectionError {
                                code: "fixture.time",
                                detail: utc,
                            });
                        }
                    };
                    Ok(CanonicalInstant {
                        epoch_milliseconds,
                        utc,
                    })
                }
                JsDateOrString::InvalidDate => Err(CurrentProjectionError {
                    code: "fixture.time",
                    detail: "invalid date".to_owned(),
                }),
            }
        }
    }

    struct RecentDigest(Rc<RefCell<usize>>);

    impl TemperatureCanaryRecentErrorDigestPort for RecentDigest {
        /// Count forwarding calls and return one approved fixture digest.
        fn digest_recent_error_state(
            &mut self,
            _state: &EcmwfTemperatureCanaryRecentErrorState,
        ) -> Result<Sha256Digest, TaskFailure> {
            *self.0.borrow_mut() += 1;
            Ok(Sha256Digest::admitted("b".repeat(64)))
        }
    }

    /// Prove per-request catalog reload, ordered ledger hashing, and canonical time forwarding.
    #[test]
    fn supplies_real_external_repository_values_without_retained_health_catalog() {
        let root = PathBuf::from("/fixture/migrations");
        let directory_reads = Rc::new(RefCell::new(0));
        let digest_inputs = Rc::new(RefCell::new(Vec::new()));
        let time_inputs = Rc::new(RefCell::new(Vec::new()));
        let recent_digest_calls = Rc::new(RefCell::new(0));
        let mut files = BTreeMap::new();
        files.insert(root.join("0001_first.sql"), b"select 1;".to_vec());
        files.insert(root.join("0002_second.sql"), b"select 2;".to_vec());
        let mut external = FilesystemWeatherServiceExternal::new(
            Source(root),
            Filesystem {
                directory_reads: Rc::clone(&directory_reads),
                files,
            },
            Order,
            Digest(Rc::clone(&digest_inputs)),
            Time(Rc::clone(&time_inputs)),
            RecentDigest(Rc::clone(&recent_digest_calls)),
        );

        let first = external.admit_health_migration_artifacts().unwrap();
        let second = external.admit_health_migration_artifacts().unwrap();
        assert_eq!(*directory_reads.borrow(), 2);
        assert_eq!(first.artifacts, second.artifacts);
        assert_eq!(first.artifacts[0].name, "0001_first.sql");
        let applied = first
            .artifacts
            .iter()
            .map(|artifact| AppliedMigration {
                name: artifact.name.clone(),
                digest: artifact.digest.clone(),
            })
            .collect::<Vec<_>>();
        let identity = external
            .admit_health_migration_identity(first, &applied)
            .unwrap();
        assert_eq!(identity.release, "fixture-release");
        assert_eq!(
            digest_inputs.borrow().last().unwrap(),
            b"0001_first.sql:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n0002_second.sql:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"
        );

        let query = external
            .admit_temperature_sidecar_read(ForecastTemperatureSidecarRead {
                site_slug: "home".to_owned(),
                as_of: "2026-08-22T05:00:00.000Z".to_owned(),
                from: "2026-08-22T06:00:00.000Z".to_owned(),
                to: "2026-08-22T07:00:00.000Z".to_owned(),
            })
            .unwrap();
        assert_eq!(query.from_epoch_milliseconds, 1_787_378_400_000);
        assert_eq!(query.to_epoch_milliseconds, 1_787_382_000_000);
        assert_eq!(
            external
                .admit_health_worker_last_loop_epoch(&JsDateOrString::DateEpochMilliseconds(
                    1_787_374_800_000,
                ))
                .unwrap(),
            1_787_374_800_000
        );
        assert_eq!(
            external
                .admit_rain_instant("2026-08-22T05:00:00.000Z")
                .unwrap()
                .utc,
            "2026-08-22T05:00:00.000Z"
        );
        assert_eq!(time_inputs.borrow().len(), 5);

        let digest = external
            .digest_recent_error_state(&recent_error_state())
            .unwrap();
        assert_eq!(digest.as_str(), "b".repeat(64));
        assert_eq!(*recent_digest_calls.borrow(), 1);
    }

    /// Build one ordinary generated recent-error state for forwarding coverage.
    fn recent_error_state() -> EcmwfTemperatureCanaryRecentErrorState {
        EcmwfTemperatureCanaryRecentErrorState {
            b24C: Nullable::Null,
            b72C: Nullable::Null,
            cohort: EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
            localDates: 0,
            mad72C: Nullable::Null,
            maximumSourceRunInitializedAt: Nullable::Null,
            maximumSourceValidAt: Nullable::Null,
            n24: 0,
            n72: 0,
            sourceKeys: Vec::new(),
            supported: false,
            targetRunInitializedAt: "2026-08-22T00:00:00.000Z".to_owned(),
            windowEndValidAt: "2026-08-22T00:00:00.000Z".to_owned(),
        }
    }
}
