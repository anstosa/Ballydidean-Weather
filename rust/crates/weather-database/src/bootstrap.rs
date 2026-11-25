//! Shared generated-contract binds and transaction lifecycle for bootstrap repositories.

use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{JsonValue, Nullable, SourceCapability};

use crate::SerializedJson;

pub const ACTIVE_SITE_SQL: &str = "SELECT id FROM sites WHERE slug = $1 AND active";
pub const SITE_SQL: &str = include_str!("../fixtures/bootstrap-site.sql");
pub const PROVIDER_SQL: &str = include_str!("../fixtures/bootstrap-provider.sql");
pub const STATION_SQL: &str = include_str!("../fixtures/bootstrap-station.sql");
pub const SOURCE_SQL: &str = include_str!("../fixtures/bootstrap-source.sql");
pub const TEMPEST_STATION_SQL: &str = include_str!("../fixtures/bootstrap-tempest-station.sql");
pub const ECOWITT_STATION_SQL: &str = include_str!("../fixtures/bootstrap-ecowitt-station.sql");
pub const PUBLIC_STATION_SQL: &str = include_str!("../fixtures/bootstrap-public-station.sql");
pub const PHYSICAL_SOURCE_SQL: &str = include_str!("../fixtures/bootstrap-physical-source.sql");

/// Borrow one exact typed driver bind for a retained SQL statement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BootstrapBind<'a> {
    Text(&'a str),
    BigintText(&'a str),
    Json(&'a str),
    Unsigned(u64),
    Float(f64),
    Bool(bool),
    NullText,
    NullInteger,
}

/// The source uses the first returned id and the explicit row count separately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapRows {
    pub row_count: Option<u64>,
    pub first_id: Option<String>,
}

/// Driver session for one transaction on the same process-owned bootstrap pool.
pub trait BootstrapSession {
    type Error;

    /// Start the source transaction before reading or updating a catalog.
    fn begin(&mut self) -> Result<(), Self::Error>;

    /// Run a retained statement with typed prepared binds.
    fn query(
        &mut self,
        sql: &'static str,
        binds: &[BootstrapBind<'_>],
    ) -> Result<BootstrapRows, Self::Error>;

    /// Commit the entire catalog.
    fn commit(&mut self) -> Result<(), Self::Error>;

    /// Discard the entire catalog after any transaction failure.
    fn rollback(&mut self) -> Result<(), Self::Error>;

    /// Return this session to the owning pool after success or failure.
    fn release(&mut self);
}

/// Borrow sessions without creating a second process pool.
pub trait BootstrapPool {
    type Session: BootstrapSession;

    /// Acquire one session for this catalog.
    fn checkout(&mut self) -> Result<Self::Session, BootstrapDriverError<Self>>;
}

/// The same driver failure type is used for checkout and later session operations.
pub type BootstrapDriverError<Pool> = <<Pool as BootstrapPool>::Session as BootstrapSession>::Error;

/// One approved JSON serializer for immutable material and capability columns.
pub trait BootstrapJsonEncoder {
    type Error;

    /// Serialize the existing generic JSON value without changing object order.
    fn value(&mut self, value: &JsonValue) -> Result<SerializedJson, Self::Error>;

    /// Serialize an already projected ordered material object as a JSON object.
    fn object(&mut self, entries: &[(Vec<u16>, JsonValue)]) -> Result<SerializedJson, Self::Error>;

    /// Serialize capability values in the source order.
    fn capabilities(&mut self, values: &[SourceCapability]) -> Result<SerializedJson, Self::Error>;
}

/// Bootstrap operation failure shared across the retained catalog families.
#[derive(Debug, PartialEq, Eq)]
pub enum BootstrapError<DriverError, EncodeError> {
    Driver(DriverError),
    Encode(EncodeError),
    MissingRow(String),
    MissingProvider(String),
    SourceChanged {
        family_prefix: &'static str,
        key: String,
    },
}

impl<DriverError: Display, EncodeError: Display> Display
    for BootstrapError<DriverError, EncodeError>
{
    /// Retain the owning typed category and exact ordinary source diagnostics.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Driver(error) => Display::fmt(error, formatter),
            Self::Encode(error) => Display::fmt(error, formatter),
            Self::MissingRow(label) => write!(formatter, "{label} returned no row"),
            Self::MissingProvider(key) => {
                write!(formatter, "public-station provider {key} is missing")
            }
            Self::SourceChanged { family_prefix, key } => write!(
                formatter,
                "{family_prefix}source {key} changed material configuration; create a new source key"
            ),
        }
    }
}

