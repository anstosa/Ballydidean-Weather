//! Bind the Weather API process environment to existing database and startup configuration owners.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::num::NonZeroU16;
use std::path::{Component, Path, PathBuf};

use weather_api::{
    AdmittedMaintenanceArchiveOrigin, WeatherApiStartupPortError,
    WeatherApiStartupRawListenerSettings,
};
use weather_database::{
    AdmittedHealthMigrationAuthorization, DatabaseConfiguration, DatabasePasswordFileRead,
    DatabaseProcessConfigurationError, DatabaseProcessEnvironmentKey,
    DatabaseProcessEnvironmentRead, DatabaseProcessReadError, MigrationDigest,
    ProcessDatabasePasswordFile, load_process_database_configuration,
};

use crate::RuntimeMigrationSource;

const DEFAULT_API_APPLICATION_NAME: &str = "weather-api";
const DEFAULT_API_RELEASE: &str = "development";
const DEFAULT_MIGRATION_DIRECTORY: &str = "packages/database/migrations";

/// Closed non-database process settings read by API startup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiProcessEnvironmentKey {
    ApiPort,
    MaintenanceArchiveOrigin,
    MaintenanceInternalPort,
    MigrationAuthorizationHistorySha256,
    MigrationAuthorizationRelease,
    MigrationDirectory,
    Release,
    TemperatureCanaryKillSwitch,
    WindCanaryKillSwitch,
}

impl ApiProcessEnvironmentKey {
    /// Return the exact TypeScript process-environment spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApiPort => "WEATHER_API_PORT",
            Self::MaintenanceArchiveOrigin => "WEATHER_ADJUSTMENT_MAINTENANCE_ARCHIVE_ORIGIN",
            Self::MaintenanceInternalPort => "WEATHER_ADJUSTMENT_MAINTENANCE_INTERNAL_PORT",
            Self::MigrationAuthorizationHistorySha256 => {
                "WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256"
            }
            Self::MigrationAuthorizationRelease => "WEATHER_MIGRATION_AUTHORIZATION_RELEASE",
            Self::MigrationDirectory => "WEATHER_MIGRATION_DIRECTORY",
            Self::Release => "WEATHER_RELEASE",
            Self::TemperatureCanaryKillSwitch => {
                "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH"
            }
            Self::WindCanaryKillSwitch => "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH",
        }
    }
}

/// Value-free failure from an API process environment or path owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiProcessReadError {
    Unavailable,
}

/// Independent exact reads suitable for the deferred API startup-settings adapter.
pub trait ApiProcessEnvironmentRead: DatabaseProcessEnvironmentRead {
    /// Read one known API key without trimming, defaulting, or parsing its value.
    fn read_api_environment(
        &self,
        key: ApiProcessEnvironmentKey,
    ) -> Result<Option<String>, ApiProcessReadError>;
}

/// Resolve one selected migration path with Node `path.resolve`-equivalent ownership.
pub trait ApiMigrationDirectoryResolve {
    /// Resolve without reading, canonicalizing, or requiring the target to exist.
    fn resolve_migration_directory(&self, path: &str) -> Result<PathBuf, ApiProcessReadError>;
}

/// Actual current-process reader over the two closed environment key sets.
#[derive(Clone, Copy, Debug, Default)]
pub struct CurrentApiProcessEnvironment;

impl DatabaseProcessEnvironmentRead for CurrentApiProcessEnvironment {
    /// Read one database key with the process string-decoding replacement behavior.
    fn read_environment(
        &self,
        key: DatabaseProcessEnvironmentKey,
    ) -> Result<Option<String>, DatabaseProcessReadError> {
        Ok(std::env::var_os(key.as_str()).map(|value| value.to_string_lossy().into_owned()))
    }
}

impl ApiProcessEnvironmentRead for CurrentApiProcessEnvironment {
    /// Read one exact API setting while preserving absence and configured emptiness.
    fn read_api_environment(
        &self,
        key: ApiProcessEnvironmentKey,
    ) -> Result<Option<String>, ApiProcessReadError> {
        Ok(std::env::var_os(key.as_str()).map(|value| value.to_string_lossy().into_owned()))
    }
}

