//! Process-shared PostgreSQL transport for synchronous retained runtime ports.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc as sync_mpsc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::BytesMut;
use jiff::civil::Date;
use serde_json::value::RawValue;
use tokio::runtime::Builder;
use tokio::sync::{Mutex as AsyncMutex, mpsc};
use tokio_postgres::types::{Format, IsNull, Json, ToSql, Type};
use tokio_postgres::{Client, NoTls, Row};
use tokio_postgres_rustls::MakeRustlsConnect;
use weather_database::DatabasePoolConfiguration;
use weather_edge::ArchiveWirePrimitives;

use crate::NativeWirePrimitives;
use crate::native_outbound_http::native_tls_client_config;

const MAXIMUM_POOL_CONNECTIONS: u32 = 10;
const DRIVER_COMMAND_CAPACITY: usize = 64;
const MINIMUM_STATEMENT_TIMEOUT_MS: u32 = 100;
const MAXIMUM_STATEMENT_TIMEOUT_MS: u32 = 300_000;

static DRIVER: OnceLock<Arc<NativePostgresDriver>> = OnceLock::new();
static DRIVER_START: Mutex<()> = Mutex::new(());

/// one owned PostgreSQL parameter and its explicit wire type
#[derive(Clone, Debug, PartialEq)]
pub enum NativePostgresParameter {
    Bool(bool),
    I16(i16),
    I32(i32),
    I64(i64),
    U64(u64),
    F64(f64),
    Text(String),
    Uuid(String),
    Timestamp(String),
    Date(String),
    Json(String),
    Bytes(Vec<u8>),
    TextArray(Vec<String>),
    I32Array(Vec<i32>),
    I64Array(Vec<i64>),
    Null(NativePostgresType),
}

/// one explicit PostgreSQL type for a null parameter
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativePostgresType {
    Bool,
    I16,
    I32,
    I64,
    F64,
    Text,
    Uuid,
    Timestamp,
    Date,
    Json,
    Bytes,
    TextArray,
    I32Array,
    I64Array,
}

/// bounded native pool, transport, transaction, or decode failure
#[derive(Debug)]
pub enum NativePostgresError {
    InvalidConfiguration(&'static str),
    InvalidParameter(&'static str),
    TlsConfiguration,
    RuntimeInitialization(String),
    RuntimeUnavailable,
    RuntimeSaturated,
    PoolClosed,
    SessionReleased,
    TransactionAlreadyActive,
    TransactionNotActive,
    ParameterOutOfRange,
    Database(tokio_postgres::Error),
    Decode { column: String, detail: String },
}

impl NativePostgresError {
    /// construct one explicit row-decoding failure
    pub fn decode(column: &str, detail: impl Display) -> Self {
        Self::Decode {
            column: column.to_owned(),
            detail: detail.to_string(),
        }
    }

    /// report whether this failure occurred after a successful query
    pub const fn is_post_query_decode(&self) -> bool {
        matches!(self, Self::Decode { .. })
    }

    /// report whether the driver closed this connection
    fn connection_closed(&self) -> bool {
        matches!(self, Self::Database(error) if error.is_closed())
    }
}

impl Display for NativePostgresError {
    /// render one bounded transport diagnostic without configuration secrets
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the owning failure category
        match self {
            Self::InvalidConfiguration(name) => {
                write!(formatter, "invalid PostgreSQL pool setting: {name}")
            }
            Self::InvalidParameter(name) => {
                write!(formatter, "invalid PostgreSQL parameter: {name}")
            }
            Self::TlsConfiguration => formatter.write_str("PostgreSQL TLS configuration failed"),
            Self::RuntimeInitialization(detail) => {
                write!(
                    formatter,
                    "PostgreSQL driver runtime initialization failed: {detail}"
                )
            }
            Self::RuntimeUnavailable => formatter.write_str("PostgreSQL driver runtime stopped"),
            Self::RuntimeSaturated => formatter.write_str("PostgreSQL driver runtime is saturated"),
            Self::PoolClosed => formatter.write_str("PostgreSQL pool is closed"),
            Self::SessionReleased => formatter.write_str("PostgreSQL session was released"),
            Self::TransactionAlreadyActive => {
                formatter.write_str("PostgreSQL transaction is already active")
            }
            Self::TransactionNotActive => {
                formatter.write_str("PostgreSQL transaction is not active")
            }
            Self::ParameterOutOfRange => {
                formatter.write_str("PostgreSQL parameter is outside its admitted range")
            }
            Self::Database(error) => Display::fmt(error, formatter),
            Self::Decode { column, detail } => {
                write!(
                    formatter,
                    "PostgreSQL column {column} could not be decoded: {detail}"
                )
            }
        }
    }
}

impl Error for NativePostgresError {
    /// retain only an originating driver error as a source
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // expose the typed driver cause without configuration material
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<tokio_postgres::Error> for NativePostgresError {
    /// retain one typed tokio-postgres failure
    fn from(value: tokio_postgres::Error) -> Self {
        Self::Database(value)
    }
}

/// one lazy bounded pool sharing the process driver runtime
#[derive(Clone)]
pub struct NativePostgresPool {
    shared: Arc<PoolShared>,
}

impl NativePostgresPool {
    /// validate and retain one lazy process pool
    pub fn new(configuration: DatabasePoolConfiguration) -> Result<Self, NativePostgresError> {
        let maximum = validate_pool_configuration(&configuration)?;
        let connect = NativePostgresConnect::new(configuration)?;
        let driver = process_driver()?;
        Ok(Self {
            shared: Arc::new(PoolShared {
                available: Condvar::new(),
                connect,
                driver,
                maximum,
                state: Mutex::new(PoolState::default()),
            }),
        })
    }

