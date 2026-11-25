use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    EcowittConfiguration, PublicStationConfiguration, SiteConfiguration, TempestConfiguration,
    TideConfiguration,
};
use weather_database::{AdmittedHealthMigrationAuthorization, DatabaseConfiguration};

const DEFAULT_ECOWITT_CONFIGURATION_PATH: &str = "config/ecowitt/gateways.json";
const DEFAULT_MIGRATION_DIRECTORY: &str = "packages/database/migrations";
const DEFAULT_PUBLIC_STATION_CONFIGURATION_PATH: &str = "config/public-stations/stations.json";
const DEFAULT_TIDE_CONFIGURATION_PATH: &str = "config/tides/noaa.json";
const DEFAULT_WORKER_INSTANCE: &str = "weather-worker";
const DEFAULT_WORKER_VERSION: &str = "development";

/// Closed environment keys read directly by the worker configuration owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerConfigurationEnvironmentKey {
    SiteConfigurationPath,
    EcowittConfigurationPath,
    TempestConfigurationPath,
    TempestApiKeyFile,
    PublicStationConfigurationPath,
    TideConfigurationPath,
    AdjustmentMaintenanceApiOrigin,
    WorkerInstance,
    MigrationAuthorizationRelease,
    MigrationAuthorizationHistorySha256,
    MigrationDirectory,
    OpenMeteoCompatibilityOrigin,
    RainCollectionEnabled,
    TemperatureCanaryKillSwitch,
    Release,
}

impl WorkerConfigurationEnvironmentKey {
    /// Return the exact Node environment spelling for one known field.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SiteConfigurationPath => "WEATHER_SITE_CONFIG_PATH",
            Self::EcowittConfigurationPath => "WEATHER_ECOWITT_CONFIG_PATH",
            Self::TempestConfigurationPath => "WEATHER_TEMPEST_CONFIG_PATH",
            Self::TempestApiKeyFile => "WEATHER_TEMPEST_API_KEY_FILE",
            Self::PublicStationConfigurationPath => "WEATHER_PUBLIC_STATIONS_CONFIG_PATH",
            Self::TideConfigurationPath => "WEATHER_TIDE_CONFIG_PATH",
            Self::AdjustmentMaintenanceApiOrigin => "WEATHER_ADJUSTMENT_MAINTENANCE_API_ORIGIN",
            Self::WorkerInstance => "WEATHER_WORKER_INSTANCE",
            Self::MigrationAuthorizationRelease => "WEATHER_MIGRATION_AUTHORIZATION_RELEASE",
            Self::MigrationAuthorizationHistorySha256 => {
                "WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256"
            }
            Self::MigrationDirectory => "WEATHER_MIGRATION_DIRECTORY",
            Self::OpenMeteoCompatibilityOrigin => "WEATHER_OPEN_METEO_COMPATIBILITY_ORIGIN",
            Self::RainCollectionEnabled => "WEATHER_RAIN_COLLECTION_ENABLED",
            Self::TemperatureCanaryKillSwitch => {
                "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH"
            }
            Self::Release => "WEATHER_RELEASE",
        }
    }
}

/// Exact literal admitted by the temperature-canary runtime loader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemperatureCanaryKillSwitch {
    Zero,
}

impl TemperatureCanaryKillSwitch {
    /// Return the only source value that activates the kill-switch override.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Zero => "0",
        }
    }
}

/// Complete worker trust-boundary configuration without ambient process access.
#[derive(Clone, PartialEq)]
pub struct WorkerConfiguration {
    pub adjustment_maintenance_api_origin: Option<String>,
    pub database: DatabaseConfiguration,
    pub ecowitt: Option<EcowittConfiguration>,
    pub instance: String,
    pub migration_authorization: Option<AdmittedHealthMigrationAuthorization>,
    pub migration_directory: String,
    pub open_meteo_compatibility_origin: Option<String>,
    pub public_stations: Option<PublicStationConfiguration>,
    pub rain_collection_enabled: bool,
    pub site: SiteConfiguration,
    pub site_configuration_path: String,
    pub tempest: Option<TempestConfiguration>,
    pub tempest_api_key: Option<String>,
    pub temperature_canary_kill_switch: Option<TemperatureCanaryKillSwitch>,
    pub tides: Option<TideConfiguration>,
    pub version: String,
}

/// One generated configuration material selected by an admitted path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerConfigurationMaterial {
    Ecowitt,
    PublicStations,
    Site,
    Tempest,
    Tides,
}

