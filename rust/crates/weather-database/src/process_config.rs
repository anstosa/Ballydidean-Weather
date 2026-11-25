//! Bind process-owned environment and password-file inputs to typed database configuration.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::config::{
    DatabaseConfiguration, DatabaseConfigurationError, DatabaseConfigurationInput,
    DatabasePasswordLoader, load_database_configuration,
};

const PASSWORD_FILE_READ_FAILURE: &str = "database password file could not be read";

/// Closed environment keys consumed by the Node database configuration owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseProcessEnvironmentKey {
    ApplicationName,
    DatabaseName,
    Host,
    LockTimeoutMs,
    PasswordFile,
    Port,
    Ssl,
    StatementTimeoutMs,
    User,
}

/// Exact process database environment surface, with no ambient key enumeration.
pub const DATABASE_PROCESS_ENVIRONMENT_KEYS: [DatabaseProcessEnvironmentKey; 9] = [
    DatabaseProcessEnvironmentKey::ApplicationName,
    DatabaseProcessEnvironmentKey::DatabaseName,
    DatabaseProcessEnvironmentKey::Host,
    DatabaseProcessEnvironmentKey::LockTimeoutMs,
    DatabaseProcessEnvironmentKey::PasswordFile,
    DatabaseProcessEnvironmentKey::Port,
    DatabaseProcessEnvironmentKey::Ssl,
    DatabaseProcessEnvironmentKey::StatementTimeoutMs,
    DatabaseProcessEnvironmentKey::User,
];

impl DatabaseProcessEnvironmentKey {
    /// Return the exact source environment spelling without accepting arbitrary names.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApplicationName => "WEATHER_DATABASE_APPLICATION_NAME",
            Self::DatabaseName => "WEATHER_DATABASE_NAME",
            Self::Host => "WEATHER_DATABASE_HOST",
            Self::LockTimeoutMs => "WEATHER_DATABASE_LOCK_TIMEOUT_MS",
            Self::PasswordFile => "WEATHER_DATABASE_PASSWORD_FILE",
            Self::Port => "WEATHER_DATABASE_PORT",
            Self::Ssl => "WEATHER_DATABASE_SSL",
            Self::StatementTimeoutMs => "WEATHER_DATABASE_STATEMENT_TIMEOUT_MS",
            Self::User => "WEATHER_DATABASE_USER",
        }
    }
}

/// Value-free failure from an injected process environment or filesystem reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseProcessReadError {
    Unavailable,
}

/// Enumerated process environment reader with configured-versus-absent preservation.
pub trait DatabaseProcessEnvironmentRead {
    /// Read one known key without normalizing or trimming its configured value.
    fn read_environment(
        &self,
        key: DatabaseProcessEnvironmentKey,
    ) -> Result<Option<String>, DatabaseProcessReadError>;
}

/// Password-file byte reader invoked only with the selected configured reference.
pub trait DatabasePasswordFileRead {
    /// Read the complete mounted password file without interpreting its contents.
    fn read_password_file(&self, path: &str) -> Result<Vec<u8>, DatabaseProcessReadError>;
}

/// Actual current-process environment implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct CurrentProcessDatabaseEnvironment;

impl DatabaseProcessEnvironmentRead for CurrentProcessDatabaseEnvironment {
    /// Read one exact known environment key while retaining empty configured values.
    fn read_environment(
        &self,
        key: DatabaseProcessEnvironmentKey,
    ) -> Result<Option<String>, DatabaseProcessReadError> {
        // reject non-unicode process values without copying them into diagnostics
        match std::env::var_os(key.as_str()) {
            None => Ok(None),
            Some(value) => value
                .into_string()
                .map(Some)
                .map_err(|_| DatabaseProcessReadError::Unavailable),
        }
    }
}

/// Actual process-owned mounted-password filesystem implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessDatabasePasswordFile;

impl DatabasePasswordFileRead for ProcessDatabasePasswordFile {
    /// Read only the path selected by typed database configuration input.
    fn read_password_file(&self, path: &str) -> Result<Vec<u8>, DatabaseProcessReadError> {
        std::fs::read(path).map_err(|_| DatabaseProcessReadError::Unavailable)
    }
}

/// Closed process-adapter failure without raw environment or password material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseProcessConfigurationError {
    EnvironmentUnavailable(DatabaseProcessEnvironmentKey),
    Configuration(DatabaseConfigurationError),
}

