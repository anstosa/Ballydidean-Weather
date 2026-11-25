use weather_database::{
    DatabaseConfigurationError, DatabaseConfigurationInput, DatabasePasswordLoader,
    load_database_configuration, to_pool_configuration,
};

/// fixture password adapter
struct FixturePasswordLoader {
    reference: &'static str,
    value: &'static str,
}

impl DatabasePasswordLoader for FixturePasswordLoader {
    /// resolve one disposable fixture reference
    fn load_password(&self, reference: &str) -> Result<String, String> {
        // require the configured fixture reference
        if reference != self.reference {
            return Err("fixture password reference differs".to_owned());
        }

        Ok(self.value.to_owned())
    }
}

/// build one minimal admitted environment
fn input() -> DatabaseConfigurationInput {
    DatabaseConfigurationInput {
        application_name: None,
        database: Some("weather".to_owned()),
        host: Some("127.0.0.1".to_owned()),
        lock_timeout_ms: None,
        password_file: Some("fixture-password".to_owned()),
        port: None,
        ssl: None,
        statement_timeout_ms: None,
        user: Some("weather_owner".to_owned()),
    }
}

/// build one disposable password adapter
fn password(value: &'static str) -> FixturePasswordLoader {
    FixturePasswordLoader {
        reference: "fixture-password",
        value,
    }
}

#[test]
// preserve source defaults and pool role mapping
fn loads_defaults_and_pool_configuration() {
    let configuration = load_database_configuration(input(), &password("secret-value\n"))
        .expect("default configuration must load");
    let pool = to_pool_configuration(&configuration);

    assert_eq!(configuration.application_name, "weather-database");
    assert_eq!(configuration.lock_timeout_ms, 10_000);
    assert_eq!(configuration.port, 5_432);
    assert!(!configuration.ssl);
    assert_eq!(configuration.statement_timeout_ms, 30_000);
    assert_eq!(configuration.password, "secret-value");
    assert_eq!(pool.application_name, "weather-database");
    assert_eq!(pool.database, "weather");
    assert_eq!(pool.host, "127.0.0.1");
    assert_eq!(pool.max, 10);
    assert_eq!(pool.password, "secret-value");
    assert_eq!(pool.port, 5_432);
    assert!(!pool.ssl);
    assert_eq!(pool.statement_timeout, 30_000);
    assert_eq!(pool.user, "weather_owner");
}

#[test]
// preserve explicit bounds ssl and application identity
fn loads_explicit_configuration() {
    let mut values = input();
    values.application_name = Some("weather-api".to_owned());
    values.lock_timeout_ms = Some(" 4.321e3 ".to_owned());
    values.port = Some("65535".to_owned());
    values.ssl = Some("true".to_owned());
    values.statement_timeout_ms = Some("300000".to_owned());
    let configuration = load_database_configuration(values, &password("secret-value \r\n"))
        .expect("explicit configuration must load");

    assert_eq!(configuration.application_name, "weather-api");
    assert_eq!(configuration.lock_timeout_ms, 4_321);
    assert_eq!(configuration.port, 65_535);
    assert!(configuration.ssl);
    assert_eq!(configuration.statement_timeout_ms, 300_000);
    assert_eq!(configuration.password, "secret-value ");
}

#[test]
// preserve node unsigned radix and trim behavior
fn loads_prefixed_integer_configuration() {
    let mut values = input();
    values.lock_timeout_ms = Some("\u{feff}0B1111101000\u{3000}".to_owned());
    values.port = Some(" 0x1538 ".to_owned());
    values.statement_timeout_ms = Some("0o72460".to_owned());

    let configuration = load_database_configuration(values, &password("secret"))
        .expect("prefixed integers must load");

    assert_eq!(configuration.lock_timeout_ms, 1_000);
    assert_eq!(configuration.port, 5_432);
    assert_eq!(configuration.statement_timeout_ms, 30_000);
}

#[test]
// reject missing required environment values
fn rejects_missing_environment_values() {
    let mut missing_database = input();
    missing_database.database = None;
    let error = load_database_configuration(missing_database, &password("secret"))
        .err()
        .expect("missing database must fail");
    assert_eq!(
        error,
        DatabaseConfigurationError::MissingEnvironment("WEATHER_DATABASE_NAME"),
    );
    assert_eq!(error.to_string(), "WEATHER_DATABASE_NAME is required");

    let mut blank_user = input();
    blank_user.user = Some("  ".to_owned());
    let error = load_database_configuration(blank_user, &password("secret"))
        .err()
        .expect("blank user must fail");
    assert_eq!(error.to_string(), "WEATHER_DATABASE_USER is required");

    let mut missing_password = input();
    missing_password.password_file = None;
    let error = load_database_configuration(missing_password, &password("secret"))
        .err()
        .expect("missing password reference must fail");
    assert_eq!(
        error.to_string(),
        "WEATHER_DATABASE_PASSWORD_FILE is required",
    );
}

#[test]
// enforce every source-owned numeric range
fn rejects_out_of_range_integer_settings() {
    let cases = [
        (
            "lock",
            "99",
            "WEATHER_DATABASE_LOCK_TIMEOUT_MS must be between 100 and 60000",
        ),
        (
            "port",
            "65536",
            "WEATHER_DATABASE_PORT must be between 1 and 65535",
        ),
        (
            "statement",
            "300001",
            "WEATHER_DATABASE_STATEMENT_TIMEOUT_MS must be between 100 and 300000",
        ),
        (
            "port",
            "1.5",
            "WEATHER_DATABASE_PORT must be between 1 and 65535",
        ),
        (
            "port",
            "+0x1538",
            "WEATHER_DATABASE_PORT must be between 1 and 65535",
        ),
        (
            "port",
            "0x+1538",
            "WEATHER_DATABASE_PORT must be between 1 and 65535",
        ),
        (
            "port",
            "-0b1",
            "WEATHER_DATABASE_PORT must be between 1 and 65535",
        ),
        (
            "port",
            "0o10ms",
            "WEATHER_DATABASE_PORT must be between 1 and 65535",
        ),
        (
            "port",
            "\u{0085}5432",
            "WEATHER_DATABASE_PORT must be between 1 and 65535",
        ),
    ];

    // exercise each bounded integer field
    for (field, value, expected) in cases {
        let mut values = input();

        // assign the selected source field
        match field {
            "lock" => values.lock_timeout_ms = Some(value.to_owned()),
            "port" => values.port = Some(value.to_owned()),
            "statement" => values.statement_timeout_ms = Some(value.to_owned()),
            _ => unreachable!("closed fixture field"),
        }
        let error = load_database_configuration(values, &password("secret"))
            .err()
            .expect("out-of-range integer must fail");
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
// enforce strict boolean and password values
fn rejects_invalid_boolean_and_password() {
    let mut invalid_ssl = input();
    invalid_ssl.ssl = Some("TRUE".to_owned());
    let error = load_database_configuration(invalid_ssl, &password("secret"))
        .err()
        .expect("nonliteral ssl must fail");
    assert_eq!(error.to_string(), "boolean setting must be true or false");

    let error = load_database_configuration(input(), &password("first\nsecond"))
        .err()
        .expect("multiline password must fail");
    assert_eq!(
        error.to_string(),
        "database password file must contain one non-empty line",
    );
}