impl WorkerConfigurationMaterial {
    /// Return the stable material label used by categorical loader failures.
    const fn label(self) -> &'static str {
        match self {
            Self::Ecowitt => "Ecowitt",
            Self::PublicStations => "public-station",
            Self::Site => "site",
            Self::Tempest => "Tempest",
            Self::Tides => "tide",
        }
    }
}

/// Exact typed failure returned by the approved migration-authorization owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationAuthorizationAdmissionError {
    InvalidRelease,
    InvalidHistorySha256,
}

/// Bounded worker configuration failure with no secret-bearing values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerConfigurationLoadError {
    EnvironmentUnavailable(WorkerConfigurationEnvironmentKey),
    RequiredEnvironment(WorkerConfigurationEnvironmentKey),
    EmptyOptionalEnvironment(WorkerConfigurationEnvironmentKey),
    BoundedEnvironment(WorkerConfigurationEnvironmentKey),
    TempestConfigurationPairRequired,
    DatabaseUnavailable,
    MaterialUnavailable(WorkerConfigurationMaterial),
    MigrationAuthorizationIncomplete,
    MigrationAuthorizationInvalidRelease,
    MigrationAuthorizationInvalidHistorySha256,
    MigrationDirectoryUnavailable,
    OpenMeteoCompatibilityOriginInvalid,
    TempestApiKeyUnavailable,
    TempestApiKeyInvalid,
}

impl Display for WorkerConfigurationLoadError {
    /// Render the existing stable diagnostics or a value-free owner failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EnvironmentUnavailable(key) => {
                write!(formatter, "{} could not be read", key.as_str())
            }
            Self::RequiredEnvironment(key) => write!(formatter, "{} is required", key.as_str()),
            Self::EmptyOptionalEnvironment(key) => write!(
                formatter,
                "{} must be non-empty when configured",
                key.as_str()
            ),
            Self::BoundedEnvironment(key) => {
                write!(formatter, "{} must be non-empty and bounded", key.as_str())
            }
            Self::TempestConfigurationPairRequired => formatter.write_str(
                "WEATHER_TEMPEST_CONFIG_PATH and WEATHER_TEMPEST_API_KEY_FILE must be configured together",
            ),
            Self::DatabaseUnavailable => {
                formatter.write_str("database configuration could not be loaded")
            }
            Self::MaterialUnavailable(material) => {
                write!(formatter, "{} configuration could not be loaded", material.label())
            }
            Self::MigrationAuthorizationIncomplete => {
                formatter.write_str("migration authorization must be complete")
            }
            Self::MigrationAuthorizationInvalidRelease => {
                formatter.write_str("migration authorization release is invalid")
            }
            Self::MigrationAuthorizationInvalidHistorySha256 => {
                formatter.write_str("migration authorization history SHA-256 is invalid")
            }
            Self::MigrationDirectoryUnavailable => {
                formatter.write_str("migration directory could not be resolved")
            }
            Self::OpenMeteoCompatibilityOriginInvalid => formatter.write_str(
                "WEATHER_OPEN_METEO_COMPATIBILITY_ORIGIN must be a credential-free HTTP origin",
            ),
            Self::TempestApiKeyUnavailable => {
                formatter.write_str("Tempest API key file could not be read")
            }
            Self::TempestApiKeyInvalid => {
                formatter.write_str("Tempest API key file must contain one bounded non-empty value")
            }
        }
    }
}

impl Error for WorkerConfigurationLoadError {}

/// Injected closed environment reader retaining configured-versus-absent values.
pub trait WorkerConfigurationEnvironment {
    type Error;

    /// Read one known key without trimming or defaulting its value.
    fn read_environment(
        &mut self,
        key: WorkerConfigurationEnvironmentKey,
    ) -> Result<Option<String>, Self::Error>;
}

/// Existing database configuration owner bound to the same admitted environment.
pub trait WorkerDatabaseConfigurationOwner {
    type Error;

    /// Load the complete database settings at the source-defined point in ordering.
    fn load_database_configuration(&mut self) -> Result<DatabaseConfiguration, Self::Error>;
}

/// Approved JSON/material owner for the five generated configuration families.
pub trait WorkerJsonConfigurationOwner {
    type Error;

    /// Load one admitted site configuration.
    fn load_site_configuration(&mut self, path: &str) -> Result<SiteConfiguration, Self::Error>;

    /// Load one admitted Ecowitt catalog.
    fn load_ecowitt_configuration(
        &mut self,
        path: &str,
    ) -> Result<EcowittConfiguration, Self::Error>;

    /// Load one admitted Tempest catalog.
    fn load_tempest_configuration(
        &mut self,
        path: &str,
    ) -> Result<TempestConfiguration, Self::Error>;