/// Current-directory path resolver matching `resolve` without filesystem canonicalization.
#[derive(Clone, Copy, Debug, Default)]
pub struct CurrentApiMigrationDirectory;

impl ApiMigrationDirectoryResolve for CurrentApiMigrationDirectory {
    /// Make the selected path absolute without requiring a migration directory to exist.
    fn resolve_migration_directory(&self, path: &str) -> Result<PathBuf, ApiProcessReadError> {
        let selected = Path::new(path);
        let absolute = if selected.is_absolute() {
            selected.to_owned()
        } else {
            std::env::current_dir()
                .map_err(|_| ApiProcessReadError::Unavailable)?
                .join(selected)
        };
        lexical_absolute_path(&absolute).ok_or(ApiProcessReadError::Unavailable)
    }
}

/// Process-owned API configuration before any server, URL, or JSON construction.
#[derive(Clone, PartialEq)]
pub struct ApiProcessConfiguration {
    pub database: DatabaseConfiguration,
    pub migration: ApiProcessMigrationSource,
}

impl ApiProcessConfiguration {
    /// Separate the password-bearing database configuration from read-only migration material.
    pub fn into_parts(self) -> (DatabaseConfiguration, ApiProcessMigrationSource) {
        (self.database, self.migration)
    }
}

/// Read-only migration path, authorization, and running release retained without database secrets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiProcessMigrationSource {
    migration_authorization: Option<AdmittedHealthMigrationAuthorization>,
    migration_directory: PathBuf,
    release: String,
}

impl RuntimeMigrationSource for ApiProcessMigrationSource {
    /// Borrow the lexically resolved migration artifact directory.
    fn migration_directory(&self) -> &Path {
        &self.migration_directory
    }

    /// Borrow the complete already admitted immutable-release authorization.
    fn migration_authorization(&self) -> Option<&AdmittedHealthMigrationAuthorization> {
        self.migration_authorization.as_ref()
    }

    /// Borrow the public running release used by health readiness.
    fn release(&self) -> &str {
        &self.release
    }
}

/// Approved URL owner retained until the verified maintenance witness exists.
pub trait ApiMaintenanceArchiveOriginOwner {
    type Error;

    /// Parse and admit the configured compose-private archive origin.
    fn admit_maintenance_archive_origin(
        &mut self,
        value: &str,
    ) -> Result<AdmittedMaintenanceArchiveOrigin, Self::Error>;
}

/// Raw public and private listener settings with all private parsing deferred.
pub struct ApiProcessListenerSettings<Origins> {
    maintenance_archive_origin: Option<String>,
    maintenance_internal_port: Option<String>,
    origins: Origins,
    public_port: Option<String>,
}

impl<Origins> ApiProcessListenerSettings<Origins> {
    /// Retain exact raw values without validating the optional private listener.
    pub fn new(
        public_port: Option<String>,
        maintenance_internal_port: Option<String>,
        maintenance_archive_origin: Option<String>,
        origins: Origins,
    ) -> Self {
        Self {
            maintenance_archive_origin,
            maintenance_internal_port,
            origins,
            public_port,
        }
    }
}