    /// acquire one checked session without exceeding the configured maximum
    pub fn checkout(&self) -> Result<NativePostgresSession, NativePostgresError> {
        loop {
            let admission = self.shared.admit()?;
            // reuse open driver sessions without an extra query on every checkout
            if let PoolAdmission::Existing(connection_id) = admission {
                // discard closed idle connections before retrying admission
                if self.shared.driver.check(connection_id).is_err() {
                    self.shared.discard_reserved(connection_id);
                    continue;
                }
                return Ok(NativePostgresSession::new(
                    Arc::clone(&self.shared),
                    connection_id,
                ));
            }

            let result = self.shared.driver.connect(self.shared.connect.clone());
            // release a failed reservation for another waiter
            let connection_id = match result {
                Ok(connection_id) => connection_id,
                Err(error) => {
                    self.shared.cancel_reservation();
                    return Err(error);
                }
            };
            return Ok(NativePostgresSession::new(
                Arc::clone(&self.shared),
                connection_id,
            ));
        }
    }

    /// stop new checkout and close every currently idle connection
    pub fn close(&self) -> Result<(), NativePostgresError> {
        self.shared.close()
    }

    /// report the admitted maximum without exposing connection settings
    pub fn maximum_connections(&self) -> usize {
        self.shared.maximum
    }
}

/// one checked-out connection with explicit transaction state
pub struct NativePostgresSession {
    broken: bool,
    connection_id: Option<u64>,
    in_transaction: bool,
    pool: Arc<PoolShared>,
}

impl NativePostgresSession {
    /// retain one admitted connection until explicit release or drop
    fn new(pool: Arc<PoolShared>, connection_id: u64) -> Self {
        Self {
            broken: false,
            connection_id: Some(connection_id),
            in_transaction: false,
            pool,
        }
    }

    /// execute one typed query on this retained connection
    pub fn query(
        &mut self,
        sql: &str,
        parameters: &[NativePostgresParameter],
    ) -> Result<Vec<Row>, NativePostgresError> {
        let connection_id = self.connection_id()?;
        let parameters = prepare_parameters(parameters)?;
        let result = self
            .pool
            .driver
            .query(connection_id, sql.to_owned(), parameters);
        self.observe(&result);
        result
    }

    /// execute one typed command on this retained connection
    pub fn execute(
        &mut self,
        sql: &str,
        parameters: &[NativePostgresParameter],
    ) -> Result<u64, NativePostgresError> {
        let connection_id = self.connection_id()?;
        let parameters = prepare_parameters(parameters)?;
        let result = self
            .pool
            .driver
            .execute(connection_id, sql.to_owned(), parameters);
        self.observe(&result);
        result
    }

    /// execute one fixed multi-statement command without parameter interpolation
    pub fn batch_execute(&mut self, sql: &str) -> Result<(), NativePostgresError> {
        let connection_id = self.connection_id()?;
        let result = self
            .pool
            .driver
            .batch_execute(connection_id, sql.to_owned());
        self.observe(&result);
        result
    }

    /// begin one explicit transaction on this retained connection
    pub fn begin(&mut self) -> Result<(), NativePostgresError> {
        // reject nested transactions rather than guessing savepoint semantics
        if self.in_transaction {
            return Err(NativePostgresError::TransactionAlreadyActive);
        }
        self.batch_execute("BEGIN")?;
        self.in_transaction = true;
        Ok(())
    }

    /// commit the active transaction
    pub fn commit(&mut self) -> Result<(), NativePostgresError> {
        // require the exact transaction lifecycle
        if !self.in_transaction {
            return Err(NativePostgresError::TransactionNotActive);
        }
        let result = self.batch_execute("COMMIT");
        // leave failed commits eligible for the caller's ordinary rollback path
        if result.is_ok() {
            self.in_transaction = false;
        }
        result
    }

    /// roll back the active transaction
    pub fn rollback(&mut self) -> Result<(), NativePostgresError> {
        // require the exact transaction lifecycle
        if !self.in_transaction {
            return Err(NativePostgresError::TransactionNotActive);
        }
        let result = self.batch_execute("ROLLBACK");
        // discard any session whose rollback could not be confirmed
        if result.is_ok() {
            self.in_transaction = false;
        } else {
            self.broken = true;
        }
        result
    }

    /// return this connection to the idle pool
    pub fn release(mut self) -> Result<(), NativePostgresError> {
        self.release_inner()
    }

    /// discard this connection instead of recycling it
    pub fn discard(mut self) -> Result<(), NativePostgresError> {
        self.broken = true;
        self.release_inner()
    }

    /// require one still-retained connection identity
    fn connection_id(&self) -> Result<u64, NativePostgresError> {
        self.connection_id
            .ok_or(NativePostgresError::SessionReleased)
    }

    /// retain closed-connection evidence for release
    fn observe<T>(&mut self, result: &Result<T, NativePostgresError>) {
        // discard only protocol closure rather than ordinary SQL errors
        if result
            .as_ref()
            .is_err_and(|error| error.connection_closed())
        {
            self.broken = true;
        }
    }