    /// Load one admitted public-station catalog.
    fn load_public_station_configuration(
        &mut self,
        path: &str,
    ) -> Result<PublicStationConfiguration, Self::Error>;

    /// Load one admitted NOAA tide catalog.
    fn load_tide_configuration(&mut self, path: &str) -> Result<TideConfiguration, Self::Error>;
}

/// Approved URL owner for the credential-free Open-Meteo origin.
pub trait WorkerCompatibilityOriginOwner {
    type Error;

    /// Parse and canonicalize one optional compatibility origin.
    fn parse_open_meteo_compatibility_origin(
        &mut self,
        value: Option<&str>,
    ) -> Result<Option<String>, Self::Error>;
}

/// Mounted Tempest credential reader without ambient filesystem access.
pub trait WorkerCredentialOwner {
    type Error;

    /// Read one selected credential file as Node-compatible decoded text.
    fn read_tempest_api_key(&mut self, path: &str) -> Result<String, Self::Error>;
}

/// Approved migration authorization and local artifact-path owner.
pub trait WorkerConfigurationArtifactOwner {
    type Error;

    /// Admit one complete immutable release and ordered-ledger digest pair.
    fn admit_migration_authorization(
        &mut self,
        release: &str,
        history_sha256: &str,
    ) -> Result<AdmittedHealthMigrationAuthorization, MigrationAuthorizationAdmissionError>;

    /// Resolve one configured or default migration directory against the admitted cwd.
    fn resolve_migration_directory(&mut self, path: &str) -> Result<String, Self::Error>;
}

/// Cohesive external owners called in the exact source property order.
pub struct WorkerConfigurationOwners<'a, Database, Json, Url, Credential, Artifact> {
    pub database: &'a mut Database,
    pub json: &'a mut Json,
    pub url: &'a mut Url,
    pub credential: &'a mut Credential,
    pub artifact: &'a mut Artifact,
}