impl Display for DatabaseProcessConfigurationError {
    /// Render only stable key names or the existing redacted configuration diagnostic.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EnvironmentUnavailable(key) => {
                write!(formatter, "{} could not be read", key.as_str())
            }
            Self::Configuration(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for DatabaseProcessConfigurationError {
    /// Expose only the existing value-free configuration failure as an underlying source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EnvironmentUnavailable(_) => None,
            Self::Configuration(error) => Some(error),
        }
    }
}

/// Read exactly the known process keys into the existing configuration input shape.
pub fn database_configuration_input_from_environment(
    environment: &impl DatabaseProcessEnvironmentRead,
) -> Result<DatabaseConfigurationInput, DatabaseProcessConfigurationError> {
    Ok(DatabaseConfigurationInput {
        application_name: read(environment, DatabaseProcessEnvironmentKey::ApplicationName)?,
        database: read(environment, DatabaseProcessEnvironmentKey::DatabaseName)?,
        host: read(environment, DatabaseProcessEnvironmentKey::Host)?,
        lock_timeout_ms: read(environment, DatabaseProcessEnvironmentKey::LockTimeoutMs)?,
        password_file: read(environment, DatabaseProcessEnvironmentKey::PasswordFile)?,
        port: read(environment, DatabaseProcessEnvironmentKey::Port)?,
        ssl: read(environment, DatabaseProcessEnvironmentKey::Ssl)?,
        statement_timeout_ms: read(
            environment,
            DatabaseProcessEnvironmentKey::StatementTimeoutMs,
        )?,
        user: read(environment, DatabaseProcessEnvironmentKey::User)?,
    })
}

/// Resolve injected process inputs through the existing Node-compatible parser.
pub fn load_process_database_configuration(
    environment: &impl DatabaseProcessEnvironmentRead,
    files: &impl DatabasePasswordFileRead,
) -> Result<DatabaseConfiguration, DatabaseProcessConfigurationError> {
    let input = database_configuration_input_from_environment(environment)?;
    load_database_configuration(input, &SelectedPasswordFile(files))
        .map_err(DatabaseProcessConfigurationError::Configuration)
}

/// Resolve the actual current process and mounted password file when invoked by runtime startup.
pub fn load_current_process_database_configuration()
-> Result<DatabaseConfiguration, DatabaseProcessConfigurationError> {
    load_process_database_configuration(
        &CurrentProcessDatabaseEnvironment,
        &ProcessDatabasePasswordFile,
    )
}

/// Read one enumerated key and retain only its key on failure.
fn read(
    environment: &impl DatabaseProcessEnvironmentRead,
    key: DatabaseProcessEnvironmentKey,
) -> Result<Option<String>, DatabaseProcessConfigurationError> {
    environment
        .read_environment(key)
        .map_err(|_| DatabaseProcessConfigurationError::EnvironmentUnavailable(key))
}

/// Adapt only the selected password-file reference to the existing configuration loader.
struct SelectedPasswordFile<'a, Files>(&'a Files);