    /// roll back unfinished work before recycling this connection
    fn release_inner(&mut self) -> Result<(), NativePostgresError> {
        let Some(connection_id) = self.connection_id.take() else {
            return Ok(());
        };
        let rollback = if self.in_transaction {
            self.in_transaction = false;
            self.pool
                .driver
                .batch_execute(connection_id, "ROLLBACK".to_owned())
        } else {
            Ok(())
        };
        // discard any connection whose cleanup state cannot be proven
        if rollback.is_err() {
            self.broken = true;
        }
        let recycled = self.pool.recycle(connection_id, !self.broken);
        rollback.and(recycled)
    }
}

impl Drop for NativePostgresSession {
    /// recycle or discard the retained connection on every exit path
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

#[derive(Default)]
struct PoolState {
    closed: bool,
    idle: Vec<u64>,
    total: usize,
}

enum PoolAdmission {
    Existing(u64),
    Reserved,
}

struct PoolShared {
    available: Condvar,
    connect: NativePostgresConnect,
    driver: Arc<NativePostgresDriver>,
    maximum: usize,
    state: Mutex<PoolState>,
}

impl PoolShared {
    /// wait for one idle connection or reserve one lazy connection slot
    fn admit(&self) -> Result<PoolAdmission, NativePostgresError> {
        let mut state = self.lock_state()?;
        loop {
            // stop waiters after explicit pool closure
            if state.closed {
                return Err(NativePostgresError::PoolClosed);
            }
            // prefer an already-open connection before growing the pool
            if let Some(connection_id) = state.idle.pop() {
                return Ok(PoolAdmission::Existing(connection_id));
            }
            // reserve before connecting so concurrent callers cannot exceed the maximum
            if state.total < self.maximum {
                state.total += 1;
                return Ok(PoolAdmission::Reserved);
            }
            state = self
                .available
                .wait(state)
                .map_err(|_| NativePostgresError::RuntimeUnavailable)?;
        }
    }

    /// release one failed lazy-connect reservation
    fn cancel_reservation(&self) {
        // retain capacity accounting even if another thread poisoned state
        if let Ok(mut state) = self.state.lock() {
            state.total = state.total.saturating_sub(1);
            self.available.notify_all();
        }
    }

    /// remove one failed idle connection from capacity after driver retirement
    fn discard_reserved(&self, connection_id: u64) {
        let _ = self.driver.close(connection_id);
        self.cancel_reservation();
    }

    /// recycle one proven connection or discard it permanently
    fn recycle(&self, connection_id: u64, reusable: bool) -> Result<(), NativePostgresError> {
        // return healthy connections only while the retained pool accepts new work
        if let Ok(mut state) = self.state.lock()
            && reusable
            && !state.closed
        {
            state.idle.push(connection_id);
            self.available.notify_one();
            return Ok(());
        }
        // retain the capacity reservation until actual driver retirement completes
        let result = self.driver.close(connection_id);
        self.cancel_reservation();
        result
    }

    /// reject new admission and await retirement of every checked-out or idle connection
    fn close(&self) -> Result<(), NativePostgresError> {
        let idle = {
            let mut state = self.lock_state()?;
            // close admission before draining either idle or in-flight connections
            state.closed = true;
            let idle = std::mem::take(&mut state.idle);
            self.available.notify_all();
            idle
        };
        let mut first_error = None;
        // close every idle driver handle without holding the pool lock
        for connection_id in idle {
            let result = self.driver.close(connection_id);
            self.cancel_reservation();
            // retain the first failure while attempting every close
            if let Err(error) = result
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        let mut state = self.lock_state()?;
        // match Pool.end by waiting for callers to return all admitted leases
        while state.total != 0 {
            state = self
                .available
                .wait(state)
                .map_err(|_| NativePostgresError::RuntimeUnavailable)?;
        }
        first_error.map_or(Ok(()), Err)
    }

    /// lock the bounded pool state
    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, PoolState>, NativePostgresError> {
        self.state
            .lock()
            .map_err(|_| NativePostgresError::RuntimeUnavailable)
    }
}

impl Drop for PoolShared {
    /// close every retained idle connection when the final owner exits
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[derive(Clone)]
struct NativePostgresConnect {
    configuration: tokio_postgres::Config,
    tls: Option<rustls::ClientConfig>,
}

impl NativePostgresConnect {
    /// map every admitted pool setting to tokio-postgres
    fn new(configuration: DatabasePoolConfiguration) -> Result<Self, NativePostgresError> {
        let mut driver = tokio_postgres::Config::new();
        driver.application_name(&configuration.application_name);
        driver.dbname(&configuration.database);
        driver.host(&configuration.host);
        driver.password(configuration.password);
        driver.port(configuration.port);
        driver.user(&configuration.user);
        driver.options(format!(
            "-c statement_timeout={}",
            configuration.statement_timeout
        ));
        let tls = if configuration.ssl {
            driver.ssl_mode(tokio_postgres::config::SslMode::Require);
            Some(native_tls_client_config().map_err(|_| NativePostgresError::TlsConfiguration)?)
        } else {
            driver.ssl_mode(tokio_postgres::config::SslMode::Disable);
            None
        };
        Ok(Self {
            configuration: driver,
            tls,
        })
    }