/// Load the exact worker trust boundary without reading ambient process or files directly.
pub fn load_worker_configuration<Environment, Database, Json, Url, Credential, Artifact>(
    environment: &mut Environment,
    owners: WorkerConfigurationOwners<'_, Database, Json, Url, Credential, Artifact>,
) -> Result<WorkerConfiguration, WorkerConfigurationLoadError>
where
    Environment: WorkerConfigurationEnvironment,
    Database: WorkerDatabaseConfigurationOwner,
    Json: WorkerJsonConfigurationOwner,
    Url: WorkerCompatibilityOriginOwner,
    Credential: WorkerCredentialOwner,
    Artifact: WorkerConfigurationArtifactOwner,
{
    let site_configuration_path = required_environment(
        read_environment(
            environment,
            WorkerConfigurationEnvironmentKey::SiteConfigurationPath,
        )?,
        WorkerConfigurationEnvironmentKey::SiteConfigurationPath,
    )?;
    let ecowitt_configuration_path = optional_environment(
        Some(
            read_environment(
                environment,
                WorkerConfigurationEnvironmentKey::EcowittConfigurationPath,
            )?
            .unwrap_or_else(|| DEFAULT_ECOWITT_CONFIGURATION_PATH.to_owned()),
        ),
        WorkerConfigurationEnvironmentKey::EcowittConfigurationPath,
    )?;
    let tempest_configuration_path = optional_environment(
        read_environment(
            environment,
            WorkerConfigurationEnvironmentKey::TempestConfigurationPath,
        )?,
        WorkerConfigurationEnvironmentKey::TempestConfigurationPath,
    )?;
    let tempest_api_key_path = optional_environment(
        read_environment(
            environment,
            WorkerConfigurationEnvironmentKey::TempestApiKeyFile,
        )?,
        WorkerConfigurationEnvironmentKey::TempestApiKeyFile,
    )?;
    let public_station_configuration_path = optional_environment(
        Some(
            read_environment(
                environment,
                WorkerConfigurationEnvironmentKey::PublicStationConfigurationPath,
            )?
            .unwrap_or_else(|| DEFAULT_PUBLIC_STATION_CONFIGURATION_PATH.to_owned()),
        ),
        WorkerConfigurationEnvironmentKey::PublicStationConfigurationPath,
    )?;
    let tide_configuration_path = optional_environment(
        Some(
            read_environment(
                environment,
                WorkerConfigurationEnvironmentKey::TideConfigurationPath,
            )?
            .unwrap_or_else(|| DEFAULT_TIDE_CONFIGURATION_PATH.to_owned()),
        ),
        WorkerConfigurationEnvironmentKey::TideConfigurationPath,
    )?;

    // require the Tempest catalog and credential together before loading any owner
    if tempest_configuration_path.is_some() != tempest_api_key_path.is_some() {
        return Err(WorkerConfigurationLoadError::TempestConfigurationPairRequired);
    }

    let adjustment_maintenance_api_origin = optional_environment(
        read_environment(
            environment,
            WorkerConfigurationEnvironmentKey::AdjustmentMaintenanceApiOrigin,
        )?,
        WorkerConfigurationEnvironmentKey::AdjustmentMaintenanceApiOrigin,
    )?;
    let database = owners
        .database
        .load_database_configuration()
        .map_err(|_| WorkerConfigurationLoadError::DatabaseUnavailable)?;
    let ecowitt = ecowitt_configuration_path
        .as_deref()
        .map(|path| {
            owners.json.load_ecowitt_configuration(path).map_err(|_| {
                WorkerConfigurationLoadError::MaterialUnavailable(
                    WorkerConfigurationMaterial::Ecowitt,
                )
            })
        })
        .transpose()?;
    let instance = bounded_environment(
        read_environment(
            environment,
            WorkerConfigurationEnvironmentKey::WorkerInstance,
        )?
        .unwrap_or_else(|| DEFAULT_WORKER_INSTANCE.to_owned()),
        WorkerConfigurationEnvironmentKey::WorkerInstance,
    )?;
    let migration_authorization_release = read_environment(
        environment,
        WorkerConfigurationEnvironmentKey::MigrationAuthorizationRelease,
    )?;
    let migration_authorization_history_sha256 = read_environment(
        environment,
        WorkerConfigurationEnvironmentKey::MigrationAuthorizationHistorySha256,
    )?;
    let migration_authorization = admit_migration_authorization(
        migration_authorization_release.as_deref(),
        migration_authorization_history_sha256.as_deref(),
        owners.artifact,
    )?;
    let migration_directory = owners
        .artifact
        .resolve_migration_directory(
            read_environment(
                environment,
                WorkerConfigurationEnvironmentKey::MigrationDirectory,
            )?
            .as_deref()
            .unwrap_or(DEFAULT_MIGRATION_DIRECTORY),
        )
        .map_err(|_| WorkerConfigurationLoadError::MigrationDirectoryUnavailable)?;
    let open_meteo_compatibility_origin = owners
        .url
        .parse_open_meteo_compatibility_origin(
            read_environment(
                environment,
                WorkerConfigurationEnvironmentKey::OpenMeteoCompatibilityOrigin,
            )?
            .as_deref(),
        )
        .map_err(|_| WorkerConfigurationLoadError::OpenMeteoCompatibilityOriginInvalid)?;
    let public_stations = public_station_configuration_path
        .as_deref()
        .map(|path| {
            owners
                .json
                .load_public_station_configuration(path)
                .map_err(|_| {
                    WorkerConfigurationLoadError::MaterialUnavailable(
                        WorkerConfigurationMaterial::PublicStations,
                    )
                })
        })
        .transpose()?;
    let rain_collection_enabled = read_environment(
        environment,
        WorkerConfigurationEnvironmentKey::RainCollectionEnabled,
    )?
    .as_deref()
        == Some("1");
    let site = owners
        .json
        .load_site_configuration(&site_configuration_path)
        .map_err(|_| {
            WorkerConfigurationLoadError::MaterialUnavailable(WorkerConfigurationMaterial::Site)
        })?;
    let tempest = tempest_configuration_path
        .as_deref()
        .map(|path| {
            owners.json.load_tempest_configuration(path).map_err(|_| {
                WorkerConfigurationLoadError::MaterialUnavailable(
                    WorkerConfigurationMaterial::Tempest,
                )
            })
        })
        .transpose()?;
    let tempest_api_key = tempest_api_key_path
        .as_deref()
        .map(|path| {
            owners
                .credential
                .read_tempest_api_key(path)
                .map_err(|_| WorkerConfigurationLoadError::TempestApiKeyUnavailable)
                .and_then(admit_tempest_api_key)
        })
        .transpose()?;
    let temperature_canary_kill_switch = read_environment(
        environment,
        WorkerConfigurationEnvironmentKey::TemperatureCanaryKillSwitch,
    )?
    .as_deref()
    .filter(|value| *value == "0")
    .map(|_| TemperatureCanaryKillSwitch::Zero);
    let tides = tide_configuration_path
        .as_deref()
        .map(|path| {
            owners.json.load_tide_configuration(path).map_err(|_| {
                WorkerConfigurationLoadError::MaterialUnavailable(
                    WorkerConfigurationMaterial::Tides,
                )
            })
        })
        .transpose()?;
    let version = admit_worker_release(read_environment(
        environment,
        WorkerConfigurationEnvironmentKey::Release,
    )?)?;

    Ok(WorkerConfiguration {
        adjustment_maintenance_api_origin,
        database,
        ecowitt,
        instance,
        migration_authorization,
        migration_directory,
        open_meteo_compatibility_origin,
        public_stations,
        rain_collection_enabled,
        site,
        site_configuration_path,
        tempest,
        tempest_api_key,
        temperature_canary_kill_switch,
        tides,
        version,
    })
}

