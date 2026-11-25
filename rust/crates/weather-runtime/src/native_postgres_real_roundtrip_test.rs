//! Opt-in real PostgreSQL 16 transport proof for the native synchronous pool.

use std::env;
use std::error::Error;
use std::net::IpAddr;

use serde_json::value::RawValue;
use tokio_postgres::types::{FromSql, Json, Type};
use uuid::Uuid;
use weather_database::DatabasePoolConfiguration;

use crate::{
    NativePostgresParameter as Parameter, NativePostgresPool, NativePostgresType as ParameterType,
};

const APPLICATION_NAME: &str = "weather-native-postgres-real-roundtrip";
const BIGINT_VALUE: i64 = 9_007_199_254_740_991;
const DATE_VALUE: &str = "2026-11-24";
const JSON_VALUE: &str = "{\"z\":1,\"a\":[true,null,\"x\"]}";
const UUID_VALUE: &str = "01234567-89ab-4cde-8123-0123456789ab";

#[derive(Debug, Eq, PartialEq)]
struct PostgresDateDays(i32);

impl<'a> FromSql<'a> for PostgresDateDays {
    /// decode PostgreSQL's four-byte DATE wire value.
    fn from_sql(
        parameter_type: &Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn Error + Sync + Send>> {
        // require the exact DATE domain and binary width
        if *parameter_type != Type::DATE || raw.len() != 4 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unexpected PostgreSQL DATE wire value",
            )
            .into());
        }
        Ok(Self(i32::from_be_bytes(raw.try_into().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid DATE width")
        })?)))
    }

    /// accept only PostgreSQL DATE rows.
    fn accepts(parameter_type: &Type) -> bool {
        *parameter_type == Type::DATE
    }
}

/// exercise real typed binds, rows, readiness, and cleanup.
#[test]
#[ignore = "requires an explicitly provisioned loopback PostgreSQL 16 fixture"]
fn native_pool_roundtrips_typed_values_against_postgres_16() {
    let configuration = fixture_configuration();
    let expected_database = configuration.database.clone();
    let expected_user = configuration.user.clone();
    let pool = NativePostgresPool::new(configuration).expect("construct native PostgreSQL pool");
    assert_eq!(pool.maximum_connections(), 2);

    let mut session = pool.checkout().expect("checkout real PostgreSQL session");
    session.begin().expect("begin real PostgreSQL transaction");
    assert_eq!(
        session
            .execute("SET TRANSACTION READ ONLY", &[])
            .expect("make fixture transaction read only"),
        0
    );

    let version = session
        .query(
            "SELECT current_setting('server_version_num') AS server_version_num",
            &[],
        )
        .expect("run exact predecessor PostgreSQL readiness query");
    assert_eq!(version.len(), 1);
    let version = version[0]
        .try_get::<_, String>("server_version_num")
        .unwrap()
        .parse::<u32>()
        .unwrap();
    assert!((160_000..170_000).contains(&version));

    let readiness = session
        .query(
            r#"SELECT
  current_database() AS database_name,
  current_user AS user_name,
  current_setting('application_name') AS application_name,
  (EXTRACT(EPOCH FROM current_setting('statement_timeout')::interval) * 1000)::bigint AS statement_timeout_ms,
  current_setting('transaction_read_only') AS transaction_read_only,
  pg_is_in_recovery() AS in_recovery"#,
            &[],
        )
        .expect("query read-only PostgreSQL readiness");
    assert_eq!(readiness.len(), 1);
    let readiness = &readiness[0];
    assert_eq!(
        readiness.try_get::<_, String>("database_name").unwrap(),
        expected_database
    );
    assert_eq!(
        readiness.try_get::<_, String>("user_name").unwrap(),
        expected_user
    );
    assert_eq!(
        readiness.try_get::<_, String>("application_name").unwrap(),
        APPLICATION_NAME
    );
    assert_eq!(
        readiness.try_get::<_, i64>("statement_timeout_ms").unwrap(),
        5_000
    );
    assert_eq!(
        readiness
            .try_get::<_, String>("transaction_read_only")
            .unwrap(),
        "on"
    );
    assert!(!readiness.try_get::<_, bool>("in_recovery").unwrap());

    let rows = session
        .query(
            r#"SELECT
  $1::date AS date_wire,
  $1::date::text AS date_value,
  ($1::date - DATE '2000-01-01')::integer AS date_days,
  $2::uuid AS uuid_value,
  $3::bigint AS bigint_value,
  $4::jsonb AS json_value,
  $5::date AS null_date,
  $6::uuid AS null_uuid,
  $7::bigint AS null_bigint,
  $8::jsonb AS null_json"#,
            &[
                Parameter::Date(DATE_VALUE.to_owned()),
                Parameter::Uuid(UUID_VALUE.to_owned()),
                Parameter::I64(BIGINT_VALUE),
                Parameter::Json(JSON_VALUE.to_owned()),
                Parameter::Null(ParameterType::Date),
                Parameter::Null(ParameterType::Uuid),
                Parameter::Null(ParameterType::I64),
                Parameter::Null(ParameterType::Json),
            ],
        )
        .expect("roundtrip exact typed parameters");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    let date_wire = row
        .try_get::<_, PostgresDateDays>("date_wire")
        .expect("decode binary DATE row");
    assert_eq!(date_wire.0, row.try_get::<_, i32>("date_days").unwrap());
    assert_eq!(row.try_get::<_, String>("date_value").unwrap(), DATE_VALUE);
    assert_eq!(
        row.try_get::<_, Uuid>("uuid_value").unwrap(),
        Uuid::parse_str(UUID_VALUE).unwrap()
    );
    assert_eq!(row.try_get::<_, i64>("bigint_value").unwrap(), BIGINT_VALUE);
    let json = row
        .try_get::<_, Json<Box<RawValue>>>("json_value")
        .expect("decode binary JSONB row");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(json.0.get()).unwrap(),
        serde_json::from_str::<serde_json::Value>(JSON_VALUE).unwrap()
    );
    assert_eq!(
        row.try_get::<_, Option<PostgresDateDays>>("null_date")
            .unwrap(),
        None
    );
    assert_eq!(row.try_get::<_, Option<Uuid>>("null_uuid").unwrap(), None);
    assert_eq!(row.try_get::<_, Option<i64>>("null_bigint").unwrap(), None);
    assert!(
        row.try_get::<_, Option<Json<Box<RawValue>>>>("null_json")
            .unwrap()
            .is_none()
    );

    session
        .rollback()
        .expect("rollback read-only fixture transaction");
    session.release().expect("release real PostgreSQL session");
    pool.close().expect("close real PostgreSQL pool");
}