    /// open one lazy client and spawn its connection driver
    async fn connect(self) -> Result<Client, NativePostgresError> {
        // select only the explicitly configured TLS policy
        if let Some(tls) = self.tls {
            let connector = MakeRustlsConnect::new(tls);
            let (client, connection) = self.configuration.connect(connector).await?;
            // keep the protocol connection active while its client is idle
            tokio::spawn(async move {
                let _ = connection.await;
            });
            return Ok(client);
        }
        let (client, connection) = self.configuration.connect(NoTls).await?;
        // keep the protocol connection active while its client is idle
        tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(client)
    }
}

struct NativePostgresDriver {
    commands: mpsc::Sender<DriverCommand>,
}

impl NativePostgresDriver {
    /// start the sole process driver thread and current-thread Tokio runtime
    fn start() -> Result<Self, NativePostgresError> {
        let (ready_sender, ready_receiver) = sync_mpsc::sync_channel(1);
        thread::Builder::new()
            .name("weather-postgres-driver".to_owned())
            .spawn(move || {
                let runtime = Builder::new_current_thread().enable_all().build();
                let runtime = match runtime {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready_sender.send(Err(error.to_string()));
                        return;
                    }
                };
                let (commands, receiver) = mpsc::channel(DRIVER_COMMAND_CAPACITY);
                // hand the bounded sender to synchronous process owners
                if ready_sender.send(Ok(commands)).is_err() {
                    return;
                }
                runtime.block_on(run_driver(receiver));
            })
            .map_err(|error| NativePostgresError::RuntimeInitialization(error.to_string()))?;
        let commands = ready_receiver
            .recv()
            .map_err(|_| NativePostgresError::RuntimeUnavailable)?
            .map_err(NativePostgresError::RuntimeInitialization)?;
        Ok(Self { commands })
    }

    /// open one client on the shared runtime
    fn connect(&self, connect: NativePostgresConnect) -> Result<u64, NativePostgresError> {
        let (reply, receiver) = reply_channel();
        self.dispatch(DriverCommand::Connect {
            connect: Box::new(connect),
            reply,
        })?;
        receive_reply(receiver)
    }

    /// observe the local driver state without an extra database round trip
    fn check(&self, connection_id: u64) -> Result<(), NativePostgresError> {
        let (reply, receiver) = reply_channel();
        self.dispatch(DriverCommand::Check {
            connection_id,
            reply,
        })?;
        receive_reply(receiver)
    }

    /// run one typed query concurrently on the shared runtime
    fn query(
        &self,
        connection_id: u64,
        sql: String,
        parameters: Vec<SqlParameter>,
    ) -> Result<Vec<Row>, NativePostgresError> {
        let (reply, receiver) = reply_channel();
        self.dispatch(DriverCommand::Query {
            connection_id,
            parameters,
            reply,
            sql,
        })?;
        receive_reply(receiver)
    }

    /// run one typed command concurrently on the shared runtime
    fn execute(
        &self,
        connection_id: u64,
        sql: String,
        parameters: Vec<SqlParameter>,
    ) -> Result<u64, NativePostgresError> {
        let (reply, receiver) = reply_channel();
        self.dispatch(DriverCommand::Execute {
            connection_id,
            parameters,
            reply,
            sql,
        })?;
        receive_reply(receiver)
    }

    /// run one fixed multi-statement command on the shared runtime
    fn batch_execute(&self, connection_id: u64, sql: String) -> Result<(), NativePostgresError> {
        let (reply, receiver) = reply_channel();
        self.dispatch(DriverCommand::BatchExecute {
            connection_id,
            reply,
            sql,
        })?;
        receive_reply(receiver)
    }

    /// remove one client from the shared runtime
    fn close(&self, connection_id: u64) -> Result<(), NativePostgresError> {
        let (reply, receiver) = reply_channel();
        let mut command = DriverCommand::Close {
            connection_id,
            reply,
        };
        // never orphan an existing driver client solely because a bounded queue is temporarily full
        loop {
            match self.commands.try_send(command) {
                Ok(()) => break,
                Err(mpsc::error::TrySendError::Full(pending)) => {
                    command = pending;
                    thread::sleep(Duration::from_millis(1));
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    return Err(NativePostgresError::RuntimeUnavailable);
                }
            }
        }
        receive_reply(receiver)
    }

    /// enqueue one bounded driver command without nested runtime entry
    fn dispatch(&self, command: DriverCommand) -> Result<(), NativePostgresError> {
        self.commands.try_send(command).map_err(|error| {
            // distinguish bounded backpressure from a stopped driver
            if matches!(error, mpsc::error::TrySendError::Full(_)) {
                NativePostgresError::RuntimeSaturated
            } else {
                NativePostgresError::RuntimeUnavailable
            }
        })
    }
}

enum DriverCommand {
    Connect {
        connect: Box<NativePostgresConnect>,
        reply: Reply<u64>,
    },
    Check {
        connection_id: u64,
        reply: Reply<()>,
    },
    Query {
        connection_id: u64,
        parameters: Vec<SqlParameter>,
        reply: Reply<Vec<Row>>,
        sql: String,
    },
    Execute {
        connection_id: u64,
        parameters: Vec<SqlParameter>,
        reply: Reply<u64>,
        sql: String,
    },
    BatchExecute {
        connection_id: u64,
        reply: Reply<()>,
        sql: String,
    },
    Close {
        connection_id: u64,
        reply: Reply<()>,
    },
}

type Reply<T> = sync_mpsc::SyncSender<Result<T, NativePostgresError>>;

struct SqlParameter {
    parameter_type: Type,
    value: Box<dyn ToSql + Sync + Send>,
}

/// One canonical source civil date retained for PostgreSQL text encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativePostgresDate(String);

impl ToSql for NativePostgresDate {
    /// Encode the already-validated source label as PostgreSQL DATE text.
    fn to_sql(
        &self,
        parameter_type: &Type,
        output: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        // refuse direct misuse even when a caller bypasses `to_sql_checked`
        if *parameter_type != Type::DATE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "native date requires PostgreSQL DATE",
            )
            .into());
        }
        output.extend_from_slice(self.0.as_bytes());
        Ok(IsNull::No)
    }