impl<Origins: ApiMaintenanceArchiveOriginOwner> WeatherApiStartupRawListenerSettings
    for ApiProcessListenerSettings<Origins>
{
    /// Apply the public default and Node numeric admission only when startup requests it.
    fn admit_public_port(&mut self) -> Result<NonZeroU16, WeatherApiStartupPortError> {
        parse_api_listener_port(self.public_port.as_deref().unwrap_or("8080"))
            .ok_or(WeatherApiStartupPortError)
    }

    /// Preserve configured-versus-absent maintenance-port state without parsing.
    fn maintenance_port_present(&self) -> bool {
        self.maintenance_internal_port.is_some()
    }

    /// Preserve configured-versus-absent archive-origin state without parsing.
    fn maintenance_archive_origin_present(&self) -> bool {
        self.maintenance_archive_origin.is_some()
    }

    /// Parse the private port only after startup has admitted its witness.
    fn admit_maintenance_port(&mut self) -> Result<NonZeroU16, WeatherApiStartupPortError> {
        self.maintenance_internal_port
            .as_deref()
            .and_then(parse_api_listener_port)
            .ok_or(WeatherApiStartupPortError)
    }

    /// Delegate private URL parsing only after startup has admitted its witness.
    fn admit_maintenance_archive_origin(
        &mut self,
    ) -> Result<AdmittedMaintenanceArchiveOrigin, WeatherApiStartupPortError> {
        let value = self
            .maintenance_archive_origin
            .as_deref()
            .ok_or(WeatherApiStartupPortError)?;
        self.origins
            .admit_maintenance_archive_origin(value)
            .map_err(|_| WeatherApiStartupPortError)
    }
}

/// Closed process-configuration failure without raw environment or credential material.
#[derive(Debug)]
pub enum ApiProcessConfigurationError {
    Database(DatabaseProcessConfigurationError),
    EnvironmentUnavailable(ApiProcessEnvironmentKey),
    InvalidRelease,
    MigrationAuthorizationIncomplete,
    MigrationAuthorizationInvalidHistorySha256,
    MigrationAuthorizationInvalidRelease,
    MigrationDirectoryUnavailable,
}

impl Display for ApiProcessConfigurationError {
    /// Render only stable owner and key diagnostics.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => Display::fmt(error, formatter),
            Self::EnvironmentUnavailable(key) => {
                write!(formatter, "{} could not be read", key.as_str())
            }
            Self::InvalidRelease => {
                formatter.write_str("WEATHER_RELEASE must be non-empty and bounded")
            }
            Self::MigrationAuthorizationIncomplete => {
                formatter.write_str("migration authorization must be complete")
            }
            Self::MigrationAuthorizationInvalidHistorySha256 => {
                formatter.write_str("migration authorization history SHA-256 is invalid")
            }
            Self::MigrationAuthorizationInvalidRelease => {
                formatter.write_str("migration authorization release is invalid")
            }
            Self::MigrationDirectoryUnavailable => {
                formatter.write_str("WEATHER_MIGRATION_DIRECTORY could not be resolved")
            }
        }
    }
}