impl<Files: DatabasePasswordFileRead> DatabasePasswordLoader for SelectedPasswordFile<'_, Files> {
    /// Read the selected file and decode it with Node's replacement-character UTF-8 behavior.
    fn load_password(&self, reference: &str) -> Result<String, String> {
        self.0
            .read_password_file(reference)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .map_err(|_| PASSWORD_FILE_READ_FAILURE.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;

    #[derive(Default)]
    struct Environment {
        failure: Option<DatabaseProcessEnvironmentKey>,
        reads: RefCell<Vec<DatabaseProcessEnvironmentKey>>,
        values: BTreeMap<&'static str, String>,
    }

    impl DatabaseProcessEnvironmentRead for Environment {
        /// Return exact configured fixture values without applying defaults or trimming.
        fn read_environment(
            &self,
            key: DatabaseProcessEnvironmentKey,
        ) -> Result<Option<String>, DatabaseProcessReadError> {
            self.reads.borrow_mut().push(key);
            // inject one value-free process read failure
            if self.failure == Some(key) {
                return Err(DatabaseProcessReadError::Unavailable);
            }
            Ok(self.values.get(key.as_str()).cloned())
        }
    }

    struct Files {
        bytes: Option<Vec<u8>>,
        paths: RefCell<Vec<String>>,
    }

    impl DatabasePasswordFileRead for Files {
        /// Return one mounted-password fixture while retaining its selected reference.
        fn read_password_file(&self, path: &str) -> Result<Vec<u8>, DatabaseProcessReadError> {
            self.paths.borrow_mut().push(path.to_owned());
            self.bytes
                .clone()
                .ok_or(DatabaseProcessReadError::Unavailable)
        }
    }

    /// Read every explicit source key and retain ordinary absent defaults.
    #[test]
    fn loads_required_values_and_absent_defaults_from_injected_readers() {
        let environment = required_environment();
        let files = password_file();
        let configuration = load_process_database_configuration(&environment, &files).unwrap();
        assert_eq!(configuration.application_name, "weather-database");
        assert_eq!(configuration.database, "weather");
        assert_eq!(configuration.host, "database");
        assert_eq!(configuration.lock_timeout_ms, 10_000);
        assert_eq!(configuration.password, "fixture-password");
        assert_eq!(configuration.port, 5_432);
        assert!(!configuration.ssl);
        assert_eq!(configuration.statement_timeout_ms, 30_000);
        assert_eq!(configuration.user, "weather_app");
        assert_eq!(
            environment.reads.borrow().as_slice(),
            DATABASE_PROCESS_ENVIRONMENT_KEYS
        );
        assert_eq!(files.paths.borrow().as_slice(), ["/fixture/password"]);
    }

    /// Preserve configured empty values instead of treating them as absent defaults.
    #[test]
    fn configured_empty_values_reach_existing_parser_unchanged() {
        let mut environment = required_environment();
        environment.values.insert(
            DatabaseProcessEnvironmentKey::ApplicationName.as_str(),
            String::new(),
        );
        let configuration =
            load_process_database_configuration(&environment, &password_file()).unwrap();
        assert_eq!(configuration.application_name, "");

        environment
            .values
            .insert(DatabaseProcessEnvironmentKey::Port.as_str(), String::new());
        // retain the expected parser refusal without requiring secret-bearing debug output
        let error = match load_process_database_configuration(&environment, &password_file()) {
            Ok(_) => panic!("empty configured port unexpectedly selected the default"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            DatabaseProcessConfigurationError::Configuration(
                DatabaseConfigurationError::InvalidInteger {
                    name: "WEATHER_DATABASE_PORT",
                    minimum: 1,
                    maximum: 65_535,
                }
            )
        );
    }

    /// Keep process-reader failure categorical and stop before filesystem access.
    #[test]
    fn environment_failure_retains_only_the_failed_key() {
        let mut environment = required_environment();
        environment.failure = Some(DatabaseProcessEnvironmentKey::Host);
        let files = password_file();
        // extract the value-free process error without formatting a configuration
        let error = match load_process_database_configuration(&environment, &files) {
            Ok(_) => panic!("failed environment read unexpectedly loaded configuration"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            DatabaseProcessConfigurationError::EnvironmentUnavailable(
                DatabaseProcessEnvironmentKey::Host
            )
        );
        assert!(files.paths.borrow().is_empty());
    }

    /// Map an ordinary mounted-file refusal to one fixed configuration failure.
    #[test]
    fn password_file_failure_uses_fixed_redacted_detail() {
        let environment = required_environment();
        let files = Files {
            bytes: None,
            paths: RefCell::new(Vec::new()),
        };
        // extract the fixed file failure without formatting configuration contents
        let error = match load_process_database_configuration(&environment, &files) {
            Ok(_) => panic!("missing password file unexpectedly loaded configuration"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            DatabaseProcessConfigurationError::Configuration(
                DatabaseConfigurationError::PasswordLoad(PASSWORD_FILE_READ_FAILURE.to_owned())
            )
        );
        assert_eq!(files.paths.borrow().as_slice(), ["/fixture/password"]);
    }

    /// Build the minimum complete configured environment without optional overrides.
    fn required_environment() -> Environment {
        Environment {
            failure: None,
            reads: RefCell::new(Vec::new()),
            values: BTreeMap::from([
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
                    "/fixture/password".to_owned(),
                ),
                (
                    DatabaseProcessEnvironmentKey::User.as_str(),
                    "weather_app".to_owned(),
                ),
            ]),
        }
    }

    /// Build one reusable mounted-password fixture with Node-compatible trailing newline.
    fn password_file() -> Files {
        Files {
            bytes: Some(b"fixture-password\n".to_vec()),
            paths: RefCell::new(Vec::new()),
        }
    }
}