    /// Accept only PostgreSQL's civil DATE domain.
    fn accepts(parameter_type: &Type) -> bool {
        *parameter_type == Type::DATE
    }

    tokio_postgres::types::to_sql_checked!();

    /// Select the documented UTF-8 text format for a canonical date label.
    fn encode_format(&self, _parameter_type: &Type) -> Format {
        Format::Text
    }
}

/// run every connection future and command on one process runtime
async fn run_driver(mut receiver: mpsc::Receiver<DriverCommand>) {
    let clients = Arc::new(AsyncMutex::new(HashMap::<u64, Arc<Client>>::new()));
    let next_connection_id = Arc::new(AtomicU64::new(1));

    // dispatch independent connection operations without serializing requests
    while let Some(command) = receiver.recv().await {
        // retain each reply owner inside its asynchronous operation
        match command {
            DriverCommand::Connect { connect, reply } => {
                let clients = Arc::clone(&clients);
                let next_connection_id = Arc::clone(&next_connection_id);
                tokio::spawn(async move {
                    let result = (*connect).connect().await.map(|client| {
                        let connection_id = next_connection_id.fetch_add(1, Ordering::Relaxed);
                        (connection_id, client)
                    });
                    let result = match result {
                        Ok((connection_id, client)) => {
                            clients.lock().await.insert(connection_id, Arc::new(client));
                            Ok(connection_id)
                        }
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                });
            }
            DriverCommand::Check {
                connection_id,
                reply,
            } => {
                let clients = Arc::clone(&clients);
                tokio::spawn(async move {
                    let result = match client(&clients, connection_id).await {
                        // protocol failures still discard on the actual statement
                        Ok(client) if !client.is_closed() => Ok(()),
                        Ok(_) => Err(NativePostgresError::SessionReleased),
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                });
            }
            DriverCommand::Query {
                connection_id,
                parameters,
                reply,
                sql,
            } => {
                let clients = Arc::clone(&clients);
                tokio::spawn(async move {
                    let result = match client(&clients, connection_id).await {
                        Ok(client) => query(&client, &sql, &parameters).await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                });
            }
            DriverCommand::Execute {
                connection_id,
                parameters,
                reply,
                sql,
            } => {
                let clients = Arc::clone(&clients);
                tokio::spawn(async move {
                    let result = match client(&clients, connection_id).await {
                        Ok(client) => execute(&client, &sql, &parameters).await,
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                });
            }
            DriverCommand::BatchExecute {
                connection_id,
                reply,
                sql,
            } => {
                let clients = Arc::clone(&clients);
                tokio::spawn(async move {
                    let result = match client(&clients, connection_id).await {
                        Ok(client) => client.batch_execute(&sql).await.map_err(Into::into),
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                });
            }
            DriverCommand::Close {
                connection_id,
                reply,
            } => {
                let clients = Arc::clone(&clients);
                tokio::spawn(async move {
                    clients.lock().await.remove(&connection_id);
                    let _ = reply.send(Ok(()));
                });
            }
        }
    }
}

/// borrow one retained client without holding the client-map lock
async fn client(
    clients: &AsyncMutex<HashMap<u64, Arc<Client>>>,
    connection_id: u64,
) -> Result<Arc<Client>, NativePostgresError> {
    clients
        .lock()
        .await
        .get(&connection_id)
        .cloned()
        .ok_or(NativePostgresError::SessionReleased)
}

/// execute one explicitly typed query
async fn query(
    client: &Client,
    sql: &str,
    parameters: &[SqlParameter],
) -> Result<Vec<Row>, NativePostgresError> {
    let parameters = typed_parameters(parameters);
    client
        .query_typed(sql, &parameters)
        .await
        .map_err(Into::into)
}

/// execute one explicitly typed command
async fn execute(
    client: &Client,
    sql: &str,
    parameters: &[SqlParameter],
) -> Result<u64, NativePostgresError> {
    let parameters = typed_parameters(parameters);
    client
        .execute_typed(sql, &parameters)
        .await
        .map_err(Into::into)
}

/// borrow owned parameters for one asynchronous driver call
fn typed_parameters(parameters: &[SqlParameter]) -> Vec<(&(dyn ToSql + Sync), Type)> {
    parameters
        .iter()
        .map(|parameter| {
            (
                parameter.value.as_ref() as &(dyn ToSql + Sync),
                parameter.parameter_type.clone(),
            )
        })
        .collect()
}

/// convert public owned parameters before crossing the runtime boundary
fn prepare_parameters(
    parameters: &[NativePostgresParameter],
) -> Result<Vec<SqlParameter>, NativePostgresError> {
    parameters.iter().cloned().map(prepare_parameter).collect()
}

/// bind one owned parameter to its exact tokio-postgres type
fn prepare_parameter(
    parameter: NativePostgresParameter,
) -> Result<SqlParameter, NativePostgresError> {
    let parameter = match parameter {
        NativePostgresParameter::Bool(value) => sql_parameter(value, Type::BOOL),
        NativePostgresParameter::I16(value) => sql_parameter(value, Type::INT2),
        NativePostgresParameter::I32(value) => sql_parameter(value, Type::INT4),
        NativePostgresParameter::I64(value) => sql_parameter(value, Type::INT8),
        NativePostgresParameter::U64(value) => sql_parameter(
            i64::try_from(value).map_err(|_| NativePostgresError::ParameterOutOfRange)?,
            Type::INT8,
        ),
        NativePostgresParameter::F64(value) => sql_parameter(value, Type::FLOAT8),
        NativePostgresParameter::Text(value) => sql_parameter(value, Type::TEXT),
        NativePostgresParameter::Uuid(value) => sql_parameter(
            uuid::Uuid::parse_str(&value)
                .map_err(|_| NativePostgresError::InvalidParameter("uuid"))?,
            Type::UUID,
        ),
        NativePostgresParameter::Timestamp(value) => {
            sql_parameter(system_time(&value)?, Type::TIMESTAMPTZ)
        }
        NativePostgresParameter::Date(value) => sql_parameter(postgres_date(&value)?, Type::DATE),
        NativePostgresParameter::Json(value) => {
            let raw = RawValue::from_string(value)
                .map_err(|_| NativePostgresError::InvalidParameter("json"))?;
            sql_parameter(Json(raw), Type::JSONB)
        }
        NativePostgresParameter::Bytes(value) => sql_parameter(value, Type::BYTEA),
        NativePostgresParameter::TextArray(value) => sql_parameter(value, Type::TEXT_ARRAY),
        NativePostgresParameter::I32Array(value) => sql_parameter(value, Type::INT4_ARRAY),
        NativePostgresParameter::I64Array(value) => sql_parameter(value, Type::INT8_ARRAY),
        NativePostgresParameter::Null(parameter_type) => null_parameter(parameter_type),
    };
    Ok(parameter)
}

/// box one non-null SQL parameter
fn sql_parameter(value: impl ToSql + Sync + Send + 'static, parameter_type: Type) -> SqlParameter {
    SqlParameter {
        parameter_type,
        value: Box::new(value),
    }
}

/// box one explicitly typed SQL null
fn null_parameter(parameter_type: NativePostgresType) -> SqlParameter {
    // bind the matching Option type so tokio-postgres emits SQL null
    match parameter_type {
        NativePostgresType::Bool => sql_parameter(Option::<bool>::None, Type::BOOL),
        NativePostgresType::I16 => sql_parameter(Option::<i16>::None, Type::INT2),
        NativePostgresType::I32 => sql_parameter(Option::<i32>::None, Type::INT4),
        NativePostgresType::I64 => sql_parameter(Option::<i64>::None, Type::INT8),
        NativePostgresType::F64 => sql_parameter(Option::<f64>::None, Type::FLOAT8),
        NativePostgresType::Text => sql_parameter(Option::<String>::None, Type::TEXT),
        NativePostgresType::Uuid => sql_parameter(Option::<uuid::Uuid>::None, Type::UUID),
        NativePostgresType::Timestamp => {
            sql_parameter(Option::<SystemTime>::None, Type::TIMESTAMPTZ)
        }
        NativePostgresType::Date => sql_parameter(Option::<NativePostgresDate>::None, Type::DATE),
        NativePostgresType::Json => sql_parameter(Option::<Json<Box<RawValue>>>::None, Type::JSONB),
        NativePostgresType::Bytes => sql_parameter(Option::<Vec<u8>>::None, Type::BYTEA),
        NativePostgresType::TextArray => {
            sql_parameter(Option::<Vec<String>>::None, Type::TEXT_ARRAY)
        }
        NativePostgresType::I32Array => sql_parameter(Option::<Vec<i32>>::None, Type::INT4_ARRAY),
        NativePostgresType::I64Array => sql_parameter(Option::<Vec<i64>>::None, Type::INT8_ARRAY),
    }
}

/// convert one strict UTC timestamp into the standard PostgreSQL time owner
fn system_time(value: &str) -> Result<SystemTime, NativePostgresError> {
    let milliseconds = NativeWirePrimitives
        .parse_explicit_utc_milliseconds(value)
        .ok_or(NativePostgresError::InvalidParameter("timestamp"))?;
    // preserve supported pre-epoch timestamps without signed-duration casts
    if milliseconds >= 0 {
        return UNIX_EPOCH
            .checked_add(Duration::from_millis(milliseconds as u64))
            .ok_or(NativePostgresError::ParameterOutOfRange);
    }
    UNIX_EPOCH
        .checked_sub(Duration::from_millis(milliseconds.unsigned_abs()))
        .ok_or(NativePostgresError::ParameterOutOfRange)
}

/// Validate one exact source calendar label for PostgreSQL DATE text encoding.
fn postgres_date(value: &str) -> Result<NativePostgresDate, NativePostgresError> {
    let bytes = value.as_bytes();
    // preserve the source's exact four-digit ASCII grammar before calendar parsing
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return Err(NativePostgresError::InvalidParameter("date"));
    }
    let date = value
        .parse::<Date>()
        .map_err(|_| NativePostgresError::InvalidParameter("date"))?;
    // preserve the source's canonical YYYY-MM-DD spelling without normalization
    if date.to_string() != value {
        return Err(NativePostgresError::InvalidParameter("date"));
    }
    Ok(NativePostgresDate(value.to_owned()))
}

/// validate direct pool construction before retaining password material
fn validate_pool_configuration(
    configuration: &DatabasePoolConfiguration,
) -> Result<usize, NativePostgresError> {
    // require every exact connection identity field
    if configuration.application_name.trim().is_empty() {
        return Err(NativePostgresError::InvalidConfiguration(
            "application_name",
        ));
    }
    // reject missing database identity
    if configuration.database.trim().is_empty() {
        return Err(NativePostgresError::InvalidConfiguration("database"));
    }
    // reject missing network identity
    if configuration.host.trim().is_empty() {
        return Err(NativePostgresError::InvalidConfiguration("host"));
    }
    // retain the approved maximum without admitting unbounded pools
    if configuration.max == 0 || configuration.max > MAXIMUM_POOL_CONNECTIONS {
        return Err(NativePostgresError::InvalidConfiguration("max"));
    }
    // reject empty password material before driver construction
    if configuration.password.is_empty() {
        return Err(NativePostgresError::InvalidConfiguration("password"));
    }
    // reject the one invalid TCP port
    if configuration.port == 0 {
        return Err(NativePostgresError::InvalidConfiguration("port"));
    }
    // preserve the database module's exact timeout bounds
    if !(MINIMUM_STATEMENT_TIMEOUT_MS..=MAXIMUM_STATEMENT_TIMEOUT_MS)
        .contains(&configuration.statement_timeout)
    {
        return Err(NativePostgresError::InvalidConfiguration(
            "statement_timeout",
        ));
    }
    // reject missing user identity
    if configuration.user.trim().is_empty() {
        return Err(NativePostgresError::InvalidConfiguration("user"));
    }
    usize::try_from(configuration.max).map_err(|_| NativePostgresError::InvalidConfiguration("max"))
}

/// return the one runtime shared by every native pool
fn process_driver() -> Result<Arc<NativePostgresDriver>, NativePostgresError> {
    // reuse the initialized driver without taking the startup lock
    if let Some(driver) = DRIVER.get() {
        return Ok(Arc::clone(driver));
    }
    let _startup = DRIVER_START
        .lock()
        .map_err(|_| NativePostgresError::RuntimeUnavailable)?;
    // another caller may have completed startup while this caller waited
    if let Some(driver) = DRIVER.get() {
        return Ok(Arc::clone(driver));
    }
    let driver = Arc::new(NativePostgresDriver::start()?);
    let _ = DRIVER.set(Arc::clone(&driver));
    Ok(DRIVER.get().map_or(driver, Arc::clone))
}

/// allocate one single-result synchronous reply channel
fn reply_channel<T>() -> (
    Reply<T>,
    sync_mpsc::Receiver<Result<T, NativePostgresError>>,
) {
    sync_mpsc::sync_channel(1)
}

/// receive one driver result without entering a Tokio runtime
fn receive_reply<T>(
    receiver: sync_mpsc::Receiver<Result<T, NativePostgresError>>,
) -> Result<T, NativePostgresError> {
    receiver
        .recv()
        .map_err(|_| NativePostgresError::RuntimeUnavailable)?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// construct one admitted offline pool configuration
    fn configuration() -> DatabasePoolConfiguration {
        DatabasePoolConfiguration {
            application_name: "weather-test".to_owned(),
            database: "weather".to_owned(),
            host: "database.invalid".to_owned(),
            max: 10,
            password: "not-a-live-secret".to_owned(),
            port: 5432,
            ssl: false,
            statement_timeout: 30_000,
            user: "weather".to_owned(),
        }
    }

    /// enforce the original maximum and timeout bounds without connecting
    #[test]
    fn validates_bounded_pool_configuration() {
        assert_eq!(validate_pool_configuration(&configuration()).unwrap(), 10);
        let mut invalid = configuration();
        invalid.max = 11;
        assert!(matches!(
            validate_pool_configuration(&invalid),
            Err(NativePostgresError::InvalidConfiguration("max"))
        ));
        invalid = configuration();
        invalid.statement_timeout = 99;
        assert!(matches!(
            validate_pool_configuration(&invalid),
            Err(NativePostgresError::InvalidConfiguration(
                "statement_timeout"
            ))
        ));
    }

    /// map owned values to exact driver types without network access
    #[test]
    fn prepares_typed_parameters_offline() {
        let parameters = prepare_parameters(&[
            NativePostgresParameter::Timestamp("2026-11-24T00:00:00.000Z".to_owned()),
            NativePostgresParameter::Date("2026-11-24".to_owned()),
            NativePostgresParameter::U64(42),
            NativePostgresParameter::Json("{\"ok\":true}".to_owned()),
            NativePostgresParameter::TextArray(vec!["a".to_owned(), "b".to_owned()]),
            NativePostgresParameter::Null(NativePostgresType::I64),
            NativePostgresParameter::I16(200),
            NativePostgresParameter::Uuid("01234567-89ab-4cde-8123-0123456789ab".to_owned()),
            NativePostgresParameter::Null(NativePostgresType::Uuid),
            NativePostgresParameter::Null(NativePostgresType::Date),
        ])
        .unwrap();
        assert_eq!(
            parameters
                .iter()
                .map(|parameter| parameter.parameter_type.clone())
                .collect::<Vec<_>>(),
            [
                Type::TIMESTAMPTZ,
                Type::DATE,
                Type::INT8,
                Type::JSONB,
                Type::TEXT_ARRAY,
                Type::INT8,
                Type::INT2,
                Type::UUID,
                Type::UUID,
                Type::DATE,
            ]
        );
        assert!(matches!(
            prepare_parameters(&[NativePostgresParameter::U64(u64::MAX)]),
            Err(NativePostgresError::ParameterOutOfRange)
        ));
        assert!(matches!(
            prepare_parameters(&[NativePostgresParameter::Date("2026-02-30".to_owned())]),
            Err(NativePostgresError::InvalidParameter("date"))
        ));
    }

    /// Pin PostgreSQL DATE text format, exact bytes, and strict source admission.
    #[test]
    fn encodes_exact_text_postgres_dates() {
        let date = postgres_date("1970-01-01").expect("admit canonical source date");
        assert_eq!(date, NativePostgresDate("1970-01-01".to_owned()));
        assert!(matches!(date.encode_format(&Type::DATE), Format::Text));

        let mut encoded = BytesMut::new();
        assert!(matches!(
            date.to_sql(&Type::DATE, &mut encoded),
            Ok(IsNull::No)
        ));
        assert_eq!(encoded.as_ref(), b"1970-01-01");
        assert!(matches!(
            postgres_date("2026-2-03"),
            Err(NativePostgresError::InvalidParameter("date"))
        ));
    }

    /// Exercise the real lazy native driver using an ordinary failed loopback connection.
    #[test]
    fn lazy_pool_connects_only_at_checkout_and_releases_failed_reservation() {
        use std::net::TcpListener;
        use std::time::Instant;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut configuration = configuration();
        configuration.host = "127.0.0.1".to_owned();
        configuration.port = listener.local_addr().unwrap().port();
        configuration.max = 1;
        let pool = NativePostgresPool::new(configuration).unwrap();
        assert_eq!(pool.maximum_connections(), 1);
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            // accept and close the connection without emulating the PostgreSQL protocol
            loop {
                match listener.accept() {
                    Ok((socket, _)) => {
                        drop(socket);
                        return;
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("loopback accept failed: {error}"),
                }
            }
        });
        assert!(pool.checkout().is_err());
        server.join().unwrap();
        // the failed reservation is not retained in the pool
        assert_eq!(pool.shared.lock_state().unwrap().total, 0);
        pool.close().unwrap();
        assert!(matches!(
            pool.checkout(),
            Err(NativePostgresError::PoolClosed)
        ));
    }

    /// Keep transaction cleanup ownership after ordinary failed driver commands.
    #[test]
    fn failed_completion_never_recycles_an_unproven_transaction() {
        let pool = NativePostgresPool::new(configuration()).unwrap();
        assert!(matches!(
            pool.shared.admit().unwrap(),
            PoolAdmission::Reserved
        ));
        let mut session = NativePostgresSession::new(Arc::clone(&pool.shared), u64::MAX);
        session.in_transaction = true;
        // an unavailable driver client exercises the real failed command path without a server
        assert!(session.commit().is_err());
        assert!(session.in_transaction);
        assert!(session.rollback().is_err());
        assert!(session.in_transaction && session.broken);
        assert!(session.release().is_err());
        assert_eq!(pool.shared.lock_state().unwrap().total, 0);
        pool.close().unwrap();
    }

    /// Wait for an admitted lease or reservation before reporting the pool closed.
    #[test]
    fn close_waits_until_admitted_work_releases_capacity() {
        use std::time::Instant;
        let pool = NativePostgresPool::new(configuration()).unwrap();
        assert!(matches!(
            pool.shared.admit().unwrap(),
            PoolAdmission::Reserved
        ));
        let closing = pool.clone();
        let (completed, result) = sync_mpsc::sync_channel(1);
        let thread = thread::spawn(move || completed.send(closing.close()).unwrap());
        let deadline = Instant::now() + Duration::from_secs(3);
        // wait until the actual close operation has stopped new admission
        while !pool.shared.lock_state().unwrap().closed {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert!(matches!(
            result.try_recv(),
            Err(sync_mpsc::TryRecvError::Empty)
        ));
        assert!(matches!(
            pool.checkout(),
            Err(NativePostgresError::PoolClosed)
        ));
        pool.shared.cancel_reservation();
        result
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        thread.join().unwrap();
    }

    /// preserve explicit transaction and decode diagnostics
    #[test]
    fn exposes_stable_local_error_categories() {
        let error = NativePostgresError::decode("valid_at", "unexpected type");
        assert!(error.is_post_query_decode());
        assert_eq!(
            error.to_string(),
            "PostgreSQL column valid_at could not be decoded: unexpected type"
        );
        assert_eq!(
            NativePostgresError::TransactionNotActive.to_string(),
            "PostgreSQL transaction is not active"
        );
    }
}