impl<DriverError, EncodeError> Error for BootstrapError<DriverError, EncodeError>
where
    DriverError: Error + 'static,
    EncodeError: Error + 'static,
{
    /// Preserve only the originating driver or serializer cause for the process owner.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Driver(error) => Some(error),
            Self::Encode(error) => Some(error),
            _ => None,
        }
    }
}

/// Run one real catalog command in the source transaction and always release its session.
pub(crate) fn with_bootstrap<Pool, J, R>(
    pool: &mut Pool,
    operation: impl FnOnce(
        &mut Pool::Session,
    ) -> Result<R, BootstrapError<BootstrapDriverError<Pool>, J>>,
) -> Result<R, BootstrapError<BootstrapDriverError<Pool>, J>>
where
    Pool: BootstrapPool,
{
    let mut session = pool.checkout().map_err(BootstrapError::Driver)?;
    let result = (|| {
        session.begin().map_err(BootstrapError::Driver)?;
        let outcome = operation(&mut session).and_then(|value| {
            session.commit().map_err(BootstrapError::Driver)?;
            Ok(value)
        });
        // keep rollback failure precedence from the retained Node transaction helper
        if outcome.is_err() {
            session.rollback().map_err(BootstrapError::Driver)?;
        }
        outcome
    })();
    session.release();
    result
}

/// Read one optional first id without imposing a row-count condition on ordinary upserts.
pub(crate) fn query_id<Session: BootstrapSession, J>(
    session: &mut Session,
    sql: &'static str,
    binds: &[BootstrapBind<'_>],
    label: impl FnOnce() -> String,
) -> Result<String, BootstrapError<Session::Error, J>> {
    session
        .query(sql, binds)
        .map_err(BootstrapError::Driver)?
        .first_id
        .ok_or_else(|| BootstrapError::MissingRow(label()))
}

/// Check a source upsert's row-count witness before returning its immutable id.
pub(crate) fn query_source_id<Session: BootstrapSession, J>(
    session: &mut Session,
    sql: &'static str,
    binds: &[BootstrapBind<'_>],
    family_prefix: &'static str,
    key: &str,
) -> Result<String, BootstrapError<Session::Error, J>> {
    let result = session.query(sql, binds).map_err(BootstrapError::Driver)?;
    // old material can cause a zero-row update rather than an explicit driver failure
    match (result.row_count, result.first_id) {
        (Some(1), Some(id)) => Ok(id),
        _ => Err(BootstrapError::SourceChanged {
            family_prefix,
            key: key.to_owned(),
        }),
    }
}

/// Bind nullable text using an actual SQL null rather than a string sentinel.
pub(crate) fn nullable_text(value: &Nullable<String>) -> BootstrapBind<'_> {
    match value {
        Nullable::Value(text) => BootstrapBind::Text(text),
        Nullable::Null => BootstrapBind::NullText,
    }
}

/// Resolve the generated wire spelling of an already admitted enum value.
pub(crate) fn wire_enum<E: PartialEq>(value: E, wire_values: &[(&'static str, E)]) -> &'static str {
    wire_values
        .iter()
        .find(|(_, candidate)| *candidate == value)
        .map(|(wire, _)| *wire)
        .expect("all generated variants carry their original wire spelling")
}

/// Preserve record order and update at the original slot on repeated source keys.
pub(crate) fn put_record(record: &mut Vec<(String, String)>, key: &str, id: String) {
    // existing entries keep their source insertion slot when overwritten
    if let Some((_, existing)) = record.iter_mut().find(|(name, _)| name == key) {
        *existing = id;
        return;
    }
    record.push((key.to_owned(), id));
}