impl Error for ApiProcessConfigurationError {
    /// Expose only the existing redacted database failure as an underlying source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

/// Load API process settings through injected environment, mounted-password, and path owners.
pub fn load_api_process_configuration<Environment, PasswordFiles, Directories>(
    environment: &Environment,
    password_files: &PasswordFiles,
    directories: &Directories,
) -> Result<ApiProcessConfiguration, ApiProcessConfigurationError>
where
    Environment: ApiProcessEnvironmentRead,
    PasswordFiles: DatabasePasswordFileRead,
    Directories: ApiMigrationDirectoryResolve,
{
    let database =
        load_process_database_configuration(&ApiDatabaseEnvironment(environment), password_files)
            .map_err(ApiProcessConfigurationError::Database)?;
    let release = read_api_process_setting(environment, ApiProcessEnvironmentKey::Release)?
        .unwrap_or_else(|| DEFAULT_API_RELEASE.to_owned());

    // preserve the public release value after applying its exact Node bounds
    if release.trim_matches(node_number_whitespace).is_empty()
        || release.encode_utf16().count() > 128
    {
        return Err(ApiProcessConfigurationError::InvalidRelease);
    }

    let migration_authorization_release = read_api_process_setting(
        environment,
        ApiProcessEnvironmentKey::MigrationAuthorizationRelease,
    )?;
    let migration_authorization_history_sha256 = read_api_process_setting(
        environment,
        ApiProcessEnvironmentKey::MigrationAuthorizationHistorySha256,
    )?;
    let migration_authorization = admit_migration_authorization(
        migration_authorization_release.as_deref(),
        migration_authorization_history_sha256.as_deref(),
    )?;
    let migration_directory =
        read_api_process_setting(environment, ApiProcessEnvironmentKey::MigrationDirectory)?
            .unwrap_or_else(|| DEFAULT_MIGRATION_DIRECTORY.to_owned());
    let migration_directory = directories
        .resolve_migration_directory(&migration_directory)
        .map_err(|_| ApiProcessConfigurationError::MigrationDirectoryUnavailable)?;

    Ok(ApiProcessConfiguration {
        database,
        migration: ApiProcessMigrationSource {
            migration_authorization,
            migration_directory,
            release,
        },
    })
}

/// Read only listener presence and raw text; parsing remains owned by startup admission.
pub fn load_api_process_listener_settings<Environment, Origins>(
    environment: &Environment,
    origins: Origins,
) -> Result<ApiProcessListenerSettings<Origins>, ApiProcessConfigurationError>
where
    Environment: ApiProcessEnvironmentRead,
{
    Ok(ApiProcessListenerSettings::new(
        read_api_process_setting(environment, ApiProcessEnvironmentKey::ApiPort)?,
        read_api_process_setting(
            environment,
            ApiProcessEnvironmentKey::MaintenanceInternalPort,
        )?,
        read_api_process_setting(
            environment,
            ApiProcessEnvironmentKey::MaintenanceArchiveOrigin,
        )?,
        origins,
    ))
}

/// Load the actual API process using its mounted password and current-directory owners.
pub fn load_current_api_process_configuration()
-> Result<ApiProcessConfiguration, ApiProcessConfigurationError> {
    load_api_process_configuration(
        &CurrentApiProcessEnvironment,
        &ProcessDatabasePasswordFile,
        &CurrentApiMigrationDirectory,
    )
}

/// Database environment wrapper applying the API application-name default only to absence.
struct ApiDatabaseEnvironment<'a, Environment>(&'a Environment);

impl<Environment: ApiProcessEnvironmentRead> DatabaseProcessEnvironmentRead
    for ApiDatabaseEnvironment<'_, Environment>
{
    /// Preserve every database value while replacing only an absent application name.
    fn read_environment(
        &self,
        key: DatabaseProcessEnvironmentKey,
    ) -> Result<Option<String>, DatabaseProcessReadError> {
        let value = self.0.read_environment(key)?;

        // match the TypeScript nullish default rather than replacing configured emptiness
        if key == DatabaseProcessEnvironmentKey::ApplicationName && value.is_none() {
            return Ok(Some(DEFAULT_API_APPLICATION_NAME.to_owned()));
        }

        Ok(value)
    }
}

/// Read one API key and retain only its identity on adapter failure.
pub fn read_api_process_setting(
    environment: &impl ApiProcessEnvironmentRead,
    key: ApiProcessEnvironmentKey,
) -> Result<Option<String>, ApiProcessConfigurationError> {
    environment
        .read_api_environment(key)
        .map_err(|_| ApiProcessConfigurationError::EnvironmentUnavailable(key))
}

/// Preserve empty-pair absence and admit one complete immutable migration authorization.
fn admit_migration_authorization(
    release: Option<&str>,
    history_sha256: Option<&str>,
) -> Result<Option<AdmittedHealthMigrationAuthorization>, ApiProcessConfigurationError> {
    let release = release.filter(|value| !value.is_empty());
    let history_sha256 = history_sha256.filter(|value| !value.is_empty());

    // treat the explicit empty compose pair the same as complete absence
    if release.is_none() && history_sha256.is_none() {
        return Ok(None);
    }
    let (Some(release), Some(history_sha256)) = (release, history_sha256) else {
        return Err(ApiProcessConfigurationError::MigrationAuthorizationIncomplete);
    };

    // admit only the immutable deployment release grammar used by migration readiness
    if !migration_authorization_release_is_valid(release) {
        return Err(ApiProcessConfigurationError::MigrationAuthorizationInvalidRelease);
    }

    // admit only a canonical lowercase textual digest without calculating one here
    if history_sha256.len() != 64
        || !history_sha256
            .bytes()
            .all(|value| value.is_ascii_digit() || matches!(value, b'a'..=b'f'))
    {
        return Err(ApiProcessConfigurationError::MigrationAuthorizationInvalidHistorySha256);
    }

    Ok(Some(AdmittedHealthMigrationAuthorization {
        history_sha256: MigrationDigest::admitted(history_sha256),
        release: release.to_owned(),
    }))
}