/// Read one known environment value and retain only the key on adapter failure.
fn read_environment(
    environment: &mut impl WorkerConfigurationEnvironment,
    key: WorkerConfigurationEnvironmentKey,
) -> Result<Option<String>, WorkerConfigurationLoadError> {
    environment
        .read_environment(key)
        .map_err(|_| WorkerConfigurationLoadError::EnvironmentUnavailable(key))
}

/// Preserve omission while rejecting a configured whitespace-only optional value.
fn optional_environment(
    value: Option<String>,
    key: WorkerConfigurationEnvironmentKey,
) -> Result<Option<String>, WorkerConfigurationLoadError> {
    // reject a present path or origin whose JavaScript trim is empty
    if value.as_ref().is_some_and(|value| value.trim().is_empty()) {
        return Err(WorkerConfigurationLoadError::EmptyOptionalEnvironment(key));
    }
    Ok(value)
}

/// Require one configured non-whitespace environment value without normalizing it.
fn required_environment(
    value: Option<String>,
    key: WorkerConfigurationEnvironmentKey,
) -> Result<String, WorkerConfigurationLoadError> {
    // reject both omission and a configured whitespace-only value
    if value.as_ref().is_none_or(|value| value.trim().is_empty()) {
        return Err(WorkerConfigurationLoadError::RequiredEnvironment(key));
    }
    Ok(value.expect("required environment after presence admission"))
}

/// Select the source release once with the same admission used by the installed worker.
pub(crate) fn admit_worker_release(
    release: Option<String>,
) -> Result<String, WorkerConfigurationLoadError> {
    bounded_environment(
        release.unwrap_or_else(|| DEFAULT_WORKER_VERSION.to_owned()),
        WorkerConfigurationEnvironmentKey::Release,
    )
}

/// Enforce the source's 128 UTF-16-unit public heartbeat bound.
fn bounded_environment(
    value: String,
    key: WorkerConfigurationEnvironmentKey,
) -> Result<String, WorkerConfigurationLoadError> {
    // match JavaScript trim and string-length checks without changing the value
    if value
        .encode_utf16()
        .all(crate::api_shadow_legacy_values::js_trim_unit)
        || value.encode_utf16().count() > 128
    {
        return Err(WorkerConfigurationLoadError::BoundedEnvironment(key));
    }
    Ok(value)
}

/// Preserve absent empty pairs and admit only complete migration authorization material.
fn admit_migration_authorization(
    release: Option<&str>,
    history_sha256: Option<&str>,
    owner: &mut impl WorkerConfigurationArtifactOwner,
) -> Result<Option<AdmittedHealthMigrationAuthorization>, WorkerConfigurationLoadError> {
    let release = release.filter(|value| !value.is_empty());
    let history_sha256 = history_sha256.filter(|value| !value.is_empty());

    // preserve the explicitly empty compose contract as absent
    if release.is_none() && history_sha256.is_none() {
        return Ok(None);
    }
    let (Some(release), Some(history_sha256)) = (release, history_sha256) else {
        return Err(WorkerConfigurationLoadError::MigrationAuthorizationIncomplete);
    };
    owner
        .admit_migration_authorization(release, history_sha256)
        .map(Some)
        .map_err(|error| match error {
            MigrationAuthorizationAdmissionError::InvalidRelease => {
                WorkerConfigurationLoadError::MigrationAuthorizationInvalidRelease
            }
            MigrationAuthorizationAdmissionError::InvalidHistorySha256 => {
                WorkerConfigurationLoadError::MigrationAuthorizationInvalidHistorySha256
            }
        })
}

/// Strip trailing line endings and admit one bounded whitespace-free Tempest key.
fn admit_tempest_api_key(value: String) -> Result<String, WorkerConfigurationLoadError> {
    let value = value.trim_end_matches(['\r', '\n']).to_owned();

    // mirror the one-line, 256 UTF-16-unit credential rule
    if value.is_empty()
        || value.encode_utf16().count() > 256
        || value.chars().any(char::is_whitespace)
    {
        return Err(WorkerConfigurationLoadError::TempestApiKeyInvalid);
    }
    Ok(value)
}
