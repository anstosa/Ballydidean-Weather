use std::error::Error;
use std::fmt::{Display, Formatter};

const DEFAULT_APPLICATION_NAME: &str = "weather-database";
const DEFAULT_LOCK_TIMEOUT_MS: &str = "10000";
const DEFAULT_PORT: &str = "5432";
const DEFAULT_SSL: &str = "false";
const DEFAULT_STATEMENT_TIMEOUT_MS: &str = "30000";

/// admitted database environment values
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DatabaseConfigurationInput {
    pub application_name: Option<String>,
    pub database: Option<String>,
    pub host: Option<String>,
    pub lock_timeout_ms: Option<String>,
    pub password_file: Option<String>,
    pub port: Option<String>,
    pub ssl: Option<String>,
    pub statement_timeout_ms: Option<String>,
    pub user: Option<String>,
}

/// resolved database settings
#[derive(Clone, PartialEq, Eq)]
pub struct DatabaseConfiguration {
    pub application_name: String,
    pub database: String,
    pub host: String,
    pub lock_timeout_ms: u32,
    pub password: String,
    pub port: u16,
    pub ssl: bool,
    pub statement_timeout_ms: u32,
    pub user: String,
}

/// pg pool settings without a protocol dependency
#[derive(Clone, PartialEq, Eq)]
pub struct DatabasePoolConfiguration {
    pub application_name: String,
    pub database: String,
    pub host: String,
    pub max: u32,
    pub password: String,
    pub port: u16,
    pub ssl: bool,
    pub statement_timeout: u32,
    pub user: String,
}

/// resolve mounted secrets outside this crate
pub trait DatabasePasswordLoader {
    /// load one admitted password reference
    fn load_password(&self, reference: &str) -> Result<String, String>;
}

/// bounded configuration failure
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseConfigurationError {
    MissingEnvironment(&'static str),
    InvalidInteger {
        name: &'static str,
        minimum: u32,
        maximum: u32,
    },
    InvalidBoolean,
    InvalidPassword,
    PasswordLoad(String),
}

impl Display for DatabaseConfigurationError {
    /// render the stable source diagnostic
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the source-specific diagnostic
        match self {
            Self::MissingEnvironment(name) => write!(formatter, "{name} is required"),
            Self::InvalidInteger {
                name,
                minimum,
                maximum,
            } => write!(formatter, "{name} must be between {minimum} and {maximum}",),
            Self::InvalidBoolean => formatter.write_str("boolean setting must be true or false"),
            Self::InvalidPassword => {
                formatter.write_str("database password file must contain one non-empty line")
            }
            Self::PasswordLoad(detail) => formatter.write_str(detail),
        }
    }
}

impl Error for DatabaseConfigurationError {}

/// resolve one database configuration
pub fn load_database_configuration(
    input: DatabaseConfigurationInput,
    password_loader: &dyn DatabasePasswordLoader,
) -> Result<DatabaseConfiguration, DatabaseConfigurationError> {
    let password_reference =
        require_environment(input.password_file, "WEATHER_DATABASE_PASSWORD_FILE")?;
    let password = password_loader
        .load_password(&password_reference)
        .map_err(DatabaseConfigurationError::PasswordLoad)?;
    let password = password.trim_end_matches(['\r', '\n']).to_owned();

    // reject empty or multiline mounted secrets
    if password.is_empty() || password.contains(['\r', '\n']) {
        return Err(DatabaseConfigurationError::InvalidPassword);
    }

    Ok(DatabaseConfiguration {
        application_name: input
            .application_name
            .unwrap_or_else(|| DEFAULT_APPLICATION_NAME.to_owned()),
        database: require_environment(input.database, "WEATHER_DATABASE_NAME")?,
        host: require_environment(input.host, "WEATHER_DATABASE_HOST")?,
        lock_timeout_ms: parse_bounded_integer(
            input
                .lock_timeout_ms
                .as_deref()
                .unwrap_or(DEFAULT_LOCK_TIMEOUT_MS),
            "WEATHER_DATABASE_LOCK_TIMEOUT_MS",
            100,
            60_000,
        )?,
        password,
        port: parse_bounded_integer(
            input.port.as_deref().unwrap_or(DEFAULT_PORT),
            "WEATHER_DATABASE_PORT",
            1,
            65_535,
        )? as u16,
        ssl: parse_boolean(input.ssl.as_deref().unwrap_or(DEFAULT_SSL))?,
        statement_timeout_ms: parse_bounded_integer(
            input
                .statement_timeout_ms
                .as_deref()
                .unwrap_or(DEFAULT_STATEMENT_TIMEOUT_MS),
            "WEATHER_DATABASE_STATEMENT_TIMEOUT_MS",
            100,
            300_000,
        )?,
        user: require_environment(input.user, "WEATHER_DATABASE_USER")?,
    })
}