/// Match the immutable release regular expression without adding a regex dependency.
pub(crate) fn migration_authorization_release_is_valid(release: &str) -> bool {
    let bytes = release.as_bytes();

    // require YYYY.MM.DD followed by a one- or two-digit nonzero ordinal
    if !matches!(bytes.len(), 12 | 13)
        || bytes[4] != b'.'
        || bytes[7] != b'.'
        || bytes[10] != b'-'
        || !(b'1'..=b'9').contains(&bytes[11])
    {
        return false;
    }

    bytes[..4].iter().all(|value| value.is_ascii_digit())
        && bytes[5..7].iter().all(|value| value.is_ascii_digit())
        && bytes[8..10].iter().all(|value| value.is_ascii_digit())
        && bytes[12..].iter().all(|value| value.is_ascii_digit())
}

/// Parse one Node numeric listener setting into the nonzero TCP port range.
fn parse_api_listener_port(source: &str) -> Option<NonZeroU16> {
    let value = source.trim_matches(node_number_whitespace);
    let prefixed_radix = match value.as_bytes() {
        [b'0', b'x' | b'X', ..] => Some(16),
        [b'0', b'o' | b'O', ..] => Some(8),
        [b'0', b'b' | b'B', ..] => Some(2),
        _ => None,
    };
    let port = if let Some(radix) = prefixed_radix {
        let digits = &value[2..];

        // reject empty signed or radix-invalid prefixed values
        if digits.is_empty() || !digits.chars().all(|digit| digit.is_digit(radix)) {
            return None;
        }
        u16::from_str_radix(digits, radix).ok()?
    } else {
        let parsed: f64 = value.parse().ok()?;

        // preserve Node Number rounding before enforcing the integer listener range
        if !parsed.is_finite()
            || parsed.fract() != 0.0
            || !(1.0..=f64::from(u16::MAX)).contains(&parsed)
        {
            return None;
        }
        parsed as u16
    };
    NonZeroU16::new(port)
}

/// Collapse lexical dot and parent components without reading the selected filesystem path.
fn lexical_absolute_path(value: &Path) -> Option<PathBuf> {
    // require one process-rooted path before lexical normalization
    if !value.is_absolute() {
        return None;
    }
    let mut output = PathBuf::from("/");

    // reproduce POSIX path.resolve component handling without following symlinks
    for component in value.components() {
        match component {
            Component::RootDir | Component::CurDir => (),
            Component::ParentDir => {
                output.pop();
            }
            Component::Normal(value) => output.push(value),
            Component::Prefix(_) => return None,
        }
    }
    Some(output)
}

