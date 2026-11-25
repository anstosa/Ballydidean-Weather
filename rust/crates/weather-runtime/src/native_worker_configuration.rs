//! Actual current-process configuration owners for the native worker.

use std::{fs, io};

use url::Url;
use weather_contracts::{
    EcowittConfiguration, PublicStationConfiguration, SiteConfiguration, TempestConfiguration,
    TideConfiguration,
};
use weather_database::{AdmittedHealthMigrationAuthorization, MigrationDigest};

use crate::{
    ApiMigrationDirectoryResolve, CurrentApiMigrationDirectory,
    MigrationAuthorizationAdmissionError, ProcessWorkerConfigurationLoader,
    WorkerCompatibilityOriginOwner, WorkerConfigurationArtifactOwner, WorkerJsonConfigurationOwner,
    api_process_configuration::migration_authorization_release_is_valid,
    native_worker_public_tide_catalogs::{
        parse_public_station_configuration, parse_tide_configuration,
    },
    native_worker_site_catalogs::{
        parse_ecowitt_configuration, parse_site_configuration, parse_tempest_configuration,
    },
};

/// Process material failure with no configuration value embedded in the diagnostic.
#[derive(Debug)]
pub enum NativeWorkerConfigurationError {
    Read(io::Error),
    Invalid,
}

/// Read the selected source file through the predecessor's Node UTF-8 replacement boundary.
fn read_configuration<T>(
    path: &str,
    parse: impl FnOnce(&[u8]) -> Option<T>,
) -> Result<T, NativeWorkerConfigurationError> {
    let bytes = fs::read(path).map_err(NativeWorkerConfigurationError::Read)?;
    let decoded = String::from_utf8_lossy(&bytes);
    parse(decoded.as_bytes()).ok_or(NativeWorkerConfigurationError::Invalid)
}

/// Zero-state actual filesystem/material owner for all original worker catalogs.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWorkerJsonConfiguration;

impl WorkerJsonConfigurationOwner for NativeWorkerJsonConfiguration {
    type Error = NativeWorkerConfigurationError;

    /// Load and validate the original site configuration.
    fn load_site_configuration(&mut self, path: &str) -> Result<SiteConfiguration, Self::Error> {
        read_configuration(path, parse_site_configuration)
    }

    /// Load the selected Ecowitt catalog without opening provider traffic.
    fn load_ecowitt_configuration(
        &mut self,
        path: &str,
    ) -> Result<EcowittConfiguration, Self::Error> {
        read_configuration(path, parse_ecowitt_configuration)
    }

    /// Load the selected Tempest catalog separately from its mounted credential.
    fn load_tempest_configuration(
        &mut self,
        path: &str,
    ) -> Result<TempestConfiguration, Self::Error> {
        read_configuration(path, parse_tempest_configuration)
    }

    /// Load the exact public-station provider and source catalog.
    fn load_public_station_configuration(
        &mut self,
        path: &str,
    ) -> Result<PublicStationConfiguration, Self::Error> {
        read_configuration(path, parse_public_station_configuration)
    }

    /// Load the exact tide provider and source catalog.
    fn load_tide_configuration(&mut self, path: &str) -> Result<TideConfiguration, Self::Error> {
        read_configuration(path, parse_tide_configuration)
    }
}

/// Reuse the approved WHATWG URL owner for the original compatibility origin.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWorkerCompatibilityOrigin;

impl WorkerCompatibilityOriginOwner for NativeWorkerCompatibilityOrigin {
    type Error = NativeWorkerConfigurationError;

    /// Preserve the absent origin and the exact Node origin-only normalization.
    fn parse_open_meteo_compatibility_origin(
        &mut self,
        value: Option<&str>,
    ) -> Result<Option<String>, Self::Error> {
        let Some(value) = value else {
            return Ok(None);
        };
        let parsed = Url::parse(value).map_err(|_| NativeWorkerConfigurationError::Invalid)?;
        // retain exactly the source's HTTP origin grammar, including Node's empty search/hash spelling
        if !matches!(parsed.scheme(), "http" | "https")
            || !parsed.username().is_empty()
            || parsed.password().is_some_and(|part| !part.is_empty())
            || parsed.path() != "/"
            || parsed.query().is_some_and(|part| !part.is_empty())
            || parsed.fragment().is_some_and(|part| !part.is_empty())
        {
            return Err(NativeWorkerConfigurationError::Invalid);
        }
        Ok(Some(parsed.origin().ascii_serialization()))
    }
}

/// Process-owned immutable migration identity and lexical path resolution.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWorkerConfigurationArtifacts;

impl WorkerConfigurationArtifactOwner for NativeWorkerConfigurationArtifacts {
    type Error = NativeWorkerConfigurationError;

    /// Admit the exact existing immutable release and lowercase ledger digest grammar.
    fn admit_migration_authorization(
        &mut self,
        release: &str,
        history_sha256: &str,
    ) -> Result<AdmittedHealthMigrationAuthorization, MigrationAuthorizationAdmissionError> {
        // reuse the same release pattern as the existing native API process owner
        if !migration_authorization_release_is_valid(release) {
            return Err(MigrationAuthorizationAdmissionError::InvalidRelease);
        }
        if history_sha256.len() != 64
            || !history_sha256
                .bytes()
                .all(|value| value.is_ascii_digit() || matches!(value, b'a'..=b'f'))
        {
            return Err(MigrationAuthorizationAdmissionError::InvalidHistorySha256);
        }
        Ok(AdmittedHealthMigrationAuthorization {
            release: release.to_owned(),
            history_sha256: MigrationDigest::admitted(history_sha256),
        })
    }

    /// Resolve a migration path using the existing Node-equivalent lexical path owner.
    fn resolve_migration_directory(&mut self, path: &str) -> Result<String, Self::Error> {
        let path = CurrentApiMigrationDirectory
            .resolve_migration_directory(path)
            .map_err(|_| NativeWorkerConfigurationError::Invalid)?;
        Ok(path.to_string_lossy().into_owned())
    }
}

/// Bind the complete actual process configuration loader without touching a database or provider.
pub const fn native_worker_configuration_loader() -> ProcessWorkerConfigurationLoader<
    NativeWorkerJsonConfiguration,
    NativeWorkerCompatibilityOrigin,
    NativeWorkerConfigurationArtifacts,
> {
    ProcessWorkerConfigurationLoader::new(
        NativeWorkerJsonConfiguration,
        NativeWorkerCompatibilityOrigin,
        NativeWorkerConfigurationArtifacts,
    )
}