/// map settings to the future pg adapter
pub fn to_pool_configuration(configuration: &DatabaseConfiguration) -> DatabasePoolConfiguration {
    DatabasePoolConfiguration {
        application_name: configuration.application_name.clone(),
        database: configuration.database.clone(),
        host: configuration.host.clone(),
        max: 10,
        password: configuration.password.clone(),
        port: configuration.port,
        ssl: configuration.ssl,
        statement_timeout: configuration.statement_timeout_ms,
        user: configuration.user.clone(),
    }
}

/// require one configured environment value
fn require_environment(
    value: Option<String>,
    name: &'static str,
) -> Result<String, DatabaseConfigurationError> {
    // reject missing or blank settings
    if value.as_deref().is_none_or(|value| value.trim().is_empty()) {
        return Err(DatabaseConfigurationError::MissingEnvironment(name));
    }

    Ok(value.expect("present environment value"))
}

/// parse one bounded integer setting
fn parse_bounded_integer(
    value: &str,
    name: &'static str,
    minimum: u32,
    maximum: u32,
) -> Result<u32, DatabaseConfigurationError> {
    let parsed = parse_javascript_number(value).unwrap_or(f64::NAN);

    // reject noninteger or out-of-range settings
    if !parsed.is_finite()
        || parsed.fract() != 0.0
        || parsed < f64::from(minimum)
        || parsed > f64::from(maximum)
    {
        return Err(DatabaseConfigurationError::InvalidInteger {
            name,
            minimum,
            maximum,
        });
    }

    Ok(parsed as u32)
}

/// parse one node-compatible numeric string
fn parse_javascript_number(value: &str) -> Option<f64> {
    let trimmed = value.trim_matches(is_javascript_trim_character);

    // accept unsigned hexadecimal notation
    if let Some(digits) = trimmed.strip_prefix("0x").or(trimmed.strip_prefix("0X")) {
        return parse_unsigned_radix(digits, 16);
    }

    // accept unsigned binary notation
    if let Some(digits) = trimmed.strip_prefix("0b").or(trimmed.strip_prefix("0B")) {
        return parse_unsigned_radix(digits, 2);
    }

    // accept unsigned octal notation
    if let Some(digits) = trimmed.strip_prefix("0o").or(trimmed.strip_prefix("0O")) {
        return parse_unsigned_radix(digits, 8);
    }

    trimmed.parse::<f64>().ok()
}

/// parse unsigned prefixed digits
fn parse_unsigned_radix(digits: &str, radix: u32) -> Option<f64> {
    // reject signs after a node numeric prefix
    if digits.starts_with('+') || digits.starts_with('-') {
        return None;
    }

    u32::from_str_radix(digits, radix).ok().map(f64::from)
}

/// identify one ecmascript trim character
const fn is_javascript_trim_character(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// parse one strict boolean setting
fn parse_boolean(value: &str) -> Result<bool, DatabaseConfigurationError> {
    // accept only the two source literals
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(DatabaseConfigurationError::InvalidBoolean),
    }
}