/// Preserve the exact ECMAScript whitespace set used by Node Number and String trim.
fn node_number_whitespace(value: char) -> bool {
    matches!(
        value,
        '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}'
            | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}'
            | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;

    #[derive(Default)]
    struct Environment {
        api: BTreeMap<&'static str, String>,
        database: BTreeMap<&'static str, String>,
    }

    impl DatabaseProcessEnvironmentRead for Environment {
        /// Return one exact configured database fixture value.
        fn read_environment(
            &self,
            key: DatabaseProcessEnvironmentKey,
        ) -> Result<Option<String>, DatabaseProcessReadError> {
            Ok(self.database.get(key.as_str()).cloned())
        }
    }

    impl ApiProcessEnvironmentRead for Environment {
        /// Return one exact configured API fixture value.
        fn read_api_environment(
            &self,
            key: ApiProcessEnvironmentKey,
        ) -> Result<Option<String>, ApiProcessReadError> {
            Ok(self.api.get(key.as_str()).cloned())
        }
    }

    struct PasswordFiles;

    impl DatabasePasswordFileRead for PasswordFiles {
        /// Return the selected mounted one-line fixture secret.
        fn read_password_file(&self, path: &str) -> Result<Vec<u8>, DatabaseProcessReadError> {
            assert_eq!(path, "/run/secrets/weather_db_password");
            Ok(b"database-secret\n".to_vec())
        }
    }

    #[derive(Default)]
    struct Directories {
        paths: RefCell<Vec<String>>,
    }

    impl ApiMigrationDirectoryResolve for Directories {
        /// Retain the selected source path and return its fixture absolute projection.
        fn resolve_migration_directory(&self, path: &str) -> Result<PathBuf, ApiProcessReadError> {
            self.paths.borrow_mut().push(path.to_owned());
            Ok(Path::new("/srv/weather").join(path))
        }
    }

    /// Defaults only absent values while retaining release and migration inputs.
    #[test]
    fn loads_database_release_and_migration_configuration() {
        let environment = Environment {
            api: BTreeMap::from([
                (
                    "WEATHER_MIGRATION_AUTHORIZATION_RELEASE",
                    "2026.11.23-1".to_owned(),
                ),
                (
                    "WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256",
                    "a".repeat(64),
                ),
            ]),
            database: database_environment(),
        };
        let directories = Directories::default();

        let configuration =
            load_api_process_configuration(&environment, &PasswordFiles, &directories).unwrap();

        assert_eq!(configuration.database.application_name, "weather-api");
        assert_eq!(configuration.database.password, "database-secret");
        assert_eq!(configuration.migration.release(), "development");
        assert_eq!(
            configuration.migration.migration_directory(),
            Path::new("/srv/weather/packages/database/migrations")
        );
        assert_eq!(
            directories.paths.borrow().as_slice(),
            [DEFAULT_MIGRATION_DIRECTORY]
        );
        assert_eq!(
            configuration
                .migration
                .migration_authorization()
                .map(|authorization| authorization.release.as_str()),
            Some("2026.11.23-1")
        );
        assert_eq!(
            configuration
                .migration
                .migration_authorization()
                .map(|authorization| authorization.history_sha256.as_str()),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    /// A configured empty application name remains configured under nullish default semantics.
    #[test]
    fn preserves_explicit_database_application_name_and_release_directory() {
        let mut environment = Environment {
            api: BTreeMap::from([
                ("WEATHER_RELEASE", "release-a".to_owned()),
                ("WEATHER_MIGRATION_DIRECTORY", "/opt/migrations".to_owned()),
            ]),
            database: database_environment(),
        };
        environment.database.insert(
            DatabaseProcessEnvironmentKey::ApplicationName.as_str(),
            String::new(),
        );
        let directories = Directories::default();

        let configuration =
            load_api_process_configuration(&environment, &PasswordFiles, &directories).unwrap();

        assert_eq!(configuration.database.application_name, "");
        assert_eq!(configuration.migration.release(), "release-a");
        assert_eq!(
            configuration.migration.migration_directory(),
            Path::new("/opt/migrations")
        );
        assert_eq!(configuration.migration.migration_authorization(), None);
        assert_eq!(directories.paths.borrow().as_slice(), ["/opt/migrations"]);
    }

    struct Origins {
        inputs: RefCell<Vec<String>>,
    }

    impl ApiMaintenanceArchiveOriginOwner for Origins {
        type Error = ();

        /// Admit the fixture origin only when the startup listener explicitly requests it.
        fn admit_maintenance_archive_origin(
            &mut self,
            value: &str,
        ) -> Result<AdmittedMaintenanceArchiveOrigin, Self::Error> {
            self.inputs.borrow_mut().push(value.to_owned());
            Ok(AdmittedMaintenanceArchiveOrigin::admitted(value.to_owned()))
        }
    }

    /// Presence remains raw while public and private values parse only on explicit admission.
    #[test]
    fn defers_listener_parsing_and_origin_admission() {
        let environment = Environment {
            api: BTreeMap::from([
                ("WEATHER_API_PORT", "\u{feff}0x1f90".to_owned()),
                (
                    "WEATHER_ADJUSTMENT_MAINTENANCE_INTERNAL_PORT",
                    "8.081e3".to_owned(),
                ),
                (
                    "WEATHER_ADJUSTMENT_MAINTENANCE_ARCHIVE_ORIGIN",
                    "http://archive:8080".to_owned(),
                ),
            ]),
            database: BTreeMap::new(),
        };
        let mut settings = load_api_process_listener_settings(
            &environment,
            Origins {
                inputs: RefCell::new(Vec::new()),
            },
        )
        .unwrap();

        assert!(settings.maintenance_port_present());
        assert!(settings.maintenance_archive_origin_present());
        assert!(settings.origins.inputs.borrow().is_empty());
        assert_eq!(settings.admit_public_port().unwrap().get(), 8_080);
        assert_eq!(settings.admit_maintenance_port().unwrap().get(), 8_081);
        assert_eq!(
            settings
                .admit_maintenance_archive_origin()
                .unwrap()
                .canonical(),
            "http://archive:8080"
        );
        assert_eq!(
            settings.origins.inputs.borrow().as_slice(),
            ["http://archive:8080"]
        );
    }

    /// Lexical directory resolution collapses parents and treats an empty input as cwd.
    #[test]
    fn resolves_migration_directories_like_node_path_resolve() {
        assert_eq!(
            lexical_absolute_path(Path::new("/srv/weather/../migrations")),
            Some(PathBuf::from("/srv/migrations"))
        );
        let expected = lexical_absolute_path(&std::env::current_dir().unwrap()).unwrap();
        assert_eq!(
            CurrentApiMigrationDirectory
                .resolve_migration_directory("")
                .unwrap(),
            expected
        );
    }

    /// ECMAScript-only trim characters still reject an empty public release.
    #[test]
    fn rejects_ecmascript_whitespace_release() {
        let environment = Environment {
            api: BTreeMap::from([("WEATHER_RELEASE", "\u{feff}".to_owned())]),
            database: database_environment(),
        };

        let result =
            load_api_process_configuration(&environment, &PasswordFiles, &Directories::default());
        // retain the typed error without requiring password-bearing configuration diagnostics
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("whitespace-only release must fail"),
        };

        assert!(matches!(
            error,
            ApiProcessConfigurationError::InvalidRelease
        ));
    }

    /// Migration authorization admits only a complete immutable release and canonical digest.
    #[test]
    fn rejects_partial_migration_authorization_before_directory_resolution() {
        let environment = Environment {
            api: BTreeMap::from([(
                "WEATHER_MIGRATION_AUTHORIZATION_RELEASE",
                "2026.11.23-1".to_owned(),
            )]),
            database: database_environment(),
        };
        let directories = Directories::default();

        let result = load_api_process_configuration(&environment, &PasswordFiles, &directories);
        // retain the typed error without requiring password-bearing configuration diagnostics
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("partial migration authorization must fail"),
        };

        assert!(matches!(
            error,
            ApiProcessConfigurationError::MigrationAuthorizationIncomplete
        ));
        assert!(directories.paths.borrow().is_empty());
    }

    /// Build the minimum complete database process fixture without an application name.
    fn database_environment() -> BTreeMap<&'static str, String> {
        BTreeMap::from([
            (
                DatabaseProcessEnvironmentKey::DatabaseName.as_str(),
                "weather".to_owned(),
            ),
            (
                DatabaseProcessEnvironmentKey::Host.as_str(),
                "database".to_owned(),
            ),
            (
                DatabaseProcessEnvironmentKey::PasswordFile.as_str(),
                "/run/secrets/weather_db_password".to_owned(),
            ),
            (
                DatabaseProcessEnvironmentKey::User.as_str(),
                "weather".to_owned(),
            ),
        ])
    }
}