/// read explicit loopback fixture settings.
fn fixture_configuration() -> DatabasePoolConfiguration {
    let host = fixture_environment("WEATHER_NATIVE_POSTGRES_TEST_HOST");
    let address = host
        .parse::<IpAddr>()
        .expect("WEATHER_NATIVE_POSTGRES_TEST_HOST must be a literal IP address");
    // refuse any non-loopback database target
    if !address.is_loopback() {
        panic!("WEATHER_NATIVE_POSTGRES_TEST_HOST must be loopback");
    }
    let port = fixture_environment("WEATHER_NATIVE_POSTGRES_TEST_PORT")
        .parse::<u16>()
        .expect("WEATHER_NATIVE_POSTGRES_TEST_PORT must be a nonzero u16");
    // refuse the invalid TCP port before pool construction
    if port == 0 {
        panic!("WEATHER_NATIVE_POSTGRES_TEST_PORT must be nonzero");
    }
    DatabasePoolConfiguration {
        application_name: APPLICATION_NAME.to_owned(),
        database: fixture_environment("WEATHER_NATIVE_POSTGRES_TEST_DATABASE"),
        host,
        max: 2,
        password: fixture_environment("WEATHER_NATIVE_POSTGRES_TEST_PASSWORD"),
        port,
        ssl: false,
        statement_timeout: 5_000,
        user: fixture_environment("WEATHER_NATIVE_POSTGRES_TEST_USER"),
    }
}

/// read one required nonempty fixture value.
fn fixture_environment(name: &'static str) -> String {
    let value =
        env::var(name).unwrap_or_else(|_| panic!("{name} is required for the ignored test"));
    // reject empty fixture identities and credentials
    if value.is_empty() {
        panic!("{name} must be nonempty for the ignored test");
    }
    value
}
